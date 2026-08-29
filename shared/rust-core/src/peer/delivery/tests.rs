use super::*;
use crate::identity::DeviceIdentity;
use crate::peer::types::{ConnectionInterface, PeerCandidate, ResolvedCandidate, ResolvedTarget};
use crate::protocol::FileOffset;
use std::sync::Arc;
use tokio::sync::{mpsc, watch};
use tokio::time::timeout;

fn transfer_id(byte: u8) -> TransferId {
    TransferId([byte; 16])
}

#[test]
fn text_payload_is_wrapped_in_an_event_envelope() {
    let frame = QueuedFrame::new(Command::TextPayload, b"hello".to_vec()).unwrap();
    assert!(matches!(frame.acknowledgement, AckExpectation::Event(_)));
    let envelope = EventEnvelope::decode(frame.payload()).unwrap();
    assert_eq!(
        envelope.message_id,
        match frame.acknowledgement {
            AckExpectation::Event(id) => id,
            _ => unreachable!(),
        }
    );
}

#[test]
fn control_payloads_are_acknowledged_implicitly() {
    let frame = QueuedFrame::new(Command::Heartbeat, b"ping".to_vec()).unwrap();
    assert!(matches!(frame.acknowledgement, AckExpectation::None));
    assert_eq!(frame.payload(), b"ping");
}

#[test]
fn oversized_payloads_are_rejected_at_enqueue_time() {
    let oversized = vec![0u8; Command::TextPayload.payload_limit() + 1];
    assert!(QueuedFrame::new(Command::TextPayload, oversized).is_err());
    let oversized_control = vec![0u8; Command::Heartbeat.payload_limit() + 1];
    assert!(QueuedFrame::new(Command::Heartbeat, oversized_control).is_err());
}

#[test]
fn shared_event_encodes_once_and_shares_bytes_across_peers() {
    let content = vec![7u8; 4096];
    let event = SharedEvent::encode(Command::ImagePayload, content.clone()).unwrap();
    let a = event.queued();
    let b = event.queued();

    // Zero-copy fan-out: both peers' frames view the same backing buffer,
    // so broadcasting to N peers holds one payload instead of N copies.
    assert_eq!(a.payload().as_ptr(), b.payload().as_ptr());
    assert_eq!(a.payload().len(), b.payload().len());

    // Every peer acknowledges the same identity. Receiver dedup is scoped
    // to (source, message_id) and each peer sees the broadcast at most
    // once, so a shared id is safe and means one event has one identity.
    let id_a = match a.acknowledgement() {
        AckExpectation::Event(id) => id,
        other => panic!("expected an event ack, got {other:?}"),
    };
    let id_b = match b.acknowledgement() {
        AckExpectation::Event(id) => id,
        other => panic!("expected an event ack, got {other:?}"),
    };
    assert_eq!(id_a, id_b);

    // The shared bytes still decode back to the original content.
    let envelope = EventEnvelope::decode(a.payload()).unwrap();
    assert_eq!(envelope.content, content);
    assert_eq!(envelope.message_id, id_a);
}

#[test]
fn shared_event_can_encode_without_consuming_history_bytes() {
    let content: Arc<[u8]> = Arc::from(vec![0x5a; 4096].into_boxed_slice());
    let history_bytes = content.clone();
    let event = SharedEvent::encode_shared(Command::ImagePayload, content).unwrap();
    let queued = event.queued();
    let envelope = EventEnvelope::decode(queued.payload()).unwrap();

    assert_eq!(history_bytes.as_ref(), envelope.content);
    assert_eq!(Arc::strong_count(&history_bytes), 1);
}

#[test]
fn shared_event_rejects_non_envelope_commands_and_oversized_payloads() {
    assert!(SharedEvent::encode(Command::Heartbeat, b"nope".to_vec()).is_err());
    let oversized = vec![0u8; Command::TextPayload.payload_limit() + 1];
    assert!(SharedEvent::encode(Command::TextPayload, oversized).is_err());
}

#[test]
fn confirmed_file_rejects_non_file_commands() {
    let (tx, _rx) = oneshot::channel();
    assert!(
        QueuedFrame::confirmed_file(Command::TextPayload, vec![1, 2, 3], transfer_id(1), tx,)
            .is_err()
    );
}

#[test]
fn confirmed_batch_rejects_oversized_payloads() {
    let (tx, _rx) = oneshot::channel();
    let oversized = vec![0u8; Command::FileBatchStart.payload_limit() + 1];
    assert!(
        QueuedFrame::confirmed_batch(Command::FileBatchStart, oversized, transfer_id(2), tx,)
            .is_err()
    );
}

#[test]
fn completion_reports_the_result_to_the_enqueuer() {
    let (tx, mut rx) = oneshot::channel();
    let frame =
        QueuedFrame::confirmed_file(Command::FileChunk, vec![1, 2, 3], transfer_id(3), tx).unwrap();
    let pending = PendingFrame {
        queued: frame,
        sequence: 7,
        undelivered_peer: None,
    };
    pending.complete(Ok(DeliveryReceipt {
        next_offset: Some(3),
    }));
    let receipt = rx.try_recv().unwrap().unwrap();
    assert_eq!(receipt.next_offset, Some(3));
}

