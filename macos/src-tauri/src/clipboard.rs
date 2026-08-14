use log::{debug, error, info, warn};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::sync::{watch, Mutex};
use tokio::time::{sleep, Duration, Instant};

use crate::clipboard_change::ClipboardChangeDetector;
use crate::clipboard_file;
use crate::crypto;
use crate::db;
use crate::network;
use crate::protocol::{Command, FileChunkPayload, TransferId, FILE_CHUNK_SIZE};
use crate::sync;

static CLIPBOARD_RECOVERY_GENERATION: AtomicU64 = AtomicU64::new(0);
static CLIPBOARD_MONITOR_LAST_TICK_MS: AtomicU64 = AtomicU64::new(0);
static CLIPBOARD_MONITOR_FAILURES: AtomicU64 = AtomicU64::new(0);
const CLIPBOARD_MONITOR_STALE_AFTER_MS: u64 = 10_000;
const IDENTICAL_CLIPBOARD_EVENT_DEBOUNCE_MS: u64 = 750;

#[derive(Default)]
struct ClipboardEventGate {
    last_processed_at: Option<Instant>,
}

impl ClipboardEventGate {
    fn should_process(
        &mut self,
        content_changed: bool,
        clipboard_changed: bool,
        now: Instant,
    ) -> bool {
        if !content_changed && !clipboard_changed {
            return false;
        }
        if !content_changed
            && self.last_processed_at.is_some_and(|last| {
                now.duration_since(last)
                    < Duration::from_millis(IDENTICAL_CLIPBOARD_EVENT_DEBOUNCE_MS)
            })
        {
            return false;
        }
        self.last_processed_at = Some(now);
        true
    }
}

pub fn request_wake_recovery() {
    CLIPBOARD_RECOVERY_GENERATION.fetch_add(1, Ordering::AcqRel);
}

pub fn monitor_is_healthy() -> bool {
    let last_tick = CLIPBOARD_MONITOR_LAST_TICK_MS.load(Ordering::Acquire);
    if last_tick == 0 {
        return false;
    }
    let now = crate::protocol::unix_timestamp_ms().max(0) as u64;
    now.saturating_sub(last_tick) <= CLIPBOARD_MONITOR_STALE_AFTER_MS
}

pub fn monitor_failure_count() -> u64 {
    CLIPBOARD_MONITOR_FAILURES.load(Ordering::Acquire)
}

fn is_managed_clipboard_file(path: &Path, managed_directory: &Path) -> bool {
    if path.starts_with(managed_directory) {
        return true;
    }
    match (path.canonicalize(), managed_directory.canonicalize()) {
        (Ok(path), Ok(managed_directory)) => path.starts_with(managed_directory),
        _ => false,
    }
}

fn files_to_broadcast(paths: &[PathBuf], managed_directory: &Path) -> Vec<PathBuf> {
    paths
        .iter()
        .filter(|path| !is_managed_clipboard_file(path, managed_directory))
        .cloned()
        .collect()
}

