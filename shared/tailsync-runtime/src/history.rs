use std::path::PathBuf;
use std::sync::Arc;
use tailsync_core::db::{
    FavoriteMutation, HistoryCollection, HistoryDB, HistoryEntry, HistoryQuery, HistoryQueryPage,
    HistoryReadError, MigrationDiagnostics, PreviewError, PreviewMetadata, PreviewPayload,
};
use tokio::sync::Mutex;

use crate::execution::{run_db_named, run_read, DbExecutionError};

/// A transport-neutral paged history request.
#[derive(Debug, Clone)]
pub struct HistoryPageRequest<'a> {
    pub collection: HistoryCollection,
    pub keyword: Option<&'a str>,
    pub category: Option<&'a str>,
    pub start_time: Option<&'a str>,
    pub end_time: Option<&'a str>,
    pub limit: usize,
    pub offset: usize,
}

impl<'a> HistoryPageRequest<'a> {
    // Keep the transport-facing constructor explicit: these names mirror the
    // stable JSON/Tauri request fields and avoid a second conversion struct.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        collection: HistoryCollection,
        keyword: Option<&'a str>,
        category: Option<&'a str>,
        start_time: Option<&'a str>,
        end_time: Option<&'a str>,
        limit: Option<usize>,
        offset: Option<usize>,
        default_limit: usize,
    ) -> Self {
        Self {
            collection,
            keyword,
            category,
            start_time,
            end_time,
            limit: limit.unwrap_or(default_limit),
            offset: offset.unwrap_or(0),
        }
    }

    fn into_core(self) -> HistoryQuery<'a> {
        HistoryQuery {
            collection: self.collection,
            keyword: self.keyword,
            category: self.category,
            start_time: self.start_time,
            end_time: self.end_time,
            limit: self.limit,
            offset: self.offset,
        }
    }
}

/// Owned form used when an async transport hands work to a blocking worker.
/// Keeping request data owned prevents borrowed IPC strings from crossing the
/// worker boundary and makes cancellation safe before the task starts.
#[derive(Debug, Clone)]
pub struct HistoryPageRequestOwned {
    pub collection: HistoryCollection,
    pub keyword: Option<String>,
    pub category: Option<String>,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub limit: usize,
    pub offset: usize,
}

impl HistoryPageRequestOwned {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        collection: HistoryCollection,
        keyword: Option<String>,
        category: Option<String>,
        start_time: Option<String>,
        end_time: Option<String>,
        limit: Option<usize>,
        offset: Option<usize>,
        default_limit: usize,
    ) -> Self {
        Self {
            collection,
            keyword,
            category,
            start_time,
            end_time,
            limit: limit.unwrap_or(default_limit),
            offset: offset.unwrap_or(0),
        }
    }

    fn as_request(&self) -> HistoryPageRequest<'_> {
        HistoryPageRequest {
            collection: self.collection,
            keyword: self.keyword.as_deref(),
            category: self.category.as_deref(),
            start_time: self.start_time.as_deref(),
            end_time: self.end_time.as_deref(),
            limit: self.limit,
            offset: self.offset,
        }
    }
}

/// The single implementation of history operations used by every transport.
///
/// The synchronous methods are the small Core-facing implementation seam used
/// by tests. The async methods below own queueing, database locking, error
/// mapping, and post-commit notification so platform adapters only map DTOs.
pub struct HistoryOperations<'a> {
    db: &'a mut HistoryDB,
}

/// Decrypted/materialized data needed by a transport to restore one history
/// entry.  The runtime decides whether a file is represented by a path or by
/// legacy inline bytes; clipboard adapters decide how to publish it.
#[derive(Debug)]
pub struct HistoryRestorePayload {
    pub entry_type: String,
    pub file_path: Option<PathBuf>,
    pub file_name: Option<String>,
    pub data: Option<Vec<u8>>,
}

impl<'a> HistoryOperations<'a> {
    pub fn new(db: &'a mut HistoryDB) -> Self {
        Self { db }
    }

