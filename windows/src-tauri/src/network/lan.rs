use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::sync::OnceLock;
use std::time::{Duration as StdDuration, Instant as StdInstant};
use tokio::net::UdpSocket;
use tokio::time::{timeout, Duration, Instant};

use super::tailscale::{LocalInfo, PeerInfo};
use super::{ConnectionInterface, PeerCandidate, PeerStatus, TCP_PORT};
use tailsync_core::peer::discovery_admission::DiscoveryAdmission;

const DISCOVERY_PORT: u16 = 19889;
const DISCOVERY_REQUEST: &[u8] = b"TAILSYNC_DISCOVER_V1";
const DISCOVERY_WINDOW: Duration = Duration::from_millis(650);
const UNICAST_DISCOVERY_WINDOW: StdDuration = StdDuration::from_millis(300);

#[derive(Debug, Serialize, Deserialize)]
struct DiscoveryResponse {
    app: String,
    version: u8,
    hostname: String,
    tcp_port: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    iroh_endpoint_id: Option<String>,
    #[serde(default)]
    iroh_rtt: bool,
}

#[derive(Debug, Clone)]
pub(super) struct ProbeResponse {
    pub hostname: String,
    pub latency_ms: u64,
    pub iroh_endpoint_id: Option<String>,
}

fn advertised_iroh_endpoint(response: &DiscoveryResponse) -> Option<String> {
    let endpoint_id = response.iroh_endpoint_id.as_deref()?;
    let endpoint_id = tailsync_core::iroh_transport::canonical_endpoint_id(endpoint_id).ok()?;
    if response.iroh_rtt {
        super::iroh::remember_rtt_capability(&endpoint_id);
    }
    Some(endpoint_id)
}

/// Ask known overlay-network addresses which TailSync hostname they expose.
///
/// Tailscale's `HostName` is a Tailnet display name and may not match the OS
/// hostname exchanged by TailSync during pairing.  A direct UDP probe reaches
/// the existing TailSync discovery responder through Tailscale and lets us
/// associate the overlay IP with the same stable hostname used by LAN pairing.
pub(super) fn probe_hostnames(addresses: &[IpAddr]) -> HashMap<IpAddr, ProbeResponse> {
    let targets = addresses
        .iter()
        .copied()
        .filter(|address| address.is_ipv4())
        .collect::<HashSet<_>>();
    if targets.is_empty() {
        return HashMap::new();
    }

    let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") else {
        return HashMap::new();
    };
    let _ = socket.set_read_timeout(Some(StdDuration::from_millis(50)));
    let started = StdInstant::now();
    for address in &targets {
        let _ = socket.send_to(DISCOVERY_REQUEST, SocketAddr::new(*address, DISCOVERY_PORT));
    }

    let deadline = StdInstant::now() + UNICAST_DISCOVERY_WINDOW;
    let mut buffer = [0u8; 1024];
    let mut resolved = HashMap::new();
    while StdInstant::now() < deadline && resolved.len() < targets.len() {
        let Ok((length, source)) = socket.recv_from(&mut buffer) else {
            continue;
        };
        if !targets.contains(&source.ip()) {
            continue;
        }
        let Ok(response) = serde_json::from_slice::<DiscoveryResponse>(&buffer[..length]) else {
            continue;
        };
        if response.app == "tailsync"
            && response.version == 1
            && response.tcp_port == TCP_PORT
            && !response.hostname.trim().is_empty()
        {
            let iroh_endpoint_id = advertised_iroh_endpoint(&response);
            resolved.insert(
                source.ip(),
                ProbeResponse {
                    hostname: response.hostname,
                    latency_ms: started.elapsed().as_millis() as u64,
                    iroh_endpoint_id,
                },
            );
        }
    }
    resolved
}

fn eligible_lan_interface(interface: &if_addrs::Interface) -> bool {
    interface.is_oper_up() && !interface.is_loopback() && !interface.is_p2p
}

fn broadcast_targets_for(
    interfaces: impl IntoIterator<Item = if_addrs::Interface>,
) -> HashSet<SocketAddr> {
    let mut targets = HashSet::from([SocketAddr::from(([255, 255, 255, 255], DISCOVERY_PORT))]);
    for interface in interfaces {
        if !eligible_lan_interface(&interface) {
            continue;
        }
        let if_addrs::IfAddr::V4(address) = interface.addr else {
            continue;
        };
        if let Some(broadcast) = address.broadcast {
            targets.insert(SocketAddr::new(IpAddr::V4(broadcast), DISCOVERY_PORT));
        }
    }
    targets
}

fn broadcast_targets() -> HashSet<SocketAddr> {
    broadcast_targets_for(if_addrs::get_if_addrs().unwrap_or_default())
}

