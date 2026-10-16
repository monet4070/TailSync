use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use tailsync_core::cancellation::Cancellation;
use tailsync_core::composite::origin::Origin;
use tailsync_core::composite::resource_archive::{
    ArchiveUrl, FetchFuture, FetchLimits, FetchResponse, ResourceTransport,
};
use tailsync_core::composite::{packed_to_png, Completeness, Mime, PortableBundle, Representation};
use tailsync_core::db::HistoryDB;
use tailsync_runtime::automatic_image_archive::{AutomaticImageArchive, EnqueueResult};
use tailsync_runtime::history::{ArchiveEntryGuard, HistoryOperations};
use tokio::sync::{watch, Mutex, Notify};

struct Transport {
    responses: Vec<(String, Vec<u8>)>,
    second_available: AtomicBool,
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
            let (mime, bytes) = &self.responses[index];
            let available = index != 1 || self.second_available.load(Ordering::SeqCst);
            Ok(FetchResponse {
                status: if available { 200 } else { 404 },
                location: None,
                content_type: Some(mime.clone()),
                bytes: if available { bytes.clone() } else { vec![] },
            })
        })
    }
}

fn root() -> std::path::PathBuf {
    // Clock resolution alone cannot distinguish concurrently starting fixtures.
    // Keep each scenario's database and cleanup path unique without serializing it.
    static NEXT_ROOT: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "tailsync-runtime-network-originals-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT_ROOT.fetch_add(1, Ordering::Relaxed)
    ))
}

fn bundle(count: usize) -> PortableBundle {
    PortableBundle {
        representations: vec![Representation {
            mime: Mime::Html,
            bytes: (0..count)
                .map(|index| format!("<p>{index}</p><img src='https://fixture.example/{index}'>"))
                .collect::<String>()
                .into_bytes(),
        }],
        resources: vec![],
        bindings: vec![],
        completeness: Completeness::ExternalReferences,
    }
}

fn responses() -> Vec<(String, Vec<u8>)> {
    use base64::Engine as _;
    vec![
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
    ]
}

#[tokio::test]
async fn manual_network_archive_persists_all_three_original_bodies_and_refresh_identity() {
    let root = root();
    let mut database = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
    let base = bundle(3);
    let id = database.add_composite(&base, None, "local").unwrap();
    let before = database.composite_identity(id).unwrap().unwrap();
    let database = Arc::new(Mutex::new(database));
    let transport = Arc::new(Transport {
        responses: responses(),
        second_available: AtomicBool::new(true),
        calls: AtomicUsize::new(0),
    });
    let notifications = Arc::new(AtomicUsize::new(0));
    let hook = notifications.clone();
    let report = HistoryOperations::archive_entry_images_with_hook(
        database.clone(),
        id,
        transport.clone(),
        Cancellation::default(),
        move || {
            hook.fetch_add(1, Ordering::SeqCst);
        },
    )
    .await
    .unwrap();
    assert_eq!(report.archived, 3);
    assert!(report.complete);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 3);
    assert_eq!(notifications.load(Ordering::SeqCst), 1);
    let database_guard = database.lock().await;
    assert_eq!(database_guard.get_composite(id).unwrap(), base);
    let after = database_guard.composite_identity(id).unwrap().unwrap();
    assert_eq!(after.content_id, before.content_id);
    assert_eq!(after.capture_id, before.capture_id);
    assert_eq!(after.archive_revision, before.archive_revision + 1);
    let originals = database_guard.get_composite_network_originals(id).unwrap();
    assert_eq!(originals.len(), 3);
    for (index, original) in originals.iter().enumerate() {
        assert_eq!(original.image_index as usize, index);
        assert_eq!(original.bytes, transport.responses[index].1);
    }
    drop(database_guard);
    drop(database);
    let reopened = HistoryDB::open_with_policy_for_probe(
        &root,
        tailsync_core::db::MigrationPolicy::CompositeAllowed,
    )
    .unwrap();
    assert_eq!(
        reopened.get_composite_network_originals(id).unwrap(),
        originals
    );
    drop(reopened);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn failed_network_image_retry_merges_originals_instead_of_replacing_the_first() {
    let root = root();
    let mut database = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
    let id = database.add_composite(&bundle(2), None, "local").unwrap();
    let database = Arc::new(Mutex::new(database));
    let transport = Arc::new(Transport {
        responses: responses(),
        second_available: AtomicBool::new(false),
        calls: AtomicUsize::new(0),
    });
    let first = HistoryOperations::archive_entry_images(
        database.clone(),
        id,
        transport.clone(),
        Cancellation::default(),
    )
    .await
    .unwrap();
    assert_eq!(first.archived, 1);
    assert!(!first.complete);
    let original_first = database
        .lock()
        .await
        .get_composite_network_originals(id)
        .unwrap();
    assert_eq!(original_first.len(), 1);
    transport.second_available.store(true, Ordering::SeqCst);
    let second = HistoryOperations::archive_entry_images(
        database.clone(),
        id,
        transport.clone(),
        Cancellation::default(),
    )
    .await
    .unwrap();
    assert_eq!(second.archived, 1);
    assert!(second.complete);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 3);
    let originals = database
        .lock()
        .await
        .get_composite_network_originals(id)
        .unwrap();
    assert_eq!(originals.len(), 2);
    assert_eq!(originals[0], original_first[0]);
    assert_eq!(originals[1].bytes, transport.responses[1].1);
    drop(database);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn automatic_capture_queue_uses_the_same_original_persistence_transaction() {
    let root = root();
    let mut database = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
    let id = database.add_composite(&bundle(3), None, "local").unwrap();
    let guard = ArchiveEntryGuard::capture(&database, id).unwrap();
    let database = Arc::new(Mutex::new(database));
    let transport = Arc::new(Transport {
        responses: responses(),
        second_available: AtomicBool::new(true),
        calls: AtomicUsize::new(0),
    });
    let completed = Arc::new(Notify::new());
    let notification = completed.clone();
    let queue = Arc::new(AutomaticImageArchive::new(
        database.clone(),
        true,
        transport.clone(),
        Arc::new(move || notification.notify_one()),
    ));
    assert_eq!(
        queue.enqueue_captured(queue.capture_ticket().unwrap(), Origin::External, id, guard),
        EnqueueResult::Queued
    );
    let (shutdown, receiver) = watch::channel(false);
    let worker_queue = queue.clone();
    let worker = tokio::spawn(async move { worker_queue.run(receiver).await });
    tokio::time::timeout(std::time::Duration::from_secs(5), completed.notified())
        .await
        .unwrap();
    shutdown.send(true).unwrap();
    worker.await.unwrap();
    let originals = database
        .lock()
        .await
        .get_composite_network_originals(id)
        .unwrap();
    assert_eq!(originals.len(), 3);
    for (index, original) in originals.iter().enumerate() {
        assert_eq!(original.bytes, transport.responses[index].1);
    }
    drop(queue);
    drop(database);
    std::fs::remove_dir_all(root).unwrap();
}
