use super::{
    files_to_broadcast, outgoing_batch_failure_disposition, peer_is_transfer_eligible,
    run_outgoing_recovery_loop, run_periodic_maintenance, summarize_file_batch_failures,
    validate_prepared_batch_sources_with, validate_prepared_file_source, ClipboardEventGate,
    FileBatchDeliveryError, OutgoingBatchFailureDisposition,
    IDENTICAL_CLIPBOARD_EVENT_DEBOUNCE_MS,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::time::{Duration, Instant};

#[tokio::test]
async fn outgoing_recovery_commits_history_before_removing_completed_journal() {
    use tokio::sync::Mutex;
    let root = std::env::temp_dir().join(format!("tailsync-recovery-history-{:016x}", rand::random::<u64>()));
    std::fs::create_dir_all(&root).unwrap();
    let source = root.join("payload.bin");
    std::fs::write(&source, b"history recovery payload").unwrap();
    let prepared = crate::sync::prepare_file_batch(vec![source], 1).unwrap();
    let batch_id = prepared.manifest.batch_id;
    crate::sync::persist_outgoing_batch_with_identities(&prepared, &[("peer".into(), "peer-key".into())]).unwrap();
    crate::sync::mark_outgoing_peer_completed_with_identity(batch_id, "peer", "peer-key").unwrap();
    let settings = Arc::new(Mutex::new(crate::crypto::Settings {
        notifications_enabled: false,
        ..crate::crypto::Settings::default()
    }));
    let pool = Arc::new(Mutex::new(crate::network::ConnectionPool::new(
        Arc::new(crate::identity::DeviceIdentity::generate_for_test()), settings.clone(),
    )));
    let database = Arc::new(Mutex::new(crate::db::HistoryDB::open_isolated_for_test(&root.join("history")).unwrap()));
    let journal = crate::sync::load_outgoing_batches().into_iter().find(|batch| batch.batch_id() == batch_id).unwrap();
    assert!(!journal.local_history_saved);
    super::resume_outgoing_batches(
        vec![journal], Vec::new(), super::ClipboardRuntime::Headless,
        database.clone(), pool, settings,
    ).await;
    assert!(database.lock().await.has_complete_file_batch(&batch_id.as_hex()).unwrap());
    assert!(!crate::sync::load_outgoing_batches().iter().any(|batch| batch.batch_id() == batch_id));
    drop(database);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn outgoing_recovery_keeps_journal_when_local_history_fails() {
    use tokio::sync::Mutex;
    let root = std::env::temp_dir().join(format!("tailsync-recovery-missing-source-{:016x}", rand::random::<u64>()));
    std::fs::create_dir_all(&root).unwrap();
    let source = root.join("payload.bin");
    std::fs::write(&source, b"history recovery payload").unwrap();
    let prepared = crate::sync::prepare_file_batch(vec![source.clone()], 1).unwrap();
    let batch_id = prepared.manifest.batch_id;
    crate::sync::persist_outgoing_batch_with_identities(&prepared, &[("peer".into(), "peer-key".into())]).unwrap();
    crate::sync::mark_outgoing_peer_completed_with_identity(batch_id, "peer", "peer-key").unwrap();
    std::fs::remove_file(source).unwrap();
    let settings = Arc::new(Mutex::new(crate::crypto::Settings {
        notifications_enabled: false,
        ..crate::crypto::Settings::default()
    }));
    let pool = Arc::new(Mutex::new(crate::network::ConnectionPool::new(
        Arc::new(crate::identity::DeviceIdentity::generate_for_test()), settings.clone(),
    )));
    let database = Arc::new(Mutex::new(crate::db::HistoryDB::open_isolated_for_test(&root.join("history")).unwrap()));
    let journal = crate::sync::load_outgoing_batches().into_iter().find(|batch| batch.batch_id() == batch_id).unwrap();
    super::resume_outgoing_batches(
        vec![journal], Vec::new(), super::ClipboardRuntime::Headless,
        database.clone(), pool, settings,
    ).await;
    let restored = crate::sync::load_outgoing_batches().into_iter().find(|batch| batch.batch_id() == batch_id).unwrap();
    assert!(!restored.local_history_saved);
    assert_eq!(restored.attempt_count, 1);
    assert!(!database.lock().await.has_complete_file_batch(&batch_id.as_hex()).unwrap());
    crate::sync::remove_outgoing_batch(batch_id).unwrap();
    drop(database);
    std::fs::remove_dir_all(root).unwrap();
}

fn transfer_peer(
    enabled: bool,
    trusted: bool,
    online: bool,
) -> crate::network::tailscale::PeerInfo {
    crate::network::tailscale::PeerInfo {
        hostname: "peer".to_string(),
        tailscale_ip: "100.64.0.2".to_string(),
        online,
        enabled,
        address: "100.64.0.2:53317".to_string(),
        connection_mode: "auto".to_string(),
        trusted,
        fingerprint: String::new(),
        candidates: Vec::new(),
        current_interface: None,
        current_address: None,
        status: Default::default(),
    }
}
#[test]
fn consecutive_native_events_for_identical_content_are_debounced() {
    let mut gate = ClipboardEventGate::default();
    let first = Instant::now();

    assert!(gate.should_process(true, true, first));
    assert!(!gate.should_process(false, true, first + Duration::from_millis(100)));
    assert!(gate.should_process(
        false,
        true,
        first + Duration::from_millis(IDENTICAL_CLIPBOARD_EVENT_DEBOUNCE_MS)
    ));
    assert!(!gate.should_process(false, false, first + Duration::from_secs(2)));
}

#[test]
fn changed_content_bypasses_the_native_event_debounce() {
    let mut gate = ClipboardEventGate::default();
    let first = Instant::now();

    assert!(gate.should_process(true, true, first));
    assert!(gate.should_process(true, true, first + Duration::from_millis(10)));
}

#[test]
fn immediate_transfers_require_enabled_trusted_peers_with_a_route() {
    assert!(peer_is_transfer_eligible(&transfer_peer(true, true, true)));
    assert!(!peer_is_transfer_eligible(&transfer_peer(
        false, true, true
    )));
    assert!(!peer_is_transfer_eligible(&transfer_peer(
        true, false, true
    )));
    assert!(peer_is_transfer_eligible(&transfer_peer(true, true, false)));
}

#[test]
fn iroh_node_ids_are_valid_broadcast_targets_without_ip_parsing() {
    let mut peer = transfer_peer(true, true, false);
    peer.address = "7f5a1b2c3d4e5f60718293a4b5c6d7e8".into();
    peer.candidates = vec![crate::network::PeerCandidate::new(
        crate::network::ConnectionInterface::Iroh,
        peer.address.clone(),
    )];
    assert!(peer_is_transfer_eligible(&peer));
}

#[test]
fn file_batch_failures_are_summarized_once() {
    assert_eq!(
        summarize_file_batch_failures(&[(
            "Mac".into(),
            FileBatchDeliveryError::Retryable("connection lost".into()),
        )]),
        "File transfer to Mac failed: connection lost"
    );
    assert_eq!(
        summarize_file_batch_failures(&[
            (
                "Mac".into(),
                FileBatchDeliveryError::Retryable("connection lost".into()),
            ),
            (
                "Laptop".into(),
                FileBatchDeliveryError::Retryable("timed out".into()),
            ),
        ]),
        "File transfer failed on 2 devices: Mac, Laptop"
    );
}

#[test]
fn permanent_source_failure_retires_the_outgoing_batch() {
    let missing_source = vec![(
        "Mac".into(),
        FileBatchDeliveryError::SourceUnavailable(
            "Cannot re-open /tmp/deleted.png: No such file or directory (os error 2)".into(),
        ),
    )];
    assert_eq!(
        outgoing_batch_failure_disposition(&missing_source),
        OutgoingBatchFailureDisposition::Retire
    );

    let transient_transport = vec![(
        "Mac".into(),
        FileBatchDeliveryError::Retryable("connection lost".into()),
    )];
    assert_eq!(
        outgoing_batch_failure_disposition(&transient_transport),
        OutgoingBatchFailureDisposition::Retry
    );
}

#[tokio::test]
async fn outgoing_recovery_retries_pending_work_after_the_peer_returns() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let recovery_attempts = attempts.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    let worker = tokio::spawn(run_outgoing_recovery_loop(
        shutdown_rx,
        Duration::from_millis(1),
        Duration::from_secs(60),
        move || {
            let attempts = recovery_attempts.clone();
            async move {
                // The first pass models the receiver being offline. Keeping
                // the journal pending must schedule a second pass, where the
                // peer has returned and the transfer succeeds.
                attempts.fetch_add(1, Ordering::SeqCst) == 0
            }
        },
    ));

    tokio::time::timeout(Duration::from_millis(250), async {
        while attempts.load(Ordering::SeqCst) < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("recovery worker did not retry pending work");
    shutdown_tx.send(true).unwrap();
    worker.await.unwrap();
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
}

#[test]
fn source_validation_is_shared_for_one_two_and_eight_peers() {
    let root = std::env::temp_dir().join(format!(
        "tailsync-shared-source-validation-{:016x}",
        rand::random::<u64>()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let first = root.join("first.bin");
    let second = root.join("second.bin");
    std::fs::write(&first, b"first").unwrap();
    std::fs::write(&second, b"second").unwrap();
    let prepared = crate::sync::prepare_file_batch(vec![first, second], 1).unwrap();
    for peer_count in [1, 2, 8] {
        let calls = AtomicUsize::new(0);
        let validated = validate_prepared_batch_sources_with(Arc::new(prepared.clone()), |_| {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .unwrap();
        let peer_views = (0..peer_count)
            .map(|_| validated.clone())
            .collect::<Vec<_>>();

        assert_eq!(peer_views.len(), peer_count);
        assert_eq!(peer_views[0].file_count(), prepared.files.len());
        assert_eq!(calls.load(Ordering::SeqCst), prepared.files.len());
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn shared_source_validation_rejects_changes_and_missing_files() {
    let root = std::env::temp_dir().join(format!(
        "tailsync-shared-source-mutation-{:016x}",
        rand::random::<u64>()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let changed = root.join("changed.bin");
    let missing = root.join("missing.bin");
    std::fs::write(&changed, b"first").unwrap();
    std::fs::write(&missing, b"second").unwrap();
    let prepared = crate::sync::prepare_file_batch(vec![changed.clone(), missing.clone()], 1).unwrap();

    std::fs::write(&changed, b"other").unwrap();
    let changed_error = validate_prepared_batch_sources_with(
        Arc::new(prepared.clone()),
        validate_prepared_file_source,
    )
    .unwrap_err();
    assert!(matches!(changed_error, FileBatchDeliveryError::SourceUnavailable(_)));

    std::fs::write(&changed, b"first").unwrap();
    let missing_prepared =
        crate::sync::prepare_file_batch(vec![missing.clone()], 2).unwrap();
    std::fs::remove_file(&missing).unwrap();
    let missing_error = validate_prepared_batch_sources_with(
        Arc::new(missing_prepared),
        validate_prepared_file_source,
    )
    .unwrap_err();
    assert!(matches!(missing_error, FileBatchDeliveryError::SourceUnavailable(_)));

    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn periodic_maintenance_runs_and_stops_on_shutdown() {
    let runs = Arc::new(AtomicUsize::new(0));
    let runs_for_worker = runs.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let worker = tokio::spawn(run_periodic_maintenance(
        shutdown_rx,
        Duration::from_millis(1),
        move || {
            let runs = runs_for_worker.clone();
            async move {
                if runs.fetch_add(1, Ordering::SeqCst) >= 1 {
                    // The test controls shutdown below; this branch simply
                    // proves one slow callback does not create overlap.
                    tokio::time::sleep(Duration::from_millis(2)).await;
                }
            }
        },
    ));

    tokio::time::timeout(Duration::from_millis(250), async {
        while runs.load(Ordering::SeqCst) < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("periodic maintenance did not run twice");
    shutdown_tx.send(true).unwrap();
    worker.await.unwrap();
    let completed = runs.load(Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert_eq!(runs.load(Ordering::SeqCst), completed);
}

#[test]
fn repeated_native_events_for_a_managed_file_never_broadcast() {
    let managed_directory = std::path::PathBuf::from("tailsync-data/clipboard-files");
    let path = managed_directory.join("transfer/report.pdf");
    let paths = vec![path.clone()];

    assert!(files_to_broadcast(&paths, &managed_directory).is_empty());
    assert!(files_to_broadcast(&paths, &managed_directory).is_empty());
}

#[test]
fn user_owned_file_is_still_broadcast() {
    let managed_directory = std::path::PathBuf::from("tailsync-data/clipboard-files");
    let path = std::path::PathBuf::from("documents/report.pdf");

    assert_eq!(
        files_to_broadcast(std::slice::from_ref(&path), &managed_directory),
        vec![path]
    );
}

#[test]
fn managed_directory_name_prefix_is_not_treated_as_managed() {
    let managed_directory = std::path::PathBuf::from("tailsync-data/clipboard-files");
    let path = std::path::PathBuf::from("tailsync-data/clipboard-files-export/report.pdf");

    assert_eq!(
        files_to_broadcast(std::slice::from_ref(&path), &managed_directory),
        vec![path]
    );
}

#[test]
fn canonical_alias_of_a_managed_file_is_not_broadcast() {
    let root = std::env::temp_dir().join(format!(
        "tailsync-managed-path-test-{:016x}",
        rand::random::<u64>()
    ));
    let actual_managed_directory = root.join("clipboard-files");
    let managed_directory = root.join("alias/../clipboard-files");
    std::fs::create_dir_all(root.join("alias")).unwrap();
    let transfer_directory = actual_managed_directory.join("transfer");
    std::fs::create_dir_all(&transfer_directory).unwrap();
    let file = transfer_directory.join("report.pdf");
    std::fs::write(&file, b"report").unwrap();

    assert!(files_to_broadcast(&[file], &managed_directory).is_empty());

    std::fs::remove_dir_all(root).unwrap();
}
