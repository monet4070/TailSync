use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use thiserror::Error;

use crate::{crypto, db};

pub const NOISE_PROTOCOL: &str = "Noise_XX_25519_ChaChaPoly_BLAKE2s";
const KEY_SIZE: usize = 32;

#[derive(Debug, Error)]
pub enum IdentityError {
    #[error("device identity does not exist")]
    NotFound,
    #[error("device identity access was denied while {operation}: {source}")]
    AccessDenied {
        operation: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("device identity I/O failed while {operation}: {source}")]
    Io {
        operation: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("device identity is corrupt: {0}")]
    Corrupt(String),
    #[error("device identity cryptography failed: {0}")]
    Crypto(String),
    #[error("device identity generation failed: {0}")]
    Generation(String),
}

impl IdentityError {
    fn io(operation: &'static str, source: std::io::Error) -> Self {
        if source.kind() == std::io::ErrorKind::PermissionDenied {
            Self::AccessDenied { operation, source }
        } else {
            Self::Io { operation, source }
        }
    }
}

#[derive(Clone)]
pub struct DeviceIdentity {
    private_key: Vec<u8>,
    public_key: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct StoredIdentity {
    version: u8,
    private_key: String,
    public_key: String,
}

pub(crate) enum CreateOutcome {
    Created,
    AlreadyExists,
}

impl DeviceIdentity {
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn generate_for_test() -> Self {
        Self::generate().expect("generate test identity")
    }

    pub fn load_or_create() -> Result<Self, IdentityError> {
        Self::load_or_create_at(&identity_path())
    }

    fn load_or_create_at(path: &Path) -> Result<Self, IdentityError> {
        match Self::load(path) {
            Ok(identity) => return Ok(identity),
            Err(IdentityError::NotFound) => {}
            Err(error) => return Err(error),
        }

        let identity = Self::generate()?;
        match identity.save_create_only(path)? {
            CreateOutcome::Created => Ok(identity),
            CreateOutcome::AlreadyExists => Self::load(path).map_err(|error| match error {
                IdentityError::NotFound => IdentityError::Io {
                    operation: "re-reading a concurrently created identity",
                    source: std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "the identity disappeared after the create race",
                    ),
                },
                other => other,
            }),
        }
    }

    fn generate() -> Result<Self, IdentityError> {
        let params = NOISE_PROTOCOL.parse().map_err(|error| {
            IdentityError::Generation(format!("invalid Noise parameters: {error}"))
        })?;
        let keypair = snow::Builder::new(params)
            .generate_keypair()
            .map_err(|error| IdentityError::Generation(error.to_string()))?;
        let identity = Self {
            private_key: keypair.private,
            public_key: keypair.public,
        };
        identity.validate()?;
        Ok(identity)
    }

    pub fn private_key(&self) -> &[u8] {
        &self.private_key
    }

    pub fn public_key(&self) -> &[u8] {
        &self.public_key
    }

    pub fn public_key_base64(&self) -> String {
        STANDARD.encode(&self.public_key)
    }

    pub fn fingerprint(&self) -> String {
        fingerprint(&self.public_key)
    }

    fn load(path: &Path) -> Result<Self, IdentityError> {
        let plaintext = read_protected_bytes(path)?;
        let stored: StoredIdentity = serde_json::from_slice(&plaintext)
            .map_err(|error| IdentityError::Corrupt(format!("invalid JSON: {error}")))?;
        if stored.version != 1 {
            return Err(IdentityError::Corrupt(format!(
                "unsupported identity version {}",
                stored.version
            )));
        }
        let identity = Self {
            private_key: STANDARD.decode(stored.private_key).map_err(|error| {
                IdentityError::Corrupt(format!("invalid private key encoding: {error}"))
            })?,
            public_key: STANDARD.decode(stored.public_key).map_err(|error| {
                IdentityError::Corrupt(format!("invalid public key encoding: {error}"))
            })?,
        };
        identity.validate()?;
        Ok(identity)
    }

    fn save_create_only(&self, path: &Path) -> Result<CreateOutcome, IdentityError> {
        let stored = StoredIdentity {
            version: 1,
            private_key: STANDARD.encode(&self.private_key),
            public_key: STANDARD.encode(&self.public_key),
        };
        let plaintext = serde_json::to_vec(&stored)
            .map_err(|error| IdentityError::Corrupt(format!("serialization failed: {error}")))?;
        persist_protected_bytes_create_only(path, &plaintext)
    }

    fn validate(&self) -> Result<(), IdentityError> {
        if self.private_key.len() != KEY_SIZE || self.public_key.len() != KEY_SIZE {
            return Err(IdentityError::Corrupt(format!(
                "X25519 keys must each contain {KEY_SIZE} bytes"
            )));
        }
        Ok(())
    }
}

