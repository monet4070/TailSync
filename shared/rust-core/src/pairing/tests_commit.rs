//! Real Noise pairing sessions; isolated receipt files; no native clipboard or
//! production Settings writes. Two-process durable Settings recovery is covered
//! separately by the acceptance probe.
use super::*;
use crate::secure::{self, CapabilitySwitches, PeerIdentity};

fn info(name: &str) -> PeerIdentity {
    PeerIdentity {
        hostname: name.into(),
        tailscale_ip: String::new(),
        iroh_endpoint_id: None,
    }
}
fn switches(proof: bool) -> CapabilitySwitches {
    CapabilitySwitches {
        pairing_commit_proof: proof,
        composite_clipboard: false,
        ..CapabilitySwitches::default()
    }
}
async fn connections(
    local: &Arc<DeviceIdentity>,
    remote: &DeviceIdentity,
    local_proof: bool,
    remote_proof: bool,
) -> (secure::AcceptedConnection, secure::AcceptedConnection) {
    connections_with_write_fault(
        local,
        remote,
        local_proof,
        remote_proof,
        Arc::new(std::sync::atomic::AtomicBool::new(false)),
        Arc::new(tokio::sync::Notify::new()),
    )
    .await
}

const PARENT_WRITE_FAILURE: &str =
    "fixture IO mentions Incompatible TailSync protocol: but remains a broken pipe";

struct PairingWriteFaultIo {
    inner: tokio::io::DuplexStream,
    fail: Arc<std::sync::atomic::AtomicBool>,
    released: Arc<tokio::sync::Notify>,
}

impl tokio::io::AsyncRead for PairingWriteFaultIo {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buffer: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        tokio::io::AsyncRead::poll_read(std::pin::Pin::new(&mut self.inner), cx, buffer)
    }
}

impl tokio::io::AsyncWrite for PairingWriteFaultIo {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        bytes: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        if self.fail.load(std::sync::atomic::Ordering::Acquire) {
            return std::task::Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                PARENT_WRITE_FAILURE,
            )));
        }
        tokio::io::AsyncWrite::poll_write(std::pin::Pin::new(&mut self.inner), cx, bytes)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        tokio::io::AsyncWrite::poll_flush(std::pin::Pin::new(&mut self.inner), cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        tokio::io::AsyncWrite::poll_shutdown(std::pin::Pin::new(&mut self.inner), cx)
    }
}

impl Drop for PairingWriteFaultIo {
    fn drop(&mut self) {
        self.released.notify_one();
    }
}

async fn connections_with_write_fault(
    local: &Arc<DeviceIdentity>,
    remote: &DeviceIdentity,
    local_proof: bool,
    remote_proof: bool,
    fail: Arc<std::sync::atomic::AtomicBool>,
    released: Arc<tokio::sync::Notify>,
) -> (secure::AcceptedConnection, secure::AcceptedConnection) {
    let (a, b) = tokio::io::duplex(256 * 1024);
    let b = PairingWriteFaultIo {
        inner: b,
        fail,
        released,
    };
    let local = local.clone();
    let task = tokio::spawn(async move {
        let (_window, enabled) = watch::channel(true);
        let mut accepted = secure::accept_with_pairing_window_and_capabilities(
            b,
            &local,
            info("server"),
            enabled,
            switches(local_proof),
        )
        .await
        .unwrap();
        secure::write_ready(&mut accepted.connection).await.unwrap();
        accepted
    });
    let client = secure::connect_pairing_with_capabilities(
        a,
        remote,
        info("client"),
        switches(remote_proof),
    )
    .await
    .unwrap();
    let server = task.await.unwrap();
    assert_eq!(client.purpose, secure::HandshakePurpose::Pairing);
    assert_eq!(server.purpose, secure::HandshakePurpose::Pairing);
    assert_eq!(
        client
            .connection
            .negotiated_capabilities()
            .pairing_commit_proof,
        local_proof && remote_proof
    );
    assert_eq!(
        server
            .connection
            .negotiated_capabilities()
            .pairing_commit_proof,
        local_proof && remote_proof
    );
    (client, server)
}

