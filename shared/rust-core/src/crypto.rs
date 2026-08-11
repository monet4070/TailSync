#[cfg(not(test))]
use log::info;
use log::warn;
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
use ring::rand::{SecureRandom, SystemRandom};
use std::path::PathBuf;
use std::sync::{Mutex as StdMutex, OnceLock};
use thiserror::Error;

use crate::db;

/// Encrypted settings stored alongside the app
#[derive(Debug, Clone, schemars::JsonSchema, serde::Serialize, serde::Deserialize)]
#[schemars(deny_unknown_fields)]
pub struct Settings {
    pub notifications_enabled: bool,
    pub progress_bar_enabled: bool,
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
    #[schemars(with = "ThemeContract")]
    pub theme: String, // "light" | "dark" | "system"
    #[serde(default = "default_color_theme")]
    #[schemars(with = "ColorThemeContract")]
    pub color_theme: String,
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

#[allow(dead_code)]
#[derive(schemars::JsonSchema, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum ThemeContract {
    System,
    Light,
    Dark,
}

#[allow(dead_code)]
#[derive(schemars::JsonSchema, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
enum ColorThemeContract {
    Tailsync,
    Ocean,
    Forest,
    Rose,
    HighContrast,
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

fn default_color_theme() -> String {
    "tailsync".to_string()
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
            history_limit: 100,
            storage_root: None,
            storage_quota_bytes: default_storage_quota_bytes(),
            enabled_peers: std::collections::HashMap::new(),
            theme: "system".to_string(),
            color_theme: default_color_theme(),
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
        let mut settings: Settings = serde_json::from_str(&data)?;
        settings.connection_mode = normalize_connection_mode(settings.connection_mode);
        Ok(settings)
    }

    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let path = settings_path();
        // Atomic write: temp file then rename
        let tmp = path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    pub fn validate_user_values(&self) -> Result<(), String> {
        if !(10..=500).contains(&self.history_limit) {
            return Err("history_limit must be between 10 and 500".to_string());
        }
        if !(MIN_STORAGE_QUOTA_BYTES..=MAX_STORAGE_QUOTA_BYTES).contains(&self.storage_quota_bytes)
        {
            return Err("storage_quota_bytes must be between 1 GiB and 16 TiB".to_string());
        }
        if self
            .storage_root
            .as_deref()
            .is_some_and(|path| path.trim().is_empty())
        {
            return Err("storage_root cannot be empty".to_string());
        }
        if !matches!(self.theme.as_str(), "system" | "light" | "dark") {
            return Err("theme must be 'system', 'light', or 'dark'".to_string());
        }
        if !matches!(
            self.color_theme.as_str(),
            "tailsync" | "ocean" | "forest" | "rose" | "high-contrast"
        ) {
            return Err(
                "color_theme must be 'tailsync', 'ocean', 'forest', 'rose', or 'high-contrast'"
                    .to_string(),
            );
        }
        if !matches!(
            self.connection_mode.as_str(),
            "auto" | "lan_only" | "tailscale_only"
        ) {
            return Err(
                "connection_mode must be 'auto', 'lan_only', or 'tailscale_only'".to_string(),
            );
        }
        if !matches!(self.language.as_str(), "en" | "zh-CN") {
            return Err("language must be 'en' or 'zh-CN'".to_string());
        }
        Ok(())
    }

    /// Builds a validated user-facing settings update while retaining fields
    /// owned by pairing, peer management, and storage migration workflows.
    pub fn prepare_user_update(&self, mut requested: Self) -> Result<Self, String> {
        requested.enabled_peers = self.enabled_peers.clone();
        requested.storage_root = self.storage_root.clone();
        requested.trusted_peer_keys = self.trusted_peer_keys.clone();
        requested.trusted_peer_addresses = self.trusted_peer_addresses.clone();
        requested.paired_peer_endpoints = self.paired_peer_endpoints.clone();
        requested.validate_user_values()?;
        Ok(requested)
    }

