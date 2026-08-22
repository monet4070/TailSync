/// Binary frame protocol for TailSync peer-to-peer communication.
///
/// Frame structure:
/// ┌──────────┬───────┬───────┬───────┬───────┬───────┬──────────┬──────────┐
/// │ Magic(4) │ Ver(1)│Flags(1)│Cmd(2) │ Seq(4)│ Len(4)│ Payload   │Blake3(32)│
/// │ "TSYN"   │ 0x03  │       │       │       │       │ (var)     │          │
/// └──────────┴───────┴───────┴───────┴───────┴───────┴──────────┴──────────┘
/// Total header: 16 bytes + 32 byte checksum = 48 bytes overhead per frame
use blake3::Hasher;
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

pub const MAGIC: [u8; 4] = *b"TSYN";
/// Protocol v3 introduces atomic file batches. Older peers are intentionally
/// rejected at framing time; their pinned identity remains valid after upgrade.
pub const VERSION: u8 = 0x03;
pub const HEADER_SIZE: usize = 16;
pub const CHECKSUM_SIZE: usize = 32;
pub const MAX_HANDSHAKE_PAYLOAD_SIZE: usize = 4 * 1024;
pub const MAX_CONTROL_PAYLOAD_SIZE: usize = 4 * 1024;
pub const MAX_TEXT_PAYLOAD_SIZE: usize = 1024 * 1024;
pub const MAX_IMAGE_PAYLOAD_SIZE: usize = 32 * 1024 * 1024;
pub const MAX_FILE_META_PAYLOAD_SIZE: usize = 16 * 1024;
pub const FILE_CHUNK_SIZE: usize = 1024 * 1024;
pub const MAX_FILE_CHUNK_PAYLOAD_SIZE: usize = FILE_CHUNK_SIZE + FILE_CHUNK_HEADER_SIZE;
pub const MAX_PAYLOAD_SIZE: usize = MAX_IMAGE_PAYLOAD_SIZE;
const MAX_IMAGE_PIXELS: usize = MAX_IMAGE_PAYLOAD_SIZE / 4;
const EVENT_MAGIC: [u8; 4] = *b"EVT1";
pub const EVENT_ENVELOPE_HEADER_SIZE: usize = 28;
pub const EVENT_TIMESTAMP_WINDOW_MS: i64 = 5 * 60 * 1000;
const FILE_CHUNK_MAGIC: [u8; 4] = *b"FCH1";
const FILE_CHUNK_HEADER_SIZE: usize = 64;
const FILE_OFFSET_PAYLOAD_SIZE: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MessageId(pub [u8; 16]);

impl MessageId {
    pub fn random() -> Self {
        Self(rand::random())
    }

    pub fn from_ack_payload(payload: &[u8]) -> Result<Self, ProtocolError> {
        let bytes = payload
            .try_into()
            .map_err(|_| ProtocolError::InvalidEventAck)?;
        Ok(Self(bytes))
    }

