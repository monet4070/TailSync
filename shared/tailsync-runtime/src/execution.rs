use std::fmt;
use std::sync::{Arc, OnceLock};
use std::time::Instant;
use tailsync_core::db::HistoryDB;
use tailsync_core::{cancellation::Cancellation, db::HistoryReadError};
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};

const MAX_DB_WORKERS: usize = 4;

static DB_PERMITS: OnceLock<Arc<Semaphore>> = OnceLock::new();
static READ_PERMITS: OnceLock<Arc<Semaphore>> = OnceLock::new();

/// Dropping a read future invalidates queued work and cooperatively stops its
/// blocking worker. Mutations deliberately do not use this guard.
struct CancelOnDrop(Cancellation);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

pub struct ReadContext {
    database: Arc<Mutex<HistoryDB>>,
    pub cancellation: Cancellation,
    operation: &'static str,
}

impl ReadContext {
    pub fn with_database<T>(
        &self,
        operation: impl FnOnce(&HistoryDB) -> Result<T, HistoryReadError>,
    ) -> Result<T, HistoryReadError> {
        let waiting = Instant::now();
        // This runs inside the blocking worker. No guard crosses threads;
        // waiting on the async mutex remains cancellable and FIFO.
        let database = tokio::runtime::Handle::current().block_on(async {
            tokio::select! {
                biased;
                _ = self.cancellation.cancelled() => Err(HistoryReadError::Cancelled),
                database = self.database.lock() => Ok(database),
            }
        })?;
        self.cancellation.check()?;
        let acquired = Instant::now();
        let result = operation(&database);
        drop(database);
        tracing::debug!(target: "tailsync.runtime.execution", operation = self.operation,
            db_lock_wait_ms = acquired.duration_since(waiting).as_secs_f64() * 1000.0,
            db_hold_ms = acquired.elapsed().as_secs_f64() * 1000.0, "read database timing");
        result
    }
}

pub async fn run_read<T: Send + 'static>(
    operation: &'static str,
    database: Arc<Mutex<HistoryDB>>,
    read: impl FnOnce(ReadContext) -> Result<T, HistoryReadError> + Send + 'static,
) -> Result<T, HistoryReadError> {
    let cancellation = Cancellation::default();
    let _scope = CancelOnDrop(cancellation.clone());
    let queued = Instant::now();
    // Readers have a separate budget so decrypting large payloads cannot
    // occupy every mutation permit while their database lock is released.
    let permit = READ_PERMITS
        .get_or_init(|| Arc::new(Semaphore::new(2)))
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| HistoryReadError::Operation("read queue closed".into()))?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        cancellation.check()?;
        tracing::debug!(target: "tailsync.runtime.execution", operation,
            queue_wait_ms = queued.elapsed().as_secs_f64() * 1000.0, "read queue timing");
        read(ReadContext {
            database,
            cancellation,
            operation,
        })
    })
    .await
    .map_err(|error| HistoryReadError::Operation(error.to_string()))?
}

fn db_permits() -> Arc<Semaphore> {
    DB_PERMITS
        .get_or_init(|| Arc::new(Semaphore::new(MAX_DB_WORKERS)))
        .clone()
}

#[derive(Debug)]
pub enum DbExecutionError<E> {
    QueueClosed,
    Worker(String),
    Operation(E),
}

impl<E: fmt::Display> fmt::Display for DbExecutionError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::QueueClosed => formatter.write_str("database execution queue is closed"),
            Self::Worker(error) => write!(formatter, "database worker failed: {error}"),
            Self::Operation(error) => error.fmt(formatter),
        }
    }
}

/// Run one database operation away from the async executor.
///
/// The permit is acquired before spawning a blocking task, so a burst of IPC
/// requests cannot create an unbounded set of workers waiting on the same
/// database mutex. The async mutex guard is acquired inside the blocking
/// closure and is never moved across an await boundary.
pub async fn run_db<T, E, F>(
    database: Arc<Mutex<HistoryDB>>,
    operation: F,
) -> Result<T, DbExecutionError<E>>
where
    T: Send + 'static,
    E: Send + 'static,
    F: FnOnce(&mut HistoryDB) -> Result<T, E> + Send + 'static,
{
    run_db_named("database", database, operation).await
}

/// Run a named database operation and emit timing-only diagnostics.
///
/// The fields contain queue/lock/operation durations and an outcome, never
/// clipboard contents, request data, capability tokens, or file paths.
pub async fn run_db_named<T, E, F>(
    operation_name: &'static str,
    database: Arc<Mutex<HistoryDB>>,
    operation: F,
) -> Result<T, DbExecutionError<E>>
where
    T: Send + 'static,
    E: Send + 'static,
    F: FnOnce(&mut HistoryDB) -> Result<T, E> + Send + 'static,
{
    let queued_at = Instant::now();
    let permit = db_permits()
        .acquire_owned()
        .await
        .map_err(|_| DbExecutionError::QueueClosed)?;
    let queue_wait = queued_at.elapsed();
    let result = tokio::task::spawn_blocking(move || {
        run_with_permit(operation_name, queue_wait, database, permit, operation)
    })
    .await
    .map_err(|error| DbExecutionError::Worker(error.to_string()))?;
    result.map_err(DbExecutionError::Operation)
}

fn run_with_permit<T, E, F>(
    operation_name: &'static str,
    queue_wait: std::time::Duration,
    database: Arc<Mutex<HistoryDB>>,
    _permit: OwnedSemaphorePermit,
    operation: F,
) -> Result<T, E>
where
    F: FnOnce(&mut HistoryDB) -> Result<T, E>,
{
    let lock_wait_started = Instant::now();
    let mut database = database.blocking_lock();
    let lock_wait = lock_wait_started.elapsed();
    let operation_started = Instant::now();
    let result = operation(&mut database);
    tracing::debug!(
        target: "tailsync.runtime.execution",
        operation = operation_name,
        queue_wait_ms = queue_wait.as_secs_f64() * 1000.0,
        db_lock_wait_ms = lock_wait.as_secs_f64() * 1000.0,
        db_hold_ms = operation_started.elapsed().as_secs_f64() * 1000.0,
        outcome = if result.is_ok() { "ok" } else { "error" },
        "database operation timing"
    );
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dropped_read_releases_a_worker_waiting_on_the_database_lock() {
        let database = Arc::new(Mutex::new(HistoryDB::new_unavailable().unwrap()));
        let guard = database.lock().await;
        let (entered, entering) = tokio::sync::oneshot::channel();
        let (stopped, stopping) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(run_read("cancel-test", database.clone(), move |context| {
            let _ = entered.send(());
            let result = context.with_database(|_| Ok(()));
            let _ = stopped.send(matches!(result, Err(HistoryReadError::Cancelled)));
            result
        }));
        entering.await.unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        // The DB stays locked: cancellation must stop waiting, not wait for
        // the holder to release it and only then notice the closed client.
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(1), stopping)
                .await
                .unwrap()
                .unwrap()
        );
        drop(guard);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn runs_operation_with_database_guard_inside_blocking_worker() {
        let database = Arc::new(Mutex::new(
            HistoryDB::new_unavailable().expect("in-memory history database"),
        ));
        let result = run_db(database, |database| {
            database
                .add_text("runtime execution", "test")
                .map_err(|error| error.to_string())?;
            database
                .get_all(None, None, 1, 0)
                .map(|entries| entries.len())
                .map_err(|error| error.to_string())
        })
        .await
        .expect("database operation");

        assert_eq!(result, 1);
    }
}