pub fn local_hostname() -> String {
    static LOCAL_HOSTNAME: OnceLock<String> = OnceLock::new();
    LOCAL_HOSTNAME
        .get_or_init(|| {
            if let Some(hostname) = ["COMPUTERNAME", "HOSTNAME"]
                .into_iter()
                .filter_map(|name| std::env::var(name).ok())
                .map(|value| value.trim().to_string())
                .find(|value| !value.is_empty())
            {
                return hostname;
            }

            #[cfg(unix)]
            if let Ok(output) = std::process::Command::new("/bin/hostname").output() {
                let hostname = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if output.status.success() && !hostname.is_empty() {
                    return hostname;
                }
            }

            "TailSync device".to_string()
        })
        .clone()
}

fn local_ip() -> String {
    // Derive the advertised LAN address from the machine's own interfaces.
    // A default-route probe reports a VPN/tunnel address under a full tunnel
    // and `0.0.0.0` with no default route; neither is a usable LAN address.
    let addresses = if_addrs::get_if_addrs()
        .map(|interfaces| {
            interfaces
                .into_iter()
                .filter(eligible_lan_interface)
                .map(|interface| interface.ip())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    tailsync_core::peer::directory::select_local_lan_ip(addresses)
        .map(|ip| ip.to_string())
        .unwrap_or_default()
}

pub async fn discover() -> Result<(LocalInfo, Vec<PeerInfo>), String> {
    let socket = UdpSocket::bind("0.0.0.0:0")
        .await
        .map_err(|e| format!("Failed to open LAN discovery socket: {e}"))?;
    socket
        .set_broadcast(true)
        .map_err(|e| format!("Failed to enable LAN broadcast: {e}"))?;
    discover_with_targets(&socket, broadcast_targets(), DISCOVERY_WINDOW, local_ip()).await
}

// Keep the production send/receive path testable with explicit targets and the
// interface snapshot. Tests use local UDP sockets and never probe the LAN.
async fn discover_with_targets(
    socket: &UdpSocket,
    targets: HashSet<SocketAddr>,
    window: Duration,
    local_address: String,
) -> Result<(LocalInfo, Vec<PeerInfo>), String> {
    let started = Instant::now();
    let mut sent = false;
    for target in targets {
        if socket.send_to(DISCOVERY_REQUEST, target).await.is_ok() {
            sent = true;
        }
    }
    if !sent {
        return Err("Failed to broadcast LAN discovery on active interfaces".to_string());
    }

    let hostname = local_hostname();
    let deadline = Instant::now() + window;
    let mut buffer = [0u8; 1024];
    let mut seen = HashSet::<(String, IpAddr)>::new();
    let mut peers = Vec::new();

    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        let received = timeout(remaining, socket.recv_from(&mut buffer)).await;
        let Ok(Ok((length, source))) = received else {
            break;
        };
        let Ok(response) = serde_json::from_slice::<DiscoveryResponse>(&buffer[..length]) else {
            continue;
        };
        if response.app != "tailsync" || response.version != 1 || response.hostname == hostname {
            continue;
        }
        if !seen.insert((response.hostname.clone(), source.ip())) {
            continue;
        }
        let mut lan_candidate =
            PeerCandidate::new(ConnectionInterface::Lan, source.ip().to_string());
        lan_candidate.latency = Some(started.elapsed().as_millis() as u64);
        let mut candidates = vec![lan_candidate];
        if let Some(endpoint_id) = advertised_iroh_endpoint(&response) {
            let mut candidate = PeerCandidate::new(ConnectionInterface::Iroh, endpoint_id);
            candidate.set_rtt_capable(super::iroh::supports_rtt(&candidate.address));
            candidates.push(candidate);
        }
        peers.push(PeerInfo {
            hostname: response.hostname,
            tailscale_ip: source.ip().to_string(),
            address: source.ip().to_string(),
            online: true,
            enabled: true,
            connection_mode: "lan".to_string(),
            trusted: false,
            fingerprint: String::new(),
            candidates,
            current_interface: None,
            current_address: None,
            status: PeerStatus::Online,
        });
    }

    peers.sort_by(|a, b| a.hostname.cmp(&b.hostname));
    let candidates = local_address
        .parse::<IpAddr>()
        .ok()
        .filter(|address| !address.is_unspecified())
        .map(|_| PeerCandidate::new(ConnectionInterface::Lan, local_address.clone()))
        .into_iter()
        .collect();
    Ok((
        LocalInfo {
            hostname,
            tailscale_ip: local_address,
            candidates,
        },
        peers,
    ))
}

pub async fn start_responder() {
    loop {
        let socket = match UdpSocket::bind(("0.0.0.0", DISCOVERY_PORT)).await {
            Ok(socket) => socket,
            Err(error) => {
                log::error!("LAN discovery responder failed to bind: {error}; retrying");
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };
        let response = DiscoveryResponse {
            app: "tailsync".to_string(),
            version: 1,
            hostname: local_hostname(),
            tcp_port: TCP_PORT,
            iroh_endpoint_id: super::iroh::local_endpoint_id(),
            iroh_rtt: true,
        };
        let payload = match serde_json::to_vec(&response) {
            Ok(payload) => payload,
            Err(error) => {
                log::error!("LAN discovery response encoding failed: {error}; retrying");
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };
        let mut buffer = [0u8; 128];
        let mut admission = DiscoveryAdmission::new(StdInstant::now());
        loop {
            match socket.recv_from(&mut buffer).await {
                Ok((length, source)) if &buffer[..length] == DISCOVERY_REQUEST => {
                    // Every reply passes the shared admission gate (source
                    // filter plus per-source/global budgets). Denied probes
                    // are silently dropped: per-packet denial logs would
                    // flood the log and leak source addresses.
                    if !admission
                        .should_reply(source.ip(), StdInstant::now())
                        .is_allowed()
                    {
                        continue;
                    }
                    if let Err(error) = socket.send_to(&payload, source).await {
                        log::debug!("LAN discovery response to {source} failed: {error}");
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    log::warn!("LAN discovery receive failed: {error}; rebuilding responder");
                    break;
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lan_discovery_response_fields_are_pinned() {
        // Part of the accepted anonymous-visible set; the LAN answer is readable by
        // anyone on the link before any handshake. See
        // docs/security/anonymous-visible-fields.md.
        let response = DiscoveryResponse {
            app: "tailsync".into(),
            version: 1,
            hostname: "host".into(),
            tcp_port: 19890,
            iroh_endpoint_id: Some("endpoint-id".into()),
            iroh_rtt: true,
        };
        let value = serde_json::to_value(&response).unwrap();
        let mut keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "app",
                "hostname",
                "iroh_endpoint_id",
                "iroh_rtt",
                "tcp_port",
                "version"
            ]
        );
    }

    // S1-P1-5: the target set always contains the global broadcast address, so
    // "no broadcast targets" is not a reachable state. A caller therefore sees
    // only two outcomes: at least one send succeeded, or every send failed.
    #[test]
    fn broadcast_targets_always_include_the_global_broadcast() {
        let targets = broadcast_targets();
        assert!(
            targets.contains(&SocketAddr::from(([255, 255, 255, 255], DISCOVERY_PORT))),
            "the global broadcast address must always be a target: {targets:?}"
        );
        assert!(
            !targets.iter().any(|target| target.ip().is_loopback()),
            "loopback must never be a discovery target: {targets:?}"
        );
        for target in &targets {
            assert_eq!(
                target.port(),
                DISCOVERY_PORT,
                "unexpected port in {target:?}"
            );
        }
    }

    fn interface(ip: [u8; 4], broadcast: [u8; 4], up: bool, p2p: bool) -> if_addrs::Interface {
        if_addrs::Interface {
            name: "fixture".into(),
            index: None,
            addr: if_addrs::IfAddr::V4(if_addrs::Ifv4Addr {
                ip: ip.into(),
                netmask: [255, 255, 255, 0].into(),
                prefixlen: 24,
                broadcast: Some(broadcast.into()),
            }),
            oper_status: if up {
                if_addrs::IfOperStatus::Up
            } else {
                if_addrs::IfOperStatus::Down
            },
            is_p2p: p2p,
            #[cfg(windows)]
            adapter_name: "fixture".into(),
        }
    }

    #[tokio::test]
    async fn lan_discovery_filters_interfaces_and_distinguishes_send_failure_from_zero_peers() {
        let global = SocketAddr::from(([255, 255, 255, 255], DISCOVERY_PORT));
        let rejected = vec![
            interface([10, 0, 0, 2], [10, 0, 0, 255], false, false),
            interface([172, 16, 0, 2], [172, 16, 0, 255], true, true),
            interface([127, 0, 0, 1], [127, 255, 255, 255], true, false),
        ];
        assert!(rejected.iter().all(|i| !eligible_lan_interface(i)));
        assert_eq!(broadcast_targets_for(rejected), HashSet::from([global]));
        let valid = interface([192, 168, 1, 2], [192, 168, 1, 255], true, false);
        assert!(eligible_lan_interface(&valid));
        assert_eq!(
            broadcast_targets_for([valid]),
            HashSet::from([
                global,
                SocketAddr::from(([192, 168, 1, 255], DISCOVERY_PORT)),
            ])
        );
        // A successful UDP send with no responder is Ok with zero devices.
        // No eligible interface is represented by an empty local address.
        let sink = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let (local, peers) = discover_with_targets(
            &socket,
            HashSet::from([sink.local_addr().unwrap()]),
            Duration::from_millis(10),
            String::new(),
        )
        .await
        .unwrap();
        assert!(local.tailscale_ip.is_empty() && local.candidates.is_empty());
        assert!(peers.is_empty());
        // An IPv4 socket cannot send to this IPv6 target; every send fails.
        let error = discover_with_targets(
            &socket,
            HashSet::from(["[::1]:19889".parse().unwrap()]),
            Duration::from_millis(10),
            String::new(),
        )
        .await
        .unwrap_err();
        assert!(error.contains("Failed to broadcast LAN discovery"));
    }
}
