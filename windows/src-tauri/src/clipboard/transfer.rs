use super::*;

pub(super) async fn send_file_batch_to_peers(
    paths: Vec<PathBuf>,
    generation: u64,
    app: AppHandle,
    pool: Arc<Mutex<network::ConnectionPool>>,
    database: Arc<Mutex<db::HistoryDB>>,
    settings: Arc<Mutex<crypto::Settings>>,
) {
    if !settings.lock().await.sync_enabled {
        info!("Sync is paused; keeping clipboard files local");
        return;
    }
    let prepared = match tokio::task::spawn_blocking(move || {
        sync::prepare_file_batch(paths, generation)
    })
    .await
    {
        Ok(Ok(batch)) => Arc::new(batch),
        Ok(Err(error)) => {
            let message = error.to_string();
            warn!("File batch rejected: {message}");
            notify_file_batch_error(&app, &settings, &message).await;
            return;
        }
        Err(error) => {
            error!("File batch preparation task failed: {error}");
            notify_file_batch_error(&app, &settings, &error.to_string()).await;
            return;
        }
    };
    let storage_status = database.lock().await.storage_status();
    if !storage_status.available {
        let message = storage_status
            .error
            .unwrap_or_else(|| "Configured storage is unavailable; file transfer is paused".into());
        notify_file_batch_error(&app, &settings, &message).await;
        return;
    }
    let batch_id = prepared.manifest.batch_id;
    let batch_id_hex = batch_id.as_hex();
    let peers = configured_peers(&settings)
        .await
        .into_iter()
        .filter(peer_is_transfer_eligible)
        .collect::<Vec<_>>();
    crate::api::set_file_batch_progress(crate::api::FileProgress {
        batch_id: batch_id_hex.clone(),
        name: prepared.files[0].entry.name.clone(),
        sent: 0,
        total: prepared.manifest.total_bytes,
        active: true,
        direction: "sending".into(),
        device: String::new(),
        completed_files: 0,
        total_files: prepared.files.len(),
        speed_bytes_per_second: 0,
        status: "preparing".into(),
        can_stop: true,
    });

    let mut tasks = tokio::task::JoinSet::new();
    for peer in peers {
        let hostname = peer.hostname.clone();
        let peer_batch = prepared.clone();
        let peer_pool = pool.clone();
        tasks.spawn(async move {
            let recovery_peer = peer.clone();
            let recovery_pool = peer_pool.clone();
            let result = send_batch_to_peer(peer_batch, peer, peer_pool).await;
            if matches!(&result, Err(error) if error != "cancelled") {
                recovery_pool
                    .lock()
                    .await
                    .disconnect_hostname(&recovery_peer.hostname);
                let _ = network::queue_peer_frame(
                    &recovery_pool,
                    &recovery_peer,
                    Command::FileBatchCancel,
                    batch_id.0.to_vec(),
                )
                .await;
            }
            (hostname, result)
        });
    }
    let mut delivered = 0_usize;
    let mut failures = Vec::new();
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok((_, Ok(()))) => delivered += 1,
            Ok((_, Err(error))) if error == "cancelled" => {}
            Ok((hostname, Err(error))) => {
                warn!("File batch delivery to {hostname} failed: {error}");
                failures.push((hostname, error));
            }
            Err(error) => {
                error!("File batch peer task failed: {error}");
                failures.push(("TailSync".to_string(), error.to_string()));
            }
        }
    }
    if !failures.is_empty() {
        let message = summarize_file_batch_failures(&failures);
        notify_file_batch_error(&app, &settings, &message).await;
    }

    let history_files = prepared
        .files
        .iter()
        .map(|file| db::HistoryFileInput {
            name: file.entry.name.clone(),
            path: file.path.clone(),
            data_hash: file.entry.hash.clone(),
            size: file.entry.size,
        })
        .collect::<Vec<_>>();
    let history_batch_id = batch_id_hex.clone();
    let history_result = tokio::task::spawn_blocking(move || {
        database
            .blocking_lock()
            .add_file_batch(&history_batch_id, &history_files, "self", false)
            .map_err(|error| error.to_string())
    })
    .await;
    if let Ok(Err(error)) = history_result {
        error!("Could not save local file batch history: {error}");
    }
    crate::api::bump_clipboard_version();
    crate::api::clear_file_progress_scope(Some(&batch_id_hex), None);
    crate::api::clear_file_batch_cancel(&batch_id_hex);
    info!("File batch {batch_id_hex} delivered to {delivered} peer(s)");
}

