use ring::hkdf::{Salt, HKDF_SHA256};
use serde::Serialize;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, watch, Mutex};
use tokio::time::Instant;

use crate::crypto::Settings;
use crate::identity::DeviceIdentity;
use crate::network::secure::SecureConnection;
use crate::protocol::{Command, Frame};

const PAIRING_CODE_CONTEXT: &[u8] = b"tailsync pairing verification code v1";
const X25519_PUBLIC_KEY_LENGTH: usize = 32;
const DEFAULT_PAIRING_WINDOW: Duration = Duration::from_secs(120);
const DEFAULT_MAX_FAILURES: u8 = 5;

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

pub(crate) struct PendingPairing {
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

    fn with_policy(
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

        let manager = Arc::downgrade(self);
        let generation = self.state.lock().await.generation;
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
        self.status().await
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

    pub async fn begin_handshake(&self) -> Result<(), String> {
        let mut state = self.state.lock().await;
        if !state.enabled {
            return Err("Pairing window is closed".to_string());
        }
        if state.control.is_some() {
            return Err("Another pairing is already in progress".to_string());
        }
        state.phase = PairingPhase::Handshaking;
        state.error = None;
        Ok(())
    }

    pub(crate) async fn install_session(
        self: &Arc<Self>,
        pending: PendingPairing,
    ) -> Result<(), String> {
        if pending.remote_public_key == self.identity.public_key() {
            return Err("Cannot pair this device with itself".to_string());
        }
        if !matches!(pending.interface.as_str(), "lan" | "tailscale") {
            return Err("Invalid pairing interface".to_string());
        }
        let verification_code = derive_verification_code(
            &pending.handshake_hash,
            self.identity.public_key(),
            &pending.remote_public_key,
        )?;
        let fingerprint = crate::identity::fingerprint(&pending.remote_public_key);
        let (control, receiver) = mpsc::channel(4);
        let session_id = {
            let mut state = self.state.lock().await;
            if !state.enabled {
                return Err("Pairing window is closed".to_string());
            }
            if state.control.is_some() {
                return Err("Another pairing is already in progress".to_string());
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

    pub async fn confirm(&self) -> Result<PairingStatus, String> {
        let control = {
            let state = self.state.lock().await;
            if !matches!(
                state.phase,
                PairingPhase::Verification | PairingPhase::WaitingForPeer
            ) {
                return Err("No pairing verification is awaiting confirmation".to_string());
            }
            state
                .control
                .clone()
                .ok_or_else(|| "Pairing session is no longer active".to_string())?
        };
        control
            .send(PairingAction::Confirm)
            .await
            .map_err(|_| "Pairing session is no longer active".to_string())?;
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
            state.error = Some(error.into());
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

    async fn expire(&self, generation: u64) {
        let control = {
            let mut state = self.state.lock().await;
            if !state.enabled || state.generation != generation {
                return;
            }
            let control = state.control.take();
            state.enabled = false;
            state.phase = PairingPhase::TimedOut;
            state.deadline = None;
            state.expires_at = None;
            state.error = Some("Pairing window timed out".to_string());
            state.generation = state.generation.wrapping_add(1);
            state.session_id = state.session_id.wrapping_add(1);
            control
        };
        self.window_signal.send_replace(false);
        if let Some(control) = control {
            let _ = control.send(PairingAction::Cancel).await;
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

        loop {
            tokio::select! {
                action = receiver.recv() => match action {
                    Some(PairingAction::Confirm) if !local_confirmed => {
                        let frame = Frame::new(Command::PairingConfirm, 0, 0, Vec::new());
                        if let Err(error) = connection.write_frame(&frame).await {
                            self.fail_session(session_id, format!("Could not confirm pairing: {error}")).await;
                            return;
                        }
                        local_confirmed = true;
                        self.set_confirmation(session_id, true, remote_confirmed).await;
                    }
                    Some(PairingAction::Confirm) => {}
                    Some(PairingAction::Cancel) => {
                        let _ = connection
                            .write_frame(&Frame::new(Command::PairingCancel, 0, 0, Vec::new()))
                            .await;
                        return;
                    }
                    None => return,
                },
                result = connection.read_frame() => match result {
                    Ok(frame) if frame.command == Command::PairingConfirm => {
                        remote_confirmed = true;
                        self.set_confirmation(session_id, local_confirmed, true).await;
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

            if local_confirmed && remote_confirmed {
                if let Err(error) = self
                    .finish_success(
                        session_id,
                        &hostname,
                        &remote_public_key,
                        &interface,
                        &address,
                    )
                    .await
                {
                    self.fail_session(session_id, error).await;
                }
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

    async fn finish_success(
        &self,
        session_id: u64,
        hostname: &str,
        remote_public_key: &[u8],
        interface: &str,
        address: &str,
    ) -> Result<(), String> {
        {
            let state = self.state.lock().await;
            if !state.enabled || state.session_id != session_id {
                return Err("Pairing session was closed before confirmation".to_string());
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
        result.map_err(|error| format!("Could not save paired device: {error}"))?;
        drop(settings);

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

    async fn expire_current_session(&self, session_id: u64) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::secure::{self, HandshakePurpose, PeerIdentity};
    use tokio::net::{TcpListener, TcpStream};

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
    async fn both_confirmations_save_both_peer_keys_and_close_windows() {
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
            let (stream, peer_address) = listener.accept().await.unwrap();
            let accepted = secure::accept_with_pairing_window(
                stream,
                &server_identity_for_task,
                PeerIdentity {
                    hostname: "server".into(),
                    tailscale_ip: String::new(),
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
                    address: peer_address.ip().to_string(),
                    interface: "lan".into(),
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
                address: address.ip().to_string(),
                interface: "lan".into(),
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
    }
}