pub(crate) fn read_protected_bytes(path: &Path) -> Result<Vec<u8>, IdentityError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            IdentityError::NotFound
        } else {
            IdentityError::io("inspecting the identity file", error)
        }
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(IdentityError::Corrupt(
            "the identity path is not a regular file".to_string(),
        ));
    }
    restrict_private_file(path)?;

    let encrypted = std::fs::read(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            IdentityError::NotFound
        } else {
            IdentityError::io("reading the identity file", error)
        }
    })?;
    if encrypted.is_empty() {
        return Err(IdentityError::Corrupt(
            "the encrypted identity file is empty".to_string(),
        ));
    }
    crypto::decrypt(&encrypted).map_err(|error| {
        if crypto::is_key_store_error(error.as_ref()) {
            IdentityError::Crypto(error.to_string())
        } else {
            IdentityError::Corrupt(format!("decryption failed: {error}"))
        }
    })
}

pub(crate) fn persist_protected_bytes_create_only(
    path: &Path,
    plaintext: &[u8],
) -> Result<CreateOutcome, IdentityError> {
    let encrypted =
        crypto::encrypt(plaintext).map_err(|error| IdentityError::Crypto(error.to_string()))?;
    persist_encrypted_create_only(path, &encrypted)
}

fn persist_encrypted_create_only(
    path: &Path,
    encrypted: &[u8],
) -> Result<CreateOutcome, IdentityError> {
    let parent = path.parent().ok_or_else(|| {
        IdentityError::Corrupt("the identity path has no parent directory".to_string())
    })?;
    std::fs::create_dir_all(parent)
        .map_err(|error| IdentityError::io("creating the identity directory", error))?;
    restrict_private_directory(parent)?;

    let (temporary, mut file) = create_private_temporary_file(parent)?;
    let result = (|| {
        file.write_all(encrypted)
            .map_err(|error| IdentityError::io("writing the temporary identity", error))?;
        file.flush()
            .map_err(|error| IdentityError::io("flushing the temporary identity", error))?;
        file.sync_all()
            .map_err(|error| IdentityError::io("syncing the temporary identity", error))?;
        drop(file);

        match std::fs::hard_link(&temporary, path) {
            Ok(()) => {
                restrict_private_file(path)?;
                Ok(CreateOutcome::Created)
            }
            Err(error) => match path.try_exists() {
                Ok(true) => Ok(CreateOutcome::AlreadyExists),
                Ok(false) => Err(IdentityError::io("installing the identity file", error)),
                Err(check_error) => Err(IdentityError::io(
                    "checking for a concurrently created identity",
                    check_error,
                )),
            },
        }
    })();

    if let Err(error) = std::fs::remove_file(&temporary) {
        if error.kind() != std::io::ErrorKind::NotFound {
            log::warn!("Could not remove a temporary device identity: {error}");
        }
    }
    if matches!(result, Ok(CreateOutcome::Created)) {
        sync_parent_directory(parent)?;
    }
    result
}

fn create_private_temporary_file(parent: &Path) -> Result<(PathBuf, std::fs::File), IdentityError> {
    for _ in 0..16 {
        let candidate = parent.join(format!(
            ".identity-v1.tmp-{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&candidate) {
            Ok(file) => {
                if let Err(error) = restrict_private_file(&candidate) {
                    drop(file);
                    let _ = std::fs::remove_file(&candidate);
                    return Err(error);
                }
                return Ok((candidate, file));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(IdentityError::io(
                    "creating a temporary identity file",
                    error,
                ));
            }
        }
    }
    Err(IdentityError::Io {
        operation: "creating a temporary identity file",
        source: std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "could not allocate a unique temporary path",
        ),
    })
}

#[cfg(unix)]
pub(crate) fn restrict_private_directory(path: &Path) -> Result<(), IdentityError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| IdentityError::io("restricting the identity directory", error))
}

#[cfg(windows)]
pub(crate) fn restrict_private_directory(path: &Path) -> Result<(), IdentityError> {
    set_private_windows_acl(path, true)
        .map_err(|error| IdentityError::io("restricting the identity directory", error))
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn restrict_private_directory(_path: &Path) -> Result<(), IdentityError> {
    Ok(())
}

#[cfg(unix)]
fn restrict_private_file(path: &Path) -> Result<(), IdentityError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|error| IdentityError::io("restricting the identity file", error))
}

#[cfg(windows)]
fn restrict_private_file(path: &Path) -> Result<(), IdentityError> {
    set_private_windows_acl(path, false)
        .map_err(|error| IdentityError::io("restricting the identity file", error))
}

