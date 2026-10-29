//! The only Windows system-notification emitter. Page notices use the runtime feed separately.

pub fn show(app: &tauri::AppHandle, key: &str, body: &str, passive: bool) {
    #[cfg(target_os = "windows")]
    if let Err(error) = register_identity(app) {
        log::warn!("Native notification identity unavailable key={key}: {error}");
        return;
    }
    #[cfg(target_os = "windows")]
    if let Err(error) = native::show(&app.config().identifier, key, body, passive, {
        let app = app.clone();
        move || {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = crate::commands::open_history_window(app).await {
                    log::warn!("Could not open history from notification: {error}");
                }
            });
        }
    }) {
        // OS boundary: acceptance and asynchronous failure are observable; never replay a committed transfer.
        log::warn!("Native notification submission failed key={key}: {error}");
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app, body, passive);
        log::warn!("Windows native notifications are unavailable on this host key={key}");
    }
}

#[cfg(target_os = "windows")]
pub fn register_identity(app: &tauri::AppHandle) -> Result<(), String> {
    static REGISTRATION: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
    // Registry/filesystem boundary: initialize once per process; a failure disables native submission without retrying.
    REGISTRATION
        .get_or_init(|| register_identity_once(app))
        .clone()
}

#[cfg(target_os = "windows")]
fn register_identity_once(app: &tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;
    let icon = app
        .path()
        .app_local_data_dir()
        .map_err(|error| error.to_string())?
        .join("notification-icon.png");
    std::fs::create_dir_all(icon.parent().expect("app-local directory"))
        .map_err(|error| error.to_string())?;
    std::fs::write(&icon, include_bytes!("../icons/128x128.png"))
        .map_err(|error| error.to_string())?;
    native::register_identity(&app.config().identifier, &icon)
}

#[cfg(target_os = "windows")]
mod native;
