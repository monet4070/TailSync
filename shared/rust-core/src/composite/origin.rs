//! Pure bounded rules; each platform owns its tracker and native revisions.
use std::collections::VecDeque;
use std::time::{Duration, Instant};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// The observation matches a known local operation: same clipboard epoch
    /// revision and the same normalised publish fingerprint (the token, when
    /// present, must match too).
    SelfWrite,
    /// No known local operation covers this revision: a genuine new user copy.
    External,
    /// A known operation covers this revision and token, but the content
    /// fingerprint differs. The write is *not* ours to suppress and *not* safe to
    /// re-broadcast as a fresh user copy, so callers must keep the diagnostic and
    /// must not treat it as `External` (ADR-004 K3).
    UnknownMismatch,
    /// The revision itself is unusable (unstable snapshot, or the platform cannot
    /// report one).
    Unknown,
}
struct Receipt {
    revision: i64,
    digest: [u8; 32],
    token: [u8; 16],
    at: Instant,
    consumed: bool,
    known: bool,
}
/// An operation whose helper is running, or whose receipt was lost.
///
/// Registered **before** the helper starts (K4): without it, a timeout would leave
/// no trace and the device would classify its own write as `External` and
/// re-broadcast it.
struct Intent {
    token: [u8; 16],
    digest: [u8; 32],
    pid: Option<u32>,
    spawn_guard: bool,
    recovered: bool,
}

/// Active/unknown intents have their own hard limit. They must never be evicted by
/// the confirmed-receipt cache: an evicted intent is exactly the case that turns a
/// self-write into an echo.
const INTENT_LIMIT: usize = 64;

/// Restart metadata contains no clipboard body or cross-epoch revision number.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DurableOrigin {
    token: [u8; 16],
    digest: [u8; 32],
    active: bool,
    known: bool,
    #[serde(default)]
    pid: Option<u32>,
    #[serde(default)]
    spawn_guard: bool,
}
#[derive(Default)]
pub struct OriginTracker {
    receipts: VecDeque<Receipt>,
    intents: Vec<Intent>,
    restored: Vec<DurableOrigin>,
    overflow_unknown: bool,
}
impl OriginTracker {
    pub fn record(&mut self, revision: i64, digest: [u8; 32], token: [u8; 16]) {
        self.record_at(revision, digest, token, Instant::now());
        self.discard_intent(token);
    }
    pub fn quarantine(&mut self) {
        self.overflow_unknown = true;
    }
    pub fn is_quarantined(&self) -> bool {
        self.overflow_unknown
    }
    pub fn durable_state(&self) -> Vec<DurableOrigin> {
        let mut state = self.restored.clone();
        state.extend(self.intents.iter().map(|r| DurableOrigin {
            token: r.token,
            digest: r.digest,
            active: true,
            known: false,
            pid: r.pid,
            spawn_guard: r.spawn_guard,
        }));
        state.extend(
            self.receipts
                .iter()
                .enumerate()
                .filter(|(i, r)| !r.consumed || *i + 1 == self.receipts.len())
                .map(|(_, r)| DurableOrigin {
                    token: r.token,
                    digest: r.digest,
                    active: false,
                    known: r.known,
                    pid: None,
                    spawn_guard: false,
                }),
        );
        state
    }
    pub fn from_durable(state: Vec<DurableOrigin>) -> Result<Self, String> {
        if state.len() > 192 {
            return Err("Origin operation capacity exceeded".into());
        }
        let mut tracker = Self::default();
        for r in state {
            if r.active {
                tracker.intents.push(Intent {
                    token: r.token,
                    digest: r.digest,
                    pid: r.pid,
                    spawn_guard: r.spawn_guard,
                    recovered: true,
                });
            } else {
                tracker.restored.push(r);
            }
        }
        if tracker.intents.len() > INTENT_LIMIT {
            return Err("Origin intent capacity exceeded".into());
        }
        Ok(tracker)
    }
    pub fn classify(
        &mut self,
        revision: Option<i64>,
        digest: [u8; 32],
        token: Option<[u8; 16]>,
    ) -> Origin {
        self.classify_at(revision, digest, token, Instant::now())
    }
    /// Register the intent for an operation that is about to touch the clipboard.
    ///
    /// Returns `false` when the intent budget is full, in which case the caller must
    /// **not** start the helper: publishing without a registered intent is what makes
    /// a lost receipt unrecoverable.
    pub fn record_intent(&mut self, token: [u8; 16], digest: [u8; 32]) -> bool {
        self.record_intent_at(token, digest, Instant::now())
    }