    pub fn set_theme(&mut self, theme: &str) -> Result<(), Box<dyn std::error::Error>> {
        let mut updated = self.clone();
        updated.theme = theme.to_string();
        updated.save()?;
        *self = updated;
        Ok(())
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

// ─── OS Keychain Integration ──────────────────────────────────────

const DEK_SIZE: usize = 32;
pub(crate) type DataKey = [u8; DEK_SIZE];

#[cfg(all(not(test), target_os = "macos"))]
const MACOS_KEYCHAIN_LEGACY_SERVICE: &str = "com.tailsync.app";
#[cfg(all(not(test), target_os = "macos"))]
const MACOS_KEYCHAIN_V2_SERVICE: &str = "com.tailsync.app.dek-v2";
#[cfg(all(not(test), target_os = "macos"))]
const MACOS_KEYCHAIN_ACCOUNT: &str = "encryption-key";

#[derive(Debug, Error)]
pub(crate) enum KeyStoreError {
    #[error("encryption key does not exist")]
    NotFound,
    #[error("encryption key access was denied: {0}")]
    AccessDenied(String),
    #[error("encryption key is corrupt: {0}")]
    Corrupt(String),
    #[cfg(any(test, target_os = "windows"))]
    #[error("encryption key I/O failed while {operation}: {source}")]
    Io {
        operation: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("encryption key platform operation failed while {operation}: {message}")]
    Platform {
        operation: &'static str,
        message: String,
    },
}

pub(crate) fn is_key_store_error(error: &(dyn std::error::Error + 'static)) -> bool {
    error.downcast_ref::<KeyStoreError>().is_some()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CreateOutcome {
    Created,
    AlreadyExists,
}

trait KeyStore {
    fn read(&self) -> Result<DataKey, KeyStoreError>;
    fn create(&self, key: &DataKey) -> Result<CreateOutcome, KeyStoreError>;
}

struct DekCache {
    key: OnceLock<DataKey>,
    initialization: StdMutex<()>,
}

impl DekCache {
    const fn new() -> Self {
        Self {
            key: OnceLock::new(),
            initialization: StdMutex::new(()),
        }
    }

    #[cfg(not(test))]
    fn get_or_try_init<S: KeyStore>(&self, store: &S) -> Result<DataKey, KeyStoreError> {
        self.get_or_try_init_with(store, generate_data_key)
    }

    fn get_or_try_init_with<S, F>(&self, store: &S, generate: F) -> Result<DataKey, KeyStoreError>
    where
        S: KeyStore,
        F: FnOnce() -> Result<DataKey, KeyStoreError>,
    {
        if let Some(key) = self.key.get() {
            return Ok(*key);
        }

        // Recover from poisoning so a panic cannot permanently disable retries.
        let _guard = self
            .initialization
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(key) = self.key.get() {
            return Ok(*key);
        }

        let key = match store.read() {
            Ok(key) => key,
            Err(KeyStoreError::NotFound) => {
                let candidate = generate()?;
                match store.create(&candidate)? {
                    CreateOutcome::Created => candidate,
                    CreateOutcome::AlreadyExists => store.read().map_err(|error| match error {
                        KeyStoreError::NotFound => KeyStoreError::Platform {
                            operation: "re-reading a concurrently created key",
                            message: "the key disappeared after the create race".to_string(),
                        },
                        other => other,
                    })?,
                }
            }
            Err(error) => return Err(error),
        };

        let _ = self.key.set(key);
        self.key
            .get()
            .copied()
            .ok_or_else(|| KeyStoreError::Platform {
                operation: "caching the encryption key",
                message: "the process key cache was not initialized".to_string(),
            })
    }
}

#[cfg(not(test))]
static DEK_CACHE: DekCache = DekCache::new();

#[cfg(not(test))]
struct SystemKeyStore;

/// Get or initialize the data encryption key from the OS keychain
pub(crate) fn get_dek() -> Result<DataKey, KeyStoreError> {
    #[cfg(test)]
    return Ok([0x54; DEK_SIZE]);

    #[cfg(not(test))]
    {
        #[cfg(feature = "test-support")]
        if running_under_cargo_test_harness() {
            return Ok([0x54; DEK_SIZE]);
        }
        DEK_CACHE.get_or_try_init(&SystemKeyStore)
    }
}

#[cfg(all(not(test), feature = "test-support"))]
fn running_under_cargo_test_harness() -> bool {
    let Ok(executable) = std::env::current_exe() else {
        return false;
    };
    is_cargo_test_harness_executable(&executable)
}

#[cfg(any(test, feature = "test-support"))]
fn is_cargo_test_harness_executable(executable: &std::path::Path) -> bool {
    let Some(parent) = executable
        .parent()
        .and_then(std::path::Path::file_name)
        .and_then(std::ffi::OsStr::to_str)
    else {
        return false;
    };
    let Some(name) = executable.file_stem().and_then(std::ffi::OsStr::to_str) else {
        return false;
    };
    let Some(hash) = name.strip_prefix("tailsync_lib-") else {
        return false;
    };

    // Cargo places libtest executables in target/<profile>/deps and appends a
    // hexadecimal crate hash. Requiring both guards against accidentally
    // enabling the test DEK in a normal or `--all-features` application build.
    parent == "deps" && hash.len() >= 16 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Verify that the process can load the existing data key or safely create
/// the first one before any encrypted state is opened.
pub fn initialize() -> Result<(), Box<dyn std::error::Error>> {
    get_dek()
        .map(|_| ())
        .map_err(|error| Box::new(error) as Box<dyn std::error::Error>)
}

#[cfg(not(test))]
fn generate_data_key() -> Result<DataKey, KeyStoreError> {
    let rng = SystemRandom::new();
    let mut key = [0u8; DEK_SIZE];
    rng.fill(&mut key).map_err(|_| KeyStoreError::Platform {
        operation: "generating a new encryption key",
        message: "the operating system random generator failed".to_string(),
    })?;
    Ok(key)
}

fn validate_key_bytes(bytes: &[u8], source: &str) -> Result<DataKey, KeyStoreError> {
    bytes.try_into().map_err(|_| {
        KeyStoreError::Corrupt(format!(
            "{source} contained {} bytes; expected {DEK_SIZE}",
            bytes.len()
        ))
    })
}

#[cfg(any(test, target_os = "macos"))]
fn decode_hex_key(encoded: &str, source: &str) -> Result<DataKey, KeyStoreError> {
    let decoded = hex::decode(encoded.trim()).map_err(|error| {
        KeyStoreError::Corrupt(format!("{source} is not valid hexadecimal: {error}"))
    })?;
    validate_key_bytes(&decoded, source)
}

#[cfg(all(not(test), target_os = "macos"))]
fn create_macos_keychain_item(
    service: &str,
    account: &str,
    password: &[u8],
) -> Result<CreateOutcome, KeyStoreError> {
    create_macos_keychain_item_in(None, service, account, password)
}

#[cfg(all(not(test), target_os = "macos"))]
fn read_macos_keychain_item(service: &str, account: &str) -> Result<DataKey, KeyStoreError> {
    read_macos_keychain_item_in(None, service, account)
}

#[cfg(all(not(test), target_os = "macos"))]
fn read_legacy_macos_keychain_item() -> Result<DataKey, KeyStoreError> {
    let output = std::process::Command::new("security")
        .args([
            "find-generic-password",
            "-s",
            MACOS_KEYCHAIN_LEGACY_SERVICE,
            "-a",
            MACOS_KEYCHAIN_ACCOUNT,
            "-w",
        ])
        .output()
        .map_err(|source| {
            classify_macos_cli_io_error("reading the legacy macOS Keychain", source)
        })?;

    if output.status.success() {
        let encoded = std::str::from_utf8(&output.stdout).map_err(|_| {
            KeyStoreError::Corrupt("the legacy macOS Keychain value is not UTF-8".to_string())
        })?;
        return decode_hex_key(encoded, "the legacy macOS Keychain value");
    }

    match output.status.code() {
        // The security CLI returns the low byte of the Security.framework OSStatus.
        Some(44) => Err(KeyStoreError::NotFound), // errSecItemNotFound (-25300)
        Some(36 | 51 | 128) => Err(KeyStoreError::AccessDenied(command_failure_message(
            &output,
        ))),
        _ => Err(KeyStoreError::Platform {
            operation: "reading the legacy macOS Keychain",
            message: command_failure_message(&output),
        }),
    }
}

#[cfg(target_os = "macos")]
fn read_macos_keychain_item_in(
    keychain: Option<security_framework::os::macos::keychain::SecKeychain>,
    service: &str,
    account: &str,
) -> Result<DataKey, KeyStoreError> {
    use security_framework::item::{ItemClass, ItemSearchOptions, SearchResult};
    use security_framework_sys::base::errSecItemNotFound;

    let mut search = ItemSearchOptions::new();
    search
        .class(ItemClass::generic_password())
        .service(service)
        .account(account)
        .load_data(true);
    if let Some(keychain) = keychain.as_ref() {
        search.keychains(std::slice::from_ref(keychain));
    }
    match search.search() {
        Ok(results) => {
            let [SearchResult::Data(stored)] = results.as_slice() else {
                return Err(KeyStoreError::Corrupt(
                    "the macOS Keychain returned an unexpected item representation".to_string(),
                ));
            };
            let encoded = std::str::from_utf8(stored).map_err(|_| {
                KeyStoreError::Corrupt("the macOS Keychain value is not UTF-8".to_string())
            })?;
            decode_hex_key(encoded, "the macOS Keychain value")
        }
        Err(error) if error.code() == errSecItemNotFound => Err(KeyStoreError::NotFound),
        Err(error) if is_macos_keychain_access_denied(error.code()) => {
            Err(KeyStoreError::AccessDenied(format!(
                "macOS Keychain refused to read the item: {error}"
            )))
        }
        Err(error) => Err(KeyStoreError::Platform {
            operation: "reading the macOS Keychain item",
            message: format!("Security.framework returned {}: {error}", error.code()),
        }),
    }
}

#[cfg(target_os = "macos")]
fn create_macos_keychain_item_in(
    keychain: Option<security_framework::os::macos::keychain::SecKeychain>,
    service: &str,
    account: &str,
    password: &[u8],
) -> Result<CreateOutcome, KeyStoreError> {
    use core_foundation::data::CFData;
    use security_framework::item::{ItemAddOptions, ItemAddValue, ItemClass, Location};
    use security_framework_sys::base::errSecDuplicateItem;

    let mut options = ItemAddOptions::new(ItemAddValue::Data {
        class: ItemClass::generic_password(),
        data: CFData::from_buffer(password),
    });
    options.set_service(service).set_account_name(account);
    if let Some(keychain) = keychain {
        options.set_location(Location::FileKeychain(keychain));
    }
    match options.add() {
        Ok(()) => Ok(CreateOutcome::Created),
        Err(error) if error.code() == errSecDuplicateItem => Ok(CreateOutcome::AlreadyExists),
        Err(error) if is_macos_keychain_access_denied(error.code()) => {
            Err(KeyStoreError::AccessDenied(format!(
                "macOS Keychain refused to create the item: {error}"
            )))
        }
        Err(error) => Err(KeyStoreError::Platform {
            operation: "creating the macOS Keychain item",
            message: format!("Security.framework returned {}: {error}", error.code()),
        }),
    }
}

#[cfg(target_os = "macos")]
fn is_macos_keychain_access_denied(status: i32) -> bool {
    use security_framework_sys::base::errSecAuthFailed;

    // These OSStatus values are stable Security.framework errors but are not
    // all exported by security-framework-sys.
    const ERR_SEC_USER_CANCELED: i32 = -128;
    const ERR_SEC_INTERACTION_NOT_ALLOWED: i32 = -25308;
    status == errSecAuthFailed
        || status == ERR_SEC_USER_CANCELED
        || status == ERR_SEC_INTERACTION_NOT_ALLOWED
}

/// Encrypt plaintext using AES-256-GCM
pub fn encrypt(plaintext: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    if plaintext.is_empty() {
        return Ok(vec![]);
    }

    let dek = get_dek()?;
    let unbound_key = UnboundKey::new(&AES_256_GCM, &dek).map_err(|_| "Invalid key")?;
    let key = LessSafeKey::new(unbound_key);

    let rng = SystemRandom::new();
    let mut nonce_bytes = [0u8; 12];
    rng.fill(&mut nonce_bytes)
        .map_err(|_| "Failed to generate nonce")?;
    let nonce = Nonce::assume_unique_for_key(nonce_bytes);

    let mut in_out = plaintext.to_vec();
    key.seal_in_place_append_tag(nonce, Aad::empty(), &mut in_out)
        .map_err(|_| "Encryption failed")?;

    // Prepend nonce to ciphertext: [nonce(12)] + [ciphertext + tag]
    let mut result = nonce_bytes.to_vec();
    result.extend_from_slice(&in_out);
    Ok(result)
}

/// Decrypt ciphertext using AES-256-GCM
pub fn decrypt(ciphertext: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    if ciphertext.is_empty() {
        return Ok(vec![]);
    }

    if ciphertext.len() < 12 {
        return Err("Ciphertext too short".into());
    }

    let dek = get_dek()?;
    let unbound_key = UnboundKey::new(&AES_256_GCM, &dek).map_err(|_| "Invalid key")?;
    let key = LessSafeKey::new(unbound_key);

    let (nonce_bytes, encrypted) = ciphertext.split_at(12);
    let nonce = Nonce::assume_unique_for_key(nonce_bytes.try_into().map_err(|_| "Invalid nonce")?);

    let mut in_out = encrypted.to_vec();
    let plaintext = key
        .open_in_place(nonce, Aad::empty(), &mut in_out)
        .map_err(|_| "Decryption failed")?;

    Ok(plaintext.to_vec())
}

#[cfg(not(test))]
impl KeyStore for SystemKeyStore {
    fn read(&self) -> Result<DataKey, KeyStoreError> {
        #[cfg(target_os = "macos")]
        {
            match read_macos_keychain_item(MACOS_KEYCHAIN_V2_SERVICE, MACOS_KEYCHAIN_ACCOUNT) {
                Ok(key) => Ok(key),
                Err(KeyStoreError::NotFound) => read_legacy_macos_keychain_item(),
                Err(error) => Err(error),
            }
        }

        #[cfg(target_os = "windows")]
        {
            use windows_sys::Win32::Security::Cryptography::{
                CryptUnprotectData, CRYPT_INTEGER_BLOB,
            };

            let path = db::get_data_dir().join(".dek");
            let protected = match std::fs::read(&path) {
                Ok(protected) => protected,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return Err(KeyStoreError::NotFound);
                }
                Err(error) => return Err(classify_io_error("reading the Windows key file", error)),
            };
            let protected_len = u32::try_from(protected.len()).map_err(|_| {
                KeyStoreError::Corrupt("the Windows key file is too large".to_string())
            })?;
            let blob_in = CRYPT_INTEGER_BLOB {
                cbData: protected_len,
                pbData: protected.as_ptr() as *mut u8,
            };
            unsafe {
                let mut blob_out = std::mem::zeroed::<CRYPT_INTEGER_BLOB>();
                if CryptUnprotectData(
                    &blob_in,
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    0,
                    &mut blob_out,
                ) == 0
                {
                    let source = std::io::Error::last_os_error();
                    return if source.kind() == std::io::ErrorKind::PermissionDenied {
                        Err(KeyStoreError::AccessDenied(format!(
                            "Windows DPAPI refused to decrypt the key: {source}"
                        )))
                    } else {
                        Err(KeyStoreError::Corrupt(format!(
                            "Windows DPAPI could not decrypt the key: {source}"
                        )))
                    };
                }
                let key = if blob_out.cbData == 0 {
                    validate_key_bytes(&[], "the Windows DPAPI payload")
                } else if blob_out.pbData.is_null() {
                    Err(KeyStoreError::Corrupt(
                        "Windows DPAPI returned an invalid output buffer".to_string(),
                    ))
                } else {
                    let bytes =
                        std::slice::from_raw_parts(blob_out.pbData, blob_out.cbData as usize)
                            .to_vec();
                    validate_key_bytes(&bytes, "the Windows DPAPI payload")
                };
                windows_sys::Win32::Foundation::LocalFree(
                    blob_out.pbData as *mut core::ffi::c_void,
                );
                key
            }
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            Err(KeyStoreError::Platform {
                operation: "reading the encryption key",
                message: "unsupported platform".to_string(),
            })
        }
    }

    fn create(&self, key: &DataKey) -> Result<CreateOutcome, KeyStoreError> {
        #[cfg(target_os = "macos")]
        {
            let hex_key = hex::encode(key);
            let outcome = create_macos_keychain_item(
                MACOS_KEYCHAIN_V2_SERVICE,
                MACOS_KEYCHAIN_ACCOUNT,
                hex_key.as_bytes(),
            )?;
            if outcome == CreateOutcome::Created {
                info!("New encryption key stored in the macOS Keychain");
            }
            Ok(outcome)
        }

        #[cfg(target_os = "windows")]
        {
            use std::io::Write;
            use windows_sys::Win32::Security::Cryptography::{
                CryptProtectData, CRYPT_INTEGER_BLOB,
            };
            let blob_in = CRYPT_INTEGER_BLOB {
                cbData: DEK_SIZE as u32,
                pbData: key.as_ptr() as *mut u8,
            };
            let protected = unsafe {
                let mut blob_out = std::mem::zeroed::<CRYPT_INTEGER_BLOB>();
                if CryptProtectData(
                    &blob_in,
                    windows_sys::core::w!("TailSync Encryption Key"),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    0,
                    &mut blob_out,
                ) == 0
                {
                    let source = std::io::Error::last_os_error();
                    return if source.kind() == std::io::ErrorKind::PermissionDenied {
                        Err(KeyStoreError::AccessDenied(format!(
                            "Windows DPAPI refused to encrypt the key: {source}"
                        )))
                    } else {
                        Err(KeyStoreError::Platform {
                            operation: "protecting the encryption key with Windows DPAPI",
                            message: source.to_string(),
                        })
                    };
                }
                if blob_out.cbData == 0 || blob_out.pbData.is_null() {
                    windows_sys::Win32::Foundation::LocalFree(
                        blob_out.pbData as *mut core::ffi::c_void,
                    );
                    return Err(KeyStoreError::Platform {
                        operation: "protecting the encryption key with Windows DPAPI",
                        message: "Windows DPAPI returned an invalid output buffer".to_string(),
                    });
                }
                let protected =
                    std::slice::from_raw_parts(blob_out.pbData, blob_out.cbData as usize).to_vec();
                windows_sys::Win32::Foundation::LocalFree(
                    blob_out.pbData as *mut core::ffi::c_void,
                );
                protected
            };

            let path = db::get_data_dir().join(".dek");
            let parent = path.parent().ok_or_else(|| KeyStoreError::Platform {
                operation: "creating the Windows key file",
                message: "the key file has no parent directory".to_string(),
            })?;
            std::fs::create_dir_all(parent).map_err(|source| {
                classify_io_error("creating the Windows data directory", source)
            })?;

            let (temporary, mut file) = (0..16)
                .find_map(|_| {
                    let candidate = parent.join(format!(
                        ".dek.tmp-{}-{:016x}",
                        std::process::id(),
                        rand::random::<u64>()
                    ));
                    match std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&candidate)
                    {
                        Ok(file) => Some(Ok((candidate, file))),
                        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => None,
                        Err(error) => Some(Err(classify_io_error(
                            "creating a temporary Windows key file",
                            error,
                        ))),
                    }
                })
                .transpose()?
                .ok_or_else(|| KeyStoreError::Platform {
                    operation: "creating a temporary Windows key file",
                    message: "could not allocate a unique temporary path".to_string(),
                })?;

            let write_result = (|| {
                file.write_all(&protected).map_err(|source| {
                    classify_io_error("writing the temporary Windows key", source)
                })?;
                file.flush().map_err(|source| {
                    classify_io_error("flushing the temporary Windows key", source)
                })?;
                file.sync_all().map_err(|source| {
                    classify_io_error("syncing the temporary Windows key to disk", source)
                })?;
                drop(file);

                match move_windows_key_file_create_only(&temporary, &path) {
                    Ok(()) => {
                        info!("New encryption key stored with Windows DPAPI");
                        Ok(CreateOutcome::Created)
                    }
                    Err(rename_error) => match path.try_exists() {
                        Ok(true) => Ok(CreateOutcome::AlreadyExists),
                        Ok(false) => Err(classify_io_error(
                            "installing the Windows key file",
                            rename_error,
                        )),
                        Err(source) => Err(classify_io_error(
                            "checking for a concurrently created Windows key",
                            source,
                        )),
                    },
                }
            })();

            if !matches!(write_result, Ok(CreateOutcome::Created)) {
                if let Err(error) = std::fs::remove_file(&temporary) {
                    if error.kind() != std::io::ErrorKind::NotFound {
                        warn!(
                            "Could not remove temporary encryption key {}: {error}",
                            temporary.display()
                        );
                    }
                }
            }
            write_result
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = key;
            Err(KeyStoreError::Platform {
                operation: "creating the encryption key",
                message: "unsupported platform".to_string(),
            })
        }
    }
}

#[cfg(all(not(test), target_os = "windows"))]
fn classify_io_error(operation: &'static str, source: std::io::Error) -> KeyStoreError {
    if source.kind() == std::io::ErrorKind::PermissionDenied {
        KeyStoreError::AccessDenied(format!("{operation}: {source}"))
    } else {
        KeyStoreError::Io { operation, source }
    }
}

#[cfg(all(not(test), target_os = "macos"))]
fn classify_macos_cli_io_error(operation: &'static str, source: std::io::Error) -> KeyStoreError {
    if source.kind() == std::io::ErrorKind::PermissionDenied {
        KeyStoreError::AccessDenied(format!("{operation}: {source}"))
    } else {
        KeyStoreError::Platform {
            operation,
            message: source.to_string(),
        }
    }
}

#[cfg(all(not(test), target_os = "macos"))]
fn command_failure_message(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let detail = stderr.trim();
    if detail.is_empty() {
        format!("security exited with status {}", output.status)
    } else {
        format!("security exited with status {}: {detail}", output.status)
    }
}

#[cfg(target_os = "windows")]
fn move_windows_key_file_create_only(
    source: &std::path::Path,
    target: &std::path::Path,
) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::MoveFileW;

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let target = target
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    if unsafe { MoveFileW(source.as_ptr(), target.as_ptr()) } == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        decode_hex_key, validate_key_bytes, CreateOutcome, DataKey, DekCache, KeyStore,
        KeyStoreError, Settings, DEK_SIZE,
    };
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    };

