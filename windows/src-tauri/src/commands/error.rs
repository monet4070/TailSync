use tailsync_runtime::contracts::{StableErrorCode, StableErrorEnvelope};

/// The Tauri command boundary only serializes the fixed local error policy.
/// Source messages can contain file paths or peer details and stay on the Rust side.
#[derive(Debug, serde::Serialize)]
#[serde(transparent)]
pub struct CommandError(StableErrorEnvelope);

impl CommandError {
    pub(crate) fn restore(outcome: &tailsync_runtime::history::RestoreOutcome) -> Self {
        Self(outcome.error_envelope())
    }
    pub(crate) fn code(code: StableErrorCode) -> Self {
        Self(StableErrorEnvelope::new(code))
    }

    /// Fixed category for a typed pairing failure. The failure's display text
    /// is discarded: it can carry a peer address, a private path or a token.
    pub(crate) fn pairing_failure(failure: &tailsync_core::pairing::PairingFailure) -> Self {
        Self(StableErrorEnvelope::from_pairing_failure(failure))
    }

    #[cfg(test)]
    pub(crate) fn envelope(&self) -> &StableErrorEnvelope {
        &self.0
    }
}

impl From<String> for CommandError {
    fn from(message: String) -> Self {
        Self::from(message.as_str())
    }
}

impl From<tailsync_core::pairing::PairingFailure> for CommandError {
    fn from(failure: tailsync_core::pairing::PairingFailure) -> Self {
        Self::pairing_failure(&failure)
    }
}

impl From<tailsync_core::pairing::PairingError> for CommandError {
    fn from(error: tailsync_core::pairing::PairingError) -> Self {
        Self::from(tailsync_core::pairing::PairingFailure::from(error))
    }
}

impl From<&str> for CommandError {
    fn from(message: &str) -> Self {
        Self::code(match message {
            "content analysis busy" | "content analysis is stale" | "history image is stale" => {
                StableErrorCode::TemporarilyBusy
            }
            "publish_partial" => StableErrorCode::PublishPartial,
            "publish_unknown" => StableErrorCode::PublishUnknown,
            "publish_superseded" => StableErrorCode::PublishSuperseded,
            "native_only" => StableErrorCode::NativeOnly,
            "originals_incomplete" => StableErrorCode::OriginalsIncomplete,
            "resources_incomplete" => StableErrorCode::ResourcesIncomplete,
            "no_single_image" => StableErrorCode::NoSingleImage,
            "format_unavailable" => StableErrorCode::FormatUnavailable,
            "unsupported_format" => StableErrorCode::UnsupportedFormat,
            _ => StableErrorCode::InternalError,
        })
    }
}

impl From<tailsync_core::themes_v2::ThemeError> for CommandError {
    fn from(error: tailsync_core::themes_v2::ThemeError) -> Self {
        let code = match error.code.as_str() {
            "THEME_NOT_FOUND" | "THEME_ASSET_NOT_FOUND" => StableErrorCode::NotFound,
            "THEME_IO" => StableErrorCode::StorageUnavailable,
            "THEME_ARCHIVE"
            | "THEME_MANIFEST"
            | "THEME_PACKAGE_TOO_LARGE"
            | "THEME_TOKEN_TYPE"
            | "THEME_COLOR_EXPRESSION"
            | "THEME_COLOR_REFERENCE"
            | "THEME_ASSET_SLOT" => StableErrorCode::InvalidArgument,
            _ => StableErrorCode::InternalError,
        };
        Self::code(code)
    }
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0.message_key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn display_admission_pressure_keeps_its_retry_policy_without_private_details() {
        for message in [
            "content analysis busy",
            "content analysis is stale",
            "history image is stale",
        ] {
            let error = CommandError::from(message);
            assert_eq!(error.envelope().code, StableErrorCode::TemporarilyBusy);
            assert!(error.envelope().retryable);
        }
        assert_eq!(
            CommandError::from("content analysis busy /private/input")
                .envelope()
                .code,
            StableErrorCode::InternalError
        );
    }
    #[test]
    fn restore_outcome_survives_the_command_boundary() {
        let error = CommandError::restore(&tailsync_runtime::history::RestoreOutcome::Unknown {
            detail: "private details".into(),
        });
        let encoded = serde_json::to_value(error).unwrap();
        assert_eq!(
            encoded["message_key"],
            "history.restore.code.publish_unknown"
        );
        assert_eq!(encoded["retryable"], true);
        assert!(!encoded.to_string().contains("private details"));
    }

    #[test]
    fn command_errors_serialize_without_source_details() {
        let error = CommandError::from("database failed at C:\\private\\history.db".to_string());
        let serialized = serde_json::to_string(&error).unwrap();
        assert_eq!(error.envelope().code, StableErrorCode::InternalError);
        assert!(!serialized.contains("private"));
        assert!(!serialized.contains("history.db"));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&serialized).unwrap()["schema_version"],
            1
        );
    }

    #[test]
    fn explicit_categories_use_fixed_policy() {
        let error = CommandError::code(StableErrorCode::Unauthorized);
        assert!(!error.envelope().retryable);
        assert_eq!(error.envelope().message_key, "error.unauthorized");
    }

    #[test]
    fn typed_pairing_failures_use_fixed_codes_without_peer_details() {
        use tailsync_core::pairing::{PairingError, PairingFailure, PairingFailureKind};

        let error = CommandError::from(PairingFailure::new(
            PairingFailureKind::ProtocolIncompatible,
            "Pairing handshake failed: Incompatible TailSync protocol: peer at /private/peer uses v2",
        ));
        let encoded = serde_json::to_string(&error).unwrap();
        assert_eq!(error.envelope().code, StableErrorCode::ProtocolIncompatible);
        assert_eq!(error.envelope().message_key, "error.protocol_incompatible");
        assert!(!encoded.contains("/private/peer"));

        // A rewritten display text that contradicts the kind, and carries a
        // private path plus a token, must not move the category or leak.
        let contradictory = CommandError::from(PairingFailure::new(
            PairingFailureKind::WindowClosed,
            "database locked at /private/history.db; token=secret; not found",
        ));
        assert_eq!(
            contradictory.envelope().code,
            StableErrorCode::PairingWindowClosed
        );
        let encoded = serde_json::to_string(&contradictory).unwrap();
        assert!(!encoded.contains("/private"));
        assert!(!encoded.contains("secret"));

        // A finite PairingError keeps its structural classification too.
        assert_eq!(
            CommandError::from(PairingError::WindowClosed)
                .envelope()
                .code,
            StableErrorCode::PairingWindowClosed
        );
        assert_eq!(
            CommandError::from(PairingError::AlreadyInProgress)
                .envelope()
                .code,
            StableErrorCode::PairingBusy
        );
        assert_eq!(
            CommandError::from(PairingError::NoVerification)
                .envelope()
                .code,
            StableErrorCode::InvalidArgument
        );
        // `Transport(String)` carries no finer structural source, so a text
        // that says "network failed" must not become a network category.
        assert_eq!(
            CommandError::from(PairingError::Transport("network failed".into()))
                .envelope()
                .code,
            StableErrorCode::InternalError
        );
    }
}
