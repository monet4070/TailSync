use std::collections::HashMap;
use std::sync::{Mutex as StdMutex, OnceLock};

use super::tailscale;
use tailsync_core::peer::health::{
    apply_peer_health as apply_peer_health_impl, update_peer_health as update_peer_health_impl,
    update_peer_health_for_failed_round as update_failed_round_impl, HealthTracker, SessionGuard,
    SessionRegistry,
};

pub use tailsync_core::peer::health::RouteKey;
pub use tailsync_core::peer::types::{ActiveRoute, ConnectionInterface};

static PEER_HEALTH: OnceLock<StdMutex<HealthTracker>> = OnceLock::new();
static AUTHENTICATED_SESSIONS: OnceLock<SessionRegistry> = OnceLock::new();

fn peer_health() -> &'static StdMutex<HealthTracker> {
    PEER_HEALTH.get_or_init(|| StdMutex::new(HealthTracker::default()))
}

pub(super) fn authenticated_sessions() -> &'static SessionRegistry {
    AUTHENTICATED_SESSIONS.get_or_init(SessionRegistry::default)
}

pub(super) fn clear_peer_health() {
    peer_health()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clear();
}

/// Register an authenticated session for a route. Registering also records a
/// probe success, because a successful handshake proves reachability.
pub(super) fn register_active_session(
    hostname: &str,
    interface: ConnectionInterface,
    address: &str,
    latency_ms: u64,
) -> SessionGuard {
    let now = tokio::time::Instant::now();
    authenticated_sessions().register(
        RouteKey::new(hostname, interface, address),
        latency_ms,
        &mut peer_health()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
        now,
    )
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

pub(super) fn update_peer_health(mode: &str, peers: &[tailscale::PeerInfo]) {
    update_peer_health_impl(
        &mut peer_health()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
        mode,
        peers,
        tokio::time::Instant::now(),
    );
}

pub(super) fn update_peer_health_for_failed_round(mode: &str) {
    update_failed_round_impl(
        &mut peer_health()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
        mode,
        tokio::time::Instant::now(),
    );
}

pub fn apply_peer_health(peers: &mut [tailscale::PeerInfo]) {
    apply_peer_health_impl(
        peers,
        &peer_health()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
        authenticated_sessions(),
        tokio::time::Instant::now(),
    );
}

pub fn active_routes_snapshot() -> HashMap<String, ActiveRoute> {
    authenticated_sessions().snapshot()
}
