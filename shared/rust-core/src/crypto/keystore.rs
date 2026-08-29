#[cfg(not(test))]
use log::info;
#[cfg(target_os = "windows")]
use log::warn;
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
use ring::rand::{SecureRandom, SystemRandom};
use std::sync::{Mutex as StdMutex, OnceLock};
use thiserror::Error;

#[cfg(target_os = "windows")]
use crate::db;

// ─── OS Keychain Integration ──────────────────────────────────────

pub(super) const DEK_SIZE: usize = 32;
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
pub(super) enum CreateOutcome {
    Created,
    AlreadyExists,
}

pub(super) trait KeyStore {
    fn read(&self) -> Result<DataKey, KeyStoreError>;
    fn create(&self, key: &DataKey) -> Result<CreateOutcome, KeyStoreError>;
}

pub(super) struct DekCache {
    key: OnceLock<DataKey>,
    initialization: StdMutex<()>,
}

impl DekCache {
    pub(super) const fn new() -> Self {
        Self {
            key: OnceLock::new(),
            initialization: StdMutex::new(()),
        }
    }

    #[cfg(not(test))]
    fn get_or_try_init<S: KeyStore>(&self, store: &S) -> Result<DataKey, KeyStoreError> {
        self.get_or_try_init_with(store, generate_data_key)
    }

    pub(super) fn get_or_try_init_with<S, F>(
        &self,
        store: &S,
        generate: F,
    ) -> Result<DataKey, KeyStoreError>
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
pub(super) fn is_cargo_test_harness_executable(executable: &std::path::Path) -> bool {
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

pub(super) fn validate_key_bytes(bytes: &[u8], source: &str) -> Result<DataKey, KeyStoreError> {
    bytes.try_into().map_err(|_| {
        KeyStoreError::Corrupt(format!(
            "{source} contained {} bytes; expected {DEK_SIZE}",
            bytes.len()
        ))
    })
}

#[cfg(any(test, target_os = "macos"))]
pub(super) fn decode_hex_key(encoded: &str, source: &str) -> Result<DataKey, KeyStoreError> {
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
pub(super) fn read_macos_keychain_item_in(
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
pub(super) fn create_macos_keychain_item_in(
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
            crate::private_fs::create_private_dir_all(parent).map_err(|source| {
                classify_io_error("creating the Windows data directory", source)
            })?;

            let (temporary, mut file) = (0..16)
                .find_map(|_| {
                    let candidate = parent.join(format!(
                        ".dek.tmp-{}-{:016x}",
                        std::process::id(),
                        rand::random::<u64>()
                    ));
                    match crate::private_fs::create_private_file(&candidate) {
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
pub(super) fn move_windows_key_file_create_only(
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
