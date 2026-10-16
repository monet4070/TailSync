use super::*;

/// S3-P1-1 has been adjudicated as accepted (no behaviour change): an anonymous
/// Noise XX initiator receives these fields in message 2, before the responder has
/// authenticated it. `docs/security/anonymous-visible-fields.md` records the
/// accepted set, including the fact that discovery already publishes the hostname
/// and Iroh id, so only `tailscale_ip` and the version fields are incremental.
/// Changing this set must be deliberate.
#[test]
fn anonymous_handshake_identity_fields_are_pinned() {
    let identity = PeerIdentity {
        hostname: "test-host".into(),
        tailscale_ip: "100.64.0.7".into(),
        iroh_endpoint_id: Some("endpoint-id".into()),
    };
    let value = serde_json::to_value(&identity).unwrap();
    let mut keys: Vec<&str> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "app_version",
            "hostname",
            "iroh_endpoint_id",
            "protocol_version",
            "tailscale_ip"
        ],
        "the anonymous-visible handshake field set changed; update docs/security/anonymous-visible-fields.md"
    );

    // Without an Iroh endpoint the key is omitted rather than sent empty.
    let without_iroh = PeerIdentity {
        hostname: "test-host".into(),
        tailscale_ip: String::new(),
        iroh_endpoint_id: None,
    };
    let value = serde_json::to_value(&without_iroh).unwrap();
    assert!(!value.as_object().unwrap().contains_key("iroh_endpoint_id"));
}
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

async fn negotiate_capabilities_for_one_session(
    server_capabilities: CapabilitySet,
    client_capabilities: CapabilitySet,
) -> (CapabilitySet, CapabilitySet) {
    let server_identity = DeviceIdentity::generate_for_test();
    let client_identity = DeviceIdentity::generate_for_test();
    let server_public = server_identity.public_key().to_vec();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let accepted = accept_with_test_capabilities(
            stream,
            &server_identity,
            PeerIdentity {
                hostname: "server".into(),
                tailscale_ip: String::new(),
                iroh_endpoint_id: None,
            },
            server_capabilities,
        )
        .await
        .unwrap();
        let mut connection = accepted.connection;
        assert_eq!(connection.wire_version(), protocol::VERSION);
        let negotiated = connection.negotiated_capabilities();
        write_ready(&mut connection).await.unwrap();
        negotiated
    });
    let client = connect_with_test_capabilities(
        TcpStream::connect(address).await.unwrap(),
        &client_identity,
        PeerIdentity {
            hostname: "client".into(),
            tailscale_ip: String::new(),
            iroh_endpoint_id: None,
        },
        "server",
        &server_public,
        client_capabilities,
    )
    .await
    .unwrap();
    assert_eq!(client.wire_version(), protocol::VERSION);
    (server.await.unwrap(), client.negotiated_capabilities())
}

