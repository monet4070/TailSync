//! S3-P1-2: a pairing that only one device completed must never become trust.
//!
//! The audited failure: the session wrote a durable trust record as soon as both
//! users had confirmed, *before* the peer acknowledged its own write. A drop or
//! a timeout in between left one device holding trust for a device the other had
//! never accepted, with nothing on disk recording that the record was
//! half-formed, so nothing could reconcile it.
//!
//! Each test injects the failure at one boundary the session crosses — after
//! local confirmation, after the local persistence note, after the send of
//! `PairingPersisted`, and at the session timeout — and asserts the same
//! invariant every time: the peer stays out of `trusted_peer_keys`, so every
//! admission path and both UIs keep seeing an unpaired device, while a pending
//! note survives so the pairing can still be completed later.

use super::tests::establish_in_memory_pair;
use super::*;

const IROH_ENDPOINT_ID: &str = "5866666666666666666666666666666666666666666666666666666666666666";

type Connection = crate::secure::SecureConnection;

fn manager_for(
    settings: &Arc<Mutex<Settings>>,
    identity: &Arc<DeviceIdentity>,
) -> Arc<PairingManager> {
    PairingManager::with_policy(
        settings.clone(),
        identity.clone(),
        Duration::from_secs(2),
        5,
        false,
    )
}

/// Install an inbound session for `client_identity` over `server_connection`.
async fn install_inbound(
    manager: &Arc<PairingManager>,
    server_connection: Connection,
    client_identity: &DeviceIdentity,
) {
    manager
        .install_session(PendingPairing {
            connection: server_connection,
            hostname: "client".into(),
            remote_public_key: client_identity.public_key().to_vec(),
            handshake_hash: vec![7; 32],
            address: IROH_ENDPOINT_ID.into(),
            interface: "iroh".into(),
            remote_invite: None,
            direction: PairingDirection::Inbound,
        })
        .await
        .expect("install the inbound pairing session");
}

/// Install a session where *this* side is the initiator. The interop fixtures
/// need both directions: a new side receiving an older peer's pairing and a new
/// side opening one against an older peer.
async fn install_outbound(
    manager: &Arc<PairingManager>,
    client_connection: Connection,
    server_identity: &DeviceIdentity,
) {
    manager
        .install_session(PendingPairing {
            connection: client_connection,
            hostname: "server".into(),
            remote_public_key: server_identity.public_key().to_vec(),
            handshake_hash: vec![9; 32],
            address: IROH_ENDPOINT_ID.into(),
            interface: "iroh".into(),
            remote_invite: None,
            direction: PairingDirection::Outbound,
        })
        .await
        .expect("install the outbound pairing session");
}

/// Both users confirm and the peer echoes its confirmation, so the local side
/// writes its pending note and sends `PairingPersisted`. Returns after the peer
/// has *received* that frame but before it answers.
async fn reach_local_persistence(
    manager: &Arc<PairingManager>,
    client: &mut Connection,
) -> Vec<u8> {
    manager.confirm().await.expect("local confirmation");
    let frame = client.read_frame().await.expect("pairing confirmation");
    assert_eq!(frame.command, Command::PairingConfirm);
    client
        .write_frame(&Frame::try_new(Command::PairingConfirm, 0, 0, Vec::new()).unwrap())
        .await
        .expect("peer confirmation");
    let frame = client
        .read_frame()
        .await
        .expect("the local side persists before it advertises completion");
    frame.payload
}

