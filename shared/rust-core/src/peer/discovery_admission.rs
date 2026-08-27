//! Admission control for the UDP discovery responder (2026-08 audit
//! follow-up, docs/SECURITY-AUDIT-2026-08.md).
//!
//! The responder answers any matching probe today, which lets a same-
//! segment device interrogate hostname/port/Iroh-endpoint metadata at an
//! unbounded rate and lets public sources elicit replies at all. This
//! module is the shared gate the platform responders must consult:
//!
//! - Source filtering: only LAN (RFC1918/link-local/loopback) and Tailscale
//!   (CGNAT/ULA) addresses are answered, reusing `source_matches_mode`
//!   with `auto` semantics. The disclosure to same-segment devices is
//!   inherent to the discovery protocol; a discoverability toggle would be
//!   a separate feature.
//! - Per-source budget: short burst allowance (wake-from-sleep storms probe
//!   in bursts) with a low sustained rate.
//! - Global budget: bounds total reply traffic when many distinct sources
//!   probe at once.
//! - The source table is capped and lazily expired so spoofed source
//!   addresses cannot grow it without bound.
//!
//! The global bucket is checked before a new source is inserted and tokens
//! are only consumed on a final allow, so a single throttled source cannot
//! burn the global budget for everyone else.
//!
//! Denial reasons are structured but deliberately carry no source address:
//! the caller must not log per-packet source IPs (log flooding plus
//! metadata leakage).

use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{Duration, Instant};

use super::directory::source_matches_mode;

/// Per-source and global reply budgets plus table capacity.
///
/// Defaults: per-source burst 8 with 2 tokens/s refill (a normal peer
/// discovers once and only re-probes after network changes or wake);
/// global burst 128 with 32 tokens/s refill (a few KB/s of sustained reply
/// traffic, absorbing the wake storm of a hundred-device LAN); the table
/// tracks at most 1024 sources with a 60 s idle expiry.
#[derive(Debug, Clone)]
pub struct DiscoveryPolicy {
    pub source_burst: f64,
    pub source_refill_per_sec: f64,
    pub global_burst: f64,
    pub global_refill_per_sec: f64,
    pub max_tracked_sources: usize,
    pub idle_ttl: Duration,
    pub cleanup_interval: Duration,
}

impl Default for DiscoveryPolicy {
    fn default() -> Self {
        Self {
            source_burst: 8.0,
            source_refill_per_sec: 2.0,
            global_burst: 128.0,
            global_refill_per_sec: 32.0,
            max_tracked_sources: 1024,
            idle_ttl: Duration::from_secs(60),
            cleanup_interval: Duration::from_secs(10),
        }
    }
}

/// Why a discovery probe was not answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenyReason {
    /// The source is outside LAN/Tailscale ranges.
    NotAllowedSource,
    /// The source exhausted its per-source budget.
    SourceBudgetExhausted,
    /// The global reply budget is exhausted.
    GlobalBudgetExhausted,
    /// The source table is at capacity after expiry cleanup.
    TableFull,
}

/// Whether a probe may be answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionDecision {
    Allow,
    Deny(DenyReason),
}

impl AdmissionDecision {
    pub fn is_allowed(self) -> bool {
        matches!(self, AdmissionDecision::Allow)
    }
}

#[derive(Debug, Clone, Copy)]
struct Bucket {
    tokens: f64,
    last_refill: Instant,
}

impl Bucket {
    fn full(burst: f64, now: Instant) -> Self {
        Self {
            tokens: burst,
            last_refill: now,
        }
    }

    fn refill(&mut self, burst: f64, rate: f64, now: Instant) {
        let elapsed = now
            .saturating_duration_since(self.last_refill)
            .as_secs_f64();
        if elapsed > 0.0 {
            self.tokens = (self.tokens + elapsed * rate).min(burst);
            self.last_refill = now;
        }
    }

    fn has_token(&self) -> bool {
        self.tokens >= 1.0
    }

    fn consume(&mut self) {
        self.tokens = (self.tokens - 1.0).max(0.0);
    }
}

