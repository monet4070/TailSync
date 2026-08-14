use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::time::Duration;

use super::lan;
use super::tailscale::{LocalInfo, PeerInfo};
use super::{source_matches_mode, ConnectionInterface, PeerCandidate, TCP_PORT};
use crate::identity::DeviceIdentity;

const SERVICE_TYPE: &str = "_tailsync._tcp.local.";

#[derive(Clone)]
struct MdnsRecord {
    hostname: String,
    addresses: Vec<String>,
    iroh_endpoint_id: Option<String>,
}

static CACHE: OnceLock<Mutex<HashMap<String, MdnsRecord>>> = OnceLock::new();

fn cache() -> &'static Mutex<HashMap<String, MdnsRecord>> {
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub async fn run(identity: Arc<DeviceIdentity>) {
    loop {
        if let Err(error) = run_once(&identity).await {
            log::warn!("mDNS discovery stopped: {error}; retrying");
        }
        cache()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

async fn run_once(identity: &DeviceIdentity) -> Result<(), String> {
    let daemon = ServiceDaemon::new().map_err(|error| error.to_string())?;
    let service = build_service_info(identity)?;
    let local_fullname = service.get_fullname().to_string();
    daemon
        .register(service)
        .map_err(|error| error.to_string())?;
    let receiver = daemon
        .browse(SERVICE_TYPE)
        .map_err(|error| error.to_string())?;
    log::info!("mDNS service registered as {local_fullname}");

    while let Ok(event) = receiver.recv_async().await {
        match event {
            ServiceEvent::ServiceResolved(service) => {
                if service.get_fullname() == local_fullname || service.get_port() != TCP_PORT {
                    continue;
                }
                if service.get_property_val_str("protocol") != Some("2") {
                    continue;
                }
                let hostname = service
                    .get_property_val_str("hostname")
                    .unwrap_or(service.get_hostname().trim_end_matches('.'))
                    .trim()
                    .to_string();
                if hostname.is_empty() || hostname == lan::local_hostname() {
                    continue;
                }
                let mut addresses = service
                    .get_addresses_v4()
                    .into_iter()
                    .map(IpAddr::V4)
                    .filter(|address| source_matches_mode(*address, "lan_only"))
                    .map(|address| address.to_string())
                    .collect::<Vec<_>>();
                addresses.sort();
                addresses.dedup();
                if addresses.is_empty() {
                    continue;
                }
                let iroh_endpoint_id = service.get_property_val_str("iroh").and_then(|value| {
                    tailsync_core::iroh_transport::canonical_endpoint_id(value).ok()
                });
                if service.get_property_val_str("iroh_rtt") == Some("1") {
                    if let Some(endpoint_id) = &iroh_endpoint_id {
                        super::iroh::remember_rtt_capability(endpoint_id);
                    }
                }
                cache()
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .insert(
                        service.get_fullname().to_string(),
                        MdnsRecord {
                            hostname,
                            addresses,
                            iroh_endpoint_id,
                        },
                    );
            }
            ServiceEvent::ServiceRemoved(_, fullname) => {
                cache()
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .remove(&fullname);
            }
            _ => {}
        }
    }
    let _ = daemon.shutdown();
    Err("mDNS event channel closed".to_string())
}

fn build_service_info(identity: &DeviceIdentity) -> Result<ServiceInfo, String> {
    let hostname = lan::local_hostname();
    let service_hostname = format!("{}.local.", dns_label(&hostname));
    let fingerprint = identity.fingerprint();
    let iroh_endpoint_id = super::iroh::local_endpoint_id().unwrap_or_default();
    let properties = [
        ("protocol", "2"),
        ("hostname", hostname.as_str()),
        ("fingerprint", fingerprint.as_str()),
        ("iroh", iroh_endpoint_id.as_str()),
        ("iroh_rtt", "1"),
    ];
    ServiceInfo::new(
        SERVICE_TYPE,
        &hostname,
        &service_hostname,
        "",
        TCP_PORT,
        &properties[..],
    )
    .map(ServiceInfo::enable_addr_auto)
    .map_err(|error| error.to_string())
}

fn dns_label(hostname: &str) -> String {
    let label = hostname
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .chars()
        .take(63)
        .collect::<String>();
    if label.is_empty() {
        "tailsync-device".to_string()
    } else {
        label
    }
}

pub fn snapshot() -> (LocalInfo, Vec<PeerInfo>) {
    let records = cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .values()
        .cloned()
        .collect::<Vec<_>>();
    let mut peers = Vec::new();
    for record in records {
        let mut candidates = record
            .addresses
            .iter()
            .map(|address| PeerCandidate::new(ConnectionInterface::Lan, address))
            .collect::<Vec<_>>();
        if let Some(endpoint_id) = record.iroh_endpoint_id {
            let mut candidate = PeerCandidate::new(ConnectionInterface::Iroh, endpoint_id);
            candidate.set_rtt_capable(super::iroh::supports_rtt(&candidate.address));
            candidates.push(candidate);
        }
        let Some(address) = record.addresses.first().cloned() else {
            continue;
        };
        peers.push(PeerInfo {
            hostname: record.hostname,
            tailscale_ip: address.clone(),
            // mDNS proves that an address was discovered, not that the peer is
            // currently reachable. The shared health monitor owns online state.
            online: false,
            enabled: true,
            address,
            connection_mode: "lan".to_string(),
            trusted: false,
            fingerprint: String::new(),
            candidates,
            current_interface: None,
        });
    }
    peers.sort_by(|left, right| left.hostname.cmp(&right.hostname));
    (
        LocalInfo {
            hostname: lan::local_hostname(),
            tailscale_ip: String::new(),
            candidates: Vec::new(),
        },
        peers,
    )
}

#[cfg(test)]
mod tests {
    use super::{build_service_info, cache, dns_label, snapshot, MdnsRecord, SERVICE_TYPE};
    use crate::identity::DeviceIdentity;
    use crate::network::TCP_PORT;

    #[test]
    fn published_service_has_expected_type_port_and_security_metadata() {
        let identity = DeviceIdentity::generate_for_test();
        let service = build_service_info(&identity).unwrap();

        assert!(service.get_fullname().ends_with(SERVICE_TYPE));
        assert_eq!(service.get_port(), TCP_PORT);
        assert_eq!(service.get_property_val_str("protocol"), Some("2"));
        assert_eq!(service.get_property_val_str("iroh_rtt"), Some("1"));
        assert_eq!(
            service.get_property_val_str("fingerprint"),
            Some(identity.fingerprint().as_str())
        );
    }

    #[test]
    fn service_hostname_is_a_valid_dns_label() {
        assert_eq!(dns_label("My Laptop (Work)"), "my-laptop--work");
        assert_eq!(dns_label("***"), "tailsync-device");
        assert!(dns_label(&"a".repeat(100)).len() <= 63);
    }

    #[test]
    fn cached_mdns_record_is_discovered_but_not_online() {
        let key = "health-test._tailsync._tcp.local.".to_string();
        cache()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(
                key.clone(),
                MdnsRecord {
                    hostname: "mdns-health-test".to_string(),
                    addresses: vec!["192.168.250.10".to_string()],
                    iroh_endpoint_id: None,
                },
            );

        let (_, peers) = snapshot();
        let peer = peers
            .iter()
            .find(|peer| peer.hostname == "mdns-health-test")
            .expect("cached mDNS peer");
        assert!(!peer.online);
        assert_eq!(peer.candidates[0].latency, None);

        cache()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&key);
    }
}
