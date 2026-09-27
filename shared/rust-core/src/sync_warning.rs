use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
pub struct SyncWarning {
    pub kind: &'static str,
    pub peer: String,
    pub occurred_at_ms: i64,
}

static LATEST_WARNING: OnceLock<Mutex<Option<SyncWarning>>> = OnceLock::new();

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
static TEST_WARNING_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

/// Serialize tests that observe the process-global warning slot. Production
/// intentionally exposes only one latest warning, so tests in other Modules
/// must not consume each other's value while the Rust harness runs in parallel.
#[cfg(test)]
pub(crate) fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    TEST_WARNING_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn latest_warning() -> &'static Mutex<Option<SyncWarning>> {
    LATEST_WARNING.get_or_init(|| Mutex::new(None))
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
    *latest_warning()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(SyncWarning {
        kind,
        peer: peer.chars().take(255).collect(),
        occurred_at_ms: crate::protocol::unix_timestamp_ms(),
    });
}

pub fn take() -> Option<SyncWarning> {
    latest_warning()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .take()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warning_variants_are_bounded_and_consumed_once() {
        type WarningRecorder = fn(&str);

        let _guard = test_lock();
        let _ = take();
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
        let _ = take();
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
