use crate::content_analysis::{analyze_document_cancellable, ContentAnalysisError, NativeAnalyzer};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant};
use tailsync_core::cancellation::Cancellation;
use tailsync_core::composite::budgets::{global, PipelineLease};
use tailsync_core::composite::display_document::{
    DisplayDocument, DocumentError, DOCUMENT_MAX_BYTES, DOCUMENT_MAX_METADATA_BYTES,
    DOCUMENT_MAX_MODEL_BYTES,
};
use tailsync_core::composite::document_build::document_asset_bytes;
use tailsync_core::db::{HistoryDB, PreviewBatchNavigation, PreviewErrorCode, PreviewErrorInfo};
use tokio::sync::{Mutex, Semaphore};

pub const DOCUMENT_RESPONSE_MAGIC: &[u8; 4] = b"TSDC";
pub const DOCUMENT_RESPONSE_VERSION: u8 = 1;
pub const DOCUMENT_HEADER_BYTES: usize = 9;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DocumentFrameMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[schemars(range(min = 1))]
    pub entry_id: i64,
    #[schemars(extend("enum" = ["document"]))]
    pub kind: String,
    #[schemars(length(min = 1))]
    pub name: String,
    #[schemars(length(min = 1))]
    pub stamp: String,
    #[schemars(extend("enum" = ["history", "favorites"]))]
    pub owner: String,
    #[schemars(range(max = 9007199254740991u64))]
    pub generation: u64,
    #[schemars(range(max = 4194304))]
    pub model_bytes: u64,
    #[schemars(range(max = 16777216))]
    pub size_bytes: u64,
    #[serde(skip)]
    #[schemars(skip)]
    pub document: DisplayDocument,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch: Option<PreviewBatchNavigation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
pub struct DocumentRequest {
    #[schemars(range(min = 1))]
    pub entry_id: i64,
    #[schemars(extend("enum" = ["history", "favorites"]))]
    pub owner: String,
    #[schemars(range(max = 9007199254740991u64))]
    pub generation: u64,
    #[schemars(length(min = 1, max = 128))]
    pub request_id: String,
}

impl DocumentRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.entry_id <= 0
            || !matches!(self.owner.as_str(), "history" | "favorites")
            || self.generation > 9_007_199_254_740_991
            || self.request_id.is_empty()
            || self.request_id.len() > 128
        {
            return Err("invalid document request".into());
        }
        Ok(())
    }
}

struct ActiveDocument {
    request: DocumentRequest,
    cancellation: Cancellation,
    stamp: Option<String>,
}

#[derive(Default)]
pub struct DocumentSession {
    active: StdMutex<Option<ActiveDocument>>,
    generations: StdMutex<[Option<u64>; 2]>,
}

impl DocumentSession {
    pub fn start(&self, request: &DocumentRequest) -> Result<Cancellation, String> {
        request.validate()?;
        let owner = usize::from(request.owner == "favorites");
        let mut generations = self
            .generations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if generations[owner].is_some_and(|previous| request.generation <= previous) {
            return Err("stale document generation".into());
        }
        generations[owner] = Some(request.generation);
        let mut active = self
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(previous) = active.take() {
            previous.cancellation.cancel();
        }
        let cancellation = Cancellation::default();
        *active = Some(ActiveDocument {
            request: request.clone(),
            cancellation: cancellation.clone(),
            stamp: None,
        });
        Ok(cancellation)
    }

    pub fn finish(&self, request: &DocumentRequest, stamp: &str) -> bool {
        let mut active = self
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(active) = active
            .as_mut()
            .filter(|active| active.request == *request && !active.cancellation.is_cancelled())
        else {
            return false;
        };
        active.stamp = Some(stamp.to_string());
        true
    }

    pub fn matches(&self, request: &DocumentRequest, stamp: &str) -> bool {
        self.active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .is_some_and(|active| {
                active.request == *request
                    && active.stamp.as_deref() == Some(stamp)
                    && !active.cancellation.is_cancelled()
            })
    }

    pub fn cancel(&self, request: &DocumentRequest) {
        if request.validate().is_err() {
            return;
        }
        let mut generations = self
            .generations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let owner = usize::from(request.owner == "favorites");
        generations[owner] = Some(generations[owner].unwrap_or(0).max(request.generation));
        let mut active = self
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if active
            .as_ref()
            .is_some_and(|active| active.request == *request)
        {
            if let Some(active) = active.as_mut() {
                active.cancellation.cancel();
                active.stamp = None;
            }
        }
    }
}

impl Drop for DocumentSession {
    fn drop(&mut self) {
        if let Some(active) = self
            .active
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            active.cancellation.cancel();
        }
    }
}

