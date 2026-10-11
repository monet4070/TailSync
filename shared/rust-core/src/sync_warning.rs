use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
pub struct SyncWarning {
    /// Stable identity of this warning, so a consumer can read it without consuming it
    /// and then acknowledge exactly the one it showed.
    pub id: u64,
    pub kind: &'static str,
    pub peer: String,
    pub occurred_at_ms: i64,
}

/// Monotonic source of `SyncWarning::id`.
static NEXT_WARNING_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[derive(Default)]
struct WarningQueue {
    pending: std::collections::VecDeque<SyncWarning>,
    overflow: u64,
}
static LATEST_WARNING: OnceLock<Mutex<WarningQueue>> = OnceLock::new();
const WARNING_LIMIT: usize = 64;

/// Set once the process begins shutting down. Delivery workers tear their
/// connection down both on shutdown and on an explicit reconnect (a route
/// change or a retried transfer to an offline peer), so only a real shutdown
/// may surface the user-facing "delivery stopped" warning.
static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

/// Mark the process as shutting down. Called once by the daemon/application
/// shutdown coordinator before connections are closed.
pub fn begin_shutdown() {
    SHUTTING_DOWN.store(true, Ordering::SeqCst);
}

pub fn is_shutting_down() -> bool {
    SHUTTING_DOWN.load(Ordering::Acquire)
}

#[cfg(test)]
pub(crate) fn reset_shutdown_for_test() {
    SHUTTING_DOWN.store(false, Ordering::SeqCst);
}

