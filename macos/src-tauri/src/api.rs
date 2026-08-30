//! JSON-line local API server for the SwiftUI frontend.
//!
//! Protocol: one JSON object per line, terminated by `\n`.
//! Request:  `{"cmd": "...", ...params}`
//! Response: `{"ok": true, ...data}` or `{"ok": false, "error": "..."}`
mod imports;
mod routes;
mod transport;

use imports::{
    append_import_chunk, begin_import, finish_import, import_response, import_size_limit,
};
use routes::handle_cmd;
pub(crate) use routes::{history_capabilities_data, peer_snapshot_data};
pub(crate) use tailsync_core::import::ImportRegistry;
pub use transport::start;
#[cfg(test)]
use transport::{bind_api_listener, read_request_with_limits};

use crate::db;
use crate::identity;
use crate::network;
use crate::sync;
use crate::{crypto, identity::DeviceIdentity};
use log::{info, warn};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{watch, Mutex, Semaphore};
use tokio::time::timeout;

use std::collections::{HashSet, VecDeque};
use std::sync::{LazyLock, Mutex as StdMutex};

static RUNTIME_REVISION: LazyLock<watch::Sender<u64>> = LazyLock::new(|| watch::channel(1).0);
const MAX_RUNTIME_NOTIFICATIONS: usize = 32;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RuntimeNotification {
    pub id: u64,
    pub level: String,
    pub message: String,
}

#[derive(Default)]
struct RuntimeNotificationBuffer {
    next_id: u64,
    entries: VecDeque<RuntimeNotification>,
}

impl RuntimeNotificationBuffer {
    fn push(&mut self, level: &str, message: String) -> u64 {
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let id = self.next_id;
        self.entries.push_back(RuntimeNotification {
            id,
            level: level.to_string(),
            message,
        });
        while self.entries.len() > MAX_RUNTIME_NOTIFICATIONS {
            self.entries.pop_front();
        }
        id
    }

    fn since(&self, id: u64) -> Vec<RuntimeNotification> {
        self.entries
            .iter()
            .filter(|entry| entry.id > id)
            .cloned()
            .collect()
    }
}

static RUNTIME_NOTIFICATIONS: LazyLock<StdMutex<RuntimeNotificationBuffer>> =
    LazyLock::new(|| StdMutex::new(RuntimeNotificationBuffer::default()));

pub fn get_runtime_revision() -> u64 {
    *RUNTIME_REVISION.borrow()
}

pub fn bump_runtime_revision() {
    RUNTIME_REVISION.send_modify(|revision| {
        *revision = revision.wrapping_add(1).max(1);
    });
}

pub async fn wait_for_runtime_revision(since: u64, wait: Duration) -> u64 {
    let mut receiver = RUNTIME_REVISION.subscribe();
    if *receiver.borrow() != since {
        return *receiver.borrow();
    }
    let _ = timeout(wait, receiver.changed()).await;
    let revision = *receiver.borrow();
    revision
}

pub fn push_runtime_notification(level: &str, message: impl Into<String>) {
    if let Ok(mut notifications) = RUNTIME_NOTIFICATIONS.lock() {
        notifications.push(level, message.into());
        drop(notifications);
        bump_runtime_revision();
    }
}

pub fn get_runtime_notifications_since(id: u64) -> Vec<RuntimeNotification> {
    RUNTIME_NOTIFICATIONS
        .lock()
        .map(|notifications| notifications.since(id))
        .unwrap_or_default()
}

/// Monotonic version — bumped on every clipboard change.
pub static CLIPBOARD_VERSION: AtomicU64 = AtomicU64::new(0);
pub fn bump_clipboard_version() {
    CLIPBOARD_VERSION.fetch_add(1, Ordering::Release);
    bump_runtime_revision();
}
pub fn get_clipboard_version() -> u64 {
    CLIPBOARD_VERSION.load(Ordering::Acquire)
}

// File transfer progress
static FILE_PROGRESS: LazyLock<StdMutex<HashMap<String, TrackedFileProgress>>> =
    LazyLock::new(|| StdMutex::new(HashMap::new()));
