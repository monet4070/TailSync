use super::*;

const OUTGOING_RECOVERY_PENDING_DELAY: Duration = Duration::from_secs(2);
const OUTGOING_RECOVERY_IDLE_DELAY: Duration = Duration::from_secs(30);
static OUTGOING_RECOVERY_NOTIFY: std::sync::LazyLock<tokio::sync::Notify> =
    std::sync::LazyLock::new(tokio::sync::Notify::new);

pub(super) fn request_outgoing_recovery() {
    OUTGOING_RECOVERY_NOTIFY.notify_one();
}

pub(super) async fn run_outgoing_recovery_loop<F, Fut>(
    mut shutdown: watch::Receiver<bool>,
    pending_delay: Duration,
    idle_delay: Duration,
    mut recover_once: F,
) where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    loop {
        if *shutdown.borrow() {
            return;
        }
        let pending = recover_once().await;
        let delay = if pending { pending_delay } else { idle_delay };
        tokio::select! {
            _ = sleep(delay) => {}
            _ = OUTGOING_RECOVERY_NOTIFY.notified() => {}
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    return;
                }
            }
        }
    }
}

pub(super) async fn send_file_batch_to_peers(
    paths: Vec<PathBuf>,
    generation: u64,
    selection_id: Option<TransferId>,
    runtime: ClipboardRuntime,
    pool: Arc<Mutex<network::ConnectionPool>>,
    database: Arc<Mutex<db::HistoryDB>>,
    settings: Arc<Mutex<crypto::Settings>>,
) {
    let _selection_claim = match selection_id {
        Some(selection_id) => match sync::try_claim_outgoing_selection(selection_id) {
            Some(claim) => Some(claim),
            None => {
                debug!(
                    "Outgoing file selection {} is already being processed",
                    selection_id.as_hex()
                );
                return;
            }
        },
        None => None,
    };
    if !settings.lock().await.sync_enabled {
        if let Some(selection_id) = selection_id {
            let _ = sync::remove_outgoing_selection(selection_id);
        }
        info!("Sync is paused; keeping clipboard files local");
        return;
    }
    let storage_status = db::storage_status_async(&database).await;
    if !storage_status.available {
        let message = storage_status
            .error
            .unwrap_or_else(|| "Configured storage is unavailable; file transfer is paused".into());
        let should_notify = match selection_id {
            Some(selection_id) => {
                match sync::schedule_outgoing_selection_retry(selection_id, &message) {
                    Ok(should_notify) => should_notify,
                    Err(error) => {
                        warn!("Could not persist outgoing selection retry state: {error}");
                        true
                    }
                }
            }
            None => true,
        };
        if should_notify {
            notify_file_batch_error(&runtime, &settings, &message).await;
        }
        return;
    }
    let prepared = match tokio::task::spawn_blocking(move || {
        sync::prepare_file_batch(paths, generation)
    })
    .await
    {
        Ok(Ok(batch)) => Arc::new(batch),
        Ok(Err(error)) => {
            if let Some(selection_id) = selection_id {
                let _ = sync::remove_outgoing_selection(selection_id);
            }
            let message = error.to_string();
            warn!("File batch rejected: {message}");
            notify_file_batch_error(&runtime, &settings, &message).await;
            return;
        }
        Err(error) => {
            error!("File batch preparation task failed: {error}");
            notify_file_batch_error(&runtime, &settings, &error.to_string()).await;
            return;
        }
    };
    let batch_id = prepared.manifest.batch_id;
    let batch_id_hex = batch_id.as_hex();
    let Some(_batch_claim) = sync::try_claim_outgoing_batch(batch_id) else {
        debug!("Outgoing file batch {batch_id_hex} is already being processed");
        return;
    };
    let peers = configured_peers(&settings)
        .await
        .into_iter()
        .filter(peer_is_transfer_eligible)
        .collect::<Vec<_>>();
    let peer_targets = peers
        .iter()
        .map(|peer| (peer.hostname.clone(), peer.fingerprint.clone()))
        .collect::<Vec<_>>();
    let persist_result = selection_id.map_or_else(
        || sync::persist_outgoing_batch_with_identities(&prepared, &peer_targets),
        |selection_id| {
            sync::persist_outgoing_batch_for_selection_with_identities(
                &prepared,
                &peer_targets,
                selection_id,
            )
        },
    );
    if let Err(error) = persist_result {
        warn!("Could not persist outgoing file batch {batch_id_hex}: {error}");
        let message = format!("Could not persist outgoing file batch: {error}");
        let should_notify = match selection_id {
            Some(selection_id) => {
                match sync::schedule_outgoing_selection_retry(selection_id, &message) {
                    Ok(should_notify) => should_notify,
                    Err(error) => {
                        warn!("Could not persist outgoing selection retry state: {error}");
                        true
                    }
                }
            }
            None => true,
        };
        if should_notify {
            notify_file_batch_error(&runtime, &settings, &message).await;
        }
        return;
    }
    if let Some(selection_id) = selection_id {
        if let Err(error) = sync::remove_outgoing_selection(selection_id) {
            warn!("Could not remove prepared outgoing selection: {error}");
        }
    }
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

    let no_eligible_peers = peers.is_empty();
    let (delivered_peers, mut failures) =
        deliver_prepared_batch_to_peers(prepared.clone(), peers, pool.clone()).await;
    if no_eligible_peers {
        failures.push((
            "TailSync".to_string(),
            "No eligible peers are currently available".to_string(),
        ));
    }
    let delivered = delivered_peers.len();
    for (hostname, fingerprint) in &delivered_peers {
        if let Err(error) =
            sync::mark_outgoing_peer_completed_with_identity(batch_id, hostname, fingerprint)
        {
            warn!("Could not update outgoing file batch {batch_id_hex}: {error}");
        }
    }
    let history_error =
        match save_local_file_batch_history(database.clone(), &prepared, &batch_id_hex).await {
            Ok(()) => None,
            Err(error) => {
                error!("Could not save local file batch history: {error}");
                Some(error)
            }
        };
    let history_saved = history_error.is_none();
    let retry_message = match (failures.is_empty(), history_error) {
        (false, Some(history_error)) => Some(format!(
            "{}; local history also failed: {history_error}",
            summarize_file_batch_failures(&failures)
        )),
        (false, None) => Some(summarize_file_batch_failures(&failures)),
        (true, Some(history_error)) => Some(format!(
            "Could not save local file batch history: {history_error}"
        )),
        (true, None) => None,
    };
    if let Some(message) = retry_message {
        let should_notify = match sync::schedule_outgoing_batch_retry(batch_id, &message) {
            Ok(should_notify) => should_notify,
            Err(error) => {
                warn!("Could not persist outgoing file batch retry state: {error}");
                true
            }
        };
        if should_notify {
            notify_file_batch_error(&runtime, &settings, &message).await;
        }
    }
    if history_saved {
        if let Err(error) = sync::mark_outgoing_history_saved(batch_id) {
            warn!("Could not mark outgoing file batch history as saved: {error}");
        }
    }
    crate::api::bump_clipboard_version();
    crate::api::clear_file_progress_scope(Some(&batch_id_hex), None);
    let cancelled = crate::api::is_file_batch_cancelled(&batch_id_hex);
    if history_saved && (failures.is_empty() || cancelled) {
        if let Err(error) = sync::remove_outgoing_batch(batch_id) {
            warn!("Could not remove completed outgoing file batch {batch_id_hex}: {error}");
        }
    }
    crate::api::clear_file_batch_cancel(&batch_id_hex);
    info!("File batch {batch_id_hex} delivered to {delivered} peer(s)");
}

