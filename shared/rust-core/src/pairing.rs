use ring::hkdf::{Salt, HKDF_SHA256};
use serde::Serialize;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
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

impl PairingManager {
    pub fn new(settings: Arc<Mutex<Settings>>, identity: Arc<DeviceIdentity>) -> Arc<Self> {
        Self::with_policy(
            settings,
            identity,
            DEFAULT_PAIRING_WINDOW,
            DEFAULT_MAX_FAILURES,
            true,
        )
    }

    pub(crate) fn with_policy(
        settings: Arc<Mutex<Settings>>,
        identity: Arc<DeviceIdentity>,
        window_duration: Duration,
        max_failures: u8,
        persist_trust: bool,
    ) -> Arc<Self> {
        let (window_signal, _) = watch::channel(false);
        Arc::new(Self {
            state: Mutex::new(PairingState {
                enabled: false,
                phase: PairingPhase::Disabled,
                deadline: None,
                expires_at: None,
                failed_attempts: 0,
                peer: None,
                error: None,
                generation: 0,
                session_id: 0,
                control: None,
            }),
            settings,
            identity,
            window_signal,
            window_duration,
            max_failures,
            persist_trust,
        })
    }

    pub async fn enable(self: &Arc<Self>) -> PairingStatus {
        let old_control = {
            let mut state = self.state.lock().await;
            let old_control = state.control.take();
            state.enabled = true;
            state.phase = PairingPhase::Waiting;
            state.deadline = Some(Instant::now() + self.window_duration);
            state.expires_at = Some(unix_timestamp_after(self.window_duration));
            state.failed_attempts = 0;
            state.peer = None;
            state.error = None;
            state.generation = state.generation.wrapping_add(1);
            state.session_id = state.session_id.wrapping_add(1);
            old_control
        };
        if let Some(control) = old_control {
            let _ = control.send(PairingAction::Cancel).await;
        }
        self.window_signal.send_replace(true);
        if crate::diagnostics::is_collected() {
            crate::diagnostics::record(crate::diagnostics::Record {
                event: crate::diagnostics::Event::PairingWindowOpened,
                peer: None,
                session: None,
                error: None,
            });
        }

        let generation = self.state.lock().await.generation;
        self.schedule_expiration(generation);
        self.status().await
    }

    fn schedule_expiration(self: &Arc<Self>, generation: u64) {
        let manager = Arc::downgrade(self);
        tokio::spawn(async move {
            tokio::time::sleep(
                manager
                    .upgrade()
                    .map_or(DEFAULT_PAIRING_WINDOW, |manager| manager.window_duration),
            )
            .await;
            if let Some(manager) = manager.upgrade() {
                manager.expire(generation).await;
            }
        });
    }

    pub async fn status(&self) -> PairingStatus {
        let state = self.state.lock().await;
        PairingStatus {
            pairing_enabled: state.enabled,
            phase: state.phase,
            expires_at: state.expires_at,
            remaining_seconds: state
                .deadline
                .map(|deadline| deadline.saturating_duration_since(Instant::now()).as_secs())
                .unwrap_or(0),
            failed_attempts: state.failed_attempts,
            max_failures: self.max_failures,
            peer: state.peer.clone(),
            error: state.error.clone(),
        }
    }

    pub async fn is_enabled(&self) -> bool {
        self.state.lock().await.enabled
    }

    pub fn subscribe_window(&self) -> watch::Receiver<bool> {
        self.window_signal.subscribe()
    }

    pub async fn begin_handshake(&self) -> Result<(), PairingError> {
        let mut state = self.state.lock().await;
        if !state.enabled {
            return Err(PairingError::WindowClosed);
        }
        if state.control.is_some() {
            return Err(PairingError::AlreadyInProgress);
        }
        state.phase = PairingPhase::Handshaking;
        state.error = None;
        if crate::diagnostics::is_collected() {
            crate::diagnostics::record(crate::diagnostics::Record {
                event: crate::diagnostics::Event::PairingHandshakeStarted,
                peer: None,
                session: None,
                error: None,
            });
        }
        Ok(())
    }