    pub fn restore_payload(&mut self, id: i64) -> Result<HistoryRestorePayload, String> {
        let entry_type = self.db.get_type(id).map_err(|error| error.to_string())?;
        let file_path = if entry_type == "file" {
            self.db
                .get_file_path(id)
                .map_err(|error| error.to_string())?
        } else {
            None
        };
        let file_name = if entry_type == "file" {
            Some(
                self.db
                    .get_description(id)
                    .map_err(|error| error.to_string())?,
            )
        } else {
            None
        };
        let data = if file_path.is_none() {
            Some(self.db.get_data(id).map_err(|error| error.to_string())?)
        } else {
            None
        };

        Ok(HistoryRestorePayload {
            entry_type,
            file_path,
            file_name,
            data,
        })
    }

    pub fn page(&mut self, request: HistoryPageRequest<'_>) -> Result<HistoryQueryPage, String> {
        self.db
            .get_page_in_collection(request.into_core())
            .map_err(|error| error.to_string())
    }

    pub fn entries(
        &mut self,
        request: HistoryPageRequest<'_>,
    ) -> Result<Vec<HistoryEntry>, String> {
        Ok(self.page(request)?.entries)
    }

    pub fn delete(&mut self, id: i64) -> Result<(), String> {
        self.db.delete(id).map_err(|error| error.to_string())
    }

    pub fn set_favorite(&mut self, id: i64, favorite: bool) -> Result<FavoriteMutation, String> {
        self.db
            .set_favorite(id, favorite)
            .map_err(|error| error.to_string())
    }

    pub fn delete_favorite(&mut self, id: i64) -> Result<FavoriteMutation, String> {
        self.db
            .delete_favorite(id)
            .map_err(|error| error.to_string())
    }

    pub fn clear(&mut self) -> Result<(), String> {
        self.db.clear_all().map_err(|error| error.to_string())
    }

    pub async fn page_async(
        database: Arc<Mutex<HistoryDB>>,
        request: HistoryPageRequestOwned,
    ) -> Result<HistoryQueryPage, String> {
        Self::page_read(database, request)
            .await
            .map_err(|error| error.to_string())
    }

    pub async fn page_read(
        database: Arc<Mutex<HistoryDB>>,
        request: HistoryPageRequestOwned,
    ) -> Result<HistoryQueryPage, HistoryReadError> {
        run_read("history.page", database, move |context| {
            // A changed revision discards the entire attempt. Bound retries
            // so a busy receiver cannot keep a search alive indefinitely.
            for attempt in 0..3 {
                let result = (|| {
                    let has_keyword = request.keyword.as_ref().is_some_and(|s| !s.trim().is_empty());
                    let (revision, total) = context.with_database(|database| {
                        let revision = database.read_revision()?;
                        let total = if has_keyword { None } else {
                            Some(database.count_in_collection(request.collection, None, request.category.as_deref(), request.start_time.as_deref(), request.end_time.as_deref())
                                .map_err(|error| HistoryReadError::Operation(error.to_string()))?)
                        };
                        database.validate_read_revision(&revision)?;
                        Ok((revision, total))
                    })?;
                    let mut entries = Vec::new();
                    let mut cursor = None;
                    let mut matched = 0usize;
                    let mut has_more = false;
                    loop {
                        context.cancellation.check()?;
                        let chunk = context.with_database(|database| database.prepare_read_chunk(request.as_request().into_core(), cursor.as_ref(), &revision, 128))?;
                        let exhausted = chunk.exhausted;
                        cursor = chunk.cursor.clone();
                        let started = std::time::Instant::now();
                        for entry in chunk.matching_entries(request.keyword.as_deref(), &context.cancellation)? {
                            if has_keyword && matched < request.offset {
                                matched += 1;
                                continue;
                            }
                            if entries.len() == request.limit { has_more = true; break; }
                            entries.push(entry);
                        }
                        tracing::debug!(target: "tailsync.runtime.execution", operation = "history.page", decrypt_match_ms = started.elapsed().as_secs_f64() * 1000.0, "history matching timing");
                        if has_more || exhausted || cursor.is_none() { break; }
                    }
                    context.with_database(|database| database.validate_read_revision(&revision))?;
                    Ok(HistoryQueryPage { entries, total, has_more })
                })();
                match result {
                    Err(HistoryReadError::Changed) if attempt < 2 => continue,
                    result => return result,
                }
            }
            unreachable!("bounded query retries always return")
        })
        .await
    }

