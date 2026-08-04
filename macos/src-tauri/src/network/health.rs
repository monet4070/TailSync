use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct RouteKey {
    pub(super) hostname: String,
    pub(super) interface: ConnectionInterface,
    pub(super) address: String,
}

#[derive(Debug, Clone, Default)]
struct PeerHealth {
    last_seen: Option<tokio::time::Instant>,
    consecutive_misses: u8,
    latency_ms: Option<u64>,
}

#[derive(Debug, Default)]
pub(super) struct PeerHealthTracker {
    routes: HashMap<RouteKey, PeerHealth>,
}

impl PeerHealthTracker {
    pub(super) fn ensure_candidate(&mut self, route: RouteKey) {
        self.routes.entry(route).or_default();
    }

    pub(super) fn apply_round(
        &mut self,
        now: tokio::time::Instant,
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
            } else if health.last_seen.is_some() {
                health.consecutive_misses = health.consecutive_misses.saturating_add(1);
            }
        }
    }

    pub(super) fn status_at(
        &self,
        route: &RouteKey,
        now: tokio::time::Instant,
        connected: bool,
    ) -> PeerStatus {
        if connected {
            return PeerStatus::Connected;
        }
        let Some(health) = self.routes.get(route) else {
            return PeerStatus::Discovered;
        };
        let Some(last_seen) = health.last_seen else {
            return PeerStatus::Discovered;
        };
        let recently_seen = now.saturating_duration_since(last_seen) < Duration::from_secs(12);
        if recently_seen && health.consecutive_misses == 0 {
            PeerStatus::Online
        } else if recently_seen && health.consecutive_misses < 2 {
            PeerStatus::Confirming
        } else {
            PeerStatus::Offline
        }
    }

    pub(super) fn latency(&self, route: &RouteKey) -> Option<u64> {
        self.routes.get(route).and_then(|health| health.latency_ms)
    }

    pub(super) fn clear(&mut self) {
        self.routes.clear();
    }
}

static PEER_HEALTH: OnceLock<StdMutex<PeerHealthTracker>> = OnceLock::new();

fn peer_health() -> &'static StdMutex<PeerHealthTracker> {
    PEER_HEALTH.get_or_init(|| StdMutex::new(PeerHealthTracker::default()))
}

pub(super) fn clear_peer_health() {
    peer_health()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clear();
}

pub fn record_address_test_success(address: &str, latency_ms: u64) {
    let now = tokio::time::Instant::now();
    let mut tracker = peer_health()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for (route, health) in &mut tracker.routes {
        if route.address == address {
            health.last_seen = Some(now);
            health.consecutive_misses = 0;
            health.latency_ms = Some(latency_ms);
        }
    }
}

pub fn record_address_test_failure(address: &str) {
    let mut tracker = peer_health()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for (route, health) in &mut tracker.routes {
        if route.address == address {
            health.consecutive_misses = health.consecutive_misses.saturating_add(1);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ActiveRoute {
    pub interface: ConnectionInterface,
    pub address: String,
    pub latency: u64,
}

#[derive(Debug, Clone)]
struct AuthenticatedSessionEntry {
    route: ActiveRoute,
    count: usize,
}

#[derive(Debug, Clone, Default)]
pub(super) struct AuthenticatedSessionRegistry {
    entries: Arc<StdMutex<HashMap<RouteKey, AuthenticatedSessionEntry>>>,
}

impl AuthenticatedSessionRegistry {
    pub(super) fn register(&self, key: RouteKey, latency: u64) -> AuthenticatedSessionGuard {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let entry = entries
            .entry(key.clone())
            .or_insert_with(|| AuthenticatedSessionEntry {
                route: ActiveRoute {
                    interface: key.interface,
                    address: key.address.clone(),
                    latency,
                },
                count: 0,
            });
        entry.count += 1;
        entry.route.latency = latency;
        AuthenticatedSessionGuard {
            registry: self.clone(),
            key,
        }
    }

    pub(super) fn unregister(&self, key: &RouteKey) {
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

    pub(super) fn is_connected(&self, key: &RouteKey) -> bool {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(key)
            .is_some_and(|entry| entry.count > 0)
    }

    pub(super) fn active_route(&self, hostname: &str) -> Option<ActiveRoute> {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .filter(|(key, entry)| key.hostname == hostname && entry.count > 0)
            .map(|(_, entry)| entry.route.clone())
            .min_by_key(|route| (route.interface.priority(), route.latency))
    }

    fn snapshot(&self) -> HashMap<String, ActiveRoute> {
        let hostnames = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .keys()
            .map(|key| key.hostname.clone())
            .collect::<std::collections::HashSet<_>>();
        hostnames
            .into_iter()
            .filter_map(|hostname| self.active_route(&hostname).map(|route| (hostname, route)))
            .collect()
    }
}

static AUTHENTICATED_SESSIONS: OnceLock<AuthenticatedSessionRegistry> = OnceLock::new();

pub(super) fn authenticated_sessions() -> &'static AuthenticatedSessionRegistry {
    AUTHENTICATED_SESSIONS.get_or_init(AuthenticatedSessionRegistry::default)
}

pub fn active_route(hostname: &str) -> Option<ActiveRoute> {
    authenticated_sessions().active_route(hostname)
}

pub fn active_routes_snapshot() -> HashMap<String, ActiveRoute> {
    authenticated_sessions().snapshot()
}

pub(super) struct AuthenticatedSessionGuard {
    registry: AuthenticatedSessionRegistry,
    key: RouteKey,
}

impl Drop for AuthenticatedSessionGuard {
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

pub(super) fn update_peer_health(mode: &str, peers: &[tailscale::PeerInfo]) {
    let now = tokio::time::Instant::now();
    let mut tracker = peer_health()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
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

pub(super) fn update_peer_health_for_failed_round(mode: &str) {
    let now = tokio::time::Instant::now();
    let mut tracker = peer_health()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for interface in interfaces_for_mode(mode) {
        tracker.apply_round(now, *interface, HashMap::new());
    }
}

fn status_rank(status: PeerStatus) -> u8 {
    match status {
        PeerStatus::Connected => 4,
        PeerStatus::Online => 3,
        PeerStatus::Confirming => 2,
        PeerStatus::Discovered => 1,
        PeerStatus::Offline => 0,
    }
}

pub fn apply_peer_health(peers: &mut [tailscale::PeerInfo]) {
    let now = tokio::time::Instant::now();
    let tracker = peer_health()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for peer in peers {
        let mut peer_status = PeerStatus::Offline;
        for candidate in &mut peer.candidates {
            let route = RouteKey {
                hostname: peer.hostname.clone(),
                interface: candidate.interface,
                address: candidate.address.clone(),
            };
            let connected = authenticated_sessions().is_connected(&route);
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
        if let Some(route) = active_route(&peer.hostname) {
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
