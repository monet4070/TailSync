use log::{debug, error, info, warn};
use socket2::{Domain, Protocol as SocketProtocol, SockAddr, Socket, Type};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot, watch, Mutex, Notify};
use tokio::time::{timeout, Duration};

use crate::crypto;
use crate::db;
use crate::identity::DeviceIdentity;
use crate::pairing::{
    PairingDirection, PairingManager, PendingPairing, RemotePairingInvite,
    RemotePairingInviteManager, DEFAULT_INVITE_TTL,
};
use crate::protocol::{Command, FileChunkPayload, FileOffset, Frame, ProtocolError, TransferId};
use crate::sync;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Default TCP port for TailSync
pub const TCP_PORT: u16 = 19890;
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(10);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const IDLE_TIMEOUT: Duration = Duration::from_secs(90);
/// Max queued frames per peer before backpressure kicks in
#[cfg(test)]
const POOL_CHANNEL_SIZE: usize = 64;
const FILE_CONFIRM_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// Reconnect back-off
const RECONNECT_DELAY: Duration = Duration::from_secs(5);
const PEER_CACHE_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const REMOTE_INVITE_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const REMOTE_INVITE_PREFACE_TIMEOUT: Duration = Duration::from_secs(5);

/// Used by the macOS SwiftUI shell to verify that the peer listener survived
/// sleep/wake transitions.
pub static TCP_SERVER_HEALTHY: AtomicBool = AtomicBool::new(false);
static PROTOCOL_COMPATIBILITY_ERRORS: OnceLock<StdMutex<HashMap<String, String>>> = OnceLock::new();

fn protocol_compatibility_errors() -> &'static StdMutex<HashMap<String, String>> {
    PROTOCOL_COMPATIBILITY_ERRORS.get_or_init(|| StdMutex::new(HashMap::new()))
}

pub(crate) fn record_protocol_compatibility_error(hostname: &str, error: &str) {
    protocol_compatibility_errors()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(hostname.to_string(), error.chars().take(500).collect());
}

pub(crate) fn clear_protocol_compatibility_error(hostname: &str) {
    protocol_compatibility_errors()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(hostname);
}

pub fn protocol_compatibility_error(hostname: &str) -> Option<String> {
    protocol_compatibility_errors()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(hostname)
        .cloned()
}

mod health;
mod iroh;
pub mod lan;
pub mod mdns;
pub(crate) use health::register_active_session;
pub use health::{
    active_routes_snapshot, apply_peer_health, record_address_test_failure,
    record_address_test_success,
};

pub use iroh::refresh_for_mode as refresh_iroh_for_mode;
mod server;
pub use iroh::start_server as start_iroh_server;

fn mode_allows_iroh(mode: &str) -> bool {
    tailsync_core::peer::types::ConnectionMode::parse(mode)
        .is_some_and(|mode| mode.allows(tailsync_core::peer::types::ConnectionInterface::Iroh))
}

pub fn local_iroh_endpoint_id(mode: &str) -> Option<String> {
    mode_allows_iroh(mode)
        .then(iroh::local_endpoint_id)
        .flatten()
}
pub use server::start_server;
#[cfg(test)]
use server::ConnectionLimiter;
#[cfg(test)]
pub(crate) use server::{
    admit_incoming_file_batch, BatchAdmission, ADMISSION_LOCK_COVERAGE_VIOLATIONS,
};
use server::{local_peer_identity, source_matches_mode};
mod pool;
use pool::wait_for_shutdown;
pub use pool::{
    acquire_peer_file_batch, prewarm_connections, queue_peer_batch_frame, queue_peer_file_frame,
    queue_peer_file_window, queue_peer_frame, queue_peer_shared_event, ConnectionPool, SharedEvent,
};
#[cfg(test)]
use pool::{
    connection_task, race_connect_and_handshake, PoolSender, QueuedFrame, ResolvedCandidate,
    ResolvedTarget,
};
mod rate_limit;
use rate_limit::check_peer_event_budget;
mod peer_cache;
#[cfg(test)]
pub(crate) use peer_cache::store_peer_cache;
pub use peer_cache::{
    cached_discover_peers, clear_peer_cache, peer_health_monitor, request_peer_refresh,
};
pub use tailsync_core::peer::directory::{
    infer_interface, merge_discovery_results, merge_lan_discovery_results, PairingTarget,
};
pub(crate) use tailsync_core::secure;

/// Platform-bound wrapper over the shared Peer Directory rules. It binds the
/// local Iroh RTT capability while keeping the caller-facing shape stable.
pub fn merge_paired_peers(
    settings: &crypto::Settings,
    mode: &str,
    discovered: Vec<tailscale::PeerInfo>,
) -> Vec<tailscale::PeerInfo> {
    for addrs in settings.trusted_peer_addresses.values() {
        if let Some(endpoint) = addrs.get("iroh") {
            iroh::remember_rtt_capability(endpoint);
        }
    }
    tailsync_core::peer::directory::merge_paired_peers(settings, mode, discovered, |endpoint_id| {
        iroh::supports_rtt(endpoint_id)
    })
}

