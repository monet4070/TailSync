//! JSON-line TCP API server for the SwiftUI frontend.
//!
//! Protocol: one JSON object per line, terminated by `\n`.
//! Request:  `{"cmd": "...", ...params}`
//! Response: `{"ok": true, ...data}` or `{"ok": false, "error": "..."}`
mod imports;
mod routes;
mod transport;

pub(crate) use imports::ImportRegistry;
use imports::{
    append_import_chunk, begin_import, finish_import, import_response, import_size_limit,
};
use routes::handle_cmd;
pub(crate) use routes::{history_capabilities_data, peer_snapshot_data};
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
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{watch, Mutex, Semaphore};
use tokio::time::timeout;

/// Monotonic version — bumped on every clipboard change.
pub static CLIPBOARD_VERSION: AtomicU64 = AtomicU64::new(0);
pub fn bump_clipboard_version() {
    CLIPBOARD_VERSION.fetch_add(1, Ordering::Release);
}
pub fn get_clipboard_version() -> u64 {
    CLIPBOARD_VERSION.load(Ordering::Acquire)
}

// File transfer progress
use std::collections::{HashSet, VecDeque};
use std::sync::{LazyLock, Mutex as StdMutex};
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
    }
}
pub fn clear_file_progress() {
    if let Ok(mut progress) = FILE_PROGRESS.lock() {
        progress.clear();
    }
}

