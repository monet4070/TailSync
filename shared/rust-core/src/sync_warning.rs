use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SyncWarning {
    pub kind: &'static str,
    pub peer: String,
    pub occurred_at_ms: i64,
}

static LATEST_WARNING: OnceLock<Mutex<Option<SyncWarning>>> = OnceLock::new();

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
    fn warning_contains_no_clipboard_content_and_is_consumed_once() {
        let _ = take();
        record_expired_event("Laptop");
        let warning = take().unwrap();
        assert_eq!(warning.kind, "expired_event");
        assert_eq!(warning.peer, "Laptop");
        assert!(warning.occurred_at_ms > 0);
        assert_eq!(take(), None);
    }

    #[test]
    fn delivery_stalled_records_its_own_kind() {
        let _ = take();
        record_delivery_stalled("Desktop");
        let warning = take().unwrap();
        assert_eq!(warning.kind, "delivery_stalled");
        assert_eq!(warning.peer, "Desktop");
        assert!(warning.occurred_at_ms > 0);
        assert_eq!(take(), None);
    }
}
