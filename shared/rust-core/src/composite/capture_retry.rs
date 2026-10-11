//! One bounded retry schedule per native revision. A new copy starts immediately.
use std::time::{Duration, Instant};
#[derive(Default)]
pub struct CaptureRetry {
    revision: Option<i64>,
    attempts: usize,
    ready_at: Option<Instant>,
}
impl CaptureRetry {
    pub fn ready(&mut self, revision: i64, now: Instant) -> bool {
        if self.revision != Some(revision) {
            *self = Self {
                revision: Some(revision),
                ..Self::default()
            };
        }
        self.ready_at.is_none_or(|at| now >= at)
    }
    /// A completed read belongs to the revision that scheduled it, never to the
    /// clipboard that happened to replace it while its provider was running.
    pub fn matches_snapshot(
        revision: i64,
        snapshot: &crate::protocol::native_clipboard::NativeSnapshot,
    ) -> bool {
        snapshot.before == revision && snapshot.after == revision
    }
    pub fn is_current(&self, revision: i64, current: Option<i64>) -> bool {
        self.revision == Some(revision) && current == Some(revision)
    }
    /// True schedules another attempt; false requires a durable terminal fault.
    pub fn retry(&mut self, revision: i64, now: Instant) -> bool {
        if self.revision != Some(revision) {
            return false;
        }
        const DELAYS: [u64; 3] = [200, 800, 2000];
        let Some(delay) = DELAYS.get(self.attempts) else {
            return false;
        };
        self.attempts += 1;
        self.ready_at = Some(now + Duration::from_millis(*delay));
        true
    }
    pub fn exhausted(&self) -> bool {
        self.attempts >= 3
    }
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn schedule_is_bounded_and_new_revision_cancels_old_retry() {
        let mut retry = CaptureRetry::default();
        let now = Instant::now();
        assert!(retry.ready(1, now));
        assert!(retry.retry(1, now));
        assert!(!retry.ready(1, now + Duration::from_millis(199)));
        assert!(retry.ready(1, now + Duration::from_millis(200)));
        assert!(retry.retry(1, now));
        assert!(!retry.ready(1, now + Duration::from_millis(799)));
        assert!(retry.retry(1, now));
        assert!(!retry.ready(1, now + Duration::from_millis(1999)));
        assert!(!retry.retry(1, now));
        assert!(retry.ready(2, now));
        assert!(!retry.exhausted());
    }
    #[test]
    fn o10_late_capture_cannot_rebind_or_spend_a_new_copy_retry() {
        use crate::protocol::native_clipboard::{NativeSnapshot, SnapshotState};
        let mut retry = CaptureRetry::default();
        let now = Instant::now();
        assert!(retry.ready(9, now));
        assert!(retry.retry(9, now));
        // Revisions are opaque: even a smaller new OS sequence supersedes 9.
        assert!(retry.ready(1, now));
        assert!(retry.retry(1, now));
        assert!(!retry.retry(9, now));
        assert!(!retry.is_current(9, Some(1)));
        assert!(!retry.is_current(1, None));
        assert!(!retry.ready(1, now + Duration::from_millis(199)));
        assert!(retry.ready(1, now + Duration::from_millis(200)));
        assert!(retry.is_current(1, Some(1)));
        let mut snapshot = NativeSnapshot {
            before: 9,
            after: 9,
            items: 0,
            state: SnapshotState::Empty,
            formats: vec![],
        };
        assert!(!CaptureRetry::matches_snapshot(1, &snapshot));
        snapshot.before = 1;
        assert!(!CaptureRetry::matches_snapshot(1, &snapshot));
        snapshot.after = 1;
        assert!(CaptureRetry::matches_snapshot(1, &snapshot));
    }
}