pub async fn validate_document_install(
    database: Arc<Mutex<HistoryDB>>,
    session: &DocumentSession,
    request: &DocumentRequest,
    stamp: &str,
) -> bool {
    if request.validate().is_err() || !session.matches(request, stamp) {
        return false;
    }
    let Ok(database) = tokio::time::timeout(Duration::from_secs(4), database.lock()).await else {
        return false;
    };
    let current = database
        .document_key(request.entry_id)
        .is_ok_and(|key| key == stamp)
        && database.document_owner_current(request.entry_id, &request.owner);
    current && session.matches(request, stamp)
}

struct CancellationScope {
    cancellation: Cancellation,
    watcher: tokio::task::JoinHandle<()>,
}

impl CancellationScope {
    fn new(caller: Cancellation, deadline: Instant) -> Self {
        let cancellation = Cancellation::default();
        if caller.is_cancelled() {
            cancellation.cancel();
        }
        let worker = cancellation.clone();
        let watcher = tokio::spawn(async move {
            tokio::select! { _ = caller.cancelled() => {}, _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => {}, }
            worker.cancel();
        });
        Self {
            cancellation,
            watcher,
        }
    }
}

impl Drop for CancellationScope {
    fn drop(&mut self) {
        self.cancellation.cancel();
        self.watcher.abort();
    }
}

struct BoundedBuffer {
    bytes: Vec<u8>,
    limit: usize,
}

