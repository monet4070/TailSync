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

    pub(super) fn priority(self) -> u8 {
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