#[test]
fn dropping_an_in_flight_frame_reports_worker_shutdown() {
    let _ = crate::sync_warning::take();
    drop(PendingFrame::new_for_peer(
        QueuedFrame::new(Command::TextPayload, b"in flight".to_vec()).unwrap(),
        9,
        "Laptop",
    ));
    let warning = crate::sync_warning::take().unwrap();
    assert_eq!(warning.kind, "delivery_shutdown");
    assert_eq!(warning.peer, "Laptop");

    let (tx, mut rx) = oneshot::channel();
    drop(PendingFrame::new_for_peer(
        QueuedFrame::confirmed_file(Command::FileChunk, vec![1], transfer_id(4), tx).unwrap(),
        10,
        "Laptop",
    ));
    assert!(matches!(
        rx.try_recv(),
        Ok(Err(DeliveryError::Transport(message))) if message == "Connection task closed"
    ));
}

#[test]
fn pending_frame_ttl_is_checked_from_enqueue_time() {
    let frame = PendingFrame::new(QueuedFrame::new(Command::Heartbeat, vec![1]).unwrap(), 1);
    assert!(frame.is_expired(Duration::ZERO));
    assert!(!frame.is_expired(Duration::from_secs(60)));
}

#[tokio::test]
async fn scheduler_serves_bulk_after_a_bounded_priority_burst() {
    let (priority_tx, mut priority_rx) = mpsc::channel(PRIORITY_BURST_LIMIT + 2);
    let (bulk_tx, mut bulk_rx) = mpsc::channel(2);
    for index in 0..=PRIORITY_BURST_LIMIT {
        priority_tx
            .send(QueuedFrame::new(Command::Heartbeat, vec![index as u8]).unwrap())
            .await
            .unwrap();
    }
    bulk_tx
        .send(QueuedFrame::new(Command::FileBatchCancel, vec![0xaa]).unwrap())
        .await
        .unwrap();

    let mut priority_streak = 0;
    for _ in 0..PRIORITY_BURST_LIMIT {
        let frame = receive_scheduled_frame(&mut priority_rx, &mut bulk_rx, &mut priority_streak)
            .await
            .unwrap();
        assert_eq!(frame.command(), Command::Heartbeat);
    }
    let frame = receive_scheduled_frame(&mut priority_rx, &mut bulk_rx, &mut priority_streak)
        .await
        .unwrap();
    assert_eq!(frame.command(), Command::FileBatchCancel);
    assert_eq!(priority_streak, 0);
}

#[tokio::test]
async fn scheduler_keeps_serving_the_open_channel() {
    let (priority_tx, mut priority_rx) = mpsc::channel(1);
    let (bulk_tx, mut bulk_rx) = mpsc::channel(1);
    drop(priority_tx);
    bulk_tx
        .send(QueuedFrame::new(Command::FileBatchCancel, vec![1]).unwrap())
        .await
        .unwrap();
    drop(bulk_tx);

    let mut priority_streak = PRIORITY_BURST_LIMIT;
    let frame = receive_scheduled_frame(&mut priority_rx, &mut bulk_rx, &mut priority_streak)
        .await
        .unwrap();
    assert_eq!(frame.command(), Command::FileBatchCancel);
    assert!(
        receive_scheduled_frame(&mut priority_rx, &mut bulk_rx, &mut priority_streak,)
            .await
            .is_none()
    );
}

#[test]
fn delivery_error_retryability_is_typed() {
    assert!(!DeliveryError::Rejected("bad".into()).is_retryable());
    assert!(!DeliveryError::Expired("stale".into()).is_retryable());
    assert!(DeliveryError::Timeout("window".into()).is_retryable());
    assert!(DeliveryError::Transport("reset".into()).is_retryable());
    assert!(DeliveryError::Protocol("mismatch".into()).is_retryable());
    assert_eq!(
        DeliveryError::Rejected("bad".into()).to_string(),
        "peer rejected: bad"
    );
    assert_eq!(
        DeliveryError::Expired("stale".into()).to_string(),
        "event expired: stale"
    );
}

#[test]
fn event_ack_must_match_sequence_and_message() {
    let message_id = MessageId([7u8; 16]);
    let frame = QueuedFrame::new(Command::TextPayload, b"hi".to_vec()).unwrap();
    let pending = PendingFrame {
        queued: frame,
        sequence: 3,
        undelivered_peer: None,
    };
    let good = Frame::try_new(Command::EventAck, 0, 3, message_id.ack_payload()).unwrap();
    validate_event_ack(&good, &pending, message_id).unwrap();

    let wrong_seq = Frame::try_new(Command::EventAck, 0, 4, message_id.ack_payload()).unwrap();
    assert!(validate_event_ack(&wrong_seq, &pending, message_id).is_err());

    let wrong_msg =
        Frame::try_new(Command::EventAck, 0, 3, MessageId([8u8; 16]).ack_payload()).unwrap();
    assert!(validate_event_ack(&wrong_msg, &pending, message_id).is_err());
}

