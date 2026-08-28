use serde::de::Error as DeserializeError;
use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use snow::{Builder, HandshakeState, TransportState};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::watch;

use crate::identity::{self, DeviceIdentity, NOISE_PROTOCOL};
use crate::protocol::{self, Command, Frame, ProtocolError};

const MAX_TRANSPORT_RECORD: usize = u16::MAX as usize;
const MAX_TRANSPORT_PLAINTEXT: usize = MAX_TRANSPORT_RECORD - 16;

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
            self.stream.write_all(&record_length.to_be_bytes()).await?;
            self.stream.write_all(&encrypted).await?;
        }
        self.stream.flush().await?;
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

pub async fn connect<S>(
    mut stream: S,
    identity: &DeviceIdentity,
    local_info: PeerIdentity,
    expected_hostname: &str,
    expected_public_key: &[u8],
) -> Result<SecureConnection, Box<dyn std::error::Error + Send + Sync>>
where
    S: SessionIo + 'static,
{
    let mut handshake = build_handshake(identity, true)?;
    let mut output = vec![0u8; protocol::MAX_HANDSHAKE_PAYLOAD_SIZE];
    let length = handshake.write_message(&[], &mut output)?;
    write_plain_frame(&mut stream, Command::HandshakeReq, &output[..length]).await?;

    let ack = read_handshake_response(&mut stream).await?;
    if ack.command == Command::PeerError {
        return Err(String::from_utf8_lossy(&ack.payload).to_string().into());
    }
    if ack.command != Command::HandshakeAck {
        return Err("Handshake rejected".into());
    }
    let length = handshake.read_message(&ack.payload, &mut output)?;
    let peer_info: PeerIdentity = serde_json::from_slice(&output[..length])?;
    validate_peer_identity(&peer_info)?;
    let remote_key = handshake
        .get_remote_static()
        .ok_or("Responder did not provide a static identity")?;
    if peer_info.hostname != expected_hostname || remote_key != expected_public_key {
        return Err(format!("Peer identity mismatch for {expected_hostname}").into());
    }

    output.fill(0);
    let local_payload = serde_json::to_vec(&local_info)?;
    let length = handshake.write_message(&local_payload, &mut output)?;
    write_plain_frame(&mut stream, Command::HandshakeFinish, &output[..length]).await?;

    let transport = handshake.into_transport_mode()?;
    let mut secure = SecureConnection {
        stream: Box::new(stream),
        transport,
        read_buffer: Vec::new(),
        partial_header: [0; 2],
        partial_header_len: 0,
        partial_record: Vec::new(),
        partial_expected: None,
        peer_identity: peer_info,
    };
    let ready = secure.read_frame().await?;
    match ready.command {
        Command::HandshakeReady => Ok(secure),
        Command::PeerError => Err(String::from_utf8_lossy(&ready.payload).to_string().into()),
        _ => Err("Secure handshake confirmation missing".into()),
    }
}