    fn record_intent_at(&mut self, token: [u8; 16], digest: [u8; 32], now: Instant) -> bool {
        self.prune(now);
        if self.intents.iter().any(|intent| intent.token == token) {
            return true;
        }
        if self.intents.len() >= INTENT_LIMIT
            || self.receipts.iter().filter(|r| !r.consumed).count() + self.restored.len() >= 128
            || self.overflow_unknown
        {
            return false;
        }
        self.intents.push(Intent {
            token,
            digest,
            pid: None,
            spawn_guard: true,
            recovered: false,
        });
        true
    }

    pub fn mark_spawned(&mut self, token: [u8; 16], pid: u32) {
        if let Some(intent) = self.intents.iter_mut().find(|r| r.token == token) {
            intent.pid = Some(pid);
        }
    }
    /// Only restarted attempts are reconciled by process liveness. A live local
    /// attempt can still be between durable intent registration and spawn.
    pub fn recovery_processes(&self) -> Vec<([u8; 16], Option<u32>, bool)> {
        self.intents
            .iter()
            .filter(|r| r.recovered)
            .map(|r| (r.token, r.pid, r.spawn_guard))
            .collect()
    }

    /// Close an intent at the resolution revision established after the helper was
    /// terminated and reaped (K4). The content stays `Unknown` as an outcome; the
    /// record exists so the observation is recognised as ours rather than rebroadcast.
    /// The digest bound to the intent is the fingerprint we *tried* to publish, so
    /// the caller only has to supply the resolution revision.
    pub fn resolve_intent(&mut self, token: [u8; 16], revision: i64) {
        let now = Instant::now();
        let Some(index) = self.intents.iter().position(|intent| intent.token == token) else {
            return;
        };
        let digest = self.intents[index].digest;
        self.intents.remove(index);
        self.record_at(revision, digest, token, now);
        if let Some(receipt) = self.receipts.back_mut() {
            receipt.known = false;
        }
    }

    /// Drop an intent for an operation that provably had no effect.
    pub fn discard_intent(&mut self, token: [u8; 16]) {
        self.intents.retain(|intent| intent.token != token);
    }

    /// Whether any operation is still unresolved, i.e. a side effect may have
    /// happened and its receipt is missing.
    pub fn has_unresolved_intent(&self) -> bool {
        !self.intents.is_empty()
    }

    pub fn unresolved_intent_count(&self) -> usize {
        self.intents.len()
    }

