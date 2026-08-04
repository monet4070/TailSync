use crate::db;
use crate::network;
use crate::AppState;
use log::info;
use tauri::{command, AppHandle, Manager, State};

#[derive(serde::Serialize)]
pub struct HistoryPage {
    pub entries: Vec<db::HistoryEntry>,
    pub total: usize,
}

/// Get clipboard history entries
#[command]
pub async fn get_history(
    state: State<'_, AppState>,
    keyword: Option<String>,
    category: Option<String>,
    start_time: Option<String>,
    end_time: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<Vec<db::HistoryEntry>, String> {
    let db = state.db.lock().await;
    db.get_all_filtered(
        keyword.as_deref(),
        category.as_deref(),
        start_time.as_deref(),
        end_time.as_deref(),
        limit.unwrap_or(50),
        offset.unwrap_or(0),
    )
    .map_err(|e| e.to_string())
}

#[command]
pub async fn get_history_page(
    state: State<'_, AppState>,
    keyword: Option<String>,
    category: Option<String>,
    start_time: Option<String>,
    end_time: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<HistoryPage, String> {
    let db = state.db.lock().await;
    let entries = db
        .get_all_filtered(
            keyword.as_deref(),
            category.as_deref(),
            start_time.as_deref(),
            end_time.as_deref(),
            limit.unwrap_or(50),
            offset.unwrap_or(0),
        )
        .map_err(|e| e.to_string())?;
    let total = db
        .count_all_filtered(
            keyword.as_deref(),
            category.as_deref(),
            start_time.as_deref(),
            end_time.as_deref(),
        )
        .map_err(|e| e.to_string())?;
    Ok(HistoryPage { entries, total })
}

#[command]
pub async fn get_history_capabilities() -> Result<serde_json::Value, String> {
    Ok(crate::api::history_capabilities_data())
}

#[command]
pub async fn get_migration_diagnostics(
    state: State<'_, AppState>,
) -> Result<db::MigrationDiagnostics, String> {
    state
        .db
        .lock()
        .await
        .migration_diagnostics(50)
        .map_err(|error| error.to_string())
}

/// Search history by keyword (searches description field)
#[command]
pub async fn search_history(
    state: State<'_, AppState>,
    keyword: String,
) -> Result<Vec<db::HistoryEntry>, String> {
    let db = state.db.lock().await;
    db.get_all(Some(&keyword), None, 100, 0)
        .map_err(|e| e.to_string())
}

/// Delete a history entry
#[command]
pub async fn delete_entry(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let mut db = state.db.lock().await;
    db.delete(id).map_err(|e| e.to_string())
}

/// Delete all clipboard history entries.
#[command]
pub async fn clear_history(state: State<'_, AppState>) -> Result<(), String> {
    let mut db = state.db.lock().await;
    db.clear_all().map_err(|e| e.to_string())?;
    crate::api::bump_clipboard_version();
    Ok(())
}

/// Restore a history entry back to clipboard.
/// Handles text (as text), images (as Image), and files (via CF_HDROP).
#[command]
pub async fn restore_entry(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> Result<(), String> {
    let db = state.db.lock().await;
    let entry_type = db.get_type(id).map_err(|e| e.to_string())?;
    let file_path = if entry_type == "file" {
        db.get_file_path(id).map_err(|e| e.to_string())?
    } else {
        None
    };
    let file_name = if entry_type == "file" {
        Some(db.get_description(id).map_err(|e| e.to_string())?)
    } else {
        None
    };
    let data = if file_path.is_none() {
        Some(db.get_data(id).map_err(|e| e.to_string())?)
    } else {
        None
    };
    drop(db);

    let clipboard = app
        .try_state::<tauri_plugin_clipboard_manager::Clipboard<tauri::Wry>>()
        .ok_or("Clipboard not available")?;

    if entry_type == "image" {
        let data = data.as_ref().ok_or("Image history data is unavailable")?;
        let image = crate::protocol::PackedImage::try_from(data.as_slice())
            .map_err(|error| error.to_string())?;
        let w = image.width;
        let h = image.height;
        let rgba = image.rgba;

        // Shadow filter to prevent re-broadcast
        {
            let mut sync = state.sync_engine.lock().await;
            sync.add_image_shadow_filter(data);
        }

        // Try arboard write_image first.
        // Fallback: bypass arboard and set CF_DIB directly via Win32 API.
        let img = tauri::image::Image::new(rgba, w, h);
        match clipboard.write_image(&img) {
            Ok(()) => {
                info!(
                    "Restored entry {} (image {}×{}) to clipboard via arboard",
                    id, w, h
                );
            }
            Err(_e) => {
                info!("write_image failed for entry {} — using raw CF_DIB", id);
                let bmp_dib = rgba_to_dib(rgba, w, h);
                if bmp_dib.is_empty() {
                    return Err("DIB encode failed".into());
                }
                #[cfg(target_os = "windows")]
                {
                    set_clipboard_dib(&bmp_dib);
                    info!(
                        "Restored entry {} (image {}×{}) to clipboard via CF_DIB",
                        id, w, h
                    );
                }
                #[cfg(not(target_os = "windows"))]
                return Err("write_image failed and no fallback on this platform".into());
            }
        }
    } else if entry_type == "file" {
        if let Some(path) = file_path {
            crate::api::restore_file_path_to_clipboard(
                &path,
                file_name.as_deref().unwrap_or("restored_file"),
            );
        } else {
            crate::api::restore_file_to_clipboard(
                data.as_deref().ok_or("File history data is unavailable")?,
                file_name.as_deref().unwrap_or("restored_file"),
            );
        }

        info!(
            "Restored entry {} (file: {}) to clipboard",
            id,
            file_name.as_deref().unwrap_or("restored_file")
        );
    } else {
        // Text entry (or fallback for unknown types)
        let text = String::from_utf8_lossy(data.as_deref().unwrap_or_default()).to_string();

        // Shadow filter to prevent re-broadcast
        {
            let mut sync = state.sync_engine.lock().await;
            sync.add_shadow_filter(&text);
        }

        clipboard
            .write_text(text.clone())
            .map_err(|e| format!("Clipboard text write failed: {}", e))?;

        info!(
            "Restored entry {} to clipboard ({} chars)",
            id,
            text.chars().count()
        );
    }

    crate::api::bump_clipboard_version();
    Ok(())
}

/// Discover online peers using the configured transport.
#[command]
pub async fn get_peers(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let settings = state.settings.lock().await.clone();
    let mode = settings.connection_mode.clone();
    let discovery = network::cached_discover_peers(&mode).await;
    Ok(crate::api::peer_snapshot_data(
        &state.identity,
        &settings,
        discovery,
    ))
}

/// Ask the single background health monitor to run an early discovery round.
#[command]
pub async fn refresh_peers(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    network::request_peer_refresh_and_wait().await?;
    get_peers(state).await
}

/// Pin a peer's Noise static public key after out-of-band verification.
#[command]
pub async fn trust_peer(
    state: State<'_, AppState>,
    hostname: String,
    public_key: String,
    address: Option<String>,
) -> Result<String, String> {
    let hostname = hostname.trim();
    if hostname.is_empty() || hostname.len() > 255 {
        return Err("Invalid peer hostname".to_string());
    }
    let public_key = crate::identity::canonical_public_key(&public_key)?;
    if public_key == state.identity.public_key_base64() {
        return Err("Cannot pair this device with itself".to_string());
    }
    let decoded = crate::identity::decode_public_key(&public_key)?;
    let fingerprint = crate::identity::fingerprint(&decoded);
    {
        let mut settings = state.settings.lock().await;
        let mode = match (settings.connection_mode.as_str(), address.as_deref()) {
            ("auto", Some(address)) => network::infer_interface(address)?.as_str().to_string(),
            (mode, _) => network::mode_interface(mode)
                .map(|interface| interface.as_str().to_string())
                .unwrap_or_else(|| "lan".to_string()),
        };
        settings
            .trust_peer(
                hostname,
                &public_key,
                &mode,
                address.as_deref().filter(|value| !value.trim().is_empty()),
            )
            .map_err(|error| error.to_string())?;
    }
    state.pool.lock().await.disconnect_hostname(hostname);
    Ok(fingerprint)
}

/// Remove a pinned peer identity. New connections are rejected immediately.
#[command]
pub async fn forget_peer(state: State<'_, AppState>, hostname: String) -> Result<(), String> {
    let hostname = hostname.trim();
    state
        .settings
        .lock()
        .await
        .forget_peer(hostname)
        .map_err(|error| error.to_string())?;
    state.pool.lock().await.disconnect_hostname(hostname);
    Ok(())
}

#[command]
pub async fn enable_pairing(
    state: State<'_, AppState>,
) -> Result<crate::pairing::PairingStatus, String> {
    Ok(state.pairing.enable().await)
}

#[command]
pub async fn get_pairing_status(
    state: State<'_, AppState>,
) -> Result<crate::pairing::PairingStatus, String> {
    Ok(state.pairing.status().await)
}

#[command]
pub async fn start_pairing(
    state: State<'_, AppState>,
    address: String,
) -> Result<crate::pairing::PairingStatus, String> {
    network::start_pairing(
        state.pairing.clone(),
        state.identity.clone(),
        state.settings.clone(),
        &address,
    )
    .await?;
    Ok(state.pairing.status().await)
}

#[command]
pub async fn confirm_pairing(
    state: State<'_, AppState>,
) -> Result<crate::pairing::PairingStatus, String> {
    state.pairing.confirm().await
}

#[command]
pub async fn cancel_pairing(
    state: State<'_, AppState>,
) -> Result<crate::pairing::PairingStatus, String> {
    Ok(state.pairing.cancel().await)
}

/// Enable or disable a peer device
#[command]
pub async fn toggle_peer(
    state: State<'_, AppState>,
    hostname: String,
    enabled: bool,
) -> Result<(), String> {
    let mut settings = state.settings.lock().await;
    settings
        .toggle_peer(&hostname, enabled)
        .map_err(|e| e.to_string())?;
    drop(settings);
    if !enabled {
        state.pool.lock().await.disconnect_hostname(&hostname);
    }
    Ok(())
}

/// Get current settings
#[command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<crate::crypto::Settings, String> {
    let settings = state.settings.lock().await;
    Ok(settings.clone())
}

/// Update settings
#[command]
pub async fn update_settings(
    state: State<'_, AppState>,
    settings_json: String,
) -> Result<(), String> {
    let mut new_settings: crate::crypto::Settings =
        serde_json::from_str(&settings_json).map_err(|e| e.to_string())?;
    new_settings.validate_user_values()?;
    let history_limit = new_settings.history_limit as i64;
    let mut settings = state.settings.lock().await;
    let mode_changed = settings.connection_mode != new_settings.connection_mode;
    new_settings.trusted_peer_keys = settings.trusted_peer_keys.clone();
    new_settings.trusted_peer_addresses = settings.trusted_peer_addresses.clone();
    new_settings.paired_peer_endpoints = settings.paired_peer_endpoints.clone();
    *settings = new_settings;
    settings.save().map_err(|e| e.to_string())?;
    drop(settings);
    state.db.lock().await.set_max_history(history_limit);
    if mode_changed {
        state.pool.lock().await.disconnect_all();
        network::clear_peer_cache().await;
    }
    Ok(())
}

/// Get image data as base64 thumbnail for frontend display
#[command]
pub async fn get_image_data(
    state: State<'_, AppState>,
    id: i64,
) -> Result<serde_json::Value, String> {
    let db = state.db.lock().await;
    let data = db.get_data(id).map_err(|e| e.to_string())?;
    let image = crate::protocol::PackedImage::try_from(data.as_slice())
        .map_err(|error| error.to_string())?;
    let (tw, th, thumb) = crate::api::thumbnail_rgba(image, 64);
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&thumb);
    Ok(serde_json::json!({
        "id": id,
        "thumbnail_b64": b64,
        "thumbnail_width": tw,
        "thumbnail_height": th,
    }))
}

/// Get current file transfer progress (for progress bar)
#[command]
pub async fn get_file_progress() -> Result<serde_json::Value, String> {
    let info = crate::api::FILE_PROGRESS
        .lock()
        .ok()
        .and_then(|progress| progress.clone());
    Ok(info.map_or(serde_json::json!({"active": false}), |p| {
        serde_json::json!({"name": p.name, "sent": p.sent, "total": p.total, "active": p.active})
    }))
}

/// Get current clipboard version (for polling-based refresh)
#[command]
pub async fn get_version() -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "version": crate::api::get_clipboard_version()
    }))
}