pub async fn connect_pairing<S>(
    mut stream: S,
    identity: &DeviceIdentity,
    local_info: PeerIdentity,
) -> Result<AcceptedConnection, Box<dyn std::error::Error + Send + Sync>>
where
    S: SessionIo + 'static,
{
    let mut handshake = build_handshake(identity, true)?;
    let mut output = vec![0u8; protocol::MAX_HANDSHAKE_PAYLOAD_SIZE];
    let length = handshake.write_message(&[], &mut output)?;
    write_plain_frame(&mut stream, Command::PairingHandshakeReq, &output[..length]).await?;

    let ack = read_handshake_response(&mut stream).await?;
    if ack.command == Command::PeerError {
        return Err(String::from_utf8_lossy(&ack.payload).to_string().into());
    }
    if ack.command != Command::PairingHandshakeAck {
        return Err("Pairing handshake rejected".into());
    }
    let length = handshake.read_message(&ack.payload, &mut output)?;
    let peer_identity: PeerIdentity = serde_json::from_slice(&output[..length])?;
    validate_peer_identity(&peer_identity)?;
    let remote_public_key = handshake
        .get_remote_static()
        .ok_or("Responder did not provide a static identity")?
        .to_vec();

    output.fill(0);
    let local_payload = serde_json::to_vec(&local_info)?;
    let length = handshake.write_message(&local_payload, &mut output)?;
    write_plain_frame(
        &mut stream,
        Command::PairingHandshakeFinish,
        &output[..length],
    )
    .await?;
    let handshake_hash = handshake.get_handshake_hash().to_vec();
    let transport = handshake.into_transport_mode()?;
    let mut connection = SecureConnection {
        stream: Box::new(stream),
        transport,
        read_buffer: Vec::new(),
        partial_header: [0; 2],
        partial_header_len: 0,
        partial_record: Vec::new(),
        partial_expected: None,
        peer_identity: peer_identity.clone(),
    };
    let ready = connection.read_frame().await?;
    match ready.command {
        Command::HandshakeReady => Ok(AcceptedConnection {
            connection,
            peer_identity,
            remote_public_key,
            handshake_hash,
            purpose: HandshakePurpose::Pairing,
        }),
        Command::PeerError => Err(String::from_utf8_lossy(&ready.payload).to_string().into()),
        _ => Err("Secure pairing confirmation missing".into()),
    }
}

#[allow(dead_code)]
#[doc(hidden)]
pub async fn accept<S>(
    stream: S,
    identity: &DeviceIdentity,
    local_info: PeerIdentity,
) -> Result<AcceptedConnection, Box<dyn std::error::Error + Send + Sync>>
where
    S: SessionIo + 'static,
{
    accept_inner(stream, identity, local_info, None).await
}

pub async fn accept_with_pairing_window<S>(
    stream: S,
    identity: &DeviceIdentity,
    local_info: PeerIdentity,
    pairing_enabled: watch::Receiver<bool>,
) -> Result<AcceptedConnection, Box<dyn std::error::Error + Send + Sync>>
where
    S: SessionIo + 'static,
{
    accept_inner(stream, identity, local_info, Some(pairing_enabled)).await
}

async fn accept_inner<S>(
    mut stream: S,
    identity: &DeviceIdentity,
    local_info: PeerIdentity,
    mut pairing_enabled: Option<watch::Receiver<bool>>,
) -> Result<AcceptedConnection, Box<dyn std::error::Error + Send + Sync>>
where
    S: SessionIo + 'static,
{
    let mut handshake = build_handshake(identity, false)?;
    let mut output = vec![0u8; protocol::MAX_HANDSHAKE_PAYLOAD_SIZE];
    let request = match read_plain_frame(&mut stream, protocol::MAX_HANDSHAKE_PAYLOAD_SIZE).await {
        Ok(request) => request,
        Err(ProtocolError::UnsupportedVersion(peer_version)) => {
            let message = ProtocolError::UnsupportedVersion(peer_version).to_string();
            let _ = write_plain_frame_with_version(
                &mut stream,
                Command::PeerError,
                message.as_bytes(),
                peer_version,
            )
            .await;
            return Err(message.into());
        }
        Err(error) => return Err(error.into()),
    };
    let purpose = match request.command {
        Command::HandshakeReq => HandshakePurpose::Connection,
        Command::PairingHandshakeReq => HandshakePurpose::Pairing,
        _ => return Err("Expected handshake request".into()),
    };
    if purpose == HandshakePurpose::Pairing
        && pairing_enabled
            .as_ref()
            .is_some_and(|enabled| !*enabled.borrow())
    {
        let message = "Pairing window is closed";
        write_plain_frame(&mut stream, Command::PeerError, message.as_bytes()).await?;
        return Err(message.into());
    }
    handshake.read_message(&request.payload, &mut output)?;

    let local_payload = serde_json::to_vec(&local_info)?;
    let length = handshake.write_message(&local_payload, &mut output)?;
    let ack_command = match purpose {
        HandshakePurpose::Connection => Command::HandshakeAck,
        HandshakePurpose::Pairing => Command::PairingHandshakeAck,
    };
    write_plain_frame(&mut stream, ack_command, &output[..length]).await?;

    let finish = if purpose == HandshakePurpose::Pairing {
        read_pairing_frame(
            &mut stream,
            protocol::MAX_HANDSHAKE_PAYLOAD_SIZE,
            pairing_enabled.as_mut(),
        )
        .await?
    } else {
        read_plain_frame(&mut stream, protocol::MAX_HANDSHAKE_PAYLOAD_SIZE).await?
    };
    let expected_finish = match purpose {
        HandshakePurpose::Connection => Command::HandshakeFinish,
        HandshakePurpose::Pairing => Command::PairingHandshakeFinish,
    };
    if finish.command != expected_finish {
        return Err("Expected handshake finish".into());
    }
    let length = handshake.read_message(&finish.payload, &mut output)?;
    let peer_info: PeerIdentity = serde_json::from_slice(&output[..length])?;
    validate_peer_identity(&peer_info)?;
    let remote_key = handshake
        .get_remote_static()
        .ok_or("Initiator did not provide a static identity")?
        .to_vec();
    let handshake_hash = handshake.get_handshake_hash().to_vec();
    let transport = handshake.into_transport_mode()?;
    Ok(AcceptedConnection {
        connection: SecureConnection {
            stream: Box::new(stream),
            transport,
            read_buffer: Vec::new(),
            partial_header: [0; 2],
            partial_header_len: 0,
            partial_record: Vec::new(),
            partial_expected: None,
            peer_identity: peer_info.clone(),
        },
        peer_identity: peer_info,
        remote_public_key: remote_key,
        handshake_hash,
        purpose,
    })
}

