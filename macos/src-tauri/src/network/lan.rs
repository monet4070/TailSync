use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use tokio::net::UdpSocket;
use tokio::time::{timeout, Duration, Instant};

use super::tailscale::{LocalInfo, PeerInfo};
use super::{ConnectionInterface, PeerCandidate, PeerStatus, TCP_PORT};

const DISCOVERY_PORT: u16 = 19889;
const DISCOVERY_REQUEST: &[u8] = b"TAILSYNC_DISCOVER_V1";
const DISCOVERY_WINDOW: Duration = Duration::from_millis(650);

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

fn advertised_iroh_endpoint(response: &DiscoveryResponse) -> Option<String> {
    let endpoint_id = response.iroh_endpoint_id.as_deref()?;
    let endpoint_id = tailsync_core::iroh_transport::canonical_endpoint_id(endpoint_id).ok()?;
    if response.iroh_rtt {
        super::iroh::remember_rtt_capability(&endpoint_id);
    }
    Some(endpoint_id)
}

pub fn local_hostname() -> String {
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
}

fn local_ip() -> String {
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|socket| {
            socket.connect("8.8.8.8:80")?;
            socket.local_addr()
        })
        .map(|address| address.ip().to_string())
        .unwrap_or_else(|_| "0.0.0.0".to_string())
}

fn local_addresses() -> HashSet<IpAddr> {
    if_addrs::get_if_addrs()
        .map(|interfaces| {
            interfaces
                .into_iter()
                .map(|interface| interface.ip())
                .collect()
        })
        .unwrap_or_default()
}

fn is_remote_response(
    response: &DiscoveryResponse,
    source_ip: IpAddr,
    hostname: &str,
    local_ips: &HashSet<IpAddr>,
) -> bool {
    response.app == "tailsync"
        && response.version == 1
        && response.tcp_port == TCP_PORT
        && response.hostname != hostname
        && !local_ips.contains(&source_ip)
}

fn lan_broadcast_targets() -> Vec<SocketAddr> {
    let mut targets = HashSet::from([SocketAddr::from((Ipv4Addr::BROADCAST, DISCOVERY_PORT))]);
    if let Ok(interfaces) = if_addrs::get_if_addrs() {
        for interface in interfaces {
            if !interface.is_oper_up() || interface.is_loopback() || interface.is_p2p {
                continue;
            }
            let if_addrs::IfAddr::V4(address) = interface.addr else {
                continue;
            };
            if let Some(broadcast) = address.broadcast {
                targets.insert(SocketAddr::from((broadcast, DISCOVERY_PORT)));
            }
        }
    }
    targets.into_iter().collect()
}

async fn probe_targets(
    targets: Vec<SocketAddr>,
    interface: ConnectionInterface,
) -> Result<Vec<PeerInfo>, String> {
    let socket = UdpSocket::bind("0.0.0.0:0")
        .await
        .map_err(|e| format!("Failed to open LAN discovery socket: {e}"))?;
    socket
        .set_broadcast(true)
        .map_err(|e| format!("Failed to enable LAN broadcast: {e}"))?;
    let started = Instant::now();
    let mut sent = 0usize;
    let mut last_error = None;
    for target in targets {
        match socket.send_to(DISCOVERY_REQUEST, target).await {
            Ok(_) => sent += 1,
            Err(error) => last_error = Some(error),
        }
    }
    if sent == 0 {
        return Err(format!(
            "Failed to send TailSync discovery probe: {}",
            last_error
                .map(|error| error.to_string())
                .unwrap_or_else(|| "no usable target addresses".to_string())
        ));
    }

    let hostname = local_hostname();
    let local_ips = local_addresses();
    let deadline = Instant::now() + DISCOVERY_WINDOW;
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
        if !is_remote_response(&response, source.ip(), &hostname, &local_ips) {
            continue;
        }
        if !seen.insert((response.hostname.clone(), source.ip())) {
            continue;
        }
        let mut candidates = vec![PeerCandidate {
            latency: Some(started.elapsed().as_millis() as u64),
            ..PeerCandidate::new(interface, source.ip().to_string())
        }];
        if let Some(endpoint_id) = advertised_iroh_endpoint(&response) {
            let mut candidate = PeerCandidate::remembered(ConnectionInterface::Iroh, endpoint_id);
            candidate.set_rtt_capable(super::iroh::supports_rtt(&candidate.address));
            candidates.push(candidate);
        }
        peers.push(PeerInfo {
            hostname: response.hostname,
            tailscale_ip: source.ip().to_string(),
            address: source.ip().to_string(),
            online: true,
            enabled: true,
            connection_mode: interface.as_str().to_string(),
            trusted: false,
            fingerprint: String::new(),
            candidates,
            current_interface: None,
            current_address: None,
            status: PeerStatus::Online,
        });
    }

    peers.sort_by(|a, b| a.hostname.cmp(&b.hostname));
    Ok(peers)
}

pub async fn discover() -> Result<(LocalInfo, Vec<PeerInfo>), String> {
    let peers = probe_targets(lan_broadcast_targets(), ConnectionInterface::Lan).await?;
    Ok((
        LocalInfo {
            hostname: local_hostname(),
            tailscale_ip: local_ip(),
            candidates: Vec::new(),
        },
        peers,
    ))
}

pub async fn probe_addresses(
    addresses: impl IntoIterator<Item = String>,
    interface: ConnectionInterface,
) -> Result<Vec<PeerInfo>, String> {
    let targets = addresses
        .into_iter()
        .filter_map(|address| address.parse::<IpAddr>().ok())
        .filter_map(|address| match address {
            IpAddr::V4(address) => Some(SocketAddr::from((address, DISCOVERY_PORT))),
            IpAddr::V6(_) => None,
        })
        .collect::<Vec<_>>();
    if targets.is_empty() {
        return Ok(Vec::new());
    }
    probe_targets(targets, interface).await
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
        loop {
            match socket.recv_from(&mut buffer).await {
                Ok((length, source)) if &buffer[..length] == DISCOVERY_REQUEST => {
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
    use super::{is_remote_response, DiscoveryResponse};
    use std::collections::HashSet;

    fn response(hostname: &str) -> DiscoveryResponse {
        DiscoveryResponse {
            app: "tailsync".into(),
            version: 1,
            hostname: hostname.into(),
            tcp_port: 19890,
            iroh_endpoint_id: None,
            iroh_rtt: false,
        }
    }

    #[test]
    fn discovery_rejects_self_by_hostname_or_local_ip() {
        let local_ips = HashSet::from(["192.168.1.10".parse().unwrap()]);

        assert!(!is_remote_response(
            &response("macbook"),
            "192.168.1.20".parse().unwrap(),
            "macbook",
            &local_ips,
        ));
        assert!(!is_remote_response(
            &response("stale-macbook-name"),
            "192.168.1.10".parse().unwrap(),
            "macbook",
            &local_ips,
        ));
        assert!(is_remote_response(
            &response("windows"),
            "192.168.1.20".parse().unwrap(),
            "macbook",
            &local_ips,
        ));
    }
}
