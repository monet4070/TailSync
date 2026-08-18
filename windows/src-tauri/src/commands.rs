use crate::db;
use crate::network;
use crate::AppState;
use log::info;
use tauri::{command, AppHandle, Emitter, Manager, State};
use tauri_plugin_global_shortcut::GlobalShortcutExt;

const SYNC_STATE_CHANGED_EVENT: &str = "sync-state-changed";

fn emit_sync_state(app: &tauri::AppHandle, enabled: bool) {
    if let Err(error) = app.emit(
        SYNC_STATE_CHANGED_EVENT,
        serde_json::json!({ "enabled": enabled }),
    ) {
        log::debug!("Could not emit sync state change: {error}");
    }
}

pub(crate) async fn set_sync_enabled_for_app(
    app: &tauri::AppHandle,
    enabled: bool,
) -> Result<(), String> {
    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| "TailSync state is unavailable".to_string())?;
    state
        .settings
        .lock()
        .await
        .set_sync_enabled(enabled)
        .map_err(|error| error.to_string())?;
    emit_sync_state(app, enabled);
    Ok(())
}

pub(crate) async fn toggle_sync_for_app(app: &tauri::AppHandle) -> Result<bool, String> {
    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| "TailSync state is unavailable".to_string())?;
    let enabled = {
        let mut settings = state.settings.lock().await;
        let enabled = !settings.sync_enabled;
        settings
            .set_sync_enabled(enabled)
            .map_err(|error| error.to_string())?;
        enabled
    };
    emit_sync_state(app, enabled);
    Ok(enabled)
}

fn install_global_shortcuts(
    app: &tauri::AppHandle,
    sync_shortcut: &str,
    history_shortcut: &str,
) -> Result<(), String> {
    if !sync_shortcut.is_empty() && sync_shortcut == history_shortcut {
        return Err("The sync and history shortcuts must be different".to_string());
    }
    app.global_shortcut()
        .unregister_all()
        .map_err(|error| error.to_string())?;
    if !sync_shortcut.is_empty() {
        if let Err(error) =
            app.global_shortcut()
                .on_shortcut(sync_shortcut, |app, _shortcut, event| {
                    use tauri_plugin_global_shortcut::ShortcutState;
                    if event.state != ShortcutState::Pressed {
                        return;
                    }
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(error) = toggle_sync_for_app(&app).await {
                            log::warn!("Could not toggle sync from shortcut: {error}");
                        }
                    });
                })
        {
            return Err(error.to_string());
        }
    }
    if !history_shortcut.is_empty() {
        if let Err(error) =
            app.global_shortcut()
                .on_shortcut(history_shortcut, |app, _shortcut, event| {
                    use tauri_plugin_global_shortcut::ShortcutState;
                    if event.state != ShortcutState::Pressed {
                        return;
                    }
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(error) = open_history_window(app).await {
                            log::warn!("Could not open history from shortcut: {error}");
                        }
                    });
                })
        {
            let _ = app.global_shortcut().unregister_all();
            return Err(error.to_string());
        }
    }
    Ok(())
}

pub(crate) fn register_saved_shortcuts(
    app: &tauri::AppHandle,
    settings: &crate::crypto::Settings,
) -> Result<(), String> {
    install_global_shortcuts(app, &settings.sync_shortcut, &settings.history_shortcut)
}

/// Apply a shortcut change as a transaction: register the next shortcut first,
/// then persist it, restoring the previous shortcut if either step fails.
/// Returns the original failure, with any restore failure appended.
fn apply_shortcut_change<T, R, S>(
    previous: &T,
    next: &T,
    mut register: R,
    mut save: S,
) -> Result<(), String>
where
    T: PartialEq + ?Sized,
    R: FnMut(&T) -> Result<(), String>,
    S: FnMut() -> Result<(), String>,
{
    if next == previous {
        return register(next);
    }
    if let Err(error) = register(next) {
        return Err(rollback_shortcut(previous, &mut register, error));
    }
    if let Err(error) = save() {
        return Err(rollback_shortcut(previous, &mut register, error));
    }
    Ok(())
}

