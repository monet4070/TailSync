//! R0 production-seam regressions for the composite-clipboard repair.
//!
//! Plan: `docs/COMPOSITE-CLIPBOARD-REPAIR-EXECUTION-PLAN-2026-10-04.md` §4 R0.2.
//! Ledger: `artifacts/composite-repair/2026-10-04-r0/expected-failures.json`.
//!
//! These assert the **desired** behaviour of the defects recorded in
//! `docs/COMPOSITE-CLIPBOARD-ACCEPTANCE-REVIEW-2026-10-04.md`. They are therefore
//! expected to FAIL until the owning stage lands; they are gated behind
//! `repair-regressions` rather than weakened or ignored, so the default gate stays
//! meaningful while the fix is in flight. Each ledger entry is removed as its stage
//! closes, and at R6 this suite joins the standard test entry.
//!
//! Run with (see plan §6):
//! `node scripts/run-isolated-tests.mjs -- cargo test --locked --offline \
//!    -p tailsync-core --features repair-regressions,test-support --test repair_regressions -- --test-threads=1`
#![cfg(feature = "repair-regressions")]

use std::sync::Mutex;
use std::time::{Duration, Instant};

use tailsync_core::composite::origin::{Origin, OriginTracker};
use tailsync_core::composite::{packed_to_png, Completeness, Mime, PortableBundle, Representation};
use tailsync_core::protocol::native_clipboard::{PublishReceipt, PublishState};
use tailsync_core::protocol::TransferId;
use tailsync_core::sync::{
    FileBatchProgress, FileReceiveCommit, PlatformResultFuture, SyncEngine, SyncPlatform,
};

// ---------------------------------------------------------------------------
// RR-05 / H-20 / T06 — origin fingerprint must not depend on representation order
// ---------------------------------------------------------------------------

/// The local fingerprint used for *origin* decisions.
///
/// R2-03 landed `composite::fingerprint::origin_fingerprint`, which is what
/// `shared/platform-clipboard-composite.rs` now records and classifies with on both
/// the publish and the read-back side. K1 keeps the *wire* digest order-sensitive on
/// purpose; only the origin fingerprint is a projection that survives OS read-back
/// normalisation.
fn origin_fingerprint(bundle: &PortableBundle) -> [u8; 32] {
    tailsync_core::composite::fingerprint::origin_fingerprint(bundle)
        .expect("fixture bundle must be a valid portable bundle")
}

fn two_representation_bundle(order: [Mime; 2]) -> PortableBundle {
    let text = b"<p>order-independent origin fixture</p>".to_vec();
    let png = packed_to_png(1, 1, &[1, 2, 3, 255]).expect("1x1 png");
    let representations = order
        .iter()
        .map(|mime| Representation {
            mime: *mime,
            bytes: match mime {
                Mime::Text => text.clone(),
                Mime::Png => png.clone(),
                other => panic!("fixture only builds Text/Png, got {other:?}"),
            },
        })
        .collect();
    PortableBundle {
        representations,
        resources: Vec::new(),
        bindings: Vec::new(),
        completeness: Completeness::Complete,
    }
}

/// H-20 was reproduced with the real packaged helper: a bundle published as
/// `[Png, Text]` is read back by the OS as `[Text, Png]`; because the recorded
/// fingerprint is order-sensitive the device classifies **its own write** as
/// `External` and re-broadcasts it (echo / duplicate history).
///
/// Desired: the origin fingerprint is a projection of the observable publish set,
/// so a permutation of the same content is still `SelfWrite`.
#[test]
fn rr05_origin_fingerprint_survives_representation_order_change() {
    let published = two_representation_bundle([Mime::Png, Mime::Text]);
    let read_back = two_representation_bundle([Mime::Text, Mime::Png]);

    let revision = 42;
    let token = [7_u8; 16];
    let mut tracker = OriginTracker::default();
    tracker.record(revision, origin_fingerprint(&published), token);

    let classified = tracker.classify(Some(revision), origin_fingerprint(&read_back), Some(token));
    assert_eq!(
        classified,
        Origin::SelfWrite,
        "an order-permuted read-back of our own publish must stay SelfWrite; \
         classifying it External re-broadcasts the device's own clipboard write (H-20)"
    );
}

