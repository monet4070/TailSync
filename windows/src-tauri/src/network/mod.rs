use log::{debug, error, info, warn};
use socket2::{Domain, Protocol as SocketProtocol, SockAddr, Socket, Type};
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use tauri::{AppHandle, Emitter};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot, watch, Mutex, Notify, OwnedSemaphorePermit, RwLock, Semaphore};
use tokio::time::{timeout, Duration};

use crate::crypto;
use crate::db;
use crate::identity::DeviceIdentity;
use crate::pairing::{PairingManager, PendingPairing};
use crate::protocol::{
    unix_timestamp_ms, Command, EventEnvelope, FileChunkPayload, FileOffset, Frame, MessageId,
    ProtocolError, TransferId, FILE_CHUNK_SIZE,
};
use crate::sync;

/// Default TCP port for TailSync
pub const TCP_PORT: u16 = 19890;
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(10);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const IDLE_TIMEOUT: Duration = Duration::from_secs(90);
/// Max queued frames per peer before backpressure kicks in
const POOL_CHANNEL_SIZE: usize = 64;
const POOL_SEND_TIMEOUT: Duration = Duration::from_secs(5);
const EVENT_ACK_TIMEOUT: Duration = Duration::from_millis(750);
const EVENT_RETRY_BASE_DELAY: Duration = Duration::from_millis(250);
const EVENT_MAX_ATTEMPTS: usize = 4;
const FILE_CONFIRM_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const FILE_ACK_TIMEOUT: Duration = Duration::from_secs(10);
/// Reconnect back-off
const RECONNECT_DELAY: Duration = Duration::from_secs(5);
const MAX_FILE_SIZE: u64 = 1024 * 1024 * 1024;
const PEER_CACHE_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const PEER_INITIAL_CACHE_WAIT: Duration = Duration::from_secs(2);
const PEER_MANUAL_REFRESH_WAIT: Duration = Duration::from_secs(5);
const PEER_ONLINE_TTL: Duration = Duration::from_secs(12);

/// Used by the macOS SwiftUI shell to verify that the peer listener survived
/// sleep/wake transitions.
pub static TCP_SERVER_HEALTHY: AtomicBool = AtomicBool::new(false);

pub mod lan;
pub mod mdns;
pub(crate) mod secure;
pub mod tailscale;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionInterface {
    Lan,
    Tailscale,
}

