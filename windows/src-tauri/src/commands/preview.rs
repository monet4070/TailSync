use super::*;
use tailsync_runtime::history::HistoryOperations;
#[cfg(test)]
use tailsync_runtime::preview as shared_preview;

/// Get image data as base64 thumbnail for frontend display
#[command]
pub async fn get_image_data(
    state: State<'_, AppState>,
    id: i64,
) -> Result<serde_json::Value, String> {
    let data = HistoryOperations::data_async(state.db.clone(), id).await?;
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

#[cfg(test)]
pub(super) const PREVIEW_RESPONSE_MAGIC: &[u8; 4] = shared_preview::PREVIEW_RESPONSE_MAGIC;
#[cfg(test)]
pub(super) const PREVIEW_RESPONSE_VERSION: u8 = shared_preview::PREVIEW_RESPONSE_VERSION;

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
#[cfg(test)]
pub(super) fn encode_preview_response(
    metadata: db::PreviewMetadata,
    payload: db::PreviewPayload,
) -> Result<Vec<u8>, db::PreviewErrorInfo> {
    shared_preview::encode_preview_response(metadata, payload)
}

/// Return a bounded history preview as a raw `ArrayBuffer` to the frontend.
#[command]
pub async fn get_preview(
    state: State<'_, AppState>,
    id: i64,
    batch_id: Option<String>,
) -> Result<tauri::ipc::Response, db::PreviewErrorInfo> {
    Ok(tauri::ipc::Response::new(
        HistoryOperations::preview_binary_async(state.db.clone(), id, batch_id, None).await?,
    ))
}
