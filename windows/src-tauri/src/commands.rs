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

mod history;
mod peers;
mod platform;
mod preview;
mod remote_pairing;
mod settings;
mod storage;
mod themes;

pub use history::*;
pub use peers::*;
pub use platform::*;
pub use preview::*;
pub use remote_pairing::*;
pub use settings::*;
pub use storage::*;
pub use themes::*;

#[cfg(test)]
use preview::{encode_preview_response, PREVIEW_RESPONSE_MAGIC, PREVIEW_RESPONSE_VERSION};

#[cfg(test)]
mod tests;
