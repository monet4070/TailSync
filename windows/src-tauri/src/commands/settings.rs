use super::*;

fn parse_settings_json(settings_json: &str) -> Result<crate::crypto::Settings, CommandError> {
    serde_json::from_str(settings_json).map_err(|_| {
        CommandError::code(tailsync_runtime::contracts::StableErrorCode::InvalidArgument)
    })
}

/// Get whether this device broadcasts clipboard changes and its configured shortcut.
#[command]
pub async fn get_sync_state(state: State<'_, AppState>) -> Result<serde_json::Value, CommandError> {
    let settings = state.settings.lock().await;
    Ok(serde_json::json!({
        "enabled": settings.sync_enabled,
        "shortcut": settings.sync_shortcut,
        "history_shortcut": settings.history_shortcut,
    }))
}

/// Enable or pause local clipboard broadcasting.
#[command]
pub async fn set_sync_enabled(app: tauri::AppHandle, enabled: bool) -> Result<(), CommandError> {
    set_sync_enabled_for_app(&app, enabled)
        .await
        .map_err(Into::into)
}

#[command]
pub async fn toggle_sync(app: tauri::AppHandle) -> Result<bool, CommandError> {
    toggle_sync_for_app(&app).await.map_err(Into::into)
}

#[command]
pub fn suspend_sync_shortcut(app: tauri::AppHandle) -> Result<(), CommandError> {
    app.global_shortcut()
        .unregister_all()
        .map_err(|error| CommandError::from(error.to_string()))
}

#[command]
pub async fn resume_sync_shortcut(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    let settings = state.settings.lock().await.clone();
    install_global_shortcuts(&app, &settings.sync_shortcut, &settings.history_shortcut)
        .map_err(Into::into)
}

#[command]
pub async fn set_sync_shortcut(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    shortcut: String,
) -> Result<(), CommandError> {
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
) -> Result<(), CommandError> {
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
pub async fn get_settings(
    state: State<'_, AppState>,
) -> Result<crate::crypto::Settings, CommandError> {
    let settings = state.settings.lock().await;
    Ok(settings.clone())
}

/// Update settings
#[command]
pub async fn update_settings(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    settings_json: String,
) -> Result<(), CommandError> {
    let requested_settings = parse_settings_json(&settings_json)?;
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
pub async fn open_history_window(app: tauri::AppHandle) -> Result<(), CommandError> {
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

/// Toggle the focused history window from the global shortcut. An unfocused
/// or hidden history window is restored and focused; a focused visible one is
/// hidden and enters the normal transient-window release path.
pub(crate) async fn toggle_history_window(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;

    if let Some(window) = app.get_webview_window(crate::window_lifecycle::HISTORY_WINDOW_LABEL) {
        let visible = window.is_visible().map_err(|error| error.to_string())?;
        let focused = window.is_focused().map_err(|error| error.to_string())?;
        if visible && focused {
            return crate::window_lifecycle::hide_then_release_window(
                app,
                crate::window_lifecycle::HISTORY_WINDOW_LABEL,
            );
        }
    }
    open_history_window(app)
        .await
        .map_err(|error| error.to_string())
}

/// Open the favorites window that shares the history row interaction model.
#[command]
pub async fn open_favorites_window(app: tauri::AppHandle) -> Result<(), CommandError> {
    use tauri::Manager;

    crate::window_lifecycle::mark_window_open(
        &app,
        crate::window_lifecycle::FAVORITES_WINDOW_LABEL,
    );

    if let Some(window) = app.get_webview_window(crate::window_lifecycle::FAVORITES_WINDOW_LABEL) {
        crate::window_lifecycle::restore_and_focus_window(&window)?;
        return Ok(());
    }

    let window =
        crate::window_lifecycle::configure_transparent_window(tauri::WebviewWindowBuilder::new(
            &app,
            crate::window_lifecycle::FAVORITES_WINDOW_LABEL,
            tauri::WebviewUrl::App("favorites.html".into()),
        ))
        .title("TailSync - Favorites")
        .inner_size(400.0, 600.0)
        .decorations(false)
        .shadow(false)
        .resizable(true)
        .visible(false)
        .center()
        .build()
        .map_err(|error| error.to_string())?;

    crate::window_lifecycle::restore_and_focus_window(&window)?;
    Ok(())
}

/// Open the settings window
#[command]
pub async fn open_settings_window(app: tauri::AppHandle) -> Result<(), CommandError> {
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
pub fn close_history_window(app: tauri::AppHandle) -> Result<(), CommandError> {
    crate::window_lifecycle::hide_then_release_window(
        app,
        crate::window_lifecycle::HISTORY_WINDOW_LABEL,
    )
    .map_err(Into::into)
}

#[command]
pub fn close_favorites_window(app: tauri::AppHandle) -> Result<(), CommandError> {
    crate::window_lifecycle::hide_then_release_window(
        app,
        crate::window_lifecycle::FAVORITES_WINDOW_LABEL,
    )
    .map_err(Into::into)
}

#[command]
pub fn close_settings_window(app: tauri::AppHandle) -> Result<(), CommandError> {
    crate::window_lifecycle::hide_then_release_window(
        app,
        crate::window_lifecycle::SETTINGS_WINDOW_LABEL,
    )
    .map_err(Into::into)
}

#[cfg(test)]
mod stable_input_error_tests {
    #[test]
    fn malformed_settings_are_invalid_arguments() {
        let error = super::parse_settings_json("{").unwrap_err();
        assert_eq!(
            error.envelope().code,
            tailsync_runtime::contracts::StableErrorCode::InvalidArgument
        );
    }
}
