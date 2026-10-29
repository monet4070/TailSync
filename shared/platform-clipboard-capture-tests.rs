// Shared composite-capture monitor tests (F3-B).
//
// These shared behaviour tests exercise the production capture transaction
// (`process_composite_capture`) that the real clipboard monitor calls. They are
// byte-identical between macOS and Windows, so they live in this canonical
// include pasted inside each wrapper's `composite_capture::tests` module, next
// to the platform fixtures, `Shape`/`OriginMode` matrix, real CNA1/CNS1 producer,
// isolated SQLite store, retry/ack/warning ledger and the real queue sender they
// use. Each platform still drives its own legal producer and storage path.
// CNA1 archive-provider regressions live in the macOS fixture's private include.

#[tokio::test]
async fn missing_formats_retries_are_bounded_then_saved_incomplete() {
    let _serial = capture_serial().lock().await;
    let _ = drain_warnings();
    let fx = fixture(Shape::MissingFormats, OriginMode::External, false, true).await;
    let mut queue = install_capture_sender(&fx.pool).await;

    for _ in 0..3 {
        assert_eq!(fx.tick(100).await, CompositeCapture::Handled);
        assert_eq!(fx.history_len().await, 0, "retries must not save yet");
        assert!(
            fx.acks().is_empty(),
            "a retry must not acknowledge the copy"
        );
        assert!(
            fx.faults().is_empty(),
            "only an exhausted capture is faulted"
        );
        assert!(
            queue.priority.try_recv().is_err(),
            "a retry cannot broadcast"
        );
        fx.advance_clock_ms(2000);
    }
    assert_eq!(fx.tick(100).await, CompositeCapture::Handled);
    assert_eq!(
        fx.history_len().await,
        1,
        "the last attempt saves incomplete history"
    );
    assert_eq!(
        fx.faults(),
        vec![(100, "capture_incomplete")],
        "the incomplete capture stays visible"
    );
    assert!(drain_warnings().contains(&"capture_failed"));
    assert_eq!(fx.acks(), vec![100]);
    let queued = queue
        .priority
        .try_recv()
        .expect("saved incomplete capture is broadcast");
    assert_eq!(queued.command(), Command::CompositePayload);
    assert!(
        queue.priority.try_recv().is_err(),
        "only one payload is queued"
    );
}

#[tokio::test]
async fn portable_save_failure_retries_without_ack_until_a_durable_fault() {
    let _serial = capture_serial().lock().await;
    let _ = drain_warnings();
    let fx = fixture(Shape::Portable, OriginMode::External, false, true).await;
    let mut queue = install_capture_sender(&fx.pool).await;
    fx.database.lock().await.mark_storage_unavailable();

    for _ in 0..3 {
        assert_eq!(fx.tick(100).await, CompositeCapture::Handled);
        assert!(
            drain_warnings().contains(&"capture_not_saved"),
            "every failed save must be visible before retries are exhausted"
        );
        assert!(
            fx.acks().is_empty(),
            "a retry must not acknowledge a failed save"
        );
        assert!(
            fx.faults().is_empty(),
            "retry allowance remains before a fault"
        );
        assert!(
            queue.priority.try_recv().is_err(),
            "a failed save cannot broadcast"
        );
        fx.advance_clock_ms(2000);
    }
    assert!(fx.acks().is_empty(), "a failed save is never acknowledged");
    assert_eq!(fx.history_len().await, 0);

    // Retries exhausted; the durable fault sink now reports failure, so the
    // copy must stay unacknowledged and un-broadcast.
    assert_eq!(fx.tick(100).await, CompositeCapture::Handled);
    assert!(drain_warnings().contains(&"capture_not_saved"));
    assert_eq!(fx.faults(), vec![(100, "capture_not_saved")]);
    assert!(
        fx.acks().is_empty(),
        "a failed fault record must not acknowledge"
    );
    assert!(queue.priority.try_recv().is_err());

    // A durable fault finally succeeds and only then acknowledges.
    fx.fault_ok.store(true, Ordering::SeqCst);
    assert_eq!(fx.tick(100).await, CompositeCapture::Handled);
    assert!(drain_warnings().contains(&"capture_not_saved"));
    assert_eq!(fx.acks(), vec![100]);
    assert_eq!(
        fx.faults(),
        vec![(100, "capture_not_saved"), (100, "capture_not_saved")]
    );
    assert!(
        queue.priority.try_recv().is_err(),
        "a dropped capture is never broadcast"
    );
}