impl std::io::Write for BoundedBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(std::io::Error::other("document model exceeds limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn serialize_bounded(value: &impl Serialize, limit: usize) -> Result<Vec<u8>, String> {
    let mut buffer = BoundedBuffer {
        bytes: Vec::with_capacity(limit),
        limit,
    };
    serde_json::to_writer(&mut buffer, value)
        .map_err(|_| "document model exceeds limit".to_string())?;
    Ok(buffer.bytes)
}

fn validate_metadata(metadata: &DocumentFrameMetadata) -> Result<(), String> {
    DocumentRequest {
        entry_id: metadata.entry_id,
        owner: metadata.owner.clone(),
        generation: metadata.generation,
        request_id: metadata.request_id.clone().unwrap_or_default(),
    }
    .validate()?;
    if metadata.kind != "document"
        || metadata.name.is_empty()
        || metadata.name.len() > 256
        || metadata.stamp.len() != 68
        || !metadata.stamp.starts_with("dc1:")
        || !metadata.stamp[4..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || metadata.batch.is_some()
    {
        return Err("invalid document metadata".into());
    }
    Ok(())
}

fn payload_error(entry_id: i64, message: impl Into<String>) -> PreviewErrorInfo {
    PreviewErrorInfo::payload_unavailable(entry_id, message)
}

fn document_error(entry_id: i64, error: DocumentError) -> PreviewErrorInfo {
    match error {
        DocumentError::UnsupportedVersion => {
            payload_error(entry_id, "unsupported document version")
        }
        DocumentError::Invalid => payload_error(entry_id, "invalid document payload"),
    }
}

pub fn encode_document_response(
    mut metadata: DocumentFrameMetadata,
    payload: &[u8],
) -> Result<Vec<u8>, PreviewErrorInfo> {
    let entry_id = metadata.entry_id;
    validate_metadata(&metadata).map_err(|error| payload_error(entry_id, error))?;
    metadata
        .document
        .validate_assets(payload)
        .map_err(|error| document_error(entry_id, error))?;
    if metadata.size_bytes != payload.len() as u64 {
        return Err(payload_error(
            entry_id,
            "document payload length does not match the declared size",
        ));
    }
    let model = serialize_bounded(&metadata.document, DOCUMENT_MAX_MODEL_BYTES)
        .map_err(|error| payload_error(entry_id, error))?;
    metadata.model_bytes = model.len() as u64;
    metadata.size_bytes = metadata.model_bytes + payload.len() as u64;
    let metadata_bytes = serialize_bounded(&metadata, DOCUMENT_MAX_METADATA_BYTES)
        .map_err(|error| payload_error(entry_id, error))?;
    if metadata_bytes.is_empty() || metadata_bytes.len() > DOCUMENT_MAX_METADATA_BYTES {
        return Err(payload_error(entry_id, "document metadata is too large"));
    }
    let total = DOCUMENT_HEADER_BYTES
        .checked_add(metadata_bytes.len())
        .and_then(|length| length.checked_add(model.len()))
        .and_then(|length| length.checked_add(payload.len()))
        .ok_or_else(|| payload_error(entry_id, "document frame is too large"))?;
    if total > DOCUMENT_MAX_BYTES {
        return Err(PreviewErrorInfo {
            code: PreviewErrorCode::PreviewTooLarge,
            message: format!(
                "document payload is too large: {} bytes (limit {} bytes)",
                total, DOCUMENT_MAX_BYTES
            ),
            entry_id: Some(entry_id),
            size_bytes: Some(total as u64),
            limit_bytes: Some(DOCUMENT_MAX_BYTES as u64),
            retryable: false,
        });
    }
    let metadata_len = u32::try_from(metadata_bytes.len())
        .map_err(|_| payload_error(entry_id, "document metadata length overflows u32"))?;
    let mut frame = Vec::with_capacity(total);
    frame.extend_from_slice(DOCUMENT_RESPONSE_MAGIC);
    frame.push(DOCUMENT_RESPONSE_VERSION);
    frame.extend_from_slice(&metadata_len.to_le_bytes());
    frame.extend_from_slice(&metadata_bytes);
    frame.extend_from_slice(&model);
    frame.extend_from_slice(payload);
    Ok(frame)
}

pub fn decode_document_response(bytes: &[u8]) -> Result<(DocumentFrameMetadata, Vec<u8>), String> {
    if bytes.len() > DOCUMENT_MAX_BYTES {
        return Err("document frame exceeds limit".into());
    }
    if bytes.len() < DOCUMENT_HEADER_BYTES || &bytes[..4] != DOCUMENT_RESPONSE_MAGIC {
        return Err("invalid document frame magic".into());
    }
    if bytes[4] != DOCUMENT_RESPONSE_VERSION {
        return Err("unsupported document frame version".into());
    }
    let length = u32::from_le_bytes(bytes[5..9].try_into().map_err(|_| "bad length")?) as usize;
    if length == 0 || length > DOCUMENT_MAX_METADATA_BYTES {
        return Err("invalid document metadata length".into());
    }
    let end = DOCUMENT_HEADER_BYTES + length;
    if bytes.len() < end {
        return Err("truncated document frame".into());
    }
    let mut metadata: DocumentFrameMetadata =
        serde_json::from_slice(&bytes[DOCUMENT_HEADER_BYTES..end])
            .map_err(|error| error.to_string())?;
    validate_metadata(&metadata)?;
    if metadata.model_bytes == 0
        || metadata.model_bytes > DOCUMENT_MAX_MODEL_BYTES as u64
        || metadata.model_bytes > metadata.size_bytes
        || metadata.size_bytes > DOCUMENT_MAX_BYTES as u64
        || bytes.len() - end != metadata.size_bytes as usize
    {
        return Err("document payload length mismatch".into());
    }
    let model_end = end + metadata.model_bytes as usize;
    metadata.document = serde_json::from_slice(&bytes[end..model_end])
        .map_err(|_| "invalid document model".to_string())?;
    let payload = &bytes[model_end..];
    metadata
        .document
        .validate_assets(payload)
        .map_err(|_| "invalid document assets".to_string())?;
    Ok((metadata, payload.to_vec()))
}

#[derive(Debug)]
pub struct DocumentPreview {
    frame: Vec<u8>,
    pub stamp: String,
    pub entry_id: i64,
    document: DisplayDocument,
    _memory: PipelineLease,
}

impl DocumentPreview {
    pub fn bytes(&self) -> &[u8] {
        &self.frame
    }
    pub fn verify(&self, database: &HistoryDB) -> Result<bool, String> {
        match database.document_key(self.entry_id) {
            Ok(key) => Ok(key == self.stamp),
            Err(_) => Ok(false),
        }
    }

    pub fn document(&self) -> Option<&DisplayDocument> {
        Some(&self.document)
    }

    pub fn handoff<Body>(mut self, transfer: impl FnOnce(Vec<u8>) -> Body) -> Body {
        transfer(std::mem::take(&mut self.frame))
    }
}

fn analysis_error(entry_id: i64, error: ContentAnalysisError) -> PreviewErrorInfo {
    let retryable = matches!(
        error,
        ContentAnalysisError::Busy | ContentAnalysisError::Timeout
    );
    PreviewErrorInfo {
        code: PreviewErrorCode::PayloadUnavailable,
        message: match error {
            ContentAnalysisError::Busy => "content analysis busy",
            ContentAnalysisError::Timeout => "document preview timed out",
            ContentAnalysisError::Cancelled => "request cancelled",
            _ => "document preview unavailable",
        }
        .into(),
        entry_id: Some(entry_id),
        size_bytes: None,
        limit_bytes: None,
        retryable,
    }
}

async fn lock_cancellable<'store>(
    database: &'store Mutex<HistoryDB>,
    cancellation: &Cancellation,
    entry_id: i64,
) -> Result<tokio::sync::MutexGuard<'store, HistoryDB>, PreviewErrorInfo> {
    tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err(payload_error(entry_id, "request cancelled")),
        guard = database.lock() => Ok(guard),
    }
}

