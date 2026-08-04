#![allow(dead_code)]

use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::io::Write as _;
use tailsync_core::{identity, protocol, secure};
use tokio::net::{TcpListener, TcpStream};

use identity::DeviceIdentity;
use protocol::{
    Command, EventEnvelope, FileChunkPayload, FileOffset, Frame, MessageId, TransferId,
};
use secure::{HandshakePurpose, PeerIdentity};

type ProbeResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[tokio::main]
async fn main() -> ProbeResult {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("server") => run_server(args.next().as_deref().unwrap_or("127.0.0.1:0")).await,
        Some("client") => {
            let address = args.next().ok_or("missing server address")?;
            let server_key = args.next().ok_or("missing server public key")?;
            run_client(&address, &server_key).await
        }
        _ => Err("usage: interop_probe server [address] | client <address> <server-key>".into()),
    }
}

async fn run_server(bind_address: &str) -> ProbeResult {
    let identity = DeviceIdentity::load_or_create().map_err(|error| error.to_string())?;
    let listener = TcpListener::bind(bind_address).await?;
    println!(
        "READY {} {}",
        listener.local_addr()?,
        STANDARD.encode(identity.public_key())
    );
    std::io::stdout().flush()?;

    let (stream, _) = listener.accept().await?;
    let accepted = secure::accept(
        stream,
        &identity,
        PeerIdentity {
            hostname: "mac-probe".into(),
            tailscale_ip: String::new(),
        },
    )
    .await?;
    if accepted.peer_identity.hostname != "win-probe" {
        return Err("unexpected client identity".into());
    }
    let mut connection = accepted.connection;
    secure::write_ready(&mut connection).await?;

    let inbound = connection.read_frame().await?;
    let envelope = expect_event(&inbound, b"win-to-mac")?;
    connection
        .write_frame(&Frame::try_new(
            Command::EventAck,
            0,
            inbound.sequence,
            envelope.message_id.ack_payload(),
        )?)
        .await?;

    let outbound = EventEnvelope::new(b"mac-to-win".to_vec());
    let outbound_id = outbound.message_id;
    connection
        .write_frame(&Frame::try_new(
            Command::TextPayload,
            0,
            22,
            outbound.encode(),
        )?)
        .await?;
    expect_ack(&connection.read_frame().await?, 22, outbound_id)?;

    let inbound_file = connection.read_frame().await?;
    let inbound_chunk = expect_file_chunk(&inbound_file, TransferId([3; 16]), b"win-file-block")?;
    connection
        .write_frame(&Frame::try_new(
            Command::FileAck,
            0,
            inbound_file.sequence,
            FileOffset {
                transfer_id: inbound_chunk.transfer_id,
                next_offset: inbound_chunk.offset + inbound_chunk.data.len() as u64,
            }
            .encode(),
        )?)
        .await?;

    let outbound_chunk = FileChunkPayload {
        transfer_id: TransferId([4; 16]),
        offset: 1_048_576,
        data: b"mac-file-block".to_vec(),
    };
    connection
        .write_frame(&Frame::try_new(
            Command::FileChunk,
            0,
            44,
            outbound_chunk.encode()?,
        )?)
        .await?;
    expect_file_ack(
        &connection.read_frame().await?,
        44,
        outbound_chunk.transfer_id,
        outbound_chunk.offset + outbound_chunk.data.len() as u64,
    )?;
    drop(connection);
    println!("SERVER_SYNC_OK");
    std::io::stdout().flush()?;

    let (pairing_stream, _) = listener.accept().await?;
    let pairing = secure::accept(
        pairing_stream,
        &identity,
        PeerIdentity {
            hostname: "mac-probe".into(),
            tailscale_ip: String::new(),
        },
    )
    .await?;
    if pairing.purpose != HandshakePurpose::Pairing
        || pairing.peer_identity.hostname != "win-probe"
        || pairing.remote_public_key.len() != 32
        || pairing.handshake_hash.is_empty()
    {
        return Err("invalid cross-project pairing handshake".into());
    }
    let mut pairing_connection = pairing.connection;
    secure::write_ready(&mut pairing_connection).await?;
    let confirmation = pairing_connection.read_frame().await?;
    if confirmation.command != Command::PairingConfirm {
        return Err("missing client pairing confirmation".into());
    }
    pairing_connection
        .write_frame(&Frame::try_new(Command::PairingConfirm, 0, 0, Vec::new())?)
        .await?;
    println!("SERVER_PAIRING_OK");
    Ok(())
}