#[tokio::test]
async fn save_superseded_during_storage_is_not_broadcast_or_re_acknowledged() {
    let _serial = capture_serial().lock().await;
    let fx = fixture(Shape::Portable, OriginMode::External, false, true).await;
    let mut queue = install_capture_sender(&fx.pool).await;

    // The injected classifier runs after the provider, the first revision
    // check and the origin decision, and immediately before the generation
    // supersede and the save. Waiting there (not on a later revision read,
    // which the held store would block) is the exact pre-save rendezvous.
    let hold = fx.database.lock().await;
    let mut task = Box::pin(fx.tick(100));
    tokio::select! {
        _ = wait_for_signal(&fx.classify_notify, "the storage rendezvous before the save") => {}
        _ = &mut task => panic!("capture finished while storage was held"),
    }
    fx.set_revision(101);
    drop(hold);

    assert_eq!(task.await, CompositeCapture::Handled);
    assert_eq!(
        fx.history_len().await,
        1,
        "the superseded capture is still stored"
    );
    assert_eq!(
        fx.acks(),
        vec![100],
        "only the observed copy may be acknowledged, never the newer revision"
    );
    assert!(!fx.acks().contains(&101));
    assert!(
        queue.priority.try_recv().is_err(),
        "a capture superseded during storage is never broadcast"
    );
}

// F3-C: the post-save publication stage. These regressions also drive the
// production `process_composite_capture` seam and observe only public results:
// the real automatic-archive queue, the real capture queue and the real
// SQLite binding. The counting transport below is the one offline test hook;
// it never reaches the network.

/// An offline transport that only counts fetch attempts, so a regression can
/// prove a stale archive guard is refused *before* any network work.
struct CountingTransport {
    fetches: Arc<std::sync::atomic::AtomicUsize>,
}

impl ResourceTransport for CountingTransport {
    fn fetch_once<'a>(
        &'a self,
        _url: &'a ArchiveUrl,
        _limits: &'a FetchLimits,
        _cancellation: &'a Cancellation,
    ) -> FetchFuture<'a> {
        Box::pin(async move {
            self.fetches.fetch_add(1, Ordering::SeqCst);
            Err(ArchiveError::Transport)
        })
    }
}

#[tokio::test]
async fn portable_html_capture_is_archived_through_the_production_seam() {
    let _serial = capture_serial().lock().await;
    let fx = fixture(Shape::HtmlImage, OriginMode::External, true, true).await;
    let mut queue = install_capture_sender(&fx.pool).await;

    assert_eq!(fx.tick(100).await, CompositeCapture::Handled);
    assert_eq!(fx.history_len().await, 1);
    assert_eq!(fx.acks(), vec![100]);

    // The production seam already enqueued this exact saved source: asking the
    // real archive queue to enqueue it again can only report a duplicate.
    let id = fx.history_id().await;
    let ticket = fx
        .observe_ticket(100)
        .expect("an enabled archive issues a ticket");
    let guard = {
        let db = fx.database.lock().await;
        tailsync_runtime::history::ArchiveEntryGuard::capture(&db, id)
    }
    .expect("the saved source binds a real archive guard");
    assert_eq!(
        fx.archive
            .enqueue_captured(ticket, Origin::External, id, guard),
        tailsync_runtime::automatic_image_archive::EnqueueResult::Duplicate,
        "the capture seam already enqueued this exact saved source"
    );

    let queued = queue
        .priority
        .try_recv()
        .expect("the html capture is broadcast");
    assert_eq!(queued.command(), Command::CompositePayload);
}

