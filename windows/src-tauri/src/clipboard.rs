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
const SYSTEM_RESUME_GAP_MS: u64 = 5_000;
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
        let resume_handle = handle.clone();
        tauri::async_runtime::spawn(resume_outgoing_file_batches(
            resume_handle,
            database.clone(),
            pool.clone(),
            settings.clone(),
            shutdown.clone(),
        ));
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
        let sleep_started_ms = crate::protocol::unix_timestamp_ms().max(0) as u64;
        tokio::select! {
            _ = sleep(Duration::from_millis(poll_interval)) => {}
            _ = wait_for_shutdown(&mut shutdown) => return,
        }
        tick += 1;
        let now_ms = crate::protocol::unix_timestamp_ms().max(0) as u64;
        CLIPBOARD_MONITOR_LAST_TICK_MS.store(now_ms, Ordering::Release);

        if now_ms.saturating_sub(sleep_started_ms) > SYSTEM_RESUME_GAP_MS {
            request_wake_recovery();
            pool.lock().await.disconnect_all();
            network::clear_peer_cache().await;
            info!("Detected system resume; resetting clipboard and peer connections");
        }

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
        let clipboard_changed = change_detector.reports_native_change_events();
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
                match sync::clipboard_files_are_readable(&outbound_paths) {
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
                                info!("Ignored {managed_count} managed file(s) in a mixed clipboard event");
                            }
                            info!("Clipboard files: {} file(s)", outbound_paths.len());
                            let generation = sync_engine.lock().await.supersede_file_clipboard();
                            crate::api::bump_clipboard_version();
                            let selection_id = if settings.lock().await.sync_enabled {
                                match sync::persist_outgoing_selection(&outbound_paths, generation)
                                {
                                    Ok(selection_id) => {
                                        request_outgoing_recovery();
                                        Some(selection_id)
                                    }
                                    Err(error) => {
                                        warn!("Could not persist outgoing file selection: {error}");
                                        None
                                    }
                                }
                            } else {
                                None
                            };
                            tokio::spawn(send_file_batch_to_peers(
                                outbound_paths,
                                generation,
                                selection_id,
                                handle.clone(),
                                pool.clone(),
                                database.clone(),
                                settings.clone(),
                            ));
                        }
                        // A file clipboard event is authoritative. Windows
                        // can expose the same file list as text, so do not
                        // fall through and broadcast a second text event.
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

                    let payload: Arc<[u8]> = Arc::from(t.into_bytes().into_boxed_slice());
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
        match clipboard.read_image() {
            Ok(image) => {
                let rgba = image.rgba();
                let w = image.width();
                let h = image.height();
                let packed: Arc<[u8]> = match crate::protocol::pack_rgba_image(w, h, rgba) {
                    Ok(packed) => Arc::from(packed.into_boxed_slice()),
                    Err(error) => {
                        warn!("Ignoring invalid clipboard image {w}×{h}: {error}");
                        continue;
                    }
                };
                // The pixels are copied into `packed`; release the source image
                // now. History and broadcast then share this buffer by Arc,
                // avoiding another full image copy before event encoding.
                drop(image);
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

mod transfer;

use transfer::*;

fn spawn_save_text(database: Arc<Mutex<db::HistoryDB>>, text: Arc<[u8]>) {
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

fn spawn_save_image(database: Arc<Mutex<db::HistoryDB>>, data: Arc<[u8]>) {
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
    payload: Arc<[u8]>,
) {
    if !settings.lock().await.sync_enabled {
        debug!("Sync is paused; skipping clipboard broadcast");
        return;
    }
    let peers = configured_peers(settings).await;

    // Encode the event exactly once and share the reference-counted buffer
    // across every peer, so broadcasting a large image holds a single payload
    // instead of one copy per peer. Every peer acknowledges the same message
    // id; receiver dedup is scoped to (source, message_id) and each peer sees
    // the broadcast at most once, so the shared id is safe.
    let event = match network::SharedEvent::encode_shared(cmd, payload) {
        Ok(event) => event,
        Err(error) => {
            warn!("Skipping clipboard broadcast: {error}");
            return;
        }
    };

    for peer in &peers {
        if !peer_is_transfer_eligible(peer) {
            continue;
        }
        let pool = pool.clone();
        let event = event.clone();
        let peer = peer.clone();
        tokio::spawn(async move {
            if let Err(error) = network::queue_peer_shared_event(&pool, &peer, &event).await {
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
mod tests;
