use snow::{Builder, HandshakeState};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::watch;
use tokio::time::timeout;

use crate::identity::{DeviceIdentity, NOISE_PROTOCOL};
use crate::protocol::{self, Command, Frame, ProtocolError};

use super::{
    AcceptedConnection, HandshakePurpose, PeerIdentity, SecureConnection, SessionIo,
    TRANSPORT_WRITE_IDLE_TIMEOUT,
};

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
    const ACCEPT_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);
    let mut handshake = build_handshake(identity, false)?;
    let mut output = vec![0u8; protocol::MAX_HANDSHAKE_PAYLOAD_SIZE];
    let request_result = timeout(
        ACCEPT_HANDSHAKE_TIMEOUT,
        read_plain_frame(&mut stream, protocol::MAX_HANDSHAKE_PAYLOAD_SIZE),
    )
    .await
    .map_err(|_| "secure handshake request timed out")?;
    let request = match request_result {
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
        timeout(
            ACCEPT_HANDSHAKE_TIMEOUT,
            read_pairing_frame(
                &mut stream,
                protocol::MAX_HANDSHAKE_PAYLOAD_SIZE,
                pairing_enabled.as_mut(),
            ),
        )
        .await
        .map_err(|_| "secure pairing handshake timed out")??
    } else {
        timeout(
            ACCEPT_HANDSHAKE_TIMEOUT,
            read_plain_frame(&mut stream, protocol::MAX_HANDSHAKE_PAYLOAD_SIZE),
        )
        .await
        .map_err(|_| "secure handshake finish timed out")??
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

pub(super) fn build_handshake(
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
    write_all_with_timeout(stream, &frame.encode()).await?;
    flush_with_timeout(stream).await?;
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
    write_all_with_timeout(stream, &frame.encode_with_version(version)).await?;
    flush_with_timeout(stream).await?;
    Ok(())
}

pub(super) async fn write_all_with_timeout<S>(stream: &mut S, bytes: &[u8]) -> std::io::Result<()>
where
    S: AsyncWrite + Unpin + ?Sized,
{
    timeout(TRANSPORT_WRITE_IDLE_TIMEOUT, stream.write_all(bytes))
        .await
        .map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "timed out writing transport data",
            )
        })?
}

pub(super) async fn flush_with_timeout<S>(stream: &mut S) -> std::io::Result<()>
where
    S: AsyncWrite + Unpin + ?Sized,
{
    timeout(TRANSPORT_WRITE_IDLE_TIMEOUT, stream.flush())
        .await
        .map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "timed out flushing transport data",
            )
        })?
}

pub(super) async fn read_plain_frame<S>(
    stream: &mut S,
    max_payload: usize,
) -> Result<Frame, ProtocolError>
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
