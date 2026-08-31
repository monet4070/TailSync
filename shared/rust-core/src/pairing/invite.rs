//! One-time invitations for pairing a device over Iroh.
//!
//! The invitation is deliberately self-contained.  The receiver does not
//! need a TailSync web service to resolve it: the EndpointId is carried in the
//! payload and Iroh's configured address lookup performs the network lookup.
//! The random secret is only an application-level capability; trust is still
//! established by the existing Noise XX handshake and verification code.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use iroh::{EndpointAddr, EndpointId};
use ring::hmac::{self, HMAC_SHA256};
use ring::rand::{SecureRandom, SystemRandom};
use serde::Serialize;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;

const INVITE_MAGIC: &[u8; 4] = b"TSI1";
const INVITE_HELLO_MAGIC: &[u8; 4] = b"TSH1";
const INVITE_VERSION: u8 = 1;
const INVITE_FLAGS: u8 = 0;
const ENDPOINT_ID_LENGTH: usize = 32;
const INVITE_ID_LENGTH: usize = 16;
const SECRET_LENGTH: usize = 32;
const INVITE_BINARY_LENGTH: usize =
    4 + 1 + 1 + 8 + ENDPOINT_ID_LENGTH + INVITE_ID_LENGTH + SECRET_LENGTH;
const INVITE_HELLO_SIZE: usize = 4 + INVITE_ID_LENGTH + SECRET_LENGTH;
const MAX_INVITE_TEXT_LENGTH: usize = 512;
const INVITE_SECRET_DIGEST_KEY: &[u8] = b"tailsync pairing invite secret digest v1";

/// The existing pairing window is two minutes, so an invite cannot outlive
/// the receiver's authorization window in the current pairing state machine.
pub const DEFAULT_INVITE_TTL: Duration = Duration::from_secs(120);

