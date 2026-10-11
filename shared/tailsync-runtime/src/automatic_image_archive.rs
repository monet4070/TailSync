use std::collections::VecDeque;
use std::sync::{Arc, Mutex as StdMutex};

use tailsync_core::cancellation::Cancellation;
use tailsync_core::composite::origin::Origin;
use tailsync_core::composite::resource_archive::ResourceTransport;
use tailsync_core::db::HistoryDB;
use tokio::sync::{watch, Mutex, Notify};

use crate::history::{ArchiveCommit, ArchiveEntryGuard, HistoryOperations};

const MAX_PENDING: usize = 8;

#[derive(Clone, Copy)]
pub struct CaptureTicket(u64);

#[derive(Default)]
pub struct CaptureTicketTracker {
    observation: Option<(i64, Option<CaptureTicket>)>,
}

impl CaptureTicketTracker {
    pub fn observe(
        &mut self,
        queue: &AutomaticImageArchive,
        revision: i64,
    ) -> Option<CaptureTicket> {
        if self
            .observation
            .is_none_or(|(observed, _)| observed != revision)
        {
            self.observation = Some((revision, queue.capture_ticket()));
        }
        self.observation.and_then(|(_, ticket)| ticket)
    }

    pub fn reset(&mut self) {
        self.observation = None;
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum EnqueueResult {
    Queued,
    Disabled,
    NotLocal,
    Full,
    Duplicate,
}

struct ArchiveJob {
    id: i64,
    guard: ArchiveEntryGuard,
}

struct QueueState {
    enabled: bool,
    closed: bool,
    running: bool,
    generation: u64,
    pending: VecDeque<ArchiveJob>,
    active: Option<(i64, Cancellation)>,
}

pub struct AutomaticImageArchive {
    state: StdMutex<QueueState>,
    changed: Notify,
    database: Arc<Mutex<HistoryDB>>,
    transport: Arc<dyn ResourceTransport>,
    on_commit: Arc<dyn Fn(ArchiveCommit) + Send + Sync>,
}

impl AutomaticImageArchive {
    pub fn new(
        database: Arc<Mutex<HistoryDB>>,
        enabled: bool,
        transport: Arc<dyn ResourceTransport>,
        on_commit: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        Self::with_commit_hook(database, enabled, transport, Arc::new(move |_| on_commit()))
    }

    pub fn with_commit_hook(
        database: Arc<Mutex<HistoryDB>>,
        enabled: bool,
        transport: Arc<dyn ResourceTransport>,
        on_commit: Arc<dyn Fn(ArchiveCommit) + Send + Sync>,
    ) -> Self {
        Self {
            state: StdMutex::new(QueueState {
                enabled,
                closed: false,
                running: false,
                generation: 0,
                pending: VecDeque::new(),
                active: None,
            }),
            changed: Notify::new(),
            database,
            transport,
            on_commit,
        }
    }

    pub fn committed(&self, commit: ArchiveCommit) {
        (self.on_commit)(commit);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, QueueState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn set_enabled(&self, enabled: bool) {
        let mut state = self.lock();
        if state.closed || state.enabled == enabled {
            return;
        }
        state.enabled = enabled;
        Self::invalidate(&mut state);
        self.changed.notify_one();
    }

    fn invalidate(state: &mut QueueState) {
        match state.generation.checked_add(1) {
            Some(generation) => state.generation = generation,
            None => state.closed = true,
        }
        state.pending.clear();
        if let Some((_, cancellation)) = &state.active {
            cancellation.cancel();
        }
    }

    pub fn cancel_pending(&self) {
        Self::invalidate(&mut self.lock());
        self.changed.notify_one();
    }

    pub fn cancel_entry(&self, id: i64) {
        let mut state = self.lock();
        state.pending.retain(|job| job.id != id);
        if let Some((active_id, cancellation)) = &state.active {
            if *active_id == id {
                cancellation.cancel();
            }
        }
    }

    pub fn capture_ticket(&self) -> Option<CaptureTicket> {
        let state = self.lock();
        (state.enabled && !state.closed).then_some(CaptureTicket(state.generation))
    }

    pub fn enqueue_captured(
        &self,
        ticket: CaptureTicket,
        origin: Origin,
        id: i64,
        guard: ArchiveEntryGuard,
    ) -> EnqueueResult {
        if origin != Origin::External {
            return EnqueueResult::NotLocal;
        }
        let mut state = self.lock();
        if !state.enabled || state.closed || ticket.0 != state.generation {
            return EnqueueResult::Disabled;
        }
        if state.pending.iter().any(|job| job.id == id)
            || state
                .active
                .as_ref()
                .is_some_and(|(active_id, _)| *active_id == id)
        {
            return EnqueueResult::Duplicate;
        }
        if state.pending.len() >= MAX_PENDING {
            return EnqueueResult::Full;
        }
        state.pending.push_back(ArchiveJob { id, guard });
        self.changed.notify_one();
        EnqueueResult::Queued
    }

    pub async fn run(&self, mut shutdown: watch::Receiver<bool>) {
        {
            let mut state = self.lock();
            if state.running || state.closed {
                return;
            }
            state.running = true;
        }
        let _stop_on_drop = StopOnDrop(self);
        loop {
            if *shutdown.borrow() {
                return;
            }
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let next = {
                let mut state = self.lock();
                if state.closed {
                    return;
                }
                state.pending.pop_front().map(|job| {
                    let cancellation = Cancellation::default();
                    state.active = Some((job.id, cancellation.clone()));
                    (job, cancellation)
                })
            };
            if let Some((job, cancellation)) = next {
                let on_commit = self.on_commit.clone();
                let result = tokio::select! {
                    biased;
                    _ = wait_for_shutdown(&mut shutdown) => return,
                    result = HistoryOperations::archive_captured_images_with_commit_hook(
                        self.database.clone(),
                        job.id,
                        job.guard,
                        self.transport.clone(),
                        cancellation,
                        move |commit| on_commit(commit),
                    ) => result,
                };
                self.lock().active = None;
                if let Err(error) = result {
                    tracing::debug!(code = %error, "automatic image archive stopped");
                }
            } else {
                tokio::select! {
                    biased;
                    _ = wait_for_shutdown(&mut shutdown) => return,
                    _ = &mut notified => {},
                }
            }
        }
    }
}

struct StopOnDrop<'a>(&'a AutomaticImageArchive);

impl Drop for StopOnDrop<'_> {
    fn drop(&mut self) {
        let mut state = self.0.lock();
        state.closed = true;
        state.enabled = false;
        AutomaticImageArchive::invalidate(&mut state);
    }
}

async fn wait_for_shutdown(shutdown: &mut watch::Receiver<bool>) {
    while !*shutdown.borrow_and_update() {
        if shutdown.changed().await.is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use tailsync_core::composite::resource_archive::{
        ArchiveError, ArchiveUrl, FetchFuture, FetchLimits, FetchResponse,
    };
    use tailsync_core::composite::{
        packed_to_png, Completeness, Mime, PortableBundle, Representation,
    };
    use tailsync_core::db::MigrationPolicy;

    struct MockTransport {
        calls: AtomicUsize,
        blocked: AtomicBool,
        started: Notify,
        release: Notify,
    }

    impl MockTransport {
        fn new(blocked: bool) -> Self {
            Self {
                calls: AtomicUsize::new(0),
                blocked: AtomicBool::new(blocked),
                started: Notify::new(),
                release: Notify::new(),
            }
        }
    }

    impl ResourceTransport for MockTransport {
        fn fetch_once<'a>(
            &'a self,
            _url: &'a ArchiveUrl,
            _limits: &'a FetchLimits,
            cancellation: &'a Cancellation,
        ) -> FetchFuture<'a> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                self.started.notify_one();
                if self.blocked.load(Ordering::SeqCst) {
                    tokio::select! {
                        biased;
                        _ = cancellation.cancelled() => return Err(ArchiveError::Cancelled),
                        _ = self.release.notified() => {},
                    }
                }
                cancellation.check().map_err(|_| ArchiveError::Cancelled)?;
                Ok(FetchResponse {
                    status: 200,
                    location: None,
                    content_type: Some("image/png".into()),
                    bytes: packed_to_png(1, 1, &[1, 2, 3, 255]).unwrap(),
                })
            })
        }
    }

    fn database() -> Arc<Mutex<HistoryDB>> {
        Arc::new(Mutex::new(
            HistoryDB::new_unavailable_with_policy(MigrationPolicy::CompositeAllowed).unwrap(),
        ))
    }

    fn bundle(body: &str) -> PortableBundle {
        PortableBundle {
            representations: vec![Representation {
                mime: Mime::Html,
                bytes: format!("<p>{body}</p><img src=\"https://fixture.example/a.png\">")
                    .into_bytes(),
            }],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::ExternalReferences,
        }
    }

    async fn capture(database: &Arc<Mutex<HistoryDB>>, body: &str) -> (i64, ArchiveEntryGuard) {
        let mut database = database.lock().await;
        let id = database
            .add_composite(&bundle(body), None, "local")
            .unwrap();
        (id, ArchiveEntryGuard::capture(&database, id).unwrap())
    }

    fn queue(
        database: Arc<Mutex<HistoryDB>>,
        enabled: bool,
        transport: Arc<MockTransport>,
        notifications: Arc<AtomicUsize>,
    ) -> Arc<AutomaticImageArchive> {
        Arc::new(AutomaticImageArchive::new(
            database,
            enabled,
            transport,
            Arc::new(move || {
                notifications.fetch_add(1, Ordering::SeqCst);
            }),
        ))
    }

    async fn wait_idle(queue: &AutomaticImageArchive) {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let idle = {
                    let state = queue.lock();
                    state.pending.is_empty() && state.active.is_none()
                };
                if idle {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
    }

    fn start(
        queue: Arc<AutomaticImageArchive>,
    ) -> (watch::Sender<bool>, tokio::task::JoinHandle<()>) {
        let (shutdown, receiver) = watch::channel(false);
        (
            shutdown,
            tokio::spawn(async move { queue.run(receiver).await }),
        )
    }

    #[tokio::test]
    async fn disabled_and_legacy_history_never_download_without_a_new_local_capture() {
        let database = database();
        capture(&database, "old history").await;
        let transport = Arc::new(MockTransport::new(false));
        let queue = queue(
            database,
            false,
            transport.clone(),
            Arc::new(AtomicUsize::new(0)),
        );
        assert!(queue.capture_ticket().is_none());
        let (shutdown, worker) = start(queue.clone());
        queue.set_enabled(true);
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
        shutdown.send(true).unwrap();
        worker.await.unwrap();
    }

    #[tokio::test]
    async fn origin_ticket_capacity_and_deduplication_are_enforced_before_dispatch() {
        let database = database();
        let transport = Arc::new(MockTransport::new(false));
        let queue = queue(
            database.clone(),
            true,
            transport,
            Arc::new(AtomicUsize::new(0)),
        );
        let ticket = queue.capture_ticket().unwrap();
        let (id, guard) = capture(&database, "first").await;
        for origin in [Origin::SelfWrite, Origin::Unknown, Origin::UnknownMismatch] {
            assert_eq!(
                queue.enqueue_captured(ticket, origin, id, guard.clone()),
                EnqueueResult::NotLocal
            );
        }
        assert_eq!(
            queue.enqueue_captured(ticket, Origin::External, id, guard.clone()),
            EnqueueResult::Queued
        );
        assert_eq!(
            queue.enqueue_captured(ticket, Origin::External, id, guard),
            EnqueueResult::Duplicate
        );
        for index in 1..MAX_PENDING {
            let (id, guard) = capture(&database, &format!("entry {index}")).await;
            assert_eq!(
                queue.enqueue_captured(ticket, Origin::External, id, guard),
                EnqueueResult::Queued
            );
        }
        let (id, guard) = capture(&database, "overflow").await;
        assert_eq!(
            queue.enqueue_captured(ticket, Origin::External, id, guard.clone()),
            EnqueueResult::Full
        );
        queue.set_enabled(false);
        queue.set_enabled(true);
        assert_eq!(
            queue.enqueue_captured(ticket, Origin::External, id, guard),
            EnqueueResult::Disabled
        );
        assert!(queue.lock().pending.is_empty());
    }

    #[tokio::test]
    async fn enabling_during_a_capture_retry_does_not_authorize_the_existing_observation() {
        let database = database();
        let queue = queue(
            database.clone(),
            false,
            Arc::new(MockTransport::new(false)),
            Arc::new(AtomicUsize::new(0)),
        );
        let mut tracker = CaptureTicketTracker::default();
        assert!(tracker.observe(&queue, 100).is_none());
        queue.set_enabled(true);
        assert!(tracker.observe(&queue, 100).is_none());
        let ticket = tracker.observe(&queue, 101).unwrap();
        queue.set_enabled(false);
        queue.set_enabled(true);
        let (id, guard) = capture(&database, "retry after disable").await;
        let stale = tracker.observe(&queue, 101).unwrap();
        assert_eq!(stale.0, ticket.0);
        assert_eq!(
            queue.enqueue_captured(stale, Origin::External, id, guard.clone()),
            EnqueueResult::Disabled
        );
        let fresh = tracker.observe(&queue, 102).unwrap();
        assert_eq!(
            queue.enqueue_captured(fresh, Origin::External, id, guard),
            EnqueueResult::Queued
        );
        tracker.reset();
        assert!(tracker.observe(&queue, 102).is_some());
    }

    #[tokio::test]
    async fn successful_archive_notifies_once_and_preserves_original_content() {
        let database = database();
        let (id, guard) = capture(&database, "new body").await;
        let original = database.lock().await.get_composite(id).unwrap();
        let notifications = Arc::new(AtomicUsize::new(0));
        let transport = Arc::new(MockTransport::new(false));
        let queue = queue(
            database.clone(),
            true,
            transport.clone(),
            notifications.clone(),
        );
        assert_eq!(
            queue.enqueue_captured(queue.capture_ticket().unwrap(), Origin::External, id, guard),
            EnqueueResult::Queued
        );
        let (shutdown, worker) = start(queue.clone());
        wait_idle(&queue).await;
        assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
        assert_eq!(notifications.load(Ordering::SeqCst), 1);
        let database = database.lock().await;
        assert_eq!(database.get_composite(id).unwrap(), original);
        let archived = database.get_composite_archived(id).unwrap().unwrap();
        assert_eq!(archived.resources.len(), 1);
        assert_eq!(archived.completeness, Completeness::Complete);
        drop(database);
        queue.set_enabled(false);
        assert!(queue
            .database
            .lock()
            .await
            .get_composite_archived(id)
            .unwrap()
            .is_some());
        shutdown.send(true).unwrap();
        worker.await.unwrap();
    }

    #[tokio::test]
    async fn disabling_cancels_active_and_pending_jobs_without_late_commit() {
        let database = database();
        let notifications = Arc::new(AtomicUsize::new(0));
        let transport = Arc::new(MockTransport::new(true));
        let queue = queue(
            database.clone(),
            true,
            transport.clone(),
            notifications.clone(),
        );
        let ticket = queue.capture_ticket().unwrap();
        let (id, guard) = capture(&database, "active").await;
        queue.enqueue_captured(ticket, Origin::External, id, guard);
        let (pending_id, guard) = capture(&database, "pending").await;
        queue.enqueue_captured(ticket, Origin::External, pending_id, guard);
        let (shutdown, worker) = start(queue.clone());
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            transport.started.notified(),
        )
        .await
        .unwrap();
        queue.set_enabled(false);
        transport.release.notify_one();
        wait_idle(&queue).await;
        assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
        assert_eq!(notifications.load(Ordering::SeqCst), 0);
        for id in [id, pending_id] {
            assert!(database
                .lock()
                .await
                .get_composite_archived(id)
                .unwrap()
                .is_none());
        }
        shutdown.send(true).unwrap();
        worker.await.unwrap();
    }

    #[tokio::test]
    async fn deletion_and_authority_replacement_reject_queued_work_before_network() {
        let database = database();
        let transport = Arc::new(MockTransport::new(false));
        let notifications = Arc::new(AtomicUsize::new(0));
        let queue = queue(
            database.clone(),
            true,
            transport.clone(),
            notifications.clone(),
        );
        let (id, guard) = capture(&database, "deleted").await;
        queue.enqueue_captured(queue.capture_ticket().unwrap(), Origin::External, id, guard);
        HistoryOperations::delete_async(database.clone(), id)
            .await
            .unwrap();
        let (id, guard) = capture(&database, "replaced database").await;
        queue.enqueue_captured(queue.capture_ticket().unwrap(), Origin::External, id, guard);
        let mut replacement =
            HistoryDB::new_unavailable_with_policy(MigrationPolicy::CompositeAllowed).unwrap();
        replacement
            .add_composite(&bundle("replacement row"), None, "remote")
            .unwrap();
        *database.lock().await = replacement;
        let (shutdown, worker) = start(queue.clone());
        wait_idle(&queue).await;
        assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
        assert_eq!(notifications.load(Ordering::SeqCst), 0);
        shutdown.send(true).unwrap();
        worker.await.unwrap();
    }

    #[tokio::test]
    async fn shutdown_and_worker_abort_cancel_active_archive() {
        for abort_worker in [false, true] {
            let database = database();
            let notifications = Arc::new(AtomicUsize::new(0));
            let transport = Arc::new(MockTransport::new(true));
            let queue = queue(
                database.clone(),
                true,
                transport.clone(),
                notifications.clone(),
            );
            let (id, guard) = capture(&database, "shutdown").await;
            queue.enqueue_captured(queue.capture_ticket().unwrap(), Origin::External, id, guard);
            let (shutdown, worker) = start(queue.clone());
            tokio::time::timeout(
                std::time::Duration::from_secs(5),
                transport.started.notified(),
            )
            .await
            .unwrap();
            if abort_worker {
                worker.abort();
                assert!(worker.await.unwrap_err().is_cancelled());
            } else {
                shutdown.send(true).unwrap();
                worker.await.unwrap();
            }
            transport.release.notify_one();
            assert!(queue.capture_ticket().is_none());
            assert_eq!(notifications.load(Ordering::SeqCst), 0);
            assert!(database
                .lock()
                .await
                .get_composite_archived(id)
                .unwrap()
                .is_none());
        }
    }

    #[tokio::test]
    async fn delete_and_clear_hooks_cancel_running_work_and_revoke_capture_tickets() {
        let database = database();
        let notifications = Arc::new(AtomicUsize::new(0));
        let transport = Arc::new(MockTransport::new(true));
        let queue = queue(
            database.clone(),
            true,
            transport.clone(),
            notifications.clone(),
        );
        let ticket = queue.capture_ticket().unwrap();
        let (id, guard) = capture(&database, "delete active").await;
        queue.enqueue_captured(ticket, Origin::External, id, guard);
        let (shutdown, worker) = start(queue.clone());
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            transport.started.notified(),
        )
        .await
        .unwrap();
        let queue_for_delete = queue.clone();
        HistoryOperations::delete_async_with_hook(database.clone(), id, move || {
            queue_for_delete.cancel_entry(id)
        })
        .await
        .unwrap();
        wait_idle(&queue).await;
        let queue_for_clear = queue.clone();
        HistoryOperations::clear_async_with_hook(database.clone(), move || {
            queue_for_clear.cancel_pending()
        })
        .await
        .unwrap();
        let (id, guard) = capture(&database, "after clear").await;
        assert_eq!(
            queue.enqueue_captured(ticket, Origin::External, id, guard),
            EnqueueResult::Disabled
        );
        assert_eq!(notifications.load(Ordering::SeqCst), 0);
        shutdown.send(true).unwrap();
        worker.await.unwrap();
    }

    #[tokio::test]
    async fn a_second_worker_cannot_bypass_the_single_active_job_limit() {
        let database = database();
        let transport = Arc::new(MockTransport::new(true));
        let queue = queue(
            database.clone(),
            true,
            transport.clone(),
            Arc::new(AtomicUsize::new(0)),
        );
        let (id, guard) = capture(&database, "single worker").await;
        queue.enqueue_captured(queue.capture_ticket().unwrap(), Origin::External, id, guard);
        let (shutdown, worker) = start(queue.clone());
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            transport.started.notified(),
        )
        .await
        .unwrap();
        let (_, second) = start(queue.clone());
        second.await.unwrap();
        assert!(queue.capture_ticket().is_some());
        assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
        shutdown.send(true).unwrap();
        worker.await.unwrap();
    }

    #[tokio::test]
    async fn a_manual_archive_supersedes_queued_automatic_work_before_another_request() {
        let database = database();
        let (id, guard) = capture(&database, "manual completion").await;
        let transport = Arc::new(MockTransport::new(false));
        HistoryOperations::archive_entry_images(
            database.clone(),
            id,
            transport.clone(),
            Cancellation::default(),
        )
        .await
        .unwrap();
        let completed = database.lock().await.composite_identity(id).unwrap();
        transport.calls.store(0, Ordering::SeqCst);
        let notifications = Arc::new(AtomicUsize::new(0));
        let queue = queue(
            database.clone(),
            true,
            transport.clone(),
            notifications.clone(),
        );
        queue.enqueue_captured(queue.capture_ticket().unwrap(), Origin::External, id, guard);
        let (shutdown, worker) = start(queue.clone());
        wait_idle(&queue).await;
        assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
        assert_eq!(notifications.load(Ordering::SeqCst), 0);
        assert_eq!(
            database.lock().await.composite_identity(id).unwrap(),
            completed
        );
        shutdown.send(true).unwrap();
        worker.await.unwrap();
    }

    #[tokio::test]
    async fn reopening_the_same_record_with_identical_identity_invalidates_its_capture_authority() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "tailsync-auto-authority-{}-{nonce}",
            std::process::id()
        ));
        let database = Arc::new(Mutex::new(
            HistoryDB::open_isolated_for_test_composite(&root).unwrap(),
        ));
        let (id, guard) = capture(&database, "same persisted record").await;
        let identity = database.lock().await.composite_identity(id).unwrap();
        let transport = Arc::new(MockTransport::new(false));
        let notifications = Arc::new(AtomicUsize::new(0));
        let queue = queue(
            database.clone(),
            true,
            transport.clone(),
            notifications.clone(),
        );
        queue.enqueue_captured(queue.capture_ticket().unwrap(), Origin::External, id, guard);
        let reopened =
            HistoryDB::open_with_policy_for_probe(&root, MigrationPolicy::CompositeAllowed)
                .unwrap();
        assert_eq!(reopened.composite_identity(id).unwrap(), identity);
        *database.lock().await = reopened;
        let (shutdown, worker) = start(queue.clone());
        wait_idle(&queue).await;
        assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
        assert_eq!(notifications.load(Ordering::SeqCst), 0);
        shutdown.send(true).unwrap();
        worker.await.unwrap();
        drop(queue);
        drop(database);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn committed_notification_survives_cancellation_of_the_waiter() {
        let database = database();
        let (id, guard) = capture(&database, "committed notification").await;
        let notifications = Arc::new(AtomicUsize::new(0));
        let cancellation = Cancellation::default();
        let cancel_after_commit = cancellation.clone();
        let on_commit = notifications.clone();
        let _result = HistoryOperations::archive_captured_images_with_hook(
            database.clone(),
            id,
            guard,
            Arc::new(MockTransport::new(false)),
            cancellation,
            move || {
                on_commit.fetch_add(1, Ordering::SeqCst);
                cancel_after_commit.cancel();
            },
        )
        .await;
        assert_eq!(notifications.load(Ordering::SeqCst), 1);
        assert!(database
            .lock()
            .await
            .get_composite_archived(id)
            .unwrap()
            .is_some());
    }
}
