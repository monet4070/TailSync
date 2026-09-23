//! Versioned contracts shared by the local UI transports.
//!
//! The capability response is intentionally small.  It lets a client choose
//! an optional transport optimization without guessing from platform names or
//! falling back after an authentication or protocol error.

use serde::{de::Error as _, Deserialize, Deserializer, Serialize};

pub const LOCAL_CONTRACT_SCHEMA_VERSION: u32 = 1;
pub const LOCAL_PREVIEW_MAX_BYTES: u64 = 64 * 1024 * 1024;
pub const STABLE_ERROR_SCHEMA_VERSION: u32 = 1;

/// Stable local-IPC error categories. Deserializers deliberately map future
/// categories to `internal_error` so an older UI never crashes or guesses a
/// retry policy for an error it does not understand.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StableErrorCode {
    InvalidArgument,
    NotFound,
    TemporarilyBusy,
    StorageUnavailable,
    Unauthorized,
    ProtocolIncompatible,
    #[serde(other)]
    InternalError,
}

/// Coarse, non-sensitive context for localizing an error. These values never
/// contain clipboard data, file paths, peer identities, keys, or tokens.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StableErrorDetailClass {
    Request,
    Resource,
    Contention,
    Storage,
    Authorization,
    Protocol,
    Internal,
}

/// Versioned error returned to clients that explicitly opt in to the stable
/// local contract. The legacy message is used only for classification and is
/// never copied into this envelope.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, schemars::JsonSchema)]
pub struct StableErrorEnvelope {
    #[schemars(range(min = 1, max = 1))]
    pub schema_version: u32,
    pub code: StableErrorCode,
    pub retryable: bool,
    pub message_key: String,
    pub detail_class: StableErrorDetailClass,
}

impl<'de> Deserialize<'de> for StableErrorEnvelope {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct WireEnvelope {
            schema_version: u32,
            code: String,
            retryable: bool,
            message_key: String,
            detail_class: String,
        }

        let wire = WireEnvelope::deserialize(deserializer)?;
        if wire.schema_version != STABLE_ERROR_SCHEMA_VERSION {
            return Err(D::Error::custom("unsupported stable error schema version"));
        }
        let code = match wire.code.as_str() {
            "invalid_argument" => StableErrorCode::InvalidArgument,
            "not_found" => StableErrorCode::NotFound,
            "temporarily_busy" => StableErrorCode::TemporarilyBusy,
            "storage_unavailable" => StableErrorCode::StorageUnavailable,
            "unauthorized" => StableErrorCode::Unauthorized,
            "protocol_incompatible" => StableErrorCode::ProtocolIncompatible,
            "internal_error" => StableErrorCode::InternalError,
            _ => return Ok(Self::new(StableErrorCode::InternalError)),
        };
        let _ = (wire.retryable, wire.message_key, wire.detail_class);
        Ok(Self::new(code))
    }
}

impl StableErrorEnvelope {
    pub fn new(code: StableErrorCode) -> Self {
        let (retryable, message_key, detail_class) = match code {
            StableErrorCode::InvalidArgument => (
                false,
                "error.invalid_argument",
                StableErrorDetailClass::Request,
            ),
            StableErrorCode::NotFound => {
                (false, "error.not_found", StableErrorDetailClass::Resource)
            }
            StableErrorCode::TemporarilyBusy => (
                true,
                "error.temporarily_busy",
                StableErrorDetailClass::Contention,
            ),
            StableErrorCode::StorageUnavailable => (
                true,
                "error.storage_unavailable",
                StableErrorDetailClass::Storage,
            ),
            StableErrorCode::Unauthorized => (
                false,
                "error.unauthorized",
                StableErrorDetailClass::Authorization,
            ),
            StableErrorCode::ProtocolIncompatible => (
                false,
                "error.protocol_incompatible",
                StableErrorDetailClass::Protocol,
            ),
            StableErrorCode::InternalError => {
                (false, "error.internal", StableErrorDetailClass::Internal)
            }
        };
        Self {
            schema_version: STABLE_ERROR_SCHEMA_VERSION,
            code,
            retryable,
            message_key: message_key.to_string(),
            detail_class,
        }
    }

