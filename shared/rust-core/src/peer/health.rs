//! Peer health derivation: the pure state machine behind online/offline
//! status and authenticated-session accounting.
//!
//! Platform health loops (discovery probes, TCP heartbeats) feed results in
//! through [`HealthTracker::record_success`] / [`HealthTracker::record_miss`]
//! and register authenticated connections in [`SessionRegistry`]; the
//! derivation functions here turn that state into the `PeerStatus` values the
//! UI and delivery path consume. No I/O happens in this module, so the
//! state-machine rules are unit-testable without sockets.
//!
//! Unified semantics (superset of the former macOS/Windows copies):
//! - A route that was never probed at all (no tracker entry) stays
//!   `Discovered`. Once a probe round records a miss, the route is
//!   "probed but never answered": two consecutive misses turn it `Offline`
//!   (per the README rule "连续两轮失败 = offline"), one miss keeps it
//!   `Discovered`.
//! - One miss after a success turns the route `Confirming` (still online
//!   within the TTL), two consecutive misses turn it `Offline`.
//! - Registering an authenticated session counts as a probe success and
//!   forces `Connected` until the last session for that route closes.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;
use tokio::time::Instant;

use crate::peer::types::{
    ActiveRoute, ConnectionInterface, PeerHealthSnapshot, PeerInfo, PeerStatus,
};

/// How recently a probe must have succeeded for a route to count as online.
const ONLINE_TTL: Duration = Duration::from_secs(12);

/// Identity of one route to a peer, keying both health and sessions.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RouteKey {
    pub hostname: String,
    pub interface: ConnectionInterface,
    pub address: String,
}