#[derive(Debug, Clone, Copy)]
struct SourceState {
    bucket: Bucket,
    last_seen: Instant,
}

/// Shared admission gate for the UDP discovery responder.
///
/// Callers pass a monotonically non-decreasing `now` (the platform passes
/// `Instant::now()`; tests inject synthetic time) and never log the source
/// address on denial.
#[derive(Debug)]
pub struct DiscoveryAdmission {
    policy: DiscoveryPolicy,
    sources: HashMap<IpAddr, SourceState>,
    global: Bucket,
    last_cleanup: Instant,
}

impl DiscoveryAdmission {
    pub fn new(now: Instant) -> Self {
        Self::with_policy(DiscoveryPolicy::default(), now)
    }

    pub fn with_policy(policy: DiscoveryPolicy, now: Instant) -> Self {
        let global = Bucket::full(policy.global_burst, now);
        Self {
            policy,
            sources: HashMap::new(),
            global,
            last_cleanup: now,
        }
    }

    /// Number of tracked sources (bounded by the policy cap).
    pub fn tracked_sources(&self) -> usize {
        self.sources.len()
    }

    /// Decide whether a probe from `source` at time `now` may be answered.
    pub fn should_reply(&mut self, source: IpAddr, now: Instant) -> AdmissionDecision {
        if !source_matches_mode(source, "auto") {
            return AdmissionDecision::Deny(DenyReason::NotAllowedSource);
        }
        self.global.refill(
            self.policy.global_burst,
            self.policy.global_refill_per_sec,
            now,
        );
        if !self.global.has_token() {
            return AdmissionDecision::Deny(DenyReason::GlobalBudgetExhausted);
        }

        if let Some(state) = self.sources.get_mut(&source) {
            state.bucket.refill(
                self.policy.source_burst,
                self.policy.source_refill_per_sec,
                now,
            );
            if !state.bucket.has_token() {
                return AdmissionDecision::Deny(DenyReason::SourceBudgetExhausted);
            }
            state.bucket.consume();
            state.last_seen = now;
        } else {
            // A new source needs a table slot: expire idle entries first,
            // then deny rather than evicting live entries when still full.
            if self.sources.len() >= self.policy.max_tracked_sources {
                self.cleanup(now);
                if self.sources.len() >= self.policy.max_tracked_sources {
                    return AdmissionDecision::Deny(DenyReason::TableFull);
                }
            }
            self.sources.insert(
                source,
                SourceState {
                    bucket: Bucket {
                        tokens: self.policy.source_burst - 1.0,
                        last_refill: now,
                    },
                    last_seen: now,
                },
            );
        }
        self.global.consume();
        AdmissionDecision::Allow
    }

    /// Expire sources idle past the TTL. The caller invokes this only under
    /// capacity pressure, and the interval guard amortises the O(n) scan so
    /// a full-table flood cannot force one scan per packet.
    fn cleanup(&mut self, now: Instant) {
        if now.saturating_duration_since(self.last_cleanup) < self.policy.cleanup_interval {
            return;
        }
        self.sources.retain(|_, state| {
            now.saturating_duration_since(state.last_seen) < self.policy.idle_ttl
        });
        self.last_cleanup = now;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, SocketAddrV4};

