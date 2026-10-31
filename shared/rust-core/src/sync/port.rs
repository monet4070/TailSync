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

/// What a failed clipboard write proves about the platform.
///
/// Only a provable no-effect may release an at-most-once claim so the effect can be
/// retried (ADR-004 K2/K4). The previous `Result<(), String>` seam could not say
/// which of the two cases a failure was, so the decision was recovered by scanning
/// the message for a marker — which meant any future message that happened to
/// contain the marker could change a safety decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteEffect {
    /// The platform was provably not touched: the write never started, or the
    /// helper reported no effect before touching the pasteboard.
    NoEffect,
    Superseded,
    /// The write may already have changed the platform. The claim must stay.
    MayHaveEffect,
}

/// A failed clipboard write: the classification the claim and receipt rules need,
/// plus the text that logs and legacy façades still show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteFailure {
    effect: WriteEffect,
    detail: String,
}

impl WriteFailure {
    pub fn superseded(detail: impl Into<String>) -> Self {
        Self {
            effect: WriteEffect::Superseded,
            detail: detail.into(),
        }
    }

    pub fn is_superseded(&self) -> bool {
        self.effect == WriteEffect::Superseded
    }

    /// The platform is provably untouched, so a caller may release the claim.
    pub fn no_effect(detail: impl Into<String>) -> Self {
        Self {
            effect: WriteEffect::NoEffect,
            detail: detail.into(),
        }
    }

    /// The conservative classification: an unknown legacy string, a worker that
    /// died, or a receipt that was lost. Never releases a claim.
    pub fn may_have_effect(detail: impl Into<String>) -> Self {
        Self {
            effect: WriteEffect::MayHaveEffect,
            detail: detail.into(),
        }
    }

    pub fn effect(&self) -> WriteEffect {
        self.effect
    }

    /// Whether the platform was provably untouched. This is the only case in which
    /// an at-most-once claim may be released and the effect retried.
    pub fn is_proven_no_effect(&self) -> bool {
        matches!(self.effect, WriteEffect::NoEffect | WriteEffect::Superseded)
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }

    /// Text for logs, user-visible errors, and the legacy `Result<(), String>`
    /// façades. Only a provable no-effect adds the marker; opaque detail may itself
    /// contain it. Nothing on the
    /// decision path reads it back.
    pub fn message(&self) -> String {
        match self.effect {
            WriteEffect::NoEffect | WriteEffect::Superseded => {
                format!("{}{}", crate::protocol::NO_EFFECT_MARKER, self.detail)
            }
            WriteEffect::MayHaveEffect => self.detail.clone(),
        }
    }
}

impl std::fmt::Display for WriteFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message())
    }
}

/// A legacy error string proves nothing about the platform, so it is classified
/// conservatively. A caller that can prove a no-effect says so with
/// [`WriteFailure::no_effect`].
impl From<String> for WriteFailure {
    fn from(message: String) -> Self {
        Self::may_have_effect(message)
    }
}

/// The legacy façade at an external boundary: logs, UI text, and the remaining
/// `Result<_, String>` seams.
impl From<WriteFailure> for String {
    fn from(failure: WriteFailure) -> Self {
        failure.message()
    }
}

/// Result of a clipboard write on the async port.
pub type WriteOutcome = Result<(), WriteFailure>;

