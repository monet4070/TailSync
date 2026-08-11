#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionInterface {
    Lan,
    Iroh,
    Tailscale,
}

impl ConnectionInterface {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lan => "lan",
            Self::Iroh => "iroh",
            Self::Tailscale => "tailscale",
        }
    }

    pub(super) fn priority(self) -> u8 {
        match self {
            Self::Lan => 0,
            Self::Iroh => 1,
            Self::Tailscale => 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PeerCandidate {
    pub interface: ConnectionInterface,
    pub address: String,
    #[serde(default = "candidate_online_default")]
    pub online: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency: Option<u64>,
    pub priority: u8,
    #[serde(default)]
    pub status: PeerStatus,
}

fn candidate_online_default() -> bool {
    true
}

impl PeerCandidate {
    pub fn new(interface: ConnectionInterface, address: impl Into<String>) -> Self {
        Self {
            interface,
            address: address.into(),
            online: true,
            latency: None,
            priority: interface.priority(),
            status: PeerStatus::Online,
        }
    }

    pub fn remembered(interface: ConnectionInterface, address: impl Into<String>) -> Self {
        Self {
            online: false,
            status: PeerStatus::Discovered,
            ..Self::new(interface, address)
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeerStatus {
    #[default]
    Discovered,
    Confirming,
    Online,
    Connected,
    Offline,
}
