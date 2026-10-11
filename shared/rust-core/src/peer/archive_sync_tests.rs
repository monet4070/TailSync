use super::archive_receiver;
use super::delivery::{
    deliver_pending_frame, DeliveryConfig, DeliveryConnection, PendingFrame, SharedEvent,
};
use crate::composite::archive_sync::{ArchiveResourceUpdate, ArchiveUpdate};
use crate::composite::budgets::BudgetLedger;
use crate::composite::chunks::{self, CompositeAssembler};
use crate::composite::network_image::{fixtures, NetworkImageFormat};
use crate::composite::{packed_to_png, Completeness, Mime, PortableBundle, Representation};
use crate::db::HistoryDB;
use crate::protocol::composite::{AckResult, CompositeAck};
use crate::protocol::{unix_timestamp_ms, Command, EventEnvelope, Frame, MessageId};
use crate::secure::CapabilitySet;
use base64::Engine as _;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};

struct RevokingConnection {
    permission: Arc<AtomicBool>,
    frames: Vec<Frame>,
}

impl DeliveryConnection for RevokingConnection {
    async fn write_frame(&mut self, frame: &Frame) -> Result<(), String> {
        self.frames.push(
            Frame::decode(&frame.encode())
                .map_err(|error| error.to_string())?
                .0,
        );
        Ok(())
    }
    async fn read_frame(&mut self) -> Result<Frame, String> {
        let frame = self.frames.last().unwrap();
        let metadata = chunks::inspect_archive(&frame.payload).unwrap();
        self.permission.store(false, Ordering::SeqCst);
        Frame::try_new(
            Command::ArchiveUpdateAck,
            0,
            frame.sequence,
            CompositeAck {
                message_id: metadata.message_id,
                digest: metadata.digest,
                chunk_index: metadata.index,
                result: AckResult::Chunk,
            }
            .encode(),
        )
        .map_err(|error| error.to_string())
    }
    fn negotiated_capabilities(&self) -> CapabilitySet {
        CapabilitySet {
            composite_clipboard: true,
            archive_resource_updates: true,
            ..CapabilitySet::disabled()
        }
    }
}

#[tokio::test]
async fn archive_authorization_is_rechecked_before_sending_and_between_chunks() {
    let base = base();
    let mut update = update(&base, MessageId::random());
    let mut pixels = vec![0; 600 * 600 * 4];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut pixels);
    update.images.truncate(1);
    update.images[0].bytes = packed_to_png(600, 600, &pixels).unwrap();
    update.images[0].format = NetworkImageFormat::Png;
    let event =
        SharedEvent::encode(Command::ArchiveUpdatePayload, update.encode().unwrap()).unwrap();
    let permission = Arc::new(AtomicBool::new(false));
    let authorized = permission.clone();
    let pending = PendingFrame::new(
        event
            .queued()
            .with_archive_authorization(Arc::new(move || authorized.load(Ordering::SeqCst)))
            .unwrap(),
        1,
    );
    let mut connection = RevokingConnection {
        permission: permission.clone(),
        frames: vec![],
    };
    let error = deliver_pending_frame(&mut connection, &pending, &DeliveryConfig::default())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("authorization revoked"));
    assert!(connection.frames.is_empty());
    permission.store(true, Ordering::SeqCst);
    let error = deliver_pending_frame(&mut connection, &pending, &DeliveryConfig::default())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("authorization revoked"));
    assert_eq!(connection.frames.len(), 1);
    assert!(
        chunks::inspect_archive(&connection.frames[0].payload)
            .unwrap()
            .count
            > 1
    );
}
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;

fn base() -> PortableBundle {
    PortableBundle {
        representations: vec![Representation {
            mime: Mime::Html,
            bytes: b"<b>saved post</b><img src='https://fixture.example/0'><img src='https://fixture.example/1'><img src='https://fixture.example/2'>".to_vec(),
        }],
        resources: vec![],
        bindings: vec![],
        completeness: Completeness::ExternalReferences,
    }
}

fn update(base: &PortableBundle, event: MessageId) -> ArchiveUpdate {
    let decode = |encoded| {
        base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap()
    };
    ArchiveUpdate {
        base_event: event,
        base_digest: base.digest().unwrap(),
        revision: 2,
        images: vec![
            ArchiveResourceUpdate {
                image_index: 0,
                format: NetworkImageFormat::Png,
                bytes: packed_to_png(1, 1, &[1, 2, 3, 255]).unwrap(),
            },
            ArchiveResourceUpdate {
                image_index: 1,
                format: NetworkImageFormat::Jpeg,
                bytes: decode(fixtures::JPEG_1X1),
            },
            ArchiveResourceUpdate {
                image_index: 2,
                format: NetworkImageFormat::WebP,
                bytes: decode(fixtures::WEBP_LOSSLESS_1X1),
            },
        ],
    }
}