struct Fixture {
    root: std::path::PathBuf,
    path: std::path::PathBuf,
    settings: Arc<Mutex<Settings>>,
    local: Arc<DeviceIdentity>,
    remote: DeviceIdentity,
}
impl Fixture {
    fn new() -> Self {
        let root =
            crate::db::get_data_dir().join(format!("commit-proof-{:016x}", rand::random::<u64>()));
        std::fs::create_dir_all(&root).unwrap();
        Self {
            path: root.join("pending.json"),
            root,
            settings: Arc::new(Mutex::new(Settings::default())),
            local: Arc::new(DeviceIdentity::generate_for_test()),
            remote: DeviceIdentity::generate_for_test(),
        }
    }
    fn manager(&self) -> Arc<PairingManager> {
        PairingManager::with_pending_store_at(
            self.settings.clone(),
            self.local.clone(),
            self.path.clone(),
        )
    }
    async fn install(&self, manager: &Arc<PairingManager>) -> SecureConnection {
        let (client, server) = connections(&self.local, &self.remote, true, true).await;
        manager.enable().await;
        manager
            .install_session(PendingPairing {
                connection: server.connection,
                hostname: "client".into(),
                remote_public_key: server.remote_public_key,
                handshake_hash: server.handshake_hash,
                address: "127.0.0.1".into(),
                interface: "lan".into(),
                remote_invite: None,
                direction: PairingDirection::Inbound,
            })
            .await
            .unwrap();
        client.connection
    }
    async fn seed(&self, nonce: [u8; 16], phase: PendingTrustPhase, key: String) {
        PendingTrustStore::load_from_path(&self.path)
            .upsert(PendingTrustRecord {
                hostname: "client".into(),
                public_key: key,
                interface: "lan".into(),
                address: "127.0.0.1".into(),
                recorded_at: 1,
                phase,
                commit_nonce: Some(nonce),
            })
            .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
async fn read(client: &mut SecureConnection, command: Command) -> Vec<u8> {
    let frame = tokio::time::timeout(Duration::from_secs(2), client.read_frame())
        .await
        .unwrap()
        .unwrap_or_else(|error| panic!("expected {command:?}: {error:?}"));
    assert_eq!(frame.command, command);
    frame.payload
}
async fn send(client: &mut SecureConnection, command: Command, payload: Vec<u8>) {
    client
        .write_frame(&Frame::try_new(command, 0, 0, payload).unwrap())
        .await
        .unwrap();
}
async fn wait(manager: &Arc<PairingManager>, phase: PairingPhase) {
    let result = tokio::time::timeout(Duration::from_secs(2), async {
        while manager.status().await.phase != phase {
            tokio::task::yield_now().await;
        }
    })
    .await;
    assert!(
        result.is_ok(),
        "expected {phase:?}, got {:?}",
        manager.status().await
    );
}
async fn pending(manager: &Arc<PairingManager>, client: &mut SecureConnection) -> Vec<u8> {
    manager.confirm().await.unwrap();
    read(client, Command::PairingConfirm).await;
    send(client, Command::PairingConfirm, vec![]).await;
    let nonce = read(client, Command::PairingPersisted).await;
    assert_eq!(nonce.len(), 16);
    nonce
}
async fn bootstrap(manager: &Arc<PairingManager>, client: &mut SecureConnection) -> Vec<u8> {
    let nonce = pending(manager, client).await;
    send(client, Command::PairingPersisted, nonce.clone()).await;
    assert_eq!(read(client, Command::PairingCommitted).await, nonce);
    assert_eq!(manager.status().await.phase, PairingPhase::Finalizing);
    nonce
}
async fn complete(manager: &Arc<PairingManager>, client: &mut SecureConnection, nonce: Vec<u8>) {
    send(client, Command::PairingCommitted, nonce.clone()).await;
    assert_eq!(read(client, Command::PairingCommittedAck).await, nonce);
    send(client, Command::PairingCommittedAck, nonce).await;
    wait(manager, PairingPhase::Paired).await;
}

#[tokio::test]
async fn two_managers_commit_then_prove_before_paired() {
    let f = Fixture::new();
    let a = f.manager();
    let b_settings = Arc::new(Mutex::new(Settings::default()));
    let b = PairingManager::with_pending_store_at(
        b_settings.clone(),
        Arc::new(f.remote.clone()),
        f.root.join("client.json"),
    );
    let (client, server) = connections(&f.local, &f.remote, true, true).await;
    a.enable().await;
    b.enable().await;
    a.install_session(PendingPairing {
        connection: server.connection,
        hostname: "client".into(),
        remote_public_key: server.remote_public_key,
        handshake_hash: server.handshake_hash,
        address: "127.0.0.1".into(),
        interface: "lan".into(),
        remote_invite: None,
        direction: PairingDirection::Inbound,
    })
    .await
    .unwrap();
    b.install_session(PendingPairing {
        connection: client.connection,
        hostname: "server".into(),
        remote_public_key: client.remote_public_key,
        handshake_hash: client.handshake_hash,
        address: "127.0.0.1".into(),
        interface: "lan".into(),
        remote_invite: None,
        direction: PairingDirection::Outbound,
    })
    .await
    .unwrap();
    a.confirm().await.unwrap();
    b.confirm().await.unwrap();
    wait(&a, PairingPhase::Paired).await;
    wait(&b, PairingPhase::Paired).await;
    assert_eq!(f.settings.lock().await.trusted_peer_keys.len(), 1);
    assert_eq!(b_settings.lock().await.trusted_peer_keys.len(), 1);
    assert!(a.pending_trust().await.is_empty());
    assert_eq!(
        PendingTrustStore::load_from_path(&f.path)
            .get("client")
            .unwrap()
            .phase,
        PendingTrustPhase::Completed
    );
}

#[tokio::test]
async fn lost_persisted_is_recovered_only_by_matching_authenticated_commit() {
    let f = Fixture::new();
    let a = f.manager();
    let mut client = f.install(&a).await;
    let nonce = pending(&a, &mut client).await;
    drop(client);
    wait(&a, PairingPhase::Waiting).await;
    assert!(f.settings.lock().await.trusted_peer_keys.is_empty());
    drop(a);
    let a = f.manager();
    assert_eq!(a.status().await.phase, PairingPhase::Disabled);
    let mut client = f.install(&a).await;
    send(&mut client, Command::PairingCommitted, nonce.clone()).await;
    assert_eq!(read(&mut client, Command::PairingCommitted).await, nonce);
    assert_eq!(read(&mut client, Command::PairingCommittedAck).await, nonce);
    send(&mut client, Command::PairingCommittedAck, nonce).await;
    wait(&a, PairingPhase::Paired).await;
    assert_eq!(f.settings.lock().await.trusted_peer_keys.len(), 1);
}

#[tokio::test]
async fn lost_commit_and_lost_ack_survive_restart_with_original_nonce() {
    for send_remote_commit in [false, true] {
        let f = Fixture::new();
        let a = f.manager();
        let mut client = f.install(&a).await;
        let nonce = bootstrap(&a, &mut client).await;
        if send_remote_commit {
            send(&mut client, Command::PairingCommitted, nonce.clone()).await;
            read(&mut client, Command::PairingCommittedAck).await;
        }
        drop(client);
        wait(&a, PairingPhase::Waiting).await;
        drop(a);
        assert_eq!(
            PendingTrustStore::load_from_path(&f.path)
                .get("client")
                .unwrap()
                .phase,
            PendingTrustPhase::CommittedAwaitingAck
        );
        let a = f.manager();
        let mut client = f.install(&a).await;
        assert_eq!(read(&mut client, Command::PairingCommitted).await, nonce);
        complete(&a, &mut client, nonce).await;
        assert_eq!(f.settings.lock().await.trusted_peer_keys.len(), 1);
    }
}

#[tokio::test]
async fn completed_receipt_can_answer_peer_that_lost_last_ack() {
    let f = Fixture::new();
    let a = f.manager();
    let mut client = f.install(&a).await;
    let nonce = bootstrap(&a, &mut client).await;
    complete(&a, &mut client, nonce.clone()).await;
    drop(client);
    drop(a);
    let a = f.manager();
    assert!(a.status().await.pending.is_empty());
    let mut client = f.install(&a).await;
    assert_eq!(read(&mut client, Command::PairingCommitted).await, nonce);
    complete(&a, &mut client, nonce).await;
}

#[tokio::test]
async fn ack_alone_never_promotes_pending_or_completes_active() {
    let f = Fixture::new();
    let a = f.manager();
    let mut client = f.install(&a).await;
    let nonce = pending(&a, &mut client).await;
    send(&mut client, Command::PairingCommittedAck, nonce.clone()).await;
    assert!(
        tokio::time::timeout(Duration::from_millis(30), client.read_frame())
            .await
            .is_err()
    );
    assert!(f.settings.lock().await.trusted_peer_keys.is_empty());
    send(&mut client, Command::PairingPersisted, nonce.clone()).await;
    read(&mut client, Command::PairingCommitted).await;
    send(&mut client, Command::PairingCommittedAck, nonce).await;
    assert!(
        tokio::time::timeout(Duration::from_millis(30), client.read_frame())
            .await
            .is_err()
    );
    assert_eq!(a.status().await.phase, PairingPhase::Finalizing);
    a.cancel().await;
}

#[tokio::test]
async fn wrong_key_nonce_and_no_consent_cannot_recover_trust() {
    for invalid in ["key", "nonce", "missing"] {
        let f = Fixture::new();
        if invalid != "missing" {
            let key = if invalid == "key" {
                DeviceIdentity::generate_for_test().public_key_base64()
            } else {
                f.remote.public_key_base64()
            };
            f.seed([7; 16], PendingTrustPhase::Pending, key).await;
        }
        let a = f.manager();
        let mut client = f.install(&a).await;
        send(
            &mut client,
            Command::PairingCommitted,
            vec![if invalid == "nonce" { 8 } else { 7 }; 16],
        )
        .await;
        wait(&a, PairingPhase::Waiting).await;
        assert!(f.settings.lock().await.trusted_peer_keys.is_empty());
        assert_ne!(a.status().await.phase, PairingPhase::Paired);
    }
}

#[tokio::test]
async fn duplicate_proofs_and_reordered_ack_are_idempotent() {
    let f = Fixture::new();
    let a = f.manager();
    let mut client = f.install(&a).await;
    let nonce = bootstrap(&a, &mut client).await;
    send(&mut client, Command::PairingCommittedAck, nonce.clone()).await;
    // A matching ACK carries no trust; the real proof is still required.
    assert!(
        tokio::time::timeout(Duration::from_millis(30), client.read_frame())
            .await
            .is_err()
    );
    send(&mut client, Command::PairingCommitted, nonce.clone()).await;
    read(&mut client, Command::PairingCommittedAck).await;
    wait(&a, PairingPhase::Paired).await;
    assert_eq!(f.settings.lock().await.trusted_peer_keys.len(), 1);
    drop(client);
    let a = f.manager();
    let mut client = f.install(&a).await;
    read(&mut client, Command::PairingCommitted).await;
    for _ in 0..2 {
        send(&mut client, Command::PairingCommitted, nonce.clone()).await;
        read(&mut client, Command::PairingCommittedAck).await;
    }
    send(&mut client, Command::PairingCommittedAck, nonce).await;
    wait(&a, PairingPhase::Paired).await;
    assert_eq!(f.settings.lock().await.trusted_peer_keys.len(), 1);
}

#[tokio::test]
async fn cancelled_commit_keeps_receipt_but_revoked_or_disabled_trust_never_replays() {
    for revoke in [false, true] {
        let f = Fixture::new();
        let a = f.manager();
        let mut client = f.install(&a).await;
        let nonce = bootstrap(&a, &mut client).await;
        a.cancel().await;
        drop(client);
        assert_eq!(
            PendingTrustStore::load_from_path(&f.path)
                .get("client")
                .unwrap()
                .phase,
            PendingTrustPhase::CommittedAwaitingAck
        );
        if revoke {
            a.revoke_peer("client").await.unwrap();
        } else {
            f.settings
                .lock()
                .await
                .enabled_peers
                .insert("client".into(), false);
        }
        let a = f.manager();
        let mut client = f.install(&a).await;
        assert!(
            tokio::time::timeout(Duration::from_millis(30), client.read_frame())
                .await
                .is_err()
        );
        send(&mut client, Command::PairingCommitted, nonce).await;
        wait(&a, PairingPhase::Waiting).await;
        assert_ne!(a.status().await.phase, PairingPhase::Paired);
        if revoke {
            assert!(f.settings.lock().await.trusted_peer_keys.is_empty());
        } else {
            assert_eq!(
                f.settings.lock().await.enabled_peers.get("client"),
                Some(&false)
            );
        }
    }
}

#[tokio::test]
async fn corrupt_store_preserves_bytes_and_never_grants_trust() {
    let f = Fixture::new();
    std::fs::write(&f.path, b"{bad-json").unwrap();
    let a = f.manager();
    let mut client = f.install(&a).await;
    a.confirm().await.unwrap();
    read(&mut client, Command::PairingConfirm).await;
    send(&mut client, Command::PairingConfirm, vec![]).await;
    wait(&a, PairingPhase::Waiting).await;
    assert!(f.settings.lock().await.trusted_peer_keys.is_empty());
    assert!(a.status().await.pending_store_unavailable);
    assert_eq!(std::fs::read(&f.path).unwrap(), b"{bad-json");
}

#[tokio::test]
async fn either_direction_without_capability_keeps_empty_legacy_persisted() {
    for (local_proof, remote_proof) in [(true, false), (false, true), (false, false)] {
        let f = Fixture::new();
        let a = f.manager();
        let (client, server) = connections(&f.local, &f.remote, local_proof, remote_proof).await;
        a.enable().await;
        a.install_session(PendingPairing {
            connection: server.connection,
            hostname: "client".into(),
            remote_public_key: server.remote_public_key,
            handshake_hash: server.handshake_hash,
            address: "127.0.0.1".into(),
            interface: "lan".into(),
            remote_invite: None,
            direction: PairingDirection::Inbound,
        })
        .await
        .unwrap();
        let mut client = client.connection;
        a.confirm().await.unwrap();
        read(&mut client, Command::PairingConfirm).await;
        send(&mut client, Command::PairingConfirm, vec![]).await;
        assert!(read(&mut client, Command::PairingPersisted)
            .await
            .is_empty());
        send(&mut client, Command::PairingPersisted, vec![]).await;
        wait(&a, PairingPhase::Paired).await;
        assert!(a.pending_trust().await.is_empty());
        assert!(client.read_frame().await.is_err());
    }
}

#[tokio::test(start_paused = true)]
async fn timeout_after_active_commit_preserves_replay_receipt() {
    let f = Fixture::new();
    let a = f.manager();
    let mut client = f.install(&a).await;
    bootstrap(&a, &mut client).await;
    tokio::time::advance(Duration::from_secs(31)).await;
    wait(&a, PairingPhase::Waiting).await;
    assert_eq!(
        PendingTrustStore::load_from_path(&f.path)
            .get("client")
            .unwrap()
            .phase,
        PendingTrustPhase::CommittedAwaitingAck
    );
    assert_eq!(f.settings.lock().await.trusted_peer_keys.len(), 1);
}

#[tokio::test]
async fn commit_check_and_write_cannot_be_interleaved_by_cancel() {
    let f = Fixture::new();
    let a = f.manager();
    let mut client = f.install(&a).await;
    let nonce = pending(&a, &mut client).await;
    let gate = Arc::new(PromotionGate {
        entered: tokio::sync::Notify::new(),
        release: tokio::sync::Notify::new(),
    });
    a.install_promotion_gate(gate.clone()).await;
    send(&mut client, Command::PairingPersisted, nonce).await;
    gate.entered.notified().await;
    let cancel_owner = a.clone();
    let mut cancel = tokio::spawn(async move { cancel_owner.cancel().await });
    assert!(tokio::time::timeout(Duration::from_millis(30), &mut cancel)
        .await
        .is_err());
    gate.release.notify_one();
    assert_eq!(cancel.await.unwrap().phase, PairingPhase::Cancelled);
    assert_eq!(f.settings.lock().await.trusted_peer_keys.len(), 1);
    assert_eq!(
        PendingTrustStore::load_from_path(&f.path)
            .get("client")
            .unwrap()
            .phase,
        PendingTrustPhase::CommittedAwaitingAck
    );
}

#[tokio::test]
async fn unwritable_intent_after_confirmation_never_grants_active_trust() {
    let f = Fixture::new();
    let a = f.manager();
    let mut client = f.install(&a).await;
    let nonce = pending(&a, &mut client).await;
    let backup = f.root.join("receipt-backup");
    std::fs::rename(&f.path, &backup).unwrap();
    std::fs::create_dir(&f.path).unwrap();
    send(&mut client, Command::PairingPersisted, nonce.clone()).await;
    wait(&a, PairingPhase::Waiting).await;
    assert!(f.settings.lock().await.trusted_peer_keys.is_empty());
    assert!(a.status().await.pending_store_unavailable);
    std::fs::remove_dir(&f.path).unwrap();
    std::fs::rename(backup, &f.path).unwrap();
    drop(client);
    // Recover storage without rebuilding the manager: only the original
    // durable consent plus a new authenticated proof may grant trust.
    let mut client = f.install(&a).await;
    send(&mut client, Command::PairingCommitted, nonce.clone()).await;
    assert_eq!(read(&mut client, Command::PairingCommitted).await, nonce);
    read(&mut client, Command::PairingCommittedAck).await;
    send(&mut client, Command::PairingCommittedAck, nonce).await;
    wait(&a, PairingPhase::Paired).await;
}
#[tokio::test]
async fn failed_receipt_cleanup_cannot_resurrect_forgotten_pending_trust() {
    for phase in [
        PendingTrustPhase::Pending,
        PendingTrustPhase::CommittedAwaitingAck,
        PendingTrustPhase::Completed,
    ] {
        let f = Fixture::new();
        f.seed([9; 16], phase, f.remote.public_key_base64()).await;
        f.settings
            .lock()
            .await
            .trust_peer_without_save(
                "client",
                &f.remote.public_key_base64(),
                "lan",
                Some("127.0.0.1"),
            )
            .unwrap();
        let a = f.manager();
        let backup = f.root.join("retained-receipt");
        std::fs::rename(&f.path, &backup).unwrap();
        std::fs::create_dir(&f.path).unwrap();
        assert!(a.revoke_peer("client").await.is_err());
        assert_eq!(f.settings.lock().await.trusted_peer_keys.len(), 1);
        assert_eq!(
            f.settings.lock().await.enabled_peers.get("client"),
            Some(&false)
        );
        // The old durable receipt becomes readable again after storage repair.
        std::fs::remove_dir(&f.path).unwrap();
        std::fs::rename(backup, &f.path).unwrap();
        let a = f.manager();
        let mut client = f.install(&a).await;
        assert!(
            tokio::time::timeout(Duration::from_millis(30), client.read_frame())
                .await
                .is_err()
        );
        send(&mut client, Command::PairingCommitted, vec![9; 16]).await;
        wait(&a, PairingPhase::Waiting).await;
        assert_eq!(
            f.settings.lock().await.enabled_peers.get("client"),
            Some(&false)
        );
        // Forget may report success only after consent invalidation succeeds.
        a.revoke_peer("client").await.unwrap();
        assert!(f.settings.lock().await.trusted_peer_keys.is_empty());
        assert!(PendingTrustStore::load_from_path(&f.path)
            .get("client")
            .is_none());
    }
}

/// F10-B: the commit session's transport failures must keep their structural
/// kind instead of being flattened into `Transport(String)`.
#[tokio::test]
async fn a_commit_session_connection_loss_keeps_its_structural_kind() {
    let f = Fixture::new();
    let a = f.manager();
    let client = f.install(&a).await;

    // The peer disappears before the proof exchange completes.
    drop(client);
    let status = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let status = a.status().await;
            if status.failure_kind.is_some() {
                return status;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the commit session never recorded a classified failure");

    assert_eq!(
        status.failure_kind,
        Some(PairingFailureKind::ConnectionClosed)
    );
    assert_eq!(
        status.error.as_deref(),
        Some("IO error: early eof while reading encrypted record length"),
        "the pre-typing text must survive verbatim"
    );
    assert!(status.pairing_enabled);
    assert_eq!(status.failed_attempts, 0);
    assert_eq!(status.phase, PairingPhase::Waiting);
}

#[tokio::test]
async fn a_completed_commit_promotion_leaves_no_classification() {
    let f = Fixture::new();
    let a = f.manager();
    let mut client = f.install(&a).await;
    let nonce = bootstrap(&a, &mut client).await;
    complete(&a, &mut client, nonce).await;

    let status = a.status().await;
    assert_eq!(status.phase, PairingPhase::Paired);
    assert_eq!(status.error, None);
    assert_eq!(status.failure_kind, None);
}

async fn parent_wait_for_pairing_failure(manager: &Arc<PairingManager>) -> PairingStatus {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let status = manager.status().await;
            if status.failure_kind.is_some() {
                return status;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the real session did not report its failure")
}

#[tokio::test]
async fn parent_real_write_failure_keeps_kind_text_and_releases_both_session_variants() {
    for proof in [false, true] {
        let f = Fixture::new();
        let manager = f.manager();
        let fail = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let released = Arc::new(tokio::sync::Notify::new());
        let (client, server) = connections_with_write_fault(
            &f.local,
            &f.remote,
            proof,
            proof,
            fail.clone(),
            released.clone(),
        )
        .await;
        manager.enable().await;
        manager
            .install_session(PendingPairing {
                connection: server.connection,
                hostname: "client".into(),
                remote_public_key: server.remote_public_key,
                handshake_hash: server.handshake_hash,
                address: "127.0.0.1".into(),
                interface: "lan".into(),
                remote_invite: None,
                direction: PairingDirection::Inbound,
            })
            .await
            .unwrap();
        // The peer stays connected and the Noise handshake already succeeded.
        // Only the server write fails, so read-side EOF cannot satisfy this test.
        fail.store(true, std::sync::atomic::Ordering::Release);
        manager.confirm().await.unwrap();
        let status = parent_wait_for_pairing_failure(&manager).await;
        assert_eq!(
            status.failure_kind,
            Some(PairingFailureKind::ConnectionClosed)
        );
        let expected = if proof {
            PARENT_WRITE_FAILURE.to_string()
        } else {
            format!("Could not confirm pairing: {PARENT_WRITE_FAILURE}")
        };
        assert_eq!(status.error.as_deref(), Some(expected.as_str()));
        assert_eq!(status.phase, PairingPhase::Waiting);
        assert!(status.pairing_enabled);
        assert_eq!(status.failed_attempts, 0);
        assert!(manager.pending_trust().await.is_empty());
        tokio::time::timeout(Duration::from_secs(1), released.notified())
            .await
            .expect("failed session retained its transport");
        drop(client);
    }
}

#[tokio::test]
async fn parent_real_commit_peer_error_uses_only_the_fixed_wire_categories() {
    let f = Fixture::new();
    let manager = f.manager();
    for (message, kind) in [
        (
            "Incompatible TailSync protocol: rewritten peer display",
            PairingFailureKind::ProtocolIncompatible,
        ),
        (
            "Pairing window was closed",
            PairingFailureKind::WindowClosed,
        ),
        (
            "fixture path/Incompatible TailSync protocol: unrelated marker",
            PairingFailureKind::Unknown,
        ),
        (
            "example says Pairing window was closed",
            PairingFailureKind::Unknown,
        ),
    ] {
        let mut client = f.install(&manager).await;
        send(&mut client, Command::PeerError, message.as_bytes().to_vec()).await;
        let status = parent_wait_for_pairing_failure(&manager).await;
        assert_eq!(status.failure_kind, Some(kind));
        assert_eq!(status.error.as_deref(), Some(message));
        assert!(status.pairing_enabled);
        assert_eq!(status.failed_attempts, 0);
    }
}

#[tokio::test(start_paused = true)]
async fn parent_real_commit_session_deadline_keeps_the_window_open_and_records_timeout() {
    let f = Fixture::new();
    let manager = f.manager();
    let _client = f.install(&manager).await;
    tokio::task::yield_now().await;
    tokio::time::advance(PAIRING_SESSION_TIMEOUT + Duration::from_secs(1)).await;
    let status = parent_wait_for_pairing_failure(&manager).await;
    assert_eq!(status.failure_kind, Some(PairingFailureKind::TimedOut));
    assert_eq!(status.error.as_deref(), Some("Pairing session timed out"));
    assert_eq!(status.phase, PairingPhase::Waiting);
    assert!(status.pairing_enabled);
    assert_eq!(status.failed_attempts, 0);
    assert!(manager.pending_trust().await.is_empty());
}
