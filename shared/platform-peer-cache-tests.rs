// Discovery-round guard, `include!`d by each platform's `network::peer_cache`
// module so one gate covers both adapters.
//
// S3-P1-3: discovery (UDP broadcast, mDNS, the Tailscale peer list) is
// unauthenticated. The audited vector is a spoofed discovery round that claims
// an already-trusted hostname from an attacker-chosen address: because the
// hostname really is trusted, the pinned-key check in `crypto` cannot reject
// it, and persisting that route would redirect every later reconnect to the
// attacker. The defence is this adapter's `remember` no-op — discovery routes
// stay in the round's in-memory candidates and are never written to `Settings`.
// Persisted addresses are written only after a handshake proves possession of
// the pinned key (see the platform network servers).
//
// A regression that makes `remember` persist a discovery route fails here.

use super::*;

#[test]
fn a_discovery_round_never_persists_a_route_for_a_trusted_hostname() {
    let pool = Arc::new(Mutex::new(ConnectionPool::new(
        Arc::new(DeviceIdentity::generate_for_test()),
        Arc::new(Mutex::new(crypto::Settings::default())),
    )));
    let adapter = Adapter {
        pool,
        app_handle: None,
    };

    let mut settings = crypto::Settings::default();
    // The device the user already paired and pinned.
    settings
        .trusted_peer_keys
        .insert("trusted-mac".to_string(), "pinned-noise-public-key".to_string());
    settings.trusted_peer_addresses.insert(
        "trusted-mac".to_string(),
        [("lan".to_string(), "100.64.0.5".to_string())]
            .into_iter()
            .collect(),
    );
    settings
        .enabled_peers
        .insert("trusted-mac".to_string(), true);
    let before = settings.clone();

    // A spoofed discovery round: the trusted hostname, an attacker address.
    let spoofed = tailscale::PeerInfo {
        hostname: "trusted-mac".to_string(),
        tailscale_ip: "100.64.0.5".to_string(),
        online: true,
        enabled: true,
        address: "203.0.113.66".to_string(),
        connection_mode: "lan".to_string(),
        trusted: true,
        fingerprint: String::new(),
        candidates: vec![PeerCandidate::new(ConnectionInterface::Lan, "203.0.113.66")],
        current_interface: None,
        current_address: None,
        status: PeerStatus::Online,
    };

    adapter.remember(&mut settings, "lan", std::slice::from_ref(&spoofed));

    assert_eq!(
        settings, before,
        "an unauthenticated discovery round must not persist any route"
    );
    assert_eq!(
        settings
            .trusted_peer_addresses
            .get("trusted-mac")
            .and_then(|routes| routes.get("lan"))
            .map(String::as_str),
        Some("100.64.0.5"),
        "the spoofed address must not overwrite the trusted device's saved route"
    );
}
