//! Shared framing for local binary history previews.
//!
//! The local UI transports use the same bounded TSPV envelope so a preview
//! implementation cannot drift between Windows and macOS.  The frame carries
//! JSON metadata followed by exactly one payload; callers remain responsible
//! for transport authentication and EOF handling.

use serde::Serialize;
use tailsync_core::db::{
    PreviewBatchNavigation, PreviewErrorCode, PreviewErrorInfo, PreviewMetadata, PreviewPayload,
};

pub const PREVIEW_RESPONSE_MAGIC: &[u8; 4] = b"TSPV";
pub const PREVIEW_RESPONSE_VERSION: u8 = 1;
pub const PREVIEW_HEADER_BYTES: usize = 9;
pub const PREVIEW_MAX_METADATA_BYTES: usize = 1024 * 1024;

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct PreviewFrameMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[schemars(range(min = 1))]
    pub entry_id: i64,
    #[schemars(extend("enum" = ["text", "image", "file"]))]
    pub kind: String,
    #[schemars(length(min = 1))]
    pub name: String,
    #[schemars(range(max = 67108864))]
    pub size_bytes: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub batch: Option<PreviewBatchNavigation>,
}

fn payload_error(entry_id: i64, message: impl Into<String>) -> PreviewErrorInfo {
    PreviewErrorInfo::payload_unavailable(entry_id, message)
}

/// Encode one validated preview into a bounded TSPV envelope.
pub fn encode_preview_response(
    metadata: PreviewMetadata,
    payload: PreviewPayload,
) -> Result<Vec<u8>, PreviewErrorInfo> {
    encode_preview_response_for_request(metadata, payload, None)
}

pub fn encode_preview_response_for_request(
    metadata: PreviewMetadata,
    payload: PreviewPayload,
    request_id: Option<String>,
) -> Result<Vec<u8>, PreviewErrorInfo> {
    let entry_id = metadata.entry_id;
    let (width, height, data) = if payload.kind == "image" {
        let image = tailsync_core::protocol::PackedImage::try_from(payload.data.as_slice())
            .map_err(|error| payload_error(entry_id, error.to_string()))?;
        (Some(image.width), Some(image.height), image.rgba)
    } else {
        (None, None, payload.data.as_slice())
    };
    if data.len() > tailsync_core::db::PREVIEW_MAX_BYTES as usize {
        return Err(PreviewErrorInfo {
            code: PreviewErrorCode::PreviewTooLarge,
            message: format!(
                "preview payload is too large: {} bytes (limit {} bytes)",
                data.len(),
                tailsync_core::db::PREVIEW_MAX_BYTES
            ),
            entry_id: Some(entry_id),
            size_bytes: Some(data.len() as u64),
            limit_bytes: Some(tailsync_core::db::PREVIEW_MAX_BYTES),
            retryable: false,
        });
    }
    let frame_metadata = PreviewFrameMetadata {
        request_id,
        entry_id,
        kind: payload.kind,
        name: payload.name,
        size_bytes: u64::try_from(data.len()).unwrap_or(u64::MAX),
        width,
        height,
        batch: metadata.batch,
    };
    let metadata_bytes = serde_json::to_vec(&frame_metadata)
        .map_err(|error| payload_error(entry_id, error.to_string()))?;
    if metadata_bytes.is_empty() || metadata_bytes.len() > PREVIEW_MAX_METADATA_BYTES {
        return Err(payload_error(entry_id, "preview metadata is too large"));
    }
    let metadata_len = u32::try_from(metadata_bytes.len())
        .map_err(|_| payload_error(entry_id, "preview metadata length overflows u32"))?;
    let capacity = PREVIEW_HEADER_BYTES
        .checked_add(metadata_bytes.len())
        .and_then(|length| length.checked_add(data.len()))
        .ok_or_else(|| payload_error(entry_id, "preview response is too large"))?;
    let mut response = Vec::with_capacity(capacity);
    response.extend_from_slice(PREVIEW_RESPONSE_MAGIC);
    response.push(PREVIEW_RESPONSE_VERSION);
    response.extend_from_slice(&metadata_len.to_le_bytes());
    response.extend_from_slice(&metadata_bytes);
    response.extend_from_slice(data);
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tailsync_core::db::{PreviewKind, PreviewMetadata};

    #[test]
    fn frame_contains_versioned_header_and_exact_payload() {
        let frame = encode_preview_response(
            PreviewMetadata {
                entry_id: 7,
                kind: PreviewKind::Text,
                name: "text.txt".into(),
                size_bytes: 5,
                batch: None,
            },
            PreviewPayload {
                kind: "text".into(),
                name: "text.txt".into(),
                size_bytes: 5,
                data: b"hello".to_vec(),
            },
        )
        .expect("preview frame");

        assert_eq!(&frame[..4], PREVIEW_RESPONSE_MAGIC);
        assert_eq!(frame[4], PREVIEW_RESPONSE_VERSION);
        let metadata_len = u32::from_le_bytes(frame[5..9].try_into().unwrap()) as usize;
        assert!(metadata_len > 0);
        assert_eq!(&frame[PREVIEW_HEADER_BYTES + metadata_len..], b"hello");
    }

    #[test]
    fn screenshot_sized_image_frame_strips_packed_dimensions_without_base64() {
        let width = 2_800_u32;
        let height = 1_000_u32;
        let rgba_len = width as usize * height as usize * 4;
        let mut packed = Vec::with_capacity(8 + rgba_len);
        packed.extend_from_slice(&width.to_le_bytes());
        packed.extend_from_slice(&height.to_le_bytes());
        packed.resize(8 + rgba_len, 0x7f);

        let frame = encode_preview_response(
            PreviewMetadata {
                entry_id: 8,
                kind: PreviewKind::Image,
                name: "image".into(),
                size_bytes: packed.len() as u64,
                batch: None,
            },
            PreviewPayload {
                kind: "image".into(),
                name: "image".into(),
                size_bytes: packed.len() as u64,
                data: packed,
            },
        )
        .expect("large image preview frame");

        let metadata_len = u32::from_le_bytes(frame[5..9].try_into().unwrap()) as usize;
        let metadata: serde_json::Value = serde_json::from_slice(
            &frame[PREVIEW_HEADER_BYTES..PREVIEW_HEADER_BYTES + metadata_len],
        )
        .unwrap();
        assert_eq!(metadata["width"], width);
        assert_eq!(metadata["height"], height);
        assert_eq!(metadata["size_bytes"], rgba_len as u64);
        assert_eq!(
            frame.len(),
            PREVIEW_HEADER_BYTES + metadata_len + rgba_len,
            "the binary frame must carry raw RGBA exactly once"
        );
    }
}