    #[doc(hidden)]
    pub async fn install_session(
        self: &Arc<Self>,
        pending: PendingPairing,
    ) -> Result<(), PairingError> {
        if pending.remote_public_key == self.identity.public_key() {
            return Err(PairingError::SelfPairing);
        }
        if !matches!(pending.interface.as_str(), "lan" | "iroh" | "tailscale") {
            return Err(PairingError::InvalidInterface);
        }
        let verification_code = derive_verification_code(
            &pending.handshake_hash,
            self.identity.public_key(),
            &pending.remote_public_key,
        )
        .map_err(PairingError::Verification)?;
        let fingerprint = crate::identity::fingerprint(&pending.remote_public_key);
        let (control, receiver) = mpsc::channel(4);
        let session_id = {
            let mut state = self.state.lock().await;
            if !state.enabled {
                return Err(PairingError::WindowClosed);
            }
            if state.control.is_some() {
                return Err(PairingError::AlreadyInProgress);
            }
            state.session_id = state.session_id.wrapping_add(1);
            state.phase = PairingPhase::Verification;
            state.peer = Some(PairingPeerStatus {
                hostname: pending.hostname.clone(),
                address: pending.address.clone(),
                fingerprint,
                verification_code,
                local_confirmed: false,
                remote_confirmed: false,
            });
            state.error = None;
            state.control = Some(control);
            state.session_id
        };

        let manager = self.clone();
        tokio::spawn(async move {
            manager.run_session(session_id, pending, receiver).await;
        });
        Ok(())
    }

    pub async fn confirm(&self) -> Result<PairingStatus, PairingError> {
        let control = {
            let state = self.state.lock().await;
            if !matches!(
                state.phase,
                PairingPhase::Verification | PairingPhase::WaitingForPeer
            ) {
                return Err(PairingError::NoVerification);
            }
            state.control.clone().ok_or(PairingError::SessionInactive)?
        };
        control
            .send(PairingAction::Confirm)
            .await
            .map_err(|_| PairingError::SessionInactive)?;
        Ok(self.status().await)
    }

    pub async fn cancel(&self) -> PairingStatus {
        let control = {
            let mut state = self.state.lock().await;
            let control = state.control.take();
            state.enabled = false;
            state.phase = PairingPhase::Cancelled;
            state.deadline = None;
            state.expires_at = None;
            state.error = Some("Pairing was cancelled".to_string());
            state.generation = state.generation.wrapping_add(1);
            state.session_id = state.session_id.wrapping_add(1);
            control
        };
        self.window_signal.send_replace(false);
        if crate::diagnostics::is_collected() {
            crate::diagnostics::record(crate::diagnostics::Record {
                event: crate::diagnostics::Event::PairingWindowClosed,
                peer: None,
                session: None,
                error: None,
            });
        }
        if let Some(control) = control {
            let _ = control.send(PairingAction::Cancel).await;
        }
        self.status().await
    }

    pub async fn record_failure(&self, error: impl Into<String>) {
        let mut close_window = false;
        {
            let mut state = self.state.lock().await;
            if !state.enabled {
                return;
            }
            state.failed_attempts = state.failed_attempts.saturating_add(1);
            state.peer = None;
            state.control = None;
            let message = error.into();
            state.error = Some(message.clone());
            if crate::diagnostics::is_collected() {
                crate::diagnostics::record(crate::diagnostics::Record {
                    event: crate::diagnostics::Event::PairingFailed,
                    peer: None,
                    session: None,
                    error: crate::diagnostics::error_ref("PairingError::record_failure", &message),
                });
            }
            if state.failed_attempts >= self.max_failures {
                state.enabled = false;
                state.phase = PairingPhase::Locked;
                state.deadline = None;
                state.expires_at = None;
                state.generation = state.generation.wrapping_add(1);
                close_window = true;
            } else {
                state.phase = PairingPhase::Waiting;
            }
        }
        if close_window {
            self.window_signal.send_replace(false);
        }
    }