#[tokio::test]
async fn current_client_uses_v4_data_frames_with_legacy_server() {
    let server_identity = DeviceIdentity::generate_for_test();
    let client_identity = DeviceIdentity::generate_for_test();
    let server_public = server_identity.public_key().to_vec();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut handshake = build_handshake(&server_identity, false).unwrap();
        let mut output = vec![0; protocol::MAX_HANDSHAKE_PAYLOAD_SIZE];
        let request = read_plain_frame(&mut stream, protocol::MAX_HANDSHAKE_PAYLOAD_SIZE)
            .await
            .unwrap();
        handshake
            .read_message(&request.payload, &mut output)
            .unwrap();
        let bare_identity = serde_json::to_vec(&PeerIdentity {
            hostname: "legacy".into(),
            tailscale_ip: String::new(),
            iroh_endpoint_id: None,
        })
        .unwrap();
        let length = handshake
            .write_message(&bare_identity, &mut output)
            .unwrap();
        let ack = Frame::try_new(Command::HandshakeAck, 0, 0, output[..length].to_vec()).unwrap();
        stream
            .write_all(&ack.encode_with_version(protocol::LEGACY_VERSION))
            .await
            .unwrap();
        let finish = read_plain_frame(&mut stream, protocol::MAX_HANDSHAKE_PAYLOAD_SIZE)
            .await
            .unwrap();
        handshake
            .read_message(&finish.payload, &mut output)
            .unwrap();
        let mut connection = SecureConnection {
            remote_public_key: vec![0; 32],
            stream: Box::new(stream),
            transport: handshake.into_transport_mode().unwrap(),
            read_buffer: Vec::new(),
            partial_header: [0; 2],
            partial_header_len: 0,
            partial_record: Vec::new(),
            partial_expected: None,
            peer_identity: PeerIdentity {
                hostname: "current".into(),
                tailscale_ip: String::new(),
                iroh_endpoint_id: None,
            },
            session_id: "legacy-fixture".into(),
            negotiated_capabilities: CapabilitySet::disabled(),
            wire_version: protocol::LEGACY_VERSION,
        };
        write_ready(&mut connection).await.unwrap();
        let frame = connection.read_frame().await.unwrap();
        assert_eq!(frame.command, Command::TextPayload);
        assert_eq!(frame.payload, b"v4 fallback");
    });
    let mut client = connect(
        TcpStream::connect(address).await.unwrap(),
        &client_identity,
        PeerIdentity {
            hostname: "current".into(),
            tailscale_ip: String::new(),
            iroh_endpoint_id: None,
        },
        "legacy",
        &server_public,
    )
    .await
    .unwrap();
    assert_eq!(client.wire_version(), protocol::LEGACY_VERSION);
    client
        .write_frame(&Frame::try_new(Command::TextPayload, 0, 1, b"v4 fallback".to_vec()).unwrap())
        .await
        .unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn current_server_uses_v4_data_frames_with_legacy_client() {
    let server_identity = DeviceIdentity::generate_for_test();
    let client_identity = DeviceIdentity::generate_for_test();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let accepted = accept(
            stream,
            &server_identity,
            PeerIdentity {
                hostname: "current-server".into(),
                tailscale_ip: String::new(),
                iroh_endpoint_id: None,
            },
        )
        .await
        .unwrap();
        let mut connection = accepted.connection;
        assert_eq!(connection.wire_version(), protocol::LEGACY_VERSION);
        write_ready(&mut connection).await.unwrap();
        let frame = connection.read_frame().await.unwrap();
        assert_eq!(frame.payload, b"legacy to current");
    });
    let mut stream = TcpStream::connect(address).await.unwrap();
    let mut handshake = build_handshake(&client_identity, true).unwrap();
    let mut output = vec![0; protocol::MAX_HANDSHAKE_PAYLOAD_SIZE];
    let length = handshake.write_message(&[], &mut output).unwrap();
    let request = Frame::try_new(Command::HandshakeReq, 0, 0, output[..length].to_vec()).unwrap();
    stream
        .write_all(&request.encode_with_version(protocol::LEGACY_VERSION))
        .await
        .unwrap();
    let ack = read_plain_frame(&mut stream, protocol::MAX_HANDSHAKE_PAYLOAD_SIZE)
        .await
        .unwrap();
    handshake.read_message(&ack.payload, &mut output).unwrap();
    let bare_identity = serde_json::to_vec(&PeerIdentity {
        hostname: "legacy-client".into(),
        tailscale_ip: String::new(),
        iroh_endpoint_id: None,
    })
    .unwrap();
    let length = handshake
        .write_message(&bare_identity, &mut output)
        .unwrap();
    let finish = Frame::try_new(Command::HandshakeFinish, 0, 0, output[..length].to_vec()).unwrap();
    stream
        .write_all(&finish.encode_with_version(protocol::LEGACY_VERSION))
        .await
        .unwrap();
    let mut connection = SecureConnection {
        remote_public_key: vec![0; 32],
        stream: Box::new(stream),
        transport: handshake.into_transport_mode().unwrap(),
        read_buffer: Vec::new(),
        partial_header: [0; 2],
        partial_header_len: 0,
        partial_record: Vec::new(),
        partial_expected: None,
        peer_identity: PeerIdentity {
            hostname: "current-server".into(),
            tailscale_ip: String::new(),
            iroh_endpoint_id: None,
        },
        session_id: "legacy-fixture".into(),
        negotiated_capabilities: CapabilitySet::disabled(),
        wire_version: protocol::LEGACY_VERSION,
    };
    assert_eq!(
        connection.read_frame().await.unwrap().command,
        Command::HandshakeReady
    );
    connection
        .write_frame(
            &Frame::try_new(Command::TextPayload, 0, 1, b"legacy to current".to_vec()).unwrap(),
        )
        .await
        .unwrap();
    server.await.unwrap();
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
        remote_public_key: vec![0; 32],
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
        negotiated_capabilities: CapabilitySet::disabled(),
        wire_version: protocol::VERSION,
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

#[test]
fn capability_advertisements_are_backward_compatible_and_fail_closed() {
    let identity = PeerIdentity {
        hostname: "current".into(),
        tailscale_ip: String::new(),
        iroh_endpoint_id: None,
    };
    let legacy = encode_handshake_advertisement(identity.clone(), CapabilitySet::disabled())
        .expect("encode legacy-compatible identity");
    assert!(!String::from_utf8_lossy(&legacy).contains("capabilities"));
    assert_eq!(
        decode_handshake_advertisement(&legacy)
            .expect("decode legacy-compatible identity")
            .1,
        CapabilitySet::disabled()
    );

    let enabled = CapabilitySet {
        file_sliding_window: true,
        image_compressed_chunks: false,
        pairing_commit_proof: false,
        composite_clipboard: false,
        archive_resource_updates: false,
    };
    let encoded = encode_handshake_advertisement(identity, enabled).expect("encode capabilities");
    assert_eq!(
        decode_handshake_advertisement(&encoded)
            .expect("decode capabilities")
            .1,
        enabled
    );

    let unknown = br#"{"hostname":"future","tailscale_ip":"","protocol_version":4,"capabilities":{"future_extension":true}}"#;
    assert_eq!(
        decode_handshake_advertisement(unknown)
            .expect("unknown capabilities must be ignored")
            .1,
        CapabilitySet::disabled()
    );
    assert_eq!(
        decode_handshake_advertisement(unknown)
            .expect("legacy peers select v4")
            .2,
        protocol::LEGACY_VERSION
    );
    assert_eq!(
        decode_handshake_advertisement(&encoded)
            .expect("modern peers select v5")
            .2,
        protocol::VERSION
    );
    let malformed = br#"{"hostname":"bad","tailscale_ip":"","protocol_version":4,"capabilities":{"file_sliding_window":"yes"}}"#;
    assert!(decode_handshake_advertisement(malformed).is_err());
}

