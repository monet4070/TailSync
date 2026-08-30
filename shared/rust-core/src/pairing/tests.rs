use super::*;
use crate::secure::{self, HandshakePurpose, PeerIdentity};
use tokio::net::{TcpListener, TcpStream};

/// True when `needle` appears inside `haystack` as an ordered
/// subsequence: every needle event must be present in the given relative
/// order, while unrelated events may appear anywhere between them.
///
/// Test-private helper for diagnostics assertions: the collector is
/// process-global, so parallel PairingManager tests can interleave
/// foreign events into the collected stream.
fn contains_ordered_subsequence(
    haystack: &[crate::diagnostics::Event],
    needle: &[crate::diagnostics::Event],
) -> bool {
    let mut rest = needle;
    for event in haystack {
        if rest.first() == Some(event) {
            rest = &rest[1..];
            if rest.is_empty() {
                return true;
            }
        }
    }
    rest.is_empty()
}

#[test]
fn code_is_six_digits_and_independent_of_key_order() {
    let hash = [0x42; 32];
    let first = [0x11; 32];
    let second = [0x22; 32];
    let forward = derive_verification_code(&hash, &first, &second).unwrap();
    let reverse = derive_verification_code(&hash, &second, &first).unwrap();

    assert_eq!(forward, reverse);
    assert_eq!(forward.len(), 6);
    assert!(forward.bytes().all(|byte| byte.is_ascii_digit()));
}

#[test]
fn invalid_inputs_are_rejected() {
    assert!(derive_verification_code(&[], &[0; 32], &[1; 32]).is_err());
    assert!(derive_verification_code(&[0; 32], &[0; 31], &[1; 32]).is_err());
}

#[tokio::test]
async fn pairing_window_defaults_closed_and_expires() {
    let manager = PairingManager::with_policy(
        Arc::new(Mutex::new(Settings::default())),
        Arc::new(DeviceIdentity::generate_for_test()),
        Duration::from_millis(25),
        5,
        false,
    );
    let mut window = manager.subscribe_window();
    assert!(!manager.status().await.pairing_enabled);

    let enabled = manager.enable().await;
    assert!(enabled.pairing_enabled);
    assert_eq!(enabled.phase, PairingPhase::Waiting);
    assert!(*window.borrow_and_update());
    tokio::time::timeout(Duration::from_secs(1), window.changed())
        .await
        .expect("pairing window did not expire")
        .expect("pairing window signal closed");
    assert!(!*window.borrow_and_update());

    let expired = manager.status().await;
    assert!(!expired.pairing_enabled);
    assert_eq!(expired.failed_attempts, 0);
    assert_eq!(expired.phase, PairingPhase::TimedOut);
}

#[tokio::test]
async fn fifth_failure_closes_pairing_window() {
    let manager = PairingManager::with_policy(
        Arc::new(Mutex::new(Settings::default())),
        Arc::new(DeviceIdentity::generate_for_test()),
        Duration::from_secs(1),
        5,
        false,
    );
    manager.enable().await;
    for attempt in 1..=5 {
        manager.record_failure(format!("failure {attempt}")).await;
    }

    let status = manager.status().await;
    assert!(!status.pairing_enabled);
    assert_eq!(status.failed_attempts, 5);
    assert_eq!(status.phase, PairingPhase::Locked);
}

#[tokio::test]
async fn session_timeouts_count_toward_pairing_lockout() {
    let manager = PairingManager::with_policy(
        Arc::new(Mutex::new(Settings::default())),
        Arc::new(DeviceIdentity::generate_for_test()),
        Duration::from_secs(1),
        3,
        false,
    );
    manager.enable().await;

    for attempt in 1..=3 {
        let generation = {
            let mut state = manager.state.lock().await;
            let (control, _receiver) = mpsc::channel(1);
            state.control = Some(control);
            state.phase = PairingPhase::Verification;
            state.generation
        };
        manager.expire(generation).await;
        let status = manager.status().await;
        assert_eq!(status.failed_attempts, attempt);
        if attempt < 3 {
            assert!(status.pairing_enabled);
            assert_eq!(status.phase, PairingPhase::Waiting);
        }
    }

    let status = manager.status().await;
    assert!(!status.pairing_enabled);
    assert_eq!(status.phase, PairingPhase::Locked);
}