pub async fn document_preview_async(
    database: Arc<Mutex<HistoryDB>>,
    id: i64,
    request_id: Option<String>,
    analyzer: Arc<dyn NativeAnalyzer>,
    cancellation: Cancellation,
) -> Result<DocumentPreview, PreviewErrorInfo> {
    document_preview_checked_async(
        database,
        DocumentRequest {
            entry_id: id,
            owner: "history".into(),
            generation: 0,
            request_id: request_id.unwrap_or_else(|| "local".into()),
        },
        analyzer,
        cancellation,
    )
    .await
}

pub async fn document_preview_checked_async(
    database: Arc<Mutex<HistoryDB>>,
    request: DocumentRequest,
    analyzer: Arc<dyn NativeAnalyzer>,
    caller: Cancellation,
) -> Result<DocumentPreview, PreviewErrorInfo> {
    let id = request.entry_id;
    request
        .validate()
        .map_err(|error| payload_error(id, error))?;
    let deadline = Instant::now() + Duration::from_secs(4);
    let scope = CancellationScope::new(caller, deadline);
    let cancellation = scope.cancellation.clone();
    static PERMITS: OnceLock<Arc<Semaphore>> = OnceLock::new();
    let permit = PERMITS
        .get_or_init(|| Arc::new(Semaphore::new(1)))
        .clone()
        .try_acquire_owned()
        .map_err(|_| analysis_error(id, ContentAnalysisError::Busy))?;
    let stamp = {
        let guard = lock_cancellable(&database, &cancellation, id).await?;
        if guard.get_type(id).ok().as_deref() != Some("composite") {
            return Err(PreviewErrorInfo {
                code: PreviewErrorCode::UnsupportedType,
                message: "document preview requires a composite entry".into(),
                entry_id: Some(id),
                size_bytes: None,
                limit_bytes: None,
                retryable: false,
            });
        }
        if !guard.document_owner_current(id, &request.owner) {
            return Err(payload_error(id, "document owner unavailable"));
        }
        guard
            .document_key(id)
            .map_err(|_| payload_error(id, "document preview unavailable"))?
    };
    let analysis =
        analyze_document_cancellable(database.clone(), id, analyzer, cancellation.clone())
            .await
            .map_err(|error| analysis_error(id, error))?;
    let memory = global()
        .reserve_pipeline(64 * 1024 * 1024)
        .map_err(|_| analysis_error(id, ContentAnalysisError::Busy))?;
    let frame_stamp = stamp.clone();
    let final_owner = request.owner.clone();
    let worker_cancel = cancellation.clone();
    let (frame, document, memory) = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let mut analysis = analysis;
        worker_cancel
            .check()
            .map_err(|_| payload_error(id, "request cancelled"))?;
        let document = analysis
            .take_document()
            .ok_or_else(|| payload_error(id, "document analysis unavailable"))?;
        let payload = document_asset_bytes(&document, &analysis.images)
            .map_err(|error| document_error(id, error))?;
        worker_cancel
            .check()
            .map_err(|_| payload_error(id, "request cancelled"))?;
        let frame = encode_document_response(
            DocumentFrameMetadata {
                request_id: Some(request.request_id),
                entry_id: id,
                kind: "document".into(),
                name: "document".into(),
                stamp: frame_stamp,
                owner: request.owner,
                generation: request.generation,
                model_bytes: 0,
                size_bytes: payload.len() as u64,
                document: document.clone(),
                batch: None,
            },
            &payload,
        )?;
        worker_cancel
            .check()
            .map_err(|_| payload_error(id, "request cancelled"))?;
        if Instant::now() >= deadline {
            return Err(analysis_error(id, ContentAnalysisError::Timeout));
        }
        Ok::<_, PreviewErrorInfo>((frame, document, memory))
    })
    .await
    .map_err(|_| payload_error(id, "document worker unavailable"))??;
    cancellation
        .check()
        .map_err(|_| payload_error(id, "request cancelled"))?;
    let current = {
        let guard = lock_cancellable(&database, &cancellation, id).await?;
        if !guard.document_owner_current(id, &final_owner) {
            return Err(payload_error(id, "document owner unavailable"));
        }
        guard
            .document_key(id)
            .map_err(|_| payload_error(id, "document preview unavailable"))?
    };
    if current != stamp {
        return Err(payload_error(id, "document preview is stale"));
    }
    Ok(DocumentPreview {
        frame,
        stamp,
        entry_id: id,
        document,
        _memory: memory,
    })
}

struct SessionGuard<'session> {
    session: &'session DocumentSession,
    request: DocumentRequest,
    finished: bool,
}
impl Drop for SessionGuard<'_> {
    fn drop(&mut self) {
        if !self.finished {
            self.session.cancel(&self.request);
        }
    }
}

