use super::*;
use crate::identity::DeviceIdentity;
use crate::pairing::derive_verification_code;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{watch, Notify};

struct StalledWriter;

impl AsyncRead for StalledWriter {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Poll::Pending
    }
}

impl AsyncWrite for StalledWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Poll::Pending
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Pending
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

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
        session_id: "test-session".into(),
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

#[tokio::test(start_paused = true)]
async fn encrypted_frame_write_does_not_wait_forever_for_a_stalled_peer() {
    let (_, transport) = transport_pair();
    let mut secure = SecureConnection {
        stream: Box::new(StalledWriter),
        transport,
        read_buffer: Vec::new(),
        partial_header: [0; 2],
        partial_header_len: 0,
        partial_record: Vec::new(),
        partial_expected: None,
        peer_identity: PeerIdentity {
            hostname: "stalled-peer".into(),
            tailscale_ip: String::new(),
            iroh_endpoint_id: None,
        },
        session_id: "test-session".into(),
    };
    let frame = Frame::try_new(Command::TextPayload, 0, 1, b"stalled".to_vec()).unwrap();

    let mut write = Box::pin(secure.write_frame(&frame));
    let result = tokio::select! {
        result = &mut write => Some(result),
        _ = tokio::time::sleep(Duration::from_secs(31)) => None,
    };
    let error = result
        .expect("write_frame remained pending after the idle deadline")
        .expect_err("stalled writer unexpectedly accepted the frame");
    assert_eq!(
        error.downcast_ref::<io::Error>().map(io::Error::kind),
        Some(io::ErrorKind::TimedOut)
    );
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
        derive_verification_code(&accepted.handshake_hash, &client_public, &changed_key).unwrap()
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
        header[12..16]
            .copy_from_slice(&((protocol::MAX_HANDSHAKE_PAYLOAD_SIZE + 1) as u32).to_be_bytes());
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
