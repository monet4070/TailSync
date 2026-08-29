use log::warn;
use std::path::PathBuf;
use thiserror::Error;

use crate::db;

/// Encrypted settings stored alongside the app
#[derive(Debug, Clone, PartialEq, schemars::JsonSchema, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct Settings {
    pub notifications_enabled: bool,
    pub progress_bar_enabled: bool,
    /// Whether this device broadcasts newly copied clipboard content.
    #[serde(default = "default_sync_enabled")]
    pub sync_enabled: bool,
    /// Optional global shortcut used to toggle sync. Empty disables it.
    #[serde(default = "default_sync_shortcut")]
    pub sync_shortcut: String,
    /// Optional global shortcut used to open the history window. Empty disables it.
    #[serde(default = "default_history_shortcut")]
    pub history_shortcut: String,
    #[schemars(range(min = 10, max = 500))]
    pub history_limit: u32,
    /// Bulk history and transfer storage. None keeps bulk data in the system
    /// application-data directory.
    #[serde(default)]
    pub storage_root: Option<String>,
    #[serde(default = "default_storage_quota_bytes")]
    #[schemars(range(min = 1073741824_u64, max = 17592186044416_u64))]
    pub storage_quota_bytes: u64,
    pub enabled_peers: std::collections::HashMap<String, bool>,
    #[schemars(with = "LanguageContract")]
    pub language: String, // "en" | "zh-CN"
    /// Transport policy used for peer discovery and delivery.
    #[serde(default = "default_connection_mode")]
    #[schemars(with = "ConnectionModeContract")]
    pub connection_mode: String,
    /// Explicit hostname -> pinned Noise static public key mapping.
    #[serde(default)]
    pub trusted_peer_keys: std::collections::HashMap<String, String>,
    /// Last known hostname -> IP mapping, used to reconnect paired devices
    /// when LAN broadcast discovery is unavailable during startup or sending.
    #[serde(default)]
    pub trusted_peer_addresses:
        std::collections::HashMap<String, std::collections::HashMap<String, String>>,
    /// Address used for the explicit pairing confirmation. Device identity is
    /// still pinned by public key so trusted peers can safely change routes.
    #[serde(default)]
    pub paired_peer_endpoints: std::collections::HashMap<String, String>,
}

/// Settings validation failures (T353 migration). Display strings reach the
/// UI and wire surfaces verbatim.
#[derive(Debug, Error)]
pub enum SettingsValidationError {
    #[error("history_limit must be between 10 and 500")]
    HistoryLimit,
    #[error("storage_quota_bytes must be between 1 GiB and 16 TiB")]
    StorageQuota,
    #[error("storage_root cannot be empty")]
    EmptyStorageRoot,
    #[error("connection_mode must be 'auto', 'lan_only', or 'tailscale_only'")]
    ConnectionMode,
    #[error("language must be 'en' or 'zh-CN'")]
    Language,
}

/// Settings-update orchestration failures (T353 migration).
#[derive(Debug, Error)]
pub enum SettingsUpdateError {
    #[error("{0}")]
    Validation(SettingsValidationError),
    #[error("{0}")]
    Persist(String),
    #[error("{0}")]
    Database(String),
}

#[allow(dead_code)]
#[derive(schemars::JsonSchema, serde::Serialize)]
enum LanguageContract {
    #[serde(rename = "en")]
    English,
    #[serde(rename = "zh-CN")]
    SimplifiedChinese,
}

#[allow(dead_code)]
#[derive(schemars::JsonSchema, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum ConnectionModeContract {
    Auto,
    LanOnly,
    TailscaleOnly,
}

fn default_connection_mode() -> String {
    "auto".to_string()
}

fn default_sync_enabled() -> bool {
    true
}

fn default_sync_shortcut() -> String {
    "CommandOrControl+Shift+S".to_string()
}

fn default_history_shortcut() -> String {
    "CommandOrControl+Shift+H".to_string()
}

pub const DEFAULT_STORAGE_QUOTA_BYTES: u64 = 10 * 1024 * 1024 * 1024;
pub const MIN_STORAGE_QUOTA_BYTES: u64 = 1024 * 1024 * 1024;
pub const MAX_STORAGE_QUOTA_BYTES: u64 = 16 * 1024 * 1024 * 1024 * 1024;

fn default_storage_quota_bytes() -> u64 {
    DEFAULT_STORAGE_QUOTA_BYTES
}