async fn read_pairing_frame<S>(
    stream: &mut S,
    max_payload: usize,
    pairing_enabled: Option<&mut watch::Receiver<bool>>,
) -> Result<Frame, Box<dyn std::error::Error + Send + Sync>>
where
    S: SessionIo,
{
    let Some(pairing_enabled) = pairing_enabled else {
        return Ok(read_plain_frame(stream, max_payload).await?);
    };
    if !*pairing_enabled.borrow() {
        return Err("Pairing window is closed".into());
    }
    tokio::select! {
        result = read_plain_frame(stream, max_payload) => Ok(result?),
        changed = pairing_enabled.changed() => {
            match changed {
                Ok(()) if !*pairing_enabled.borrow() => Err("Pairing window was closed".into()),
                Ok(()) => Ok(read_plain_frame(stream, max_payload).await?),
                Err(_) => Err("Pairing window was closed".into()),
            }
        }
    }
}

pub async fn write_ready(
    secure: &mut SecureConnection,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let frame = Frame::try_new(Command::HandshakeReady, 0, 0, Vec::new())?;
    secure.write_frame(&frame).await
}

pub async fn write_error(
    secure: &mut SecureConnection,
    message: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let payload = message.as_bytes();
    let length = payload.len().min(protocol::MAX_CONTROL_PAYLOAD_SIZE);
    let frame = Frame::try_new(Command::PeerError, 0, 0, payload[..length].to_vec())?;
    secure.write_frame(&frame).await
}

fn build_handshake(
    identity: &DeviceIdentity,
    initiator: bool,
) -> Result<HandshakeState, Box<dyn std::error::Error + Send + Sync>> {
    let params = NOISE_PROTOCOL.parse()?;
    let builder = Builder::new(params).local_private_key(identity.private_key())?;
    if initiator {
        Ok(builder.build_initiator()?)
    } else {
        Ok(builder.build_responder()?)
    }
}

fn validate_peer_identity(
    identity: &PeerIdentity,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if identity.hostname.is_empty() || identity.hostname.len() > 255 {
        return Err("Invalid peer hostname".into());
    }
    if identity.tailscale_ip.len() > 64 {
        return Err("Invalid peer address metadata".into());
    }
    if let Some(endpoint_id) = &identity.iroh_endpoint_id {
        crate::iroh_transport::canonical_endpoint_id(endpoint_id)?;
    }
    Ok(())
}

