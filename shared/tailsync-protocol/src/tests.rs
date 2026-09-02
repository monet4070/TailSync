use super::*;

#[test]
fn test_encode_decode_roundtrip() {
    let frame = Frame::try_new(Command::TextPayload, 0, 42, b"hello world".to_vec())
        .expect("valid text fixture");
    let encoded = frame.encode();
    let (decoded, bytes) = Frame::decode(&encoded).unwrap();

    assert_eq!(bytes, encoded.len());
    assert_eq!(decoded.command, Command::TextPayload);
    assert_eq!(decoded.sequence, 42);
    assert_eq!(decoded.payload, b"hello world");
}

#[test]
fn test_invalid_magic() {
    let result = Frame::decode(b"XXXX");
    assert!(matches!(result, Err(ProtocolError::IncompleteFrame { .. })));
}

#[test]
fn protocol_v2_frames_are_intentionally_rejected() {
    let frame = Frame::try_new(Command::TextPayload, 0, 1, b"legacy".to_vec()).unwrap();
    let mut encoded = frame.encode();
    encoded[4] = 0x02;
    assert!(matches!(
        Frame::decode(&encoded),
        Err(ProtocolError::UnsupportedVersion(0x02))
    ));
}

#[test]
fn test_checksum_mismatch() {
    let frame =
        Frame::try_new(Command::TextPayload, 0, 1, b"test".to_vec()).expect("valid text fixture");
    let mut encoded = frame.encode();
    // Corrupt a byte in payload area
    encoded[HEADER_SIZE] ^= 0xFF;
    let result = Frame::decode(&encoded);
    assert!(matches!(result, Err(ProtocolError::ChecksumMismatch)));
}

#[test]
fn test_command_payload_limit() {
    assert!(matches!(
        Frame::try_new(
            Command::TextPayload,
            0,
            0,
            vec![0u8; MAX_TEXT_PAYLOAD_SIZE + 1],
        ),
        Err(ProtocolError::CommandPayloadTooLarge {
            command: Command::TextPayload,
            ..
        })
    ));
}

#[test]
fn packed_image_rejects_invalid_dimensions_and_lengths() {
    assert!(matches!(
        PackedImage::try_from(&[0_u8; 7][..]),
        Err(ProtocolError::InvalidImage)
    ));

    let mut zero_width = Vec::new();
    zero_width.extend_from_slice(&0_u32.to_le_bytes());
    zero_width.extend_from_slice(&1_u32.to_le_bytes());
    zero_width.extend_from_slice(&[0_u8; 4]);
    assert!(PackedImage::try_from(zero_width.as_slice()).is_err());

    let mut wrong_length = Vec::new();
    wrong_length.extend_from_slice(&2_u32.to_le_bytes());
    wrong_length.extend_from_slice(&2_u32.to_le_bytes());
    wrong_length.extend_from_slice(&[0_u8; 15]);
    assert!(PackedImage::try_from(wrong_length.as_slice()).is_err());

    let mut valid = Vec::new();
    valid.extend_from_slice(&2_u32.to_le_bytes());
    valid.extend_from_slice(&2_u32.to_le_bytes());
    valid.extend_from_slice(&[0_u8; 16]);
    let parsed = PackedImage::try_from(valid.as_slice()).unwrap();
    assert_eq!((parsed.width, parsed.height, parsed.rgba.len()), (2, 2, 16));
}

#[test]
fn platform_image_packing_validates_before_copying() {
    let rgba = [0x7b_u8; 16];
    let packed = pack_rgba_image(2, 2, &rgba).unwrap();
    let parsed = PackedImage::try_from(packed.as_slice()).unwrap();
    assert_eq!((parsed.width, parsed.height), (2, 2));
    assert_eq!(parsed.rgba, rgba);

    assert!(pack_rgba_image(0, 2, &rgba).is_err());
    assert!(pack_rgba_image(2, 2, &rgba[..15]).is_err());
    assert!(pack_rgba_image(u32::MAX, u32::MAX, &[]).is_err());
}

#[test]
fn event_envelope_round_trips_and_rejects_stale_timestamps() {
    let envelope = EventEnvelope {
        message_id: MessageId([0x5a; 16]),
        timestamp_ms: 1_000_000,
        content: b"clipboard".to_vec(),
    };
    assert_eq!(EventEnvelope::decode(&envelope.encode()).unwrap(), envelope);
    assert!(envelope.validate_timestamp(1_000_001).is_ok());
    assert!(envelope
        .validate_timestamp(1_000_000 + EVENT_TIMESTAMP_WINDOW_MS + 1)
        .is_err());
    assert!(EventEnvelope::decode(b"clipboard").is_err());
}

#[test]
fn event_ack_requires_exactly_one_message_id() {
    let message_id = MessageId([0x7b; 16]);
    assert_eq!(
        MessageId::from_ack_payload(&message_id.ack_payload()).unwrap(),
        message_id
    );
    assert!(MessageId::from_ack_payload(&[0u8; 15]).is_err());
    assert!(MessageId::from_ack_payload(&[0u8; 17]).is_err());
    assert_eq!(message_id.as_hex(), "7b".repeat(16));
}

#[test]
fn resumable_file_chunk_round_trips_and_detects_corruption() {
    let chunk = FileChunkPayload {
        transfer_id: TransferId([3; 16]),
        offset: 1024,
        data: b"file block".to_vec(),
    };
    let encoded = chunk.encode().unwrap();
    assert_eq!(FileChunkPayload::decode(&encoded).unwrap(), chunk);

    let mut corrupted = encoded;
    *corrupted.last_mut().unwrap() ^= 1;
    assert!(matches!(
        FileChunkPayload::decode(&corrupted),
        Err(ProtocolError::FileChunkChecksumMismatch)
    ));
}

#[test]
fn resumable_file_chunk_rejects_empty_payloads() {
    let chunk = FileChunkPayload {
        transfer_id: TransferId([8; 16]),
        offset: 0,
        data: Vec::new(),
    };
    assert!(matches!(chunk.encode(), Err(ProtocolError::EmptyFileChunk)));

    let mut encoded = FileChunkPayload {
        data: vec![1],
        ..chunk
    }
    .encode()
    .unwrap();
    assert_eq!(encoded.len(), MIN_FILE_CHUNK_PAYLOAD_SIZE);
    encoded.truncate(MIN_FILE_CHUNK_PAYLOAD_SIZE - 1);
    encoded[28..32].copy_from_slice(&0_u32.to_be_bytes());
    encoded[32..64].copy_from_slice(blake3::hash(&[]).as_bytes());
    assert!(matches!(
        FileChunkPayload::decode(&encoded),
        Err(ProtocolError::InvalidFileChunk)
    ));
}

#[test]
fn file_offset_ack_round_trips() {
    let offset = FileOffset {
        transfer_id: TransferId([4; 16]),
        next_offset: 8 * 1024 * 1024,
    };
    assert_eq!(FileOffset::decode(&offset.encode()).unwrap(), offset);
    assert!(FileOffset::decode(&[0; 23]).is_err());
}
