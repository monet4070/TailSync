use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct PeerRouteKey {
    pub(super) hostname: String,
    pub(super) interface: ConnectionInterface,
    pub(super) address: String,
}

impl PeerRouteKey {
    pub(super) fn new(hostname: &str, interface: ConnectionInterface, address: &str) -> Self {
        Self {
            hostname: hostname.to_string(),
            interface,
            address: address.to_string(),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct PeerHealth {
    last_seen: Option<tokio::time::Instant>,
    consecutive_misses: u8,
    latency_ms: Option<u64>,
}

#[derive(Debug, Clone, Default)]
struct ActiveSession {
    count: usize,
    latency_ms: u64,
}

static PEER_HEALTH: OnceLock<StdMutex<HashMap<PeerRouteKey, PeerHealth>>> = OnceLock::new();
static ACTIVE_SESSIONS: OnceLock<StdMutex<HashMap<PeerRouteKey, ActiveSession>>> = OnceLock::new();

fn peer_health() -> &'static StdMutex<HashMap<PeerRouteKey, PeerHealth>> {
    PEER_HEALTH.get_or_init(|| StdMutex::new(HashMap::new()))
}

fn active_sessions() -> &'static StdMutex<HashMap<PeerRouteKey, ActiveSession>> {
    ACTIVE_SESSIONS.get_or_init(|| StdMutex::new(HashMap::new()))
}

fn session_count(key: &PeerRouteKey) -> usize {
    active_sessions()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(key)
        .map(|session| session.count)
        .unwrap_or_default()
}

fn health_status(key: &PeerRouteKey, health: Option<&PeerHealth>) -> PeerHealthSnapshot {
    let connected = session_count(key) > 0;
    if connected {
        let latency_ms = active_sessions()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(key)
            .map(|session| session.latency_ms)
            .or_else(|| health.and_then(|value| value.latency_ms));
        return PeerHealthSnapshot {
            status: PeerStatus::Connected,
            online: true,
            connected: true,
            latency_ms,
        };
    }
    let Some(health) = health else {
        return PeerHealthSnapshot {
            status: PeerStatus::Discovered,
            online: false,
            connected: false,
            latency_ms: None,
        };
    };
    let status = match health.last_seen {
        None if health.consecutive_misses >= 2 => PeerStatus::Offline,
        None => PeerStatus::Discovered,
        Some(last_seen)
            if health.consecutive_misses == 0 && last_seen.elapsed() < PEER_ONLINE_TTL =>
        {
            PeerStatus::Online
        }
        Some(last_seen)
            if health.consecutive_misses == 1 && last_seen.elapsed() < PEER_ONLINE_TTL =>
        {
            PeerStatus::Confirming
        }
        Some(_) => PeerStatus::Offline,
    };
    PeerHealthSnapshot {
        online: matches!(status, PeerStatus::Online | PeerStatus::Confirming),
        connected: false,
        status,
        latency_ms: health.latency_ms,
    }
}

pub fn route_health(
    hostname: &str,
    interface: ConnectionInterface,
    address: &str,
) -> PeerHealthSnapshot {
    let key = PeerRouteKey::new(hostname, interface, address);
    let health = peer_health()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&key)
        .cloned();
    health_status(&key, health.as_ref())
}

pub(super) fn record_probe_success(key: &PeerRouteKey, latency_ms: u64) {
    let mut health = peer_health()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let entry = health.entry(key.clone()).or_default();
    entry.last_seen = Some(tokio::time::Instant::now());
    entry.consecutive_misses = 0;
    entry.latency_ms = Some(latency_ms);
}

pub(super) fn record_probe_miss(key: &PeerRouteKey) {
    let mut health = peer_health()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let entry = health.entry(key.clone()).or_default();
    entry.consecutive_misses = entry.consecutive_misses.saturating_add(1);
}

pub fn record_address_test_success(address: &str, latency_ms: u64) {
    let keys = peer_health()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .keys()
        .filter(|key| key.address == address)
        .cloned()
        .collect::<Vec<_>>();
    for key in keys {
        record_probe_success(&key, latency_ms);
    }
}

pub fn record_address_test_failure(address: &str) {
    let keys = peer_health()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .keys()
        .filter(|key| key.address == address)
        .cloned()
        .collect::<Vec<_>>();
    for key in keys {
        record_probe_miss(&key);
    }
}

pub fn active_route(hostname: &str) -> Option<ActiveRoute> {
    active_sessions()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter()
        .filter(|(key, session)| key.hostname == hostname && session.count > 0)
        .map(|(key, session)| ActiveRoute {
            interface: key.interface,
            address: key.address.clone(),
            latency: session.latency_ms,
        })
        .min_by_key(|route| (route.interface.priority(), route.latency))
}

pub fn active_routes_snapshot() -> HashMap<String, ActiveRoute> {
    let hostnames = active_sessions()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .keys()
        .map(|key| key.hostname.clone())
        .collect::<HashSet<_>>();
    hostnames
        .into_iter()
        .filter_map(|hostname| active_route(&hostname).map(|route| (hostname, route)))
        .collect()
}

pub(crate) fn register_active_session(
    hostname: &str,
    interface: ConnectionInterface,
    address: &str,
    latency_ms: u64,
) -> ActiveSessionGuard {
    let key = PeerRouteKey::new(hostname, interface, address);
    let mut sessions = active_sessions()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let entry = sessions.entry(key.clone()).or_default();
    entry.count += 1;
    entry.latency_ms = latency_ms;
    drop(sessions);
    record_probe_success(&key, latency_ms);
    ActiveSessionGuard { key }
}

pub(crate) struct ActiveSessionGuard {
    key: PeerRouteKey,
}

impl Drop for ActiveSessionGuard {
    fn drop(&mut self) {
        let mut sessions = active_sessions()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(session) = sessions.get_mut(&self.key) {
            session.count = session.count.saturating_sub(1);
            if session.count == 0 {
                sessions.remove(&self.key);
            }
        }
    }
}
