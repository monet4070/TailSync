//! Bounded zlib image chunks for authenticated v5 sessions.

use std::io::{Read, Write};

use flate2::{read::ZlibDecoder, write::ZlibEncoder, Compression};

use crate::protocol::{
    unix_timestamp_ms, EventEnvelope, MessageId, PackedImage, ProtocolError,
    EVENT_ENVELOPE_HEADER_SIZE, MAX_IMAGE_PAYLOAD_SIZE,
};

const MAGIC: &[u8; 4] = b"IMC1";
const HEADER: usize = 72;
const CHUNK_DATA: usize = 512 * 1024;
const MAX_CHUNKS: usize = 64;
const MAX_RAW: usize = MAX_IMAGE_PAYLOAD_SIZE - EVENT_ENVELOPE_HEADER_SIZE;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Metadata {
    message_id: MessageId,
    timestamp_ms: i64,
    count: u16,
    compressed_len: u32,
    raw_len: u32,
    digest: [u8; 32],
}

fn invalid() -> ProtocolError {
    ProtocolError::InvalidImageChunk
}

/// Returns None when compression cannot reduce the wire size; the caller
/// then uses the existing reliable image event on the same connection.
pub fn compress(envelope: &EventEnvelope) -> Result<Option<Vec<Vec<u8>>>, ProtocolError> {
    PackedImage::try_from(envelope.content.as_slice())?;
    if envelope.content.len() > MAX_RAW {
        return Err(invalid());
    }
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder
        .write_all(&envelope.content)
        .map_err(|_| invalid())?;
    let compressed = encoder.finish().map_err(|_| invalid())?;
    if compressed.len() >= envelope.content.len() || compressed.len() > MAX_IMAGE_PAYLOAD_SIZE {
        return Ok(None);
    }
    let count = compressed.len().div_ceil(CHUNK_DATA);
    if count == 0 || count > MAX_CHUNKS {
        return Ok(None);
    }
    let digest = *blake3::hash(&envelope.content).as_bytes();
    let mut chunks = Vec::with_capacity(count);
    for (index, data) in compressed.chunks(CHUNK_DATA).enumerate() {
        let mut chunk = Vec::with_capacity(HEADER + data.len());
        chunk.extend_from_slice(MAGIC);
        chunk.extend_from_slice(&envelope.message_id.0);
        chunk.extend_from_slice(&envelope.timestamp_ms.to_be_bytes());
        chunk.extend_from_slice(&(index as u16).to_be_bytes());
        chunk.extend_from_slice(&(count as u16).to_be_bytes());
        chunk.extend_from_slice(&(compressed.len() as u32).to_be_bytes());
        chunk.extend_from_slice(&(envelope.content.len() as u32).to_be_bytes());
        chunk.extend_from_slice(&digest);
        chunk.extend_from_slice(data);
        chunks.push(chunk);
    }
    Ok(Some(chunks))
}

struct Assembly {
    metadata: Metadata,
    next_index: u16,
    bytes: Vec<u8>,
}

#[derive(Default)]
pub struct ImageAssembler {
    active: Option<Assembly>,
}

impl ImageAssembler {
    pub fn accept(
        &mut self,
        payload: &[u8],
    ) -> Result<(MessageId, Option<EventEnvelope>), ProtocolError> {
        let result = self.accept_inner(payload);
        if result.is_err() {
            self.active = None;
        }
        result
    }