/// Start clipboard monitor — polls system clipboard for text *and* image
/// changes.  Saves to local history and broadcasts to enabled Tailscale peers
/// via the connection pool.
pub fn start_monitor(
    handle: AppHandle,
    database: Arc<Mutex<db::HistoryDB>>,
    sync_engine: Arc<Mutex<sync::SyncEngine>>,
    pool: Arc<Mutex<network::ConnectionPool>>,
    settings: Arc<Mutex<crypto::Settings>>,
    mut shutdown: watch::Receiver<bool>,
) -> tauri::async_runtime::JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        let warm_pool = pool.clone();
        let warm_settings = settings.clone();
        tauri::async_runtime::spawn(async move {
            let peers = configured_peers(&warm_settings).await;
            network::prewarm_connections(warm_pool, peers).await;
        });
        let mut consecutive_failures = 0_u32;
        loop {
            let worker_started = tokio::time::Instant::now();
            let mut worker = tokio::spawn(clipboard_loop(
                handle.clone(),
                database.clone(),
                sync_engine.clone(),
                pool.clone(),
                settings.clone(),
                shutdown.clone(),
            ));
            let result = tokio::select! {
                result = &mut worker => Some(result),
                _ = wait_for_shutdown(&mut shutdown) => {
                    worker.abort();
                    let _ = worker.await;
                    None
                }
            };
            let Some(result) = result else {
                info!("Clipboard monitor stopped for application shutdown");
                return;
            };
            match result {
                Ok(()) => warn!("Clipboard monitor stopped unexpectedly"),
                Err(error) if error.is_panic() => error!("Clipboard monitor panicked"),
                Err(error) => error!("Clipboard monitor task failed: {error}"),
            }
            CLIPBOARD_MONITOR_LAST_TICK_MS.store(0, Ordering::Release);
            CLIPBOARD_MONITOR_FAILURES.fetch_add(1, Ordering::AcqRel);
            if worker_started.elapsed() >= Duration::from_secs(60) {
                consecutive_failures = 0;
            }
            consecutive_failures = consecutive_failures.saturating_add(1);
            let backoff_seconds = (1_u64 << consecutive_failures.saturating_sub(1).min(5)).min(30);
            warn!("Restarting clipboard monitor in {backoff_seconds} seconds");
            tokio::select! {
                _ = sleep(Duration::from_secs(backoff_seconds)) => {}
                _ = wait_for_shutdown(&mut shutdown) => return,
            }
        }
    })
}

