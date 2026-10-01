// Local production-worker benchmark. Native clipboard/history durability and
// network/device performance remain separate acceptance gates.
struct BenchmarkPlatform;
impl crate::sync::SyncPlatform for BenchmarkPlatform {
    fn write_text(&self, _text: &str) -> Result<(), String> {
        Ok(())
    }
    fn write_image(&self, _width: u32, _height: u32, _rgba: &[u8]) -> Result<(), String> {
        Ok(())
    }
    fn set_file_progress(&self, _name: &str, _received: u64, _total: u64) {}
    fn clear_file_progress(&self, _batch: Option<TransferId>, _device: Option<&str>) {}
    fn set_file_batch_progress(&self, _progress: crate::sync::FileBatchProgress) {}
    fn files_received(
        &self,
        _commit: crate::sync::FileReceiveCommit,
    ) -> crate::sync::PlatformResultFuture {
        Box::pin(async { Ok(()) })
    }
    fn file_batch_failed(&self, _batch: Option<TransferId>, _message: &str) {}
}
async fn benchmark_file_receiver(
    mut server: SecureConnection,
    incoming: std::path::PathBuf,
    engine: Arc<tokio::sync::Mutex<crate::sync::SyncEngine>>,
    meta_delay: Duration,
) -> u64 {
    use crate::sync::{FileBatchManifest, FileMeta, SyncEngine};
    let mut bytes = 0;
    loop {
        let frame = server.read_frame().await.unwrap();
        let (command, payload) = match frame.command {
            Command::FileBatchStart => {
                let manifest: FileBatchManifest = serde_json::from_slice(&frame.payload).unwrap();
                manifest.validate().unwrap();
                SyncEngine::begin_file_batch_shared(
                    &engine,
                    manifest.clone(),
                    "client".into(),
                    "authenticated-client".into(),
                    incoming.clone(),
                    1,
                )
                .await
                .unwrap();
                (Command::FileBatchAccept, manifest.batch_id.0.to_vec())
            }
            Command::FileMeta => {
                let meta: FileMeta = serde_json::from_slice(&frame.payload).unwrap();
                let id = meta.transfer_id.unwrap();
                SyncEngine::begin_file_receive_shared(
                    &engine,
                    meta.clone(),
                    &incoming.join(&meta.name),
                    "client".into(),
                    1,
                )
                .await
                .unwrap();
                tokio::time::sleep(meta_delay).await;
                (
                    Command::FileAck,
                    FileOffset {
                        transfer_id: id,
                        next_offset: 0,
                    }
                    .encode(),
                )
            }
            Command::FileChunk => {
                let chunk = crate::protocol::FileChunkPayload::decode(&frame.payload).unwrap();
                let progress = SyncEngine::handle_resumable_file_chunk_shared(
                    &engine,
                    &chunk,
                    "client".into(),
                )
                .await
                .unwrap();
                if let Some(completed) = progress.completed {
                    crate::sync::verify_and_commit_received_file(&engine, "client", completed)
                        .await
                        .unwrap();
                }
                bytes += chunk.data.len() as u64;
                // Controlled slow-receiver load keeps file activity throughout
                // the 60-second text interval. This is recorded, not hidden.
                tokio::time::sleep(Duration::from_millis(250)).await;
                (
                    Command::FileAck,
                    FileOffset {
                        transfer_id: chunk.transfer_id,
                        next_offset: chunk.offset + chunk.data.len() as u64,
                    }
                    .encode(),
                )
            }
            Command::FileBatchComplete => {
                let id = TransferId(frame.payload.as_slice().try_into().unwrap());
                SyncEngine::finish_file_batch_shared(&engine, "client", id)
                    .await
                    .unwrap();
                server
                    .write_frame(
                        &Frame::try_new(Command::FileBatchAccept, 0, frame.sequence, id.0.to_vec())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                return bytes;
            }
            Command::Heartbeat => (Command::HeartbeatAck, Vec::new()),
            other => panic!("unexpected file command {other:?}"),
        };
        server
            .write_frame(&Frame::try_new(command, 0, frame.sequence, payload).unwrap())
            .await
            .unwrap();
    }
}
async fn benchmark_confirm(
    sender: &crate::peer::pool::PoolSender,
    queued: QueuedFrame,
    completion: oneshot::Receiver<Result<DeliveryReceipt, DeliveryError>>,
) {
    sender
        .channel_for(queued.command())
        .send(queued)
        .await
        .unwrap();
    timeout(Duration::from_secs(20), completion)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}
fn benchmark_quantile(values: &[u128], percentile: usize) -> Option<u128> {
    (!values.is_empty()).then(|| values[(values.len() - 1) * percentile / 100])
}

#[derive(Default)]
struct BenchmarkTextCounts {
    acked: usize,
    rejected: usize,
    expired: usize,
    failed: usize,
}
impl BenchmarkTextCounts {
    fn record(&mut self, result: &Result<DeliveryReceipt, DeliveryError>) {
        match result {
            Ok(_) => self.acked += 1,
            Err(DeliveryError::Rejected(_)) => self.rejected += 1,
            Err(DeliveryError::Expired(_)) => self.expired += 1,
            Err(_) => self.failed += 1,
        }
    }
}
#[test]
fn worker_benchmark_counts_actual_rejections_and_expirations() {
    let mut counts = BenchmarkTextCounts::default();
    counts.record(&Err(DeliveryError::Rejected("denied".into())));
    counts.record(&Err(DeliveryError::Expired("stale".into())));
    counts.record(&Err(DeliveryError::Timeout("timeout".into())));
    counts.record(&Ok(DeliveryReceipt::default()));
    assert_eq!((counts.acked, counts.rejected, counts.expired, counts.failed), (1, 1, 1, 1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "9 x 60s production worker/Noise/core receive benchmark; retain every observation"]
async fn s2_f5_authenticated_worker_baseline() {
    use crate::sync::{FileBatchEntry, FileBatchManifest, FileBatchRef, FileMeta, SyncEngine};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    let mut observations = Vec::new();
    for round in 1..=3 {
        for meta_ms in [0, 50, 500] {
            let root = std::env::temp_dir()
                .join(format!("tailsync-auth-benchmark-{}", rand::random::<u64>()));
            crate::private_fs::create_private_dir_all(&root).unwrap();
            let mut pool = crate::peer::pool::ConnectionPoolState::new();
            let server_identity = server_identity();
            let client_identity = DeviceIdentity::generate_for_test();
            let (file_sender, file_server, file_worker) = authenticated_pool_sender(
                &mut pool,
                Command::FileMeta,
                &server_identity,
                &client_identity,
            )
            .await;
            let (text_sender, mut text_server, text_worker) = authenticated_pool_sender(
                &mut pool,
                Command::TextPayload,
                &server_identity,
                &client_identity,
            )
            .await;
            assert!(!file_sender.same_channel(&text_sender));
            let engine = Arc::new(tokio::sync::Mutex::new(SyncEngine::new()));
            engine
                .lock()
                .await
                .set_platform(Arc::new(BenchmarkPlatform));
            let received_times = Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
            let text_times = received_times.clone();
            let text_engine = engine.clone();
            let text_receiver = tokio::spawn(async move {
                let mut received = 0;
                while let Ok(frame) = text_server.read_frame().await {
                    if frame.command == Command::Heartbeat {
                        text_server
                            .write_frame(
                                &Frame::try_new(Command::HeartbeatAck, 0, frame.sequence, vec![])
                                    .unwrap(),
                            )
                            .await
                            .unwrap();
                        continue;
                    }
                    assert_eq!(frame.command, Command::TextPayload);
                    let event = EventEnvelope::decode(&frame.payload).unwrap();
                    let id: u64 = std::str::from_utf8(&event.content)
                        .unwrap()
                        .parse()
                        .unwrap();
                    text_times
                        .lock()
                        .unwrap()
                        .insert(id, std::time::Instant::now());
                    text_engine
                        .lock()
                        .await
                        .handle_incoming_text(
                            std::str::from_utf8(&event.content).unwrap(),
                            "client".into(),
                        )
                        .await
                        .unwrap();
                    text_server
                        .write_frame(
                            &Frame::try_new(
                                Command::EventAck,
                                0,
                                frame.sequence,
                                event.message_id.ack_payload(),
                            )
                            .unwrap(),
                        )
                        .await
                        .unwrap();
                    received += 1;
                }
                received
            });
            let file_receiver = tokio::spawn(benchmark_file_receiver(
                file_server,
                root.join("incoming"),
                engine,
                Duration::from_millis(meta_ms),
            ));
            let data = vec![0x5a; crate::protocol::FILE_CHUNK_SIZE];
            let mut hasher = blake3::Hasher::new();
            for _ in 0..16 {
                hasher.update(&data);
            }
            let hash = hasher.finalize().to_hex().to_string();
            let manifest = FileBatchManifest {
                batch_id: TransferId::random(),
                generation: 1,
                total_bytes: 256 * 1024 * 1024,
                files: (0..16)
                    .map(|index| FileBatchEntry {
                        transfer_id: TransferId::random(),
                        index,
                        name: format!("file-{index}.bin"),
                        source_parent: String::new(),
                        size: 16 * 1024 * 1024,
                        hash: hash.clone(),
                        chunk_size: crate::protocol::FILE_CHUNK_SIZE as u32,
                    })
                    .collect(),
            };
            manifest.validate().unwrap();
            let active = Arc::new(AtomicBool::new(true));
            let file_active = active.clone();
            let file_tx = tokio::spawn(async move {
                let (tx, rx) = oneshot::channel();
                benchmark_confirm(
                    &file_sender,
                    QueuedFrame::confirmed_batch(
                        Command::FileBatchStart,
                        serde_json::to_vec(&manifest).unwrap(),
                        manifest.batch_id,
                        tx,
                    )
                    .unwrap(),
                    rx,
                )
                .await;
                for entry in &manifest.files {
                    let meta = FileMeta {
                        transfer_id: Some(entry.transfer_id),
                        name: entry.name.clone(),
                        size: entry.size,
                        hash: entry.hash.clone(),
                        chunk_size: entry.chunk_size,
                        batch: Some(FileBatchRef {
                            batch_id: manifest.batch_id,
                            index: entry.index,
                        }),
                    };
                    let (tx, rx) = oneshot::channel();
                    benchmark_confirm(
                        &file_sender,
                        QueuedFrame::confirmed_file(
                            Command::FileMeta,
                            serde_json::to_vec(&meta).unwrap(),
                            entry.transfer_id,
                            tx,
                        )
                        .unwrap(),
                        rx,
                    )
                    .await;
                    for index in 0..16 {
                        let chunk = crate::protocol::FileChunkPayload {
                            transfer_id: entry.transfer_id,
                            offset: index * data.len() as u64,
                            data: data.clone(),
                        };
                        let (tx, rx) = oneshot::channel();
                        benchmark_confirm(
                            &file_sender,
                            QueuedFrame::confirmed_file(
                                Command::FileChunk,
                                chunk.encode().unwrap(),
                                entry.transfer_id,
                                tx,
                            )
                            .unwrap(),
                            rx,
                        )
                        .await;
                    }
                }
                let (tx, rx) = oneshot::channel();
                benchmark_confirm(
                    &file_sender,
                    QueuedFrame::confirmed_batch(
                        Command::FileBatchComplete,
                        manifest.batch_id.0.to_vec(),
                        manifest.batch_id,
                        tx,
                    )
                    .unwrap(),
                    rx,
                )
                .await;
                file_active.store(false, Ordering::SeqCst);
            });
            let peak = Arc::new(AtomicUsize::new(0));
            let enqueued_count = Arc::new(AtomicUsize::new(0));
            let started = std::time::Instant::now();
            let mut tick = tokio::time::interval(Duration::from_millis(50));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Burst);
            let mut attempted = 0_u64;
            let mut completions = tokio::task::JoinSet::new();
            for _ in 0..1200 {
                let scheduled = tick.tick().await.into_std();
                let id = attempted;
                attempted += 1;
                let sender = text_sender.clone();
                let peak = peak.clone();
                let enqueued_count = enqueued_count.clone();
                let times = received_times.clone();
                let during_file = active.load(Ordering::SeqCst);
                completions.spawn(async move {
                    let enqueued = scheduled;
                    let (tx, rx) = oneshot::channel();
                    let mut queued =
                        QueuedFrame::new(Command::TextPayload, id.to_string().into_bytes())
                            .unwrap();
                    queued.completion = Some(tx);
                    let permit = match sender.channel_for(Command::TextPayload).reserve().await {
                        Ok(permit) => permit,
                        Err(error) => return (during_file, None, None, Err(DeliveryError::Transport(error.to_string()))),
                    };
                    // Includes the reserved slot; only the event lane is sampled.
                    peak.fetch_max(CHANNEL_SIZE - sender.channel_for(Command::TextPayload).capacity(), Ordering::SeqCst);
                    permit.send(queued);
                    enqueued_count.fetch_add(1, Ordering::SeqCst);
                    let result = match timeout(Duration::from_secs(20), rx).await {
                        Ok(Ok(result)) => result,
                        Ok(Err(error)) => Err(DeliveryError::Transport(error.to_string())),
                        Err(error) => Err(DeliveryError::Timeout(error.to_string())),
                    };
                    if result.is_err() { return (during_file, None, None, result); }
                    let acknowledged = enqueued.elapsed().as_micros();
                    let delivered = times
                        .lock()
                        .unwrap()
                        .remove(&id)
                        .unwrap()
                        .duration_since(enqueued)
                        .as_micros();
                    (during_file, Some(delivered), Some(acknowledged), result)
                });
            }
            let mut counts = BenchmarkTextCounts::default();
            let mut acked = Vec::new();
            let mut activity = Vec::new();
            let mut delivered = Vec::new();
            while let Some(sample) = completions.join_next().await {
                let (during_file, wire, ack, result) = sample.unwrap();
                counts.record(&result);
                if let (Some(wire), Some(ack)) = (wire, ack) {
                    acked.push(ack);
                    delivered.push(wire);
                    if during_file { activity.push(ack); }
                }
            }
            file_tx.await.unwrap();
            let file_bytes = file_receiver.await.unwrap();
            pool.disconnect_all();
            file_worker.await.unwrap();
            text_worker.await.unwrap();
            let received = text_receiver.await.unwrap();
            acked.sort_unstable();
            delivered.sort_unstable();
            activity.sort_unstable();
            let observation = serde_json::json!({"benchmark":"authenticated_worker_core_receiver", "round":round, "elapsed_ms":started.elapsed().as_millis(), "meta_ack_delay_ms":meta_ms,
            "chunk_ack_delay_ms":250, "text_interval_ms":50, "text_run_s":60, "file_count":16, "file_bytes":file_bytes,
            "attempted":attempted, "enqueued":enqueued_count.load(Ordering::SeqCst), "received":received, "acked":counts.acked, "rejected":counts.rejected, "expired":counts.expired, "failed":counts.failed,
            "file_active_samples":activity.len(), "peak_event_priority_depth":peak.load(Ordering::SeqCst), "event_priority_capacity":CHANNEL_SIZE, "watermark_scope":"event_lane_reserved_slots_only",
            "wire_us":{"p99":benchmark_quantile(&delivered,99),"max":delivered.last()},
            "ack_us":{"p50":benchmark_quantile(&acked,50),"p99":benchmark_quantile(&acked,99),"max":acked.last()},
            "file_active_ack_us":{"p99":benchmark_quantile(&activity,99),"max":activity.last()}});
            println!("{observation}");
            observations.push((
                counts.rejected + counts.expired + counts.failed,
                enqueued_count.load(Ordering::SeqCst),
                attempted,
                received,
                acked.len(),
                benchmark_quantile(&activity, 99),
                peak.load(Ordering::SeqCst),
            ));
            assert_eq!(file_bytes, 256 * 1024 * 1024);
            std::fs::remove_dir_all(root).unwrap();
        }
    }
    // Preserve all nine observations before judging, including failed budgets.
    for (failed, enqueued, attempted, received, acked, p99, peak) in observations {
        assert_eq!(failed, 0);
        assert_eq!(attempted as usize, enqueued);
        assert_eq!(attempted, received);
        assert_eq!(attempted as usize, acked);
        assert!(
            p99.is_some_and(|value| value <= 100_000),
            "file-active text ACK p99 exceeded 100ms: {p99:?}us"
        );
        // Lane isolation changed the metric's meaning. This checks channel
        // geometry only; it does not certify the signed shared-queue watermark.
        assert!(peak <= CHANNEL_SIZE);
    }
}