pub async fn load_document_preview(
    database: Arc<Mutex<HistoryDB>>,
    session: &DocumentSession,
    request: DocumentRequest,
    analyzer: Arc<dyn NativeAnalyzer>,
) -> Result<DocumentPreview, PreviewErrorInfo> {
    let cancellation = session
        .start(&request)
        .map_err(|error| payload_error(request.entry_id, error))?;
    let mut guard = SessionGuard {
        session,
        request: request.clone(),
        finished: false,
    };
    let preview =
        document_preview_checked_async(database, request.clone(), analyzer, cancellation).await?;
    if !session.finish(&request, &preview.stamp) {
        return Err(payload_error(request.entry_id, "document preview is stale"));
    }
    guard.finished = true;
    Ok(preview)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tailsync_core::composite::display_document::{
        DocumentBlock, DocumentBlockKind, DocumentParagraph, DocumentRun, DocumentRunStyle,
    };
    use tailsync_core::composite::{Completeness, Mime, PortableBundle, Representation};

    struct BlockingAnalyzer {
        entered: Arc<AtomicBool>,
        stopped: Arc<AtomicBool>,
        release: Arc<AtomicBool>,
    }

    impl NativeAnalyzer for BlockingAnalyzer {
        fn analyze(
            &self,
            _task: &crate::content_analysis::NativeAnalysisTask,
            cancelled: &AtomicBool,
        ) -> Result<tailsync_core::composite::content_analysis::NativeAnalysisFacts, String>
        {
            self.entered.store(true, Ordering::SeqCst);
            while !cancelled.load(Ordering::SeqCst) && !self.release.load(Ordering::SeqCst) {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            self.stopped.store(true, Ordering::SeqCst);
            Err("cancelled".into())
        }
    }

    fn root(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "tailsync-document-{tag}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn html_bundle(html: &str) -> PortableBundle {
        PortableBundle {
            representations: vec![Representation {
                mime: Mime::Html,
                bytes: html.as_bytes().to_vec(),
            }],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::Complete,
        }
    }

    fn paragraph_document(text: &str) -> DisplayDocument {
        DisplayDocument {
            version: tailsync_core::composite::display_document::DOCUMENT_VERSION,
            blocks: vec![DocumentBlock {
                kind: DocumentBlockKind::Paragraph,
                paragraph: Some(DocumentParagraph {
                    list: None,
                    heading: None,
                    quote_depth: 0,
                    code_block: false,
                    list_depth: 0,
                    runs: vec![DocumentRun {
                        text: text.into(),
                        link: None,
                        style: DocumentRunStyle::default(),
                        unsupported: false,
                    }],
                }),
                table: None,
                image: None,
            }],
            assets: vec![],
            partial: vec![],
            styles_partial: false,
            first_image: None,
        }
    }

    fn metadata(document: DisplayDocument, size_bytes: u64) -> DocumentFrameMetadata {
        DocumentFrameMetadata {
            request_id: Some("fixture-request".into()),
            entry_id: 7,
            kind: "document".into(),
            name: "composite.txt".into(),
            stamp: format!("dc1:{}", "0a".repeat(32)),
            owner: "history".into(),
            generation: 0,
            model_bytes: 0,
            size_bytes,
            document,
            batch: None,
        }
    }

    fn analyzer() -> Arc<dyn NativeAnalyzer> {
        Arc::new(crate::content_analysis::UnsupportedNativeAnalyzer)
    }

    async fn preview_with_retry(
        database: &Arc<Mutex<HistoryDB>>,
        id: i64,
        request_id: Option<String>,
    ) -> DocumentPreview {
        for attempt in 0..50 {
            match document_preview_async(
                database.clone(),
                id,
                request_id.clone(),
                analyzer(),
                Cancellation::default(),
            )
            .await
            {
                Ok(preview) => return preview,
                Err(error) if error.retryable && attempt < 49 => {
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
                Err(error) => panic!("document preview failed: {error:?}"),
            }
        }
        unreachable!("retry loop always returns or panics")
    }

    #[test]
    fn frame_round_trips_and_rejects_unknown_versions_and_kinds() {
        let frame =
            encode_document_response(metadata(paragraph_document("hello"), 0), &[]).unwrap();
        let (decoded, payload) = decode_document_response(&frame).unwrap();
        assert_eq!(decoded.kind, "document");
        assert!(payload.is_empty());

        let mut bad_version = frame.clone();
        bad_version[4] = 2;
        assert!(decode_document_response(&bad_version).is_err());

        let mut bad_magic = frame.clone();
        bad_magic[0] = b'X';
        assert!(decode_document_response(&bad_magic).is_err());

        let mut bad_kind = metadata(paragraph_document("hello"), 0);
        bad_kind.kind = "text".into();
        assert!(encode_document_response(bad_kind, &[]).is_err());

        assert!(encode_document_response(metadata(paragraph_document("hello"), 3), &[]).is_err());
    }

    #[test]
    fn frame_rejects_a_truncated_or_trailing_payload() {
        let frame =
            encode_document_response(metadata(paragraph_document("hello"), 0), &[]).unwrap();
        assert!(decode_document_response(&frame[..frame.len() - 1]).is_err());
        let mut trailing = frame.clone();
        trailing.push(0);
        assert!(decode_document_response(&trailing).is_err());
    }

    #[tokio::test]
    async fn document_preview_returns_a_decodable_frame_for_a_composite() {
        let path = root("composite");
        let mut database = HistoryDB::open_isolated_for_test_composite(&path).unwrap();
        let id = database
            .add_composite(&html_bundle("<p>hello world</p>"), None, "self")
            .unwrap();
        let database = Arc::new(Mutex::new(database));
        let preview = preview_with_retry(&database, id, Some("request".into())).await;
        let (decoded, payload) = decode_document_response(&preview.frame).unwrap();
        assert_eq!(decoded.request_id.as_deref(), Some("request"));
        assert_eq!(decoded.entry_id, id);
        assert!(payload.is_empty());
        let paragraph = decoded.document.blocks[0].paragraph.as_ref().unwrap();
        assert_eq!(paragraph.runs[0].text, "hello world");
        {
            let guard = database.lock().await;
            assert!(preview.verify(&guard).unwrap());
        }
        drop(database);
        std::fs::remove_dir_all(&path).unwrap();
    }

    #[tokio::test]
    async fn reading_semantics_survive_preview_frames_and_database_reopen_without_rewriting_originals(
    ) {
        let path = root("reading-semantics");
        let html = "<h1>标题</h1><blockquote><p><a href=\"https://example.com/reading\">链接</a><mark>高亮</mark></p></blockquote><pre>  first\n\tsecond  </pre><p>end</p>";
        let bundle = html_bundle(html);
        let mut database = HistoryDB::open_isolated_for_test_composite(&path).unwrap();
        let id = database.add_composite(&bundle, None, "self").unwrap();
        let original_identity = database.composite_identity(id).unwrap();
        let database = Arc::new(Mutex::new(database));
        let first = preview_with_retry(&database, id, Some("reading-first".into())).await;
        let (frame, payload) = decode_document_response(&first.frame).unwrap();
        assert!(payload.is_empty());
        assert_eq!(
            frame.document.blocks[0].paragraph.as_ref().unwrap().heading,
            Some(1)
        );
        let quotation = frame.document.blocks[1].paragraph.as_ref().unwrap();
        assert_eq!(quotation.quote_depth, 1);
        assert_eq!(
            quotation.runs[0].link.as_deref(),
            Some("https://example.com/reading")
        );
        assert!(quotation.runs[1].style.highlight.is_some());
        let code = frame.document.blocks[2].paragraph.as_ref().unwrap();
        assert!(code.code_block);
        assert_eq!(
            code.runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<String>(),
            "  first\n\tsecond  "
        );
        {
            let guard = database.lock().await;
            assert_eq!(guard.get_composite(id).unwrap(), bundle);
            assert_eq!(guard.composite_identity(id).unwrap(), original_identity);
        }
        drop(first);
        drop(database);
        let reopened = HistoryDB::open_with_policy_for_probe(
            &path,
            tailsync_core::db::MigrationPolicy::CompositeAllowed,
        )
        .unwrap();
        assert_eq!(reopened.get_composite(id).unwrap(), bundle);
        assert_eq!(reopened.composite_identity(id).unwrap(), original_identity);
        let reopened = Arc::new(Mutex::new(reopened));
        let second = preview_with_retry(&reopened, id, Some("reading-second".into())).await;
        let (decoded, _) = decode_document_response(&second.frame).unwrap();
        assert_eq!(decoded.document, frame.document);
        drop(second);
        drop(reopened);
        std::fs::remove_dir_all(&path).unwrap();
    }

    #[tokio::test]
    async fn rtf_reading_styles_use_original_bytes_without_an_available_native_helper() {
        let path = root("rtf-reading");
        let bundle = PortableBundle {
            representations: vec![Representation {
                mime: Mime::Rtf,
                bytes: br"{\rtf1 Plain {\b bold} plain}".to_vec(),
            }],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::Complete,
        };
        let mut database = HistoryDB::open_isolated_for_test_composite(&path).unwrap();
        let id = database.add_composite(&bundle, None, "self").unwrap();
        let database = Arc::new(Mutex::new(database));
        let preview = preview_with_retry(&database, id, Some("rtf-reading".into())).await;
        let (decoded, payload) = decode_document_response(&preview.frame).unwrap();
        assert!(payload.is_empty());
        let runs = &decoded.document.blocks[0].paragraph.as_ref().unwrap().runs;
        assert_eq!(
            runs.iter().map(|run| run.text.as_str()).collect::<String>(),
            "Plain bold plain"
        );
        assert!(runs.iter().any(|run| run.text == "bold" && run.style.bold));
        assert!(!runs[0].style.bold && !runs.last().unwrap().style.bold);
        assert_eq!(database.lock().await.get_composite(id).unwrap(), bundle);
        drop(preview);
        drop(database);
        std::fs::remove_dir_all(&path).unwrap();
    }

    #[tokio::test]
    async fn document_preview_rejects_a_non_composite_entry() {
        let path = root("text");
        let mut database = HistoryDB::open_isolated_for_test_composite(&path).unwrap();
        database.add_text("plain", "self").unwrap();
        let id = database.get_all(None, None, 1, 0).unwrap()[0].id;
        let database = Arc::new(Mutex::new(database));
        let error = document_preview_async(
            database.clone(),
            id,
            None,
            analyzer(),
            Cancellation::default(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, PreviewErrorCode::UnsupportedType);
        drop(database);
        std::fs::remove_dir_all(&path).unwrap();
    }

    #[tokio::test]
    async fn document_preview_stamp_survives_favorite_and_changes_on_archive_update() {
        let path = root("stamp");
        let mut database = HistoryDB::open_isolated_for_test_composite(&path).unwrap();
        let id = database
            .add_composite(&html_bundle("<p>body</p>"), None, "self")
            .unwrap();
        let database = Arc::new(Mutex::new(database));
        let preview = preview_with_retry(&database, id, None).await;
        {
            let mut guard = database.lock().await;
            guard.set_favorite(id, true).unwrap();
        }
        let favorited = preview_with_retry(&database, id, None).await;
        assert_eq!(favorited.stamp, preview.stamp);
        {
            let guard = database.lock().await;
            assert!(favorited.verify(&guard).unwrap());
        }
        let identity = {
            let guard = database.lock().await;
            guard.composite_identity(id).unwrap().unwrap()
        };
        {
            let mut guard = database.lock().await;
            assert!(guard
                .update_composite_archive_summary(id, &identity, "updated summary")
                .unwrap());
        }
        {
            let guard = database.lock().await;
            assert!(!favorited.verify(&guard).unwrap());
        }
        drop(database);
        std::fs::remove_dir_all(&path).unwrap();
    }

    #[tokio::test]
    async fn document_preview_stamp_is_bound_to_the_database_instance() {
        let first_path = root("instance-a");
        let second_path = root("instance-b");
        let mut first = HistoryDB::open_isolated_for_test_composite(&first_path).unwrap();
        let id = first
            .add_composite(&html_bundle("<p>body</p>"), None, "self")
            .unwrap();
        let first = Arc::new(Mutex::new(first));
        let preview = preview_with_retry(&first, id, None).await;
        let second = HistoryDB::open_isolated_for_test_composite(&second_path).unwrap();
        assert!(!preview.verify(&second).unwrap());
        drop(second);
        drop(first);
        std::fs::remove_dir_all(&first_path).unwrap();
        std::fs::remove_dir_all(&second_path).unwrap();
    }

    #[tokio::test]
    async fn document_preview_cancellation_returns_an_error() {
        let path = root("cancel");
        let mut database = HistoryDB::open_isolated_for_test_composite(&path).unwrap();
        let id = database
            .add_composite(&html_bundle("<p>body</p>"), None, "self")
            .unwrap();
        let database = Arc::new(Mutex::new(database));
        let cancellation = Cancellation::default();
        cancellation.cancel();
        let error = document_preview_async(database.clone(), id, None, analyzer(), cancellation)
            .await
            .unwrap_err();
        assert_eq!(error.code, PreviewErrorCode::PayloadUnavailable);
        drop(database);
        std::fs::remove_dir_all(&path).unwrap();
    }

    #[tokio::test]
    async fn document_preview_stamp_is_invalidated_by_deletion() {
        let path = root("deleted");
        let mut database = HistoryDB::open_isolated_for_test_composite(&path).unwrap();
        let id = database
            .add_composite(&html_bundle("<p>body</p>"), None, "self")
            .unwrap();
        let database = Arc::new(Mutex::new(database));
        let preview = preview_with_retry(&database, id, None).await;
        {
            let mut guard = database.lock().await;
            guard.delete(id).unwrap();
        }
        {
            let guard = database.lock().await;
            assert!(!preview.verify(&guard).unwrap());
        }
        drop(database);
        std::fs::remove_dir_all(&path).unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dropping_the_preview_future_cancels_the_analysis_worker() {
        let path = root("cancel-worker");
        let mut database = HistoryDB::open_isolated_for_test_composite(&path).unwrap();
        let bundle = PortableBundle {
            representations: vec![Representation {
                mime: Mime::Rtf,
                bytes: b"{\\rtf1 body}".to_vec(),
            }],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::Complete,
        };
        let id = database.add_composite(&bundle, None, "self").unwrap();
        let database = Arc::new(Mutex::new(database));
        let entered = Arc::new(AtomicBool::new(false));
        let stopped = Arc::new(AtomicBool::new(false));
        let mut attempts = 0;
        loop {
            attempts += 1;
            assert!(attempts <= 40, "analysis worker never started");
            entered.store(false, Ordering::SeqCst);
            stopped.store(false, Ordering::SeqCst);
            let analyzer = Arc::new(BlockingAnalyzer {
                entered: entered.clone(),
                stopped: stopped.clone(),
                release: Arc::new(AtomicBool::new(false)),
            });
            let task = tokio::spawn(document_preview_async(
                database.clone(),
                id,
                None,
                analyzer,
                Cancellation::default(),
            ));
            for _ in 0..50 {
                if entered.load(Ordering::SeqCst) || task.is_finished() {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            if !entered.load(Ordering::SeqCst) {
                let _ = task.await;
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                continue;
            }
            task.abort();
            let _ = task.await;
            for _ in 0..300 {
                if stopped.load(Ordering::SeqCst) {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            assert!(
                stopped.load(Ordering::SeqCst),
                "analysis worker kept running after the future was dropped"
            );
            break;
        }
        drop(database);
        std::fs::remove_dir_all(&path).unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn document_preview_waits_on_the_database_lock_cancellably() {
        let path = root("lock");
        let mut database = HistoryDB::open_isolated_for_test_composite(&path).unwrap();
        let id = database
            .add_composite(&html_bundle("<p>body</p>"), None, "self")
            .unwrap();
        let database = Arc::new(Mutex::new(database));
        let guard = database.lock().await;
        let cancellation = Cancellation::default();
        let worker = tokio::spawn({
            let database = database.clone();
            let cancellation = cancellation.clone();
            async move { document_preview_async(database, id, None, analyzer(), cancellation).await }
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        cancellation.cancel();
        let result = tokio::time::timeout(std::time::Duration::from_secs(5), worker)
            .await
            .expect("the lock wait must be cancellable")
            .unwrap();
        assert!(result.is_err());
        drop(guard);
        drop(database);
        std::fs::remove_dir_all(&path).unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn document_preview_rejects_a_mutation_that_lands_after_prepare() {
        let path = root("generation");
        let mut database = HistoryDB::open_isolated_for_test_composite(&path).unwrap();
        let bundle = PortableBundle {
            representations: vec![Representation {
                mime: Mime::Rtf,
                bytes: b"{\\rtf1 body}".to_vec(),
            }],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::Complete,
        };
        let id = database.add_composite(&bundle, None, "self").unwrap();
        let identity = database.composite_identity(id).unwrap().unwrap();
        let database = Arc::new(Mutex::new(database));
        let entered = Arc::new(AtomicBool::new(false));
        let stopped = Arc::new(AtomicBool::new(false));
        let release = Arc::new(AtomicBool::new(false));
        let mut attempts = 0;
        loop {
            attempts += 1;
            assert!(attempts <= 40, "analysis worker never started");
            entered.store(false, Ordering::SeqCst);
            stopped.store(false, Ordering::SeqCst);
            release.store(false, Ordering::SeqCst);
            let analyzer = Arc::new(BlockingAnalyzer {
                entered: entered.clone(),
                stopped: stopped.clone(),
                release: release.clone(),
            });
            let task = tokio::spawn(document_preview_async(
                database.clone(),
                id,
                None,
                analyzer,
                Cancellation::default(),
            ));
            for _ in 0..50 {
                if entered.load(Ordering::SeqCst) || task.is_finished() {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            if !entered.load(Ordering::SeqCst) {
                let _ = task.await;
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                continue;
            }
            {
                let mut guard = database.lock().await;
                assert!(guard
                    .update_composite_archive_summary(id, &identity, "mutated during analysis")
                    .unwrap());
            }
            release.store(true, Ordering::SeqCst);
            let error = task.await.unwrap().unwrap_err();
            assert!(!error.retryable, "a stale analysis must fail closed");
            break;
        }
        drop(database);
        std::fs::remove_dir_all(&path).unwrap();
    }
}
