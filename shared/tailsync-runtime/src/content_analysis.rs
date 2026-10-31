use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tailsync_core::cancellation::Cancellation;
use tailsync_core::composite::budgets::{global, PipelineLease};
use tailsync_core::composite::content_analysis::{
    analyze, AnalysisError, AnalysisIdentity, AnalysisRequest, ContentAnalysis,
    NativeAnalysisFacts, NativeItemFact, RepresentationSource,
};
use tailsync_core::composite::Mime;
use tailsync_core::db::{AnalysisHandle, ContentAnalysisInput, HistoryDB};
use tailsync_core::protocol::content_analysis::SavedAnalysisKind;
use tailsync_core::protocol::native_clipboard::FormatState;
use tokio::sync::{Mutex, Semaphore};

pub const NATIVE_ANALYSIS_DEADLINE: Duration = Duration::from_secs(4);
const ANALYSIS_WORKSPACE_BYTES: u64 = 192 * 1024 * 1024;

pub struct AnalyzedContent {
    facts: ContentAnalysis,
    handle: AnalysisHandle,
    _memory: PipelineLease,
}

impl std::ops::Deref for AnalyzedContent {
    type Target = ContentAnalysis;
    fn deref(&self) -> &ContentAnalysis {
        &self.facts
    }
}

impl std::fmt::Debug for AnalyzedContent {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.facts.fmt(formatter)
    }
}

impl AnalyzedContent {
    pub fn document(&self) -> Option<&tailsync_core::composite::display_document::DisplayDocument> {
        self.facts.document.as_ref()
    }
    pub fn take_document(
        &mut self,
    ) -> Option<tailsync_core::composite::display_document::DisplayDocument> {
        self.facts.document.take()
    }

    pub fn handle(&self) -> &AnalysisHandle {
        &self.handle
    }

    pub fn commit_display(
        &self,
        database: &mut HistoryDB,
        cancellation: &Cancellation,
    ) -> Result<bool, ContentAnalysisError> {
        database
            .commit_display_classification(
                &self.handle,
                &tailsync_core::composite::display::classify(&self.facts),
                cancellation,
            )
            .map_err(|error| ContentAnalysisError::Operation(error.to_string()))
    }

    pub fn is_current(&self, database: &HistoryDB) -> Result<bool, ContentAnalysisError> {
        database
            .verify_content_analysis(&self.handle)
            .map_err(|error| ContentAnalysisError::Operation(error.to_string()))
    }
}

#[derive(Debug)]
pub enum ContentAnalysisError {
    Unavailable,
    Busy,
    Cancelled,
    Timeout,
    Stale,
    Invalid,
    Operation(String),
}

impl std::fmt::Display for ContentAnalysisError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable => formatter.write_str("content analysis unavailable"),
            Self::Busy => formatter.write_str("content analysis busy"),
            Self::Cancelled => formatter.write_str("content analysis cancelled"),
            Self::Timeout => formatter.write_str("content analysis timed out"),
            Self::Stale => formatter.write_str("content analysis is stale"),
            Self::Invalid => formatter.write_str("content analysis rejected invalid input"),
            Self::Operation(detail) => write!(formatter, "content analysis failed: {detail}"),
        }
    }
}

impl std::error::Error for ContentAnalysisError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeAnalysisTask {
    pub kind: SavedAnalysisKind,
    pub item: u16,
    pub source_format: String,
    pub bytes: Vec<u8>,
}

pub trait NativeAnalyzer: Send + Sync {
    fn analyze(
        &self,
        task: &NativeAnalysisTask,
        cancelled: &AtomicBool,
    ) -> Result<NativeAnalysisFacts, String>;
}

pub struct UnsupportedNativeAnalyzer;

impl NativeAnalyzer for UnsupportedNativeAnalyzer {
    fn analyze(
        &self,
        _task: &NativeAnalysisTask,
        _cancelled: &AtomicBool,
    ) -> Result<NativeAnalysisFacts, String> {
        Err("native analysis helper is unavailable".into())
    }
}

static ANALYSIS_PERMITS: OnceLock<Arc<Semaphore>> = OnceLock::new();

