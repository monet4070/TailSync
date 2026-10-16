//! Finite, typed pairing failure classification.
//!
//! [`PairingFailure`] pairs a finite [`PairingFailureKind`] with the exact
//! display message the pre-typing code emitted. Every `Display` path prints
//! that message verbatim, so the pairing status text, the local IPC error
//! string and both platform command replies stay byte-for-byte unchanged.
//!
//! A kind is only ever derived from a structural source: a finite enum variant,
//! an [`std::io::ErrorKind`], a framing [`ProtocolError`] or the typed
//! [`ConnectionFailure`] produced by the handshake layer. Display text is never
//! scanned to recover a category.

use std::fmt;

use serde::Serialize;

use crate::protocol::ProtocolError;
use crate::secure::{ConnectionFailure, ConnectionFailureKind};

use super::PairingError;

/// Finite classification of a pairing failure.
///
/// The set is deliberately small: a failure that cannot be attributed to one
/// of these buckets stays [`PairingFailureKind::Unknown`] rather than being
/// guessed from text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PairingFailureKind {
    // The peer speaks an incompatible wire protocol version.
    ProtocolIncompatible,
    // The authenticated peer identity did not match the expected peer.
    IdentityMismatch,
    // The pairing window, invite or session this request belonged to is gone.
    WindowClosed,
    // A bounded pairing attempt ran out of time.
    TimedOut,
    // The transport closed (EOF/reset) before the pairing completed.
    ConnectionClosed,
    // A peer-supplied advertisement/identity claim was structurally invalid.
    InvalidAdvertisement,
    // A target, interface, mode or invite argument was rejected.
    InvalidArgument,
    // Another pairing is already in progress, including glare arbitration.
    Busy,
    // The user or the peer cancelled the pairing.
    Cancelled,
    // A transport-level failure with no finer structural classification.
    Transport,
    // No structural classification was available.
    Unknown,
}

impl PairingFailureKind {
    /// Stable identifier for diagnostics that must not depend on the
    /// human-readable message.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProtocolIncompatible => "protocol_incompatible",
            Self::IdentityMismatch => "identity_mismatch",
            Self::WindowClosed => "window_closed",
            Self::TimedOut => "timed_out",
            Self::ConnectionClosed => "connection_closed",
            Self::InvalidAdvertisement => "invalid_advertisement",
            Self::InvalidArgument => "invalid_argument",
            Self::Busy => "busy",
            Self::Cancelled => "cancelled",
            Self::Transport => "transport",
            Self::Unknown => "unknown",
        }
    }
}

impl From<ConnectionFailureKind> for PairingFailureKind {
    /// Explicit mapping from the handshake-layer vocabulary. Kept as a plain
    /// match so a future `ConnectionFailureKind` variant fails to compile here
    /// instead of silently degrading to `Unknown`.
    fn from(kind: ConnectionFailureKind) -> Self {
        match kind {
            ConnectionFailureKind::ProtocolIncompatible => Self::ProtocolIncompatible,
            ConnectionFailureKind::IdentityMismatch => Self::IdentityMismatch,
            ConnectionFailureKind::WindowClosed => Self::WindowClosed,
            ConnectionFailureKind::TimedOut => Self::TimedOut,
            ConnectionFailureKind::ConnectionClosed => Self::ConnectionClosed,
            ConnectionFailureKind::InvalidAdvertisement => Self::InvalidAdvertisement,
            ConnectionFailureKind::Transport => Self::Transport,
            ConnectionFailureKind::Unknown => Self::Unknown,
        }
    }
}

/// A typed pairing failure: a finite [`PairingFailureKind`] plus the display
/// message. `Display` prints the message verbatim.
#[derive(Debug, Clone)]
pub struct PairingFailure {
    kind: PairingFailureKind,
    message: String,
}

impl PairingFailure {
    pub fn new(kind: PairingFailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// A failure with no structural classification.
    pub fn unknown(message: impl Into<String>) -> Self {
        Self::new(PairingFailureKind::Unknown, message)
    }

    pub const fn kind(&self) -> PairingFailureKind {
        self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn into_message(self) -> String {
        self.message
    }

    /// Re-wrap a classified failure with a legacy message prefix, keeping the
    /// kind. Used where the old code built `format!("{prefix}{error}")`.
    pub fn with_context(self, prefix: &str) -> Self {
        let Self { kind, message } = self;
        Self::new(kind, format!("{prefix}{message}"))
    }

    /// Classify an IO error by its [`std::io::ErrorKind`] only.
    pub fn from_io(error: std::io::Error) -> Self {
        Self::from(ConnectionFailure::from_io(error))
    }

    /// Classify a framing error by its structure, keeping the historical text.
    pub fn from_protocol_error(error: ProtocolError) -> Self {
        Self::from(ConnectionFailure::from_protocol_error(error))
    }

    /// Classify an already-boxed platform error, preserving a typed
    /// [`ConnectionFailure`] when present and otherwise falling back by
    /// structure. Never scans display text to recover a kind.
    pub fn from_boxed(error: Box<dyn std::error::Error + Send + Sync>) -> Self {
        Self::from(ConnectionFailure::from_boxed(error))
    }

    /// The only place legacy *peer-supplied* error text is interpreted, via the
    /// handshake layer's wire-boundary helper. Applied strictly to the payload
    /// of a real `PeerError` frame.
    pub fn from_peer_error(message: &str) -> Self {
        Self::from(ConnectionFailure::from_peer_error(message))
    }
}

impl From<ConnectionFailure> for PairingFailure {
    fn from(failure: ConnectionFailure) -> Self {
        Self::new(failure.kind().into(), failure.into_message())
    }
}

impl From<ProtocolError> for PairingFailure {
    fn from(error: ProtocolError) -> Self {
        Self::from_protocol_error(error)
    }
}

impl From<Box<dyn std::error::Error + Send + Sync>> for PairingFailure {
    fn from(error: Box<dyn std::error::Error + Send + Sync>) -> Self {
        Self::from_boxed(error)
    }
}

impl From<PairingError> for PairingFailure {
    /// Map a finite [`PairingError`] variant to its kind without reading the
    /// display text.
    ///
    /// Session-gone variants (`SessionInactive`, `SessionClosed`) map to
    /// [`PairingFailureKind::WindowClosed`]: from the caller's point of view the
    /// pairing window that request belonged to is no longer usable.
    /// `AlreadyInProgress`/`GlareSuperseded` are the busy cases.
    /// `Transport(String)` and `Verification(&str)` carry no finer structural
    /// source, so they stay coarse (`Transport`/`Unknown`).
    fn from(error: PairingError) -> Self {
        let kind = match &error {
            PairingError::WindowClosed
            | PairingError::SessionInactive
            | PairingError::SessionClosed => PairingFailureKind::WindowClosed,
            PairingError::AlreadyInProgress | PairingError::GlareSuperseded => {
                PairingFailureKind::Busy
            }
            PairingError::SelfPairing
            | PairingError::InvalidInterface
            | PairingError::NoVerification => PairingFailureKind::InvalidArgument,
            PairingError::Verification(_) => PairingFailureKind::Unknown,
            PairingError::Transport(_) => PairingFailureKind::Transport,
        };
        Self::new(kind, error.to_string())
    }
}

impl fmt::Display for PairingFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for PairingFailure {}
