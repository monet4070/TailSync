use ring::hkdf::{Salt, HKDF_SHA256};
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::{mpsc, watch, Mutex};
use tokio::time::Instant;

use crate::crypto::Settings;
use crate::identity::DeviceIdentity;
use crate::protocol::{Command, Frame};
use crate::secure::SecureConnection;

const PAIRING_CODE_CONTEXT: &[u8] = b"tailsync pairing verification code v1";
const X25519_PUBLIC_KEY_LENGTH: usize = 32;
const DEFAULT_PAIRING_WINDOW: Duration = Duration::from_secs(120);
const DEFAULT_MAX_FAILURES: u8 = 5;
const PAIRING_SESSION_TIMEOUT: Duration = Duration::from_secs(30);
const PAIRING_FINALIZE_TIMEOUT: Duration = Duration::from_secs(5);

/// Pairing state-machine errors (T351 migration). The `Display` strings are
/// part of the observable wire contract — Swift localizes by substring
/// (ApiClient.swift `pairingErrorDescription`) — and must stay stable.
#[derive(Debug, Error)]
pub enum PairingError {
    #[error("Pairing window is closed")]
    WindowClosed,
    #[error("Another pairing is already in progress")]
    AlreadyInProgress,
    #[error("Cannot pair this device with itself")]
    SelfPairing,
    #[error("Invalid pairing interface")]
    InvalidInterface,
    #[error("No pairing verification is awaiting confirmation")]
    NoVerification,
    #[error("Pairing session is no longer active")]
    SessionInactive,
    #[error("Pairing session was closed before confirmation")]
    SessionClosed,
    #[error("{0}")]
    Verification(&'static str),
    #[error("{0}")]
    Transport(String),
}

struct VerificationCodeLength;

impl ring::hkdf::KeyType for VerificationCodeLength {
    fn len(&self) -> usize {
        8
    }
}

pub fn derive_verification_code(
    handshake_hash: &[u8],
    local_public_key: &[u8],
    remote_public_key: &[u8],
) -> Result<String, &'static str> {
    if handshake_hash.is_empty() {
        return Err("Noise handshake hash is empty");
    }
    if local_public_key.len() != X25519_PUBLIC_KEY_LENGTH
        || remote_public_key.len() != X25519_PUBLIC_KEY_LENGTH
    {
        return Err("Pairing identity keys must be 32-byte X25519 public keys");
    }

    let (first_key, second_key) = if local_public_key <= remote_public_key {
        (local_public_key, remote_public_key)
    } else {
        (remote_public_key, local_public_key)
    };
    let salt = Salt::new(HKDF_SHA256, PAIRING_CODE_CONTEXT);
    let pseudo_random_key = salt.extract(handshake_hash);
    let info = [PAIRING_CODE_CONTEXT, first_key, second_key];
    let output_key_material = pseudo_random_key
        .expand(&info, VerificationCodeLength)
        .map_err(|_| "Failed to derive pairing verification code")?;
    let mut output = [0u8; 8];
    output_key_material
        .fill(&mut output)
        .map_err(|_| "Failed to fill pairing verification code")?;

    Ok(format!("{:06}", u64::from_be_bytes(output) % 1_000_000))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PairingPhase {
    Disabled,
    Waiting,
    Handshaking,
    Verification,
    WaitingForPeer,
    Finalizing,
    Paired,
    Cancelled,
    TimedOut,
    Locked,
}

#[derive(Debug, Clone, Serialize)]
pub struct PairingPeerStatus {
    pub hostname: String,
    pub address: String,
    pub fingerprint: String,
    pub verification_code: String,
    pub local_confirmed: bool,
    pub remote_confirmed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PairingStatus {
    pub pairing_enabled: bool,
    pub phase: PairingPhase,
    pub expires_at: Option<u64>,
    pub remaining_seconds: u64,
    pub failed_attempts: u8,
    pub max_failures: u8,
    pub peer: Option<PairingPeerStatus>,
    pub error: Option<String>,
}

#[doc(hidden)]
pub struct PendingPairing {
    pub connection: SecureConnection,
    pub hostname: String,
    pub remote_public_key: Vec<u8>,
    pub handshake_hash: Vec<u8>,
    pub address: String,
    pub interface: String,
}

enum PairingAction {
    Confirm,
    Cancel,
}

struct PairingState {
    enabled: bool,
    phase: PairingPhase,
    deadline: Option<Instant>,
    expires_at: Option<u64>,
    failed_attempts: u8,
    peer: Option<PairingPeerStatus>,
    error: Option<String>,
    generation: u64,
    session_id: u64,
    control: Option<mpsc::Sender<PairingAction>>,
}

pub struct PairingManager {
    state: Mutex<PairingState>,
    settings: Arc<Mutex<Settings>>,
    identity: Arc<DeviceIdentity>,
    window_signal: watch::Sender<bool>,
    window_duration: Duration,
    max_failures: u8,
    persist_trust: bool,
}

mod manager;

/// Installs an inbound pairing session for an accepted connection (T110
/// migration). When `pairing` is absent (Iroh transport cannot pair), an
/// error frame is written to the stream and the call succeeds so the
/// connection winds down normally.
pub async fn install_pairing_session(
    pairing: Option<&Arc<PairingManager>>,
    mut stream: SecureConnection,
    hostname: String,
    remote_public_key: Vec<u8>,
    handshake_hash: Vec<u8>,
    address: String,
    interface: String,
) -> Result<(), PairingError> {
    let Some(pairing) = pairing else {
        crate::secure::write_error(&mut stream, "Pairing over Iroh is not supported")
            .await
            .map_err(|error| PairingError::Transport(error.to_string()))?;
        return Ok(());
    };
    crate::secure::write_ready(&mut stream)
        .await
        .map_err(|error| PairingError::Transport(error.to_string()))?;
    pairing
        .install_session(PendingPairing {
            connection: stream,
            hostname,
            remote_public_key,
            handshake_hash,
            address,
            interface,
        })
        .await
}

#[cfg(test)]
mod tests;