    #[derive(Default)]
    struct MemoryStore {
        key: Mutex<Option<DataKey>>,
        create_calls: AtomicUsize,
    }

    impl KeyStore for MemoryStore {
        fn read(&self) -> Result<DataKey, KeyStoreError> {
            self.key
                .lock()
                .unwrap()
                .as_ref()
                .copied()
                .ok_or(KeyStoreError::NotFound)
        }

        fn create(&self, key: &DataKey) -> Result<CreateOutcome, KeyStoreError> {
            self.create_calls.fetch_add(1, Ordering::SeqCst);
            let mut stored = self.key.lock().unwrap();
            if stored.is_some() {
                Ok(CreateOutcome::AlreadyExists)
            } else {
                *stored = Some(*key);
                Ok(CreateOutcome::Created)
            }
        }
    }

    #[derive(Default)]
    struct RetryStore {
        key: Mutex<Option<DataKey>>,
        read_calls: AtomicUsize,
        create_calls: AtomicUsize,
    }

    impl KeyStore for RetryStore {
        fn read(&self) -> Result<DataKey, KeyStoreError> {
            if self.read_calls.fetch_add(1, Ordering::SeqCst) == 0 {
                return Err(KeyStoreError::AccessDenied("test denial".to_string()));
            }
            self.key
                .lock()
                .unwrap()
                .as_ref()
                .copied()
                .ok_or(KeyStoreError::NotFound)
        }

