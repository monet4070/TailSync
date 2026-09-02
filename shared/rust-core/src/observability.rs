//! Correlation helpers shared by the transport, receive, and platform layers.
//!
//! The protocol already carries stable event, transfer, and batch identifiers.
//! These helpers add a local connection identifier and derive a short session
//! identifier from the Noise transcript without logging secret material or
//! adding another wire field.

use rand::random;

/// Generate a process-local identifier for one connection attempt.
pub fn connection_id() -> String {
    format!("{:016x}", random::<u64>())
}

/// Derive the same short identifier on both sides of a completed Noise
/// handshake. The raw handshake hash is intentionally never written to logs.
pub fn session_id(handshake_hash: &[u8]) -> String {
    let digest = blake3::hash(handshake_hash);
    hex::encode(&digest.as_bytes()[..8])
}

/// Keep peer identifiers useful for correlation while avoiding a full public
/// key or hostname in every release log line.
pub fn peer_id(public_key: &[u8]) -> String {
    let digest = blake3::hash(public_key);
    hex::encode(&digest.as_bytes()[..8])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_id_is_stable_and_short() {
        assert_eq!(session_id(b"handshake"), session_id(b"handshake"));
        assert_eq!(session_id(b"handshake").len(), 16);
    }

    #[test]
    fn generated_ids_have_expected_shape() {
        assert_eq!(connection_id().len(), 16);
        assert_eq!(peer_id(b"public-key").len(), 16);
    }
}