pub(super) async fn notify_file_batch_error(
    app: &AppHandle,
    settings: &Arc<Mutex<crypto::Settings>>,
    message: &str,
) {
    if !settings.lock().await.notifications_enabled {
        return;
    }
    use tauri_plugin_notification::NotificationExt;
    if let Err(error) = app
        .notification()
        .builder()
        .title("TailSync")
        .body(message)
        .show()
    {
        log::warn!("Could not show file transfer notification: {error}");
    }
}

pub(super) fn peer_is_transfer_eligible(peer: &network::tailscale::PeerInfo) -> bool {
    let has_iroh_route = peer
        .candidates
        .iter()
        .any(|candidate| candidate.interface == network::ConnectionInterface::Iroh);
    peer.enabled
        && peer.trusted
        && (!peer.candidates.is_empty()
            || !peer.address.is_empty()
            || !peer.tailscale_ip.is_empty())
        && (peer.online
            || has_iroh_route
            || !peer.address.is_empty()
            || !peer.tailscale_ip.is_empty())
}

pub(super) fn summarize_file_batch_failures(failures: &[(String, String)]) -> String {
    if let [(hostname, error)] = failures {
        return format!("File transfer to {hostname} failed: {error}");
    }
    let devices = failures
        .iter()
        .map(|(hostname, _)| hostname.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "File transfer failed on {} devices: {devices}",
        failures.len()
    )
}

pub(super) async fn send_batch_to_peer(
    prepared: Arc<sync::PreparedFileBatch>,
    peer: network::tailscale::PeerInfo,
    pool: Arc<Mutex<network::ConnectionPool>>,
) -> Result<(), String> {
    let _peer_batch_guard = network::acquire_peer_file_batch(&pool, &peer.hostname).await;
    let batch_id = prepared.manifest.batch_id;
    let batch_id_hex = batch_id.as_hex();
    if crate::api::is_file_batch_cancelled(&batch_id_hex) {
        return Err("cancelled".to_string());
    }
    let manifest = serde_json::to_vec(&prepared.manifest).map_err(|error| error.to_string())?;
    network::queue_peer_batch_frame(&pool, &peer, Command::FileBatchStart, manifest, batch_id)
        .await?;

    let mut completed_bytes = 0_u64;
    for (file_index, prepared_file) in prepared.files.iter().enumerate() {
        if crate::api::is_file_batch_cancelled(&batch_id_hex) {
            let _ = network::queue_peer_frame(
                &pool,
                &peer,
                Command::FileBatchCancel,
                batch_id.0.to_vec(),
            )
            .await;
            return Err("cancelled".to_string());
        }
        let validation_file = prepared_file.clone();
        tokio::task::spawn_blocking(move || sync::revalidate_prepared_file(&validation_file))
            .await
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())?;
        let transfer_id = prepared_file.entry.transfer_id;
        let meta = sync::FileMeta {
            transfer_id: Some(transfer_id),
            name: prepared_file.entry.name.clone(),
            size: prepared_file.entry.size,
            hash: prepared_file.entry.hash.clone(),
            chunk_size: prepared_file.entry.chunk_size,
            batch: Some(sync::FileBatchRef {
                batch_id,
                index: prepared_file.entry.index,
            }),
        };
        let mut confirmed = network::queue_peer_file_frame(
            &pool,
            &peer,
            Command::FileMeta,
            serde_json::to_vec(&meta).map_err(|error| error.to_string())?,
            transfer_id,
        )
        .await?
        .next_offset
        .unwrap_or(0);
        if confirmed > meta.size {
            return Err(format!(
                "{} returned an invalid resume offset",
                peer.hostname
            ));
        }
        let mut file = tokio::fs::File::open(&prepared_file.path)
            .await
            .map_err(|error| error.to_string())?;
        file.seek(std::io::SeekFrom::Start(confirmed))
            .await
            .map_err(|error| error.to_string())?;
        let mut buffer = vec![0_u8; FILE_CHUNK_SIZE];
        while confirmed < meta.size {
            if crate::api::is_file_batch_cancelled(&batch_id_hex) {
                let _ = network::queue_peer_frame(
                    &pool,
                    &peer,
                    Command::FileBatchCancel,
                    batch_id.0.to_vec(),
                )
                .await;
                return Err("cancelled".to_string());
            }
            let remaining = usize::try_from((meta.size - confirmed).min(FILE_CHUNK_SIZE as u64))
                .unwrap_or(FILE_CHUNK_SIZE);
            let count = file
                .read(&mut buffer[..remaining])
                .await
                .map_err(|error| error.to_string())?;
            if count == 0 {
                return Err(format!("{} ended before its declared size", meta.name));
            }
            let payload = FileChunkPayload {
                transfer_id,
                offset: confirmed,
                data: buffer[..count].to_vec(),
            }
            .encode()
            .map_err(|error| error.to_string())?;
            confirmed = network::queue_peer_file_frame(
                &pool,
                &peer,
                Command::FileChunk,
                payload,
                transfer_id,
            )
            .await?
            .next_offset
            .unwrap_or(confirmed);
            file.seek(std::io::SeekFrom::Start(confirmed))
                .await
                .map_err(|error| error.to_string())?;
            crate::api::set_file_batch_progress(crate::api::FileProgress {
                batch_id: batch_id_hex.clone(),
                name: meta.name.clone(),
                sent: completed_bytes.saturating_add(confirmed),
                total: prepared.manifest.total_bytes,
                active: true,
                direction: "sending".into(),
                device: peer.hostname.clone(),
                completed_files: file_index,
                total_files: prepared.files.len(),
                speed_bytes_per_second: 0,
                status: "transferring".into(),
                can_stop: true,
            });
        }
        completed_bytes = completed_bytes.saturating_add(meta.size);
    }
    network::queue_peer_batch_frame(
        &pool,
        &peer,
        Command::FileBatchComplete,
        batch_id.0.to_vec(),
        batch_id,
    )
    .await?;
    Ok(())
}