impl ConnectionInterface {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lan => "lan",
            Self::Tailscale => "tailscale",
        }
    }

    fn priority(self) -> u8 {
        match self {
            Self::Lan => 0,
            Self::Tailscale => 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PeerCandidate {
    pub interface: ConnectionInterface,
    pub address: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency: Option<u64>,
    pub priority: u8,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ActiveRoute {
    pub interface: ConnectionInterface,
    pub address: String,
    pub latency: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PeerStatus {
    Discovered,
    Online,
    Confirming,
    Offline,
    Connected,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PeerHealthSnapshot {
    pub status: PeerStatus,
    pub online: bool,
    pub connected: bool,
    pub latency_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PeerRouteKey {
    hostname: String,
    interface: ConnectionInterface,
    address: String,
}

impl PeerRouteKey {
    fn new(hostname: &str, interface: ConnectionInterface, address: &str) -> Self {
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

fn record_probe_success(key: &PeerRouteKey, latency_ms: u64) {
    let mut health = peer_health()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let entry = health.entry(key.clone()).or_default();
    entry.last_seen = Some(tokio::time::Instant::now());
    entry.consecutive_misses = 0;
    entry.latency_ms = Some(latency_ms);
}

fn record_probe_miss(key: &PeerRouteKey) {
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

impl PeerCandidate {
    pub fn new(interface: ConnectionInterface, address: impl Into<String>) -> Self {
        Self {
            interface,
            address: address.into(),
            latency: None,
            priority: interface.priority(),
        }
    }
}

/// Bind a TCP listener with address reuse enabled so a clean daemon restart
/// is not blocked by sockets left in TIME_WAIT after sleep/wake or upgrades.
pub fn bind_tcp_listener(addr: SocketAddr) -> Result<TcpListener, std::io::Error> {
    let domain = match addr {
        SocketAddr::V4(_) => Domain::IPV4,
        SocketAddr::V6(_) => Domain::IPV6,
    };
    let socket = Socket::new(domain, Type::STREAM, Some(SocketProtocol::TCP))?;
    #[cfg(not(target_os = "windows"))]
    socket.set_reuse_address(true)?;
    socket.bind(&SockAddr::from(addr))?;
    socket.listen(128)?;
    socket.set_nonblocking(true)?;
    TcpListener::from_std(socket.into())
}

pub async fn discover_peers(
    mode: &str,
) -> Result<(tailscale::LocalInfo, Vec<tailscale::PeerInfo>), String> {
    match mode {
        "auto" => discover_auto().await,
        "lan_only" | "lan" => discover_lan_hybrid().await,
        "tailscale_only" | "tailscale" => {
            tokio::task::spawn_blocking(tailscale::get_peers)
                .await
                .map_err(|error| format!("Tailscale discovery task failed: {error}"))?
        }
        other => Err(format!("Unsupported connection mode: {other}")),
    }
}

async fn discover_lan_hybrid() -> Result<(tailscale::LocalInfo, Vec<tailscale::PeerInfo>), String> {
    let udp_result = lan::discover().await;
    let mdns_result = Ok(mdns::snapshot());
    merge_lan_discovery_results(udp_result, mdns_result)
}

fn merge_lan_discovery_results(
    udp_result: Result<(tailscale::LocalInfo, Vec<tailscale::PeerInfo>), String>,
    mdns_result: Result<(tailscale::LocalInfo, Vec<tailscale::PeerInfo>), String>,
) -> Result<(tailscale::LocalInfo, Vec<tailscale::PeerInfo>), String> {
    let mut local: Option<tailscale::LocalInfo> = None;
    let mut peers = std::collections::BTreeMap::<String, tailscale::PeerInfo>::new();
    let mut errors = Vec::new();
    for (source, result) in [("udp", udp_result), ("mdns", mdns_result)] {
        match result {
            Ok((mut found_local, found_peers)) => {
                match &mut local {
                    Some(existing) => {
                        if existing.tailscale_ip.is_empty() && !found_local.tailscale_ip.is_empty()
                        {
                            existing.tailscale_ip.clone_from(&found_local.tailscale_ip);
                        }
                        existing.candidates.append(&mut found_local.candidates);
                    }
                    None => local = Some(found_local),
                }
                for peer in found_peers {
                    match peers.get_mut(&peer.hostname) {
                        Some(existing) => {
                            existing.online |= peer.online;
                            existing.candidates.extend(peer.candidates);
                        }
                        None => {
                            peers.insert(peer.hostname.clone(), peer);
                        }
                    }
                }
            }
            Err(error) => errors.push(format!("{source}: {error}")),
        }
    }
    let Some(mut local) = local else {
        return Err(format!("LAN discovery failed ({})", errors.join("; ")));
    };
    local.candidates.sort_by(|left, right| {
        left.priority
            .cmp(&right.priority)
            .then_with(|| left.address.cmp(&right.address))
    });
    local
        .candidates
        .dedup_by(|left, right| left.interface == right.interface && left.address == right.address);
    let mut peers = peers.into_values().collect::<Vec<_>>();
    for peer in &mut peers {
        peer.candidates.sort_by(|left, right| {
            left.priority
                .cmp(&right.priority)
                .then_with(|| left.address.cmp(&right.address))
        });
        peer.candidates.dedup_by(|left, right| {
            left.interface == right.interface && left.address == right.address
        });
        if let Some(candidate) = peer.candidates.first() {
            peer.address.clone_from(&candidate.address);
            peer.tailscale_ip.clone_from(&candidate.address);
        }
    }
    Ok((local, peers))
}

async fn discover_auto() -> Result<(tailscale::LocalInfo, Vec<tailscale::PeerInfo>), String> {
    let (lan_result, tailscale_result) = tokio::join!(
        discover_lan_hybrid(),
        tokio::task::spawn_blocking(tailscale::get_peers)
    );
    let tailscale_result =
        tailscale_result.map_err(|error| format!("Tailscale discovery task failed: {error}"))?;
    merge_discovery_results(lan_result, tailscale_result)
}

fn merge_discovery_results(
    lan_result: Result<(tailscale::LocalInfo, Vec<tailscale::PeerInfo>), String>,
    tailscale_result: Result<(tailscale::LocalInfo, Vec<tailscale::PeerInfo>), String>,
) -> Result<(tailscale::LocalInfo, Vec<tailscale::PeerInfo>), String> {
    let mut local: Option<tailscale::LocalInfo> = None;
    let mut merged = std::collections::BTreeMap::<String, tailscale::PeerInfo>::new();
    let mut errors = Vec::new();

    for (interface, result) in [
        (ConnectionInterface::Lan, lan_result),
        (ConnectionInterface::Tailscale, tailscale_result),
    ] {
        match result {
            Ok((mut found_local, peers)) => {
                if found_local.candidates.is_empty() && !found_local.tailscale_ip.is_empty() {
                    found_local.candidates.push(PeerCandidate::new(
                        interface,
                        found_local.tailscale_ip.clone(),
                    ));
                }
                match &mut local {
                    Some(existing) => {
                        if interface == ConnectionInterface::Lan {
                            existing.hostname.clone_from(&found_local.hostname);
                            if !found_local.tailscale_ip.is_empty() {
                                existing.tailscale_ip.clone_from(&found_local.tailscale_ip);
                            }
                        }
                        existing.candidates.append(&mut found_local.candidates);
                    }
                    None => local = Some(found_local),
                }
                for mut peer in peers {
                    let address = if peer.address.is_empty() {
                        peer.tailscale_ip.clone()
                    } else {
                        peer.address.clone()
                    };
                    if peer.candidates.is_empty() && !address.is_empty() {
                        peer.candidates.push(PeerCandidate::new(interface, address));
                    }
                    peer.connection_mode = "auto".to_string();
                    match merged.get_mut(&peer.hostname) {
                        Some(existing) => {
                            existing.online |= peer.online;
                            existing.candidates.extend(peer.candidates);
                        }
                        None => {
                            merged.insert(peer.hostname.clone(), peer);
                        }
                    }
                }
            }
            Err(error) => errors.push(format!("{}: {error}", interface.as_str())),
        }
    }

    let Some(mut local) = local else {
        return Err(format!(
            "Automatic discovery failed ({})",
            errors.join("; ")
        ));
    };
    local.candidates.sort_by(|left, right| {
        left.priority
            .cmp(&right.priority)
            .then_with(|| left.address.cmp(&right.address))
    });
    local
        .candidates
        .dedup_by(|left, right| left.interface == right.interface && left.address == right.address);
    let mut peers = merged.into_values().collect::<Vec<_>>();
    for peer in &mut peers {
        peer.candidates.sort_by_key(|candidate| candidate.priority);
        peer.candidates.dedup_by(|left, right| {
            left.interface == right.interface && left.address == right.address
        });
        if let Some(preferred) = peer.candidates.first() {
            peer.address.clone_from(&preferred.address);
        }
    }
    Ok((local, peers))
}

pub fn merge_paired_peers(
    settings: &crypto::Settings,
    mode: &str,
    discovered: Vec<tailscale::PeerInfo>,
) -> Vec<tailscale::PeerInfo> {
    let mut discovered_by_hostname = std::collections::BTreeMap::new();

    for mut peer in discovered {
        // A device name can change between pairing and later discovery (for
        // example, the TailSync hostname and Tailscale HostName may differ).
        // Re-associate it with the trusted record by its pinned route so the
        // UI does not show the same physical device twice.
        if !settings.trusted_peer_keys.contains_key(&peer.hostname) {
            let mut matching_hostnames = settings
                .trusted_peer_addresses
                .iter()
                .filter(|(_, remembered)| peer_matches_remembered_addresses(&peer, remembered))
                .map(|(hostname, _)| hostname.clone());
            if let (Some(hostname), None) = (matching_hostnames.next(), matching_hostnames.next()) {
                peer.hostname = hostname;
            }
        }

        match discovered_by_hostname.get_mut(&peer.hostname) {
            Some(existing) => merge_peer_discovery(existing, peer),
            None => {
                discovered_by_hostname.insert(peer.hostname.clone(), peer);
            }
        }
    }

    let mut discovered = discovered_by_hostname.into_values().collect::<Vec<_>>();
    let mut known_hostnames = std::collections::HashSet::new();
    for peer in &mut discovered {
        known_hostnames.insert(peer.hostname.clone());
        peer.enabled = settings
            .enabled_peers
            .get(&peer.hostname)
            .copied()
            .unwrap_or(true);
        if let Some(encoded_key) = settings.trusted_peer_keys.get(&peer.hostname) {
            if let Ok(key) = crate::identity::decode_public_key(encoded_key) {
                peer.trusted = true;
                peer.fingerprint = crate::identity::fingerprint(&key);
            }
        }
        if peer.candidates.is_empty() {
            let address = if peer.address.is_empty() {
                &peer.tailscale_ip
            } else {
                &peer.address
            };
            if let Some(interface) = mode_interface(mode) {
                if !address.is_empty() {
                    peer.candidates
                        .push(PeerCandidate::new(interface, address.clone()));
                }
            }
        }
        if peer.trusted {
            if let Some(remembered) = settings.trusted_peer_addresses.get(&peer.hostname) {
                for interface in [ConnectionInterface::Lan, ConnectionInterface::Tailscale] {
                    if mode != "auto" && mode_interface(mode) != Some(interface) {
                        continue;
                    }
                    let Some(address) = remembered.get(interface.as_str()) else {
                        continue;
                    };
                    if !peer.candidates.iter().any(|candidate| {
                        candidate.interface == interface && candidate.address == *address
                    }) {
                        peer.candidates.push(PeerCandidate::new(interface, address));
                    }
                }
            }
        }
        peer.candidates.sort_by(|left, right| {
            left.priority
                .cmp(&right.priority)
                .then_with(|| left.address.cmp(&right.address))
        });
        peer.candidates.dedup_by(|left, right| {
            left.interface == right.interface && left.address == right.address
        });
        if let Some(preferred) = peer.candidates.first() {
            peer.address.clone_from(&preferred.address);
        }
        if let Some(tailscale) = peer
            .candidates
            .iter()
            .find(|candidate| candidate.interface == ConnectionInterface::Tailscale)
        {
            peer.tailscale_ip.clone_from(&tailscale.address);
        }
    }

    for (hostname, encoded_key) in &settings.trusted_peer_keys {
        if known_hostnames.contains(hostname) {
            continue;
        }
        let remembered = settings.trusted_peer_addresses.get(hostname);
        let mut candidates = Vec::new();
        for interface in [ConnectionInterface::Lan, ConnectionInterface::Tailscale] {
            if mode != "auto" && mode_interface(mode) != Some(interface) {
                continue;
            }
            if let Some(address) =
                remembered.and_then(|addresses| addresses.get(interface.as_str()))
            {
                candidates.push(PeerCandidate::new(interface, address));
            }
        }
        candidates.sort_by_key(|candidate| candidate.priority);
        let address = candidates
            .first()
            .map(|candidate| candidate.address.clone())
            .unwrap_or_default();
        let fingerprint = crate::identity::decode_public_key(encoded_key)
            .map(|key| crate::identity::fingerprint(&key))
            .unwrap_or_default();
        discovered.push(tailscale::PeerInfo {
            hostname: hostname.clone(),
            tailscale_ip: address.clone(),
            online: false,
            enabled: settings
                .enabled_peers
                .get(hostname)
                .copied()
                .unwrap_or(true),
            address,
            connection_mode: mode.to_string(),
            trusted: true,
            fingerprint,
            candidates,
            current_interface: None,
        });
    }
    discovered.sort_by(|left, right| left.hostname.cmp(&right.hostname));
    discovered
}

fn peer_matches_remembered_addresses(
    peer: &tailscale::PeerInfo,
    remembered: &HashMap<String, String>,
) -> bool {
    peer.candidates.iter().any(|candidate| {
        remembered
            .get(candidate.interface.as_str())
            .is_some_and(|address| address == &candidate.address)
    }) || [&peer.address, &peer.tailscale_ip]
        .into_iter()
        .any(|address| !address.is_empty() && remembered.values().any(|known| known == address))
}

fn merge_peer_discovery(existing: &mut tailscale::PeerInfo, mut peer: tailscale::PeerInfo) {
    existing.online |= peer.online;
    existing.candidates.append(&mut peer.candidates);
    existing.candidates.sort_by(|left, right| {
        left.priority
            .cmp(&right.priority)
            .then_with(|| left.address.cmp(&right.address))
    });
    existing
        .candidates
        .dedup_by(|left, right| left.interface == right.interface && left.address == right.address);
    if let Some(preferred) = existing.candidates.first() {
        existing.address.clone_from(&preferred.address);
        if preferred.interface == ConnectionInterface::Tailscale {
            existing.tailscale_ip.clone_from(&preferred.address);
        }
    } else if existing.address.is_empty() {
        existing.address = peer.address;
        existing.tailscale_ip = peer.tailscale_ip;
    }
}

#[derive(Clone)]
struct PeerCacheEntry {
    local: tailscale::LocalInfo,
    peers: Vec<tailscale::PeerInfo>,
}

static PEER_CACHE: OnceLock<RwLock<HashMap<String, PeerCacheEntry>>> = OnceLock::new();
static PEER_REFRESH_NOTIFY: OnceLock<Notify> = OnceLock::new();
static PEER_REFRESH_GENERATION: AtomicU64 = AtomicU64::new(0);
static PEER_REFRESH_COMPLETED: OnceLock<watch::Sender<u64>> = OnceLock::new();
static PEER_REFRESH_MODE: OnceLock<StdMutex<String>> = OnceLock::new();

fn peer_cache() -> &'static RwLock<HashMap<String, PeerCacheEntry>> {
    PEER_CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

fn peer_refresh_notify() -> &'static Notify {
    PEER_REFRESH_NOTIFY.get_or_init(Notify::new)
}

fn peer_refresh_completed() -> &'static watch::Sender<u64> {
    PEER_REFRESH_COMPLETED.get_or_init(|| watch::channel(0).0)
}

fn last_peer_refresh_mode() -> &'static StdMutex<String> {
    PEER_REFRESH_MODE.get_or_init(|| StdMutex::new(String::new()))
}

fn refresh_completed_for_mode(generation: u64, baseline: u64, mode: &str) -> bool {
    generation > baseline
        && last_peer_refresh_mode()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_str()
            == mode
}

async fn store_peer_cache(
    mode: &str,
    local: tailscale::LocalInfo,
    peers: Vec<tailscale::PeerInfo>,
) {
    peer_cache()
        .write()
        .await
        .insert(mode.to_string(), PeerCacheEntry { local, peers });
}

pub async fn clear_peer_cache() {
    peer_cache().write().await.clear();
    peer_refresh_notify().notify_one();
}

async fn cached_peer_entry(mode: &str) -> Option<PeerCacheEntry> {
    peer_cache().read().await.get(mode).cloned()
}

pub async fn cached_discover_peers(
    mode: &str,
) -> Result<(tailscale::LocalInfo, Vec<tailscale::PeerInfo>), String> {
    if let Some(entry) = cached_peer_entry(mode).await {
        return Ok((entry.local, entry.peers));
    }

    let generation = PEER_REFRESH_GENERATION.load(Ordering::Acquire);
    let mut completed = peer_refresh_completed().subscribe();
    peer_refresh_notify().notify_one();
    let _ = timeout(PEER_INITIAL_CACHE_WAIT, async {
        while !refresh_completed_for_mode(
            PEER_REFRESH_GENERATION.load(Ordering::Acquire),
            generation,
            mode,
        ) {
            if completed.changed().await.is_err() {
                break;
            }
        }
    })
    .await;

    cached_peer_entry(mode)
        .await
        .map(|entry| (entry.local, entry.peers))
        .ok_or_else(|| "Peer discovery is still starting".to_string())
}

pub async fn request_peer_refresh(mode: &str) -> Result<(), String> {
    let generation = PEER_REFRESH_GENERATION.load(Ordering::Acquire);
    let mut completed = peer_refresh_completed().subscribe();
    peer_refresh_notify().notify_one();
    timeout(PEER_MANUAL_REFRESH_WAIT, async {
        while !refresh_completed_for_mode(
            PEER_REFRESH_GENERATION.load(Ordering::Acquire),
            generation,
            mode,
        ) {
            completed
                .changed()
                .await
                .map_err(|_| "Peer health monitor stopped".to_string())?;
        }
        Ok::<(), String>(())
    })
    .await
    .map_err(|_| "Peer refresh timed out".to_string())?
}

async fn run_peer_health_round(
    settings: &Arc<Mutex<crypto::Settings>>,
) -> Result<(String, Vec<tailscale::PeerInfo>), String> {
    let snapshot = settings.lock().await.clone();
    let mode = snapshot.connection_mode.clone();
    let discovery = discover_peers(&mode).await;
    let discovered = match discovery {
        Ok((local, peers)) => {
            store_peer_cache(&mode, local, peers.clone()).await;
            remember_peer_addresses(settings, &mode, &peers).await;
            peers
        }
        Err(error) => {
            debug!("Peer discovery failed for {mode} mode: {error}");
            let mut peers = cached_peer_entry(&mode)
                .await
                .map(|entry| entry.peers)
                .unwrap_or_default();
            for peer in &mut peers {
                peer.online = false;
                for candidate in &mut peer.candidates {
                    candidate.latency = None;
                }
            }
            peers
        }
    };

    let peers = merge_paired_peers(&snapshot, &mode, discovered);
    let mut routes = HashMap::<PeerRouteKey, Option<u64>>::new();
    for peer in &peers {
        for candidate in &peer.candidates {
            let key = PeerRouteKey::new(&peer.hostname, candidate.interface, &candidate.address);
            let result = routes.entry(key).or_default();
            if candidate.latency.is_some() {
                *result = candidate.latency;
            }
        }
    }

    let addresses = routes
        .iter()
        .filter(|(_, latency)| latency.is_none())
        .filter_map(|(key, _)| key.address.parse::<IpAddr>().ok())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let probed = if addresses.is_empty() {
        HashMap::new()
    } else {
        tokio::task::spawn_blocking(move || lan::probe_hostnames(&addresses))
            .await
            .map_err(|error| format!("Peer health probe task failed: {error}"))?
    };

    for (key, latency) in routes {
        let latency = latency.or_else(|| {
            key.address
                .parse::<IpAddr>()
                .ok()
                .and_then(|address| probed.get(&address))
                .map(|response| response.latency_ms)
        });
        if let Some(latency_ms) = latency {
            record_probe_success(&key, latency_ms);
        } else {
            record_probe_miss(&key);
        }
    }

    Ok((mode, peers))
}

pub async fn peer_health_monitor(
    settings: Arc<Mutex<crypto::Settings>>,
    pool: Arc<Mutex<ConnectionPool>>,
    app_handle: AppHandle,
) {
    loop {
        let mode = match run_peer_health_round(&settings).await {
            Ok((mode, peers)) => {
                prewarm_connections(pool.clone(), peers).await;
                mode
            }
            Err(error) => {
                debug!("Peer health round failed: {error}");
                settings.lock().await.connection_mode.clone()
            }
        };
        *last_peer_refresh_mode()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = mode.clone();
        let generation = PEER_REFRESH_GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
        let _ = peer_refresh_completed().send(generation);
        let _ = app_handle.emit(
            "peer-health-changed",
            serde_json::json!({ "generation": generation, "mode": mode }),
        );

        tokio::select! {
            () = tokio::time::sleep(PEER_CACHE_REFRESH_INTERVAL) => {}
            () = peer_refresh_notify().notified() => {}
        }
    }
}

pub async fn remember_peer_addresses(
    settings: &Arc<Mutex<crypto::Settings>>,
    mode: &str,
    peers: &[tailscale::PeerInfo],
) {
    let mut settings = settings.lock().await;
    for peer in peers {
        if !peer.candidates.is_empty() {
            for candidate in &peer.candidates {
                if let Err(error) = settings.remember_peer_address(
                    &peer.hostname,
                    candidate.interface.as_str(),
                    &candidate.address,
                ) {
                    debug!(
                        "Could not remember {} address for {}: {error}",
                        candidate.interface.as_str(),
                        peer.hostname
                    );
                }
            }
            continue;
        }
        let address = if peer.address.is_empty() {
            &peer.tailscale_ip
        } else {
            &peer.address
        };
        let Some(interface) = mode_interface(mode) else {
            continue;
        };
        if let Err(error) =
            settings.remember_peer_address(&peer.hostname, interface.as_str(), address)
        {
            debug!(
                "Could not remember {mode} address for {}: {error}",
                peer.hostname
            );
        }
    }
}

pub fn mode_interface(mode: &str) -> Option<ConnectionInterface> {
    match mode {
        "lan" | "lan_only" => Some(ConnectionInterface::Lan),
        "tailscale" | "tailscale_only" => Some(ConnectionInterface::Tailscale),
        _ => None,
    }
}

pub fn infer_interface(address: &str) -> Result<ConnectionInterface, String> {
    let ip: IpAddr = address
        .parse()
        .map_err(|error| format!("Invalid peer address {address}: {error}"))?;
    if source_matches_mode(ip, "tailscale_only") {
        Ok(ConnectionInterface::Tailscale)
    } else {
        Ok(ConnectionInterface::Lan)
    }
}

pub async fn start_discovery_responder(identity: Arc<DeviceIdentity>) {
    tokio::join!(lan::start_responder(), mdns::run(identity));
}

pub fn peer_socket_addr(peer: &tailscale::PeerInfo) -> Result<SocketAddr, String> {
    let address = if peer.address.is_empty() {
        &peer.tailscale_ip
    } else {
        &peer.address
    };
    let ip: IpAddr = address
        .parse()
        .map_err(|e| format!("Invalid peer address {address}: {e}"))?;
    Ok(SocketAddr::new(ip, TCP_PORT))
}

// ═══════════════════════════════════════════════════════════════════
// Connection pool — maintains a persistent TCP connection per peer
// so clipboard bursts don't pay the handshake cost on every send.
// ═══════════════════════════════════════════════════════════════════

#[derive(Clone)]
struct PoolSender {
    priority: mpsc::Sender<QueuedFrame>,
    bulk: mpsc::Sender<QueuedFrame>,
}

struct QueuedFrame {
    command: Command,
    payload: Vec<u8>,
    acknowledgement: AckExpectation,
    completion: Option<oneshot::Sender<Result<DeliveryReceipt, String>>>,
}

struct PendingFrame {
    queued: QueuedFrame,
    sequence: u32,
}

impl PendingFrame {
    fn complete(mut self, result: Result<DeliveryReceipt, String>) {
        if let Some(completion) = self.queued.completion.take() {
            let _ = completion.send(result);
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum AckExpectation {
    None,
    Event(MessageId),
    File(TransferId),
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DeliveryReceipt {
    pub next_offset: Option<u64>,
}

impl QueuedFrame {
    fn new(command: Command, content: Vec<u8>) -> Result<Self, String> {
        let (payload, acknowledgement) =
            if matches!(command, Command::TextPayload | Command::ImagePayload) {
                let envelope = EventEnvelope::new(content);
                if envelope.encoded_len() > command.payload_limit() {
                    return Err(format!(
                        "{:?} reliable payload exceeds the {} byte limit",
                        command,
                        command.payload_limit()
                    ));
                }
                let message_id = envelope.message_id;
                (envelope.encode(), AckExpectation::Event(message_id))
            } else {
                if content.len() > command.payload_limit() {
                    return Err(format!(
                        "{:?} payload exceeds the {} byte limit",
                        command,
                        command.payload_limit()
                    ));
                }
                (content, AckExpectation::None)
            };
        Ok(Self {
            command,
            payload,
            acknowledgement,
            completion: None,
        })
    }

    fn confirmed_file(
        command: Command,
        payload: Vec<u8>,
        transfer_id: TransferId,
        completion: oneshot::Sender<Result<DeliveryReceipt, String>>,
    ) -> Result<Self, String> {
        if !matches!(
            command,
            Command::FileMeta | Command::FileChunk | Command::FileComplete
        ) {
            return Err(format!("{:?} is not a confirmable file command", command));
        }
        if payload.len() > command.payload_limit() {
            return Err(format!(
                "{:?} payload exceeds the {} byte limit",
                command,
                command.payload_limit()
            ));
        }
        Ok(Self {
            command,
            payload,
            acknowledgement: AckExpectation::File(transfer_id),
            completion: Some(completion),
        })
    }
}

impl PoolSender {
    #[cfg(test)]
    fn same_channel(&self, other: &Self) -> bool {
        self.priority.same_channel(&other.priority) && self.bulk.same_channel(&other.bulk)
    }

    fn channel_for(&self, command: Command) -> &mpsc::Sender<QueuedFrame> {
        if command == Command::FileChunk {
            &self.bulk
        } else {
            &self.priority
        }
    }
}

pub struct ConnectionPool {
    senders: HashMap<(SocketAddr, String), PoolSender>,
    identity: Arc<DeviceIdentity>,
    settings: Arc<Mutex<crypto::Settings>>,
}

#[derive(Clone)]
struct ResolvedCandidate {
    candidate: PeerCandidate,
    socket_addr: SocketAddr,
}

fn resolve_candidates(peer: &tailscale::PeerInfo) -> Result<Vec<ResolvedCandidate>, String> {
    let mut candidates = peer.candidates.clone();
    if candidates.is_empty() {
        let address = if peer.address.is_empty() {
            &peer.tailscale_ip
        } else {
            &peer.address
        };
        let interface = mode_interface(&peer.connection_mode)
            .or_else(|| infer_interface(address).ok())
            .ok_or_else(|| format!("Peer {} has no connection candidates", peer.hostname))?;
        candidates.push(PeerCandidate::new(interface, address));
    }
    candidates.sort_by_key(|candidate| candidate.priority);
    candidates
        .into_iter()
        .map(|candidate| {
            let ip: IpAddr = candidate
                .address
                .parse()
                .map_err(|error| format!("Invalid peer address {}: {error}", candidate.address))?;
            Ok(ResolvedCandidate {
                socket_addr: SocketAddr::new(ip, TCP_PORT),
                candidate,
            })
        })
        .collect()
}

impl ConnectionPool {
    pub fn new(identity: Arc<DeviceIdentity>, settings: Arc<Mutex<crypto::Settings>>) -> Self {
        ConnectionPool {
            senders: HashMap::new(),
            identity,
            settings,
        }
    }

    fn sender_for(&mut self, addr: SocketAddr, hostname: String) -> PoolSender {
        let interface = infer_interface(&addr.ip().to_string()).unwrap_or(ConnectionInterface::Lan);
        self.sender_for_candidates(
            hostname,
            vec![ResolvedCandidate {
                candidate: PeerCandidate::new(interface, addr.ip().to_string()),
                socket_addr: addr,
            }],
        )
    }

    fn sender_for_peer(&mut self, peer: &tailscale::PeerInfo) -> Result<PoolSender, String> {
        Ok(self.sender_for_candidates(peer.hostname.clone(), resolve_candidates(peer)?))
    }

    fn sender_for_candidates(
        &mut self,
        hostname: String,
        candidates: Vec<ResolvedCandidate>,
    ) -> PoolSender {
        let addr = candidates
            .first()
            .expect("connection candidates must not be empty")
            .socket_addr;
        let key = (addr, hostname.clone());
        if let Some(tx) = self.senders.get(&key) {
            return tx.clone();
        }

        self.senders
            .retain(|(_, peer_hostname), _| peer_hostname != &hostname);

        let (priority, priority_rx) = mpsc::channel::<QueuedFrame>(POOL_CHANNEL_SIZE);
        let (bulk, bulk_rx) = mpsc::channel::<QueuedFrame>(POOL_CHANNEL_SIZE);
        let tx = PoolSender { priority, bulk };
        self.senders.insert(key, tx.clone());
        tokio::spawn(connection_task(
            candidates,
            hostname,
            priority_rx,
            bulk_rx,
            self.identity.clone(),
            self.settings.clone(),
        ));
        tx
    }

    /// Push a frame to the peer. Creates a persistent background connection on
    /// first use. Prefer `queue_pool_frame` when calling through the shared
    /// `Arc<Mutex<ConnectionPool>>`, because it releases the pool lock before
    /// waiting for queue capacity.
    pub async fn send(
        &mut self,
        addr: SocketAddr,
        hostname: String,
        cmd: Command,
        payload: Vec<u8>,
    ) -> Result<(), String> {
        let trusted_key = self
            .settings
            .lock()
            .await
            .trusted_peer_keys
            .get(&hostname)
            .cloned()
            .ok_or_else(|| format!("Peer {hostname} is not paired"))?;
        secure::decode_trusted_key(&trusted_key)
            .map_err(|error| format!("Peer {hostname} has an invalid pinned key: {error}"))?;
        if payload.len() > cmd.payload_limit() {
            return Err(format!(
                "{:?} payload exceeds the {} byte limit",
                cmd,
                cmd.payload_limit()
            ));
        }
        let tx = self.sender_for(addr, hostname);

        enqueue_pool_frame(tx, addr, cmd, payload).await
    }

    /// Remove a peer from the pool (e.g. when user disables it).
    pub fn disconnect_hostname(&mut self, hostname: &str) {
        self.senders
            .retain(|(_, peer_hostname), _| peer_hostname != hostname);
    }

    pub fn disconnect_all(&mut self) {
        self.senders.clear();
    }
}

#[allow(dead_code)]
pub(crate) async fn queue_pool_frame(
    pool: &Arc<Mutex<ConnectionPool>>,
    addr: SocketAddr,
    hostname: String,
    cmd: Command,
    payload: Vec<u8>,
) -> Result<(), String> {
    if payload.len() > cmd.payload_limit() {
        return Err(format!(
            "{:?} payload exceeds the {} byte limit",
            cmd,
            cmd.payload_limit()
        ));
    }

    let settings = { pool.lock().await.settings.clone() };
    let trusted_key = settings
        .lock()
        .await
        .trusted_peer_keys
        .get(&hostname)
        .cloned()
        .ok_or_else(|| format!("Peer {hostname} is not paired"))?;
    secure::decode_trusted_key(&trusted_key)
        .map_err(|error| format!("Peer {hostname} has an invalid pinned key: {error}"))?;

    let tx = { pool.lock().await.sender_for(addr, hostname) };
    enqueue_pool_frame(tx, addr, cmd, payload).await
}

pub async fn queue_peer_frame(
    pool: &Arc<Mutex<ConnectionPool>>,
    peer: &tailscale::PeerInfo,
    cmd: Command,
    payload: Vec<u8>,
) -> Result<(), String> {
    if payload.len() > cmd.payload_limit() {
        return Err(format!(
            "{:?} payload exceeds the {} byte limit",
            cmd,
            cmd.payload_limit()
        ));
    }
    let settings = { pool.lock().await.settings.clone() };
    let trusted_key = settings
        .lock()
        .await
        .trusted_peer_keys
        .get(&peer.hostname)
        .cloned()
        .ok_or_else(|| format!("Peer {} is not paired", peer.hostname))?;
    secure::decode_trusted_key(&trusted_key)
        .map_err(|error| format!("Peer {} has an invalid pinned key: {error}", peer.hostname))?;

    let tx = { pool.lock().await.sender_for_peer(peer)? };
    let preferred = resolve_candidates(peer)?
        .first()
        .map(|candidate| candidate.socket_addr)
        .ok_or_else(|| format!("Peer {} has no connection candidates", peer.hostname))?;
    enqueue_pool_frame(tx, preferred, cmd, payload).await
}

pub async fn queue_peer_file_frame(
    pool: &Arc<Mutex<ConnectionPool>>,
    peer: &tailscale::PeerInfo,
    command: Command,
    payload: Vec<u8>,
    transfer_id: TransferId,
) -> Result<DeliveryReceipt, String> {
    let settings = { pool.lock().await.settings.clone() };
    let trusted_key = settings
        .lock()
        .await
        .trusted_peer_keys
        .get(&peer.hostname)
        .cloned()
        .ok_or_else(|| format!("Peer {} is not paired", peer.hostname))?;
    secure::decode_trusted_key(&trusted_key)
        .map_err(|error| format!("Peer {} has an invalid pinned key: {error}", peer.hostname))?;

    let tx = { pool.lock().await.sender_for_peer(peer)? };
    let preferred = resolve_candidates(peer)?
        .first()
        .map(|candidate| candidate.socket_addr)
        .ok_or_else(|| format!("Peer {} has no connection candidates", peer.hostname))?;
    let (completion_tx, completion_rx) = oneshot::channel();
    let queued = QueuedFrame::confirmed_file(command, payload, transfer_id, completion_tx)?;
    enqueue_queued_frame(tx, preferred, queued).await?;
    timeout(FILE_CONFIRM_TIMEOUT, completion_rx)
        .await
        .map_err(|_| format!("Timed out waiting for {:?} confirmation", command))?
        .map_err(|_| format!("Connection task for {} closed", peer.hostname))?
}

/// Start persistent connection tasks before the first clipboard payload so
/// copying does not pay TCP and Noise handshake latency.
pub async fn prewarm_connections(
    pool: Arc<Mutex<ConnectionPool>>,
    peers: Vec<tailscale::PeerInfo>,
) {
    for peer in peers
        .into_iter()
        .filter(|peer| peer.enabled && peer.trusted)
    {
        if let Err(error) = pool.lock().await.sender_for_peer(&peer) {
            debug!("Could not prewarm {}: {error}", peer.hostname);
        }
    }
}

async fn enqueue_pool_frame(
    tx: PoolSender,
    addr: SocketAddr,
    cmd: Command,
    payload: Vec<u8>,
) -> Result<(), String> {
    let queued = QueuedFrame::new(cmd, payload)?;
    enqueue_queued_frame(tx, addr, queued).await
}

async fn enqueue_queued_frame(
    tx: PoolSender,
    addr: SocketAddr,
    queued: QueuedFrame,
) -> Result<(), String> {
    let command = queued.command;
    timeout(POOL_SEND_TIMEOUT, tx.channel_for(command).send(queued))
        .await
        .map_err(|_| format!("Timed out queueing frame for {}", addr))?
        .map_err(|_| format!("Connection to {} closed", addr))
}

/// Background task for one pooled connection.
///
/// - Connects + handshakes, then loops reading from `rx`.
/// - Each `(cmd, payload)` becomes a frame on the wire.
/// - Sends periodic heartbeats.
/// - Reconnects transparently on write errors.
async fn connection_task(
    candidates: Vec<ResolvedCandidate>,
    hostname: String,
    mut priority_rx: mpsc::Receiver<QueuedFrame>,
    mut bulk_rx: mpsc::Receiver<QueuedFrame>,
    identity: Arc<DeviceIdentity>,
    settings: Arc<Mutex<crypto::Settings>>,
) {
    let preferred_addr = candidates
        .first()
        .map(|candidate| candidate.socket_addr)
        .expect("connection task must have at least one candidate");
    let mut pending: Option<PendingFrame> = None;
    let mut next_sequence = 1u32;
    loop {
        let (mut stream, route) =
            match race_connect_and_handshake(&candidates, &hostname, &identity, &settings).await {
                Ok(result) => result,
                Err(e) => {
                    warn!(
                        "Pool connect to {} ({}) failed: {} — retrying in {:?}",
                        preferred_addr, hostname, e, RECONNECT_DELAY
                    );
                    tokio::time::sleep(RECONNECT_DELAY).await;
                    continue;
                }
            };
        let addr = route.socket_addr;
        let latency_ms = route.candidate.latency.unwrap_or_default();
        debug!(
            "Pool connected to {} via {} in {} ms",
            addr,
            route.candidate.interface.as_str(),
            latency_ms
        );
        let _active_guard = register_active_session(
            &hostname,
            route.candidate.interface,
            &route.candidate.address,
            latency_ms,
        );

        let mut last_heartbeat = tokio::time::Instant::now();

        // A write can fail after the frame has been removed from the queue.
        // Keep that frame across reconnects so transient breaks do not lose
        // clipboard content silently.
        if let Some(frame) = pending.take() {
            match deliver_pending_frame(&mut stream, &frame).await {
                Ok(receipt) => frame.complete(Ok(receipt)),
                Err(error) => {
                    debug!(
                        "Pool delivery to {} failed: {error} — reselecting path",
                        addr
                    );
                    pending = Some(frame);
                    continue;
                }
            }
        }

        // Inner loop: read from channel, write to wire
        loop {
            if last_heartbeat.elapsed() >= HEARTBEAT_INTERVAL {
                let hb = Frame::new(Command::Heartbeat, 0, next_sequence, vec![]);
                next_sequence = next_sequence.wrapping_add(1).max(1);
                if stream.write_frame(&hb).await.is_err()
                    || !matches!(
                        timeout(CONNECTION_TIMEOUT, stream.read_frame()).await,
                        Ok(Ok(Frame {
                            command: Command::HeartbeatAck,
                            ..
                        }))
                    )
                {
                    debug!("Pool heartbeat to {} failed — reconnecting", addr);
                    break;
                }
                last_heartbeat = tokio::time::Instant::now();
            }

            // Wait for next frame or heartbeat deadline
            let deadline = HEARTBEAT_INTERVAL.saturating_sub(last_heartbeat.elapsed());
            let next_frame = async {
                tokio::select! {
                    biased;
                    frame = priority_rx.recv() => frame,
                    frame = bulk_rx.recv() => frame,
                }
            };
            match tokio::time::timeout(deadline, next_frame).await {
                Ok(Some(queued)) => {
                    let frame = PendingFrame {
                        queued,
                        sequence: next_sequence,
                    };
                    next_sequence = next_sequence.wrapping_add(1).max(1);
                    match deliver_pending_frame(&mut stream, &frame).await {
                        Ok(receipt) => frame.complete(Ok(receipt)),
                        Err(error) => {
                            pending = Some(frame);
                            debug!(
                                "Pool delivery to {} failed: {error} — reselecting path",
                                addr
                            );
                            break;
                        }
                    }
                }
                Ok(None) => {
                    // All senders dropped — exit this connection for good
                    debug!("Pool channel for {} closed — shutting down", addr);
                    return;
                }
                Err(_) => {
                    // Timeout — loop back to send heartbeat
                }
            }
        }
        // Outer loop: reconnect and try again
    }
}

async fn deliver_pending_frame(
    stream: &mut secure::SecureConnection,
    pending: &PendingFrame,
) -> Result<DeliveryReceipt, String> {
    let frame = Frame::new(
        pending.queued.command,
        0,
        pending.sequence,
        pending.queued.payload.clone(),
    );
    match pending.queued.acknowledgement {
        AckExpectation::None => {
            stream
                .write_frame(&frame)
                .await
                .map_err(|error| error.to_string())?;
            Ok(DeliveryReceipt::default())
        }
        AckExpectation::Event(message_id) => {
            deliver_event_frame(stream, pending, &frame, message_id).await?;
            Ok(DeliveryReceipt::default())
        }
        AckExpectation::File(transfer_id) => {
            return deliver_file_frame(stream, pending, &frame, transfer_id).await;
        }
    }
}

async fn deliver_event_frame(
    stream: &mut secure::SecureConnection,
    pending: &PendingFrame,
    frame: &Frame,
    message_id: MessageId,
) -> Result<(), String> {
    for attempt in 0..EVENT_MAX_ATTEMPTS {
        stream
            .write_frame(frame)
            .await
            .map_err(|error| error.to_string())?;
        match timeout(EVENT_ACK_TIMEOUT, stream.read_frame()).await {
            Ok(Ok(ack)) if ack.command == Command::EventAck => {
                let acknowledged =
                    MessageId::from_ack_payload(&ack.payload).map_err(|error| error.to_string())?;
                if ack.sequence != pending.sequence || acknowledged != message_id {
                    return Err("received an acknowledgement for a different event".to_string());
                }
                return Ok(());
            }
            Ok(Ok(frame)) => {
                return Err(format!("expected EventAck, received {:?}", frame.command));
            }
            Ok(Err(error)) => return Err(error.to_string()),
            Err(_) if attempt + 1 < EVENT_MAX_ATTEMPTS => {
                let multiplier = 1u32 << attempt;
                tokio::time::sleep(EVENT_RETRY_BASE_DELAY * multiplier).await;
            }
            Err(_) => {
                return Err(format!(
                    "event acknowledgement timed out after {EVENT_MAX_ATTEMPTS} attempts"
                ));
            }
        }
    }
    unreachable!("event retry loop always returns")
}

async fn deliver_file_frame(
    stream: &mut secure::SecureConnection,
    pending: &PendingFrame,
    frame: &Frame,
    transfer_id: TransferId,
) -> Result<DeliveryReceipt, String> {
    for attempt in 0..EVENT_MAX_ATTEMPTS {
        stream
            .write_frame(frame)
            .await
            .map_err(|error| error.to_string())?;
        match timeout(FILE_ACK_TIMEOUT, stream.read_frame()).await {
            Ok(Ok(ack)) if matches!(ack.command, Command::FileAck | Command::FileResume) => {
                let offset = FileOffset::decode(&ack.payload).map_err(|error| error.to_string())?;
                if ack.sequence != pending.sequence || offset.transfer_id != transfer_id {
                    return Err("received a file acknowledgement for another transfer".to_string());
                }
                return Ok(DeliveryReceipt {
                    next_offset: Some(offset.next_offset),
                });
            }
            Ok(Ok(frame)) => {
                return Err(format!(
                    "expected file acknowledgement, received {:?}",
                    frame.command
                ));
            }
            Ok(Err(error)) => return Err(error.to_string()),
            Err(_) if attempt + 1 < EVENT_MAX_ATTEMPTS => {
                let multiplier = 1u32 << attempt;
                tokio::time::sleep(EVENT_RETRY_BASE_DELAY * multiplier).await;
            }
            Err(_) => {
                return Err(format!(
                    "file acknowledgement timed out after {EVENT_MAX_ATTEMPTS} attempts"
                ));
            }
        }
    }
    unreachable!("file retry loop always returns")
}

async fn race_connect_and_handshake(
    candidates: &[ResolvedCandidate],
    hostname: &str,
    identity: &Arc<DeviceIdentity>,
    settings: &Arc<Mutex<crypto::Settings>>,
) -> Result<(secure::SecureConnection, ResolvedCandidate), String> {
    let has_lan = candidates
        .iter()
        .any(|candidate| candidate.candidate.interface == ConnectionInterface::Lan);
    let (tx, mut rx) = mpsc::channel(candidates.len().max(1));
    let mut tasks = Vec::with_capacity(candidates.len());

    for candidate in candidates.iter().cloned() {
        let tx = tx.clone();
        let hostname = hostname.to_string();
        let identity = identity.clone();
        let settings = settings.clone();
        let delay = if has_lan && candidate.candidate.interface == ConnectionInterface::Tailscale {
            Duration::from_millis(250)
        } else {
            Duration::ZERO
        };
        tasks.push(tokio::spawn(async move {
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
            let started = tokio::time::Instant::now();
            let result = timeout(
                HANDSHAKE_TIMEOUT,
                connect_and_handshake(
                    candidate.socket_addr,
                    &hostname,
                    &identity,
                    &settings,
                    candidate.candidate.interface,
                ),
            )
            .await
            .map_err(|_| "handshake timed out".to_string())
            .and_then(|result| result.map_err(|error| error.to_string()));
            let mut candidate = candidate;
            candidate.candidate.latency = Some(started.elapsed().as_millis() as u64);
            let _ = tx.send((candidate, result)).await;
        }));
    }
    drop(tx);

    let mut errors = Vec::new();
    while let Some((candidate, result)) = rx.recv().await {
        match result {
            Ok(stream) => {
                for task in tasks {
                    task.abort();
                }
                return Ok((stream, candidate));
            }
            Err(error) => errors.push(format!(
                "{} {}: {error}",
                candidate.candidate.interface.as_str(),
                candidate.socket_addr
            )),
        }
    }
    Err(errors.join("; "))
}

/// One-shot connect + handshake.  Returns an authenticated stream.
async fn connect_and_handshake(
    addr: SocketAddr,
    hostname: &str,
    identity: &DeviceIdentity,
    settings: &Arc<Mutex<crypto::Settings>>,
    interface: ConnectionInterface,
) -> Result<secure::SecureConnection, Box<dyn std::error::Error + Send + Sync>> {
    let expected_key = {
        let settings = settings.lock().await;
        settings
            .trusted_peer_keys
            .get(hostname)
            .cloned()
            .ok_or_else(|| format!("Peer {hostname} is not paired"))?
    };
    let expected_key = secure::decode_trusted_key(&expected_key)?;
    let stream = timeout(CONNECTION_TIMEOUT, TcpStream::connect(&addr)).await??;
    secure::connect(
        stream,
        identity,
        local_peer_identity(interface.as_str()),
        hostname,
        &expected_key,
    )
    .await
}

// ═══════════════════════════════════════════════════════════════════
// TCP server (inbound connections from peers)
// ═══════════════════════════════════════════════════════════════════

struct ConnectionLimiter {
    total: Arc<Semaphore>,
    per_ip: StdMutex<HashMap<IpAddr, usize>>,
    max_per_ip: usize,
}

struct ConnectionPermit {
    limiter: Arc<ConnectionLimiter>,
    ip: IpAddr,
    _total: OwnedSemaphorePermit,
}

impl ConnectionLimiter {
    fn new(max_total: usize, max_per_ip: usize) -> Arc<Self> {
        Arc::new(Self {
            total: Arc::new(Semaphore::new(max_total)),
            per_ip: StdMutex::new(HashMap::new()),
            max_per_ip,
        })
    }

    fn try_acquire(self: &Arc<Self>, ip: IpAddr) -> Option<ConnectionPermit> {
        let total = self.total.clone().try_acquire_owned().ok()?;
        let mut counts = self
            .per_ip
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let count = counts.entry(ip).or_default();
        if *count >= self.max_per_ip {
            return None;
        }
        *count += 1;
        drop(counts);
        Some(ConnectionPermit {
            limiter: self.clone(),
            ip,
            _total: total,
        })
    }
}

impl Drop for ConnectionPermit {
    fn drop(&mut self) {
        let mut counts = self
            .limiter
            .per_ip
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(count) = counts.get_mut(&self.ip) {
            *count -= 1;
            if *count == 0 {
                counts.remove(&self.ip);
            }
        }
    }
}

/// Start the async TCP server.  Runs until the app shuts down.
pub async fn start_server(
    sync_engine: Arc<Mutex<sync::SyncEngine>>,
    database: Arc<Mutex<db::HistoryDB>>,
    settings: Arc<Mutex<crypto::Settings>>,
    identity: Arc<DeviceIdentity>,
    pairing: Arc<PairingManager>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let addr = SocketAddr::from(([0, 0, 0, 0], TCP_PORT));
    let limiter = ConnectionLimiter::new(64, 8);

    loop {
        TCP_SERVER_HEALTHY.store(false, Ordering::Release);
        let listener = match bind_tcp_listener(addr) {
            Ok(listener) => listener,
            Err(error) => {
                error!("TCP server bind error: {}", error);
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };
        TCP_SERVER_HEALTHY.store(true, Ordering::Release);
        info!("TCP server listening on port {}", TCP_PORT);

        loop {
            match listener.accept().await {
                Ok((stream, peer_addr)) => {
                    let Some(permit) = limiter.try_acquire(peer_addr.ip()) else {
                        warn!("Connection limit reached for {}", peer_addr.ip());
                        continue;
                    };
                    debug!("New connection from {}", peer_addr);
                    let sync = sync_engine.clone();
                    let db = database.clone();
                    let settings = settings.clone();
                    let identity = identity.clone();
                    let pairing = pairing.clone();
                    tokio::spawn(async move {
                        let _permit = permit;
                        if let Err(e) = handle_connection(
                            stream, peer_addr, sync, db, settings, identity, pairing,
                        )
                        .await
                        {
                            warn!("Connection {} error: {}", peer_addr, e);
                        }
                    });
                }
                Err(error) => {
                    error!("TCP accept error: {}; rebuilding listener", error);
                    TCP_SERVER_HEALTHY.store(false, Ordering::Release);
                    break;
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn handle_connection(
    stream: TcpStream,
    peer_addr: SocketAddr,
    sync_engine: Arc<Mutex<sync::SyncEngine>>,
    database: Arc<Mutex<db::HistoryDB>>,
    settings: Arc<Mutex<crypto::Settings>>,
    identity: Arc<DeviceIdentity>,
    pairing: Arc<PairingManager>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (source_allowed, mode) = {
        let settings = settings.lock().await;
        (
            source_matches_mode(peer_addr.ip(), &settings.connection_mode),
            settings.connection_mode.clone(),
        )
    };
    if !source_allowed {
        return Err("Connection source is outside the selected network".into());
    }

    let accepted = timeout(
        HANDSHAKE_TIMEOUT,
        secure::accept_with_pairing_window(
            stream,
            &identity,
            local_peer_identity(&mode),
            pairing.subscribe_window(),
        ),
    )
    .await
    .map_err(|_| "Handshake timed out")??;
    let purpose = accepted.purpose;
    let handshake_hash = accepted.handshake_hash;
    let mut stream = accepted.connection;
    let peer_info = accepted.peer_identity;
    let peer_public_key = accepted.remote_public_key;

    if purpose == secure::HandshakePurpose::Pairing {
        let address = peer_addr.ip().to_string();
        let interface = infer_interface(&address)?.as_str().to_string();
        secure::write_ready(&mut stream).await?;
        return pairing
            .install_session(PendingPairing {
                connection: stream,
                hostname: peer_info.hostname,
                remote_public_key: peer_public_key,
                handshake_hash,
                address,
                interface,
            })
            .await
            .map_err(Into::into);
    }

    let (trusted_key, peer_enabled, source_allowed) = {
        let settings = settings.lock().await;
        (
            settings
                .trusted_peer_keys
                .get(&peer_info.hostname)
                .and_then(|key| secure::decode_trusted_key(key).ok()),
            settings
                .enabled_peers
                .get(&peer_info.hostname)
                .copied()
                .unwrap_or(true),
            source_matches_mode(peer_addr.ip(), &settings.connection_mode),
        )
    };
    if !source_allowed
        || !peer_enabled
        || trusted_key.as_deref() != Some(peer_public_key.as_slice())
    {
        secure::write_error(&mut stream, "Peer is not paired or is disabled").await?;
        return Ok(());
    }

    info!(
        "Authenticated peer {} ({}) connected as {} [{}]",
        peer_addr,
        peer_info.tailscale_ip,
        peer_info.hostname,
        secure::fingerprint(&peer_public_key)
    );
    let source_interface = infer_interface(&peer_addr.ip().to_string())?;
    if let Err(error) = settings.lock().await.remember_peer_address(
        &peer_info.hostname,
        source_interface.as_str(),
        &peer_addr.ip().to_string(),
    ) {
        warn!(
            "Could not remember address for {}: {error}",
            peer_info.hostname
        );
    }
    secure::write_ready(&mut stream).await?;
    let _active_guard = register_active_session(
        &peer_info.hostname,
        source_interface,
        &peer_addr.ip().to_string(),
        0,
    );

    // ── Receive loop ─────────────────────────────────────────────
    let mut last_activity = tokio::time::Instant::now();
    let mut last_reliable_sequence = None;

    loop {
        let frame = match timeout(CONNECTION_TIMEOUT, stream.read_frame()).await {
            Ok(Ok(f)) => f,
            Ok(Err(ProtocolError::IncompleteFrame { .. })) => continue,
            Ok(Err(e)) => {
                warn!("Protocol error from {}: {}", peer_addr, e);
                break;
            }
            Err(_) => {
                if last_activity.elapsed() > IDLE_TIMEOUT {
                    debug!("Connection {} idle timeout", peer_addr);
                    break;
                }
                continue;
            }
        };

        last_activity = tokio::time::Instant::now();

        let still_authorized = {
            let settings = settings.lock().await;
            settings
                .trusted_peer_keys
                .get(&peer_info.hostname)
                .and_then(|key| secure::decode_trusted_key(key).ok())
                .as_deref()
                == Some(peer_public_key.as_slice())
                && settings
                    .enabled_peers
                    .get(&peer_info.hostname)
                    .copied()
                    .unwrap_or(true)
                && source_matches_mode(peer_addr.ip(), &settings.connection_mode)
        };
        if !still_authorized {
            secure::write_error(&mut stream, "Peer authorization was revoked").await?;
            break;
        }

        match frame.command {
            Command::Heartbeat => {
                let ack = Frame::new(Command::HeartbeatAck, 0, frame.sequence, vec![]);
                stream.write_frame(&ack).await?;
            }
            Command::TextPayload => {
                if let Err(error) = receive_reliable_event(
                    &mut stream,
                    frame,
                    &peer_info.hostname,
                    &sync_engine,
                    &database,
                    &mut last_reliable_sequence,
                )
                .await
                {
                    warn!("Rejected text event from {}: {error}", peer_addr);
                    secure::write_error(&mut stream, &error).await?;
                }
            }
            Command::ImagePayload => {
                if let Err(error) = receive_reliable_event(
                    &mut stream,
                    frame,
                    &peer_info.hostname,
                    &sync_engine,
                    &database,
                    &mut last_reliable_sequence,
                )
                .await
                {
                    warn!("Rejected image event from {}: {error}", peer_addr);
                    secure::write_error(&mut stream, &error).await?;
                }
            }
            Command::FileMeta => {
                let mut meta: sync::FileMeta = serde_json::from_slice(&frame.payload)?;
                if meta.size > MAX_FILE_SIZE {
                    secure::write_error(&mut stream, "File exceeds the 1 GiB receive limit")
                        .await?;
                    continue;
                }
                let Some(file_name) = std::path::Path::new(&meta.name).file_name() else {
                    secure::write_error(&mut stream, "Invalid file name").await?;
                    continue;
                };
                meta.name = file_name.to_string_lossy().to_string();
                meta.name = sync::normalize_transferred_file_name(&meta.name, &meta.hash);
                if meta.name.is_empty() {
                    secure::write_error(&mut stream, "Invalid file name").await?;
                    continue;
                }
                if meta.transfer_id.is_some()
                    && (meta.chunk_size == 0 || meta.chunk_size as usize > FILE_CHUNK_SIZE)
                {
                    secure::write_error(&mut stream, "Invalid file chunk size").await?;
                    continue;
                }
                let resumable = meta.transfer_id.is_some();
                info!(
                    "Receiving file from {}: {} ({} bytes)",
                    peer_addr, meta.name, meta.size
                );
                let incoming_dir = db::get_incoming_dir();
                std::fs::create_dir_all(&incoming_dir)?;
                let file_path =
                    incoming_dir.join(format!("{:016x}-{}", rand::random::<u64>(), meta.name));
                let result = sync_engine
                    .lock()
                    .await
                    .begin_file_receive(meta, &file_path, peer_info.hostname.clone())
                    .await;
                match result {
                    Ok((transfer_id, next_offset)) if resumable => {
                        stream
                            .write_frame(&Frame::new(
                                Command::FileResume,
                                0,
                                frame.sequence,
                                FileOffset {
                                    transfer_id,
                                    next_offset,
                                }
                                .encode(),
                            ))
                            .await?;
                    }
                    Ok(_) => {}
                    Err(error) => secure::write_error(&mut stream, &error).await?,
                }
            }
            Command::FileChunk => {
                if frame.payload.starts_with(b"FCH1") {
                    match FileChunkPayload::decode(&frame.payload) {
                        Ok(chunk) => {
                            let expected_end = chunk.offset.saturating_add(chunk.data.len() as u64);
                            match sync_engine
                                .lock()
                                .await
                                .handle_resumable_file_chunk(&chunk, peer_info.hostname.clone())
                                .await
                            {
                                Ok(next_offset) => {
                                    let command = if next_offset >= expected_end {
                                        Command::FileAck
                                    } else {
                                        Command::FileResume
                                    };
                                    stream
                                        .write_frame(&Frame::new(
                                            command,
                                            0,
                                            frame.sequence,
                                            FileOffset {
                                                transfer_id: chunk.transfer_id,
                                                next_offset,
                                            }
                                            .encode(),
                                        ))
                                        .await?;
                                }
                                Err(error) => secure::write_error(&mut stream, &error).await?,
                            }
                        }
                        Err(error) => {
                            secure::write_error(&mut stream, &error.to_string()).await?;
                        }
                    }
                } else {
                    sync_engine
                        .lock()
                        .await
                        .handle_file_chunk(&frame.payload, peer_info.hostname.clone())
                        .await;
                }
            }
            Command::CancelTransfer => {
                warn!("Transfer cancelled by {}", peer_addr);
                sync_engine
                    .lock()
                    .await
                    .cancel_receive(&peer_info.hostname)
                    .await;
            }
            Command::PeerError => {
                let msg = String::from_utf8_lossy(&frame.payload);
                warn!("Peer {} error: {}", peer_addr, msg);
            }
            _ => {
                debug!("Unhandled command {:?} from {}", frame.command, peer_addr);
            }
        }
    }

    info!("Connection {} closed", peer_addr);
    Ok(())
}

async fn receive_reliable_event(
    stream: &mut secure::SecureConnection,
    frame: Frame,
    source: &str,
    sync_engine: &Arc<Mutex<sync::SyncEngine>>,
    database: &Arc<Mutex<db::HistoryDB>>,
    last_sequence: &mut Option<u32>,
) -> Result<(), String> {
    let envelope = EventEnvelope::decode(&frame.payload).map_err(|error| error.to_string())?;
    envelope
        .validate_timestamp(unix_timestamp_ms())
        .map_err(|error| error.to_string())?;

    let duplicate = sync_engine
        .lock()
        .await
        .has_seen_message(source, envelope.message_id);
    if last_sequence.is_some_and(|last| frame.sequence <= last) && !duplicate {
        return Err(format!(
            "replayed or out-of-order event sequence {}",
            frame.sequence
        ));
    }

    if !duplicate {
        let kind = match frame.command {
            Command::TextPayload => "text",
            Command::ImagePayload => "image",
            _ => {
                return Err(format!(
                    "{:?} is not a reliable event command",
                    frame.command
                ))
            }
        };
        process_event_content(
            frame.command,
            &envelope.content,
            source,
            sync_engine,
            database,
        )
        .await?;
        info!("{kind} event from {source} applied");
        sync_engine
            .lock()
            .await
            .record_message(source, envelope.message_id);
        crate::api::bump_clipboard_version();
    } else {
        debug!("Reliable event from {source} was already applied; acknowledging again");
    }

    if last_sequence.is_none_or(|last| frame.sequence > last) {
        *last_sequence = Some(frame.sequence);
    }
    let ack = Frame::new(
        Command::EventAck,
        0,
        frame.sequence,
        envelope.message_id.ack_payload(),
    );
    stream
        .write_frame(&ack)
        .await
        .map_err(|error| error.to_string())
}

async fn process_event_content(
    command: Command,
    content: &[u8],
    source: &str,
    sync_engine: &Arc<Mutex<sync::SyncEngine>>,
    database: &Arc<Mutex<db::HistoryDB>>,
) -> Result<(), String> {
    match command {
        Command::TextPayload => {
            let text = String::from_utf8(content.to_vec())
                .map_err(|_| "text event is not valid UTF-8".to_string())?;
            let db = database.clone();
            let db_text = text.clone();
            let db_source = source.to_string();
            tokio::task::spawn_blocking(move || {
                db.blocking_lock()
                    .add_text(&db_text, &db_source)
                    .map_err(|error| error.to_string())
            })
            .await
            .map_err(|error| error.to_string())??;

            info!("Received text from {}: {} chars", source, text.len());
            sync_engine
                .lock()
                .await
                .handle_incoming_text(&text, source.to_string())
                .await;
        }
        Command::ImagePayload => {
            validate_packed_image(content)?;
            let db = database.clone();
            let image = content.to_vec();
            let db_source = source.to_string();
            tokio::task::spawn_blocking(move || {
                db.blocking_lock()
                    .add_image(&image, &db_source)
                    .map_err(|error| error.to_string())
            })
            .await
            .map_err(|error| error.to_string())??;

            info!("Received image from {}: {} bytes", source, content.len());
            sync_engine
                .lock()
                .await
                .handle_incoming_image(content, source.to_string())
                .await;
        }
        _ => return Err(format!("{:?} is not a reliable event command", command)),
    }
    Ok(())
}

fn validate_packed_image(content: &[u8]) -> Result<(), String> {
    if content.len() < 8 {
        return Err("image event header is incomplete".to_string());
    }
    let width = u32::from_le_bytes(content[0..4].try_into().expect("validated image header"));
    let height = u32::from_le_bytes(content[4..8].try_into().expect("validated image header"));
    let rgba_len = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "image dimensions overflow".to_string())?;
    if content.len() != 8 + rgba_len {
        return Err("image event dimensions do not match its RGBA payload".to_string());
    }
    Ok(())
}

fn source_matches_mode(ip: std::net::IpAddr, mode: &str) -> bool {
    if mode == "auto" {
        return source_matches_mode(ip, "lan_only") || source_matches_mode(ip, "tailscale_only");
    }
    match (ip, mode) {
        (std::net::IpAddr::V4(ip), "tailscale" | "tailscale_only") => {
            let octets = ip.octets();
            octets[0] == 100 && (64..=127).contains(&octets[1])
        }
        (std::net::IpAddr::V6(ip), "tailscale" | "tailscale_only") => {
            let segments = ip.segments();
            segments[0] == 0xfd7a && segments[1] == 0x115c && segments[2] == 0xa1e0
        }
        (std::net::IpAddr::V4(ip), "lan" | "lan_only") => {
            ip.is_private() || ip.is_link_local() || ip.is_loopback()
        }
        (std::net::IpAddr::V6(ip), "lan" | "lan_only") => {
            let first = ip.segments()[0];
            (first & 0xfe00) == 0xfc00 || ip.is_unicast_link_local() || ip.is_loopback()
        }
        _ => false,
    }
}

fn local_peer_identity(mode: &str) -> secure::PeerIdentity {
    // Peer authentication is bound to the Noise static key and hostname. The
    // socket address is recorded separately, so handshakes must not block on
    // spawning `tailscale status` for every connection attempt.
    let _ = mode;
    secure::PeerIdentity {
        hostname: lan::local_hostname(),
        tailscale_ip: String::new(),
    }
}

pub async fn start_pairing(
    pairing: Arc<PairingManager>,
    identity: Arc<DeviceIdentity>,
    settings: Arc<Mutex<crypto::Settings>>,
    address: &str,
) -> Result<(), String> {
    pairing.begin_handshake().await?;
    let ip: IpAddr = match address.trim().parse() {
        Ok(ip) => ip,
        Err(error) => {
            let message = format!("Invalid peer address {address}: {error}");
            pairing.record_failure(message.clone()).await;
            return Err(message);
        }
    };
    let (mode, source_allowed) = {
        let settings = settings.lock().await;
        (
            settings.connection_mode.clone(),
            source_matches_mode(ip, &settings.connection_mode),
        )
    };
    if !source_allowed {
        let message = "Peer address is outside the selected network".to_string();
        pairing.record_failure(message.clone()).await;
        return Err(message);
    }

    let socket_address = SocketAddr::new(ip, TCP_PORT);
    let mut window = pairing.subscribe_window();
    let operation = timeout(HANDSHAKE_TIMEOUT, async {
        let stream = TcpStream::connect(socket_address).await?;
        secure::connect_pairing(stream, &identity, local_peer_identity(&mode)).await
    });
    tokio::pin!(operation);
    let accepted = loop {
        tokio::select! {
            result = &mut operation => {
                break match result {
                    Ok(Ok(accepted)) => accepted,
                    Ok(Err(error)) => {
                        let message = format!("Pairing handshake failed: {error}");
                        pairing.record_failure(message.clone()).await;
                        return Err(message);
                    }
                    Err(_) => {
                        let message = "Pairing handshake timed out".to_string();
                        pairing.record_failure(message.clone()).await;
                        return Err(message);
                    }
                };
            }
            changed = window.changed() => {
                if changed.is_err() || !*window.borrow() {
                    return Err("Pairing window was closed".to_string());
                }
            }
        }
    };

    pairing
        .install_session(PendingPairing {
            connection: accepted.connection,
            hostname: accepted.peer_identity.hostname,
            remote_public_key: accepted.remote_public_key,
            handshake_hash: accepted.handshake_hash,
            address: ip.to_string(),
            interface: infer_interface(&ip.to_string())?.as_str().to_string(),
        })
        .await
}

/// Test whether a discovered peer is accepting TailSync TCP connections.
pub async fn test_connection(address: &str) -> Result<u64, String> {
    let ip: IpAddr = address
        .parse()
        .map_err(|error| format!("Invalid peer address {address}: {error}"))?;
    let addr = SocketAddr::new(ip, TCP_PORT);
    let started = tokio::time::Instant::now();
    match timeout(Duration::from_secs(3), TcpStream::connect(addr)).await {
        Ok(Ok(_)) => Ok(started.elapsed().as_millis() as u64),
        Ok(Err(error)) => Err(format!("Connection failed: {error}")),
        Err(_) => Err("Connection timed out after 3 seconds".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        bind_tcp_listener, cached_discover_peers, clear_peer_cache, deliver_pending_frame,
        merge_discovery_results, merge_lan_discovery_results, merge_paired_peers, peer_socket_addr,
        prewarm_connections, queue_pool_frame, race_connect_and_handshake, record_probe_miss,
        record_probe_success, register_active_session, route_health, secure, source_matches_mode,
        store_peer_cache, AckExpectation, ConnectionInterface, ConnectionLimiter, ConnectionPool,
        PeerCandidate, PeerRouteKey, PeerStatus, PendingFrame, PoolSender, QueuedFrame,
        ResolvedCandidate, POOL_CHANNEL_SIZE, TCP_PORT,
    };
    use crate::crypto::{self, Settings};
    use crate::identity::DeviceIdentity;
    use crate::network::tailscale::{LocalInfo, PeerInfo};
    use crate::protocol::{Command, EventEnvelope, Frame, MessageId};
    use base64::{engine::general_purpose::STANDARD, Engine};
    use std::collections::HashMap;
    use std::net::IpAddr;
    use std::sync::Arc;
    use tokio::net::TcpListener;
    use tokio::sync::{mpsc, Mutex};
    use tokio::time::{timeout, Duration};

    #[tokio::test]
    async fn listener_can_rebind_after_a_connection_closes() {
        let listener = bind_tcp_listener("127.0.0.1:0".parse().unwrap()).unwrap();
        let address = listener.local_addr().unwrap();
        let (client, accepted) =
            tokio::join!(tokio::net::TcpStream::connect(address), listener.accept());
        drop(client.unwrap());
        drop(accepted.unwrap());
        drop(listener);

        let rebound = bind_tcp_listener(address).unwrap();
        assert_eq!(rebound.local_addr().unwrap(), address);
    }

    #[test]
    fn candidate_without_heartbeat_is_discovered_not_online() {
        let health = route_health(
            "never-seen-health-test",
            ConnectionInterface::Lan,
            "192.168.250.20",
        );
        assert_eq!(health.status, PeerStatus::Discovered);
        assert!(!health.online);
        assert!(!health.connected);
    }

    #[test]
    fn heartbeat_misses_confirm_then_offline_and_recover_immediately() {
        let key = PeerRouteKey::new(
            "health-transition-test",
            ConnectionInterface::Lan,
            "192.168.250.21",
        );
        record_probe_success(&key, 11);
        let health = route_health(&key.hostname, key.interface, &key.address);
        assert_eq!(health.status, PeerStatus::Online);
        assert_eq!(health.latency_ms, Some(11));

        record_probe_miss(&key);
        let health = route_health(&key.hostname, key.interface, &key.address);
        assert_eq!(health.status, PeerStatus::Confirming);
        assert!(health.online);

        record_probe_miss(&key);
        let health = route_health(&key.hostname, key.interface, &key.address);
        assert_eq!(health.status, PeerStatus::Offline);
        assert!(!health.online);

        record_probe_success(&key, 7);
        let health = route_health(&key.hostname, key.interface, &key.address);
        assert_eq!(health.status, PeerStatus::Online);
        assert_eq!(health.latency_ms, Some(7));
    }

    #[test]
    fn lan_and_tailscale_health_are_independent() {
        let hostname = "route-independence-test";
        let lan = PeerRouteKey::new(hostname, ConnectionInterface::Lan, "192.168.250.22");
        let tailscale =
            PeerRouteKey::new(hostname, ConnectionInterface::Tailscale, "100.100.250.22");
        record_probe_success(&lan, 4);
        record_probe_miss(&tailscale);
        record_probe_miss(&tailscale);

        assert_eq!(
            route_health(hostname, lan.interface, &lan.address).status,
            PeerStatus::Online
        );
        assert_eq!(
            route_health(hostname, tailscale.interface, &tailscale.address).status,
            PeerStatus::Offline
        );
    }

    #[test]
    fn authenticated_sessions_force_connected_until_last_session_closes() {
        let hostname = "session-reference-count-test";
        let address = "100.100.250.23";
        let first = register_active_session(hostname, ConnectionInterface::Tailscale, address, 9);
        let second = register_active_session(hostname, ConnectionInterface::Tailscale, address, 8);
        let health = route_health(hostname, ConnectionInterface::Tailscale, address);
        assert_eq!(health.status, PeerStatus::Connected);
        assert!(health.connected);

        drop(first);
        assert_eq!(
            route_health(hostname, ConnectionInterface::Tailscale, address).status,
            PeerStatus::Connected
        );

        drop(second);
        let health = route_health(hostname, ConnectionInterface::Tailscale, address);
        assert_eq!(health.status, PeerStatus::Online);
        assert!(!health.connected);
    }

    #[test]
    fn connection_modes_only_accept_expected_address_ranges() {
        let tailscale: IpAddr = "100.96.1.2".parse().unwrap();
        let lan: IpAddr = "192.168.1.24".parse().unwrap();
        let public: IpAddr = "203.0.113.5".parse().unwrap();

        assert!(source_matches_mode(tailscale, "tailscale"));
        assert!(!source_matches_mode(lan, "tailscale"));
        assert!(source_matches_mode(lan, "lan"));
        assert!(!source_matches_mode(public, "lan"));
        assert!(source_matches_mode(tailscale, "auto"));
        assert!(source_matches_mode(lan, "auto"));
        assert!(!source_matches_mode(public, "auto"));
    }

    #[test]
    fn peer_socket_addr_supports_ipv6_addresses() {
        let peer = PeerInfo {
            hostname: "macbook".into(),
            tailscale_ip: "fd7a:115c:a1e0::1".into(),
            online: true,
            enabled: true,
            address: String::new(),
            connection_mode: "tailscale".into(),
            trusted: false,
            fingerprint: String::new(),
            candidates: Vec::new(),
            current_interface: None,
        };
        assert_eq!(
            peer_socket_addr(&peer).unwrap(),
            "[fd7a:115c:a1e0::1]:19890".parse().unwrap()
        );
    }

    #[test]
    fn paired_peer_with_remembered_address_survives_empty_discovery() {
        let identity = DeviceIdentity::generate_for_test();
        let mut settings = crypto::Settings {
            connection_mode: "lan".into(),
            ..Default::default()
        };
        settings
            .trusted_peer_keys
            .insert("windows".into(), STANDARD.encode(identity.public_key()));
        settings.trusted_peer_addresses.insert(
            "windows".into(),
            HashMap::from([("lan".into(), "192.168.1.20".into())]),
        );

        let peers = merge_paired_peers(&settings, "lan", Vec::new());

        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].hostname, "windows");
        assert_eq!(peers[0].address, "192.168.1.20");
        assert!(peers[0].trusted);
        assert!(!peers[0].online);
        assert_eq!(peer_socket_addr(&peers[0]).unwrap().port(), TCP_PORT);
    }

    #[test]
    fn discovered_alias_with_paired_address_is_not_listed_twice() {
        let identity = DeviceIdentity::generate_for_test();
        let mut settings = crypto::Settings {
            connection_mode: "tailscale_only".into(),
            ..Default::default()
        };
        settings
            .trusted_peer_keys
            .insert("Mac".into(), STANDARD.encode(identity.public_key()));
        settings.trusted_peer_addresses.insert(
            "Mac".into(),
            HashMap::from([("tailscale".into(), "100.111.236.101".into())]),
        );

        let peers = merge_paired_peers(
            &settings,
            "tailscale_only",
            vec![discovered_peer(
                "monet's MacBook Air",
                "100.111.236.101",
                ConnectionInterface::Tailscale,
            )],
        );

        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].hostname, "Mac");
        assert_eq!(peers[0].address, "100.111.236.101");
        assert!(peers[0].online);
        assert!(peers[0].trusted);
    }

    #[test]
    fn connection_limiter_caps_each_source_ip() {
        let limiter = ConnectionLimiter::new(64, 8);
        let first_ip: IpAddr = "192.168.1.10".parse().unwrap();
        let second_ip: IpAddr = "192.168.1.11".parse().unwrap();
        let mut permits = (0..8)
            .map(|_| limiter.try_acquire(first_ip).expect("permit"))
            .collect::<Vec<_>>();
        assert!(limiter.try_acquire(first_ip).is_none());
        assert!(limiter.try_acquire(second_ip).is_some());
        permits.pop();
        assert!(limiter.try_acquire(first_ip).is_some());
    }

    #[tokio::test]
    async fn connection_pool_reuses_sender_for_peer() {
        let identity = Arc::new(DeviceIdentity::generate_for_test());
        let settings = Arc::new(Mutex::new(crypto::Settings::default()));
        let mut pool = ConnectionPool::new(identity, settings);
        let addr = "127.0.0.1:19890".parse().unwrap();

        let first = pool.sender_for(addr, "macbook".into());
        let second = pool.sender_for(addr, "macbook".into());

        assert_eq!(pool.senders.len(), 1);
        assert!(first.same_channel(&second));
    }

    #[tokio::test]
    async fn prewarm_recreates_a_trusted_connection_after_pool_disconnect() {
        let identity = Arc::new(DeviceIdentity::generate_for_test());
        let settings = Arc::new(Mutex::new(crypto::Settings::default()));
        let pool = Arc::new(Mutex::new(ConnectionPool::new(identity, settings)));
        let mut trusted = discovered_peer(
            "prewarm-mode-switch-test",
            "192.168.252.40",
            ConnectionInterface::Lan,
        );
        trusted.trusted = true;

        prewarm_connections(pool.clone(), vec![trusted.clone()]).await;
        assert_eq!(pool.lock().await.senders.len(), 1);

        pool.lock().await.disconnect_all();
        assert!(pool.lock().await.senders.is_empty());

        prewarm_connections(pool.clone(), vec![trusted]).await;
        assert_eq!(pool.lock().await.senders.len(), 1);

        let untrusted = discovered_peer(
            "untrusted-prewarm-test",
            "192.168.252.41",
            ConnectionInterface::Lan,
        );
        pool.lock().await.disconnect_all();
        prewarm_connections(pool.clone(), vec![untrusted]).await;
        assert!(pool.lock().await.senders.is_empty());
    }

    #[tokio::test]
    async fn cached_peer_lookup_does_not_run_discovery() {
        clear_peer_cache().await;
        let local = LocalInfo {
            hostname: "local".into(),
            tailscale_ip: "127.0.0.1".into(),
            candidates: Vec::new(),
        };
        let peers = vec![PeerInfo {
            hostname: "cached-peer".into(),
            tailscale_ip: "127.0.0.2".into(),
            online: true,
            enabled: true,
            address: String::new(),
            connection_mode: "cache-test".into(),
            trusted: true,
            fingerprint: String::new(),
            candidates: Vec::new(),
            current_interface: None,
        }];
        store_peer_cache("cache-test", local, peers).await;

        // "cache-test" is not a discoverable mode, so this only succeeds on a cache hit.
        let (_, cached_peers) = cached_discover_peers("cache-test").await.unwrap();

        assert_eq!(cached_peers.len(), 1);
        assert_eq!(cached_peers[0].hostname, "cached-peer");
        clear_peer_cache().await;
    }

    fn discovered_peer(hostname: &str, address: &str, interface: ConnectionInterface) -> PeerInfo {
        PeerInfo {
            hostname: hostname.into(),
            tailscale_ip: address.into(),
            online: true,
            enabled: true,
            address: address.into(),
            connection_mode: interface.as_str().into(),
            trusted: false,
            fingerprint: String::new(),
            candidates: vec![PeerCandidate::new(interface, address)],
            current_interface: None,
        }
    }

    #[test]
    fn automatic_discovery_merges_interfaces_and_prefers_lan() {
        let lan_local = LocalInfo {
            hostname: "macbook".into(),
            tailscale_ip: "192.168.1.10".into(),
            candidates: vec![PeerCandidate::new(ConnectionInterface::Lan, "192.168.1.10")],
        };
        let tailscale_local = LocalInfo {
            hostname: "macbook".into(),
            tailscale_ip: "100.64.0.1".into(),
            candidates: vec![PeerCandidate::new(
                ConnectionInterface::Tailscale,
                "100.64.0.1",
            )],
        };
        let (local, peers) = merge_discovery_results(
            Ok((
                lan_local,
                vec![discovered_peer(
                    "windows",
                    "192.168.1.20",
                    ConnectionInterface::Lan,
                )],
            )),
            Ok((
                tailscale_local,
                vec![discovered_peer(
                    "windows",
                    "100.64.0.2",
                    ConnectionInterface::Tailscale,
                )],
            )),
        )
        .unwrap();

        assert_eq!(local.candidates.len(), 2);
        assert_eq!(local.candidates[0].interface, ConnectionInterface::Lan);
        assert_eq!(
            local.candidates[1].interface,
            ConnectionInterface::Tailscale
        );
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].address, "192.168.1.20");
        assert_eq!(peers[0].candidates.len(), 2);
        assert_eq!(peers[0].candidates[0].interface, ConnectionInterface::Lan);
        assert_eq!(
            peers[0].candidates[1].interface,
            ConnectionInterface::Tailscale
        );
    }

    #[test]
    fn automatic_discovery_survives_one_unavailable_interface() {
        let local = LocalInfo {
            hostname: "macbook".into(),
            tailscale_ip: "100.64.0.1".into(),
            candidates: vec![PeerCandidate::new(
                ConnectionInterface::Tailscale,
                "100.64.0.1",
            )],
        };
        let (_, peers) = merge_discovery_results(
            Err("UDP blocked".into()),
            Ok((
                local,
                vec![discovered_peer(
                    "windows",
                    "100.64.0.2",
                    ConnectionInterface::Tailscale,
                )],
            )),
        )
        .unwrap();

        assert_eq!(peers.len(), 1);
        assert_eq!(
            peers[0].candidates[0].interface,
            ConnectionInterface::Tailscale
        );
    }

    #[test]
    fn mdns_and_udp_results_are_deduplicated_without_losing_udp_compatibility() {
        let local = LocalInfo {
            hostname: "macbook".into(),
            tailscale_ip: "192.168.1.10".into(),
            candidates: vec![PeerCandidate::new(ConnectionInterface::Lan, "192.168.1.10")],
        };
        let udp_peer = discovered_peer("windows", "192.168.1.20", ConnectionInterface::Lan);
        let mdns_peer = udp_peer.clone();
        let (_, peers) = merge_lan_discovery_results(
            Ok((local.clone(), vec![udp_peer])),
            Ok((local, vec![mdns_peer])),
        )
        .unwrap();

        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].hostname, "windows");
        assert_eq!(peers[0].candidates.len(), 1);
        assert_eq!(peers[0].candidates[0].address, "192.168.1.20");
    }

    async fn serve_noise_once(listener: TcpListener, identity: Arc<DeviceIdentity>) {
        let (stream, _) = listener.accept().await.unwrap();
        let accepted = secure::accept(
            stream,
            &identity,
            secure::PeerIdentity {
                hostname: "server".into(),
                tailscale_ip: String::new(),
            },
        )
        .await
        .unwrap();
        let mut connection = accepted.connection;
        secure::write_ready(&mut connection).await.unwrap();
    }

    fn resolved_candidate(
        interface: ConnectionInterface,
        socket_addr: std::net::SocketAddr,
    ) -> ResolvedCandidate {
        ResolvedCandidate {
            candidate: PeerCandidate::new(interface, socket_addr.ip().to_string()),
            socket_addr,
        }
    }

    fn race_settings(server_identity: &DeviceIdentity) -> Arc<Mutex<Settings>> {
        let mut settings = Settings::default();
        settings.trusted_peer_keys.insert(
            "server".into(),
            STANDARD.encode(server_identity.public_key()),
        );
        Arc::new(Mutex::new(settings))
    }

    #[tokio::test]
    async fn connection_race_uses_reachable_lan_before_tailscale_delay() {
        let server_identity = Arc::new(DeviceIdentity::generate_for_test());
        let client_identity = Arc::new(DeviceIdentity::generate_for_test());
        let settings = race_settings(&server_identity);
        let lan_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let lan_address = lan_listener.local_addr().unwrap();
        let tailscale_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let tailscale_address = tailscale_listener.local_addr().unwrap();
        let server = tokio::spawn(serve_noise_once(lan_listener, server_identity));

        let (_, winner) = race_connect_and_handshake(
            &[
                resolved_candidate(ConnectionInterface::Lan, lan_address),
                resolved_candidate(ConnectionInterface::Tailscale, tailscale_address),
            ],
            "server",
            &client_identity,
            &settings,
        )
        .await
        .unwrap();

        assert_eq!(winner.candidate.interface, ConnectionInterface::Lan);
        assert!(
            timeout(Duration::from_millis(350), tailscale_listener.accept())
                .await
                .is_err()
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn connection_race_falls_back_to_tailscale_after_delay() {
        let server_identity = Arc::new(DeviceIdentity::generate_for_test());
        let client_identity = Arc::new(DeviceIdentity::generate_for_test());
        let settings = race_settings(&server_identity);
        let tailscale_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let tailscale_address = tailscale_listener.local_addr().unwrap();
        // Keep the LAN socket bound without accepting so another parallel test
        // cannot reuse the address while the fallback race is running.
        let unavailable = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let unavailable_address = unavailable.local_addr().unwrap();
        let server = tokio::spawn(serve_noise_once(tailscale_listener, server_identity));
        let started = tokio::time::Instant::now();

        let (_, winner) = race_connect_and_handshake(
            &[
                resolved_candidate(ConnectionInterface::Lan, unavailable_address),
                resolved_candidate(ConnectionInterface::Tailscale, tailscale_address),
            ],
            "server",
            &client_identity,
            &settings,
        )
        .await
        .unwrap();

        assert_eq!(winner.candidate.interface, ConnectionInterface::Tailscale);
        assert!(started.elapsed() >= Duration::from_millis(200));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn full_peer_queue_does_not_hold_connection_pool_lock() {
        let identity = Arc::new(DeviceIdentity::generate_for_test());
        let mut settings_value = crypto::Settings::default();
        settings_value.trusted_peer_keys.insert(
            "blocked-peer".into(),
            STANDARD.encode(DeviceIdentity::generate_for_test().public_key()),
        );
        let settings = Arc::new(Mutex::new(settings_value));
        let addr = "127.0.0.1:19890".parse().unwrap();
        let (priority, _priority_rx) = mpsc::channel(POOL_CHANNEL_SIZE);
        let (bulk, _bulk_rx) = mpsc::channel(POOL_CHANNEL_SIZE);
        for _ in 0..POOL_CHANNEL_SIZE {
            priority
                .try_send(QueuedFrame::new(Command::TextPayload, vec![1]).unwrap())
                .unwrap();
        }
        let tx = PoolSender { priority, bulk };

        let mut pool_value = ConnectionPool::new(identity, settings);
        pool_value.senders.insert((addr, "blocked-peer".into()), tx);
        let pool = Arc::new(Mutex::new(pool_value));
        let queued_pool = pool.clone();
        let blocked_send = tokio::spawn(async move {
            queue_pool_frame(
                &queued_pool,
                addr,
                "blocked-peer".into(),
                Command::TextPayload,
                vec![2],
            )
            .await
        });

        tokio::task::yield_now().await;
        let lock = timeout(Duration::from_millis(100), pool.lock())
            .await
            .expect("full peer queue held the global connection pool lock");
        drop(lock);
        blocked_send.abort();
    }

    #[tokio::test]
    async fn file_chunks_use_a_separate_queue_from_priority_messages() {
        let (priority, mut priority_rx) = mpsc::channel(1);
        let (bulk, mut bulk_rx) = mpsc::channel(1);
        let sender = PoolSender { priority, bulk };

        sender
            .channel_for(Command::FileChunk)
            .send(QueuedFrame::new(Command::FileChunk, vec![1]).unwrap())
            .await
            .unwrap();
        sender
            .channel_for(Command::TextPayload)
            .send(QueuedFrame::new(Command::TextPayload, vec![2]).unwrap())
            .await
            .unwrap();

        let priority = priority_rx.recv().await.unwrap();
        let bulk = bulk_rx.recv().await.unwrap();
        assert_eq!(priority.command, Command::TextPayload);
        assert!(matches!(priority.acknowledgement, AckExpectation::Event(_)));
        assert_eq!(bulk.command, Command::FileChunk);
        assert!(matches!(bulk.acknowledgement, AckExpectation::None));
    }

    #[tokio::test]
    async fn reliable_delivery_retries_the_same_event_until_acknowledged() {
        let server_identity = Arc::new(DeviceIdentity::generate_for_test());
        let client_identity = DeviceIdentity::generate_for_test();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let expected_key = server_identity.public_key().to_vec();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let accepted = secure::accept(
                stream,
                &server_identity,
                secure::PeerIdentity {
                    hostname: "server".into(),
                    tailscale_ip: String::new(),
                },
            )
            .await
            .unwrap();
            let mut connection = accepted.connection;
            secure::write_ready(&mut connection).await.unwrap();
            let first = connection.read_frame().await.unwrap();
            let retry = connection.read_frame().await.unwrap();
            assert_eq!(retry.sequence, first.sequence);
            assert_eq!(retry.payload, first.payload);
            let message_id = EventEnvelope::decode(&retry.payload).unwrap().message_id;
            connection
                .write_frame(&Frame::new(
                    Command::EventAck,
                    0,
                    retry.sequence,
                    message_id.ack_payload(),
                ))
                .await
                .unwrap();
        });
        let mut client = secure::connect(
            tokio::net::TcpStream::connect(address).await.unwrap(),
            &client_identity,
            secure::PeerIdentity {
                hostname: "client".into(),
                tailscale_ip: String::new(),
            },
            "server",
            &expected_key,
        )
        .await
        .unwrap();
        let pending = PendingFrame {
            queued: QueuedFrame::new(Command::TextPayload, b"reliable".to_vec()).unwrap(),
            sequence: 42,
        };

        deliver_pending_frame(&mut client, &pending).await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn reliable_delivery_rejects_an_ack_for_another_message() {
        let server_identity = Arc::new(DeviceIdentity::generate_for_test());
        let client_identity = DeviceIdentity::generate_for_test();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let expected_key = server_identity.public_key().to_vec();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let accepted = secure::accept(
                stream,
                &server_identity,
                secure::PeerIdentity {
                    hostname: "server".into(),
                    tailscale_ip: String::new(),
                },
            )
            .await
            .unwrap();
            let mut connection = accepted.connection;
            secure::write_ready(&mut connection).await.unwrap();
            let event = connection.read_frame().await.unwrap();
            connection
                .write_frame(&Frame::new(
                    Command::EventAck,
                    0,
                    event.sequence,
                    MessageId::random().ack_payload(),
                ))
                .await
                .unwrap();
        });
        let mut client = secure::connect(
            tokio::net::TcpStream::connect(address).await.unwrap(),
            &client_identity,
            secure::PeerIdentity {
                hostname: "client".into(),
                tailscale_ip: String::new(),
            },
            "server",
            &expected_key,
        )
        .await
        .unwrap();
        let pending = PendingFrame {
            queued: QueuedFrame::new(Command::TextPayload, b"reliable".to_vec()).unwrap(),
            sequence: 7,
        };

        let error = deliver_pending_frame(&mut client, &pending)
            .await
            .unwrap_err();
        assert!(error.contains("different event"));
        server.await.unwrap();
    }
}