fn permits() -> Arc<Semaphore> {
    ANALYSIS_PERMITS
        .get_or_init(|| Arc::new(Semaphore::new(1)))
        .clone()
}

struct CancellationGuard {
    cancelled: Arc<AtomicBool>,
    cancellation: Cancellation,
    watcher: tokio::task::JoinHandle<()>,
}

impl Drop for CancellationGuard {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.cancellation.cancel();
        self.watcher.abort();
    }
}

pub async fn analyze_entry(
    database: Arc<Mutex<HistoryDB>>,
    entry_id: i64,
    analyzer: Arc<dyn NativeAnalyzer>,
) -> Result<AnalyzedContent, ContentAnalysisError> {
    analyze_entry_cancellable(database, entry_id, analyzer, Cancellation::default()).await
}

pub async fn analyze_entry_cancellable(
    database: Arc<Mutex<HistoryDB>>,
    entry_id: i64,
    analyzer: Arc<dyn NativeAnalyzer>,
    cancellation: Cancellation,
) -> Result<AnalyzedContent, ContentAnalysisError> {
    analyze_entry_impl(database, entry_id, analyzer, cancellation, false).await
}

pub async fn analyze_document_cancellable(
    database: Arc<Mutex<HistoryDB>>,
    entry_id: i64,
    analyzer: Arc<dyn NativeAnalyzer>,
    cancellation: Cancellation,
) -> Result<AnalyzedContent, ContentAnalysisError> {
    analyze_entry_impl(database, entry_id, analyzer, cancellation, true).await
}

async fn analyze_entry_impl(
    database: Arc<Mutex<HistoryDB>>,
    entry_id: i64,
    analyzer: Arc<dyn NativeAnalyzer>,
    cancellation: Cancellation,
    with_document: bool,
) -> Result<AnalyzedContent, ContentAnalysisError> {
    if cancellation.is_cancelled() {
        return Err(ContentAnalysisError::Cancelled);
    }
    let permit = permits()
        .try_acquire_owned()
        .map_err(|_| ContentAnalysisError::Busy)?;
    let mut memory = global()
        .reserve_pipeline(ANALYSIS_WORKSPACE_BYTES)
        .map_err(|_| ContentAnalysisError::Busy)?;
    let deadline = Instant::now() + NATIVE_ANALYSIS_DEADLINE;
    let worker_cancellation = Cancellation::default();
    let cancelled = Arc::new(AtomicBool::new(false));
    let watcher = {
        let flag = cancelled.clone();
        let cancellation = cancellation.clone();
        let worker_cancellation = worker_cancellation.clone();
        tokio::spawn(async move {
            tokio::select! {
                _ = cancellation.cancelled() => {},
                _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => {},
            }
            flag.store(true, Ordering::SeqCst);
            worker_cancellation.cancel();
        })
    };
    let _guard = CancellationGuard {
        cancelled: cancelled.clone(),
        cancellation: worker_cancellation.clone(),
        watcher,
    };
    let worker_cancelled = cancelled.clone();
    let worker = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let (facts, handle) = run_analysis(
            &database,
            entry_id,
            analyzer.as_ref(),
            &worker_cancelled,
            &worker_cancellation,
            deadline,
            with_document,
        )?;
        memory.shrink_to(
            (facts.text.text.capacity()
                + facts
                    .images
                    .iter()
                    .map(|image| image.png.capacity())
                    .sum::<usize>()
                + facts
                    .document
                    .as_ref()
                    .map(|document| document.retained_bytes())
                    .unwrap_or(0)
                + 65536) as u64,
        );
        Ok(AnalyzedContent {
            facts,
            handle,
            _memory: memory,
        })
    });
    worker
        .await
        .map_err(|error| ContentAnalysisError::Operation(error.to_string()))?
}

