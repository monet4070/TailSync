//! Start the guided overlay in the real connections window.

#[path = "onboarding_state.rs"]
mod state;

fn markers() -> state::TourMarkers {
    let executable = std::env::current_exe().unwrap_or_default();
    state::TourMarkers::new(
        crate::db::get_data_dir(),
        state::installation_identity(&executable),
    )
}

pub(crate) fn should_show_on_start() -> bool {
    markers().should_show()
}

#[tauri::command]
pub async fn open_onboarding_window(app: tauri::AppHandle) -> Result<(), String> {
    let markers = markers();
    if markers.suppressed() {
        return Err("Onboarding has been permanently disabled for this installation".into());
    }
    markers.dismiss()?;
    crate::commands::open_connections_window_for_tour(app)
        .await
        .map_err(|error| format!("{error:?}"))
}

#[tauri::command]
pub async fn dismiss_onboarding(_app: tauri::AppHandle) -> Result<(), String> {
    markers().dismiss()
}

#[tauri::command]
pub async fn suppress_onboarding(_app: tauri::AppHandle) -> Result<(), String> {
    markers().suppress()
}

#[tauri::command]
pub fn onboarding_suppressed() -> bool {
    markers().suppressed()
}