#[test]
fn file_ack_must_match_transfer_and_returns_offset() {
    let transfer = TransferId([9u8; 16]);
    let (tx, _rx) = oneshot::channel();
    let frame =
        QueuedFrame::confirmed_file(Command::FileChunk, vec![1, 2, 3], transfer, tx).unwrap();
    let pending = PendingFrame {
        queued: frame,
        sequence: 5,
        undelivered_peer: None,
    };
    let mut payload = Vec::new();
    payload.extend_from_slice(&transfer.0);
    payload.extend_from_slice(&42u64.to_be_bytes());
    let good = Frame::try_new(Command::FileAck, 0, 5, payload).unwrap();
    let receipt = validate_file_ack(&good, &pending, transfer).unwrap();
    assert_eq!(receipt.next_offset, Some(42));

    let mut wrong_payload = Vec::new();
    wrong_payload.extend_from_slice(&TransferId([10u8; 16]).0);
    wrong_payload.extend_from_slice(&42u64.to_be_bytes());
    let wrong = Frame::try_new(Command::FileAck, 0, 5, wrong_payload).unwrap();
    assert!(validate_file_ack(&wrong, &pending, transfer).is_err());
}

#[test]
fn delivery_config_defaults_match_shared_constants() {
    let config = DeliveryConfig::default();
    assert_eq!(config.event_ack_timeout, Duration::from_millis(750));
    assert_eq!(config.file_ack_timeout, Duration::from_secs(10));
    assert_eq!(config.event_retry_base_delay, Duration::from_millis(250));
    assert_eq!(config.max_attempts, 4);
}

fn resolved_candidate(interface: ConnectionInterface, address: &str) -> ResolvedCandidate {
    ResolvedCandidate {
        candidate: PeerCandidate::new(interface, address),
        target: ResolvedTarget::Tcp(format!("{address}:19890").parse().unwrap()),
    }
}
#[test]
fn candidate_delay_prefers_lan_without_serializing_fallbacks() {
    assert_eq!(
        candidate_delay(ConnectionInterface::Lan, true, true),
        Duration::ZERO
    );
    assert_eq!(
        candidate_delay(ConnectionInterface::Iroh, true, true),
        Duration::from_millis(150)
    );
    assert_eq!(
        candidate_delay(ConnectionInterface::Iroh, false, true),
        Duration::ZERO
    );
    assert_eq!(
        candidate_delay(ConnectionInterface::Tailscale, true, true),
        Duration::from_millis(300)
    );
    assert_eq!(
        candidate_delay(ConnectionInterface::Tailscale, false, true),
        Duration::from_millis(150)
    );
    assert_eq!(
        candidate_delay(ConnectionInterface::Tailscale, false, false),
        Duration::ZERO
    );
}

#[tokio::test]
async fn race_wins_with_first_successful_attempt() {
    let candidates = vec![
        resolved_candidate(ConnectionInterface::Lan, "192.168.1.2"),
        resolved_candidate(ConnectionInterface::Tailscale, "100.64.0.2"),
    ];
    let (stream, winner) = race_connections(
        &candidates,
        Duration::from_secs(2),
        |target, _candidate| async move {
            if target.to_string().contains("192.168") {
                Ok("lan-stream")
            } else {
                Err("tailscale failed".to_string())
            }
        },
    )
    .await
    .unwrap();
    assert_eq!(stream, "lan-stream");
    assert_eq!(winner.candidate.interface, ConnectionInterface::Lan);
}

#[tokio::test]
async fn race_joins_all_failures() {
    let candidates = vec![resolved_candidate(ConnectionInterface::Lan, "192.168.1.2")];
    let err = race_connections(
        &candidates,
        Duration::from_secs(1),
        |_target, _candidate| async move { Err::<(), String>("boom".to_string()) },
    )
    .await
    .unwrap_err();
    assert!(err.contains("lan") && err.contains("boom"));
}

#[tokio::test]
async fn race_without_candidates_fails() {
    let err = race_connections::<(), _, _>(
        &[],
        Duration::from_secs(1),
        |_target, _candidate| async move { Ok(()) },
    )
    .await
    .unwrap_err();
    assert_eq!(err, "no connection candidates to race");
}

#[tokio::test]
async fn race_applies_handshake_timeout() {
    let candidates = vec![resolved_candidate(ConnectionInterface::Lan, "192.168.1.2")];
    let err = race_connections(
        &candidates,
        Duration::from_millis(20),
        |_target, _candidate| async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            Ok::<&str, String>("late")
        },
    )
    .await
    .unwrap_err();
    assert!(err.contains("timed out"));
}

