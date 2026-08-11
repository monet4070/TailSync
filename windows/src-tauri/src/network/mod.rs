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
pub(crate) const MAX_FILE_SIZE: u64 = 1024 * 1024 * 1024;
const PEER_CACHE_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const PEER_INITIAL_CACHE_WAIT: Duration = Duration::from_secs(2);
const PEER_MANUAL_REFRESH_WAIT: Duration = Duration::from_secs(5);
const PEER_ONLINE_TTL: Duration = Duration::from_secs(12);

/// Used by the macOS SwiftUI shell to verify that the peer listener survived
/// sleep/wake transitions.
pub static TCP_SERVER_HEALTHY: AtomicBool = AtomicBool::new(false);

mod health;
mod iroh;
pub mod lan;
pub mod mdns;
pub(crate) use health::register_active_session;
pub use health::{
    active_routes_snapshot, record_address_test_failure, record_address_test_success, route_health,
};
use health::{record_probe_miss, record_probe_success, PeerRouteKey};
pub use iroh::refresh_for_mode as refresh_iroh_for_mode;
mod server;
pub use iroh::start_server as start_iroh_server;
pub use server::start_server;
#[cfg(test)]
use server::ConnectionLimiter;
use server::{local_peer_identity, source_matches_mode};
mod pool;
use pool::wait_for_shutdown;
pub use pool::{
    acquire_peer_file_batch, prewarm_connections, queue_peer_batch_frame, queue_peer_file_frame,
    queue_peer_frame, ConnectionPool,
};
#[cfg(test)]
use pool::{
    connection_task, deliver_pending_frame, queue_pool_frame, race_connect_and_handshake,
    AckExpectation, PendingFrame, PoolSender, QueuedFrame, ResolvedCandidate, ResolvedTarget,
};
mod rate_limit;
use rate_limit::check_peer_event_budget;
mod peer_cache;
#[cfg(test)]
pub(crate) use peer_cache::store_peer_cache;
pub use peer_cache::{
    cached_discover_peers, clear_peer_cache, peer_health_monitor, request_peer_refresh,
};
pub(crate) use tailsync_core::secure;
pub mod tailscale;
mod types;
pub use types::{ActiveRoute, ConnectionInterface, PeerCandidate, PeerHealthSnapshot, PeerStatus};

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

