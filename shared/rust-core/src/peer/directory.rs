//! Peer Directory: pure state derivation for the peer directory.
//!
//! This module owns the cross-platform rules for turning raw discovery
//! snapshots (UDP/mDNS/Tailscale adapter outputs) and remembered settings
//! into the merged, ranked peer view that the UI, health monitor, and
//! delivery path all consume. It performs no I/O: adapters feed snapshots
//! in and receive the derived view back, so every rule here is unit-testable
//! without sockets or a running daemon.
//!
//! The implementations are the unified superset of the former macOS and
//! Windows copies, which had drifted: Windows merged `LocalInfo` candidates,
//! re-associated discovered aliases with their paired hostname, and completed
//! trusted peers from every remembered interface; macOS used offline
//! `remembered` candidates. The merged behavior keeps all of those rules and
//! uses `PeerCandidate::remembered` for settings-derived candidates so a
//! remembered route is never presented as already online.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};

use crate::crypto::Settings;
use crate::identity;
use crate::peer::types::{
    ConnectionInterface, ConnectionMode, LocalInfo, PeerCandidate, PeerInfo, PeerStatus,
    ResolvedCandidate, ResolvedTarget,
};

/// Map a connection mode string to the single interface it allows.
/// `Auto` allows every interface and therefore has no single mapping.
pub fn mode_interface(mode: &str) -> Option<ConnectionInterface> {
    match ConnectionMode::parse(mode)? {
        ConnectionMode::Auto => None,
        ConnectionMode::LanOnly => Some(ConnectionInterface::Lan),
        ConnectionMode::TailscaleOnly => Some(ConnectionInterface::Tailscale),
    }
}

/// Whether an address may be used for the given mode. Tailscale addresses
/// are the CGNAT range `100.64.0.0/10` and the ULA prefix `fd7a:115c:a1e0::/48`;
/// LAN addresses are private, link-local, or loopback.
pub fn source_matches_mode(ip: IpAddr, mode: &str) -> bool {
    if mode == "auto" {
        return source_matches_mode(ip, "lan_only") || source_matches_mode(ip, "tailscale_only");
    }
    match (ip, mode) {
        (IpAddr::V4(ip), "tailscale" | "tailscale_only") => {
            let octets = ip.octets();
            octets[0] == 100 && (64..=127).contains(&octets[1])
        }
        (IpAddr::V6(ip), "tailscale" | "tailscale_only") => {
            let segments = ip.segments();
            segments[0] == 0xfd7a && segments[1] == 0x115c && segments[2] == 0xa1e0
        }
        (IpAddr::V4(ip), "lan" | "lan_only") => {
            ip.is_private() || ip.is_link_local() || ip.is_loopback()
        }
        (IpAddr::V6(ip), "lan" | "lan_only") => {
            let first = ip.segments()[0];
            (first & 0xfe00) == 0xfc00 || ip.is_unicast_link_local() || ip.is_loopback()
        }
        _ => false,
    }
}

/// Infer the interface for an address: Tailscale ranges map to Tailscale,
/// everything else is treated as LAN.
pub fn infer_interface(address: &str) -> Result<ConnectionInterface, String> {
    let ip: IpAddr = address
        .parse()
        .map_err(|error| format!("Invalid peer address {address}: {error}"))?;
    if source_matches_mode(ip, "tailscale_only") {
        Ok(ConnectionInterface::Tailscale)
    } else {
        Ok(ConnectionInterface::Lan)
    }
}

/// Build the TCP socket address for a peer, using the platform peer port.
pub fn peer_socket_addr(peer: &PeerInfo, port: u16) -> Result<SocketAddr, String> {
    let address = if peer.address.is_empty() {
        &peer.tailscale_ip
    } else {
        &peer.address
    };
    let ip: IpAddr = address
        .parse()
        .map_err(|e| format!("Invalid peer address {address}: {e}"))?;
    Ok(SocketAddr::new(ip, port))
}

/// Where an outbound pairing attempt should connect: either a literal IP
/// (plain TCP) or a device endpoint ID (Iroh transport).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairingTarget {
    Tcp(IpAddr),
    Iroh(String),
}

/// Parse a user-supplied pairing address: an IP parses as a TCP target,
/// anything else is treated as an Iroh endpoint ID and canonicalized.
pub fn parse_pairing_target(address: &str) -> Result<PairingTarget, String> {
    let address = address.trim();
    match address.parse::<IpAddr>() {
        Ok(ip) => Ok(PairingTarget::Tcp(ip)),
        Err(_) => Ok(PairingTarget::Iroh(
            crate::iroh_transport::canonical_endpoint_id(address)?,
        )),
    }
}