#[test]
fn capability_negotiation_requires_build_runtime_and_peer_support() {
    let requested = CapabilitySwitches {
        file_sliding_window: true,
        image_compressed_chunks: true,
        pairing_commit_proof: false,
        composite_clipboard: false,
    }
    .advertised();
    assert_eq!(
        requested.file_sliding_window,
        cfg!(feature = "protocol-file-sliding-window")
    );
    assert_eq!(
        requested.image_compressed_chunks,
        cfg!(feature = "protocol-image-compressed-chunks")
    );

    let both = CapabilitySet {
        file_sliding_window: true,
        image_compressed_chunks: true,
        pairing_commit_proof: false,
        composite_clipboard: false,
        archive_resource_updates: false,
    };
    assert_eq!(
        CapabilitySet::negotiate(both, CapabilitySet::disabled()),
        CapabilitySet::disabled()
    );
    assert_eq!(
        CapabilitySet::negotiate(
            both,
            CapabilitySet {
                file_sliding_window: true,
                image_compressed_chunks: false,
                pairing_commit_proof: false,
                composite_clipboard: false,
                archive_resource_updates: false,
            }
        ),
        CapabilitySet {
            file_sliding_window: true,
            image_compressed_chunks: false,
            pairing_commit_proof: false,
            composite_clipboard: false,
            archive_resource_updates: false,
        }
    );
}

