use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SyncWarning {
    pub kind: &'static str,
    pub peer: String,
    pub occurred_at_ms: i64,
}

static LATEST_WARNING: OnceLock<Mutex<Option<SyncWarning>>> = OnceLock::new();

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

pub fn record_delivery_shutdown(peer: &str) {
    record(peer, "delivery_shutdown");
}

pub fn record_delivery_expired(peer: &str) {
    record(peer, "delivery_expired");
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
        let variants: [(&str, WarningRecorder); 4] = [
            ("expired_event", record_expired_event),
            ("delivery_stalled", record_delivery_stalled),
            ("delivery_shutdown", record_delivery_shutdown),
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
    }
}