fn normalize_connection_mode(mode: String) -> String {
    match mode.as_str() {
        // Older builds called the direct local-network mode "manual".
        "manual" | "lan" => "lan_only".to_string(),
        "tailscale" => "tailscale_only".to_string(),
        "auto" | "lan_only" | "tailscale_only" => mode,
        other => {
            warn!("Unknown connection mode {other:?}; falling back to auto");
            default_connection_mode()
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            notifications_enabled: true,
            progress_bar_enabled: true,
            sync_enabled: default_sync_enabled(),
            sync_shortcut: default_sync_shortcut(),
            history_shortcut: default_history_shortcut(),
            history_limit: 100,
            storage_root: None,
            storage_quota_bytes: default_storage_quota_bytes(),
            enabled_peers: std::collections::HashMap::new(),
            language: "en".to_string(),
            connection_mode: default_connection_mode(),
            trusted_peer_keys: std::collections::HashMap::new(),
            trusted_peer_addresses: std::collections::HashMap::new(),
            paired_peer_endpoints: std::collections::HashMap::new(),
        }
    }
}

impl Settings {
    pub fn load() -> Result<Self, Box<dyn std::error::Error>> {
        let path = settings_path();
        Self::load_from_path(&path)
    }

    fn load_from_path(path: &std::path::Path) -> Result<Self, Box<dyn std::error::Error>> {
        let data = match std::fs::read_to_string(path) {
            Ok(data) => data,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Settings::default());
            }
            Err(error) => return Err(error.into()),
        };
        let mut value: serde_json::Value = serde_json::from_str(&data)?;
        // Pre-V2 builds persisted `theme` and `color_theme` inside
        // config-v2.json. The struct deliberately rejects them
        // (deny_unknown_fields), so they are recognized and removed here
        // before the remaining fields are parsed strictly: any *other*
        // unknown field still fails the load.
        let legacy = take_legacy_theme_fields(&mut value);
        let mut settings: Settings = serde_json::from_value(value.clone())?;
        settings.connection_mode = normalize_connection_mode(settings.connection_mode);
        if let Some((theme, color_theme)) = legacy {
            migrate_legacy_theme_fields(path, theme.as_deref(), color_theme.as_deref(), &value)?;
        }
        Ok(settings)
    }

    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let path = settings_path();
        let json = serde_json::to_string_pretty(self)?;
        write_atomic(&path, &json)?;
        Ok(())
    }

    pub fn validate_user_values(&self) -> Result<(), SettingsValidationError> {
        if !(10..=500).contains(&self.history_limit) {
            return Err(SettingsValidationError::HistoryLimit);
        }
        if !(MIN_STORAGE_QUOTA_BYTES..=MAX_STORAGE_QUOTA_BYTES).contains(&self.storage_quota_bytes)
        {
            return Err(SettingsValidationError::StorageQuota);
        }
        if self
            .storage_root
            .as_deref()
            .is_some_and(|path| path.trim().is_empty())
        {
            return Err(SettingsValidationError::EmptyStorageRoot);
        }
        if !matches!(
            self.connection_mode.as_str(),
            "auto" | "lan_only" | "tailscale_only"
        ) {
            return Err(SettingsValidationError::ConnectionMode);
        }
        if !matches!(self.language.as_str(), "en" | "zh-CN") {
            return Err(SettingsValidationError::Language);
        }
        Ok(())
    }

    /// Builds a validated user-facing settings update while retaining fields
    /// owned by pairing, peer management, and storage migration workflows.
    pub fn prepare_user_update(
        &self,
        mut requested: Self,
    ) -> Result<Self, SettingsValidationError> {
        requested.enabled_peers = self.enabled_peers.clone();
        requested.storage_root = self.storage_root.clone();
        requested.trusted_peer_keys = self.trusted_peer_keys.clone();
        requested.trusted_peer_addresses = self.trusted_peer_addresses.clone();
        requested.paired_peer_endpoints = self.paired_peer_endpoints.clone();
        requested.validate_user_values()?;
        Ok(requested)
    }

    pub fn toggle_peer(
        &mut self,
        hostname: &str,
        enabled: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut updated = self.clone();
        updated.enabled_peers.insert(hostname.to_string(), enabled);
        updated.save()?;
        *self = updated;
        Ok(())
    }

    pub fn set_sync_enabled(&mut self, enabled: bool) -> Result<(), Box<dyn std::error::Error>> {
        let mut updated = self.clone();
        updated.sync_enabled = enabled;
        updated.save()?;
        *self = updated;
        Ok(())
    }

    pub fn set_sync_shortcut(&mut self, shortcut: &str) -> Result<(), Box<dyn std::error::Error>> {
        let mut updated = self.clone();
        updated.sync_shortcut = shortcut.trim().to_string();
        updated.save()?;
        *self = updated;
        Ok(())
    }

    pub fn set_history_shortcut(
        &mut self,
        shortcut: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut updated = self.clone();
        updated.history_shortcut = shortcut.trim().to_string();
        updated.save()?;
        *self = updated;
        Ok(())
    }

    pub fn trust_peer(
        &mut self,
        hostname: &str,
        public_key: &str,
        mode: &str,
        address: Option<&str>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut updated = self.clone();
        updated.trust_peer_without_save(hostname, public_key, mode, address)?;
        updated.save()?;
        *self = updated;
        Ok(())
    }

    #[doc(hidden)]
    pub fn trust_peer_without_save(
        &mut self,
        hostname: &str,
        public_key: &str,
        mode: &str,
        address: Option<&str>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.trusted_peer_keys
            .insert(hostname.to_string(), public_key.to_string());
        if let Some(address) = address {
            self.remember_peer_address_without_save(hostname, mode, address)?;
            self.paired_peer_endpoints
                .insert(hostname.to_string(), address.to_string());
        }
        self.enabled_peers.insert(hostname.to_string(), true);
        Ok(())
    }

    pub fn remember_peer_address(
        &mut self,
        hostname: &str,
        mode: &str,
        address: &str,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let mut updated = self.clone();
        let changed = updated.remember_peer_address_without_save(hostname, mode, address)?;
        if changed {
            updated.save()?;
            *self = updated;
        }
        Ok(changed)
    }

    fn remember_peer_address_without_save(
        &mut self,
        hostname: &str,
        mode: &str,
        address: &str,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let address = address.trim();
        match mode {
            "lan" | "tailscale" => {
                address.parse::<std::net::IpAddr>()?;
            }
            "iroh" => {
                let _ = crate::iroh_transport::canonical_endpoint_id(address)?;
            }
            _ => return Err(format!("Unsupported connection mode: {mode}").into()),
        }
        let Some(public_key) = self.trusted_peer_keys.get(hostname).cloned() else {
            return Ok(false);
        };
        let duplicate_hostnames = self
            .trusted_peer_keys
            .iter()
            .filter(|(known_hostname, known_key)| {
                known_hostname.as_str() != hostname && known_key.as_str() == public_key
            })
            .map(|(known_hostname, _)| known_hostname.clone())
            .collect::<Vec<_>>();
        let mut changed = !duplicate_hostnames.is_empty();
        for duplicate in duplicate_hostnames {
            self.trusted_peer_keys.remove(&duplicate);
            self.trusted_peer_addresses.remove(&duplicate);
            self.paired_peer_endpoints.remove(&duplicate);
            self.enabled_peers.remove(&duplicate);
        }
        let addresses = self
            .trusted_peer_addresses
            .entry(hostname.to_string())
            .or_default();
        if addresses.get(mode).map(String::as_str) == Some(address) {
            return Ok(changed);
        }
        addresses.insert(mode.to_string(), address.to_string());
        changed = true;
        Ok(changed)
    }

    pub fn forget_peer(&mut self, hostname: &str) -> Result<(), Box<dyn std::error::Error>> {
        let mut updated = self.clone();
        updated.trusted_peer_keys.remove(hostname);
        updated.trusted_peer_addresses.remove(hostname);
        updated.paired_peer_endpoints.remove(hostname);
        updated.enabled_peers.remove(hostname);
        updated.save()?;
        *self = updated;
        Ok(())
    }
}

