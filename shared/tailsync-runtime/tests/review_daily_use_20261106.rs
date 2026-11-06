use std::sync::Arc;
use tailsync_core::{
    composite::{origin::{Origin, OriginTracker}, Completeness, Mime, PortableBundle, Representation},
    db::HistoryDB,
    peer::delivery::{deliver_pending_frame, DeliveryConfig, DeliveryConnection, PendingFrame, QueuedFrame},
    protocol::{Command, Frame},
    secure::CapabilitySet,
};
use tailsync_runtime::history::HistoryOperations;
use tokio::sync::Mutex;

#[tokio::test]
async fn review_wpf_noncomposite_restore_should_accept_existing_record() {
    let root = std::env::temp_dir().join(format!("tailsync-review-legacy-{}", std::process::id()));
    let mut database = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
    database.add_text("review synthetic legacy text", "review").unwrap();
    database.add_file("review.txt", b"review synthetic file", "review").unwrap();
    let entries = database.get_all(None, None, 10, 0).unwrap();
    let database = Arc::new(Mutex::new(database));
    let mut rejected = Vec::new();
    for entry in entries {
        let ordinary = HistoryOperations::restore_payload_checked_async(
            database.clone(), entry.id, None, None).await;
        assert!(ordinary.is_ok(), "control request should work: {ordinary:?}");
        // NativeHistoryClient.RestoreEntryAsync sends originalId for EVERY row.
        let wpf = HistoryOperations::restore_payload_checked_async(
            database.clone(), entry.id, Some(entry.id), None).await;
        if let Err(error) = wpf {
            rejected.push((entry.id, error));
        }
    }
    drop(database);
    std::fs::remove_dir_all(&root).unwrap();
    assert!(rejected.is_empty(), "valid legacy/file records rejected by WPF arguments: {rejected:?}");
}

#[test]
fn review_superseded_receipts_should_not_permanently_block_publication() {
    let mut tracker = OriginTracker::default();
    for revision in 1..=128_i64 {
        let mut token = [0; 16];
        token[..8].copy_from_slice(&revision.to_le_bytes());
        assert!(tracker.record_intent(token, [1; 32]));
        tracker.record(revision, [1; 32], token);
        // User changes the clipboard before the monitor sees our write.
        assert_eq!(tracker.classify(Some(revision + 1000), [9; 32], None), Origin::External);
    }
    assert!(!tracker.has_unresolved_intent(), "all operations have valid receipts");
    assert!(tracker.record_intent([255; 16], [1; 32]),
        "129th publish is permanently refused although every write completed and a newer copy was observed");
}

struct PressurePeer;
impl DeliveryConnection for PressurePeer {
    async fn write_frame(&mut self, frame: &Frame) -> Result<(), String> {
        assert_eq!(frame.command, Command::CompositeChunk);
        Ok(())
    }
    async fn read_frame(&mut self) -> Result<Frame, String> {
        let ledger = Arc::new(tailsync_core::composite::budgets::BudgetLedger::default());
        let held = ledger.admit_assembly(&[7; 32], 1024, std::time::Instant::now()).unwrap();
        let pressure = ledger.admit_assembly(&[7; 32], 1024, std::time::Instant::now()).unwrap_err();
        drop(held);
        assert!(tailsync_core::peer::delivery::peer_error_is_temporary(&pressure.to_string()));
        Ok(Frame::try_new(Command::PeerError, 0, 7, pressure.to_string().into_bytes()).unwrap())
    }
    fn negotiated_capabilities(&self) -> CapabilitySet {
        CapabilitySet { composite_clipboard: true, ..CapabilitySet::disabled() }
    }
}
#[tokio::test]
async fn review_real_composite_delivery_should_retry_receiver_pressure() {
    let bundle = PortableBundle {
        representations: vec![Representation { mime: Mime::Text, bytes: b"review synthetic".to_vec() }],
        resources: vec![], bindings: vec![], completeness: Completeness::Complete,
    };
    let frame = QueuedFrame::new(Command::CompositePayload, bundle.encode().unwrap()).unwrap();
    let pending = PendingFrame::new(frame, 7);
    let error = deliver_pending_frame(&mut PressurePeer, &pending, &DeliveryConfig::default())
        .await.unwrap_err();
    assert!(error.is_retryable(), "production sender drops temporary pressure as permanent: {error:?}");
}

#[test]
fn review_failed_resolution_should_be_recoverable_without_restart() {
    let mut tracker = OriginTracker::default();
    tracker.record_intent([1; 16], [2; 32]);
    tracker.mark_spawned([1; 16], 123456);
    // The helper has exited but its one-shot readback failed. The adapter
    // currently leaves this intent untouched; later classification only
    // reconciles recovery_processes(), which excludes local-session intents.
    let candidates = tracker.recovery_processes();
    let origin = tracker.classify(Some(100), [3; 32], None);
    assert!(!candidates.is_empty() || origin == Origin::External,
        "a terminated local helper cannot be reconciled: candidates={candidates:?}, origin={origin:?}");
}
