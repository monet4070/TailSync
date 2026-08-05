use serde::{Deserialize, Serialize};
use snow::{Builder, HandshakeState, TransportState};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::watch;

use crate::identity::{self, DeviceIdentity, NOISE_PROTOCOL};
use crate::protocol::{self, Command, Frame, ProtocolError};

const MAX_TRANSPORT_RECORD: usize = u16::MAX as usize;
const MAX_TRANSPORT_PLAINTEXT: usize = MAX_TRANSPORT_RECORD - 16;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerIdentity {
    pub hostname: String,
    pub tailscale_ip: String,
}

pub struct SecureConnection {
    stream: TcpStream,
    transport: TransportState,
    read_buffer: Vec<u8>,
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
        let mut length = [0u8; 2];
        self.stream.read_exact(&mut length).await?;
        let encrypted_length = u16::from_be_bytes(length) as usize;
        if !(16..=MAX_TRANSPORT_RECORD).contains(&encrypted_length) {
            return Err(ProtocolError::InvalidEncryptedRecord);
        }
        let mut encrypted = vec![0u8; encrypted_length];
        self.stream.read_exact(&mut encrypted).await?;
        let mut plaintext = vec![0u8; encrypted_length];
        let length = self
            .transport
            .read_message(&encrypted, &mut plaintext)
            .map_err(|error| ProtocolError::TransportEncryption(error.to_string()))?;
        plaintext.truncate(length);
        self.read_buffer.extend_from_slice(&plaintext);
        Ok(())
    }
}

pub async fn connect(
    mut stream: TcpStream,
    identity: &DeviceIdentity,
    local_info: PeerIdentity,
    expected_hostname: &str,
    expected_public_key: &[u8],
) -> Result<SecureConnection, Box<dyn std::error::Error + Send + Sync>> {
    let mut handshake = build_handshake(identity, true)?;
    let mut output = vec![0u8; protocol::MAX_HANDSHAKE_PAYLOAD_SIZE];
    let length = handshake.write_message(&[], &mut output)?;
    write_plain_frame(&mut stream, Command::HandshakeReq, &output[..length]).await?;

    let ack = read_plain_frame(&mut stream, protocol::MAX_HANDSHAKE_PAYLOAD_SIZE).await?;
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
        stream,
        transport,
        read_buffer: Vec::new(),
    };
    let ready = secure.read_frame().await?;
    match ready.command {
        Command::HandshakeReady => Ok(secure),
        Command::PeerError => Err(String::from_utf8_lossy(&ready.payload).to_string().into()),
        _ => Err("Secure handshake confirmation missing".into()),
    }
}

pub async fn connect_pairing(
    mut stream: TcpStream,
    identity: &DeviceIdentity,
    local_info: PeerIdentity,
) -> Result<AcceptedConnection, Box<dyn std::error::Error + Send + Sync>> {
    let mut handshake = build_handshake(identity, true)?;
    let mut output = vec![0u8; protocol::MAX_HANDSHAKE_PAYLOAD_SIZE];
    let length = handshake.write_message(&[], &mut output)?;
    write_plain_frame(&mut stream, Command::PairingHandshakeReq, &output[..length]).await?;

    let ack = read_plain_frame(&mut stream, protocol::MAX_HANDSHAKE_PAYLOAD_SIZE).await?;
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
        stream,
        transport,
        read_buffer: Vec::new(),
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
pub async fn accept(
    stream: TcpStream,
    identity: &DeviceIdentity,
    local_info: PeerIdentity,
) -> Result<AcceptedConnection, Box<dyn std::error::Error + Send + Sync>> {
    accept_inner(stream, identity, local_info, None).await
}

pub async fn accept_with_pairing_window(
    stream: TcpStream,
    identity: &DeviceIdentity,
    local_info: PeerIdentity,
    pairing_enabled: watch::Receiver<bool>,
) -> Result<AcceptedConnection, Box<dyn std::error::Error + Send + Sync>> {
    accept_inner(stream, identity, local_info, Some(pairing_enabled)).await
}

async fn accept_inner(
    mut stream: TcpStream,
    identity: &DeviceIdentity,
    local_info: PeerIdentity,
    mut pairing_enabled: Option<watch::Receiver<bool>>,
) -> Result<AcceptedConnection, Box<dyn std::error::Error + Send + Sync>> {
    let mut handshake = build_handshake(identity, false)?;
    let mut output = vec![0u8; protocol::MAX_HANDSHAKE_PAYLOAD_SIZE];
    let request = read_plain_frame(&mut stream, protocol::MAX_HANDSHAKE_PAYLOAD_SIZE).await?;
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
            stream,
            transport,
            read_buffer: Vec::new(),
        },
        peer_identity: peer_info,
        remote_public_key: remote_key,
        handshake_hash,
        purpose,
    })
}

async fn read_pairing_frame(
    stream: &mut TcpStream,
    max_payload: usize,
    pairing_enabled: Option<&mut watch::Receiver<bool>>,
) -> Result<Frame, Box<dyn std::error::Error + Send + Sync>> {
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
    let builder = Builder::new(params).local_private_key(identity.private_key());
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
    Ok(())
}

async fn write_plain_frame(
    stream: &mut TcpStream,
    command: Command,
    payload: &[u8],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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

async fn read_plain_frame(
    stream: &mut TcpStream,
    max_payload: usize,
) -> Result<Frame, ProtocolError> {
    let mut header = [0u8; protocol::HEADER_SIZE];
    stream.read_exact(&mut header).await?;
    if header[..4] != protocol::MAGIC {
        return Err(ProtocolError::InvalidMagic);
    }
    if header[4] != protocol::VERSION {
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
    identity::decode_public_key(encoded)
}

pub fn fingerprint(public_key: &[u8]) -> String {
    identity::fingerprint(public_key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::DeviceIdentity;
    use crate::pairing::derive_verification_code;
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn noise_handshake_pins_identity_and_round_trips_encrypted_frame() {
        let server_identity = DeviceIdentity::generate_for_test();
        let client_identity = DeviceIdentity::generate_for_test();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server_public = server_identity.public_key().to_vec();
        let client_public = client_identity.public_key().to_vec();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let accepted = accept(
                stream,
                &server_identity,
                PeerIdentity {
                    hostname: "server".into(),
                    tailscale_ip: "127.0.0.1".into(),
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
            },
            "server",
            &server_public,
        )
        .await
        .unwrap();
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
}
