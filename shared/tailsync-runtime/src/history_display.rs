use crate::content_analysis::{analyze_entry_cancellable, ContentAnalysisError, NativeAnalyzer};
use crate::history::{HistoryOperations, HistoryPageRequestOwned};
use std::sync::Arc;
use tailsync_core::cancellation::Cancellation;
use tailsync_core::composite::budgets::{global, PipelineLease};
use tailsync_core::db::{HistoryDB, HistoryQueryPage};
use tokio::sync::Mutex;

pub struct CheckedHistoryImage {
    pub data: Vec<u8>,
    pub cache_key: String,
    _memory: PipelineLease,
}

struct CancelOnDrop {
    cancellation: Cancellation,
    watcher: tokio::task::JoinHandle<()>,
}

impl CancelOnDrop {
    fn new(caller: Cancellation) -> Self {
        let cancellation = Cancellation::default();
        if caller.is_cancelled() {
            cancellation.cancel();
        }
        let worker = cancellation.clone();
        let watcher = tokio::spawn(async move {
            caller.cancelled().await;
            worker.cancel();
        });
        Self {
            cancellation,
            watcher,
        }
    }
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.cancellation.cancel();
        self.watcher.abort();
    }
}

async fn lock_database<'store>(
    database: &'store Mutex<HistoryDB>,
    cancellation: &Cancellation,
) -> Result<tokio::sync::MutexGuard<'store, HistoryDB>, String> {
    tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err("request cancelled".into()),
        stored = database.lock() => Ok(stored),
    }
}

pub async fn rebuild_display_batch(
    database: Arc<Mutex<HistoryDB>>,
    analyzer: Arc<dyn NativeAnalyzer>,
    cancellation: Cancellation,
) -> Result<bool, String> {
    let guard = CancelOnDrop::new(cancellation);
    let cancellation = guard.cancellation.clone();
    let ids = lock_database(&database, &cancellation)
        .await?
        .display_rebuild_candidates(4)
        .map_err(|error| error.to_string())?;
    for id in ids {
        cancellation.check().map_err(|error| error.to_string())?;
        let revision = lock_database(&database, &cancellation)
            .await?
            .read_revision()
            .map_err(|error| error.to_string())?;
        let facts = match analyze_entry_cancellable(
            database.clone(),
            id,
            analyzer.clone(),
            cancellation.clone(),
        )
        .await
        {
            Ok(facts) => facts,
            Err(ContentAnalysisError::Busy | ContentAnalysisError::Stale) => return Ok(true),
            Err(ContentAnalysisError::Cancelled) => return Err("request cancelled".into()),
            Err(_) => {
                if !lock_database(&database, &cancellation)
                    .await?
                    .commit_unavailable_display(id, &revision, &cancellation)
                    .map_err(|error| error.to_string())?
                {
                    return Ok(true);
                }
                continue;
            }
        };
        if !facts
            .commit_display(
                &mut *lock_database(&database, &cancellation).await?,
                &cancellation,
            )
            .map_err(|error| error.to_string())?
        {
            return Ok(true);
        }
    }
    let pending = !lock_database(&database, &cancellation)
        .await?
        .display_rebuild_candidates(1)
        .map_err(|error| error.to_string())?
        .is_empty();
    Ok(pending)
}

pub async fn page_display_async(
    database: Arc<Mutex<HistoryDB>>,
    request: HistoryPageRequestOwned,
    analyzer: Arc<dyn NativeAnalyzer>,
) -> Result<HistoryQueryPage, String> {
    let pending =
        rebuild_display_batch(database.clone(), analyzer, Cancellation::default()).await?;
    let mut page = HistoryOperations::page_async(database.clone(), request).await?;
    let pending = pending
        || !database
            .lock()
            .await
            .display_rebuild_candidates(1)
            .map_err(|error| error.to_string())?
            .is_empty();
    page.display_index_pending = Some(pending);
    if pending {
        page.total = None;
    }
    Ok(page)
}

pub async fn entries_display_async(
    database: Arc<Mutex<HistoryDB>>,
    request: HistoryPageRequestOwned,
    analyzer: Arc<dyn NativeAnalyzer>,
) -> Result<Vec<tailsync_core::db::HistoryEntry>, String> {
    let page = page_display_async(database, request, analyzer).await?;
    if page.display_index_pending == Some(true) {
        return Err("content analysis busy".into());
    }
    Ok(page.entries)
}

pub async fn image_payload_checked_async(
    database: Arc<Mutex<HistoryDB>>,
    id: i64,
    expected_key: Option<String>,
    analyzer: Arc<dyn NativeAnalyzer>,
    cancellation: Cancellation,
) -> Result<CheckedHistoryImage, String> {
    let guard = CancelOnDrop::new(cancellation);
    let cancellation = guard.cancellation.clone();
    let (key, entry_type) = {
        let stored = lock_database(&database, &cancellation).await?;
        let key = stored
            .thumbnail_key(id)
            .map_err(|error| error.to_string())?;
        if expected_key
            .as_ref()
            .is_some_and(|expected| expected != &key)
        {
            return Err("history image is stale".into());
        }
        (key, stored.get_type(id).map_err(|error| error.to_string())?)
    };
    let (data, memory) = if entry_type == "composite" {
        let facts = analyze_entry_cancellable(database.clone(), id, analyzer, cancellation.clone())
            .await
            .map_err(|error| error.to_string())?;
        let cancel_decode = cancellation.clone();
        let (facts, data, memory) = tokio::task::spawn_blocking(move || -> Result<_, String> {
            cancel_decode.check().map_err(|error| error.to_string())?;
            let memory = global()
                .reserve_pipeline(64 * 1024 * 1024)
                .map_err(|_| ContentAnalysisError::Busy.to_string())?;
            let first = facts
                .first_image_if_available()
                .ok_or("Composite has no displayable first image")?;
            let data = tailsync_core::composite::png_to_packed(&first.png)
                .map_err(|error| error.to_string())?;
            cancel_decode.check().map_err(|error| error.to_string())?;
            Ok((facts, data, memory))
        })
        .await
        .map_err(|error| error.to_string())??;
        let stored = lock_database(&database, &cancellation).await?;
        if !facts
            .is_current(&stored)
            .map_err(|error| error.to_string())?
        {
            return Err("history image is stale".into());
        }
        (data, memory)
    } else {
        let memory = global()
            .reserve_pipeline(64 * 1024 * 1024)
            .map_err(|_| ContentAnalysisError::Busy.to_string())?;
        let data = HistoryOperations::image_payload_async(database.clone(), id).await?;
        (data, memory)
    };
    cancellation.check().map_err(|error| error.to_string())?;
    if lock_database(&database, &cancellation)
        .await?
        .thumbnail_key(id)
        .map_err(|error| error.to_string())?
        != key
    {
        return Err("history image is stale".into());
    }
    Ok(CheckedHistoryImage {
        data,
        cache_key: key,
        _memory: memory,
    })
}
