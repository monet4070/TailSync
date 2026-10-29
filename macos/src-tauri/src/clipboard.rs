use log::{debug, error, info, warn};
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

use composite_capture::{
    process_composite_capture, CaptureEnvironment, CompositeCapture, CompositeCaptureContext,
};

#[derive(Clone)]
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub enum ClipboardRuntime {
    Tauri(AppHandle),
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Headless,
}

impl ClipboardRuntime {
    async fn notify_file_batch_error(
        &self,
        settings: &Arc<Mutex<crypto::Settings>>,
        message: &str,
    ) {
        let settings = settings.lock().await;
        if !settings.notifications_enabled {
            return;
        }
        crate::api::push_system_notification(
            "error",
            tailsync_runtime::notification_text::SystemNotification::FileTransferFailed,
            &settings.language,
        );
        log::warn!("File transfer failed: {message}");
    }
}

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
    runtime: ClipboardRuntime,
    database: Arc<Mutex<db::HistoryDB>>,
    sync_engine: Arc<Mutex<sync::SyncEngine>>,
    pool: Arc<Mutex<network::ConnectionPool>>,
    settings: Arc<Mutex<crypto::Settings>>,
    image_archive: Arc<crate::automatic_images::AutomaticImageArchive>,
    mut shutdown: watch::Receiver<bool>,
) -> tauri::async_runtime::JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        let warm_pool = pool.clone();
        let warm_settings = settings.clone();
        tauri::async_runtime::spawn(async move {
            let peers = configured_peers(&warm_settings).await;
            network::prewarm_connections(warm_pool, peers).await;
        });
        let resume_runtime = runtime.clone();
        tauri::async_runtime::spawn(resume_outgoing_file_batches(
            resume_runtime,
            database.clone(),
            pool.clone(),
            settings.clone(),
            shutdown.clone(),
        ));
        tauri::async_runtime::spawn(run_expired_transfer_maintenance(
            shutdown.clone(),
            database.clone(),
        ));
        let mut consecutive_failures = 0_u32;
        loop {
            let worker_started = tokio::time::Instant::now();
            let mut worker = tokio::spawn(clipboard_loop(
                runtime.clone(),
                database.clone(),
                sync_engine.clone(),
                pool.clone(),
                settings.clone(),
                image_archive.clone(),
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
    runtime: ClipboardRuntime,
    database: Arc<Mutex<db::HistoryDB>>,
    sync_engine: Arc<Mutex<sync::SyncEngine>>,
    pool: Arc<Mutex<network::ConnectionPool>>,
    settings: Arc<Mutex<crypto::Settings>>,
    image_archive: Arc<crate::automatic_images::AutomaticImageArchive>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut snapshot_retry = tailsync_core::composite::capture_retry::CaptureRetry::default();
    let mut archive_observation =
        tailsync_runtime::automatic_image_archive::CaptureTicketTracker::default();
    let mut change_detector = ClipboardChangeDetector::new();
    let active_poll_interval = change_detector.poll_interval_ms();
    let mut poll_interval = active_poll_interval;
    info!("Clipboard monitor started ({poll_interval} ms change polling)");

    let mut last_text_hash = String::new();
    let mut last_image_hash = String::new();
    let mut last_file_list: Vec<std::path::PathBuf> = vec![];
    let mut text_event_gate = ClipboardEventGate::default();
    let mut image_event_gate = ClipboardEventGate::default();
    let mut file_event_gate = ClipboardEventGate::default();
    let mut tick: u64 = 0;
    let mut recovery_generation = CLIPBOARD_RECOVERY_GENERATION.load(Ordering::Acquire);
    let capture_env = CaptureEnvironment::platform();

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
            // A wake/reset must return to the responsive cadence: the board may
            // have changed while the process was suspended, and the detector was
            // rebuilt without acknowledging anything.
            poll_interval = active_poll_interval;
            archive_observation.reset();
            info!("Clipboard monitor reset after system wake");
            continue;
        }
        tick += 1;

        if tick.is_multiple_of(30_000 / poll_interval) {
            info!("Clipboard monitor alive (tick={tick})");
        }

        if cfg!(feature = "protocol-composite-clipboard")
            && std::env::var_os("TAILSYNC_DISABLE_COMPOSITE_CLIPBOARD").is_none()
            && !tailsync_core::secure::composite_capture_serviceable()
        {
            if tick == 1 {
                crate::clipboard_composite::capture_fault(-1, "clipboard_unavailable").await;
            }
            // The helper is unavailable: keep the slow cadence instead of busy
            // polling. A later tick re-checks serviceability before capturing.
            poll_interval = change_detector.idle_poll_interval_ms();
            continue;
        }
        if crate::clipboard_composite::capture_enabled() {
            if let Some(observed) = change_detector.observed_revision() {
                // A revision is visible: stay responsive while the capture (or its
                // bounded retry) is in progress. `observed_revision()` is a pure
                // read, so nothing is acknowledged here.
                poll_interval = active_poll_interval;
                let outcome = process_composite_capture(
                    observed,
                    CompositeCaptureContext {
                        database: &database,
                        sync_engine: &sync_engine,
                        pool: &pool,
                        settings: &settings,
                        image_archive: &image_archive,
                        retry: &mut snapshot_retry,
                        archive_observation: &mut archive_observation,
                        ack: &mut change_detector,
                        recovery_generation,
                        env: &capture_env,
                    },
                )
                .await;
                if matches!(outcome, CompositeCapture::Handled) {
                    continue;
                }
            } else {
                // The board is stable and no revision is pending: fall back to the
                // idle cadence. Nothing is acknowledged here, so a later copy is
                // still observed and captured.
                poll_interval = change_detector.idle_poll_interval_ms();
                continue;
            }
        }
        let clipboard_changed = change_detector.changed();
        poll_interval = if clipboard_changed {
            active_poll_interval
        } else {
            change_detector.idle_poll_interval_ms()
        };
        if !clipboard_changed {
            continue;
        }
        // NSPasteboard.changeCount changes for every copy operation, including
        // copying a value that is byte-for-byte identical to the last value.
        // Keep this event separate from content hashes so repeated copies are
        // still delivered.
        let clipboard_event_at = Instant::now();

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
                let files_are_readable = sync::clipboard_files_are_readable(&outbound_paths);
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
                                runtime.clone(),
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
        match read_clipboard_text(&runtime) {
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
                            None,
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
        let image = read_clipboard_image(&runtime);
        match image {
            Ok(image) => {
                let w = image.width;
                let h = image.height;
                let packed: Arc<[u8]> = match crate::protocol::pack_rgba_image(w, h, &image.rgba) {
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
                        None,
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

fn read_clipboard_text(runtime: &ClipboardRuntime) -> Result<String, String> {
    match runtime {
        ClipboardRuntime::Tauri(handle) => handle
            .try_state::<tauri_plugin_clipboard_manager::Clipboard<tauri::Wry>>()
            .ok_or_else(|| "Clipboard plugin state is unavailable".to_string())?
            .read_text()
            .map_err(|error| error.to_string()),
        ClipboardRuntime::Headless => {
            #[cfg(target_os = "macos")]
            {
                clipboard_file::read_clipboard_text()
            }
            #[cfg(not(target_os = "macos"))]
            {
                Err("Headless clipboard access is only available on macOS".to_string())
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn read_clipboard_image(
    runtime: &ClipboardRuntime,
) -> Result<clipboard_file::ClipboardImageData, String> {
    match runtime {
        ClipboardRuntime::Tauri(handle) => handle
            .try_state::<tauri_plugin_clipboard_manager::Clipboard<tauri::Wry>>()
            .ok_or_else(|| "Clipboard plugin state is unavailable".to_string())?
            .read_image()
            .map(|image| clipboard_file::ClipboardImageData {
                width: image.width(),
                height: image.height(),
                rgba: image.rgba().to_vec(),
            })
            .map_err(|error| error.to_string()),
        ClipboardRuntime::Headless => {
            Err("Headless clipboard access is only available on macOS".to_string())
        }
    }
}

pub fn write_clipboard_text(runtime: &ClipboardRuntime, text: &str) -> Result<(), String> {
    match runtime {
        ClipboardRuntime::Tauri(handle) => handle
            .try_state::<tauri_plugin_clipboard_manager::Clipboard<tauri::Wry>>()
            .ok_or_else(|| "Clipboard plugin state is unavailable".to_string())?
            .write_text(text.to_string())
            .map_err(|error| error.to_string()),
        ClipboardRuntime::Headless => {
            #[cfg(target_os = "macos")]
            {
                clipboard_file::write_clipboard_text(text)
            }
            #[cfg(not(target_os = "macos"))]
            {
                Err("Headless clipboard access is only available on macOS".to_string())
            }
        }
    }
}

pub fn write_clipboard_image(
    runtime: &ClipboardRuntime,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Result<(), String> {
    match runtime {
        ClipboardRuntime::Tauri(handle) => {
            let image = tauri::image::Image::new(rgba, width, height);
            handle
                .try_state::<tauri_plugin_clipboard_manager::Clipboard<tauri::Wry>>()
                .ok_or_else(|| "Clipboard plugin state is unavailable".to_string())?
                .write_image(&image)
                .map_err(|error| error.to_string())
        }
        ClipboardRuntime::Headless => {
            #[cfg(target_os = "macos")]
            {
                clipboard_file::write_clipboard_image(width, height, rgba)
            }
            #[cfg(not(target_os = "macos"))]
            {
                Err("Headless clipboard access is only available on macOS".to_string())
            }
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

#[derive(Clone)]
struct CaptureBroadcastGate {
    engine: Arc<Mutex<sync::SyncEngine>>,
    generation: u64,
    revision: i64,
    recovery_generation: u64,
}

async fn broadcast_to_peers(
    capture: Option<CaptureBroadcastGate>,
    pool: &Arc<Mutex<network::ConnectionPool>>,
    settings: &Arc<Mutex<crypto::Settings>>,
    cmd: Command,
    payload: Arc<[u8]>,
) {
    if !settings.lock().await.sync_enabled {
        debug!("Sync is paused; skipping clipboard broadcast");
        return;
    }
    // Encode the event exactly once and share the reference-counted buffer
    // across every peer, so broadcasting a large image holds a single payload
    // instead of one copy per peer. Every peer acknowledges the same message
    // id; receiver dedup is scoped to (source, message_id) and each peer sees
    // the broadcast at most once, so the shared id is safe.
    let event = match network::SharedEvent::encode_shared(cmd, payload) {
        Ok(event) => event,
        Err(error) => {
            warn!("Skipping clipboard broadcast: {error}");
            if cmd == Command::CompositePayload {
                tailsync_core::sync_warning::record_capture_not_shared();
            }
            return;
        }
    };
    broadcast_shared_to_peers(capture, pool, settings, event, false).await;
}

include!("../../../shared/platform-clipboard-archive-sync.rs");

async fn broadcast_shared_to_peers(
    capture: Option<CaptureBroadcastGate>,
    pool: &Arc<Mutex<network::ConnectionPool>>,
    settings: &Arc<Mutex<crypto::Settings>>,
    event: network::SharedEvent,
    ordered: bool,
) {
    if !settings.lock().await.sync_enabled {
        return;
    }
    let peers = configured_peers(settings).await;
    for peer in &peers {
        if !peer_is_transfer_eligible(peer) {
            continue;
        }
        let pool = pool.clone();
        let event = event.clone();
        let peer = peer.clone();
        let capture = capture.clone();
        let send = async move {
            let current = async move {
                let Some(capture) = capture else {
                    return Some(None);
                };
                let engine = capture.engine.lock_owned().await;
                (engine.clipboard_generation() == capture.generation
                    && engine.clipboard_revision() == Some(capture.revision)
                    && CLIPBOARD_RECOVERY_GENERATION.load(Ordering::Acquire)
                        == capture.recovery_generation)
                    .then_some(Some(engine))
            };
            if let Err(error) =
                network::queue_peer_shared_event(&pool, &peer, &event, current).await
            {
                warn!("Broadcast to {} failed: {}", peer.hostname, error);
            }
        };
        if ordered {
            send.await;
        } else {
            tokio::spawn(send);
        }
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

mod composite_capture {
    //! The macOS single-observed composite-capture transaction.
    //!
    //! `clipboard_loop` used to inline this transaction: read the native archive,
    //! reserve the CNA1 pipeline budget, convert it, classify its origin, save it,
    //! retry bounded failures, then re-check the clipboard generation before the
    //! final broadcast gate. It is now one private function the monitor really
    //! calls, so the production ordering can be exercised by behaviour tests.
    //!
    //! Only the OS/provider edge and the clock are replaceable (see
    //! [`CaptureEnvironment`]); the memory reserve, `from_archive` conversion,
    //! storage write, retry schedule, archive binding and the broadcast gate all
    //! run in the real transaction below.
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::atomic::Ordering;
    use std::sync::Arc;

    use log::warn;
    use tokio::sync::Mutex;

    use tailsync_core::composite::origin::Origin;
    use tailsync_core::composite::PortableBundle;
    use tailsync_core::protocol::native_archive::NativeArchive;
    use tailsync_core::protocol::native_clipboard::NativeSnapshot;

    use crate::clipboard_change::ClipboardChangeDetector;
    use crate::{crypto, db, network, sync};

    use super::{
        broadcast_composite_to_peers, enqueue_automatic_archive, CaptureBroadcastGate,
        CLIPBOARD_RECOVERY_GENERATION,
    };

    include!("../../../shared/platform-clipboard-capture-terminal.rs");
    include!("../../../shared/platform-clipboard-capture-publish.rs");

    /// The outcome of one composite-capture tick. The monitor keeps the legacy
    /// files/text/image flow for [`CompositeCapture::FilesFallback`] and treats
    /// every other outcome as fully owned by the composite path.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) enum CompositeCapture {
        Handled,
        FilesFallback,
    }

    /// Acknowledgment boundary for the capture seam. Production advances the
    /// platform change detector; tests record the acknowledged revision so an
    /// acknowledgment of a superseded copy is observable.
    pub(super) trait CaptureAcknowledger: Send {
        fn acknowledge(&mut self, revision: i64);
    }

    impl CaptureAcknowledger for ClipboardChangeDetector {
        fn acknowledge(&mut self, revision: i64) {
            ClipboardChangeDetector::acknowledge(self, revision);
        }
    }

    type CaptureFaultFuture = Pin<Box<dyn Future<Output = bool> + Send>>;

    /// Injectable side-effect boundary for the composite capture seam.
    ///
    /// The production adapter calls the real platform helpers; tests supply a
    /// validated fixture archive, a controllable revision, a real `OriginTracker`
    /// fingerprint projection and a controllable clock/fault sink.
    #[derive(Clone)]
    #[allow(clippy::type_complexity)]
    pub(super) struct CaptureEnvironment {
        revision: Arc<dyn Fn() -> Option<i64> + Send + Sync>,
        read_archive: Arc<dyn Fn() -> Result<NativeArchive, String> + Send + Sync>,
        classify: Arc<dyn Fn(&NativeSnapshot, &PortableBundle) -> Origin + Send + Sync>,
        classify_native: Arc<dyn Fn(&NativeSnapshot) -> Origin + Send + Sync>,
        clock: Arc<dyn Fn() -> std::time::Instant + Send + Sync>,
        capture_fault: Arc<dyn Fn(i64, &'static str) -> CaptureFaultFuture + Send + Sync>,
        #[cfg(test)]
        pipeline_lease: Arc<
            std::sync::Mutex<
                Option<std::sync::Weak<tailsync_core::composite::budgets::PipelineLease>>,
            >,
        >,
    }

    impl CaptureEnvironment {
        /// The production adapter: every seam calls the real platform helper.
        pub(super) fn platform() -> Self {
            Self {
                revision: Arc::new(crate::clipboard_composite::revision),
                read_archive: Arc::new(crate::clipboard_composite::read_archive),
                classify: Arc::new(crate::clipboard_composite::classify),
                classify_native: Arc::new(crate::clipboard_composite::classify_native),
                clock: Arc::new(std::time::Instant::now),
                capture_fault: Arc::new(|revision, kind| {
                    Box::pin(crate::clipboard_composite::capture_fault(revision, kind))
                }),
                #[cfg(test)]
                pipeline_lease: Arc::default(),
            }
        }

        fn revision(&self) -> Option<i64> {
            (self.revision)()
        }

        fn read_archive(&self) -> Result<NativeArchive, String> {
            (self.read_archive)()
        }

        fn classify(&self, snapshot: &NativeSnapshot, bundle: &PortableBundle) -> Origin {
            (self.classify)(snapshot, bundle)
        }

        fn classify_native(&self, snapshot: &NativeSnapshot) -> Origin {
            (self.classify_native)(snapshot)
        }

        fn now(&self) -> std::time::Instant {
            (self.clock)()
        }

        async fn capture_fault(&self, revision: i64, kind: &'static str) -> bool {
            (self.capture_fault)(revision, kind).await
        }
    }

    /// Everything one composite-capture tick reads or mutates. Borrowing the
    /// per-loop state mutably keeps a single transaction implementation in the
    /// source instead of a second copy inside the tests.
    pub(super) struct CompositeCaptureContext<'a> {
        pub database: &'a Arc<Mutex<db::HistoryDB>>,
        pub sync_engine: &'a Arc<Mutex<sync::SyncEngine>>,
        pub pool: &'a Arc<Mutex<network::ConnectionPool>>,
        pub settings: &'a Arc<Mutex<crypto::Settings>>,
        pub image_archive: &'a Arc<crate::automatic_images::AutomaticImageArchive>,
        pub retry: &'a mut tailsync_core::composite::capture_retry::CaptureRetry,
        pub archive_observation:
            &'a mut tailsync_runtime::automatic_image_archive::CaptureTicketTracker,
        pub ack: &'a mut dyn CaptureAcknowledger,
        pub recovery_generation: u64,
        pub env: &'a CaptureEnvironment,
    }

    /// One observed composite capture. The monitor calls this from the real
    /// clipboard loop; tests call the same function so the transaction below is the
    /// one under test.
    pub(super) async fn process_composite_capture(
        observed: i64,
        context: CompositeCaptureContext<'_>,
    ) -> CompositeCapture {
        let CompositeCaptureContext {
            database,
            sync_engine,
            pool,
            settings,
            image_archive,
            retry: snapshot_retry,
            archive_observation,
            ack,
            recovery_generation,
            env,
        } = context;

        if !snapshot_retry.ready(observed, env.now()) {
            return CompositeCapture::Handled;
        }
        let archive_ticket = archive_observation.observe(image_archive, observed);
        let provider = env.clone();
        let captured = tokio::task::spawn_blocking(move || {
            // Covers helper output/snapshot copies, model/history copies,
            // PNG pixels/decoder, archive roles and HTML workspace. The shared
            // 256 MiB limit stays fixed; CNA1 reserves more of it.
            let memory = std::sync::Arc::new(
                tailsync_core::composite::budgets::global()
                    .reserve_pipeline(240 * 1024 * 1024)
                    .map_err(|e| e.to_string())?,
            );
            #[cfg(test)]
            {
                *provider.pipeline_lease.lock().unwrap() = Some(Arc::downgrade(&memory));
            }
            let archive = provider.read_archive()?;
            let snapshot = archive.original.clone();
            if !tailsync_core::composite::capture_retry::CaptureRetry::matches_snapshot(
                observed, &snapshot,
            ) {
                return Err("Clipboard changed during capture".to_string());
            }
            let captured = tailsync_core::composite::from_archive(&archive);
            Ok::<_, String>((snapshot, captured, memory, archive))
        })
        .await;
        if !snapshot_retry.is_current(observed, env.revision())
            || CLIPBOARD_RECOVERY_GENERATION.load(Ordering::Acquire) != recovery_generation
        {
            // Discard late provider/conversion results without acknowledging
            // the new clipboard or spending its retry allowance.
            snapshot_retry.clear();
            return CompositeCapture::Handled;
        }
        match captured {
            Ok(Ok((snapshot, Ok(bundle), memory, archive))) => {
                use tailsync_core::composite::Completeness;
                let origin = if archive.derived.is_empty() {
                    env.classify(&snapshot, &bundle)
                } else {
                    env.classify_native(&snapshot)
                };
                if origin == Origin::SelfWrite {
                    acknowledge_capture(snapshot_retry, ack, snapshot.after);
                    return CompositeCapture::Handled;
                }
                let capture_generation = sync_engine.lock().await.supersede_file_clipboard();
                if bundle.completeness == Completeness::MissingFormats
                    && !snapshot_retry.exhausted()
                {
                    snapshot_retry.retry(observed, env.now());
                    return CompositeCapture::Handled;
                }

                let retained_bytes = archive
                    .original
                    .formats
                    .iter()
                    .map(|f| f.bytes.len())
                    .fold(0usize, usize::saturating_add)
                    .saturating_mul(2)
                    .saturating_add(archive.derived.iter().map(|f| f.bytes.len()).sum::<usize>());
                let images = prepare_capture_images(
                    CaptureImageInput {
                        bundle: &bundle,
                        retained_bytes,
                        memory: memory.clone(),
                    },
                    archive_ticket,
                    origin,
                    capture_generation,
                    snapshot.after,
                    PortablePublishContext {
                        database,
                        sync_engine,
                        pool,
                        settings,
                        image_archive,
                        recovery_generation,
                        env,
                    },
                )
                .await;
                let archive_candidate = origin == Origin::External
                    && bundle.representations.iter().any(|representation| {
                        representation.mime == tailsync_core::composite::Mime::Html
                    });
                let db = database.clone();
                let held = memory.clone();
                let saved = tokio::task::spawn_blocking(move || {
                    let _held = held;
                    let mut db = db.blocking_lock();
                    if !db.is_storage_available() {
                        return Err("History storage unavailable".to_string());
                    }
                    let (id, enriched) = match images {
                        Some(images) => match db.add_native_archive_with_images(&archive, &images, "self") {
                            Ok(id) => (id, Some(images.bundle)),
                            Err(error) => {
                                warn!("Network image storage failed; preserving the source: {error}");
                                (db.add_native_archive(&archive, "self").map_err(|e| e.to_string())?, None)
                            }
                        },
                        None => (db.add_native_archive(&archive, "self").map_err(|e| e.to_string())?, None),
                    };
                    let guard = archive_candidate
                        .then(|| {
                            tailsync_runtime::history::ArchiveEntryGuard::capture(&db, id).ok()
                        })
                        .flatten();
                    Ok::<_, String>((id, guard, enriched))
                })
                .await;
                let (id, guard, enriched) = match saved {
                    Ok(Ok(saved)) => saved,
                    _ => {
                        finish_failed_capture(
                            observed,
                            CaptureFailure::NotSaved,
                            snapshot_retry,
                            ack,
                            env,
                        )
                        .await;
                        return CompositeCapture::Handled;
                    }
                };
                let bundle = enriched.unwrap_or(bundle);
                acknowledge_capture(snapshot_retry, ack, snapshot.after);
                crate::api::bump_clipboard_version();
                let archive_binding =
                    guard.map(|guard| tailsync_runtime::history::ArchiveCommit { id, guard });
                publish_saved_portable_capture(
                    SavedPortableCapture {
                        snapshot,
                        bundle,
                        memory,
                        origin,
                        capture_generation,
                        archive_ticket,
                        archive_binding,
                    },
                    PortablePublishContext {
                        database,
                        sync_engine,
                        pool,
                        settings,
                        image_archive,
                        recovery_generation,
                        env,
                    },
                )
                .await;
                CompositeCapture::Handled
            }
            Ok(Ok((
                snapshot,
                Err(
                    tailsync_core::composite::CaptureError::UnsupportedItems
                    | tailsync_core::composite::CaptureError::NativeOnly,
                ),
                memory,
                archive,
            ))) => {
                let origin = env.classify_native(&snapshot);
                if origin == Origin::SelfWrite {
                    acknowledge_capture(snapshot_retry, ack, snapshot.after);
                    return CompositeCapture::Handled;
                }
                sync_engine.lock().await.supersede_file_clipboard();
                let db = database.clone();
                let held = memory.clone();
                let saved = tokio::task::spawn_blocking(move || {
                    let _held = held;
                    let mut db = db.blocking_lock();
                    if !db.is_storage_available() {
                        return Err("History storage unavailable".to_string());
                    }
                    db.add_native_archive(&archive, "self")
                        .map_err(|e| e.to_string())
                })
                .await;
                if matches!(saved, Ok(Ok(_))) {
                    acknowledge_capture(snapshot_retry, ack, snapshot.after);
                    tailsync_core::sync_warning::record_saved_capture(
                        &snapshot,
                        None,
                        origin,
                        crate::clipboard_composite::supports_native(&snapshot),
                    );
                    crate::api::bump_clipboard_version();
                } else {
                    finish_failed_capture(
                        observed,
                        CaptureFailure::NotSaved,
                        snapshot_retry,
                        ack,
                        env,
                    )
                    .await;
                }
                CompositeCapture::Handled
            }
            Ok(Ok((_, Err(tailsync_core::composite::CaptureError::Files), _memory, _archive))) => {
                snapshot_retry.clear();
                CompositeCapture::FilesFallback
            }
            Ok(Ok((
                snapshot,
                Err(tailsync_core::composite::CaptureError::Empty),
                _memory,
                _archive,
            ))) => {
                env.classify_native(&snapshot);
                sync_engine.lock().await.supersede_file_clipboard();
                acknowledge_capture(snapshot_retry, ack, snapshot.after);
                CompositeCapture::Handled
            }
            Ok(Ok((
                snapshot,
                Err(tailsync_core::composite::CaptureError::Suppressed),
                _memory,
                _archive,
            ))) => {
                // Metadata suppression is a completed decision with no
                // history, broadcast, fault body or fallback reader.
                sync_engine.lock().await.supersede_file_clipboard();
                acknowledge_capture(snapshot_retry, ack, snapshot.after);
                CompositeCapture::Handled
            }
            _ => {
                finish_failed_capture(observed, CaptureFailure::Failed, snapshot_retry, ack, env)
                    .await;
                CompositeCapture::Handled
            }
        }
    }

    #[cfg(test)]
    mod tests {
        //! Behaviour tests for the production composite-capture seam.
        //!
        //! Every test drives [`process_composite_capture`] — the function the real
        //! `clipboard_loop` calls — with a validated `NativeArchive`, the real
        //! `from_archive` conversion, the real CNA1 240 MiB reserve, an isolated
        //! SQLite store, a real `SyncEngine`/`ConnectionPool` and the real broadcast
        //! gate. Only the OS/provider edge, the clock, the origin tracker instance
        //! and the fault sink are injected.
        use super::*;
        use std::path::PathBuf;
        use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64};
        use std::sync::{Condvar, Mutex as StdMutex};
        use std::time::Duration;

        use tailsync_core::cancellation::Cancellation;
        use tailsync_core::composite::fingerprint::{
            origin_fingerprint, origin_fingerprint_from_native, requires_original_domain,
        };
        use tailsync_core::composite::origin::OriginTracker;
        use tailsync_core::composite::resource_archive::{
            ArchiveError, ArchiveUrl, FetchFuture, FetchLimits, ResourceTransport,
        };
        use tailsync_core::peer::delivery::QueuedFrame;
        use tailsync_core::peer::pool::PoolSender;
        use tailsync_core::peer::types::ResolvedTarget;
        use tailsync_core::protocol::native_archive::NativeArchive;
        use tailsync_core::protocol::native_clipboard::{
            FormatState, NativeFormat, NativeSnapshot, SnapshotState,
        };

        use crate::automatic_images::AutomaticImageArchive;
        use crate::crypto::Settings;
        use crate::db::HistoryDB;
        use crate::identity::DeviceIdentity;
        use crate::network::ConnectionPool;
        use crate::protocol::{Command, TransferId};
        use crate::sync::{
            FileBatchProgress, FileReceiveCommit, PlatformResultFuture, SyncEngine, SyncPlatform,
        };

        include!("../../../shared/platform-clipboard-capture-tests.rs");
        include!("../tests/support/clipboard_capture_archives.rs");
        include!("../tests/support/format_roundtrip.rs");

        const PEER_HOST: &str = "peer-f3a1";
        const PEER_ADDRESS: &str = "100.64.0.2";
        const SELF_TOKEN: [u8; 16] = [7; 16];

        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        enum Shape {
            Portable,
            Html,
            HtmlImage,
            MissingFormats,
            NativeOnly,
            Files,
            Suppressed,
            Empty,
            Failed,
        }

        #[derive(Clone, Copy, PartialEq, Eq)]
        enum OriginMode {
            External,
            SelfWrite,
            Mismatch,
        }

        fn text_format() -> NativeFormat {
            NativeFormat {
                item: 0,
                name: "public.utf8-plain-text".into(),
                state: FormatState::Captured,
                bytes: b"hello f3a1".to_vec(),
            }
        }

        fn snapshot(shape: Shape, rev: i64) -> NativeSnapshot {
            let base = |state, items| NativeSnapshot {
                before: rev,
                after: rev,
                items,
                state,
                formats: vec![],
            };
            match shape {
                Shape::Portable => NativeSnapshot {
                    before: rev,
                    after: rev,
                    items: 1,
                    state: SnapshotState::Stable,
                    formats: vec![text_format()],
                },
                Shape::Html | Shape::HtmlImage => NativeSnapshot {
                    before: rev,
                    after: rev,
                    items: 1,
                    state: SnapshotState::Stable,
                    formats: vec![
                        text_format(),
                        NativeFormat {
                            item: 0,
                            name: "public.html".into(),
                            state: FormatState::Captured,
                            bytes: if shape == Shape::HtmlImage {
                                b"<html><body>f3 image<img src=\"https://fixture.example/capture-image.png\"></body></html>".to_vec()
                            } else {
                                b"<html><body>f3a1</body></html>".to_vec()
                            },
                        },
                    ],
                },
                Shape::MissingFormats => NativeSnapshot {
                    before: rev,
                    after: rev,
                    items: 1,
                    state: SnapshotState::Stable,
                    formats: vec![
                        text_format(),
                        NativeFormat {
                            item: 0,
                            name: "public.rtf".into(),
                            state: FormatState::Failed,
                            bytes: vec![],
                        },
                    ],
                },
                Shape::NativeOnly => NativeSnapshot {
                    before: rev,
                    after: rev,
                    items: 1,
                    state: SnapshotState::Stable,
                    formats: vec![NativeFormat {
                        item: 0,
                        name: "org.fixture.original".into(),
                        state: FormatState::Captured,
                        bytes: b"opaque original".to_vec(),
                    }],
                },
                Shape::Files => base(SnapshotState::FileSelection, 1),
                Shape::Suppressed => base(SnapshotState::Suppressed, 0),
                Shape::Empty => base(SnapshotState::Empty, 0),
                Shape::Failed => base(SnapshotState::Failed, 0),
            }
        }

        /// A complete, protocol-valid native archive, exactly like the helper emits.
        fn fixture_archive(shape: Shape, rev: i64) -> NativeArchive {
            let archive = NativeArchive {
                platform: 1,
                enumeration_complete: true,
                original: snapshot(shape, rev),
                derived: vec![],
            };
            archive
                .validate()
                .expect("fixture must be a valid native archive");
            archive
        }

        /// Blocks the provider thread until the test releases it, so a late result
        /// can be raced against a new clipboard revision.
        #[derive(Clone)]
        struct ProviderGate {
            state: Arc<(StdMutex<bool>, Condvar)>,
        }

        impl Default for ProviderGate {
            fn default() -> Self {
                // Open by default: a test closes it only to hold the provider.
                Self {
                    state: Arc::new((StdMutex::new(true), Condvar::new())),
                }
            }
        }

        /// Releases the provider gate even if the test unwinds, so a failed
        /// assertion cannot leave a blocking worker parked forever.
        struct GateRelease(ProviderGate);
        impl Drop for GateRelease {
            fn drop(&mut self) {
                self.0.open();
            }
        }

        impl ProviderGate {
            fn open_on_drop(&self) -> GateRelease {
                GateRelease(self.clone())
            }
            fn close(&self) {
                *self.state.0.lock().unwrap() = false;
            }
            fn open(&self) {
                let (lock, cv) = &*self.state;
                *lock.lock().unwrap() = true;
                cv.notify_all();
            }
            fn wait(&self) {
                let (lock, cv) = &*self.state;
                let mut open = lock.lock().unwrap();
                while !*open {
                    open = cv.wait(open).unwrap();
                }
            }
        }

        struct NoFetch;
        impl ResourceTransport for NoFetch {
            fn fetch_once<'a>(
                &'a self,
                _url: &'a ArchiveUrl,
                _limits: &'a FetchLimits,
                _cancellation: &'a Cancellation,
            ) -> FetchFuture<'a> {
                Box::pin(async { Err(ArchiveError::Transport) })
            }
        }

        struct RecordingAck {
            acks: Arc<StdMutex<Vec<i64>>>,
        }
        impl CaptureAcknowledger for RecordingAck {
            fn acknowledge(&mut self, revision: i64) {
                self.acks.lock().unwrap().push(revision);
            }
        }

        struct TestPlatform {
            revision: Arc<AtomicI64>,
        }
        impl SyncPlatform for TestPlatform {
            fn write_text(&self, _: &str) -> Result<(), String> {
                Ok(())
            }
            fn write_image(&self, _: u32, _: u32, _: &[u8]) -> Result<(), String> {
                Ok(())
            }
            fn clipboard_revision(&self) -> Option<i64> {
                let value = self.revision.load(Ordering::SeqCst);
                (value >= 0).then_some(value)
            }
            fn set_file_progress(&self, _: &str, _: u64, _: u64) {}
            fn clear_file_progress(&self, _: Option<TransferId>, _: Option<&str>) {}
            fn set_file_batch_progress(&self, _: FileBatchProgress) {}
            fn files_received(&self, _: FileReceiveCommit) -> PlatformResultFuture {
                Box::pin(async { Ok(()) })
            }
            fn file_batch_failed(&self, _: Option<TransferId>, _: &str) {}
        }

        struct Fixture {
            root: PathBuf,
            database: Arc<Mutex<HistoryDB>>,
            engine: Arc<Mutex<SyncEngine>>,
            pool: Arc<Mutex<ConnectionPool>>,
            settings: Arc<Mutex<Settings>>,
            archive: Arc<AutomaticImageArchive>,
            revision: Arc<AtomicI64>,
            classify_notify: Arc<tokio::sync::Notify>,
            clock_offset_ms: Arc<AtomicU64>,
            faults: Arc<StdMutex<Vec<(i64, &'static str)>>>,
            fault_ok: Arc<AtomicBool>,
            provider_entered: Arc<tokio::sync::Notify>,
            gate: ProviderGate,
            fixture_archive: NativeArchive,
            retry: StdMutex<tailsync_core::composite::capture_retry::CaptureRetry>,
            tickets: StdMutex<tailsync_runtime::automatic_image_archive::CaptureTicketTracker>,
            ack: StdMutex<RecordingAck>,
            env: CaptureEnvironment,
        }

        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.root);
            }
        }

        async fn fixture(
            shape: Shape,
            origin_mode: OriginMode,
            archive_enabled: bool,
            with_peer: bool,
        ) -> Fixture {
            // A real isolated store with the composite schema. The production
            // policy would only create it when the capture helper is serviceable,
            // which it is not inside a unit test, so the fixture asks for the
            // composite schema explicitly (the same shape production runs with).
            let root =
                std::env::temp_dir().join(format!("tailsync-f3a1-{:016x}", rand::random::<u64>()));

            let revision = 100_i64;
            let archive_fixture = fixture_archive(shape, revision);

            let database = Arc::new(Mutex::new(
                HistoryDB::open_isolated_for_test_composite(&root).unwrap(),
            ));
            let settings = Arc::new(Mutex::new(Settings {
                notifications_enabled: false,
                sync_enabled: true,
                ..Settings::default()
            }));
            let identity = Arc::new(DeviceIdentity::generate_for_test());

            if with_peer {
                // The peer is resolved through the production `merge_paired_peers`
                // path from the pinned key plus a remembered LAN address. The
                // process-global peer cache is deliberately not seeded: it holds a
                // single (mode, cache) state, so seeding it would race every other
                // `store_peer_cache`/`clear_peer_cache` case in this binary. The
                // cost is the cold-cache discovery timeout inside `configured_peers`.
                let mut s = settings.lock().await;
                s.trusted_peer_keys
                    .insert(PEER_HOST.into(), identity.public_key_base64());
                s.trusted_peer_addresses
                    .entry(PEER_HOST.into())
                    .or_default()
                    .insert("lan".into(), PEER_ADDRESS.into());
                s.enabled_peers.insert(PEER_HOST.into(), true);
            }

            let pool = Arc::new(Mutex::new(ConnectionPool::new(
                identity.clone(),
                settings.clone(),
            )));
            let rev_atomic = Arc::new(AtomicI64::new(revision));
            let engine = Arc::new(Mutex::new(SyncEngine::new()));
            engine.lock().await.set_platform(Arc::new(TestPlatform {
                revision: rev_atomic.clone(),
            }));

            let archive = Arc::new(AutomaticImageArchive::new(
                database.clone(),
                archive_enabled,
                Arc::new(NoFetch),
                Arc::new(|| {}),
            ));

            // Seed a real origin tracker with the *real* fingerprint of the bundle
            // this fixture converts to, so SelfWrite/Mismatch are genuine tracker
            // outcomes rather than a fabricated `Origin` value.
            let tracker = Arc::new(StdMutex::new(OriginTracker::default()));
            if origin_mode != OriginMode::External {
                let bundle = tailsync_core::composite::from_archive(&archive_fixture)
                    .expect("fixture must convert to a portable bundle");
                let digest = origin_fingerprint(&bundle).expect("fixture fingerprint");
                let seeded = if origin_mode == OriginMode::SelfWrite {
                    digest
                } else {
                    let mut flipped = digest;
                    flipped[0] ^= 0xff;
                    flipped
                };
                tracker.lock().unwrap().record(revision, seeded, SELF_TOKEN);
            }

            let classify_notify = Arc::new(tokio::sync::Notify::new());
            let clock_base = std::time::Instant::now();
            let clock_offset_ms = Arc::new(AtomicU64::new(0));
            let faults: Arc<StdMutex<Vec<(i64, &'static str)>>> =
                Arc::new(StdMutex::new(Vec::new()));
            let fault_ok = Arc::new(AtomicBool::new(false));
            let provider_entered = Arc::new(tokio::sync::Notify::new());
            let gate = ProviderGate::default();
            let acks = Arc::new(StdMutex::new(Vec::new()));

            let env = {
                let rev = rev_atomic.clone();
                let archive_for_read = archive_fixture.clone();
                let gate = gate.clone();
                let entered = provider_entered.clone();
                let tracker_classify = tracker.clone();
                let tracker_native = tracker.clone();
                let classify_signal = classify_notify.clone();
                let classify_signal_native = classify_notify.clone();
                let clock = clock_offset_ms.clone();
                let faults_sink = faults.clone();
                let fault_ok_sink = fault_ok.clone();
                CaptureEnvironment {
                    revision: Arc::new(move || {
                        let value = rev.load(Ordering::SeqCst);
                        (value >= 0).then_some(value)
                    }),
                    read_archive: Arc::new(move || {
                        entered.notify_one();
                        gate.wait();
                        Ok(archive_for_read.clone())
                    }),
                    classify: Arc::new(
                        move |snapshot: &NativeSnapshot, bundle: &PortableBundle| {
                            classify_signal.notify_one();
                            let mut tracker = tracker_classify.lock().unwrap();
                            let digest = if requires_original_domain(snapshot) {
                                tailsync_core::composite::fingerprint::origin_fingerprint_from_capture(snapshot).ok()
                            } else {
                                origin_fingerprint(bundle).ok()
                            };
                            let token = snapshot
                                .formats
                                .iter()
                                .find(|f| f.name == "org.tailsync.clipboard-source")
                                .and_then(|f| f.bytes.as_slice().try_into().ok());
                            match digest {
                                Some(digest) => {
                                    tracker.classify(Some(snapshot.after), digest, token)
                                }
                                None => Origin::UnknownMismatch,
                            }
                        },
                    ),
                    classify_native: Arc::new(move |snapshot: &NativeSnapshot| {
                        classify_signal_native.notify_one();
                        let mut tracker = tracker_native.lock().unwrap();
                        let digest =
                            tailsync_core::composite::fingerprint::origin_fingerprint_from_capture(
                                snapshot,
                            )
                            .ok();
                        match digest {
                            Some(digest) => tracker.classify(Some(snapshot.after), digest, None),
                            None => Origin::UnknownMismatch,
                        }
                    }),
                    clock: Arc::new(move || {
                        clock_base + Duration::from_millis(clock.load(Ordering::SeqCst))
                    }),
                    capture_fault: Arc::new(move |revision, kind| {
                        faults_sink.lock().unwrap().push((revision, kind));
                        let ok = fault_ok_sink.load(Ordering::SeqCst);
                        Box::pin(async move { ok })
                    }),
                    pipeline_lease: Arc::default(),
                }
            };

            Fixture {
                root,
                database,
                engine,
                pool,
                settings,
                archive,
                revision: rev_atomic,
                classify_notify,
                clock_offset_ms,
                faults,
                fault_ok,
                provider_entered,
                gate,
                fixture_archive: archive_fixture,
                retry: StdMutex::new(Default::default()),
                tickets: StdMutex::new(Default::default()),
                ack: StdMutex::new(RecordingAck { acks }),
                env,
            }
        }

        impl Fixture {
            fn set_revision(&self, revision: i64) {
                self.revision.store(revision, Ordering::SeqCst);
            }

            fn advance_clock_ms(&self, ms: u64) {
                self.clock_offset_ms.fetch_add(ms, Ordering::SeqCst);
            }

            fn acks(&self) -> Vec<i64> {
                self.ack.lock().unwrap().acks.lock().unwrap().clone()
            }

            fn faults(&self) -> Vec<(i64, &'static str)> {
                self.faults.lock().unwrap().clone()
            }

            fn observe_ticket(
                &self,
                revision: i64,
            ) -> Option<tailsync_runtime::automatic_image_archive::CaptureTicket> {
                self.tickets
                    .lock()
                    .unwrap()
                    .observe(&self.archive, revision)
            }

            // The per-loop state is a real `&mut` in production; the fixture keeps
            // it behind a std mutex so one test can drive several ticks. Nothing
            // contends these guards, so holding them across the seam's awaits is
            // exactly the production shape.
            #[allow(clippy::await_holding_lock)]
            async fn tick(&self, observed: i64) -> CompositeCapture {
                let recovery_generation = CLIPBOARD_RECOVERY_GENERATION.load(Ordering::Acquire);
                let mut retry = self.retry.lock().unwrap();
                let mut tickets = self.tickets.lock().unwrap();
                let mut ack = self.ack.lock().unwrap();
                let result = tokio::time::timeout(
                    TEST_WAIT,
                    process_composite_capture(
                        observed,
                        CompositeCaptureContext {
                            database: &self.database,
                            sync_engine: &self.engine,
                            pool: &self.pool,
                            settings: &self.settings,
                            image_archive: &self.archive,
                            retry: &mut retry,
                            archive_observation: &mut tickets,
                            ack: &mut *ack,
                            recovery_generation,
                            env: &self.env,
                        },
                    ),
                )
                .await
                .expect("capture transaction timed out");
                let lease = self.env.pipeline_lease.lock().unwrap().clone();
                if let Some(lease) = lease {
                    assert!(
                        lease.upgrade().is_none(),
                        "finished capture retained an original pipeline admission clone"
                    );
                }
                result
            }

            async fn history_len(&self) -> usize {
                self.database
                    .lock()
                    .await
                    .get_all(None, None, 50, 0)
                    .unwrap()
                    .len()
            }

            async fn history_id(&self) -> i64 {
                self.database
                    .lock()
                    .await
                    .get_all(None, None, 50, 0)
                    .unwrap()[0]
                    .id
            }
        }

        /// Install a real `PoolSender` for the capture lane so the production
        /// broadcast reaches a bounded queue instead of a real socket.
        struct CaptureQueue {
            filler: tokio::sync::mpsc::Sender<QueuedFrame>,
            priority: tokio::sync::mpsc::Receiver<QueuedFrame>,
            // Both receivers must stay alive: a dropped receiver makes the pooled
            // sender look dead and the pool would spawn a fresh worker instead of
            // reusing the queue under test.
            _bulk: tokio::sync::mpsc::Receiver<QueuedFrame>,
            _shutdown: tokio::sync::watch::Receiver<bool>,
        }

        async fn install_capture_sender(pool: &Arc<Mutex<ConnectionPool>>) -> CaptureQueue {
            let (priority, priority_rx) = tokio::sync::mpsc::channel(1);
            let filler = priority.clone();
            let (bulk, bulk_rx) = tokio::sync::mpsc::channel(1);
            let (shutdown, shutdown_rx) = tokio::sync::watch::channel(false);
            let sender = PoolSender::new(priority, bulk, shutdown);
            pool.lock().await.insert_capture_sender(
                ResolvedTarget::Tcp(format!("{PEER_ADDRESS}:19890").parse().unwrap()),
                PEER_HOST.into(),
                sender,
            );
            CaptureQueue {
                filler,
                priority: priority_rx,
                _bulk: bulk_rx,
                _shutdown: shutdown_rx,
            }
        }

        /// A single capture reserves 240 MiB of the process-global 256 MiB pipeline
        /// budget, so two capture ticks in this binary cannot overlap: the second
        /// reservation would fail and every assertion would see a spurious capture
        /// fault. The recovery counter is a second process-global, and the warning
        /// queue a third. This lock serialises only this module's tests — each test
        /// still runs its own workers and races internally.
        fn capture_serial() -> &'static tokio::sync::Mutex<()> {
            static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
            LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
        }

        #[derive(Clone, Copy, Debug)]
        enum Supersede {
            Nothing,
            Revision,
            Generation,
            Recovery,
        }

        /// Every rendezvous in these tests is bounded, so a future regression can
        /// only fail a test, never park the whole suite on a leaked wait.
        const TEST_WAIT: Duration = Duration::from_secs(15);

        async fn wait_for_signal(signal: &tokio::sync::Notify, what: &str) {
            tokio::time::timeout(TEST_WAIT, signal.notified())
                .await
                .unwrap_or_else(|_| panic!("timed out waiting for {what}"));
        }

        async fn wait_for_capture_queue(pool: &Arc<Mutex<ConnectionPool>>) {
            tokio::time::timeout(TEST_WAIT, async {
                loop {
                    if pool.lock().await.capture_queue_attempts_for_test() > 0 {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("capture never reached the actual enqueue");
        }

        async fn recv_bounded(
            receiver: &mut tokio::sync::mpsc::Receiver<QueuedFrame>,
            what: &str,
        ) -> QueuedFrame {
            tokio::time::timeout(TEST_WAIT, receiver.recv())
                .await
                .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
                .unwrap_or_else(|| panic!("the capture queue closed before {what}"))
        }

        fn drain_warnings() -> Vec<&'static str> {
            let mut kinds = Vec::new();
            while let Some(warning) = tailsync_core::sync_warning::take() {
                kinds.push(warning.kind);
            }
            kinds
        }

        #[tokio::test]
        async fn external_portable_is_saved_before_it_is_broadcast() {
            let _serial = capture_serial().lock().await;
            let fx = fixture(Shape::Portable, OriginMode::External, false, true).await;
            let mut queue = install_capture_sender(&fx.pool).await;

            // Hold the store while the capture reaches its save, then prove nothing
            // has been queued yet.
            // The injected classifier runs after the provider, the first revision
            // check and the origin decision, and immediately before the generation
            // supersede and the save. Waiting there (not on a later revision read,
            // which the held store would block) is the exact pre-save rendezvous.
            let hold = fx.database.lock().await;
            let mut task = Box::pin(fx.tick(100));
            tokio::select! {
                _ = wait_for_signal(&fx.classify_notify, "the storage rendezvous before the save") => {}
                _ = &mut task => panic!("capture finished while storage was held"),
            }
            assert!(
                queue.priority.try_recv().is_err(),
                "no payload may be queued before the save"
            );
            drop(hold);

            assert_eq!(task.await, CompositeCapture::Handled);
            assert_eq!(fx.history_len().await, 1);
            assert_eq!(fx.acks(), vec![100]);
            assert!(fx.faults().is_empty());
            let queued = queue
                .priority
                .try_recv()
                .expect("external capture must be broadcast");
            assert_eq!(queued.command(), Command::CompositePayload);
            assert!(
                queue.priority.try_recv().is_err(),
                "exactly one payload per capture"
            );
            fx.set_revision(100);
        }

        #[tokio::test]
        async fn self_write_is_acknowledged_without_saving_or_broadcasting() {
            let _serial = capture_serial().lock().await;
            let fx = fixture(Shape::Portable, OriginMode::SelfWrite, false, true).await;
            let mut queue = install_capture_sender(&fx.pool).await;

            assert_eq!(fx.tick(100).await, CompositeCapture::Handled);
            assert_eq!(fx.history_len().await, 0, "our own write is not history");
            assert_eq!(fx.acks(), vec![100]);
            assert!(fx.faults().is_empty());
            assert!(
                queue.priority.try_recv().is_err(),
                "our own write is never re-broadcast"
            );
        }

        #[tokio::test]
        async fn unknown_mismatch_keeps_history_and_warns_without_broadcast() {
            let _serial = capture_serial().lock().await;
            let _ = drain_warnings();
            let fx = fixture(Shape::Html, OriginMode::Mismatch, false, true).await;
            let mut queue = install_capture_sender(&fx.pool).await;

            assert_eq!(fx.tick(100).await, CompositeCapture::Handled);
            assert_eq!(fx.history_len().await, 1, "an unknown origin is still kept");
            assert_eq!(fx.acks(), vec![100]);
            assert!(
                queue.priority.try_recv().is_err(),
                "an unknown origin is never re-broadcast"
            );
            assert!(
                drain_warnings().contains(&"capture_origin_unknown"),
                "an unknown origin must be a visible warning"
            );
        }

        #[tokio::test]
        async fn suppressed_decision_has_no_side_effects() {
            let _serial = capture_serial().lock().await;
            let fx = fixture(Shape::Suppressed, OriginMode::External, false, true).await;
            let mut queue = install_capture_sender(&fx.pool).await;

            assert_eq!(fx.tick(100).await, CompositeCapture::Handled);
            assert_eq!(fx.history_len().await, 0);
            assert_eq!(fx.acks(), vec![100]);
            assert!(fx.faults().is_empty(), "suppression is not a capture fault");
            assert!(queue.priority.try_recv().is_err());
        }

        #[tokio::test]
        async fn files_decision_returns_the_legacy_fallback() {
            let _serial = capture_serial().lock().await;
            let fx = fixture(Shape::Files, OriginMode::External, false, true).await;
            let mut queue = install_capture_sender(&fx.pool).await;

            assert_eq!(fx.tick(100).await, CompositeCapture::FilesFallback);
            assert_eq!(fx.history_len().await, 0);
            assert!(fx.acks().is_empty());
            assert!(queue.priority.try_recv().is_err());
        }

        #[tokio::test]
        async fn empty_capture_is_acknowledged_without_history() {
            let _serial = capture_serial().lock().await;
            let fx = fixture(Shape::Empty, OriginMode::External, false, true).await;

            assert_eq!(fx.tick(100).await, CompositeCapture::Handled);
            assert_eq!(fx.history_len().await, 0);
            assert_eq!(fx.acks(), vec![100]);
        }

        #[tokio::test]
        async fn failed_capture_retries_without_acking_until_the_fault_is_durable() {
            let _serial = capture_serial().lock().await;
            let fx = fixture(Shape::Failed, OriginMode::External, false, true).await;

            for _ in 0..3 {
                assert_eq!(fx.tick(100).await, CompositeCapture::Handled);
                assert!(
                    fx.acks().is_empty(),
                    "a retrying failure is not acknowledged"
                );
                fx.advance_clock_ms(2000);
            }
            assert_eq!(fx.tick(100).await, CompositeCapture::Handled);
            assert_eq!(fx.faults(), vec![(100, "capture_failed")]);
            assert!(
                fx.acks().is_empty(),
                "a failed fault record must not swallow the copy"
            );

            fx.fault_ok.store(true, Ordering::SeqCst);
            assert_eq!(fx.tick(100).await, CompositeCapture::Handled);
            assert_eq!(fx.acks(), vec![100]);
        }

        #[tokio::test]
        async fn native_only_keeps_the_original_without_portable_broadcast() {
            let _serial = capture_serial().lock().await;
            let fx = fixture(Shape::NativeOnly, OriginMode::External, false, true).await;
            let mut queue = install_capture_sender(&fx.pool).await;

            assert_eq!(fx.tick(100).await, CompositeCapture::Handled);
            assert_eq!(
                fx.history_len().await,
                1,
                "the original archive is retained"
            );
            assert_eq!(fx.acks(), vec![100]);
            assert!(
                queue.priority.try_recv().is_err(),
                "a native-only capture has no portable payload to broadcast"
            );
            let id = fx.history_id().await;
            assert!(
                fx.database
                    .lock()
                    .await
                    .get_composite_archive(id)
                    .unwrap()
                    .is_some(),
                "the retained original must be readable as an archive"
            );
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        async fn late_provider_result_does_not_spend_the_new_copies_retry_budget() {
            let _serial = capture_serial().lock().await;
            let mut fx = fixture(Shape::Portable, OriginMode::External, false, true).await;
            let mut queue = install_capture_sender(&fx.pool).await;
            fx.gate.close();
            let _release = fx.gate.open_on_drop();

            let mut task = Box::pin(fx.tick(100));
            tokio::select! {
                _ = wait_for_signal(&fx.provider_entered, "the capture provider to start") => {}
                _ = &mut task => panic!("capture finished before the provider was entered"),
            }
            // A newer copy supersedes the in-flight read while it is still running.
            fx.set_revision(101);
            fx.gate.open();
            drop(_release);

            assert_eq!(task.await, CompositeCapture::Handled);
            assert_eq!(fx.history_len().await, 0, "a late result must not save");
            assert!(fx.acks().is_empty());
            assert!(
                fx.faults().is_empty(),
                "a discarded late result is not a fault"
            );
            assert!(queue.priority.try_recv().is_err());

            // The new copy starts with a fresh allowance: it is captured, not blocked
            // by the discarded attempt's exhausted budget.
            fx.fixture_archive = fixture_archive(Shape::Portable, 101);
            let mut env = fx.env.clone();
            let archive = fx.fixture_archive.clone();
            let gate = fx.gate.clone();
            let entered = fx.provider_entered.clone();
            env.read_archive = Arc::new(move || {
                entered.notify_one();
                gate.wait();
                Ok(archive.clone())
            });
            fx.env = env;

            assert_eq!(fx.tick(101).await, CompositeCapture::Handled);
            assert_eq!(
                fx.history_len().await,
                1,
                "the new copy is saved immediately"
            );
            assert_eq!(fx.acks(), vec![101]);
        }

        /// The final broadcast gate re-checks the generation, revision and recovery
        /// counter under the engine lock. Each item is superseded on its own, after
        /// the save and the pre-broadcast check have already run, so a refusal can
        /// only come from the real enqueue gate.
        #[tokio::test]
        async fn final_broadcast_gate_refuses_a_capture_superseded_after_storage() {
            let _serial = capture_serial().lock().await;
            for supersede in [
                Supersede::Nothing,
                Supersede::Revision,
                Supersede::Generation,
                Supersede::Recovery,
            ] {
                let fx = fixture(Shape::Portable, OriginMode::External, false, true).await;
                let mut queue = install_capture_sender(&fx.pool).await;
                // Fill the bounded capture queue so the broadcast parks on real
                // backpressure instead of racing the gate.
                queue
                    .filler
                    .send(QueuedFrame::new(Command::Heartbeat, vec![]).unwrap())
                    .await
                    .unwrap();

                let mut task = Box::pin(fx.tick(100));
                tokio::select! {
                    _ = wait_for_capture_queue(&fx.pool) => {}
                    _ = &mut task => panic!("capture finished before reaching the broadcast gate"),
                }
                // The store and the pre-broadcast generation check have both run:
                // the capture is parked on the reserved queue slot, not returning
                // early. Superseding now can only be observed by the final gate.
                std::future::poll_fn(|cx| {
                    assert!(
                        task.as_mut().poll(cx).is_pending(),
                        "the capture must be blocked on queue backpressure, not an early return"
                    );
                    std::task::Poll::Ready(())
                })
                .await;
                assert_eq!(fx.history_len().await, 1, "the save already completed");

                match supersede {
                    Supersede::Nothing => {}
                    Supersede::Revision => fx.set_revision(101),
                    Supersede::Generation => {
                        fx.engine.lock().await.supersede_file_clipboard();
                    }
                    Supersede::Recovery => crate::clipboard::request_wake_recovery(),
                }

                // Release the reserved slot; the gate now decides the handoff.
                let _ = recv_bounded(&mut queue.priority, "the reserved slot to be released").await;
                assert_eq!(task.await, CompositeCapture::Handled);

                match supersede {
                    Supersede::Nothing => {
                        let queued = queue
                            .priority
                            .try_recv()
                            .expect("a capture that stayed current is broadcast");
                        assert_eq!(queued.command(), Command::CompositePayload);
                    }
                    other => assert!(
                        queue.priority.try_recv().is_err(),
                        "{other:?}: a superseded capture must never enter the real worker queue"
                    ),
                }
            }
        }

        #[tokio::test]
        async fn parent_unknown_origin_is_saved_but_never_automatically_archived() {
            let _serial = capture_serial().lock().await;
            let mut fx = fixture(Shape::Html, OriginMode::External, true, true).await;
            let mut queue = install_capture_sender(&fx.pool).await;
            fx.env.classify = Arc::new(|_, bundle| {
                OriginTracker::default().classify(None, origin_fingerprint(bundle).unwrap(), None)
            });
            assert_eq!(fx.tick(100).await, CompositeCapture::Handled);
            assert_eq!(fx.history_len().await, 1);
            assert_eq!(fx.acks(), vec![100]);
            assert!(queue.priority.try_recv().is_err());
            assert!(drain_warnings().contains(&"capture_origin_unknown"));
            let id = fx.history_id().await;
            let guard = tailsync_runtime::history::ArchiveEntryGuard::capture(
                &*fx.database.lock().await,
                id,
            )
            .unwrap();
            assert_eq!(
                fx.archive.enqueue_captured(
                    fx.observe_ticket(100).unwrap(),
                    Origin::External,
                    id,
                    guard,
                ),
                tailsync_runtime::automatic_image_archive::EnqueueResult::Queued,
                "the unknown-origin seam must not already have enqueued this source"
            );
        }

        #[tokio::test]
        async fn parent_actual_sqlite_rejections_retry_both_saved_shapes() {
            let _serial = capture_serial().lock().await;
            for shape in [Shape::Portable, Shape::NativeOnly] {
                let fx = fixture(shape, OriginMode::External, false, true).await;
                let mut queue = install_capture_sender(&fx.pool).await;
                let mut database = fx.database.lock().await;
                assert!(database.is_storage_available());
                database.reject_composite_inserts_for_test(true).unwrap();
                drop(database);
                for _ in 0..3 {
                    assert_eq!(fx.tick(100).await, CompositeCapture::Handled);
                    assert!(fx.acks().is_empty());
                    assert_eq!(fx.history_len().await, 0);
                    fx.advance_clock_ms(2000);
                }
                assert_eq!(fx.tick(100).await, CompositeCapture::Handled);
                assert_eq!(fx.faults(), vec![(100, "capture_not_saved")]);
                assert!(fx.acks().is_empty());
                assert!(queue.priority.try_recv().is_err());
                fx.database
                    .lock()
                    .await
                    .reject_composite_inserts_for_test(false)
                    .unwrap();
                fx.settings.lock().await.sync_enabled = false;
                assert_eq!(fx.tick(100).await, CompositeCapture::Handled);
                assert_eq!(fx.history_len().await, 1);
                assert_eq!(fx.acks(), vec![100]);
            }
        }

        #[tokio::test]
        async fn parent_fault_ack_requires_a_real_durable_journal_write() {
            let _serial = capture_serial().lock().await;
            let mut fx = fixture(Shape::Failed, OriginMode::External, false, false).await;
            let blocked_root = fx.root.join("fault-root-is-a-file");
            std::fs::write(&blocked_root, b"not a directory").unwrap();
            let healthy_root = fx.root.join("durable-fault");
            let writable = Arc::new(AtomicBool::new(false));
            let flag = writable.clone();
            fx.env.capture_fault = Arc::new(move |revision, kind| {
                let root = if flag.load(Ordering::SeqCst) {
                    healthy_root.clone()
                } else {
                    blocked_root.clone()
                };
                Box::pin(async move {
                    matches!(
                        tokio::task::spawn_blocking(move || {
                            tailsync_core::peer::effect_journal::record_capture_fault(
                                &root, revision, kind,
                            )
                        })
                        .await,
                        Ok(Ok(()))
                    )
                })
            });
            for _ in 0..3 {
                fx.tick(100).await;
                fx.advance_clock_ms(2000);
            }
            fx.tick(100).await;
            assert!(
                fx.acks().is_empty(),
                "an actual journal error must not acknowledge"
            );
            writable.store(true, Ordering::SeqCst);
            fx.tick(100).await;
            assert_eq!(fx.acks(), vec![100]);
            assert!(fx.root.join("durable-fault").is_dir());
        }

        #[tokio::test]
        async fn parent_original_domain_suppresses_native_and_derived_self_writes() {
            use base64::Engine as _;
            use image::ImageEncoder as _;
            use tailsync_core::protocol::native_archive::{DerivedFormat, CONVERTER_STATIC_COVER};
            let _serial = capture_serial().lock().await;
            for color in [None, Some(0_u8), Some(255_u8)] {
                let mut fx = fixture(
                    if color.is_some() {
                        Shape::Portable
                    } else {
                        Shape::NativeOnly
                    },
                    OriginMode::External,
                    false,
                    true,
                )
                .await;
                let mut queue = install_capture_sender(&fx.pool).await;
                let mut archive = fx.fixture_archive.clone();
                if let Some(color) = color {
                    archive.original.formats.push(NativeFormat {
                        item: 0,
                        name: "public.gif".into(),
                        state: FormatState::Captured,
                        bytes: base64::engine::general_purpose::STANDARD
                            .decode("R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7")
                            .unwrap(),
                    });
                    let mut png = Vec::new();
                    image::codecs::png::PngEncoder::new(&mut png)
                        .write_image(&[color, 0, 0, 255], 1, 1, image::ExtendedColorType::Rgba8)
                        .unwrap();
                    archive.derived.push(DerivedFormat {
                        item: 0,
                        name: "public.png".into(),
                        source_name: "public.gif".into(),
                        converter: CONVERTER_STATIC_COVER.into(),
                        bytes: png,
                    });
                    assert!(tailsync_core::composite::from_archive(&archive).is_ok());
                    fx.env.classify =
                        Arc::new(|_, _| panic!("a derived capture must use its original domain"));
                }
                archive.validate().unwrap();
                let mut tracker = OriginTracker::default();
                tracker.record(
                    100,
                    origin_fingerprint_from_native(&archive.original).unwrap(),
                    SELF_TOKEN,
                );
                let tracker = StdMutex::new(tracker);
                fx.env.classify_native = Arc::new(move |snapshot| {
                    tracker.lock().unwrap().classify(
                        Some(snapshot.after),
                        origin_fingerprint_from_native(snapshot).unwrap(),
                        None,
                    )
                });
                fx.env.read_archive = Arc::new(move || Ok(archive.clone()));
                assert_eq!(fx.tick(100).await, CompositeCapture::Handled);
                assert_eq!(fx.acks(), vec![100]);
                assert_eq!(fx.history_len().await, 0);
                assert!(queue.priority.try_recv().is_err());
            }
        }

        #[tokio::test]
        async fn parent_cancelled_provider_releases_the_exact_pipeline_lease() {
            let _serial = capture_serial().lock().await;
            let fx = fixture(Shape::Portable, OriginMode::External, false, false).await;
            fx.gate.close();
            let _release = fx.gate.open_on_drop();
            let mut task = Box::pin(fx.tick(100));
            tokio::select! {
                _ = wait_for_signal(&fx.provider_entered, "the blocked provider") => {}
                _ = &mut task => panic!("provider was not blocked"),
            }
            let lease = fx.env.pipeline_lease.lock().unwrap().clone().unwrap();
            assert!(
                lease.upgrade().is_some(),
                "running provider must hold its reservation"
            );
            drop(task);
            assert!(
                lease.upgrade().is_some(),
                "cancellation cannot free a still-running provider's memory"
            );
            fx.gate.open();
            tokio::time::timeout(TEST_WAIT, async {
                while lease.upgrade().is_some() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("detached provider retained the reservation");
            assert_eq!(fx.history_len().await, 0);
            assert!(fx.acks().is_empty());
        }

        #[tokio::test]
        async fn parent_late_read_and_conversion_errors_do_not_charge_the_new_capture() {
            let _serial = capture_serial().lock().await;
            for (read_error, recovery) in [(true, false), (false, false), (false, true)] {
                let mut fx = fixture(Shape::Portable, OriginMode::External, false, false).await;
                fx.settings.lock().await.sync_enabled = false;
                let gate = fx.gate.clone();
                let entered = fx.provider_entered.clone();
                fx.env.read_archive = Arc::new(move || {
                    entered.notify_one();
                    gate.wait();
                    if read_error {
                        Err("injected native read failure".into())
                    } else {
                        Ok(fixture_archive(Shape::Failed, 100))
                    }
                });
                fx.gate.close();
                let _release = fx.gate.open_on_drop();
                let mut task = Box::pin(fx.tick(100));
                tokio::select! {
                    _ = wait_for_signal(&fx.provider_entered, "the old failing provider") => {}
                    _ = &mut task => panic!("provider was not blocked"),
                }
                let current = if recovery { 100 } else { 101 };
                fx.set_revision(current);
                if recovery {
                    crate::clipboard::request_wake_recovery();
                }
                fx.gate.open();
                assert_eq!(task.await, CompositeCapture::Handled);
                assert!(fx.acks().is_empty());
                assert!(fx.faults().is_empty());
                assert!(!fx.retry.lock().unwrap().exhausted());
                fx.env.read_archive =
                    Arc::new(move || Ok(fixture_archive(Shape::Portable, current)));
                assert_eq!(fx.tick(current).await, CompositeCapture::Handled);
                assert_eq!(fx.history_len().await, 1);
                assert_eq!(fx.acks(), vec![current]);
            }
        }

        #[tokio::test]
        async fn parent_storage_wait_keeps_generation_and_recovery_gates() {
            let _serial = capture_serial().lock().await;
            for supersede in [Supersede::Generation, Supersede::Recovery] {
                let fx = fixture(Shape::Portable, OriginMode::External, false, true).await;
                let mut queue = install_capture_sender(&fx.pool).await;
                let hold = fx.database.lock().await;
                let mut task = Box::pin(fx.tick(100));
                tokio::select! {
                    _ = wait_for_signal(&fx.classify_notify, "the storage attempt") => {}
                    _ = &mut task => panic!("capture finished while storage was held"),
                }
                std::future::poll_fn(|cx| {
                    assert!(task.as_mut().poll(cx).is_pending());
                    std::task::Poll::Ready(())
                })
                .await;
                match supersede {
                    Supersede::Generation => {
                        fx.engine.lock().await.supersede_file_clipboard();
                    }
                    Supersede::Recovery => crate::clipboard::request_wake_recovery(),
                    _ => unreachable!(),
                }
                drop(hold);
                assert_eq!(task.await, CompositeCapture::Handled);
                assert_eq!(fx.history_len().await, 1);
                assert_eq!(fx.acks(), vec![100]);
                assert!(queue.priority.try_recv().is_err());
            }
        }

        #[tokio::test]
        async fn parent_failed_origin_fingerprint_never_fabricates_a_match() {
            let _serial = capture_serial().lock().await;
            let _ = drain_warnings();
            let mut fx = fixture(Shape::NativeOnly, OriginMode::External, true, false).await;
            let mut native = fx.fixture_archive.original.clone();
            native.formats.push(NativeFormat {
                item: 0,
                name: "org.fixture.unreadable".into(),
                state: FormatState::Failed,
                bytes: vec![],
            });
            native.validate().unwrap();
            assert!(origin_fingerprint_from_native(&native).is_err());
            let mut archive = fx.fixture_archive.clone();
            archive.original = native;
            fx.env.read_archive = Arc::new(move || Ok(archive.clone()));
            assert_eq!(fx.tick(100).await, CompositeCapture::Handled);
            assert_eq!(fx.history_len().await, 1);
            assert_eq!(fx.acks(), vec![100]);
            assert!(
                drain_warnings().contains(&"capture_origin_unknown"),
                "failed fingerprint must keep the production UnknownMismatch outcome"
            );
        }

        #[tokio::test]
        async fn automatic_archive_policy_keeps_unknown_origins_local() {
            let _serial = capture_serial().lock().await;
            use tailsync_runtime::automatic_image_archive::EnqueueResult;
            let fx = fixture(Shape::Html, OriginMode::External, true, false).await;
            let _ = fx.tick(100).await;
            assert_eq!(fx.history_len().await, 1);
            let id = fx.history_id().await;

            let ticket = fx
                .observe_ticket(100)
                .expect("an enabled archive issues a ticket");
            let guard = {
                let db = fx.database.lock().await;
                tailsync_runtime::history::ArchiveEntryGuard::capture(&db, id)
            }
            .expect("the saved source binds a real archive guard");
            assert_eq!(
                fx.archive
                    .enqueue_captured(ticket, Origin::External, id, guard.clone()),
                EnqueueResult::Duplicate,
                "the capture seam already enqueued this exact saved source"
            );
            assert_eq!(
                fx.archive
                    .enqueue_captured(ticket, Origin::Unknown, id, guard.clone()),
                EnqueueResult::NotLocal,
                "unknown origins stay out of the automatic archive"
            );
            assert_eq!(
                fx.archive
                    .enqueue_captured(ticket, Origin::UnknownMismatch, id, guard),
                EnqueueResult::NotLocal
            );
        }
    }
}
