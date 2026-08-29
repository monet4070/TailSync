use super::{
    bind_api_listener, bump_runtime_revision, clear_file_progress, clear_file_progress_scope,
    get_file_progress, get_runtime_revision, history_capabilities_data, peer_snapshot_data,
    read_request_with_limits, set_file_batch_progress, thumbnail_rgba, wait_for_runtime_revision,
    ApiToken, FileProgress, Request, RuntimeNotificationBuffer, MAX_RUNTIME_NOTIFICATIONS,
    THUMBNAIL_MAX_SIDE,
};
use crate::crypto::Settings;
use crate::identity::DeviceIdentity;
use crate::network::tailscale::{LocalInfo, PeerInfo};
use crate::network::{ConnectionInterface, PeerCandidate};
use std::time::Duration;

fn thumbnail_of(width: u32, height: u32, rgba: &[u8], max_side: usize) -> (usize, usize, Vec<u8>) {
    let image = crate::protocol::PackedImage {
        width,
        height,
        rgba,
    };
    thumbnail_rgba(image, max_side)
}

#[test]
fn thumbnail_preserves_aspect_ratio_and_caps_longest_edge() {
    let rgba = [10u8, 20, 30, 255].repeat(320 * 160);
    let (tw, th, out) = thumbnail_of(320, 160, &rgba, THUMBNAIL_MAX_SIDE);
    assert_eq!((tw, th), (160, 80));
    assert_eq!(out.len(), 160 * 80 * 4);
}

#[test]
fn thumbnail_leaves_images_within_the_cap_untouched() {
    let rgba = vec![1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255];
    let (tw, th, out) = thumbnail_of(4, 1, &rgba, THUMBNAIL_MAX_SIDE);
    assert_eq!((tw, th), (4, 1));
    assert_eq!(out, rgba);
}

#[test]
fn thumbnail_keeps_a_solid_color_pure() {
    let rgba = [10u8, 20, 30, 255].repeat(200 * 100);
    let (_, _, out) = thumbnail_of(200, 100, &rgba, THUMBNAIL_MAX_SIDE);
    assert!(out.chunks_exact(4).all(|px| *px == [10u8, 20, 30, 255]));
}

#[test]
fn thumbnail_box_averages_instead_of_point_sampling() {
    // Black beside white must average to gray; nearest-neighbor would keep
    // one original (0 or 255) and discard the other entirely.
    let rgba = [0u8, 0, 0, 255, 255, 255, 255, 255];
    let (tw, th, out) = thumbnail_of(2, 1, &rgba, 1);
    assert_eq!((tw, th), (1, 1));
    assert!(
        (125..=130).contains(&out[0]),
        "expected gray, got {}",
        out[0]
    );
    assert_eq!(out[0], out[1]);
    assert_eq!(out[1], out[2]);
    assert_eq!(out[3], 255);
}

#[test]
fn thumbnail_alpha_weights_prevent_transparent_color_bleed() {
    // A fully transparent red pixel must not tint the opaque blue neighbor;
    // a plain RGB mean would leak red (127) into the result.
    let rgba = [255u8, 0, 0, 0, 0, 0, 255, 255];
    let (_, _, out) = thumbnail_of(2, 1, &rgba, 1);
    assert_eq!(
        out[0], 0,
        "transparent red must not bleed into the red channel"
    );
    assert_eq!(out[2], 255, "opaque blue must survive");
    assert_eq!(out[3], 127, "alpha is the plain mean of 255 and 0");
}

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

#[test]
fn runtime_snapshot_request_accepts_bounded_wait_fields() {
    let request: Request = serde_json::from_value(serde_json::json!({
        "cmd": "wait_runtime_snapshot",
        "since_revision": 42,
        "wait_ms": 2500,
        "since_notification_id": 7
    }))
    .unwrap();
    assert_eq!(request.since_revision, Some(42));
    assert_eq!(request.wait_ms, Some(2500));
    assert_eq!(request.since_notification_id, Some(7));
}

