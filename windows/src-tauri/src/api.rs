//! JSON-line TCP API server for the SwiftUI frontend.
//!
//! Protocol: one JSON object per line, terminated by `\n`.
//! Request:  `{"cmd": "...", ...params}`
//! Response: `{"ok": true, ...data}` or `{"ok": false, "error": "..."}`

use crate::db;
use crate::identity;
use crate::network;
use crate::sync;
use crate::{crypto, identity::DeviceIdentity};
use log::info;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tauri::Manager;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;

/// Monotonic version — bumped on every clipboard change.
pub static CLIPBOARD_VERSION: AtomicU64 = AtomicU64::new(0);
pub fn bump_clipboard_version() {
    CLIPBOARD_VERSION.fetch_add(1, Ordering::Release);
}
pub fn get_clipboard_version() -> u64 {
    CLIPBOARD_VERSION.load(Ordering::Acquire)
}

// File transfer progress
use std::sync::Mutex as StdMutex;
pub static FILE_PROGRESS: StdMutex<Option<FileProgress>> = StdMutex::new(None);
#[derive(Clone, Serialize)]
pub struct FileProgress {
    pub name: String,
    pub sent: u64,
    pub total: u64,
    pub active: bool,
}
pub fn set_file_progress(name: &str, sent: u64, total: u64) {
    if let Ok(mut p) = FILE_PROGRESS.lock() {
        *p = Some(FileProgress {
            name: name.into(),
            sent,
            total,
            active: sent < total,
        });
    }
}
pub fn clear_file_progress() {
    if let Ok(mut p) = FILE_PROGRESS.lock() {
        *p = None;
    }
}

/// Write file content to a temp file and put its URL on the macOS clipboard.
#[cfg(target_os = "macos")]
pub fn restore_file_to_clipboard(data: &[u8], fname: &str) {
    let tmp = std::env::temp_dir().join(fname);
    if std::fs::write(&tmp, data).is_err() {
        return;
    }
    restore_file_path_to_clipboard(&tmp, fname);
}

pub fn restore_file_path_to_clipboard(file_path: &Path, file_name: &str) {
    let clipboard_path = match crate::db::materialize_clipboard_file(file_path, file_name) {
        Ok(path) => path,
        Err(error) => {
            log::error!("Could not prepare file for clipboard: {error}");
            return;
        }
    };
    write_file_path_to_clipboard(&clipboard_path);
}

#[cfg(target_os = "macos")]
fn write_file_path_to_clipboard(file_path: &Path) {
    match crate::clipboard_file::write_clipboard_files(&[file_path.to_path_buf()]) {
        Ok(()) => log::info!("Restored file to clipboard: {}", file_path.display()),
        Err(error) => log::error!("Could not restore file to clipboard: {error}"),
    }
}

#[cfg(target_os = "windows")]
pub fn restore_file_to_clipboard(data: &[u8], fname: &str) {
    // Legacy BLOB entries are materialized once in the temp directory.
    let tmp_dir = std::env::temp_dir().join("tailsync");
    let _ = std::fs::create_dir_all(&tmp_dir);
    let file_path = tmp_dir.join(fname);
    if std::fs::write(&file_path, data).is_err() {
        return;
    }
    restore_file_path_to_clipboard(&file_path, fname);
}

#[cfg(target_os = "windows")]
fn write_file_path_to_clipboard(file_path: &Path) {
    use std::os::windows::ffi::OsStrExt;
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
            return;
        }
        EmptyClipboard();
        let h = GlobalAlloc(GHND, total_size);
        if !h.is_null() {
            let ptr = GlobalLock(h) as *mut u8;
            // Write DROPFILES header
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
            // Write wide path + double null
            let path_ptr = ptr.add(path_offset as usize) as *mut u16;
            std::ptr::copy_nonoverlapping(wide_path.as_ptr(), path_ptr, wide_path.len());
            path_ptr.add(wide_path.len()).write(0); // double null
            GlobalUnlock(h);
            SetClipboardData(15, h); // CF_HDROP
        }
        CloseClipboard();
    }
    log::info!(
        "Restored file to Windows clipboard: {}",
        file_path.display()
    );
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
pub fn restore_file_to_clipboard(_data: &[u8], _fname: &str) {}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn write_file_path_to_clipboard(_file_path: &Path) {}