fn run_analysis(
    database: &Mutex<HistoryDB>,
    entry_id: i64,
    analyzer: &dyn NativeAnalyzer,
    cancelled: &AtomicBool,
    cancellation: &Cancellation,
    deadline: Instant,
    with_document: bool,
) -> Result<(ContentAnalysis, AnalysisHandle), ContentAnalysisError> {
    let (prepared, handle) = {
        let guard = lock_database(database, cancellation, deadline)?;
        let prepared = guard
            .prepare_content_analysis(entry_id)
            .map_err(|error| ContentAnalysisError::Operation(error.to_string()))?;
        let handle = prepared.as_ref().map(|prepared| prepared.handle());
        (prepared, handle)
    };
    let Some(prepared) = prepared else {
        return Err(ContentAnalysisError::Unavailable);
    };
    let handle = handle.ok_or(ContentAnalysisError::Invalid)?;
    check(cancellation, cancelled, deadline)?;

    let native_only = prepared.native_only();
    let input = prepared
        .read_input(cancellation)
        .map_err(|error| ContentAnalysisError::Operation(error.to_string()))?;
    check(cancellation, cancelled, deadline)?;

    let tasks = native_tasks(&input);
    let mut native = Vec::with_capacity(tasks.len());
    let primary = tasks.into_iter().min_by_key(|task| {
        (
            task.item,
            if task.kind == SavedAnalysisKind::Rtfd {
                0
            } else {
                1
            },
        )
    });
    for task in primary.into_iter() {
        check(cancellation, cancelled, deadline)?;
        match analyzer.analyze(&task, cancelled) {
            Ok(facts) => {
                let source_matches = facts.source_format == task.source_format
                    || (task.source_format == "Rich Text Format"
                        && facts.source_format == "public.rtf");
                let converter_matches = match task.kind {
                    SavedAnalysisKind::Rtf => matches!(
                        facts.converter.as_str(),
                        "tailsync.rtf-to-facts.v1" | "tailsync.rtf-scan.v1"
                    ),
                    SavedAnalysisKind::Rtfd => facts.converter == "tailsync.rtfd-to-facts.v1",
                };
                if facts.kind != task.kind
                    || facts.item != task.item
                    || !source_matches
                    || !converter_matches
                {
                    return Err(ContentAnalysisError::Invalid);
                }
                let saved = tailsync_core::protocol::content_analysis::SavedTextAnalysis {
                    kind: facts.kind,
                    source_format: facts.source_format.clone(),
                    converter: facts.converter.clone(),
                    body: facts.body.clone(),
                    truncated: facts.truncated,
                    partial: facts.partial,
                    style_bits: facts.style_bits,
                    table_rows: facts.table_rows,
                    table_cells: facts.table_cells,
                    attachments: facts
                        .attachments
                        .iter()
                        .map(|attachment| {
                            tailsync_core::protocol::content_analysis::SavedAttachment {
                                utf16_position: attachment.utf16_position,
                                state: attachment.state,
                                png: attachment.png.clone(),
                            }
                        })
                        .collect(),
                };
                saved
                    .validate()
                    .map_err(|_| ContentAnalysisError::Invalid)?;
                native.push(facts);
            }
            Err(_) => {
                tracing::debug!(target: "tailsync.runtime.content_analysis", item = task.item,
                    kind = ?task.kind, outcome = "native_unavailable",
                    "native analysis helper did not produce facts");
            }
        }
    }
    check(cancellation, cancelled, deadline)?;

    let representations = provenance(input.archive.as_ref(), &input.bundle);
    let items = native_items(&input);
    let analysis_identity = AnalysisIdentity {
        entry_id: handle.entry_id,
        authority: handle.authority.clone(),
        generation: handle.generation,
        owner: handle.owner.clone(),
        capture_id: handle
            .identity
            .as_ref()
            .map(|value| value.capture_id.clone())
            .unwrap_or_default(),
        content_id: handle
            .identity
            .as_ref()
            .map(|value| value.content_id.clone())
            .unwrap_or_default(),
        archive_revision: handle
            .identity
            .as_ref()
            .map(|value| value.archive_revision)
            .unwrap_or(0),
        archive_digest: handle
            .identity
            .as_ref()
            .map(|value| value.archive_digest.clone())
            .unwrap_or_default(),
    };
    let request = AnalysisRequest {
        bundle: input.bundle,
        provenance: representations,
        native_only,
        native,
        native_items: items,
        decode_images: true,
        document: with_document,
    };
    let analysis = analyze(analysis_identity, &request, cancellation, deadline).map_err(
        |error| match error {
            AnalysisError::Cancelled => ContentAnalysisError::Cancelled,
            AnalysisError::Deadline => ContentAnalysisError::Timeout,
            AnalysisError::Invalid => ContentAnalysisError::Invalid,
        },
    )?;

    let verified = {
        let guard = lock_database(database, cancellation, deadline)?;
        guard
            .verify_content_analysis(&handle)
            .map_err(|error| ContentAnalysisError::Operation(error.to_string()))?
    };
    if !verified {
        return Err(ContentAnalysisError::Stale);
    }
    check(cancellation, cancelled, deadline)?;
    Ok((analysis, handle))
}