#[tokio::test]
async fn archive_generation_invalidated_during_storage_keeps_the_old_ticket_out() {
    let _serial = capture_serial().lock().await;
    use tailsync_runtime::automatic_image_archive::EnqueueResult;
    let fx = fixture(Shape::HtmlImage, OriginMode::External, true, true).await;
    let mut queue = install_capture_sender(&fx.pool).await;

    let hold = fx.database.lock().await;
    let mut task = Box::pin(fx.tick(100));
    tokio::select! {
        _ = wait_for_signal(&fx.classify_notify, "the storage rendezvous before the save") => {}
        _ = &mut task => panic!("capture finished while storage was held"),
    }
    // Invalidate the archive generation while the capture waits for storage:
    // the ticket the seam already holds belongs to the replaced generation.
    fx.archive.cancel_pending();
    drop(hold);

    assert_eq!(task.await, CompositeCapture::Handled);
    assert_eq!(fx.history_len().await, 1, "the save still completes");
    assert_eq!(fx.acks(), vec![100]);

    let id = fx.history_id().await;
    let guard = {
        let db = fx.database.lock().await;
        tailsync_runtime::history::ArchiveEntryGuard::capture(&db, id)
    }
    .expect("the saved source binds a real archive guard");
    let stale = fx
        .observe_ticket(100)
        .expect("the seam captured a ticket before the invalidation");
    assert_eq!(
        fx.archive
            .enqueue_captured(stale, Origin::External, id, guard.clone()),
        EnqueueResult::Disabled,
        "an invalidated generation ticket must not enqueue"
    );
    let current = fx
        .archive
        .capture_ticket()
        .expect("the archive is still enabled");
    assert_eq!(
        fx.archive
            .enqueue_captured(current, Origin::External, id, guard),
        EnqueueResult::Queued,
        "the replaced-generation source was never enqueued by the seam"
    );

    let queued = queue
        .priority
        .try_recv()
        .expect("the html capture is broadcast");
    assert_eq!(queued.command(), Command::CompositePayload);
}

#[tokio::test]
async fn broadcast_payload_matches_the_stored_local_source_binding() {
    let _serial = capture_serial().lock().await;
    use tailsync_core::composite::{Completeness, Mime, PortableBundle, Representation};
    use tailsync_core::peer::delivery::AckExpectation;
    let mut fx = fixture(Shape::HtmlImage, OriginMode::External, true, true).await;
    let probe = WorkerProbe::install(&mut fx);
    let mut queue = install_capture_sender(&fx.pool).await;

    assert_eq!(fx.tick(100).await, CompositeCapture::Handled);
    let queued = queue
        .priority
        .try_recv()
        .expect("the html capture is broadcast");
    assert_eq!(queued.command(), Command::CompositePayload);
    let message_id = match queued.acknowledgement() {
        AckExpectation::Event(message_id) => message_id,
        other => {
            panic!("a composite broadcast must await its event acknowledgement, got {other:?}")
        }
    };

    let id = fx.history_id().await;
    let mut db = fx.database.lock().await;
    let stored = db.get_composite(id).expect("the saved source is stored");
    let wire = PortableBundle::decode(
        &queued.payload()[tailsync_core::protocol::EVENT_ENVELOPE_HEADER_SIZE..],
    )
    .expect("the broadcast payload is a portable bundle");
    assert_eq!(
        wire.digest().expect("wire digest"),
        stored.digest().expect("stored digest"),
        "the wire payload is the stored local source the event is bound to"
    );

    // The wire event is bound to the saved source: rebinding that same event
    // under another stored row can only be refused as already bound.
    let second = PortableBundle {
        representations: vec![Representation {
            mime: Mime::Text,
            bytes: b"f3c second source".to_vec(),
        }],
        resources: vec![],
        bindings: vec![],
        completeness: Completeness::Complete,
    };
    let second_id = db
        .add_composite(&second, None, "self")
        .expect("a second stored source");
    let second_identity = db
        .composite_identity(second_id)
        .expect("second identity query")
        .expect("the second source has an identity");
    let second_digest = db
        .get_composite(second_id)
        .expect("second source read")
        .digest()
        .expect("second source digest");
    let error = db
        .bind_composite_outgoing_event(second_id, &second_identity, message_id, second_digest, 0)
        .expect_err("the wire event is already bound to the saved source");
    assert!(
        error.to_string().contains("already bound"),
        "the broadcast event must already be bound to the saved source: {error}"
    );
    drop(db);

    // Drive the actual queued archive to a successful offline commit, then
    // verify the public exporter consumes the same persisted event binding.
    probe.complete_one(&fx.archive).await;
    assert_eq!(*probe.commits.lock().unwrap(), vec![id]);
    let db = fx.database.lock().await;
    let identity = db.composite_identity(id).unwrap().unwrap();
    let update = db
        .export_composite_archive_update(id, &identity)
        .unwrap()
        .expect("the committed image has an outgoing archive binding");
    assert_eq!(update.base_event, message_id);
    assert_eq!(update.base_digest, wire.digest().unwrap());
    assert_eq!(update.images.len(), 1);
}