/// Convert RGBA to CF_DIB clipboard format (BITMAPINFOHEADER + bottom-up BGRA pixels).
/// No file header — this is what Windows stores in the clipboard as CF_DIB.
fn rgba_to_dib(rgba: &[u8], w: u32, h: u32) -> Vec<u8> {
    let w = w as i32;
    let h = h as i32;
    let row_size = w * 4; // 32 bpp, naturally 4-byte aligned
    let pixel_data_size = (row_size * h) as u32;
    let dib_size = 40 + pixel_data_size as usize;

    let mut dib = Vec::with_capacity(dib_size);

    // BITMAPINFOHEADER (40 bytes)
    dib.extend_from_slice(&(40u32).to_le_bytes());
    dib.extend_from_slice(&w.to_le_bytes());
    dib.extend_from_slice(&h.to_le_bytes());
    dib.extend_from_slice(&(1u16).to_le_bytes()); // planes
    dib.extend_from_slice(&(32u16).to_le_bytes()); // bpp = 32
    dib.extend_from_slice(&(0u32).to_le_bytes()); // BI_RGB (no compression)
    dib.extend_from_slice(&pixel_data_size.to_le_bytes());
    dib.extend_from_slice(&(2835i32).to_le_bytes()); // 72 DPI
    dib.extend_from_slice(&(2835i32).to_le_bytes());
    dib.extend_from_slice(&0u32.to_le_bytes());
    dib.extend_from_slice(&0u32.to_le_bytes());

    // Pixels: bottom-up, RGBA → BGRA
    for y in (0..h).rev() {
        let src_start = (y * w * 4) as usize;
        let src_end = src_start + (w * 4) as usize;
        if src_end > rgba.len() {
            break;
        }
        let row = &rgba[src_start..src_end];
        for x in 0..w as usize {
            dib.push(row[x * 4 + 2]); // B
            dib.push(row[x * 4 + 1]); // G
            dib.push(row[x * 4]); // R
            dib.push(row[x * 4 + 3]); // A
        }
    }

    dib
}

/// Set CF_DIB data on the Windows clipboard (raw Win32, no file involved).
#[cfg(target_os = "windows")]
fn set_clipboard_dib(dib: &[u8]) {
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GHND};

    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return;
        }
        EmptyClipboard();
        let h = GlobalAlloc(GHND, dib.len());
        if !h.is_null() {
            let ptr = GlobalLock(h) as *mut u8;
            std::ptr::copy_nonoverlapping(dib.as_ptr(), ptr, dib.len());
            GlobalUnlock(h);
            SetClipboardData(8, h); // CF_DIB = 8
        }
        CloseClipboard();
    }
}
