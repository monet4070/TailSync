//! L4 simulation only: independent processes, identities, settings, SQLite,
//! loopback TCP/Noise and a filesystem-backed minimal clipboard boundary.
#![cfg(feature = "test-support")]
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command as Process,
    sync::Arc,
    time::{Duration, Instant},
};
use tailsync_core::{
    crypto::Settings,
    db::{self, HistoryDB},
    identity::DeviceIdentity,
    peer::event_receiver::process_reliable_event,
    protocol::{self, Command, EventEnvelope, Frame, TransferId},
    secure::{self, PeerIdentity, SecureConnection},
    sync::{FileBatchProgress, FileReceiveCommit, PlatformResultFuture, SyncEngine, SyncPlatform},
};
use tokio::sync::Mutex;
struct Platform(PathBuf);
impl Platform {
    fn write(&self, bytes: &[u8]) -> Result<(), String> {
        fs::write(self.0.join("clipboard"), bytes).map_err(|e| e.to_string())?;
        use std::io::Write;
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.0.join("effects"))
            .map_err(|e| e.to_string())?
            .write_all(b"effect\n")
            .map_err(|e| e.to_string())
    }
}
impl SyncPlatform for Platform {
    fn write_text(&self, s: &str) -> Result<(), String> {
        self.write(s.as_bytes())
    }
    fn write_image(&self, w: u32, h: u32, rgba: &[u8]) -> Result<(), String> {
        self.write(&protocol::pack_rgba_image(w, h, rgba).unwrap())
    }
    fn set_file_progress(&self, _: &str, _: u64, _: u64) {}
    fn clear_file_progress(&self, _: Option<TransferId>, _: Option<&str>) {}
    fn set_file_batch_progress(&self, _: FileBatchProgress) {}
    fn files_received(&self, _: FileReceiveCommit) -> PlatformResultFuture {
        Box::pin(async { Ok(()) })
    }
    fn file_batch_failed(&self, _: Option<TransferId>, _: &str) {}
}
fn info(name: &str) -> PeerIdentity {
    PeerIdentity {
        hostname: name.into(),
        tailscale_ip: String::new(),
        iroh_endpoint_id: None,
    }
}
fn wait(path: &Path) {
    let start = Instant::now();
    while !path.exists() {
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "missing {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
async fn receive(
    stream: &mut SecureConnection,
    engine: &Arc<Mutex<SyncEngine>>,
    database: &Arc<Mutex<HistoryDB>>,
    seq: &mut Option<u32>,
) {
    let frame = stream.read_frame().await.unwrap();
    process_reliable_event(
        stream,
        &frame,
        if stream.peer_identity().hostname == "A" {
            "A"
        } else {
            "B"
        },
        engine,
        database,
        seq,
        || (),
    )
    .await
    .unwrap();
}
async fn send(
    stream: &mut SecureConnection,
    command: Command,
    sequence: u32,
    event: EventEnvelope,
) {
    let frame = Frame::try_new(command, 0, sequence, event.encode()).unwrap();
    stream.write_frame(&frame).await.unwrap();
    let ack = stream.read_frame().await.unwrap();
    assert_eq!(ack.command, Command::EventAck);
    assert_eq!(ack.payload, event.message_id.ack_payload());
}
#[tokio::test]
#[ignore]
async fn process_worker() {
    let own = db::get_data_dir();
    db::configure_storage_dir(Some(&own)).unwrap();
    let shared = PathBuf::from(std::env::var_os("P35_SHARED").unwrap());
    let role = std::env::var("P35_ROLE").unwrap();
    let round = std::env::var("P35_ROUND").unwrap();
    let first = round == "first";
    let identity = DeviceIdentity::load_or_create().unwrap();

    let settings = if first {
        let s = Settings {
            sync_enabled: role == "a",
            ..Settings::default()
        };
        s.save().unwrap();
        s
    } else {
        Settings::load().unwrap()
    };
    assert_eq!(settings.sync_enabled, role == "a");
    fs::write(own.join("public"), identity.public_key()).unwrap();
    let platform = Arc::new(Platform(own.clone()));
    let mut sync = SyncEngine::new();
    sync.set_platform(platform.clone());
    let engine = Arc::new(Mutex::new(sync));
    let database = Arc::new(Mutex::new(HistoryDB::new().unwrap()));
    if role == "b" {
        if !first {
            platform.write_text("newer local B").unwrap();
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        fs::write(
            shared.join(format!("port-{round}")),
            listener.local_addr().unwrap().port().to_string(),
        )
        .unwrap();
        let (io, _) = listener.accept().await.unwrap();
        let accepted = secure::accept(io, &identity, info("B")).await.unwrap();
        assert_eq!(
            accepted.remote_public_key,
            fs::read(shared.join("a/public")).unwrap()
        );
        let mut stream = accepted.connection;
        secure::write_ready(&mut stream).await.unwrap();
        let mut seq = None;
        receive(&mut stream, &engine, &database, &mut seq).await;
        if !first {
            assert_eq!(fs::read(own.join("clipboard")).unwrap(), b"newer local B");
        }
        receive(&mut stream, &engine, &database, &mut seq).await;
        send(
            &mut stream,
            Command::TextPayload,
            10,
            EventEnvelope::new(b"B to A text".to_vec()),
        )
        .await;
        send(
            &mut stream,
            Command::ImagePayload,
            11,
            EventEnvelope::new(protocol::pack_rgba_image(1, 1, &[9; 4]).unwrap()),
        )
        .await;
        // Fresh TCP/Noise connection with the same durable identities.
        drop(stream);
        let (io, _) = listener.accept().await.unwrap();
        let accepted = secure::accept(io, &identity, info("B")).await.unwrap();
        let mut stream = accepted.connection;
        secure::write_ready(&mut stream).await.unwrap();
        let mut seq = None;
        receive(&mut stream, &engine, &database, &mut seq).await;
        assert_eq!(
            fs::read(own.join("clipboard")).unwrap(),
            b"old A",
            "new event ID with same content must acquire freshness"
        );
        assert_eq!(
            database
                .lock()
                .await
                .get_all(None, None, 100, 0)
                .unwrap()
                .len(),
            2
        );
    } else {
        let port = fs::read_to_string(shared.join(format!("port-{round}"))).unwrap();
        let key = fs::read(shared.join("b/public")).unwrap();
        let address = format!("127.0.0.1:{port}");
        let mut stream = secure::connect(
            tokio::net::TcpStream::connect(&address).await.unwrap(),
            &identity,
            info("A"),
            "B",
            &key,
        )
        .await
        .unwrap();
        let event_path = own.join("old-event");
        let event = if first {
            let e = EventEnvelope::new(b"old A".to_vec());
            fs::write(&event_path, e.encode()).unwrap();
            e
        } else {
            EventEnvelope::decode(&fs::read(&event_path).unwrap()).unwrap()
        };
        send(&mut stream, Command::TextPayload, 1, event).await;
        send(
            &mut stream,
            Command::ImagePayload,
            2,
            EventEnvelope::new(protocol::pack_rgba_image(1, 1, &[7; 4]).unwrap()),
        )
        .await;
        let mut seq = None;
        receive(&mut stream, &engine, &database, &mut seq).await;
        receive(&mut stream, &engine, &database, &mut seq).await;
        drop(stream);
        let mut stream = secure::connect(
            tokio::net::TcpStream::connect(address).await.unwrap(),
            &identity,
            info("A"),
            "B",
            &key,
        )
        .await
        .unwrap();
        send(
            &mut stream,
            Command::TextPayload,
            1,
            EventEnvelope::new(b"old A".to_vec()),
        )
        .await;
        assert_eq!(
            database
                .lock()
                .await
                .get_all(None, None, 100, 0)
                .unwrap()
                .len(),
            2
        );
    }
    fs::write(own.join(format!("done-{round}")), identity.fingerprint()).unwrap();
}
#[test]
fn two_process_text_image_reconnect_and_receiver_sender_restart() {
    let root = db::get_data_dir().join(format!("two-process-{:016x}", rand::random::<u64>()));
    fs::create_dir_all(&root).unwrap();
    for round in ["first", "restart"] {
        let make = |role: &str| {
            let mut c = Process::new(std::env::current_exe().unwrap());
            c.args(["process_worker", "--exact", "--ignored", "--nocapture"])
                .env("P35_ROLE", role)
                .env("P35_ROUND", round)
                .env("P35_SHARED", &root)
                .env("TAILSYNC_DATA_DIR", root.join(role))
                .env("TAILSYNC_STORAGE_DIR", root.join(role))
                .env("TAILSYNC_V1_DATA_DIR", root.join(role).join("legacy"));
            c
        };
        let mut b = make("b").spawn().unwrap();
        wait(&root.join(format!("port-{round}")));
        assert!(make("a").status().unwrap().success());
        assert!(b.wait().unwrap().success());
    }
    for role in ["a", "b"] {
        assert_eq!(
            fs::read(root.join(role).join("done-first")).unwrap(),
            fs::read(root.join(role).join("done-restart")).unwrap()
        );
    }
    assert_ne!(
        fs::read(root.join("a/public")).unwrap(),
        fs::read(root.join("b/public")).unwrap()
    );
    println!("L4 simulated two-device PASS: two independent processes/identities/settings/SQLite; real TCP/Noise; text+image both ways; reconnect; both processes restart; old event ACK preserves newer local B; identical content with new event ID applies. Native clipboard/L5 not exercised.");
}