static CANCELLED_FILE_BATCHES: LazyLock<StdMutex<HashSet<String>>> =
    LazyLock::new(|| StdMutex::new(HashSet::new()));
#[derive(Clone, Serialize)]
pub struct FileProgress {
    pub batch_id: String,
    pub name: String,
    pub sent: u64,
    pub total: u64,
    pub active: bool,
    pub direction: String,
    pub device: String,
    pub completed_files: usize,
    pub total_files: usize,
    pub speed_bytes_per_second: u64,
    pub status: String,
    pub can_stop: bool,
}

struct TrackedFileProgress {
    progress: FileProgress,
    samples: VecDeque<(Instant, u64)>,
    updated_at: Instant,
}

fn progress_key(progress: &FileProgress) -> String {
    format!(
        "{}\u{1f}{}\u{1f}{}",
        progress.batch_id, progress.direction, progress.device
    )
}

pub fn get_file_progress() -> Option<FileProgress> {
    FILE_PROGRESS.lock().ok().and_then(|progress| {
        progress
            .values()
            .filter(|tracked| tracked.progress.active)
            .max_by_key(|tracked| tracked.updated_at)
            .map(|tracked| tracked.progress.clone())
    })
}

pub fn has_active_file_progress() -> bool {
    FILE_PROGRESS
        .lock()
        .is_ok_and(|progress| progress.values().any(|tracked| tracked.progress.active))
}

pub fn set_file_progress(name: &str, sent: u64, total: u64) {
    set_file_batch_progress(FileProgress {
        batch_id: String::new(),
        name: name.into(),
        sent,
        total,
        active: sent < total,
        direction: "receiving".into(),
        device: String::new(),
        completed_files: usize::from(sent == total),
        total_files: 1,
        speed_bytes_per_second: 0,
        status: "transferring".into(),
        can_stop: true,
    });
}
pub fn set_file_batch_progress(mut progress: FileProgress) {
    let now = Instant::now();
    if let Ok(mut state) = FILE_PROGRESS.lock() {
        let key = progress_key(&progress);
        let tracked = state.entry(key).or_insert_with(|| TrackedFileProgress {
            progress: progress.clone(),
            samples: VecDeque::new(),
            updated_at: now,
        });
        tracked.samples.push_back((now, progress.sent));
        while tracked
            .samples
            .front()
            .is_some_and(|(at, _)| now.duration_since(*at).as_secs_f64() > 5.0)
        {
            tracked.samples.pop_front();
        }
        if let (Some((first_at, first_bytes)), Some((last_at, last_bytes))) =
            (tracked.samples.front(), tracked.samples.back())
        {
            let seconds = last_at.duration_since(*first_at).as_secs_f64();
            if seconds > 0.05 {
                progress.speed_bytes_per_second =
                    (last_bytes.saturating_sub(*first_bytes) as f64 / seconds) as u64;
            }
        }
        tracked.progress = progress;
        tracked.updated_at = now;
        drop(state);
        bump_runtime_revision();
    }
}
pub fn clear_file_progress() {
    if let Ok(mut progress) = FILE_PROGRESS.lock() {
        let changed = !progress.is_empty();
        progress.clear();
        drop(progress);
        if changed {
            bump_runtime_revision();
        }
    }
}

pub fn clear_file_progress_scope(batch_id: Option<&str>, device: Option<&str>) {
    if let Ok(mut progress) = FILE_PROGRESS.lock() {
        let previous_count = progress.len();
        progress.retain(|_, tracked| {
            let batch_matches =
                batch_id.is_none_or(|batch_id| tracked.progress.batch_id == batch_id);
            let device_matches = device.is_none_or(|device| tracked.progress.device == device);
            !(batch_matches && device_matches)
        });
        let changed = progress.len() != previous_count;
        drop(progress);
        if changed {
            bump_runtime_revision();
        }
    }
}

pub fn request_file_batch_cancel(batch_id: &str) {
    if let Ok(mut cancelled) = CANCELLED_FILE_BATCHES.lock() {
        cancelled.insert(batch_id.to_string());
    }
}