    fn accept_inner(
        &mut self,
        payload: &[u8],
    ) -> Result<(MessageId, Option<EventEnvelope>), ProtocolError> {
        if payload.len() <= HEADER || payload.len() > HEADER + CHUNK_DATA || &payload[..4] != MAGIC
        {
            return Err(invalid());
        }
        let mut id = [0; 16];
        id.copy_from_slice(&payload[4..20]);
        let timestamp_ms = i64::from_be_bytes(payload[20..28].try_into().map_err(|_| invalid())?);
        let index = u16::from_be_bytes(payload[28..30].try_into().map_err(|_| invalid())?);
        let count = u16::from_be_bytes(payload[30..32].try_into().map_err(|_| invalid())?);
        let compressed_len = u32::from_be_bytes(payload[32..36].try_into().map_err(|_| invalid())?);
        let raw_len = u32::from_be_bytes(payload[36..40].try_into().map_err(|_| invalid())?);
        let mut digest = [0; 32];
        digest.copy_from_slice(&payload[40..72]);
        let metadata = Metadata {
            message_id: MessageId(id),
            timestamp_ms,
            count,
            compressed_len,
            raw_len,
            digest,
        };
        if timestamp_ms.abs_diff(unix_timestamp_ms())
            > crate::protocol::EVENT_TIMESTAMP_WINDOW_MS as u64
            || count == 0
            || count as usize > MAX_CHUNKS
            || index >= count
            || compressed_len == 0
            || compressed_len as usize > MAX_IMAGE_PAYLOAD_SIZE
            || raw_len < 12
            || raw_len as usize > MAX_RAW
            || (compressed_len as usize).div_ceil(CHUNK_DATA) != count as usize
        {
            return Err(invalid());
        }
        if index == 0 {
            if self.active.is_some() {
                return Err(invalid());
            }
            self.active = Some(Assembly {
                metadata,
                next_index: 0,
                bytes: Vec::new(),
            });
        }
        let active = self.active.as_mut().ok_or_else(invalid)?;
        if active.metadata != metadata || active.next_index != index {
            return Err(invalid());
        }
        let data = &payload[HEADER..];
        if active.bytes.len() + data.len() > compressed_len as usize {
            return Err(invalid());
        }
        active.bytes.extend_from_slice(data);
        active.next_index += 1;
        if active.next_index < count {
            return Ok((metadata.message_id, None));
        }
        let completed = self.active.take().ok_or_else(invalid)?;
        if completed.bytes.len() != compressed_len as usize {
            return Err(invalid());
        }
        let mut decoder = ZlibDecoder::new(completed.bytes.as_slice());
        let mut raw = Vec::with_capacity(raw_len as usize);
        decoder
            .by_ref()
            .take(raw_len as u64 + 1)
            .read_to_end(&mut raw)
            .map_err(|_| invalid())?;
        if raw.len() != raw_len as usize
            || decoder.total_in() != compressed_len as u64
            || blake3::hash(&raw).as_bytes() != &digest
        {
            return Err(invalid());
        }
        PackedImage::try_from(raw.as_slice())?;
        Ok((
            metadata.message_id,
            Some(EventEnvelope {
                message_id: metadata.message_id,
                timestamp_ms,
                content: raw,
            }),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image() -> EventEnvelope {
        EventEnvelope::new(
            crate::protocol::pack_rgba_image(1024, 512, &vec![7; 1024 * 512 * 4]).unwrap(),
        )
    }

    #[test]
    fn compresses_and_reassembles_bounded_image() {
        let original = image();
        let chunks = compress(&original).unwrap().unwrap();
        assert!(!chunks.is_empty());
        let mut assembler = ImageAssembler::default();
        let mut complete = None;
        for chunk in chunks {
            complete = assembler.accept(&chunk).unwrap().1;
        }
        assert_eq!(complete.unwrap(), original);
    }

    #[test]
    fn rejects_malformed_sizes_and_missing_chunks() {
        let chunks = compress(&image()).unwrap().unwrap();
        let mut malformed = chunks[0].clone();
        malformed[36..40].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(ImageAssembler::default().accept(&malformed).is_err());
        let mut malformed = chunks[0].clone();
        malformed[30..32].copy_from_slice(&0u16.to_be_bytes());
        assert!(ImageAssembler::default().accept(&malformed).is_err());
    }

    #[test]
    fn multiple_chunks_require_order_and_match_the_digest() {
        let mut rgba = vec![0; 1024 * 512 * 4];
        let mut state = 0x1234_5678_u32;
        for pair in rgba.chunks_exact_mut(32 * 1024) {
            let (first, second) = pair.split_at_mut(16 * 1024);
            for byte in first.iter_mut() {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                *byte = state as u8;
            }
            second.copy_from_slice(first);
        }
        let original =
            EventEnvelope::new(crate::protocol::pack_rgba_image(1024, 512, &rgba).unwrap());
        let chunks = compress(&original).unwrap().unwrap();
        assert!(chunks.len() >= 2);
        assert!(ImageAssembler::default().accept(&chunks[1]).is_err());
        let mut duplicate = ImageAssembler::default();
        assert!(duplicate.accept(&chunks[0]).unwrap().1.is_none());
        assert!(duplicate.accept(&chunks[0]).is_err());
        let mut assembler = ImageAssembler::default();
        assert!(assembler.accept(&chunks[0]).unwrap().1.is_none());
        for chunk in &chunks[1..chunks.len() - 1] {
            assert!(assembler.accept(chunk).unwrap().1.is_none());
        }
        let mut corrupt = chunks.last().unwrap().clone();
        *corrupt.last_mut().unwrap() ^= 1;
        assert!(assembler.accept(&corrupt).is_err());
        let mut completed = None;
        for chunk in chunks {
            completed = assembler.accept(&chunk).unwrap().1;
        }
        assert_eq!(completed.unwrap(), original);
    }

    #[test]
    fn rejects_decompression_bomb_and_forged_dimensions() {
        let original = image();
        let mut bomb = compress(&original).unwrap().unwrap().remove(0);
        bomb[36..40].copy_from_slice(&12_u32.to_be_bytes());
        assert!(ImageAssembler::default().accept(&bomb).is_err());

        let mut raw = Vec::new();
        raw.extend_from_slice(&u32::MAX.to_le_bytes());
        raw.extend_from_slice(&1_u32.to_le_bytes());
        raw.extend_from_slice(&[0_u8; 4]);
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(&raw).unwrap();
        let compressed = encoder.finish().unwrap();
        let mut chunk = Vec::new();
        chunk.extend_from_slice(MAGIC);
        chunk.extend_from_slice(&[1_u8; 16]);
        chunk.extend_from_slice(&unix_timestamp_ms().to_be_bytes());
        chunk.extend_from_slice(&0_u16.to_be_bytes());
        chunk.extend_from_slice(&1_u16.to_be_bytes());
        chunk.extend_from_slice(&(compressed.len() as u32).to_be_bytes());
        chunk.extend_from_slice(&(raw.len() as u32).to_be_bytes());
        chunk.extend_from_slice(blake3::hash(&raw).as_bytes());
        chunk.extend_from_slice(&compressed);
        assert!(ImageAssembler::default().accept(&chunk).is_err());
    }
}
