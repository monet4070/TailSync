use tailsync_runtime::contracts::{StableErrorCode, StableErrorEnvelope};

/// The Tauri command boundary only serializes the fixed local error policy.
/// Source messages can contain file paths or peer details and stay on the Rust side.
#[derive(Debug, serde::Serialize)]
#[serde(transparent)]
pub struct CommandError(StableErrorEnvelope);

impl CommandError {
    pub(crate) fn code(code: StableErrorCode) -> Self {
        Self(StableErrorEnvelope::new(code))
    }

    #[cfg(test)]
    pub(crate) fn envelope(&self) -> &StableErrorEnvelope {
        &self.0
    }
}

impl From<String> for CommandError {
    fn from(_message: String) -> Self {
        Self::code(StableErrorCode::InternalError)
    }
}

impl From<&str> for CommandError {
    fn from(_message: &str) -> Self {
        Self::code(StableErrorCode::InternalError)
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
}