/// Guard for the inverse direction required by K3: a *new user copy* of identical
/// content must never be swallowed by the self-write marker, even when the
/// fingerprint matches and the old token is still present.
#[test]
fn rr05b_new_revision_with_equal_content_is_still_external() {
    let published = two_representation_bundle([Mime::Png, Mime::Text]);
    let same_content = two_representation_bundle([Mime::Png, Mime::Text]);

    let mut tracker = OriginTracker::default();
    let token = [7_u8; 16];
    tracker.record(42, origin_fingerprint(&published), token);

    assert_eq!(
        tracker.classify(Some(43), origin_fingerprint(&same_content), Some(token)),
        Origin::External,
        "a new native revision is a new user intent even when the bytes are equal"
    );
}

// ---------------------------------------------------------------------------
// RR-11 / F / T12 — native I/O must not run inside the engine lock on an async worker
// ---------------------------------------------------------------------------

/// Stands in for the platform adapter whose `write_text` now reaches the helper
/// subprocess (up to a 5 s deadline) instead of an in-process pasteboard call.
struct SlowPlatform {
    delay: Duration,
}

impl SyncPlatform for SlowPlatform {
    fn write_text(&self, _: &str) -> Result<(), String> {
        std::thread::sleep(self.delay);
        Ok(())
    }
    fn write_image(&self, _: u32, _: u32, _: &[u8]) -> Result<(), String> {
        Ok(())
    }
    fn write_composite(
        &self,
        _: &PortableBundle,
        _: Option<i64>,
    ) -> Result<tailsync_core::composite::NativePublish, String> {
        Ok(tailsync_core::composite::NativePublish::Published(
            PublishReceipt {
                state: PublishState::Published,
                revision: 1,
                token: [1; 16],
            },
        ))
    }
    fn set_file_progress(&self, _: &str, _: u64, _: u64) {}
    fn clear_file_progress(&self, _: Option<TransferId>, _: Option<&str>) {}
    fn set_file_batch_progress(&self, _: FileBatchProgress) {}
    fn file_batch_failed(&self, _: Option<TransferId>, _: &str) {}
    fn files_received(&self, _: FileReceiveCommit) -> PlatformResultFuture {
        Box::pin(async { Ok(()) })
    }
}

/// The verification measured the old path with the production
/// `handle_incoming_text`: a 250 ms platform call dragged a 10 ms timer out to
/// 255 ms, because `sync_engine.lock()` was held across the await.
///
/// R3-01 replaced that with an async port plus a two-phase engine API. This asserts
/// the public contract of the new seam: the engine lock is only held for the
/// prepare/commit bookkeeping, and the port call itself runs off the async worker.
/// The authoritative production-path check (through `process_reliable_event`) lives
/// next to the receiver.
#[tokio::test(flavor = "current_thread")]
async fn rr11_native_write_does_not_stall_the_async_runtime() {
    const NATIVE_DELAY_MS: u64 = 250;
    const TIMER_MS: u64 = 10;
    // Generous ceiling: catches "blocked for the whole native call" without being
    // flaky about scheduler jitter on a loaded machine.
    const CEILING_MS: u128 = 150;

    let platform = std::sync::Arc::new(SlowPlatform {
        delay: Duration::from_millis(NATIVE_DELAY_MS),
    });
    let mut engine = SyncEngine::new();
    engine.set_platform(platform);
    let engine = std::sync::Arc::new(tokio::sync::Mutex::new(engine));

    let started = Instant::now();
    let pulse = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(TIMER_MS)).await;
        started.elapsed().as_millis()
    });
    tokio::task::yield_now().await;

    // Phase 1: prepare under the lock. Phase 2: the port call, with no lock held.
    let (prepared, port) = {
        let mut engine = engine.lock().await;
        (
            engine
                .prepare_incoming_text("synthetic delayed adapter")
                .expect("prepare must succeed"),
            engine
                .clipboard_port()
                .expect("the mock platform is attached"),
        )
    };
    let outcome = port
        .write_text("synthetic delayed adapter".to_string())
        .await;
    // Phase 3: commit under the lock.
    engine
        .lock()
        .await
        .finish_incoming_text(prepared, outcome)
        .expect("the mock platform always succeeds");

    let observed = pulse.await.expect("timer task must run");
    assert!(
        observed < CEILING_MS,
        "a {TIMER_MS}ms timer was observed at {observed}ms while a {NATIVE_DELAY_MS}ms native \
         write was in flight: the native call still runs inside the engine guard or on an \
         async worker (F, G-13, H-18)"
    );
}

/// The `Mutex` import keeps the mock honest about `Send + Sync` requirements and
/// documents that these regressions are single-threaded by contract (plan §6).
#[allow(dead_code)]
static SERIAL: Mutex<()> = Mutex::new(());
