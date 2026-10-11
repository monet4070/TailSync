//! Asynchronous clipboard port (repair plan §4 R3-01).
//!
//! The synchronous `SyncPlatform` seam was written when a clipboard write was an
//! in-process call. Composite publishing turned it into "spawn a helper process and
//! wait up to five seconds", and the seam had no way to express that: the call site
//! held the engine guard across it, so one peer sending text events could stall the
//! whole engine and a Tokio worker with it.
//!
//! This port is the async shape the work actually has: object-safe, `Arc`-held, and
//! returning a boxed `Send` future so it can be awaited from any task. The engine is
//! only ever asked for the *context* of an operation under its lock; the port call
//! itself happens outside it.
//!
//! [`BlockingPlatformPort`] adapts the existing synchronous implementations: the
//! blocking call runs on the blocking pool, never on an async worker. That is a
//! bridge, not the destination — the plan's target is for the platform adapters to
//! implement this trait directly.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::composite::NativePublish;
use crate::sync::SyncPlatform;

/// FIFO publication coordinator. Hold this before checking engine generation and
/// through outcome settlement. The blocking adapter additionally owns its permit
/// until the worker exits, including when the async caller is cancelled.
pub async fn publication_permit() -> tokio::sync::OwnedMutexGuard<()> {
    static GATE: std::sync::LazyLock<Arc<tokio::sync::Mutex<()>>> =
        std::sync::LazyLock::new(|| Arc::new(tokio::sync::Mutex::new(())));
    GATE.clone().lock_owned().await
}

pub type AdmissionHold = Arc<dyn Send + Sync>;
struct CancelOnDrop(Arc<std::sync::atomic::AtomicBool>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::Release);
    }
}

/// A boxed future returned by the port.
pub type PortFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Object-safe asynchronous clipboard port.
///
/// Every method takes owned data: the future must not borrow from the caller, so it
/// can outlive the scope that produced the payload.
pub trait ClipboardPort: Send + Sync {
    fn prepare_restore_plan_admitted<'a>(
        &'a self,
        plan: crate::composite::restore::RestorePlan,
        cancelled: Arc<std::sync::atomic::AtomicBool>,
        admission: Vec<AdmissionHold>,
    ) -> PortFuture<'a, Result<crate::composite::restore::RestorePlan, String>> {
        Box::pin(async move {
            let _admission = admission;
            if cancelled.load(std::sync::atomic::Ordering::Acquire) {
                return Err("Restore preparation cancelled".into());
            }
            Ok(plan)
        })
    }
    fn publish_restore_plan_admitted<'a>(
        &'a self,
        plan: crate::composite::restore::RestorePlan,
        expected: Option<i64>,
        cancelled: Arc<std::sync::atomic::AtomicBool>,
        admission: Vec<AdmissionHold>,
    ) -> PortFuture<'a, Result<NativePublish, String>>;
    fn write_text<'a>(&'a self, text: String) -> PortFuture<'a, Result<(), String>>;
    fn write_text_admitted<'a>(
        &'a self,
        text: String,
        admission: Vec<AdmissionHold>,
    ) -> PortFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let _admission = admission;
            self.write_text(text).await
        })
    }
    fn write_image<'a>(
        &'a self,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    ) -> PortFuture<'a, Result<(), String>>;
    fn write_image_admitted<'a>(
        &'a self,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
        admission: Vec<AdmissionHold>,
    ) -> PortFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let _admission = admission;
            self.write_image(width, height, rgba).await
        })
    }
    /// Cheap, non-blocking: used to capture the revision *before* an operation.
    fn clipboard_revision(&self) -> Option<i64>;
}

/// Bridges a synchronous [`SyncPlatform`] onto the async port.
///
/// The blocking call is moved to the blocking pool, so awaiting the port never
/// occupies an async worker. Callers must still release the engine guard first:
/// moving the call off the worker does not help if the lock is held across the
/// await.
pub struct BlockingPlatformPort {
    platform: Arc<dyn SyncPlatform>,
}