    fn record_at(&mut self, revision: i64, digest: [u8; 32], token: [u8; 16], now: Instant) {
        self.prune(now);
        if self.receipts.len() >= 128 {
            if let Some(index) = self.receipts.iter().position(|r| r.consumed) {
                self.receipts.remove(index);
            } else {
                self.overflow_unknown = true;
                return;
            }
        }
        self.receipts.push_back(Receipt {
            revision,
            digest,
            token,
            at: now,
            consumed: false,
            known: true,
        });
    }
    fn prune(&mut self, now: Instant) {
        self.receipts.retain(|r| {
            !r.consumed || now.saturating_duration_since(r.at) <= Duration::from_secs(60)
        });
    }
    fn classify_at(
        &mut self,
        revision: Option<i64>,
        digest: [u8; 32],
        token: Option<[u8; 16]>,
        now: Instant,
    ) -> Origin {
        self.prune(now);
        let Some(revision) = revision else {
            return Origin::Unknown;
        };
        // Across a restart, re-anchor only against a stable observation. Never
        // compare a persisted raw sequence/changeCount across clipboard epochs.
        let restored = std::mem::take(&mut self.restored);
        for r in restored {
            self.record_at(revision, digest, r.token, now);
            if let Some(receipt) = self.receipts.back_mut() {
                receipt.known = r.known && token == Some(r.token) && digest == r.digest;
            }
        }
        let mut mismatch = self.overflow_unknown;
        for receipt in &mut self.receipts {
            if !receipt.known && receipt.revision == revision {
                receipt.consumed = true;
                mismatch = true;
                continue;
            }
            if receipt.revision != revision {
                continue;
            }
            // A token, when supplied, is correlation evidence: it must match before
            // the receipt can speak for this observation at all.
            if let Some(token) = token {
                if receipt.token != token {
                    continue;
                }
            }
            receipt.consumed = true;
            if receipt.known && receipt.digest == digest {
                return Origin::SelfWrite;
            }
            mismatch = true;
        }
        if mismatch {
            return Origin::UnknownMismatch;
        }
        // K4: while an operation is unresolved we cannot prove whether the observed
        // content is ours. Broadcasting it as a fresh user copy would either echo our
        // own write or duplicate the user's, so the observation stays a mismatch the
        // caller must not publish. It is explicitly *not* returned to `External` by
        // any TTL.
        if self.has_unresolved_intent() {
            return Origin::UnknownMismatch;
        }
        Origin::External
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ten_thousand_writes_never_echo_and_equal_user_copies_are_external() {
        let mut t = OriginTracker::default();
        for r in 1..10001 {
            t.record(r, [1; 32], [2; 16]);
            assert_eq!(
                t.classify(Some(r), [1; 32], Some([2; 16])),
                Origin::SelfWrite
            );
            assert_eq!(
                t.classify(Some(r + 100000), [1; 32], Some([2; 16])),
                Origin::External
            );
        }
        assert_eq!(t.receipts.len(), 128);
    }
    #[test]
    fn multiple_writes_and_unknown_revision() {
        let mut t = OriginTracker::default();
        t.record(1, [1; 32], [1; 16]);
        t.record(2, [2; 32], [2; 16]);
        assert_eq!(t.classify(Some(1), [1; 32], None), Origin::SelfWrite);
        assert_eq!(t.classify(Some(2), [2; 32], None), Origin::SelfWrite);
        assert_eq!(t.classify(None, [2; 32], None), Origin::Unknown);
    }
    /// R3-03 / K4 — an intent registered before the helper runs survives a lost
    /// receipt: once resolved at the post-termination revision, the observation is
    /// recognised as ours instead of being re-broadcast.
    #[test]
    fn a_lost_receipt_does_not_turn_our_own_write_into_an_echo() {
        let mut t = OriginTracker::default();
        let token = [4_u8; 16];
        let digest = [5_u8; 32];
        assert!(t.record_intent(token, digest));
        assert!(t.has_unresolved_intent());

        // While unresolved, an unrelated observation must not be published as a
        // fresh user copy — we cannot prove it is not ours.
        assert_eq!(t.classify(Some(11), [9; 32], None), Origin::UnknownMismatch);
        assert_eq!(
            t.classify(Some(11), digest, Some(token)),
            Origin::UnknownMismatch
        );

        // The helper was killed and reaped; the resolution revision is established.
        t.resolve_intent(token, 12);
        assert!(!t.has_unresolved_intent());
        assert_eq!(
            t.classify(Some(12), digest, Some(token)),
            Origin::UnknownMismatch,
            "the resolution boundary suppresses echo without inventing success"
        );
        // A genuinely new user copy afterwards is external again.
        assert_eq!(t.classify(Some(13), [7; 32], None), Origin::External);
    }

    /// R3-03 — the intent budget is a hard limit, and a full ledger must make the
    /// caller *refuse to start* rather than publish without an intent.
    #[test]
    fn the_intent_budget_is_a_hard_limit_and_is_not_evicted_by_receipts() {
        let mut t = OriginTracker::default();
        let now = Instant::now();
        for index in 0..INTENT_LIMIT {
            let mut token = [0_u8; 16];
            token[0] = index as u8;
            token[1] = (index >> 8) as u8;
            assert!(t.record_intent_at(token, [1; 32], now));
        }
        let mut extra = [0_u8; 16];
        extra[0] = 0xff;
        extra[1] = 0xff;
        assert!(
            !t.record_intent_at(extra, [1; 32], now),
            "a full intent budget must refuse rather than silently drop one"
        );
        // Confirmed receipts are pruned by age; intents are not.
        for revision in 0..200 {
            t.record_at(revision, [2; 32], [3; 16], now);
        }
        t.prune(now + Duration::from_secs(61));
        assert_eq!(
            t.unresolved_intent_count(),
            INTENT_LIMIT,
            "the receipt TTL must not expire an unresolved operation"
        );
    }

    /// R2-03 / K3 — a known revision whose content does not match is a *mismatch*,
    /// not a fresh user copy. Re-broadcasting it would either echo our own write or
    /// mislabel the user's.
    #[test]
    fn known_revision_with_different_content_is_a_mismatch() {
        let mut t = OriginTracker::default();
        t.record(7, [1; 32], [2; 16]);
        assert_eq!(
            t.classify(Some(7), [9; 32], Some([2; 16])),
            Origin::UnknownMismatch
        );
        // A different token at the same revision is not our operation at all.
        assert_eq!(
            t.classify(Some(7), [9; 32], Some([3; 16])),
            Origin::External
        );
        // Without a token, revision + digest is still enough to claim our write.
        assert_eq!(t.classify(Some(7), [1; 32], None), Origin::SelfWrite);
    }

    #[test]
    fn unconsumed_receipts_survive_ttl_but_new_revision_is_external() {
        let mut t = OriginTracker::default();
        let now = Instant::now();
        t.record_at(1, [1; 32], [1; 16], now);
        assert_eq!(
            t.classify_at(Some(2), [1; 32], Some([1; 16]), now),
            Origin::External
        );
        assert_eq!(
            t.classify_at(
                Some(1),
                [1; 32],
                Some([1; 16]),
                now + Duration::from_secs(61)
            ),
            Origin::SelfWrite
        );
        assert_eq!(
            t.classify_at(
                Some(2),
                [1; 32],
                Some([1; 16]),
                now + Duration::from_secs(62)
            ),
            Origin::External
        );
    }
    #[test]
    fn durable_recovery_reanchors_epoch_and_never_discards_unconsumed_receipts() {
        let mut original = OriginTracker::default();
        original.record(999, [2; 32], [1; 16]);
        let bytes = serde_json::to_vec(&original.durable_state()).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("revision"));
        let mut restored =
            OriginTracker::from_durable(serde_json::from_slice(&bytes).unwrap()).unwrap();
        assert_eq!(
            restored.classify(Some(4), [2; 32], Some([1; 16])),
            Origin::SelfWrite
        );
        assert_eq!(
            restored.classify(Some(5), [2; 32], Some([1; 16])),
            Origin::External
        );
        for index in 0..128 {
            let mut token = [0; 16];
            token[..8].copy_from_slice(&(index as u64).to_le_bytes());
            restored.record(index + 10, [3; 32], token);
        }
        assert!(
            !restored.record_intent([255; 16], [3; 32]),
            "unconsumed receipt pressure refuses new effects"
        );
    }
    #[test]
    fn durable_active_intent_retains_process_guard_until_stable_resolution() {
        let mut original = OriginTracker::default();
        original.record_intent([7; 16], [8; 32]);
        original.mark_spawned([7; 16], 123);
        assert!(
            original.recovery_processes().is_empty(),
            "in-process work must not be reconciled as restarted work"
        );
        let mut restored = OriginTracker::from_durable(original.durable_state()).unwrap();
        assert_eq!(
            restored.recovery_processes(),
            vec![([7; 16], Some(123), true)]
        );
        restored.resolve_intent([7; 16], 9);
        assert_eq!(
            restored.classify(Some(9), [8; 32], Some([7; 16])),
            Origin::UnknownMismatch
        );
        assert_eq!(restored.classify(Some(10), [8; 32], None), Origin::External);
    }
}
