#[cfg(not(test))]
use log::info;
use log::warn;
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
use ring::rand::{SecureRandom, SystemRandom};
use std::path::PathBuf;
#[cfg(not(test))]
use std::sync::OnceLock;

use crate::db;

/// Encrypted settings stored alongside the app
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Settings {
    pub notifications_enabled: bool,
    pub progress_bar_enabled: bool,
    pub history_limit: u32,
    pub enabled_peers: std::collections::HashMap<String, bool>,
    pub theme: String,    // "light" | "dark" | "system"
    pub language: String, // "en" | "zh-CN"
    /// Transport policy used for peer discovery and delivery.
    #[serde(default = "default_connection_mode")]
    pub connection_mode: String,
    /// Explicit hostname -> pinned Noise static public key mapping.
    #[serde(default)]
    pub trusted_peer_keys: std::collections::HashMap<String, String>,
    /// Last known hostname -> IP mapping, used to reconnect paired devices
    /// when LAN broadcast discovery is unavailable during startup or sending.
    #[serde(default)]
    pub trusted_peer_addresses:
        std::collections::HashMap<String, std::collections::HashMap<String, String>>,
}

fn default_connection_mode() -> String {
    "auto".to_string()
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
            enabled_peers: std::collections::HashMap::new(),
            theme: "system".to_string(),
            language: "en".to_string(),
            connection_mode: default_connection_mode(),
            trusted_peer_keys: std::collections::HashMap::new(),
            trusted_peer_addresses: std::collections::HashMap::new(),
        }
    }
}

impl Settings {
    pub fn load() -> Result<Self, Box<dyn std::error::Error>> {
        let path = settings_path();
        if !path.exists() {
            return Ok(Settings::default());
        }
        let data = std::fs::read_to_string(&path)?;
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
        if !matches!(
            self.connection_mode.as_str(),
            "auto" | "lan_only" | "tailscale_only"
        ) {
            return Err(
                "connection_mode must be 'auto', 'lan_only', or 'tailscale_only'".to_string(),
            );
        }
        Ok(())
    }

    pub fn set_theme(&mut self, theme: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.theme = theme.to_string();
        self.save()
    }

    pub fn toggle_peer(
        &mut self,
        hostname: &str,
        enabled: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.enabled_peers.insert(hostname.to_string(), enabled);
        self.save()
    }

    pub fn trust_peer(
        &mut self,
        hostname: &str,
        public_key: &str,
        mode: &str,
        address: Option<&str>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.trust_peer_without_save(hostname, public_key, mode, address)?;
        self.save()
    }

    pub(crate) fn trust_peer_without_save(
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
        let changed = self.remember_peer_address_without_save(hostname, mode, address)?;
        if changed {
            self.save()?;
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
        address.parse::<std::net::IpAddr>()?;
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
            self.enabled_peers.remove(&duplicate);
        }
        if !matches!(mode, "lan" | "tailscale") {
            return Err(format!("Unsupported connection mode: {mode}").into());
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
        self.trusted_peer_keys.remove(hostname);
        self.trusted_peer_addresses.remove(hostname);
        self.enabled_peers.remove(hostname);
        self.save()
    }
}

fn settings_path() -> PathBuf {
    db::get_data_dir().join("config-v2.json")
}

// ─── OS Keychain Integration ──────────────────────────────────────

#[cfg(not(test))]
static DEK: OnceLock<Vec<u8>> = OnceLock::new();

/// Get or initialize the data encryption key from the OS keychain
pub(crate) fn get_dek() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    #[cfg(test)]
    return Ok(vec![0x54; 32]);

    #[cfg(not(test))]
    {
        if let Some(k) = DEK.get() {
            return Ok(k.clone());
        }

        // Try to read existing key from keychain
        let key = match read_keychain() {
            Ok(k) => k,
            Err(_) => {
                // Generate new key
                warn!("No existing encryption key found, generating new one");
                let rng = SystemRandom::new();
                let mut key = vec![0u8; 32]; // AES-256
                rng.fill(&mut key)
                    .map_err(|_| "Failed to generate encryption key")?;
                write_keychain(&key)?;
                info!("New encryption key stored in OS keychain");
                key
            }
        };

        // Safety: DEK is initialized exactly once. If another thread
        // initialized it between our get() check and set(), we lose the race
        // but the stored value is used consistently.
        let _ = DEK.set(key.clone());
        Ok(key)
    }
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

/// Read encryption key from OS keychain (or fallback file)
#[cfg(not(test))]
fn read_keychain() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    #[cfg(target_os = "macos")]
    {
        // Try macOS Keychain via the `security` CLI (portable, always works)
        let output = std::process::Command::new("security")
            .args([
                "find-generic-password",
                "-s",
                "com.tailsync.app",
                "-a",
                "encryption-key",
                "-w",
            ])
            .output();

        match output {
            Ok(o) if o.status.success() => {
                let pass = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if !pass.is_empty() {
                    return Ok(hex::decode(&pass).unwrap_or_default());
                }
            }
            _ => {}
        }
        Err("No keychain entry found".into())
    }

    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB};
        let path = db::get_data_dir().join(".dek");
        if !path.exists() {
            return Err("No key file".into());
        }
        let protected = std::fs::read(&path)?;
        let blob_in = CRYPT_INTEGER_BLOB {
            cbData: protected.len() as u32,
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
                return Err("DPAPI decrypt failed".into());
            }
            let key =
                std::slice::from_raw_parts(blob_out.pbData, blob_out.cbData as usize).to_vec();
            windows_sys::Win32::Foundation::LocalFree(blob_out.pbData as *mut core::ffi::c_void);
            Ok(key)
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Err("Unsupported platform".into())
    }
}

/// Write encryption key to OS keychain (or fallback file)
#[cfg(not(test))]
fn write_keychain(key: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "macos")]
    {
        let hex_key = hex::encode(key);
        let status = std::process::Command::new("security")
            .args([
                "add-generic-password",
                "-s",
                "com.tailsync.app",
                "-a",
                "encryption-key",
                "-w",
                &hex_key,
                "-U", // Update if exists
            ])
            .status()?;
        if !status.success() {
            return Err("Failed to store key in Keychain".into());
        }
        Ok(())
    }

    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Security::Cryptography::{CryptProtectData, CRYPT_INTEGER_BLOB};
        let blob_in = CRYPT_INTEGER_BLOB {
            cbData: key.len() as u32,
            pbData: key.as_ptr() as *mut u8,
        };
        unsafe {
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
                return Err("DPAPI encrypt failed".into());
            }
            let protected =
                std::slice::from_raw_parts(blob_out.pbData, blob_out.cbData as usize).to_vec();
            windows_sys::Win32::Foundation::LocalFree(blob_out.pbData as *mut core::ffi::c_void);
            let path = db::get_data_dir().join(".dek");
            std::fs::write(&path, protected)?;
        }
        Ok(())
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Err("Unsupported platform".into())
    }
}

#[cfg(test)]
mod tests {
    use super::Settings;

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
    }
}