async fn clipboard_loop(
    handle: AppHandle,
    database: Arc<Mutex<db::HistoryDB>>,
    sync_engine: Arc<Mutex<sync::SyncEngine>>,
    pool: Arc<Mutex<network::ConnectionPool>>,
    settings: Arc<Mutex<crypto::Settings>>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut change_detector = ClipboardChangeDetector::new();
    let poll_interval = change_detector.poll_interval_ms();
    info!("Clipboard monitor started ({poll_interval} ms change polling)");

    let mut last_text_hash = String::new();
    let mut last_image_hash = String::new();
    let mut last_file_list: Vec<std::path::PathBuf> = vec![];
    let mut text_event_gate = ClipboardEventGate::default();
    let mut image_event_gate = ClipboardEventGate::default();
    let mut file_event_gate = ClipboardEventGate::default();
    let mut tick: u64 = 0;
    let mut recovery_generation = CLIPBOARD_RECOVERY_GENERATION.load(Ordering::Acquire);

    loop {
        tokio::select! {
            _ = sleep(Duration::from_millis(poll_interval)) => {}
            _ = wait_for_shutdown(&mut shutdown) => return,
        }
        CLIPBOARD_MONITOR_LAST_TICK_MS.store(
            crate::protocol::unix_timestamp_ms().max(0) as u64,
            Ordering::Release,
        );
        let requested_generation = CLIPBOARD_RECOVERY_GENERATION.load(Ordering::Acquire);
        if requested_generation != recovery_generation {
            recovery_generation = requested_generation;
            change_detector = ClipboardChangeDetector::new();
            last_text_hash.clear();
            last_image_hash.clear();
            last_file_list.clear();
            text_event_gate = ClipboardEventGate::default();
            image_event_gate = ClipboardEventGate::default();
            file_event_gate = ClipboardEventGate::default();
            info!("Clipboard monitor reset after system wake");
            continue;
        }
        tick += 1;

        if tick.is_multiple_of(30_000 / poll_interval) {
            info!("Clipboard monitor alive (tick={tick})");
        }

        if !change_detector.changed() {
            continue;
        }
        // NSPasteboard.changeCount changes for every copy operation, including
        // copying a value that is byte-for-byte identical to the last value.
        // Keep this event separate from content hashes so repeated copies are
        // still delivered.
        let clipboard_changed = true;
        let clipboard_event_at = Instant::now();

        let clipboard =
            match handle.try_state::<tauri_plugin_clipboard_manager::Clipboard<tauri::Wry>>() {
                Some(c) => c,
                None => {
                    if tick == 1 {
                        error!("Clipboard plugin state NOT FOUND! Monitor will do nothing.");
                    }
                    continue;
                }
            };
        let clipboard = &*clipboard;

        // ── 1. Try files FIRST (macOS: text check also matches filenames) ──
        #[cfg(target_os = "macos")]
        let file_paths =
            match tokio::task::spawn_blocking(clipboard_file::read_clipboard_files).await {
                Ok(paths) => paths,
                Err(error) => {
                    error!("Clipboard file helper task failed: {error}");
                    None
                }
            };
        #[cfg(not(target_os = "macos"))]
        let file_paths = clipboard_file::read_clipboard_files();

        if tick.is_multiple_of(600) {
            db::cleanup_clipboard_files(
                file_paths.as_deref().unwrap_or_default(),
                Duration::from_secs(10 * 60),
            );
        }

        if let Some(ref paths) = file_paths {
            let outbound_paths = files_to_broadcast(paths, &db::get_clipboard_files_dir());
            let managed_count = paths.len().saturating_sub(outbound_paths.len());
            if !paths.is_empty() && outbound_paths.is_empty() {
                last_file_list.clone_from(paths);
                last_text_hash.clear();
                last_image_hash.clear();
                info!(
                    "Managed clipboard file suppressed: {} file(s)",
                    managed_count
                );
                continue;
            }
            if !outbound_paths.is_empty() {
                #[cfg(target_os = "macos")]
                let files_are_readable =
                    clipboard_file::clipboard_files_are_readable(&outbound_paths);
                #[cfg(not(target_os = "macos"))]
                let files_are_readable: Result<(), String> = Ok(());
                match files_are_readable {
                    Ok(()) => {
                        if file_event_gate.should_process(
                            paths != &last_file_list,
                            clipboard_changed,
                            clipboard_event_at,
                        ) {
                            last_file_list.clone_from(paths);
                            last_text_hash.clear();
                            last_image_hash.clear();
                            if managed_count > 0 {
                                info!(
                                    "Ignored {managed_count} managed file(s) in a mixed clipboard event"
                                );
                            }
                            info!("Clipboard files: {} file(s)", outbound_paths.len());
                            let generation = sync_engine.lock().await.supersede_file_clipboard();
                            crate::api::bump_clipboard_version();
                            tokio::spawn(send_file_batch_to_peers(
                                outbound_paths,
                                generation,
                                handle.clone(),
                                pool.clone(),
                                database.clone(),
                                settings.clone(),
                            ));
                        }
                        continue;
                    }
                    Err(error) => {
                        warn!("{error}; trying other clipboard representations");
                    }
                }
            }
        }

        // ── 2. Try text ────────────────────────────────────────────
        match clipboard.read_text() {
            Ok(t) if !t.is_empty() => {
                let hash = blake3::hash(t.as_bytes()).to_hex().to_string();
                if text_event_gate.should_process(
                    hash != last_text_hash,
                    clipboard_changed,
                    clipboard_event_at,
                ) {
                    last_text_hash = hash.clone();

                    let is_echo = shadow_check(&sync_engine, &hash).await;
                    if is_echo {
                        info!("Text shadow-filtered, skipping: {} chars", t.len());
                        continue;
                    }

                    sync_engine.lock().await.supersede_file_clipboard();
                    crate::api::bump_clipboard_version();

                    let payload = t.into_bytes();
                    let broadcast_pool = pool.clone();
                    let broadcast_settings = settings.clone();
                    let broadcast_payload = payload.clone();
                    tokio::spawn(async move {
                        broadcast_to_peers(
                            &broadcast_pool,
                            &broadcast_settings,
                            Command::TextPayload,
                            broadcast_payload,
                        )
                        .await;
                    });
                    spawn_save_text(database.clone(), payload);
                }
                continue; // text present → skip image check
            }
            Err(ref e) if tick % 150 == 1 => {
                error!("read_text failed: {:?}", e);
            }
            _ => {} // Ok but empty
        }

        // ── 3. Try image ──────────────────────────────────────────
        #[cfg(target_os = "macos")]
        let image = match tokio::task::spawn_blocking(clipboard_file::read_clipboard_image).await {
            Ok(result) => result,
            Err(error) => Err(format!("Clipboard image helper task failed: {error}")),
        };
        #[cfg(not(target_os = "macos"))]
        let image = clipboard
            .read_image()
            .map(|image| clipboard_file::ClipboardImageData {
                width: image.width(),
                height: image.height(),
                rgba: image.rgba().to_vec(),
            })
            .map_err(|error| error.to_string());
        match image {
            Ok(image) => {
                let w = image.width;
                let h = image.height;
                let packed = pack_image_data(w, h, &image.rgba);
                let hash = blake3::hash(&packed).to_hex().to_string();
                if !image_event_gate.should_process(
                    hash != last_image_hash,
                    clipboard_changed,
                    clipboard_event_at,
                ) {
                    continue;
                }
                last_image_hash = hash.clone();

                let is_echo = image_shadow_check(&sync_engine, &hash).await;
                if is_echo {
                    continue;
                }

                sync_engine.lock().await.supersede_file_clipboard();
                crate::api::bump_clipboard_version();

                info!(
                    "Clipboard image changed: {}×{} {} bytes",
                    w,
                    h,
                    packed.len()
                );
                let broadcast_pool = pool.clone();
                let broadcast_settings = settings.clone();
                let broadcast_payload = packed.clone();
                tokio::spawn(async move {
                    broadcast_to_peers(
                        &broadcast_pool,
                        &broadcast_settings,
                        Command::ImagePayload,
                        broadcast_payload,
                    )
                    .await;
                });
                spawn_save_image(database.clone(), packed);
            }
            Err(ref e) => {
                debug!("read_image failed: {:?}", e);
            }
        }
    }
}