    async fn expire(self: &Arc<Self>, generation: u64) {
        let (control, close_window, next_generation) = {
            let mut state = self.state.lock().await;
            if !state.enabled || state.generation != generation {
                return;
            }
            let control = state.control.take();
            if control.is_some() {
                state.failed_attempts = state.failed_attempts.saturating_add(1);
                state.peer = None;
                state.error = Some("Pairing session timed out".to_string());
                if state.failed_attempts >= self.max_failures {
                    state.enabled = false;
                    state.phase = PairingPhase::Locked;
                    state.deadline = None;
                    state.expires_at = None;
                    state.generation = state.generation.wrapping_add(1);
                    state.session_id = state.session_id.wrapping_add(1);
                    (control, true, None)
                } else {
                    state.phase = PairingPhase::Waiting;
                    state.deadline = Some(Instant::now() + self.window_duration);
                    state.expires_at = Some(unix_timestamp_after(self.window_duration));
                    state.generation = state.generation.wrapping_add(1);
                    state.session_id = state.session_id.wrapping_add(1);
                    (control, false, Some(state.generation))
                }
            } else {
                state.enabled = false;
                state.phase = PairingPhase::TimedOut;
                state.deadline = None;
                state.expires_at = None;
                state.error = Some("Pairing window timed out".to_string());
                state.generation = state.generation.wrapping_add(1);
                state.session_id = state.session_id.wrapping_add(1);
                (control, true, None)
            }
        };
        if close_window {
            self.window_signal.send_replace(false);
        }
        if let Some(control) = control {
            let _ = control.send(PairingAction::Cancel).await;
        }
        if let Some(generation) = next_generation {
            self.schedule_expiration(generation);
        }
    }

    async fn run_session(
        self: Arc<Self>,
        session_id: u64,
        pending: PendingPairing,
        mut receiver: mpsc::Receiver<PairingAction>,
    ) {
        let PendingPairing {
            mut connection,
            hostname,
            remote_public_key,
            address,
            interface,
            ..
        } = pending;
        let deadline = {
            let state = self.state.lock().await;
            state.deadline.unwrap_or_else(Instant::now)
        };
        let timeout = tokio::time::sleep_until(deadline);
        tokio::pin!(timeout);
        let mut local_confirmed = false;
        let mut remote_confirmed = false;
        let mut local_persisted = false;
        let mut remote_persisted = false;

        loop {
            tokio::select! {
                action = receiver.recv() => match action {
                    Some(PairingAction::Confirm) if !local_confirmed => {
                        let Ok(frame) = Frame::try_new(Command::PairingConfirm, 0, 0, Vec::new()) else {
                            self.fail_session(session_id, "Could not construct pairing confirmation".to_string()).await;
                            return;
                        };
                        if let Err(error) = connection.write_frame(&frame).await {
                            self.fail_session(session_id, format!("Could not confirm pairing: {error}")).await;
                            return;
                        }
                        local_confirmed = true;
                        self.set_confirmation(session_id, true, remote_confirmed).await;
                    }
                    Some(PairingAction::Confirm) => {}
                    Some(PairingAction::Cancel) => {
                        if let Ok(frame) = Frame::try_new(Command::PairingCancel, 0, 0, Vec::new()) {
                            let _ = connection.write_frame(&frame).await;
                        }
                        return;
                    }
                    None => return,
                },
                result = connection.read_frame() => match result {
                    Ok(frame) if frame.command == Command::PairingConfirm => {
                        remote_confirmed = true;
                        self.set_confirmation(session_id, local_confirmed, true).await;
                    }
                    Ok(frame) if frame.command == Command::PairingPersisted => {
                        if !remote_confirmed {
                            self.fail_session(
                                session_id,
                                "Received pairing completion before confirmation".to_string(),
                            )
                            .await;
                            return;
                        }
                        remote_persisted = true;
                    }
                    Ok(frame) if frame.command == Command::PairingCancel => {
                        self.fail_session(session_id, "The other device cancelled pairing".to_string()).await;
                        return;
                    }
                    Ok(frame) if frame.command == Command::PeerError => {
                        self.fail_session(
                            session_id,
                            String::from_utf8_lossy(&frame.payload).to_string(),
                        ).await;
                        return;
                    }
                    Ok(_) => {
                        self.fail_session(session_id, "Unexpected message during pairing".to_string()).await;
                        return;
                    }
                    Err(error) => {
                        self.fail_session(session_id, format!("Pairing connection failed: {error}")).await;
                        return;
                    }
                },
                _ = &mut timeout => {
                    self.expire_current_session(session_id).await;
                    return;
                }
            }

            if local_confirmed && remote_confirmed && !local_persisted {
                if let Err(error) = self
                    .persist_pairing(
                        session_id,
                        &hostname,
                        &remote_public_key,
                        &interface,
                        &address,
                    )
                    .await
                {
                    self.fail_session(session_id, error.to_string()).await;
                    return;
                }
                let Ok(frame) = Frame::try_new(Command::PairingPersisted, 0, 0, Vec::new()) else {
                    self.fail_session(
                        session_id,
                        "Could not construct pairing completion".to_string(),
                    )
                    .await;
                    return;
                };
                if let Err(error) = connection.write_frame(&frame).await {
                    self.fail_session(
                        session_id,
                        format!("Could not confirm saved pairing: {error}"),
                    )
                    .await;
                    return;
                }
                local_persisted = true;
            }

            if local_persisted && remote_persisted {
                if let Err(error) = self.finish_success(session_id, &hostname).await {
                    self.fail_session(session_id, error.to_string()).await;
                }
                // Both peers have observed the other's persisted state. Finish
                // the underlying send stream explicitly; Iroh's flush is not a
                // delivery acknowledgement and Drop may close the QUIC
                // connection before the final frame is consumed.
                let _ = connection.shutdown().await;
                return;
            }
        }
    }