fn store(
    base: &PortableBundle,
    event: MessageId,
) -> (std::path::PathBuf, i64, Arc<Mutex<HistoryDB>>) {
    let root = std::env::temp_dir().join(format!(
        "tailsync-parent-archive-sync-{}-{}",
        std::process::id(),
        MessageId::random().as_hex()
    ));
    let mut database = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
    let id = database
        .add_composite_event(
            base,
            None,
            "sender",
            Some(("authenticated-sender", event, unix_timestamp_ms())),
        )
        .unwrap();
    (root, id, Arc::new(Mutex::new(database)))
}

struct MemoryConnection {
    stream: tokio::io::DuplexStream,
    capabilities: CapabilitySet,
    writes: Arc<AtomicUsize>,
}

impl DeliveryConnection for MemoryConnection {
    async fn write_frame(&mut self, frame: &Frame) -> Result<(), String> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        let bytes = frame.encode();
        self.stream
            .write_all(&(bytes.len() as u32).to_be_bytes())
            .await
            .map_err(|error| error.to_string())?;
        self.stream
            .write_all(&bytes)
            .await
            .map_err(|error| error.to_string())
    }

    async fn read_frame(&mut self) -> Result<Frame, String> {
        let mut length = [0; 4];
        self.stream
            .read_exact(&mut length)
            .await
            .map_err(|error| error.to_string())?;
        let mut bytes = vec![0; u32::from_be_bytes(length) as usize];
        self.stream
            .read_exact(&mut bytes)
            .await
            .map_err(|error| error.to_string())?;
        Frame::decode(&bytes)
            .map(|(frame, _)| frame)
            .map_err(|error| error.to_string())
    }

    fn negotiated_capabilities(&self) -> CapabilitySet {
        self.capabilities
    }
}