/// Legacy single-file sender retained only for source-level regression tests.
#[allow(dead_code)]
pub(super) async fn send_file_to_peers(
    path: std::path::PathBuf,
    pool: Arc<Mutex<network::ConnectionPool>>,
    database: Arc<Mutex<db::HistoryDB>>,
    _sync: Arc<Mutex<sync::SyncEngine>>,
    settings: Arc<Mutex<crypto::Settings>>,
) {
    let fname = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let hash_path = path.clone();
    let (total, hash) = match tokio::task::spawn_blocking(move || hash_file(&hash_path)).await {
        Ok(Ok(result)) => result,
        Ok(Err(e)) => {
            error!("Read file {}: {}", path.display(), e);
            return;
        }
        Err(e) => {
            error!("File hash task failed for {}: {}", path.display(), e);
            return;
        }
    };
    let fname = sync::normalize_transferred_file_name(&fname, &hash);
    info!("Sending file: {} ({} bytes)", fname, total);

    // Send to peers
    let peers = if total > network::MAX_FILE_SIZE {
        warn!("File {fname} exceeds the 1 GiB transfer limit; keeping only local history");
        Vec::new()
    } else {
        configured_peers(&settings)
            .await
            .into_iter()
            .filter(peer_is_transfer_eligible)
            .collect()
    };
    let transfer_id = TransferId::random();
    let meta = sync::FileMeta {
        transfer_id: Some(transfer_id),
        name: fname.clone(),
        size: total,
        hash: hash.clone(),
        chunk_size: FILE_CHUNK_SIZE as u32,
        batch: None,
    };
    let meta_json = serde_json::to_vec(&meta).unwrap_or_default();
    crate::api::set_file_progress(&fname, 0, total);

    let mut sent_to: usize = 0;
    for peer in &peers {
        let addr: std::net::SocketAddr = match network::peer_socket_addr(peer) {
            Ok(a) => a,
            Err(e) => {
                warn!("Bad address configured for peer {}", peer.hostname);
                debug!("Peer address parse error: {e}");
                continue;
            }
        };
        info!("Sending file to peer {}", peer.hostname);
        debug!("Selected peer route: {addr}");
        let mut confirmed = match network::queue_peer_file_frame(
            &pool,
            peer,
            Command::FileMeta,
            meta_json.clone(),
            transfer_id,
        )
        .await
        {
            Ok(receipt) => receipt.next_offset.unwrap_or(0),
            Err(error) => {
                warn!("File metadata to {} failed: {}", peer.hostname, error);
                continue;
            }
        };
        if confirmed > total {
            warn!("Peer {} returned an invalid resume offset", peer.hostname);
            continue;
        }
        let mut file = match tokio::fs::File::open(&path).await {
            Ok(file) => file,
            Err(error) => {
                warn!(
                    "Could not reopen {} for {}: {error}",
                    path.display(),
                    peer.hostname
                );
                continue;
            }
        };
        if let Err(error) = file.seek(std::io::SeekFrom::Start(confirmed)).await {
            warn!(
                "Could not seek {} for {}: {error}",
                path.display(),
                peer.hostname
            );
            continue;
        }
        let mut buffer = vec![0_u8; FILE_CHUNK_SIZE];
        let mut chunk_ok = true;
        while confirmed < total {
            let remaining = usize::try_from((total - confirmed).min(FILE_CHUNK_SIZE as u64))
                .unwrap_or(FILE_CHUNK_SIZE);
            let count = match file.read(&mut buffer[..remaining]).await {
                Ok(0) => break,
                Ok(count) => count,
                Err(error) => {
                    warn!("File read for {} failed: {error}", peer.hostname);
                    chunk_ok = false;
                    break;
                }
            };
            let payload = match (FileChunkPayload {
                transfer_id,
                offset: confirmed,
                data: buffer[..count].to_vec(),
            })
            .encode()
            {
                Ok(payload) => payload,
                Err(error) => {
                    warn!("Could not encode file chunk for {}: {error}", peer.hostname);
                    chunk_ok = false;
                    break;
                }
            };
            let acknowledged = match network::queue_peer_file_frame(
                &pool,
                peer,
                Command::FileChunk,
                payload,
                transfer_id,
            )
            .await
            {
                Ok(receipt) => receipt.next_offset.unwrap_or(confirmed),
                Err(error) => {
                    warn!(
                        "File chunk at {} to {} failed: {}",
                        confirmed, peer.hostname, error
                    );
                    chunk_ok = false;
                    break;
                }
            };
            if acknowledged > total {
                warn!("Peer {} returned an invalid file ACK offset", peer.hostname);
                chunk_ok = false;
                break;
            }
            confirmed = acknowledged;
            crate::api::set_file_progress(&fname, confirmed, total);
            if let Err(error) = file.seek(std::io::SeekFrom::Start(confirmed)).await {
                warn!(
                    "Could not seek to confirmed offset for {}: {error}",
                    peer.hostname
                );
                chunk_ok = false;
                break;
            }
        }
        if chunk_ok && confirmed == total {
            sent_to += 1;
            info!("File {} sent to {} ({})", fname, peer.hostname, total);
        }
    }
    crate::api::set_file_progress(&fname, total, total);
    crate::api::clear_file_progress();
    info!(
        "Sent file {} ({} bytes) to {}/{} peer(s)",
        fname,
        total,
        sent_to,
        peers.len()
    );

    let database = database.clone();
    tokio::spawn(async move {
        let db_fname = fname.clone();
        let result = tokio::task::spawn_blocking(move || {
            database
                .blocking_lock()
                .add_file_from_path(&db_fname, &path, &hash, total, "self")
                .map_err(|error| error.to_string())
        })
        .await;
        match result {
            Ok(Ok(_)) => info!("DB: file entry saved ({}: {} bytes)", fname, total),
            Ok(Err(error)) => error!("DB save file failed: {error}"),
            Err(error) => error!("DB file task failed: {error}"),
        }
        crate::api::bump_clipboard_version();
    });
}

pub(super) fn hash_file(path: &std::path::Path) -> std::io::Result<(u64, String)> {
    let file = File::open(path)?;
    let mut reader = BufReader::with_capacity(64 * 1024, file);
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
        total += count as u64;
    }
    Ok((total, hasher.finalize().to_hex().to_string()))
}

// ── Shadow-filter helpers ────────────────────────────────────────────

pub(super) async fn shadow_check(sync_engine: &Arc<Mutex<sync::SyncEngine>>, hash: &str) -> bool {
    let mut sync = sync_engine.lock().await;
    if sync.contains_shadow_filter(hash) {
        debug!("Text shadow-filter hit: {}", &hash[..8]);
        true
    } else {
        false
    }
}

pub(super) async fn image_shadow_check(
    sync_engine: &Arc<Mutex<sync::SyncEngine>>,
    hash: &str,
) -> bool {
    let mut sync = sync_engine.lock().await;
    if sync.contains_image_shadow_filter(hash) {
        debug!("Image shadow-filter hit: {}", &hash[..8]);
        true
    } else {
        false
    }
}