    /// Compatibility classifier used while platform adapters are migrated
    /// from text errors to typed sources. Matching affects only the stable
    /// category; the original message is intentionally discarded.
    pub fn from_legacy_message(command: &str, message: &str) -> Self {
        let command = command.to_ascii_lowercase();
        let message = message.to_ascii_lowercase();
        let contains_any = |needles: &[&str]| needles.iter().any(|needle| message.contains(needle));
        let code = if contains_any(&["unauthorized", "permission denied", "access denied"]) {
            StableErrorCode::Unauthorized
        } else if contains_any(&[
            "incompatible protocol",
            "protocol incompatible",
            "unsupported version",
            "requires v4",
        ]) {
            StableErrorCode::ProtocolIncompatible
        } else if contains_any(&[
            "missing ",
            "invalid ",
            "malformed",
            "must be",
            "out of range",
            "exceeds the",
            "unsupported type",
        ]) {
            StableErrorCode::InvalidArgument
        } else if contains_any(&[
            "not found",
            "no such",
            "unknown command",
            "does not exist",
            "unavailable entry",
        ]) {
            StableErrorCode::NotFound
        } else if contains_any(&[
            "temporarily busy",
            "would block",
            "locked",
            "timed out",
            "timeout",
            "try again",
        ]) {
            StableErrorCode::TemporarilyBusy
        } else if command.contains("storage")
            || contains_any(&[
                "database",
                "sqlite",
                "disk",
                "storage",
                "quota",
                "no space",
                "read-only",
            ])
        {
            StableErrorCode::StorageUnavailable
        } else {
            StableErrorCode::InternalError
        };
        Self::new(code)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, schemars::JsonSchema)]
