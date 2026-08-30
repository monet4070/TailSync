use super::*;

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
    notifications: Vec<crate::api::RuntimeNotification>,
}

/// Wait until history or transfer state changes, then return one coherent
/// snapshot. The bounded timeout lets the UI recover if a notification is
/// missed without reverting to high-frequency polling.
#[command]
pub async fn wait_runtime_snapshot(
    since_revision: u64,
    wait_ms: Option<u64>,
    since_notification_id: Option<u64>,
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
        notifications: crate::api::get_runtime_notifications_since(
            since_notification_id.unwrap_or_default(),
        ),
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

/// Report updater availability separately from checking the network so a build
/// with a missing trust anchor can explain why updates are unavailable in the UI.
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