#[tokio::test]
async fn archive_operation_refuses_the_old_guard_of_a_replaced_source() {
    let _serial = capture_serial().lock().await;
    let fx = fixture(Shape::HtmlImage, OriginMode::External, true, false).await;
    assert_eq!(fx.tick(100).await, CompositeCapture::Handled);
    assert_eq!(fx.history_len().await, 1);
    let id = fx.history_id().await;

    // Capture the guard the seam would archive with, then replace the stored
    // source (a new archive revision) before the worker runs.
    let guard = {
        let db = fx.database.lock().await;
        tailsync_runtime::history::ArchiveEntryGuard::capture(&db, id)
    }
    .expect("the saved source binds a real archive guard");
    let replaced = {
        let mut db = fx.database.lock().await;
        let identity = db
            .composite_identity(id)
            .expect("identity query")
            .expect("the saved source has an identity");
        db.update_composite_archive_summary(id, &identity, "replaced after save")
            .expect("the source is replaced")
    };
    assert!(replaced, "the stored source must actually change");

    let fetches = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let committed = Arc::new(AtomicBool::new(false));
    let committed_sink = committed.clone();
    let result =
        tailsync_runtime::history::HistoryOperations::archive_captured_images_with_commit_hook(
            fx.database.clone(),
            id,
            guard,
            Arc::new(CountingTransport {
                fetches: fetches.clone(),
            }),
            Cancellation::default(),
            move |_| committed_sink.store(true, Ordering::SeqCst),
        )
        .await;
    assert_eq!(result.unwrap_err(), "archive_superseded");
    assert_eq!(
        fetches.load(Ordering::SeqCst),
        0,
        "a replaced source is refused before any network fetch"
    );
    assert!(
        !committed.load(Ordering::SeqCst),
        "a replaced source never commits an archive"
    );
}

struct WorkerProbe {
    fetches: Arc<std::sync::Mutex<Vec<String>>>,
    commits: Arc<std::sync::Mutex<Vec<i64>>>,
    committed: Arc<tokio::sync::Notify>,
    capture_attempts: Arc<std::sync::atomic::AtomicUsize>,
}

struct SuccessfulTransport {
    fetches: Arc<std::sync::Mutex<Vec<String>>>,
    capture_attempts: Arc<std::sync::atomic::AtomicUsize>,
    capture_succeeds: bool,
}

impl ResourceTransport for SuccessfulTransport {
    fn fetch_once<'a>(
        &'a self,
        url: &'a ArchiveUrl,
        limits: &'a FetchLimits,
        _cancellation: &'a Cancellation,
    ) -> FetchFuture<'a> {
        Box::pin(async move {
            // Existing background-worker scenarios model a resource that fails
            // the short first-publish attempt but is available on a later save.
            if limits.request_timeout < std::time::Duration::from_secs(1) {
                self.capture_attempts.fetch_add(1, Ordering::SeqCst);
                if !self.capture_succeeds {
                    return Err(ArchiveError::Transport);
                }
            }
            self.fetches.lock().unwrap().push(url.as_str().to_owned());
            Ok(tailsync_core::composite::resource_archive::FetchResponse {
                status: 200,
                location: None,
                content_type: Some("image/png".into()),
                bytes: tailsync_core::composite::packed_to_png(1, 1, &[1, 2, 3, 255]).unwrap(),
            })
        })
    }
}

impl WorkerProbe {
    fn install(fx: &mut Fixture) -> Self {
        Self::install_with_capture(fx, false)
    }