pub fn is_file_batch_cancelled(batch_id: &str) -> bool {
    CANCELLED_FILE_BATCHES
        .lock()
        .is_ok_and(|cancelled| cancelled.contains(batch_id))
}

pub fn clear_file_batch_cancel(batch_id: &str) {
    if let Ok(mut cancelled) = CANCELLED_FILE_BATCHES.lock() {
        cancelled.remove(batch_id);
    }
}

/// Write file content to a temp file and put its URL on the macOS clipboard.
#[cfg(target_os = "macos")]
pub fn restore_file_to_clipboard(data: &[u8], fname: &str) -> Result<(), String> {
    let path = crate::db::materialize_clipboard_bytes(data, fname)
        .map_err(|error| format!("Could not prepare legacy file for clipboard: {error}"))?;
    write_file_path_to_clipboard(&path)
}

pub fn restore_file_path_to_clipboard(file_path: &Path, file_name: &str) -> Result<(), String> {
    let clipboard_path = crate::db::materialize_clipboard_file(file_path, file_name)
        .map_err(|error| format!("Could not prepare file for clipboard: {error}"))?;
    write_file_path_to_clipboard(&clipboard_path)
}

#[cfg(target_os = "macos")]
fn write_file_path_to_clipboard(file_path: &Path) -> Result<(), String> {
    crate::clipboard_file::write_clipboard_files(&[file_path.to_path_buf()])
        .map_err(|error| format!("Could not restore file to clipboard: {error}"))?;
    log::info!("Restored file to clipboard: {}", file_path.display());
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn restore_file_to_clipboard(data: &[u8], fname: &str) -> Result<(), String> {
    let path = crate::db::materialize_clipboard_bytes(data, fname)
        .map_err(|error| format!("Could not prepare legacy file for clipboard: {error}"))?;
    write_file_path_to_clipboard(&path)
}

#[cfg(target_os = "windows")]
fn write_file_path_to_clipboard(file_path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::GlobalFree;
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GHND};

    let wide_path: Vec<u16> = file_path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // DROPFILES header (20 bytes) + wide path (including double-null terminator)
    let df_size = std::mem::size_of::<DropFilesHeader>() as u32;
    let total_size = df_size as usize + (wide_path.len() + 1) * 2; // +1 for final null
    let path_offset = df_size;

    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return Err("Could not open the Windows clipboard".to_string());
        }
        if EmptyClipboard() == 0 {
            CloseClipboard();
            return Err("Could not clear the Windows clipboard".to_string());
        }
        let h = GlobalAlloc(GHND, total_size);
        if h.is_null() {
            CloseClipboard();
            return Err("Could not allocate Windows clipboard memory".to_string());
        }
        let ptr = GlobalLock(h) as *mut u8;
        if ptr.is_null() {
            GlobalFree(h);
            CloseClipboard();
            return Err("Could not lock Windows clipboard memory".to_string());
        }
        let header = DropFilesHeader {
            p_files: path_offset,
            pt: [0, 0],
            f_nc: 0,
            f_wide: 1,
        };
        std::ptr::copy_nonoverlapping(
            &header as *const _ as *const u8,
            ptr,
            std::mem::size_of::<DropFilesHeader>(),
        );
        let path_ptr = ptr.add(path_offset as usize) as *mut u16;
        std::ptr::copy_nonoverlapping(wide_path.as_ptr(), path_ptr, wide_path.len());
        path_ptr.add(wide_path.len()).write(0);
        GlobalUnlock(h);
        if SetClipboardData(15, h).is_null() {
            GlobalFree(h);
            CloseClipboard();
            return Err("Could not publish the file to the Windows clipboard".to_string());
        }
        CloseClipboard();
    }
    log::info!(
        "Restored file to Windows clipboard: {}",
        file_path.display()
    );
    Ok(())
}