    fn lan_source(index: u16) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(192, 168, 1, (index % 254 + 1) as u8))
    }

    #[test]
    fn public_and_unspecified_sources_are_never_answered() {
        let t0 = Instant::now();
        let mut admission = DiscoveryAdmission::new(t0);
        for source in [
            IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
            IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)),
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            IpAddr::V6(std::net::Ipv6Addr::UNSPECIFIED),
        ] {
            assert_eq!(
                admission.should_reply(source, t0),
                AdmissionDecision::Deny(DenyReason::NotAllowedSource),
                "{source} must never be answered"
            );
        }
        // Denied sources never allocate table state or budget.
        assert_eq!(admission.tracked_sources(), 0);
    }

    #[test]
    fn lan_loopback_and_tailscale_sources_are_admitted() {
        let t0 = Instant::now();
        let mut admission = DiscoveryAdmission::new(t0);
        for source in [
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 5)),
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)),
            IpAddr::V4(Ipv4Addr::new(172, 16, 0, 1)),
            IpAddr::V4(Ipv4Addr::new(169, 254, 3, 4)),
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V4(Ipv4Addr::new(100, 100, 1, 1)), // Tailscale CGNAT
        ] {
            assert!(
                admission.should_reply(source, t0).is_allowed(),
                "{source} should be admitted"
            );
        }
    }

    #[test]
    fn single_source_exhausts_its_burst_and_refills_over_time() {
        let t0 = Instant::now();
        let mut admission = DiscoveryAdmission::new(t0);
        let source = lan_source(7);
        for _ in 0..8 {
            assert!(admission.should_reply(source, t0).is_allowed());
        }
        assert_eq!(
            admission.should_reply(source, t0),
            AdmissionDecision::Deny(DenyReason::SourceBudgetExhausted)
        );
        // 2 tokens/s: half a second later exactly one token has refilled.
        let half_second = t0 + Duration::from_millis(500);
        assert!(admission.should_reply(source, half_second).is_allowed());
        assert_eq!(
            admission.should_reply(source, half_second),
            AdmissionDecision::Deny(DenyReason::SourceBudgetExhausted)
        );
        // A full burst recovers once burst/refill seconds have elapsed
        // since the last consumption (which happened at the half-second
        // mark, so five seconds covers it).
        let recovered = t0 + Duration::from_secs(5);
        for _ in 0..8 {
            assert!(admission.should_reply(source, recovered).is_allowed());
        }
    }

    #[test]
    fn sources_are_billed_independently_under_the_global_cap() {
        let t0 = Instant::now();
        let mut admission = DiscoveryAdmission::new(t0);
        // 16 sources x 8 burst requests = 128 = the global burst budget.
        for index in 0..16u16 {
            let source = lan_source(index);
            for _ in 0..8 {
                assert!(
                    admission.should_reply(source, t0).is_allowed(),
                    "source {source} should stay within its own budget"
                );
            }
        }
        // The 17th source is healthy per-source-wise but the global budget
        // is exhausted.
        assert_eq!(
            admission.should_reply(lan_source(99), t0),
            AdmissionDecision::Deny(DenyReason::GlobalBudgetExhausted)
        );
        // Global refill admits it again after a while.
        let later = t0 + Duration::from_secs(1);
        assert!(admission.should_reply(lan_source(99), later).is_allowed());
    }

    #[test]
    fn throttled_source_does_not_burn_the_global_budget() {
        let t0 = Instant::now();
        let mut admission = DiscoveryAdmission::new(t0);
        let noisy = lan_source(1);
        for _ in 0..8 {
            assert!(admission.should_reply(noisy, t0).is_allowed());
        }
        // The noisy source keeps hammering and is denied per-source...
        for _ in 0..5 {
            assert_eq!(
                admission.should_reply(noisy, t0),
                AdmissionDecision::Deny(DenyReason::SourceBudgetExhausted)
            );
        }
        // ...without consuming global tokens: a fresh source still passes.
        assert!(admission.should_reply(lan_source(2), t0).is_allowed());
    }

    #[test]
    fn wake_storm_bursts_are_not_throttled() {
        // Five devices waking at once, each probing five times inside a
        // hundred milliseconds: well inside both per-source bursts and the
        // global burst.
        let t0 = Instant::now();
        let mut admission = DiscoveryAdmission::new(t0);
        for step in 0..5 {
            let now = t0 + Duration::from_millis(20 * step);
            for index in 0..5u16 {
                assert!(admission.should_reply(lan_source(index), now).is_allowed());
            }
        }
    }

    #[test]
    fn spoofed_source_flood_keeps_the_table_bounded() {
        let t0 = Instant::now();
        let mut admission = DiscoveryAdmission::new(t0);
        let mut allowed = 0usize;
        // 10k distinct LAN addresses, 10 ms apart: the global refill admits
        // roughly 32/s, idle expiry keeps churning, and the table can never
        // exceed its cap.
        for i in 0..10_000u32 {
            let source = IpAddr::V4(Ipv4Addr::new(
                10,
                (i >> 16) as u8,
                (i >> 8) as u8,
                (i & 0xff) as u8,
            ));
            let now = t0 + Duration::from_millis(u64::from(i) * 10);
            if admission.should_reply(source, now).is_allowed() {
                allowed += 1;
            }
            assert!(
                admission.tracked_sources() <= 1024,
                "table must stay bounded at step {i}"
            );
        }
        // The flood got some replies through (bounded by global refill) but
        // the responder stayed functional and bounded.
        assert!(allowed > 0 && allowed < 10_000);
    }

    #[test]
    fn table_full_denies_new_sources_without_evicting_live_ones() {
        let policy = DiscoveryPolicy {
            source_burst: 2.0,
            source_refill_per_sec: 2.0,
            global_burst: 1000.0,
            global_refill_per_sec: 1000.0,
            max_tracked_sources: 4,
            idle_ttl: Duration::from_secs(600),
            cleanup_interval: Duration::from_secs(600),
        };
        let t0 = Instant::now();
        let mut admission = DiscoveryAdmission::with_policy(policy, t0);
        for index in 0..4u16 {
            assert!(admission.should_reply(lan_source(index), t0).is_allowed());
        }
        assert_eq!(
            admission.should_reply(lan_source(50), t0),
            AdmissionDecision::Deny(DenyReason::TableFull)
        );
        // Existing sources are unaffected.
        assert!(admission.should_reply(lan_source(0), t0).is_allowed());
        assert_eq!(admission.tracked_sources(), 4);
    }

    #[test]
    fn full_table_does_not_rescan_before_cleanup_interval() {
        let policy = DiscoveryPolicy {
            source_burst: 2.0,
            source_refill_per_sec: 2.0,
            global_burst: 1000.0,
            global_refill_per_sec: 1000.0,
            max_tracked_sources: 1,
            idle_ttl: Duration::from_millis(1),
            cleanup_interval: Duration::from_secs(60),
        };
        let t0 = Instant::now();
        let mut admission = DiscoveryAdmission::with_policy(policy, t0);
        assert!(admission.should_reply(lan_source(1), t0).is_allowed());

        // Even though the entry is now technically idle, a table-full flood
        // must not force an O(table-size) retain scan on every packet. It is
        // reclaimed only once the amortised cleanup interval is due.
        assert_eq!(
            admission.should_reply(lan_source(2), t0 + Duration::from_millis(2)),
            AdmissionDecision::Deny(DenyReason::TableFull)
        );
        assert_eq!(admission.tracked_sources(), 1);
    }

    #[test]
    fn idle_sources_expire_and_free_table_slots() {
        let policy = DiscoveryPolicy {
            source_burst: 2.0,
            source_refill_per_sec: 2.0,
            global_burst: 1000.0,
            global_refill_per_sec: 1000.0,
            max_tracked_sources: 2,
            idle_ttl: Duration::from_secs(60),
            cleanup_interval: Duration::from_secs(10),
        };
        let t0 = Instant::now();
        let mut admission = DiscoveryAdmission::with_policy(policy, t0);
        assert!(admission.should_reply(lan_source(1), t0).is_allowed());
        assert!(admission.should_reply(lan_source(2), t0).is_allowed());
        assert_eq!(
            admission.should_reply(lan_source(3), t0),
            AdmissionDecision::Deny(DenyReason::TableFull)
        );
        // Well past the idle TTL the stale entries expire on the next
        // capacity pressure and the new source gets in.
        let later = t0 + Duration::from_secs(120);
        assert!(admission.should_reply(lan_source(3), later).is_allowed());
    }

    #[test]
    fn socket_source_ip_extraction_matches_responder_usage() {
        // The responder receives a SocketAddr; document the .ip() handoff.
        let socket = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 42), 52341);
        let t0 = Instant::now();
        let mut admission = DiscoveryAdmission::new(t0);
        assert!(admission
            .should_reply(IpAddr::from(*socket.ip()), t0)
            .is_allowed());
    }
}
