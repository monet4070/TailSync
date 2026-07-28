use log::{debug, error, info, warn};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};

use crate::clipboard_change::ClipboardChangeDetector;
use crate::clipboard_file;
use crate::crypto;
use crate::db;
use crate::network;
use crate::protocol::{Command, FileChunkPayload, TransferId, FILE_CHUNK_SIZE};
use crate::sync;

static CLIPBOARD_RECOVERY_GENERATION: AtomicU64 = AtomicU64::new(0);
static CLIPBOARD_MONITOR_LAST_TICK_MS: AtomicU64 = AtomicU64::new(0);
const CLIPBOARD_MONITOR_STALE_AFTER_MS: u64 = 10_000;

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
) {
    tauri::async_runtime::spawn(async move {
        let warm_pool = pool.clone();
        let warm_settings = settings.clone();
        tauri::async_runtime::spawn(async move {
            let peers = configured_peers(&warm_settings).await;
            network::prewarm_connections(warm_pool, peers).await;
        });
        clipboard_loop(handle, database, sync_engine, pool, settings).await;
    });
}

async fn clipboard_loop(
    handle: AppHandle,
    database: Arc<Mutex<db::HistoryDB>>,
    sync_engine: Arc<Mutex<sync::SyncEngine>>,
    pool: Arc<Mutex<network::ConnectionPool>>,
    settings: Arc<Mutex<crypto::Settings>>,
) {
    let mut change_detector = ClipboardChangeDetector::new();
    let poll_interval = change_detector.poll_interval_ms();
    info!("Clipboard monitor started ({poll_interval} ms change polling)");

    let mut last_text_hash = String::new();
    let mut last_image_hash = String::new();
    let mut last_file_list: Vec<std::path::PathBuf> = vec![];
    let mut tick: u64 = 0;
    let mut recovery_generation = CLIPBOARD_RECOVERY_GENERATION.load(Ordering::Acquire);

    loop {
        sleep(Duration::from_millis(poll_interval)).await;
        tick += 1;
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

        // Panic guard — catch any crash so the loop stays alive
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // Just return the clipboard state; we handle it outside
        }));
        if result.is_err() {
            error!("Clipboard monitor panic caught, continuing");
            continue;
        }

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
            if !outbound_paths.is_empty()
                && should_process_clipboard_item(paths != &last_file_list, clipboard_changed)
            {
                last_file_list.clone_from(paths);
                last_text_hash.clear();
                last_image_hash.clear();
                if managed_count > 0 {
                    info!("Ignored {managed_count} managed file(s) in a mixed clipboard event");
                }
                info!("Clipboard files: {} file(s)", outbound_paths.len());
                for path in outbound_paths {
                    tokio::spawn(send_file_to_peers(
                        path,
                        pool.clone(),
                        database.clone(),
                        sync_engine.clone(),
                        settings.clone(),
                    ));
                }
                // Only skip text/image when files actually changed this round
                continue;
            }
            // Files on clipboard but unchanged — fall through to check
            // text/image in case user copied text over the file selection
        }

        // ── 2. Try text ────────────────────────────────────────────
        match clipboard.read_text() {
            Ok(t) if !t.is_empty() => {
                let hash = blake3::hash(t.as_bytes()).to_hex().to_string();
                if should_process_clipboard_item(hash != last_text_hash, clipboard_changed) {
                    last_text_hash = hash.clone();

                    let is_echo = shadow_check(&sync_engine, &hash).await;
                    if is_echo {
                        info!("Text shadow-filtered, skipping: {} chars", t.len());
                        continue;
                    }

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
        match clipboard.read_image() {
            Ok(image) => {
                let rgba = image.rgba();
                let w = image.width();
                let h = image.height();
                let packed = pack_image_data(w, h, rgba);
                let hash = blake3::hash(&packed).to_hex().to_string();
                if !should_process_clipboard_item(hash != last_image_hash, clipboard_changed) {
                    continue;
                }
                last_image_hash = hash.clone();

                let is_echo = image_shadow_check(&sync_engine, &hash).await;
                if is_echo {
                    continue;
                }

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

// ── Pack / unpack helpers ────────────────────────────────────────────

/// Send one file to all enabled peers.
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
    let peers = configured_peers(&settings).await;
    let transfer_id = TransferId::random();
    let meta = sync::FileMeta {
        transfer_id: Some(transfer_id),
        name: fname.clone(),
        size: total,
        hash: hash.clone(),
        chunk_size: FILE_CHUNK_SIZE as u32,
    };
    let meta_json = serde_json::to_vec(&meta).unwrap_or_default();
    crate::api::set_file_progress(&fname, 0, total);

    let mut sent_to: usize = 0;
    for peer in &peers {
        if !peer.enabled {
            continue;
        }
        let addr: std::net::SocketAddr = match network::peer_socket_addr(peer) {
            Ok(a) => a,
            Err(e) => {
                warn!("Bad peer address for {}: {}", peer.hostname, e);
                continue;
            }
        };
        info!("Sending file to peer {} at {}", peer.hostname, addr);
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
                warn!("FileMeta to {} failed: {}", addr, error);
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
                    warn!("File chunk at {} to {} failed: {}", confirmed, addr, error);
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

fn should_process_clipboard_item(content_changed: bool, clipboard_changed: bool) -> bool {
    content_changed || clipboard_changed
}

// ── Shadow-filter helpers ────────────────────────────────────────────

async fn shadow_check(sync_engine: &Arc<Mutex<sync::SyncEngine>>, hash: &str) -> bool {
    let mut sync = sync_engine.lock().await;
    let key = hash.to_string();
    if sync.shadow_filter.contains(&key) {
        debug!("Text shadow-filter hit: {}", &hash[..8]);
        sync.shadow_filter.retain(|h| h != &key);
        true
    } else {
        false
    }
}

async fn image_shadow_check(sync_engine: &Arc<Mutex<sync::SyncEngine>>, hash: &str) -> bool {
    let mut sync = sync_engine.lock().await;
    let key = hash.to_string();
    if sync.image_shadow_filter.contains(&key) {
        debug!("Image shadow-filter hit: {}", &hash[..8]);
        sync.image_shadow_filter.retain(|h| h != &key);
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
    let peers = configured_peers(settings).await;

    for peer in &peers {
        if !peer.enabled {
            continue;
        }
        if let Ok(addr) = network::peer_socket_addr(peer) {
            let pool = pool.clone();
            let payload = payload.clone();
            let peer = peer.clone();
            tokio::spawn(async move {
                if let Err(e) = network::queue_peer_frame(&pool, &peer, cmd, payload).await {
                    debug!("Broadcast to {} failed: {}", addr, e);
                }
            });
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
mod tests {
    use super::{files_to_broadcast, should_process_clipboard_item};

    #[test]
    fn native_copy_event_processes_identical_content_again() {
        assert!(should_process_clipboard_item(false, true));
        assert!(!should_process_clipboard_item(false, false));
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