        fn create(&self, key: &DataKey) -> Result<CreateOutcome, KeyStoreError> {
            self.create_calls.fetch_add(1, Ordering::SeqCst);
            *self.key.lock().unwrap() = Some(*key);
            Ok(CreateOutcome::Created)
        }
    }

    struct CreateRaceStore {
        read_calls: AtomicUsize,
        winner: DataKey,
    }

    enum ReadFailure {
        Corrupt,
        Io,
        Platform,
    }

    struct FailingStore {
        failure: ReadFailure,
        create_calls: AtomicUsize,
    }

    impl KeyStore for FailingStore {
        fn read(&self) -> Result<DataKey, KeyStoreError> {
            Err(match self.failure {
                ReadFailure::Corrupt => KeyStoreError::Corrupt("test corruption".to_string()),
                ReadFailure::Io => KeyStoreError::Io {
                    operation: "test read",
                    source: std::io::Error::other("test I/O failure"),
                },
                ReadFailure::Platform => KeyStoreError::Platform {
                    operation: "test read",
                    message: "test platform failure".to_string(),
                },
            })
        }

        fn create(&self, _key: &DataKey) -> Result<CreateOutcome, KeyStoreError> {
            self.create_calls.fetch_add(1, Ordering::SeqCst);
            Ok(CreateOutcome::Created)
        }
    }

