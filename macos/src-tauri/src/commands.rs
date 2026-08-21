use crate::db;
use crate::network;
use crate::AppState;
use log::info;
use tauri::{command, AppHandle, Manager, State};

#[derive(serde::Serialize)]
pub struct HistoryPage {
    pub entries: Vec<db::HistoryEntry>,
    pub total: Option<usize>,
    pub has_more: bool,
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
    let page = db
        .get_page_filtered(
            keyword.as_deref(),
            category.as_deref(),
            start_time.as_deref(),
            end_time.as_deref(),
            limit.unwrap_or(50),
            offset.unwrap_or(0),
        )
        .map_err(|e| e.to_string())?;
    Ok(HistoryPage {
        entries: page.entries,
        total: page.total,
        has_more: page.has_more,
    })
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
    db.delete(id).map_err(|e| e.to_string())?;
    crate::api::bump_clipboard_version();
    Ok(())
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
                    state
                        .sync_engine
                        .lock()
                        .await
                        .remove_image_shadow_filter(data);
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
                {
                    state
                        .sync_engine
                        .lock()
                        .await
                        .remove_image_shadow_filter(data);
                    return Err("write_image failed and no fallback on this platform".into());
                }
            }
        }
    } else if entry_type == "file" {
        if let Some(path) = file_path {
            crate::api::restore_file_path_to_clipboard(
                &path,
                file_name.as_deref().unwrap_or("restored_file"),
            )?;
        } else {
            crate::api::restore_file_to_clipboard(
                data.as_deref().ok_or("File history data is unavailable")?,
                file_name.as_deref().unwrap_or("restored_file"),
            )?;
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

        if let Err(error) = clipboard.write_text(text.clone()) {
            state.sync_engine.lock().await.remove_shadow_filter(&text);
            return Err(format!("Clipboard text write failed: {}", error));
        }

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
    let fingerprint = crate::identity::trust_peer(
        &state.identity,
        &state.settings,
        &|settings: &crate::crypto::Settings| settings.save().map_err(|error| error.to_string()),
        &hostname,
        &public_key,
        address.as_deref(),
    )
    .await
    .map_err(|failure| match failure {
        crate::identity::TrustPeerFailure::InvalidHostname => "Invalid peer hostname".to_string(),
        crate::identity::TrustPeerFailure::SelfPairing => {
            "Cannot pair this device with itself".to_string()
        }
        crate::identity::TrustPeerFailure::Key(error)
        | crate::identity::TrustPeerFailure::Interface(error)
        | crate::identity::TrustPeerFailure::Trust(error) => error,
    })?;
    state.pool.lock().await.disconnect_hostname(hostname.trim());
    crate::network::clear_protocol_compatibility_error(hostname.trim());
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
    crate::network::clear_protocol_compatibility_error(hostname);
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
    state
        .pairing
        .confirm()
        .await
        .map_err(|error| error.to_string())
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
    let requested_settings: crate::crypto::Settings =
        serde_json::from_str(&settings_json).map_err(|e| e.to_string())?;
    let outcome = crate::crypto::apply_settings_update(
        &state.settings,
        &state.db,
        requested_settings,
        &|settings: &crate::crypto::Settings| settings.save().map_err(|error| error.to_string()),
        None,
    )
    .await
    .map_err(|error| error.to_string())?;
    if outcome.mode_changed {
        state.pool.lock().await.disconnect_all();
        network::clear_peer_cache().await;
        network::refresh_iroh_for_mode(&outcome.connection_mode).await;
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
    let (tw, th, thumb) = crate::api::thumbnail_rgba(image, crate::api::THUMBNAIL_MAX_SIDE);
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&thumb);
    Ok(serde_json::json!({
        "id": id,
        "thumbnail_b64": b64,
        "thumbnail_width": tw,
        "thumbnail_height": th,
    }))
}

fn v2_package(path: &str) -> Result<Vec<u8>, Box<tailsync_core::themes_v2::ThemeError>> {
    if !path.ends_with(".tailsync-theme") {
        return Err(Box::new(tailsync_core::themes_v2::ThemeError {
            code: "THEME_EXTENSION".into(),
            message: "theme package must end in .tailsync-theme".into(),
            json_pointer: "/path".into(),
            platforms: vec!["windows".into(), "macos".into()],
            severity: "error".into(),
            recoverable: true,
            fallback_applied: false,
        }));
    }
    std::fs::read(path).map_err(|e| {
        Box::new(tailsync_core::themes_v2::ThemeError {
            code: "THEME_IO".into(),
            message: e.to_string(),
            json_pointer: "/path".into(),
            platforms: vec!["windows".into(), "macos".into()],
            severity: "error".into(),
            recoverable: true,
            fallback_applied: false,
        })
    })
}
#[command]
pub async fn validate_theme(
    path: String,
    mode: String,
    high_contrast: bool,
) -> tailsync_core::themes_v2::ThemeValidation {
    match v2_package(&path) {
        Ok(bytes) => tailsync_core::themes_v2::validate_theme_for_platform(
            &bytes,
            &mode,
            "macos",
            high_contrast,
        ),
        Err(error) => tailsync_core::themes_v2::ThemeValidation {
            valid: false,
            digest: None,
            candidate_version: None,
            preview: None,
            diagnostics: vec![*error],
            assets: vec![],
            compatible: false,
        },
    }
}
#[command]
pub async fn install_theme(
    path: String,
    expected_digest: String,
) -> Result<tailsync_core::themes_v2::ThemeDescriptor, tailsync_core::themes_v2::ThemeError> {
    let package = v2_package(&path).map_err(|error| *error)?;
    tailsync_core::themes_v2::install_theme(&package, &expected_digest)
}
#[command]
pub async fn update_theme(
    path: String,
    expected_digest: String,
    options: tailsync_core::themes_v2::UpdateThemeOptions,
) -> Result<tailsync_core::themes_v2::ThemeDescriptor, tailsync_core::themes_v2::ThemeError> {
    let package = v2_package(&path).map_err(|error| *error)?;
    tailsync_core::themes_v2::update_theme(&package, &expected_digest, options)
}
#[command]
pub async fn rollback_theme(
    theme_id: String,
) -> Result<tailsync_core::themes_v2::ThemeDescriptor, tailsync_core::themes_v2::ThemeError> {
    tailsync_core::themes_v2::rollback_theme(&theme_id)
}
#[command]
pub async fn delete_theme_v2(
    theme_id: Option<String>,
    storage_handle: Option<String>,
) -> Result<(), tailsync_core::themes_v2::ThemeError> {
    if let Some(handle) = storage_handle {
        tailsync_core::themes_v2::delete_theme_by_handle_for_theme(
            &handle,
            theme_id.as_deref().unwrap_or(""),
        )
    } else if let Some(id) = theme_id {
        tailsync_core::themes_v2::delete_theme(&id)
    } else {
        Err(tailsync_core::themes_v2::ThemeError {
            code: "THEME_ID".into(),
            message: "missing theme_id or storage_handle".into(),
            json_pointer: "".into(),
            platforms: vec!["macos".into()],
            severity: "error".into(),
            recoverable: true,
            fallback_applied: false,
        })
    }
}
#[command]
pub async fn list_themes_v2() -> Vec<tailsync_core::themes_v2::ThemeDescriptor> {
    tailsync_core::themes_v2::list_themes_v2()
}
#[command]
pub async fn get_local_theme_settings() -> tailsync_core::themes_v2::LocalThemeSettings {
    tailsync_core::themes_v2::get_local_theme_settings()
}
#[command]
pub async fn set_local_theme_settings(
    settings: tailsync_core::themes_v2::LocalThemeSettings,
) -> Result<(), tailsync_core::themes_v2::ThemeError> {
    tailsync_core::themes_v2::set_local_theme_settings(settings)
}
#[command]
pub async fn resolve_theme(
    theme_id: String,
    mode: String,
    platform: String,
    high_contrast: bool,
) -> Result<tailsync_core::themes_v2::ResolvedTheme, tailsync_core::themes_v2::ThemeError> {
    tailsync_core::themes_v2::resolve_theme(&theme_id, &mode, &platform, high_contrast)
}

#[command]
pub async fn get_theme_asset(
    theme_id: String,
    digest: String,
    asset_key: String,
) -> Result<tauri::ipc::Response, tailsync_core::themes_v2::ThemeError> {
    let (_mime, bytes) = tailsync_core::themes_v2::get_theme_asset(&theme_id, &digest, &asset_key)?;
    Ok(tauri::ipc::Response::new(bytes))
}

#[command]
pub async fn get_theme_asset_slot(
    theme_id: String,
    digest: String,
    slot: String,
) -> Result<tauri::ipc::Response, tailsync_core::themes_v2::ThemeError> {
    let (_descriptor, bytes) =
        tailsync_core::themes_v2::get_theme_asset_slot(&theme_id, &digest, &slot)?;
    Ok(tauri::ipc::Response::new(bytes))
}

#[command]
pub async fn preview_theme_asset_slot(
    path: String,
    digest: String,
    slot: String,
) -> Result<tauri::ipc::Response, tailsync_core::themes_v2::ThemeError> {
    let bytes = v2_package(&path).map_err(|error| *error)?;
    let (_descriptor, asset) =
        tailsync_core::themes_v2::get_theme_asset_slot_from_package(&bytes, &digest, &slot)?;
    Ok(tauri::ipc::Response::new(asset))
}

/// Get current file transfer progress (for progress bar)
#[command]
pub async fn get_file_progress() -> Result<serde_json::Value, String> {
    let info = crate::api::get_file_progress();
    info.map_or_else(
        || Ok(serde_json::json!({"active": false})),
        |progress| serde_json::to_value(progress).map_err(|error| error.to_string()),
    )
}

#[command]
pub async fn cancel_file_batch(state: State<'_, AppState>, batch_id: String) -> Result<(), String> {
    let id = crate::protocol::TransferId::from_hex(&batch_id)?;
    cancel_file_batch_impl(&state.sync_engine, &state.pool, &state.settings, id).await;
    Ok(())
}

pub(crate) async fn cancel_file_batch_impl(
    sync_engine: &std::sync::Arc<tokio::sync::Mutex<crate::sync::SyncEngine>>,
    pool: &std::sync::Arc<tokio::sync::Mutex<network::ConnectionPool>>,
    settings: &std::sync::Arc<tokio::sync::Mutex<crate::crypto::Settings>>,
    batch_id: crate::protocol::TransferId,
) {
    let batch_id_hex = batch_id.as_hex();
    let source = sync_engine
        .lock()
        .await
        .cancel_file_batch_local(batch_id)
        .await;
    crate::api::clear_file_progress_scope(Some(&batch_id_hex), None);
    if let Some(source) = source {
        if let Err(error) = network::send_file_batch_cancel(pool, settings, &source, batch_id).await
        {
            log::warn!("Could not notify {source} that file batch was cancelled: {error}");
        }
    } else {
        crate::api::request_file_batch_cancel(&batch_id_hex);
    }
}

#[command]
pub async fn get_storage_status(state: State<'_, AppState>) -> Result<db::StorageStatus, String> {
    Ok(state.db.lock().await.storage_status())
}

#[command]
pub async fn change_storage_location(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    parent: String,
) -> Result<db::StorageMigrationResult, String> {
    use tauri_plugin_notification::NotificationExt;
    let parent = std::path::PathBuf::from(parent);
    let notifications_enabled = state.settings.lock().await.notifications_enabled;
    let show = || {
        if notifications_enabled {
            let _ = app
                .notification()
                .builder()
                .title("TailSync")
                .body("File transfers finished. Moving TailSync data now.")
                .show();
        }
    };
    let notify: Option<&(dyn Fn() + Send + Sync)> = Some(&show);
    db::migrate_storage_with_rollback(
        &state.db,
        &state.settings,
        &parent,
        db::StorageMigrationHooks {
            wait_timeout: std::time::Duration::from_secs(60),
            has_active_transfers: &crate::api::has_active_file_progress,
            notify,
            persist_settings: &|settings: &crate::crypto::Settings| {
                settings.save().map_err(|error| error.to_string())
            },
        },
    )
    .await
    .map_err(|failure| match failure {
        db::StorageMigrationFailure::TimedOutWaitingForTransfers => {
            "Timed out waiting for active file transfers to finish".to_string()
        }
        db::StorageMigrationFailure::Migrate(error) => error,
        db::StorageMigrationFailure::SaveFailedAfterRollback { save_error } => format!(
            "Could not save the new storage location; TailSync returned to the old location: {save_error}"
        ),
        db::StorageMigrationFailure::RollbackAlsoFailed { save_error, rollback_error } => {
            format!(
                "Could not save the new storage location ({save_error}); rollback also failed: {rollback_error}"
            )
        }
    })
}

#[command]
pub async fn set_history_pinned(
    state: State<'_, AppState>,
    id: i64,
    pinned: bool,
) -> Result<(), String> {
    state
        .db
        .lock()
        .await
        .set_pinned(id, pinned)
        .map_err(|error| error.to_string())?;
    crate::api::bump_clipboard_version();
    Ok(())
}

#[command]
pub async fn delete_old_storage(path: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        db::delete_old_storage(std::path::Path::new(&path)).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[command]
pub async fn restore_file_batch(
    state: State<'_, AppState>,
    batch_id: String,
) -> Result<(), String> {
    let paths = materialize_file_batch_paths(state.db.clone(), batch_id).await?;
    crate::clipboard_file::write_clipboard_files(&paths)?;
    crate::api::bump_clipboard_version();
    Ok(())
}

pub(crate) async fn materialize_file_batch_paths(
    database: std::sync::Arc<tokio::sync::Mutex<db::HistoryDB>>,
    batch_id: String,
) -> Result<Vec<std::path::PathBuf>, String> {
    tokio::task::spawn_blocking(move || {
        database
            .blocking_lock()
            .materialize_file_batch(&batch_id)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Get current clipboard version (for polling-based refresh)
#[command]
pub async fn get_version() -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "version": crate::api::get_clipboard_version()
    }))
}

#[command]
pub async fn get_sync_warning() -> Result<Option<tailsync_core::sync_warning::SyncWarning>, String>
{
    Ok(tailsync_core::sync_warning::take())
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