    pub fn ack_payload(self) -> Vec<u8> {
        self.0.to_vec()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct TransferId(pub [u8; 16]);

impl TransferId {
    pub fn random() -> Self {
        Self(rand::random())
    }

    pub fn as_hex(self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    pub fn from_hex(value: &str) -> Result<Self, String> {
        let bytes = hex::decode(value).map_err(|_| "Invalid transfer ID".to_string())?;
        let bytes: [u8; 16] = bytes
            .try_into()
            .map_err(|_| "Invalid transfer ID".to_string())?;
        Ok(Self(bytes))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChunkPayload {
    pub transfer_id: TransferId,
    pub offset: u64,
    pub data: Vec<u8>,
}

impl FileChunkPayload {
    pub fn encode(&self) -> Result<Vec<u8>, ProtocolError> {
        if self.data.len() > FILE_CHUNK_SIZE {
            return Err(ProtocolError::FileChunkTooLarge(self.data.len()));
        }
        let mut payload = Vec::with_capacity(FILE_CHUNK_HEADER_SIZE + self.data.len());
        payload.extend_from_slice(&FILE_CHUNK_MAGIC);
        payload.extend_from_slice(&self.transfer_id.0);
        payload.extend_from_slice(&self.offset.to_be_bytes());
        payload.extend_from_slice(&(self.data.len() as u32).to_be_bytes());
        payload.extend_from_slice(blake3::hash(&self.data).as_bytes());
        payload.extend_from_slice(&self.data);
        Ok(payload)
    }

    pub fn decode(payload: &[u8]) -> Result<Self, ProtocolError> {
        if payload.len() < FILE_CHUNK_HEADER_SIZE || payload[..4] != FILE_CHUNK_MAGIC {
            return Err(ProtocolError::InvalidFileChunk);
        }
        let mut transfer_id = [0u8; 16];
        transfer_id.copy_from_slice(&payload[4..20]);
        let offset = u64::from_be_bytes(
            payload[20..28]
                .try_into()
                .map_err(|_| ProtocolError::InvalidFileChunk)?,
        );
        let data_len = u32::from_be_bytes(
            payload[28..32]
                .try_into()
                .map_err(|_| ProtocolError::InvalidFileChunk)?,
        ) as usize;
        if data_len > FILE_CHUNK_SIZE || payload.len() != FILE_CHUNK_HEADER_SIZE + data_len {
            return Err(ProtocolError::InvalidFileChunk);
        }
        let data = payload[FILE_CHUNK_HEADER_SIZE..].to_vec();
        if blake3::hash(&data).as_bytes() != &payload[32..64] {
            return Err(ProtocolError::FileChunkChecksumMismatch);
        }
        Ok(Self {
            transfer_id: TransferId(transfer_id),
            offset,
            data,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileOffset {
    pub transfer_id: TransferId,
    pub next_offset: u64,
}

impl FileOffset {
    pub fn encode(self) -> Vec<u8> {
        let mut payload = Vec::with_capacity(FILE_OFFSET_PAYLOAD_SIZE);
        payload.extend_from_slice(&self.transfer_id.0);
        payload.extend_from_slice(&self.next_offset.to_be_bytes());
        payload
    }

    pub fn decode(payload: &[u8]) -> Result<Self, ProtocolError> {
        if payload.len() != FILE_OFFSET_PAYLOAD_SIZE {
            return Err(ProtocolError::InvalidFileOffset);
        }
        let mut transfer_id = [0u8; 16];
        transfer_id.copy_from_slice(&payload[..16]);
        let next_offset = u64::from_be_bytes(
            payload[16..24]
                .try_into()
                .map_err(|_| ProtocolError::InvalidFileOffset)?,
        );
        Ok(Self {
            transfer_id: TransferId(transfer_id),
            next_offset,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventEnvelope {
    pub message_id: MessageId,
    pub timestamp_ms: i64,
    pub content: Vec<u8>,
}

impl EventEnvelope {
    pub fn new(content: Vec<u8>) -> Self {
        Self {
            message_id: MessageId::random(),
            timestamp_ms: unix_timestamp_ms(),
            content,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut encoded = Vec::with_capacity(EVENT_ENVELOPE_HEADER_SIZE + self.content.len());
        encoded.extend_from_slice(&EVENT_MAGIC);
        encoded.extend_from_slice(&self.message_id.0);
        encoded.extend_from_slice(&self.timestamp_ms.to_be_bytes());
        encoded.extend_from_slice(&self.content);
        encoded
    }

    pub fn encoded_len(&self) -> usize {
        EVENT_ENVELOPE_HEADER_SIZE + self.content.len()
    }

    pub fn decode(payload: &[u8]) -> Result<Self, ProtocolError> {
        if payload.len() < EVENT_ENVELOPE_HEADER_SIZE || payload[..4] != EVENT_MAGIC {
            return Err(ProtocolError::InvalidEventEnvelope);
        }
        let mut message_id = [0u8; 16];
        message_id.copy_from_slice(&payload[4..20]);
        let timestamp_ms = i64::from_be_bytes(
            payload[20..28]
                .try_into()
                .map_err(|_| ProtocolError::InvalidEventEnvelope)?,
        );
        Ok(Self {
            message_id: MessageId(message_id),
            timestamp_ms,
            content: payload[EVENT_ENVELOPE_HEADER_SIZE..].to_vec(),
        })
    }

    pub fn validate_timestamp(&self, now_ms: i64) -> Result<(), ProtocolError> {
        if self.timestamp_ms.abs_diff(now_ms) > EVENT_TIMESTAMP_WINDOW_MS as u64 {
            return Err(ProtocolError::EventTimestampOutsideWindow);
        }
        Ok(())
    }
}

pub fn unix_timestamp_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

/// Frame command types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Command {
    // Connection lifecycle
    HandshakeReq = 0x0001,
    HandshakeAck = 0x0002,
    Heartbeat = 0x0003,
    HeartbeatAck = 0x0004,
    HandshakeFinish = 0x0008,
    HandshakeReady = 0x0009,
    PairingHandshakeReq = 0x000a,
    PairingHandshakeAck = 0x000b,
    PairingHandshakeFinish = 0x000c,
    PairingConfirm = 0x000d,
    PairingCancel = 0x000e,
    EventAck = 0x000f,
    // Content transfer
    TextPayload = 0x0101,
    ImagePayload = 0x0102,
    FileMeta = 0x0103,
    FileChunk = 0x0104,
    FileAck = 0x0105,
    FileResume = 0x0106,
    FileComplete = 0x0107,
    FileBatchStart = 0x0108,
    FileBatchAccept = 0x0109,
    FileBatchReject = 0x010a,
    FileBatchComplete = 0x010b,
    FileBatchCancel = 0x010c,
    // Control
    CancelTransfer = 0x0005,
    PeerError = 0x0006,
    PeerInfo = 0x0007,
}

impl Command {
    pub fn from_u16(v: u16) -> Option<Self> {
        match v {
            0x0001 => Some(Self::HandshakeReq),
            0x0002 => Some(Self::HandshakeAck),
            0x0003 => Some(Self::Heartbeat),
            0x0004 => Some(Self::HeartbeatAck),
            0x0008 => Some(Self::HandshakeFinish),
            0x0009 => Some(Self::HandshakeReady),
            0x000a => Some(Self::PairingHandshakeReq),
            0x000b => Some(Self::PairingHandshakeAck),
            0x000c => Some(Self::PairingHandshakeFinish),
            0x000d => Some(Self::PairingConfirm),
            0x000e => Some(Self::PairingCancel),
            0x000f => Some(Self::EventAck),
            0x0101 => Some(Self::TextPayload),
            0x0102 => Some(Self::ImagePayload),
            0x0103 => Some(Self::FileMeta),
            0x0104 => Some(Self::FileChunk),
            0x0105 => Some(Self::FileAck),
            0x0106 => Some(Self::FileResume),
            0x0107 => Some(Self::FileComplete),
            0x0108 => Some(Self::FileBatchStart),
            0x0109 => Some(Self::FileBatchAccept),
            0x010a => Some(Self::FileBatchReject),
            0x010b => Some(Self::FileBatchComplete),
            0x010c => Some(Self::FileBatchCancel),
            0x0005 => Some(Self::CancelTransfer),
            0x0006 => Some(Self::PeerError),
            0x0007 => Some(Self::PeerInfo),
            _ => None,
        }
    }

    pub fn payload_limit(self) -> usize {
        match self {
            Self::HandshakeReq
            | Self::HandshakeAck
            | Self::HandshakeFinish
            | Self::PairingHandshakeReq
            | Self::PairingHandshakeAck
            | Self::PairingHandshakeFinish => MAX_HANDSHAKE_PAYLOAD_SIZE,
            Self::TextPayload => MAX_TEXT_PAYLOAD_SIZE,
            Self::ImagePayload => MAX_IMAGE_PAYLOAD_SIZE,
            Self::FileMeta | Self::FileResume | Self::FileBatchStart => MAX_FILE_META_PAYLOAD_SIZE,
            Self::FileChunk => MAX_FILE_CHUNK_PAYLOAD_SIZE,
            Self::Heartbeat
            | Self::HeartbeatAck
            | Self::HandshakeReady
            | Self::PairingConfirm
            | Self::PairingCancel
            | Self::EventAck
            | Self::FileAck
            | Self::FileComplete
            | Self::FileBatchAccept
            | Self::FileBatchReject
            | Self::FileBatchComplete
            | Self::FileBatchCancel
            | Self::CancelTransfer
            | Self::PeerError
            | Self::PeerInfo => MAX_CONTROL_PAYLOAD_SIZE,
        }
    }
}

/// Frame flags
#[derive(Debug, Default)]
pub struct Flags(u8);

/// A complete frame with header, payload, and checksum
#[derive(Debug)]
pub struct Frame {
    pub command: Command,
    pub flags: Flags,
    pub sequence: u32,
    pub payload: Vec<u8>,
}

#[derive(Error, Debug)]
pub enum ProtocolError {
    #[error("invalid magic bytes: expected TSYN")]
    InvalidMagic,
    #[error(
        "Incompatible TailSync protocol: peer uses v{0}, this version requires v{VERSION}. Update TailSync on both devices."
    )]
    UnsupportedVersion(u8),
    #[error("checksum mismatch")]
    ChecksumMismatch,
    #[error("payload too large: {0} bytes (max {MAX_PAYLOAD_SIZE})")]
    PayloadTooLarge(usize),
    #[error("{command:?} payload too large: {actual} bytes (max {limit})")]
    CommandPayloadTooLarge {
        command: Command,
        actual: usize,
        limit: usize,
    },
    #[error("unknown command: 0x{0:04x}")]
    UnknownCommand(u16),
    #[error("invalid encrypted transport record")]
    InvalidEncryptedRecord,
    #[error("encrypted transport error: {0}")]
    TransportEncryption(String),
    #[error("invalid reliable event envelope")]
    InvalidEventEnvelope,
    #[error("invalid event acknowledgement")]
    InvalidEventAck,
    #[error("event timestamp is outside the accepted window")]
    EventTimestampOutsideWindow,
    #[error("file chunk exceeds the {FILE_CHUNK_SIZE} byte logical block size: {0}")]
    FileChunkTooLarge(usize),
    #[error("invalid resumable file chunk")]
    InvalidFileChunk,
    #[error("file chunk checksum mismatch")]
    FileChunkChecksumMismatch,
    #[error("invalid file offset acknowledgement")]
    InvalidFileOffset,
    #[error("invalid packed RGBA image")]
    InvalidImage,
    #[error("frame admission rejected: {0}")]
    AdmissionRejected(String),
    #[error("incomplete frame: expected {expected}, got {got}")]
    IncompleteFrame { expected: usize, got: usize },
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Copy)]
pub struct PackedImage<'a> {
    pub width: u32,
    pub height: u32,
    pub rgba: &'a [u8],
}

impl<'a> TryFrom<&'a [u8]> for PackedImage<'a> {
    type Error = ProtocolError;

    fn try_from(value: &'a [u8]) -> Result<Self, Self::Error> {
        let width = u32::from_le_bytes(
            value
                .get(0..4)
                .ok_or(ProtocolError::InvalidImage)?
                .try_into()
                .map_err(|_| ProtocolError::InvalidImage)?,
        );
        let height = u32::from_le_bytes(
            value
                .get(4..8)
                .ok_or(ProtocolError::InvalidImage)?
                .try_into()
                .map_err(|_| ProtocolError::InvalidImage)?,
        );
        if width == 0 || height == 0 {
            return Err(ProtocolError::InvalidImage);
        }
        let pixels = (width as usize)
            .checked_mul(height as usize)
            .filter(|pixels| *pixels <= MAX_IMAGE_PIXELS)
            .ok_or(ProtocolError::InvalidImage)?;
        let expected = pixels.checked_mul(4).ok_or(ProtocolError::InvalidImage)?;
        let rgba = value.get(8..).ok_or(ProtocolError::InvalidImage)?;
        if rgba.len() != expected {
            return Err(ProtocolError::InvalidImage);
        }
        Ok(Self {
            width,
            height,
            rgba,
        })
    }
}

impl Frame {
    /// Create a frame after validating the command-specific payload limit.
    pub fn try_new(
        command: Command,
        flags: u8,
        sequence: u32,
        payload: Vec<u8>,
    ) -> Result<Self, ProtocolError> {
        let limit = command.payload_limit();
        if payload.len() > limit {
            return Err(ProtocolError::CommandPayloadTooLarge {
                command,
                actual: payload.len(),
                limit,
            });
        }
        Ok(Self {
            command,
            flags: Flags(flags),
            sequence,
            payload,
        })
    }

    /// Encode frame to bytes for wire transmission
    pub fn encode(&self) -> Vec<u8> {
        self.encode_with_version(VERSION)
    }

    /// Encode a control response for a peer whose framing version is unsupported.
    ///
    /// Not for normal traffic: regular frames must always use [`VERSION`] via
    /// [`Frame::encode`]. Exposed as `#[doc(hidden)] pub` (rather than crate-private)
    /// only so `tailsync-core`'s handshake layer can reply to peers pinned to an
    /// unsupported version now that the wire protocol lives in its own crate.
    #[doc(hidden)]
    pub fn encode_with_version(&self, version: u8) -> Vec<u8> {
        let payload_len = self.payload.len() as u32;
        let mut buf = Vec::with_capacity(HEADER_SIZE + self.payload.len() + CHECKSUM_SIZE);

        // Header
        buf.extend_from_slice(&MAGIC);
        buf.push(version);
        buf.push(self.flags.0);
        buf.extend_from_slice(&(self.command as u16).to_be_bytes());
        buf.extend_from_slice(&self.sequence.to_be_bytes());
        buf.extend_from_slice(&payload_len.to_be_bytes());

        // Payload + checksum
        let mut hasher = Hasher::new();
        hasher.update(&buf);
        hasher.update(&self.payload);
        let checksum = hasher.finalize();

        buf.extend_from_slice(&self.payload);
        buf.extend_from_slice(checksum.as_bytes());

        buf
    }

    /// Decode a frame from bytes. Returns (frame, bytes_consumed).
    pub fn decode(data: &[u8]) -> Result<(Self, usize), ProtocolError> {
        if data.len() < HEADER_SIZE + CHECKSUM_SIZE {
            return Err(ProtocolError::IncompleteFrame {
                expected: HEADER_SIZE + CHECKSUM_SIZE,
                got: data.len(),
            });
        }

        // Parse header
        let magic = &data[0..4];
        if magic != MAGIC {
            return Err(ProtocolError::InvalidMagic);
        }

        let version = data[4];
        if version != VERSION {
            return Err(ProtocolError::UnsupportedVersion(version));
        }

        let flags = data[5];
        let cmd = u16::from_be_bytes([data[6], data[7]]);
        let command = Command::from_u16(cmd).ok_or(ProtocolError::UnknownCommand(cmd))?;
        let sequence = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
        let payload_len = u32::from_be_bytes([data[12], data[13], data[14], data[15]]) as usize;

        if payload_len > MAX_PAYLOAD_SIZE {
            return Err(ProtocolError::PayloadTooLarge(payload_len));
        }
        let command_limit = command.payload_limit();
        if payload_len > command_limit {
            return Err(ProtocolError::CommandPayloadTooLarge {
                command,
                actual: payload_len,
                limit: command_limit,
            });
        }

        let total_size = HEADER_SIZE + payload_len + CHECKSUM_SIZE;
        if data.len() < total_size {
            return Err(ProtocolError::IncompleteFrame {
                expected: total_size,
                got: data.len(),
            });
        }

        let payload = data[HEADER_SIZE..HEADER_SIZE + payload_len].to_vec();
        let received_checksum = &data[HEADER_SIZE + payload_len..total_size];

        // Verify checksum
        let mut hasher = Hasher::new();
        hasher.update(&data[..HEADER_SIZE + payload_len]);
        let computed = hasher.finalize();

        if computed.as_bytes() != received_checksum {
            return Err(ProtocolError::ChecksumMismatch);
        }

        Ok((
            Frame {
                command,
                flags: Flags(flags),
                sequence,
                payload,
            },
            total_size,
        ))
    }
}

#[cfg(test)]
mod tests {
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
        let frame = Frame::try_new(Command::TextPayload, 0, 1, b"test".to_vec())
            .expect("valid text fixture");
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
    fn file_offset_ack_round_trips() {
        let offset = FileOffset {
            transfer_id: TransferId([4; 16]),
            next_offset: 8 * 1024 * 1024,
        };
        assert_eq!(FileOffset::decode(&offset.encode()).unwrap(), offset);
        assert!(FileOffset::decode(&[0; 23]).is_err());
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    fn command_strategy() -> impl Strategy<Value = Command> {
        let variants = (0x0001..=0x010c)
            .filter_map(Command::from_u16)
            .collect::<Vec<_>>();
        assert!(!variants.is_empty());
        proptest::sample::select(variants)
    }

    fn small_bytes(max: usize) -> impl Strategy<Value = Vec<u8>> {
        prop::collection::vec(any::<u8>(), 0..max)
    }

    proptest! {
        #[test]
        fn frame_round_trips_through_encode_and_decode(
            command in command_strategy(),
            flags in any::<u8>(),
            sequence in any::<u32>(),
            payload in small_bytes(256),
        ) {
            let frame = Frame::try_new(command, flags, sequence, payload).unwrap();
            let encoded = frame.encode();
            let (decoded, consumed) = Frame::decode(&encoded).unwrap();
            prop_assert_eq!(consumed, encoded.len());
            prop_assert_eq!(decoded.command, frame.command);
            prop_assert_eq!(decoded.flags.0, frame.flags.0);
            prop_assert_eq!(decoded.sequence, frame.sequence);
            prop_assert_eq!(decoded.payload, frame.payload);
        }

        #[test]
        fn frame_decode_never_panics_on_arbitrary_bytes(data in small_bytes(2048)) {
            let _ = Frame::decode(&data);
        }

        #[test]
        fn frame_decode_consumes_at_most_the_input_length(data in small_bytes(1024)) {
            if let Ok((_, consumed)) = Frame::decode(&data) {
                prop_assert!(consumed <= data.len());
            }
        }

        #[test]
        fn file_chunk_payload_decode_never_panics_on_arbitrary_bytes(data in small_bytes(1024)) {
            let _ = FileChunkPayload::decode(&data);
        }

        #[test]
        fn file_offset_decode_never_panics_on_arbitrary_bytes(data in small_bytes(1024)) {
            let _ = FileOffset::decode(&data);
        }

        #[test]
        fn event_envelope_decode_never_panics_on_arbitrary_bytes(data in small_bytes(1024)) {
            let _ = EventEnvelope::decode(&data);
        }
    }
}
