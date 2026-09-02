//! Shared peer discovery, health, and delivery types.
//!
//! These types are the cross-platform contract for the Peer Directory and
//! Peer Delivery modules. The macOS and Windows clients re-export them from
//! their `network` modules; serialized field names are part of the JSON
//! contract consumed by the SwiftUI shell and the React settings page, so
//! serde attributes must not be changed casually.

use std::fmt;
use std::net::SocketAddr;

/// The transport a peer route uses.
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

    /// Route preference for candidate ordering: LAN first, then Iroh, then
    /// Tailscale.
    pub fn priority(self) -> u8 {
        match self {
            Self::Lan => 0,
            Self::Iroh => 1,
            Self::Tailscale => 2,
        }
    }
}

/// The user-selected connection strategy. Parsed once at the core
/// boundary so directory and health rules share one interpretation of the
/// mode strings that come from settings and the JSON API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionMode {
    Auto,
    LanOnly,
    TailscaleOnly,
}

impl ConnectionMode {
    /// Parse a settings/API mode string. Accepts the canonical and legacy
    /// spellings used by both platforms.
    pub fn parse(mode: &str) -> Option<Self> {
        match mode {
            "auto" => Some(Self::Auto),
            "lan" | "lan_only" => Some(Self::LanOnly),
            "tailscale" | "tailscale_only" => Some(Self::TailscaleOnly),
            _ => None,
        }
    }

    /// The canonical serialized form of this mode.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::LanOnly => "lan_only",
            Self::TailscaleOnly => "tailscale_only",
        }
    }

    /// Whether this mode permits connecting over `interface`.
    pub fn allows(self, interface: ConnectionInterface) -> bool {
        match self {
            Self::Auto => true,
            Self::LanOnly => interface == ConnectionInterface::Lan,
            Self::TailscaleOnly => interface == ConnectionInterface::Tailscale,
        }
    }

    /// The discovery interfaces this mode probes (Auto probes both).
    pub fn interfaces(self) -> &'static [ConnectionInterface] {
        match self {
            Self::Auto => &[ConnectionInterface::Lan, ConnectionInterface::Tailscale],
            Self::LanOnly => &[ConnectionInterface::Lan],
            Self::TailscaleOnly => &[ConnectionInterface::Tailscale],
        }
    }
}

/// Lifecycle status of a discovered peer.
///
/// Projected health state is consistent when a peer or candidate is `online`
/// iff its status is
/// [`PeerStatus::Connected`], [`PeerStatus::Online`], or
/// [`PeerStatus::Confirming`]. [`PeerInfo::is_consistent`] and
/// [`PeerCandidate::is_consistent`] check this. Legacy wire data can contain
/// older field combinations; the health projection (`apply_peer_health`)
/// normalizes both fields before exposing a fresh snapshot.
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

impl PeerStatus {
    /// Stable wire/log label for status transitions.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Discovered => "discovered",
            Self::Confirming => "confirming",
            Self::Online => "online",
            Self::Connected => "connected",
            Self::Offline => "offline",
        }
    }

    /// Whether the status counts as online in the health model.
    pub fn is_online(self) -> bool {
        matches!(self, Self::Connected | Self::Online | Self::Confirming)
    }
}

/// One reachable route for a peer, with the health fields the health monitor
/// derives. Legacy serialized candidates without `online`/`status` keep
/// working through serde defaults.
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
    /// Whether `online` and `status` agree under the health model and
    /// `priority` matches the interface rank.
    pub fn is_consistent(&self) -> bool {
        self.online == self.status.is_online() && self.priority == self.interface.priority()
    }

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

    /// A candidate remembered from settings rather than fresh discovery:
    /// it has not answered any probe yet.
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

/// The route a peer is currently using for authenticated traffic.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ActiveRoute {
    pub interface: ConnectionInterface,
    pub address: String,
    pub latency: u64,
}

/// Health snapshot for one peer, derived by the platform health monitor.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PeerHealthSnapshot {
    pub status: PeerStatus,
    pub online: bool,
    pub connected: bool,
    pub latency_ms: Option<u64>,
}

impl PeerInfo {
    /// Whether the peer-level fields and every candidate satisfy the
    /// health-model invariant (`online` iff status is connected/online/
    /// confirming). The health projection derives all of these; legacy wire
    /// values may remain inconsistent until that projection runs.
    pub fn is_consistent(&self) -> bool {
        self.online == self.status.is_online()
            && self.candidates.iter().all(PeerCandidate::is_consistent)
    }
}

