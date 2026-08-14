use std::collections::HashMap;
use std::net::IpAddr;
use std::path::PathBuf;
use std::process::Command;

use super::{lan, ConnectionInterface, PeerCandidate, PeerStatus};

pub use tailsync_core::peer::types::{LocalInfo, PeerInfo};

fn tailscale_binary() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        for candidate in [
            "/Applications/Tailscale.app/Contents/MacOS/Tailscale",
            "/Applications/Tailscale.app/Contents/MacOS/tailscale",
            "/opt/homebrew/bin/tailscale",
            "/usr/local/bin/tailscale",
        ] {
            let path = PathBuf::from(candidate);
            if path.is_file() {
                return path;
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        let mut candidates = vec![
            PathBuf::from(r"C:\Program Files\Tailscale\tailscale.exe"),
            PathBuf::from(r"C:\Program Files (x86)\Tailscale\tailscale.exe"),
        ];
        for variable in ["ProgramW6432", "ProgramFiles", "LOCALAPPDATA"] {
            if let Ok(root) = std::env::var(variable) {
                candidates.push(PathBuf::from(root).join("Tailscale").join("tailscale.exe"));
            }
        }
        if let Some(path) = candidates.into_iter().find(|path| path.is_file()) {
            return path;
        }
    }

    // Preserve normal PATH lookup for package managers and custom installs.
    PathBuf::from("tailscale")
}

/// Run `tailscale status --json` and parse online peers
pub fn get_peers() -> Result<(LocalInfo, Vec<PeerInfo>), String> {
    let mut cmd = Command::new(tailscale_binary());
    cmd.args(["status", "--json"]);
    // Suppress console window flash on Windows
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let output = cmd
        .output()
        .map_err(|e| format!("Failed to run tailscale: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("tailscale status failed: {}", stderr.trim()));
    }

    let (local, mut peers) = parse_status(&output.stdout)?;
    let addresses = peers
        .iter()
        .filter_map(|peer| peer.tailscale_ip.parse::<IpAddr>().ok())
        .collect::<Vec<_>>();
    let hostnames = lan::probe_hostnames(&addresses);
    apply_probed_hostnames(&mut peers, &hostnames);
    Ok((local, peers))
}

fn apply_probed_hostnames(peers: &mut [PeerInfo], hostnames: &HashMap<IpAddr, lan::ProbeResponse>) {
    for peer in peers {
        let Ok(address) = peer.tailscale_ip.parse::<IpAddr>() else {
            continue;
        };
        peer.online = false;
        if let Some(response) = hostnames.get(&address) {
            peer.hostname.clone_from(&response.hostname);
            peer.online = true;
            for candidate in &mut peer.candidates {
                candidate.latency = Some(response.latency_ms);
            }
            if let Some(endpoint_id) = &response.iroh_endpoint_id {
                if !peer.candidates.iter().any(|candidate| {
                    candidate.interface == ConnectionInterface::Iroh
                        && candidate.address == *endpoint_id
                }) {
                    let mut candidate = PeerCandidate::new(ConnectionInterface::Iroh, endpoint_id);
                    candidate.set_rtt_capable(super::iroh::supports_rtt(&candidate.address));
                    peer.candidates.push(candidate);
                }
            }
        }
    }
}

fn parse_status(bytes: &[u8]) -> Result<(LocalInfo, Vec<PeerInfo>), String> {
    let data: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|e| format!("JSON parse error: {e}"))?;
    match data.get("BackendState").and_then(serde_json::Value::as_str) {
        Some("Running") => {}
        Some(state) => return Err(format!("Tailscale backend is not running ({state})")),
        None => return Err("Tailscale status does not contain backend state".to_string()),
    }

    // Self info
    let self_node = data["Self"]
        .as_object()
        .ok_or("Tailscale status does not contain local device information")?;
    let hostname = self_node
        .get("HostName")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or("Tailscale status does not contain a local hostname")?
        .to_string();
    let self_ips = self_node
        .get("TailscaleIPs")
        .and_then(serde_json::Value::as_array)
        .and_then(|ips| ips.first())
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or("Tailscale status does not contain a local IP address")?
        .to_string();

    let local = LocalInfo {
        hostname,
        tailscale_ip: self_ips.clone(),
        candidates: vec![PeerCandidate::new(ConnectionInterface::Tailscale, self_ips)],
    };

    // Peer info
    let mut peers = Vec::new();
    if let Some(peer_map) = data["Peer"].as_object() {
        for (_key, peer) in peer_map {
            let Some(peer_hostname) = peer["HostName"]
                .as_str()
                .filter(|value| !value.trim().is_empty())
            else {
                continue;
            };
            let peer_ip = peer["TailscaleIPs"]
                .as_array()
                .and_then(|ips| ips.first())
                .and_then(|ip| ip.as_str())
                .unwrap_or("");

            if !peer_ip.is_empty() {
                peers.push(PeerInfo {
                    hostname: peer_hostname.to_string(),
                    tailscale_ip: peer_ip.to_string(),
                    online: false,
                    enabled: true,
                    address: peer_ip.to_string(),
                    connection_mode: "tailscale".to_string(),
                    trusted: false,
                    fingerprint: String::new(),
                    candidates: vec![PeerCandidate::new(ConnectionInterface::Tailscale, peer_ip)],
                    current_interface: None,
                    current_address: None,
                    status: PeerStatus::Discovered,
                });
            }
        }
    }

    Ok((local, peers))
}