/// Validate a pairing target against the connection mode: TCP targets must
/// be in the mode's address ranges, and Iroh pairing requires automatic mode.
pub fn validate_pairing_target(target: &PairingTarget, mode: &str) -> Result<(), String> {
    match target {
        PairingTarget::Tcp(ip) if !source_matches_mode(*ip, mode) => {
            Err("Peer address is outside the selected network".to_string())
        }
        PairingTarget::Iroh(_) if mode != "auto" => {
            Err("Iroh pairing is only available in automatic mode".to_string())
        }
        _ => Ok(()),
    }
}

fn sort_and_dedup_candidates(candidates: &mut Vec<PeerCandidate>) {
    candidates.sort_by(|left, right| {
        left.priority
            .cmp(&right.priority)
            .then_with(|| left.address.cmp(&right.address))
    });
    candidates
        .dedup_by(|left, right| left.interface == right.interface && left.address == right.address);
}

/// Merge the UDP and mDNS discovery snapshots into one LAN view. Local info
/// is combined (the first non-empty tailscale IP wins, candidates merge),
/// peers are merged by hostname, and every candidate list is ranked and
/// deduplicated.
pub fn merge_lan_discovery_results(
    udp_result: Result<(LocalInfo, Vec<PeerInfo>), String>,
    mdns_result: Result<(LocalInfo, Vec<PeerInfo>), String>,
) -> Result<(LocalInfo, Vec<PeerInfo>), String> {
    let mut local: Option<LocalInfo> = None;
    let mut peers = BTreeMap::<String, PeerInfo>::new();
    let mut errors = Vec::new();
    for (source, result) in [("udp", udp_result), ("mdns", mdns_result)] {
        match result {
            Ok((mut found_local, found_peers)) => {
                match &mut local {
                    Some(existing) => {
                        if existing.tailscale_ip.is_empty() && !found_local.tailscale_ip.is_empty()
                        {
                            existing.tailscale_ip.clone_from(&found_local.tailscale_ip);
                        }
                        existing.candidates.append(&mut found_local.candidates);
                    }
                    None => local = Some(found_local),
                }
                for peer in found_peers {
                    match peers.get_mut(&peer.hostname) {
                        Some(existing) => {
                            existing.online |= peer.online;
                            existing.candidates.extend(peer.candidates);
                        }
                        None => {
                            peers.insert(peer.hostname.clone(), peer);
                        }
                    }
                }
            }
            Err(error) => errors.push(format!("{source}: {error}")),
        }
    }
    let Some(mut local) = local else {
        return Err(format!("LAN discovery failed ({})", errors.join("; ")));
    };
    sort_and_dedup_candidates(&mut local.candidates);
    let mut peers = peers.into_values().collect::<Vec<_>>();
    for peer in &mut peers {
        sort_and_dedup_candidates(&mut peer.candidates);
        if let Some(candidate) = peer.candidates.first() {
            peer.address.clone_from(&candidate.address);
            peer.tailscale_ip.clone_from(&candidate.address);
        }
    }
    Ok((local, peers))
}

/// Merge the LAN and Tailscale discovery snapshots into the automatic-mode
/// view. Local info is combined with LAN preferred for hostname and IP;
/// peers are merged by hostname with candidates ranked and deduplicated.
pub fn merge_discovery_results(
    lan_result: Result<(LocalInfo, Vec<PeerInfo>), String>,
    tailscale_result: Result<(LocalInfo, Vec<PeerInfo>), String>,
) -> Result<(LocalInfo, Vec<PeerInfo>), String> {
    let mut local: Option<LocalInfo> = None;
    let mut merged = BTreeMap::<String, PeerInfo>::new();
    let mut errors = Vec::new();

    for (interface, result) in [
        (ConnectionInterface::Lan, lan_result),
        (ConnectionInterface::Tailscale, tailscale_result),
    ] {
        match result {
            Ok((mut found_local, peers)) => {
                if found_local.candidates.is_empty() && !found_local.tailscale_ip.is_empty() {
                    found_local.candidates.push(PeerCandidate::new(
                        interface,
                        found_local.tailscale_ip.clone(),
                    ));
                }
                match &mut local {
                    Some(existing) => {
                        if interface == ConnectionInterface::Lan {
                            existing.hostname.clone_from(&found_local.hostname);
                            if !found_local.tailscale_ip.is_empty() {
                                existing.tailscale_ip.clone_from(&found_local.tailscale_ip);
                            }
                        }
                        existing.candidates.append(&mut found_local.candidates);
                    }
                    None => local = Some(found_local),
                }
                for mut peer in peers {
                    let address = if peer.address.is_empty() {
                        peer.tailscale_ip.clone()
                    } else {
                        peer.address.clone()
                    };
                    if peer.candidates.is_empty() && !address.is_empty() {
                        peer.candidates.push(PeerCandidate::new(interface, address));
                    }
                    peer.connection_mode = "auto".to_string();
                    match merged.get_mut(&peer.hostname) {
                        Some(existing) => {
                            existing.online |= peer.online;
                            existing.candidates.extend(peer.candidates);
                        }
                        None => {
                            merged.insert(peer.hostname.clone(), peer);
                        }
                    }
                }
            }
            Err(error) => errors.push(format!("{}: {error}", interface.as_str())),
        }
    }

    let Some(mut local) = local else {
        return Err(format!(
            "Automatic discovery failed ({})",
            errors.join("; ")
        ));
    };
    sort_and_dedup_candidates(&mut local.candidates);
    let mut peers = merged.into_values().collect::<Vec<_>>();
    for peer in &mut peers {
        sort_and_dedup_candidates(&mut peer.candidates);
        if let Some(preferred) = peer.candidates.first() {
            peer.address.clone_from(&preferred.address);
        }
    }
    Ok((local, peers))
}

