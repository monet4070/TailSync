use serde::{Deserialize, Serialize};
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use super::{ConnectionInterface, PeerCandidate, PeerStatus};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub hostname: String,
    pub tailscale_ip: String,
    pub online: bool,
    pub enabled: bool, // User-toggled
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub connection_mode: String,
    #[serde(default)]
    pub trusted: bool,
    #[serde(default)]
    pub fingerprint: String,
    #[serde(default)]
    pub candidates: Vec<PeerCandidate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_interface: Option<ConnectionInterface>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_address: Option<String>,
    #[serde(default)]
    pub status: PeerStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalInfo {
    pub hostname: String,
    pub tailscale_ip: String,
}

#[cfg(not(target_os = "macos"))]
fn tailscale_binary() -> Option<PathBuf> {
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
            return Some(path);
        }
    }

    // Preserve normal PATH lookup for package managers and custom installs.
    Some(PathBuf::from("tailscale"))
}

#[cfg(target_os = "macos")]
fn tailscale_binary() -> Option<PathBuf> {
    // These are CLI launchers installed by Tailscale or a package manager.
    // Do not invoke Tailscale.app's GUI executable directly: its argv[0] and
    // launch context can activate the GUI instead of behaving as a CLI.
    ["/usr/local/bin/tailscale", "/opt/homebrew/bin/tailscale"]
        .into_iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
}

/// Run `tailscale status --json` and parse online peers
pub fn get_peers() -> Result<(LocalInfo, Vec<PeerInfo>), String> {
    get_peers_from_binary(tailscale_binary())
}

fn get_peers_from_binary(binary: Option<PathBuf>) -> Result<(LocalInfo, Vec<PeerInfo>), String> {
    let binary = binary
        .ok_or("Tailscale CLI was not found; install its CLI launcher to enable live discovery")?;
    run_status(&binary)
}

fn run_status(binary: &Path) -> Result<(LocalInfo, Vec<PeerInfo>), String> {
    let mut cmd = Command::new(binary);
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

    parse_status(&output.stdout).map_err(|error| {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim();
        if stderr.is_empty() {
            error
        } else {
            format!("{error}; {stderr}")
        }
    })
}

fn parse_status(bytes: &[u8]) -> Result<(LocalInfo, Vec<PeerInfo>), String> {
    let output = String::from_utf8_lossy(bytes);
    let output = output.trim_start_matches('\u{feff}').trim_start();
    let Some(json_start) = output.find('{') else {
        let message = output.split_whitespace().collect::<Vec<_>>().join(" ");
        return Err(if message.is_empty() {
            "Tailscale CLI returned no JSON output".to_string()
        } else {
            format!("Tailscale CLI did not return JSON: {message}")
        });
    };
    let data: serde_json::Value = serde_json::from_str(&output[json_start..])
        .map_err(|e| format!("JSON parse error: {e}"))?;
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
        tailscale_ip: self_ips,
    };

    // Peer info
    let mut peers = Vec::new();
    if let Some(peer_map) = data["Peer"].as_object() {
        for (_key, peer) in peer_map {
            let online = peer["Online"].as_bool().unwrap_or(false);
            if !online {
                continue;
            }
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
                    online: true,
                    enabled: true,
                    address: peer_ip.to_string(),
                    connection_mode: "tailscale".to_string(),
                    trusted: false,
                    fingerprint: String::new(),
                    candidates: vec![PeerCandidate::new(ConnectionInterface::Tailscale, peer_ip)],
                    current_interface: None,
                    current_address: None,
                    status: PeerStatus::Online,
                });
            }
        }
    }

    Ok((local, peers))
}

/// Get local machine info only (no peer scan)
#[cfg(test)]
mod tests {
    #[cfg(target_os = "macos")]
    use super::tailscale_binary;
    use super::{get_peers_from_binary, parse_status};

    #[test]
    fn status_parser_returns_only_online_valid_peers() {
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
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].hostname, "windows");
        assert_eq!(peers[0].address, "100.64.0.2");
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
    fn status_parser_surfaces_non_json_cli_output() {
        let error = parse_status(b"The Tailscale CLI failed to start: Failed to load preferences.")
            .unwrap_err();
        assert_eq!(
            error,
            "Tailscale CLI did not return JSON: The Tailscale CLI failed to start: Failed to load preferences."
        );
    }

    #[test]
    fn status_parser_accepts_a_notice_before_json() {
        let status = br#"Tailscale notice
        {
            "BackendState": "Running",
            "Self": {"HostName": "macbook", "TailscaleIPs": ["100.64.0.1"]},
            "Peer": {}
        }"#;
        let (local, peers) = parse_status(status).unwrap();
        assert_eq!(local.hostname, "macbook");
        assert!(peers.is_empty());
    }

    #[test]
    fn discovery_requires_the_live_cli() {
        let error = get_peers_from_binary(None).unwrap_err();
        assert!(error.contains("Tailscale CLI was not found"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_uses_the_installed_cli_launcher_instead_of_the_gui_executable() {
        if let Some(binary) = tailscale_binary() {
            assert!(
                binary == std::path::Path::new("/usr/local/bin/tailscale")
                    || binary == std::path::Path::new("/opt/homebrew/bin/tailscale")
            );
            assert!(!binary.starts_with("/Applications/Tailscale.app"));
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "requires a running macOS Tailscale installation"]
    fn live_macos_discovery_contains_an_online_peer() {
        let (_, peers) = super::get_peers().unwrap();
        assert!(peers.iter().any(|peer| peer.online));
    }
}
