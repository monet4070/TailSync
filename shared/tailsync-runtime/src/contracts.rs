//! Versioned contracts shared by the local UI transports.
//!
//! The capability response is intentionally small.  It lets a client choose
//! an optional transport optimization without guessing from platform names or
//! falling back after an authentication or protocol error.

use serde::{Deserialize, Serialize};

pub const LOCAL_CONTRACT_SCHEMA_VERSION: u32 = 1;
pub const LOCAL_PREVIEW_MAX_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, schemars::JsonSchema)]
pub struct LocalCapabilities {
    #[schemars(range(min = 1, max = 1))]
    pub schema_version: u32,
    #[schemars(range(min = 4, max = 4))]
    pub wire_version: u32,
    pub platform: String,
    #[schemars(range(min = 67108864, max = 67108864))]
    pub max_preview_bytes: u64,
    pub supports_binary_preview: bool,
    pub supports_runtime_snapshot: bool,
    pub supports_stable_errors: bool,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct DaemonStatus {
    pub tcp_server_healthy: bool,
    pub clipboard_monitor_healthy: bool,
    pub clipboard_monitor_failures: u64,
    pub active_routes: std::collections::HashMap<String, tailsync_core::peer::types::ActiveRoute>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct MacRuntimeSnapshot {
    pub revision: u64,
    pub history_version: u64,
    pub progress: Option<FileProgress>,
    pub storage: tailsync_core::db::StorageStatus,
    pub sync_enabled: bool,
    pub status: DaemonStatus,
    pub notifications: Vec<RuntimeNotification>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct WindowsRuntimeSnapshot {
    pub revision: u64,
    pub history_version: u64,
    pub progress: Option<FileProgress>,
    pub sync_warning: Option<tailsync_core::sync_warning::SyncWarning>,
    pub notifications: Vec<RuntimeNotification>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct PeerSnapshot {
    #[serde(flatten)]
    pub peer: tailsync_core::peer::types::PeerInfo,
    pub routes: Vec<tailsync_core::peer::types::PeerRouteSnapshot>,
    pub protocol_error: Option<String>,
    pub required_protocol_version: Option<u8>,
}

impl PeerSnapshot {
    pub fn new(
        peer: tailsync_core::peer::types::PeerInfo,
        paired_endpoint: Option<&String>,
        protocol_error: Option<String>,
        version: u8,
    ) -> Self {
        use tailsync_core::peer::types::{PeerRouteSnapshot, PeerStatus};
        let routes = peer
            .candidates
            .iter()
            .map(|candidate| {
                let connected = peer.current_address.as_deref() == Some(&candidate.address)
                    && peer.current_interface == Some(candidate.interface);
                PeerRouteSnapshot {
                    interface: candidate.interface,
                    address: candidate.address.clone(),
                    status: if connected {
                        PeerStatus::Connected
                    } else {
                        candidate.status
                    },
                    online: candidate.online,
                    connected,
                    latency_ms: candidate.latency,
                    pairing_endpoint: paired_endpoint == Some(&candidate.address),
                    rtt_capable: candidate.rtt_capable,
                }
            })
            .collect();
        Self {
            peer,
            routes,
            required_protocol_version: protocol_error.as_ref().map(|_| version),
            protocol_error,
        }
    }
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct LocalDeviceSnapshot {
    pub hostname: String,
    pub tailscale_ip: String,
    pub routes: Vec<tailsync_core::peer::types::PeerRouteSnapshot>,
    pub connection_mode: String,
    pub public_key: String,
    pub fingerprint: String,
    pub iroh_endpoint_id: Option<String>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct PeersResponse {
    #[serde(rename = "self")]
    pub local: LocalDeviceSnapshot,
    pub peers: Vec<PeerSnapshot>,
    pub paired_peer_endpoints: std::collections::HashMap<String, String>,
    pub discovery_error: Option<String>,
}

impl LocalCapabilities {
    pub fn current(
        platform: impl Into<String>,
        wire_version: u8,
        supports_binary_preview: bool,
        supports_runtime_snapshot: bool,
    ) -> Self {
        Self {
            schema_version: LOCAL_CONTRACT_SCHEMA_VERSION,
            wire_version: u32::from(wire_version),
            platform: platform.into(),
            max_preview_bytes: LOCAL_PREVIEW_MAX_BYTES,
            supports_binary_preview,
            supports_runtime_snapshot,
            supports_stable_errors: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_capabilities_are_stable_and_json_safe() {
        let capabilities = LocalCapabilities::current("macos", 4, true, true);
        assert_eq!(capabilities.schema_version, LOCAL_CONTRACT_SCHEMA_VERSION);
        assert_eq!(capabilities.wire_version, 4);
        assert_eq!(capabilities.max_preview_bytes, 64 * 1024 * 1024);
        let round_trip = serde_json::from_value::<LocalCapabilities>(
            serde_json::to_value(&capabilities).expect("serialize capabilities"),
        )
        .expect("decode capabilities");
        assert_eq!(round_trip, capabilities);
    }
}

#[derive(Clone, Serialize, schemars::JsonSchema)]
pub struct FileProgress {
    pub batch_id: String,
    pub name: String,
    pub sent: u64,
    pub total: u64,
    pub active: bool,
    #[schemars(extend("enum" = ["sending", "receiving"]))]
    pub direction: String,
    pub device: String,
    pub completed_files: usize,
    pub total_files: usize,
    pub speed_bytes_per_second: u64,
    pub status: String,
    pub can_stop: bool,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, schemars::JsonSchema)]
pub struct RuntimeNotification {
    pub id: u64,
    pub level: String,
    pub message: String,
}
