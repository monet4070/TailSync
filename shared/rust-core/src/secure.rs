use serde::de::Error as DeserializeError;
use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use snow::TransportState;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::identity;
use crate::protocol::{self, Command, Frame, ProtocolError};

const MAX_TRANSPORT_RECORD: usize = u16::MAX as usize;
const MAX_TRANSPORT_PLAINTEXT: usize = MAX_TRANSPORT_RECORD - 16;
const TRANSPORT_WRITE_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
pub struct PeerIdentity {
    pub hostname: String,
    pub tailscale_ip: String,
    pub iroh_endpoint_id: Option<String>,
}

impl Serialize for PeerIdentity {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("PeerIdentity", 5)?;
        state.serialize_field("hostname", &self.hostname)?;
        state.serialize_field("tailscale_ip", &self.tailscale_ip)?;
        if let Some(endpoint_id) = &self.iroh_endpoint_id {
            state.serialize_field("iroh_endpoint_id", endpoint_id)?;
        }
        state.serialize_field("protocol_version", &protocol::VERSION)?;
        state.serialize_field("app_version", env!("CARGO_PKG_VERSION"))?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for PeerIdentity {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct WireIdentity {
            hostname: String,
            tailscale_ip: String,
            #[serde(default)]
            iroh_endpoint_id: Option<String>,
            #[serde(default)]
            protocol_version: Option<u8>,
            #[serde(default)]
            app_version: Option<String>,
        }

        let identity = WireIdentity::deserialize(deserializer)?;
        if let Some(version) = identity.protocol_version {
            if version != protocol::VERSION {
                let app = identity
                    .app_version
                    .as_deref()
                    .map(|version| format!(" ({version})"))
                    .unwrap_or_default();
                return Err(D::Error::custom(format!(
                    "Incompatible TailSync protocol: peer{app} uses v{version}, this version requires v{}. Update TailSync on both devices.",
                    protocol::VERSION
                )));
            }
        }
        Ok(Self {
            hostname: identity.hostname,
            tailscale_ip: identity.tailscale_ip,
            iroh_endpoint_id: identity.iroh_endpoint_id,
        })
    }
}

pub trait SessionIo: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T> SessionIo for T where T: AsyncRead + AsyncWrite + Unpin + Send {}

pub type BoxedSessionIo = Box<dyn SessionIo>;

pub struct SecureConnection {
    stream: BoxedSessionIo,
    transport: TransportState,
    read_buffer: Vec<u8>,
    partial_header: [u8; 2],
    partial_header_len: usize,
    partial_record: Vec<u8>,
    partial_expected: Option<usize>,
    peer_identity: PeerIdentity,
}

pub struct AcceptedConnection {
    pub connection: SecureConnection,
    pub peer_identity: PeerIdentity,
    pub remote_public_key: Vec<u8>,
    pub handshake_hash: Vec<u8>,
    pub purpose: HandshakePurpose,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandshakePurpose {
    Connection,
    Pairing,
}

impl SecureConnection {
    pub fn peer_identity(&self) -> &PeerIdentity {
        &self.peer_identity
    }

    pub async fn read_frame(&mut self) -> Result<Frame, ProtocolError> {
        self.read_frame_with_admission(|_, _| Ok(())).await
    }

    pub async fn read_frame_with_admission(
        &mut self,
        admission: impl FnOnce(Command, usize) -> Result<(), ProtocolError>,
    ) -> Result<Frame, ProtocolError> {
        let mut admission = Some(admission);
        let mut expected_size = None;
        loop {
            if expected_size.is_none() {
                if let Some((command, payload_length, total_size)) =
                    self.pending_frame_metadata()?
                {
                    if let Some(admit) = admission.take() {
                        admit(command, payload_length)?;
                    }
                    expected_size = Some(total_size);
                }
            }
            if let Some(total_size) = expected_size {
                if self.read_buffer.len() >= total_size {
                    let frame_bytes: Vec<u8> = self.read_buffer.drain(..total_size).collect();
                    return Frame::decode(&frame_bytes).map(|(frame, _)| frame);
                }
            }
            self.read_transport_record().await?;
        }
    }

