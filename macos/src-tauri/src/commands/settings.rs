use super::*;

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
    state: State<'_, AppState>,
    settings_json: String,
) -> Result<crate::crypto::Settings, CommandError> {
    let patch: crate::crypto::SettingsPatch =
        serde_json::from_str(&settings_json).map_err(|_| {
            CommandError::code(tailsync_runtime::contracts::StableErrorCode::InvalidArgument)
        })?;
    let outcome = crate::crypto::apply_settings_patch(
        &state.settings,
        &state.db,
        patch,
        &|settings: &crate::crypto::Settings| settings.save().map_err(|error| error.to_string()),
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
) -> Result<serde_json::Value, CommandError> {
    let data =
        tailsync_runtime::history::HistoryOperations::image_payload_async(state.db.clone(), id)
            .await?;
    let image = crate::protocol::PackedImage::try_from(data.as_slice())
        .map_err(|error| error.to_string())?;
    let (tw, th, thumb) = crate::api::thumbnail_rgba(image, crate::api::THUMBNAIL_MAX_SIDE);
    // The thumbnail is built; the full-size RGBA (up to 32 MiB) is now dead.
    // Release it before base64-encoding the ~100 KB thumbnail and building the
    // response, so the large buffer and the encoded copy never coexist.
    drop(data);
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&thumb);
    Ok(serde_json::json!({
        "id": id,
        "thumbnail_b64": b64,
        "thumbnail_width": tw,
        "thumbnail_height": th,
    }))
}