pub struct LocalCapabilities {
    #[schemars(range(min = 1, max = 1))]
    pub schema_version: u32,
    #[schemars(range(min = 4, max = 4))]
    pub wire_version: u32,
    pub platform: String,
    #[schemars(range(min = 67108864, max = 67108864))]
    pub max_preview_bytes: u64,
    pub supports_binary_preview: bool,
    pub supports_runtime_snapshot: bool,
    pub supports_stable_errors: bool,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct DaemonStatus {
    pub tcp_server_healthy: bool,
    pub clipboard_monitor_healthy: bool,
    pub clipboard_monitor_failures: u64,
    pub active_routes: std::collections::HashMap<String, tailsync_core::peer::types::ActiveRoute>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct MacRuntimeSnapshot {
    pub revision: u64,
    pub history_version: u64,
    pub progress: Option<FileProgress>,
    pub storage: tailsync_core::db::StorageStatus,
    pub sync_enabled: bool,
    pub status: DaemonStatus,
    pub notifications: Vec<RuntimeNotification>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct WindowsRuntimeSnapshot {
    pub revision: u64,
    pub history_version: u64,
    pub progress: Option<FileProgress>,
    pub sync_warning: Option<tailsync_core::sync_warning::SyncWarning>,
    pub notifications: Vec<RuntimeNotification>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct PeerSnapshot {
    #[serde(flatten)]
    pub peer: tailsync_core::peer::types::PeerInfo,
    pub routes: Vec<tailsync_core::peer::types::PeerRouteSnapshot>,
    pub protocol_error: Option<String>,
    pub required_protocol_version: Option<u8>,
}

impl PeerSnapshot {
    pub fn new(
        peer: tailsync_core::peer::types::PeerInfo,
        paired_endpoint: Option<&String>,
        protocol_error: Option<String>,
        version: u8,
    ) -> Self {
        use tailsync_core::peer::types::{PeerRouteSnapshot, PeerStatus};
        let routes = peer
            .candidates
            .iter()
            .map(|candidate| {
                let connected = peer.current_address.as_deref() == Some(&candidate.address)
                    && peer.current_interface == Some(candidate.interface);
                PeerRouteSnapshot {
                    interface: candidate.interface,
                    address: candidate.address.clone(),
                    status: if connected {
                        PeerStatus::Connected
                    } else {
                        candidate.status
                    },
                    online: candidate.online,
                    connected,
                    latency_ms: candidate.latency,
                    pairing_endpoint: paired_endpoint == Some(&candidate.address),
                    rtt_capable: candidate.rtt_capable,
                }
            })
            .collect();
        Self {
            peer,
            routes,
            required_protocol_version: protocol_error.as_ref().map(|_| version),
            protocol_error,
        }
    }
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct LocalDeviceSnapshot {
    pub hostname: String,
    pub tailscale_ip: String,
    pub routes: Vec<tailsync_core::peer::types::PeerRouteSnapshot>,
    pub connection_mode: String,
    pub public_key: String,
    pub fingerprint: String,
    pub iroh_endpoint_id: Option<String>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct PeersResponse {
    #[serde(rename = "self")]
    pub local: LocalDeviceSnapshot,
    pub peers: Vec<PeerSnapshot>,
    pub paired_peer_endpoints: std::collections::HashMap<String, String>,
    pub discovery_error: Option<String>,
}

impl LocalCapabilities {
    pub fn current(
        platform: impl Into<String>,
        wire_version: u8,
        supports_binary_preview: bool,
        supports_runtime_snapshot: bool,
    ) -> Self {
        Self {
            schema_version: LOCAL_CONTRACT_SCHEMA_VERSION,
            wire_version: u32::from(wire_version),
            platform: platform.into(),
            max_preview_bytes: LOCAL_PREVIEW_MAX_BYTES,
            supports_binary_preview,
            supports_runtime_snapshot,
            supports_stable_errors: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_capabilities_are_stable_and_json_safe() {
        let capabilities = LocalCapabilities::current("macos", 4, true, true);
        assert_eq!(capabilities.schema_version, LOCAL_CONTRACT_SCHEMA_VERSION);
        assert_eq!(capabilities.wire_version, 4);
        assert_eq!(capabilities.max_preview_bytes, 64 * 1024 * 1024);
        assert!(!capabilities.supports_stable_errors);
        let round_trip = serde_json::from_value::<LocalCapabilities>(
            serde_json::to_value(&capabilities).expect("serialize capabilities"),
        )
        .expect("decode capabilities");
        assert_eq!(round_trip, capabilities);
    }

    #[test]
    fn stable_errors_have_fixed_policy_and_discard_legacy_details() {
        let envelope = StableErrorEnvelope::from_legacy_message(
            "change_storage_location",
            r#"database failed at C:\Users\private\history.db with token secret"#,
        );
        assert_eq!(envelope.schema_version, STABLE_ERROR_SCHEMA_VERSION);
        assert_eq!(envelope.code, StableErrorCode::StorageUnavailable);
        assert!(envelope.retryable);
        let serialized = serde_json::to_string(&envelope).expect("serialize stable error");
        assert!(!serialized.contains("Users"));
        assert!(!serialized.contains("private"));
        assert!(!serialized.contains("secret"));
    }

    #[test]
    fn unknown_stable_error_codes_fail_closed_to_internal_error() {
        let decoded: StableErrorEnvelope = serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "code": "future_error",
            "retryable": true,
            "message_key": "future.key",
            "detail_class": "future_detail"
        }))
        .expect("unknown code remains decodable");
        assert_eq!(decoded.code, StableErrorCode::InternalError);
        assert!(!decoded.retryable);
        assert_eq!(decoded.message_key, "error.internal");
        assert_eq!(decoded.detail_class, StableErrorDetailClass::Internal);
    }

    #[test]
    fn known_stable_error_codes_use_fixed_policy() {
        let decoded: StableErrorEnvelope = serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "code": "unauthorized",
            "retryable": true,
            "message_key": "untrusted.message",
            "detail_class": "future_detail"
        }))
        .expect("known code remains decodable");
        assert_eq!(
            decoded,
            StableErrorEnvelope::new(StableErrorCode::Unauthorized)
        );
    }

    #[test]
    fn stable_error_classifier_covers_public_categories() {
        let cases = [
            ("cmd", "missing id", StableErrorCode::InvalidArgument),
            ("cmd", "entry not found", StableErrorCode::NotFound),
            ("cmd", "database locked", StableErrorCode::TemporarilyBusy),
            (
                "get_history",
                "database unavailable",
                StableErrorCode::StorageUnavailable,
            ),
            ("cmd", "unauthorized", StableErrorCode::Unauthorized),
            (
                "cmd",
                "incompatible protocol version",
                StableErrorCode::ProtocolIncompatible,
            ),
            ("cmd", "unexpected failure", StableErrorCode::InternalError),
        ];
        for (command, message, expected) in cases {
            assert_eq!(
                StableErrorEnvelope::from_legacy_message(command, message).code,
                expected
            );
        }
    }
}

#[derive(Clone, Serialize, schemars::JsonSchema)]
pub struct FileProgress {
    pub batch_id: String,
    pub name: String,
    pub sent: u64,
    pub total: u64,
    pub active: bool,
    #[schemars(extend("enum" = ["sending", "receiving"]))]
    pub direction: String,
    pub device: String,
    pub completed_files: usize,
    pub total_files: usize,
    pub speed_bytes_per_second: u64,
    pub status: String,
    pub can_stop: bool,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, schemars::JsonSchema)]
pub struct RuntimeNotification {
    pub id: u64,
    pub level: String,
    pub message: String,
}