async fn wait_for_phase(manager: &Arc<PairingManager>, phase: PairingPhase) {
    for _ in 0..80 {
        if manager.status().await.phase == phase {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!(
        "pairing never reached {phase:?}; it is {:?}",
        manager.status().await.phase
    );
}

/// The audited boundary: the peer never acknowledges, and the link dies.
/// The local side must not be left trusting it, and the half-confirmed pairing
/// must still be discoverable.
#[tokio::test]
async fn a_link_that_dies_before_the_peer_acknowledges_leaves_a_pending_note_not_trust() {
    let settings = Arc::new(Mutex::new(Settings::default()));
    let (server_identity, client_identity) = (
        Arc::new(DeviceIdentity::generate_for_test()),
        DeviceIdentity::generate_for_test(),
    );
    let manager = manager_for(&settings, &server_identity);
    manager.enable().await;

    let (mut client, server) = establish_in_memory_pair(&server_identity, &client_identity).await;
    install_inbound(&manager, server, &client_identity).await;

    manager.confirm().await.expect("local confirmation");
    let frame = client.read_frame().await.unwrap();
    assert_eq!(frame.command, Command::PairingConfirm);
    client
        .write_frame(&Frame::try_new(Command::PairingConfirm, 0, 0, Vec::new()).unwrap())
        .await
        .unwrap();
    let frame = client.read_frame().await.unwrap();
    assert_eq!(
        frame.command,
        Command::PairingPersisted,
        "the local side must advertise its persistence before the peer acks"
    );
    // The peer never answers; the link dies here.
    drop(client);
    tokio::time::sleep(Duration::from_millis(100)).await;

    assert_ne!(
        manager.status().await.phase,
        PairingPhase::Paired,
        "a pairing the peer never acknowledged must not be reported as completed"
    );
    assert_eq!(
        settings.lock().await.trusted_peer_keys.get("client"),
        None,
        "the peer must not become trusted, or every admission path will accept it"
    );
    let pending = manager.pending_trust().await;
    assert_eq!(
        pending.len(),
        1,
        "the half-confirmed pairing must be recorded"
    );
    assert_eq!(pending[0].hostname, "client");
    assert_eq!(pending[0].public_key, client_identity.public_key_base64());
}

/// The same invariant when the session times out after confirmation. The note
/// must survive: the peer may already have persisted and lost the
/// acknowledgement, so deleting the local half would strand it.
#[tokio::test]
async fn a_session_timeout_after_confirmation_keeps_the_pending_note() {
    let settings = Arc::new(Mutex::new(Settings::default()));
    let server_identity = Arc::new(DeviceIdentity::generate_for_test());
    let client_identity = DeviceIdentity::generate_for_test();
    let manager = PairingManager::with_policy(
        settings.clone(),
        server_identity.clone(),
        Duration::from_millis(300),
        5,
        false,
    );
    manager.enable().await;

    let (mut client, server) = establish_in_memory_pair(&server_identity, &client_identity).await;
    install_inbound(&manager, server, &client_identity).await;
    let payload = reach_local_persistence(&manager, &mut client).await;
    assert!(
        payload.is_empty(),
        "the completion frame carries no payload yet"
    );

    wait_for_phase(&manager, PairingPhase::TimedOut).await;

    assert_eq!(
        settings.lock().await.trusted_peer_keys.get("client"),
        None,
        "a timed-out pairing must not leave trust behind"
    );
    assert_eq!(
        manager.pending_trust().await.len(),
        1,
        "a timeout is not evidence the peer failed to persist, so the note stays"
    );
}

/// A repeated confirmation is a user clicking twice, not a second pairing. It
/// must not write a second note, and must not grant trust on its own.
#[tokio::test]
async fn repeated_confirmation_is_idempotent_and_grants_nothing() {
    let settings = Arc::new(Mutex::new(Settings::default()));
    let server_identity = Arc::new(DeviceIdentity::generate_for_test());
    let client_identity = DeviceIdentity::generate_for_test();
    let manager = manager_for(&settings, &server_identity);
    manager.enable().await;

    let (mut client, server) = establish_in_memory_pair(&server_identity, &client_identity).await;
    install_inbound(&manager, server, &client_identity).await;

    manager.confirm().await.expect("first confirmation");
    manager.confirm().await.expect("repeat confirmation");
    let frame = client.read_frame().await.unwrap();
    assert_eq!(frame.command, Command::PairingConfirm);
    client
        .write_frame(&Frame::try_new(Command::PairingConfirm, 0, 0, Vec::new()).unwrap())
        .await
        .unwrap();
    let frame = client.read_frame().await.unwrap();
    assert_eq!(
        frame.command,
        Command::PairingPersisted,
        "exactly one persistence advertisement follows"
    );
    tokio::time::sleep(Duration::from_millis(50)).await;

    assert_eq!(manager.pending_trust().await.len(), 1);
    assert_eq!(settings.lock().await.trusted_peer_keys.get("client"), None);
}

/// The happy path still ends in exactly one authoritative record and no leftover
/// note — the fix must not make pairing permanently pending.
#[tokio::test]
async fn a_completed_pairing_promotes_to_trust_and_clears_the_note() {
    let settings = Arc::new(Mutex::new(Settings::default()));
    let server_identity = Arc::new(DeviceIdentity::generate_for_test());
    let client_identity = DeviceIdentity::generate_for_test();
    let manager = manager_for(&settings, &server_identity);
    manager.enable().await;

    let (mut client, server) = establish_in_memory_pair(&server_identity, &client_identity).await;
    install_inbound(&manager, server, &client_identity).await;

    manager.confirm().await.expect("local confirmation");
    let frame = client.read_frame().await.unwrap();
    assert_eq!(frame.command, Command::PairingConfirm);
    client
        .write_frame(&Frame::try_new(Command::PairingConfirm, 0, 0, Vec::new()).unwrap())
        .await
        .unwrap();
    let frame = client.read_frame().await.unwrap();
    assert_eq!(frame.command, Command::PairingPersisted);

    // Before the peer's acknowledgement the device is recorded but not trusted.
    assert_eq!(manager.pending_trust().await.len(), 1);
    assert_eq!(settings.lock().await.trusted_peer_keys.get("client"), None);

    client
        .write_frame(&Frame::try_new(Command::PairingPersisted, 0, 0, Vec::new()).unwrap())
        .await
        .unwrap();
    wait_for_phase(&manager, PairingPhase::Paired).await;

    assert_eq!(
        settings.lock().await.trusted_peer_keys.get("client"),
        Some(&client_identity.public_key_base64())
    );
    assert!(
        manager.pending_trust().await.is_empty(),
        "the note is cleared once the pairing is authoritative"
    );
}

/// A restart must not turn the note into trust, and must not drop it either.
#[tokio::test]
async fn a_restart_keeps_the_note_and_still_reports_the_device_unpaired() {
    let root = std::env::temp_dir().join(format!(
        "tailsync-pending-restart-{:016x}",
        rand::random::<u64>()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("pairing-pending.json");

    let server_identity = Arc::new(DeviceIdentity::generate_for_test());
    let client_identity = DeviceIdentity::generate_for_test();
    let settings = Arc::new(Mutex::new(Settings::default()));

    // First run: reach local persistence, then die before the peer acknowledges.
    {
        let manager = PairingManager::with_pending_store_at(
            settings.clone(),
            server_identity.clone(),
            path.clone(),
        );
        manager.enable().await;
        let (mut client, server) =
            establish_in_memory_pair(&server_identity, &client_identity).await;
        install_inbound(&manager, server, &client_identity).await;
        reach_local_persistence(&manager, &mut client).await;
        drop(client);
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(manager.pending_trust().await.len(), 1);
        assert!(path.exists(), "the note is on disk before the session ends");
    }

    // Second run: a fresh manager over the same file.
    let restarted = PairingManager::with_pending_store_at(
        settings.clone(),
        server_identity.clone(),
        path.clone(),
    );
    let pending = restarted.pending_trust().await;
    assert_eq!(pending.len(), 1, "the note survives a restart");
    assert_eq!(pending[0].hostname, "client");
    assert_eq!(
        settings.lock().await.trusted_peer_keys.get("client"),
        None,
        "a restarted device must not adopt a half-confirmed pairing as trust"
    );

    std::fs::remove_dir_all(&root).unwrap();
}

/// The check in `promote_pairing` and the settings write that follows it must be
/// indivisible. If they are not, a cancellation that lands in between is
/// silently ignored: the session reports itself cancelled while the device stays
/// durably trusted.
///
/// The window is forced open with a test-only gate that parks the promotion
/// between the two steps, so this is deterministic rather than a race that only
/// sometimes reproduces.
#[tokio::test]
async fn a_cancellation_cannot_slip_between_the_check_and_the_trust_write() {
    let settings = Arc::new(Mutex::new(Settings::default()));
    let server_identity = Arc::new(DeviceIdentity::generate_for_test());
    let client_identity = DeviceIdentity::generate_for_test();
    let manager = manager_for(&settings, &server_identity);
    manager.enable().await;

    let gate = Arc::new(PromotionGate {
        entered: tokio::sync::Notify::new(),
        release: tokio::sync::Notify::new(),
    });
    manager.install_promotion_gate(gate.clone()).await;

    let (mut client, server) = establish_in_memory_pair(&server_identity, &client_identity).await;
    install_inbound(&manager, server, &client_identity).await;
    reach_local_persistence(&manager, &mut client).await;

    // The peer's acknowledgement drives the session into promotion, which parks
    // inside the window.
    client
        .write_frame(&Frame::try_new(Command::PairingPersisted, 0, 0, Vec::new()).unwrap())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), gate.entered.notified())
        .await
        .expect("promotion must reach its critical section");

    // Nothing may complete in there: a cancellation that did would leave the
    // session reported as cancelled and the device trusted at the same time.
    let cancelled = tokio::time::timeout(Duration::from_millis(250), manager.cancel()).await;
    assert!(
        cancelled.is_err(),
        "a cancellation completed inside the promotion window"
    );

    gate.release.notify_one();
    wait_for_phase(&manager, PairingPhase::Paired).await;
    assert_eq!(
        settings.lock().await.trusted_peer_keys.get("client"),
        Some(&client_identity.public_key_base64())
    );
}

/// A session the user cancels before the peer acknowledges must not become
/// trusted, and must not be reported as paired.
#[tokio::test]
async fn a_cancelled_session_never_writes_trust() {
    let settings = Arc::new(Mutex::new(Settings::default()));
    let server_identity = Arc::new(DeviceIdentity::generate_for_test());
    let client_identity = DeviceIdentity::generate_for_test();
    let manager = manager_for(&settings, &server_identity);
    manager.enable().await;

    let (mut client, server) = establish_in_memory_pair(&server_identity, &client_identity).await;
    install_inbound(&manager, server, &client_identity).await;
    reach_local_persistence(&manager, &mut client).await;

    manager.cancel().await;
    assert_eq!(manager.status().await.phase, PairingPhase::Cancelled);

    // The peer acknowledges into a session the user has already ended.
    let _ = client
        .write_frame(&Frame::try_new(Command::PairingPersisted, 0, 0, Vec::new()).unwrap())
        .await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    assert_eq!(
        settings.lock().await.trusted_peer_keys.get("client"),
        None,
        "a cancelled session must never leave the device trusted"
    );
    assert_ne!(manager.status().await.phase, PairingPhase::Paired);
}

/// A note for a device the user removes must not linger on disk.
#[tokio::test]
async fn forgetting_a_device_also_drops_its_half_confirmed_note() {
    let root = std::env::temp_dir().join(format!(
        "tailsync-pending-forget-{:016x}",
        rand::random::<u64>()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("pairing-pending.json");

    let settings = Arc::new(Mutex::new(Settings::default()));
    let server_identity = Arc::new(DeviceIdentity::generate_for_test());
    let client_identity = DeviceIdentity::generate_for_test();
    let manager = PairingManager::with_pending_store_at(
        settings.clone(),
        server_identity.clone(),
        path.clone(),
    );
    manager.enable().await;

    let (mut client, server) = establish_in_memory_pair(&server_identity, &client_identity).await;
    install_inbound(&manager, server, &client_identity).await;
    reach_local_persistence(&manager, &mut client).await;
    assert_eq!(manager.pending_trust().await.len(), 1);

    manager.forget_pending("client").await;
    assert!(manager.pending_trust().await.is_empty());

    // And the removal is durable, not just in memory.
    let reloaded = PendingTrustStore::load_from_path(&path);
    assert!(
        reloaded.is_empty(),
        "the note must not come back on the next start"
    );

    std::fs::remove_dir_all(&root).unwrap();
}

/// The store is a sidecar, not a `Settings` field, precisely so an older build
/// can still read the configuration. The first half is a **shape** guard on the
/// settings struct: `Settings` denies unknown fields, so any new key (not just a
/// `pending*` one) would make an older build fail to load the whole file — it
/// cannot fail unless someone edits `Settings`, and it is here to make that
/// consequence explicit at the point where a change would be tempting. The
/// second half parses a frozen older settings file for real.
#[tokio::test]
async fn the_settings_shape_stays_readable_by_an_older_build() {
    let serialized = serde_json::to_value(Settings::default()).unwrap();
    let object = serialized.as_object().unwrap();
    assert!(
        !object.keys().any(|key| key.contains("pending")),
        "a new Settings key would make deny_unknown_fields reject the whole file on an older build; \
         the pending pairing lives in a sidecar for exactly this reason"
    );
    // The frozen shape an older build writes and expects to read back.
    let legacy = serde_json::json!({
        "notifications_enabled": true,
        "progress_bar_enabled": true,
        "sync_enabled": true,
        "sync_shortcut": "CommandOrControl+Shift+S",
        "history_shortcut": "CommandOrControl+Shift+H",
        "history_limit": 100,
        "storage_root": null,
        "storage_quota_bytes": 10737418240u64,
        "enabled_peers": {},
        "language": "en",
        "connection_mode": "auto",
        "trusted_peer_keys": {},
        "trusted_peer_addresses": {},
        "paired_peer_endpoints": {}
    });
    let parsed: Settings =
        serde_json::from_value(legacy).expect("an older settings file still loads");
    assert!(parsed.trusted_peer_keys.is_empty());
}

/// The frozen artifact: a `config-v2.json` exactly as the pre-fix build writes
/// it, including a device that build had already paired. A new build must read
/// it unchanged — the paired device stays paired, and the sidecar's absence is
/// simply "nothing pending".
#[test]
fn a_frozen_legacy_configuration_still_loads_with_its_trust_intact() {
    let frozen = include_str!("../../tests/fixtures/legacy-config-v2.json");
    let settings: Settings =
        serde_json::from_str(frozen).expect("the frozen legacy configuration must load");

    assert_eq!(
        settings.trusted_peer_keys.get("frozen-legacy-mac"),
        Some(&"dGhpcyBpcyBhIGZyb3plbiBsZWdhY3kgcHVibGljIGtleSB2YWx1ZQ==".to_string()),
        "a device the older build paired must stay paired"
    );
    assert_eq!(
        settings
            .trusted_peer_addresses
            .get("frozen-legacy-mac")
            .and_then(|routes| routes.get("lan"))
            .map(String::as_str),
        Some("100.64.0.7")
    );

    // And the new store, absent on that older installation, reads as empty
    // rather than as an error.
    let root = std::env::temp_dir().join(format!(
        "tailsync-legacy-store-{:016x}",
        rand::random::<u64>()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let store = PendingTrustStore::load_from_path(&root.join("pairing-pending.json"));
    assert!(
        store.is_empty(),
        "a missing sidecar is simply no pending pairing"
    );
    std::fs::remove_dir_all(&root).unwrap();
}

/// An older peer, frozen: it persists as soon as both users confirmed, sends
/// `PairingPersisted`, and considers itself paired once it receives ours. This is
/// the wire behaviour a new side must still interoperate with, so the sequence
/// is written out rather than described.
struct FrozenLegacyPeer {
    connection: Connection,
}

impl FrozenLegacyPeer {
    fn new(connection: Connection) -> Self {
        Self { connection }
    }

    /// Confirm, then persist immediately (the pre-fix policy), then wait for the
    /// new side's persistence frame.
    async fn run(&mut self) -> Result<(), String> {
        self.connection
            .write_frame(&Frame::try_new(Command::PairingConfirm, 0, 0, Vec::new()).unwrap())
            .await
            .map_err(|error| error.to_string())?;
        let frame = self
            .connection
            .read_frame()
            .await
            .map_err(|error| error.to_string())?;
        if frame.command != Command::PairingConfirm {
            return Err(format!("expected a confirmation, got {:?}", frame.command));
        }
        self.connection
            .write_frame(&Frame::try_new(Command::PairingPersisted, 0, 0, Vec::new()).unwrap())
            .await
            .map_err(|error| error.to_string())?;
        let frame = self
            .connection
            .read_frame()
            .await
            .map_err(|error| error.to_string())?;
        if frame.command != Command::PairingPersisted {
            return Err(format!(
                "the older peer only concludes success on our persistence frame, got {:?}",
                frame.command
            ));
        }
        Ok(())
    }
}

/// Old peer → new side. The older device's policy is unchanged and its sequence
/// is exactly what it always was; the new side must still complete the pairing.
#[tokio::test]
async fn an_older_peer_pairing_into_the_new_side_still_completes() {
    let settings = Arc::new(Mutex::new(Settings::default()));
    let server_identity = Arc::new(DeviceIdentity::generate_for_test());
    let client_identity = DeviceIdentity::generate_for_test();
    let manager = manager_for(&settings, &server_identity);
    manager.enable().await;

    let (client, server) = establish_in_memory_pair(&server_identity, &client_identity).await;
    install_inbound(&manager, server, &client_identity).await;
    let mut legacy = FrozenLegacyPeer::new(client);
    let legacy_task = tokio::spawn(async move { legacy.run().await });

    manager.confirm().await.expect("local confirmation");
    wait_for_phase(&manager, PairingPhase::Paired).await;
    legacy_task
        .await
        .unwrap()
        .expect("the older peer's sequence");

    assert_eq!(
        settings.lock().await.trusted_peer_keys.get("client"),
        Some(&client_identity.public_key_base64())
    );
    assert!(manager.pending_trust().await.is_empty());
}

/// New side → old peer: this side opens the session. The pass condition names
/// both interop directions, so the initiator is exercised rather than assumed —
/// the note/promote split must not depend on who started the pairing.
#[tokio::test]
async fn the_new_side_pairing_out_to_an_older_peer_still_completes() {
    let settings = Arc::new(Mutex::new(Settings::default()));
    let server_identity = Arc::new(DeviceIdentity::generate_for_test());
    let client_identity = Arc::new(DeviceIdentity::generate_for_test());
    // This side is the initiator, so the manager holds the outbound half.
    let manager = manager_for(&settings, &client_identity);
    manager.enable().await;

    let (client, server) = establish_in_memory_pair(&server_identity, &client_identity).await;
    install_outbound(&manager, client, &server_identity).await;
    let mut legacy = FrozenLegacyPeer::new(server);
    let legacy_task = tokio::spawn(async move { legacy.run().await });

    manager.confirm().await.expect("local confirmation");
    wait_for_phase(&manager, PairingPhase::Paired).await;
    legacy_task
        .await
        .unwrap()
        .expect("the older peer's sequence");

    assert_eq!(
        settings.lock().await.trusted_peer_keys.get("server"),
        Some(&server_identity.public_key_base64())
    );
    assert!(manager.pending_trust().await.is_empty());
}

/// The boundary before a note exists at all: the local user confirms, the peer
/// never does, and the link dies. Nothing may be recorded, because there is no
/// half-pairing yet — a note here would claim the peer had taken part in
/// something it never saw.
#[tokio::test]
async fn a_link_that_dies_before_the_peer_confirms_records_nothing() {
    let settings = Arc::new(Mutex::new(Settings::default()));
    let server_identity = Arc::new(DeviceIdentity::generate_for_test());
    let client_identity = DeviceIdentity::generate_for_test();
    let manager = manager_for(&settings, &server_identity);
    manager.enable().await;

    let (mut client, server) = establish_in_memory_pair(&server_identity, &client_identity).await;
    install_inbound(&manager, server, &client_identity).await;

    manager.confirm().await.expect("local confirmation");
    let frame = client.read_frame().await.expect("our confirmation");
    assert_eq!(frame.command, Command::PairingConfirm);
    // The peer never confirms; the link dies.
    drop(client);
    tokio::time::sleep(Duration::from_millis(100)).await;

    assert_ne!(manager.status().await.phase, PairingPhase::Paired);
    assert_eq!(settings.lock().await.trusted_peer_keys.get("client"), None);
    assert!(
        manager.pending_trust().await.is_empty(),
        "one confirmation is not a half-pairing, so nothing may be recorded"
    );
}

/// The ordering property the interop safety rests on: the older peer can only
/// conclude success after it has received our persistence frame, which we send
/// only once the local note is durable. So an older peer that shows "paired"
/// always corresponds to a new side that holds a record it can still act on.
#[tokio::test]
async fn the_older_peer_concludes_success_only_after_the_new_note_is_durable() {
    let root = std::env::temp_dir().join(format!(
        "tailsync-interop-order-{:016x}",
        rand::random::<u64>()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("pairing-pending.json");

    let settings = Arc::new(Mutex::new(Settings::default()));
    let server_identity = Arc::new(DeviceIdentity::generate_for_test());
    let client_identity = DeviceIdentity::generate_for_test();
    let manager = PairingManager::with_pending_store_at(
        settings.clone(),
        server_identity.clone(),
        path.clone(),
    );
    manager.enable().await;

    let (client, server) = establish_in_memory_pair(&server_identity, &client_identity).await;
    install_inbound(&manager, server, &client_identity).await;

    // Stand in for the older peer: it reads our frames and records whether the
    // durable note existed on disk at the moment it received our persistence
    // frame.
    let note_path = path.clone();
    let observer = tokio::spawn(async move {
        let mut connection = client;
        let frame = connection.read_frame().await.unwrap();
        assert_eq!(frame.command, Command::PairingConfirm);
        connection
            .write_frame(&Frame::try_new(Command::PairingConfirm, 0, 0, Vec::new()).unwrap())
            .await
            .unwrap();
        let frame = connection.read_frame().await.unwrap();
        assert_eq!(frame.command, Command::PairingPersisted);
        note_path.exists()
    });

    manager.confirm().await.expect("local confirmation");
    let note_existed = observer.await.unwrap();
    assert!(
        note_existed,
        "the older peer must never be able to conclude success before the new side's record is on disk"
    );
}