/// Dedicated Iroh preface for an invite connection.  It keeps invite traffic
/// out of the already-paired business protocol and prevents a random business
/// connection from accidentally consuming an invite.
pub const INVITE_HELLO_LENGTH: usize = INVITE_HELLO_SIZE;
pub const INVITE_ACK_ACCEPTED: u8 = 0;
pub const INVITE_ACK_REJECTED: u8 = 1;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum InviteError {
    #[error("Invalid TailSync pairing invite")]
    InvalidFormat,
    #[error("Unsupported TailSync pairing invite version")]
    UnsupportedVersion,
    #[error("TailSync pairing invite has expired")]
    Expired,
    #[error("TailSync pairing invite is already in use")]
    AlreadyClaimed,
    #[error("TailSync pairing invite is unavailable")]
    Unavailable,
    #[error("Could not generate a secure pairing invite")]
    Randomness,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InviteHello {
    invite_id: [u8; INVITE_ID_LENGTH],
    secret: [u8; SECRET_LENGTH],
}

impl InviteHello {
    pub fn new(invite_id: [u8; INVITE_ID_LENGTH], secret: [u8; SECRET_LENGTH]) -> Self {
        Self { invite_id, secret }
    }

    pub fn encode(self) -> [u8; INVITE_HELLO_SIZE] {
        let mut output = [0u8; INVITE_HELLO_SIZE];
        output[..4].copy_from_slice(INVITE_HELLO_MAGIC);
        output[4..4 + INVITE_ID_LENGTH].copy_from_slice(&self.invite_id);
        output[4 + INVITE_ID_LENGTH..].copy_from_slice(&self.secret);
        output
    }

    pub fn decode(input: &[u8]) -> Result<Self, InviteError> {
        if input.len() != INVITE_HELLO_SIZE || &input[..4] != INVITE_HELLO_MAGIC {
            return Err(InviteError::InvalidFormat);
        }
        let mut invite_id = [0u8; INVITE_ID_LENGTH];
        invite_id.copy_from_slice(&input[4..4 + INVITE_ID_LENGTH]);
        let mut secret = [0u8; SECRET_LENGTH];
        secret.copy_from_slice(&input[4 + INVITE_ID_LENGTH..]);
        Ok(Self { invite_id, secret })
    }

    pub fn invite_id(&self) -> [u8; INVITE_ID_LENGTH] {
        self.invite_id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemotePairingInvite {
    endpoint_id: EndpointId,
    invite_id: [u8; INVITE_ID_LENGTH],
    secret: [u8; SECRET_LENGTH],
    expires_at: u64,
}

impl RemotePairingInvite {
    pub fn generate(endpoint_id: EndpointId, ttl: Duration) -> Result<Self, InviteError> {
        let rng = SystemRandom::new();
        let mut invite_id = [0u8; INVITE_ID_LENGTH];
        let mut secret = [0u8; SECRET_LENGTH];
        rng.fill(&mut invite_id)
            .map_err(|_| InviteError::Randomness)?;
        rng.fill(&mut secret).map_err(|_| InviteError::Randomness)?;
        Ok(Self {
            endpoint_id,
            invite_id,
            secret,
            expires_at: unix_now().saturating_add(ttl.as_secs()),
        })
    }

    pub fn parse(input: &str) -> Result<Self, InviteError> {
        let input = input.trim();
        if input.is_empty()
            || input.len() > MAX_INVITE_TEXT_LENGTH
            || input.chars().any(char::is_whitespace)
        {
            return Err(InviteError::InvalidFormat);
        }
        let payload = input
            .strip_prefix("tailsync://pair/v1/")
            .or_else(|| input.strip_prefix("TSI1-"))
            .ok_or(InviteError::InvalidFormat)?;
        if payload.is_empty() || payload.contains(['/', '?', '#', '=']) {
            return Err(InviteError::InvalidFormat);
        }
        let bytes = URL_SAFE_NO_PAD
            .decode(payload)
            .map_err(|_| InviteError::InvalidFormat)?;
        if bytes.len() != INVITE_BINARY_LENGTH || &bytes[..4] != INVITE_MAGIC {
            return Err(InviteError::InvalidFormat);
        }
        if bytes[4] != INVITE_VERSION {
            return Err(InviteError::UnsupportedVersion);
        }
        if bytes[5] != INVITE_FLAGS {
            return Err(InviteError::InvalidFormat);
        }
        let expires_at = u64::from_be_bytes(
            bytes[6..14]
                .try_into()
                .map_err(|_| InviteError::InvalidFormat)?,
        );
        if expires_at <= unix_now() {
            return Err(InviteError::Expired);
        }
        let endpoint_bytes: [u8; ENDPOINT_ID_LENGTH] = bytes[14..46]
            .try_into()
            .map_err(|_| InviteError::InvalidFormat)?;
        let endpoint_id =
            EndpointId::from_bytes(&endpoint_bytes).map_err(|_| InviteError::InvalidFormat)?;
        let mut invite_id = [0u8; INVITE_ID_LENGTH];
        invite_id.copy_from_slice(&bytes[46..62]);
        let mut secret = [0u8; SECRET_LENGTH];
        secret.copy_from_slice(&bytes[62..]);
        Ok(Self {
            endpoint_id,
            invite_id,
            secret,
            expires_at,
        })
    }

    pub fn endpoint_id(&self) -> EndpointId {
        self.endpoint_id
    }

    pub fn endpoint_id_string(&self) -> String {
        self.endpoint_id.to_string()
    }

    pub fn endpoint_addr(&self) -> EndpointAddr {
        self.endpoint_id.into()
    }

    pub fn invite_id(&self) -> [u8; INVITE_ID_LENGTH] {
        self.invite_id
    }

    pub fn expires_at(&self) -> u64 {
        self.expires_at
    }

    pub fn remaining_seconds(&self) -> u64 {
        self.expires_at.saturating_sub(unix_now())
    }

    pub fn hello(&self) -> InviteHello {
        InviteHello::new(self.invite_id, self.secret)
    }

    pub fn as_link(&self) -> String {
        let mut bytes = Vec::with_capacity(INVITE_BINARY_LENGTH);
        bytes.extend_from_slice(INVITE_MAGIC);
        bytes.push(INVITE_VERSION);
        bytes.push(INVITE_FLAGS);
        bytes.extend_from_slice(&self.expires_at.to_be_bytes());
        bytes.extend_from_slice(self.endpoint_id.as_bytes());
        bytes.extend_from_slice(&self.invite_id);
        bytes.extend_from_slice(&self.secret);
        format!("tailsync://pair/v1/{}", URL_SAFE_NO_PAD.encode(bytes))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteInviteState {
    Ready,
    Claimed,
}

#[derive(Debug, Clone, Serialize)]
pub struct RemoteInviteStatus {
    pub active: bool,
    pub state: Option<RemoteInviteState>,
    pub expires_at: Option<u64>,
    pub remaining_seconds: u64,
}

struct ActiveInvite {
    invite_id: [u8; INVITE_ID_LENGTH],
    secret_digest: [u8; 32],
    expires_at: u64,
    claimed: bool,
}

/// Holds the invitation capability only in memory.  Restarting TailSync,
/// cancelling the invitation, or successfully pairing invalidates it.
pub struct RemotePairingInviteManager {
    active: Mutex<Option<ActiveInvite>>,
}

impl Default for RemotePairingInviteManager {
    fn default() -> Self {
        Self::new()
    }
}

impl RemotePairingInviteManager {
    pub fn new() -> Self {
        Self {
            active: Mutex::new(None),
        }
    }

    pub fn create(
        self: &Arc<Self>,
        endpoint_id: EndpointId,
        ttl: Duration,
    ) -> Result<RemotePairingInvite, InviteError> {
        let invite = RemotePairingInvite::generate(endpoint_id, ttl)?;
        let mut active = self
            .active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *active = Some(ActiveInvite {
            invite_id: invite.invite_id,
            secret_digest: secret_digest(&invite.secret),
            expires_at: invite.expires_at,
            claimed: false,
        });
        Ok(invite)
    }

    pub fn create_from_endpoint_id(
        self: &Arc<Self>,
        endpoint_id: &str,
        ttl: Duration,
    ) -> Result<RemotePairingInvite, InviteError> {
        let endpoint_id =
            EndpointId::from_str(endpoint_id.trim()).map_err(|_| InviteError::InvalidFormat)?;
        self.create(endpoint_id, ttl)
    }

    pub fn claim(self: &Arc<Self>, hello: &InviteHello) -> Result<InviteClaim, InviteError> {
        let mut active = self
            .active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(active_invite) = active.as_mut() else {
            return Err(InviteError::Unavailable);
        };
        if active_invite.expires_at <= unix_now() {
            *active = None;
            return Err(InviteError::Expired);
        }
        if active_invite.invite_id != hello.invite_id {
            return Err(InviteError::Unavailable);
        }
        if !secret_matches(&hello.secret, &active_invite.secret_digest) {
            return Err(InviteError::Unavailable);
        }
        if active_invite.claimed {
            return Err(InviteError::AlreadyClaimed);
        }
        active_invite.claimed = true;
        Ok(InviteClaim {
            manager: self.clone(),
            invite_id: hello.invite_id,
            committed: false,
        })
    }

    pub fn status(&self) -> RemoteInviteStatus {
        let mut active = self
            .active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if active
            .as_ref()
            .is_some_and(|invite| invite.expires_at <= unix_now())
        {
            *active = None;
        }
        let Some(invite) = active.as_ref() else {
            return RemoteInviteStatus {
                active: false,
                state: None,
                expires_at: None,
                remaining_seconds: 0,
            };
        };
        RemoteInviteStatus {
            active: true,
            state: Some(if invite.claimed {
                RemoteInviteState::Claimed
            } else {
                RemoteInviteState::Ready
            }),
            expires_at: Some(invite.expires_at),
            remaining_seconds: invite.expires_at.saturating_sub(unix_now()),
        }
    }

    pub fn cancel(&self) {
        *self
            .active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }

    fn release(&self, invite_id: &[u8; INVITE_ID_LENGTH]) {
        let mut active = self
            .active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(invite) = active
            .as_mut()
            .filter(|invite| &invite.invite_id == invite_id)
        {
            invite.claimed = false;
        }
    }

    fn commit(&self, invite_id: &[u8; INVITE_ID_LENGTH]) {
        let mut active = self
            .active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if active
            .as_ref()
            .is_some_and(|invite| &invite.invite_id == invite_id)
        {
            *active = None;
        }
    }
}

/// A valid invite reservation.  Dropping it releases the reservation so a
/// transient transport failure can be retried; calling `commit` consumes it.
pub struct InviteClaim {
    manager: Arc<RemotePairingInviteManager>,
    invite_id: [u8; INVITE_ID_LENGTH],
    committed: bool,
}

impl InviteClaim {
    pub fn invite_id(&self) -> [u8; INVITE_ID_LENGTH] {
        self.invite_id
    }

    pub fn commit(mut self) {
        self.manager.commit(&self.invite_id);
        self.committed = true;
    }
}

impl Drop for InviteClaim {
    fn drop(&mut self) {
        if !self.committed {
            self.manager.release(&self.invite_id);
        }
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn secret_digest(secret: &[u8; SECRET_LENGTH]) -> [u8; 32] {
    let key = hmac::Key::new(HMAC_SHA256, INVITE_SECRET_DIGEST_KEY);
    let tag = hmac::sign(&key, secret);
    tag.as_ref()
        .try_into()
        .expect("HMAC-SHA256 has a 32-byte tag")
}

fn secret_matches(secret: &[u8; SECRET_LENGTH], expected: &[u8; 32]) -> bool {
    let key = hmac::Key::new(HMAC_SHA256, INVITE_SECRET_DIGEST_KEY);
    hmac::verify(&key, secret, expected).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::SecretKey;

    fn endpoint_id() -> EndpointId {
        SecretKey::generate().public()
    }

    #[test]
    fn invite_round_trips_as_a_native_link() {
        let manager = Arc::new(RemotePairingInviteManager::new());
        let invite = manager
            .create(endpoint_id(), Duration::from_secs(60))
            .unwrap();
        let link = invite.as_link();
        let parsed = RemotePairingInvite::parse(&link).unwrap();
        assert_eq!(parsed.endpoint_id(), invite.endpoint_id());
        assert_eq!(parsed.invite_id(), invite.invite_id());
        assert_eq!(parsed.expires_at(), invite.expires_at());
        assert_eq!(parsed.hello(), invite.hello());
        assert!(link.starts_with("tailsync://pair/v1/"));
    }

    #[test]
    fn malformed_and_tampered_invites_are_rejected() {
        assert_eq!(
            RemotePairingInvite::parse("https://example.invalid/pair/x"),
            Err(InviteError::InvalidFormat)
        );
        assert_eq!(
            RemotePairingInvite::parse("tailsync://pair/v1/not-base64"),
            Err(InviteError::InvalidFormat)
        );

        let manager = Arc::new(RemotePairingInviteManager::new());
        let invite = manager
            .create(endpoint_id(), Duration::from_secs(60))
            .unwrap();
        let link = invite.as_link();
        let payload = link.strip_prefix("tailsync://pair/v1/").unwrap();
        let mut bytes = URL_SAFE_NO_PAD.decode(payload).unwrap();
        *bytes.last_mut().unwrap() ^= 0x01;
        let tampered = format!("tailsync://pair/v1/{}", URL_SAFE_NO_PAD.encode(bytes));
        let parsed = RemotePairingInvite::parse(&tampered).unwrap();
        assert!(matches!(
            manager.claim(&parsed.hello()),
            Err(InviteError::Unavailable)
        ));
    }

    #[test]
    fn expired_invites_are_rejected_without_waiting() {
        let manager = Arc::new(RemotePairingInviteManager::new());
        let invite = manager.create(endpoint_id(), Duration::ZERO).unwrap();

        assert_eq!(
            RemotePairingInvite::parse(&invite.as_link()),
            Err(InviteError::Expired)
        );
        assert!(matches!(
            manager.claim(&invite.hello()),
            Err(InviteError::Expired)
        ));
        assert!(!manager.status().active);
    }

    #[test]
    fn a_claim_is_released_on_drop_and_consumed_on_commit() {
        let manager = Arc::new(RemotePairingInviteManager::new());
        let invite = manager
            .create(endpoint_id(), Duration::from_secs(60))
            .unwrap();
        {
            let claim = manager.claim(&invite.hello()).unwrap();
            assert_eq!(manager.status().state, Some(RemoteInviteState::Claimed));
            drop(claim);
        }
        assert_eq!(manager.status().state, Some(RemoteInviteState::Ready));
        let claim = manager.claim(&invite.hello()).unwrap();
        claim.commit();
        assert!(!manager.status().active);
        assert!(matches!(
            manager.claim(&invite.hello()),
            Err(InviteError::Unavailable)
        ));
    }
}
