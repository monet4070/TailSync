use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tailsync_core::cancellation::Cancellation;
use tailsync_core::composite::content_analysis::{NativeAnalysisFacts, NativeAttachment};
use tailsync_core::composite::{
    packed_to_png, Completeness, Mime, PortableBundle, Representation, Resource, ResourceBinding,
};
use tailsync_core::db::{HistoryCollection, HistoryDB, MigrationPolicy};
use tailsync_core::protocol::content_analysis::{AttachmentState, SavedAnalysisKind};
use tailsync_core::protocol::native_archive::NativeArchive;
use tailsync_core::protocol::native_clipboard::{
    FormatState, NativeFormat, NativeSnapshot, SnapshotState,
};
use tailsync_runtime::content_analysis::{
    analyze_entry, NativeAnalysisTask, NativeAnalyzer, UnsupportedNativeAnalyzer,
};
use tailsync_runtime::history::HistoryPageRequestOwned;
use tailsync_runtime::history_display::{
    image_payload_checked_async, page_display_async, rebuild_display_batch,
};
use tokio::sync::Mutex;

// These scenarios reserve process-wide analysis permits and assert ledger
// totals. Give each fixture exclusive ownership of that shared test resource;
// its own two-worker runtime still exercises concurrent reads and cancellation.
static DISPLAY_SCENARIO: Mutex<()> = Mutex::const_new(());

struct Store {
    path: std::path::PathBuf,
    database: Arc<Mutex<HistoryDB>>,
}

impl Store {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "tailsync-display-runtime-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
        ));
        let database = HistoryDB::open_isolated_for_test_composite(&path).unwrap();
        Self {
            path,
            database: Arc::new(Mutex::new(database)),
        }
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn request(
    collection: HistoryCollection,
    category: Option<&str>,
    keyword: Option<&str>,
) -> HistoryPageRequestOwned {
    HistoryPageRequestOwned {
        collection,
        category: category.map(str::to_string),
        keyword: keyword.map(str::to_string),
        start_time: None,
        end_time: None,
        limit: 30,
        offset: 0,
    }
}

