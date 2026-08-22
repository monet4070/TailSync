use log::{debug, error, info, warn};
use socket2::{Domain, Protocol as SocketProtocol, SockAddr, Socket, Type};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot, watch, Mutex, Notify, RwLock};
use tokio::time::{timeout, Duration};

use crate::crypto;
use crate::db;
use crate::identity::DeviceIdentity;
use crate::pairing::{PairingManager, PendingPairing};
use crate::protocol::{Command, FileChunkPayload, FileOffset, Frame, ProtocolError, TransferId};
use crate::sync;

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
pub(crate) use tailsync_core::sync::MAX_FILE_SIZE;
const PEER_CACHE_REFRESH_INTERVAL: Duration = Duration::from_secs(5);

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
pub use health::{
    active_routes_snapshot, apply_peer_health, record_address_test_failure,
    record_address_test_success,
};
use health::{
    clear_peer_health, register_active_session, update_peer_health,
    update_peer_health_for_failed_round,
};
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
    cached_discover_peers, clear_peer_cache, peer_cache_refresh_loop, request_peer_refresh_and_wait,
};
pub use tailsync_core::peer::directory::{
    infer_interface, merge_discovery_results, merge_lan_discovery_results, mode_interface,
    PairingTarget,
};
pub(crate) use tailsync_core::secure;

/// Platform-bound wrappers over the shared Peer Directory rules: they bind
/// platform capabilities (Iroh RTT probing) and constants (peer TCP port)
/// while keeping the external signatures stable for callers.
pub fn merge_paired_peers(
    settings: &crypto::Settings,
    mode: &str,
    discovered: Vec<tailscale::PeerInfo>,
) -> Vec<tailscale::PeerInfo> {
    tailsync_core::peer::directory::merge_paired_peers(settings, mode, discovered, |endpoint_id| {
        iroh::supports_rtt(endpoint_id)
    })
}

