//! Per-peer event/byte rate limiting for inbound network events.
//!
//! Shared by the macOS and Windows network layers (T101 migration). The
//! budget table is a process-global registry keyed by peer hostname; each
//! peer gets a token bucket for inbound events and one for inbound bytes.
//! Idle entries expire after [`PEER_BUDGET_IDLE_TTL`].

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const PEER_EVENT_BURST: f64 = 30.0;
const PEER_EVENT_REFILL_PER_SECOND: f64 = 120.0 / 60.0;
const PEER_BYTE_BURST: f64 = 64.0 * 1024.0 * 1024.0;
const PEER_BYTE_REFILL_PER_SECOND: f64 = PEER_BYTE_BURST / 60.0;
const PEER_BUDGET_MAX_ENTRIES: usize = 1024;
const PEER_BUDGET_IDLE_TTL: Duration = Duration::from_secs(10 * 60);

struct TokenBucket {
    tokens: f64,
    capacity: f64,
    refill_per_second: f64,
    updated_at: Instant,
}

impl TokenBucket {
    fn new(capacity: f64, refill_per_second: f64, now: Instant) -> Self {
        Self {
            tokens: capacity,
            capacity,
            refill_per_second,
            updated_at: now,
        }
    }

    fn refill(&mut self, now: Instant) {
        let elapsed = now.duration_since(self.updated_at).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_per_second).min(self.capacity);
        self.updated_at = now;
    }
}

struct PeerBudget {
    events: TokenBucket,
    bytes: TokenBucket,
    last_seen: Instant,
}

impl PeerBudget {
    fn new(now: Instant) -> Self {
        Self {
            events: TokenBucket::new(PEER_EVENT_BURST, PEER_EVENT_REFILL_PER_SECOND, now),
            bytes: TokenBucket::new(PEER_BYTE_BURST, PEER_BYTE_REFILL_PER_SECOND, now),
            last_seen: now,
        }
    }

    fn allow(&mut self, bytes: usize, now: Instant) -> Result<(), &'static str> {
        self.events.refill(now);
        self.bytes.refill(now);
        self.last_seen = now;
        if self.events.tokens < 1.0 {
            return Err("peer event rate limit exceeded");
        }
        if self.bytes.tokens < bytes as f64 {
            return Err("peer event byte rate limit exceeded");
        }
        self.events.tokens -= 1.0;
        self.bytes.tokens -= bytes as f64;
        Ok(())
    }

    fn allow_bytes(&mut self, bytes: usize, now: Instant) -> Result<(), &'static str> {
        self.bytes.refill(now);
        self.last_seen = now;
        if self.bytes.tokens < bytes as f64 {
            return Err("peer event byte rate limit exceeded");
        }
        self.bytes.tokens -= bytes as f64;
        Ok(())
    }
}

struct PeerBudgetTable {
    peers: HashMap<String, PeerBudget>,
}

static BUDGETS: OnceLock<Mutex<PeerBudgetTable>> = OnceLock::new();

/// Checks whether a peer may send another event of `bytes` bytes.
///
/// Consumes one event token and `bytes` byte tokens on success. The error
/// strings are part of the observable contract (callers surface them in
/// logs); keep them stable.
pub fn check_peer_event_budget(peer: &str, bytes: usize) -> Result<(), String> {
    check_peer_budget(peer, bytes, true)
}

/// Charge only bytes for continuation chunks of one image event.
pub fn check_peer_chunk_bytes(peer: &str, bytes: usize) -> Result<(), String> {
    check_peer_budget(peer, bytes, false)
}

fn check_peer_budget(peer: &str, bytes: usize, event: bool) -> Result<(), String> {
    let now = Instant::now();
    let mut budgets = BUDGETS
        .get_or_init(|| {
            Mutex::new(PeerBudgetTable {
                peers: HashMap::new(),
            })
        })
        .lock()
        .map_err(|_| "peer event budget lock is poisoned".to_string())?;
    budgets
        .peers
        .retain(|_, budget| now.duration_since(budget.last_seen) <= PEER_BUDGET_IDLE_TTL);
    if !budgets.peers.contains_key(peer) && budgets.peers.len() >= PEER_BUDGET_MAX_ENTRIES {
        if let Some(oldest) = budgets
            .peers
            .iter()
            .min_by_key(|(_, budget)| budget.last_seen)
            .map(|(peer, _)| peer.clone())
        {
            budgets.peers.remove(&oldest);
        }
    }
    let budget = budgets
        .peers
        .entry(peer.to_string())
        .or_insert_with(|| PeerBudget::new(now));
    if event {
        budget.allow(bytes, now)
    } else {
        budget.allow_bytes(bytes, now)
    }
    .map_err(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peer_budget_enforces_event_burst_and_byte_capacity() {
        let now = Instant::now();
        let mut events = PeerBudget::new(now);
        for _ in 0..30 {
            assert!(events.allow(1, now).is_ok());
        }
        assert_eq!(events.allow(1, now), Err("peer event rate limit exceeded"));

        let mut bytes = PeerBudget::new(now);
        assert!(bytes.allow(64 * 1024 * 1024, now).is_ok());
        assert_eq!(
            bytes.allow(1, now),
            Err("peer event byte rate limit exceeded")
        );
    }

    #[test]
    fn image_continuation_chunks_consume_bytes_without_exhausting_event_burst() {
        let now = Instant::now();
        let mut budget = PeerBudget::new(now);
        for _ in 0..30 {
            assert!(budget.allow(0, now).is_ok());
        }
        assert_eq!(budget.allow(0, now), Err("peer event rate limit exceeded"));
        assert!(budget.allow_bytes(512 * 1024, now).is_ok());
        assert!(budget.allow_bytes(64 * 1024 * 1024, now).is_err());
    }

    #[test]
    fn peer_budget_refills_over_time() {
        let start = Instant::now();
        let mut budget = PeerBudget::new(start);
        for _ in 0..30 {
            assert!(budget.allow(1, start).is_ok());
        }
        assert_eq!(
            budget.allow(1, start),
            Err("peer event rate limit exceeded")
        );

        // 120 events/minute refill = 2 tokens/second; two seconds restore 4.
        // The threshold is `tokens < 1.0`, so exactly 1.0 still allows.
        let after_two_seconds = start + Duration::from_secs(2);
        for _ in 0..4 {
            assert!(budget.allow(1, after_two_seconds).is_ok());
        }
        assert_eq!(
            budget.allow(1, after_two_seconds),
            Err("peer event rate limit exceeded")
        );
    }

    #[test]
    fn check_peer_event_budget_accepts_events_and_rejects_oversize() {
        // Exercise the public entry point; the process-global table is
        // shared with other tests, so only capacity-independent assertions
        // are safe here. Distinct peer names keep the table deterministic.
        assert!(check_peer_event_budget("rate-limit-probe-a", 1).is_ok());
        // The byte bucket refills at ~1 MiB/s, so any real elapsed time can
        // top a nearly-full bucket back up to its 64 MiB capacity. A request
        // strictly above capacity is rejected regardless of timing.
        assert_eq!(
            check_peer_event_budget("rate-limit-probe-a", 64 * 1024 * 1024 + 1),
            Err("peer event byte rate limit exceeded".to_string())
        );
        assert!(check_peer_event_budget("rate-limit-probe-b", 1).is_ok());
    }
}