#[tokio::test]
async fn archive_delivery_populates_the_original_remote_row_without_native_publication() {
    let base = base();
    let event = MessageId::random();
    let (root, id, database) = store(&base, event);
    let identity = database
        .lock()
        .await
        .composite_identity(id)
        .unwrap()
        .unwrap();
    let update = update(&base, event);
    let notices = Arc::new(AtomicUsize::new(0));
    let encoded = update.encode().unwrap();
    let shared = SharedEvent::encode(Command::ArchiveUpdatePayload, encoded.clone()).unwrap();
    let pending = PendingFrame::new(shared.queued(), 40);
    let (client_stream, server_stream) = tokio::io::duplex(64 * 1024);
    let capabilities = CapabilitySet {
        composite_clipboard: true,
        archive_resource_updates: true,
        ..CapabilitySet::disabled()
    };
    let mut client = MemoryConnection {
        stream: client_stream,
        capabilities,
        writes: Arc::new(AtomicUsize::new(0)),
    };
    let server_database = database.clone();
    let server_notices = notices.clone();
    let server = tokio::spawn(async move {
        let mut connection = MemoryConnection {
            stream: server_stream,
            capabilities,
            writes: Arc::new(AtomicUsize::new(0)),
        };
        let mut assembler =
            CompositeAssembler::archive_with_ledger([41; 32], Arc::new(BudgetLedger::default()));
        loop {
            let frame = connection.read_frame().await.unwrap();
            assert_eq!(frame.command, Command::ArchiveUpdateChunk);
            let accepted = assembler.accept(&frame.payload, 0).unwrap();
            let _budget = accepted.budget;
            let terminal = accepted.complete.is_some();
            let ack = if let Some((envelope, _, _)) = accepted.complete {
                let notices = server_notices.clone();
                archive_receiver::receive(
                    envelope,
                    "authenticated-sender".into(),
                    [41; 32],
                    accepted.metadata.index,
                    server_database.clone(),
                    Arc::new(|| true),
                    Arc::new(move || {
                        notices.fetch_add(1, Ordering::SeqCst);
                    }),
                )
                .await
                .unwrap()
            } else {
                CompositeAck {
                    message_id: accepted.metadata.message_id,
                    digest: accepted.metadata.digest,
                    chunk_index: accepted.metadata.index,
                    result: AckResult::Chunk,
                }
            };
            connection
                .write_frame(
                    &Frame::try_new(Command::ArchiveUpdateAck, 0, frame.sequence, ack.encode())
                        .unwrap(),
                )
                .await
                .unwrap();
            if terminal {
                break;
            }
        }
    });
    deliver_pending_frame(&mut client, &pending, &DeliveryConfig::default())
        .await
        .unwrap();
    server.await.unwrap();
    let after = database
        .lock()
        .await
        .composite_identity(id)
        .unwrap()
        .unwrap();
    assert_eq!(after.capture_id, identity.capture_id);
    assert_eq!(after.content_id, identity.content_id);
    assert_eq!(after.dedupe_key, identity.dedupe_key);
    assert_eq!(after.archive_revision, identity.archive_revision + 1);
    assert_eq!(database.lock().await.get_composite(id).unwrap(), base);
    let originals = database
        .lock()
        .await
        .get_composite_network_originals(id)
        .unwrap();
    assert_eq!(originals.len(), update.images.len());
    for (original, image) in originals.iter().zip(&update.images) {
        assert_eq!(original.bytes, image.bytes);
        assert_eq!(original.format, image.format);
        assert_eq!(original.image_index, image.image_index);
    }
    let notification = notices.clone();
    let ack = archive_receiver::receive(
        EventEnvelope::new(encoded),
        "authenticated-sender".into(),
        [41; 32],
        0,
        database.clone(),
        Arc::new(|| true),
        Arc::new(move || {
            notification.fetch_add(1, Ordering::SeqCst);
        }),
    )
    .await
    .unwrap();
    assert_eq!(ack.result, AckResult::Applied);
    assert_eq!(
        database
            .lock()
            .await
            .composite_identity(id)
            .unwrap()
            .unwrap(),
        after
    );
    assert_eq!(notices.load(Ordering::SeqCst), 1);
    drop(database);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn archive_delivery_without_capability_has_no_legacy_or_native_fallback() {
    let base = base();
    let update = update(&base, MessageId::random());
    let event =
        SharedEvent::encode(Command::ArchiveUpdatePayload, update.encode().unwrap()).unwrap();
    let pending = PendingFrame::new(event.queued(), 4);
    let (stream, _other) = tokio::io::duplex(64);
    let writes = Arc::new(AtomicUsize::new(0));
    let mut connection = MemoryConnection {
        stream,
        capabilities: CapabilitySet {
            composite_clipboard: true,
            ..CapabilitySet::disabled()
        },
        writes: writes.clone(),
    };
    let error = deliver_pending_frame(&mut connection, &pending, &DeliveryConfig::default())
        .await
        .unwrap_err();
    assert!(!error.is_retryable());
    assert_eq!(writes.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn archive_receiver_rejects_wrong_source_and_revocation_and_never_resurrects_deletion() {
    let base = base();
    let event = MessageId::random();
    let (root, id, database) = store(&base, event);
    let update = update(&base, event);
    let identity = database
        .lock()
        .await
        .composite_identity(id)
        .unwrap()
        .unwrap();
    let notification = Arc::new(|| panic!("uncommitted update must not notify"));
    let mismatch = archive_receiver::receive(
        EventEnvelope::new(update.encode().unwrap()),
        "different-authenticated-sender".into(),
        [9; 32],
        0,
        database.clone(),
        Arc::new(|| true),
        notification.clone(),
    )
    .await
    .unwrap_err();
    assert!(mismatch.contains(crate::protocol::TEMPORARY_PRESSURE_MARKER));
    let revoked = archive_receiver::receive(
        EventEnvelope::new(update.encode().unwrap()),
        "authenticated-sender".into(),
        [9; 32],
        0,
        database.clone(),
        Arc::new(|| false),
        notification.clone(),
    )
    .await
    .unwrap_err();
    assert!(revoked.contains("authorization revoked"));
    assert_eq!(
        database
            .lock()
            .await
            .composite_identity(id)
            .unwrap()
            .unwrap(),
        identity
    );
    database.lock().await.delete(id).unwrap();
    let ack = archive_receiver::receive(
        EventEnvelope::new(update.encode().unwrap()),
        "authenticated-sender".into(),
        [9; 32],
        0,
        database.clone(),
        Arc::new(|| true),
        notification,
    )
    .await
    .unwrap();
    assert_eq!(ack.result, AckResult::Superseded);
    assert!(database.lock().await.get_type(id).is_err());
    drop(database);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn archive_chunks_cannot_cross_clipboard_kind_or_bypass_per_identity_admission() {
    use rand::RngCore;
    let base = base();
    let mut update = update(&base, MessageId::random());
    let mut pixels = vec![0; 600 * 600 * 4];
    rand::thread_rng().fill_bytes(&mut pixels);
    update.images.truncate(1);
    update.images[0].bytes = packed_to_png(600, 600, &pixels).unwrap();
    let archive = EventEnvelope::new(update.encode().unwrap());
    let archive_chunks = chunks::encode_archive(&archive).unwrap();
    assert!(archive_chunks.len() > 1);
    let clipboard_chunks = chunks::encode(&EventEnvelope::new(base.encode().unwrap())).unwrap();
    assert!(chunks::inspect(&archive_chunks[0]).is_err());
    assert!(chunks::inspect_archive(&clipboard_chunks[0]).is_err());
    let ledger = Arc::new(BudgetLedger::default());
    let mut archive_assembler = CompositeAssembler::archive_with_ledger([81; 32], ledger.clone());
    let mut clipboard_assembler = CompositeAssembler::with_ledger([81; 32], ledger.clone());
    assert!(archive_assembler
        .accept(&archive_chunks[0], 0)
        .unwrap()
        .complete
        .is_none());
    assert!(clipboard_assembler.accept(&clipboard_chunks[0], 0).is_err());
    let mut complete = None;
    for chunk in &archive_chunks[1..] {
        complete = archive_assembler.accept(chunk, 0).unwrap().complete;
    }
    assert_eq!(complete.unwrap().0, archive);
    drop(archive_assembler);
    drop(clipboard_assembler);
    assert_eq!(ledger.snapshot().pipeline_reserved, 0);
}
