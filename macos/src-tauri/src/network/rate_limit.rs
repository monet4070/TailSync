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
}

struct PeerBudgetTable {
    peers: HashMap<String, PeerBudget>,
}

pub(super) fn check_peer_event_budget(peer: &str, bytes: usize) -> Result<(), String> {
    static BUDGETS: OnceLock<Mutex<PeerBudgetTable>> = OnceLock::new();
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
    budgets
        .peers
        .entry(peer.to_string())
        .or_insert_with(|| PeerBudget::new(now))
        .allow(bytes, now)
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
}
