use std::collections::HashMap;
use std::sync::{Mutex as StdMutex, OnceLock};

use super::tailscale;
use tailsync_core::peer::health::{
    apply_peer_health as apply_peer_health_impl, HealthTracker, SessionGuard, SessionRegistry,
};
use tailsync_core::peer::health::{
    record_probe_round as record_probe_round_impl, ProbeObservation,
};
use tailsync_core::peer::types::{ActiveRoute, ConnectionInterface};

/// Platform alias for the shared route key, kept for existing call sites.
pub(super) use tailsync_core::peer::health::RouteKey as PeerRouteKey;

static PEER_HEALTH: OnceLock<StdMutex<HealthTracker>> = OnceLock::new();
static ACTIVE_SESSIONS: OnceLock<SessionRegistry> = OnceLock::new();

fn peer_health() -> &'static StdMutex<HealthTracker> {
    PEER_HEALTH.get_or_init(|| StdMutex::new(HealthTracker::default()))
}

fn active_sessions() -> &'static SessionRegistry {
    ACTIVE_SESSIONS.get_or_init(SessionRegistry::default)
}

pub(super) fn clear_peer_health() {
    peer_health()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clear();
}

pub(super) fn record_probe_round(
    mode: &str,
    candidates: impl IntoIterator<Item = PeerRouteKey>,
    observations: impl IntoIterator<Item = (PeerRouteKey, u64)>,
) {
    record_probe_round_impl(
        &mut peer_health()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
        mode,
        candidates,
        observations
            .into_iter()
            .map(|(route, latency_ms)| ProbeObservation::new(route, latency_ms)),
        tokio::time::Instant::now(),
    );
}

pub fn record_address_test_success(address: &str, latency_ms: u64) {
    peer_health()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .record_address_test_success(address, latency_ms, tokio::time::Instant::now());
}

pub fn record_address_test_failure(address: &str) {
    peer_health()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .record_address_test_failure(address, tokio::time::Instant::now());
}

/// Register an authenticated session for a route. Registering also records a
/// probe success, because a successful handshake proves reachability.
pub(crate) fn register_active_session(
    hostname: &str,
    interface: ConnectionInterface,
    address: &str,
    latency_ms: u64,
) -> SessionGuard {
    let now = tokio::time::Instant::now();
    active_sessions().register(
        PeerRouteKey::new(hostname, interface, address),
        latency_ms,
        &mut peer_health()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
        now,
    )
}

pub fn apply_peer_health(peers: &mut [tailscale::PeerInfo]) {
    apply_peer_health_impl(
        peers,
        &peer_health()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
        active_sessions(),
        tokio::time::Instant::now(),
    );
}

pub fn active_routes_snapshot() -> HashMap<String, ActiveRoute> {
    active_sessions().snapshot()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tailsync_core::peer::types::PeerStatus;

    #[test]
    fn clear_peer_health_discards_pre_wake_probe_state() {
        let route = PeerRouteKey::new(
            "windows-wake-reset-test",
            ConnectionInterface::Lan,
            "192.0.2.240",
        );
        let now = tokio::time::Instant::now();
        {
            let mut tracker = peer_health()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            tracker.record_success(&route, 17, now);
            tracker.record_miss(&route, now);
            assert_eq!(
                tracker.status_at(&route, now, false),
                PeerStatus::Confirming
            );
            assert_eq!(tracker.latency(&route), Some(17));
        }

        clear_peer_health();

        let tracker = peer_health()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(
            tracker.status_at(&route, now, false),
            PeerStatus::Discovered
        );
        assert_eq!(tracker.latency(&route), None);
    }
}