/// Get local machine info only (no peer scan)
#[cfg(test)]
mod tests {
    use super::{apply_probed_hostnames, parse_status};
    use crate::network::lan;
    use std::collections::HashMap;

    #[test]
    fn status_parser_returns_all_valid_candidates_without_marking_them_online() {
        let status = br#"{
            "BackendState": "Running",
            "Self": {"HostName": "macbook", "TailscaleIPs": ["100.64.0.1"]},
            "Peer": {
                "online": {"HostName": "windows", "Online": true, "TailscaleIPs": ["100.64.0.2"]},
                "offline": {"HostName": "old-device", "Online": false, "TailscaleIPs": ["100.64.0.3"]},
                "invalid": {"Online": true, "TailscaleIPs": ["100.64.0.4"]}
            }
        }"#;
        let (local, peers) = parse_status(status).unwrap();
        assert_eq!(local.hostname, "macbook");
        assert_eq!(local.tailscale_ip, "100.64.0.1");
        assert_eq!(peers.len(), 2);
        let windows = peers
            .iter()
            .find(|peer| peer.hostname == "windows")
            .unwrap();
        assert_eq!(windows.address, "100.64.0.2");
        assert!(!windows.online);
        let old_device = peers
            .iter()
            .find(|peer| peer.hostname == "old-device")
            .unwrap();
        assert!(!old_device.online);
    }

    #[test]
    fn status_parser_rejects_stopped_backend_and_missing_identity() {
        assert!(parse_status(br#"{}"#).is_err());
        assert!(parse_status(br#"{"BackendState":"Stopped"}"#).is_err());
        assert!(parse_status(br#"{"BackendState":"Running","Self":{}}"#).is_err());
        assert!(
            parse_status(br#"{"BackendState":"Running","Self":{"HostName":"macbook"}}"#).is_err()
        );
        assert!(parse_status(
            br#"{"BackendState":"Running","Self":{"TailscaleIPs":["100.64.0.1"]}}"#
        )
        .is_err());
    }

    #[test]
    fn tailsync_probe_replaces_tailnet_alias_with_paired_hostname() {
        let (_, mut peers) = parse_status(
            br#"{
                "BackendState":"Running",
                "Self":{"HostName":"windows","TailscaleIPs":["100.64.0.1"]},
                "Peer":{"node":{"Online":true,"HostName":"monet's MacBook Air","TailscaleIPs":["100.64.0.2"]}}
            }"#,
        )
        .unwrap();
        let hostnames = HashMap::from([(
            "100.64.0.2".parse().unwrap(),
            lan::ProbeResponse {
                hostname: "Mac".to_string(),
                latency_ms: 12,
                iroh_endpoint_id: None,
            },
        )]);

        apply_probed_hostnames(&mut peers, &hostnames);

        assert_eq!(peers[0].hostname, "Mac");
        assert_eq!(peers[0].tailscale_ip, "100.64.0.2");
        assert!(peers[0].online);
        assert_eq!(peers[0].candidates[0].latency, Some(12));
    }
}