/// Whether a discovered peer matches a remembered address record. Used to
/// re-associate a peer whose device name changed since pairing.
fn peer_matches_remembered_addresses(
    peer: &PeerInfo,
    remembered: &HashMap<String, String>,
) -> bool {
    peer.candidates.iter().any(|candidate| {
        remembered
            .get(candidate.interface.as_str())
            .is_some_and(|address| address == &candidate.address)
    }) || [&peer.address, &peer.tailscale_ip]
        .into_iter()
        .any(|address| !address.is_empty() && remembered.values().any(|known| known == address))
}

/// Merge a second discovery record for the same hostname into the first,
/// keeping the ranked, deduplicated candidate list and the preferred address.
fn merge_peer_discovery(existing: &mut PeerInfo, mut peer: PeerInfo) {
    existing.online |= peer.online;
    existing.candidates.append(&mut peer.candidates);
    sort_and_dedup_candidates(&mut existing.candidates);
    if let Some(preferred) = existing.candidates.first() {
        existing.address.clone_from(&preferred.address);
        if preferred.interface == ConnectionInterface::Tailscale {
            existing.tailscale_ip.clone_from(&preferred.address);
        }
    } else if existing.address.is_empty() {
        existing.address = peer.address;
        existing.tailscale_ip = peer.tailscale_ip;
    }
}

