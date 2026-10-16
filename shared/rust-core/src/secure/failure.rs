//! Finite, typed handshake/connection failure classification.
//!
//! The connect path produces [`ConnectionFailure`] values so the per-peer
//! worker can react to a *kind* instead of scanning flattened display text.
//! `Display` still prints the exact message the pre-typing code emitted, so
//! every existing log line, error string, and peer-facing rejection reply is
//! byte-for-byte unchanged.

use std::fmt;

use crate::protocol::ProtocolError;

/// Finite classification of a handshake/connection failure.
///
/// The set is deliberately small: a failure that cannot be attributed to one
/// of these buckets stays [`ConnectionFailureKind::Unknown`] rather than being
/// guessed from text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionFailureKind {
    /// The peer speaks an incompatible wire protocol version.
    ProtocolIncompatible,
    /// The authenticated peer identity did not match the expected peer.
    IdentityMismatch,
    /// The pairing window closed before the handshake completed.
    WindowClosed,
    /// A bounded handshake/connection attempt ran out of time.
    TimedOut,
    /// The transport closed (EOF/reset) before the handshake completed.
    ConnectionClosed,
    /// The peer advertisement was malformed or structurally invalid.
    InvalidAdvertisement,
    /// A transport-level failure with no finer structural classification.
    Transport,
    /// No structural classification was available.
    Unknown,
}

impl ConnectionFailureKind {
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
            Self::Transport => "transport",
            Self::Unknown => "unknown",
        }
    }
}

/// A typed handshake/connection failure: a finite [`ConnectionFailureKind`]
/// plus the display message. `Display` prints the message verbatim.
#[derive(Debug, Clone)]
pub struct ConnectionFailure {
    kind: ConnectionFailureKind,
    message: String,
}

impl ConnectionFailure {
    pub fn new(kind: ConnectionFailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// A failure with no structural classification.
    pub fn unknown(message: impl Into<String>) -> Self {
        Self::new(ConnectionFailureKind::Unknown, message)
    }

    pub const fn kind(&self) -> ConnectionFailureKind {
        self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn into_message(self) -> String {
        self.message
    }

    /// The exact message the pre-typing identity check produced for an
    /// incompatible advertised wire version. Kept byte-for-byte identical to
    /// the historical `PeerIdentity` deserializer text (`requires v4`) so both
    /// the log lines and the public `PeerIdentity` error string are unchanged.
    pub fn protocol_incompatible(peer_version: u8, app_version: Option<&str>) -> Self {
        let app = app_version
            .map(|version| format!(" ({version})"))
            .unwrap_or_default();
        Self::new(
            ConnectionFailureKind::ProtocolIncompatible,
            format!(
                "Incompatible TailSync protocol: peer{app} uses v{peer_version}, this version requires v{}. Update TailSync on both devices.",
                crate::protocol::LEGACY_VERSION
            ),
        )
    }

    /// Classify a framing-layer [`ProtocolError`] by its *structure*. A wire
    /// version mismatch maps to [`ConnectionFailureKind::ProtocolIncompatible`]
    /// using the historical `ProtocolError` display text unchanged.
    pub fn from_protocol_error(error: ProtocolError) -> Self {
        let kind = match &error {
            ProtocolError::UnsupportedVersion(_) => ConnectionFailureKind::ProtocolIncompatible,
            ProtocolError::Io(error) => io_failure_kind(error),
            _ => ConnectionFailureKind::Transport,
        };
        Self::new(kind, error.to_string())
    }

    /// Classify an IO error by its [`std::io::ErrorKind`] only.
    pub fn from_io(error: std::io::Error) -> Self {
        Self::new(io_failure_kind(&error), error.to_string())
    }

    /// Classify an already-boxed platform error, preserving a typed
    /// [`ConnectionFailure`] when present and otherwise falling back by
    /// structure. Never scans display text to recover a kind.
    pub fn from_boxed(error: Box<dyn std::error::Error + Send + Sync>) -> Self {
        let kind = classify_boxed(error.as_ref());
        match error.downcast::<ConnectionFailure>() {
            Ok(failure) => *failure,
            Err(error) => Self::new(kind, error.to_string()),
        }
    }

    /// The only place the legacy *peer-supplied* error text is interpreted.
    ///
    /// This is applied strictly to the payload of a real `PeerError` frame and
    /// never to local IO errors or internal strings, so a local message that
    /// merely contains the marker cannot be mistaken for a peer's protocol
    /// rejection. Only the fixed leading protocol category and exact window
    /// messages are recognized; unrelated peer text stays conservative.
    pub fn from_peer_error(message: &str) -> Self {
        if message.starts_with("Incompatible TailSync protocol:") {
            return Self::new(ConnectionFailureKind::ProtocolIncompatible, message);
        }
        if matches!(
            message,
            "Pairing window is closed" | "Pairing window was closed"
        ) {
            return Self::new(ConnectionFailureKind::WindowClosed, message);
        }
        Self::new(ConnectionFailureKind::Unknown, message)
    }
}

fn io_failure_kind(error: &std::io::Error) -> ConnectionFailureKind {
    use std::io::ErrorKind;
    match error.kind() {
        ErrorKind::UnexpectedEof
        | ErrorKind::ConnectionReset
        | ErrorKind::ConnectionAborted
        | ErrorKind::BrokenPipe
        | ErrorKind::NotConnected => ConnectionFailureKind::ConnectionClosed,
        ErrorKind::TimedOut => ConnectionFailureKind::TimedOut,
        _ => ConnectionFailureKind::Transport,
    }
}

/// Classify an already-boxed error by structure. Shared with
/// [`ConnectionFailure::from_boxed`]; never scans display text.
pub fn classify_boxed(
    error: &(dyn std::error::Error + Send + Sync + 'static),
) -> ConnectionFailureKind {
    if let Some(failure) = error.downcast_ref::<ConnectionFailure>() {
        return failure.kind();
    }
    if let Some(error) = error.downcast_ref::<std::io::Error>() {
        return io_failure_kind(error);
    }
    if let Some(error) = error.downcast_ref::<ProtocolError>() {
        return match error {
            ProtocolError::UnsupportedVersion(_) => ConnectionFailureKind::ProtocolIncompatible,
            ProtocolError::Io(error) => io_failure_kind(error),
            _ => ConnectionFailureKind::Transport,
        };
    }
    if error
        .downcast_ref::<tokio::time::error::Elapsed>()
        .is_some()
    {
        return ConnectionFailureKind::TimedOut;
    }
    ConnectionFailureKind::Unknown
}

impl fmt::Display for ConnectionFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ConnectionFailure {}