async fn run_client(address: &str, server_key: &str) -> ProbeResult {
    let identity = DeviceIdentity::load_or_create().map_err(|error| error.to_string())?;
    let expected_key = STANDARD.decode(server_key)?;
    let stream = TcpStream::connect(address).await?;
    let mut connection = secure::connect(
        stream,
        &identity,
        PeerIdentity {
            hostname: "win-probe".into(),
            tailscale_ip: String::new(),
        },
        "mac-probe",
        &expected_key,
    )
    .await?;

    let outbound = EventEnvelope::new(b"win-to-mac".to_vec());
    let outbound_id = outbound.message_id;
    connection
        .write_frame(&Frame::try_new(
            Command::TextPayload,
            0,
            11,
            outbound.encode(),
        )?)
        .await?;
    expect_ack(&connection.read_frame().await?, 11, outbound_id)?;

    let inbound = connection.read_frame().await?;
    let envelope = expect_event(&inbound, b"mac-to-win")?;
    connection
        .write_frame(&Frame::try_new(
            Command::EventAck,
            0,
            inbound.sequence,
            envelope.message_id.ack_payload(),
        )?)
        .await?;

    let outbound_chunk = FileChunkPayload {
        transfer_id: TransferId([3; 16]),
        offset: 0,
        data: b"win-file-block".to_vec(),
    };
    connection
        .write_frame(&Frame::try_new(
            Command::FileChunk,
            0,
            33,
            outbound_chunk.encode()?,
        )?)
        .await?;
    expect_file_ack(
        &connection.read_frame().await?,
        33,
        outbound_chunk.transfer_id,
        outbound_chunk.data.len() as u64,
    )?;

    let inbound_file = connection.read_frame().await?;
    let inbound_chunk = expect_file_chunk(&inbound_file, TransferId([4; 16]), b"mac-file-block")?;
    connection
        .write_frame(&Frame::try_new(
            Command::FileAck,
            0,
            inbound_file.sequence,
            FileOffset {
                transfer_id: inbound_chunk.transfer_id,
                next_offset: inbound_chunk.offset + inbound_chunk.data.len() as u64,
            }
            .encode(),
        )?)
        .await?;
    drop(connection);
    println!("CLIENT_SYNC_OK");

    let pairing_stream = TcpStream::connect(address).await?;
    let mut pairing = secure::connect_pairing(
        pairing_stream,
        &identity,
        PeerIdentity {
            hostname: "win-probe".into(),
            tailscale_ip: String::new(),
        },
    )
    .await?;
    if pairing.purpose != HandshakePurpose::Pairing
        || pairing.peer_identity.hostname != "mac-probe"
        || pairing.remote_public_key != expected_key
        || pairing.handshake_hash.is_empty()
    {
        return Err("invalid pairing responder identity".into());
    }
    pairing
        .connection
        .write_frame(&Frame::try_new(Command::PairingConfirm, 0, 0, Vec::new())?)
        .await?;
    let confirmation = pairing.connection.read_frame().await?;
    if confirmation.command != Command::PairingConfirm {
        return Err("missing server pairing confirmation".into());
    }
    println!("CLIENT_PAIRING_OK");
    Ok(())
}

fn expect_event(frame: &Frame, content: &[u8]) -> ProbeResult<EventEnvelope> {
    if frame.command != Command::TextPayload {
        return Err(format!("expected TextPayload, got {:?}", frame.command).into());
    }
    let envelope = EventEnvelope::decode(&frame.payload)?;
    envelope.validate_timestamp(protocol::unix_timestamp_ms())?;
    if envelope.content != content {
        return Err("event content mismatch".into());
    }
    Ok(envelope)
}

fn expect_ack(frame: &Frame, sequence: u32, message_id: MessageId) -> ProbeResult {
    if frame.command != Command::EventAck
        || frame.sequence != sequence
        || MessageId::from_ack_payload(&frame.payload)? != message_id
    {
        return Err("event acknowledgement mismatch".into());
    }
    Ok(())
}

fn expect_file_chunk(
    frame: &Frame,
    transfer_id: TransferId,
    content: &[u8],
) -> ProbeResult<FileChunkPayload> {
    if frame.command != Command::FileChunk {
        return Err(format!("expected FileChunk, got {:?}", frame.command).into());
    }
    let chunk = FileChunkPayload::decode(&frame.payload)?;
    if chunk.transfer_id != transfer_id || chunk.data != content {
        return Err("file chunk mismatch".into());
    }
    Ok(chunk)
}

fn expect_file_ack(
    frame: &Frame,
    sequence: u32,
    transfer_id: TransferId,
    next_offset: u64,
) -> ProbeResult {
    if frame.command != Command::FileAck || frame.sequence != sequence {
        return Err("file acknowledgement command mismatch".into());
    }
    let offset = FileOffset::decode(&frame.payload)?;
    if offset.transfer_id != transfer_id || offset.next_offset != next_offset {
        return Err("file acknowledgement offset mismatch".into());
    }
    Ok(())
}