async fn write_plain_frame<S>(
    stream: &mut S,
    command: Command,
    payload: &[u8],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    S: AsyncWrite + Unpin + Send,
{
    if payload.len() > command.payload_limit() {
        return Err(ProtocolError::CommandPayloadTooLarge {
            command,
            actual: payload.len(),
            limit: command.payload_limit(),
        }
        .into());
    }
    let frame = Frame::try_new(command, 0, 0, payload.to_vec())?;
    stream.write_all(&frame.encode()).await?;
    stream.flush().await?;
    Ok(())
}

async fn read_handshake_response<S>(
    stream: &mut S,
) -> Result<Frame, Box<dyn std::error::Error + Send + Sync>>
where
    S: AsyncRead + Unpin + Send,
{
    read_plain_frame(stream, protocol::MAX_HANDSHAKE_PAYLOAD_SIZE)
        .await
        .map_err(Into::into)
}

async fn write_plain_frame_with_version<S>(
    stream: &mut S,
    command: Command,
    payload: &[u8],
    version: u8,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    S: AsyncWrite + Unpin + Send,
{
    if payload.len() > command.payload_limit() {
        return Err(ProtocolError::CommandPayloadTooLarge {
            command,
            actual: payload.len(),
            limit: command.payload_limit(),
        }
        .into());
    }
    let frame = Frame::try_new(command, 0, 0, payload.to_vec())?;
    stream
        .write_all(&frame.encode_with_version(version))
        .await?;
    stream.flush().await?;
    Ok(())
}

async fn read_plain_frame<S>(stream: &mut S, max_payload: usize) -> Result<Frame, ProtocolError>
where
    S: AsyncRead + Unpin + Send,
{
    let mut header = [0u8; protocol::HEADER_SIZE];
    stream.read_exact(&mut header).await?;
    if header[..4] != protocol::MAGIC {
        return Err(ProtocolError::InvalidMagic);
    }
    if header[4] != protocol::VERSION {
        let payload_length =
            u32::from_be_bytes([header[12], header[13], header[14], header[15]]) as usize;
        if payload_length > max_payload {
            return Err(ProtocolError::PayloadTooLarge(payload_length));
        }
        let remaining_length = payload_length
            .checked_add(protocol::CHECKSUM_SIZE)
            .ok_or(ProtocolError::PayloadTooLarge(payload_length))?;
        let mut remaining = vec![0u8; remaining_length];
        stream.read_exact(&mut remaining).await?;
        return Err(ProtocolError::UnsupportedVersion(header[4]));
    }
    let command_code = u16::from_be_bytes([header[6], header[7]]);
    let command =
        Command::from_u16(command_code).ok_or(ProtocolError::UnknownCommand(command_code))?;
    let payload_length =
        u32::from_be_bytes([header[12], header[13], header[14], header[15]]) as usize;
    let limit = max_payload.min(command.payload_limit());
    if payload_length > limit {
        return Err(ProtocolError::CommandPayloadTooLarge {
            command,
            actual: payload_length,
            limit,
        });
    }
    let mut encoded =
        Vec::with_capacity(protocol::HEADER_SIZE + payload_length + protocol::CHECKSUM_SIZE);
    encoded.extend_from_slice(&header);
    let mut payload = vec![0u8; payload_length];
    stream.read_exact(&mut payload).await?;
    encoded.extend_from_slice(&payload);
    let mut checksum = [0u8; protocol::CHECKSUM_SIZE];
    stream.read_exact(&mut checksum).await?;
    encoded.extend_from_slice(&checksum);
    Frame::decode(&encoded).map(|(frame, _)| frame)
}

pub fn decode_trusted_key(encoded: &str) -> Result<Vec<u8>, String> {
    identity::decode_public_key(encoded).map_err(|error| error.to_string())
}

pub fn fingerprint(public_key: &[u8]) -> String {
    identity::fingerprint(public_key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::DeviceIdentity;
    use crate::pairing::derive_verification_code;
    use std::time::Duration;
    use tokio::io::AsyncWriteExt;
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::Notify;

    fn transport_pair() -> (TransportState, TransportState) {
        let initiator_identity = DeviceIdentity::generate_for_test();
        let responder_identity = DeviceIdentity::generate_for_test();
        let mut initiator = build_handshake(&initiator_identity, true).unwrap();
        let mut responder = build_handshake(&responder_identity, false).unwrap();
        let mut message = vec![0u8; protocol::MAX_HANDSHAKE_PAYLOAD_SIZE];
        let mut plaintext = vec![0u8; protocol::MAX_HANDSHAKE_PAYLOAD_SIZE];

        let length = initiator.write_message(&[], &mut message).unwrap();
        responder
            .read_message(&message[..length], &mut plaintext)
            .unwrap();
        let length = responder.write_message(&[], &mut message).unwrap();
        initiator
            .read_message(&message[..length], &mut plaintext)
            .unwrap();
        let length = initiator.write_message(&[], &mut message).unwrap();
        responder
            .read_message(&message[..length], &mut plaintext)
            .unwrap();

        (
            initiator.into_transport_mode().unwrap(),
            responder.into_transport_mode().unwrap(),
        )
    }

    fn encrypted_record(transport: &mut TransportState, frame: &Frame) -> Vec<u8> {
        let encoded = frame.encode();
        let mut encrypted = vec![0u8; encoded.len() + 32];
        let length = transport.write_message(&encoded, &mut encrypted).unwrap();
        encrypted.truncate(length);
        let mut record = Vec::with_capacity(2 + encrypted.len());
        record.extend_from_slice(&(encrypted.len() as u16).to_be_bytes());
        record.extend_from_slice(&encrypted);
        record
    }

    async fn assert_read_resumes_after_cancellation(split_at: usize) {
        let (mut sender_transport, receiver_transport) = transport_pair();
        let expected = Frame::try_new(
            Command::TextPayload,
            0,
            42,
            b"cancel-safe encrypted frame".to_vec(),
        )
        .unwrap();
        let record = encrypted_record(&mut sender_transport, &expected);
        assert!(split_at > 0 && split_at < record.len());

        let (mut writer, reader) = tokio::io::duplex(record.len() * 2);
        let release = std::sync::Arc::new(Notify::new());
        let writer_release = release.clone();
        let sender = tokio::spawn(async move {
            writer.write_all(&record[..split_at]).await.unwrap();
            writer_release.notified().await;
            writer.write_all(&record[split_at..]).await.unwrap();
        });
        let mut secure = SecureConnection {
            stream: Box::new(reader),
            transport: receiver_transport,
            read_buffer: Vec::new(),
            partial_header: [0; 2],
            partial_header_len: 0,
            partial_record: Vec::new(),
            partial_expected: None,
            peer_identity: PeerIdentity {
                hostname: "sender".into(),
                tailscale_ip: String::new(),
                iroh_endpoint_id: None,
            },
        };

        assert!(
            tokio::time::timeout(Duration::from_millis(20), secure.read_frame())
                .await
                .is_err()
        );
        release.notify_one();
        let received = tokio::time::timeout(Duration::from_secs(1), secure.read_frame())
            .await
            .expect("resumed read timed out")
            .expect("resumed read failed");
        sender.await.unwrap();

        assert_eq!(received.command, expected.command);
        assert_eq!(received.sequence, expected.sequence);
        assert_eq!(received.payload, expected.payload);
    }

    #[test]
    fn peer_identity_is_backward_compatible_and_only_serializes_iroh_when_present() {
        let legacy: PeerIdentity =
            serde_json::from_str(r#"{"hostname":"legacy","tailscale_ip":"100.64.0.2"}"#).unwrap();
        assert!(legacy.iroh_endpoint_id.is_none());
        let serialized_legacy = serde_json::to_string(&legacy).unwrap();
        assert!(!serialized_legacy.contains("iroh_endpoint_id"));
        assert!(serialized_legacy.contains("\"protocol_version\":4"));

        let with_iroh = PeerIdentity {
            hostname: "current".into(),
            tailscale_ip: String::new(),
            iroh_endpoint_id: Some(
                "5866666666666666666666666666666666666666666666666666666666666666".into(),
            ),
        };
        assert!(serde_json::to_string(&with_iroh)
            .unwrap()
            .contains("iroh_endpoint_id"));
    }

    #[test]
    fn peer_identity_rejects_an_explicitly_incompatible_wire_version() {
        let error = serde_json::from_str::<PeerIdentity>(
            r#"{"hostname":"old","tailscale_ip":"","protocol_version":2,"app_version":"2.0.2"}"#,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("peer (2.0.2) uses v2"));
        assert!(error.contains("requires v4"));
        assert!(error.contains("Update TailSync on both devices"));
    }

    #[tokio::test]
    async fn noise_handshake_pins_identity_and_round_trips_encrypted_frame() {
        let server_identity = DeviceIdentity::generate_for_test();
        let client_identity = DeviceIdentity::generate_for_test();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server_public = server_identity.public_key().to_vec();
        let client_public = client_identity.public_key().to_vec();
        let server_iroh_endpoint_id =
            "5866666666666666666666666666666666666666666666666666666666666666".to_string();
        let expected_server_iroh_endpoint_id = server_iroh_endpoint_id.clone();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let accepted = accept(
                stream,
                &server_identity,
                PeerIdentity {
                    hostname: "server".into(),
                    tailscale_ip: "127.0.0.1".into(),
                    iroh_endpoint_id: Some(server_iroh_endpoint_id),
                },
            )
            .await
            .unwrap();
            let mut secure = accepted.connection;
            let peer = accepted.peer_identity;
            let remote_key = accepted.remote_public_key;
            assert_eq!(peer.hostname, "client");
            assert_eq!(remote_key, client_public);
            write_ready(&mut secure).await.unwrap();
            secure.read_frame().await.unwrap()
        });

        let stream = TcpStream::connect(address).await.unwrap();
        let mut client = connect(
            stream,
            &client_identity,
            PeerIdentity {
                hostname: "client".into(),
                tailscale_ip: "127.0.0.1".into(),
                iroh_endpoint_id: None,
            },
            "server",
            &server_public,
        )
        .await
        .unwrap();
        assert_eq!(
            client.peer_identity().iroh_endpoint_id.as_deref(),
            Some(expected_server_iroh_endpoint_id.as_str())
        );
        client
            .write_frame(
                &Frame::try_new(Command::TextPayload, 0, 7, b"encrypted clipboard".to_vec())
                    .expect("valid encrypted clipboard fixture"),
            )
            .await
            .unwrap();

        let received = server.await.unwrap();
        assert_eq!(received.command, Command::TextPayload);
        assert_eq!(received.sequence, 7);
        assert_eq!(received.payload, b"encrypted clipboard");
    }

    #[tokio::test]
    async fn encrypted_record_read_resumes_after_length_prefix_cancellation() {
        assert_read_resumes_after_cancellation(1).await;
    }

    #[tokio::test]
    async fn encrypted_record_read_resumes_after_ciphertext_cancellation() {
        assert_read_resumes_after_cancellation(7).await;
    }

    #[tokio::test]
    async fn pairing_handshake_derives_matching_verification_codes() {
        let server_identity = DeviceIdentity::generate_for_test();
        let client_identity = DeviceIdentity::generate_for_test();
        let server_public = server_identity.public_key().to_vec();
        let client_public = client_identity.public_key().to_vec();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();

        let expected_client_public = client_public.clone();
        let server_public_for_task = server_public.clone();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let accepted = accept(
                stream,
                &server_identity,
                PeerIdentity {
                    hostname: "server".into(),
                    tailscale_ip: "127.0.0.1".into(),
                    iroh_endpoint_id: None,
                },
            )
            .await
            .unwrap();
            assert_eq!(accepted.remote_public_key, expected_client_public);
            let code = derive_verification_code(
                &accepted.handshake_hash,
                &server_public_for_task,
                &accepted.remote_public_key,
            )
            .unwrap();
            let mut connection = accepted.connection;
            write_ready(&mut connection).await.unwrap();
            (code, accepted.handshake_hash)
        });

        let accepted = connect_pairing(
            TcpStream::connect(address).await.unwrap(),
            &client_identity,
            PeerIdentity {
                hostname: "client".into(),
                tailscale_ip: "127.0.0.1".into(),
                iroh_endpoint_id: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(accepted.remote_public_key, server_public);
        let client_code = derive_verification_code(
            &accepted.handshake_hash,
            &client_public,
            &accepted.remote_public_key,
        )
        .unwrap();
        let (server_code, server_hash) = server.await.unwrap();

        assert_eq!(accepted.handshake_hash, server_hash);
        assert_eq!(client_code, server_code);

        let mut changed_hash = accepted.handshake_hash.clone();
        changed_hash[0] ^= 0x80;
        assert_ne!(
            client_code,
            derive_verification_code(&changed_hash, &client_public, &server_public).unwrap()
        );

        let mut changed_key = server_public.clone();
        changed_key[0] ^= 0x80;
        assert_ne!(
            client_code,
            derive_verification_code(&accepted.handshake_hash, &client_public, &changed_key)
                .unwrap()
        );
    }

    #[tokio::test]
    async fn closed_pairing_window_returns_a_clear_rejection() {
        let server_identity = DeviceIdentity::generate_for_test();
        let client_identity = DeviceIdentity::generate_for_test();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (_, pairing_window) = watch::channel(false);

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            match accept_with_pairing_window(
                stream,
                &server_identity,
                PeerIdentity {
                    hostname: "server".into(),
                    tailscale_ip: "127.0.0.1".into(),
                    iroh_endpoint_id: None,
                },
                pairing_window,
            )
            .await
            {
                Ok(_) => panic!("closed pairing window accepted a connection"),
                Err(error) => error.to_string(),
            }
        });

        let error = match connect_pairing(
            TcpStream::connect(address).await.unwrap(),
            &client_identity,
            PeerIdentity {
                hostname: "client".into(),
                tailscale_ip: "127.0.0.1".into(),
                iroh_endpoint_id: None,
            },
        )
        .await
        {
            Ok(_) => panic!("closed pairing window accepted a connection"),
            Err(error) => error.to_string(),
        };

        assert_eq!(error, "Pairing window is closed");
        assert_eq!(server.await.unwrap(), "Pairing window is closed");
    }

    #[tokio::test]
    async fn encrypted_transport_fragments_a_full_file_chunk() {
        let server_identity = DeviceIdentity::generate_for_test();
        let client_identity = DeviceIdentity::generate_for_test();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server_public = server_identity.public_key().to_vec();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let accepted = accept(
                stream,
                &server_identity,
                PeerIdentity {
                    hostname: "server".into(),
                    tailscale_ip: String::new(),
                    iroh_endpoint_id: None,
                },
            )
            .await
            .unwrap();
            let mut secure = accepted.connection;
            write_ready(&mut secure).await.unwrap();
            secure.read_frame().await.unwrap()
        });

        let mut client = connect(
            TcpStream::connect(address).await.unwrap(),
            &client_identity,
            PeerIdentity {
                hostname: "client".into(),
                tailscale_ip: String::new(),
                iroh_endpoint_id: None,
            },
            "server",
            &server_public,
        )
        .await
        .unwrap();
        let chunk = vec![0xa5; protocol::MAX_FILE_CHUNK_PAYLOAD_SIZE];
        client
            .write_frame(
                &Frame::try_new(Command::FileChunk, 0, 1, chunk.clone())
                    .expect("valid maximum-size chunk fixture"),
            )
            .await
            .unwrap();
        assert_eq!(server.await.unwrap().payload, chunk);
    }

    #[tokio::test]
    async fn noise_handshake_rejects_wrong_pinned_server_key() {
        let server_identity = DeviceIdentity::generate_for_test();
        let client_identity = DeviceIdentity::generate_for_test();
        let wrong_identity = DeviceIdentity::generate_for_test();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            accept(
                stream,
                &server_identity,
                PeerIdentity {
                    hostname: "server".into(),
                    tailscale_ip: "127.0.0.1".into(),
                    iroh_endpoint_id: None,
                },
            )
            .await
        });
        let result = connect(
            TcpStream::connect(address).await.unwrap(),
            &client_identity,
            PeerIdentity {
                hostname: "client".into(),
                tailscale_ip: "127.0.0.1".into(),
                iroh_endpoint_id: None,
            },
            "server",
            wrong_identity.public_key(),
        )
        .await;
        assert!(result.is_err());
        let _ = server.await;
    }

    #[tokio::test]
    async fn oversized_handshake_is_rejected_from_header_before_body_read() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let sender = tokio::spawn(async move {
            let mut stream = TcpStream::connect(address).await.unwrap();
            let mut header = [0u8; protocol::HEADER_SIZE];
            header[..4].copy_from_slice(&protocol::MAGIC);
            header[4] = protocol::VERSION;
            header[6..8].copy_from_slice(&(Command::HandshakeReq as u16).to_be_bytes());
            header[12..16].copy_from_slice(
                &((protocol::MAX_HANDSHAKE_PAYLOAD_SIZE + 1) as u32).to_be_bytes(),
            );
            stream.write_all(&header).await.unwrap();
        });
        let (mut stream, _) = listener.accept().await.unwrap();
        let result = read_plain_frame(&mut stream, protocol::MAX_HANDSHAKE_PAYLOAD_SIZE).await;
        assert!(matches!(
            result,
            Err(ProtocolError::CommandPayloadTooLarge {
                command: Command::HandshakeReq,
                ..
            })
        ));
        sender.await.unwrap();
    }

    #[tokio::test]
    async fn incompatible_handshake_gets_an_actionable_response_in_the_peer_version() {
        let server_identity = DeviceIdentity::generate_for_test();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            accept(
                stream,
                &server_identity,
                PeerIdentity {
                    hostname: "server".into(),
                    tailscale_ip: "127.0.0.1".into(),
                    iroh_endpoint_id: None,
                },
            )
            .await
        });

        let mut stream = TcpStream::connect(address).await.unwrap();
        let legacy_request = Frame::try_new(Command::HandshakeReq, 0, 0, b"legacy".to_vec())
            .unwrap()
            .encode_with_version(2);
        stream.write_all(&legacy_request).await.unwrap();

        let message = ProtocolError::UnsupportedVersion(2).to_string();
        let expected = Frame::try_new(Command::PeerError, 0, 0, message.into_bytes())
            .unwrap()
            .encode_with_version(2);
        let mut response = vec![0; expected.len()];
        stream.read_exact(&mut response).await.unwrap();
        assert_eq!(response, expected);

        let error = match server.await.unwrap() {
            Ok(_) => panic!("incompatible protocol unexpectedly completed the handshake"),
            Err(error) => error.to_string(),
        };
        assert!(error.contains("peer uses v2"));
        assert!(error.contains("Update TailSync on both devices"));
    }

    #[tokio::test]
    async fn peer_closing_the_handshake_remains_a_network_error() {
        let client_identity = DeviceIdentity::generate_for_test();
        let expected_identity = DeviceIdentity::generate_for_test();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();

        let legacy_peer = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = read_plain_frame(&mut stream, protocol::MAX_HANDSHAKE_PAYLOAD_SIZE)
                .await
                .unwrap();
            assert_eq!(request.command, Command::HandshakeReq);
        });

        let result = connect(
            TcpStream::connect(address).await.unwrap(),
            &client_identity,
            PeerIdentity {
                hostname: "client".into(),
                tailscale_ip: "127.0.0.1".into(),
                iroh_endpoint_id: None,
            },
            "legacy",
            expected_identity.public_key(),
        )
        .await;
        let error = match result {
            Ok(_) => panic!("legacy peer unexpectedly completed the handshake"),
            Err(error) => error.to_string(),
        };
        assert!(error.contains("early eof") || error.contains("connection reset"));
        assert!(!error.contains("older TailSync version"));
        assert!(!error.contains("update TailSync on both devices"));
        legacy_peer.await.unwrap();
    }
}
