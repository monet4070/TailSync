use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;

use tailsync_core::cancellation::Cancellation;
use tailsync_core::composite::budgets::BudgetLedger;
use tailsync_core::composite::chunks::CompositeAssembler;
use tailsync_core::composite::origin::Origin;
use tailsync_core::composite::resource_archive::{
    ArchiveUrl, FetchFuture, FetchLimits, FetchResponse, ResourceTransport,
};
use tailsync_core::composite::{packed_to_png, Completeness, Mime, PortableBundle, Representation};
use tailsync_core::db::HistoryDB;
use tailsync_core::peer::{
    archive_receiver,
    delivery::{
        deliver_pending_frame, DeliveryConfig, DeliveryConnection, PendingFrame, SharedEvent,
    },
};
use tailsync_core::protocol::composite::{AckResult, CompositeAck};
use tailsync_core::protocol::{Command, Frame};
use tailsync_core::secure::CapabilitySet;
use tailsync_runtime::archive_sync::{bind_local_event, ArchiveSyncQueue, EnqueueResult};
use tailsync_runtime::automatic_image_archive::AutomaticImageArchive;
use tailsync_runtime::history::{ArchiveCommit, ArchiveEntryGuard, HistoryOperations};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{watch, Mutex};

// Independent scenarios share Core's process-wide pipeline budget. Their
// actual socket peers, archive workers and cancellation remain concurrent.
static PIPELINE_FIXTURE: Mutex<()> = Mutex::const_new(());

struct Transport {
    bodies: Vec<(String, Vec<u8>)>,
    available: AtomicBool,
    calls: AtomicUsize,
}

impl ResourceTransport for Transport {
    fn fetch_once<'a>(
        &'a self,
        url: &'a ArchiveUrl,
        _limits: &'a FetchLimits,
        cancellation: &'a Cancellation,
    ) -> FetchFuture<'a> {
        Box::pin(async move {
            cancellation
                .check()
                .map_err(|_| tailsync_core::composite::resource_archive::ArchiveError::Cancelled)?;
            self.calls.fetch_add(1, Ordering::SeqCst);
            let index = url.path().trim_start_matches('/').parse::<usize>().unwrap();
            let (mime, bytes) = &self.bodies[index];
            let available = index != 1 || self.available.load(Ordering::SeqCst);
            Ok(FetchResponse {
                status: if available { 200 } else { 404 },
                location: None,
                content_type: Some(mime.clone()),
                bytes: if available { bytes.clone() } else { vec![] },
            })
        })
    }
}

fn transport() -> Arc<Transport> {
    use base64::Engine as _;
    Arc::new(Transport {
        bodies: vec![
            (
                "image/png".into(),
                packed_to_png(1, 1, &[1, 2, 3, 255]).unwrap(),
            ),
            (
                "image/jpeg".into(),
                include_bytes!("../../tailsync-protocol/tests/fixtures/images/color.jpg").to_vec(),
            ),
            (
                "image/webp".into(),
                base64::engine::general_purpose::STANDARD
                    .decode("UklGRhwAAABXRUJQVlA4TA8AAAAvAAAAAAcQ0f/+ByKi/wEA")
                    .unwrap(),
            ),
        ],
        available: AtomicBool::new(false),
        calls: AtomicUsize::new(0),
    })
}

fn root() -> std::path::PathBuf {
    static NEXT_ROOT: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "tailsync-runtime-archive-sync-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT_ROOT.fetch_add(1, Ordering::Relaxed)
    ))
}

#[test]
fn isolated_store_roots_are_unique_for_back_to_back_calls() {
    let mut roots = std::collections::HashSet::new();
    for _ in 0..16_384 {
        let path = root();
        assert!(
            roots.insert(path.clone()),
            "duplicate isolated store root: {path:?}"
        );
    }
}

fn base() -> PortableBundle {
    PortableBundle { representations: vec![Representation { mime: Mime::Html, bytes: b"<p><b>offline</b></p><img src='https://fixture.example/0'><img src='https://fixture.example/1'><img src='https://fixture.example/2'>".to_vec() }], resources: vec![], bindings: vec![], completeness: Completeness::ExternalReferences }
}

struct MemoryConnection(tokio::io::DuplexStream);