#[tokio::test(start_paused = true)]
async fn a_stalled_pairing_session_releases_the_window_before_window_expiry() {
    let server_identity = Arc::new(DeviceIdentity::generate_for_test());
    let client_identity = DeviceIdentity::generate_for_test();
    let manager = PairingManager::with_policy(
        Arc::new(Mutex::new(Settings::default())),
        server_identity.clone(),
        Duration::from_secs(120),
        5,
        false,
    );
    manager.enable().await;
    let (_client, server) = establish_in_memory_pair(&server_identity, &client_identity).await;
    manager
        .install_session(PendingPairing {
            connection: server,
            hostname: "client".into(),
            remote_public_key: client_identity.public_key().to_vec(),
            handshake_hash: vec![7; 32],
            address: "192.168.1.5".into(),
            interface: "lan".into(),
        })
        .await
        .unwrap();

    tokio::task::yield_now().await;
    tokio::time::advance(PAIRING_SESSION_TIMEOUT + Duration::from_secs(1)).await;
    for _ in 0..8 {
        tokio::task::yield_now().await;
    }

    let status = manager.status().await;
    assert!(status.pairing_enabled);
    assert_eq!(status.phase, PairingPhase::Waiting);
    assert_eq!(status.failed_attempts, 1);
    assert!(status
        .error
        .as_deref()
        .is_some_and(|error| error.contains("timed out")));
}