fn settings_path() -> PathBuf {
    db::get_data_dir().join("config-v2.json")
}

/// Remove the pre-V2 `theme` and `color_theme` keys from a parsed settings
/// object, returning them for migration. Only these two fields are ever
/// tolerated: every other unknown field still fails the strict parse that
/// runs afterwards.
fn take_legacy_theme_fields(
    value: &mut serde_json::Value,
) -> Option<(Option<String>, Option<String>)> {
    let serde_json::Value::Object(map) = value else {
        return None;
    };
    let mut take = |key: &str| {
        map.remove(key).and_then(|removed| match removed {
            serde_json::Value::String(text) => Some(text),
            other => {
                warn!("Legacy {key} field is not a string ({other}); ignoring it");
                None
            }
        })
    };
    let theme = take("theme");
    let color_theme = take("color_theme");
    (theme.is_some() || color_theme.is_some()).then_some((theme, color_theme))
}

/// Persist the legacy theme selection into the Theme V2 local settings and,
/// only once that V2 state is safely on disk, atomically strip the obsolete
/// fields from `config-v2.json`. The V2 write happens strictly before the
/// config cleanup, so a failure can never lose the user's theme choice.
fn migrate_legacy_theme_fields(
    config_path: &std::path::Path,
    theme: Option<&str>,
    color_theme: Option<&str>,
    cleaned_value: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let base = config_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    let outcome = crate::themes_v2::migrate_legacy_theme_selection_at(base, theme, color_theme)?;
    if matches!(
        outcome,
        crate::themes_v2::LegacyThemeMigration::Migrated
            | crate::themes_v2::LegacyThemeMigration::AlreadyPresent
    ) {
        // The V2 selection is on disk (written just now, or already present):
        // atomically drop the obsolete fields from the config file. A failed
        // rewrite leaves the original file untouched and is retried on the
        // next start.
        write_atomic(config_path, &serde_json::to_string_pretty(cleaned_value)?)?;
        log::info!("Migrated legacy theme fields to Theme V2 local settings");
    }
    Ok(())
}

