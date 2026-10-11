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

    pub(crate) fn from_pairing_failure(message: &str) -> Self {
        let code =
            if message.starts_with("Pairing handshake failed: Incompatible TailSync protocol:") {
                StableErrorCode::ProtocolIncompatible
            } else {
                StableErrorCode::InternalError
            };
        Self::code(code)
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

impl From<tailsync_core::db::PreviewErrorInfo> for CommandError {
    fn from(error: tailsync_core::db::PreviewErrorInfo) -> Self {
        use tailsync_core::db::PreviewErrorCode;
        if error.retryable {
            return Self::code(StableErrorCode::TemporarilyBusy);
        }
        let code = match error.code {
            PreviewErrorCode::EntryNotFound
            | PreviewErrorCode::BatchNotFound
            | PreviewErrorCode::EntryNotInBatch
            | PreviewErrorCode::MetadataUnavailable => StableErrorCode::NotFound,
            PreviewErrorCode::PayloadUnavailable => StableErrorCode::StorageUnavailable,
            PreviewErrorCode::UnsupportedType => StableErrorCode::UnsupportedFormat,
            PreviewErrorCode::PreviewTooLarge | PreviewErrorCode::InvalidSize => {
                StableErrorCode::InvalidArgument
            }
        };
        Self::code(code)
    }
}

impl From<tailsync_core::themes_v2::ThemeError> for CommandError {
    fn from(error: tailsync_core::themes_v2::ThemeError) -> Self {
        let code = match error.code.as_str() {
            "THEME_NOT_FOUND" | "THEME_ASSET_NOT_FOUND" => StableErrorCode::NotFound,
            "THEME_IO" => StableErrorCode::StorageUnavailable,
            "THEME_ID"
            | "THEME_ARCHIVE"
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
    fn incompatible_pairing_error_uses_fixed_code_without_peer_details() {
        let error = CommandError::from_pairing_failure(
            "Pairing handshake failed: Incompatible TailSync protocol: peer at /private/peer uses v2",
        );
        let encoded = serde_json::to_string(&error).unwrap();
        assert_eq!(error.envelope().code, StableErrorCode::ProtocolIncompatible);
        assert_eq!(error.envelope().message_key, "error.protocol_incompatible");
        assert!(!encoded.contains("/private/peer"));
    }
}