/// Merge discovered peers with the trusted peers remembered in settings.
///
/// Rules (unified superset of the former platform copies):
/// - A discovered peer whose pinned routes uniquely match one remembered
///   record is re-associated with the paired hostname so the same physical
///   device is never listed twice.
/// - Discovery records for the same hostname are merged before pairing.
/// - `enabled`/`trusted`/`fingerprint` are backfilled from settings.
/// - Trusted peers are completed with every remembered interface candidate
///   allowed by the mode; settings-derived candidates start offline
///   (`PeerCandidate::remembered`).
/// - Remembered peers that were not discovered are appended as offline.
///
/// `rtt_capable` reports whether the platform knows the peer's Iroh endpoint
/// supports RTT probing; the platform binds this to its RTT capability set.
pub fn merge_paired_peers(
    settings: &Settings,
    mode: &str,
    discovered: Vec<PeerInfo>,
    rtt_capable: impl Fn(&str) -> bool,
) -> Vec<PeerInfo> {
    // Unknown modes (e.g. cache-only lookups) allow no remembered routes,
    // matching the historic `mode_interface` behavior.
    let mode_parsed = ConnectionMode::parse(mode);
    let allows = |interface| mode_parsed.map(|m| m.allows(interface)).unwrap_or(false);
    let mut discovered_by_hostname = BTreeMap::new();

    for mut peer in discovered {
        // A device name can change between pairing and later discovery (for
        // example, the TailSync hostname and Tailscale HostName may differ).
        // Re-associate it with the trusted record by its pinned route so the
        // UI does not show the same physical device twice.
        if !settings.trusted_peer_keys.contains_key(&peer.hostname) {
            let mut matching_hostnames = settings
                .trusted_peer_addresses
                .iter()
                .filter(|(_, remembered)| peer_matches_remembered_addresses(&peer, remembered))
                .map(|(hostname, _)| hostname.clone());
            if let (Some(hostname), None) = (matching_hostnames.next(), matching_hostnames.next()) {
                peer.hostname = hostname;
            }
        }

        match discovered_by_hostname.get_mut(&peer.hostname) {
            Some(existing) => merge_peer_discovery(existing, peer),
            None => {
                discovered_by_hostname.insert(peer.hostname.clone(), peer);
            }
        }
    }

    let mut discovered = discovered_by_hostname.into_values().collect::<Vec<_>>();
    let mut known_hostnames = HashSet::new();
    for peer in &mut discovered {
        known_hostnames.insert(peer.hostname.clone());
        peer.enabled = settings
            .enabled_peers
            .get(&peer.hostname)
            .copied()
            .unwrap_or(true);
        if let Some(encoded_key) = settings.trusted_peer_keys.get(&peer.hostname) {
            if let Ok(key) = identity::decode_public_key(encoded_key) {
                peer.trusted = true;
                peer.fingerprint = identity::fingerprint(&key);
            }
        }
        if peer.candidates.is_empty() {
            let address = if peer.address.is_empty() {
                &peer.tailscale_ip
            } else {
                &peer.address
            };
            if let Some(interface) = mode_interface(mode) {
                if !address.is_empty() {
                    peer.candidates
                        .push(PeerCandidate::new(interface, address.clone()));
                }
            }
        }
        if peer.trusted {
            if let Some(remembered) = settings.trusted_peer_addresses.get(&peer.hostname) {
                for interface in [
                    ConnectionInterface::Lan,
                    ConnectionInterface::Iroh,
                    ConnectionInterface::Tailscale,
                ] {
                    if !allows(interface) {
                        continue;
                    }
                    let Some(address) = remembered.get(interface.as_str()) else {
                        continue;
                    };
                    if !peer.candidates.iter().any(|candidate| {
                        candidate.interface == interface && candidate.address == *address
                    }) {
                        let mut candidate = PeerCandidate::remembered(interface, address);
                        if interface == ConnectionInterface::Iroh {
                            candidate.set_rtt_capable(rtt_capable(&candidate.address));
                        }
                        peer.candidates.push(candidate);
                    }
                }
            }
        }
        sort_and_dedup_candidates(&mut peer.candidates);
        if let Some(preferred) = peer.candidates.first() {
            peer.address.clone_from(&preferred.address);
        }
        if let Some(tailscale) = peer
            .candidates
            .iter()
            .find(|candidate| candidate.interface == ConnectionInterface::Tailscale)
        {
            peer.tailscale_ip.clone_from(&tailscale.address);
        }
    }

    for (hostname, encoded_key) in &settings.trusted_peer_keys {
        if known_hostnames.contains(hostname) {
            continue;
        }
        let remembered = settings.trusted_peer_addresses.get(hostname);
        let mut candidates = Vec::new();
        for interface in [
            ConnectionInterface::Lan,
            ConnectionInterface::Iroh,
            ConnectionInterface::Tailscale,
        ] {
            if !allows(interface) {
                continue;
            }
            if let Some(address) =
                remembered.and_then(|addresses| addresses.get(interface.as_str()))
            {
                let mut candidate = PeerCandidate::remembered(interface, address);
                if interface == ConnectionInterface::Iroh {
                    candidate.set_rtt_capable(rtt_capable(&candidate.address));
                }
                candidates.push(candidate);
            }
        }
        sort_and_dedup_candidates(&mut candidates);
        let address = candidates
            .first()
            .map(|candidate| candidate.address.clone())
            .unwrap_or_default();
        let fingerprint = identity::decode_public_key(encoded_key)
            .map(|key| identity::fingerprint(&key))
            .unwrap_or_default();
        discovered.push(PeerInfo {
            hostname: hostname.clone(),
            tailscale_ip: address.clone(),
            online: false,
            enabled: settings
                .enabled_peers
                .get(hostname)
                .copied()
                .unwrap_or(true),
            address,
            connection_mode: mode_parsed
                .map(|m| m.as_str().to_string())
                .unwrap_or_default(),
            trusted: true,
            fingerprint,
            candidates,
            current_interface: None,
            current_address: None,
            status: PeerStatus::Offline,
        });
    }
    discovered.sort_by(|left, right| left.hostname.cmp(&right.hostname));
    discovered
}