impl RouteKey {
    pub fn new(hostname: &str, interface: ConnectionInterface, address: &str) -> Self {
        Self {
            hostname: hostname.to_string(),
            interface,
            address: address.to_string(),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct PeerHealth {
    last_seen: Option<Instant>,
    consecutive_misses: u8,
    latency_ms: Option<u64>,
}

/// Per-route health state. `now` is passed in by callers so tests can drive
/// the TTL boundary deterministically.
#[derive(Debug, Default)]
pub struct HealthTracker {
    routes: HashMap<RouteKey, PeerHealth>,
}

impl HealthTracker {
    pub fn ensure_candidate(&mut self, route: RouteKey) {
        self.routes.entry(route).or_default();
    }

    pub fn record_success(&mut self, route: &RouteKey, latency_ms: u64, now: Instant) {
        let health = self.routes.entry(route.clone()).or_default();
        health.last_seen = Some(now);
        health.consecutive_misses = 0;
        health.latency_ms = Some(latency_ms);
    }

    /// Every probe round a route is expected but not observed counts as a
    /// miss, including routes that never answered: two consecutive misses
    /// move a route offline per the shared status model.
    pub fn record_miss(&mut self, route: &RouteKey, now: Instant) {
        let health = self.routes.entry(route.clone()).or_default();
        health.consecutive_misses = health.consecutive_misses.saturating_add(1);
        let _ = now;
    }

    /// Apply one probe round for one interface: routes observed in this round
    /// count as successes; routes of the same interface that were seen in
    /// earlier rounds but not observed now accumulate a miss.
    pub fn apply_round(
        &mut self,
        now: Instant,
        interface: ConnectionInterface,
        observed: HashMap<RouteKey, u64>,
    ) {
        for route in observed.keys() {
            self.routes.entry(route.clone()).or_default();
        }
        for (route, health) in &mut self.routes {
            if route.interface != interface {
                continue;
            }
            if let Some(latency_ms) = observed.get(route) {
                health.last_seen = Some(now);
                health.consecutive_misses = 0;
                health.latency_ms = Some(*latency_ms);
            } else {
                health.consecutive_misses = health.consecutive_misses.saturating_add(1);
            }
        }
    }

    /// Derive the status for one route. An authenticated session forces
    /// `Connected` regardless of probe state. A route that was probed but
    /// never answered goes offline after two consecutive misses; a route
    /// that was never probed at all stays `Discovered`.
    pub fn status_at(&self, route: &RouteKey, now: Instant, connected: bool) -> PeerStatus {
        if connected {
            return PeerStatus::Connected;
        }
        let Some(health) = self.routes.get(route) else {
            return PeerStatus::Discovered;
        };
        let Some(last_seen) = health.last_seen else {
            return if health.consecutive_misses >= 2 {
                PeerStatus::Offline
            } else {
                PeerStatus::Discovered
            };
        };
        let recently_seen = now.saturating_duration_since(last_seen) < ONLINE_TTL;
        if recently_seen && health.consecutive_misses == 0 {
            PeerStatus::Online
        } else if recently_seen && health.consecutive_misses < 2 {
            PeerStatus::Confirming
        } else {
            PeerStatus::Offline
        }
    }

    pub fn latency(&self, route: &RouteKey) -> Option<u64> {
        self.routes.get(route).and_then(|health| health.latency_ms)
    }

    pub fn clear(&mut self) {
        self.routes.clear();
    }

    pub fn record_address_test_success(&mut self, address: &str, latency_ms: u64, now: Instant) {
        let routes = self
            .routes
            .iter()
            .filter(|(key, _)| key.address == address)
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        for route in routes {
            self.record_success(&route, latency_ms, now);
        }
    }

    pub fn record_address_test_failure(&mut self, address: &str, now: Instant) {
        let routes = self
            .routes
            .iter()
            .filter(|(key, _)| key.address == address)
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        for route in routes {
            self.record_miss(&route, now);
        }
    }
}

/// Order used when folding per-route statuses into a peer-level status.
pub fn status_rank(status: PeerStatus) -> u8 {
    match status {
        PeerStatus::Connected => 4,
        PeerStatus::Online => 3,
        PeerStatus::Confirming => 2,
        PeerStatus::Discovered => 1,
        PeerStatus::Offline => 0,
    }
}

#[derive(Debug, Clone)]
struct SessionEntry {
    route: ActiveRoute,
    count: usize,
}

/// Reference-counted authenticated sessions per route. A session forces the
/// route `Connected`; registering one also records a probe success, because
/// a successful handshake proves reachability.
#[derive(Debug, Clone, Default)]
pub struct SessionRegistry {
    entries: Arc<StdMutex<HashMap<RouteKey, SessionEntry>>>,
}

impl SessionRegistry {
    pub fn register(
        &self,
        key: RouteKey,
        latency_ms: u64,
        tracker: &mut HealthTracker,
        now: Instant,
    ) -> SessionGuard {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let entry = entries.entry(key.clone()).or_insert_with(|| SessionEntry {
            route: ActiveRoute {
                interface: key.interface,
                address: key.address.clone(),
                latency: latency_ms,
            },
            count: 0,
        });
        entry.count += 1;
        entry.route.latency = latency_ms;
        drop(entries);
        tracker.record_success(&key, latency_ms, now);
        SessionGuard {
            registry: self.clone(),
            key,
        }
    }

    fn unregister(&self, key: &RouteKey) {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let remove = if let Some(entry) = entries.get_mut(key) {
            entry.count = entry.count.saturating_sub(1);
            entry.count == 0
        } else {
            false
        };
        if remove {
            entries.remove(key);
        }
    }

    pub fn is_connected(&self, key: &RouteKey) -> bool {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(key)
            .is_some_and(|entry| entry.count > 0)
    }

    pub fn active_route(&self, hostname: &str) -> Option<ActiveRoute> {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .filter(|(key, entry)| key.hostname == hostname && entry.count > 0)
            .map(|(_, entry)| entry.route.clone())
            .min_by_key(|route| (route.interface.priority(), route.latency))
    }

    pub fn snapshot(&self) -> HashMap<String, ActiveRoute> {
        let hostnames = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .keys()
            .map(|key| key.hostname.clone())
            .collect::<HashSet<_>>();
        hostnames
            .into_iter()
            .filter_map(|hostname| self.active_route(&hostname).map(|route| (hostname, route)))
            .collect()
    }
}

/// RAII guard: unregisters the session when dropped.
pub struct SessionGuard {
    registry: SessionRegistry,
    key: RouteKey,
}

impl Drop for SessionGuard {
    fn drop(&mut self) {
        self.registry.unregister(&self.key);
    }
}

fn interfaces_for_mode(mode: &str) -> &'static [ConnectionInterface] {
    match mode {
        "lan" | "lan_only" => &[ConnectionInterface::Lan],
        "tailscale" | "tailscale_only" => &[ConnectionInterface::Tailscale],
        _ => &[ConnectionInterface::Lan, ConnectionInterface::Tailscale],
    }
}

/// Feed one discovery round into the tracker: ensure every candidate route is
/// tracked, then apply observed (online) routes per interface.
pub fn update_peer_health(
    tracker: &mut HealthTracker,
    mode: &str,
    peers: &[PeerInfo],
    now: Instant,
) {
    for peer in peers {
        for candidate in &peer.candidates {
            tracker.ensure_candidate(RouteKey {
                hostname: peer.hostname.clone(),
                interface: candidate.interface,
                address: candidate.address.clone(),
            });
        }
    }
    for interface in interfaces_for_mode(mode) {
        let observed = peers
            .iter()
            .flat_map(|peer| {
                peer.candidates
                    .iter()
                    .filter(move |candidate| candidate.interface == *interface && candidate.online)
                    .map(move |candidate| {
                        (
                            RouteKey {
                                hostname: peer.hostname.clone(),
                                interface: candidate.interface,
                                address: candidate.address.clone(),
                            },
                            candidate.latency.unwrap_or_default(),
                        )
                    })
            })
            .collect();
        tracker.apply_round(now, *interface, observed);
    }
}

/// Feed a failed discovery round: every interface misses.
pub fn update_peer_health_for_failed_round(tracker: &mut HealthTracker, mode: &str, now: Instant) {
    for interface in interfaces_for_mode(mode) {
        tracker.apply_round(now, *interface, HashMap::new());
    }
}

/// Write derived statuses back onto a peer snapshot: per-candidate status,
/// online flag, and latency, plus the peer-level status and the current
/// authenticated route.
pub fn apply_peer_health(
    peers: &mut [PeerInfo],
    tracker: &HealthTracker,
    sessions: &SessionRegistry,
    now: Instant,
) {
    for peer in peers {
        // Projection always starts from a clean slate: if the peer's last
        // authenticated session closed, the stale current route must not
        // survive into the new snapshot.
        peer.current_interface = None;
        peer.current_address = None;
        let mut peer_status = PeerStatus::Offline;
        for candidate in &mut peer.candidates {
            let route = RouteKey {
                hostname: peer.hostname.clone(),
                interface: candidate.interface,
                address: candidate.address.clone(),
            };
            let connected = sessions.is_connected(&route);
            candidate.status = tracker.status_at(&route, now, connected);
            candidate.online = matches!(
                candidate.status,
                PeerStatus::Connected | PeerStatus::Online | PeerStatus::Confirming
            );
            candidate.latency = tracker.latency(&route).or(candidate.latency);
            if status_rank(candidate.status) > status_rank(peer_status) {
                peer_status = candidate.status;
            }
        }
        if let Some(route) = sessions.active_route(&peer.hostname) {
            peer.current_interface = Some(route.interface);
            peer.current_address = Some(route.address);
            peer_status = PeerStatus::Connected;
        }
        peer.status = peer_status;
        peer.online = matches!(
            peer_status,
            PeerStatus::Connected | PeerStatus::Online | PeerStatus::Confirming
        );
    }
}

/// Derive the health snapshot for one route, as consumed by the settings UI
/// route list. An authenticated session forces `Connected` and its latency
/// takes precedence.
pub fn health_snapshot(
    tracker: &HealthTracker,
    sessions: &SessionRegistry,
    route: &RouteKey,
    now: Instant,
) -> PeerHealthSnapshot {
    let connected = sessions.is_connected(route);
    if connected {
        let latency_ms = sessions
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(route)
            .map(|entry| entry.route.latency)
            .or_else(|| tracker.latency(route));
        return PeerHealthSnapshot {
            status: PeerStatus::Connected,
            online: true,
            connected: true,
            latency_ms,
        };
    }
    let status = tracker.status_at(route, now, false);
    PeerHealthSnapshot {
        online: matches!(
            status,
            PeerStatus::Connected | PeerStatus::Online | PeerStatus::Confirming
        ),
        connected: false,
        status,
        latency_ms: tracker.latency(route),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::peer::types::PeerCandidate;

    fn candidate_route(hostname: &str, interface: ConnectionInterface, address: &str) -> RouteKey {
        RouteKey::new(hostname, interface, address)
    }

    #[test]
    fn candidate_without_heartbeat_is_discovered_not_online() {
        let tracker = HealthTracker::default();
        let sessions = SessionRegistry::default();
        let route = candidate_route(
            "never-seen-health-test",
            ConnectionInterface::Lan,
            "192.168.250.20",
        );
        let snapshot = health_snapshot(&tracker, &sessions, &route, Instant::now());
        assert_eq!(snapshot.status, PeerStatus::Discovered);
        assert!(!snapshot.online);
        assert!(!snapshot.connected);
    }

    #[test]
    fn heartbeat_misses_confirm_then_offline_and_recover_immediately() {
        let mut tracker = HealthTracker::default();
        let sessions = SessionRegistry::default();
        let route = candidate_route(
            "health-transition-test",
            ConnectionInterface::Lan,
            "192.168.250.21",
        );
        let now = Instant::now();
        tracker.record_success(&route, 11, now);
        let snapshot = health_snapshot(&tracker, &sessions, &route, now);
        assert_eq!(snapshot.status, PeerStatus::Online);
        assert_eq!(snapshot.latency_ms, Some(11));

        tracker.record_miss(&route, now);
        let snapshot = health_snapshot(&tracker, &sessions, &route, now);
        assert_eq!(snapshot.status, PeerStatus::Confirming);
        assert!(snapshot.online);

        tracker.record_miss(&route, now);
        let snapshot = health_snapshot(&tracker, &sessions, &route, now);
        assert_eq!(snapshot.status, PeerStatus::Offline);
        assert!(!snapshot.online);

        tracker.record_success(&route, 7, now);
        let snapshot = health_snapshot(&tracker, &sessions, &route, now);
        assert_eq!(snapshot.status, PeerStatus::Online);
        assert_eq!(snapshot.latency_ms, Some(7));
    }

    #[test]
    fn lan_and_tailscale_health_are_independent() {
        let mut tracker = HealthTracker::default();
        let sessions = SessionRegistry::default();
        let hostname = "route-independence-test";
        let lan = candidate_route(hostname, ConnectionInterface::Lan, "192.168.250.22");
        let tailscale = candidate_route(hostname, ConnectionInterface::Tailscale, "100.100.250.22");
        let now = Instant::now();
        tracker.record_success(&lan, 4, now);
        tracker.record_miss(&tailscale, now);
        tracker.record_miss(&tailscale, now);

        assert_eq!(
            health_snapshot(&tracker, &sessions, &lan, now).status,
            PeerStatus::Online
        );
        assert_eq!(
            health_snapshot(&tracker, &sessions, &tailscale, now).status,
            PeerStatus::Offline
        );
    }

    #[test]
    fn authenticated_sessions_force_connected_until_last_session_closes() {
        let mut tracker = HealthTracker::default();
        let sessions = SessionRegistry::default();
        let hostname = "session-reference-count-test";
        let address = "100.100.250.23";
        let route = candidate_route(hostname, ConnectionInterface::Tailscale, address);
        let now = Instant::now();
        let first = sessions.register(route.clone(), 9, &mut tracker, now);
        let second = sessions.register(route.clone(), 8, &mut tracker, now);
        let snapshot = health_snapshot(&tracker, &sessions, &route, now);
        assert_eq!(snapshot.status, PeerStatus::Connected);
        assert!(snapshot.connected);
        assert_eq!(snapshot.latency_ms, Some(8));

        drop(first);
        assert_eq!(
            health_snapshot(&tracker, &sessions, &route, now).status,
            PeerStatus::Connected
        );

        drop(second);
        let snapshot = health_snapshot(&tracker, &sessions, &route, now);
        assert_eq!(snapshot.status, PeerStatus::Online);
    }

    #[test]
    fn probed_but_never_seen_routes_go_offline_after_two_misses() {
        let mut tracker = HealthTracker::default();
        let route = candidate_route("never-answered", ConnectionInterface::Lan, "192.168.250.24");
        let now = Instant::now();
        tracker.record_miss(&route, now);
        assert_eq!(
            tracker.status_at(&route, now, false),
            PeerStatus::Discovered
        );
        tracker.record_miss(&route, now);
        assert_eq!(tracker.status_at(&route, now, false), PeerStatus::Offline);
    }

    #[test]
    fn heartbeat_rounds_confirm_then_offline_and_recover() {
        let route = candidate_route("windows", ConnectionInterface::Lan, "192.168.1.20");
        let started = Instant::now();
        let mut tracker = HealthTracker::default();

        tracker.ensure_candidate(route.clone());
        assert_eq!(
            tracker.status_at(&route, started, false),
            PeerStatus::Discovered
        );

        tracker.apply_round(
            started,
            ConnectionInterface::Lan,
            [(route.clone(), 8)].into_iter().collect(),
        );
        assert_eq!(
            tracker.status_at(&route, started, false),
            PeerStatus::Online
        );
        assert_eq!(tracker.latency(&route), Some(8));

        tracker.apply_round(
            started + Duration::from_secs(5),
            ConnectionInterface::Lan,
            HashMap::new(),
        );
        assert_eq!(
            tracker.status_at(&route, started + Duration::from_secs(5), false),
            PeerStatus::Confirming
        );

        tracker.apply_round(
            started + Duration::from_secs(10),
            ConnectionInterface::Lan,
            HashMap::new(),
        );
        assert_eq!(
            tracker.status_at(&route, started + Duration::from_secs(10), false),
            PeerStatus::Offline
        );

        tracker.apply_round(
            started + Duration::from_secs(15),
            ConnectionInterface::Lan,
            [(route.clone(), 5)].into_iter().collect(),
        );
        assert_eq!(
            tracker.status_at(&route, started + Duration::from_secs(15), false),
            PeerStatus::Online
        );
        assert_eq!(tracker.latency(&route), Some(5));
    }

    #[test]
    fn online_ttl_expiry_moves_routes_to_offline() {
        let mut tracker = HealthTracker::default();
        let route = candidate_route("ttl-test", ConnectionInterface::Lan, "192.168.250.25");
        let start = Instant::now();
        tracker.record_success(&route, 5, start);
        let later = start + Duration::from_secs(13);
        assert_eq!(tracker.status_at(&route, later, false), PeerStatus::Offline);
        let inside = start + Duration::from_secs(11);
        assert_eq!(tracker.status_at(&route, inside, false), PeerStatus::Online);
    }

    #[test]
    fn apply_peer_health_writes_candidate_and_peer_status() {
        let mut tracker = HealthTracker::default();
        let sessions = SessionRegistry::default();
        let now = Instant::now();
        let lan_route = candidate_route("peer-a", ConnectionInterface::Lan, "192.168.250.26");
        let tail_route =
            candidate_route("peer-a", ConnectionInterface::Tailscale, "100.100.250.26");
        tracker.record_success(&lan_route, 3, now);
        tracker.record_miss(&tail_route, now);
        tracker.record_miss(&tail_route, now);

        let mut peers = vec![PeerInfo {
            hostname: "peer-a".into(),
            tailscale_ip: "100.100.250.26".into(),
            online: true,
            enabled: true,
            address: "192.168.250.26".into(),
            connection_mode: "auto".into(),
            trusted: true,
            fingerprint: String::new(),
            candidates: vec![
                PeerCandidate::new(ConnectionInterface::Lan, "192.168.250.26"),
                PeerCandidate::new(ConnectionInterface::Tailscale, "100.100.250.26"),
            ],
            current_interface: None,
            current_address: None,
            status: PeerStatus::Discovered,
        }];

        apply_peer_health(&mut peers, &tracker, &sessions, now);
        assert_eq!(peers[0].status, PeerStatus::Online);
        assert!(peers[0].online);
        assert_eq!(peers[0].candidates[0].status, PeerStatus::Online);
        assert_eq!(peers[0].candidates[0].latency, Some(3));
        assert_eq!(peers[0].candidates[1].status, PeerStatus::Offline);
        assert!(!peers[0].candidates[1].online);
    }

    #[test]
    fn apply_peer_health_marks_connected_route_current() {
        let mut tracker = HealthTracker::default();
        let sessions = SessionRegistry::default();
        let now = Instant::now();
        let route = candidate_route("peer-b", ConnectionInterface::Tailscale, "100.100.250.27");
        let _guard = sessions.register(route.clone(), 6, &mut tracker, now);

        let mut peers = vec![PeerInfo {
            hostname: "peer-b".into(),
            tailscale_ip: "100.100.250.27".into(),
            online: false,
            enabled: true,
            address: "100.100.250.27".into(),
            connection_mode: "auto".into(),
            trusted: true,
            fingerprint: String::new(),
            candidates: vec![PeerCandidate::new(
                ConnectionInterface::Tailscale,
                "100.100.250.27",
            )],
            current_interface: None,
            current_address: None,
            status: PeerStatus::Discovered,
        }];

        apply_peer_health(&mut peers, &tracker, &sessions, now);
        assert_eq!(peers[0].status, PeerStatus::Connected);
        assert_eq!(
            peers[0].current_interface,
            Some(ConnectionInterface::Tailscale)
        );
        assert_eq!(peers[0].current_address.as_deref(), Some("100.100.250.27"));
    }

    #[test]
    fn apply_peer_health_clears_stale_current_route_after_session_closes() {
        let mut tracker = HealthTracker::default();
        let sessions = SessionRegistry::default();
        let now = Instant::now();
        let route = candidate_route("peer-c", ConnectionInterface::Tailscale, "100.100.250.28");
        let guard = sessions.register(route.clone(), 6, &mut tracker, now);

        let mut peers = vec![PeerInfo {
            hostname: "peer-c".into(),
            tailscale_ip: "100.100.250.28".into(),
            online: false,
            enabled: true,
            address: "100.100.250.28".into(),
            connection_mode: "auto".into(),
            trusted: true,
            fingerprint: String::new(),
            candidates: vec![PeerCandidate::new(
                ConnectionInterface::Tailscale,
                "100.100.250.28",
            )],
            current_interface: Some(ConnectionInterface::Tailscale),
            current_address: Some("100.100.250.28".into()),
            status: PeerStatus::Connected,
        }];

        // First projection with the session open keeps the current route.
        apply_peer_health(&mut peers, &tracker, &sessions, now);
        assert_eq!(peers[0].status, PeerStatus::Connected);
        assert!(peers[0].current_address.is_some());

        // Session closes: the next projection must clear the stale route and
        // fall back to the probe-derived status.
        drop(guard);
        apply_peer_health(&mut peers, &tracker, &sessions, now);
        assert_eq!(peers[0].status, PeerStatus::Online);
        assert!(peers[0].current_interface.is_none());
        assert!(peers[0].current_address.is_none());
    }

    #[test]
    fn probe_rounds_move_never_answered_candidates_offline() {
        // macOS-style feed: a discovery round registers candidates, then
        // failed rounds apply misses. A route that never answered goes
        // offline after two failed rounds, per the shared status model.
        let mut tracker = HealthTracker::default();
        let mut now = Instant::now();
        let peers = vec![PeerInfo {
            hostname: "peer-d".into(),
            tailscale_ip: "100.100.250.29".into(),
            online: false,
            enabled: true,
            address: "100.100.250.29".into(),
            connection_mode: "auto".into(),
            trusted: false,
            fingerprint: String::new(),
            candidates: vec![PeerCandidate::remembered(
                ConnectionInterface::Tailscale,
                "100.100.250.29",
            )],
            current_interface: None,
            current_address: None,
            status: PeerStatus::Discovered,
        }];

        update_peer_health(&mut tracker, "auto", &peers, now);
        let route = candidate_route("peer-d", ConnectionInterface::Tailscale, "100.100.250.29");
        assert_eq!(
            tracker.status_at(&route, now, false),
            PeerStatus::Discovered
        );

        update_peer_health_for_failed_round(&mut tracker, "auto", now);
        now += Duration::from_secs(5);
        update_peer_health_for_failed_round(&mut tracker, "auto", now);
        assert_eq!(tracker.status_at(&route, now, false), PeerStatus::Offline);
    }
}