#[cfg(not(any(unix, windows)))]
fn restrict_private_file(_path: &Path) -> Result<(), IdentityError> {
    Ok(())
}

#[cfg(windows)]
fn set_private_windows_acl(path: &Path, directory: bool) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{
        SetFileSecurityW, DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
    };

    // OW is the Windows Owner Rights SID. The protected DACL permits only the
    // object owner, SYSTEM and administrators; directory ACEs inherit.
    let sddl = if directory {
        "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;OW)"
    } else {
        "D:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FA;;;OW)"
    };
    let sddl = sddl
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let path = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut descriptor = std::ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    let result = unsafe {
        SetFileSecurityW(
            path.as_ptr(),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor,
        )
    };
    unsafe {
        LocalFree(descriptor);
    }
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(unix)]
fn sync_parent_directory(path: &Path) -> Result<(), IdentityError> {
    std::fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| IdentityError::io("syncing the identity directory", error))
}

#[cfg(not(unix))]
fn sync_parent_directory(_path: &Path) -> Result<(), IdentityError> {
    Ok(())
}

/// Device public-key decoding failures (T353 migration). Display strings
/// reach the UI and wire surfaces verbatim.
#[derive(Debug, Error)]
pub enum IdentityKeyError {
    #[error("Device public key is not valid Base64")]
    InvalidBase64,
    #[error("Device public key must decode to 32 bytes")]
    WrongLength,
}

pub fn decode_public_key(encoded: &str) -> Result<Vec<u8>, IdentityKeyError> {
    let key = STANDARD
        .decode(encoded.trim())
        .map_err(|_| IdentityKeyError::InvalidBase64)?;
    if key.len() != KEY_SIZE {
        return Err(IdentityKeyError::WrongLength);
    }
    Ok(key)
}

pub fn canonical_public_key(encoded: &str) -> Result<String, IdentityKeyError> {
    decode_public_key(encoded).map(|key| STANDARD.encode(key))
}