/// Check cancellation before choosing either a native or a legacy writer.
/// Once the body starts, its typed outcome owns the effect classification: a
/// later cancellation cannot prove that an effect did not already happen.
pub fn run_cancellable_write(
    cancelled: Arc<std::sync::atomic::AtomicBool>,
    write: impl FnOnce(Arc<std::sync::atomic::AtomicBool>) -> WriteOutcome,
) -> WriteOutcome {
    if cancelled.load(std::sync::atomic::Ordering::Acquire) {
        return Err(WriteFailure::no_effect("cancelled before write"));
    }
    write(cancelled)
}

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
    ) -> PortFuture<'a, Result<NativePublish, WriteFailure>>;
    fn write_text<'a>(&'a self, text: String) -> PortFuture<'a, WriteOutcome>;
    fn write_text_admitted<'a>(
        &'a self,
        text: String,
        expected_revision: Option<i64>,
        admission: Vec<AdmissionHold>,
    ) -> PortFuture<'a, WriteOutcome> {
        Box::pin(async move {
            let _admission = admission;
            if expected_revision.is_some() {
                return Err(WriteFailure::no_effect("Clipboard CAS is unavailable"));
            }
            self.write_text(text).await
        })
    }
    fn write_image<'a>(
        &'a self,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    ) -> PortFuture<'a, WriteOutcome>;
    fn write_image_admitted<'a>(
        &'a self,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
        expected_revision: Option<i64>,
        admission: Vec<AdmissionHold>,
    ) -> PortFuture<'a, WriteOutcome> {
        Box::pin(async move {
            let _admission = admission;
            if expected_revision.is_some() {
                return Err(WriteFailure::no_effect("Clipboard CAS is unavailable"));
            }
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
    async fn offload<T, F>(work: F) -> Result<T, WriteFailure>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T, WriteFailure> + Send + 'static,
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
        // A worker that died leaves the platform's state unknown: never a
        // no-effect, so a claim stays and the write is not retried automatically.
        .map_err(|_| WriteFailure::may_have_effect("clipboard worker failed"))?
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
    ) -> PortFuture<'a, Result<NativePublish, WriteFailure>> {
        Box::pin(async move {
            let platform = self.platform.clone();
            let _cancel = CancelOnDrop(cancelled.clone());
            Self::offload(move || {
                let _admission = admission;
                // The platform reports the five native outcomes itself; only a
                // transport-level failure becomes a typed write failure here.
                platform
                    .write_restore_plan(&plan, expected, cancelled)
                    .map_err(WriteFailure::from)
            })
            .await
        })
    }
    fn write_text<'a>(&'a self, text: String) -> PortFuture<'a, WriteOutcome> {
        Box::pin(async move {
            let platform = self.platform.clone();
            let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let _cancel = CancelOnDrop(flag.clone());
            Self::offload(move || platform.write_text_cancellable_typed(&text, None, flag)).await
        })
    }

    fn write_image<'a>(
        &'a self,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    ) -> PortFuture<'a, WriteOutcome> {
        Box::pin(async move {
            let platform = self.platform.clone();
            let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let _cancel = CancelOnDrop(flag.clone());
            Self::offload(move || {
                platform.write_image_cancellable_typed(width, height, &rgba, None, flag)
            })
            .await
        })
    }

    fn write_text_admitted<'a>(
        &'a self,
        text: String,
        expected_revision: Option<i64>,
        admission: Vec<AdmissionHold>,
    ) -> PortFuture<'a, WriteOutcome> {
        Box::pin(async move {
            let platform = self.platform.clone();
            let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let _cancel = CancelOnDrop(flag.clone());
            Self::offload(move || {
                let _admission = admission;
                platform.write_text_cancellable_typed(&text, expected_revision, flag)
            })
            .await
        })
    }
    fn write_image_admitted<'a>(
        &'a self,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
        expected_revision: Option<i64>,
        admission: Vec<AdmissionHold>,
    ) -> PortFuture<'a, WriteOutcome> {
        Box::pin(async move {
            let platform = self.platform.clone();
            let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let _cancel = CancelOnDrop(flag.clone());
            Self::offload(move || {
                let _admission = admission;
                platform.write_image_cancellable_typed(
                    width,
                    height,
                    &rgba,
                    expected_revision,
                    flag,
                )
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

    /// A platform that reports its own typed classification.
    struct TypedPlatform {
        text: crate::sync::WriteFailure,
        panic: bool,
    }

    impl SyncPlatform for TypedPlatform {
        fn write_text(&self, _: &str) -> Result<(), String> {
            Err(self.text.message())
        }
        fn write_text_typed(&self, _: &str) -> Result<(), crate::sync::WriteFailure> {
            if self.panic {
                panic!("fixture worker panic");
            }
            Err(self.text.clone())
        }
        fn write_text_cancellable_typed(
            &self,
            text: &str,
            _expected_revision: Option<i64>,
            cancelled: Arc<std::sync::atomic::AtomicBool>,
        ) -> Result<(), crate::sync::WriteFailure> {
            if cancelled.load(std::sync::atomic::Ordering::Acquire) {
                return Err(crate::sync::WriteFailure::no_effect(
                    "cancelled before write",
                ));
            }
            self.write_text_typed(text)
        }
        fn write_image(&self, _: u32, _: u32, _: &[u8]) -> Result<(), String> {
            Ok(())
        }
        fn set_file_progress(&self, _: &str, _: u64, _: u64) {}
        fn clear_file_progress(&self, _: Option<TransferId>, _: Option<&str>) {}
        fn set_file_batch_progress(&self, _: FileBatchProgress) {}
        fn file_batch_failed(&self, _: Option<TransferId>, _: &str) {}
        fn files_received(&self, _: FileReceiveCommit) -> PlatformResultFuture {
            Box::pin(async { Ok(()) })
        }
    }

    /// A platform that only implements the legacy string seam.
    struct LegacyStringPlatform {
        message: String,
    }

    impl SyncPlatform for LegacyStringPlatform {
        fn write_text(&self, _: &str) -> Result<(), String> {
            Err(self.message.clone())
        }
        fn write_image(&self, _: u32, _: u32, _: &[u8]) -> Result<(), String> {
            Ok(())
        }
        fn set_file_progress(&self, _: &str, _: u64, _: u64) {}
        fn clear_file_progress(&self, _: Option<TransferId>, _: Option<&str>) {}
        fn set_file_batch_progress(&self, _: FileBatchProgress) {}
        fn file_batch_failed(&self, _: Option<TransferId>, _: &str) {}
        fn files_received(&self, _: FileReceiveCommit) -> PlatformResultFuture {
            Box::pin(async { Ok(()) })
        }
    }

    /// F2 — the port carries the platform's typed classification through unchanged,
    /// and the detail text never decides it.
    #[tokio::test]
    async fn the_port_preserves_the_typed_write_classification() {
        let no_effect = BlockingPlatformPort::new(Arc::new(TypedPlatform {
            text: crate::sync::WriteFailure::no_effect("helper never started"),
            panic: false,
        }));
        let failure = no_effect.write_text("text".into()).await.unwrap_err();
        assert!(failure.is_proven_no_effect());
        assert!(failure
            .message()
            .contains(crate::protocol::NO_EFFECT_MARKER));

        let unknown = BlockingPlatformPort::new(Arc::new(TypedPlatform {
            text: crate::sync::WriteFailure::may_have_effect("clipboard helper timed out"),
            panic: false,
        }));
        let failure = unknown.write_text("text".into()).await.unwrap_err();
        assert!(!failure.is_proven_no_effect());
        assert!(!failure
            .message()
            .contains(crate::protocol::NO_EFFECT_MARKER));

        // A detail that mentions the marker must not change the kind.
        let misleading = BlockingPlatformPort::new(Arc::new(TypedPlatform {
            text: crate::sync::WriteFailure::may_have_effect(format!(
                "{}worker died after writing",
                crate::protocol::NO_EFFECT_MARKER
            )),
            panic: false,
        }));
        let failure = misleading.write_text("text".into()).await.unwrap_err();
        assert!(!failure.is_proven_no_effect());
    }

    /// F2 — a worker that dies cannot prove the platform was untouched.
    #[tokio::test]
    async fn a_worker_panic_is_a_may_have_effect_failure() {
        let port = BlockingPlatformPort::new(Arc::new(TypedPlatform {
            text: crate::sync::WriteFailure::no_effect("unused"),
            panic: true,
        }));
        let failure = port.write_text("text".into()).await.unwrap_err();
        assert!(!failure.is_proven_no_effect());
        assert_eq!(failure.effect(), WriteEffect::MayHaveEffect);
    }

    /// F2 — the legacy string bridge cannot prove a no-effect: a message that carries
    /// the marker is still classified conservatively, because a string is not
    /// evidence. (Boundary simulation of the old seam, not a historical incident.)
    #[tokio::test]
    async fn a_legacy_string_failure_stays_conservative() {
        let port = BlockingPlatformPort::new(Arc::new(LegacyStringPlatform {
            message: format!(
                "{}worker died after writing",
                crate::protocol::NO_EFFECT_MARKER
            ),
        }));
        let failure = port.write_text("text".into()).await.unwrap_err();
        assert!(!failure.is_proven_no_effect());
        assert_eq!(failure.effect(), WriteEffect::MayHaveEffect);
    }

    /// F2 — a write that never started is provably no-effect, and the default bridge
    /// decides that from the cancellation flag rather than from any message.
    #[tokio::test]
    async fn a_pre_write_cancellation_is_a_no_effect() {
        let platform = LegacyStringPlatform {
            message: format!("{}must not be consulted", crate::protocol::NO_EFFECT_MARKER),
        };
        let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let failure = platform
            .write_text_cancellable_typed("text", None, cancelled)
            .unwrap_err();
        assert!(failure.is_proven_no_effect());
        assert_eq!(failure.detail(), "cancelled before write");
    }

    #[test]
    fn a_pre_cancelled_writer_cannot_start_an_effect() {
        let calls = AtomicUsize::new(0);
        let flag = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let failure = run_cancellable_write(flag, |_| {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .unwrap_err();
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(failure.is_proven_no_effect());
    }

    #[test]
    fn cancellation_after_the_body_started_does_not_reclassify_its_effect() {
        let calls = AtomicUsize::new(0);
        let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let expected = flag.clone();
        let failure = run_cancellable_write(flag, |observed| {
            assert!(Arc::ptr_eq(&observed, &expected));
            calls.fetch_add(1, Ordering::SeqCst); // The native effect has started.
            observed.store(true, Ordering::Release);
            Err(WriteFailure::may_have_effect(
                "no-effect: receipt lost after writing",
            ))
        })
        .unwrap_err();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(!failure.is_proven_no_effect());
    }
}
