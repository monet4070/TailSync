use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::time::Duration;

use super::lan;
use super::tailscale::{LocalInfo, PeerInfo};
use super::{source_matches_mode, ConnectionInterface, PeerCandidate, PeerStatus, TCP_PORT};
use crate::identity::DeviceIdentity;

const SERVICE_TYPE: &str = "_tailsync._tcp.local.";

#[derive(Clone)]
struct MdnsRecord {
    hostname: String,
    addresses: Vec<String>,
}

static CACHE: OnceLock<Mutex<HashMap<String, MdnsRecord>>> = OnceLock::new();

fn cache() -> &'static Mutex<HashMap<String, MdnsRecord>> {
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn is_remote_service(
    fullname: &str,
    fingerprint: Option<&str>,
    port: u16,
    local_fullname: &str,
    local_fingerprint: &str,
) -> bool {
    fullname != local_fullname && fingerprint != Some(local_fingerprint) && port == TCP_PORT
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
    let local_fingerprint = identity.fingerprint();
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
                if !is_remote_service(
                    service.get_fullname(),
                    service.get_property_val_str("fingerprint"),
                    service.get_port(),
                    &local_fullname,
                    &local_fingerprint,
                ) {
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
                cache()
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .insert(
                        service.get_fullname().to_string(),
                        MdnsRecord {
                            hostname,
                            addresses,
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
    let properties = [
        ("protocol", "2"),
        ("hostname", hostname.as_str()),
        ("fingerprint", fingerprint.as_str()),
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
        let candidates = record
            .addresses
            .iter()
            .map(|address| PeerCandidate::remembered(ConnectionInterface::Lan, address))
            .collect::<Vec<_>>();
        let Some(address) = record.addresses.first().cloned() else {
            continue;
        };
        peers.push(PeerInfo {
            hostname: record.hostname,
            tailscale_ip: address.clone(),
            online: false,
            enabled: true,
            address,
            connection_mode: "lan".to_string(),
            trusted: false,
            fingerprint: String::new(),
            candidates,
            current_interface: None,
            current_address: None,
            status: PeerStatus::Discovered,
        });
    }
    peers.sort_by(|left, right| left.hostname.cmp(&right.hostname));
    (
        LocalInfo {
            hostname: lan::local_hostname(),
            tailscale_ip: String::new(),
        },
        peers,
    )
}

#[cfg(test)]
mod tests {
    use super::{build_service_info, dns_label, is_remote_service, SERVICE_TYPE};
    use crate::identity::DeviceIdentity;
    use crate::network::TCP_PORT;

    #[test]
    fn published_service_has_expected_type_port_and_security_metadata() {
        let identity = DeviceIdentity::generate_for_test();
        let service = build_service_info(&identity).unwrap();

        assert!(service.get_fullname().ends_with(SERVICE_TYPE));
        assert_eq!(service.get_port(), TCP_PORT);
        assert_eq!(service.get_property_val_str("protocol"), Some("2"));
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
    fn discovery_rejects_self_by_fullname_or_device_fingerprint() {
        assert!(!is_remote_service(
            "macbook._tailsync._tcp.local.",
            Some("self-fingerprint"),
            TCP_PORT,
            "macbook._tailsync._tcp.local.",
            "self-fingerprint",
        ));
        assert!(!is_remote_service(
            "stale-macbook._tailsync._tcp.local.",
            Some("self-fingerprint"),
            TCP_PORT,
            "macbook._tailsync._tcp.local.",
            "self-fingerprint",
        ));
        assert!(is_remote_service(
            "windows._tailsync._tcp.local.",
            Some("peer-fingerprint"),
            TCP_PORT,
            "macbook._tailsync._tcp.local.",
            "self-fingerprint",
        ));
    }
}