/// One serialized route row of the peer snapshot, shared by both platform
/// JSON APIs (mirrored by the Swift `Route` DTO and the React `PeerRoute`
/// interface). Field names are part of the wire contract; the drift check
/// compares this struct's fields against the contract lists.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PeerRouteSnapshot {
    pub interface: ConnectionInterface,
    pub address: String,
    pub status: PeerStatus,
    pub online: bool,
    pub connected: bool,
    pub latency_ms: Option<u64>,
    pub pairing_endpoint: bool,
    pub rtt_capable: bool,
}

/// A discovered or remembered peer device. This is the unified cross-platform
/// shape: macOS and Windows previously carried slightly different field sets
/// (Windows lacked `current_address` and `status`), which made shared
/// directory logic impossible and let the JSON contracts drift.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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

/// Information about the local device gathered during discovery.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LocalInfo {
    pub hostname: String,
    pub tailscale_ip: String,
    #[serde(default)]
    pub candidates: Vec<PeerCandidate>,
}

/// Result of delivering one reliable frame: a file offset ACK when the peer
/// acknowledged a chunk. `resume_required` is set when the peer received a
/// file frame but no longer has the preceding transfer metadata (for example
/// after its process restarted). The file sender must replay the batch and
/// file metadata before sending another chunk.
#[derive(Debug, Clone, Copy, Default)]
pub struct DeliveryReceipt {
    pub next_offset: Option<u64>,
    pub resume_required: bool,
}

/// A resolved transport target for one candidate route.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ResolvedTarget {
    Tcp(SocketAddr),
    Iroh(String),
}

impl fmt::Display for ResolvedTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tcp(address) => address.fmt(formatter),
            Self::Iroh(endpoint_id) => write!(formatter, "iroh:{endpoint_id}"),
        }
    }
}

/// A candidate route paired with its resolved transport target.
#[derive(Debug, Clone)]
pub struct ResolvedCandidate {
    pub candidate: PeerCandidate,
    pub target: ResolvedTarget,
}

