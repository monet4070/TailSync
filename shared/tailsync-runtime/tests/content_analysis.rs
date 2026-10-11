use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};

use tailsync_core::cancellation::Cancellation;
use tailsync_core::composite::content_analysis::{ImageAvailability, NativeAnalysisFacts};
use tailsync_core::composite::{
    packed_to_png, Completeness, Mime, PortableBundle, Representation, Resource, ResourceBinding,
};
use tailsync_core::db::HistoryDB;
use tailsync_core::protocol::native_archive::NativeArchive;
use tailsync_core::protocol::native_clipboard::{
    FormatState, NativeFormat, NativeSnapshot, SnapshotState,
};
use tailsync_runtime::content_analysis::{
    analyze_entry, analyze_entry_cancellable, ContentAnalysisError, NativeAnalysisTask,
    NativeAnalyzer, UnsupportedNativeAnalyzer,
};
use tokio::sync::Mutex as AsyncMutex;

fn root() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "tailsync-content-analysis-{}-{:016x}",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retained_analysis_keeps_memory_and_authority_until_drop() {
    let (path, database) = database();
    let id = store(&database, &html_bundle("<p>中文🙂</p>", vec![])).await;
    let ledger = tailsync_core::composite::budgets::global();
    let before = ledger.snapshot().pipeline_reserved;
    let cancellation = Cancellation::default();
    let result = analyze_entry_cancellable(
        database.clone(),
        id,
        Arc::new(UnsupportedNativeAnalyzer),
        cancellation.clone(),
    )
    .await
    .unwrap();
    assert!(!cancellation.is_cancelled());
    assert!(ledger.snapshot().pipeline_reserved > before);
    assert!(result.is_current(&*database.lock().await).unwrap());
    let reopened = HistoryDB::open_with_policy_for_probe(
        &path,
        tailsync_core::db::MigrationPolicy::CompositeAllowed,
    )
    .unwrap();
    assert_eq!(
        reopened.composite_identity(id).unwrap(),
        database.lock().await.composite_identity(id).unwrap()
    );
    assert!(!result.is_current(&reopened).unwrap());
    database.lock().await.set_favorite(id, true).unwrap();
    assert!(!result.is_current(&*database.lock().await).unwrap());
    database.lock().await.set_favorite(id, false).unwrap();
    database.lock().await.delete(id).unwrap();
    assert!(!result.is_current(&*database.lock().await).unwrap());
    drop(result);
    assert_eq!(ledger.snapshot().pipeline_reserved, before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pipeline_pressure_refuses_analysis_before_owned_reads() {
    let (_path, database) = database();
    let ledger = tailsync_core::composite::budgets::global();
    let lease = ledger
        .reserve_pipeline(tailsync_core::composite::budgets::PIPELINE_BUDGET_BYTES)
        .unwrap();
    let outcome = analyze_entry(database.clone(), 1, Arc::new(UnsupportedNativeAnalyzer)).await;
    assert!(matches!(outcome, Err(ContentAnalysisError::Busy)));
    drop(lease);
    assert!(matches!(
        analyze_entry(database, 1, Arc::new(UnsupportedNativeAnalyzer)).await,
        Err(ContentAnalysisError::Unavailable)
    ));
}

struct DeadlineAnalyzer;

struct WrongItemAnalyzer;

impl NativeAnalyzer for WrongItemAnalyzer {
    fn analyze(
        &self,
        task: &NativeAnalysisTask,
        _cancelled: &AtomicBool,
    ) -> Result<NativeAnalysisFacts, String> {
        Ok(NativeAnalysisFacts::from_saved(
            tailsync_core::protocol::content_analysis::scan_rtf(&task.bytes),
            task.item + 1,
        ))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn helper_cannot_reassign_the_saved_source_to_another_item() {
    let (_path, database) = database();
    let id = store(&database, &rtf_bundle("{\\rtf1 body}")).await;
    assert!(matches!(
        analyze_entry(database, id, Arc::new(WrongItemAnalyzer)).await,
        Err(ContentAnalysisError::Invalid)
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn aborting_a_db_lock_waiter_releases_the_worker_not_the_callers_token() {
    let (_path, database) = database();
    let held = database.lock().await;
    let cancellation = Cancellation::default();
    let worker = {
        let database = database.clone();
        let cancellation = cancellation.clone();
        tokio::spawn(async move {
            analyze_entry_cancellable(
                database,
                1,
                Arc::new(UnsupportedNativeAnalyzer),
                cancellation,
            )
            .await
        })
    };
    tokio::time::sleep(std::time::Duration::from_millis(60)).await;
    worker.abort();
    let _ = worker.await;
    tokio::time::sleep(std::time::Duration::from_millis(60)).await;
    assert!(!cancellation.is_cancelled());
    let (_, other) = self::database();
    assert!(matches!(
        analyze_entry(other, 1, Arc::new(UnsupportedNativeAnalyzer)).await,
        Err(ContentAnalysisError::Unavailable)
    ));
    drop(held);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn archived_overlay_is_analyzed_without_replacing_original_identity() {
    let (_path, database) = database();
    let original = html_bundle(
        "<p>caption</p><img src='https://tailsync.invalid/red'>",
        vec![],
    );
    let id = store(&database, &original).await;
    let identity = database
        .lock()
        .await
        .composite_identity(id)
        .unwrap()
        .unwrap();
    let prior = analyze_entry(database.clone(), id, Arc::new(UnsupportedNativeAnalyzer))
        .await
        .unwrap();
    let spans =
        tailsync_core::composite::html::reference_inventory(&original.representations[0].bytes)
            .unwrap()
            .images;
    let (archived, _) = tailsync_core::composite::resource_archive::apply_resource_bindings(
        &original,
        &[
            tailsync_core::composite::resource_archive::ResourceReplacement {
                span: spans[0],
                png: png(0x55),
            },
        ],
    )
    .unwrap();
    assert!(database
        .lock()
        .await
        .commit_composite_archive(id, &identity, &archived)
        .unwrap()
        .is_some());
    assert!(!prior.is_current(&*database.lock().await).unwrap());
    let result = analyze_entry(database.clone(), id, Arc::new(UnsupportedNativeAnalyzer))
        .await
        .unwrap();
    assert_eq!(result.identity.content_id, identity.content_id);
    assert_eq!(result.images[0].availability, ImageAvailability::Available);
    assert_eq!(result.images[0].png, archived.resources[0].png);
    assert!(result.is_current(&*database.lock().await).unwrap());
}

impl NativeAnalyzer for DeadlineAnalyzer {
    fn analyze(
        &self,
        _task: &NativeAnalysisTask,
        cancelled: &AtomicBool,
    ) -> Result<NativeAnalysisFacts, String> {
        while !cancelled.load(std::sync::atomic::Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        Err("worker cancelled".into())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deadline_cancels_and_joins_the_real_worker_then_releases_admission() {
    let (_path, database) = database();
    let id = store(&database, &rtf_bundle("{\\rtf1 body}")).await;
    let ledger = tailsync_core::composite::budgets::global();
    let before = ledger.snapshot().pipeline_reserved;
    let began = std::time::Instant::now();
    let outcome = analyze_entry(database.clone(), id, Arc::new(DeadlineAnalyzer)).await;
    assert!(matches!(outcome, Err(ContentAnalysisError::Timeout)));
    assert!(began.elapsed() < std::time::Duration::from_secs(5));
    assert_eq!(ledger.snapshot().pipeline_reserved, before);
    assert!(
        analyze_entry(database, id, Arc::new(UnsupportedNativeAnalyzer))
            .await
            .is_ok()
    );
}

fn png(tint: u8) -> Vec<u8> {
    packed_to_png(1, 1, &[tint, 0, 0, 255]).unwrap()
}

fn html_bundle(html: &str, resources: Vec<Resource>) -> PortableBundle {
    let bindings = resources
        .iter()
        .filter_map(|resource| {
            let reference = format!("tailsync-resource:{}", resource.id);
            html.find(&reference).map(|offset| ResourceBinding {
                representation: 0,
                resource: resource.id,
                offset: offset as u32,
                length: reference.len() as u32,
            })
        })
        .collect();
    PortableBundle {
        representations: vec![Representation {
            mime: Mime::Html,
            bytes: html.as_bytes().to_vec(),
        }],
        resources,
        bindings,
        completeness: Completeness::Complete,
    }
}

fn rtf_bundle(rtf: &str) -> PortableBundle {
    PortableBundle {
        representations: vec![Representation {
            mime: Mime::Rtf,
            bytes: rtf.as_bytes().to_vec(),
        }],
        resources: vec![],
        bindings: vec![],
        completeness: Completeness::Complete,
    }
}

fn database() -> (PathBuf, Arc<AsyncMutex<HistoryDB>>) {
    let path = root();
    let db = HistoryDB::open_isolated_for_test_composite(&path).unwrap();
    (path, Arc::new(AsyncMutex::new(db)))
}

async fn store(database: &Arc<AsyncMutex<HistoryDB>>, bundle: &PortableBundle) -> i64 {
    let mut guard = database.lock().await;
    guard.add_composite(bundle, None, "s2-02").unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn node_order_and_first_image_survive_the_production_read_path() {
    let (_path, database) = database();
    let html = "<p><img src=\"tailsync-resource:2\"><img src=\"tailsync-resource:1\"></p>";
    let bundle = html_bundle(
        html,
        vec![
            Resource {
                id: 1,
                png: png(0x11),
            },
            Resource {
                id: 2,
                png: png(0x22),
            },
        ],
    );
    let id = store(&database, &bundle).await;
    let analysis = analyze_entry(database.clone(), id, Arc::new(UnsupportedNativeAnalyzer))
        .await
        .unwrap();
    let order: Vec<u16> = analysis
        .images
        .iter()
        .map(|image| image.resource_id.unwrap())
        .collect();
    assert_eq!(order, vec![2, 1]);
    assert_eq!(analysis.first_image, Some(0));
    assert_eq!(analysis.selected, Some(0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn duplicate_reference_keeps_both_positions() {
    let (_path, database) = database();
    let html = "<img src=\"tailsync-resource:1\"><p>x</p><img src=\"tailsync-resource:1\">";
    let bundle = html_bundle(
        html,
        vec![Resource {
            id: 1,
            png: png(0x33),
        }],
    );
    let id = store(&database, &bundle).await;
    let analysis = analyze_entry(database.clone(), id, Arc::new(UnsupportedNativeAnalyzer))
        .await
        .unwrap();
    assert_eq!(analysis.images.len(), 2);
    assert_ne!(analysis.images[0].coordinate, analysis.images[1].coordinate);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn first_image_missing_is_located_and_not_replaced_by_the_second() {
    let (_path, database) = database();
    let html = "<img src=\"https://example.com/first.png\"><img src=\"tailsync-resource:2\">";
    let mut bundle = html_bundle(
        html,
        vec![Resource {
            id: 2,
            png: png(0x44),
        }],
    );
    bundle.completeness = Completeness::ExternalReferences;
    let id = store(&database, &bundle).await;
    let analysis = analyze_entry(database.clone(), id, Arc::new(UnsupportedNativeAnalyzer))
        .await
        .unwrap();
    assert_eq!(analysis.first_image, Some(0));
    assert_eq!(
        analysis.images[0].availability,
        ImageAvailability::ExternalReference
    );
    assert_eq!(
        analysis.images[1].availability,
        ImageAvailability::Available
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn analysis_leaves_the_stored_identity_and_original_untouched() {
    let (_path, database) = database();
    let archive = NativeArchive {
        platform: 1,
        enumeration_complete: true,
        original: NativeSnapshot {
            before: 7,
            after: 7,
            items: 1,
            state: SnapshotState::Stable,
            formats: vec![
                NativeFormat {
                    item: 0,
                    name: "com.apple.flat-rtfd".into(),
                    state: FormatState::Captured,
                    bytes: b"original RTFD bytes".to_vec(),
                },
                NativeFormat {
                    item: 0,
                    name: "public.utf8-plain-text".into(),
                    state: FormatState::Captured,
                    bytes: b"original text".to_vec(),
                },
            ],
        },
        derived: vec![],
    };
    let id = {
        let mut guard = database.lock().await;
        guard.add_native_archive(&archive, "s2-02").unwrap()
    };
    let before_identity = database.lock().await.composite_identity(id).unwrap();
    let before_archive = database.lock().await.get_composite_archive(id).unwrap();
    database.lock().await.set_favorite(id, true).unwrap();
    let before_row =
        serde_json::to_value(database.lock().await.get_all(None, None, 10, 0).unwrap()).unwrap();
    let analysis = analyze_entry(database.clone(), id, Arc::new(UnsupportedNativeAnalyzer))
        .await
        .unwrap();
    assert!(analysis.native_only);
    let after_identity = database.lock().await.composite_identity(id).unwrap();
    let after_archive = database.lock().await.get_composite_archive(id).unwrap();
    assert_eq!(before_identity, after_identity);
    assert_eq!(before_archive, after_archive);
    let after_row =
        serde_json::to_value(database.lock().await.get_all(None, None, 10, 0).unwrap()).unwrap();
    assert_eq!(before_row, after_row);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deleted_entry_is_rejected_as_stale() {
    let (_path, database) = database();
    let id = store(&database, &rtf_bundle("{\\rtf1 body}")).await;
    let (analyzer, entered) = GateAnalyzer::new();
    let analyzer = Arc::new(analyzer);
    let worker = {
        let database = database.clone();
        let analyzer = analyzer.clone();
        tokio::spawn(async move { analyze_entry(database, id, analyzer).await })
    };
    entered.await.unwrap();
    database.lock().await.delete(id).unwrap();
    analyzer.release();
    let outcome = worker.await.unwrap();
    assert!(
        matches!(outcome, Err(ContentAnalysisError::Stale)),
        "a deleted entry must not yield a consumable analysis: {outcome:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_stops_the_worker_before_it_returns_facts() {
    let (_path, database) = database();
    let id = store(&database, &rtf_bundle("{\\rtf1 body}")).await;
    let (analyzer, entered) = GateAnalyzer::new();
    let analyzer = Arc::new(analyzer);
    let cancellation = Cancellation::default();
    let worker = {
        let database = database.clone();
        let analyzer = analyzer.clone();
        let cancellation = cancellation.clone();
        tokio::spawn(async move {
            analyze_entry_cancellable(database, id, analyzer, cancellation).await
        })
    };
    entered.await.unwrap();
    cancellation.cancel();
    analyzer.release();
    let outcome = worker.await.unwrap();
    assert!(
        matches!(outcome, Err(ContentAnalysisError::Cancelled)),
        "a cancelled analysis must not return facts: {outcome:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_second_analysis_is_refused_while_one_is_running() {
    let (_path, database) = database();
    let id = store(&database, &rtf_bundle("{\\rtf1 gate}")).await;
    let (analyzer, entered) = GateAnalyzer::new();
    let analyzer = Arc::new(analyzer);
    let first = {
        let database = database.clone();
        let analyzer = analyzer.clone();
        tokio::spawn(async move { analyze_entry(database, id, analyzer).await })
    };
    entered.await.unwrap();
    let second = analyze_entry(database.clone(), id, analyzer.clone()).await;
    assert!(
        matches!(second, Err(ContentAnalysisError::Busy)),
        "concurrent native analysis must be refused: {second:?}"
    );
    analyzer.release();
    let _ = first.await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn multi_item_archive_keeps_item_boundaries() {
    let (_path, database) = database();
    let archive = NativeArchive {
        platform: 1,
        enumeration_complete: true,
        original: NativeSnapshot {
            before: 1,
            after: 1,
            items: 2,
            state: SnapshotState::Stable,
            formats: vec![
                NativeFormat {
                    item: 0,
                    name: "public.utf8-plain-text".into(),
                    state: FormatState::Captured,
                    bytes: b"first item".to_vec(),
                },
                NativeFormat {
                    item: 1,
                    name: "public.png".into(),
                    state: FormatState::Captured,
                    bytes: png(0x55),
                },
            ],
        },
        derived: vec![],
    };
    let id = {
        let mut guard = database.lock().await;
        guard.add_native_archive(&archive, "s2-02").unwrap()
    };
    let analysis = analyze_entry(database.clone(), id, Arc::new(UnsupportedNativeAnalyzer))
        .await
        .unwrap();
    assert_eq!(analysis.items.len(), 2);
    assert_eq!(analysis.items[0].item, 0);
    assert_eq!(analysis.items[1].item, 1);
    assert!(analysis.items[1]
        .formats
        .iter()
        .any(|format| format == "public.png"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn helper_facts_replace_the_native_summary_for_a_native_only_capture() {
    let (_path, database) = database();
    let archive = NativeArchive {
        platform: 1,
        enumeration_complete: true,
        original: NativeSnapshot {
            before: 2,
            after: 2,
            items: 1,
            state: SnapshotState::Stable,
            formats: vec![NativeFormat {
                item: 0,
                name: "com.apple.flat-rtfd".into(),
                state: FormatState::Captured,
                bytes: b"rtfd".to_vec(),
            }],
        },
        derived: vec![],
    };
    let id = {
        let mut guard = database.lock().await;
        guard.add_native_archive(&archive, "s2-02").unwrap()
    };
    let analysis = analyze_entry(
        database.clone(),
        id,
        Arc::new(ScriptedAnalyzer::rtfd("RTFD 中文\u{fffc}", 7)),
    )
    .await
    .unwrap();
    assert!(analysis.native_only);
    assert_eq!(analysis.text.text, "RTFD 中文\u{fffc}");
    assert_eq!(analysis.first_image, Some(0));
    assert!(!analysis.unknown);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_entry_is_reported_as_unavailable() {
    let (_path, database) = database();
    let outcome = analyze_entry(database.clone(), 4242, Arc::new(UnsupportedNativeAnalyzer)).await;
    assert!(matches!(outcome, Err(ContentAnalysisError::Unavailable)));
}

struct GateAnalyzer {
    entered: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    release: Mutex<std::sync::mpsc::Receiver<()>>,
    release_tx: std::sync::mpsc::Sender<()>,
}

impl GateAnalyzer {
    fn new() -> (Self, tokio::sync::oneshot::Receiver<()>) {
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release) = std::sync::mpsc::channel();
        (
            Self {
                entered: Mutex::new(Some(entered_tx)),
                release: Mutex::new(release),
                release_tx,
            },
            entered_rx,
        )
    }
    fn release(&self) {
        let _ = self.release_tx.send(());
    }
}

impl NativeAnalyzer for GateAnalyzer {
    fn analyze(
        &self,
        _task: &NativeAnalysisTask,
        _cancelled: &AtomicBool,
    ) -> Result<NativeAnalysisFacts, String> {
        if let Some(sender) = self.entered.lock().unwrap().take() {
            let _ = sender.send(());
        }
        let _ = self.release.lock().unwrap().recv();
        Err("gate released".into())
    }
}

struct ScriptedAnalyzer {
    body: String,
    position: u32,
}

impl ScriptedAnalyzer {
    fn rtfd(body: &str, position: u32) -> Self {
        Self {
            body: body.to_string(),
            position,
        }
    }
}

impl NativeAnalyzer for ScriptedAnalyzer {
    fn analyze(
        &self,
        _task: &NativeAnalysisTask,
        _cancelled: &AtomicBool,
    ) -> Result<NativeAnalysisFacts, String> {
        Ok(NativeAnalysisFacts {
            kind: tailsync_core::protocol::content_analysis::SavedAnalysisKind::Rtfd,
            item: 0,
            source_format: "com.apple.flat-rtfd".into(),
            converter: "tailsync.rtfd-to-facts.v1".into(),
            body: self.body.clone(),
            truncated: false,
            partial: false,
            style_bits: tailsync_core::protocol::content_analysis::STYLE_BOLD,
            table_rows: 1,
            table_cells: 2,
            attachments: vec![
                tailsync_core::composite::content_analysis::NativeAttachment {
                    utf16_position: self.position,
                    state: tailsync_core::protocol::content_analysis::AttachmentState::Available,
                    png: png(0x66),
                },
            ],
        })
    }
}
