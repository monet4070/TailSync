use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

fn empty_object() -> Value {
    Value::Object(serde_json::Map::new())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ThemeError {
    pub code: String,
    pub message: String,
    pub json_pointer: String,
    pub platforms: Vec<String>,
    pub severity: String,
    pub recoverable: bool,
    pub fallback_applied: bool,
}

impl ThemeError {
    pub(crate) fn new(code: &str, message: impl Into<String>, pointer: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            json_pointer: pointer.into(),
            platforms: vec!["windows".into(), "macos".into()],
            severity: "error".into(),
            recoverable: true,
            fallback_applied: false,
        }
    }

    pub(crate) fn warning(
        code: &str,
        message: impl Into<String>,
        pointer: impl Into<String>,
    ) -> Self {
        let mut diagnostic = Self::new(code, message, pointer);
        diagnostic.severity = "warning".into();
        diagnostic
    }
}

impl std::fmt::Display for ThemeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ThemeError {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeManifest {
    pub format_version: u32,
    pub id: String,
    pub version: String,
    pub min_core_version: String,
    pub name: BTreeMap<String, String>,
    pub extends: String,
    #[serde(default)]
    pub required_capabilities: Vec<String>,
    #[serde(default)]
    pub digest: Option<String>,
    #[serde(default)]
    pub signature: Option<Value>,
    pub light: Value,
    pub dark: Value,
    #[serde(default)]
    pub high_contrast: Option<ModePair>,
    #[serde(default = "empty_object")]
    pub foundation: Value,
    #[serde(default = "empty_object")]
    pub components: Value,
    #[serde(default)]
    pub platform: PlatformTokens,
    #[serde(default)]
    pub asset_slots: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModePair {
    pub light: Value,
    pub dark: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlatformTokens {
    #[serde(default = "empty_object")]
    pub windows: Value,
    #[serde(default = "empty_object")]
    pub macos: Value,
}

impl Default for PlatformTokens {
    fn default() -> Self {
        Self {
            windows: empty_object(),
            macos: empty_object(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetMetadata {
    pub key: String,
    pub digest: String,
    pub mime_type: String,
    pub bytes: u64,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeAssetDescriptor {
    pub slot: String,
    pub key: String,
    pub digest: String,
    pub mime_type: String,
    pub bytes: u64,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedTheme {
    pub id: String,
    pub digest: String,
    pub mode: String,
    pub high_contrast: bool,
    pub tokens: Value,
    pub provenance: BTreeMap<String, String>,
    pub assets: Vec<AssetMetadata>,
    pub asset_slots: BTreeMap<String, ThemeAssetDescriptor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeDescriptor {
    pub id: String,
    /// Opaque, storage-scoped handle used for destructive operations. This
    /// remains usable even when a damaged package no longer has a valid id.
    pub storage_handle: String,
    pub source: String,
    pub format_version: u32,
    pub version: String,
    pub digest: String,
    pub name: BTreeMap<String, String>,
    pub resolved: Option<ResolvedTheme>,
    pub resolved_light: Option<ResolvedTheme>,
    pub resolved_dark: Option<ResolvedTheme>,
    pub diagnostics: Vec<ThemeError>,
    pub status: String,
    pub platforms: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateThemeOptions {
    #[serde(default)]
    pub allow_same_version: bool,
    #[serde(default)]
    pub allow_downgrade: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeValidation {
    pub valid: bool,
    pub digest: Option<String>,
    pub candidate_version: Option<String>,
    pub preview: Option<ResolvedTheme>,
    pub diagnostics: Vec<ThemeError>,
    pub assets: Vec<AssetMetadata>,
    pub compatible: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalThemeSettings {
    pub active_theme_id: String,
    pub appearance: String,
    pub high_contrast: bool,
}