fn lock_database<'a>(
    database: &'a Mutex<HistoryDB>,
    cancellation: &Cancellation,
    deadline: Instant,
) -> Result<tokio::sync::MutexGuard<'a, HistoryDB>, ContentAnalysisError> {
    tokio::runtime::Handle::current().block_on(async {
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => Err(ContentAnalysisError::Cancelled),
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => Err(ContentAnalysisError::Timeout),
            guard = database.lock() => Ok(guard),
        }
    })
}

fn check(
    cancellation: &Cancellation,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), ContentAnalysisError> {
    if Instant::now() >= deadline {
        return Err(ContentAnalysisError::Timeout);
    }
    if cancellation.is_cancelled() || cancelled.load(Ordering::SeqCst) {
        return Err(ContentAnalysisError::Cancelled);
    }
    Ok(())
}

fn format_name(mime: Mime) -> &'static str {
    match mime {
        Mime::Text => "public.utf8-plain-text",
        Mime::Html => "public.html",
        Mime::Rtf => "public.rtf",
        Mime::Png => "public.png",
    }
}

fn provenance(
    archive: Option<&tailsync_core::protocol::native_archive::NativeArchive>,
    bundle: &tailsync_core::composite::PortableBundle,
) -> Vec<RepresentationSource> {
    use tailsync_core::protocol::native_archive::{
        CONVERTER_QQ_TO_PORTABLE, CONVERTER_RTFD_TO_PORTABLE, QQ_MESSAGE_FORMAT,
    };
    // Inline resource extraction and CF_HTML decoding legitimately change the
    // bytes. Re-run the admitted projection and compare the entire bundle,
    // including image bytes and bindings. Do this once, only when raw equality
    // cannot establish provenance (QQ always needs the full projection proof).
    let projection_verified = std::cell::OnceCell::new();
    let verifies_projection = || {
        *projection_verified.get_or_init(|| {
            archive.is_some_and(|archive| {
                tailsync_core::composite::from_archive(archive).is_ok_and(|projected| {
                    projected == *bundle
                        || tailsync_core::composite::resource_archive::validate_archive_derivation(
                            &projected, bundle,
                        )
                        .is_ok()
                })
            })
        })
    };
    bundle
        .representations
        .iter()
        .map(|representation| {
            let name = format_name(representation.mime);
            let derived = archive.and_then(|archive| {
                archive.derived.iter().find(|derived| {
                    derived.item == 0
                        && derived.name == name
                        && if derived.converter == CONVERTER_QQ_TO_PORTABLE {
                            derived.source_name == QQ_MESSAGE_FORMAT && verifies_projection()
                        } else {
                            derived.bytes == representation.bytes
                                || (derived.name == "public.html"
                                    && derived.source_name == "com.apple.flat-rtfd"
                                    && derived.converter == CONVERTER_RTFD_TO_PORTABLE
                                    && verifies_projection())
                        }
                        && archive.original.formats.iter().any(|format| {
                            format.item == derived.item
                                && format.name == derived.source_name
                                && format.state == FormatState::Captured
                        })
                })
            });
            match derived {
                Some(derived) => RepresentationSource::Derived {
                    converter: derived.converter.clone(),
                    source_name: derived.source_name.clone(),
                },
                None if archive.is_none()
                    || archive.is_some_and(|archive| {
                        archive.original.formats.iter().any(|format| {
                            format.item == 0
                                && format.name == name
                                && format.state == FormatState::Captured
                                && format.bytes == representation.bytes
                        })
                    }) =>
                {
                    RepresentationSource::Original
                }
                None if verifies_projection() => archive
                    .and_then(|archive| {
                        archive.original.formats.iter().find(|format| {
                            format.item == 0
                                && format.state == FormatState::Captured
                                && matches!(
                                    (representation.mime, format.name.as_str()),
                                    (Mime::Text, "public.utf8-plain-text" | "CF_UNICODETEXT")
                                        | (Mime::Html, "public.html" | "HTML Format")
                                        | (Mime::Rtf, "public.rtf" | "Rich Text Format")
                                        | (Mime::Png, "public.png" | "PNG")
                                )
                        })
                    })
                    .map(|format| RepresentationSource::Derived {
                        converter: "tailsync.native-to-portable.v1".into(),
                        source_name: format.name.clone(),
                    })
                    .unwrap_or(RepresentationSource::Unverified),
                None => RepresentationSource::Unverified,
            }
        })
        .collect()
}

