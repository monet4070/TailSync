// Structured diagnostics seam (T402 pilot, design per DIAGNOSTICS_SCHEMA).
//
// Mount points call [`record`] at key state transitions; without a
// registered collector this is a pure no-op, so existing log output stays
// byte-for-byte unchanged. The v1 schema carries the event name plus
// optional peer/session/error context — error kinds map 1:1 onto the R012
// typed enums. Prohibited content (clipboard payloads, keys, verification
// codes, §18) must never be placed in a [`Record`].

use serde::Serialize;
use std::sync::{Arc, LazyLock, Mutex};

/// Diagnostic event names (v1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    PairingWindowOpened,
    PairingWindowClosed,
    PairingHandshakeStarted,
    PairingConfirmed,
    PairingFailed,
}

/// Session context attached to transfer/pairing/import records.
#[derive(Debug, Clone, Serialize)]
pub struct SessionRef {
    pub kind: &'static str,
    pub id: String,
}

/// Structured view of a R012 typed error; `kind` is the enum variant path
/// and `message` is its Display output (wire contract, unchanged).
#[derive(Debug, Clone, Serialize)]
pub struct ErrorRef {
    pub kind: &'static str,
    pub message: String,
}

/// One diagnostics record (v1 schema).
#[derive(Debug, Clone, Serialize)]
pub struct Record {
    pub event: Event,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<SessionRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorRef>,
}

type Collector = dyn Fn(&Record) + Send + Sync;

static COLLECTOR: LazyLock<Mutex<Option<Arc<Collector>>>> = LazyLock::new(|| Mutex::new(None));

/// Register the diagnostics consumer (test/telemetry hook). Replaces any
/// previous collector.
pub fn set_collector(collector: Option<Box<Collector>>) {
    *COLLECTOR
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = collector.map(Arc::from);
}

/// Whether a collector is registered. Mount points guard on this so the
/// no-op path stays allocation-free.
pub fn is_collected() -> bool {
    COLLECTOR
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .is_some()
}

/// Emit a diagnostics record. No-op until a collector is registered, so
/// production log behavior is unchanged.
///
/// The collector is invoked **outside** the registry lock: the lock is only
/// held to clone the registered `Arc`, so a collector may safely reenter
/// [`record`], [`is_collected`], or [`set_collector`] without deadlocking,
/// and a slow collector never blocks other threads from registering or
/// inspecting the collector. The collector still runs synchronously on the
/// caller's thread — keep it lightweight; async export stays a future
/// (T402+) concern.
pub fn record(record: Record) {
    let collector = COLLECTOR
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    if let Some(collector) = collector {
        collector(&record);
    }
}

/// Convenience: build an [`ErrorRef`] from a Display error without
/// allocating when no collector is registered.
pub fn error_ref(kind: &'static str, message: impl std::fmt::Display) -> Option<ErrorRef> {
    if COLLECTOR
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .is_none()
    {
        return None;
    }
    Some(ErrorRef {
        kind,
        message: message.to_string(),
    })
}

/// Serializes tests that register the process-global collector.
#[cfg(test)]
pub(crate) fn diagnostics_test_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_is_a_noop_without_a_collector() {
        let _guard = diagnostics_test_lock().blocking_lock();
        use std::sync::atomic::{AtomicBool, Ordering};
        let called = std::sync::Arc::new(AtomicBool::new(false));
        let flag = called.clone();
        set_collector(Some(Box::new(move |_record: &Record| {
            flag.store(true, Ordering::SeqCst);
        })));
        record(Record {
            event: Event::PairingWindowOpened,
            peer: None,
            session: None,
            error: None,
        });
        assert!(called.load(Ordering::SeqCst));
        set_collector(None);
        called.store(false, Ordering::SeqCst);
        record(Record {
            event: Event::PairingWindowOpened,
            peer: None,
            session: None,
            error: None,
        });
        assert!(
            !called.load(Ordering::SeqCst),
            "without a collector the record must be dropped"
        );
    }

    #[test]
    fn error_ref_skips_alloc_when_uncollected() {
        let _guard = diagnostics_test_lock().blocking_lock();
        set_collector(None);
        assert!(error_ref("PairingError::WindowClosed", "Pairing window is closed").is_none());
        set_collector(Some(Box::new(|_record: &Record| {})));
        let reference = error_ref("PairingError::WindowClosed", "Pairing window is closed");
        assert!(reference.is_some());
        assert_eq!(reference.unwrap().message, "Pairing window is closed");
        set_collector(None);
    }

    #[tokio::test]
    async fn collector_may_reenter_record_and_set_collector_without_deadlock() {
        let _guard = diagnostics_test_lock().lock().await;
        use std::sync::atomic::{AtomicUsize, Ordering};
        let reentered = std::sync::Arc::new(AtomicUsize::new(0));
        let flag = reentered.clone();
        set_collector(Some(Box::new(move |entry: &Record| {
            if entry.event == Event::PairingWindowOpened {
                // Reenter record() and set_collector() from inside the
                // collector; both must complete (no deadlock).
                record(Record {
                    event: Event::PairingWindowClosed,
                    peer: None,
                    session: None,
                    error: None,
                });
                set_collector(None);
                flag.fetch_add(1, Ordering::SeqCst);
            }
        })));
        let handle = tokio::spawn(async {
            record(Record {
                event: Event::PairingWindowOpened,
                peer: None,
                session: None,
                error: None,
            });
        });
        tokio::time::timeout(std::time::Duration::from_secs(5), handle)
            .await
            .expect("collector reentry must not deadlock")
            .unwrap();
        assert_eq!(reentered.load(Ordering::SeqCst), 1);
        set_collector(None);
    }

    #[test]
    fn collector_runs_outside_the_registry_lock() {
        let _guard = diagnostics_test_lock().blocking_lock();
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::mpsc;
        use std::sync::{Condvar, Mutex as StdMutex};
        let (entered_tx, entered_rx) = mpsc::channel();
        let (probe_tx, probe_rx) = mpsc::channel();
        let finished = std::sync::Arc::new(AtomicBool::new(false));
        let done = finished.clone();
        let release = std::sync::Arc::new((StdMutex::new(false), Condvar::new()));
        let release_wait = release.clone();
        set_collector(Some(Box::new(move |_entry: &Record| {
            entered_tx.send(()).unwrap();
            let (lock, cvar) = &*release_wait;
            let mut released = lock.lock().unwrap();
            while !*released {
                released = cvar.wait(released).unwrap();
            }
            done.store(true, Ordering::SeqCst);
        })));
        let collector_thread = std::thread::spawn(move || {
            record(Record {
                event: Event::PairingWindowOpened,
                peer: None,
                session: None,
                error: None,
            });
        });
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("collector must be entered");
        // While the collector is still running, the registry lock must be
        // free: replacing the collector must succeed immediately.
        let probe = std::thread::spawn(move || {
            set_collector(None);
            probe_tx.send(()).unwrap();
        });
        probe_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("the registry lock must not be held while a collector runs");
        {
            let (lock, cvar) = &*release;
            *lock.lock().unwrap() = true;
            cvar.notify_all();
        }
        collector_thread.join().unwrap();
        probe.join().unwrap();
        assert!(finished.load(Ordering::SeqCst));
    }
}
