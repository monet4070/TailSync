use super::*;

fn v2_package(path: &str) -> Result<Vec<u8>, Box<tailsync_core::themes_v2::ThemeError>> {
    tailsync_core::themes_v2::read_theme_package_file(std::path::Path::new(path)).map_err(Box::new)
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