pub fn clear_file_progress_scope(batch_id: Option<&str>, device: Option<&str>) {
    if let Ok(mut progress) = FILE_PROGRESS.lock() {
        progress.retain(|_, tracked| {
            let batch_matches =
                batch_id.is_none_or(|batch_id| tracked.progress.batch_id == batch_id);
            let device_matches = device.is_none_or(|device| tracked.progress.device == device);
            !(batch_matches && device_matches)
        });
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

/// Nearest-neighbor downscale of a validated packed RGBA image.
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
    let mut out = vec![0u8; tw * th * 4];
    for y in 0..th {
        for x in 0..tw {
            let sx = x * w / tw;
            let sy = y * h / th;
            let si = (sy * w + sx) * 4;
            let di = (y * tw + x) * 4;
            out[di..di + 4].copy_from_slice(&image.rgba[si..si + 4]);
        }
    }
    (tw, th, out)
}

pub const API_PORT: u16 = 19889;
const MAX_API_LINE: usize = 1024 * 1024;
const API_READ_TIMEOUT: Duration = Duration::from_secs(5);
const API_WRITE_TIMEOUT: Duration = Duration::from_secs(5);
const API_BIND_RETRY_DELAY: Duration = Duration::from_millis(250);
const API_MAX_CONNECTIONS: usize = 16;
const IMPORT_CHUNK_MAX_BYTES: usize = 512 * 1024;
const API_MAX_IMPORTS: usize = 4;
const IMPORT_SESSION_TTL: Duration = Duration::from_secs(10 * 60);
const MAX_IMPORT_FILE_SIZE: u64 = 1024 * 1024 * 1024;

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
    pinned: Option<bool>,
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
mod tests {
    use super::{
        bind_api_listener, clear_file_progress, clear_file_progress_scope, get_file_progress,
        history_capabilities_data, peer_snapshot_data, read_request_with_limits,
        set_file_batch_progress, ApiToken, FileProgress, Request,
    };
    use crate::crypto::Settings;
    use crate::identity::DeviceIdentity;
    use crate::network::tailscale::{LocalInfo, PeerInfo};
    use crate::network::{register_active_session, ConnectionInterface, PeerCandidate};
    use std::collections::HashMap;
    use std::time::Duration;

    fn progress(batch_id: &str, device: &str, sent: u64) -> FileProgress {
        FileProgress {
            batch_id: batch_id.into(),
            name: "file.bin".into(),
            sent,
            total: 100,
            active: true,
            direction: "receiving".into(),
            device: device.into(),
            completed_files: 0,
            total_files: 1,
            speed_bytes_per_second: 0,
            status: "transferring".into(),
            can_stop: true,
        }
    }

    #[test]
    fn progress_scope_keeps_other_concurrent_devices_visible() {
        clear_file_progress();
        set_file_batch_progress(progress("batch-a", "peer-a", 25));
        set_file_batch_progress(progress("batch-b", "peer-b", 50));
        clear_file_progress_scope(Some("batch-b"), Some("peer-b"));
        let remaining = get_file_progress().unwrap();
        assert_eq!(remaining.batch_id, "batch-a");
        assert_eq!(remaining.device, "peer-a");
        clear_file_progress();
    }

    #[tokio::test]
    async fn api_listener_recovers_after_the_address_is_released() {
        let blocker = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = blocker.local_addr().unwrap();
        let (_shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
        let waiter = tokio::spawn(async move {
            bind_api_listener(address, &mut shutdown_rx, Duration::from_millis(10)).await
        });

        tokio::time::sleep(Duration::from_millis(30)).await;
        drop(blocker);

        let listener = tokio::time::timeout(Duration::from_secs(2), waiter)
            .await
            .expect("API listener did not retry after the address was released")
            .expect("API listener retry task failed")
            .expect("API listener stopped without a shutdown request");
        assert_eq!(listener.local_addr().unwrap(), address);
    }

    #[tokio::test]
    async fn api_listener_bind_retry_stops_for_shutdown() {
        let blocker = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = blocker.local_addr().unwrap();
        let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
        let waiter = tokio::spawn(async move {
            bind_api_listener(address, &mut shutdown_rx, Duration::from_secs(30)).await
        });

        tokio::time::sleep(Duration::from_millis(30)).await;
        shutdown_tx.send(true).unwrap();

        let listener = tokio::time::timeout(Duration::from_secs(1), waiter)
            .await
            .expect("API listener bind retry ignored shutdown")
            .expect("API listener retry task failed");
        assert!(listener.is_none());
    }

    #[test]
    fn api_token_requires_exact_hex_and_compares_all_bytes() {
        let token = ApiToken::parse(&"ab".repeat(32)).unwrap();
        assert!(token.matches(Some(&"ab".repeat(32))));
        assert!(!token.matches(Some(&format!("{}ac", "ab".repeat(31)))));
        assert!(!token.matches(Some("ab")));
        assert!(!token.matches(None));
    }

    #[tokio::test]
    async fn api_reader_rejects_oversized_and_incomplete_requests() {
        let oversized = format!("{{\"cmd\":\"{}\"}}\n", "x".repeat(64));
        let error = read_request_with_limits(oversized.as_bytes(), 32, Duration::from_millis(100))
            .await
            .expect_err("oversized request must be rejected");
        assert!(error.contains("exceeds 32 byte limit"));

        let error = read_request_with_limits(
            b"{\"cmd\":\"get_version\"}".as_slice(),
            64,
            Duration::from_millis(100),
        )
        .await
        .expect_err("request without a newline must be rejected");
        assert_eq!(error, "incomplete request");
    }

    #[tokio::test]
    async fn api_reader_times_out_slow_clients() {
        let (_writer, reader) = tokio::io::duplex(64);
        let error = read_request_with_limits(reader, 64, Duration::from_millis(10))
            .await
            .expect_err("silent client must time out");
        assert_eq!(error, "request read timed out");
    }

    #[test]
    fn history_capabilities_advertise_multi_label_and_date_contracts() {
        let capabilities = history_capabilities_data();
        assert_eq!(
            capabilities["classifier_version"].as_i64(),
            Some(crate::history_classifier::CLASSIFIER_VERSION)
        );
        assert_eq!(capabilities["multiple_labels"].as_bool(), Some(true));
        assert_eq!(capabilities["date_range_filter"].as_bool(), Some(true));
        assert_eq!(
            capabilities["categories"].as_array().map(Vec::len),
            Some(crate::history_classifier::CATEGORIES.len())
        );
    }

    #[test]
    fn history_request_accepts_new_filters_and_legacy_omissions() {
        let filtered: Request = serde_json::from_value(serde_json::json!({
            "cmd": "get_history",
            "keyword": "needle",
            "category": "text",
            "start_time": "2026-02-01T10:00:00Z",
            "end_time": "2026-02-01T11:00:00Z",
            "limit": 31,
            "offset": 62
        }))
        .unwrap();
        assert_eq!(filtered.keyword.as_deref(), Some("needle"));
        assert_eq!(filtered.category.as_deref(), Some("text"));
        assert_eq!(filtered.start_time.as_deref(), Some("2026-02-01T10:00:00Z"));
        assert_eq!(filtered.end_time.as_deref(), Some("2026-02-01T11:00:00Z"));
        assert_eq!(filtered.limit, Some(31));
        assert_eq!(filtered.offset, Some(62));

        let legacy: Request =
            serde_json::from_value(serde_json::json!({ "cmd": "get_history" })).unwrap();
        assert!(legacy.keyword.is_none());
        assert!(legacy.category.is_none());
        assert!(legacy.start_time.is_none());
        assert!(legacy.end_time.is_none());
    }

    #[test]
    fn peer_snapshot_keeps_local_identity_when_discovery_fails() {
        let identity = DeviceIdentity::generate_for_test();
        let settings = Settings::default();
        let data = peer_snapshot_data(
            &identity,
            &settings,
            Err("Tailscale status unavailable".to_string()),
        );
        let public_key = identity.public_key_base64();
        let fingerprint = identity.fingerprint();

        let local = data["self"].as_object().expect("local device snapshot");
        assert!(!local["hostname"].as_str().unwrap_or_default().is_empty());
        assert_eq!(local["public_key"].as_str(), Some(public_key.as_str()));
        assert_eq!(local["fingerprint"].as_str(), Some(fingerprint.as_str()));
        assert_eq!(data["peers"].as_array().map(Vec::len), Some(0));
        assert_eq!(
            data["discovery_error"].as_str(),
            Some("Tailscale status unavailable")
        );
    }

    #[test]
    fn automatic_snapshot_does_not_treat_discovery_flags_as_online_health() {
        let identity = DeviceIdentity::generate_for_test();
        let remote = DeviceIdentity::generate_for_test();
        let mut settings = Settings {
            connection_mode: "auto".into(),
            ..Settings::default()
        };
        settings
            .trusted_peer_keys
            .insert("Mac".into(), remote.public_key_base64());
        settings.trusted_peer_addresses.insert(
            "Mac".into(),
            HashMap::from([
                ("lan".into(), "192.168.31.247".into()),
                ("tailscale".into(), "100.111.236.101".into()),
            ]),
        );
        settings
            .paired_peer_endpoints
            .insert("Mac".into(), "192.168.31.247".into());
        let data = peer_snapshot_data(
            &identity,
            &settings,
            Ok((
                LocalInfo {
                    hostname: "windows".into(),
                    tailscale_ip: "192.168.31.78".into(),
                    candidates: vec![PeerCandidate::new(
                        ConnectionInterface::Lan,
                        "192.168.31.78",
                    )],
                },
                vec![PeerInfo {
                    hostname: "Mac".into(),
                    tailscale_ip: "192.168.31.247".into(),
                    online: true,
                    enabled: true,
                    address: "192.168.31.247".into(),
                    connection_mode: "auto".into(),
                    trusted: false,
                    fingerprint: String::new(),
                    candidates: vec![PeerCandidate::new(
                        ConnectionInterface::Lan,
                        "192.168.31.247",
                    )],
                    current_interface: None,
                }],
            )),
        );

        let peer = &data["peers"][0];
        assert_eq!(data["self"]["routes"].as_array().map(Vec::len), Some(1));
        assert_eq!(peer["trusted"].as_bool(), Some(true));
        let routes = peer["routes"].as_array().expect("peer routes");
        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0]["interface"].as_str(), Some("lan"));
        assert_eq!(routes[0]["status"].as_str(), Some("discovered"));
        assert_eq!(routes[0]["online"].as_bool(), Some(false));
        assert_eq!(routes[0]["connected"].as_bool(), Some(false));
        assert_eq!(routes[0]["pairing_endpoint"].as_bool(), Some(true));
        assert_eq!(routes[0]["rtt_capable"].as_bool(), Some(true));
        assert_eq!(routes[1]["interface"].as_str(), Some("tailscale"));
        assert_eq!(routes[1]["address"].as_str(), Some("100.111.236.101"));
        assert_eq!(routes[1]["online"].as_bool(), Some(false));
        assert_eq!(routes[1]["connected"].as_bool(), Some(false));
        assert_eq!(routes[1]["pairing_endpoint"].as_bool(), Some(false));
        assert_eq!(routes[1]["rtt_capable"].as_bool(), Some(true));
        assert_eq!(
            data["paired_peer_endpoints"]["Mac"].as_str(),
            Some("192.168.31.247")
        );
    }

    #[test]
    fn peer_snapshot_exposes_an_actionable_protocol_upgrade_diagnostic() {
        let identity = DeviceIdentity::generate_for_test();
        let remote = DeviceIdentity::generate_for_test();
        let hostname = format!("protocol-snapshot-test-{}", rand::random::<u64>());
        let address = "192.168.252.31";
        let mut settings = Settings::default();
        settings
            .trusted_peer_keys
            .insert(hostname.clone(), remote.public_key_base64());
        settings.trusted_peer_addresses.insert(
            hostname.clone(),
            HashMap::from([("lan".into(), address.into())]),
        );
        crate::network::record_protocol_compatibility_error(
            &hostname,
            "Incompatible TailSync protocol: peer uses v2",
        );

        let data = peer_snapshot_data(
            &identity,
            &settings,
            Ok((
                LocalInfo {
                    hostname: "windows".into(),
                    tailscale_ip: String::new(),
                    candidates: Vec::new(),
                },
                Vec::new(),
            )),
        );
        crate::network::clear_protocol_compatibility_error(&hostname);

        let peer = data["peers"]
            .as_array()
            .and_then(|peers| peers.iter().find(|peer| peer["hostname"] == hostname))
            .expect("trusted peer snapshot");
        assert_eq!(
            peer["protocol_error"].as_str(),
            Some("Incompatible TailSync protocol: peer uses v2")
        );
        assert_eq!(
            peer["required_protocol_version"].as_u64(),
            Some(crate::protocol::VERSION.into())
        );
    }

    #[test]
    fn automatic_snapshot_connects_only_the_exact_authenticated_route() {
        let identity = DeviceIdentity::generate_for_test();
        let remote = DeviceIdentity::generate_for_test();
        let hostname = "snapshot-route-session-test";
        let lan_address = "192.168.251.31";
        let tailscale_address = "100.100.251.31";
        let mut settings = Settings {
            connection_mode: "auto".into(),
            ..Settings::default()
        };
        settings
            .trusted_peer_keys
            .insert(hostname.into(), remote.public_key_base64());
        settings.trusted_peer_addresses.insert(
            hostname.into(),
            HashMap::from([
                ("lan".into(), lan_address.into()),
                ("tailscale".into(), tailscale_address.into()),
            ]),
        );
        let _session = register_active_session(hostname, ConnectionInterface::Lan, lan_address, 6);
        let data = peer_snapshot_data(
            &identity,
            &settings,
            Ok((
                LocalInfo {
                    hostname: "windows".into(),
                    tailscale_ip: "192.168.251.30".into(),
                    candidates: Vec::new(),
                },
                vec![PeerInfo {
                    hostname: hostname.into(),
                    tailscale_ip: lan_address.into(),
                    online: true,
                    enabled: true,
                    address: lan_address.into(),
                    connection_mode: "auto".into(),
                    trusted: false,
                    fingerprint: String::new(),
                    candidates: vec![PeerCandidate::new(ConnectionInterface::Lan, lan_address)],
                    current_interface: None,
                }],
            )),
        );

        let routes = data["peers"][0]["routes"].as_array().expect("peer routes");
        let lan = routes
            .iter()
            .find(|route| route["interface"] == "lan")
            .expect("LAN route");
        let tailscale = routes
            .iter()
            .find(|route| route["interface"] == "tailscale")
            .expect("Tailscale route");
        assert_eq!(lan["status"].as_str(), Some("connected"));
        assert_eq!(lan["connected"].as_bool(), Some(true));
        assert_eq!(tailscale["status"].as_str(), Some("discovered"));
        assert_eq!(tailscale["connected"].as_bool(), Some(false));
    }
}