/// Nearest-neighbor downscale of RGBA pixel data to a max-side thumbnail.
pub fn thumbnail_rgba(w: usize, h: usize, rgba: &[u8], max_side: usize) -> (usize, usize, Vec<u8>) {
    let scale = (max_side as f64 / w.max(h) as f64).min(1.0);
    let tw = (w as f64 * scale) as usize;
    let th = (h as f64 * scale) as usize;
    let mut out = vec![0u8; tw * th * 4];
    for y in 0..th {
        for x in 0..tw {
            let sx = (x as f64 / scale) as usize;
            let sy = (y as f64 / scale) as usize;
            let si = (sy * w + sx) * 4;
            let di = (y * tw + x) * 4;
            if si + 3 < rgba.len() {
                out[di..di + 4].copy_from_slice(&rgba[si..si + 4]);
            }
        }
    }
    (tw, th, out)
}

pub const API_PORT: u16 = 19889;

pub struct ApiState {
    pub db: Arc<Mutex<db::HistoryDB>>,
    pub sync_engine: Arc<Mutex<sync::SyncEngine>>,
    pub settings: Arc<Mutex<crypto::Settings>>,
    pub identity: Arc<DeviceIdentity>,
    pub pool: Arc<Mutex<network::ConnectionPool>>,
    pub pairing: Arc<crate::pairing::PairingManager>,
}

#[derive(Debug, Deserialize)]
struct Request {
    cmd: String,
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
    hostname: Option<String>,
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    public_key: Option<String>,
    #[serde(default)]
    address: Option<String>,
}

#[derive(Debug, Serialize)]
struct Response {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

pub(crate) fn peer_snapshot_data(
    identity: &DeviceIdentity,
    settings: &crypto::Settings,
    discovery: Result<
        (
            network::tailscale::LocalInfo,
            Vec<network::tailscale::PeerInfo>,
        ),
        String,
    >,
) -> Value {
    let mode = settings.connection_mode.clone();
    let (local, peers, discovery_error) = match discovery {
        Ok((local, peers)) => (local, peers, None),
        Err(error) => (
            network::tailscale::LocalInfo {
                hostname: network::lan::local_hostname(),
                tailscale_ip: String::new(),
                candidates: Vec::new(),
            },
            Vec::new(),
            Some(error),
        ),
    };

    let peers = network::merge_paired_peers(settings, &mode, peers)
        .into_iter()
        .map(|mut peer| {
            let route_health = peer
                .candidates
                .iter()
                .map(|candidate| {
                    let health = network::route_health(
                        &peer.hostname,
                        candidate.interface,
                        &candidate.address,
                    );
                    (candidate.clone(), health)
                })
                .collect::<Vec<_>>();
            peer.online = route_health.iter().any(|(_, health)| health.online);
            peer.current_interface = route_health
                .iter()
                .find(|(_, health)| health.connected)
                .map(|(candidate, _)| candidate.interface);
            let routes = route_health
                .into_iter()
                .map(|(candidate, health)| {
                    serde_json::json!({
                        "interface": candidate.interface,
                        "address": candidate.address,
                        "status": health.status,
                        "online": health.online,
                        "connected": health.connected,
                        "latency_ms": health.latency_ms,
                    })
                })
                .collect::<Vec<_>>();
            let mut value = serde_json::to_value(peer).expect("peer snapshot is serializable");
            value["routes"] = Value::Array(routes);
            value
        })
        .collect::<Vec<_>>();
    let local_routes = local
        .candidates
        .iter()
        .map(|candidate| {
            serde_json::json!({
                "interface": candidate.interface,
                "address": candidate.address,
                "status": network::PeerStatus::Connected,
                "online": true,
                "connected": true,
                "latency_ms": Value::Null,
            })
        })
        .collect::<Vec<_>>();

    serde_json::json!({
        "self": {
            "hostname": local.hostname,
            "tailscale_ip": local.tailscale_ip,
            "routes": local_routes,
            "connection_mode": mode,
            "public_key": identity.public_key_base64(),
            "fingerprint": identity.fingerprint(),
        },
        "peers": peers,
        "discovery_error": discovery_error,
    })
}

pub async fn start(state: Arc<ApiState>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let addr = format!("127.0.0.1:{}", API_PORT);
    let listener = network::bind_tcp_listener(addr.parse()?)?;
    info!("API server listening on {}", addr);

    loop {
        let (stream, _) = listener.accept().await?;
        let st = state.clone();
        tokio::spawn(async move {
            let (reader, mut writer) = stream.into_split();
            let mut reader = BufReader::new(reader);
            let mut line = String::new();
            if reader.read_line(&mut line).await.is_ok() {
                let req: Request = match serde_json::from_str(line.trim()) {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = send_json(&mut writer, false, None, &e.to_string()).await;
                        return;
                    }
                };
                let resp = handle_cmd(req, &st).await;
                let _ = send_json(
                    &mut writer,
                    resp.ok,
                    resp.data,
                    &resp.error.unwrap_or_default(),
                )
                .await;
            }
        });
    }
}