    pub async fn write_frame(
        &mut self,
        frame: &Frame,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if frame.payload.len() > frame.command.payload_limit() {
            return Err(ProtocolError::CommandPayloadTooLarge {
                command: frame.command,
                actual: frame.payload.len(),
                limit: frame.command.payload_limit(),
            }
            .into());
        }

        let encoded = frame.encode();
        for chunk in encoded.chunks(MAX_TRANSPORT_PLAINTEXT) {
            let mut encrypted = vec![0u8; chunk.len() + 32];
            let length = self.transport.write_message(chunk, &mut encrypted)?;
            encrypted.truncate(length);
            let record_length = u16::try_from(encrypted.len())
                .map_err(|_| "Encrypted transport record is too large")?;
            write_all_with_timeout(&mut self.stream, &record_length.to_be_bytes()).await?;
            write_all_with_timeout(&mut self.stream, &encrypted).await?;
        }
        flush_with_timeout(&mut self.stream).await?;
        Ok(())
    }

    /// Finish the underlying write side after the application protocol has
    /// completed its final acknowledgement exchange. Iroh waits for the
    /// peer's transport acknowledgement here so the final frame is not lost
    /// when the stream is dropped immediately afterwards.
    pub async fn shutdown(&mut self) -> std::io::Result<()> {
        self.stream.shutdown().await
    }

    fn pending_frame_metadata(&self) -> Result<Option<(Command, usize, usize)>, ProtocolError> {
        if self.read_buffer.len() < protocol::HEADER_SIZE {
            return Ok(None);
        }
        if self.read_buffer[..4] != protocol::MAGIC {
            return Err(ProtocolError::InvalidMagic);
        }
        if self.read_buffer[4] != protocol::VERSION {
            return Err(ProtocolError::UnsupportedVersion(self.read_buffer[4]));
        }
        let command_code = u16::from_be_bytes([self.read_buffer[6], self.read_buffer[7]]);
        let command =
            Command::from_u16(command_code).ok_or(ProtocolError::UnknownCommand(command_code))?;
        let payload_length = u32::from_be_bytes([
            self.read_buffer[12],
            self.read_buffer[13],
            self.read_buffer[14],
            self.read_buffer[15],
        ]) as usize;
        let limit = command.payload_limit();
        if payload_length > limit {
            return Err(ProtocolError::CommandPayloadTooLarge {
                command,
                actual: payload_length,
                limit,
            });
        }
        let total_size = protocol::HEADER_SIZE
            .checked_add(payload_length)
            .and_then(|size| size.checked_add(protocol::CHECKSUM_SIZE))
            .ok_or(ProtocolError::PayloadTooLarge(payload_length))?;
        Ok(Some((command, payload_length, total_size)))
    }

    async fn read_transport_record(&mut self) -> Result<(), ProtocolError> {
        while self.partial_header_len < self.partial_header.len() {
            let read = self
                .stream
                .read(&mut self.partial_header[self.partial_header_len..])
                .await?;
            if read == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "early eof while reading encrypted record length",
                )
                .into());
            }
            self.partial_header_len += read;
        }

        if self.partial_expected.is_none() {
            let encrypted_length = u16::from_be_bytes(self.partial_header) as usize;
            if !(16..=MAX_TRANSPORT_RECORD).contains(&encrypted_length) {
                return Err(ProtocolError::InvalidEncryptedRecord);
            }
            self.partial_record.clear();
            self.partial_record.reserve(encrypted_length);
            self.partial_expected = Some(encrypted_length);
        }

        let expected = self
            .partial_expected
            .expect("encrypted record length set above");
        let mut chunk = [0u8; 8192];
        while self.partial_record.len() < expected {
            let remaining = expected - self.partial_record.len();
            let chunk_len = remaining.min(chunk.len());
            let read = self.stream.read(&mut chunk[..chunk_len]).await?;
            if read == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "early eof while reading encrypted record",
                )
                .into());
            }
            self.partial_record.extend_from_slice(&chunk[..read]);
        }

        let encrypted = std::mem::take(&mut self.partial_record);
        self.partial_expected = None;
        self.partial_header_len = 0;
        let mut plaintext = vec![0u8; encrypted.len()];
        let length = self
            .transport
            .read_message(&encrypted, &mut plaintext)
            .map_err(|error| ProtocolError::TransportEncryption(error.to_string()))?;
        plaintext.truncate(length);
        self.read_buffer.extend_from_slice(&plaintext);
        Ok(())
    }
}

mod handshake;

use handshake::{flush_with_timeout, write_all_with_timeout};

pub use handshake::{
    accept, accept_with_pairing_window, connect, connect_pairing, write_error, write_ready,
};

#[cfg(test)]
use handshake::{build_handshake, read_plain_frame};

pub fn decode_trusted_key(encoded: &str) -> Result<Vec<u8>, String> {
    identity::decode_public_key(encoded).map_err(|error| error.to_string())
}

pub fn fingerprint(public_key: &[u8]) -> String {
    identity::fingerprint(public_key)
}

#[cfg(test)]
mod tests;