fn rollback_shortcut<T, R>(previous: &T, register: &mut R, original_error: String) -> String
where
    T: ?Sized,
    R: FnMut(&T) -> Result<(), String>,
{
    match register(previous) {
        Ok(()) => original_error,
        Err(restore_error) => {
            format!("{original_error}; could not restore the previous shortcut: {restore_error}")
        }
    }
}

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

/// Read the latest peer-health snapshot maintained by the background monitor.
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

/// Ask the background monitor to execute a health round immediately.
#[command]
pub async fn refresh_peers(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let mode = state.settings.lock().await.connection_mode.clone();
    network::request_peer_refresh(&mode).await?;
    get_peers(state).await
}

/// Measure latency over a peer's selected TailSync route.
#[command]
pub async fn test_connection(address: String) -> Result<serde_json::Value, String> {
    let address = address.trim();
    if address.is_empty() {
        return Err("Missing peer address".to_string());
    }
    match network::test_connection(address).await {
        Ok(route) => {
            network::record_address_test_success(address, route.latency_ms);
            Ok(serde_json::to_value(route).map_err(|error| error.to_string())?)
        }
        Err(error) => {
            network::record_address_test_failure(address);
            Err(error)
        }
    }
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

/// Get whether this device broadcasts clipboard changes and its configured shortcut.
#[command]
pub async fn get_sync_state(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let settings = state.settings.lock().await;
    Ok(serde_json::json!({
        "enabled": settings.sync_enabled,
        "shortcut": settings.sync_shortcut,
        "history_shortcut": settings.history_shortcut,
    }))
}

/// Enable or pause local clipboard broadcasting.
#[command]
pub async fn set_sync_enabled(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    set_sync_enabled_for_app(&app, enabled).await
}

#[command]
pub async fn toggle_sync(app: tauri::AppHandle) -> Result<bool, String> {
    toggle_sync_for_app(&app).await
}

#[command]
pub fn suspend_sync_shortcut(app: tauri::AppHandle) -> Result<(), String> {
    app.global_shortcut()
        .unregister_all()
        .map_err(|error| error.to_string())
}

#[command]
pub async fn resume_sync_shortcut(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let settings = state.settings.lock().await.clone();
    install_global_shortcuts(&app, &settings.sync_shortcut, &settings.history_shortcut)
}

#[command]
pub async fn set_sync_shortcut(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    shortcut: String,
) -> Result<(), String> {
    let shortcut = shortcut.trim().to_string();
    let mut settings = state.settings.lock().await;
    let previous = settings.clone();
    let mut next = previous.clone();
    next.sync_shortcut = shortcut;
    let register = |candidate: &crate::crypto::Settings| {
        install_global_shortcuts(&app, &candidate.sync_shortcut, &candidate.history_shortcut)
    };
    apply_shortcut_change(&previous, &next, register, || {
        next.save().map_err(|error| error.to_string())
    })?;
    *settings = next;
    Ok(())
}

#[command]
pub async fn set_history_shortcut(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    shortcut: String,
) -> Result<(), String> {
    let shortcut = shortcut.trim().to_string();
    let mut settings = state.settings.lock().await;
    let previous = settings.clone();
    let mut next = previous.clone();
    next.history_shortcut = shortcut;
    let register = |candidate: &crate::crypto::Settings| {
        install_global_shortcuts(&app, &candidate.sync_shortcut, &candidate.history_shortcut)
    };
    apply_shortcut_change(&previous, &next, register, || {
        next.save().map_err(|error| error.to_string())
    })?;
    *settings = next;
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
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    settings_json: String,
) -> Result<(), String> {
    let requested_settings: crate::crypto::Settings =
        serde_json::from_str(&settings_json).map_err(|e| e.to_string())?;
    let apply_shortcut_transaction =
        |previous: &crate::crypto::Settings, new_settings: &crate::crypto::Settings| {
            let register = |candidate: &crate::crypto::Settings| {
                install_global_shortcuts(
                    &app,
                    &candidate.sync_shortcut,
                    &candidate.history_shortcut,
                )
            };
            apply_shortcut_change(previous, new_settings, register, || {
                new_settings.save().map_err(|error| error.to_string())
            })
        };
    let outcome = crate::crypto::apply_settings_update(
        &state.settings,
        &state.db,
        requested_settings,
        &|settings: &crate::crypto::Settings| settings.save().map_err(|error| error.to_string()),
        Some(&apply_shortcut_transaction),
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

/// Open the history window
#[command]
pub async fn open_history_window(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;

    crate::window_lifecycle::mark_window_open(&app, crate::window_lifecycle::HISTORY_WINDOW_LABEL);

    // Check if window already exists
    if let Some(window) = app.get_webview_window(crate::window_lifecycle::HISTORY_WINDOW_LABEL) {
        window.show().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }

    // Create new history window
    let _window = tauri::WebviewWindowBuilder::new(
        &app,
        crate::window_lifecycle::HISTORY_WINDOW_LABEL,
        tauri::WebviewUrl::App("history.html".into()),
    )
    .title("TailSync - History")
    .inner_size(400.0, 600.0)
    .decorations(false) // Borderless, per user preference
    .resizable(true)
    .center()
    .build()
    .map_err(|e| e.to_string())?;

    Ok(())
}

/// Open the settings window
#[command]
pub async fn open_settings_window(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;

    crate::window_lifecycle::mark_window_open(&app, crate::window_lifecycle::SETTINGS_WINDOW_LABEL);

    if let Some(window) = app.get_webview_window(crate::window_lifecycle::SETTINGS_WINDOW_LABEL) {
        window.show().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }

    let _window = tauri::WebviewWindowBuilder::new(
        &app,
        crate::window_lifecycle::SETTINGS_WINDOW_LABEL,
        tauri::WebviewUrl::App("settings.html".into()),
    )
    .title("TailSync - Settings")
    .inner_size(520.0, 700.0)
    .decorations(false)
    .min_inner_size(440.0, 560.0)
    .resizable(true)
    .center()
    .build()
    .map_err(|e| e.to_string())?;

    Ok(())
}

#[command]
pub fn close_history_window(app: tauri::AppHandle) -> Result<(), String> {
    crate::window_lifecycle::hide_then_release_window(
        app,
        crate::window_lifecycle::HISTORY_WINDOW_LABEL,
    )
}

#[command]
pub fn close_settings_window(app: tauri::AppHandle) -> Result<(), String> {
    crate::window_lifecycle::hide_then_release_window(
        app,
        crate::window_lifecycle::SETTINGS_WINDOW_LABEL,
    )
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

const PREVIEW_RESPONSE_MAGIC: &[u8; 4] = b"TSPV";
const PREVIEW_RESPONSE_VERSION: u8 = 1;

#[derive(serde::Serialize)]
struct PreviewResponseMetadata {
    entry_id: i64,
    kind: String,
    name: String,
    size_bytes: u64,
    width: Option<u32>,
    height: Option<u32>,
    batch: Option<db::PreviewBatchNavigation>,
}

fn preview_payload_error(entry_id: i64, message: impl Into<String>) -> db::PreviewErrorInfo {
    db::PreviewErrorInfo::payload_unavailable(entry_id, message)
}

/// Encode preview metadata and bytes into one raw IPC response.
///
/// `tauri::ipc::Response` can return an `ArrayBuffer` without base64, but it
/// cannot carry a JSON object alongside that buffer. The response therefore
/// uses a small versioned envelope:
///
/// `TSPV | version:u8 | metadata_length:u32(le) | metadata_json | payload`
///
/// Image payloads are decoded from the stored `PackedImage` representation to
/// raw RGBA bytes; their dimensions are included in the metadata.
fn encode_preview_response(
    metadata: db::PreviewMetadata,
    payload: db::PreviewPayload,
) -> Result<Vec<u8>, db::PreviewErrorInfo> {
    let entry_id = metadata.entry_id;
    let (width, height, data) = if payload.kind == "image" {
        let image = crate::protocol::PackedImage::try_from(payload.data.as_slice())
            .map_err(|error| preview_payload_error(entry_id, error.to_string()))?;
        (Some(image.width), Some(image.height), image.rgba.to_vec())
    } else {
        (None, None, payload.data)
    };
    let metadata = PreviewResponseMetadata {
        entry_id,
        kind: payload.kind,
        name: payload.name,
        size_bytes: u64::try_from(data.len()).unwrap_or(u64::MAX),
        width,
        height,
        batch: metadata.batch,
    };
    let metadata = serde_json::to_vec(&metadata)
        .map_err(|error| preview_payload_error(entry_id, error.to_string()))?;
    let metadata_len = u32::try_from(metadata.len())
        .map_err(|_| preview_payload_error(entry_id, "preview metadata is too large"))?;
    let capacity = 9_usize
        .checked_add(metadata.len())
        .and_then(|length| length.checked_add(data.len()))
        .ok_or_else(|| preview_payload_error(entry_id, "preview response is too large"))?;

    let mut response = Vec::with_capacity(capacity);
    response.extend_from_slice(PREVIEW_RESPONSE_MAGIC);
    response.push(PREVIEW_RESPONSE_VERSION);
    response.extend_from_slice(&metadata_len.to_le_bytes());
    response.extend_from_slice(&metadata);
    response.extend_from_slice(&data);
    Ok(response)
}

/// Return a bounded history preview as a raw `ArrayBuffer` to the frontend.
#[command]
pub async fn get_preview(
    state: State<'_, AppState>,
    id: i64,
    batch_id: Option<String>,
) -> Result<tauri::ipc::Response, db::PreviewErrorInfo> {
    let db = state.db.lock().await;
    if let Some(batch_id) = batch_id.as_deref() {
        db.get_preview_batch_navigation(batch_id, id)
            .map_err(db::PreviewErrorInfo::from)?;
    }
    let preview_id = id;
    let metadata = db
        .get_preview_metadata(preview_id)
        .map_err(db::PreviewErrorInfo::from)?;
    let payload = db
        .get_preview_payload(preview_id)
        .map_err(db::PreviewErrorInfo::from)?;
    Ok(tauri::ipc::Response::new(encode_preview_response(
        metadata, payload,
    )?))
}

// V2 package boundary. These commands deliberately use the shared Core model
// rather than re-validating JSON in a platform renderer.
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
            "windows",
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
    app: AppHandle,
    theme_id: String,
    storage_handle: Option<String>,
) -> Result<(), tailsync_core::themes_v2::ThemeError> {
    if let Some(handle) = storage_handle {
        tailsync_core::themes_v2::delete_theme_by_handle_for_theme(&handle, &theme_id)?;
    } else {
        tailsync_core::themes_v2::delete_theme(&theme_id)?;
    }
    let _ = app.emit(
        "theme_changed",
        tailsync_core::themes_v2::get_local_theme_settings(),
    );
    Ok(())
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
    app: AppHandle,
    settings: tailsync_core::themes_v2::LocalThemeSettings,
) -> Result<(), tailsync_core::themes_v2::ThemeError> {
    tailsync_core::themes_v2::set_local_theme_settings(settings.clone())?;
    // Theme selection is deliberately local, but every open webview must see
    // it immediately.  Do not use the synchronised AppSettings channel here.
    let _ = app.emit("theme_changed", settings);
    Ok(())
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

/// Raw binary IPC; MIME and dimensions are supplied by the descriptor's asset
/// metadata, so no image is ever expanded into a Base64 JSON listing.
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

#[derive(serde::Serialize)]
pub struct RuntimeSnapshot {
    revision: u64,
    history_version: u64,
    progress: Option<crate::api::FileProgress>,
    sync_warning: Option<tailsync_core::sync_warning::SyncWarning>,
}

/// Wait until history or transfer state changes, then return one coherent
/// snapshot. The bounded timeout lets the UI recover if a notification is
/// missed without reverting to high-frequency polling.
#[command]
pub async fn wait_runtime_snapshot(
    since_revision: u64,
    wait_ms: Option<u64>,
) -> Result<RuntimeSnapshot, String> {
    let wait_ms = wait_ms.unwrap_or(2_500).clamp(50, 15_000);
    let _ = crate::api::wait_for_runtime_revision(
        since_revision,
        std::time::Duration::from_millis(wait_ms),
    )
    .await;
    let revision = crate::api::get_runtime_revision();
    Ok(RuntimeSnapshot {
        revision,
        history_version: crate::api::get_clipboard_version(),
        progress: crate::api::get_file_progress(),
        sync_warning: tailsync_core::sync_warning::take(),
    })
}

#[command]
pub async fn get_sync_warning() -> Result<Option<tailsync_core::sync_warning::SyncWarning>, String>
{
    Ok(tailsync_core::sync_warning::take())
}

#[derive(serde::Serialize)]
pub struct UpdateStatus {
    current_version: &'static str,
    updates_enabled: bool,
}

/// Report updater availability separately from checking the network so a
/// development build can explain why updates are unavailable in the UI.
#[command]
pub async fn get_update_status() -> Result<UpdateStatus, String> {
    Ok(UpdateStatus {
        current_version: env!("CARGO_PKG_VERSION"),
        updates_enabled: crate::updates::public_key_configured(),
    })
}

#[command]
pub async fn check_for_update(
    app: AppHandle,
) -> Result<Option<crate::updates::UpdateInfo>, String> {
    crate::updates::check_for_update(&app).await
}

#[command]
pub async fn install_update(app: AppHandle) -> Result<bool, String> {
    crate::updates::install_available_update(&app).await
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

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_preview_response(response: &[u8]) -> (serde_json::Value, &[u8]) {
        assert_eq!(&response[..4], PREVIEW_RESPONSE_MAGIC);
        assert_eq!(response[4], PREVIEW_RESPONSE_VERSION);
        let metadata_len = u32::from_le_bytes(response[5..9].try_into().unwrap()) as usize;
        let payload_offset = 9 + metadata_len;
        let metadata = serde_json::from_slice(&response[9..payload_offset]).unwrap();
        (metadata, &response[payload_offset..])
    }

    #[test]
    fn preview_response_keeps_text_metadata_and_raw_bytes() {
        let response = encode_preview_response(
            db::PreviewMetadata {
                entry_id: 17,
                kind: db::PreviewKind::Text,
                name: "text.txt".to_string(),
                size_bytes: 13,
                batch: None,
            },
            db::PreviewPayload {
                kind: "text".to_string(),
                name: "text.txt".to_string(),
                size_bytes: 13,
                data: b"preview bytes".to_vec(),
            },
        )
        .unwrap();
        let (metadata, data) = decode_preview_response(&response);

        assert_eq!(metadata["kind"], "text");
        assert_eq!(metadata["entry_id"], 17);
        assert_eq!(metadata["name"], "text.txt");
        assert_eq!(metadata["size_bytes"], 13);
        assert!(metadata["width"].is_null());
        assert!(metadata["height"].is_null());
        assert!(metadata["batch"].is_null());
        assert_eq!(data, b"preview bytes");
    }

    #[test]
    fn preview_response_decodes_images_to_rgba_with_dimensions() {
        let rgba = [1, 2, 3, 4, 5, 6, 7, 8];
        let mut packed = Vec::new();
        packed.extend_from_slice(&2_u32.to_le_bytes());
        packed.extend_from_slice(&1_u32.to_le_bytes());
        packed.extend_from_slice(&rgba);
        let response = encode_preview_response(
            db::PreviewMetadata {
                entry_id: 23,
                kind: db::PreviewKind::Image,
                name: "image".to_string(),
                size_bytes: packed.len() as u64,
                batch: None,
            },
            db::PreviewPayload {
                kind: "image".to_string(),
                name: "image".to_string(),
                size_bytes: packed.len() as u64,
                data: packed,
            },
        )
        .unwrap();
        let (metadata, data) = decode_preview_response(&response);

        assert_eq!(metadata["kind"], "image");
        assert_eq!(metadata["size_bytes"], rgba.len());
        assert_eq!(metadata["width"], 2);
        assert_eq!(metadata["height"], 1);
        assert_eq!(data, rgba);
    }

    #[test]
    fn shortcut_transaction_registers_new_then_persists() {
        let calls = std::cell::RefCell::new(Vec::<String>::new());
        let register = |next: &str| {
            calls.borrow_mut().push(format!("register:{next}"));
            Ok(())
        };
        let saved = std::cell::Cell::new(false);
        let save = || {
            saved.set(true);
            Ok(())
        };
        assert!(apply_shortcut_change("old", "new", register, save).is_ok());
        assert!(saved.get());
        assert_eq!(*calls.borrow(), vec!["register:new"]);
    }

    #[test]
    fn shortcut_transaction_register_failure_restores_previous() {
        let calls = std::cell::RefCell::new(Vec::<String>::new());
        let register = |next: &str| {
            calls.borrow_mut().push(format!("register:{next}"));
            if next == "taken" {
                Err("shortcut is taken".to_string())
            } else {
                Ok(())
            }
        };
        let save = || panic!("save must not run after register failure");
        let error = apply_shortcut_change("old", "taken", register, save).unwrap_err();
        assert_eq!(error, "shortcut is taken");
        assert_eq!(*calls.borrow(), vec!["register:taken", "register:old"]);
    }

    #[test]
    fn shortcut_transaction_save_failure_restores_previous() {
        let calls = std::cell::RefCell::new(Vec::<String>::new());
        let register = |next: &str| {
            calls.borrow_mut().push(format!("register:{next}"));
            Ok(())
        };
        let save = || Err("disk is full".to_string());
        let error = apply_shortcut_change("old", "new", register, save).unwrap_err();
        assert_eq!(error, "disk is full");
        assert_eq!(*calls.borrow(), vec!["register:new", "register:old"]);
    }

    #[test]
    fn shortcut_transaction_restore_failure_mentions_both_errors() {
        let calls = std::cell::RefCell::new(Vec::<String>::new());
        let register = |next: &str| {
            calls.borrow_mut().push(format!("register:{next}"));
            if next == "new" {
                Ok(())
            } else {
                Err("old shortcut no longer available".to_string())
            }
        };
        let save = || Err("disk is full".to_string());
        let error = apply_shortcut_change("old", "new", register, save).unwrap_err();
        assert!(error.contains("disk is full"), "got: {error}");
        assert!(
            error.contains("old shortcut no longer available"),
            "got: {error}"
        );
        assert_eq!(*calls.borrow(), vec!["register:new", "register:old"]);
    }

    #[test]
    fn shortcut_transaction_reregisters_unchanged_shortcut_without_saving() {
        let calls = std::cell::RefCell::new(Vec::<String>::new());
        let register = |next: &str| {
            calls.borrow_mut().push(format!("register:{next}"));
            Ok(())
        };
        let save = || {
            panic!("save must not run for an unchanged shortcut");
        };
        assert!(apply_shortcut_change("same", "same", register, save).is_ok());
        assert_eq!(*calls.borrow(), vec!["register:same"]);
    }
}
