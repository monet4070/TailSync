// Shared terminal policy for one observed composite capture (F3-B).
//
// Both platform capture transactions (`macos/src-tauri/src/clipboard.rs` and
// `windows/src-tauri/src/clipboard.rs`) end a tick with the same two decisions:
// whether a failed capture stays visible and unacknowledged until a durable
// fault is recorded, and whether a completed decision acknowledges the observed
// revision and clears its retry budget. The platform save, warning, version and
// binding steps stay in each wrapper; only the failed terminal and the
// acknowledge-and-clear terminal are single-sourced here.
//
// This file is a canonical include pasted inside each wrapper's private
// `composite_capture` module, so [`CaptureAcknowledger`], [`CaptureEnvironment`]
// and the surrounding `CaptureRetry` type resolve to the platform definitions.

/// The two ways a composite-capture tick can end without a stored body.
///
/// `NotSaved` is a failed history/storage write for a capture the platform did
/// recognise; `Failed` is a capture that never produced a saveable body. They
/// differ in exactly one step: `NotSaved` records the visible warning and
/// re-checks that the observed revision is still current before spending its
/// retry allowance, while `Failed` goes straight to the bounded retry schedule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CaptureFailure {
    NotSaved,
    Failed,
}

/// Acknowledges the observed revision and clears its retry budget.
///
/// Every terminal decision that owns the observed copy (self-write, a completed
/// save, an empty or suppressed capture) uses this order: acknowledge, then
/// clear. Late provider results are discarded before this terminal. A copy
/// superseded during a successful save may still acknowledge its own stored
/// revision; it must never acknowledge the newer revision.
pub(super) fn acknowledge_capture(
    retry: &mut tailsync_core::composite::capture_retry::CaptureRetry,
    ack: &mut dyn CaptureAcknowledger,
    revision: i64,
) {
    ack.acknowledge(revision);
    retry.clear();
}

/// Runs the shared failed-capture terminal for one tick.
///
/// `NotSaved` first records the visible warning, then requires the observed
/// revision to still be current before it spends a retry; once the bounded
/// retries are exhausted only a durable fault record acknowledges the copy.
/// `Failed` keeps its original ordering: the bounded retries run first and only
/// an exhausted schedule records the durable fault. A `false` fault never
/// acknowledges, so a dropped capture is never acknowledged or broadcast.
pub(super) async fn finish_failed_capture(
    observed: i64,
    failure: CaptureFailure,
    retry: &mut tailsync_core::composite::capture_retry::CaptureRetry,
    ack: &mut dyn CaptureAcknowledger,
    env: &CaptureEnvironment,
) {
    let kind = match failure {
        CaptureFailure::NotSaved => {
            tailsync_core::sync_warning::record_capture_not_saved();
            if !retry.is_current(observed, env.revision()) {
                retry.clear();
                return;
            }
            "capture_not_saved"
        }
        CaptureFailure::Failed => "capture_failed",
    };
    if !retry.retry(observed, env.now()) && env.capture_fault(observed, kind).await {
        acknowledge_capture(retry, ack, observed);
    }
}