async fn deliver_prepared_batch_to_peers(
    prepared: Arc<sync::PreparedFileBatch>,
    peers: Vec<network::tailscale::PeerInfo>,
    pool: Arc<Mutex<network::ConnectionPool>>,
) -> (Vec<(String, String)>, Vec<(String, String)>) {
    let mut tasks = tokio::task::JoinSet::new();
    for peer in peers {
        let hostname = peer.hostname.clone();
        let fingerprint = peer.fingerprint.clone();
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
            }
            (hostname, fingerprint, result)
        });
    }
    let mut delivered = Vec::new();
    let mut failures = Vec::new();
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok((hostname, fingerprint, Ok(()))) => delivered.push((hostname, fingerprint)),
            Ok((_, _, Err(error))) if error == "cancelled" => {}
            Ok((hostname, _, Err(error))) => {
                warn!("File batch delivery to {hostname} failed: {error}");
                failures.push((hostname, error));
            }
            Err(error) => {
                error!("File batch peer task failed: {error}");
                failures.push(("TailSync".to_string(), error.to_string()));
            }
        }
    }
    (delivered, failures)
}

async fn save_local_file_batch_history(
    database: Arc<Mutex<db::HistoryDB>>,
    prepared: &sync::PreparedFileBatch,
    batch_id: &str,
) -> Result<(), String> {
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
    let history_batch_id = batch_id.to_string();
    tokio::task::spawn_blocking(move || {
        let mut database = database.blocking_lock();
        if database
            .has_complete_file_batch(&history_batch_id)
            .map_err(|error| error.to_string())?
        {
            return Ok(());
        }
        database
            .add_file_batch(&history_batch_id, &history_files, "self", false)
            .map(|_| ())
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

pub(super) async fn resume_outgoing_file_batches(
    runtime: ClipboardRuntime,
    database: Arc<Mutex<db::HistoryDB>>,
    pool: Arc<Mutex<network::ConnectionPool>>,
    settings: Arc<Mutex<crypto::Settings>>,
    shutdown: watch::Receiver<bool>,
) {
    let recovery_runtime = runtime.clone();
    let recovery_database = database.clone();
    let recovery_pool = pool.clone();
    let recovery_settings = settings.clone();
    run_outgoing_recovery_loop(
        shutdown,
        OUTGOING_RECOVERY_PENDING_DELAY,
        OUTGOING_RECOVERY_IDLE_DELAY,
        move || {
            let runtime = recovery_runtime.clone();
            let database = recovery_database.clone();
            let pool = recovery_pool.clone();
            let settings = recovery_settings.clone();
            async move {
                resume_outgoing_file_batches_once(runtime, database, pool, settings).await
            }
        },
    )
    .await;
}

async fn resume_outgoing_file_batches_once(
    runtime: ClipboardRuntime,
    database: Arc<Mutex<db::HistoryDB>>,
    pool: Arc<Mutex<network::ConnectionPool>>,
    settings: Arc<Mutex<crypto::Settings>>,
) -> bool {
    if !settings.lock().await.sync_enabled {
        return outgoing_file_work_is_pending();
    }
    let prepared_batches = sync::load_outgoing_batches();
    let selections = sync::load_outgoing_selections();
    for selection in selections {
        if prepared_batches
            .iter()
            .any(|batch| batch.selection_id == Some(selection.selection_id))
        {
            if let Err(error) = sync::remove_outgoing_selection(selection.selection_id) {
                warn!("Could not remove superseded outgoing selection: {error}");
            }
            continue;
        }
        if !sync::outgoing_retry_due(selection.next_attempt_at) {
            continue;
        }
        send_file_batch_to_peers(
            selection.paths,
            selection.generation,
            Some(selection.selection_id),
            runtime.clone(),
            pool.clone(),
            database.clone(),
            settings.clone(),
        )
        .await;
    }
    let batches = sync::load_outgoing_batches();
    if batches.is_empty() {
        return outgoing_file_work_is_pending();
    }
    info!(
        "Found {} outgoing file batch journal(s) to resume",
        batches.len()
    );
    let peers = configured_peers(&settings)
        .await
        .into_iter()
        .filter(peer_is_transfer_eligible)
        .collect::<Vec<_>>();
    let peer_targets = peers
        .iter()
        .map(|peer| (peer.hostname.clone(), peer.fingerprint.clone()))
        .collect::<Vec<_>>();

    for journal in batches {
        let batch_id = journal.batch_id();
        let batch_id_hex = batch_id.as_hex();
        if !sync::outgoing_retry_due(journal.next_attempt_at) {
            continue;
        }
        let Some(_batch_claim) = sync::try_claim_outgoing_batch(batch_id) else {
            continue;
        };
        let prepared = match journal.prepared_file_batch() {
            Ok(prepared) => Arc::new(prepared),
            Err(error) => {
                warn!("Skipping outgoing file batch {batch_id_hex}: {error}");
                continue;
            }
        };
        if journal.peers.is_empty() && !peer_targets.is_empty() {
            if let Err(error) = sync::enroll_outgoing_batch_peers(batch_id, &peer_targets) {
                warn!("Could not enroll recovered peers for outgoing file batch {batch_id_hex}: {error}");
                continue;
            }
        }
        let journal = sync::load_outgoing_batches()
            .into_iter()
            .find(|saved| saved.batch_id() == batch_id)
            .unwrap_or(journal);
        let pending_names = journal
            .pending_peers(&peer_targets)
            .into_iter()
            .map(|peer| (*peer).clone())
            .collect::<std::collections::HashSet<_>>();
        let pending_peers = peers
            .iter()
            .filter(|peer| pending_names.contains(&peer.hostname))
            .cloned()
            .collect::<Vec<_>>();

        let mut all_peers_completed = journal.all_peers_completed();
        let mut retry_message = None;
        if !pending_peers.is_empty() {
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
                status: "resuming".into(),
                can_stop: true,
            });
            let (delivered_peers, failures) =
                deliver_prepared_batch_to_peers(prepared.clone(), pending_peers, pool.clone())
                    .await;
            for (hostname, fingerprint) in &delivered_peers {
                if let Err(error) = sync::mark_outgoing_peer_completed_with_identity(
                    batch_id,
                    hostname,
                    fingerprint,
                ) {
                    warn!("Could not update resumed file batch {batch_id_hex}: {error}");
                }
            }
            if !failures.is_empty() {
                warn!(
                    "Resumed file batch {batch_id_hex} remains pending on {} peer(s)",
                    failures.len()
                );
                retry_message = Some(summarize_file_batch_failures(&failures));
            }
            crate::api::clear_file_progress_scope(Some(&batch_id_hex), None);
            all_peers_completed = sync::load_outgoing_batches()
                .into_iter()
                .find(|saved| saved.batch_id() == batch_id)
                .is_some_and(|saved| saved.all_peers_completed());
        } else if !all_peers_completed {
            retry_message = Some("No eligible peers are currently available".to_string());
        }

        if all_peers_completed {
            let history_saved = if journal.local_history_saved {
                true
            } else {
                match save_local_file_batch_history(database.clone(), &prepared, &batch_id_hex)
                    .await
                {
                    Ok(()) => {
                        let _ = sync::mark_outgoing_history_saved(batch_id);
                        true
                    }
                    Err(error) => {
                        error!("Could not save resumed file batch history: {error}");
                        retry_message =
                            Some(format!("Could not save local file batch history: {error}"));
                        false
                    }
                }
            };
            if history_saved {
                if let Err(error) = sync::remove_outgoing_batch(batch_id) {
                    warn!("Could not remove resumed file batch {batch_id_hex}: {error}");
                }
            }
        }
        if let Some(message) = retry_message {
            let should_notify = match sync::schedule_outgoing_batch_retry(batch_id, &message) {
                Ok(should_notify) => should_notify,
                Err(error) => {
                    warn!("Could not persist resumed file batch retry state: {error}");
                    true
                }
            };
            if should_notify {
                notify_file_batch_error(&runtime, &settings, &message).await;
            }
        }
    }
    outgoing_file_work_is_pending()
}

fn outgoing_file_work_is_pending() -> bool {
    !sync::load_outgoing_selections().is_empty() || !sync::load_outgoing_batches().is_empty()
}

pub(super) async fn notify_file_batch_error(
    runtime: &ClipboardRuntime,
    settings: &Arc<Mutex<crypto::Settings>>,
    message: &str,
) {
    if !settings.lock().await.notifications_enabled {
        return;
    }
    if let ClipboardRuntime::Tauri(app) = runtime {
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
    } else {
        crate::api::push_runtime_notification("error", message);
        log::warn!("File transfer failed: {message}");
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
    let mut resume_attempts = 0_u8;
    let mut reported_bytes = 0_u64;

    // A FileChunk is only meaningful after the receiver has accepted the
    // batch and its FileMeta. If the receiver process or connection session
    // disappeared, it answers the orphaned chunk with FileResume. Restart the
    // whole batch transaction with the same IDs so the durable `.part` state
    // is reopened before another chunk is sent.
    'restart_batch: loop {
        network::queue_peer_batch_frame(
            &pool,
            &peer,
            Command::FileBatchStart,
            manifest.clone(),
            batch_id,
        )
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
            let progress_bytes = completed_bytes.saturating_add(confirmed);
            reported_bytes = reported_bytes.max(progress_bytes);
            crate::api::set_file_batch_progress(crate::api::FileProgress {
                batch_id: batch_id_hex.clone(),
                name: meta.name.clone(),
                sent: reported_bytes,
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
                let remaining =
                    usize::try_from((meta.size - confirmed).min(FILE_CHUNK_SIZE as u64))
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
                let receipt = network::queue_peer_file_frame(
                    &pool,
                    &peer,
                    Command::FileChunk,
                    payload,
                    transfer_id,
                )
                .await?;
                if receipt.resume_required {
                    resume_attempts = resume_attempts.saturating_add(1);
                    if resume_attempts > 32 {
                        return Err(format!(
                            "{} repeatedly lost file transfer session state",
                            peer.hostname
                        ));
                    }
                    continue 'restart_batch;
                }
                confirmed = receipt.next_offset.unwrap_or(confirmed);
                file.seek(std::io::SeekFrom::Start(confirmed))
                    .await
                    .map_err(|error| error.to_string())?;
                let progress_bytes = completed_bytes.saturating_add(confirmed);
                reported_bytes = reported_bytes.max(progress_bytes);
                crate::api::set_file_batch_progress(crate::api::FileProgress {
                    batch_id: batch_id_hex.clone(),
                    name: meta.name.clone(),
                    sent: reported_bytes,
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
        break;
    }
    Ok(())
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