#[tokio::test]
async fn both_confirmations_save_both_peer_keys_and_close_windows() {
    const IROH_ENDPOINT_ID: &str =
        "5866666666666666666666666666666666666666666666666666666666666666";
    let server_settings = Arc::new(Mutex::new(Settings::default()));
    let client_settings = Arc::new(Mutex::new(Settings::default()));
    let server_identity = Arc::new(DeviceIdentity::generate_for_test());
    let client_identity = Arc::new(DeviceIdentity::generate_for_test());
    let server_manager = PairingManager::with_policy(
        server_settings.clone(),
        server_identity.clone(),
        Duration::from_secs(2),
        5,
        false,
    );
    let client_manager = PairingManager::with_policy(
        client_settings.clone(),
        client_identity.clone(),
        Duration::from_secs(2),
        5,
        false,
    );
    server_manager.enable().await;
    client_manager.enable().await;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server_identity_for_task = server_identity.clone();
    let server_manager_for_task = server_manager.clone();
    let server = tokio::spawn(async move {
        let (stream, _peer_address) = listener.accept().await.unwrap();
        let accepted = secure::accept_with_pairing_window(
            stream,
            &server_identity_for_task,
            PeerIdentity {
                hostname: "server".into(),
                tailscale_ip: String::new(),
                iroh_endpoint_id: None,
            },
            server_manager_for_task.subscribe_window(),
        )
        .await
        .unwrap();
        assert_eq!(accepted.purpose, HandshakePurpose::Pairing);
        let mut connection = accepted.connection;
        secure::write_ready(&mut connection).await.unwrap();
        server_manager_for_task
            .install_session(PendingPairing {
                connection,
                hostname: accepted.peer_identity.hostname,
                remote_public_key: accepted.remote_public_key,
                handshake_hash: accepted.handshake_hash,
                address: IROH_ENDPOINT_ID.into(),
                interface: "iroh".into(),
            })
            .await
            .unwrap();
    });

    let accepted = secure::connect_pairing(
        TcpStream::connect(address).await.unwrap(),
        &client_identity,
        PeerIdentity {
            hostname: "client".into(),
            tailscale_ip: String::new(),
            iroh_endpoint_id: None,
        },
    )
    .await
    .unwrap();
    client_manager
        .install_session(PendingPairing {
            connection: accepted.connection,
            hostname: accepted.peer_identity.hostname,
            remote_public_key: accepted.remote_public_key,
            handshake_hash: accepted.handshake_hash,
            address: IROH_ENDPOINT_ID.into(),
            interface: "iroh".into(),
        })
        .await
        .unwrap();
    server.await.unwrap();

    let server_code = server_manager
        .status()
        .await
        .peer
        .unwrap()
        .verification_code;
    let client_code = client_manager
        .status()
        .await
        .peer
        .unwrap()
        .verification_code;
    assert_eq!(server_code, client_code);

    server_manager.confirm().await.unwrap();
    client_manager.confirm().await.unwrap();
    for _ in 0..50 {
        if server_manager.status().await.phase == PairingPhase::Paired
            && client_manager.status().await.phase == PairingPhase::Paired
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    assert_eq!(server_manager.status().await.phase, PairingPhase::Paired);
    assert_eq!(client_manager.status().await.phase, PairingPhase::Paired);
    assert!(!server_manager.status().await.pairing_enabled);
    assert!(!client_manager.status().await.pairing_enabled);
    assert_eq!(
        server_settings.lock().await.trusted_peer_keys.get("client"),
        Some(&client_identity.public_key_base64())
    );
    assert_eq!(
        client_settings.lock().await.trusted_peer_keys.get("server"),
        Some(&server_identity.public_key_base64())
    );
    assert_eq!(
        server_settings
            .lock()
            .await
            .trusted_peer_addresses
            .get("client")
            .and_then(|routes| routes.get("iroh"))
            .map(String::as_str),
        Some(IROH_ENDPOINT_ID)
    );
    assert_eq!(
        client_settings
            .lock()
            .await
            .trusted_peer_addresses
            .get("server")
            .and_then(|routes| routes.get("iroh"))
            .map(String::as_str),
        Some(IROH_ENDPOINT_ID)
    );
}

#[tokio::test]
async fn pairing_waits_for_remote_persisted_ack_before_marking_paired() {
    let server_identity = Arc::new(DeviceIdentity::generate_for_test());
    let client_identity = DeviceIdentity::generate_for_test();
    let server_settings = Arc::new(Mutex::new(Settings::default()));
    let server_manager = PairingManager::with_policy(
        server_settings.clone(),
        server_identity.clone(),
        Duration::from_secs(2),
        5,
        false,
    );
    server_manager.enable().await;

    let (mut client, server) = establish_in_memory_pair(&server_identity, &client_identity).await;
    server_manager
        .install_session(PendingPairing {
            connection: server,
            hostname: "client".into(),
            remote_public_key: client_identity.public_key().to_vec(),
            handshake_hash: vec![7; 32],
            address: "5866666666666666666666666666666666666666666666666666666666666666".into(),
            interface: "iroh".into(),
        })
        .await
        .unwrap();

    server_manager.confirm().await.unwrap();
    let frame = client.read_frame().await.unwrap();
    assert_eq!(frame.command, Command::PairingConfirm);
    client
        .write_frame(&Frame::try_new(Command::PairingConfirm, 0, 0, Vec::new()).unwrap())
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_ne!(server_manager.status().await.phase, PairingPhase::Paired);

    let frame = client.read_frame().await.unwrap();
    assert_eq!(frame.command, Command::PairingPersisted);
    client
        .write_frame(&Frame::try_new(Command::PairingPersisted, 0, 0, Vec::new()).unwrap())
        .await
        .unwrap();

    for _ in 0..50 {
        if server_manager.status().await.phase == PairingPhase::Paired {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(server_manager.status().await.phase, PairingPhase::Paired);
    assert_eq!(
        server_settings.lock().await.trusted_peer_keys.get("client"),
        Some(&client_identity.public_key_base64())
    );
}

// ------------------------------------------------------------------
// install_pairing_session (T110): inbound pairing session install.
// ------------------------------------------------------------------

fn test_peer_identity() -> PeerIdentity {
    PeerIdentity {
        hostname: "server".into(),
        tailscale_ip: String::new(),
        iroh_endpoint_id: None,
    }
}

async fn establish_in_memory_pair(
    server_identity: &Arc<DeviceIdentity>,
    client_identity: &DeviceIdentity,
) -> (
    crate::secure::SecureConnection,
    crate::secure::SecureConnection,
) {
    let expected_key = server_identity.public_key().to_vec();
    let (client_io, server_io) = tokio::io::duplex(256 * 1024);
    let server_identity = server_identity.clone();
    let server = tokio::spawn(async move {
        let accepted = crate::secure::accept(server_io, &server_identity, test_peer_identity())
            .await
            .unwrap();
        let mut connection = accepted.connection;
        crate::secure::write_ready(&mut connection).await.unwrap();
        connection
    });
    let client = crate::secure::connect(
        client_io,
        client_identity,
        test_peer_identity(),
        "server",
        &expected_key,
    )
    .await
    .unwrap();
    let server = server.await.unwrap();
    (client, server)
}

fn default_manager() -> Arc<PairingManager> {
    PairingManager::with_policy(
        Arc::new(Mutex::new(Settings::default())),
        Arc::new(DeviceIdentity::generate_for_test()),
        Duration::from_secs(60),
        5,
        false,
    )
}

#[tokio::test]
async fn install_pairing_session_without_manager_writes_error_frame() {
    let server_identity = Arc::new(DeviceIdentity::generate_for_test());
    let client_identity = DeviceIdentity::generate_for_test();
    let (mut client, server) = establish_in_memory_pair(&server_identity, &client_identity).await;

    let result = install_pairing_session(
        None,
        server,
        "peer".into(),
        vec![1; 32],
        vec![2; 32],
        "192.168.1.5".into(),
        "lan".into(),
    )
    .await;
    assert!(result.is_ok());

    let frame = client.read_frame().await.unwrap();
    assert_eq!(frame.command, crate::protocol::Command::PeerError);
    assert!(String::from_utf8_lossy(&frame.payload).contains("Pairing over Iroh is not supported"));
}

#[tokio::test]
async fn install_pairing_session_rejects_closed_window() {
    let server_identity = Arc::new(DeviceIdentity::generate_for_test());
    let client_identity = DeviceIdentity::generate_for_test();
    let (_client, server) = establish_in_memory_pair(&server_identity, &client_identity).await;

    let error = install_pairing_session(
        Some(&default_manager()),
        server,
        "peer".into(),
        vec![1; 32],
        vec![2; 32],
        "192.168.1.5".into(),
        "lan".into(),
    )
    .await
    .unwrap_err();
    assert!(matches!(error, PairingError::WindowClosed));
    assert_eq!(error.to_string(), "Pairing window is closed");
}

#[tokio::test]
async fn install_pairing_session_rejects_self_pairing() {
    let server_identity = Arc::new(DeviceIdentity::generate_for_test());
    let client_identity = DeviceIdentity::generate_for_test();
    let (_client, server) = establish_in_memory_pair(&server_identity, &client_identity).await;
    let manager_identity = Arc::new(DeviceIdentity::generate_for_test());
    let manager = PairingManager::with_policy(
        Arc::new(Mutex::new(Settings::default())),
        manager_identity.clone(),
        Duration::from_secs(60),
        5,
        false,
    );
    let own_key = manager_identity.public_key().to_vec();

    let error = install_pairing_session(
        Some(&manager),
        server,
        "peer".into(),
        own_key,
        vec![2; 32],
        "192.168.1.5".into(),
        "lan".into(),
    )
    .await
    .unwrap_err();
    assert!(matches!(error, PairingError::SelfPairing));
    assert_eq!(error.to_string(), "Cannot pair this device with itself");
}

#[tokio::test]
async fn install_pairing_session_rejects_invalid_interface() {
    let server_identity = Arc::new(DeviceIdentity::generate_for_test());
    let client_identity = DeviceIdentity::generate_for_test();
    let (_client, server) = establish_in_memory_pair(&server_identity, &client_identity).await;

    let error = install_pairing_session(
        Some(&default_manager()),
        server,
        "peer".into(),
        vec![1; 32],
        vec![2; 32],
        "192.168.1.5".into(),
        "bogus".into(),
    )
    .await
    .unwrap_err();
    assert!(matches!(error, PairingError::InvalidInterface));
    assert_eq!(error.to_string(), "Invalid pairing interface");
}

#[tokio::test]
async fn diagnostics_events_follow_the_pairing_lifecycle() {
    let _guard = crate::diagnostics::diagnostics_test_lock().lock().await;
    use crate::diagnostics::{Event, Record};
    let emitted = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = emitted.clone();
    crate::diagnostics::set_collector(Some(Box::new(move |record: &Record| {
        sink.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(record.event);
    })));

    let manager = default_manager();
    let status = manager.enable().await;
    assert!(status.pairing_enabled);
    manager.begin_handshake().await.unwrap();
    let _ = manager.cancel().await;
    manager.enable().await;
    let _ = manager.cancel().await;

    crate::diagnostics::set_collector(None);
    let events = emitted
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // The collector is process-global, so unrelated PairingManager tests
    // running in parallel may emit their own events into this sink.
    // Assert the lifecycle as an ordered subsequence: the five events
    // this test triggers must all be present in their relative order,
    // while foreign events may appear anywhere between them.
    let expected = [
        Event::PairingWindowOpened,
        Event::PairingHandshakeStarted,
        Event::PairingWindowClosed,
        Event::PairingWindowOpened,
        Event::PairingWindowClosed,
    ];
    assert!(
        contains_ordered_subsequence(&events, &expected),
        "open/handshake/close/open/close must appear in order, got {events:?}"
    );
}

#[tokio::test]
async fn diagnostics_pairing_failed_records_the_error() {
    let _guard = crate::diagnostics::diagnostics_test_lock().lock().await;
    use crate::diagnostics::{Event, Record};
    let errors = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = errors.clone();
    crate::diagnostics::set_collector(Some(Box::new(move |record: &Record| {
        // The collector is process-global: other PairingManager tests may
        // emit their own PairingFailed events concurrently (e.g. the
        // lockout tests). The error message uniquely identifies the
        // failure this test triggers, so only that event is collected.
        if let Some(error) = &record.error {
            if record.event == Event::PairingFailed && error.message == "simulated failure" {
                sink.lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(error.message.clone());
            }
        }
    })));

    let manager = default_manager();
    manager.enable().await;
    manager.record_failure("simulated failure").await;

    crate::diagnostics::set_collector(None);
    let messages = errors
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert_eq!(
        messages.as_slice(),
        &["simulated failure".to_string()],
        "the targeted failure must be recorded exactly once"
    );
}