fn native_items(input: &ContentAnalysisInput) -> Vec<NativeItemFact> {
    let snapshot = input
        .archive
        .as_ref()
        .map(|archive| &archive.original)
        .or(input.native_snapshot.as_ref());
    let Some(snapshot) = snapshot else {
        return vec![];
    };
    (0..snapshot.items)
        .map(|item| NativeItemFact {
            item,
            formats: snapshot
                .formats
                .iter()
                .filter(|format| {
                    format.item == item
                        && format.state == FormatState::Captured
                        && !tailsync_core::protocol::native_policy::control(&format.name)
                })
                .map(|format| format.name.clone())
                .collect(),
        })
        .collect()
}

fn native_tasks(input: &ContentAnalysisInput) -> Vec<NativeAnalysisTask> {
    let mut tasks = vec![];
    let snapshot = input
        .archive
        .as_ref()
        .map(|archive| &archive.original)
        .or(input.native_snapshot.as_ref());
    if let Some(snapshot) = snapshot {
        for format in &snapshot.formats {
            if format.state != FormatState::Captured {
                continue;
            }
            let kind = match format.name.as_str() {
                "com.apple.flat-rtfd" => SavedAnalysisKind::Rtfd,
                "public.rtf" | "Rich Text Format" => SavedAnalysisKind::Rtf,
                _ => continue,
            };
            tasks.push(NativeAnalysisTask {
                kind,
                item: format.item,
                source_format: format.name.clone(),
                bytes: format.bytes.clone(),
            });
        }
    }
    if tasks.is_empty() {
        if let Some(rtf) = input.bundle.representation(Mime::Rtf) {
            tasks.push(NativeAnalysisTask {
                kind: SavedAnalysisKind::Rtf,
                item: 0,
                source_format: "public.rtf".into(),
                bytes: rtf.to_vec(),
            });
        }
    }
    tasks
}

#[cfg(test)]
mod tests {
    use super::*;
    use tailsync_core::composite::Completeness;
    use tailsync_core::protocol::native_archive::NativeArchive;
    use tailsync_core::protocol::native_clipboard::{NativeFormat, NativeSnapshot, SnapshotState};

    fn input(archive: NativeArchive) -> ContentAnalysisInput {
        ContentAnalysisInput {
            bundle: tailsync_core::composite::PortableBundle {
                representations: vec![],
                resources: vec![],
                bindings: vec![],
                completeness: Completeness::Complete,
            },
            archive: Some(archive),
            native_snapshot: None,
        }
    }

