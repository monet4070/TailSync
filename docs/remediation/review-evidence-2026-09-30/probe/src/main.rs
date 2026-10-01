use std::{
    path::PathBuf,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::{Duration, Instant},
};
use tailsync_core::{
    crypto::Settings,
    db::HistoryDB,
    identity::DeviceIdentity,
    pairing::{
        PairingDirection, PairingManager, PairingPhase, PendingPairing, PendingTrustRecord,
        PendingTrustStore,
    },
    protocol::{Command, Frame},
    secure::{self, PeerIdentity},
};
use tokio::{
    io::{AsyncRead, AsyncWrite, DuplexStream, ReadBuf},
    sync::{Mutex, Notify},
};

struct DelayedClose {
    inner: DuplexStream,
    entered: Arc<Notify>,
}
impl AsyncRead for DelayedClose {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        b: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, b)
    }
}
impl AsyncWrite for DelayedClose {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        b: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, b)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        self.entered.notify_one();
        Poll::Pending
    }
}
fn identity(name: &str) -> PeerIdentity {
    PeerIdentity {
        hostname: name.into(),
        tailscale_ip: "100.64.0.1".into(),
        iroh_endpoint_id: None,
    }
}
async fn pairing(
    delay_close: bool,
    conflict: bool,
) -> (
    Arc<PairingManager>,
    Arc<Mutex<Settings>>,
    secure::SecureConnection,
    Arc<Notify>,
) {
    let settings = Arc::new(Mutex::new(Settings::default()));
    let server_identity = Arc::new(DeviceIdentity::generate_for_test());
    let client_identity = DeviceIdentity::generate_for_test();
    if conflict {
        settings
            .lock()
            .await
            .trust_peer(
                "client",
                &DeviceIdentity::generate_for_test().public_key_base64(),
                "lan",
                Some("192.168.1.3"),
            )
            .unwrap();
    }
    let manager = PairingManager::new(settings.clone(), server_identity.clone());
    manager.enable().await;
    let (client_io, server_io) = tokio::io::duplex(256 * 1024);
    let entered = Arc::new(Notify::new());
    let server_io: secure::BoxedSessionIo = if delay_close {
        Box::new(DelayedClose {
            inner: server_io,
            entered: entered.clone(),
        })
    } else {
        Box::new(server_io)
    };
    let window = manager.subscribe_window();
    let server = tokio::spawn(async move {
        secure::accept_with_pairing_window(server_io, &server_identity, identity("server"), window)
            .await
            .unwrap()
    });
    // The platform normally sends HandshakeReady after the acceptance future finishes.
    let ready = tokio::spawn(async move {
        let mut accepted = server.await.unwrap();
        secure::write_ready(&mut accepted.connection).await.unwrap();
        accepted
    });
    let mut client = secure::connect_pairing(client_io, &client_identity, identity("client"))
        .await
        .unwrap()
        .connection;
    let accepted = ready.await.unwrap();
    manager
        .install_session(PendingPairing {
            connection: accepted.connection,
            hostname: "client".into(),
            remote_public_key: accepted.remote_public_key,
            handshake_hash: accepted.handshake_hash,
            address: "192.168.1.2".into(),
            interface: "lan".into(),
            remote_invite: None,
            direction: PairingDirection::Inbound,
        })
        .await
        .unwrap();
    manager.confirm().await.unwrap();
    assert_eq!(
        client.read_frame().await.unwrap().command,
        Command::PairingConfirm
    );
    client
        .write_frame(&Frame::try_new(Command::PairingConfirm, 0, 0, vec![]).unwrap())
        .await
        .unwrap();
    assert_eq!(
        client.read_frame().await.unwrap().command,
        Command::PairingPersisted
    );
    (manager, settings, client, entered)
}
#[tokio::main]
async fn main() {
    let root = PathBuf::from(std::env::args().nth(1).unwrap());
    std::fs::create_dir_all(&root).unwrap();
    std::env::set_var("TAILSYNC_DATA_DIR", root.join("data"));
    std::env::set_var("TAILSYNC_V1_DATA_DIR", root.join("v1"));
    std::fs::create_dir_all(root.join("data")).unwrap();
    let (manager, settings, mut client, entered) = pairing(true, false).await;
    client
        .write_frame(&Frame::try_new(Command::PairingPersisted, 0, 0, vec![]).unwrap())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), entered.notified())
        .await
        .unwrap();
    assert_eq!(manager.status().await.phase, PairingPhase::Finalizing);
    let cancelled = manager.cancel().await;
    println!(
        "finalize_cancel phase={:?} trust={} disk_trust={}",
        cancelled.phase,
        settings
            .lock()
            .await
            .trusted_peer_keys
            .contains_key("client"),
        Settings::load()
            .unwrap()
            .trusted_peer_keys
            .contains_key("client")
    );
    drop(client);
    let (manager, settings, mut client, _) = pairing(false, false).await;
    settings.lock().await.forget_peer("client").unwrap();
    manager.forget_pending("client").await;
    println!(
        "after_forget pending={} trust={}",
        manager.pending_trust().await.len(),
        settings
            .lock()
            .await
            .trusted_peer_keys
            .contains_key("client")
    );
    client
        .write_frame(&Frame::try_new(Command::PairingPersisted, 0, 0, vec![]).unwrap())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while manager.status().await.phase != PairingPhase::Paired {
            tokio::task::yield_now().await
        }
    })
    .await
    .unwrap();
    println!(
        "late_ack_after_forget phase={:?} trust={}",
        manager.status().await.phase,
        settings
            .lock()
            .await
            .trusted_peer_keys
            .contains_key("client")
    );
    let (conflicted, conflict_settings, mut conflict_client, _) = pairing(false, true).await;
    let old_key = conflict_settings.lock().await.trusted_peer_keys["client"].clone();
    conflict_client
        .write_frame(&Frame::try_new(Command::PairingPersisted, 0, 0, vec![]).unwrap())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while conflicted.status().await.phase != PairingPhase::Waiting {
            tokio::task::yield_now().await
        }
    })
    .await
    .unwrap();
    println!(
        "known_key_conflict persisted_frame_sent=true phase={:?} old_pin_retained={} error={:?}",
        conflicted.status().await.phase,
        conflict_settings.lock().await.trusted_peer_keys["client"] == old_key,
        conflicted.status().await.error
    );
    let mut store = PendingTrustStore::load_from_path(&root.join("missing-parent/pending.json"));
    let result = store.upsert(PendingTrustRecord {
        hostname: "failed".into(),
        public_key: "x".into(),
        interface: "lan".into(),
        address: "127.0.0.1".into(),
        recorded_at: 1,
    });
    println!(
        "failed_note_write is_error={} memory_records={}",
        result.is_err(),
        store.records().len()
    );
    let dbroot = root.join("db");
    tailsync_core::db::configure_storage_dir(Some(&dbroot)).unwrap();
    let mut db = HistoryDB::new().unwrap();
    db.add_text("delete me", "self").unwrap();
    let id = db.get_all(None, None, 1, 0).unwrap()[0].id;
    let reader = rusqlite::Connection::open(dbroot.join("history-v2.db")).unwrap();
    reader
        .execute_batch("BEGIN; SELECT COUNT(*) FROM history;")
        .unwrap();
    let started = Instant::now();
    let result = db.delete(id);
    println!(
        "delete_with_live_reader ok={} elapsed_ms={}",
        result.is_ok(),
        started.elapsed().as_millis()
    );
    reader.execute_batch("ROLLBACK;").unwrap();
    let body = b"case alias live payload";
    let first = db.add_file("report.txt", body, "self").unwrap();
    let first_id = db.get_all(Some("file"), None, 1, 0).unwrap()[0].id;
    db.set_favorite(first_id, true).unwrap();
    let second = db.add_file("REPORT.TXT", body, "self").unwrap();
    let second_id = db.get_all(Some("file"), None, 1, 0).unwrap()[0].id;
    let same_file =
        std::fs::canonicalize(&first).unwrap() == std::fs::canonicalize(&second).unwrap();
    db.delete(second_id).unwrap();
    println!("delete_case_alias same_file={same_file} favorite_still_readable={} favorite_path_exists={}",db.get_data(first_id).is_ok(),first.exists());
    for ip in ["fd7a:115c:a1e0::1", "fe80::1", "169.254.1.1"] {
        println!(
            "local_lan_selection input={ip} output={:?}",
            tailsync_core::peer::directory::select_local_lan_ip([ip.parse().unwrap()])
        );
    }
}
