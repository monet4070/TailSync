//! Inbound connection rate limiting for the peer server.
//!
//! Shared by the macOS and Windows network layers (T103 migration). A
//! limiter caps both the total number of inbound connections and the number
//! of connections per source (IP address for TCP, endpoint id for Iroh);
//! [`ConnectionPermit`] releases its slots on drop.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Caps concurrent inbound connections with a per-source sub-limit.
pub struct ConnectionLimiter {
    total: Arc<Semaphore>,
    per_source: StdMutex<HashMap<String, usize>>,
    max_per_source: usize,
}

/// One acquired inbound-connection slot; releasing it on drop frees both the
/// total and the per-source slot.
pub struct ConnectionPermit {
    limiter: Arc<ConnectionLimiter>,
    source: String,
    _total: OwnedSemaphorePermit,
}

impl ConnectionLimiter {
    pub fn new(max_total: usize, max_per_source: usize) -> Arc<Self> {
        Arc::new(Self {
            total: Arc::new(Semaphore::new(max_total)),
            per_source: StdMutex::new(HashMap::new()),
            max_per_source,
        })
    }

    pub fn try_acquire(self: &Arc<Self>, ip: IpAddr) -> Option<ConnectionPermit> {
        self.try_acquire_source(ip.to_string())
    }

    pub fn try_acquire_source(self: &Arc<Self>, source: String) -> Option<ConnectionPermit> {
        let total = self.total.clone().try_acquire_owned().ok()?;
        let mut counts = self
            .per_source
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let count = counts.entry(source.clone()).or_default();
        if *count >= self.max_per_source {
            return None;
        }
        *count += 1;
        drop(counts);
        Some(ConnectionPermit {
            limiter: self.clone(),
            source,
            _total: total,
        })
    }
}

impl Drop for ConnectionPermit {
    fn drop(&mut self) {
        let mut counts = self
            .limiter
            .per_source
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(count) = counts.get_mut(&self.source) {
            *count -= 1;
            if *count == 0 {
                counts.remove(&self.source);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limiter_enforces_total_and_per_source_caps() {
        let limiter = ConnectionLimiter::new(2, 1);
        // Permits must be held; dropping them (e.g. inside an assert
        // temporary) immediately returns the slots to the limiter.
        let first = limiter.try_acquire_source("a".to_string());
        assert!(first.is_some());
        // Per-source cap reached for "a".
        assert!(limiter.try_acquire_source("a".to_string()).is_none());
        // Total cap reached for a second distinct source.
        let second = limiter.try_acquire_source("b".to_string());
        assert!(second.is_some());
        assert!(limiter.try_acquire_source("c".to_string()).is_none());
        drop(first);
        drop(second);
        // Both slots are free again.
        assert!(limiter.try_acquire_source("c".to_string()).is_some());
    }

    #[test]
    fn dropping_permits_returns_capacity() {
        let limiter = ConnectionLimiter::new(2, 1);
        let first = limiter.try_acquire_source("a".to_string());
        assert!(first.is_some());
        assert!(limiter.try_acquire_source("a".to_string()).is_none());
        drop(first);
        // Both the per-source slot and the total slot are free again.
        let second = limiter.try_acquire_source("a".to_string());
        assert!(second.is_some());
        // Total cap still applies across distinct sources.
        let third = limiter.try_acquire_source("b".to_string());
        assert!(third.is_some());
        assert!(limiter.try_acquire_source("c".to_string()).is_none());
        drop(second);
        drop(third);
        assert!(limiter.try_acquire_source("c".to_string()).is_some());
    }

    #[test]
    fn ip_address_variant_uses_the_same_source_accounting() {
        let limiter = ConnectionLimiter::new(4, 1);
        let ip: IpAddr = "192.168.1.5".parse().unwrap();
        let first = limiter.try_acquire(ip);
        assert!(first.is_some());
        assert!(limiter.try_acquire(ip).is_none());
        let other: IpAddr = "192.168.1.6".parse().unwrap();
        let second = limiter.try_acquire(other);
        assert!(second.is_some());
        drop(first);
        drop(second);
    }
}