pub async fn send_file_batch_cancel(
    pool: &Arc<Mutex<ConnectionPool>>,
    settings: &Arc<Mutex<crypto::Settings>>,
    hostname: &str,
    batch_id: TransferId,
) -> Result<(), String> {
    let snapshot = settings.lock().await.clone();
    let mode = snapshot.connection_mode.clone();
    let discovered = cached_discover_peers(&mode)
        .await
        .map(|(_, peers)| peers)
        .unwrap_or_default();
    let peer = merge_paired_peers(&snapshot, &mode, discovered)
        .into_iter()
        .find(|peer| peer.hostname == hostname)
        .ok_or_else(|| format!("Cannot find a route to cancel file batch on {hostname}"))?;
    queue_peer_frame(pool, &peer, Command::FileBatchCancel, batch_id.0.to_vec()).await
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
                for interface in [
                    ConnectionInterface::Lan,
                    ConnectionInterface::Iroh,
                    ConnectionInterface::Tailscale,
                ] {
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
        for interface in [
            ConnectionInterface::Lan,
            ConnectionInterface::Iroh,
            ConnectionInterface::Tailscale,
        ] {
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

pub async fn start_discovery_responder(
    identity: Arc<DeviceIdentity>,
    mut shutdown: watch::Receiver<bool>,
) {
    tokio::select! {
        _ = async { tokio::join!(lan::start_responder(), mdns::run(identity)); } => {}
        _ = wait_for_shutdown(&mut shutdown) => info!("Discovery responders stopped"),
    }
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

/// Test whether a discovered TailSync route accepts a transport connection.
pub async fn test_connection(address: &str) -> Result<u64, String> {
    let started = tokio::time::Instant::now();
    if let Ok(ip) = address.parse::<IpAddr>() {
        let addr = SocketAddr::new(ip, TCP_PORT);
        return match timeout(Duration::from_secs(3), TcpStream::connect(addr)).await {
            Ok(Ok(_)) => Ok(started.elapsed().as_millis() as u64),
            Ok(Err(error)) => Err(format!("Connection failed: {error}")),
            Err(_) => Err("Connection timed out after 3 seconds".to_string()),
        };
    }

    let endpoint_id = tailsync_core::iroh_transport::canonical_endpoint_id(address)?;
    let endpoint = iroh::endpoint().await?;
    match timeout(Duration::from_secs(3), endpoint.connect(&endpoint_id)).await {
        Ok(Ok(_)) => Ok(started.elapsed().as_millis() as u64),
        Ok(Err(error)) => Err(format!("Connection failed: {error}")),
        Err(_) => Err("Connection timed out after 3 seconds".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        acquire_peer_file_batch, bind_tcp_listener, cached_discover_peers, clear_peer_cache,
        connection_task, deliver_pending_frame, merge_discovery_results,
        merge_lan_discovery_results, merge_paired_peers, peer_socket_addr, prewarm_connections,
        queue_pool_frame, race_connect_and_handshake, record_probe_miss, record_probe_success,
        register_active_session, route_health, secure, source_matches_mode, store_peer_cache,
        AckExpectation, ConnectionInterface, ConnectionLimiter, ConnectionPool, PeerCandidate,
        PeerRouteKey, PeerStatus, PendingFrame, PoolSender, QueuedFrame, ResolvedCandidate,
        ResolvedTarget, POOL_CHANNEL_SIZE, TCP_PORT,
    };
    use crate::crypto::{self, Settings};
    use crate::identity::DeviceIdentity;
    use crate::network::tailscale::{LocalInfo, PeerInfo};
    use crate::protocol::{
        unix_timestamp_ms, Command, EventEnvelope, Frame, MessageId, EVENT_TIMESTAMP_WINDOW_MS,
    };
    use base64::{engine::general_purpose::STANDARD, Engine};
    use std::collections::HashMap;
    use std::net::IpAddr;
    use std::sync::Arc;
    use tokio::net::TcpListener;
    use tokio::sync::{mpsc, watch, Mutex};
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
    fn automatic_mode_adds_iroh_between_lan_and_tailscale_only() {
        let identity = DeviceIdentity::generate_for_test();
        let mut settings = crypto::Settings::default();
        settings
            .trusted_peer_keys
            .insert("windows".into(), STANDARD.encode(identity.public_key()));
        settings.trusted_peer_addresses.insert(
            "windows".into(),
            HashMap::from([
                ("lan".into(), "192.168.1.20".into()),
                (
                    "iroh".into(),
                    "5866666666666666666666666666666666666666666666666666666666666666".into(),
                ),
                ("tailscale".into(), "100.64.0.2".into()),
            ]),
        );

        let automatic = merge_paired_peers(&settings, "auto", Vec::new());
        assert_eq!(
            automatic[0]
                .candidates
                .iter()
                .map(|candidate| candidate.interface)
                .collect::<Vec<_>>(),
            vec![
                ConnectionInterface::Lan,
                ConnectionInterface::Iroh,
                ConnectionInterface::Tailscale,
            ]
        );
        let lan_only = merge_paired_peers(&settings, "lan_only", Vec::new());
        assert_eq!(lan_only[0].candidates.len(), 1);
        assert_eq!(
            lan_only[0].candidates[0].interface,
            ConnectionInterface::Lan
        );
        let tailscale_only = merge_paired_peers(&settings, "tailscale_only", Vec::new());
        assert_eq!(tailscale_only[0].candidates.len(), 1);
        assert_eq!(
            tailscale_only[0].candidates[0].interface,
            ConnectionInterface::Tailscale
        );
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

        let first = pool.sender_for(addr, "macbook".into()).unwrap();
        let second = pool.sender_for(addr, "macbook".into()).unwrap();

        assert_eq!(pool.senders.len(), 1);
        assert!(first.same_channel(&second));
    }

    #[tokio::test]
    async fn file_batches_are_serial_per_peer_and_parallel_between_peers() {
        let identity = Arc::new(DeviceIdentity::generate_for_test());
        let settings = Arc::new(Mutex::new(crypto::Settings::default()));
        let pool = Arc::new(Mutex::new(ConnectionPool::new(identity, settings)));

        let first = acquire_peer_file_batch(&pool, "peer-a").await;
        assert!(timeout(
            Duration::from_millis(20),
            acquire_peer_file_batch(&pool, "peer-a")
        )
        .await
        .is_err());
        assert!(timeout(
            Duration::from_millis(20),
            acquire_peer_file_batch(&pool, "peer-b")
        )
        .await
        .is_ok());
        drop(first);
        assert!(timeout(
            Duration::from_millis(20),
            acquire_peer_file_batch(&pool, "peer-a")
        )
        .await
        .is_ok());
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
                iroh_endpoint_id: None,
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
            target: ResolvedTarget::Tcp(socket_addr),
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
    async fn connection_worker_stops_when_the_pool_disconnects_it() {
        let server_identity = Arc::new(DeviceIdentity::generate_for_test());
        let client_identity = Arc::new(DeviceIdentity::generate_for_test());
        let settings = race_settings(&server_identity);
        let unavailable = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = unavailable.local_addr().unwrap();
        let (priority, priority_rx) = mpsc::channel(POOL_CHANNEL_SIZE);
        let (bulk, bulk_rx) = mpsc::channel(POOL_CHANNEL_SIZE);
        let (shutdown, shutdown_rx) = watch::channel(false);
        let sender = PoolSender {
            priority,
            bulk,
            shutdown,
        };
        let worker = tokio::spawn(connection_task(
            vec![resolved_candidate(ConnectionInterface::Lan, address)],
            "server".into(),
            priority_rx,
            bulk_rx,
            client_identity.clone(),
            settings.clone(),
            shutdown_rx,
        ));
        let mut pool = ConnectionPool::new(client_identity, settings);
        pool.senders
            .insert((ResolvedTarget::Tcp(address), "server".into()), sender);

        tokio::task::yield_now().await;
        pool.disconnect_all();

        timeout(Duration::from_millis(500), worker)
            .await
            .expect("connection worker ignored the pool shutdown request")
            .unwrap();
    }

    #[tokio::test]
    async fn expired_pending_event_does_not_block_a_new_event_after_reconnect() {
        let server_identity = Arc::new(DeviceIdentity::generate_for_test());
        let client_identity = Arc::new(DeviceIdentity::generate_for_test());
        let settings = race_settings(&server_identity);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (first_stream, _) = listener.accept().await.unwrap();
            let first = secure::accept(
                first_stream,
                &server_identity,
                secure::PeerIdentity {
                    hostname: "server".into(),
                    tailscale_ip: String::new(),
                    iroh_endpoint_id: None,
                },
            )
            .await
            .unwrap();
            let mut first_connection = first.connection;
            secure::write_ready(&mut first_connection).await.unwrap();
            let before_sleep = first_connection.read_frame().await.unwrap();
            let first_envelope = EventEnvelope::decode(&before_sleep.payload).unwrap();
            drop(first_connection);

            let (second_stream, _) = listener.accept().await.unwrap();
            let second = secure::accept(
                second_stream,
                &server_identity,
                secure::PeerIdentity {
                    hostname: "server".into(),
                    tailscale_ip: String::new(),
                    iroh_endpoint_id: None,
                },
            )
            .await
            .unwrap();
            let mut second_connection = second.connection;
            secure::write_ready(&mut second_connection).await.unwrap();
            let retried = second_connection.read_frame().await.unwrap();
            let retried_envelope = EventEnvelope::decode(&retried.payload).unwrap();
            assert_eq!(retried_envelope.message_id, first_envelope.message_id);
            retried_envelope
                .validate_timestamp(unix_timestamp_ms())
                .unwrap();
            second_connection
                .write_frame(
                    &Frame::try_new(
                        Command::PeerError,
                        0,
                        retried.sequence,
                        b"event timestamp outside window".to_vec(),
                    )
                    .expect("valid peer error fixture"),
                )
                .await
                .unwrap();

            let after_wake = second_connection.read_frame().await.unwrap();
            let after_wake_envelope = EventEnvelope::decode(&after_wake.payload).unwrap();
            second_connection
                .write_frame(
                    &Frame::try_new(
                        Command::EventAck,
                        0,
                        after_wake.sequence,
                        after_wake_envelope.message_id.ack_payload(),
                    )
                    .expect("valid event acknowledgement fixture"),
                )
                .await
                .unwrap();
            (
                first_envelope.content,
                retried_envelope.content,
                after_wake_envelope.content,
            )
        });

        let (priority, priority_rx) = mpsc::channel(POOL_CHANNEL_SIZE);
        let (_bulk, bulk_rx) = mpsc::channel(POOL_CHANNEL_SIZE);
        let (shutdown, shutdown_rx) = watch::channel(false);
        let worker = tokio::spawn(connection_task(
            vec![resolved_candidate(ConnectionInterface::Lan, address)],
            "server".into(),
            priority_rx,
            bulk_rx,
            client_identity,
            settings,
            shutdown_rx,
        ));

        let mut before_sleep =
            QueuedFrame::new(Command::TextPayload, b"before-sleep".to_vec()).unwrap();
        let mut stale = EventEnvelope::decode(&before_sleep.payload).unwrap();
        stale.timestamp_ms = unix_timestamp_ms() - EVENT_TIMESTAMP_WINDOW_MS - 1;
        before_sleep.payload = stale.encode();
        priority.send(before_sleep).await.unwrap();
        priority
            .send(QueuedFrame::new(Command::TextPayload, b"after-wake".to_vec()).unwrap())
            .await
            .unwrap();

        let (first, retried, delivered) = timeout(Duration::from_secs(10), server)
            .await
            .expect("new event remained blocked behind the rejected pending event")
            .unwrap();
        assert_eq!(first, b"before-sleep");
        assert_eq!(retried, b"before-sleep");
        assert_eq!(delivered, b"after-wake");
        let _ = shutdown.send(true);
        timeout(Duration::from_secs(1), worker)
            .await
            .expect("connection worker did not stop after the regression test")
            .unwrap();
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
        let (shutdown, _shutdown_rx) = watch::channel(false);
        for _ in 0..POOL_CHANNEL_SIZE {
            priority
                .try_send(QueuedFrame::new(Command::TextPayload, vec![1]).unwrap())
                .unwrap();
        }
        let tx = PoolSender {
            priority,
            bulk,
            shutdown,
        };

        let mut pool_value = ConnectionPool::new(identity, settings);
        pool_value
            .senders
            .insert((ResolvedTarget::Tcp(addr), "blocked-peer".into()), tx);
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
        let (shutdown, _shutdown_rx) = watch::channel(false);
        let sender = PoolSender {
            priority,
            bulk,
            shutdown,
        };

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
                    iroh_endpoint_id: None,
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
                .write_frame(
                    &Frame::try_new(
                        Command::EventAck,
                        0,
                        retry.sequence,
                        message_id.ack_payload(),
                    )
                    .expect("valid event acknowledgement fixture"),
                )
                .await
                .unwrap();
        });
        let mut client = secure::connect(
            tokio::net::TcpStream::connect(address).await.unwrap(),
            &client_identity,
            secure::PeerIdentity {
                hostname: "client".into(),
                tailscale_ip: String::new(),
                iroh_endpoint_id: None,
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
                    iroh_endpoint_id: None,
                },
            )
            .await
            .unwrap();
            let mut connection = accepted.connection;
            secure::write_ready(&mut connection).await.unwrap();
            let event = connection.read_frame().await.unwrap();
            connection
                .write_frame(
                    &Frame::try_new(
                        Command::EventAck,
                        0,
                        event.sequence,
                        MessageId::random().ack_payload(),
                    )
                    .expect("valid event acknowledgement fixture"),
                )
                .await
                .unwrap();
        });
        let mut client = secure::connect(
            tokio::net::TcpStream::connect(address).await.unwrap(),
            &client_identity,
            secure::PeerIdentity {
                hostname: "client".into(),
                tailscale_ip: String::new(),
                iroh_endpoint_id: None,
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