fn document(body: &str) -> PortableBundle {
    let html = format!("<p>{body}</p><img src='tailsync-resource:1'>");
    let offset = html.find("tailsync-resource:1").unwrap();
    PortableBundle {
        representations: vec![Representation {
            mime: Mime::Html,
            bytes: html.into_bytes(),
        }],
        resources: vec![Resource {
            id: 1,
            png: packed_to_png(1, 1, &[255, 0, 0, 255]).unwrap(),
        }],
        bindings: vec![ResourceBinding {
            representation: 0,
            resource: 1,
            offset: offset as u32,
            length: 19,
        }],
        completeness: Completeness::Complete,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bounded_rebuild_preserves_originals_and_unifies_filters_search_favorites_counts() {
    let _scenario = DISPLAY_SCENARIO.lock().await;
    let store = Store::new();
    let mut originals = Vec::new();
    for index in 0..9 {
        let bundle = document(&format!("正文-{index}"));
        let mut database = store.database.lock().await;
        let id = database
            .add_composite(&bundle, None, "display-fixture")
            .unwrap();
        if index % 3 == 0 {
            database.set_favorite(id, true).unwrap();
        }
        originals.push((id, bundle, database.composite_identity(id).unwrap()));
    }
    let before = store
        .database
        .lock()
        .await
        .get_all(None, None, 30, 0)
        .unwrap();
    assert_eq!(
        store
            .database
            .lock()
            .await
            .display_rebuild_candidates(100)
            .unwrap()
            .len(),
        4
    );
    let page = page_display_async(
        store.database.clone(),
        request(HistoryCollection::All, Some("image"), None),
        Arc::new(UnsupportedNativeAnalyzer),
    )
    .await
    .unwrap();
    assert_eq!(page.display_index_pending, Some(true));
    assert_eq!(page.total, None);
    let entries = store
        .database
        .lock()
        .await
        .get_all(None, None, 30, 0)
        .unwrap();
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry.classifier_version
                == tailsync_core::composite::display::DISPLAY_CLASSIFIER_VERSION)
            .count(),
        4
    );
    assert!(rebuild_display_batch(
        store.database.clone(),
        Arc::new(UnsupportedNativeAnalyzer),
        Cancellation::default()
    )
    .await
    .unwrap());
    let caller = Cancellation::default();
    assert!(!rebuild_display_batch(
        store.database.clone(),
        Arc::new(UnsupportedNativeAnalyzer),
        caller.clone()
    )
    .await
    .unwrap());
    assert!(!caller.is_cancelled());
    for category in ["text", "image"] {
        let page = page_display_async(
            store.database.clone(),
            request(HistoryCollection::All, Some(category), Some("正文")),
            Arc::new(UnsupportedNativeAnalyzer),
        )
        .await
        .unwrap();
        assert_eq!(page.total, None);
        assert_eq!(page.entries.len(), 9);
        assert_eq!(page.display_index_pending, Some(false));
        assert!(page
            .entries
            .iter()
            .all(|entry| entry.display.as_ref().unwrap().kind == "document"));
        let complete = page_display_async(
            store.database.clone(),
            request(HistoryCollection::All, Some(category), None),
            Arc::new(UnsupportedNativeAnalyzer),
        )
        .await
        .unwrap();
        assert_eq!(complete.total, Some(9));
        let favorites = page_display_async(
            store.database.clone(),
            request(HistoryCollection::Favorites, Some(category), None),
            Arc::new(UnsupportedNativeAnalyzer),
        )
        .await
        .unwrap();
        assert_eq!(favorites.total, Some(3));
    }
    let database = store.database.lock().await;
    for (id, original, identity) in originals {
        assert_eq!(database.get_composite(id).unwrap(), original);
        assert_eq!(database.composite_identity(id).unwrap(), identity);
    }
    let after = database.get_all(None, None, 30, 0).unwrap();
    for (old, new) in before.iter().zip(after.iter()) {
        assert_eq!(
            (
                &old.data_hash,
                old.size_bytes,
                &old.source_peer,
                old.pinned,
                &old.category
            ),
            (
                &new.data_hash,
                new.size_bytes,
                &new.source_peer,
                new.pinned,
                &new.category
            )
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reopen_and_deleted_rows_reject_old_thumbnail_authority_and_release_memory() {
    let _scenario = DISPLAY_SCENARIO.lock().await;
    let store = Store::new();
    let id = store
        .database
        .lock()
        .await
        .add_composite(&document("body"), None, "fixture")
        .unwrap();
    rebuild_display_batch(
        store.database.clone(),
        Arc::new(UnsupportedNativeAnalyzer),
        Cancellation::default(),
    )
    .await
    .unwrap();
    let key = store.database.lock().await.thumbnail_key(id).unwrap();
    let caller = Cancellation::default();
    let ledger = tailsync_core::composite::budgets::global();
    let before = ledger.snapshot().pipeline_reserved;
    let image = image_payload_checked_async(
        store.database.clone(),
        id,
        Some(key.clone()),
        Arc::new(UnsupportedNativeAnalyzer),
        caller.clone(),
    )
    .await
    .unwrap();
    assert_eq!(&image.data[8..], &[255, 0, 0, 255]);
    assert!(ledger.snapshot().pipeline_reserved > before);
    assert!(!caller.is_cancelled());
    drop(image);
    assert_eq!(ledger.snapshot().pipeline_reserved, before);
    *store.database.lock().await =
        HistoryDB::open_with_policy_for_probe(&store.path, MigrationPolicy::CompositeAllowed)
            .unwrap();
    assert_ne!(key, store.database.lock().await.thumbnail_key(id).unwrap());
    assert!(image_payload_checked_async(
        store.database.clone(),
        id,
        Some(key),
        Arc::new(UnsupportedNativeAnalyzer),
        caller.clone()
    )
    .await
    .is_err());
    assert!(!caller.is_cancelled());
    store.database.lock().await.delete(id).unwrap();
    assert!(image_payload_checked_async(
        store.database.clone(),
        id,
        None,
        Arc::new(UnsupportedNativeAnalyzer),
        caller.clone()
    )
    .await
    .is_err());
    assert!(!caller.is_cancelled());
    assert_eq!(ledger.snapshot().pipeline_reserved, before);
}

struct AttachmentAnalyzer;

impl NativeAnalyzer for AttachmentAnalyzer {
    fn analyze(
        &self,
        task: &NativeAnalysisTask,
        _cancelled: &AtomicBool,
    ) -> Result<NativeAnalysisFacts, String> {
        Ok(NativeAnalysisFacts {
            kind: SavedAnalysisKind::Rtfd,
            item: task.item,
            source_format: task.source_format.clone(),
            converter: "tailsync.rtfd-to-facts.v1".into(),
            body: "正文🙂\u{fffc}".into(),
            truncated: false,
            partial: false,
            style_bits: 0,
            table_rows: 0,
            table_cells: 0,
            attachments: vec![NativeAttachment {
                utf16_position: 4,
                state: AttachmentState::Available,
                png: packed_to_png(1, 1, &[0, 0, 255, 255]).unwrap(),
            }],
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn saved_native_only_attachment_is_a_document_and_thumbnail_without_portable_images() {
    let _scenario = DISPLAY_SCENARIO.lock().await;
    let store = Store::new();
    let archive = NativeArchive {
        platform: 1,
        enumeration_complete: true,
        original: NativeSnapshot {
            before: 1,
            after: 1,
            items: 1,
            state: SnapshotState::Stable,
            formats: vec![NativeFormat {
                item: 0,
                name: "com.apple.flat-rtfd".into(),
                state: FormatState::Captured,
                bytes: b"saved-fixture-not-general-clipboard".to_vec(),
            }],
        },
        derived: vec![],
    };
    let id = store
        .database
        .lock()
        .await
        .add_native_archive(&archive, "fixture")
        .unwrap();
    let identity = store.database.lock().await.composite_identity(id).unwrap();
    let page = page_display_async(
        store.database.clone(),
        request(HistoryCollection::All, Some("image"), None),
        Arc::new(AttachmentAnalyzer),
    )
    .await
    .unwrap();
    assert_eq!(page.total, Some(1));
    let entry = &page.entries[0];
    assert_eq!(entry.composite.as_ref().unwrap().image_count, 0);
    assert!(entry.composite.as_ref().unwrap().native_only);
    assert_eq!(entry.display.as_ref().unwrap().kind, "document");
    assert!(entry.display.as_ref().unwrap().thumbnail_eligible);
    let image = image_payload_checked_async(
        store.database.clone(),
        id,
        Some(entry.display.as_ref().unwrap().cache_key.clone()),
        Arc::new(AttachmentAnalyzer),
        Cancellation::default(),
    )
    .await
    .unwrap();
    assert_eq!(&image.data[8..], &[0, 0, 255, 255]);
    assert_eq!(
        store
            .database
            .lock()
            .await
            .get_composite_archive(id)
            .unwrap()
            .unwrap(),
        archive
    );
    assert_eq!(
        store.database.lock().await.composite_identity(id).unwrap(),
        identity
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_rebuild_and_stale_analysis_cannot_commit() {
    let _scenario = DISPLAY_SCENARIO.lock().await;
    let store = Store::new();
    let id = store
        .database
        .lock()
        .await
        .add_composite(&document("body"), None, "fixture")
        .unwrap();
    let caller = Cancellation::default();
    caller.cancel();
    assert!(rebuild_display_batch(
        store.database.clone(),
        Arc::new(UnsupportedNativeAnalyzer),
        caller
    )
    .await
    .is_err());
    assert_eq!(
        store
            .database
            .lock()
            .await
            .display_rebuild_candidates(4)
            .unwrap(),
        vec![id]
    );
    let facts = analyze_entry(
        store.database.clone(),
        id,
        Arc::new(UnsupportedNativeAnalyzer),
    )
    .await
    .unwrap();
    store.database.lock().await.set_favorite(id, true).unwrap();
    assert!(!facts
        .commit_display(&mut *store.database.lock().await, &Cancellation::default())
        .unwrap());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelling_and_aborting_lock_waiters_never_pollute_another_callers_token() {
    let _scenario = DISPLAY_SCENARIO.lock().await;
    let store = Store::new();
    let held = store.database.lock().await;
    let caller = Cancellation::default();
    let worker = {
        let database = store.database.clone();
        let cancellation = caller.clone();
        tokio::spawn(async move {
            rebuild_display_batch(database, Arc::new(UnsupportedNativeAnalyzer), cancellation).await
        })
    };
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    caller.cancel();
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(1), worker)
            .await
            .unwrap()
            .unwrap()
            .is_err()
    );
    let independent = Cancellation::default();
    let worker = {
        let database = store.database.clone();
        let cancellation = independent.clone();
        tokio::spawn(async move {
            image_payload_checked_async(
                database,
                1,
                None,
                Arc::new(UnsupportedNativeAnalyzer),
                cancellation,
            )
            .await
            .is_ok()
        })
    };
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    worker.abort();
    assert!(worker.await.is_err());
    assert!(!independent.is_cancelled());
    drop(held);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn text_only_composites_keep_text_preview_and_share_one_code_filter() {
    let _scenario = DISPLAY_SCENARIO.lock().await;
    use tailsync_core::db::PreviewKind;
    use tailsync_runtime::history::HistoryOperations;
    let store = Store::new();
    for (text, category) in [
        ("just a normal note", "text"),
        ("git status --short", "code"),
        (r#"{"enabled":true}"#, "code"),
    ] {
        let bundle = PortableBundle {
            representations: vec![
                Representation {
                    mime: Mime::Text,
                    bytes: text.as_bytes().to_vec(),
                },
                Representation {
                    mime: Mime::Html,
                    bytes: format!("<p><b>{text}</b></p>").into_bytes(),
                },
            ],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::Complete,
        };
        let id = store
            .database
            .lock()
            .await
            .add_composite(&bundle, None, "fixture")
            .unwrap();
        let page = page_display_async(
            store.database.clone(),
            request(HistoryCollection::All, Some(category), Some(text)),
            Arc::new(UnsupportedNativeAnalyzer),
        )
        .await
        .unwrap();
        assert_eq!(page.entries.len(), 1, "{text}");
        let entry = &page.entries[0];
        assert_eq!(entry.id, id);
        assert_eq!(entry.category, category);
        assert_eq!(entry.categories, vec![category]);
        assert_eq!(entry.display.as_ref().unwrap().kind, "text");
        assert!(!entry.display.as_ref().unwrap().thumbnail_eligible);
        let (metadata, payload) =
            HistoryOperations::preview_async(store.database.clone(), id, None)
                .await
                .unwrap();
        assert_eq!(metadata.kind, PreviewKind::Text);
        assert_eq!(payload.data, text.as_bytes());
        assert_eq!(
            store.database.lock().await.get_composite(id).unwrap(),
            bundle
        );
    }
    let page = page_display_async(
        store.database.clone(),
        request(HistoryCollection::All, Some("code"), None),
        Arc::new(UnsupportedNativeAnalyzer),
    )
    .await
    .unwrap();
    assert_eq!(page.entries.len(), 2);
    assert_eq!(page.total, Some(2));
}