async fn handle_cmd(req: Request, state: &ApiState) -> Response {
    match req.cmd.as_str() {
        "ping" => Response {
            ok: true,
            data: None,
            error: None,
        },

        "get_file_progress" => {
            let info = FILE_PROGRESS.lock().unwrap().clone();
            Response {
                ok: true,
                data: info.map(|p| {
                    serde_json::json!({
                        "name": p.name, "sent": p.sent, "total": p.total, "active": p.active
                    })
                }),
                error: None,
            }
        }

        "get_version" => Response {
            ok: true,
            data: Some(serde_json::json!(CLIPBOARD_VERSION.load(Ordering::Acquire))),
            error: None,
        },

        "get_history_capabilities" => Response {
            ok: true,
            data: Some(history_capabilities_data()),
            error: None,
        },

        "get_status" => Response {
            ok: true,
            data: Some(serde_json::json!({
                "tcp_server_healthy": network::TCP_SERVER_HEALTHY.load(Ordering::Acquire),
                "clipboard_monitor_healthy": crate::clipboard::monitor_is_healthy(),
                "active_routes": network::active_routes_snapshot(),
            })),
            error: None,
        },

        "enable_pairing" => Response {
            ok: true,
            data: Some(serde_json::to_value(state.pairing.enable().await).unwrap_or_default()),
            error: None,
        },

        "get_pairing_status" => Response {
            ok: true,
            data: Some(serde_json::to_value(state.pairing.status().await).unwrap_or_default()),
            error: None,
        },

        "start_pairing" => {
            let address = req.address.as_deref().unwrap_or_default().trim();
            if address.is_empty() {
                return Response {
                    ok: false,
                    data: None,
                    error: Some("missing peer address".into()),
                };
            }
            match network::start_pairing(
                state.pairing.clone(),
                state.identity.clone(),
                state.settings.clone(),
                address,
            )
            .await
            {
                Ok(()) => Response {
                    ok: true,
                    data: Some(
                        serde_json::to_value(state.pairing.status().await).unwrap_or_default(),
                    ),
                    error: None,
                },
                Err(error) => Response {
                    ok: false,
                    data: Some(
                        serde_json::to_value(state.pairing.status().await).unwrap_or_default(),
                    ),
                    error: Some(error),
                },
            }
        }

        "confirm_pairing" => match state.pairing.confirm().await {
            Ok(status) => Response {
                ok: true,
                data: Some(serde_json::to_value(status).unwrap_or_default()),
                error: None,
            },
            Err(error) => Response {
                ok: false,
                data: Some(serde_json::to_value(state.pairing.status().await).unwrap_or_default()),
                error: Some(error),
            },
        },

        "cancel_pairing" => Response {
            ok: true,
            data: Some(serde_json::to_value(state.pairing.cancel().await).unwrap_or_default()),
            error: None,
        },

        "get_history" => {
            let db = state.db.lock().await;
            // Consume before await for Send safety
            let result = db
                .get_all_filtered(
                    req.keyword.as_deref(),
                    req.category.as_deref(),
                    req.start_time.as_deref(),
                    req.end_time.as_deref(),
                    req.limit.unwrap_or(30),
                    req.offset.unwrap_or(0),
                )
                .map_err(|e| e.to_string());
            drop(db);
            match result {
                Ok(entries) => Response {
                    ok: true,
                    data: Some(serde_json::to_value(entries).unwrap_or_default()),
                    error: None,
                },
                Err(e) => Response {
                    ok: false,
                    data: None,
                    error: Some(e),
                },
            }
        }

        "delete_entry" => {
            let Some(id) = req.id else {
                return Response {
                    ok: false,
                    data: None,
                    error: Some("missing id".into()),
                };
            };
            let mut db = state.db.lock().await;
            match db.delete(id) {
                Ok(()) => Response {
                    ok: true,
                    data: None,
                    error: None,
                },
                Err(e) => Response {
                    ok: false,
                    data: None,
                    error: Some(e.to_string()),
                },
            }
        }

        "clear_history" | "clear_all" => {
            let mut db = state.db.lock().await;
            match db.clear_all() {
                Ok(()) => {
                    bump_clipboard_version();
                    Response {
                        ok: true,
                        data: None,
                        error: None,
                    }
                }
                Err(e) => Response {
                    ok: false,
                    data: None,
                    error: Some(e.to_string()),
                },
            }
        }

        "restore_entry" => {
            let Some(id) = req.id else {
                return Response {
                    ok: false,
                    data: None,
                    error: Some("missing id".into()),
                };
            };
            let db = state.db.lock().await;
            let entry_type = db
                .get_type(id)
                .map_err(|e| e.to_string())
                .unwrap_or_default();
            let file_path = if entry_type == "file" {
                db.get_file_path(id).map_err(|e| e.to_string())
            } else {
                Ok(None)
            };
            let file_name = if entry_type == "file" {
                db.get_description(id)
                    .unwrap_or_else(|_| "restored_file".into())
            } else {
                String::new()
            };
            let data_result = match &file_path {
                Ok(Some(_)) => Ok(None),
                Ok(None) => db.get_data(id).map(Some).map_err(|e| e.to_string()),
                Err(error) => Err(error.clone()),
            };
            drop(db);
            match (data_result, file_path) {
                (Ok(data), Ok(file_path)) => {
                    let sync = state.sync_engine.lock().await;
                    let handle = sync.app_handle.clone();
                    drop(sync);

                    if entry_type == "image" && data.as_ref().is_some_and(|data| data.len() >= 8) {
                        let data = data.as_ref().expect("image data checked above");
                        let w = u32::from_le_bytes(data[0..4].try_into().unwrap());
                        let h = u32::from_le_bytes(data[4..8].try_into().unwrap());
                        let rgba = &data[8..];
                        let img = tauri::image::Image::new(rgba, w, h);
                        {
                            let mut sync = state.sync_engine.lock().await;
                            sync.add_image_shadow_filter(data);
                        }
                        if let Some(ref handle) = handle {
                            if let Some(cb) = handle
                                .try_state::<tauri_plugin_clipboard_manager::Clipboard<tauri::Wry>>(
                                )
                            {
                                let _ = cb.write_image(&img);
                            }
                        }
                    } else if entry_type == "file" {
                        if let Some(path) = file_path {
                            restore_file_path_to_clipboard(&path, &file_name);
                        } else if let Some(data) = data.as_deref() {
                            restore_file_to_clipboard(data, &file_name);
                        }
                    } else {
                        let text = String::from_utf8_lossy(data.as_deref().unwrap_or_default())
                            .to_string();
                        {
                            let mut sync = state.sync_engine.lock().await;
                            sync.add_shadow_filter(&text);
                        }
                        if let Some(ref handle) = handle {
                            if let Some(cb) = handle
                                .try_state::<tauri_plugin_clipboard_manager::Clipboard<tauri::Wry>>(
                                )
                            {
                                let _ = cb.write_text(text);
                            }
                        }
                    }

                    crate::api::bump_clipboard_version();
                    Response {
                        ok: true,
                        data: None,
                        error: None,
                    }
                }
                (Err(e), _) | (_, Err(e)) => Response {
                    ok: false,
                    data: None,
                    error: Some(e),
                },
            }
        }

        "get_settings" => {
            let settings = state.settings.lock().await.clone();
            Response {
                ok: true,
                data: Some(serde_json::to_value(settings).unwrap_or_default()),
                error: None,
            }
        }

        "update_settings" => {
            let Some(settings_json) = req.settings else {
                return Response {
                    ok: false,
                    data: None,
                    error: Some("missing settings".into()),
                };
            };
            match serde_json::from_value::<crate::crypto::Settings>(settings_json) {
                Ok(mut new_settings) => {
                    if let Err(error) = new_settings.validate_user_values() {
                        return Response {
                            ok: false,
                            data: None,
                            error: Some(error),
                        };
                    }
                    let mut settings = state.settings.lock().await;
                    let mode_changed = settings.connection_mode != new_settings.connection_mode;
                    new_settings.trusted_peer_keys = settings.trusted_peer_keys.clone();
                    new_settings.trusted_peer_addresses = settings.trusted_peer_addresses.clone();
                    let limit = new_settings.history_limit as i64;
                    *settings = new_settings;
                    if let Err(e) = settings.save() {
                        return Response {
                            ok: false,
                            data: None,
                            error: Some(e.to_string()),
                        };
                    }
                    drop(settings);
                    if mode_changed {
                        state.pool.lock().await.disconnect_all();
                        network::clear_peer_cache().await;
                    }
                    let mut db = state.db.lock().await;
                    db.set_max_history(limit);
                    Response {
                        ok: true,
                        data: None,
                        error: None,
                    }
                }
                Err(e) => Response {
                    ok: false,
                    data: None,
                    error: Some(e.to_string()),
                },
            }
        }

        "get_peers" => {
            let settings = state.settings.lock().await.clone();
            let mode = settings.connection_mode.clone();
            let discovery = network::cached_discover_peers(&mode).await;
            Response {
                ok: true,
                data: Some(peer_snapshot_data(&state.identity, &settings, discovery)),
                error: None,
            }
        }

        "refresh_peers" => {
            let mode = state.settings.lock().await.connection_mode.clone();
            match network::request_peer_refresh(&mode).await {
                Ok(()) => {
                    let settings = state.settings.lock().await.clone();
                    let mode = settings.connection_mode.clone();
                    let discovery = network::cached_discover_peers(&mode).await;
                    Response {
                        ok: true,
                        data: Some(peer_snapshot_data(&state.identity, &settings, discovery)),
                        error: None,
                    }
                }
                Err(error) => Response {
                    ok: false,
                    data: None,
                    error: Some(error),
                },
            }
        }

        "toggle_peer" => {
            let hostname = req.hostname.as_deref().unwrap_or_default().trim();
            if hostname.is_empty() {
                return Response {
                    ok: false,
                    data: None,
                    error: Some("missing hostname".into()),
                };
            }
            let enabled = req.enabled.unwrap_or(true);
            let result = state
                .settings
                .lock()
                .await
                .toggle_peer(hostname, enabled)
                .map_err(|error| error.to_string());
            if result.is_ok() && !enabled {
                state.pool.lock().await.disconnect_hostname(hostname);
            }
            match result {
                Ok(()) => Response {
                    ok: true,
                    data: None,
                    error: None,
                },
                Err(error) => Response {
                    ok: false,
                    data: None,
                    error: Some(error),
                },
            }
        }

        "trust_peer" => {
            let hostname = req.hostname.as_deref().unwrap_or_default().trim();
            let public_key = req.public_key.as_deref().unwrap_or_default();
            let address = req
                .address
                .as_deref()
                .filter(|value| !value.trim().is_empty());
            if hostname.is_empty() || hostname.len() > 255 {
                return Response {
                    ok: false,
                    data: None,
                    error: Some("invalid hostname".into()),
                };
            }
            let public_key = match identity::canonical_public_key(public_key) {
                Ok(key) => key,
                Err(error) => {
                    return Response {
                        ok: false,
                        data: None,
                        error: Some(error),
                    }
                }
            };
            if public_key == state.identity.public_key_base64() {
                return Response {
                    ok: false,
                    data: None,
                    error: Some("cannot pair this device with itself".into()),
                };
            }
            let decoded = identity::decode_public_key(&public_key).expect("canonical key");
            let fingerprint = identity::fingerprint(&decoded);
            let result = {
                let mut settings = state.settings.lock().await;
                let mode = match (settings.connection_mode.as_str(), req.address.as_deref()) {
                    ("auto", Some(address)) => match network::infer_interface(address) {
                        Ok(interface) => interface.as_str().to_string(),
                        Err(error) => {
                            return Response {
                                ok: false,
                                data: None,
                                error: Some(error),
                            }
                        }
                    },
                    (mode, _) => network::mode_interface(mode)
                        .map(|interface| interface.as_str().to_string())
                        .unwrap_or_else(|| "lan".to_string()),
                };
                settings
                    .trust_peer(hostname, &public_key, &mode, address)
                    .map_err(|error| error.to_string())
            };
            if result.is_ok() {
                state.pool.lock().await.disconnect_hostname(hostname);
            }
            match result {
                Ok(()) => Response {
                    ok: true,
                    data: Some(serde_json::json!({ "fingerprint": fingerprint })),
                    error: None,
                },
                Err(error) => Response {
                    ok: false,
                    data: None,
                    error: Some(error),
                },
            }
        }

        "forget_peer" => {
            let hostname = req.hostname.as_deref().unwrap_or_default().trim();
            if hostname.is_empty() {
                return Response {
                    ok: false,
                    data: None,
                    error: Some("missing hostname".into()),
                };
            }
            let result = state
                .settings
                .lock()
                .await
                .forget_peer(hostname)
                .map_err(|error| error.to_string());
            if result.is_ok() {
                state.pool.lock().await.disconnect_hostname(hostname);
            }
            match result {
                Ok(()) => Response {
                    ok: true,
                    data: None,
                    error: None,
                },
                Err(error) => Response {
                    ok: false,
                    data: None,
                    error: Some(error),
                },
            }
        }

        "test_connection" => {
            let hostname = req.hostname.as_deref().unwrap_or_default().trim();
            if hostname.is_empty() {
                return Response {
                    ok: false,
                    data: None,
                    error: Some("missing address".into()),
                };
            }
            match network::test_connection(hostname).await {
                Ok(latency_ms) => {
                    network::record_address_test_success(hostname, latency_ms);
                    Response {
                        ok: true,
                        data: Some(serde_json::json!({ "latency_ms": latency_ms })),
                        error: None,
                    }
                }
                Err(error) => {
                    network::record_address_test_failure(hostname);
                    Response {
                        ok: false,
                        data: None,
                        error: Some(error),
                    }
                }
            }
        }

        "reconnect_peers" => {
            state.pool.lock().await.disconnect_all();
            crate::clipboard::request_wake_recovery();
            network::clear_peer_cache().await;
            Response {
                ok: true,
                data: None,
                error: None,
            }
        }

        "get_image_data" => {
            let Some(id) = req.id else {
                return Response {
                    ok: false,
                    data: None,
                    error: Some("missing id".into()),
                };
            };
            let db = state.db.lock().await;
            let result = db.get_data(id).map_err(|e| e.to_string());
            drop(db);
            match result {
                Ok(data) if data.len() >= 8 => {
                    let w = u32::from_le_bytes(data[0..4].try_into().unwrap());
                    let h = u32::from_le_bytes(data[4..8].try_into().unwrap());
                    let rgba = &data[8..];
                    // Downsample to thumbnail (max 64px)
                    let (tw, th, thumb) = thumbnail_rgba(w as usize, h as usize, rgba, 64);
                    use base64::Engine;
                    let b64 = base64::engine::general_purpose::STANDARD.encode(&thumb);
                    Response {
                        ok: true,
                        data: Some(serde_json::json!({
                            "width": tw,
                            "height": th,
                            "rgba_b64": b64,
                        })),
                        error: None,
                    }
                }
                Ok(_) => Response {
                    ok: false,
                    data: None,
                    error: Some("not an image".into()),
                },
                Err(e) => Response {
                    ok: false,
                    data: None,
                    error: Some(e),
                },
            }
        }

        "migrate_entry" => {
            let (Some(time), Some(etype), Some(desc), Some(data_b64)) =
                (&req.time, &req.entry_type, &req.desc, &req.data_b64)
            else {
                return Response {
                    ok: false,
                    data: None,
                    error: Some("missing fields".into()),
                };
            };
            use base64::Engine;
            let data = match base64::engine::general_purpose::STANDARD.decode(data_b64) {
                Ok(d) => d,
                Err(e) => {
                    return Response {
                        ok: false,
                        data: None,
                        error: Some(e.to_string()),
                    }
                }
            };
            let mut db = state.db.lock().await;
            let result = match etype.as_str() {
                "text" => db.add_text_migrated(time, desc, &data),
                "image" => db.add_image_migrated(time, desc, &data),
                "file" => db.add_file_migrated(time, desc, &data),
                _ => Err("unknown type".into()),
            };
            match result {
                Ok(()) => {
                    crate::api::bump_clipboard_version();
                    Response {
                        ok: true,
                        data: None,
                        error: None,
                    }
                }
                Err(e) => Response {
                    ok: false,
                    data: None,
                    error: Some(e.to_string()),
                },
            }
        }

        "quit" => {
            info!("Quit via API");
            std::process::exit(0);
        }

        _ => Response {
            ok: false,
            data: None,
            error: Some(format!("unknown command: {}", req.cmd)),
        },
    }
}