/// Latency of one measured route, as reported to the settings UI. The path is
/// "tcp" for plain TCP routes and "direct" or "relay" for Iroh routes, so
/// callers can distinguish a direct connection from a cold-start relay sample.
#[derive(Debug, serde::Serialize)]
pub struct RouteLatency {
    pub latency_ms: u64,
    pub path: String,
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // The serde default treats a candidate as online until a probe proves
        // otherwise, matching the historic macOS semantics.
        assert!(candidate.online);
        assert_eq!(candidate.status, PeerStatus::Discovered);
    }

    #[test]
    fn remembered_candidates_start_discovered_and_offline() {
        let candidate = PeerCandidate::remembered(ConnectionInterface::Iroh, "endpoint");
        assert!(!candidate.online);
        assert_eq!(candidate.status, PeerStatus::Discovered);
        assert_eq!(candidate.priority, 1);
    }

    #[test]
    fn connection_mode_parses_canonical_and_legacy_spellings() {
        assert_eq!(ConnectionMode::parse("auto"), Some(ConnectionMode::Auto));
        assert_eq!(ConnectionMode::parse("lan"), Some(ConnectionMode::LanOnly));
        assert_eq!(
            ConnectionMode::parse("lan_only"),
            Some(ConnectionMode::LanOnly)
        );
        assert_eq!(
            ConnectionMode::parse("tailscale"),
            Some(ConnectionMode::TailscaleOnly)
        );
        assert_eq!(
            ConnectionMode::parse("tailscale_only"),
            Some(ConnectionMode::TailscaleOnly)
        );
        assert_eq!(ConnectionMode::parse("cache-test"), None);
        assert_eq!(ConnectionMode::Auto.as_str(), "auto");
        assert_eq!(ConnectionMode::LanOnly.as_str(), "lan_only");
        assert_eq!(ConnectionMode::TailscaleOnly.as_str(), "tailscale_only");
    }

    #[test]
    fn connection_mode_allows_only_its_interfaces() {
        let lan = ConnectionInterface::Lan;
        let tail = ConnectionInterface::Tailscale;
        let iroh = ConnectionInterface::Iroh;
        assert!(
            ConnectionMode::Auto.allows(lan)
                && ConnectionMode::Auto.allows(tail)
                && ConnectionMode::Auto.allows(iroh)
        );
        assert!(ConnectionMode::LanOnly.allows(lan) && !ConnectionMode::LanOnly.allows(tail));
        assert!(
            !ConnectionMode::TailscaleOnly.allows(lan)
                && ConnectionMode::TailscaleOnly.allows(tail)
        );
        assert_eq!(
            ConnectionMode::LanOnly.interfaces(),
            &[ConnectionInterface::Lan]
        );
    }

    #[test]
    fn health_consistency_invariant_holds_for_constructors() {
        // Constructors produce consistent state: online matches status.
        let discovered = PeerCandidate::remembered(ConnectionInterface::Lan, "192.168.1.2");
        assert!(discovered.is_consistent());
        let fresh = PeerCandidate::new(ConnectionInterface::Lan, "192.168.1.3");
        assert!(fresh.is_consistent());
        let peer = PeerInfo {
            hostname: "peer".into(),
            tailscale_ip: String::new(),
            online: false,
            enabled: true,
            address: String::new(),
            connection_mode: "lan".into(),
            trusted: false,
            fingerprint: String::new(),
            candidates: vec![discovered],
            current_interface: None,
            current_address: None,
            status: PeerStatus::Discovered,
        };
        assert!(peer.is_consistent());
    }

    #[test]
    fn health_consistency_invariant_detects_contradictions() {
        let mut candidate = PeerCandidate::new(ConnectionInterface::Lan, "192.168.1.2");
        candidate.status = PeerStatus::Offline;
        assert!(!candidate.is_consistent());
        let peer = PeerInfo {
            hostname: "peer".into(),
            tailscale_ip: String::new(),
            online: true,
            enabled: true,
            address: String::new(),
            connection_mode: "lan".into(),
            trusted: false,
            fingerprint: String::new(),
            candidates: Vec::new(),
            current_interface: None,
            current_address: None,
            status: PeerStatus::Offline,
        };
        assert!(!peer.is_consistent());
    }

    #[test]
    fn peer_status_defaults_to_discovered() {
        assert_eq!(PeerStatus::default(), PeerStatus::Discovered);
    }

    #[test]
    fn health_snapshot_serializes_snake_case_fields() {
        let snapshot = PeerHealthSnapshot {
            status: PeerStatus::Online,
            online: true,
            connected: true,
            latency_ms: Some(12),
        };
        let json = serde_json::to_value(snapshot).unwrap();
        assert_eq!(json["status"], "online");
        assert_eq!(json["online"], true);
        assert_eq!(json["latency_ms"], 12);
    }

    #[test]
    fn resolved_target_displays_iroh_prefix() {
        assert_eq!(ResolvedTarget::Iroh("abc".into()).to_string(), "iroh:abc");
        assert_eq!(
            ResolvedTarget::Tcp("192.168.1.2:19890".parse().unwrap()).to_string(),
            "192.168.1.2:19890"
        );
    }

    #[test]
    fn route_latency_serializes() {
        let latency = RouteLatency {
            latency_ms: 5,
            path: "direct".into(),
        };
        let json = serde_json::to_value(latency).unwrap();
        assert_eq!(json["latency_ms"], 5);
        assert_eq!(json["path"], "direct");
    }

    #[test]
    fn active_route_serializes() {
        let route = ActiveRoute {
            interface: ConnectionInterface::Lan,
            address: "192.168.1.2".into(),
            latency: 3,
        };
        let json = serde_json::to_value(route).unwrap();
        assert_eq!(json["interface"], "lan");
    }

    #[test]
    fn legacy_peer_info_without_status_fields_defaults_safely() {
        // Windows previously serialized PeerInfo without current_address and
        // status; deserializing such JSON must keep working.
        let peer: PeerInfo = serde_json::from_value(serde_json::json!({
            "hostname": "windows",
            "tailscale_ip": "100.64.0.2",
            "online": true,
            "enabled": true,
        }))
        .expect("legacy peer info");
        assert_eq!(peer.current_address, None);
        assert_eq!(peer.status, PeerStatus::Discovered);
        assert!(peer.candidates.is_empty());
    }

    #[test]
    fn local_info_defaults_candidates_when_absent() {
        let local: LocalInfo = serde_json::from_value(serde_json::json!({
            "hostname": "macbook",
            "tailscale_ip": "100.64.0.1",
        }))
        .expect("local info without candidates");
        assert!(local.candidates.is_empty());
    }
}