async fn wait_for_shutdown(shutdown: &mut watch::Receiver<bool>) {
    if *shutdown.borrow() {
        return;
    }
    while shutdown.changed().await.is_ok() {
        if *shutdown.borrow() {
            return;
        }
    }
}

// ── Pack / unpack helpers ────────────────────────────────────────────

async fn send_file_batch_to_peers(
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
            warn!("File batch rejected: {error}");
            notify_file_batch_error(&app, &settings, &error).await;
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

async fn notify_file_batch_error(
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

fn peer_is_transfer_eligible(peer: &network::tailscale::PeerInfo) -> bool {
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

fn summarize_file_batch_failures(failures: &[(String, String)]) -> String {
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

async fn send_batch_to_peer(
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
            .map_err(|error| error.to_string())??;
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
async fn send_file_to_peers(
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

fn hash_file(path: &std::path::Path) -> std::io::Result<(u64, String)> {
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

/// Pack width (u32 LE) + height (u32 LE) + RGBA bytes into one buffer.
fn pack_image_data(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + rgba.len());
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(rgba);
    out
}

// ── Shadow-filter helpers ────────────────────────────────────────────

async fn shadow_check(sync_engine: &Arc<Mutex<sync::SyncEngine>>, hash: &str) -> bool {
    let mut sync = sync_engine.lock().await;
    if sync.contains_shadow_filter(hash) {
        debug!("Text shadow-filter hit: {}", &hash[..8]);
        true
    } else {
        false
    }
}

async fn image_shadow_check(sync_engine: &Arc<Mutex<sync::SyncEngine>>, hash: &str) -> bool {
    let mut sync = sync_engine.lock().await;
    if sync.contains_image_shadow_filter(hash) {
        debug!("Image shadow-filter hit: {}", &hash[..8]);
        true
    } else {
        false
    }
}

fn spawn_save_text(database: Arc<Mutex<db::HistoryDB>>, text: Vec<u8>) {
    tokio::spawn(async move {
        let length = text.len();
        let result = tokio::task::spawn_blocking(move || {
            database
                .blocking_lock()
                .add_text(&String::from_utf8_lossy(&text), "self")
                .map_err(|error| error.to_string())
        })
        .await;
        match result {
            Ok(Ok(())) => info!("DB: text entry saved ({length} chars)"),
            Ok(Err(error)) => error!("DB save text failed: {error}"),
            Err(error) => error!("DB text task failed: {error}"),
        }
        crate::api::bump_clipboard_version();
    });
}

fn spawn_save_image(database: Arc<Mutex<db::HistoryDB>>, data: Vec<u8>) {
    tokio::spawn(async move {
        let length = data.len();
        let result = tokio::task::spawn_blocking(move || {
            database
                .blocking_lock()
                .add_image(&data, "self")
                .map_err(|error| error.to_string())
        })
        .await;
        match result {
            Ok(Ok(())) => info!("DB: image entry saved ({length} bytes)"),
            Ok(Err(error)) => error!("DB save image failed: {error}"),
            Err(error) => error!("DB image task failed: {error}"),
        }
        crate::api::bump_clipboard_version();
    });
}

async fn broadcast_to_peers(
    pool: &Arc<Mutex<network::ConnectionPool>>,
    settings: &Arc<Mutex<crypto::Settings>>,
    cmd: Command,
    payload: Vec<u8>,
) {
    if !settings.lock().await.sync_enabled {
        debug!("Sync is paused; skipping clipboard broadcast");
        return;
    }
    let peers = configured_peers(settings).await;

    for peer in &peers {
        if !peer_is_transfer_eligible(peer) {
            continue;
        }
        let pool = pool.clone();
        let payload = payload.clone();
        let peer = peer.clone();
        tokio::spawn(async move {
            if let Err(error) = network::queue_peer_frame(&pool, &peer, cmd, payload).await {
                debug!("Broadcast to {} failed: {}", peer.hostname, error);
            }
        });
    }
}

async fn configured_peers(
    settings: &Arc<Mutex<crypto::Settings>>,
) -> Vec<network::tailscale::PeerInfo> {
    let snapshot = settings.lock().await.clone();
    let mode = snapshot.connection_mode.clone();
    let discovered = match network::cached_discover_peers(&mode).await {
        Ok((_, peers)) => peers,
        Err(error) => {
            warn!("Peer discovery failed for {mode} mode: {error}");
            Vec::new()
        }
    };
    network::merge_paired_peers(&snapshot, &mode, discovered)
}

#[cfg(test)]
mod tests {
    use super::{
        files_to_broadcast, peer_is_transfer_eligible, summarize_file_batch_failures,
        ClipboardEventGate, IDENTICAL_CLIPBOARD_EVENT_DEBOUNCE_MS,
    };
    use tokio::time::{Duration, Instant};

    fn transfer_peer(
        enabled: bool,
        trusted: bool,
        online: bool,
    ) -> crate::network::tailscale::PeerInfo {
        crate::network::tailscale::PeerInfo {
            hostname: "peer".to_string(),
            tailscale_ip: "100.64.0.2".to_string(),
            online,
            enabled,
            address: "100.64.0.2:53317".to_string(),
            connection_mode: "auto".to_string(),
            trusted,
            fingerprint: String::new(),
            candidates: Vec::new(),
            current_interface: None,
            current_address: None,
            status: Default::default(),
        }
    }

    #[test]
    fn consecutive_native_events_for_identical_content_are_debounced() {
        let mut gate = ClipboardEventGate::default();
        let first = Instant::now();

        assert!(gate.should_process(true, true, first));
        assert!(!gate.should_process(false, true, first + Duration::from_millis(100)));
        assert!(gate.should_process(
            false,
            true,
            first + Duration::from_millis(IDENTICAL_CLIPBOARD_EVENT_DEBOUNCE_MS)
        ));
        assert!(!gate.should_process(false, false, first + Duration::from_secs(2)));
    }

    #[test]
    fn changed_content_bypasses_the_native_event_debounce() {
        let mut gate = ClipboardEventGate::default();
        let first = Instant::now();

        assert!(gate.should_process(true, true, first));
        assert!(gate.should_process(true, true, first + Duration::from_millis(10)));
    }

    #[test]
    fn immediate_transfers_require_enabled_trusted_peers_with_a_route() {
        assert!(peer_is_transfer_eligible(&transfer_peer(true, true, true)));
        assert!(!peer_is_transfer_eligible(&transfer_peer(
            false, true, true
        )));
        assert!(!peer_is_transfer_eligible(&transfer_peer(
            true, false, true
        )));
        assert!(peer_is_transfer_eligible(&transfer_peer(true, true, false)));
    }

    #[test]
    fn iroh_node_ids_are_valid_broadcast_targets_without_ip_parsing() {
        let mut peer = transfer_peer(true, true, false);
        peer.address = "7f5a1b2c3d4e5f60718293a4b5c6d7e8".into();
        peer.candidates = vec![crate::network::PeerCandidate::new(
            crate::network::ConnectionInterface::Iroh,
            peer.address.clone(),
        )];
        assert!(peer_is_transfer_eligible(&peer));
    }

    #[test]
    fn file_batch_failures_are_summarized_once() {
        assert_eq!(
            summarize_file_batch_failures(&[("Mac".into(), "connection lost".into())]),
            "File transfer to Mac failed: connection lost"
        );
        assert_eq!(
            summarize_file_batch_failures(&[
                ("Mac".into(), "connection lost".into()),
                ("Laptop".into(), "timed out".into()),
            ]),
            "File transfer failed on 2 devices: Mac, Laptop"
        );
    }

    #[test]
    fn repeated_native_events_for_a_managed_file_never_broadcast() {
        let managed_directory = std::path::PathBuf::from("tailsync-data/clipboard-files");
        let path = managed_directory.join("transfer/report.pdf");
        let paths = vec![path.clone()];

        assert!(files_to_broadcast(&paths, &managed_directory).is_empty());
        assert!(files_to_broadcast(&paths, &managed_directory).is_empty());
    }

    #[test]
    fn user_owned_file_is_still_broadcast() {
        let managed_directory = std::path::PathBuf::from("tailsync-data/clipboard-files");
        let path = std::path::PathBuf::from("documents/report.pdf");

        assert_eq!(
            files_to_broadcast(std::slice::from_ref(&path), &managed_directory),
            vec![path]
        );
    }

    #[test]
    fn managed_directory_name_prefix_is_not_treated_as_managed() {
        let managed_directory = std::path::PathBuf::from("tailsync-data/clipboard-files");
        let path = std::path::PathBuf::from("tailsync-data/clipboard-files-export/report.pdf");

        assert_eq!(
            files_to_broadcast(std::slice::from_ref(&path), &managed_directory),
            vec![path]
        );
    }

    #[test]
    fn canonical_alias_of_a_managed_file_is_not_broadcast() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-managed-path-test-{:016x}",
            rand::random::<u64>()
        ));
        let actual_managed_directory = root.join("clipboard-files");
        let managed_directory = root.join("alias/../clipboard-files");
        std::fs::create_dir_all(root.join("alias")).unwrap();
        let transfer_directory = actual_managed_directory.join("transfer");
        std::fs::create_dir_all(&transfer_directory).unwrap();
        let file = transfer_directory.join("report.pdf");
        std::fs::write(&file, b"report").unwrap();

        assert!(files_to_broadcast(&[file], &managed_directory).is_empty());

        std::fs::remove_dir_all(root).unwrap();
    }
}