pub mod tailscale;
mod types;
pub use types::{ConnectionInterface, PeerCandidate, PeerStatus};

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
        "iroh_only" | "iroh" => Ok((
            tailscale::LocalInfo {
                hostname: lan::local_hostname(),
                tailscale_ip: String::new(),
                candidates: local_iroh_endpoint_id(mode)
                    .map(|endpoint_id| {
                        vec![PeerCandidate::new(ConnectionInterface::Iroh, endpoint_id)]
                    })
                    .unwrap_or_default(),
            },
            Vec::new(),
        )),
        "lan_only" | "lan" => {
            let (mut local, mut peers) = discover_lan_hybrid().await?;
            local
                .candidates
                .retain(|candidate| candidate.interface == ConnectionInterface::Lan);
            for peer in &mut peers {
                peer.candidates
                    .retain(|candidate| candidate.interface == ConnectionInterface::Lan);
            }
            Ok((local, peers))
        }
        "tailscale_only" | "tailscale" => {
            let (mut local, mut peers) =
                tokio::task::spawn_blocking(tailscale::get_peers)
                    .await
                    .map_err(|error| format!("Tailscale discovery task failed: {error}"))??;
            local
                .candidates
                .retain(|candidate| candidate.interface == ConnectionInterface::Tailscale);
            for peer in &mut peers {
                peer.candidates
                    .retain(|candidate| candidate.interface == ConnectionInterface::Tailscale);
            }
            Ok((local, peers))
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

async fn discover_auto() -> Result<(tailscale::LocalInfo, Vec<tailscale::PeerInfo>), String> {
    let (lan_result, tailscale_result) = tokio::join!(
        discover_lan_hybrid(),
        tokio::task::spawn_blocking(tailscale::get_peers)
    );
    let tailscale_result =
        tailscale_result.map_err(|error| format!("Tailscale discovery task failed: {error}"))?;
    merge_discovery_results(lan_result, tailscale_result)
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

// ═══════════════════════════════════════════════════════════════════
// Connection pool — maintains a persistent TCP connection per peer
// so clipboard bursts don't pay the handshake cost on every send.
// ═══════════════════════════════════════════════════════════════════

include!("../../../../shared/platform-pairing.rs");
pub use tailsync_core::peer::types::RouteLatency;

/// Measure the latency of a discovered TailSync route. The path is "tcp" for
/// plain TCP routes and "direct" or "relay" for Iroh routes, so callers can
/// distinguish a direct connection from a cold-start relay sample.
pub async fn test_connection(address: &str) -> Result<RouteLatency, String> {
    let started = tokio::time::Instant::now();
    if let Ok(ip) = address.parse::<IpAddr>() {
        // A link-local IPv6 route cannot be dialed without an interface scope
        // id, so report that instead of attempting a scope-0 socket.
        if matches!(ip, IpAddr::V6(v6) if v6.is_unicast_link_local()) {
            return Err(
                "link-local IPv6 requires an interface scope, which this client cannot supply"
                    .to_string(),
            );
        }
        let addr = SocketAddr::new(ip, TCP_PORT);
        return match timeout(Duration::from_secs(3), TcpStream::connect(addr)).await {
            Ok(Ok(_)) => Ok(RouteLatency {
                latency_ms: started.elapsed().as_millis() as u64,
                path: "tcp".into(),
            }),
            Ok(Err(error)) => Err(format!("Connection failed: {error}")),
            Err(_) => Err("Connection timed out after 3 seconds".to_string()),
        };
    }

    let endpoint_id = tailsync_core::iroh_transport::canonical_endpoint_id(address)?;
    if !iroh::supports_rtt(&endpoint_id) {
        return Err(
            "The peer must be updated and rediscovered before Iroh latency can be tested safely"
                .to_string(),
        );
    }
    let endpoint = iroh::endpoint().await?;
    // Reuse the normal Iroh connection budget. A cold relay/direct path can
    // legitimately take longer than the three-second TCP route probe.
    let probe = match timeout(CONNECTION_TIMEOUT, endpoint.connect_rtt(&endpoint_id)).await {
        Ok(Ok(probe)) => probe,
        Ok(Err(error)) => return Err(format!("Connection failed: {error}")),
        Err(_) => {
            return Err(format!(
                "Iroh connection timed out after {} seconds",
                CONNECTION_TIMEOUT.as_secs()
            ))
        }
    };
    let sample = probe
        .measure_rtt(Duration::from_millis(500))
        .await
        .ok_or_else(|| "Iroh connection did not report route latency".to_string())?;
    let path = match sample.path {
        tailsync_core::iroh_transport::RttPath::Direct => "direct",
        tailsync_core::iroh_transport::RttPath::Relay => "relay",
    };
    Ok(RouteLatency {
        latency_ms: sample.rtt.as_millis().min(u64::MAX as u128) as u64,
        path: path.into(),
    })
}

#[cfg(test)]
mod tests;
