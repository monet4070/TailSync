use super::{
    bind_api_listener, bump_runtime_revision, clear_file_progress, clear_file_progress_scope,
    get_file_progress, get_runtime_revision, history_capabilities_data, peer_snapshot_data,
    read_request_with_limits, set_file_batch_progress, wait_for_runtime_revision, ApiToken,
    FileProgress, Request,
};
use crate::crypto::Settings;
use crate::identity::DeviceIdentity;
use crate::network::tailscale::{LocalInfo, PeerInfo};
use crate::network::{register_active_session, ConnectionInterface, PeerCandidate};
use std::collections::HashMap;
use std::time::Duration;

fn progress(batch_id: &str, device: &str, sent: u64) -> FileProgress {
    FileProgress {
        batch_id: batch_id.into(),
        name: "file.bin".into(),
        sent,
        total: 100,
        active: true,
        direction: "receiving".into(),
        device: device.into(),
        completed_files: 0,
        total_files: 1,
        speed_bytes_per_second: 0,
        status: "transferring".into(),
        can_stop: true,
    }
}

#[test]
fn progress_scope_keeps_other_concurrent_devices_visible() {
    clear_file_progress();
    set_file_batch_progress(progress("batch-a", "peer-a", 25));
    set_file_batch_progress(progress("batch-b", "peer-b", 50));
    clear_file_progress_scope(Some("batch-b"), Some("peer-b"));
    let remaining = get_file_progress().unwrap();
    assert_eq!(remaining.batch_id, "batch-a");
    assert_eq!(remaining.device, "peer-a");
    clear_file_progress();
}

#[tokio::test]
async fn runtime_revision_waiter_wakes_on_change() {
    let current = get_runtime_revision();
    let waiter =
        tokio::spawn(
            async move { wait_for_runtime_revision(current, Duration::from_secs(1)).await },
        );
    tokio::task::yield_now().await;
    bump_runtime_revision();
    let changed = waiter.await.unwrap();
    assert_ne!(changed, current);
}