#[tokio::test]
async fn negotiated_capabilities_are_bound_to_each_authenticated_session() {
    let both = CapabilitySet {
        file_sliding_window: true,
        image_compressed_chunks: true,
        pairing_commit_proof: false,
        composite_clipboard: false,
        archive_resource_updates: false,
    };
    let first = negotiate_capabilities_for_one_session(both, both).await;
    assert_eq!(first, (both, both));

    let image_only = CapabilitySet {
        file_sliding_window: false,
        image_compressed_chunks: true,
        pairing_commit_proof: false,
        composite_clipboard: false,
        archive_resource_updates: false,
    };
    let second = negotiate_capabilities_for_one_session(both, image_only).await;
    assert_eq!(second, (image_only, image_only));
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
        remote_public_key: vec![0; 32],
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
        negotiated_capabilities: CapabilitySet::disabled(),
        wire_version: protocol::VERSION,
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
        header[4] = protocol::LEGACY_VERSION;
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

#[test]
fn oversized_image_chunk_is_rejected_from_decrypted_header_before_body_read() {
    let (_, receiver_transport) = transport_pair();
    let mut header = [0u8; protocol::HEADER_SIZE];
    header[..4].copy_from_slice(&protocol::MAGIC);
    header[4] = protocol::VERSION;
    header[6..8].copy_from_slice(&(Command::ImageChunk as u16).to_be_bytes());
    header[12..16]
        .copy_from_slice(&((protocol::MAX_IMAGE_CHUNK_PAYLOAD_SIZE + 1) as u32).to_be_bytes());
    let (_, reader) = tokio::io::duplex(64);
    let secure = SecureConnection {
        remote_public_key: vec![0; 32],
        stream: Box::new(reader),
        transport: receiver_transport,
        read_buffer: header.to_vec(),
        partial_header: [0; 2],
        partial_header_len: 0,
        partial_record: Vec::new(),
        partial_expected: None,
        peer_identity: PeerIdentity {
            hostname: "sender".into(),
            tailscale_ip: String::new(),
            iroh_endpoint_id: None,
        },
        session_id: "oversized-image-test".into(),
        negotiated_capabilities: CapabilitySet {
            file_sliding_window: false,
            image_compressed_chunks: true,
            pairing_commit_proof: false,
            composite_clipboard: false,
            archive_resource_updates: false,
        },
        wire_version: protocol::VERSION,
    };
    assert!(matches!(
        secure.pending_frame_metadata(),
        Err(ProtocolError::CommandPayloadTooLarge {
            command: Command::ImageChunk,
            actual,
            limit,
        }) if actual == protocol::MAX_IMAGE_CHUNK_PAYLOAD_SIZE + 1
            && limit == protocol::MAX_IMAGE_CHUNK_PAYLOAD_SIZE
    ));
    assert_eq!(secure.read_buffer.len(), protocol::HEADER_SIZE);
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
#[tokio::test]
async fn pairing_runtime_defaults_select_proof_and_gate_unknown_commands() {
    let expected = std::env::var_os("TAILSYNC_DISABLE_PAIRING_COMMIT_PROOF").is_none();
    assert_eq!(
        CapabilitySwitches::runtime_defaults().pairing_commit_proof,
        expected
    );
    let local = DeviceIdentity::generate_for_test();
    let remote = DeviceIdentity::generate_for_test();
    let (a, b) = tokio::io::duplex(256 * 1024);
    let server = tokio::spawn(async move {
        let (_window, enabled) = tokio::sync::watch::channel(true);
        let mut accepted = accept_with_pairing_window_and_capabilities(
            b,
            &local,
            PeerIdentity {
                hostname: "server".into(),
                tailscale_ip: String::new(),
                iroh_endpoint_id: None,
            },
            enabled,
            CapabilitySwitches::runtime_defaults(),
        )
        .await
        .unwrap();
        write_ready(&mut accepted.connection).await.unwrap();
        assert_eq!(
            accepted
                .connection
                .negotiated_capabilities()
                .pairing_commit_proof,
            expected
        );
        if expected {
            assert_eq!(
                accepted.connection.read_frame().await.unwrap().command,
                Command::PairingCommitted
            );
        }
    });
    let mut accepted = connect_pairing(
        a,
        &remote,
        PeerIdentity {
            hostname: "client".into(),
            tailscale_ip: String::new(),
            iroh_endpoint_id: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        accepted
            .connection
            .negotiated_capabilities()
            .pairing_commit_proof,
        expected
    );
    let sent = accepted
        .connection
        .write_frame(&Frame::try_new(Command::PairingCommitted, 0, 0, vec![1; 16]).unwrap())
        .await;
    assert_eq!(sent.is_ok(), expected);
    server.await.unwrap();
}

#[tokio::test]
async fn plaintext_commit_frame_cannot_create_an_authenticated_pairing() {
    let identity = DeviceIdentity::generate_for_test();
    let (mut a, b) = tokio::io::duplex(4096);
    a.write_all(
        &Frame::try_new(Command::PairingCommitted, 0, 0, vec![1; 16])
            .unwrap()
            .encode_with_version(protocol::LEGACY_VERSION),
    )
    .await
    .unwrap();
    a.shutdown().await.unwrap();
    let result = accept(
        b,
        &identity,
        PeerIdentity {
            hostname: "server".into(),
            tailscale_ip: String::new(),
            iroh_endpoint_id: None,
        },
    )
    .await;
    assert!(result.is_err());
}

#[test]
fn composite_runtime_switch_reads_its_kill_switch() {
    let disabled =
        CapabilitySwitches::from_env(|name| name == "TAILSYNC_DISABLE_COMPOSITE_CLIPBOARD");
    assert!(!disabled.composite_clipboard);
    assert!(!disabled.advertised().archive_resource_updates);
    assert!(disabled.file_sliding_window);
    assert!(disabled.image_compressed_chunks);
    assert!(disabled.pairing_commit_proof);
    assert!(CapabilitySwitches::from_env(|_| false).composite_clipboard);
}

#[test]
fn composite_capability_is_bilateral_v5_and_build_runtime_gated() {
    let supported = CapabilitySet {
        composite_clipboard: true,
        ..CapabilitySet::disabled()
    };
    assert!(!CapabilitySet::negotiate(supported, CapabilitySet::disabled()).composite_clipboard);
    assert!(CapabilitySet::negotiate(supported, supported).composite_clipboard);
    assert!(!capabilities_for_wire(supported, protocol::LEGACY_VERSION).composite_clipboard);
    assert!(capabilities_for_wire(supported, protocol::VERSION).composite_clipboard);
    let legacy: CapabilitySet = serde_json::from_str("{}").unwrap();
    assert!(!legacy.composite_clipboard);
    let off = CapabilitySwitches {
        composite_clipboard: false,
        file_sliding_window: true,
        image_compressed_chunks: true,
        pairing_commit_proof: true,
    };
    assert!(!off.advertised().composite_clipboard);
    let on = CapabilitySwitches {
        composite_clipboard: true,
        ..off
    };
    assert_eq!(
        on.advertised().composite_clipboard,
        cfg!(feature = "protocol-composite-clipboard")
    );
}

#[test]
fn unnegotiated_composite_is_rejected_from_header_without_allocating_a_body() {
    for command in [
        Command::CompositePayload,
        Command::CompositeChunk,
        Command::CompositeAck,
        Command::ArchiveUpdatePayload,
        Command::ArchiveUpdateChunk,
        Command::ArchiveUpdateAck,
    ] {
        let (_, receiver_transport) = transport_pair();
        let mut header = [0; protocol::HEADER_SIZE];
        header[..4].copy_from_slice(&protocol::MAGIC);
        header[4] = protocol::VERSION;
        header[6..8].copy_from_slice(&(command as u16).to_be_bytes());
        header[12..16].copy_from_slice(&(command.payload_limit() as u32).to_be_bytes());
        let (_, reader) = tokio::io::duplex(64);
        let secure = SecureConnection {
            remote_public_key: vec![0; 32],
            stream: Box::new(reader),
            transport: receiver_transport,
            read_buffer: header.to_vec(),
            partial_header: [0; 2],
            partial_header_len: 0,
            partial_record: vec![],
            partial_expected: None,
            peer_identity: PeerIdentity {
                hostname: "fixture".into(),
                tailscale_ip: String::new(),
                iroh_endpoint_id: None,
            },
            session_id: "composite-header-fixture".into(),
            negotiated_capabilities: CapabilitySet::disabled(),
            wire_version: protocol::VERSION,
        };
        assert!(matches!(
            secure.pending_frame_metadata(),
            Err(ProtocolError::AdmissionRejected(_))
        ));
        assert_eq!(secure.read_buffer.len(), protocol::HEADER_SIZE);
    }
}

#[test]
fn archive_updates_require_bilateral_composite_and_v5_capabilities() {
    let supported = CapabilitySet {
        composite_clipboard: true,
        archive_resource_updates: true,
        ..CapabilitySet::disabled()
    };
    let legacy: CapabilitySet = serde_json::from_str(r#"{"composite_clipboard":true}"#).unwrap();
    assert!(!legacy.archive_resource_updates);
    assert!(!CapabilitySet::negotiate(supported, legacy).archive_resource_updates);
    assert!(CapabilitySet::negotiate(supported, supported).archive_resource_updates);
    let orphaned = CapabilitySet {
        composite_clipboard: false,
        ..supported
    };
    assert!(!CapabilitySet::negotiate(supported, orphaned).archive_resource_updates);
    assert!(!capabilities_for_wire(supported, protocol::LEGACY_VERSION).archive_resource_updates);
    assert!(capabilities_for_wire(supported, protocol::VERSION).archive_resource_updates);
    let disabled = CapabilitySwitches {
        composite_clipboard: false,
        ..CapabilitySwitches::runtime_defaults()
    };
    assert!(!disabled.advertised().archive_resource_updates);
}

// ------------------------------------------------------------------
// F10-A: typed handshake/connection failure classification. These tests
// drive a real Noise exchange and assert the structural failure *kind*
// survives the `Box<dyn Error>` boundary, so the worker never has to
// recover a classification from display text.
// ------------------------------------------------------------------

fn expect_connect_err(
    result: Result<SecureConnection, Box<dyn std::error::Error + Send + Sync>>,
) -> Box<dyn std::error::Error + Send + Sync> {
    match result {
        Ok(_) => panic!("handshake unexpectedly completed"),
        Err(error) => error,
    }
}

fn typed_failure<'a>(
    error: &'a (dyn std::error::Error + Send + Sync + 'static),
) -> &'a ConnectionFailure {
    error
        .downcast_ref::<ConnectionFailure>()
        .expect("expected a typed ConnectionFailure")
}

fn local_client_identity() -> PeerIdentity {
    PeerIdentity {
        hostname: "client".into(),
        tailscale_ip: String::new(),
        iroh_endpoint_id: None,
    }
}

/// Drive a real client `connect` against a scripted *plain* response frame
/// (no Noise): used for the plain-wire-header and `PeerError` paths.
async fn client_connect_against_plain_response(
    response: Vec<u8>,
) -> Box<dyn std::error::Error + Send + Sync> {
    let client_identity = DeviceIdentity::generate_for_test();
    let expected_identity = DeviceIdentity::generate_for_test();
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let server = tokio::spawn(async move {
        let mut stream = server_io;
        let request = read_plain_frame(&mut stream, protocol::MAX_HANDSHAKE_PAYLOAD_SIZE)
            .await
            .unwrap();
        assert_eq!(request.command, Command::HandshakeReq);
        stream.write_all(&response).await.unwrap();
        stream.flush().await.unwrap();
    });
    let error = expect_connect_err(
        connect(
            client_io,
            &client_identity,
            local_client_identity(),
            "server",
            expected_identity.public_key(),
        )
        .await,
    );
    server.await.unwrap();
    error
}

/// Drive a real client `connect` against a real Noise responder that encrypts
/// the supplied advertisement bytes into its `HandshakeAck`.
async fn client_connect_against_scripted_advertisement(
    advertisement: Vec<u8>,
) -> Box<dyn std::error::Error + Send + Sync> {
    let server_identity = DeviceIdentity::generate_for_test();
    let client_identity = DeviceIdentity::generate_for_test();
    let expected_key = server_identity.public_key().to_vec();
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let server = tokio::spawn(async move {
        let mut stream = server_io;
        let mut handshake = build_handshake(&server_identity, false).unwrap();
        let request = read_plain_frame(&mut stream, protocol::MAX_HANDSHAKE_PAYLOAD_SIZE)
            .await
            .unwrap();
        assert_eq!(request.command, Command::HandshakeReq);
        let mut output = vec![0u8; protocol::MAX_HANDSHAKE_PAYLOAD_SIZE];
        handshake
            .read_message(&request.payload, &mut output)
            .unwrap();
        let length = handshake
            .write_message(&advertisement, &mut output)
            .unwrap();
        let frame = Frame::try_new(Command::HandshakeAck, 0, 0, output[..length].to_vec()).unwrap();
        stream
            .write_all(&frame.encode_with_version(protocol::LEGACY_VERSION))
            .await
            .unwrap();
        stream.flush().await.unwrap();
    });
    let error = expect_connect_err(
        connect(
            client_io,
            &client_identity,
            local_client_identity(),
            "server",
            &expected_key,
        )
        .await,
    );
    server.await.unwrap();
    error
}

#[tokio::test]
async fn advertised_incompatible_protocol_version_is_typed() {
    for (advertisement, expected) in [
        (
            serde_json::json!({"hostname":"server","tailscale_ip":"","protocol_version":2,"app_version":"fixture-version","max_wire_version":4}),
            "peer (fixture-version) uses v2",
        ),
        (
            serde_json::json!({"hostname":"server","tailscale_ip":"","protocol_version":2,"max_wire_version":4}),
            "peer uses v2",
        ),
    ] {
        let error = client_connect_against_scripted_advertisement(
            serde_json::to_vec(&advertisement).unwrap(),
        )
        .await;
        let failure = typed_failure(error.as_ref());
        assert_eq!(failure.kind(), ConnectionFailureKind::ProtocolIncompatible);
        assert!(
            failure.message().contains(expected),
            "{}",
            failure.message()
        );
        assert!(failure.message().contains("requires v4"));
    }
}

#[tokio::test]
async fn unsupported_plain_wire_header_is_typed_protocol_incompatible() {
    let frame = Frame::try_new(Command::HandshakeAck, 0, 0, b"legacy".to_vec()).unwrap();
    let error = client_connect_against_plain_response(frame.encode_with_version(5)).await;
    let failure = typed_failure(error.as_ref());
    assert_eq!(failure.kind(), ConnectionFailureKind::ProtocolIncompatible);
    assert!(
        failure.message().contains("peer uses v5"),
        "{}",
        failure.message()
    );
}

#[tokio::test]
async fn unknown_peer_error_is_not_misclassified() {
    let frame = Frame::try_new(Command::PeerError, 0, 0, b"peer is busy".to_vec()).unwrap();
    let error =
        client_connect_against_plain_response(frame.encode_with_version(protocol::LEGACY_VERSION))
            .await;
    let failure = typed_failure(error.as_ref());
    assert_eq!(failure.kind(), ConnectionFailureKind::Unknown);
    assert_eq!(failure.message(), "peer is busy");
}

#[test]
fn peer_error_text_is_anchored_only_for_genuine_peer_bytes() {
    let protocol =
        ConnectionFailure::from_peer_error("Incompatible TailSync protocol: peer uses v2");
    assert_eq!(protocol.kind(), ConnectionFailureKind::ProtocolIncompatible);
    // An unrelated peer message containing the marker is not a fixed legacy category.
    let mid = ConnectionFailure::from_peer_error(
        "rejected: Incompatible TailSync protocol: peer uses v2",
    );
    assert_eq!(mid.kind(), ConnectionFailureKind::Unknown);
    let window = ConnectionFailure::from_peer_error("Pairing window is closed");
    assert_eq!(window.kind(), ConnectionFailureKind::WindowClosed);
    let unknown = ConnectionFailure::from_peer_error("peer is busy");
    assert_eq!(unknown.kind(), ConnectionFailureKind::Unknown);
}

#[test]
fn local_transport_text_is_never_reclassified_as_protocol() {
    let failure = ConnectionFailure::new(
        ConnectionFailureKind::Transport,
        "Incompatible TailSync protocol: peer uses v9 (local transport noise)",
    );
    assert_eq!(failure.kind(), ConnectionFailureKind::Transport);
    let boxed: Box<dyn std::error::Error + Send + Sync> = Box::new(failure);
    assert_eq!(
        classify_boxed(boxed.as_ref()),
        ConnectionFailureKind::Transport
    );
    assert_eq!(
        ConnectionFailure::from_boxed(boxed).kind(),
        ConnectionFailureKind::Transport
    );
    let message = "Incompatible TailSync protocol: local IO detail";
    let io = std::io::Error::other(message);
    let framed = ProtocolError::Io(io);
    let expected = framed.to_string();
    let failure = ConnectionFailure::from_protocol_error(framed);
    assert_eq!(failure.kind(), ConnectionFailureKind::Transport);
    assert_eq!(failure.message(), expected);
    let boxed = Box::new(std::io::Error::other(message));
    assert_eq!(
        ConnectionFailure::from_boxed(boxed).kind(),
        ConnectionFailureKind::Transport
    );
}

#[tokio::test]
async fn identity_mismatch_is_typed() {
    let server_identity = DeviceIdentity::generate_for_test();
    let client_identity = DeviceIdentity::generate_for_test();
    let expected_key = server_identity.public_key().to_vec();
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let server = tokio::spawn(async move {
        accept(
            server_io,
            &server_identity,
            PeerIdentity {
                hostname: "other".into(),
                tailscale_ip: String::new(),
                iroh_endpoint_id: None,
            },
        )
        .await
    });
    let error = expect_connect_err(
        connect(
            client_io,
            &client_identity,
            local_client_identity(),
            "server",
            &expected_key,
        )
        .await,
    );
    let failure = typed_failure(error.as_ref());
    assert_eq!(failure.kind(), ConnectionFailureKind::IdentityMismatch);
    assert!(failure
        .message()
        .contains("Peer identity mismatch for server"));
    let _ = server.await;
}

#[tokio::test]
async fn early_eof_is_typed_connection_closed() {
    let client_identity = DeviceIdentity::generate_for_test();
    let expected_identity = DeviceIdentity::generate_for_test();
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let server = tokio::spawn(async move {
        let mut stream = server_io;
        let request = read_plain_frame(&mut stream, protocol::MAX_HANDSHAKE_PAYLOAD_SIZE)
            .await
            .unwrap();
        assert_eq!(request.command, Command::HandshakeReq);
        drop(stream);
    });
    let error = expect_connect_err(
        connect(
            client_io,
            &client_identity,
            local_client_identity(),
            "server",
            expected_identity.public_key(),
        )
        .await,
    );
    server.await.unwrap();
    assert_eq!(
        typed_failure(error.as_ref()).kind(),
        ConnectionFailureKind::ConnectionClosed
    );
    assert!(
        error.to_string().starts_with("IO error: "),
        "legacy framing display changed: {error}"
    );
}

#[tokio::test]
async fn malformed_advertisement_is_typed_invalid_advertisement() {
    let error = client_connect_against_scripted_advertisement(b"{".to_vec()).await;
    assert_eq!(
        typed_failure(error.as_ref()).kind(),
        ConnectionFailureKind::InvalidAdvertisement
    );
}

#[tokio::test]
async fn closed_pairing_window_is_typed_on_both_sides() {
    let server_identity = DeviceIdentity::generate_for_test();
    let client_identity = DeviceIdentity::generate_for_test();
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let (_, pairing_window) = watch::channel(false);
    let server = tokio::spawn(async move {
        accept_with_pairing_window(
            server_io,
            &server_identity,
            PeerIdentity {
                hostname: "server".into(),
                tailscale_ip: "127.0.0.1".into(),
                iroh_endpoint_id: None,
            },
            pairing_window,
        )
        .await
    });
    let client_error = match connect_pairing(
        client_io,
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
        Err(error) => error,
    };
    assert_eq!(
        typed_failure(client_error.as_ref()).kind(),
        ConnectionFailureKind::WindowClosed
    );
    let server_error = match server.await.unwrap() {
        Ok(_) => panic!("closed pairing window accepted a connection"),
        Err(error) => error,
    };
    assert_eq!(
        typed_failure(server_error.as_ref()).kind(),
        ConnectionFailureKind::WindowClosed
    );
}

/// F10-A: the private handshake advertisement is a projection of
/// [`PeerIdentity`]; splitting it out must not move a single wire byte. This
/// pins the exact JSON the peer sees, including the version fields the public
/// serializer emits.
#[test]
fn handshake_advertisement_projection_matches_peer_identity_wire_text() {
    let with_iroh = PeerIdentity {
        hostname: "host".into(),
        tailscale_ip: "100.64.0.9".into(),
        iroh_endpoint_id: Some(
            "5866666666666666666666666666666666666666666666666666666666666666".into(),
        ),
    };
    let capabilities = CapabilitySet {
        file_sliding_window: true,
        ..CapabilitySet::disabled()
    };
    let encoded = encode_handshake_advertisement(with_iroh, capabilities).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(
        value,
        serde_json::json!({
            "hostname": "host",
            "tailscale_ip": "100.64.0.9",
            "iroh_endpoint_id": "5866666666666666666666666666666666666666666666666666666666666666",
            "protocol_version": protocol::LEGACY_VERSION,
            "app_version": env!("CARGO_PKG_VERSION"),
            "capabilities": {
                "file_sliding_window": true,
                "image_compressed_chunks": false,
                "pairing_commit_proof": false,
                "composite_clipboard": false,
                "archive_resource_updates": false,
            },
            "max_wire_version": protocol::VERSION,
        })
    );

    let without_iroh = PeerIdentity {
        hostname: "host".into(),
        tailscale_ip: String::new(),
        iroh_endpoint_id: None,
    };
    let encoded = encode_handshake_advertisement(without_iroh, CapabilitySet::disabled()).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
    assert!(value.get("iroh_endpoint_id").is_none());
    assert!(value.get("capabilities").is_none());
    assert_eq!(value["protocol_version"], protocol::LEGACY_VERSION);
    assert_eq!(value["max_wire_version"], protocol::VERSION);
}

// Golden messages were captured from the original private decoder, including
// UTF-8 byte columns and protocol failure precedence over trailing JSON.
#[test]
fn typed_advertisement_preserves_legacy_messages_and_error_precedence() {
    let cases = [
        (
            r###"{"hostname": "fixture", "tailscale_ip": "127.0.0.1", "protocol_version": 2, "app_version": "版本 fixture"}"###,
            r###"Incompatible TailSync protocol: peer (版本 fixture) uses v2, this version requires v4. Update TailSync on both devices. at line 1 column 108"###,
            ConnectionFailureKind::ProtocolIncompatible,
        ),
        (
            r###"{"hostname": "fixture", "tailscale_ip": "127.0.0.1", "protocol_version": 2, "app_version": "版本 fixture"} trailing"###,
            r###"Incompatible TailSync protocol: peer (版本 fixture) uses v2, this version requires v4. Update TailSync on both devices. at line 1 column 108"###,
            ConnectionFailureKind::ProtocolIncompatible,
        ),
        (
            r###"{
  "hostname": "fixture",
  "tailscale_ip": "127.0.0.1",
  "protocol_version": 2,
  "app_version": "版本 fixture"
}"###,
            r###"Incompatible TailSync protocol: peer (版本 fixture) uses v2, this version requires v4. Update TailSync on both devices. at line 6 column 1"###,
            ConnectionFailureKind::ProtocolIncompatible,
        ),
        (
            r###"{
  "hostname": "fixture",
  "tailscale_ip": "127.0.0.1",
  "protocol_version": 2,
  "app_version": "版本 fixture"
} trailing"###,
            r###"Incompatible TailSync protocol: peer (版本 fixture) uses v2, this version requires v4. Update TailSync on both devices. at line 6 column 1"###,
            ConnectionFailureKind::ProtocolIncompatible,
        ),
        (
            r###"{"hostname": "fixture😀", "tailscale_ip": "127.0.0.1", "protocol_version": 2, "app_version": "版本 fixture😀"}"###,
            r###"Incompatible TailSync protocol: peer (版本 fixture😀) uses v2, this version requires v4. Update TailSync on both devices. at line 1 column 116"###,
            ConnectionFailureKind::ProtocolIncompatible,
        ),
        (
            r###"{
  "hostname": "fixture😀",
  "tailscale_ip": "127.0.0.1",
  "protocol_version": 2,
  "app_version": "版本 fixture😀"
}"###,
            r###"Incompatible TailSync protocol: peer (版本 fixture😀) uses v2, this version requires v4. Update TailSync on both devices. at line 6 column 1"###,
            ConnectionFailureKind::ProtocolIncompatible,
        ),
        (
            r###"{"hostname": "fixture", "tailscale_ip": "127.0.0.1", "protocol_version": 4, "app_version": "版本 fixture"} trailing"###,
            r###"trailing characters at line 1 column 110"###,
            ConnectionFailureKind::InvalidAdvertisement,
        ),
        (
            r###"{"hostname":"fixture","tailscale_ip":"127.0.0.1","protocol_version":4,"protocol_version":2}"###,
            r###"duplicate field `protocol_version` at line 1 column 91"###,
            ConnectionFailureKind::InvalidAdvertisement,
        ),
        (
            r###"{"hostname":"fixture","tailscale_ip":"127.0.0.1","protocol_version":2,"capabilities":false}"###,
            r###"invalid type: boolean `false`, expected struct CapabilitySet at line 1 column 90"###,
            ConnectionFailureKind::InvalidAdvertisement,
        ),
    ];
    for (input, expected, kind) in cases {
        let failure = decode_handshake_advertisement(input.as_bytes()).unwrap_err();
        assert_eq!(failure.kind(), kind, "{input}");
        assert_eq!(failure.message(), expected, "{input}");
    }
}