    fn install_with_capture(fx: &mut Fixture, capture_succeeds: bool) -> Self {
        let probe = Self {
            fetches: Arc::default(),
            commits: Arc::default(),
            committed: Arc::default(),
            capture_attempts: Arc::default(),
        };
        let commits = probe.commits.clone();
        let committed = probe.committed.clone();
        fx.archive = Arc::new(AutomaticImageArchive::with_commit_hook(
            fx.database.clone(),
            true,
            Arc::new(SuccessfulTransport {
                fetches: probe.fetches.clone(),
                capture_attempts: probe.capture_attempts.clone(),
                capture_succeeds,
            }),
            Arc::new(move |commit| {
                commits.lock().unwrap().push(commit.id);
                committed.notify_one();
            }),
        ));
        probe
    }

    async fn complete_one(&self, archive: &Arc<AutomaticImageArchive>) {
        let (shutdown, receiver) = tokio::sync::watch::channel(false);
        let worker = archive.run(receiver);
        tokio::pin!(worker);
        tokio::select! {
            _ = wait_for_signal(&self.committed, "the real archive worker commit") => {}
            _ = &mut worker => panic!("archive worker stopped before a commit"),
        }
        shutdown.send(true).unwrap();
        tokio::time::timeout(TEST_WAIT, worker)
            .await
            .expect("archive worker did not stop");
    }
}

#[tokio::test]
async fn url_image_is_in_the_first_frozen_event_and_the_original_capture_is_retained() {
    let _serial = capture_serial().lock().await;
    let mut fx = fixture(Shape::HtmlImage, OriginMode::External, true, true).await;
    let probe = WorkerProbe::install_with_capture(&mut fx, true);
    let mut queue = install_capture_sender(&fx.pool).await;
    assert_eq!(fx.tick(100).await, CompositeCapture::Handled);
    let queued = queue.priority.try_recv().expect("one complete first event");
    let wire = tailsync_core::composite::PortableBundle::decode(
        &queued.payload()[tailsync_core::protocol::EVENT_ENVELOPE_HEADER_SIZE..],
    )
    .unwrap();
    assert_eq!(
        wire.completeness,
        tailsync_core::composite::Completeness::Complete
    );
    assert_eq!(wire.bindings.len(), 1);
    assert_eq!(wire.resources.len(), 1);
    assert!(tailsync_core::composite::restore::dependencies_complete(&wire).unwrap());
    assert!(
        queue.priority.try_recv().is_err(),
        "no text-first or second image event"
    );
    assert_eq!(probe.capture_attempts.load(Ordering::SeqCst), 1);
    assert_eq!(probe.fetches.lock().unwrap().len(), 1);
    assert!(
        probe.commits.lock().unwrap().is_empty(),
        "no background update was needed"
    );
    let id = fx.history_id().await;
    let db = fx.database.lock().await;
    assert_eq!(db.get_composite(id).unwrap(), wire);
    assert_eq!(db.get_composite_network_originals(id).unwrap().len(), 1);
    let native = db.get_composite_native(id).unwrap().unwrap();
    assert!(native
        .formats
        .iter()
        .any(|format| String::from_utf8_lossy(&format.bytes)
            .contains("https://fixture.example/capture-image.png")));
    let identity = db.composite_identity(id).unwrap().unwrap();
    assert!(db
        .export_composite_archive_update(id, &identity)
        .unwrap()
        .is_none());
}

struct PendingCaptureTransport {
    started: tokio::sync::Notify,
}

impl ResourceTransport for PendingCaptureTransport {
    fn fetch_once<'a>(
        &'a self,
        _url: &'a ArchiveUrl,
        _limits: &'a FetchLimits,
        cancellation: &'a Cancellation,
    ) -> FetchFuture<'a> {
        Box::pin(async move {
            self.started.notify_one();
            cancellation.cancelled().await;
            Err(ArchiveError::Cancelled)
        })
    }
}

#[tokio::test]
async fn new_copy_or_restore_cancels_image_preparation_without_sending_the_old_article() {
    let _serial = capture_serial().lock().await;
    for new_copy in [true, false] {
        let mut fx = fixture(Shape::HtmlImage, OriginMode::External, true, true).await;
        let transport = Arc::new(PendingCaptureTransport {
            started: tokio::sync::Notify::new(),
        });
        fx.archive = Arc::new(AutomaticImageArchive::new(
            fx.database.clone(),
            true,
            transport.clone(),
            Arc::new(|| {}),
        ));
        let mut queue = install_capture_sender(&fx.pool).await;
        let task = fx.tick(100);
        tokio::pin!(task);
        tokio::select! {
            _ = wait_for_signal(&transport.started, "first-publish image fetch") => {},
            _ = &mut task => panic!("the image fetch should wait"),
        }
        if new_copy {
            fx.revision.store(101, Ordering::SeqCst);
        } else {
            fx.engine.lock().await.supersede_file_clipboard();
        }
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_millis(300), task)
                .await
                .unwrap(),
            CompositeCapture::Handled
        );
        assert_eq!(
            fx.history_len().await,
            1,
            "preserve the already captured source"
        );
        assert_eq!(fx.acks(), vec![100]);
        assert!(
            queue.priority.try_recv().is_err(),
            "the old article must not be sent"
        );
    }
}

