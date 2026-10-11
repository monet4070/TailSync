#![cfg(feature = "repair-regressions")]

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tailsync_core::{
    composite::{
        origin::{Origin, OriginTracker},
        Completeness, Mime, NativePublish, PortableBundle, Representation,
    },
    db::HistoryDB,
    peer::composite_receiver,
    protocol::{
        native_clipboard::{PublishReceipt, PublishState},
        EventEnvelope, TransferId,
    },
    sync::{FileBatchProgress, FileReceiveCommit, PlatformResultFuture, SyncEngine, SyncPlatform},
};
use tokio::sync::Mutex;

fn bundle(text: &str) -> PortableBundle {
    PortableBundle {
        representations: vec![Representation {
            mime: Mime::Text,
            bytes: text.as_bytes().to_vec(),
        }],
        resources: vec![],
        bindings: vec![],
        completeness: Completeness::Complete,
    }
}

struct Platform {
    db: Arc<Mutex<HistoryDB>>,
    writes: AtomicUsize,
    native_attempts: AtomicUsize,
    history_seen: AtomicUsize,
    hold_for_cancel: std::sync::atomic::AtomicBool,
    entered: tokio::sync::Notify,
}

#[tokio::test]
async fn maximum_legal_rgba_image_reaches_history_and_native_publish() {
    let (root, db, engine, platform, source) = fixture().await;
    let rgba = vec![0x7f; tailsync_core::composite::RGBA_LIMIT];
    let image = PortableBundle {
        representations: vec![Representation {
            mime: Mime::Png,
            bytes: tailsync_core::composite::packed_to_png(2048, 4096, &rgba).unwrap(),
        }],
        resources: vec![],
        bindings: vec![],
        completeness: Completeness::Complete,
    };
    image.validate().unwrap();
    let result = run(
        EventEnvelope::new(image.encode().unwrap()),
        &source,
        db.clone(),
        engine,
    )
    .await;
    assert!(
        result.is_ok(),
        "a legal 32 MiB RGBA image must be admitted: {result:?}"
    );
    assert_eq!(platform.writes.load(Ordering::SeqCst), 1);
    assert_eq!(platform.history_seen.load(Ordering::SeqCst), 1);
    assert_eq!(
        db.lock().await.get_all(None, None, 100, 0).unwrap().len(),
        1
    );
    assert_eq!(
        tailsync_core::composite::budgets::global()
            .snapshot()
            .pipeline_reserved,
        0
    );
    drop(platform);
    drop(db);
    std::fs::remove_dir_all(root).unwrap();
}
impl SyncPlatform for Platform {
    fn write_text(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn write_image(&self, _: u32, _: u32, _: &[u8]) -> Result<(), String> {
        Ok(())
    }
    fn clipboard_revision(&self) -> Option<i64> {
        Some(7)
    }
    fn write_composite(&self, _: &PortableBundle, _: Option<i64>) -> Result<NativePublish, String> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        self.history_seen.store(
            self.db
                .blocking_lock()
                .get_all(None, None, 100, 0)
                .unwrap()
                .len(),
            Ordering::SeqCst,
        );
        Ok(NativePublish::Published(PublishReceipt {
            state: PublishState::Published,
            revision: 1,
            token: [1; 16],
        }))
    }
    fn write_composite_cancellable(
        &self,
        bundle: &PortableBundle,
        expected: Option<i64>,
        cancelled: Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<NativePublish, String> {
        self.native_attempts.fetch_add(1, Ordering::SeqCst);
        if self.hold_for_cancel.load(Ordering::Acquire) {
            self.entered.notify_one();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            while !cancelled.load(Ordering::Acquire) && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            assert!(
                cancelled.load(Ordering::Acquire),
                "revocation must reach the running native adapter"
            );
            return Ok(NativePublish::Unknown {
                detail: "revoked while running".into(),
            });
        }
        self.write_composite(bundle, expected)
    }
    fn set_file_progress(&self, _: &str, _: u64, _: u64) {}
    fn clear_file_progress(&self, _: Option<TransferId>, _: Option<&str>) {}
    fn set_file_batch_progress(&self, _: FileBatchProgress) {}
    fn file_batch_failed(&self, _: Option<TransferId>, _: &str) {}
    fn files_received(&self, _: FileReceiveCommit) -> PlatformResultFuture {
        Box::pin(async { Ok(()) })
    }
}

async fn run(
    e: EventEnvelope,
    source: &str,
    db: Arc<Mutex<HistoryDB>>,
    engine: Arc<Mutex<SyncEngine>>,
) -> Result<tailsync_core::protocol::composite::CompositeAck, String> {
    composite_receiver::receive(
        e,
        source.into(),
        "fixture".into(),
        *blake3::hash(source.as_bytes()).as_bytes(),
        0,
        Some(7),
        0,
        engine,
        db,
        Arc::new(|| true),
    )
    .await
}

async fn fixture() -> (
    std::path::PathBuf,
    Arc<Mutex<HistoryDB>>,
    Arc<Mutex<SyncEngine>>,
    Arc<Platform>,
    String,
) {
    let root =
        std::env::temp_dir().join(format!("tailsync-followup-{:016x}", rand::random::<u64>()));
    let db = Arc::new(Mutex::new(
        HistoryDB::open_isolated_for_test_composite(&root).unwrap(),
    ));
    let platform = Arc::new(Platform {
        db: db.clone(),
        writes: AtomicUsize::new(0),
        native_attempts: AtomicUsize::new(0),
        history_seen: AtomicUsize::new(0),
        hold_for_cancel: false.into(),
        entered: tokio::sync::Notify::new(),
    });
    let mut engine = SyncEngine::new();
    engine.set_platform(platform.clone());
    (
        root,
        db,
        Arc::new(Mutex::new(engine)),
        platform,
        format!("{:064x}", rand::random::<u64>()),
    )
}

// Structured observations for fault-matrix.json. Counts are read from the
// production receiver DB/budget ledger and the native adapter's actual entries.
async fn fault_evidence(
    point: &str,
    terminal: &str,
    db: &Arc<Mutex<HistoryDB>>,
    platform: &Platform,
) {
    let ledger = tailsync_core::composite::budgets::global();
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while db.lock().await.protected_history_count_for_test() != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let snapshot = ledger.snapshot();
    assert_eq!(
        snapshot.pipeline_reserved, 0,
        "receiver must release working memory"
    );
    assert_eq!(snapshot.assembly_total, 0);
    assert_eq!(snapshot.decode_total, 0);
    let attempts = platform.native_attempts.load(Ordering::SeqCst);
    assert!(attempts <= 1);
    let history_count = db.lock().await.get_all(None, None, 100, 0).unwrap().len();
    let pending_history_count = db.lock().await.protected_history_count_for_test();
    println!(
        "R6_FAULT {}",
        serde_json::json!({
            "fault_point": point, "terminal": terminal, "native_attempts": attempts,
            "history_count": history_count,
            "pending_history_count": pending_history_count,
            "budget_released": true, "pipeline_reserved_after": snapshot.pipeline_reserved,
            "assembly_after": snapshot.assembly_total, "decode_after": snapshot.decode_total,
            "child_reaped": serde_json::Value::Null,
            "child_scope": "in-process native adapter; OS child measured in private-board supervisor probe"
        })
    );
}

#[test]
fn successful_receipt_closes_intent_and_preserves_new_user_copies() {
    let mut tracker = OriginTracker::default();
    assert!(tracker.record_intent([1; 16], [2; 32]));
    tracker.record(10, [2; 32], [1; 16]);
    assert_eq!(tracker.unresolved_intent_count(), 0);
    assert_eq!(
        tracker.classify(Some(10), [2; 32], Some([1; 16])),
        Origin::SelfWrite
    );
    assert_eq!(
        tracker.classify(Some(11), [2; 32], Some([1; 16])),
        Origin::External
    );
}

#[tokio::test]
async fn completed_event_rejects_a_different_digest_without_an_ack() {
    let (root, db, engine, platform, source) = fixture().await;
    let mut e = EventEnvelope::new(bundle("A").encode().unwrap());
    run(e.clone(), &source, db.clone(), engine.clone())
        .await
        .unwrap();
    e.content = bundle("B").encode().unwrap();
    assert!(
        run(e, &source, db.clone(), engine).await.is_err(),
        "same identity with changed content must be rejected"
    );
    assert_eq!(platform.writes.load(Ordering::SeqCst), 1);
    assert_eq!(
        db.lock().await.get_all(None, None, 100, 0).unwrap().len(),
        1
    );
    fault_evidence("digest-collision", "RejectedCollision", &db, &platform).await;
    drop(platform);
    drop(db);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn recoverable_history_exists_before_the_native_effect() {
    let (root, db, engine, platform, source) = fixture().await;
    let e = EventEnvelope::new(bundle("durable before native").encode().unwrap());
    let id = e.message_id.as_hex();
    run(e, &source, db.clone(), engine).await.unwrap();
    assert_eq!(
        platform.history_seen.load(Ordering::SeqCst),
        1,
        "native effect must see the prepared history"
    );
    let journal = tailsync_core::peer::effect_journal::EffectJournal::open(
        &tailsync_core::db::get_data_dir(),
    )
    .unwrap();
    let record = journal.lookup(&source, &id).unwrap().unwrap();
    assert!(
        record.history_ref.is_some(),
        "a restarted receiver must recover without sender retry"
    );
    drop(journal);
    fault_evidence("history-before-native", "Published", &db, &platform).await;
    drop(platform);
    drop(db);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn failed_history_preparation_never_starts_native_io() {
    let (root, db, engine, platform, source) = fixture().await;
    db.lock()
        .await
        .reject_composite_inserts_for_test(true)
        .unwrap();
    let e = EventEnvelope::new(bundle("injected disk failure").encode().unwrap());
    let id = e.message_id.as_hex();
    assert!(run(e, &source, db.clone(), engine).await.is_err());
    assert_eq!(platform.writes.load(Ordering::SeqCst), 0);
    let journal = tailsync_core::peer::effect_journal::EffectJournal::open(
        &tailsync_core::db::get_data_dir(),
    )
    .unwrap();
    assert!(journal.lookup(&source, &id).unwrap().is_none());
    drop(journal);
    fault_evidence("history-insert-failure", "StorageRejected", &db, &platform).await;
    drop(platform);
    drop(db);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn startup_recovers_prepared_history_without_sender_or_native_io() {
    let (root, db, engine, platform, source) = fixture().await;
    let e = EventEnvelope::new(bundle("interrupted claim").encode().unwrap());
    let id = db
        .lock()
        .await
        .prepare_composite_event(
            &bundle("interrupted claim"),
            "fixture",
            (&source, e.message_id, e.timestamp_ms),
        )
        .unwrap();
    let journal_root = tailsync_core::db::get_data_dir();
    let journal = tailsync_core::peer::effect_journal::EffectJournal::open(&journal_root).unwrap();
    let mut digest = blake3::Hasher::new();
    digest.update(&e.timestamp_ms.to_be_bytes());
    digest.update(&e.content);
    journal
        .claim_prepared(
            &source,
            &e.message_id.as_hex(),
            &digest.finalize().to_hex(),
            e.timestamp_ms,
            None,
            None,
            Some(id),
        )
        .unwrap();
    drop(journal);
    tailsync_core::peer::effect_journal::recover_prepared_history(
        &journal_root,
        &mut *db.lock().await,
    )
    .unwrap();
    assert_eq!(platform.writes.load(Ordering::SeqCst), 0);
    let ack = run(e, &source, db.clone(), engine).await.unwrap();
    assert_eq!(
        ack.result,
        tailsync_core::protocol::composite::AckResult::ApplyUnknown
    );
    assert_eq!(platform.writes.load(Ordering::SeqCst), 0);
    assert_eq!(
        db.lock().await.get_all(None, None, 100, 0).unwrap().len(),
        1
    );
    fault_evidence(
        "crash-after-claim-before-native",
        "ApplyUnknown",
        &db,
        &platform,
    )
    .await;
    drop(platform);
    drop(db);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn real_handshake_disables_unhealthy_composite_and_retains_capture() {
    use tailsync_core::{
        identity::DeviceIdentity,
        secure::{self, CapabilitySwitches, PeerIdentity},
    };
    for (helper, origin, journal, expected) in [
        (true, true, true, true),
        (false, true, true, false),
        (true, false, true, false),
        (true, true, false, false),
    ] {
        secure::set_helper_health(helper);
        secure::set_origin_health(origin);
        secure::set_journal_health(journal);
        assert_eq!(
            secure::composite_capture_serviceable(),
            helper && cfg!(feature = "protocol-composite-clipboard")
        );
        let server_identity = DeviceIdentity::generate_for_test();
        let client_identity = DeviceIdentity::generate_for_test();
        let server_public = server_identity.public_key().to_vec();
        let (client, server) = tokio::io::duplex(256 * 1024);
        let switches = CapabilitySwitches::runtime_defaults();
        let server = tokio::spawn(async move {
            let (_keep, pairing) = tokio::sync::watch::channel(true);
            let mut accepted = secure::accept_with_pairing_window_and_capabilities(
                server,
                &server_identity,
                PeerIdentity {
                    hostname: "fixture-server".into(),
                    tailscale_ip: String::new(),
                    iroh_endpoint_id: None,
                },
                pairing,
                switches,
            )
            .await
            .unwrap();
            secure::write_ready(&mut accepted.connection).await.unwrap();
            accepted
                .connection
                .negotiated_capabilities()
                .composite_clipboard
        });
        let connection = secure::connect_with_capabilities(
            client,
            &client_identity,
            PeerIdentity {
                hostname: "fixture-client".into(),
                tailscale_ip: String::new(),
                iroh_endpoint_id: None,
            },
            "fixture-server",
            &server_public,
            switches,
        )
        .await
        .unwrap();
        assert_eq!(
            connection.negotiated_capabilities().composite_clipboard,
            expected && cfg!(feature = "protocol-composite-clipboard")
        );
        assert_eq!(
            server.await.unwrap(),
            expected && cfg!(feature = "protocol-composite-clipboard")
        );
    }
    secure::set_helper_health(true);
    secure::set_origin_health(true);
    secure::set_journal_health(true);
}

#[tokio::test(flavor = "current_thread")]
async fn revoke_cancels_running_native_work_and_retry_never_republishes() {
    let (root, db, engine, platform, source) = fixture().await;
    platform.hold_for_cancel.store(true, Ordering::Release);
    let authorized = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let check: composite_receiver::AuthorizationCheck = {
        let authorized = authorized.clone();
        Arc::new(move || authorized.load(Ordering::Acquire))
    };
    let event = EventEnvelope::new(bundle("cancel revoked receive").encode().unwrap());
    let task = tokio::spawn(composite_receiver::receive(
        event.clone(),
        source.clone(),
        "fixture".into(),
        *blake3::hash(source.as_bytes()).as_bytes(),
        0,
        Some(7),
        0,
        engine.clone(),
        db.clone(),
        check,
    ));
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        platform.entered.notified(),
    )
    .await
    .unwrap();
    authorized.store(false, Ordering::Release);
    drop(
        tokio::time::timeout(std::time::Duration::from_millis(100), engine.lock())
            .await
            .expect("native worker cannot hold engine"),
    );
    let ack = task.await.unwrap().unwrap();
    assert_eq!(
        ack.result,
        tailsync_core::protocol::composite::AckResult::ApplyUnknown
    );
    let duplicate = run(event, &source, db.clone(), engine).await.unwrap();
    assert_eq!(duplicate.result, ack.result);
    assert_eq!(platform.writes.load(Ordering::Acquire), 0);
    fault_evidence(
        "revoke-during-native-and-duplicate",
        "ApplyUnknown",
        &db,
        &platform,
    )
    .await;
    drop(platform);
    drop(db);
    std::fs::remove_dir_all(root).unwrap();
}

struct AckConnection {
    pending: Option<tailsync_core::protocol::Frame>,
}
impl tailsync_core::peer::delivery::DeliveryConnection for AckConnection {
    async fn write_frame(&mut self, frame: &tailsync_core::protocol::Frame) -> Result<(), String> {
        let e = EventEnvelope::decode(&frame.payload).map_err(|e| e.to_string())?;
        self.pending = Some(
            tailsync_core::protocol::Frame::try_new(
                tailsync_core::protocol::Command::EventAck,
                0,
                frame.sequence,
                e.message_id.ack_payload(),
            )
            .unwrap(),
        );
        Ok(())
    }
    async fn read_frame(&mut self) -> Result<tailsync_core::protocol::Frame, String> {
        self.pending.take().ok_or("no frame".into())
    }
}
#[tokio::test]
async fn temporary_fallback_pressure_is_retryable_and_does_not_poison_shared_cache() {
    use tailsync_core::{
        composite::budgets,
        peer::delivery::{deliver_pending_frame, DeliveryConfig, PendingFrame, SharedEvent},
        protocol::Command,
    };
    let event = SharedEvent::encode(
        Command::CompositePayload,
        bundle("fallback text").encode().unwrap(),
    )
    .unwrap();
    let queued = event.queued();
    let pending = PendingFrame::new(queued, 1);
    let ledger = budgets::global();
    // Leave room for wire/scratch but exclude the bounded fallback workspace.
    let held = ledger
        .reserve_pipeline(
            budgets::PIPELINE_BUDGET_BYTES - ledger.snapshot().pipeline_reserved - 8 * 1024 * 1024,
        )
        .unwrap();
    let mut connection = AckConnection { pending: None };
    let error = deliver_pending_frame(&mut connection, &pending, &DeliveryConfig::default())
        .await
        .unwrap_err();
    assert!(error.is_retryable());
    drop(held);
    deliver_pending_frame(&mut connection, &pending, &DeliveryConfig::default())
        .await
        .unwrap();
}

#[tokio::test]
async fn cancelled_receiver_releases_pending_history_after_native_worker_exits() {
    let (root, db, engine, platform, source) = fixture().await;
    platform.hold_for_cancel.store(true, Ordering::Release);
    let event = EventEnvelope::new(bundle("cancelled receiver history").encode().unwrap());
    let task = tokio::spawn(composite_receiver::receive(
        event.clone(),
        source.clone(),
        "fixture".into(),
        *blake3::hash(source.as_bytes()).as_bytes(),
        0,
        Some(7),
        0,
        engine.clone(),
        db.clone(),
        Arc::new(|| true),
    ));
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        platform.entered.notified(),
    )
    .await
    .unwrap();
    assert_eq!(db.lock().await.protected_history_count_for_test(), 1);
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while db.lock().await.protected_history_count_for_test() != 0 {
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
    })
    .await
    .expect("cancelled receives cannot permanently block quota or migration");
    assert_eq!(
        run(event, &source, db.clone(), engine)
            .await
            .unwrap()
            .result,
        tailsync_core::protocol::composite::AckResult::ApplyUnknown
    );
    assert_eq!(platform.writes.load(Ordering::Acquire), 0);
    fault_evidence(
        "cancelled-waiter-and-duplicate",
        "ApplyUnknown",
        &db,
        &platform,
    )
    .await;
    drop(platform);
    drop(db);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn startup_recovers_legacy_claim_as_history_received_with_unknown_native_result() {
    let (root, db, engine, platform, source) = fixture().await;
    let id = db
        .lock()
        .await
        .prepare_legacy_text("interrupted legacy receive", "fixture")
        .unwrap();
    let journal_root = root.join("legacy-journal");
    let journal = tailsync_core::peer::effect_journal::EffectJournal::open(&journal_root).unwrap();
    journal
        .claim_prepared(
            &source,
            "legacy-event",
            "digest",
            tailsync_core::protocol::unix_timestamp_ms(),
            None,
            None,
            Some(id),
        )
        .unwrap();
    journal.mark_legacy(&source, "legacy-event", false).unwrap();
    drop(journal);
    assert_eq!(
        tailsync_core::peer::effect_journal::recover_prepared_history(
            &journal_root,
            &mut *db.lock().await
        )
        .unwrap(),
        1
    );
    let journal = tailsync_core::peer::effect_journal::EffectJournal::open(&journal_root).unwrap();
    let record = journal.lookup(&source, "legacy-event").unwrap().unwrap();
    assert!(record.completed);
    assert_eq!(
        record.result,
        Some(tailsync_core::protocol::composite::AckResult::ApplyUnknown as u8)
    );
    assert!(journal.legacy_received(&source, "legacy-event").unwrap());
    assert_eq!(platform.writes.load(Ordering::Acquire), 0);
    assert_eq!(db.lock().await.protected_history_count_for_test(), 0);
    drop(journal);
    drop(engine);
    fault_evidence("legacy-crash-after-claim", "ApplyUnknown", &db, &platform).await;
    drop(platform);
    drop(db);
    std::fs::remove_dir_all(root).unwrap();
}