#[tokio::test]
async fn peer_error_frames_only_classify_fixed_legacy_categories() {
    for (message, kind) in [
        (
            "Incompatible TailSync protocol: peer uses v2",
            ConnectionFailureKind::ProtocolIncompatible,
        ),
        (
            "rejected: Incompatible TailSync protocol: peer uses v2",
            ConnectionFailureKind::Unknown,
        ),
        (
            "/private/Incompatible TailSync protocol:/fixture",
            ConnectionFailureKind::Unknown,
        ),
        (
            "Pairing window is closed",
            ConnectionFailureKind::WindowClosed,
        ),
        (
            "Pairing window was closed",
            ConnectionFailureKind::WindowClosed,
        ),
        (
            "peer notes Pairing window is closed",
            ConnectionFailureKind::Unknown,
        ),
        (
            "Pairing window is closed is an example",
            ConnectionFailureKind::Unknown,
        ),
        ("peer is busy", ConnectionFailureKind::Unknown),
    ] {
        let frame = Frame::try_new(Command::PeerError, 0, 0, message.as_bytes().to_vec()).unwrap();
        let error = client_connect_against_plain_response(
            frame.encode_with_version(protocol::LEGACY_VERSION),
        )
        .await;
        let failure = typed_failure(error.as_ref());
        assert_eq!(failure.kind(), kind, "{message}");
        assert_eq!(failure.message(), message);
    }
}

#[tokio::test]
async fn typed_advertisement_keeps_the_full_legacy_error_through_noise() {
    let input = r###"{
  "hostname": "fixture",
  "tailscale_ip": "127.0.0.1",
  "protocol_version": 2,
  "app_version": "版本 fixture"
} trailing"###;
    let expected = r###"Incompatible TailSync protocol: peer (版本 fixture) uses v2, this version requires v4. Update TailSync on both devices. at line 6 column 1"###;
    let error = client_connect_against_scripted_advertisement(input.as_bytes().to_vec()).await;
    let failure = typed_failure(error.as_ref());
    assert_eq!(failure.kind(), ConnectionFailureKind::ProtocolIncompatible);
    assert_eq!(failure.message(), expected);
}