    pub async fn entries_async(
        database: Arc<Mutex<HistoryDB>>,
        request: HistoryPageRequestOwned,
    ) -> Result<Vec<HistoryEntry>, String> {
        Ok(Self::page_async(database, request).await?.entries)
    }

    pub async fn delete_async(database: Arc<Mutex<HistoryDB>>, id: i64) -> Result<(), String> {
        Self::delete_async_with_hook(database, id, || {}).await
    }

    pub async fn delete_async_with_hook<H: FnOnce() + Send + 'static>(
        database: Arc<Mutex<HistoryDB>>,
        id: i64,
        on_commit: H,
    ) -> Result<(), String> {
        run_db_named("history.delete", database, move |database| {
            let result = HistoryOperations { db: database }.delete(id);
            if result.is_ok() {
                on_commit();
            }
            result
        })
        .await
        .map_err(|error: DbExecutionError<String>| error.to_string())
    }

    pub async fn set_favorite_async(
        database: Arc<Mutex<HistoryDB>>,
        id: i64,
        favorite: bool,
    ) -> Result<FavoriteMutation, String> {
        Self::set_favorite_async_with_hook(database, id, favorite, || {}).await
    }

    pub async fn set_favorite_async_with_hook<H: FnOnce() + Send + 'static>(
        database: Arc<Mutex<HistoryDB>>,
        id: i64,
        favorite: bool,
        on_commit: H,
    ) -> Result<FavoriteMutation, String> {
        run_db_named("history.set_favorite", database, move |database| {
            let result = HistoryOperations { db: database }.set_favorite(id, favorite);
            if result.is_ok() {
                on_commit();
            }
            result
        })
        .await
        .map_err(|error: DbExecutionError<String>| error.to_string())
    }

    pub async fn delete_favorite_async(
        database: Arc<Mutex<HistoryDB>>,
        id: i64,
    ) -> Result<FavoriteMutation, String> {
        Self::delete_favorite_async_with_hook(database, id, || {}).await
    }

    pub async fn delete_favorite_async_with_hook<H: FnOnce() + Send + 'static>(
        database: Arc<Mutex<HistoryDB>>,
        id: i64,
        on_commit: H,
    ) -> Result<FavoriteMutation, String> {
        run_db_named("history.delete_favorite", database, move |database| {
            let result = HistoryOperations { db: database }.delete_favorite(id);
            if result.is_ok() {
                on_commit();
            }
            result
        })
        .await
        .map_err(|error: DbExecutionError<String>| error.to_string())
    }

    pub async fn clear_async(database: Arc<Mutex<HistoryDB>>) -> Result<(), String> {
        Self::clear_async_with_hook(database, || {}).await
    }

    pub async fn clear_async_with_hook<H: FnOnce() + Send + 'static>(
        database: Arc<Mutex<HistoryDB>>,
        on_commit: H,
    ) -> Result<(), String> {
        run_db_named("history.clear", database, move |database| {
            let result = HistoryOperations { db: database }.clear();
            if result.is_ok() {
                on_commit();
            }
            result
        })
        .await
        .map_err(|error: DbExecutionError<String>| error.to_string())
    }

    pub async fn restore_payload_async(
        database: Arc<Mutex<HistoryDB>>,
        id: i64,
    ) -> Result<HistoryRestorePayload, String> {
        run_db_named("history.restore_payload", database, move |database| {
            HistoryOperations { db: database }.restore_payload(id)
        })
        .await
        .map_err(|error: DbExecutionError<String>| error.to_string())
    }

    pub async fn data_async(database: Arc<Mutex<HistoryDB>>, id: i64) -> Result<Vec<u8>, String> {
        run_db_named("history.data", database, move |database| {
            database.get_data(id).map_err(|error| error.to_string())
        })
        .await
        .map_err(|error: DbExecutionError<String>| error.to_string())
    }

    pub async fn migration_diagnostics_async(
        database: Arc<Mutex<HistoryDB>>,
        limit: usize,
    ) -> Result<MigrationDiagnostics, String> {
        run_db_named("history.migration_diagnostics", database, move |database| {
            database
                .migration_diagnostics(limit)
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|error: DbExecutionError<String>| error.to_string())
    }

    /// Read, decrypt and encode under the same bounded/cancellable worker
    /// permit. Large frame copies never run on an async executor thread.
    pub async fn preview_binary_async(
        database: Arc<Mutex<HistoryDB>>,
        id: i64,
        batch_id: Option<String>,
        request_id: Option<String>,
    ) -> Result<Vec<u8>, tailsync_core::db::PreviewErrorInfo> {
        use tailsync_core::db::PreviewErrorInfo;
        run_read("history.preview_binary", database, move |context| {
            let prepared = context
                .with_database(|database| Ok(database.prepare_preview(id, batch_id.as_deref())))?;
            let result = prepared
                .and_then(|prepared| prepared.read(&context.cancellation))
                .map_err(PreviewErrorInfo::from)
                .and_then(|(metadata, payload)| {
                    crate::preview::encode_preview_response_for_request(
                        metadata, payload, request_id,
                    )
                });
            context.cancellation.check()?;
            Ok(result)
        })
        .await
        .map_err(|error| PreviewErrorInfo::payload_unavailable(id, error.to_string()))?
    }

    pub async fn preview_async(
        database: Arc<Mutex<HistoryDB>>,
        id: i64,
        batch_id: Option<String>,
    ) -> Result<(PreviewMetadata, PreviewPayload), PreviewError> {
        let result = run_read("history.preview", database, move |context| {
            let prepared = context
                .with_database(|database| Ok(database.prepare_preview(id, batch_id.as_deref())))?;
            Ok(prepared.and_then(|prepared| prepared.read(&context.cancellation)))
        })
        .await;
        match result {
            Ok(result) => result,
            Err(error) => Err(PreviewError::PayloadUnavailable {
                entry_id: id,
                reason: error.to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn chunked_search_preserves_legacy_offset_filter_and_has_more() {
        let mut database = HistoryDB::new_unavailable().unwrap();
        database.set_max_history(1000);
        for index in 0..300 {
            database
                .add_text(
                    &format!(
                        "fixture {index} {}",
                        if index % 3 == 0 { "needle" } else { "other" }
                    ),
                    "fixture",
                )
                .unwrap();
        }
        let expected = database
            .get_page_filtered(Some("needle"), None, None, None, 17, 80)
            .unwrap();
        let database = Arc::new(Mutex::new(database));
        let request = HistoryPageRequestOwned::new(
            HistoryCollection::All,
            Some("needle".into()),
            None,
            None,
            None,
            Some(17),
            Some(80),
            50,
        );
        let actual = HistoryOperations::page_read(database.clone(), request)
            .await
            .unwrap();
        assert_eq!(
            actual.entries.iter().map(|e| e.id).collect::<Vec<_>>(),
            expected.entries.iter().map(|e| e.id).collect::<Vec<_>>()
        );
        assert_eq!(actual.has_more, expected.has_more);
        assert_eq!(actual.total, expected.total);
        let expected = database
            .lock()
            .await
            .get_page_filtered(None, None, None, None, 17, 290)
            .unwrap();
        let actual = HistoryOperations::page_read(
            database,
            HistoryPageRequestOwned::new(
                HistoryCollection::All,
                None,
                None,
                None,
                None,
                Some(17),
                Some(290),
                50,
            ),
        )
        .await
        .unwrap();
        assert_eq!(actual.entries.len(), expected.entries.len());
        assert_eq!(actual.total, Some(300));
        assert!(!actual.has_more);
    }

    #[tokio::test]
    async fn client_cancellation_after_commit_does_not_drop_notification() {
        let mut database = HistoryDB::new_unavailable().unwrap();
        database.add_text("commit then cancel", "fixture").unwrap();
        let id = database.get_all(None, None, 1, 0).unwrap()[0].id;
        let database = Arc::new(Mutex::new(database));
        let (committed, commit) = tokio::sync::oneshot::channel();
        let (release, released) = std::sync::mpsc::channel();
        let (published, publication) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(HistoryOperations::delete_async_with_hook(
            database.clone(),
            id,
            move || {
                let _ = committed.send(());
                released
                    .recv_timeout(std::time::Duration::from_secs(2))
                    .unwrap();
                let _ = published.send(());
            },
        ));
        commit.await.unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        release.send(()).unwrap();
        publication.await.unwrap();
        assert!(database
            .lock()
            .await
            .get_all(None, None, 10, 0)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn request_defaults_are_applied_at_the_use_case_seam() {
        let request = HistoryPageRequest::new(
            HistoryCollection::All,
            None,
            None,
            None,
            None,
            None,
            None,
            37,
        );
        assert_eq!(request.limit, 37);
        assert_eq!(request.offset, 0);
    }

    #[test]
    fn restore_payload_reads_the_core_entry_once_without_transport_policy() {
        let mut db = HistoryDB::new_unavailable().expect("in-memory history database");
        db.add_text("runtime seam", "test").unwrap();
        let id = db.get_all(None, None, 1, 0).unwrap()[0].id;

        let payload = HistoryOperations::new(&mut db)
            .restore_payload(id)
            .expect("history payload");

        assert_eq!(payload.entry_type, "text");
        assert!(payload.file_path.is_none());
        assert!(payload.file_name.is_none());
        assert_eq!(
            String::from_utf8(payload.data.expect("text data")).unwrap(),
            "runtime seam"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn successful_mutation_publishes_after_commit_inside_the_worker() {
        let database = Arc::new(Mutex::new(
            HistoryDB::new_unavailable().expect("in-memory history database"),
        ));
        crate::execution::run_db(database.clone(), |database| {
            database
                .add_text("commit notification", "test")
                .map_err(|error| error.to_string())
        })
        .await
        .expect("history entry");
        let id = HistoryOperations::entries_async(
            database.clone(),
            HistoryPageRequestOwned::new(
                HistoryCollection::All,
                None,
                None,
                None,
                None,
                Some(1),
                Some(0),
                1,
            ),
        )
        .await
        .expect("history page")[0]
            .id;
        let notifications = Arc::new(AtomicUsize::new(0));
        let callback_notifications = notifications.clone();

        HistoryOperations::delete_async_with_hook(database.clone(), id, move || {
            callback_notifications.fetch_add(1, Ordering::SeqCst);
        })
        .await
        .expect("delete entry");

        assert_eq!(notifications.load(Ordering::SeqCst), 1);
        assert!(HistoryOperations::entries_async(
            database,
            HistoryPageRequestOwned::new(
                HistoryCollection::All,
                None,
                None,
                None,
                None,
                Some(1),
                Some(0),
                1,
            ),
        )
        .await
        .expect("history page")
        .is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn failed_mutation_does_not_publish_a_revision() {
        let database = Arc::new(Mutex::new(
            HistoryDB::new_unavailable().expect("in-memory history database"),
        ));
        let notifications = Arc::new(AtomicUsize::new(0));
        let callback_notifications = notifications.clone();

        let result = HistoryOperations::delete_async_with_hook(database, 404, move || {
            callback_notifications.fetch_add(1, Ordering::SeqCst);
        })
        .await;

        assert!(result.is_err());
        assert_eq!(notifications.load(Ordering::SeqCst), 0);
    }
}