#[repr(C)]
#[cfg(target_os = "windows")]
struct DropFilesHeader {
    p_files: u32,
    pt: [i32; 2],
    f_nc: i32,
    f_wide: i32,
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn restore_file_to_clipboard(_data: &[u8], _fname: &str) -> Result<(), String> {
    Err("File clipboard restore is unavailable on this platform".to_string())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn write_file_path_to_clipboard(_file_path: &Path) -> Result<(), String> {
    Err("File clipboard restore is unavailable on this platform".to_string())
}

/// Longest-edge size (in pixels) for history thumbnails.
///
/// 160px keeps a thumbnail recognizable — a long screenshot or wide banner
/// stays legible instead of collapsing into a blurry smudge — while remaining
/// tiny to transfer and cache (~100 KB of RGBA at most, ~140 KB base64).
pub const THUMBNAIL_MAX_SIDE: usize = 160;

/// Box-averaging downscale of a validated packed RGBA image, preserving the
/// aspect ratio (the longest edge is clamped to `max_side`).
///
/// Each destination pixel is the average of the source rectangle it covers,
/// which avoids the aliasing and blur of nearest-neighbor point sampling. The
/// average is alpha-weighted (premultiplied) so fully or partially transparent
/// regions do not bleed their arbitrary RGB toward the opaque neighbors.
pub fn thumbnail_rgba(
    image: crate::protocol::PackedImage<'_>,
    max_side: usize,
) -> (usize, usize, Vec<u8>) {
    let w = image.width as usize;
    let h = image.height as usize;
    let longest = w.max(h);
    let max_side = max_side.max(1);
    let (tw, th) = if longest <= max_side {
        (w, h)
    } else {
        (
            (w * max_side / longest).max(1),
            (h * max_side / longest).max(1),
        )
    };

    // No downscale needed — hand back the pixels unchanged.
    if tw == w && th == h {
        return (w, h, image.rgba.to_vec());
    }

    let src = image.rgba;
    let mut out = vec![0u8; tw * th * 4];
    for ty in 0..th {
        // Source rows [sy0, sy1) that map to this destination row.
        let sy0 = ty * h / th;
        let sy1 = (((ty + 1) * h) / th).max(sy0 + 1).min(h);
        for tx in 0..tw {
            // Source columns [sx0, sx1) that map to this destination column.
            let sx0 = tx * w / tw;
            let sx1 = (((tx + 1) * w) / tw).max(sx0 + 1).min(w);

            let mut sum_r: u64 = 0;
            let mut sum_g: u64 = 0;
            let mut sum_b: u64 = 0;
            let mut sum_a: u64 = 0;
            let mut count: u64 = 0;
            for sy in sy0..sy1 {
                let row = sy * w;
                for sx in sx0..sx1 {
                    let si = (row + sx) * 4;
                    let a = src[si + 3] as u64;
                    sum_r += src[si] as u64 * a;
                    sum_g += src[si + 1] as u64 * a;
                    sum_b += src[si + 2] as u64 * a;
                    sum_a += a;
                    count += 1;
                }
            }

            let di = (ty * tw + tx) * 4;
            if sum_a > 0 {
                // Un-premultiply: divide the weighted color sum by total alpha.
                out[di] = (sum_r / sum_a) as u8;
                out[di + 1] = (sum_g / sum_a) as u8;
                out[di + 2] = (sum_b / sum_a) as u8;
            }
            // A fully transparent region leaves RGB at 0; only alpha matters.
            out[di + 3] = (sum_a / count) as u8;
        }
    }
    (tw, th, out)
}

#[cfg(not(target_os = "macos"))]
pub const API_PORT: u16 = 19889;
const MAX_API_LINE: usize = 1024 * 1024;
const API_READ_TIMEOUT: Duration = Duration::from_secs(5);
const API_WRITE_TIMEOUT: Duration = Duration::from_secs(5);
const API_BIND_RETRY_DELAY: Duration = Duration::from_millis(250);
const API_MAX_CONNECTIONS: usize = 16;

#[derive(Clone)]
pub struct ApiToken([u8; 32]);

impl ApiToken {
    fn parse(value: &str) -> Result<Self, String> {
        let decoded = hex::decode(value.trim())
            .map_err(|_| "TAILSYNC_API_TOKEN must be 64 hexadecimal characters".to_string())?;
        let bytes: [u8; 32] = decoded
            .try_into()
            .map_err(|_| "TAILSYNC_API_TOKEN must encode exactly 32 bytes".to_string())?;
        Ok(Self(bytes))
    }

    fn matches(&self, value: Option<&str>) -> bool {
        let Some(value) = value else {
            return false;
        };
        let Ok(candidate) = Self::parse(value) else {
            return false;
        };
        let difference = self
            .0
            .iter()
            .zip(candidate.0.iter())
            .fold(0_u8, |difference, (expected, actual)| {
                difference | (expected ^ actual)
            });
        difference == 0
    }
}

pub fn load_api_token() -> Result<ApiToken, String> {
    if std::env::var("TAILSYNC_API_TOKEN_STDIN").as_deref() == Ok("1") {
        use std::io::Read;

        let mut value = String::new();
        std::io::stdin()
            .take(130)
            .read_to_string(&mut value)
            .map_err(|error| format!("could not read local API token from stdin: {error}"))?;
        if value.len() > 65 {
            return Err("local API token from stdin exceeds 64 hexadecimal characters".to_string());
        }
        return ApiToken::parse(&value);
    }

    if let Ok(value) = std::env::var("TAILSYNC_API_TOKEN") {
        return ApiToken::parse(&value);
    }

    let mut bytes = [0_u8; 32];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| "could not generate local API token".to_string())?;
    Ok(ApiToken(bytes))
}

pub struct ApiState {
    pub db: Arc<Mutex<db::HistoryDB>>,
    pub sync_engine: Arc<Mutex<sync::SyncEngine>>,
    pub settings: Arc<Mutex<crypto::Settings>>,
    pub identity: Arc<DeviceIdentity>,
    pub pool: Arc<Mutex<network::ConnectionPool>>,
    pub pairing: Arc<crate::pairing::PairingManager>,
    pub token: ApiToken,
    pub shutdown: watch::Sender<bool>,
    pub pending_storage_cleanup: Arc<Mutex<Option<std::path::PathBuf>>>,
    pub(crate) imports: Mutex<ImportRegistry>,
}

#[derive(Debug, Deserialize)]
struct Request {
    cmd: String,
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    keyword: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    start_time: Option<String>,
    #[serde(default)]
    end_time: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    id: Option<i64>,
    #[serde(default)]
    settings: Option<Value>,
    // migrate_entry fields
    #[serde(default)]
    time: Option<String>,
    #[serde(rename = "type", default)]
    entry_type: Option<String>,
    #[serde(default)]
    desc: Option<String>,
    #[serde(default)]
    data_b64: Option<String>,
    #[serde(default)]
    import_id: Option<String>,
    #[serde(default)]
    total_size: Option<u64>,
    #[serde(default)]
    data_hash: Option<String>,
    #[serde(default)]
    import_offset: Option<u64>,
    #[serde(default)]
    chunk_b64: Option<String>,
    #[serde(default)]
    hostname: Option<String>,
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    shortcut: Option<String>,
    #[serde(default)]
    public_key: Option<String>,
    #[serde(default)]
    address: Option<String>,
    #[serde(default)]
    batch_id: Option<String>,
    #[serde(default)]
    parent: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    theme_id: Option<String>,
    #[serde(default)]
    asset_slot: Option<String>,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    high_contrast: Option<bool>,
    #[serde(default)]
    expected_digest: Option<String>,
    #[serde(default)]
    storage_handle: Option<String>,
    #[serde(default)]
    options: Option<Value>,
    #[serde(default)]
    pinned: Option<bool>,
    #[serde(default)]
    favorite: Option<bool>,
    #[serde(default)]
    collection: Option<String>,
    #[serde(default)]
    since_revision: Option<u64>,
    #[serde(default)]
    wait_ms: Option<u64>,
    #[serde(default)]
    since_notification_id: Option<u64>,
}

#[derive(Debug, Serialize)]
struct Response {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[cfg(test)]
mod tests;
