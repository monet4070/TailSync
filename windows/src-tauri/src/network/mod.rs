use log::{debug, error, info, warn};
use socket2::{Domain, Protocol as SocketProtocol, SockAddr, Socket, Type};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use tauri::{AppHandle, Emitter};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot, watch, Mutex, Notify, RwLock};
use tokio::time::{timeout, Duration};

use crate::crypto;
use crate::db;
use crate::identity::DeviceIdentity;
use crate::pairing::{
    PairingManager, PendingPairing, RemotePairingInvite, RemotePairingInviteManager,
    DEFAULT_INVITE_TTL,
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
const POOL_CHANNEL_SIZE: usize = 64;
const POOL_SEND_TIMEOUT: Duration = Duration::from_secs(5);
const FILE_CONFIRM_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// Reconnect back-off
const RECONNECT_DELAY: Duration = Duration::from_secs(5);
const PEER_CACHE_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const PEER_INITIAL_CACHE_WAIT: Duration = Duration::from_secs(2);
const PEER_MANUAL_REFRESH_WAIT: Duration = Duration::from_secs(5);
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
use health::{record_probe_round, PeerRouteKey};
pub use iroh::refresh_for_mode as refresh_iroh_for_mode;
mod server;
pub use iroh::start_server as start_iroh_server;

pub fn local_iroh_endpoint_id(mode: &str) -> Option<String> {
    (mode == "auto").then(iroh::local_endpoint_id).flatten()
}
pub use server::start_server;
#[cfg(test)]
use server::ConnectionLimiter;
use server::{local_peer_identity, source_matches_mode};
mod pool;
use pool::wait_for_shutdown;
pub use pool::{
    acquire_peer_file_batch, prewarm_connections, queue_peer_batch_frame, queue_peer_file_frame,
    queue_peer_frame, queue_peer_shared_event, ConnectionPool, SharedEvent,
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
    infer_interface, merge_discovery_results, merge_lan_discovery_results, mode_interface,
    PairingTarget,
};
pub(crate) use tailsync_core::secure;

/// Platform-bound wrapper over the shared Peer Directory rules. It binds the
/// local Iroh RTT capability while keeping the caller-facing shape stable.
pub fn merge_paired_peers(
    settings: &crypto::Settings,
    mode: &str,
    discovered: Vec<tailscale::PeerInfo>,
) -> Vec<tailscale::PeerInfo> {
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

pub async fn start_pairing(
    pairing: Arc<PairingManager>,
    identity: Arc<DeviceIdentity>,
    settings: Arc<Mutex<crypto::Settings>>,
    address: &str,
) -> Result<(), String> {
    let target = tailsync_core::peer::directory::parse_pairing_target(address)?;
    let mode = settings.lock().await.connection_mode.clone();
    if let Err(message) = tailsync_core::peer::directory::validate_pairing_target(&target, &mode) {
        pairing.record_failure(message.clone()).await;
        return Err(message);
    }
    pairing
        .begin_handshake()
        .await
        .map_err(|error| error.to_string())?;

    let mut window = pairing.subscribe_window();
    let operation = timeout(HANDSHAKE_TIMEOUT, async {
        match &target {
            PairingTarget::Tcp(ip) => {
                let stream = TcpStream::connect(SocketAddr::new(*ip, TCP_PORT))
                    .await
                    .map_err(|error| error.to_string())?;
                let accepted =
                    secure::connect_pairing(stream, &identity, local_peer_identity(&mode))
                        .await
                        .map_err(|error| error.to_string())?;
                Ok((accepted, ip.to_string(), infer_interface(&ip.to_string())?))
            }
            PairingTarget::Iroh(endpoint_id) => {
                let endpoint = iroh::endpoint().await?;
                let stream = endpoint.connect(endpoint_id).await?;
                let accepted =
                    secure::connect_pairing(stream, &identity, local_peer_identity("auto"))
                        .await
                        .map_err(|error| error.to_string())?;
                let claimed = accepted
                    .peer_identity
                    .iroh_endpoint_id
                    .as_deref()
                    .ok_or("Peer did not bind its Noise identity to an Iroh endpoint")?;
                if tailsync_core::iroh_transport::canonical_endpoint_id(claimed)? != *endpoint_id {
                    return Err("Peer Iroh endpoint does not match its Noise identity".to_string());
                }
                iroh::remember_rtt_capability(endpoint_id);
                Ok((accepted, endpoint_id.clone(), ConnectionInterface::Iroh))
            }
        }
    });
    tokio::pin!(operation);
    let (accepted, pairing_address, pairing_interface) = loop {
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
            address: pairing_address,
            interface: pairing_interface.as_str().to_string(),
            remote_invite: None,
        })
        .await
        .map_err(|error| error.to_string())
}

/// Create a self-contained, one-time invite. The endpoint is started here so
/// the displayed ID is backed by the live listener even when the server was
/// just switched to automatic mode.
pub async fn create_remote_pairing_invite(
    pairing: Arc<PairingManager>,
    settings: Arc<Mutex<crypto::Settings>>,
    invites: Arc<RemotePairingInviteManager>,
) -> Result<RemotePairingInvite, String> {
    if settings.lock().await.connection_mode != "auto" {
        return Err("Remote Iroh pairing requires automatic connection mode".to_string());
    }
    let endpoint = iroh::endpoint().await?;
    let invite = invites
        .create_from_endpoint_id(&endpoint.endpoint_id(), DEFAULT_INVITE_TTL)
        .map_err(|error| error.to_string())?;
    pairing.enable().await;
    Ok(invite)
}

/// Consume a remote invite on the new device, then enter the existing Noise
/// XX verification flow. The invite only authorizes reaching this pairing
/// handshake; it never replaces the fingerprint/code confirmation step.
pub async fn start_remote_pairing(
    pairing: Arc<PairingManager>,
    identity: Arc<DeviceIdentity>,
    settings: Arc<Mutex<crypto::Settings>>,
    link: &str,
) -> Result<(), String> {
    let invite = RemotePairingInvite::parse(link).map_err(|error| error.to_string())?;
    if settings.lock().await.connection_mode != "auto" {
        return Err("Remote Iroh pairing requires automatic connection mode".to_string());
    }
    pairing.enable().await;
    pairing
        .begin_handshake()
        .await
        .map_err(|error| error.to_string())?;

    let mut window = pairing.subscribe_window();
    let operation = async {
        let endpoint = iroh::endpoint().await?;
        let mut stream = timeout(
            REMOTE_INVITE_CONNECT_TIMEOUT,
            endpoint.connect_invite(&invite.endpoint_id_string()),
        )
        .await
        .map_err(|_| "Remote Iroh invite connection timed out".to_string())??;
        let hello = invite.hello().encode();
        timeout(REMOTE_INVITE_PREFACE_TIMEOUT, stream.write_all(&hello))
            .await
            .map_err(|_| "Remote Iroh invite preface timed out".to_string())?
            .map_err(|error| error.to_string())?;
        timeout(REMOTE_INVITE_PREFACE_TIMEOUT, stream.flush())
            .await
            .map_err(|_| "Remote Iroh invite preface timed out".to_string())?
            .map_err(|error| error.to_string())?;
        let mut ack = [0_u8; 1];
        timeout(REMOTE_INVITE_PREFACE_TIMEOUT, stream.read_exact(&mut ack))
            .await
            .map_err(|_| "Remote Iroh invite acknowledgement timed out".to_string())?
            .map_err(|error| error.to_string())?;
        if ack[0] != tailsync_core::pairing::invite::INVITE_ACK_ACCEPTED {
            return Err(
                "The remote pairing invite was rejected or is no longer available".to_string(),
            );
        }
        let accepted = secure::connect_pairing(stream, &identity, local_peer_identity("auto"))
            .await
            .map_err(|error| error.to_string())?;
        let claimed = accepted
            .peer_identity
            .iroh_endpoint_id
            .as_deref()
            .ok_or("Peer did not bind its Noise identity to an Iroh endpoint")?;
        if tailsync_core::iroh_transport::canonical_endpoint_id(claimed)?
            != invite.endpoint_id_string()
        {
            return Err("Peer Iroh endpoint does not match the invite".to_string());
        }
        iroh::remember_rtt_capability(&invite.endpoint_id_string());
        Ok::<_, String>((accepted, invite.endpoint_id_string()))
    };
    tokio::pin!(operation);
    let (accepted, pairing_address) = loop {
        tokio::select! {
            result = &mut operation => {
                break match result {
                    Ok(accepted) => accepted,
                    Err(error) => {
                        let message = format!("Remote pairing failed: {error}");
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
            address: pairing_address,
            interface: ConnectionInterface::Iroh.as_str().to_string(),
            remote_invite: None,
        })
        .await
        .map_err(|error| error.to_string())
}

pub use tailsync_core::peer::types::RouteLatency;

/// Measure the latency of a discovered TailSync route. The path is "tcp" for
/// plain TCP routes and "direct" or "relay" for Iroh routes, so callers can
/// distinguish a direct connection from a cold-start relay sample.
pub async fn test_connection(address: &str) -> Result<RouteLatency, String> {
    let started = tokio::time::Instant::now();
    if let Ok(ip) = address.parse::<IpAddr>() {
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