    #[test]
    fn native_tasks_use_saved_rtfd_bytes_and_keep_item_boundaries() {
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
                        name: "com.apple.flat-rtfd".into(),
                        state: FormatState::Captured,
                        bytes: b"rtfd-0".to_vec(),
                    },
                    NativeFormat {
                        item: 1,
                        name: "public.rtf".into(),
                        state: FormatState::Captured,
                        bytes: b"rtf-1".to_vec(),
                    },
                ],
            },
            derived: vec![],
        };
        let tasks = native_tasks(&input(archive));
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].kind, SavedAnalysisKind::Rtfd);
        assert_eq!(tasks[0].item, 0);
        assert_eq!(tasks[1].kind, SavedAnalysisKind::Rtf);
        assert_eq!(tasks[1].item, 1);
    }

    #[test]
    fn provenance_requires_a_captured_source_for_the_derived_item() {
        use tailsync_core::protocol::native_archive::DerivedFormat;
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
                    bytes: b"rtfd".to_vec(),
                }],
            },
            derived: vec![DerivedFormat {
                item: 0,
                name: "public.html".into(),
                source_name: "com.apple.flat-rtfd".into(),
                converter: "tailsync.rtfd-to-portable.v1".into(),
                bytes: b"<p>x</p>".to_vec(),
            }],
        };
        let bundle = tailsync_core::composite::PortableBundle {
            representations: vec![tailsync_core::composite::Representation {
                mime: Mime::Html,
                bytes: b"<p>x</p>".to_vec(),
            }],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::Complete,
        };
        let sources = provenance(Some(&archive), &bundle);
        assert!(matches!(
            sources.as_slice(),
            [RepresentationSource::Derived { converter, .. }] if converter == "tailsync.rtfd-to-portable.v1"
        ));

        let mut orphan = archive.clone();
        orphan.original.formats.clear();
        let sources = provenance(Some(&orphan), &bundle);
        assert!(matches!(
            sources.as_slice(),
            [RepresentationSource::Unverified]
        ));
    }

    #[test]
    fn qq_resource_extraction_proves_the_whole_message_and_rejects_substitution() {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        use tailsync_core::protocol::native_archive::{
            DerivedFormat, CONVERTER_QQ_TO_PORTABLE, QQ_MESSAGE_FORMAT,
        };
        let png = tailsync_core::composite::packed_to_png(1, 1, &[255, 0, 0, 255]).unwrap();
        let html = format!(
            "<p>QQ 中文</p><img src='data:image/png;base64,{}'>",
            STANDARD.encode(png)
        );
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
                    name: QQ_MESSAGE_FORMAT.into(),
                    state: FormatState::Captured,
                    bytes: b"retained QQ source".to_vec(),
                }],
            },
            derived: [
                ("public.html", html),
                ("public.utf8-plain-text", "QQ 中文".into()),
            ]
            .into_iter()
            .map(|(name, bytes)| DerivedFormat {
                item: 0,
                name: name.into(),
                source_name: QQ_MESSAGE_FORMAT.into(),
                converter: CONVERTER_QQ_TO_PORTABLE.into(),
                bytes: bytes.into_bytes(),
            })
            .collect(),
        };
        let bundle = tailsync_core::composite::from_archive(&archive).unwrap();
        assert_ne!(
            bundle.representation(Mime::Html).unwrap(),
            archive.derived[0].bytes
        );
        assert!(provenance(Some(&archive), &bundle).iter().all(
            |source| matches!(source, RepresentationSource::Derived { converter, source_name }
                if converter == CONVERTER_QQ_TO_PORTABLE && source_name == QQ_MESSAGE_FORMAT)
        ));
        let mut substituted = bundle.clone();
        substituted.resources[0].png =
            tailsync_core::composite::packed_to_png(1, 1, &[0, 0, 255, 255]).unwrap();
        assert!(provenance(Some(&archive), &substituted)
            .iter()
            .all(|source| *source == RepresentationSource::Unverified));
        let mut substituted = bundle.clone();
        substituted
            .representations
            .iter_mut()
            .find(|r| r.mime == Mime::Text)
            .unwrap()
            .bytes = b"substituted text".to_vec();
        assert!(provenance(Some(&archive), &substituted)
            .iter()
            .all(|source| *source == RepresentationSource::Unverified));
        let mut incomplete = archive.clone();
        incomplete.enumeration_complete = false;
        assert!(provenance(Some(&incomplete), &bundle)
            .iter()
            .all(|source| *source == RepresentationSource::Unverified));
    }

    fn inline_html_archive(windows: bool) -> NativeArchive {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        let html = format!(
            "<p>body 中文</p><img src='data:image/png;base64,{}'>",
            STANDARD
                .encode(tailsync_core::composite::packed_to_png(1, 1, &[255, 0, 0, 255]).unwrap())
        );
        NativeArchive {
            platform: if windows { 2 } else { 1 },
            enumeration_complete: true,
            original: NativeSnapshot {
                before: 1,
                after: 1,
                items: 1,
                state: SnapshotState::Stable,
                formats: vec![NativeFormat {
                    item: 0,
                    name: if windows {
                        "HTML Format"
                    } else {
                        "public.html"
                    }
                    .into(),
                    state: FormatState::Captured,
                    bytes: if windows {
                        tailsync_core::composite::html::encode_cf_html(&html).unwrap()
                    } else {
                        html.into_bytes()
                    },
                }],
            },
            derived: vec![],
        }
    }

    #[test]
    fn mac_html_resource_extraction_has_verified_provenance() {
        let archive = inline_html_archive(false);
        let bundle = tailsync_core::composite::from_archive(&archive).unwrap();
        assert_ne!(
            bundle.representation(Mime::Html).unwrap(),
            archive.original.formats[0].bytes
        );
        assert_eq!(bundle.resources.len(), 1);
        assert!(!matches!(
            provenance(Some(&archive), &bundle)[0],
            RepresentationSource::Unverified
        ));
    }

    #[test]
    fn windows_cf_html_normalization_has_verified_provenance() {
        let archive = inline_html_archive(true);
        let bundle = tailsync_core::composite::from_archive(&archive).unwrap();
        assert!(
            std::str::from_utf8(bundle.representation(Mime::Html).unwrap())
                .unwrap()
                .contains("body 中文")
        );
        assert!(!matches!(
            provenance(Some(&archive), &bundle)[0],
            RepresentationSource::Unverified
        ));
    }

    #[test]
    fn rtfd_html_resource_extraction_has_verified_provenance() {
        use tailsync_core::protocol::native_archive::{DerivedFormat, CONVERTER_RTFD_TO_PORTABLE};
        let mut archive = inline_html_archive(false);
        let html = archive.original.formats[0].bytes.clone();
        archive.original.formats[0].name = "com.apple.flat-rtfd".into();
        archive.original.formats[0].bytes = b"retained RTFD source".to_vec();
        archive.derived = [
            ("public.html", html),
            ("public.utf8-plain-text", "body 中文".as_bytes().to_vec()),
        ]
        .into_iter()
        .map(|(name, bytes)| DerivedFormat {
            item: 0,
            name: name.into(),
            source_name: "com.apple.flat-rtfd".into(),
            converter: CONVERTER_RTFD_TO_PORTABLE.into(),
            bytes,
        })
        .collect();
        let bundle = tailsync_core::composite::from_archive(&archive).unwrap();
        assert!(provenance(Some(&archive), &bundle).iter().all(|source| matches!(source,
            RepresentationSource::Derived { converter, .. } if converter == CONVERTER_RTFD_TO_PORTABLE)));
    }

    #[test]
    fn normalized_html_provenance_rejects_changed_body_resources_bindings_and_missing_source() {
        let archive = inline_html_archive(false);
        let bundle = tailsync_core::composite::from_archive(&archive).unwrap();
        let mut body = bundle.clone();
        body.representations[0].bytes = std::str::from_utf8(&body.representations[0].bytes)
            .unwrap()
            .replace("body", "evil")
            .into_bytes();
        let mut resource = bundle.clone();
        resource.resources[0].png =
            tailsync_core::composite::packed_to_png(1, 1, &[0, 0, 255, 255]).unwrap();
        let mut binding = bundle.clone();
        binding.bindings.clear();
        for substituted in [body, resource, binding] {
            assert!(matches!(
                provenance(Some(&archive), &substituted)[0],
                RepresentationSource::Unverified
            ));
        }
        let mut missing = archive.clone();
        missing.original.formats.clear();
        assert!(matches!(
            provenance(Some(&missing), &bundle)[0],
            RepresentationSource::Unverified
        ));
        let mut other_item = archive.clone();
        other_item.original.items = 2;
        other_item.original.formats[0].item = 1;
        assert!(matches!(
            provenance(Some(&other_item), &bundle)[0],
            RepresentationSource::Unverified
        ));
    }
}