#[test]
fn runtime_notification_buffer_is_bounded_and_incremental() {
    let mut notifications = RuntimeNotificationBuffer::default();
    for index in 0..(MAX_RUNTIME_NOTIFICATIONS + 3) {
        notifications.push("error", format!("failure {index}"));
    }

    let all = notifications.since(0);
    assert_eq!(all.len(), MAX_RUNTIME_NOTIFICATIONS);
    assert_eq!(all.first().map(|entry| entry.id), Some(4));
    let latest = notifications.since((MAX_RUNTIME_NOTIFICATIONS + 1) as u64);
    assert_eq!(latest.len(), 2);
    assert_eq!(latest[0].message, "failure 33");
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
fn v2_theme_request_carries_path_and_theme_id() {
    let request: Request = serde_json::from_value(serde_json::json!({
        "cmd": "install_theme",
        "path": "/tmp/my-theme.json",
        "theme_id": "studio",
    }))
    .unwrap();
    assert_eq!(request.path.as_deref(), Some("/tmp/my-theme.json"));
    assert_eq!(request.theme_id.as_deref(), Some("studio"));
    let themed: Request = serde_json::from_value(serde_json::json!({
        "cmd": "resolve_theme",
        "theme_id": "studio",
        "mode": "dark",
    }))
    .unwrap();
    assert_eq!(themed.theme_id.as_deref(), Some("studio"));
    assert_eq!(themed.mode.as_deref(), Some("dark"));
    let rollback: Request = serde_json::from_value(serde_json::json!({
        "cmd": "rollback_theme",
        "theme_id": "custom:studio@1",
    }))
    .unwrap();
    assert_eq!(rollback.cmd, "rollback_theme");
    assert_eq!(rollback.theme_id.as_deref(), Some("custom:studio@1"));
    let minimal: Request = serde_json::from_value(serde_json::json!({
        "cmd": "list_themes_v2",
    }))
    .unwrap();
    assert_eq!(minimal.path, None);
    assert_eq!(minimal.theme_id, None);
    assert_eq!(minimal.mode, None);
}

#[test]
fn history_request_accepts_new_filters_and_legacy_omissions() {
    let filtered: Request = serde_json::from_value(serde_json::json!({
        "cmd": "get_history",
        "keyword": "needle",
        "category": "text",
        "start_time": "2026-02-01T10:00:00Z",
        "end_time": "2026-02-01T11:00:00Z",
        "limit": 31,
        "offset": 62
    }))
    .unwrap();
    assert_eq!(filtered.keyword.as_deref(), Some("needle"));
    assert_eq!(filtered.category.as_deref(), Some("text"));
    assert_eq!(filtered.start_time.as_deref(), Some("2026-02-01T10:00:00Z"));
    assert_eq!(filtered.end_time.as_deref(), Some("2026-02-01T11:00:00Z"));
    assert_eq!(filtered.limit, Some(31));
    assert_eq!(filtered.offset, Some(62));

    let legacy: Request =
        serde_json::from_value(serde_json::json!({ "cmd": "get_history" })).unwrap();
    assert!(legacy.keyword.is_none());
    assert!(legacy.category.is_none());
    assert!(legacy.start_time.is_none());
    assert!(legacy.end_time.is_none());
}

#[test]
fn peer_snapshot_keeps_local_identity_when_discovery_fails() {
    let identity = DeviceIdentity::generate_for_test();
    let mut settings = Settings::default();
    settings
        .paired_peer_endpoints
        .insert("windows".into(), "192.168.1.20".into());
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
        data["paired_peer_endpoints"]["windows"].as_str(),
        Some("192.168.1.20")
    );
    assert_eq!(
        data["discovery_error"].as_str(),
        Some("Tailscale status unavailable")
    );
}

#[test]
fn peer_snapshot_does_not_infer_a_connection_from_selected_mode() {
    let identity = DeviceIdentity::generate_for_test();
    let mut settings = Settings {
        connection_mode: "tailscale_only".into(),
        ..Settings::default()
    };
    settings
        .paired_peer_endpoints
        .insert("mode-only-peer".into(), "100.64.0.2".into());
    let peer = PeerInfo {
        hostname: "mode-only-peer".into(),
        tailscale_ip: "100.64.0.2".into(),
        online: false,
        enabled: true,
        address: "100.64.0.2".into(),
        connection_mode: "tailscale".into(),
        trusted: false,
        fingerprint: String::new(),
        candidates: vec![PeerCandidate::new(
            ConnectionInterface::Tailscale,
            "100.64.0.2",
        )],
        current_interface: None,
        current_address: None,
        status: Default::default(),
    };

    let data = peer_snapshot_data(
        &identity,
        &settings,
        Ok((
            LocalInfo {
                hostname: "macbook".into(),
                tailscale_ip: "100.64.0.1".into(),
                candidates: Vec::new(),
            },
            vec![peer],
        )),
    );

    assert!(data["peers"][0]["current_interface"].is_null());
    assert!(data["peers"][0]["current_address"].is_null());
    let routes = data["peers"][0]["routes"].as_array().expect("peer routes");
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0]["interface"].as_str(), Some("tailscale"));
    assert_eq!(routes[0]["connected"].as_bool(), Some(false));
    assert_eq!(routes[0]["pairing_endpoint"].as_bool(), Some(true));
    assert_eq!(routes[0]["rtt_capable"].as_bool(), Some(true));
    assert_eq!(data["self"]["routes"].as_array().map(Vec::len), Some(1));
}