impl DeliveryConnection for MemoryConnection {
    async fn write_frame(&mut self, frame: &Frame) -> Result<(), String> {
        let bytes = frame.encode();
        self.0
            .write_all(&(bytes.len() as u32).to_be_bytes())
            .await
            .map_err(|error| error.to_string())?;
        self.0
            .write_all(&bytes)
            .await
            .map_err(|error| error.to_string())
    }
    async fn read_frame(&mut self) -> Result<Frame, String> {
        let mut length = [0; 4];
        self.0
            .read_exact(&mut length)
            .await
            .map_err(|error| error.to_string())?;
        let mut bytes = vec![0; u32::from_be_bytes(length) as usize];
        self.0
            .read_exact(&mut bytes)
            .await
            .map_err(|error| error.to_string())?;
        Frame::decode(&bytes)
            .map(|(frame, _)| frame)
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

async fn deliver(
    event: &SharedEvent,
    database: Arc<Mutex<HistoryDB>>,
    notices: Arc<AtomicUsize>,
) -> i64 {
    let archive = event.queued().command() == Command::ArchiveUpdatePayload;
    let pending = PendingFrame::new(event.queued(), 64);
    let (client, server) = tokio::io::duplex(64 * 1024);
    let server = tokio::spawn(async move {
        let mut stream = MemoryConnection(server);
        let ledger = Arc::new(BudgetLedger::default());
        let mut assembler = if archive {
            CompositeAssembler::archive_with_ledger([151; 32], ledger)
        } else {
            CompositeAssembler::with_ledger([151; 32], ledger)
        };
        loop {
            let frame = stream.read_frame().await.unwrap();
            assert_eq!(
                frame.command,
                if archive {
                    Command::ArchiveUpdateChunk
                } else {
                    Command::CompositeChunk
                }
            );
            let accepted = assembler.accept(&frame.payload, 0).unwrap();
            let _budget = accepted.budget;
            let mut id = 0;
            let terminal = accepted.complete.is_some();
            let ack = if let Some((envelope, _, _)) = accepted.complete {
                if archive {
                    let notices = notices.clone();
                    let ack = archive_receiver::receive(
                        envelope,
                        "fixture-sender-key".into(),
                        [151; 32],
                        accepted.metadata.index,
                        database.clone(),
                        Arc::new(|| true),
                        Arc::new(move || {
                            notices.fetch_add(1, Ordering::SeqCst);
                        }),
                    )
                    .await
                    .unwrap();
                    let entries = HistoryOperations::new(&mut *database.lock().await)
                        .entries(tailsync_runtime::history::HistoryPageRequest::new(
                            tailsync_core::db::HistoryCollection::All,
                            None,
                            None,
                            None,
                            None,
                            Some(10),
                            None,
                            10,
                        ))
                        .unwrap();
                    assert_eq!(entries.len(), 1);
                    id = entries[0].id;
                    ack
                } else {
                    let bundle = PortableBundle::decode(&envelope.content).unwrap();
                    id = database
                        .lock()
                        .await
                        .add_composite_event(
                            &bundle,
                            None,
                            "sender",
                            Some((
                                "fixture-sender-key",
                                envelope.message_id,
                                envelope.timestamp_ms,
                            )),
                        )
                        .unwrap();
                    CompositeAck {
                        message_id: envelope.message_id,
                        digest: bundle.digest().unwrap(),
                        chunk_index: accepted.metadata.index,
                        result: AckResult::Applied,
                    }
                }
            } else {
                CompositeAck {
                    message_id: accepted.metadata.message_id,
                    digest: accepted.metadata.digest,
                    chunk_index: accepted.metadata.index,
                    result: AckResult::Chunk,
                }
            };
            stream
                .write_frame(
                    &Frame::try_new(
                        if archive {
                            Command::ArchiveUpdateAck
                        } else {
                            Command::CompositeAck
                        },
                        0,
                        frame.sequence,
                        ack.encode(),
                    )
                    .unwrap(),
                )
                .await
                .unwrap();
            if terminal {
                return id;
            }
        }
    });
    let mut client = MemoryConnection(client);
    deliver_pending_frame(
        &mut client,
        &pending,
        &DeliveryConfig::try_new(Duration::from_secs(5), Duration::from_secs(5)).unwrap(),
    )
    .await
    .unwrap();
    server.await.unwrap()
}

#[tokio::test]
async fn download_hooks_and_real_archive_frames_complete_one_history_then_reopen_offline() {
    let _fixture = PIPELINE_FIXTURE.lock().await;
    for automatic in [false, true] {
        let sender_root = root();
        let receiver_root = root();
        let base = base();
        let mut database = HistoryDB::open_isolated_for_test_composite(&sender_root).unwrap();
        let id = database.add_composite(&base, None, "local").unwrap();
        let original = ArchiveCommit {
            id,
            guard: ArchiveEntryGuard::capture(&database, id).unwrap(),
        };
        let database = Arc::new(Mutex::new(database));
        let outgoing = Arc::new(ArchiveSyncQueue::new(database.clone()));
        let payload: Arc<[u8]> = base.encode().unwrap().into();
        let event = SharedEvent::encode_shared(Command::CompositePayload, payload.clone()).unwrap();
        let bound = bind_local_event(database.clone(), original, &event, payload)
            .await
            .unwrap()
            .unwrap();
        let receiver = Arc::new(Mutex::new(
            HistoryDB::open_isolated_for_test_composite(&receiver_root).unwrap(),
        ));
        let notices = Arc::new(AtomicUsize::new(0));
        let remote_id = deliver(&event, receiver.clone(), notices.clone()).await;
        let before = receiver
            .lock()
            .await
            .composite_identity(remote_id)
            .unwrap()
            .unwrap();
        let transport = transport();
        let on_commit = outgoing.clone();
        let queue = Arc::new(AutomaticImageArchive::with_commit_hook(
            database.clone(),
            true,
            transport.clone(),
            Arc::new(move |commit| {
                assert_ne!(on_commit.enqueue(commit), EnqueueResult::Full);
            }),
        ));
        let (shutdown, stop) = watch::channel(false);
        let mut worker = None;
        if automatic {
            assert_eq!(
                queue.enqueue_captured(
                    queue.capture_ticket().unwrap(),
                    Origin::External,
                    id,
                    bound.guard
                ),
                tailsync_runtime::automatic_image_archive::EnqueueResult::Queued
            );
            let runner = queue.clone();
            worker = Some(tokio::spawn(async move { runner.run(stop).await }));
        } else {
            let committed = queue.clone();
            let report = HistoryOperations::archive_entry_images_with_commit_hook(
                database.clone(),
                id,
                transport.clone(),
                Cancellation::default(),
                move |commit| committed.committed(commit),
            )
            .await
            .unwrap();
            assert_eq!(report.archived, 2);
            assert!(!report.complete);
        }
        let first = tokio::time::timeout(Duration::from_secs(5), outgoing.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(outgoing.current(first.commit.clone()).await.is_some());
        assert_eq!(
            deliver(&first.event, receiver.clone(), notices.clone()).await,
            remote_id
        );
        assert_eq!(
            receiver
                .lock()
                .await
                .get_composite_network_originals(remote_id)
                .unwrap()
                .len(),
            2
        );
        if let Some(worker) = worker {
            shutdown.send(true).unwrap();
            worker.await.unwrap();
        }
        transport.available.store(true, Ordering::SeqCst);
        let committed = queue.clone();
        let report = HistoryOperations::archive_entry_images_with_commit_hook(
            database.clone(),
            id,
            transport.clone(),
            Cancellation::default(),
            move |commit| committed.committed(commit),
        )
        .await
        .unwrap();
        assert_eq!(report.archived, 1);
        assert!(report.complete);
        let second = outgoing.next().await.unwrap().unwrap();
        assert_eq!(
            deliver(&second.event, receiver.clone(), notices.clone()).await,
            remote_id
        );
        assert_eq!(
            deliver(&second.event, receiver.clone(), notices.clone()).await,
            remote_id
        );
        assert_eq!(notices.load(Ordering::SeqCst), 2);
        assert_eq!(transport.calls.load(Ordering::SeqCst), 4);
        drop(receiver);
        let receiver = Arc::new(Mutex::new(
            HistoryDB::open_with_policy_for_probe(
                &receiver_root,
                tailsync_core::db::MigrationPolicy::CompositeAllowed,
            )
            .unwrap(),
        ));
        let originals = receiver
            .lock()
            .await
            .get_composite_network_originals(remote_id)
            .unwrap();
        for (original, body) in originals.iter().zip(&transport.bodies) {
            assert_eq!(original.bytes, body.1);
        }
        assert_eq!(originals.len(), 3);
        let current = receiver
            .lock()
            .await
            .composite_identity(remote_id)
            .unwrap()
            .unwrap();
        assert_eq!(current.capture_id, before.capture_id);
        assert_eq!(current.content_id, before.content_id);
        assert_eq!(current.dedupe_key, before.dedupe_key);
        assert_eq!(
            receiver.lock().await.get_composite(remote_id).unwrap(),
            base
        );
        let preview = tailsync_runtime::document_preview::document_preview_async(
            receiver.clone(),
            remote_id,
            None,
            Arc::new(tailsync_runtime::content_analysis::UnsupportedNativeAnalyzer),
            Cancellation::default(),
        )
        .await
        .unwrap();
        let document = preview.document().unwrap();
        assert_eq!(document.assets.len(), 3);
        let restore = HistoryOperations::new(&mut *receiver.lock().await)
            .restore_payload(remote_id)
            .unwrap();
        let restored = PortableBundle::decode(restore.data.as_deref().unwrap()).unwrap();
        assert_eq!(restored.resources.len(), 3);
        assert_eq!(transport.calls.load(Ordering::SeqCst), 4);
        drop(preview);
        drop(receiver);
        drop(queue);
        drop(outgoing);
        drop(database);
        std::fs::remove_dir_all(sender_root).unwrap();
        std::fs::remove_dir_all(receiver_root).unwrap();
    }
}

#[tokio::test]
async fn queue_is_bounded_coalesces_and_rejects_deleted_or_replaced_authority() {
    let _fixture = PIPELINE_FIXTURE.lock().await;
    let directory = root();
    let mut database = HistoryDB::open_isolated_for_test_composite(&directory).unwrap();
    let id = database.add_composite(&base(), None, "local").unwrap();
    let commit = ArchiveCommit {
        id,
        guard: ArchiveEntryGuard::capture(&database, id).unwrap(),
    };
    let database = Arc::new(Mutex::new(database));
    let outgoing = ArchiveSyncQueue::new(database.clone());
    assert_eq!(outgoing.enqueue(commit.clone()), EnqueueResult::Queued);
    assert_eq!(outgoing.enqueue(commit.clone()), EnqueueResult::Coalesced);
    for index in 1..8 {
        let mut job = commit.clone();
        job.id = id + index;
        assert_eq!(outgoing.enqueue(job), EnqueueResult::Queued);
    }
    let mut overflow = commit.clone();
    overflow.id = id + 8;
    assert_eq!(outgoing.enqueue(overflow), EnqueueResult::Full);
    database.lock().await.delete(id).unwrap();
    assert!(outgoing.current(commit.clone()).await.is_none());
    assert!(outgoing.next().await.unwrap().is_none());
    let replacement_root = root();
    *database.lock().await =
        HistoryDB::open_isolated_for_test_composite(&replacement_root).unwrap();
    assert!(outgoing.current(commit).await.is_none());
    drop(outgoing);
    drop(database);
    std::fs::remove_dir_all(directory).unwrap();
    std::fs::remove_dir_all(replacement_root).unwrap();
}

#[tokio::test]
async fn binding_is_exact_and_catches_a_manual_archive_saved_before_the_base_was_queued() {
    let _fixture = PIPELINE_FIXTURE.lock().await;
    let directory = root();
    let base = base();
    let mut database = HistoryDB::open_isolated_for_test_composite(&directory).unwrap();
    let id = database.add_composite(&base, None, "local").unwrap();
    let original = ArchiveCommit {
        id,
        guard: ArchiveEntryGuard::capture(&database, id).unwrap(),
    };
    let database = Arc::new(Mutex::new(database));
    let payload: Arc<[u8]> = base.encode().unwrap().into();
    let event = SharedEvent::encode_shared(Command::CompositePayload, payload.clone()).unwrap();
    let text = SharedEvent::encode(Command::TextPayload, payload.to_vec()).unwrap();
    assert!(
        bind_local_event(database.clone(), original.clone(), &text, payload.clone())
            .await
            .is_err()
    );
    assert!(bind_local_event(
        database.clone(),
        original.clone(),
        &event,
        Arc::from(&b"wrong"[..])
    )
    .await
    .is_err());
    let transport = transport();
    transport.available.store(true, Ordering::SeqCst);
    HistoryOperations::archive_entry_images(
        database.clone(),
        id,
        transport,
        Cancellation::default(),
    )
    .await
    .unwrap();
    let bound = bind_local_event(database.clone(), original, &event, payload)
        .await
        .unwrap()
        .unwrap();
    assert!(bound.has_saved_archive());
    let outgoing = ArchiveSyncQueue::new(database.clone());
    outgoing.enqueue(bound);
    let dispatch = outgoing.next().await.unwrap().unwrap();
    let envelope =
        tailsync_core::protocol::EventEnvelope::decode(dispatch.event.queued().payload()).unwrap();
    let update =
        tailsync_core::composite::archive_sync::ArchiveUpdate::decode(&envelope.content).unwrap();
    assert_eq!(update.base_event, event.envelope_identity().0);
    assert_eq!(update.images.len(), 3);
    drop(outgoing);
    drop(database);
    std::fs::remove_dir_all(directory).unwrap();
}