    async fn set_confirmation(&self, session_id: u64, local: bool, remote: bool) {
        let mut state = self.state.lock().await;
        if state.session_id != session_id {
            return;
        }
        if let Some(peer) = &mut state.peer {
            peer.local_confirmed = local;
            peer.remote_confirmed = remote;
        }
        state.phase = if local && !remote {
            PairingPhase::WaitingForPeer
        } else {
            PairingPhase::Verification
        };
    }

    async fn persist_pairing(
        &self,
        session_id: u64,
        hostname: &str,
        remote_public_key: &[u8],
        interface: &str,
        address: &str,
    ) -> Result<(), PairingError> {
        {
            let state = self.state.lock().await;
            if !state.enabled || state.session_id != session_id {
                return Err(PairingError::SessionClosed);
            }
        }
        let public_key = {
            use base64::{engine::general_purpose::STANDARD, Engine as _};
            STANDARD.encode(remote_public_key)
        };
        let mut settings = self.settings.lock().await;
        let result = if self.persist_trust {
            settings.trust_peer(hostname, &public_key, interface, Some(address))
        } else {
            settings.trust_peer_without_save(hostname, &public_key, interface, Some(address))
        };
        result.map_err(|error| {
            PairingError::Transport(format!("Could not save paired device: {error}"))
        })?;
        Ok(())
    }

    async fn finish_success(&self, session_id: u64, hostname: &str) -> Result<(), PairingError> {
        let mut state = self.state.lock().await;
        if state.session_id != session_id {
            return Ok(());
        }
        state.enabled = false;
        state.phase = PairingPhase::Paired;
        state.deadline = None;
        state.expires_at = None;
        state.error = None;
        state.control = None;
        state.generation = state.generation.wrapping_add(1);
        self.window_signal.send_replace(false);
        if crate::diagnostics::is_collected() {
            crate::diagnostics::record(crate::diagnostics::Record {
                event: crate::diagnostics::Event::PairingConfirmed,
                peer: Some(hostname.to_string()),
                session: None,
                error: None,
            });
        }
        Ok(())
    }

    async fn fail_session(&self, session_id: u64, error: String) {
        {
            let state = self.state.lock().await;
            if state.session_id != session_id {
                return;
            }
        }
        self.record_failure(error).await;
    }

    async fn expire_current_session(self: &Arc<Self>, session_id: u64) {
        let generation = {
            let state = self.state.lock().await;
            if state.session_id != session_id {
                return;
            }
            state.generation
        };
        self.expire(generation).await;
    }
}

fn unix_timestamp_after(duration: Duration) -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .saturating_add(duration)
        .as_secs()
}

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
mod tests {
    use super::*;
    use crate::secure::{self, HandshakePurpose, PeerIdentity};
    use tokio::net::{TcpListener, TcpStream};