/// Atomic write: temp file then rename, so a crash or error never leaves a
/// truncated config file behind. The temp file is owner-only from creation
/// and synced before the rename.
fn write_atomic(path: &std::path::Path, json: &str) -> Result<(), Box<dyn std::error::Error>> {
    crate::private_fs::write_private_file(path, json.as_bytes())?;
    Ok(())
}

/// Persist the settings after a user update (T303 extraction). The platform
/// surfaces pass `Settings::save`; tests inject fakes.
pub type SettingsPersist<'a> = &'a (dyn Fn(&Settings) -> Result<(), String> + Send + Sync);

/// Optional shortcut transaction used by surfaces that register global
/// shortcuts (Windows commands). Surfaces without a shortcut plugin pass
/// `None` and a plain save is used instead.
pub type ShortcutChangeHook<'a> =
    &'a (dyn Fn(&Settings, &Settings) -> Result<(), String> + Send + Sync);

/// What changed in a settings update, for the platform reaction.
#[derive(Debug)]
pub struct SettingsUpdateOutcome {
    pub mode_changed: bool,
    pub connection_mode: String,
}

/// Merge, validate, persist, and commit a user settings update, then apply
/// the resulting history/storage limits to the database (T303 extraction
/// from the Tauri command and API route surfaces).
///
/// The settings are only committed after persistence succeeds; database
/// limit enforcement happens after the commit, matching the command
/// surface's previous ordering. Changed global shortcuts are routed through
/// `hooks.apply_shortcut_change` when present (registration + save +
/// rollback); otherwise a plain save is used.
pub async fn apply_settings_update(
    settings: &tokio::sync::Mutex<Settings>,
    database: &tokio::sync::Mutex<db::HistoryDB>,
    requested: Settings,
    persist: SettingsPersist<'_>,
    apply_shortcut_change: Option<ShortcutChangeHook<'_>>,
) -> Result<SettingsUpdateOutcome, SettingsUpdateError> {
    let mut settings_guard = settings.lock().await;
    let new_settings = settings_guard
        .prepare_user_update(requested)
        .map_err(SettingsUpdateError::Validation)?;
    let history_limit = new_settings.history_limit as i64;
    let storage_quota_bytes = new_settings.storage_quota_bytes;
    let mode_changed = settings_guard.connection_mode != new_settings.connection_mode;
    let shortcuts_changed = settings_guard.sync_shortcut != new_settings.sync_shortcut
        || settings_guard.history_shortcut != new_settings.history_shortcut;
    let connection_mode = new_settings.connection_mode.clone();
    if shortcuts_changed {
        if let Some(apply_shortcut_change) = apply_shortcut_change {
            apply_shortcut_change(&settings_guard, &new_settings)
                .map_err(SettingsUpdateError::Persist)?;
        } else {
            persist(&new_settings).map_err(SettingsUpdateError::Persist)?;
        }
    } else {
        persist(&new_settings).map_err(SettingsUpdateError::Persist)?;
    }
    *settings_guard = new_settings;
    drop(settings_guard);
    let mut database_guard = database.lock().await;
    database_guard.set_max_history(history_limit);
    database_guard.set_storage_quota(storage_quota_bytes);
    database_guard
        .enforce_limits()
        .map_err(|error| SettingsUpdateError::Database(error.to_string()))?;
    Ok(SettingsUpdateOutcome {
        mode_changed,
        connection_mode,
    })
}

mod keystore;

pub use keystore::{decrypt, encrypt, initialize};
pub(crate) use keystore::{get_dek, is_key_store_error};

#[cfg(all(test, not(target_os = "macos")))]
use keystore::is_cargo_test_harness_executable;
#[cfg(all(test, target_os = "windows"))]
use keystore::move_windows_key_file_create_only;
#[cfg(all(test, target_os = "macos"))]
use keystore::{
    create_macos_keychain_item_in, is_cargo_test_harness_executable, read_macos_keychain_item_in,
};
#[cfg(test)]
use keystore::{
    decode_hex_key, validate_key_bytes, CreateOutcome, DataKey, DekCache, KeyStore, KeyStoreError,
    DEK_SIZE,
};

#[cfg(test)]
mod tests;