#[cfg(test)]
static TEST_WARNING_PERMIT: OnceLock<std::sync::Arc<tokio::sync::Semaphore>> = OnceLock::new();
#[cfg(test)]
fn test_permit() -> std::sync::Arc<tokio::sync::Semaphore> {
    TEST_WARNING_PERMIT
        .get_or_init(|| std::sync::Arc::new(tokio::sync::Semaphore::new(1)))
        .clone()
}
/// Sync harness cases use the same permit as async cases. No production lock
/// crosses an await; async cases wait cooperatively on their runtime.
#[cfg(test)]
pub(crate) fn test_lock() -> tokio::sync::OwnedSemaphorePermit {
    loop {
        if let Ok(permit) = test_permit().try_acquire_owned() {
            return permit;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}
#[cfg(test)]
pub(crate) async fn test_lock_async() -> tokio::sync::OwnedSemaphorePermit {
    test_permit().acquire_owned().await.unwrap()
}

fn latest_warning() -> &'static Mutex<WarningQueue> {
    LATEST_WARNING.get_or_init(|| Mutex::new(WarningQueue::default()))
}

pub fn record_composite_result(peer: &str, result: crate::protocol::composite::AckResult) {
    use crate::protocol::composite::AckResult;
    match result {
        AckResult::StoredIncomplete => record(peer, "composite_incomplete"),
        AckResult::ApplyUnknown => record(peer, "composite_apply_unknown"),
        AckResult::FailedPartial => record(peer, "composite_partial"),
        AckResult::Superseded => record(peer, "composite_superseded"),
        _ => {}
    }
}
/// A permanent rejection dropped clipboard content.
///
/// R4-04 / D-5: `CompositeResult` and `Expired` already raised warnings, but a
/// `Rejected` — an assembly-budget rejection, a capability downgrade, an
/// un-degradable RTF-only fallback — was log-only while the content was lost for
/// good. The user has to be able to see that.
/// A captured clipboard item could not be written to history.
///
/// R5-01: this used to be a `warn!` and nothing else. The clipboard had already been
/// read and (for a composite item) already broadcast, so the content was gone with no
/// trace the user could see. A storage failure has to be a visible outcome, not a log
/// line — the plan is explicit that a body which could not be stored must never be
/// reported as saved.
pub fn record_capture_not_saved() {
    record("", "capture_not_saved");
}
/// A received event failed history admission before any native effect.
pub fn record_receive_not_saved(peer: &str) {
    record(peer, "receive_not_saved");
}
pub fn record_capture_failed() {
    record("", "capture_failed");
}
/// Called only after a successful history commit. Conversion support is a
/// separate fact from reading and saving the source bytes.
pub fn record_saved_capture(
    snapshot: &crate::protocol::native_clipboard::NativeSnapshot,
    portable: Option<&crate::composite::PortableBundle>,
    origin: crate::composite::origin::Origin,
    native_formats_admitted: bool,
) {
    let read_incomplete = snapshot.formats.iter().any(|f| {
        matches!(
            f.state,
            crate::protocol::native_clipboard::FormatState::Failed
                | crate::protocol::native_clipboard::FormatState::TooLarge
        )
    }) || portable
        .is_some_and(|b| b.completeness == crate::composite::Completeness::MissingFormats);
    tracing::debug!(
        history_saved = true,
        native_formats_admitted,
        portable_convertible = portable.is_some(),
        archived_resource_count = portable.map_or(0, |b| b.resources.len()),
        read_incomplete,
        ?origin,
        "capture outcome"
    );
    if read_incomplete {
        record_capture_failed();
    }
    if matches!(
        origin,
        crate::composite::origin::Origin::Unknown
            | crate::composite::origin::Origin::UnknownMismatch
    ) {
        record("", "capture_origin_unknown");
    }
}
pub fn record_capture_not_shared() {
    record("", "capture_not_shared");
}
pub fn record_clipboard_unavailable() {
    record("", "clipboard_unavailable");
}

pub fn record_delivery_rejected(peer: &str) {
    record(peer, "delivery_rejected");
}

pub fn record_expired_event(peer: &str) {
    record(peer, "expired_event");
}

/// A clipboard frame could not even be handed to the connection worker for
/// delivery — the send channel was full past the pool timeout or its worker
/// had exited. Surfaced so a wedged link is visible instead of silent.
pub fn record_delivery_stalled(peer: &str) {
    record(peer, "delivery_stalled");
}

pub fn record_delivery_expired(peer: &str) {
    record(peer, "delivery_expired");
}

/// A clipboard file selection was rejected before any transfer started because
/// it contained a symbolic link. Recorded so the History/Favorites window can
/// explain why nothing was added, even when system notifications are disabled;
/// the detail (the offending path) stays in the log.
pub fn record_file_selection_symlink() {
    record("", "file_selection_symlink");
}

/// A clipboard file selection was rejected before any transfer started because
/// it contained a folder or other non-file item. See
/// [`record_file_selection_symlink`] for why this is surfaced in-window.
pub fn record_file_selection_folder() {
    record("", "file_selection_folder");
}

/// Record that an in-flight delivery was abandoned because the process is
/// shutting down. Returns whether a warning was recorded.
///
/// A forced reconnect (route reselection, or a retried transfer to a peer that
/// dropped off the network) also abandons the in-flight frame, but the durable
/// retry resends it, so that path must not raise a shutdown warning.
pub fn record_delivery_shutdown(peer: &str) -> bool {
    if !is_shutting_down() {
        return false;
    }
    record(peer, "delivery_shutdown");
    true
}

fn record(peer: &str, kind: &'static str) {
    let id = NEXT_WARNING_ID
        .fetch_add(1, Ordering::SeqCst)
        .wrapping_add(1)
        .max(1);
    let mut queue = latest_warning().lock().unwrap_or_else(|p| p.into_inner());
    let peer: String = peer.chars().take(255).collect();
    if queue
        .pending
        .iter()
        .any(|w| w.kind == kind && w.peer == peer)
    {
        return;
    }
    if queue.pending.len() >= WARNING_LIMIT - 1 {
        queue.overflow = queue.overflow.saturating_add(1);
        if !queue.pending.iter().any(|w| w.kind == "warning_overflow") {
            queue.pending.push_back(SyncWarning {
                id,
                kind: "warning_overflow",
                peer: String::new(),
                occurred_at_ms: crate::protocol::unix_timestamp_ms(),
            });
        }
        return;
    }
    queue.pending.push_back(SyncWarning {
        id,
        kind,
        peer,
        occurred_at_ms: crate::protocol::unix_timestamp_ms(),
    });
}

/// Read the oldest undisplayed warning; the existing optional DTO stays compatible.
pub fn peek() -> Option<SyncWarning> {
    latest_warning()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .pending
        .front()
        .cloned()
}
/// Acknowledging one warning never clears a different pending warning.
pub fn ack(id: u64) -> bool {
    let mut queue = latest_warning().lock().unwrap_or_else(|p| p.into_inner());
    if let Some(index) = queue.pending.iter().position(|w| w.id == id) {
        queue.pending.remove(index);
        return true;
    }
    false
}
pub fn overflow_count() -> u64 {
    latest_warning()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .overflow
}
/// Compatibility consuming read. New UIs use peek + ack after presentation.
pub fn take() -> Option<SyncWarning> {
    latest_warning()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .pending
        .pop_front()
}
#[cfg(test)]
fn clear_for_test() {
    *latest_warning().lock().unwrap_or_else(|p| p.into_inner()) = WarningQueue::default();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warning_variants_are_bounded_and_consumed_once() {
        type WarningRecorder = fn(&str);

        let _guard = test_lock();
        clear_for_test();
        let peer = "x".repeat(300);
        let variants: [(&str, WarningRecorder); 3] = [
            ("expired_event", record_expired_event),
            ("delivery_stalled", record_delivery_stalled),
            ("delivery_expired", record_delivery_expired),
        ];

        for (kind, record_variant) in variants {
            record_variant(&peer);
            let warning = take().expect("recorded warning");
            assert_eq!(warning.kind, kind);
            assert_eq!(warning.peer.len(), 255);
            assert!(warning.occurred_at_ms > 0);
            assert_eq!(take(), None);
        }

        // The shutdown variant is gated on the process actually shutting down.
        begin_shutdown();
        assert!(record_delivery_shutdown(&peer));
        let warning = take().expect("recorded shutdown warning");
        assert_eq!(warning.kind, "delivery_shutdown");
        assert_eq!(warning.peer.len(), 255);
        assert_eq!(take(), None);
        reset_shutdown_for_test();
    }

    #[test]
    fn delivery_shutdown_warning_requires_a_real_shutdown() {
        let _guard = test_lock();
        clear_for_test();
        reset_shutdown_for_test();

        // A forced reconnect abandons the in-flight frame but the durable
        // retry resends it, so it must not raise a shutdown warning.
        assert!(!record_delivery_shutdown("peer"));
        assert_eq!(take(), None, "a reconnect must not look like a shutdown");

        begin_shutdown();
        assert!(record_delivery_shutdown("peer"));
        let warning = take().expect("a real shutdown records the warning");
        assert_eq!(warning.kind, "delivery_shutdown");
        assert_eq!(warning.peer, "peer");
        assert_eq!(take(), None);
        reset_shutdown_for_test();
    }
}

#[cfg(test)]
mod p2_4_tests {
    use super::*;

    /// S6-P2-4: reading must not consume, and acknowledging must consume exactly the
    /// warning that was displayed. This is what lets a window that cannot show the
    /// warning (hidden in the tray) observe it without taking it from the one that can.
    #[test]
    fn peek_does_not_consume_and_ack_consumes_exactly_that_warning() {
        let _guard = test_lock();
        clear_for_test(); // clear anything another test left behind

        record_expired_event("peer-a");
        let shown = peek().expect("a warning is readable");
        assert_eq!(
            peek().map(|warning| warning.id),
            Some(shown.id),
            "peek must not consume the warning"
        );

        record_delivery_stalled("peer-b");
        assert_eq!(
            peek().unwrap().id,
            shown.id,
            "a second warning must not overwrite the first"
        );
        assert!(ack(shown.id));
        let newer = peek().expect("the next warning remains pending");
        assert_ne!(newer.id, shown.id);
        assert!(!ack(shown.id));
        assert!(ack(newer.id));
        assert!(peek().is_none());
        assert!(!ack(newer.id), "acking twice must not succeed");
    }

    #[test]
    fn warning_ids_are_monotonic_and_nonzero() {
        let _guard = test_lock();
        clear_for_test();
        record_expired_event("peer-a");
        let first = peek().expect("warning").id;
        ack(first);
        record_expired_event("peer-a");
        let second = peek().expect("warning").id;
        assert!(first > 0 && second > first, "{first} then {second}");
        clear_for_test();
    }
    #[test]
    fn queue_preserves_undisplayed_warnings_and_reports_overflow() {
        let _guard = test_lock();
        clear_for_test();
        record_expired_event("first");
        let first = peek().unwrap();
        for index in 0..100 {
            record_delivery_rejected(&index.to_string());
        }
        assert_eq!(peek(), Some(first.clone()));
        assert_eq!(
            latest_warning().lock().unwrap().pending.len(),
            WARNING_LIMIT
        );
        assert!(overflow_count() > 0);
        assert!(ack(first.id));
        let mut overflow = 0;
        while let Some(warning) = take() {
            if warning.kind == "warning_overflow" {
                overflow += 1;
            }
        }
        assert_eq!(overflow, 1);
        clear_for_test();
    }
}
