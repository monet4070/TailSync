use super::*;

/// Get image data as base64 thumbnail for frontend display
#[command]
pub async fn get_image_data(
    state: State<'_, AppState>,
    id: i64,
) -> Result<serde_json::Value, String> {
    let db = state.db.lock().await;
    let data = db.get_data(id).map_err(|e| e.to_string())?;
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

pub(super) const PREVIEW_RESPONSE_MAGIC: &[u8; 4] = b"TSPV";
pub(super) const PREVIEW_RESPONSE_VERSION: u8 = 1;

#[derive(serde::Serialize)]
pub(super) struct PreviewResponseMetadata {
    entry_id: i64,
    kind: String,
    name: String,
    size_bytes: u64,
    width: Option<u32>,
    height: Option<u32>,
    batch: Option<db::PreviewBatchNavigation>,
}

pub(super) fn preview_payload_error(
    entry_id: i64,
    message: impl Into<String>,
) -> db::PreviewErrorInfo {
    db::PreviewErrorInfo::payload_unavailable(entry_id, message)
}

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
pub(super) fn encode_preview_response(
    metadata: db::PreviewMetadata,
    payload: db::PreviewPayload,
) -> Result<Vec<u8>, db::PreviewErrorInfo> {
    let entry_id = metadata.entry_id;
    let (width, height, data) = if payload.kind == "image" {
        let image = crate::protocol::PackedImage::try_from(payload.data.as_slice())
            .map_err(|error| preview_payload_error(entry_id, error.to_string()))?;
        (Some(image.width), Some(image.height), image.rgba.to_vec())
    } else {
        (None, None, payload.data)
    };
    let metadata = PreviewResponseMetadata {
        entry_id,
        kind: payload.kind,
        name: payload.name,
        size_bytes: u64::try_from(data.len()).unwrap_or(u64::MAX),
        width,
        height,
        batch: metadata.batch,
    };
    let metadata = serde_json::to_vec(&metadata)
        .map_err(|error| preview_payload_error(entry_id, error.to_string()))?;
    let metadata_len = u32::try_from(metadata.len())
        .map_err(|_| preview_payload_error(entry_id, "preview metadata is too large"))?;
    let capacity = 9_usize
        .checked_add(metadata.len())
        .and_then(|length| length.checked_add(data.len()))
        .ok_or_else(|| preview_payload_error(entry_id, "preview response is too large"))?;

    let mut response = Vec::with_capacity(capacity);
    response.extend_from_slice(PREVIEW_RESPONSE_MAGIC);
    response.push(PREVIEW_RESPONSE_VERSION);
    response.extend_from_slice(&metadata_len.to_le_bytes());
    response.extend_from_slice(&metadata);
    response.extend_from_slice(&data);
    Ok(response)
}

/// Return a bounded history preview as a raw `ArrayBuffer` to the frontend.
#[command]
pub async fn get_preview(
    state: State<'_, AppState>,
    id: i64,
    batch_id: Option<String>,
) -> Result<tauri::ipc::Response, db::PreviewErrorInfo> {
    let db = state.db.lock().await;
    if let Some(batch_id) = batch_id.as_deref() {
        db.get_preview_batch_navigation(batch_id, id)
            .map_err(db::PreviewErrorInfo::from)?;
    }
    let preview_id = id;
    let metadata = db
        .get_preview_metadata(preview_id)
        .map_err(db::PreviewErrorInfo::from)?;
    let payload = db
        .get_preview_payload(preview_id)
        .map_err(db::PreviewErrorInfo::from)?;
    Ok(tauri::ipc::Response::new(encode_preview_response(
        metadata, payload,
    )?))
}
