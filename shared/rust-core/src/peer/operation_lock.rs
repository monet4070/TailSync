//! Bounded per-event operation locks (repair plan §4 R4-02).
//!
//! The receiver used a single process-wide `FINISH_LOCK` around the whole native
//! publish. That serialised *every* composite event from *every* peer, so one slow
//! publish blocked unrelated ones — a head-of-line block with no purpose, because
//! two different events have nothing to do with each other.
//!
//! What actually needs serialising is a single event's own processing: the same
//! `(authenticated source, event id)` arriving twice — over two sessions, or twice
//! on one — must not run two native attempts. Different events must proceed in
//! parallel.
//!
//! Two properties this type is careful about:
//!
//! - **A terminal result short-circuits before the lock is taken.** A duplicate of an
//!   already-finished event is answered from the journal without queueing behind
//!   anything.
//! - **Reclamation never removes an entry that is held or awaited.** An entry is
//!   dropped only while nobody else can hold a reference to it, so a waiter can never
//!   be left holding a lock that has been evicted from the table.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

/// Identifies one event processing operation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct OperationKey {
    /// Authenticated peer fingerprint, hex encoded.
    pub source: String,
    /// Event message id, hex encoded.
    pub event_id: String,
}

impl OperationKey {
    pub fn new(source: impl Into<String>, event_id: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            event_id: event_id.into(),
        }
    }
}

/// Upper bound on the lock table. Entries are reclaimed on release and pruned when
/// the table is full, so the bound is on live operations rather than on history.
const MAX_ENTRIES: usize = 1024;

#[derive(Default)]
pub struct OperationLocks {
    entries: Mutex<BTreeMap<OperationKey, Arc<tokio::sync::Mutex<()>>>>,
}

impl OperationLocks {
    /// Acquire the lock for one event.
    ///
    /// Callers should check for a terminal result *first*: a duplicate of a finished
    /// event never needs this lock at all.
    pub async fn acquire(self: &Arc<Self>, key: OperationKey) -> Result<OperationGuard, String> {
        let entry = {
            let mut table = self
                .entries
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if table.len() >= MAX_ENTRIES && !table.contains_key(&key) {
                // Prune only entries nobody holds or awaits: `strong_count == 1` means
                // the table itself is the only owner.
                table.retain(|_, value| Arc::strong_count(value) > 1);
            }
            if table.len() >= MAX_ENTRIES && !table.contains_key(&key) {
                return Err(format!(
                    "{}event operation capacity reached",
                    crate::protocol::TEMPORARY_PRESSURE_MARKER
                ));
            }
            table.entry(key.clone()).or_default().clone()
        };
        let guard = entry.lock_owned().await;
        Ok(OperationGuard {
            owner: self.clone(),
            key,
            guard: Some(guard),
        })
    }

    /// Number of live entries. Test/diagnostic surface.
    pub fn len(&self) -> usize {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Holds one event's operation lock and reclaims the table entry on release.
pub struct OperationGuard {
    owner: Arc<OperationLocks>,
    key: OperationKey,
    /// The only owned reference to the entry while the guard is alive, so dropping it
    /// leaves the table as the sole owner unless a waiter cloned it first.
    guard: Option<tokio::sync::OwnedMutexGuard<()>>,
}

impl Drop for OperationGuard {
    fn drop(&mut self) {
        // Release the mutex first, so the table becomes the sole owner when nobody
        // else holds or awaits this entry.
        self.guard = None;
        let mut table = self
            .owner
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let removable = table
            .get(&self.key)
            .is_some_and(|current| Arc::strong_count(current) == 1);
        if removable {
            table.remove(&self.key);
        }
    }
}

/// The process-wide table, so two sessions for the same event serialise.
pub fn global() -> Arc<OperationLocks> {
    static GLOBAL: std::sync::LazyLock<Arc<OperationLocks>> =
        std::sync::LazyLock::new(|| Arc::new(OperationLocks::default()));
    GLOBAL.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Different events must not block each other — that was the whole point of
    /// replacing the global lock.
    #[tokio::test(flavor = "current_thread")]
    async fn different_events_proceed_in_parallel() {
        let locks = Arc::new(OperationLocks::default());
        let first = locks.acquire(OperationKey::new("peer", "a")).await.unwrap();

        let started = std::time::Instant::now();
        let second = locks.acquire(OperationKey::new("peer", "b")).await.unwrap();
        assert!(
            started.elapsed() < Duration::from_millis(100),
            "a different event must not wait for the first"
        );

        // The same event does serialise.
        let third = tokio::spawn({
            let locks = locks.clone();
            async move {
                let _guard = locks.acquire(OperationKey::new("peer", "a")).await.unwrap();
                std::time::Instant::now()
            }
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        drop(first);
        let acquired_at = third.await.unwrap();
        assert!(
            acquired_at.elapsed() < Duration::from_millis(100),
            "the same event proceeds once the first holder releases"
        );
        drop(second);
    }

    /// A released entry is reclaimed, so the table does not grow with history.
    #[tokio::test]
    async fn released_entries_are_reclaimed() {
        let locks = Arc::new(OperationLocks::default());
        for index in 0..64 {
            let guard = locks
                .acquire(OperationKey::new("peer", format!("evt-{index}")))
                .await
                .unwrap();
            drop(guard);
        }
        assert!(
            locks.is_empty(),
            "every released entry must be reclaimed, table holds {}",
            locks.len()
        );
    }

    /// Reclamation must never evict an entry that is held or awaited.
    #[tokio::test(flavor = "current_thread")]
    async fn held_and_awaited_entries_are_never_evicted() {
        let locks = Arc::new(OperationLocks::default());
        let held = locks
            .acquire(OperationKey::new("peer", "held"))
            .await
            .unwrap();

        // A waiter clones the entry before awaiting, so the table must keep it.
        let waiter = tokio::spawn({
            let locks = locks.clone();
            async move {
                let _guard = locks
                    .acquire(OperationKey::new("peer", "held"))
                    .await
                    .unwrap();
            }
        });
        tokio::time::sleep(Duration::from_millis(20)).await;

        // Churn enough other keys to force a prune pass.
        for index in 0..(MAX_ENTRIES + 32) {
            let guard = locks
                .acquire(OperationKey::new("other", format!("evt-{index}")))
                .await
                .unwrap();
            drop(guard);
        }
        assert!(
            locks.len() <= MAX_ENTRIES,
            "the table must stay bounded, holds {}",
            locks.len()
        );

        drop(held);
        waiter.await.unwrap();
        assert!(locks.is_empty() || locks.len() <= MAX_ENTRIES);
    }
    #[tokio::test]
    async fn full_active_table_refuses_new_keys_without_evicting_waiters() {
        let locks = Arc::new(OperationLocks::default());
        let mut held = Vec::new();
        for index in 0..MAX_ENTRIES {
            held.push(
                locks
                    .acquire(OperationKey::new("peer", index.to_string()))
                    .await
                    .unwrap(),
            );
        }
        assert!(locks
            .acquire(OperationKey::new("extra", "new"))
            .await
            .is_err());
        assert_eq!(locks.len(), MAX_ENTRIES);
        let waiting = tokio::spawn({
            let locks = locks.clone();
            async move { locks.acquire(OperationKey::new("peer", "0")).await.unwrap() }
        });
        tokio::task::yield_now().await;
        assert!(!waiting.is_finished());
        drop(held.remove(0));
        let same = waiting.await.unwrap();
        assert_eq!(locks.len(), MAX_ENTRIES);
        drop(same);
        held.clear();
        assert!(locks.is_empty());
    }
}