    /// True when `needle` appears inside `haystack` as an ordered
    /// subsequence: every needle event must be present in the given relative
    /// order, while unrelated events may appear anywhere between them.
    ///
    /// Test-private helper for diagnostics assertions: the collector is
    /// process-global, so parallel PairingManager tests can interleave
    /// foreign events into the collected stream.
    fn contains_ordered_subsequence(
        haystack: &[crate::diagnostics::Event],
        needle: &[crate::diagnostics::Event],
    ) -> bool {
        let mut rest = needle;
        for event in haystack {
            if rest.first() == Some(event) {
                rest = &rest[1..];
                if rest.is_empty() {
                    return true;
                }
            }
        }
        rest.is_empty()
    }

    #[test]
    fn code_is_six_digits_and_independent_of_key_order() {
        let hash = [0x42; 32];
        let first = [0x11; 32];
        let second = [0x22; 32];
        let forward = derive_verification_code(&hash, &first, &second).unwrap();
        let reverse = derive_verification_code(&hash, &second, &first).unwrap();

        assert_eq!(forward, reverse);
        assert_eq!(forward.len(), 6);
        assert!(forward.bytes().all(|byte| byte.is_ascii_digit()));
    }

    #[test]
    fn invalid_inputs_are_rejected() {
        assert!(derive_verification_code(&[], &[0; 32], &[1; 32]).is_err());
        assert!(derive_verification_code(&[0; 32], &[0; 31], &[1; 32]).is_err());
    }

    #[tokio::test]
    async fn pairing_window_defaults_closed_and_expires() {
        let manager = PairingManager::with_policy(
            Arc::new(Mutex::new(Settings::default())),
            Arc::new(DeviceIdentity::generate_for_test()),
            Duration::from_millis(25),
            5,
            false,
        );
        assert!(!manager.status().await.pairing_enabled);

        let enabled = manager.enable().await;
        assert!(enabled.pairing_enabled);
        assert_eq!(enabled.phase, PairingPhase::Waiting);
        tokio::time::sleep(Duration::from_millis(50)).await;

        let expired = manager.status().await;
        assert!(!expired.pairing_enabled);
        assert_eq!(expired.failed_attempts, 0);
        assert_eq!(expired.phase, PairingPhase::TimedOut);
    }

    #[tokio::test]
    async fn fifth_failure_closes_pairing_window() {
        let manager = PairingManager::with_policy(
            Arc::new(Mutex::new(Settings::default())),
            Arc::new(DeviceIdentity::generate_for_test()),
            Duration::from_secs(1),
            5,
            false,
        );
        manager.enable().await;
        for attempt in 1..=5 {
            manager.record_failure(format!("failure {attempt}")).await;
        }

        let status = manager.status().await;
        assert!(!status.pairing_enabled);
        assert_eq!(status.failed_attempts, 5);
        assert_eq!(status.phase, PairingPhase::Locked);
    }

    #[tokio::test]
    async fn session_timeouts_count_toward_pairing_lockout() {
        let manager = PairingManager::with_policy(
            Arc::new(Mutex::new(Settings::default())),
            Arc::new(DeviceIdentity::generate_for_test()),
            Duration::from_secs(1),
            3,
            false,
        );
        manager.enable().await;

        for attempt in 1..=3 {
            let generation = {
                let mut state = manager.state.lock().await;
                let (control, _receiver) = mpsc::channel(1);
                state.control = Some(control);
                state.phase = PairingPhase::Verification;
                state.generation
            };
            manager.expire(generation).await;
            let status = manager.status().await;
            assert_eq!(status.failed_attempts, attempt);
            if attempt < 3 {
                assert!(status.pairing_enabled);
                assert_eq!(status.phase, PairingPhase::Waiting);
            }
        }

        let status = manager.status().await;
        assert!(!status.pairing_enabled);
        assert_eq!(status.phase, PairingPhase::Locked);
    }