#[tokio::test]
async fn parent_superseded_html_still_archives_the_source_it_saved() {
    let _serial = capture_serial().lock().await;
    let fx = fixture(Shape::HtmlImage, OriginMode::External, true, true).await;
    let mut queue = install_capture_sender(&fx.pool).await;
    let hold = fx.database.lock().await;
    let mut task = Box::pin(fx.tick(100));
    tokio::select! {
        _ = wait_for_signal(&fx.classify_notify, "the storage rendezvous before the save") => {}
        _ = &mut task => panic!("capture finished while storage was held"),
    }
    fx.set_revision(101);
    drop(hold);
    assert_eq!(task.await, CompositeCapture::Handled);
    assert_eq!(fx.acks(), vec![100]);
    assert_eq!(fx.history_len().await, 1);
    assert!(
        queue.priority.try_recv().is_err(),
        "the old copy cannot broadcast"
    );
    let id = fx.history_id().await;
    let guard =
        tailsync_runtime::history::ArchiveEntryGuard::capture(&*fx.database.lock().await, id)
            .unwrap();
    assert_eq!(
        fx.archive
            .enqueue_captured(fx.observe_ticket(100).unwrap(), Origin::External, id, guard),
        tailsync_runtime::automatic_image_archive::EnqueueResult::Duplicate,
        "the early return still enqueues the real stored local source"
    );
}

#[tokio::test]
async fn parent_real_archive_worker_rejects_the_queued_old_guard_and_commits_a_control() {
    let _serial = capture_serial().lock().await;
    use tailsync_core::composite::{Completeness, Mime, PortableBundle, Representation};
    let mut fx = fixture(Shape::HtmlImage, OriginMode::External, true, false).await;
    let probe = WorkerProbe::install(&mut fx);
    assert_eq!(fx.tick(100).await, CompositeCapture::Handled);
    let old_id = fx.history_id().await;
    let (control_id, control_guard) = {
        let mut db = fx.database.lock().await;
        let old_guard = tailsync_runtime::history::ArchiveEntryGuard::capture(&db, old_id).unwrap();
        assert_eq!(
            fx.archive.enqueue_captured(
                fx.observe_ticket(100).unwrap(),
                Origin::External,
                old_id,
                old_guard
            ),
            tailsync_runtime::automatic_image_archive::EnqueueResult::Duplicate,
            "the first job must come from the actual capture transaction"
        );
        let identity = db.composite_identity(old_id).unwrap().unwrap();
        assert!(db
            .update_composite_archive_summary(old_id, &identity, "replaced before queued worker")
            .unwrap());
        let control = PortableBundle {
            representations: vec![Representation {
                mime: Mime::Html,
                bytes: b"<p>positive control</p><img src=\"https://fixture.example/parent-control.png\">".to_vec(),
            }],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::ExternalReferences,
        };
        let id = db.add_composite(&control, None, "self").unwrap();
        assert_ne!(id, old_id);
        (
            id,
            tailsync_runtime::history::ArchiveEntryGuard::capture(&db, id).unwrap(),
        )
    };
    assert_eq!(
        fx.archive.enqueue_captured(
            fx.archive.capture_ticket().unwrap(),
            Origin::External,
            control_id,
            control_guard
        ),
        tailsync_runtime::automatic_image_archive::EnqueueResult::Queued
    );
    probe.complete_one(&fx.archive).await;
    assert_eq!(
        *probe.fetches.lock().unwrap(),
        vec!["https://fixture.example/parent-control.png"]
    );
    assert_eq!(*probe.commits.lock().unwrap(), vec![control_id]);
}
