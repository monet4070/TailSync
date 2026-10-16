use super::*;

/// Get current settings
#[command]
pub async fn get_settings(
    state: State<'_, AppState>,
) -> Result<crate::crypto::Settings, CommandError> {
    let settings = state.settings.lock().await;
    Ok(settings.clone())
}

/// Parse a window's field intents, rejecting whole snapshots and server-owned
/// fields with the fixed request-error envelope.
fn parse_settings_patch(settings_json: &str) -> Result<crate::crypto::SettingsPatch, CommandError> {
    let value: serde_json::Value = serde_json::from_str(settings_json).map_err(|_| {
        CommandError::code(tailsync_runtime::contracts::StableErrorCode::InvalidArgument)
    })?;
    crate::crypto::SettingsPatch::from_json_value(value).map_err(|_| {
        CommandError::code(tailsync_runtime::contracts::StableErrorCode::InvalidArgument)
    })
}

/// Update the editable settings fields and return the committed snapshot.
#[command]
pub async fn update_settings(
    state: State<'_, AppState>,
    settings_json: String,
) -> Result<crate::crypto::Settings, CommandError> {
    let patch = parse_settings_patch(&settings_json)?;
    let outcome = crate::crypto::apply_settings_patch(
        &state.settings,
        &state.db,
        patch,
        &|settings: &crate::crypto::Settings| {
            crate::automatic_images::persist(settings, &state.image_archive)
        },
    )
    .await
    .map_err(|error| error.to_string())?;
    if outcome.mode_changed {
        state.pool.lock().await.disconnect_all();
        network::clear_peer_cache().await;
        network::refresh_iroh_for_mode(&outcome.connection_mode).await;
    }
    Ok(outcome.persisted)
}

/// Get image data as base64 thumbnail for frontend display
#[command]
pub async fn get_image_data(
    state: State<'_, AppState>,
    id: i64,
    cache_key: Option<String>,
) -> Result<serde_json::Value, CommandError> {
    let mut data = tailsync_runtime::history_display::image_payload_checked_async(
        state.db.clone(),
        id,
        cache_key,
        crate::clipboard_composite::native_analyzer(),
        tailsync_core::cancellation::Cancellation::default(),
    )
    .await?;
    let image = crate::protocol::PackedImage::try_from(data.data.as_slice())
        .map_err(|error| error.to_string())?;
    let (tw, th, thumb) = crate::api::thumbnail_rgba(image, crate::api::THUMBNAIL_MAX_SIDE);
    // The thumbnail is built; the full-size RGBA (up to 32 MiB) is now dead.
    // Release it before base64-encoding the ~100 KB thumbnail and building the
    // response, so the large buffer and the encoded copy never coexist.
    data.data = Vec::new();
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&thumb);
    Ok(serde_json::json!({
        "id": id,
        "thumbnail_b64": b64,
        "thumbnail_width": tw,
        "thumbnail_height": th,
        "cache_key": data.cache_key,
    }))
}
