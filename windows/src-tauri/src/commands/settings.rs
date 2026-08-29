use super::*;

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
        crate::window_lifecycle::restore_and_focus_window(&window)?;
        return Ok(());
    }

    // Create new history window
    let window =
        crate::window_lifecycle::configure_transparent_window(tauri::WebviewWindowBuilder::new(
            &app,
            crate::window_lifecycle::HISTORY_WINDOW_LABEL,
            tauri::WebviewUrl::App("history.html".into()),
        ))
        .title("TailSync - History")
        .inner_size(400.0, 600.0)
        .decorations(false) // Borderless, per user preference
        // Let the rounded `.app` surface own the window shape. Tauri otherwise
        // keeps the Windows undecorated shadow, which paints a square/white edge
        // around transparent corners.
        .shadow(false)
        .resizable(true)
        .visible(false)
        .center()
        .build()
        .map_err(|e| e.to_string())?;

    crate::window_lifecycle::restore_and_focus_window(&window)?;

    Ok(())
}

/// Open the settings window
#[command]
pub async fn open_settings_window(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;

    crate::window_lifecycle::mark_window_open(&app, crate::window_lifecycle::SETTINGS_WINDOW_LABEL);

    if let Some(window) = app.get_webview_window(crate::window_lifecycle::SETTINGS_WINDOW_LABEL) {
        crate::window_lifecycle::restore_and_focus_window(&window)?;
        return Ok(());
    }

    let window =
        crate::window_lifecycle::configure_transparent_window(tauri::WebviewWindowBuilder::new(
            &app,
            crate::window_lifecycle::SETTINGS_WINDOW_LABEL,
            tauri::WebviewUrl::App("settings.html".into()),
        ))
        .title("TailSync - Settings")
        .inner_size(520.0, 700.0)
        .decorations(false)
        .shadow(false)
        .min_inner_size(440.0, 560.0)
        .resizable(true)
        .center()
        .visible(false)
        .build()
        .map_err(|e| e.to_string())?;

    crate::window_lifecycle::restore_and_focus_window(&window)?;

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