    #[derive(Default)]
    struct CreateRetryStore {
        key: Mutex<Option<DataKey>>,
        create_calls: AtomicUsize,
    }

    impl KeyStore for CreateRetryStore {
        fn read(&self) -> Result<DataKey, KeyStoreError> {
            self.key
                .lock()
                .unwrap()
                .as_ref()
                .copied()
                .ok_or(KeyStoreError::NotFound)
        }

        fn create(&self, key: &DataKey) -> Result<CreateOutcome, KeyStoreError> {
            if self.create_calls.fetch_add(1, Ordering::SeqCst) == 0 {
                return Err(KeyStoreError::Platform {
                    operation: "test create",
                    message: "test write failure".to_string(),
                });
            }
            *self.key.lock().unwrap() = Some(*key);
            Ok(CreateOutcome::Created)
        }
    }

    impl KeyStore for CreateRaceStore {
        fn read(&self) -> Result<DataKey, KeyStoreError> {
            if self.read_calls.fetch_add(1, Ordering::SeqCst) == 0 {
                Err(KeyStoreError::NotFound)
            } else {
                Ok(self.winner)
            }
        }

        fn create(&self, _key: &DataKey) -> Result<CreateOutcome, KeyStoreError> {
            Ok(CreateOutcome::AlreadyExists)
        }
    }