impl BlockingPlatformPort {
    pub fn new(platform: Arc<dyn SyncPlatform>) -> Self {
        Self { platform }
    }

    /// Run a blocking platform call on the blocking pool and flatten the two error
    /// layers: the outer one is "the worker died", the inner one is the platform's own
    /// `Result`.
    async fn offload<T, F>(work: F) -> Result<T, String>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T, String> + Send + 'static,
    {
        // Move the owned permit into the worker: cancellation of the waiter must
        // not let a second publisher overlap a worker that is still running.
        static GATE: std::sync::LazyLock<Arc<tokio::sync::Mutex<()>>> =
            std::sync::LazyLock::new(|| Arc::new(tokio::sync::Mutex::new(())));
        let permit = GATE.clone().lock_owned().await;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            work()
        })
        .await
        .map_err(|_| "clipboard worker failed".to_string())?
    }
}

impl ClipboardPort for BlockingPlatformPort {
    fn prepare_restore_plan_admitted<'a>(
        &'a self,
        plan: crate::composite::restore::RestorePlan,
        cancelled: Arc<std::sync::atomic::AtomicBool>,
        admission: Vec<AdmissionHold>,
    ) -> PortFuture<'a, Result<crate::composite::restore::RestorePlan, String>> {
        Box::pin(async move {
            let _cancel = CancelOnDrop(cancelled.clone());
            let platform = self.platform.clone();
            tokio::task::spawn_blocking(move || {
                let _admission = admission;
                platform.prepare_restore_plan(plan, cancelled)
            })
            .await
            .map_err(|_| "Restore preparation worker failed".to_string())?
        })
    }
    fn publish_restore_plan_admitted<'a>(
        &'a self,
        plan: crate::composite::restore::RestorePlan,
        expected: Option<i64>,
        cancelled: Arc<std::sync::atomic::AtomicBool>,
        admission: Vec<AdmissionHold>,
    ) -> PortFuture<'a, Result<NativePublish, String>> {
        Box::pin(async move {
            let platform = self.platform.clone();
            let _cancel = CancelOnDrop(cancelled.clone());
            Self::offload(move || {
                let _admission = admission;
                platform.write_restore_plan(&plan, expected, cancelled)
            })
            .await
        })
    }
    fn write_text<'a>(&'a self, text: String) -> PortFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let platform = self.platform.clone();
            let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let _cancel = CancelOnDrop(flag.clone());
            Self::offload(move || platform.write_text_cancellable(&text, flag)).await
        })
    }

    fn write_image<'a>(
        &'a self,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    ) -> PortFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let platform = self.platform.clone();
            let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let _cancel = CancelOnDrop(flag.clone());
            Self::offload(move || platform.write_image_cancellable(width, height, &rgba, flag))
                .await
        })
    }

    fn write_text_admitted<'a>(
        &'a self,
        text: String,
        admission: Vec<AdmissionHold>,
    ) -> PortFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let platform = self.platform.clone();
            let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let _cancel = CancelOnDrop(flag.clone());
            Self::offload(move || {
                let _admission = admission;
                platform.write_text_cancellable(&text, flag)
            })
            .await
        })
    }
    fn write_image_admitted<'a>(
        &'a self,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
        admission: Vec<AdmissionHold>,
    ) -> PortFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let platform = self.platform.clone();
            let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let _cancel = CancelOnDrop(flag.clone());
            Self::offload(move || {
                let _admission = admission;
                platform.write_image_cancellable(width, height, &rgba, flag)
            })
            .await
        })
    }
    fn clipboard_revision(&self) -> Option<i64> {
        self.platform.clipboard_revision()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composite::PortableBundle;
    use crate::protocol::TransferId;
    use crate::sync::{FileBatchProgress, FileReceiveCommit, PlatformResultFuture};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    struct Slow {
        calls: AtomicUsize,
        revision: Option<i64>,
    }

    impl SyncPlatform for Slow {
        fn write_text(&self, _: &str) -> Result<(), String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(250));
            Ok(())
        }
        fn write_image(&self, _: u32, _: u32, _: &[u8]) -> Result<(), String> {
            Ok(())
        }
        fn clipboard_revision(&self) -> Option<i64> {
            self.revision
        }
        fn write_composite_cancellable(
            &self,
            _: &PortableBundle,
            _: Option<i64>,
            _: Arc<std::sync::atomic::AtomicBool>,
        ) -> Result<NativePublish, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(NativePublish::NoEffect {
                detail: "fixture".into(),
            })
        }
        fn set_file_progress(&self, _: &str, _: u64, _: u64) {}
        fn clear_file_progress(&self, _: Option<TransferId>, _: Option<&str>) {}
        fn set_file_batch_progress(&self, _: FileBatchProgress) {}
        fn file_batch_failed(&self, _: Option<TransferId>, _: &str) {}
        fn files_received(&self, _: FileReceiveCommit) -> PlatformResultFuture {
            Box::pin(async { Ok(()) })
        }
    }

    /// R3-01 / Gate R3 — awaiting the port must not stall the async runtime: a 10 ms
    /// timer has to fire while a 250 ms blocking write is in flight.
    #[tokio::test(flavor = "current_thread")]
    async fn the_port_does_not_block_the_async_runtime() {
        let platform = Arc::new(Slow {
            calls: AtomicUsize::new(0),
            revision: None,
        });
        let port = BlockingPlatformPort::new(platform.clone());

        let started = Instant::now();
        let pulse = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            started.elapsed().as_millis()
        });
        tokio::task::yield_now().await;

        port.write_text("fixture".to_string())
            .await
            .expect("the mock always succeeds");

        let observed = pulse.await.expect("timer task must run");
        assert_eq!(platform.calls.load(Ordering::SeqCst), 1);
        assert!(
            observed < 150,
            "a 10ms timer was observed at {observed}ms while a 250ms native write was in \
             flight: the port is still blocking the runtime"
        );
    }

    /// The port is object-safe and usable behind an `Arc<dyn ClipboardPort>`.
    #[tokio::test]
    async fn the_port_is_object_safe() {
        let platform = Arc::new(Slow {
            calls: AtomicUsize::new(0),
            revision: None,
        });
        let port: Arc<dyn ClipboardPort> = Arc::new(BlockingPlatformPort::new(platform.clone()));
        port.write_text("fixture".to_string()).await.unwrap();
        assert_eq!(port.clipboard_revision(), None);
        assert_eq!(platform.calls.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn default_plan_guard_uses_observed_revision_and_never_writes_without_it() {
        use crate::composite::{restore::*, Completeness, Mime, Representation};
        let plan = RestorePlan::compatibility(
            PortableBundle {
                representations: vec![Representation {
                    mime: Mime::Text,
                    bytes: b"fixture".to_vec(),
                }],
                resources: vec![],
                bindings: vec![],
                completeness: Completeness::Complete,
            },
            RestoreInput {
                source_platform: None,
                requested_format: None,
            },
            RestoreTarget::Adapter,
        )
        .unwrap();
        for revision in [None, Some(41)] {
            let platform = Slow {
                calls: AtomicUsize::new(0),
                revision,
            };
            let outcome = platform
                .write_restore_plan(&plan, Some(42), Arc::new(false.into()))
                .unwrap();
            match revision {
                Some(observed) => assert_eq!(outcome.receipt().unwrap().revision, observed),
                None => assert!(matches!(outcome, NativePublish::NoEffect { .. })),
            }
            assert_eq!(platform.calls.load(Ordering::SeqCst), 0);
            if let Some(observed) = revision {
                platform
                    .write_restore_plan(&plan, Some(observed), Arc::new(false.into()))
                    .unwrap();
                assert_eq!(platform.calls.load(Ordering::SeqCst), 1);
            }
        }
    }
    struct Controlled {
        entered: tokio::sync::Notify,
        release: std::sync::atomic::AtomicBool,
        cancellation_seen: std::sync::atomic::AtomicBool,
        stopped: tokio::sync::Notify,
    }
    impl SyncPlatform for Controlled {
        fn write_text(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn write_image(&self, _: u32, _: u32, _: &[u8]) -> Result<(), String> {
            Ok(())
        }
        fn write_composite_cancellable(
            &self,
            bundle: &PortableBundle,
            _: Option<i64>,
            cancelled: Arc<std::sync::atomic::AtomicBool>,
        ) -> Result<NativePublish, String> {
            assert_eq!(
                bundle.representation(crate::composite::Mime::Text).unwrap(),
                b"owned output after source deletion"
            );
            self.entered.notify_one();
            let deadline = Instant::now() + Duration::from_secs(2);
            while !self.release.load(Ordering::Acquire) && Instant::now() < deadline {
                if cancelled.load(Ordering::Acquire) {
                    self.cancellation_seen.store(true, Ordering::Release);
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            assert_eq!(
                bundle.representation(crate::composite::Mime::Text).unwrap(),
                b"owned output after source deletion"
            );
            self.stopped.notify_one();
            Ok(NativePublish::Unknown {
                detail: "cancelled fixture".into(),
            })
        }
        fn set_file_progress(&self, _: &str, _: u64, _: u64) {}
        fn clear_file_progress(&self, _: Option<TransferId>, _: Option<&str>) {}
        fn set_file_batch_progress(&self, _: FileBatchProgress) {}
        fn file_batch_failed(&self, _: Option<TransferId>, _: &str) {}
        fn files_received(&self, _: FileReceiveCommit) -> PlatformResultFuture {
            Box::pin(async { Ok(()) })
        }
    }

    #[tokio::test]
    async fn o06_cancelled_plan_keeps_owned_output_fifo_and_memory_until_worker_stops() {
        use crate::composite::restore::{RestoreInput, RestorePlan, RestoreTarget};
        let ledger = Arc::new(crate::composite::budgets::BudgetLedger::default());
        let memory: AdmissionHold = Arc::new(ledger.reserve_pipeline(1024 * 1024).unwrap());
        let publication: AdmissionHold = Arc::new(publication_permit().await);
        let platform = Arc::new(Controlled {
            entered: tokio::sync::Notify::new(),
            release: false.into(),
            cancellation_seen: false.into(),
            stopped: tokio::sync::Notify::new(),
        });
        let port = Arc::new(BlockingPlatformPort::new(platform.clone()));
        let plan = RestorePlan::compatibility(
            PortableBundle {
                representations: vec![crate::composite::Representation {
                    mime: crate::composite::Mime::Text,
                    bytes: b"owned output after source deletion".to_vec(),
                }],
                resources: vec![],
                bindings: vec![],
                completeness: crate::composite::Completeness::Complete,
            },
            RestoreInput {
                source_platform: None,
                requested_format: None,
            },
            RestoreTarget::Adapter,
        )
        .unwrap();
        let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_flag = flag.clone();
        let task = tokio::spawn(async move {
            port.publish_restore_plan_admitted(plan, None, worker_flag, vec![memory, publication])
                .await
        });
        tokio::time::timeout(Duration::from_secs(2), platform.entered.notified())
            .await
            .unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(flag.load(Ordering::Acquire));
        assert_eq!(ledger.snapshot().pipeline_reserved, 1024 * 1024);
        assert!(
            tokio::time::timeout(Duration::from_millis(40), publication_permit())
                .await
                .is_err()
        );
        assert!(platform.cancellation_seen.load(Ordering::Acquire));
        platform.release.store(true, Ordering::Release);
        tokio::time::timeout(Duration::from_secs(1), platform.stopped.notified())
            .await
            .unwrap();
        let _next = tokio::time::timeout(Duration::from_secs(1), publication_permit())
            .await
            .unwrap();
        assert_eq!(ledger.snapshot().pipeline_reserved, 0);
    }
}