/// Resolves a peer's discovery candidates into concrete connection targets
/// (T111 migration). `tcp_port` is the platform peer port (19890). When a
/// peer has no candidates, one is synthesized from its remembered
/// address/tailscale IP under the mode-derived or inferred interface.
pub fn resolve_candidates(
    peer: &PeerInfo,
    tcp_port: u16,
) -> Result<Vec<ResolvedCandidate>, String> {
    let mut candidates = peer.candidates.clone();
    if candidates.is_empty() {
        let address = if peer.address.is_empty() {
            &peer.tailscale_ip
        } else {
            &peer.address
        };
        let interface = mode_interface(&peer.connection_mode)
            .or_else(|| infer_interface(address).ok())
            .ok_or_else(|| format!("Peer {} has no connection candidates", peer.hostname))?;
        candidates.push(PeerCandidate::new(interface, address));
    }
    candidates.sort_by_key(|candidate| candidate.priority);
    candidates
        .into_iter()
        .map(|candidate| {
            let target = match candidate.interface {
                ConnectionInterface::Iroh => ResolvedTarget::Iroh(
                    crate::iroh_transport::canonical_endpoint_id(&candidate.address)?,
                ),
                ConnectionInterface::Lan | ConnectionInterface::Tailscale => {
                    let ip: IpAddr = candidate.address.parse().map_err(|error| {
                        format!("Invalid peer address {}: {error}", candidate.address)
                    })?;
                    ResolvedTarget::Tcp(SocketAddr::new(ip, tcp_port))
                }
            };
            Ok(ResolvedCandidate { candidate, target })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::DeviceIdentity;
    use base64::{engine::general_purpose::STANDARD, Engine};

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
    fn connection_modes_only_accept_expected_address_ranges() {
        let tailscale: IpAddr = "100.96.1.2".parse().unwrap();
        let lan: IpAddr = "192.168.1.24".parse().unwrap();
        let public: IpAddr = "203.0.113.5".parse().unwrap();

        assert!(source_matches_mode(tailscale, "tailscale"));
        assert!(!source_matches_mode(lan, "tailscale"));
        assert!(source_matches_mode(lan, "lan"));
        assert!(!source_matches_mode(public, "lan"));
        assert!(source_matches_mode(tailscale, "auto"));
        assert!(source_matches_mode(lan, "auto"));
        assert!(!source_matches_mode(public, "auto"));
    }

    #[test]
    fn peer_socket_addr_supports_ipv6_addresses() {
        let peer = PeerInfo {
            hostname: "macbook".into(),
            tailscale_ip: "fd7a:115c:a1e0::1".into(),
            online: true,
            enabled: true,
            address: String::new(),
            connection_mode: "tailscale".into(),
            trusted: false,
            fingerprint: String::new(),
            candidates: Vec::new(),
            current_interface: None,
            current_address: None,
            status: PeerStatus::default(),
        };
        assert_eq!(
            peer_socket_addr(&peer, 19890).unwrap(),
            "[fd7a:115c:a1e0::1]:19890".parse().unwrap()
        );
    }

    #[test]
    fn paired_peer_with_remembered_address_survives_empty_discovery() {
        let identity = DeviceIdentity::generate_for_test();
        let mut settings = Settings {
            connection_mode: "lan".into(),
            ..Default::default()
        };
        settings
            .trusted_peer_keys
            .insert("windows".into(), STANDARD.encode(identity.public_key()));
        settings.trusted_peer_addresses.insert(
            "windows".into(),
            HashMap::from([("lan".into(), "192.168.1.20".into())]),
        );

        let peers = merge_paired_peers(&settings, "lan", Vec::new(), |_| false);

        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].hostname, "windows");
        assert_eq!(peers[0].address, "192.168.1.20");
        assert!(peers[0].trusted);
        assert!(!peers[0].online);
        assert!(peers[0]
            .candidates
            .iter()
            .all(|candidate| !candidate.online));
        assert!(peers[0].current_address.is_none());
        assert_eq!(peer_socket_addr(&peers[0], 19890).unwrap().port(), 19890);
    }

    #[test]
    fn automatic_mode_adds_iroh_between_lan_and_tailscale_only() {
        let identity = DeviceIdentity::generate_for_test();
        let mut settings = Settings::default();
        settings
            .trusted_peer_keys
            .insert("windows".into(), STANDARD.encode(identity.public_key()));
        settings.trusted_peer_addresses.insert(
            "windows".into(),
            HashMap::from([
                ("lan".into(), "192.168.1.20".into()),
                (
                    "iroh".into(),
                    "5866666666666666666666666666666666666666666666666666666666666666".into(),
                ),
                ("tailscale".into(), "100.64.0.2".into()),
            ]),
        );

        let automatic = merge_paired_peers(&settings, "auto", Vec::new(), |_| false);
        assert_eq!(
            automatic[0]
                .candidates
                .iter()
                .map(|candidate| candidate.interface)
                .collect::<Vec<_>>(),
            vec![
                ConnectionInterface::Lan,
                ConnectionInterface::Iroh,
                ConnectionInterface::Tailscale,
            ]
        );
        let lan_only = merge_paired_peers(&settings, "lan_only", Vec::new(), |_| false);
        assert_eq!(lan_only[0].candidates.len(), 1);
        assert_eq!(
            lan_only[0].candidates[0].interface,
            ConnectionInterface::Lan
        );
        let tailscale_only = merge_paired_peers(&settings, "tailscale_only", Vec::new(), |_| false);
        assert_eq!(tailscale_only[0].candidates.len(), 1);
        assert_eq!(
            tailscale_only[0].candidates[0].interface,
            ConnectionInterface::Tailscale
        );
    }

    #[test]
    fn discovered_alias_with_paired_address_is_not_listed_twice() {
        let identity = DeviceIdentity::generate_for_test();
        let mut settings = Settings {
            connection_mode: "tailscale_only".into(),
            ..Default::default()
        };
        settings
            .trusted_peer_keys
            .insert("Mac".into(), STANDARD.encode(identity.public_key()));
        settings.trusted_peer_addresses.insert(
            "Mac".into(),
            HashMap::from([("tailscale".into(), "100.111.236.101".into())]),
        );

        let peers = merge_paired_peers(
            &settings,
            "tailscale_only",
            vec![discovered_peer(
                "monet's MacBook Air",
                "100.111.236.101",
                ConnectionInterface::Tailscale,
            )],
            |_| false,
        );

        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].hostname, "Mac");
        assert_eq!(peers[0].address, "100.111.236.101");
        assert!(peers[0].online);
        assert!(peers[0].trusted);
    }

    #[test]
    fn automatic_discovery_merges_interfaces_and_prefers_lan() {
        let lan_local = LocalInfo {
            hostname: "macbook".into(),
            tailscale_ip: "192.168.1.10".into(),
            candidates: vec![PeerCandidate::new(ConnectionInterface::Lan, "192.168.1.10")],
        };
        let tailscale_local = LocalInfo {
            hostname: "macbook".into(),
            tailscale_ip: "100.64.0.1".into(),
            candidates: vec![PeerCandidate::new(
                ConnectionInterface::Tailscale,
                "100.64.0.1",
            )],
        };
        let (local, peers) = merge_discovery_results(
            Ok((
                lan_local,
                vec![discovered_peer(
                    "windows",
                    "192.168.1.20",
                    ConnectionInterface::Lan,
                )],
            )),
            Ok((
                tailscale_local,
                vec![discovered_peer(
                    "windows",
                    "100.64.0.2",
                    ConnectionInterface::Tailscale,
                )],
            )),
        )
        .unwrap();

        assert_eq!(local.candidates.len(), 2);
        assert_eq!(local.candidates[0].interface, ConnectionInterface::Lan);
        assert_eq!(
            local.candidates[1].interface,
            ConnectionInterface::Tailscale
        );
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].address, "192.168.1.20");
        assert_eq!(peers[0].candidates.len(), 2);
        assert_eq!(peers[0].candidates[0].interface, ConnectionInterface::Lan);
        assert_eq!(
            peers[0].candidates[1].interface,
            ConnectionInterface::Tailscale
        );
    }

    #[test]
    fn automatic_discovery_survives_one_unavailable_interface() {
        let local = LocalInfo {
            hostname: "macbook".into(),
            tailscale_ip: "100.64.0.1".into(),
            candidates: vec![PeerCandidate::new(
                ConnectionInterface::Tailscale,
                "100.64.0.1",
            )],
        };
        let (_, peers) = merge_discovery_results(
            Err("UDP blocked".into()),
            Ok((
                local,
                vec![discovered_peer(
                    "windows",
                    "100.64.0.2",
                    ConnectionInterface::Tailscale,
                )],
            )),
        )
        .unwrap();

        assert_eq!(peers.len(), 1);
        assert_eq!(
            peers[0].candidates[0].interface,
            ConnectionInterface::Tailscale
        );
    }

    #[test]
    fn mdns_and_udp_results_are_deduplicated_without_losing_udp_compatibility() {
        let local = LocalInfo {
            hostname: "macbook".into(),
            tailscale_ip: "192.168.1.10".into(),
            candidates: vec![PeerCandidate::new(ConnectionInterface::Lan, "192.168.1.10")],
        };
        let udp_peer = discovered_peer("windows", "192.168.1.20", ConnectionInterface::Lan);
        let mdns_peer = udp_peer.clone();
        let (_, peers) = merge_lan_discovery_results(
            Ok((local.clone(), vec![udp_peer])),
            Ok((local, vec![mdns_peer])),
        )
        .unwrap();

        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].hostname, "windows");
        assert_eq!(peers[0].candidates.len(), 1);
        assert_eq!(peers[0].candidates[0].address, "192.168.1.20");
    }

    #[test]
    fn mdns_only_candidate_is_discovered_but_not_online() {
        let local = LocalInfo {
            hostname: "macbook".into(),
            tailscale_ip: "192.168.1.10".into(),
            candidates: Vec::new(),
        };
        let mut mdns_peer = discovered_peer("windows", "192.168.1.20", ConnectionInterface::Lan);
        mdns_peer.online = false;
        mdns_peer.status = PeerStatus::Discovered;
        mdns_peer.candidates = vec![PeerCandidate::remembered(
            ConnectionInterface::Lan,
            "192.168.1.20",
        )];

        let (_, peers) = merge_lan_discovery_results(
            Ok((local.clone(), Vec::new())),
            Ok((local, vec![mdns_peer])),
        )
        .unwrap();

        assert_eq!(peers.len(), 1);
        assert!(!peers[0].online);
        assert_eq!(peers[0].status, PeerStatus::Discovered);
        assert!(!peers[0].candidates[0].online);
    }

    #[test]
    fn local_info_merging_backfills_tailscale_ip_and_candidates() {
        let udp_local = LocalInfo {
            hostname: "macbook".into(),
            tailscale_ip: String::new(),
            candidates: vec![PeerCandidate::new(ConnectionInterface::Lan, "192.168.1.10")],
        };
        let mdns_local = LocalInfo {
            hostname: "macbook".into(),
            tailscale_ip: "192.168.1.10".into(),
            candidates: vec![PeerCandidate::new(ConnectionInterface::Lan, "192.168.1.10")],
        };
        let (local, _) =
            merge_lan_discovery_results(Ok((udp_local, Vec::new())), Ok((mdns_local, Vec::new())))
                .unwrap();

        assert_eq!(local.tailscale_ip, "192.168.1.10");
        assert_eq!(local.candidates.len(), 1);
        assert_eq!(local.candidates[0].interface, ConnectionInterface::Lan);
    }

    #[test]
    fn trusted_peer_candidates_are_completed_from_all_remembered_interfaces() {
        let identity = DeviceIdentity::generate_for_test();
        let mut settings = Settings {
            connection_mode: "auto".into(),
            ..Default::default()
        };
        settings
            .trusted_peer_keys
            .insert("windows".into(), STANDARD.encode(identity.public_key()));
        settings.trusted_peer_addresses.insert(
            "windows".into(),
            HashMap::from([
                ("lan".into(), "192.168.1.20".into()),
                (
                    "iroh".into(),
                    "5866666666666666666666666666666666666666666666666666666666666666".into(),
                ),
                ("tailscale".into(), "100.64.0.2".into()),
            ]),
        );

        // The discovered record only knows the LAN route; the merged view
        // must complete it with the remembered Iroh and Tailscale routes.
        let peers = merge_paired_peers(
            &settings,
            "auto",
            vec![discovered_peer(
                "windows",
                "192.168.1.20",
                ConnectionInterface::Lan,
            )],
            |_| false,
        );

        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].hostname, "windows");
        assert_eq!(peers[0].candidates.len(), 3);
        assert_eq!(
            peers[0]
                .candidates
                .iter()
                .map(|candidate| candidate.interface)
                .collect::<Vec<_>>(),
            vec![
                ConnectionInterface::Lan,
                ConnectionInterface::Iroh,
                ConnectionInterface::Tailscale,
            ]
        );
        // Settings-derived candidates start offline and keep the peer's own
        // discovered online status untouched.
        assert!(peers[0].online);
        assert!(!peers[0].candidates[1].online);
        assert_eq!(peers[0].tailscale_ip, "100.64.0.2");
    }

    #[test]
    fn pairing_target_parses_ips_and_endpoint_ids() {
        assert_eq!(
            parse_pairing_target("192.168.1.20").unwrap(),
            PairingTarget::Tcp("192.168.1.20".parse().unwrap())
        );
        assert_eq!(
            parse_pairing_target(" 100.64.0.2 ").unwrap(),
            PairingTarget::Tcp("100.64.0.2".parse().unwrap())
        );
        let endpoint = "5866666666666666666666666666666666666666666666666666666666666666";
        assert_eq!(
            parse_pairing_target(endpoint).unwrap(),
            PairingTarget::Iroh(endpoint.to_string())
        );
        assert!(parse_pairing_target("not-an-endpoint").is_err());
    }

    #[test]
    fn pairing_target_validation_enforces_mode_ranges() {
        let lan = PairingTarget::Tcp("192.168.1.20".parse().unwrap());
        let tailscale = PairingTarget::Tcp("100.64.0.2".parse().unwrap());
        let iroh = PairingTarget::Iroh(
            "5866666666666666666666666666666666666666666666666666666666666666".into(),
        );

        assert!(validate_pairing_target(&lan, "lan").is_ok());
        assert!(validate_pairing_target(&lan, "tailscale").is_err());
        assert!(validate_pairing_target(&tailscale, "tailscale").is_ok());
        assert!(validate_pairing_target(&iroh, "auto").is_ok());
        assert!(validate_pairing_target(&iroh, "lan").is_err());
        assert_eq!(
            validate_pairing_target(&lan, "tailscale").unwrap_err(),
            "Peer address is outside the selected network"
        );
        assert_eq!(
            validate_pairing_target(&iroh, "lan").unwrap_err(),
            "Iroh pairing is only available in automatic mode"
        );
    }

    // ------------------------------------------------------------------
    // resolve_candidates (T111): peer candidates -> connection targets.
    // ------------------------------------------------------------------

    fn peer_with_candidates(hostname: &str, candidates: Vec<PeerCandidate>) -> PeerInfo {
        PeerInfo {
            hostname: hostname.into(),
            tailscale_ip: String::new(),
            online: true,
            enabled: true,
            address: String::new(),
            connection_mode: "auto".into(),
            trusted: true,
            fingerprint: String::new(),
            candidates,
            current_interface: None,
            current_address: None,
            status: PeerStatus::Online,
        }
    }

    #[test]
    fn resolve_candidates_maps_and_sorts_existing_candidates() {
        let low = PeerCandidate::new(ConnectionInterface::Tailscale, "100.101.102.103");
        let high = PeerCandidate::new(ConnectionInterface::Lan, "192.168.1.5");
        let peer = peer_with_candidates("mac", vec![low, high]);

        let resolved = resolve_candidates(&peer, 19890).unwrap();
        // LAN has a higher priority and must sort first.
        assert_eq!(resolved.len(), 2);
        assert_eq!(resolved[0].candidate.interface, ConnectionInterface::Lan);
        assert_eq!(
            resolved[0].target,
            ResolvedTarget::Tcp("192.168.1.5:19890".parse().unwrap())
        );
        assert_eq!(
            resolved[1].candidate.interface,
            ConnectionInterface::Tailscale
        );
        assert_eq!(
            resolved[1].target,
            ResolvedTarget::Tcp("100.101.102.103:19890".parse().unwrap())
        );
    }

    #[test]
    fn resolve_candidates_synthesizes_candidate_without_discovery() {
        let mut peer = peer_with_candidates("mac", vec![]);
        peer.address = "192.168.1.9".into();
        peer.connection_mode = "lan_only".into();

        let resolved = resolve_candidates(&peer, 19890).unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].candidate.interface, ConnectionInterface::Lan);
        assert_eq!(resolved[0].candidate.address, "192.168.1.9");
        assert_eq!(
            resolved[0].target,
            ResolvedTarget::Tcp("192.168.1.9:19890".parse().unwrap())
        );
    }

    #[test]
    fn resolve_candidates_uses_tailscale_ip_when_address_is_empty() {
        let mut peer = peer_with_candidates("mac", vec![]);
        peer.tailscale_ip = "100.64.1.2".into();
        peer.connection_mode = "tailscale_only".into();

        let resolved = resolve_candidates(&peer, 19890).unwrap();
        assert_eq!(
            resolved[0].candidate.interface,
            ConnectionInterface::Tailscale
        );
        assert_eq!(resolved[0].candidate.address, "100.64.1.2");
    }

    #[test]
    fn resolve_candidates_fails_without_any_usable_address() {
        let peer = peer_with_candidates("mac", vec![]);
        let error = resolve_candidates(&peer, 19890).unwrap_err();
        assert_eq!(error, "Peer mac has no connection candidates");
    }

    #[test]
    fn resolve_candidates_rejects_invalid_tcp_addresses() {
        let peer = peer_with_candidates(
            "mac",
            vec![PeerCandidate::new(ConnectionInterface::Lan, "not-an-ip")],
        );
        let error = resolve_candidates(&peer, 19890).unwrap_err();
        assert!(error.contains("Invalid peer address not-an-ip"));
    }

    #[test]
    fn resolve_candidates_rejects_invalid_iroh_endpoints() {
        let peer = peer_with_candidates(
            "mac",
            vec![PeerCandidate::new(
                ConnectionInterface::Iroh,
                "not-an-endpoint",
            )],
        );
        assert!(resolve_candidates(&peer, 19890).is_err());
    }
}
