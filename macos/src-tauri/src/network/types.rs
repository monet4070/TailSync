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
    /// Whether latency can be measured over this route. LAN/Tailscale routes
    /// are always testable; Iroh routes require the peer to have advertised
    /// RTT support and to have been rediscovered.
    #[serde(default = "candidate_rtt_capable_default")]
    pub rtt_capable: bool,
}

fn candidate_online_default() -> bool {
    true
}

fn candidate_rtt_capable_default() -> bool {
    false
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
            rtt_capable: interface != ConnectionInterface::Iroh,
        }
    }

    pub fn remembered(interface: ConnectionInterface, address: impl Into<String>) -> Self {
        Self {
            online: false,
            status: PeerStatus::Discovered,
            ..Self::new(interface, address)
        }
    }

    pub fn set_rtt_capable(&mut self, capable: bool) {
        self.rtt_capable = capable;
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

#[cfg(test)]
mod tests {
    use super::{ConnectionInterface, PeerCandidate};

    #[test]
    fn new_candidates_only_assume_tcp_routes_support_rtt() {
        assert!(!PeerCandidate::new(ConnectionInterface::Iroh, "endpoint").rtt_capable);
        assert!(PeerCandidate::new(ConnectionInterface::Lan, "192.168.1.2").rtt_capable);
        assert!(PeerCandidate::new(ConnectionInterface::Tailscale, "100.64.0.2").rtt_capable);
    }

    #[test]
    fn legacy_candidates_without_capability_fail_closed() {
        let candidate: PeerCandidate = serde_json::from_value(serde_json::json!({
            "interface": "iroh",
            "address": "endpoint",
            "priority": 1
        }))
        .expect("legacy candidate");

        assert!(!candidate.rtt_capable);
    }
}