#[tokio::test]
async fn api_listener_recovers_after_the_address_is_released() {
    let blocker = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = blocker.local_addr().unwrap();
    let (_shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
    let waiter = tokio::spawn(async move {
        bind_api_listener(address, &mut shutdown_rx, Duration::from_millis(10)).await
    });

    tokio::time::sleep(Duration::from_millis(30)).await;
    drop(blocker);

    let listener = tokio::time::timeout(Duration::from_secs(2), waiter)
        .await
        .expect("API listener did not retry after the address was released")
        .expect("API listener retry task failed")
        .expect("API listener stopped without a shutdown request");
    assert_eq!(listener.local_addr().unwrap(), address);
}

#[tokio::test]
async fn api_listener_bind_retry_stops_for_shutdown() {
    let blocker = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = blocker.local_addr().unwrap();
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
    let waiter = tokio::spawn(async move {
        bind_api_listener(address, &mut shutdown_rx, Duration::from_secs(30)).await
    });

    tokio::time::sleep(Duration::from_millis(30)).await;
    shutdown_tx.send(true).unwrap();

    let listener = tokio::time::timeout(Duration::from_secs(1), waiter)
        .await
        .expect("API listener bind retry ignored shutdown")
        .expect("API listener retry task failed");
    assert!(listener.is_none());
}

#[test]
fn api_token_requires_exact_hex_and_compares_all_bytes() {
    let token = ApiToken::parse(&"ab".repeat(32)).unwrap();
    assert!(token.matches(Some(&"ab".repeat(32))));
    assert!(!token.matches(Some(&format!("{}ac", "ab".repeat(31)))));
    assert!(!token.matches(Some("ab")));
    assert!(!token.matches(None));
}

#[tokio::test]
async fn api_reader_rejects_oversized_and_incomplete_requests() {
    let oversized = format!("{{\"cmd\":\"{}\"}}\n", "x".repeat(64));
    let error = read_request_with_limits(oversized.as_bytes(), 32, Duration::from_millis(100))
        .await
        .expect_err("oversized request must be rejected");
    assert!(error.contains("exceeds 32 byte limit"));

    let error = read_request_with_limits(
        b"{\"cmd\":\"get_version\"}".as_slice(),
        64,
        Duration::from_millis(100),
    )
    .await
    .expect_err("request without a newline must be rejected");
    assert_eq!(error, "incomplete request");
}

#[tokio::test]
async fn api_reader_times_out_slow_clients() {
    let (_writer, reader) = tokio::io::duplex(64);
    let error = read_request_with_limits(reader, 64, Duration::from_millis(10))
        .await
        .expect_err("silent client must time out");
    assert_eq!(error, "request read timed out");
}

#[test]
fn history_capabilities_advertise_multi_label_and_date_contracts() {
    let capabilities = history_capabilities_data();
    assert_eq!(
        capabilities["classifier_version"].as_i64(),
        Some(crate::history_classifier::CLASSIFIER_VERSION)
    );
    assert_eq!(capabilities["multiple_labels"].as_bool(), Some(true));
    assert_eq!(capabilities["date_range_filter"].as_bool(), Some(true));
    assert_eq!(
        capabilities["categories"].as_array().map(Vec::len),
        Some(crate::history_classifier::CATEGORIES.len())
    );
}

#[test]
fn history_request_accepts_new_filters_and_legacy_omissions() {
    let filtered: Request = serde_json::from_value(serde_json::json!({
        "cmd": "get_history",
        "keyword": "needle",
        "category": "text",
        "start_time": "2026-02-01T10:00:00Z",
        "end_time": "2026-02-01T11:00:00Z",
        "collection": "favorites",
        "limit": 31,
        "offset": 62
    }))
    .unwrap();
    assert_eq!(filtered.keyword.as_deref(), Some("needle"));
    assert_eq!(filtered.category.as_deref(), Some("text"));
    assert_eq!(filtered.start_time.as_deref(), Some("2026-02-01T10:00:00Z"));
    assert_eq!(filtered.end_time.as_deref(), Some("2026-02-01T11:00:00Z"));
    assert_eq!(filtered.collection.as_deref(), Some("favorites"));
    assert_eq!(filtered.limit, Some(31));
    assert_eq!(filtered.offset, Some(62));

    let legacy: Request =
        serde_json::from_value(serde_json::json!({ "cmd": "get_history" })).unwrap();
    assert!(legacy.keyword.is_none());
    assert!(legacy.category.is_none());
    assert!(legacy.start_time.is_none());
    assert!(legacy.end_time.is_none());
    assert!(legacy.collection.is_none());

    let favorite: Request = serde_json::from_value(serde_json::json!({
        "cmd": "set_history_favorite",
        "id": 17,
        "favorite": false
    }))
    .unwrap();
    assert_eq!(favorite.id, Some(17));
    assert_eq!(favorite.favorite, Some(false));
}

#[test]
fn peer_snapshot_keeps_local_identity_when_discovery_fails() {
    let identity = DeviceIdentity::generate_for_test();
    let settings = Settings::default();
    let data = peer_snapshot_data(
        &identity,
        &settings,
        Err("Tailscale status unavailable".to_string()),
    );
    let public_key = identity.public_key_base64();
    let fingerprint = identity.fingerprint();

    let local = data["self"].as_object().expect("local device snapshot");
    assert!(!local["hostname"].as_str().unwrap_or_default().is_empty());
    assert_eq!(local["public_key"].as_str(), Some(public_key.as_str()));
    assert_eq!(local["fingerprint"].as_str(), Some(fingerprint.as_str()));
    assert_eq!(data["peers"].as_array().map(Vec::len), Some(0));
    assert_eq!(
        data["discovery_error"].as_str(),
        Some("Tailscale status unavailable")
    );
}

#[test]
fn automatic_snapshot_does_not_treat_discovery_flags_as_online_health() {
    let identity = DeviceIdentity::generate_for_test();
    let remote = DeviceIdentity::generate_for_test();
    let mut settings = Settings {
        connection_mode: "auto".into(),
        ..Settings::default()
    };
    settings
        .trusted_peer_keys
        .insert("Mac".into(), remote.public_key_base64());
    settings.trusted_peer_addresses.insert(
        "Mac".into(),
        HashMap::from([
            ("lan".into(), "192.168.31.247".into()),
            ("tailscale".into(), "100.111.236.101".into()),
        ]),
    );
    settings
        .paired_peer_endpoints
        .insert("Mac".into(), "192.168.31.247".into());
    let data = peer_snapshot_data(
        &identity,
        &settings,
        Ok((
            LocalInfo {
                hostname: "windows".into(),
                tailscale_ip: "192.168.31.78".into(),
                candidates: vec![PeerCandidate::new(
                    ConnectionInterface::Lan,
                    "192.168.31.78",
                )],
            },
            vec![PeerInfo {
                hostname: "Mac".into(),
                tailscale_ip: "192.168.31.247".into(),
                online: false,
                enabled: true,
                address: "192.168.31.247".into(),
                connection_mode: "auto".into(),
                trusted: false,
                fingerprint: String::new(),
                candidates: vec![PeerCandidate::new(
                    ConnectionInterface::Lan,
                    "192.168.31.247",
                )],
                current_interface: None,
                current_address: None,
                status: Default::default(),
            }],
        )),
    );

    let peer = &data["peers"][0];
    assert_eq!(data["self"]["routes"].as_array().map(Vec::len), Some(1));
    assert_eq!(peer["trusted"].as_bool(), Some(true));
    let routes = peer["routes"].as_array().expect("peer routes");
    assert_eq!(routes.len(), 2);
    assert_eq!(routes[0]["interface"].as_str(), Some("lan"));
    assert_eq!(routes[0]["status"].as_str(), Some("discovered"));
    assert_eq!(routes[0]["online"].as_bool(), Some(false));
    assert_eq!(routes[0]["connected"].as_bool(), Some(false));
    assert_eq!(routes[0]["pairing_endpoint"].as_bool(), Some(true));
    assert_eq!(routes[0]["rtt_capable"].as_bool(), Some(true));
    assert_eq!(routes[1]["interface"].as_str(), Some("tailscale"));
    assert_eq!(routes[1]["address"].as_str(), Some("100.111.236.101"));
    assert_eq!(routes[1]["online"].as_bool(), Some(false));
    assert_eq!(routes[1]["connected"].as_bool(), Some(false));
    assert_eq!(routes[1]["pairing_endpoint"].as_bool(), Some(false));
    assert_eq!(routes[1]["rtt_capable"].as_bool(), Some(true));
    assert_eq!(
        data["paired_peer_endpoints"]["Mac"].as_str(),
        Some("192.168.31.247")
    );
}

#[test]
fn peer_snapshot_exposes_an_actionable_protocol_upgrade_diagnostic() {
    let identity = DeviceIdentity::generate_for_test();
    let remote = DeviceIdentity::generate_for_test();
    let hostname = format!("protocol-snapshot-test-{}", rand::random::<u64>());
    let address = "192.168.252.31";
    let mut settings = Settings::default();
    settings
        .trusted_peer_keys
        .insert(hostname.clone(), remote.public_key_base64());
    settings.trusted_peer_addresses.insert(
        hostname.clone(),
        HashMap::from([("lan".into(), address.into())]),
    );
    crate::network::record_protocol_compatibility_error(
        &hostname,
        "Incompatible TailSync protocol: peer uses v2",
    );

    let data = peer_snapshot_data(
        &identity,
        &settings,
        Ok((
            LocalInfo {
                hostname: "windows".into(),
                tailscale_ip: String::new(),
                candidates: Vec::new(),
            },
            Vec::new(),
        )),
    );
    crate::network::clear_protocol_compatibility_error(&hostname);

    let peer = data["peers"]
        .as_array()
        .and_then(|peers| peers.iter().find(|peer| peer["hostname"] == hostname))
        .expect("trusted peer snapshot");
    assert_eq!(
        peer["protocol_error"].as_str(),
        Some("Incompatible TailSync protocol: peer uses v2")
    );
    assert_eq!(
        peer["required_protocol_version"].as_u64(),
        Some(crate::protocol::VERSION.into())
    );
}

#[test]
fn automatic_snapshot_connects_only_the_exact_authenticated_route() {
    let identity = DeviceIdentity::generate_for_test();
    let remote = DeviceIdentity::generate_for_test();
    let hostname = "snapshot-route-session-test";
    let lan_address = "192.168.251.31";
    let tailscale_address = "100.100.251.31";
    let mut settings = Settings {
        connection_mode: "auto".into(),
        ..Settings::default()
    };
    settings
        .trusted_peer_keys
        .insert(hostname.into(), remote.public_key_base64());
    settings.trusted_peer_addresses.insert(
        hostname.into(),
        HashMap::from([
            ("lan".into(), lan_address.into()),
            ("tailscale".into(), tailscale_address.into()),
        ]),
    );
    let _session = register_active_session(hostname, ConnectionInterface::Lan, lan_address, 6);
    let data = peer_snapshot_data(
        &identity,
        &settings,
        Ok((
            LocalInfo {
                hostname: "windows".into(),
                tailscale_ip: "192.168.251.30".into(),
                candidates: Vec::new(),
            },
            vec![PeerInfo {
                hostname: hostname.into(),
                tailscale_ip: lan_address.into(),
                online: false,
                enabled: true,
                address: lan_address.into(),
                connection_mode: "auto".into(),
                trusted: false,
                fingerprint: String::new(),
                candidates: vec![PeerCandidate::new(ConnectionInterface::Lan, lan_address)],
                current_interface: None,
                current_address: None,
                status: Default::default(),
            }],
        )),
    );

    let routes = data["peers"][0]["routes"].as_array().expect("peer routes");
    let lan = routes
        .iter()
        .find(|route| route["interface"] == "lan")
        .expect("LAN route");
    let tailscale = routes
        .iter()
        .find(|route| route["interface"] == "tailscale")
        .expect("Tailscale route");
    assert_eq!(lan["status"].as_str(), Some("connected"));
    assert_eq!(lan["connected"].as_bool(), Some(true));
    assert_eq!(tailscale["status"].as_str(), Some("discovered"));
    assert_eq!(tailscale["connected"].as_bool(), Some(false));
}
