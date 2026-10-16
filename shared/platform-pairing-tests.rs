// ---------------------------------------------------------------------------
// F10-B: typed pairing entry points.
//
// The direct entry runs its production connector seam through duplex I/O.
// Remote invites run real preface bytes and Noise XX through the production
// `complete_remote_invite` helper. Both platforms include this same suite.
// ---------------------------------------------------------------------------
use super::{
    complete_remote_invite, start_pairing, start_pairing_classified, start_pairing_with_connector,
    start_remote_pairing, start_remote_pairing_classified,
};
use crate::pairing::{
    PairingFailure, PairingFailureKind, PairingManager, PairingPhase, RemotePairingInviteManager,
};

const INVITE_HELLO_LENGTH: usize = tailsync_core::pairing::invite::INVITE_HELLO_LENGTH;
const INVITE_ACK_ACCEPTED: u8 = tailsync_core::pairing::invite::INVITE_ACK_ACCEPTED;
const INVITE_ACK_REJECTED: u8 = tailsync_core::pairing::invite::INVITE_ACK_REJECTED;

fn hex_key(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn pairing_settings(mode: &str) -> Arc<Mutex<Settings>> {
    let settings = Settings {
        connection_mode: mode.to_string(),
        ..Settings::default()
    };
    Arc::new(Mutex::new(settings))
}

fn pairing_identity() -> Arc<DeviceIdentity> {
    Arc::new(DeviceIdentity::generate_for_test())
}

/// A canonical Iroh endpoint id: 32 bytes that decode to a valid ed25519 point.
/// The production canonicalizer is the oracle, so the fixture cannot drift from
/// the real accepted format.
fn valid_endpoint_ids(count: usize) -> Vec<String> {
    let mut ids = Vec::new();
    for byte in 0u8..=255 {
        let candidate = hex_key(&[byte; 32]);
        if tailsync_core::iroh_transport::canonical_endpoint_id(&candidate).is_ok() {
            ids.push(candidate);
            if ids.len() == count {
                return ids;
            }
        }
    }
    panic!("not enough valid Iroh endpoint ids");
}

fn invite_link(endpoint_id: &str) -> String {
    Arc::new(RemotePairingInviteManager::new())
        .create_from_endpoint_id(endpoint_id, tailsync_core::pairing::DEFAULT_INVITE_TTL)
        .unwrap()
        .as_link()
}

/// `AcceptedConnection` is not `Debug`, so the invite failures are unwrapped by
/// hand instead of through `expect_err`.
fn expect_invite_failure(
    result: Result<secure::AcceptedConnection, PairingFailure>,
) -> PairingFailure {
    match result {
        Ok(_) => panic!("the invite handshake unexpectedly succeeded"),
        Err(failure) => failure,
    }
}

/// The responder side of a remote invite: consume the hello, answer with `ack`
/// and, when accepted, complete the real Noise XX pairing handshake.
async fn serve_invite_peer(
    stream: tokio::io::DuplexStream,
    ack: u8,
    peer_identity: secure::PeerIdentity,
    identity: Arc<DeviceIdentity>,
) {
    let mut stream = stream;
    let mut hello = [0_u8; INVITE_HELLO_LENGTH];
    tokio::io::AsyncReadExt::read_exact(&mut stream, &mut hello)
        .await
        .unwrap();
    tokio::io::AsyncWriteExt::write_all(&mut stream, &[ack])
        .await
        .unwrap();
    tokio::io::AsyncWriteExt::flush(&mut stream).await.unwrap();
    if ack != INVITE_ACK_ACCEPTED {
        return;
    }
    let accepted = secure::accept(stream, &identity, peer_identity)
        .await
        .unwrap();
    let mut connection = accepted.connection;
    secure::write_ready(&mut connection).await.unwrap();
}

#[tokio::test]
async fn pairing_entry_rejects_a_malformed_target_with_the_legacy_text() {
    let identity = pairing_identity();
    let settings = pairing_settings("auto");
    let manager = PairingManager::new(settings.clone(), identity.clone());

    let classified = start_pairing_classified(
        manager.clone(),
        identity.clone(),
        settings.clone(),
        "not-an-address",
    )
    .await
    .expect_err("a non-address target must be rejected");
    assert_eq!(classified.kind(), PairingFailureKind::InvalidArgument);
    let legacy = start_pairing(
        manager.clone(),
        identity.clone(),
        settings.clone(),
        "not-an-address",
    )
    .await
    .expect_err("a non-address target must be rejected");
    assert_eq!(legacy, classified.message());

    // Parsing happens before the window opens, so nothing is recorded.
    let status = manager.status().await;
    assert_eq!(status.error, None);
    assert_eq!(status.failure_kind, None);
    assert!(!status.pairing_enabled);
}

#[tokio::test]
async fn pairing_entry_records_an_out_of_mode_target_as_a_non_ban() {
    let identity = pairing_identity();
    let settings = pairing_settings("lan_only");
    let manager = PairingManager::new(settings.clone(), identity.clone());
    manager.enable().await;

    // 100.64.0.0/10 is the tailnet range; `lan_only` must refuse it.
    let classified = start_pairing_classified(
        manager.clone(),
        identity.clone(),
        settings.clone(),
        "100.64.0.9",
    )
    .await
    .expect_err("an out-of-mode target must be rejected");
    assert_eq!(classified.kind(), PairingFailureKind::InvalidArgument);
    assert_eq!(
        classified.message(),
        "Peer address is outside the selected network"
    );

    let status = manager.status().await;
    assert_eq!(status.error.as_deref(), Some(classified.message()));
    assert_eq!(
        status.failure_kind,
        Some(PairingFailureKind::InvalidArgument)
    );
    assert!(status.pairing_enabled, "the first failure must not ban");
    assert_eq!(status.failed_attempts, 0);
    assert_eq!(status.phase, PairingPhase::Waiting);

    let legacy = start_pairing(
        manager.clone(),
        identity.clone(),
        settings.clone(),
        "100.64.0.9",
    )
    .await
    .expect_err("an out-of-mode target must be rejected");
    assert_eq!(legacy, classified.message());
    assert_eq!(legacy, "Peer address is outside the selected network");
}

#[tokio::test]
async fn pairing_entry_reports_a_closed_window_without_recording() {
    let identity = pairing_identity();
    let settings = pairing_settings("auto");
    let manager = PairingManager::new(settings.clone(), identity.clone());

    let classified = start_pairing_classified(
        manager.clone(),
        identity.clone(),
        settings.clone(),
        "127.0.0.1",
    )
    .await
    .expect_err("a closed window must reject the pairing attempt");
    assert_eq!(classified.kind(), PairingFailureKind::WindowClosed);
    assert_eq!(classified.message(), "Pairing window is closed");

    let status = manager.status().await;
    assert_eq!(status.error, None);
    assert_eq!(status.failure_kind, None);
    assert_eq!(status.failed_attempts, 0);

    let legacy = start_pairing(
        manager.clone(),
        identity.clone(),
        settings.clone(),
        "127.0.0.1",
    )
    .await
    .expect_err("a closed window must reject the pairing attempt");
    assert_eq!(legacy, classified.message());
}

#[tokio::test]
async fn remote_pairing_entry_rejects_a_malformed_link_and_an_iroh_less_mode() {
    let identity = pairing_identity();
    let settings = pairing_settings("lan_only");
    let manager = PairingManager::new(settings.clone(), identity.clone());

    let classified = start_remote_pairing_classified(
        manager.clone(),
        identity.clone(),
        settings.clone(),
        "not-a-link",
    )
    .await
    .expect_err("a malformed invite must be rejected");
    assert_eq!(classified.kind(), PairingFailureKind::InvalidArgument);
    assert_eq!(classified.message(), "Invalid TailSync pairing invite");
    let legacy = start_remote_pairing(
        manager.clone(),
        identity.clone(),
        settings.clone(),
        "not-a-link",
    )
    .await
    .expect_err("a malformed invite must be rejected");
    assert_eq!(legacy, classified.message());

    // A well-formed invite in a mode without Iroh is refused before the window opens.
    let link = invite_link(&valid_endpoint_ids(1)[0]);
    let classified =
        start_remote_pairing_classified(manager.clone(), identity.clone(), settings.clone(), &link)
            .await
            .expect_err("an Iroh-less mode must refuse a remote invite");
    assert_eq!(classified.kind(), PairingFailureKind::InvalidArgument);
    assert_eq!(
        classified.message(),
        "Remote Iroh pairing is unavailable in the selected connection mode"
    );
    let status = manager.status().await;
    assert_eq!(status.error, None);
    assert_eq!(status.failure_kind, None);
    assert!(!status.pairing_enabled);
}

#[tokio::test]
async fn invite_preface_rejection_is_a_typed_invalid_argument() {
    let identity = pairing_identity();
    let settings = pairing_settings("auto");
    let mode = settings.lock().await.connection_mode.clone();
    let invited = valid_endpoint_ids(1).remove(0);
    let link = invite_link(&invited);
    let invite = tailsync_core::pairing::RemotePairingInvite::parse(&link).unwrap();

    let (client_io, server_io) = tokio::io::duplex(256 * 1024);
    let peer_identity = secure::PeerIdentity {
        hostname: "invite-peer".into(),
        tailscale_ip: String::new(),
        iroh_endpoint_id: None,
    };
    let server = tokio::spawn(serve_invite_peer(
        server_io,
        INVITE_ACK_REJECTED,
        peer_identity,
        pairing_identity(),
    ));

    let failure =
        expect_invite_failure(complete_remote_invite(client_io, &invite, &identity, &mode).await);
    assert_eq!(failure.kind(), PairingFailureKind::InvalidArgument);
    assert_eq!(
        failure.message(),
        "The remote pairing invite was rejected or is no longer available"
    );
    server.await.unwrap();
}

#[tokio::test]
async fn invite_handshake_binds_the_peer_to_the_invited_endpoint() {
    let identity = pairing_identity();
    let settings = pairing_settings("auto");
    let mode = settings.lock().await.connection_mode.clone();
    let mut ids = valid_endpoint_ids(2);
    let other = ids.remove(1);
    let invited = ids.remove(0);
    let invite =
        tailsync_core::pairing::RemotePairingInvite::parse(&invite_link(&invited)).unwrap();

    for (advertised, expected) in [
        (
            None,
            Some("Peer did not bind its Noise identity to an Iroh endpoint"),
        ),
        (
            Some(other.as_str()),
            Some("Peer Iroh endpoint does not match the invite"),
        ),
        (Some(invited.as_str()), None),
    ] {
        let (client_io, server_io) = tokio::io::duplex(256 * 1024);
        let peer_identity = secure::PeerIdentity {
            hostname: "invite-peer".into(),
            tailscale_ip: String::new(),
            iroh_endpoint_id: advertised.map(str::to_string),
        };
        let server = tokio::spawn(serve_invite_peer(
            server_io,
            INVITE_ACK_ACCEPTED,
            peer_identity,
            pairing_identity(),
        ));

        let result = complete_remote_invite(client_io, &invite, &identity, &mode).await;
        match expected {
            Some(message) => {
                let failure = expect_invite_failure(result);
                assert_eq!(failure.kind(), PairingFailureKind::IdentityMismatch);
                assert_eq!(failure.message(), message);
            }
            None => {
                let accepted = result.expect("the invited endpoint must be accepted");
                assert_eq!(
                    accepted.peer_identity.iroh_endpoint_id.as_deref(),
                    Some(invited.as_str())
                );
            }
        }
        server.await.unwrap();
    }
}

#[tokio::test]
async fn invite_handshake_surfaces_a_peer_protocol_rejection() {
    let identity = pairing_identity();
    let settings = pairing_settings("auto");
    let mode = settings.lock().await.connection_mode.clone();
    let invited = valid_endpoint_ids(1).remove(0);
    let invite =
        tailsync_core::pairing::RemotePairingInvite::parse(&invite_link(&invited)).unwrap();

    let (client_io, server_io) = tokio::io::duplex(256 * 1024);
    let server = tokio::spawn(async move {
        let mut stream = server_io;
        let mut hello = [0_u8; INVITE_HELLO_LENGTH];
        tokio::io::AsyncReadExt::read_exact(&mut stream, &mut hello)
            .await
            .unwrap();
        tokio::io::AsyncWriteExt::write_all(&mut stream, &[INVITE_ACK_ACCEPTED])
            .await
            .unwrap();
        tokio::io::AsyncWriteExt::flush(&mut stream).await.unwrap();
        // Consume the client's handshake request so its write completes, then
        // reject the pairing handshake with an unsupported plain wire version.
        let mut request = [0_u8; 4096];
        let _ = tokio::io::AsyncReadExt::read(&mut stream, &mut request).await;
        let frame = Frame::try_new(Command::HandshakeAck, 0, 0, b"legacy".to_vec()).unwrap();
        tokio::io::AsyncWriteExt::write_all(&mut stream, &frame.encode_with_version(5))
            .await
            .unwrap();
        tokio::io::AsyncWriteExt::flush(&mut stream).await.unwrap();
        // Hold the stream open so the client reads the rejection, not an EOF.
        let _held = stream;
        std::future::pending::<()>().await;
    });

    let failure =
        expect_invite_failure(complete_remote_invite(client_io, &invite, &identity, &mode).await);
    assert_eq!(
        failure.kind(),
        PairingFailureKind::ProtocolIncompatible,
        "{}",
        failure.message()
    );
    assert!(
        failure.message().contains("peer uses v5"),
        "{}",
        failure.message()
    );
    server.abort();
}

#[tokio::test(start_paused = true)]
async fn invite_acknowledgement_timeout_keeps_the_timed_out_kind() {
    let identity = pairing_identity();
    let settings = pairing_settings("auto");
    let mode = settings.lock().await.connection_mode.clone();
    let invited = valid_endpoint_ids(1).remove(0);
    let invite =
        tailsync_core::pairing::RemotePairingInvite::parse(&invite_link(&invited)).unwrap();

    let (client_io, server_io) = tokio::io::duplex(256 * 1024);
    let server = tokio::spawn(async move {
        let stream = server_io;
        let mut stream = stream;
        let mut hello = [0_u8; INVITE_HELLO_LENGTH];
        tokio::io::AsyncReadExt::read_exact(&mut stream, &mut hello)
            .await
            .unwrap();
        // Hold the stream open without answering: the client must time out.
        let _held = stream;
        std::future::pending::<()>().await;
    });

    let failure =
        expect_invite_failure(complete_remote_invite(client_io, &invite, &identity, &mode).await);
    assert_eq!(failure.kind(), PairingFailureKind::TimedOut);
    assert_eq!(
        failure.message(),
        "Remote Iroh invite acknowledgement timed out"
    );
    server.abort();
}

/// The direct entry's real failure handling, driven through the injected TCP
/// connector. `TCP_PORT` on this host is owned by the running daemon, so the
/// socket is a duplex stream; everything else (parse, validate, window
/// subscription, timeout, prefix, recorder and budget effects) is the
/// production path.
#[tokio::test]
async fn direct_entry_records_handshake_failures_as_typed_non_bans() {
    let identity = pairing_identity();
    let settings = pairing_settings("auto");
    let manager = PairingManager::new(settings.clone(), identity.clone());
    manager.enable().await;

    // A real peer rejects the handshake with an unsupported plain wire version.
    let (client_io, server_io) = tokio::io::duplex(256 * 1024);
    let server = tokio::spawn(async move {
        let mut stream = server_io;
        let mut request = [0_u8; 4096];
        let _ = tokio::io::AsyncReadExt::read(&mut stream, &mut request).await;
        let frame = Frame::try_new(Command::HandshakeAck, 0, 0, b"legacy".to_vec()).unwrap();
        tokio::io::AsyncWriteExt::write_all(&mut stream, &frame.encode_with_version(5))
            .await
            .unwrap();
        tokio::io::AsyncWriteExt::flush(&mut stream).await.unwrap();
        let _held = stream;
        std::future::pending::<()>().await;
    });

    let failure = start_pairing_with_connector(
        manager.clone(),
        identity.clone(),
        settings.clone(),
        "127.0.0.1",
        |_address| async move { Ok::<_, PairingFailure>(client_io) },
    )
    .await
    .expect_err("an incompatible peer must not start a session");
    assert_eq!(
        failure.kind(),
        PairingFailureKind::ProtocolIncompatible,
        "{}",
        failure.message()
    );
    assert!(
        failure.message().starts_with("Pairing handshake failed: "),
        "{}",
        failure.message()
    );
    assert!(
        failure.message().contains("peer uses v5"),
        "{}",
        failure.message()
    );
    server.abort();

    let status = manager.status().await;
    assert_eq!(
        status.failure_kind,
        Some(PairingFailureKind::ProtocolIncompatible)
    );
    assert_eq!(status.error.as_deref(), Some(failure.message()));
    assert!(status.pairing_enabled, "the first failure must not ban");
    assert_eq!(status.failed_attempts, 0);
    assert_eq!(status.phase, PairingPhase::Waiting);

    // A structural connect failure keeps its transport kind.
    let failure = start_pairing_with_connector(
        manager.clone(),
        identity.clone(),
        settings.clone(),
        "127.0.0.1",
        |_address| async move {
            Err::<tokio::io::DuplexStream, _>(PairingFailure::from_io(std::io::Error::new(
                std::io::ErrorKind::ConnectionRefused,
                "connection refused",
            )))
        },
    )
    .await
    .expect_err("a refused connection must not start a session");
    assert_eq!(failure.kind(), PairingFailureKind::Transport);
    assert_eq!(
        failure.message(),
        "Pairing handshake failed: connection refused"
    );
    let status = manager.status().await;
    assert_eq!(status.failure_kind, Some(PairingFailureKind::Transport));
    assert_eq!(
        status.error.as_deref(),
        Some("Pairing handshake failed: connection refused")
    );
    assert!(status.pairing_enabled);
    assert_eq!(status.failed_attempts, 0);
}

struct PairingAttemptDropSignal(Arc<tokio::sync::Notify>);
impl Drop for PairingAttemptDropSignal {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}

#[tokio::test(start_paused = true)]
async fn parent_direct_handshake_deadline_records_timeout_and_drops_the_attempt() {
    let identity = pairing_identity();
    let settings = pairing_settings("auto");
    let manager = PairingManager::new(settings.clone(), identity.clone());
    manager.enable().await;
    let released = Arc::new(tokio::sync::Notify::new());
    let signal = released.clone();
    let failure = start_pairing_with_connector(
        manager.clone(),
        identity,
        settings,
        "192.168.1.5",
        move |_address| async move {
            let _drop = PairingAttemptDropSignal(signal);
            std::future::pending::<Result<tokio::io::DuplexStream, PairingFailure>>().await
        },
    )
    .await
    .unwrap_err();
    assert_eq!(failure.kind(), PairingFailureKind::TimedOut);
    assert_eq!(failure.message(), "Pairing handshake timed out");
    let status = manager.status().await;
    assert_eq!(status.failure_kind, Some(PairingFailureKind::TimedOut));
    assert_eq!(status.error.as_deref(), Some(failure.message()));
    assert!(status.pairing_enabled);
    assert_eq!(status.phase, PairingPhase::Waiting);
    assert_eq!(status.failed_attempts, 0);
    tokio::time::timeout(Duration::from_secs(1), released.notified())
        .await
        .expect("timed-out connector was retained");
}

#[tokio::test]
async fn parent_closing_window_cancels_the_real_attempt_without_overwriting_cancelled_state() {
    let identity = pairing_identity();
    let settings = pairing_settings("auto");
    let manager = PairingManager::new(settings.clone(), identity.clone());
    manager.enable().await;
    let entered = Arc::new(tokio::sync::Notify::new());
    let released = Arc::new(tokio::sync::Notify::new());
    let start = entered.clone();
    let signal = released.clone();
    let attempt_manager = manager.clone();
    let attempt = tokio::spawn(async move {
        start_pairing_with_connector(
            attempt_manager,
            identity,
            settings,
            "192.168.1.5",
            move |_address| async move {
                let _drop = PairingAttemptDropSignal(signal);
                start.notify_one();
                std::future::pending::<Result<tokio::io::DuplexStream, PairingFailure>>().await
            },
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(1), entered.notified())
        .await
        .unwrap();
    manager.cancel().await;
    let failure = tokio::time::timeout(Duration::from_secs(1), attempt)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert_eq!(failure.kind(), PairingFailureKind::WindowClosed);
    assert_eq!(failure.message(), "Pairing window was closed");
    let status = manager.status().await;
    assert_eq!(status.phase, PairingPhase::Cancelled);
    assert_eq!(status.failure_kind, Some(PairingFailureKind::Cancelled));
    assert_eq!(status.error.as_deref(), Some("Pairing was cancelled"));
    assert_eq!(status.failed_attempts, 0);
    tokio::time::timeout(Duration::from_secs(1), released.notified())
        .await
        .expect("cancelled connector was retained");
}