fn history_capabilities_data() -> Value {
    serde_json::json!({
        "classifier_version": crate::history_classifier::CLASSIFIER_VERSION,
        "categories": crate::history_classifier::CATEGORIES,
        "multiple_labels": true,
        "date_range_filter": true,
    })
}

async fn send_json(
    w: &mut (impl AsyncWriteExt + Unpin),
    ok: bool,
    data: Option<Value>,
    error: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let resp = if ok {
        serde_json::json!({ "ok": true, "data": data })
    } else {
        serde_json::json!({ "ok": false, "error": error })
    };
    let mut bytes = serde_json::to_vec(&resp)?;
    bytes.push(b'\n');
    w.write_all(&bytes).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{history_capabilities_data, peer_snapshot_data, Request};
    use crate::crypto::Settings;
    use crate::identity::DeviceIdentity;
    use crate::network::tailscale::{LocalInfo, PeerInfo};
    use crate::network::{register_active_session, ConnectionInterface, PeerCandidate};
    use std::collections::HashMap;

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
        assert_eq!(routes[1]["interface"].as_str(), Some("tailscale"));
        assert_eq!(routes[1]["address"].as_str(), Some("100.111.236.101"));
        assert_eq!(routes[1]["online"].as_bool(), Some(false));
        assert_eq!(routes[1]["connected"].as_bool(), Some(false));
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