    #[tokio::test]
    async fn both_confirmations_save_both_peer_keys_and_close_windows() {
        const IROH_ENDPOINT_ID: &str =
            "5866666666666666666666666666666666666666666666666666666666666666";
        let server_settings = Arc::new(Mutex::new(Settings::default()));
        let client_settings = Arc::new(Mutex::new(Settings::default()));
        let server_identity = Arc::new(DeviceIdentity::generate_for_test());
        let client_identity = Arc::new(DeviceIdentity::generate_for_test());
        let server_manager = PairingManager::with_policy(
            server_settings.clone(),
            server_identity.clone(),
            Duration::from_secs(2),
            5,
            false,
        );
        let client_manager = PairingManager::with_policy(
            client_settings.clone(),
            client_identity.clone(),
            Duration::from_secs(2),
            5,
            false,
        );
        server_manager.enable().await;
        client_manager.enable().await;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server_identity_for_task = server_identity.clone();
        let server_manager_for_task = server_manager.clone();
        let server = tokio::spawn(async move {
            let (stream, _peer_address) = listener.accept().await.unwrap();
            let accepted = secure::accept_with_pairing_window(
                stream,
                &server_identity_for_task,
                PeerIdentity {
                    hostname: "server".into(),
                    tailscale_ip: String::new(),
                    iroh_endpoint_id: None,
                },
                server_manager_for_task.subscribe_window(),
            )
            .await
            .unwrap();
            assert_eq!(accepted.purpose, HandshakePurpose::Pairing);
            let mut connection = accepted.connection;
            secure::write_ready(&mut connection).await.unwrap();
            server_manager_for_task
                .install_session(PendingPairing {
                    connection,
                    hostname: accepted.peer_identity.hostname,
                    remote_public_key: accepted.remote_public_key,
                    handshake_hash: accepted.handshake_hash,
                    address: IROH_ENDPOINT_ID.into(),
                    interface: "iroh".into(),
                })
                .await
                .unwrap();
        });

        let accepted = secure::connect_pairing(
            TcpStream::connect(address).await.unwrap(),
            &client_identity,
            PeerIdentity {
                hostname: "client".into(),
                tailscale_ip: String::new(),
                iroh_endpoint_id: None,
            },
        )
        .await
        .unwrap();
        client_manager
            .install_session(PendingPairing {
                connection: accepted.connection,
                hostname: accepted.peer_identity.hostname,
                remote_public_key: accepted.remote_public_key,
                handshake_hash: accepted.handshake_hash,
                address: IROH_ENDPOINT_ID.into(),
                interface: "iroh".into(),
            })
            .await
            .unwrap();
        server.await.unwrap();

        let server_code = server_manager
            .status()
            .await
            .peer
            .unwrap()
            .verification_code;
        let client_code = client_manager
            .status()
            .await
            .peer
            .unwrap()
            .verification_code;
        assert_eq!(server_code, client_code);

        server_manager.confirm().await.unwrap();
        client_manager.confirm().await.unwrap();
        for _ in 0..50 {
            if server_manager.status().await.phase == PairingPhase::Paired
                && client_manager.status().await.phase == PairingPhase::Paired
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        assert_eq!(server_manager.status().await.phase, PairingPhase::Paired);
        assert_eq!(client_manager.status().await.phase, PairingPhase::Paired);
        assert!(!server_manager.status().await.pairing_enabled);
        assert!(!client_manager.status().await.pairing_enabled);
        assert_eq!(
            server_settings.lock().await.trusted_peer_keys.get("client"),
            Some(&client_identity.public_key_base64())
        );
        assert_eq!(
            client_settings.lock().await.trusted_peer_keys.get("server"),
            Some(&server_identity.public_key_base64())
        );
        assert_eq!(
            server_settings
                .lock()
                .await
                .trusted_peer_addresses
                .get("client")
                .and_then(|routes| routes.get("iroh"))
                .map(String::as_str),
            Some(IROH_ENDPOINT_ID)
        );
        assert_eq!(
            client_settings
                .lock()
                .await
                .trusted_peer_addresses
                .get("server")
                .and_then(|routes| routes.get("iroh"))
                .map(String::as_str),
            Some(IROH_ENDPOINT_ID)
        );
    }

    #[tokio::test]
    async fn pairing_waits_for_remote_persisted_ack_before_marking_paired() {
        let server_identity = Arc::new(DeviceIdentity::generate_for_test());
        let client_identity = DeviceIdentity::generate_for_test();
        let server_settings = Arc::new(Mutex::new(Settings::default()));
        let server_manager = PairingManager::with_policy(
            server_settings.clone(),
            server_identity.clone(),
            Duration::from_secs(2),
            5,
            false,
        );
        server_manager.enable().await;

        let (mut client, server) =
            establish_in_memory_pair(&server_identity, &client_identity).await;
        server_manager
            .install_session(PendingPairing {
                connection: server,
                hostname: "client".into(),
                remote_public_key: client_identity.public_key().to_vec(),
                handshake_hash: vec![7; 32],
                address: "5866666666666666666666666666666666666666666666666666666666666666".into(),
                interface: "iroh".into(),
            })
            .await
            .unwrap();

        server_manager.confirm().await.unwrap();
        let frame = client.read_frame().await.unwrap();
        assert_eq!(frame.command, Command::PairingConfirm);
        client
            .write_frame(&Frame::try_new(Command::PairingConfirm, 0, 0, Vec::new()).unwrap())
            .await
            .unwrap();

        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_ne!(server_manager.status().await.phase, PairingPhase::Paired);

        let frame = client.read_frame().await.unwrap();
        assert_eq!(frame.command, Command::PairingPersisted);
        client
            .write_frame(&Frame::try_new(Command::PairingPersisted, 0, 0, Vec::new()).unwrap())
            .await
            .unwrap();

        for _ in 0..50 {
            if server_manager.status().await.phase == PairingPhase::Paired {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(server_manager.status().await.phase, PairingPhase::Paired);
        assert_eq!(
            server_settings.lock().await.trusted_peer_keys.get("client"),
            Some(&client_identity.public_key_base64())
        );
    }

    // ------------------------------------------------------------------
    // install_pairing_session (T110): inbound pairing session install.
    // ------------------------------------------------------------------

    fn test_peer_identity() -> PeerIdentity {
        PeerIdentity {
            hostname: "server".into(),
            tailscale_ip: String::new(),
            iroh_endpoint_id: None,
        }
    }

    async fn establish_in_memory_pair(
        server_identity: &Arc<DeviceIdentity>,
        client_identity: &DeviceIdentity,
    ) -> (
        crate::secure::SecureConnection,
        crate::secure::SecureConnection,
    ) {
        let expected_key = server_identity.public_key().to_vec();
        let (client_io, server_io) = tokio::io::duplex(256 * 1024);
        let server_identity = server_identity.clone();
        let server = tokio::spawn(async move {
            let accepted = crate::secure::accept(server_io, &server_identity, test_peer_identity())
                .await
                .unwrap();
            let mut connection = accepted.connection;
            crate::secure::write_ready(&mut connection).await.unwrap();
            connection
        });
        let client = crate::secure::connect(
            client_io,
            client_identity,
            test_peer_identity(),
            "server",
            &expected_key,
        )
        .await
        .unwrap();
        let server = server.await.unwrap();
        (client, server)
    }

    fn default_manager() -> Arc<PairingManager> {
        PairingManager::with_policy(
            Arc::new(Mutex::new(Settings::default())),
            Arc::new(DeviceIdentity::generate_for_test()),
            Duration::from_secs(60),
            5,
            false,
        )
    }

    #[tokio::test]
    async fn install_pairing_session_without_manager_writes_error_frame() {
        let server_identity = Arc::new(DeviceIdentity::generate_for_test());
        let client_identity = DeviceIdentity::generate_for_test();
        let (mut client, server) =
            establish_in_memory_pair(&server_identity, &client_identity).await;

        let result = install_pairing_session(
            None,
            server,
            "peer".into(),
            vec![1; 32],
            vec![2; 32],
            "192.168.1.5".into(),
            "lan".into(),
        )
        .await;
        assert!(result.is_ok());

        let frame = client.read_frame().await.unwrap();
        assert_eq!(frame.command, crate::protocol::Command::PeerError);
        assert!(
            String::from_utf8_lossy(&frame.payload).contains("Pairing over Iroh is not supported")
        );
    }

    #[tokio::test]
    async fn install_pairing_session_rejects_closed_window() {
        let server_identity = Arc::new(DeviceIdentity::generate_for_test());
        let client_identity = DeviceIdentity::generate_for_test();
        let (_client, server) = establish_in_memory_pair(&server_identity, &client_identity).await;

        let error = install_pairing_session(
            Some(&default_manager()),
            server,
            "peer".into(),
            vec![1; 32],
            vec![2; 32],
            "192.168.1.5".into(),
            "lan".into(),
        )
        .await
        .unwrap_err();
        assert!(matches!(error, PairingError::WindowClosed));
        assert_eq!(error.to_string(), "Pairing window is closed");
    }

    #[tokio::test]
    async fn install_pairing_session_rejects_self_pairing() {
        let server_identity = Arc::new(DeviceIdentity::generate_for_test());
        let client_identity = DeviceIdentity::generate_for_test();
        let (_client, server) = establish_in_memory_pair(&server_identity, &client_identity).await;
        let manager_identity = Arc::new(DeviceIdentity::generate_for_test());
        let manager = PairingManager::with_policy(
            Arc::new(Mutex::new(Settings::default())),
            manager_identity.clone(),
            Duration::from_secs(60),
            5,
            false,
        );
        let own_key = manager_identity.public_key().to_vec();

        let error = install_pairing_session(
            Some(&manager),
            server,
            "peer".into(),
            own_key,
            vec![2; 32],
            "192.168.1.5".into(),
            "lan".into(),
        )
        .await
        .unwrap_err();
        assert!(matches!(error, PairingError::SelfPairing));
        assert_eq!(error.to_string(), "Cannot pair this device with itself");
    }

    #[tokio::test]
    async fn install_pairing_session_rejects_invalid_interface() {
        let server_identity = Arc::new(DeviceIdentity::generate_for_test());
        let client_identity = DeviceIdentity::generate_for_test();
        let (_client, server) = establish_in_memory_pair(&server_identity, &client_identity).await;

        let error = install_pairing_session(
            Some(&default_manager()),
            server,
            "peer".into(),
            vec![1; 32],
            vec![2; 32],
            "192.168.1.5".into(),
            "bogus".into(),
        )
        .await
        .unwrap_err();
        assert!(matches!(error, PairingError::InvalidInterface));
        assert_eq!(error.to_string(), "Invalid pairing interface");
    }

    #[tokio::test]
    async fn diagnostics_events_follow_the_pairing_lifecycle() {
        let _guard = crate::diagnostics::diagnostics_test_lock().lock().await;
        use crate::diagnostics::{Event, Record};
        let emitted = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = emitted.clone();
        crate::diagnostics::set_collector(Some(Box::new(move |record: &Record| {
            sink.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(record.event);
        })));

        let manager = default_manager();
        let status = manager.enable().await;
        assert!(status.pairing_enabled);
        manager.begin_handshake().await.unwrap();
        let _ = manager.cancel().await;
        manager.enable().await;
        let _ = manager.cancel().await;

        crate::diagnostics::set_collector(None);
        let events = emitted
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // The collector is process-global, so unrelated PairingManager tests
        // running in parallel may emit their own events into this sink.
        // Assert the lifecycle as an ordered subsequence: the five events
        // this test triggers must all be present in their relative order,
        // while foreign events may appear anywhere between them.
        let expected = [
            Event::PairingWindowOpened,
            Event::PairingHandshakeStarted,
            Event::PairingWindowClosed,
            Event::PairingWindowOpened,
            Event::PairingWindowClosed,
        ];
        assert!(
            contains_ordered_subsequence(&events, &expected),
            "open/handshake/close/open/close must appear in order, got {events:?}"
        );
    }

    #[tokio::test]
    async fn diagnostics_pairing_failed_records_the_error() {
        let _guard = crate::diagnostics::diagnostics_test_lock().lock().await;
        use crate::diagnostics::{Event, Record};
        let errors = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = errors.clone();
        crate::diagnostics::set_collector(Some(Box::new(move |record: &Record| {
            // The collector is process-global: other PairingManager tests may
            // emit their own PairingFailed events concurrently (e.g. the
            // lockout tests). The error message uniquely identifies the
            // failure this test triggers, so only that event is collected.
            if let Some(error) = &record.error {
                if record.event == Event::PairingFailed && error.message == "simulated failure" {
                    sink.lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push(error.message.clone());
                }
            }
        })));

        let manager = default_manager();
        manager.enable().await;
        manager.record_failure("simulated failure").await;

        crate::diagnostics::set_collector(None);
        let messages = errors
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(
            messages.as_slice(),
            &["simulated failure".to_string()],
            "the targeted failure must be recorded exactly once"
        );
    }
}