    #[test]
    fn key_cache_initializes_only_once_across_threads() {
        let cache = Arc::new(DekCache::new());
        let store = Arc::new(MemoryStore::default());
        let generations = Arc::new(AtomicUsize::new(0));
        let handles = (0..32)
            .map(|_| {
                let cache = cache.clone();
                let store = store.clone();
                let generations = generations.clone();
                std::thread::spawn(move || {
                    cache
                        .get_or_try_init_with(store.as_ref(), || {
                            generations.fetch_add(1, Ordering::SeqCst);
                            Ok([0x11; DEK_SIZE])
                        })
                        .unwrap()
                })
            })
            .collect::<Vec<_>>();

        for handle in handles {
            assert_eq!(handle.join().unwrap(), [0x11; DEK_SIZE]);
        }
        assert_eq!(generations.load(Ordering::SeqCst), 1);
        assert_eq!(store.create_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn access_errors_do_not_create_or_poison_the_cache() {
        let cache = DekCache::new();
        let store = RetryStore::default();
        let generations = AtomicUsize::new(0);

        let first = cache.get_or_try_init_with(&store, || {
            generations.fetch_add(1, Ordering::SeqCst);
            Ok([0x22; DEK_SIZE])
        });
        assert!(matches!(first, Err(KeyStoreError::AccessDenied(_))));
        assert_eq!(generations.load(Ordering::SeqCst), 0);
        assert_eq!(store.create_calls.load(Ordering::SeqCst), 0);

        let second = cache
            .get_or_try_init_with(&store, || {
                generations.fetch_add(1, Ordering::SeqCst);
                Ok([0x22; DEK_SIZE])
            })
            .unwrap();
        assert_eq!(second, [0x22; DEK_SIZE]);
        assert_eq!(generations.load(Ordering::SeqCst), 1);
        assert_eq!(store.create_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn non_not_found_errors_never_generate_or_create() {
        for failure in [ReadFailure::Corrupt, ReadFailure::Io, ReadFailure::Platform] {
            let cache = DekCache::new();
            let store = FailingStore {
                failure,
                create_calls: AtomicUsize::new(0),
            };
            let generations = AtomicUsize::new(0);
            assert!(cache
                .get_or_try_init_with(&store, || {
                    generations.fetch_add(1, Ordering::SeqCst);
                    Ok([0x24; DEK_SIZE])
                })
                .is_err());
            assert_eq!(generations.load(Ordering::SeqCst), 0);
            assert_eq!(store.create_calls.load(Ordering::SeqCst), 0);
        }
    }

    #[test]
    fn failed_create_does_not_cache_the_generated_key() {
        let cache = DekCache::new();
        let store = CreateRetryStore::default();

        assert!(cache
            .get_or_try_init_with(&store, || Ok([0x66; DEK_SIZE]))
            .is_err());
        let key = cache
            .get_or_try_init_with(&store, || Ok([0x77; DEK_SIZE]))
            .unwrap();
        assert_eq!(key, [0x77; DEK_SIZE]);
        assert_eq!(store.create_calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn create_race_rereads_the_winning_key() {
        let cache = DekCache::new();
        let store = CreateRaceStore {
            read_calls: AtomicUsize::new(0),
            winner: [0x33; DEK_SIZE],
        };

        let key = cache
            .get_or_try_init_with(&store, || Ok([0x44; DEK_SIZE]))
            .unwrap();
        assert_eq!(key, [0x33; DEK_SIZE]);
        assert_eq!(store.read_calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn encryption_keys_must_be_exactly_32_bytes() {
        assert_eq!(
            validate_key_bytes(&[0x55; DEK_SIZE], "test").unwrap(),
            [0x55; DEK_SIZE]
        );
        assert!(matches!(
            validate_key_bytes(&[0x55; DEK_SIZE - 1], "test"),
            Err(KeyStoreError::Corrupt(_))
        ));
        assert!(matches!(
            validate_key_bytes(&[0x55; DEK_SIZE + 1], "test"),
            Err(KeyStoreError::Corrupt(_))
        ));
        assert!(matches!(
            decode_hex_key("not-hex", "test"),
            Err(KeyStoreError::Corrupt(_))
        ));
        assert!(matches!(
            decode_hex_key("", "test"),
            Err(KeyStoreError::Corrupt(_))
        ));
    }

    #[test]
    fn test_dek_is_limited_to_cargo_platform_test_executables() {
        assert!(super::is_cargo_test_harness_executable(
            std::path::Path::new("/workspace/target/debug/deps/tailsync_lib-b130076dc5f25812")
        ));
        for path in [
            "/workspace/target/debug/tailsync",
            "/workspace/target/debug/deps/tailsync_lib",
            "/workspace/target/debug/deps/tailsync_lib-not-a-hash",
            "/workspace/target/debug/deps/tailsync_core-b130076dc5f25812",
            "/workspace/target/release/deps/tailsync-b130076dc5f25812",
        ] {
            assert!(!super::is_cargo_test_harness_executable(
                std::path::Path::new(path)
            ));
        }
    }

    #[test]
    fn corrupt_settings_are_reported_and_preserved() {
        let path = std::env::temp_dir().join(format!(
            "tailsync-corrupt-settings-{}-{:016x}.json",
            std::process::id(),
            rand::random::<u64>()
        ));
        let original = b"{ not valid json";
        std::fs::write(&path, original).unwrap();

        assert!(Settings::load_from_path(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn missing_settings_use_defaults() {
        let path = std::env::temp_dir().join(format!(
            "tailsync-missing-settings-{}-{:016x}.json",
            std::process::id(),
            rand::random::<u64>()
        ));
        assert!(!path.exists());

        let settings = Settings::load_from_path(&path).unwrap();
        assert_eq!(settings.history_limit, Settings::default().history_limit);
        assert!(settings.trusted_peer_keys.is_empty());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_key_install_never_replaces_an_existing_key() {
        let directory = std::env::temp_dir().join(format!(
            "tailsync-key-install-{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir(&directory).unwrap();
        let temporary = directory.join("candidate.tmp");
        let target = directory.join(".dek");
        std::fs::write(&temporary, b"candidate").unwrap();
        std::fs::write(&target, b"winner").unwrap();

        assert!(super::move_windows_key_file_create_only(&temporary, &target).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"winner");
        assert_eq!(std::fs::read(&temporary).unwrap(), b"candidate");

        std::fs::remove_file(temporary).unwrap();
        std::fs::remove_file(target).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_keychain_create_and_read_round_trip_without_cli_or_tty() {
        use security_framework::os::macos::keychain::CreateOptions;

        struct TemporaryDirectory(std::path::PathBuf);

        impl Drop for TemporaryDirectory {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        let directory = std::env::temp_dir().join(format!(
            "tailsync-keychain-test-{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir(&directory).unwrap();
        let _temporary_directory = TemporaryDirectory(directory.clone());
        let path = directory.join("test.keychain-db");
        let mut create = CreateOptions::new();
        create.password("tailsync-test");
        let keychain = create.create(&path).unwrap();
        let service = format!("com.tailsync.test.{:016x}", rand::random::<u64>());
        let account = "encryption-key";
        let key = [0x5a; DEK_SIZE];
        let password = hex::encode(key);
        let replacement = hex::encode([0x33; DEK_SIZE]);

        assert_eq!(
            super::create_macos_keychain_item_in(
                Some(keychain.clone()),
                &service,
                account,
                password.as_bytes(),
            )
            .unwrap(),
            CreateOutcome::Created
        );
        assert_eq!(
            super::create_macos_keychain_item_in(
                Some(keychain.clone()),
                &service,
                account,
                replacement.as_bytes(),
            )
            .unwrap(),
            CreateOutcome::AlreadyExists
        );
        assert_eq!(
            super::read_macos_keychain_item_in(Some(keychain.clone()), &service, account).unwrap(),
            key
        );
        assert!(matches!(
            super::read_macos_keychain_item_in(
                Some(keychain),
                "com.tailsync.test.missing",
                account,
            ),
            Err(KeyStoreError::NotFound)
        ));
    }

    #[test]
    fn legacy_settings_default_to_an_empty_trust_store() {
        let settings: Settings = serde_json::from_str(
            r#"{
                "notifications_enabled": true,
                "progress_bar_enabled": true,
                "history_limit": 100,
                "enabled_peers": {},
                "theme": "system",
                "language": "en",
                "connection_mode": "lan"
            }"#,
        )
        .unwrap();
        assert!(settings.trusted_peer_keys.is_empty());
        assert!(settings.trusted_peer_addresses.is_empty());
        assert!(settings.paired_peer_endpoints.is_empty());
        assert_eq!(settings.color_theme, "tailsync");
    }

    #[test]
    fn paired_peer_address_is_persisted_and_removed_with_pairing() {
        let mut settings = Settings::default();
        settings
            .trusted_peer_keys
            .insert("windows".into(), "key".into());
        settings
            .trusted_peer_keys
            .insert("old-alias".into(), "key".into());

        assert!(settings
            .remember_peer_address_without_save("windows", "lan", "192.168.1.20")
            .unwrap());
        assert_eq!(
            settings
                .trusted_peer_addresses
                .get("windows")
                .and_then(|addresses| addresses.get("lan"))
                .map(String::as_str),
            Some("192.168.1.20")
        );
        assert!(!settings.trusted_peer_keys.contains_key("old-alias"));
        assert!(!settings
            .remember_peer_address_without_save("windows", "lan", "192.168.1.20")
            .unwrap());
        assert!(settings
            .remember_peer_address_without_save("windows", "lan", "not-an-ip")
            .is_err());
    }

    #[test]
    fn paired_peer_iroh_endpoint_is_validated_and_remembered() {
        const ENDPOINT_ID: &str =
            "5866666666666666666666666666666666666666666666666666666666666666";
        let mut settings = Settings::default();
        settings
            .trusted_peer_keys
            .insert("windows".into(), "key".into());

        assert!(settings
            .remember_peer_address_without_save("windows", "iroh", ENDPOINT_ID)
            .unwrap());
        assert_eq!(
            settings
                .trusted_peer_addresses
                .get("windows")
                .and_then(|addresses| addresses.get("iroh"))
                .map(String::as_str),
            Some(ENDPOINT_ID)
        );
        assert!(settings
            .remember_peer_address_without_save("windows", "iroh", "not-an-endpoint")
            .is_err());
    }

    #[test]
    fn pairing_endpoint_is_not_changed_by_later_route_discovery() {
        let mut settings = Settings::default();
        settings
            .trust_peer_without_save("windows", "key", "lan", Some("192.168.1.20"))
            .unwrap();

        settings
            .remember_peer_address_without_save("windows", "tailscale", "100.64.0.2")
            .unwrap();

        assert_eq!(
            settings
                .paired_peer_endpoints
                .get("windows")
                .map(String::as_str),
            Some("192.168.1.20")
        );
    }

    #[test]
    fn legacy_manual_connection_mode_maps_to_lan() {
        assert_eq!(
            super::normalize_connection_mode("manual".into()),
            "lan_only"
        );
        assert_eq!(super::normalize_connection_mode("auto".into()), "auto");
        assert_eq!(super::normalize_connection_mode("lan".into()), "lan_only");
        assert_eq!(
            super::normalize_connection_mode("tailscale".into()),
            "tailscale_only"
        );
        assert_eq!(super::normalize_connection_mode("invalid".into()), "auto");
    }

    #[test]
    fn legacy_settings_without_connection_mode_default_to_auto() {
        let settings: Settings = serde_json::from_str(
            r#"{
                "notifications_enabled": true,
                "progress_bar_enabled": true,
                "history_limit": 100,
                "enabled_peers": {},
                "theme": "system",
                "language": "en"
            }"#,
        )
        .unwrap();

        assert_eq!(settings.connection_mode, "auto");
        assert_eq!(settings.color_theme, "tailsync");
    }

    #[test]
    fn appearance_values_are_validated() {
        let mut settings = Settings {
            theme: "dark".into(),
            color_theme: "forest".into(),
            ..Settings::default()
        };
        assert!(settings.validate_user_values().is_ok());

        settings.color_theme = "unknown".into();
        assert!(settings.validate_user_values().is_err());
        settings.color_theme = "tailsync".into();
        settings.theme = "sepia".into();
        assert!(settings.validate_user_values().is_err());
    }

    #[test]
    fn settings_contract_bounds_are_validated() {
        let mut settings = Settings {
            history_limit: 9,
            ..Settings::default()
        };
        assert!(settings.validate_user_values().is_err());
        settings.history_limit = 501;
        assert!(settings.validate_user_values().is_err());
        settings.history_limit = 100;
        settings.language = "fr".into();
        assert!(settings.validate_user_values().is_err());
        settings.language = "zh-CN".into();
        assert!(settings.validate_user_values().is_ok());
    }

    #[test]
    fn user_update_preserves_server_owned_settings() {
        let mut current = Settings::default();
        current.enabled_peers.insert("desktop".into(), true);
        current.storage_root = Some("/managed/storage".into());
        current
            .trusted_peer_keys
            .insert("desktop".into(), "public-key".into());
        current.trusted_peer_addresses.insert(
            "desktop".into(),
            std::collections::HashMap::from([("lan".into(), "192.0.2.8".into())]),
        );
        current
            .paired_peer_endpoints
            .insert("desktop".into(), "192.0.2.8".into());

        let mut requested = Settings {
            history_limit: 250,
            theme: "dark".into(),
            ..Settings::default()
        };
        requested.enabled_peers.insert("stale".into(), false);
        requested.storage_root = Some(String::new());
        requested
            .trusted_peer_keys
            .insert("stale".into(), "wrong-key".into());

        let updated = current.prepare_user_update(requested).unwrap();

        assert_eq!(updated.history_limit, 250);
        assert_eq!(updated.theme, "dark");
        assert_eq!(updated.enabled_peers, current.enabled_peers);
        assert_eq!(updated.storage_root, current.storage_root);
        assert_eq!(updated.trusted_peer_keys, current.trusted_peer_keys);
        assert_eq!(
            updated.trusted_peer_addresses,
            current.trusted_peer_addresses
        );
        assert_eq!(updated.paired_peer_endpoints, current.paired_peer_endpoints);
    }
}