pub fn peer_socket_addr(peer: &tailscale::PeerInfo) -> Result<SocketAddr, String> {
    tailsync_core::peer::directory::peer_socket_addr(peer, TCP_PORT)
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
            let (local, mut peers) = discover_lan_hybrid().await?;
            for peer in &mut peers {
                peer.candidates
                    .retain(|candidate| candidate.interface == ConnectionInterface::Lan);
            }
            Ok((local, peers))
        }
        "tailscale_only" | "tailscale" => {
            let (local, mut peers) = discover_tailscale().await?;
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

async fn discover_tailscale() -> Result<(tailscale::LocalInfo, Vec<tailscale::PeerInfo>), String> {
    let (local, peers) = tokio::task::spawn_blocking(tailscale::get_peers)
        .await
        .map_err(|error| format!("Tailscale discovery task failed: {error}"))??;
    let addresses = peers
        .iter()
        .map(|peer| peer.address.clone())
        .filter(|address| !address.is_empty())
        .collect::<Vec<_>>();
    let responses = match lan::probe_addresses(addresses, ConnectionInterface::Tailscale).await {
        Ok(responses) => responses,
        Err(error) => {
            debug!("Tailscale UDP heartbeat failed: {error}");
            Vec::new()
        }
    };
    Ok((local, merge_tailscale_heartbeat(peers, responses)))
}

fn merge_tailscale_heartbeat(
    mut peers: Vec<tailscale::PeerInfo>,
    responses: Vec<tailscale::PeerInfo>,
) -> Vec<tailscale::PeerInfo> {
    for peer in &mut peers {
        peer.online = false;
        peer.status = PeerStatus::Discovered;
        for candidate in &mut peer.candidates {
            candidate.online = false;
            candidate.status = PeerStatus::Discovered;
            candidate.latency = None;
        }
    }
    for response in responses {
        if let Some(peer) = peers
            .iter_mut()
            .find(|peer| peer.hostname == response.hostname || peer.address == response.address)
        {
            peer.online = true;
            peer.status = PeerStatus::Online;
            for response_candidate in response.candidates {
                if let Some(candidate) = peer.candidates.iter_mut().find(|candidate| {
                    candidate.interface == response_candidate.interface
                        && candidate.address == response_candidate.address
                }) {
                    *candidate = response_candidate;
                } else {
                    peer.candidates.push(response_candidate);
                }
            }
        } else {
            peers.push(response);
        }
    }
    peers
}

async fn discover_lan_hybrid() -> Result<(tailscale::LocalInfo, Vec<tailscale::PeerInfo>), String> {
    let udp_result = lan::discover().await;
    let mdns_result = Ok(mdns::snapshot());
    merge_lan_discovery_results(udp_result, mdns_result)
}

async fn discover_auto() -> Result<(tailscale::LocalInfo, Vec<tailscale::PeerInfo>), String> {
    let (lan_result, tailscale_result) = tokio::join!(discover_lan_hybrid(), discover_tailscale());
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
    let probe = match timeout(Duration::from_secs(3), endpoint.connect_rtt(&endpoint_id)).await {
        Ok(Ok(probe)) => probe,
        Ok(Err(error)) => return Err(format!("Connection failed: {error}")),
        Err(_) => return Err("Connection timed out after 3 seconds".to_string()),
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
mod tests {
    use super::{
        acquire_peer_file_batch, bind_tcp_listener, cached_discover_peers, clear_peer_cache,
        connection_task, merge_tailscale_heartbeat, queue_peer_frame, race_connect_and_handshake,
        record_protocol_compatibility_error, secure, store_peer_cache, ConnectionInterface,
        ConnectionLimiter, ConnectionPool, PeerCandidate, PeerStatus, PoolSender, QueuedFrame,
        ResolvedCandidate, ResolvedTarget, POOL_CHANNEL_SIZE,
    };
    use crate::crypto::{self, Settings};
    use crate::identity::DeviceIdentity;
    use crate::network::tailscale::{LocalInfo, PeerInfo};
    use crate::protocol::{unix_timestamp_ms, Command, EventEnvelope, Frame, MessageId};
    use base64::{engine::general_purpose::STANDARD, Engine};
    use std::net::IpAddr;
    use std::sync::Arc;
    use tailsync_core::peer::delivery::AckExpectation;
    use tailsync_core::peer::delivery::{deliver_pending_frame, PendingFrame};
    use tokio::net::TcpListener;
    use tokio::sync::{mpsc, watch, Mutex};
    use tokio::time::{timeout, Duration};

    #[test]
    fn protocol_compatibility_diagnostic_is_recorded_and_cleared() {
        let hostname = format!("compatibility-test-{}", rand::random::<u64>());
        let message = "Incompatible TailSync protocol: peer uses v2";
        record_protocol_compatibility_error(&hostname, message);
        assert_eq!(
            super::protocol_compatibility_error(&hostname).as_deref(),
            Some(message)
        );
        super::clear_protocol_compatibility_error(&hostname);
        assert_eq!(super::protocol_compatibility_error(&hostname), None);
    }

    #[tokio::test]
    async fn listener_can_rebind_after_a_connection_closes() {
        // Let the OS choose the port so this test never collides with a
        // running daemon or another test process.
        let listener =
            bind_tcp_listener(([127, 0, 0, 1], 0).into()).expect("a free ephemeral test port");
        let address = listener.local_addr().unwrap();
        let (client, accepted) =
            tokio::join!(tokio::net::TcpStream::connect(address), listener.accept());
        drop(client.unwrap());
        let (mut accepted, _) = accepted.unwrap();
        let mut eof = [0_u8; 1];
        assert_eq!(
            tokio::io::AsyncReadExt::read(&mut accepted, &mut eof)
                .await
                .unwrap(),
            0
        );
        drop(accepted);
        drop(listener);

        let rebound = bind_tcp_listener(address).unwrap();
        assert_eq!(rebound.local_addr().unwrap(), address);
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
    async fn connection_pool_rebuilds_a_dead_cached_sender() {
        // Regression: if a worker task has exited it dropped its receivers, so
        // the cached sender's channels read as closed. Reusing it would
        // silently black-hole every frame; sender_for must rebuild instead.
        let identity = Arc::new(DeviceIdentity::generate_for_test());
        let settings = Arc::new(Mutex::new(crypto::Settings::default()));
        let mut pool = ConnectionPool::new(identity, settings);
        let addr: std::net::SocketAddr = "127.0.0.1:19890".parse().unwrap();

        // A sender whose worker has "exited": drop both receivers so the
        // channels report closed, then seed it into the cache under the key
        // sender_for computes for this (target, hostname).
        let (priority, priority_rx) = mpsc::channel::<QueuedFrame>(POOL_CHANNEL_SIZE);
        let (bulk, bulk_rx) = mpsc::channel::<QueuedFrame>(POOL_CHANNEL_SIZE);
        let (shutdown, _shutdown_rx) = watch::channel(false);
        drop(priority_rx);
        drop(bulk_rx);
        let dead = PoolSender {
            priority,
            bulk,
            shutdown,
        };
        assert!(dead.priority.is_closed() && dead.bulk.is_closed());
        pool.senders
            .insert((ResolvedTarget::Tcp(addr), "macbook".into()), dead.clone());

        let rebuilt = pool.sender_for(addr, "macbook".into()).unwrap();

        assert_eq!(pool.senders.len(), 1, "the dead entry must be replaced");
        assert!(
            !dead.same_channel(&rebuilt),
            "sender_for must hand back a freshly built worker, not the dead one"
        );
        assert!(
            !rebuilt.priority.is_closed(),
            "the rebuilt worker's channel must be live"
        );
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
            current_address: None,
            status: Default::default(),
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
            current_address: None,
            status: PeerStatus::Online,
        }
    }

    #[test]
    fn tailscale_status_requires_a_tailsync_udp_response_to_be_online() {
        let base = discovered_peer("windows", "100.64.0.2", ConnectionInterface::Tailscale);
        let without_heartbeat = merge_tailscale_heartbeat(vec![base.clone()], Vec::new());
        assert!(!without_heartbeat[0].online);
        assert_eq!(without_heartbeat[0].status, PeerStatus::Discovered);

        let with_heartbeat = merge_tailscale_heartbeat(vec![base.clone()], vec![base]);
        assert!(with_heartbeat[0].online);
        assert_eq!(with_heartbeat[0].status, PeerStatus::Online);
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
        let (shutdown, _shutdown_rx) = watch::channel(false);
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
        let peer = PeerInfo {
            hostname: "blocked-peer".into(),
            tailscale_ip: addr.ip().to_string(),
            online: true,
            enabled: true,
            address: addr.ip().to_string(),
            connection_mode: "lan".into(),
            trusted: true,
            fingerprint: String::new(),
            candidates: vec![PeerCandidate::new(
                ConnectionInterface::Lan,
                addr.ip().to_string(),
            )],
            current_interface: None,
            current_address: None,
            status: PeerStatus::Online,
        };
        let blocked_send = tokio::spawn(async move {
            queue_peer_frame(&queued_pool, &peer, Command::TextPayload, vec![2]).await
        });

        tokio::task::yield_now().await;
        let lock = timeout(Duration::from_millis(100), pool.lock())
            .await
            .expect("full peer queue held the global connection pool lock");
        drop(lock);
        blocked_send.abort();
    }

    #[tokio::test]
    async fn connection_worker_stops_when_the_pool_disconnects_it() {
        let server_identity = DeviceIdentity::generate_for_test();
        let client_identity = Arc::new(DeviceIdentity::generate_for_test());
        let settings = race_settings(&server_identity);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (_priority_tx, priority_rx) = mpsc::channel(1);
        let (_bulk_tx, bulk_rx) = mpsc::channel(1);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let worker = tokio::spawn(connection_task(
            vec![resolved_candidate(ConnectionInterface::Lan, address)],
            "server".into(),
            priority_rx,
            bulk_rx,
            client_identity,
            settings,
            shutdown_rx,
        ));

        tokio::task::yield_now().await;
        shutdown_tx.send(true).unwrap();

        timeout(Duration::from_millis(250), worker)
            .await
            .expect("connection worker ignored pool shutdown")
            .unwrap();
        drop(listener);
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
        assert_eq!(priority.command(), Command::TextPayload);
        assert!(matches!(
            priority.acknowledgement(),
            AckExpectation::Event(_)
        ));
        assert_eq!(bulk.command(), Command::FileChunk);
        assert!(matches!(bulk.acknowledgement(), AckExpectation::None));
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
        let pending = PendingFrame::new(
            QueuedFrame::new(Command::TextPayload, b"reliable".to_vec()).unwrap(),
            42,
        );

        deliver_pending_frame(
            &mut client,
            &pending,
            &tailsync_core::peer::delivery::DeliveryConfig::DEFAULT,
        )
        .await
        .unwrap();
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
        let pending = PendingFrame::new(
            QueuedFrame::new(Command::TextPayload, b"reliable".to_vec()).unwrap(),
            7,
        );

        let error = deliver_pending_frame(
            &mut client,
            &pending,
            &tailsync_core::peer::delivery::DeliveryConfig::DEFAULT,
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("different event"));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn fifteen_minute_old_event_is_not_revived_after_reconnect() {
        let server_identity = Arc::new(DeviceIdentity::generate_for_test());
        let client_identity = Arc::new(DeviceIdentity::generate_for_test());
        let settings = race_settings(&server_identity);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();

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
            let first_envelope = EventEnvelope::decode(&first.payload).unwrap();

            let second = if first_envelope
                .validate_timestamp(unix_timestamp_ms())
                .is_err()
            {
                secure::write_error(
                    &mut connection,
                    "event timestamp is outside the accepted window",
                )
                .await
                .unwrap();
                drop(connection);

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
                connection.read_frame().await.unwrap()
            } else {
                connection
                    .write_frame(
                        &Frame::try_new(
                            Command::EventAck,
                            0,
                            first.sequence,
                            first_envelope.message_id.ack_payload(),
                        )
                        .expect("valid event acknowledgement fixture"),
                    )
                    .await
                    .unwrap();
                connection.read_frame().await.unwrap()
            };
            EventEnvelope::decode(&second.payload).unwrap().content
        });

        let (priority_tx, priority_rx) = mpsc::channel(4);
        let (_bulk_tx, bulk_rx) = mpsc::channel(4);
        let old_envelope = EventEnvelope {
            message_id: MessageId::random(),
            timestamp_ms: unix_timestamp_ms() - 15 * 60 * 1000,
            content: b"before-sleep".to_vec(),
        };
        priority_tx
            .send(QueuedFrame::new_with_envelope(Command::TextPayload, old_envelope).unwrap())
            .await
            .unwrap();
        priority_tx
            .send(QueuedFrame::new(Command::TextPayload, b"after-wake".to_vec()).unwrap())
            .await
            .unwrap();

        let (_shutdown_tx, shutdown_rx) = watch::channel(false);
        let worker = tokio::spawn(connection_task(
            vec![resolved_candidate(ConnectionInterface::Lan, address)],
            "server".into(),
            priority_rx,
            bulk_rx,
            client_identity,
            settings,
            shutdown_rx,
        ));
        let delivered = timeout(Duration::from_secs(3), server)
            .await
            .unwrap()
            .unwrap();
        worker.abort();

        assert_eq!(delivered, b"after-wake");
    }
}