pub fn fingerprint(public_key: &[u8]) -> String {
    let hash = blake3::hash(public_key);
    let short = hex::encode(&hash.as_bytes()[..10]).to_uppercase();
    short
        .as_bytes()
        .chunks(4)
        .map(|chunk| {
            chunk
                .iter()
                .map(|byte| char::from(*byte))
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("-")
}

/// Reasons a peer-trust operation can fail once the surface-specific
/// message formatting has been peeled away.
#[derive(Debug)]
pub enum TrustPeerFailure {
    InvalidHostname,
    SelfPairing,
    Key(String),
    Interface(String),
    Trust(String),
}

/// Pin a peer's Noise static public key after out-of-band verification
/// (T302 extraction from the Tauri command and API route surfaces).
///
/// Validates the hostname and key, resolves the connection-mode interface
/// for the peer address, records the trusted identity in settings, and
/// persists through the injected hook. Persistence follows the same
/// clone-then-commit semantics as `Settings::trust_peer`: on any failure
/// the in-memory settings are restored to their previous state.
pub async fn trust_peer(
    identity: &DeviceIdentity,
    settings: &tokio::sync::Mutex<crypto::Settings>,
    persist_settings: &(dyn Fn(&crypto::Settings) -> Result<(), String> + Send + Sync),
    hostname: &str,
    public_key: &str,
    address: Option<&str>,
) -> Result<String, TrustPeerFailure> {
    let hostname = hostname.trim();
    if hostname.is_empty() || hostname.len() > 255 {
        return Err(TrustPeerFailure::InvalidHostname);
    }
    let public_key = canonical_public_key(public_key)
        .map_err(|error| TrustPeerFailure::Key(error.to_string()))?;
    if public_key == identity.public_key_base64() {
        return Err(TrustPeerFailure::SelfPairing);
    }
    let decoded =
        decode_public_key(&public_key).map_err(|error| TrustPeerFailure::Key(error.to_string()))?;
    let fingerprint = fingerprint(&decoded);
    let mut settings_guard = settings.lock().await;
    let address = address.filter(|value| !value.trim().is_empty());
    let mode = match (settings_guard.connection_mode.as_str(), address) {
        ("auto", Some(address)) => crate::peer::directory::infer_interface(address)
            .map_err(TrustPeerFailure::Interface)?
            .as_str()
            .to_string(),
        (mode, _) => crate::peer::directory::mode_interface(mode)
            .map(|interface| interface.as_str().to_string())
            .unwrap_or_else(|| "lan".to_string()),
    };
    let snapshot = settings_guard.clone();
    if let Err(error) =
        settings_guard.trust_peer_without_save(hostname, &public_key, &mode, address)
    {
        *settings_guard = snapshot;
        return Err(TrustPeerFailure::Trust(error.to_string()));
    }
    if let Err(error) = persist_settings(&settings_guard) {
        *settings_guard = snapshot;
        return Err(TrustPeerFailure::Trust(error));
    }
    Ok(fingerprint)
}

fn identity_path() -> PathBuf {
    db::get_data_dir().join("identity-v1.bin")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_directory(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "tailsync-identity-{label}-{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        ))
    }

    #[test]
    fn public_key_validation_is_canonical_and_fingerprinted() {
        let key = [0x5a; KEY_SIZE];
        let encoded = STANDARD.encode(key);
        assert_eq!(canonical_public_key(&encoded).unwrap(), encoded);
        assert_eq!(decode_public_key(&encoded).unwrap(), key);
        assert_eq!(fingerprint(&key).split('-').count(), 5);
        assert!(decode_public_key("not-a-key").is_err());
    }

    #[test]
    fn missing_identity_is_created_once_and_reloaded() {
        let directory = test_directory("create");
        let path = directory.join("identity-v1.bin");
        let first = DeviceIdentity::load_or_create_at(&path).unwrap();
        let second = DeviceIdentity::load_or_create_at(&path).unwrap();
        assert_eq!(first.private_key(), second.private_key());
        assert_eq!(first.public_key(), second.public_key());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn corrupt_identity_is_reported_without_replacement() {
        let directory = test_directory("corrupt");
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("identity-v1.bin");
        let original = b"not an encrypted identity";
        std::fs::write(&path, original).unwrap();

        assert!(matches!(
            DeviceIdentity::load_or_create_at(&path),
            Err(IdentityError::Corrupt(_))
        ));
        assert_eq!(std::fs::read(&path).unwrap(), original);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn invalid_identity_version_and_key_lengths_are_preserved() {
        for stored in [
            StoredIdentity {
                version: 2,
                private_key: STANDARD.encode([0x11; KEY_SIZE]),
                public_key: STANDARD.encode([0x22; KEY_SIZE]),
            },
            StoredIdentity {
                version: 1,
                private_key: STANDARD.encode([0x11; KEY_SIZE - 1]),
                public_key: STANDARD.encode([0x22; KEY_SIZE]),
            },
        ] {
            let directory = test_directory("invalid");
            std::fs::create_dir(&directory).unwrap();
            let path = directory.join("identity-v1.bin");
            let original = crypto::encrypt(&serde_json::to_vec(&stored).unwrap()).unwrap();
            std::fs::write(&path, &original).unwrap();

            assert!(matches!(
                DeviceIdentity::load_or_create_at(&path),
                Err(IdentityError::Corrupt(_))
            ));
            assert_eq!(std::fs::read(&path).unwrap(), original);
            std::fs::remove_dir_all(directory).unwrap();
        }
    }

    #[test]
    fn create_only_persistence_never_replaces_an_existing_identity() {
        let directory = test_directory("preserve");
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("identity-v1.bin");
        let original = b"existing identity";
        std::fs::write(&path, original).unwrap();

        assert!(matches!(
            persist_encrypted_create_only(&path, b"candidate"),
            Ok(CreateOutcome::AlreadyExists)
        ));
        assert_eq!(std::fs::read(&path).unwrap(), original);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn concurrent_initialization_converges_on_one_identity() {
        let directory = test_directory("concurrent");
        let path = directory.join("identity-v1.bin");
        let handles = (0..16)
            .map(|_| {
                let path = path.clone();
                std::thread::spawn(move || DeviceIdentity::load_or_create_at(&path).unwrap())
            })
            .collect::<Vec<_>>();
        let identities = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();
        for identity in &identities[1..] {
            assert_eq!(identity.private_key(), identities[0].private_key());
            assert_eq!(identity.public_key(), identities[0].public_key());
        }
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn identity_directory_and_file_are_private() {
        use std::os::unix::fs::PermissionsExt;

        let directory = test_directory("permissions");
        let path = directory.join("identity-v1.bin");
        DeviceIdentity::load_or_create_at(&path).unwrap();
        assert_eq!(
            std::fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    fn trust_peer_settings() -> std::sync::Arc<tokio::sync::Mutex<crypto::Settings>> {
        std::sync::Arc::new(tokio::sync::Mutex::new(crypto::Settings::default()))
    }

    #[tokio::test]
    async fn trust_peer_rejects_an_empty_hostname() {
        let identity = DeviceIdentity::generate_for_test();
        let settings = trust_peer_settings();
        let persist = |_settings: &crypto::Settings| -> Result<(), String> { Ok(()) };
        let error = trust_peer(
            &identity,
            &settings,
            &persist,
            "   ",
            &identity.public_key_base64(),
            None,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, TrustPeerFailure::InvalidHostname));
    }

    #[tokio::test]
    async fn trust_peer_rejects_pinning_this_device_itself() {
        let identity = DeviceIdentity::generate_for_test();
        let settings = trust_peer_settings();
        let persist = |_settings: &crypto::Settings| -> Result<(), String> { Ok(()) };
        let error = trust_peer(
            &identity,
            &settings,
            &persist,
            "laptop",
            &identity.public_key_base64(),
            None,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, TrustPeerFailure::SelfPairing));
    }

    #[tokio::test]
    async fn trust_peer_pins_the_key_and_persists_via_the_hook() {
        let identity = DeviceIdentity::generate_for_test();
        let peer = DeviceIdentity::generate_for_test();
        let settings = trust_peer_settings();
        let persisted = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let persist_counter = persisted.clone();
        let persist = move |_settings: &crypto::Settings| -> Result<(), String> {
            persist_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        };

        let pinned_fingerprint = trust_peer(
            &identity,
            &settings,
            &persist,
            "laptop",
            &peer.public_key_base64(),
            Some("192.168.1.5"),
        )
        .await
        .unwrap();
        assert_eq!(pinned_fingerprint, fingerprint(peer.public_key()));
        let settings_guard = settings.lock().await;
        assert_eq!(
            settings_guard
                .trusted_peer_keys
                .get("laptop")
                .map(String::as_str),
            Some(peer.public_key_base64().as_str())
        );
        assert_eq!(
            settings_guard
                .paired_peer_endpoints
                .get("laptop")
                .map(String::as_str),
            Some("192.168.1.5")
        );
        assert_eq!(settings_guard.enabled_peers.get("laptop"), Some(&true));
        drop(settings_guard);
        assert_eq!(persisted.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn trust_peer_rolls_back_the_settings_when_persistence_fails() {
        let identity = DeviceIdentity::generate_for_test();
        let peer = DeviceIdentity::generate_for_test();
        let settings = trust_peer_settings();
        let fail_persist = |_settings: &crypto::Settings| -> Result<(), String> {
            Err("simulated save failure".to_string())
        };

        let error = trust_peer(
            &identity,
            &settings,
            &fail_persist,
            "laptop",
            &peer.public_key_base64(),
            None,
        )
        .await
        .unwrap_err();
        assert!(matches!(
            error,
            TrustPeerFailure::Trust(ref message) if message == "simulated save failure"
        ));
        let settings_guard = settings.lock().await;
        assert!(!settings_guard.trusted_peer_keys.contains_key("laptop"));
        assert!(!settings_guard.enabled_peers.contains_key("laptop"));
    }

    #[tokio::test]
    async fn trust_peer_filters_blank_addresses_and_falls_back_to_lan_in_auto_mode() {
        let identity = DeviceIdentity::generate_for_test();
        let peer = DeviceIdentity::generate_for_test();
        let settings = trust_peer_settings();
        assert_eq!(settings.lock().await.connection_mode, "auto");
        let persist = |_settings: &crypto::Settings| -> Result<(), String> { Ok(()) };

        trust_peer(
            &identity,
            &settings,
            &persist,
            "laptop",
            &peer.public_key_base64(),
            Some("   "),
        )
        .await
        .unwrap();
        let settings_guard = settings.lock().await;
        assert!(settings_guard.trusted_peer_keys.contains_key("laptop"));
        assert!(!settings_guard.paired_peer_endpoints.contains_key("laptop"));
        assert!(
            !settings_guard.trusted_peer_addresses.contains_key("laptop"),
            "a blank address must not be remembered"
        );
    }

    #[tokio::test]
    async fn trust_peer_infers_tailscale_mode_for_cgnat_addresses() {
        let identity = DeviceIdentity::generate_for_test();
        let peer = DeviceIdentity::generate_for_test();
        let settings = trust_peer_settings();
        let persist = |_settings: &crypto::Settings| -> Result<(), String> { Ok(()) };

        trust_peer(
            &identity,
            &settings,
            &persist,
            "laptop",
            &peer.public_key_base64(),
            Some("100.64.0.9"),
        )
        .await
        .unwrap();
        let settings_guard = settings.lock().await;
        assert_eq!(
            settings_guard
                .trusted_peer_addresses
                .get("laptop")
                .and_then(|modes| modes.get("tailscale")),
            Some(&"100.64.0.9".to_string())
        );
    }
}
