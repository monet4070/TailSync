//! Inbound peer admission rules.
//!
//! Shared by the macOS and Windows network layers (T105 migration). An
//! inbound connection is admitted only when the peer is enabled, its source
//! matches the connection mode, and its presented public key matches the
//! stored trusted key.

use crate::crypto::Settings;
use crate::peer::inbound_source::InboundSource;
use crate::secure;

/// Whether an inbound connection from `source` presenting `public_key` is
/// admitted under `settings`.
///
/// Mirrors the platform server admission check: an unpaired peer, a
/// malformed stored key, a disabled peer, or a source that does not match
/// the connection mode is rejected.
pub fn peer_is_allowed(
    settings: &Settings,
    hostname: &str,
    public_key: &[u8],
    source: &InboundSource,
) -> bool {
    let trusted_key = settings
        .trusted_peer_keys
        .get(hostname)
        .and_then(|key| secure::decode_trusted_key(key).ok());
    let peer_enabled = settings
        .enabled_peers
        .get(hostname)
        .copied()
        .unwrap_or(true);
    source.is_allowed(&settings.connection_mode)
        && peer_enabled
        && trusted_key.as_deref() == Some(public_key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secure::decode_trusted_key;
    use base64::Engine;
    use std::net::SocketAddr;

    fn encoded_key(byte: u8) -> String {
        base64::engine::general_purpose::STANDARD.encode([byte; 32])
    }

    fn tcp_source(address: &str) -> InboundSource {
        InboundSource::Tcp(address.parse::<SocketAddr>().unwrap())
    }

    fn paired_settings(byte: u8) -> Settings {
        let mut settings = Settings::default();
        settings
            .trusted_peer_keys
            .insert("mac".to_string(), encoded_key(byte));
        settings.connection_mode = "auto".to_string();
        settings
    }

    #[test]
    fn paired_enabled_peer_is_allowed() {
        let settings = paired_settings(7);
        let key = decode_trusted_key(&encoded_key(7)).unwrap();
        assert!(peer_is_allowed(
            &settings,
            "mac",
            &key,
            &tcp_source("192.168.1.5:19890")
        ));
    }

    #[test]
    fn unpaired_peer_is_rejected() {
        let settings = Settings::default();
        let key = decode_trusted_key(&encoded_key(7)).unwrap();
        assert!(!peer_is_allowed(
            &settings,
            "mac",
            &key,
            &tcp_source("192.168.1.5:19890")
        ));
    }

    #[test]
    fn wrong_key_is_rejected() {
        let settings = paired_settings(7);
        let wrong = decode_trusted_key(&encoded_key(8)).unwrap();
        assert!(!peer_is_allowed(
            &settings,
            "mac",
            &wrong,
            &tcp_source("192.168.1.5:19890")
        ));
    }

    #[test]
    fn malformed_trusted_key_is_rejected() {
        let mut settings = paired_settings(7);
        settings
            .trusted_peer_keys
            .insert("mac".to_string(), "not-base64!".to_string());
        let key = decode_trusted_key(&encoded_key(7)).unwrap();
        assert!(!peer_is_allowed(
            &settings,
            "mac",
            &key,
            &tcp_source("192.168.1.5:19890")
        ));
    }

    #[test]
    fn disabled_peer_is_rejected() {
        let mut settings = paired_settings(7);
        settings.enabled_peers.insert("mac".to_string(), false);
        let key = decode_trusted_key(&encoded_key(7)).unwrap();
        assert!(!peer_is_allowed(
            &settings,
            "mac",
            &key,
            &tcp_source("192.168.1.5:19890")
        ));
    }

    #[test]
    fn missing_enabled_entry_defaults_to_enabled() {
        let settings = paired_settings(7);
        let key = decode_trusted_key(&encoded_key(7)).unwrap();
        assert!(peer_is_allowed(
            &settings,
            "mac",
            &key,
            &tcp_source("192.168.1.5:19890")
        ));
    }

    #[test]
    fn source_mode_mismatch_is_rejected() {
        let mut settings = paired_settings(7);
        settings.connection_mode = "lan_only".to_string();
        let key = decode_trusted_key(&encoded_key(7)).unwrap();
        // Tailscale IP under lan_only.
        let tailscale = tcp_source("100.101.102.103:19890");
        assert!(!peer_is_allowed(&settings, "mac", &key, &tailscale));
        // Iroh under lan_only.
        assert!(!peer_is_allowed(
            &settings,
            "mac",
            &key,
            &InboundSource::Iroh("endpoint-1".to_string())
        ));
    }
}