#[tokio::test]
async fn race_aborts_remaining_attempts_on_first_success() {
    struct DropCounter(std::sync::Arc<std::sync::atomic::AtomicUsize>);
    impl Drop for DropCounter {
        fn drop(&mut self) {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    let dropped = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let completed = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let dropped_for_assert = dropped.clone();
    let completed_for_assert = completed.clone();
    let candidates = vec![
        resolved_candidate(ConnectionInterface::Lan, "192.168.1.2"),
        resolved_candidate(ConnectionInterface::Lan, "192.168.1.3"),
    ];
    let (stream, winner) = race_connections(
        &candidates,
        Duration::from_secs(2),
        move |target, _candidate| {
            let dropped = dropped.clone();
            let completed = completed.clone();
            async move {
                if target.to_string().starts_with("192.168.1.2:") {
                    Ok("lan-stream")
                } else {
                    // A slow losing attempt holding a drop guard: it must
                    // be aborted (and dropped) once the LAN attempt wins,
                    // before it could ever complete on its own.
                    let _guard = DropCounter(dropped);
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    completed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Ok::<&str, String>("tailscale-stream")
                }
            }
        },
    )
    .await
    .unwrap();
    assert_eq!(stream, "lan-stream");
    assert_eq!(winner.candidate.interface, ConnectionInterface::Lan);

    // Give the abort a scheduling opportunity: the losing attempt must
    // have been dropped without ever completing.
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        dropped_for_assert.load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    assert_eq!(
        completed_for_assert.load(std::sync::atomic::Ordering::SeqCst),
        0
    );
}

// ------------------------------------------------------------------
// Real delivery over an in-memory Noise connection pair. These tests
// exercise the full delivery path (handshake, framing, ACK validation,
// retry, rejection) without sockets, so both platforms share them.
// ------------------------------------------------------------------

fn server_identity() -> Arc<DeviceIdentity> {
    Arc::new(DeviceIdentity::generate_for_test())
}

fn server_peer_identity() -> crate::secure::PeerIdentity {
    crate::secure::PeerIdentity {
        hostname: "server".into(),
        tailscale_ip: String::new(),
        iroh_endpoint_id: None,
    }
}

fn client_peer_identity() -> crate::secure::PeerIdentity {
    crate::secure::PeerIdentity {
        hostname: "client".into(),
        tailscale_ip: String::new(),
        iroh_endpoint_id: None,
    }
}

/// Establish a Noise-authenticated pair over an in-memory duplex and
/// return the client connection plus the server connection (the caller
/// drives both sides from the test).
async fn establish_pair(
    server_identity: &Arc<DeviceIdentity>,
    client_identity: &DeviceIdentity,
) -> (SecureConnection, SecureConnection) {
    let expected_key = server_identity.public_key().to_vec();
    let (client_io, server_io) = tokio::io::duplex(256 * 1024);
    let server_identity = server_identity.clone();
    let server = tokio::spawn(async move {
        let accepted = crate::secure::accept(server_io, &server_identity, server_peer_identity())
            .await
            .unwrap();
        let mut connection = accepted.connection;
        crate::secure::write_ready(&mut connection).await.unwrap();
        connection
    });
    let client = crate::secure::connect(
        client_io,
        client_identity,
        client_peer_identity(),
        "server",
        &expected_key,
    )
    .await
    .unwrap();
    let server = server.await.unwrap();
    (client, server)
}

#[tokio::test]
async fn delivers_event_with_acknowledgement() {
    let server_identity = server_identity();
    let client_identity = DeviceIdentity::generate_for_test();
    let (mut client, mut server) = establish_pair(&server_identity, &client_identity).await;

    let server_task = tokio::spawn(async move {
        let frame = server.read_frame().await.unwrap();
        let message_id = EventEnvelope::decode(&frame.payload).unwrap().message_id;
        server
            .write_frame(
                &Frame::try_new(
                    Command::EventAck,
                    0,
                    frame.sequence,
                    message_id.ack_payload(),
                )
                .unwrap(),
            )
            .await
            .unwrap();
    });

    let pending = PendingFrame::new(
        QueuedFrame::new(Command::TextPayload, b"hello".to_vec()).unwrap(),
        1,
    );
    let receipt = deliver_pending_frame(&mut client, &pending, &DeliveryConfig::DEFAULT)
        .await
        .unwrap();
    assert_eq!(receipt.next_offset, None);
    server_task.await.unwrap();
}

#[tokio::test]
async fn event_rejection_is_permanent() {
    let server_identity = server_identity();
    let client_identity = DeviceIdentity::generate_for_test();
    let (mut client, mut server) = establish_pair(&server_identity, &client_identity).await;

    let server_task = tokio::spawn(async move {
        let frame = server.read_frame().await.unwrap();
        server
            .write_frame(
                &Frame::try_new(Command::PeerError, 0, frame.sequence, b"bad event".to_vec())
                    .unwrap(),
            )
            .await
            .unwrap();
    });

    let pending = PendingFrame::new(
        QueuedFrame::new(Command::TextPayload, b"hello".to_vec()).unwrap(),
        2,
    );
    let err = deliver_pending_frame(&mut client, &pending, &DeliveryConfig::DEFAULT)
        .await
        .unwrap_err();
    assert!(matches!(err, DeliveryError::Rejected(_)));
    assert!(!err.is_retryable());
    server_task.await.unwrap();
}

#[tokio::test]
async fn expired_event_rejection_is_typed_and_permanent() {
    let server_identity = server_identity();
    let client_identity = DeviceIdentity::generate_for_test();
    let (mut client, mut server) = establish_pair(&server_identity, &client_identity).await;

    let server_task = tokio::spawn(async move {
        let frame = server.read_frame().await.unwrap();
        server
            .write_frame(
                &Frame::try_new(
                    Command::PeerError,
                    0,
                    frame.sequence,
                    b"event timestamp is outside the accepted window".to_vec(),
                )
                .unwrap(),
            )
            .await
            .unwrap();
    });

    let pending = PendingFrame::new(
        QueuedFrame::new(Command::TextPayload, b"hello".to_vec()).unwrap(),
        4,
    );
    let err = deliver_pending_frame(&mut client, &pending, &DeliveryConfig::DEFAULT)
        .await
        .unwrap_err();
    assert!(matches!(err, DeliveryError::Expired(_)));
    assert!(!err.is_retryable());
    server_task.await.unwrap();
}

#[tokio::test]
async fn event_ack_for_another_message_is_a_protocol_error() {
    let server_identity = server_identity();
    let client_identity = DeviceIdentity::generate_for_test();
    let (mut client, mut server) = establish_pair(&server_identity, &client_identity).await;

    let server_task = tokio::spawn(async move {
        let frame = server.read_frame().await.unwrap();
        server
            .write_frame(
                &Frame::try_new(
                    Command::EventAck,
                    0,
                    frame.sequence,
                    MessageId([0xEE; 16]).ack_payload(),
                )
                .unwrap(),
            )
            .await
            .unwrap();
    });

    let pending = PendingFrame::new(
        QueuedFrame::new(Command::TextPayload, b"hello".to_vec()).unwrap(),
        3,
    );
    let err = deliver_pending_frame(&mut client, &pending, &DeliveryConfig::DEFAULT)
        .await
        .unwrap_err();
    assert!(matches!(err, DeliveryError::Protocol(_)));
    assert!(err.is_retryable());
    server_task.await.unwrap();
}

#[tokio::test]
async fn event_is_retried_with_the_same_sequence_until_acknowledged() {
    let server_identity = server_identity();
    let client_identity = DeviceIdentity::generate_for_test();
    let (mut client, mut server) = establish_pair(&server_identity, &client_identity).await;

    // The server reads the first attempt but stays silent; the client's
    // ACK window expires and it retries the identical frame. Only the
    // second attempt is acknowledged.
    let server_task = tokio::spawn(async move {
        let first = server.read_frame().await.unwrap();
        let retry = server.read_frame().await.unwrap();
        assert_eq!(retry.sequence, first.sequence);
        assert_eq!(retry.payload, first.payload);
        let message_id = EventEnvelope::decode(&retry.payload).unwrap().message_id;
        server
            .write_frame(
                &Frame::try_new(
                    Command::EventAck,
                    0,
                    retry.sequence,
                    message_id.ack_payload(),
                )
                .unwrap(),
            )
            .await
            .unwrap();
    });

    let pending = PendingFrame::new(
        QueuedFrame::new(Command::TextPayload, b"reliable".to_vec()).unwrap(),
        42,
    );
    // Event ACK window is 750 ms; one silent round + ack must fit in the
    // 4 default attempts.
    deliver_pending_frame(&mut client, &pending, &DeliveryConfig::DEFAULT)
        .await
        .unwrap();
    server_task.await.unwrap();
}

#[tokio::test]
async fn delivers_file_chunk_with_offset_ack() {
    let server_identity = server_identity();
    let client_identity = DeviceIdentity::generate_for_test();
    let (mut client, mut server) = establish_pair(&server_identity, &client_identity).await;
    let transfer = TransferId([0xAB; 16]);

    let server_task = tokio::spawn(async move {
        let frame = server.read_frame().await.unwrap();
        let ack = FileOffset {
            transfer_id: transfer,
            next_offset: 4096,
        };
        server
            .write_frame(
                &Frame::try_new(Command::FileAck, 0, frame.sequence, ack.encode()).unwrap(),
            )
            .await
            .unwrap();
    });

    let (tx, _rx) = oneshot::channel();
    let pending = PendingFrame::new(
        QueuedFrame::confirmed_file(Command::FileChunk, vec![0u8; 8], transfer, tx).unwrap(),
        5,
    );
    let receipt = deliver_pending_frame(&mut client, &pending, &DeliveryConfig::DEFAULT)
        .await
        .unwrap();
    assert_eq!(receipt.next_offset, Some(4096));
    server_task.await.unwrap();
}

#[tokio::test]
async fn delivers_batch_with_accept_ack() {
    let server_identity = server_identity();
    let client_identity = DeviceIdentity::generate_for_test();
    let (mut client, mut server) = establish_pair(&server_identity, &client_identity).await;
    let batch = TransferId([0xCD; 16]);

    let server_task = tokio::spawn(async move {
        let frame = server.read_frame().await.unwrap();
        server
            .write_frame(
                &Frame::try_new(
                    Command::FileBatchAccept,
                    0,
                    frame.sequence,
                    batch.0.to_vec(),
                )
                .unwrap(),
            )
            .await
            .unwrap();
    });

    let (tx, _rx) = oneshot::channel();
    let pending = PendingFrame::new(
        QueuedFrame::confirmed_batch(Command::FileBatchStart, Vec::new(), batch, tx).unwrap(),
        6,
    );
    let receipt = deliver_pending_frame(&mut client, &pending, &DeliveryConfig::DEFAULT)
        .await
        .unwrap();
    assert_eq!(receipt.next_offset, None);
    server_task.await.unwrap();
}

// ------------------------------------------------------------------
// Connection worker tests: the worker runs against in-memory frame
// connections and a scripted fake adapter, so reconnect, keep-frame,
// rejection, and shutdown behavior are testable without sockets.
// ------------------------------------------------------------------

struct MemoryConnection {
    io: tokio::io::DuplexStream,
}

impl DeliveryConnection for MemoryConnection {
    async fn write_frame(&mut self, frame: &Frame) -> Result<(), String> {
        let bytes = frame.encode();
        let len = (bytes.len() as u32).to_be_bytes();
        tokio::io::AsyncWriteExt::write_all(&mut self.io, &len)
            .await
            .map_err(|e| e.to_string())?;
        tokio::io::AsyncWriteExt::write_all(&mut self.io, &bytes)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn read_frame(&mut self) -> Result<Frame, String> {
        let mut len = [0u8; 4];
        tokio::io::AsyncReadExt::read_exact(&mut self.io, &mut len)
            .await
            .map_err(|e| e.to_string())?;
        let mut buf = vec![0u8; u32::from_be_bytes(len) as usize];
        tokio::io::AsyncReadExt::read_exact(&mut self.io, &mut buf)
            .await
            .map_err(|e| e.to_string())?;
        let (frame, _) = Frame::decode(&buf).map_err(|e| e.to_string())?;
        Ok(frame)
    }
}

struct FakeAdapter {
    connects: tokio::sync::Mutex<
        std::collections::VecDeque<Result<(MemoryConnection, ResolvedCandidate), String>>,
    >,
    sessions: std::sync::Mutex<Vec<(String, ConnectionInterface, String, u64)>>,
    active_sessions: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    connect_calls: std::sync::atomic::AtomicUsize,
    /// When set, `refresh_candidates` never returns. Models the settings
    /// lock contending or its owning task wedging — the park point that
    /// froze the live link. The worker must still reach `connect`.
    hang_refresh: std::sync::atomic::AtomicBool,
}

struct FakeSessionLease {
    active_sessions: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl Drop for FakeSessionLease {
    fn drop(&mut self) {
        self.active_sessions
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

impl ConnectionAdapter for FakeAdapter {
    type Connection = MemoryConnection;
    type SessionLease = FakeSessionLease;

    async fn connect(
        &self,
        _hostname: &str,
        _candidates: &[ResolvedCandidate],
    ) -> Result<(MemoryConnection, ResolvedCandidate), String> {
        self.connect_calls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.connects
            .lock()
            .await
            .pop_front()
            .unwrap_or_else(|| Err("no more connections scripted".to_string()))
    }

    fn register_session(
        &self,
        hostname: &str,
        interface: ConnectionInterface,
        address: &str,
        latency_ms: u64,
    ) -> FakeSessionLease {
        self.active_sessions
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.sessions.lock().unwrap().push((
            hostname.to_string(),
            interface,
            address.to_string(),
            latency_ms,
        ));
        FakeSessionLease {
            active_sessions: self.active_sessions.clone(),
        }
    }

    fn record_protocol_error(&self, _hostname: &str, _error: &str) {}
    fn clear_protocol_error(&self, _hostname: &str) {}

    async fn refresh_candidates(
        &self,
        _hostname: &str,
        _candidates: &mut Vec<ResolvedCandidate>,
    ) -> bool {
        if self.hang_refresh.load(std::sync::atomic::Ordering::SeqCst) {
            // Never resolves; the worker relies on its refresh_timeout to
            // proceed past this await.
            std::future::pending::<()>().await;
        }
        false
    }
}

fn fast_worker_config() -> WorkerConfig {
    WorkerConfig {
        heartbeat_interval: Duration::from_secs(30),
        heartbeat_ack_timeout: Duration::from_millis(100),
        reconnect_delay: Duration::from_millis(10),
        refresh_timeout: Duration::from_secs(5),
        pending_frame_ttl: Duration::from_secs(5 * 60),
        delivery: DeliveryConfig::DEFAULT,
    }
}

fn fast_file_config() -> DeliveryConfig {
    DeliveryConfig::try_new(
        Duration::from_millis(50),
        Duration::from_millis(50),
        Duration::from_millis(10),
        1,
    )
    .unwrap()
}

fn scripted_adapter(
    connects: Vec<Result<(MemoryConnection, ResolvedCandidate), String>>,
) -> FakeAdapter {
    FakeAdapter {
        connects: tokio::sync::Mutex::new(connects.into()),
        sessions: std::sync::Mutex::new(Vec::new()),
        active_sessions: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        connect_calls: std::sync::atomic::AtomicUsize::new(0),
        hang_refresh: std::sync::atomic::AtomicBool::new(false),
    }
}

#[tokio::test]
async fn worker_delivers_frames_and_registers_session() {
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let candidate = resolved_candidate(ConnectionInterface::Lan, "192.168.1.2");
    let adapter = std::sync::Arc::new(scripted_adapter(vec![Ok((
        MemoryConnection { io: client_io },
        candidate.clone(),
    ))]));

    let server = tokio::spawn(async move {
        let mut server = MemoryConnection { io: server_io };
        let frame = server.read_frame().await.unwrap();
        let transfer = TransferId([0x11; 16]);
        let ack = FileOffset {
            transfer_id: transfer,
            next_offset: 1024,
        };
        server
            .write_frame(
                &Frame::try_new(Command::FileAck, 0, frame.sequence, ack.encode()).unwrap(),
            )
            .await
            .unwrap();
    });

    let (priority_tx, priority_rx) = mpsc::channel(4);
    let (bulk_tx, bulk_rx) = mpsc::channel(4);
    let (_shutdown_tx, shutdown_rx) = watch::channel(false);
    let adapter_for_worker = adapter.clone();
    let worker = tokio::spawn(async move {
        run_connection_worker(
            adapter_for_worker.as_ref(),
            &fast_worker_config(),
            vec![candidate],
            "peer".into(),
            priority_rx,
            bulk_rx,
            shutdown_rx,
        )
        .await
    });

    let (completion_tx, mut completion_rx) = oneshot::channel();
    let queued = QueuedFrame::confirmed_file(
        Command::FileChunk,
        vec![0u8; 8],
        TransferId([0x11; 16]),
        completion_tx,
    )
    .unwrap();
    priority_tx.send(queued).await.unwrap();

    let receipt = timeout(Duration::from_secs(2), &mut completion_rx)
        .await
        .expect("delivery completion")
        .unwrap()
        .unwrap();
    assert_eq!(receipt.next_offset, Some(1024));
    server.await.unwrap();
    assert_eq!(
        adapter
            .active_sessions
            .load(std::sync::atomic::Ordering::SeqCst),
        1,
        "the session lease must remain active while the worker owns the connection"
    );

    // Worker keeps running after the delivery: drop senders to let it
    // exit, then verify the session was registered.
    drop(priority_tx);
    drop(bulk_tx);
    timeout(Duration::from_secs(2), worker)
        .await
        .unwrap()
        .unwrap();
    let sessions = adapter.sessions.lock().unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].0, "peer");
    assert_eq!(sessions[0].1, ConnectionInterface::Lan);
    assert_eq!(
        adapter
            .active_sessions
            .load(std::sync::atomic::Ordering::SeqCst),
        0,
        "the session lease must be released when the worker exits"
    );
}

#[tokio::test]
async fn worker_reaches_connect_when_refresh_hangs() {
    // Regression: a wedged candidate refresh (settings-lock contention or a
    // stalled settings task) must not park the worker before it connects.
    // Without the refresh_timeout bound the worker would await here forever
    // and the link could never self-heal — the exact live freeze observed.
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let candidate = resolved_candidate(ConnectionInterface::Lan, "192.168.1.2");
    let adapter = std::sync::Arc::new(scripted_adapter(vec![Ok((
        MemoryConnection { io: client_io },
        candidate.clone(),
    ))]));
    adapter
        .hang_refresh
        .store(true, std::sync::atomic::Ordering::SeqCst);

    let server = tokio::spawn(async move {
        let mut server = MemoryConnection { io: server_io };
        let frame = server.read_frame().await.unwrap();
        let ack = FileOffset {
            transfer_id: TransferId([0x33; 16]),
            next_offset: 512,
        };
        server
            .write_frame(
                &Frame::try_new(Command::FileAck, 0, frame.sequence, ack.encode()).unwrap(),
            )
            .await
            .unwrap();
    });

    let mut config = fast_worker_config();
    config.refresh_timeout = Duration::from_millis(50);
    let (priority_tx, priority_rx) = mpsc::channel(4);
    let (bulk_tx, bulk_rx) = mpsc::channel(4);
    let (_shutdown_tx, shutdown_rx) = watch::channel(false);
    let adapter_for_worker = adapter.clone();
    let worker = tokio::spawn(async move {
        run_connection_worker(
            adapter_for_worker.as_ref(),
            &config,
            vec![candidate],
            "peer".into(),
            priority_rx,
            bulk_rx,
            shutdown_rx,
        )
        .await
    });

    let (completion_tx, mut completion_rx) = oneshot::channel();
    let queued = QueuedFrame::confirmed_file(
        Command::FileChunk,
        vec![0u8; 8],
        TransferId([0x33; 16]),
        completion_tx,
    )
    .unwrap();
    priority_tx.send(queued).await.unwrap();

    let receipt = timeout(Duration::from_secs(2), &mut completion_rx)
        .await
        .expect("delivery must complete despite the hanging refresh")
        .unwrap()
        .unwrap();
    assert_eq!(receipt.next_offset, Some(512));
    assert!(
        adapter
            .connect_calls
            .load(std::sync::atomic::Ordering::SeqCst)
            >= 1,
        "worker must reach connect even though refresh never returns"
    );

    server.await.unwrap();
    drop(priority_tx);
    drop(bulk_tx);
    timeout(Duration::from_secs(2), worker)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn worker_keeps_pending_frame_across_reconnect() {
    let (client_a, server_a) = tokio::io::duplex(64 * 1024);
    let (client_b, server_b) = tokio::io::duplex(64 * 1024);
    let candidate = resolved_candidate(ConnectionInterface::Lan, "192.168.1.2");
    let adapter = std::sync::Arc::new(scripted_adapter(vec![
        Ok((MemoryConnection { io: client_a }, candidate.clone())),
        Ok((MemoryConnection { io: client_b }, candidate.clone())),
    ]));

    // First connection: the server reads the frame but stays silent, so
    // the delivery times out (file ACK window 50 ms, one attempt) and the
    // worker reconnects with the frame kept pending.
    let server_a_task = tokio::spawn(async move {
        let mut server = MemoryConnection { io: server_a };
        let _frame = server.read_frame().await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
    });
    // Second connection: the server acknowledges the retried frame.
    let server_b_task = tokio::spawn(async move {
        let mut server = MemoryConnection { io: server_b };
        let frame = server.read_frame().await.unwrap();
        let ack = FileOffset {
            transfer_id: TransferId([0x22; 16]),
            next_offset: 2048,
        };
        server
            .write_frame(
                &Frame::try_new(Command::FileAck, 0, frame.sequence, ack.encode()).unwrap(),
            )
            .await
            .unwrap();
    });

    let mut config = fast_worker_config();
    config.delivery = fast_file_config();
    let config = std::sync::Arc::new(config);
    let (priority_tx, priority_rx) = mpsc::channel(4);
    let (bulk_tx, bulk_rx) = mpsc::channel(4);
    let (_shutdown_tx, shutdown_rx) = watch::channel(false);
    let adapter_for_worker = adapter.clone();
    let config_for_worker = config.clone();
    let worker = tokio::spawn(async move {
        run_connection_worker(
            adapter_for_worker.as_ref(),
            &config_for_worker,
            vec![candidate],
            "peer".into(),
            priority_rx,
            bulk_rx,
            shutdown_rx,
        )
        .await
    });

    let (completion_tx, mut completion_rx) = oneshot::channel();
    let queued = QueuedFrame::confirmed_file(
        Command::FileChunk,
        vec![0u8; 8],
        TransferId([0x22; 16]),
        completion_tx,
    )
    .unwrap();
    priority_tx.send(queued).await.unwrap();

    // The frame must be delivered on the second connection with the
    // same transfer, proving it survived the reconnect.
    let receipt = timeout(Duration::from_secs(3), &mut completion_rx)
        .await
        .expect("retried delivery completion")
        .unwrap()
        .unwrap();
    assert_eq!(receipt.next_offset, Some(2048));

    drop(priority_tx);
    drop(bulk_tx);
    timeout(Duration::from_secs(2), worker)
        .await
        .unwrap()
        .unwrap();
    server_a_task.await.unwrap();
    server_b_task.await.unwrap();
    assert_eq!(
        adapter
            .connect_calls
            .load(std::sync::atomic::Ordering::SeqCst),
        2
    );
}

#[tokio::test]
async fn worker_drops_permanently_rejected_frames() {
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let candidate = resolved_candidate(ConnectionInterface::Lan, "192.168.1.2");
    let adapter = std::sync::Arc::new(scripted_adapter(vec![Ok((
        MemoryConnection { io: client_io },
        candidate.clone(),
    ))]));

    let server = tokio::spawn(async move {
        let mut server = MemoryConnection { io: server_io };
        let frame = server.read_frame().await.unwrap();
        server
            .write_frame(
                &Frame::try_new(
                    Command::PeerError,
                    0,
                    frame.sequence,
                    b"rejected batch: quota".to_vec(),
                )
                .unwrap(),
            )
            .await
            .unwrap();
    });

    let (priority_tx, priority_rx) = mpsc::channel(4);
    let (bulk_tx, bulk_rx) = mpsc::channel(4);
    let (_shutdown_tx, shutdown_rx) = watch::channel(false);
    let adapter_for_worker = adapter.clone();
    let worker = tokio::spawn(async move {
        run_connection_worker(
            adapter_for_worker.as_ref(),
            &fast_worker_config(),
            vec![candidate],
            "peer".into(),
            priority_rx,
            bulk_rx,
            shutdown_rx,
        )
        .await
    });

    let (completion_tx, mut completion_rx) = oneshot::channel();
    let queued = QueuedFrame::confirmed_batch(
        Command::FileBatchStart,
        Vec::new(),
        TransferId([0x33; 16]),
        completion_tx,
    )
    .unwrap();
    priority_tx.send(queued).await.unwrap();

    let err = timeout(Duration::from_secs(2), &mut completion_rx)
        .await
        .expect("rejection completion")
        .unwrap()
        .expect_err("rejected delivery must fail");
    assert!(matches!(err, DeliveryError::Rejected(_)));
    server.await.unwrap();

    // The worker keeps serving (rejections are dropped, not fatal):
    // drop senders and confirm clean exit without any reconnect.
    drop(priority_tx);
    drop(bulk_tx);
    timeout(Duration::from_secs(2), worker)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        adapter
            .connect_calls
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
}

#[tokio::test]
async fn worker_exits_on_shutdown() {
    let (client_io, _server_io) = tokio::io::duplex(64 * 1024);
    let candidate = resolved_candidate(ConnectionInterface::Lan, "192.168.1.2");
    let adapter = std::sync::Arc::new(scripted_adapter(vec![Ok((
        MemoryConnection { io: client_io },
        candidate.clone(),
    ))]));

    let (priority_tx, priority_rx) = mpsc::channel(4);
    let (_bulk_tx, bulk_rx) = mpsc::channel(4);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let adapter_for_worker = adapter.clone();
    let worker = tokio::spawn(async move {
        run_connection_worker(
            adapter_for_worker.as_ref(),
            &fast_worker_config(),
            vec![candidate],
            "peer".into(),
            priority_rx,
            bulk_rx,
            shutdown_rx,
        )
        .await
    });

    // Let the worker connect first, then signal shutdown.
    tokio::time::sleep(Duration::from_millis(50)).await;
    shutdown_tx.send(true).unwrap();
    timeout(Duration::from_secs(2), worker)
        .await
        .expect("worker must exit on shutdown")
        .unwrap();
    drop(priority_tx);
}
