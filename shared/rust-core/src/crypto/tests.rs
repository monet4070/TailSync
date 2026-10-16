use super::{
    apply_settings_patch, apply_settings_update, decode_hex_key, validate_key_bytes, CreateOutcome,
    DataKey, DekCache, KeyStore, KeyStoreError, Settings, SettingsPatch, SettingsUpdateError,
    SettingsValidationError, DEK_SIZE,
};
use crate::db;
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
    assert!(super::is_cargo_test_harness_executable(
        std::path::Path::new("/workspace/target/debug/deps/tailsync_runtime-b130076dc5f25812.exe")
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
    assert!(!settings.auto_save_network_images);
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
        super::read_macos_keychain_item_in(Some(keychain), "com.tailsync.test.missing", account,),
        Err(KeyStoreError::NotFound)
    ));
}

#[test]
fn legacy_settings_preserve_safe_defaults_when_loaded() {
    let dir = temp_data_dir("legacy-no-auto-save");
    let path = write_config(
        &dir,
        r#"{
            "notifications_enabled": true,
            "progress_bar_enabled": true,
            "history_limit": 100,
            "enabled_peers": {},
            "language": "en"
        }"#,
    );

    let loaded = Settings::load_from_path(&path).unwrap();
    assert!(!loaded.auto_save_network_images);
    assert!(loaded.trusted_peer_keys.is_empty());
    assert!(loaded.trusted_peer_addresses.is_empty());
    assert!(loaded.paired_peer_endpoints.is_empty());
    assert_eq!(loaded.sync_shortcut, "CommandOrControl+Shift+S");
    assert_eq!(loaded.history_shortcut, "CommandOrControl+Shift+H");
    assert_eq!(loaded.connection_mode, "auto");

    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn auto_save_network_images_round_trips_through_save_and_load() {
    let dir = temp_data_dir("auto-save-round-trip");
    for enabled in [true, false] {
        let settings = Settings {
            auto_save_network_images: enabled,
            ..Settings::default()
        };
        let path = write_config(&dir, &serde_json::to_string_pretty(&settings).unwrap());

        let loaded = Settings::load_from_path(&path).unwrap();
        assert_eq!(loaded.auto_save_network_images, enabled);

        let encoded = serde_json::to_value(&loaded).unwrap();
        assert_eq!(
            encoded.get("auto_save_network_images"),
            Some(&serde_json::Value::Bool(enabled))
        );
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn auto_save_network_images_rejects_non_boolean_values() {
    let error = serde_json::from_str::<Settings>(
        r#"{
            "notifications_enabled": true,
            "progress_bar_enabled": true,
            "history_limit": 100,
            "enabled_peers": {},
            "language": "en",
            "auto_save_network_images": "yes"
        }"#,
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("boolean"),
        "unexpected error: {error}"
    );
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

/// S3-P1-3: the audit claimed an unauthenticated discovery broadcast could rewrite a
/// trusted device's persisted address. `remember_peer_address*` refuses to record
/// anything for a hostname that is not trusted, so discovery has no path into the
/// persisted route table at all — the claim does not reproduce, and this test is the
/// evidence for excluding it rather than adding address-provenance state.
#[test]
fn remembering_an_address_for_an_untrusted_hostname_persists_nothing() {
    let mut settings = Settings::default();

    assert!(
        !settings
            .remember_peer_address_without_save("stranger", "lan", "192.168.1.99")
            .unwrap(),
        "an untrusted hostname must not gain a remembered route"
    );
    assert!(
        settings.trusted_peer_addresses.is_empty(),
        "nothing may be persisted for an untrusted hostname"
    );

    // The same holds once trust is gone: a revoked hostname cannot update its route.
    settings
        .trusted_peer_keys
        .insert("trusted".into(), "key".into());
    assert!(settings
        .remember_peer_address_without_save("trusted", "lan", "192.168.1.20")
        .unwrap());
    settings.trusted_peer_keys.remove("trusted");
    assert!(
        !settings
            .remember_peer_address_without_save("trusted", "lan", "10.0.0.9")
            .unwrap(),
        "a revoked hostname must not be able to write a new route"
    );
}

#[test]
fn paired_peer_iroh_endpoint_is_validated_and_remembered() {
    const ENDPOINT_ID: &str = "5866666666666666666666666666666666666666666666666666666666666666";
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
fn re_pairing_same_hostname_with_a_different_key_is_rejected() {
    let mut settings = Settings::default();
    settings
        .trust_peer_without_save("windows", "key-a", "lan", Some("192.168.1.20"))
        .unwrap();

    // Re-pairing the same key is idempotent and may refresh the route.
    settings
        .trust_peer_without_save("windows", "key-a", "lan", Some("192.168.1.21"))
        .unwrap();
    assert_eq!(
        settings
            .trusted_peer_keys
            .get("windows")
            .map(String::as_str),
        Some("key-a")
    );

    // A different key under the same hostname must not silently replace the
    // existing trust anchor or its remembered route.
    let error = settings
        .trust_peer_without_save("windows", "key-b", "lan", Some("10.0.0.9"))
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("already paired with a different key"));
    assert_eq!(
        settings
            .trusted_peer_keys
            .get("windows")
            .map(String::as_str),
        Some("key-a")
    );
    assert_eq!(
        settings
            .trusted_peer_addresses
            .get("windows")
            .and_then(|addresses| addresses.get("lan"))
            .map(String::as_str),
        Some("192.168.1.21")
    );

    // After an explicit revoke the new device can be paired.
    settings.trusted_peer_keys.remove("windows");
    settings.trusted_peer_addresses.remove("windows");
    settings
        .trust_peer_without_save("windows", "key-b", "lan", Some("10.0.0.9"))
        .unwrap();
    assert_eq!(
        settings
            .trusted_peer_keys
            .get("windows")
            .map(String::as_str),
        Some("key-b")
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
    assert_eq!(super::normalize_connection_mode("iroh".into()), "iroh_only");
    assert_eq!(
        super::normalize_connection_mode("iroh_only".into()),
        "iroh_only"
    );
    assert_eq!(
        super::normalize_connection_mode("tailscale".into()),
        "tailscale_only"
    );
    assert_eq!(super::normalize_connection_mode("invalid".into()), "auto");
}

#[test]
fn obsolete_theme_fields_are_rejected() {
    let error = serde_json::from_str::<Settings>(r#"{"theme":"dark"}"#).unwrap_err();
    assert!(error.to_string().contains("unknown field"));
    let error = serde_json::from_str::<Settings>(r#"{"color_theme":"forest"}"#).unwrap_err();
    assert!(error.to_string().contains("unknown field"));
}

#[tokio::test]
async fn connection_mode_update_preserves_concurrent_fields_and_skips_noop_save() {
    let original = Settings {
        sync_enabled: false,
        history_limit: 250,
        ..Settings::default()
    };
    let state = tokio::sync::Mutex::new(original.clone());
    let calls = AtomicUsize::new(0);
    let persist = |settings: &Settings| {
        calls.fetch_add(1, Ordering::SeqCst);
        assert!(!settings.sync_enabled);
        assert_eq!(settings.history_limit, 250);
        Ok(())
    };
    let (updated, changed) = super::apply_connection_mode_update(&state, "lan_only", &persist)
        .await
        .unwrap();
    let mut expected = original;
    expected.connection_mode = "lan_only".into();
    assert!(changed);
    assert_eq!(updated, expected);
    assert_eq!(*state.lock().await, expected);
    let (_, changed) = super::apply_connection_mode_update(&state, "lan_only", &persist)
        .await
        .unwrap();
    assert!(!changed);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn connection_mode_update_does_not_commit_failed_or_invalid_values() {
    let original = Settings {
        sync_enabled: false,
        ..Settings::default()
    };
    let state = tokio::sync::Mutex::new(original.clone());
    let failure = |_: &Settings| Err("disk full".into());
    assert!(matches!(
        super::apply_connection_mode_update(&state, "lan_only", &failure).await,
        Err(super::SettingsUpdateError::Persist(_))
    ));
    assert_eq!(*state.lock().await, original);
    assert!(matches!(
        super::apply_connection_mode_update(&state, "invalid", &|_| panic!("must not persist"))
            .await,
        Err(super::SettingsUpdateError::Validation(_))
    ));
    assert_eq!(*state.lock().await, original);
}

// ─── Legacy theme migration (config-v2.json theme/color_theme → V2) ───

fn temp_data_dir(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "tailsync-{label}-{}-{:016x}",
        std::process::id(),
        rand::random::<u64>()
    ))
}

fn write_config(dir: &std::path::Path, body: &str) -> std::path::PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join("config-v2.json");
    std::fs::write(&path, body).unwrap();
    path
}

fn read_v2_local_settings(dir: &std::path::Path) -> crate::themes_v2::LocalThemeSettings {
    let bytes = std::fs::read(dir.join("themes-v2/local-settings.json")).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

/// A realistic pre-V2 config carrying every current field plus the two
/// obsolete theme fields, exactly as an upgraded user would have it.
fn full_legacy_config() -> &'static str {
    r#"{
        "notifications_enabled": true,
        "progress_bar_enabled": false,
        "sync_enabled": true,
        "sync_shortcut": "CommandOrControl+Shift+S",
        "history_shortcut": "CommandOrControl+Shift+H",
        "history_limit": 250,
        "storage_root": null,
        "storage_quota_bytes": 10737418240,
        "enabled_peers": { "desktop": true, "laptop": false },
        "language": "zh-CN",
        "connection_mode": "lan",
        "trusted_peer_keys": { "desktop": "public-key" },
        "trusted_peer_addresses": { "desktop": { "lan": "192.168.1.20" } },
        "paired_peer_endpoints": { "desktop": "192.168.1.20" },
        "theme": "dark",
        "color_theme": "forest"
    }"#
}

#[test]
fn legacy_config_with_all_old_fields_loads_and_migrates() {
    let dir = temp_data_dir("legacy-full");
    let path = write_config(&dir, full_legacy_config());

    let settings = Settings::load_from_path(&path).unwrap();

    // Every remaining field parses with its original value.
    assert_eq!(settings.history_limit, 250);
    assert_eq!(settings.language, "zh-CN");
    assert_eq!(settings.connection_mode, "lan_only"); // legacy "lan" normalization
    assert!(!settings.progress_bar_enabled);
    assert!(settings
        .enabled_peers
        .get("desktop")
        .copied()
        .unwrap_or(false));
    assert_eq!(
        settings
            .trusted_peer_keys
            .get("desktop")
            .map(String::as_str),
        Some("public-key")
    );

    // The V2 local selection was created from the legacy fields.
    let local = read_v2_local_settings(&dir);
    assert_eq!(local.active_theme_id, "builtin:ledger@1"); // forest -> ledger
    assert_eq!(local.appearance, "dark");
    assert!(!local.high_contrast);

    // The config file no longer carries the obsolete fields.
    let cleaned = std::fs::read_to_string(&path).unwrap();
    assert!(!cleaned.contains("\"theme\""));
    assert!(!cleaned.contains("\"color_theme\""));
    assert!(cleaned.contains("\"history_limit\": 250"));

    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn legacy_builtin_theme_mapping_is_exact() {
    for (legacy, expected) in [
        ("tailsync", "builtin:canvas@1"),
        ("ocean", "builtin:flux@1"),
        ("forest", "builtin:ledger@1"),
        ("rose", "builtin:aura@1"),
        ("high-contrast", "builtin:mono@1"),
    ] {
        let dir = temp_data_dir("legacy-map");
        let path = write_config(
            &dir,
            &format!(
                r#"{{"notifications_enabled": true, "progress_bar_enabled": true,
                     "history_limit": 100, "enabled_peers": {{}}, "language": "en",
                     "color_theme": "{legacy}" }}"#
            ),
        );
        Settings::load_from_path(&path).unwrap();
        assert_eq!(
            read_v2_local_settings(&dir).active_theme_id,
            expected,
            "legacy color_theme {legacy:?} must map to {expected}"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn existing_v2_local_selection_is_not_overwritten() {
    let dir = temp_data_dir("v2-present");
    std::fs::create_dir_all(dir.join("themes-v2")).unwrap();
    let local_path = dir.join("themes-v2/local-settings.json");
    std::fs::write(
        &local_path,
        r#"{"activeThemeId":"builtin:mono@1","appearance":"light","highContrast":true}"#,
    )
    .unwrap();
    let path = write_config(&dir, full_legacy_config());

    let settings = Settings::load_from_path(&path).unwrap();
    assert_eq!(settings.history_limit, 250);

    let local = read_v2_local_settings(&dir);
    assert_eq!(local.active_theme_id, "builtin:mono@1");
    assert_eq!(local.appearance, "light");
    assert!(local.high_contrast);

    // The config file is still cleaned: the V2 state is authoritative.
    let cleaned = std::fs::read_to_string(&path).unwrap();
    assert!(!cleaned.contains("\"theme\""));
    assert!(!cleaned.contains("\"color_theme\""));

    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn repeated_migration_is_idempotent() {
    let dir = temp_data_dir("idempotent");
    let path = write_config(&dir, full_legacy_config());

    Settings::load_from_path(&path).unwrap();
    let local_after_first = std::fs::read(dir.join("themes-v2/local-settings.json")).unwrap();
    let config_after_first = std::fs::read(&path).unwrap();

    // A second load must not rewrite anything or change the outcome.
    let settings = Settings::load_from_path(&path).unwrap();
    assert_eq!(settings.history_limit, 250);
    assert_eq!(
        std::fs::read(dir.join("themes-v2/local-settings.json")).unwrap(),
        local_after_first
    );
    assert_eq!(std::fs::read(&path).unwrap(), config_after_first);
    assert_eq!(
        read_v2_local_settings(&dir).active_theme_id,
        "builtin:ledger@1"
    );

    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn legacy_custom_theme_falls_back_to_canvas_and_keeps_old_files() {
    let dir = temp_data_dir("legacy-custom");
    // A pre-V2 custom theme file, in the old themes directory.
    std::fs::create_dir_all(dir.join("themes")).unwrap();
    let old_theme = dir.join("themes/studio.json");
    std::fs::write(&old_theme, br#"{"format":1,"id":"studio"}"#).unwrap();
    let path = write_config(
        &dir,
        r#"{"notifications_enabled": true, "progress_bar_enabled": true,
            "history_limit": 100, "enabled_peers": {}, "language": "en",
            "theme": "system", "color_theme": "custom:studio"}"#,
    );

    Settings::load_from_path(&path).unwrap();

    assert_eq!(
        read_v2_local_settings(&dir).active_theme_id,
        "builtin:canvas@1"
    );
    // The old theme file is preserved byte-for-byte.
    assert_eq!(
        std::fs::read(&old_theme).unwrap(),
        br#"{"format":1,"id":"studio"}"#
    );
    // And the legacy directory itself is untouched (no auto-conversion).
    assert!(!dir.join("themes-v2/studio").exists());

    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn invalid_legacy_theme_values_fall_back_without_failing() {
    let dir = temp_data_dir("legacy-lenient");
    let path = write_config(
        &dir,
        r#"{"notifications_enabled": true, "progress_bar_enabled": true,
            "history_limit": 100, "enabled_peers": {}, "language": "en",
            "theme": "sepia", "color_theme": "chartreuse"}"#,
    );

    Settings::load_from_path(&path).unwrap();
    let local = read_v2_local_settings(&dir);
    assert_eq!(local.appearance, "system");
    assert_eq!(local.active_theme_id, "builtin:canvas@1");

    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn non_theme_unknown_fields_still_error_and_preserve_config() {
    let dir = temp_data_dir("unknown-field");
    let original = br#"{"notifications_enabled": true, "progress_bar_enabled": true,
        "history_limit": 100, "enabled_peers": {}, "language": "en",
        "theme": "dark", "color_theme": "forest", "bogus_field": true}"#;
    let path = write_config(&dir, std::str::from_utf8(original).unwrap());

    let error = Settings::load_from_path(&path).unwrap_err();
    assert!(error.to_string().contains("unknown field"));

    // Original file preserved; nothing was migrated.
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert!(!dir.join("themes-v2/local-settings.json").exists());

    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn corrupt_config_errors_and_preserves_file() {
    let dir = temp_data_dir("corrupt");
    let original = b"{ not valid json";
    let path = write_config(&dir, std::str::from_utf8(original).unwrap());

    assert!(Settings::load_from_path(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert!(!dir.join("themes-v2").exists());

    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn v2_write_failure_errors_and_preserves_config() {
    let dir = temp_data_dir("write-fail");
    std::fs::create_dir_all(&dir).unwrap();
    // A plain file at the themes-v2 path blocks create_dir_all, so the
    // V2 write cannot succeed — and the config must not be rewritten.
    std::fs::write(dir.join("themes-v2"), b"not a directory").unwrap();
    let original = full_legacy_config().as_bytes().to_vec();
    let path = write_config(&dir, full_legacy_config());

    assert!(Settings::load_from_path(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), original);

    std::fs::remove_dir_all(dir).unwrap();
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
    settings.connection_mode = "iroh_only".into();
    assert!(settings.validate_user_values().is_ok());
    settings.connection_mode = "future_mode".into();
    assert!(matches!(
        settings.validate_user_values(),
        Err(SettingsValidationError::ConnectionMode)
    ));
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
        ..Settings::default()
    };
    requested.enabled_peers.insert("stale".into(), false);
    requested.storage_root = Some(String::new());
    requested
        .trusted_peer_keys
        .insert("stale".into(), "wrong-key".into());

    let updated = current.prepare_user_update(requested).unwrap();

    assert_eq!(updated.history_limit, 250);
    assert_eq!(updated.enabled_peers, current.enabled_peers);
    assert_eq!(updated.storage_root, current.storage_root);
    assert_eq!(updated.trusted_peer_keys, current.trusted_peer_keys);
    assert_eq!(
        updated.trusted_peer_addresses,
        current.trusted_peer_addresses
    );
    assert_eq!(updated.paired_peer_endpoints, current.paired_peer_endpoints);
}

#[test]
fn user_update_carries_auto_save_network_images() {
    let current = Settings::default();
    let requested = Settings {
        auto_save_network_images: true,
        ..Settings::default()
    };

    let updated = current.prepare_user_update(requested).unwrap();

    assert!(updated.auto_save_network_images);
}

#[tokio::test]
async fn settings_update_persists_and_applies_db_limits() {
    let root = std::env::temp_dir().join(format!(
        "tailsync-settings-update-{:016x}",
        rand::random::<u64>()
    ));
    let database = std::sync::Arc::new(tokio::sync::Mutex::new(
        db::HistoryDB::open_at(&root).unwrap(),
    ));
    for index in 0..15 {
        database
            .lock()
            .await
            .add_text(&format!("entry {index}"), "self")
            .unwrap();
    }
    let settings = std::sync::Arc::new(tokio::sync::Mutex::new(Settings::default()));
    let persisted = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let persist_counter = persisted.clone();
    let persist = move |_settings: &Settings| -> Result<(), String> {
        persist_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    };
    let requested = Settings {
        history_limit: 10,
        ..Settings::default()
    };

    let outcome = apply_settings_update(&settings, &database, requested, &persist, None)
        .await
        .unwrap();
    assert!(!outcome.mode_changed);
    assert_eq!(settings.lock().await.history_limit, 10);
    let remaining = database.lock().await.get_all(None, None, 100, 0).unwrap();
    assert_eq!(
        remaining.len(),
        10,
        "enforce_limits must evict to the limit"
    );
    assert_eq!(persisted.load(std::sync::atomic::Ordering::SeqCst), 1);
    drop(database);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn settings_update_routes_changed_shortcuts_through_the_hook() {
    let root = std::env::temp_dir().join(format!(
        "tailsync-settings-shortcut-{:016x}",
        rand::random::<u64>()
    ));
    let database = std::sync::Arc::new(tokio::sync::Mutex::new(
        db::HistoryDB::open_at(&root).unwrap(),
    ));
    let settings = std::sync::Arc::new(tokio::sync::Mutex::new(Settings::default()));
    let previous = settings.lock().await.clone();
    let persisted = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let persist_counter = persisted.clone();
    let persist = move |_settings: &Settings| -> Result<(), String> {
        persist_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    };
    let hook_calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let hook_counter = hook_calls.clone();
    let hook = move |seen_previous: &Settings, seen_next: &Settings| -> Result<(), String> {
        assert_eq!(seen_previous.sync_shortcut, previous.sync_shortcut);
        assert_eq!(seen_previous.history_shortcut, previous.history_shortcut);
        assert_eq!(seen_next.sync_shortcut, "Control+Shift+Z");
        assert_eq!(seen_next.history_shortcut, previous.history_shortcut);
        hook_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    };
    let requested = Settings {
        sync_shortcut: "Control+Shift+Z".to_string(),
        ..Settings::default()
    };

    let outcome = apply_settings_update(&settings, &database, requested, &persist, Some(&hook))
        .await
        .unwrap();
    assert!(!outcome.mode_changed);
    assert_eq!(settings.lock().await.sync_shortcut, "Control+Shift+Z");
    assert_eq!(hook_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(
        persisted.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "the shortcut hook performs its own persistence"
    );
    drop(database);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn settings_update_uses_plain_save_without_a_shortcut_hook() {
    let root = std::env::temp_dir().join(format!(
        "tailsync-settings-plain-save-{:016x}",
        rand::random::<u64>()
    ));
    let database = std::sync::Arc::new(tokio::sync::Mutex::new(
        db::HistoryDB::open_at(&root).unwrap(),
    ));
    let settings = std::sync::Arc::new(tokio::sync::Mutex::new(Settings::default()));
    let persisted = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let persist_counter = persisted.clone();
    let persist = move |_settings: &Settings| -> Result<(), String> {
        persist_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    };
    let requested = Settings {
        sync_shortcut: "Control+Shift+Z".to_string(),
        ..Settings::default()
    };

    apply_settings_update(&settings, &database, requested, &persist, None)
        .await
        .unwrap();
    assert_eq!(settings.lock().await.sync_shortcut, "Control+Shift+Z");
    assert_eq!(persisted.load(std::sync::atomic::Ordering::SeqCst), 1);
    drop(database);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn settings_update_rejects_invalid_values_without_persisting() {
    let root = std::env::temp_dir().join(format!(
        "tailsync-settings-invalid-{:016x}",
        rand::random::<u64>()
    ));
    let database = std::sync::Arc::new(tokio::sync::Mutex::new(
        db::HistoryDB::open_at(&root).unwrap(),
    ));
    let settings = std::sync::Arc::new(tokio::sync::Mutex::new(Settings::default()));
    let persisted = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let persist_counter = persisted.clone();
    let persist = move |_settings: &Settings| -> Result<(), String> {
        persist_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    };
    let requested = Settings {
        history_limit: 5,
        ..Settings::default()
    };

    let error = apply_settings_update(&settings, &database, requested, &persist, None)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        SettingsUpdateError::Validation(SettingsValidationError::HistoryLimit)
    ));
    assert_eq!(
        error.to_string(),
        "history_limit must be between 10 and 500"
    );
    assert_eq!(
        settings.lock().await.history_limit,
        Settings::default().history_limit
    );
    assert_eq!(persisted.load(std::sync::atomic::Ordering::SeqCst), 0);
    drop(database);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn settings_update_does_not_commit_when_persistence_fails() {
    let root = std::env::temp_dir().join(format!(
        "tailsync-settings-save-failed-{:016x}",
        rand::random::<u64>()
    ));
    let database = std::sync::Arc::new(tokio::sync::Mutex::new(
        db::HistoryDB::open_at(&root).unwrap(),
    ));
    let settings = std::sync::Arc::new(tokio::sync::Mutex::new(Settings::default()));
    let fail_persist =
        |_settings: &Settings| -> Result<(), String> { Err("simulated save failure".to_string()) };
    let requested = Settings {
        history_limit: 10,
        ..Settings::default()
    };

    let error = apply_settings_update(&settings, &database, requested, &fail_persist, None)
        .await
        .unwrap_err();
    assert!(
        matches!(error, SettingsUpdateError::Persist(ref message) if message == "simulated save failure")
    );
    assert_eq!(error.to_string(), "simulated save failure");
    assert_eq!(
        settings.lock().await.history_limit,
        Settings::default().history_limit
    );
    drop(database);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn settings_update_persists_auto_save_network_images() {
    let root = std::env::temp_dir().join(format!(
        "tailsync-settings-auto-save-{:016x}",
        rand::random::<u64>()
    ));
    let database = std::sync::Arc::new(tokio::sync::Mutex::new(
        db::HistoryDB::open_at(&root).unwrap(),
    ));
    let settings = std::sync::Arc::new(tokio::sync::Mutex::new(Settings::default()));
    let persisted = std::sync::Arc::new(Mutex::new(Vec::new()));
    let persisted_writer = persisted.clone();
    let persist = move |seen: &Settings| -> Result<(), String> {
        persisted_writer
            .lock()
            .unwrap()
            .push(seen.auto_save_network_images);
        Ok(())
    };
    let requested = Settings {
        auto_save_network_images: true,
        ..Settings::default()
    };

    apply_settings_update(&settings, &database, requested, &persist, None)
        .await
        .unwrap();

    assert!(settings.lock().await.auto_save_network_images);
    assert_eq!(persisted.lock().unwrap().as_slice(), &[true]);
    drop(database);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn settings_update_keeps_auto_save_value_when_persistence_fails() {
    let root = std::env::temp_dir().join(format!(
        "tailsync-settings-auto-save-failed-{:016x}",
        rand::random::<u64>()
    ));
    let database = std::sync::Arc::new(tokio::sync::Mutex::new(
        db::HistoryDB::open_at(&root).unwrap(),
    ));
    let settings = std::sync::Arc::new(tokio::sync::Mutex::new(Settings::default()));
    let fail_persist =
        |_settings: &Settings| -> Result<(), String> { Err("simulated save failure".to_string()) };
    let requested = Settings {
        auto_save_network_images: true,
        ..Settings::default()
    };

    let error = apply_settings_update(&settings, &database, requested, &fail_persist, None)
        .await
        .unwrap_err();

    assert!(
        matches!(error, SettingsUpdateError::Persist(ref message) if message == "simulated save failure")
    );
    assert!(!settings.lock().await.auto_save_network_images);
    drop(database);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn auto_save_toggle_does_not_wait_for_unrelated_history_limit_enforcement() {
    let settings = tokio::sync::Mutex::new(Settings::default());
    let database = tokio::sync::Mutex::new(db::HistoryDB::new_unavailable().unwrap());
    let held = database.lock().await;
    let requested = Settings {
        auto_save_network_images: true,
        ..Settings::default()
    };
    let result = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        apply_settings_update(&settings, &database, requested, &|_| Ok(()), None),
    )
    .await;
    drop(held);
    assert!(
        result.is_ok(),
        "A persisted toggle waited for unrelated database work"
    );
    assert!(result.unwrap().is_ok());
    assert!(settings.lock().await.auto_save_network_images);
}

#[tokio::test]
async fn settings_update_reports_connection_mode_changes() {
    let root = std::env::temp_dir().join(format!(
        "tailsync-settings-mode-{:016x}",
        rand::random::<u64>()
    ));
    let database = std::sync::Arc::new(tokio::sync::Mutex::new(
        db::HistoryDB::open_at(&root).unwrap(),
    ));
    let settings = std::sync::Arc::new(tokio::sync::Mutex::new(Settings::default()));
    let persist = |_settings: &Settings| -> Result<(), String> { Ok(()) };
    let requested = Settings {
        connection_mode: "lan_only".to_string(),
        ..Settings::default()
    };

    let outcome = apply_settings_update(&settings, &database, requested, &persist, None)
        .await
        .unwrap();
    assert!(outcome.mode_changed);
    assert_eq!(outcome.connection_mode, "lan_only");

    let outcome = apply_settings_update(&settings, &database, Settings::default(), &persist, None)
        .await
        .unwrap();
    assert!(
        outcome.mode_changed,
        "requesting auto after lan_only must report a change"
    );

    let outcome = apply_settings_update(&settings, &database, Settings::default(), &persist, None)
        .await
        .unwrap();
    assert!(
        !outcome.mode_changed,
        "requesting the current mode again must not report a change"
    );
    drop(database);
    std::fs::remove_dir_all(root).unwrap();
}

fn patch_database(label: &str) -> (std::path::PathBuf, Arc<tokio::sync::Mutex<db::HistoryDB>>) {
    let root = std::env::temp_dir().join(format!(
        "tailsync-settings-patch-{label}-{:016x}",
        rand::random::<u64>()
    ));
    let database = Arc::new(tokio::sync::Mutex::new(
        db::HistoryDB::open_at(&root).unwrap(),
    ));
    (root, database)
}

fn patch_from(json: serde_json::Value) -> SettingsPatch {
    SettingsPatch::from_json_value(json).expect("valid settings patch")
}

#[test]
fn settings_patch_fields_match_the_derived_schema() {
    let schema = serde_json::to_value(schemars::schema_for!(SettingsPatch)).unwrap();
    let mut properties = schema["properties"]
        .as_object()
        .expect("patch schema properties")
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    properties.sort();
    let mut declared = SettingsPatch::FIELDS
        .iter()
        .map(|field| (*field).to_string())
        .collect::<Vec<_>>();
    declared.sort();
    assert_eq!(declared, properties);
    assert_eq!(schema["additionalProperties"], serde_json::json!(false));
}

#[tokio::test]
async fn settings_patch_merges_two_stale_windows_in_either_order() {
    for reverse in [false, true] {
        let (root, database) = patch_database(if reverse { "reverse" } else { "forward" });
        let settings = Arc::new(tokio::sync::Mutex::new(Settings::default()));
        // Both windows read the same snapshot before either of them saves.
        let stale = settings.lock().await.clone();
        assert_eq!(stale.history_limit, 100);
        assert_eq!(stale.connection_mode, "auto");
        let persist = |_settings: &Settings| -> Result<(), String> { Ok(()) };

        let limit = patch_from(serde_json::json!({ "history_limit": 250 }));
        let mode = patch_from(serde_json::json!({ "connection_mode": "lan_only" }));
        let mut outcome = None;
        for patch in if reverse {
            [mode, limit]
        } else {
            [limit, mode]
        } {
            outcome = Some(
                apply_settings_patch(&settings, &database, patch, &persist)
                    .await
                    .unwrap(),
            );
        }
        let outcome = outcome.unwrap();

        let committed = settings.lock().await.clone();
        assert_eq!(committed.history_limit, 250);
        assert_eq!(committed.connection_mode, "lan_only");
        assert_eq!(
            outcome.persisted, committed,
            "canonical snapshot must match"
        );
        assert_eq!(
            outcome.mode_changed, !reverse,
            "only the patch that names the mode may report a mode change"
        );
        assert_eq!(outcome.connection_mode, "lan_only");
        drop(database);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[tokio::test]
async fn settings_patch_rejects_fields_owned_by_other_commands() {
    let (root, database) = patch_database("protected");
    let mut original = Settings::default();
    original
        .trusted_peer_keys
        .insert("paired-device".into(), "pinned-key".into());
    original.enabled_peers.insert("paired-device".into(), true);
    original.storage_root = Some("/tmp/tailsync-storage".into());
    original.sync_shortcut = "Control+Shift+Z".into();
    let settings = Arc::new(tokio::sync::Mutex::new(original.clone()));

    for rejected in [
        serde_json::json!({ "sync_enabled": false }),
        serde_json::json!({ "sync_shortcut": "Control+Shift+Q" }),
        serde_json::json!({ "history_shortcut": "Control+Shift+Q" }),
        serde_json::json!({ "enabled_peers": { "peer": false } }),
        serde_json::json!({ "storage_root": "/tmp/elsewhere" }),
        serde_json::json!({ "trusted_peer_keys": { "peer": "key" } }),
        serde_json::json!({ "trusted_peer_addresses": { "peer": { "lan": "1.2.3.4" } } }),
        serde_json::json!({ "paired_peer_endpoints": { "peer": "1.2.3.4" } }),
        serde_json::json!({ "unexpected_field": "private-value" }),
    ] {
        let error = SettingsPatch::from_json_value(rejected.clone()).unwrap_err();
        assert!(
            error.starts_with("invalid request json: update_settings expects a patch"),
            "{error}"
        );
        assert!(
            !error.contains("private-value"),
            "the rejection must not echo values: {error}"
        );
        assert!(
            !error.contains("pinned-key") && !error.contains("/tmp/"),
            "the rejection must not echo values: {error}"
        );
    }
    assert_eq!(*settings.lock().await, original, "state must not move");
    drop(database);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn settings_patch_rejects_invalid_values_without_persisting() {
    let (root, database) = patch_database("invalid");
    let settings = Arc::new(tokio::sync::Mutex::new(Settings::default()));
    let persisted = Arc::new(AtomicUsize::new(0));
    let counter = persisted.clone();
    let persist = move |_settings: &Settings| -> Result<(), String> {
        counter.fetch_add(1, Ordering::SeqCst);
        Ok(())
    };

    for invalid in [
        serde_json::json!({ "history_limit": 5 }),
        serde_json::json!({ "history_limit": 501 }),
        serde_json::json!({ "storage_quota_bytes": 1024u64 }),
        serde_json::json!({ "language": "fr" }),
        serde_json::json!({ "connection_mode": "bogus" }),
    ] {
        let patch = patch_from(invalid.clone());
        let error = apply_settings_patch(&settings, &database, patch, &persist)
            .await
            .unwrap_err();
        assert!(
            matches!(error, SettingsUpdateError::Validation(_)),
            "{invalid} must be rejected: {error}"
        );
    }
    assert_eq!(persisted.load(Ordering::SeqCst), 0);
    assert_eq!(*settings.lock().await, Settings::default());
    drop(database);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn settings_patch_does_not_commit_when_persistence_fails() {
    let (root, database) = patch_database("persist-failure");
    let original = Settings::default();
    let settings = Arc::new(tokio::sync::Mutex::new(original.clone()));
    let persist = |_settings: &Settings| -> Result<(), String> { Err("disk full".into()) };

    let patch = patch_from(serde_json::json!({
        "notifications_enabled": false,
        "auto_save_network_images": true,
    }));
    let error = apply_settings_patch(&settings, &database, patch, &persist)
        .await
        .unwrap_err();
    assert!(matches!(error, SettingsUpdateError::Persist(_)));
    assert_eq!(*settings.lock().await, original);
    drop(database);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn settings_patch_persists_auto_save_network_images_and_keeps_it_on_failure() {
    let (root, database) = patch_database("auto-save");
    let settings = Arc::new(tokio::sync::Mutex::new(Settings::default()));
    let persist = |_settings: &Settings| -> Result<(), String> { Ok(()) };

    let patch = patch_from(serde_json::json!({ "auto_save_network_images": true }));
    let outcome = apply_settings_patch(&settings, &database, patch, &persist)
        .await
        .unwrap();
    assert!(outcome.persisted.auto_save_network_images);
    assert!(settings.lock().await.auto_save_network_images);

    let failing = |_settings: &Settings| -> Result<(), String> { Err("read-only".into()) };
    let patch = patch_from(serde_json::json!({ "auto_save_network_images": false }));
    apply_settings_patch(&settings, &database, patch, &failing)
        .await
        .unwrap_err();
    assert!(
        settings.lock().await.auto_save_network_images,
        "a failed save must keep the committed auto-save value"
    );
    drop(database);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn settings_patch_never_changes_shortcuts_or_sync_state() {
    let (root, database) = patch_database("no-shortcuts");
    let original = Settings {
        sync_shortcut: "Control+Shift+Z".into(),
        history_shortcut: "Control+Shift+Y".into(),
        sync_enabled: false,
        ..Settings::default()
    };
    let settings = Arc::new(tokio::sync::Mutex::new(original.clone()));
    let persist = |_settings: &Settings| -> Result<(), String> { Ok(()) };

    let patch = patch_from(serde_json::json!({
        "notifications_enabled": false,
        "progress_bar_enabled": false,
    }));
    let outcome = apply_settings_patch(&settings, &database, patch, &persist)
        .await
        .unwrap();
    assert!(!outcome.mode_changed);
    assert!(!outcome.persisted.notifications_enabled);
    assert!(!outcome.persisted.progress_bar_enabled);
    assert_eq!(outcome.persisted.sync_shortcut, "Control+Shift+Z");
    assert_eq!(outcome.persisted.history_shortcut, "Control+Shift+Y");
    assert!(!outcome.persisted.sync_enabled);
    drop(database);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn settings_patch_keeps_server_owned_state_while_applying_user_fields() {
    let (root, database) = patch_database("server-owned");
    let mut original = Settings::default();
    original
        .trusted_peer_keys
        .insert("paired-device".into(), "pinned-key".into());
    original.enabled_peers.insert("paired-device".into(), true);
    original.storage_root = Some("/tmp/tailsync-storage".into());
    let settings = Arc::new(tokio::sync::Mutex::new(original.clone()));
    let persist = |_settings: &Settings| -> Result<(), String> { Ok(()) };

    let patch = patch_from(serde_json::json!({ "language": "zh-CN" }));
    let outcome = apply_settings_patch(&settings, &database, patch, &persist)
        .await
        .unwrap();
    assert_eq!(outcome.persisted.language, "zh-CN");
    assert_eq!(
        outcome.persisted.trusted_peer_keys,
        original.trusted_peer_keys
    );
    assert_eq!(outcome.persisted.enabled_peers, original.enabled_peers);
    assert_eq!(outcome.persisted.storage_root, original.storage_root);
    drop(database);
    std::fs::remove_dir_all(root).unwrap();
}

/// Compatibility pin for the retained whole-snapshot helper, and the reason the
/// window surfaces must send finite patches: two windows that each send the
/// snapshot they read will roll back fields the other one changed. Here the
/// second (stale) snapshot restores `history_limit` to 100 after the first
/// window set it to 250.
#[tokio::test]
async fn legacy_full_snapshot_helper_still_replaces_whole_snapshots() {
    let (root, database) = patch_database("legacy-snapshot");
    let settings = Arc::new(tokio::sync::Mutex::new(Settings::default()));
    let stale = settings.lock().await.clone();
    let persist = |_settings: &Settings| -> Result<(), String> { Ok(()) };

    apply_settings_update(
        &settings,
        &database,
        Settings {
            history_limit: 250,
            ..stale.clone()
        },
        &persist,
        None,
    )
    .await
    .unwrap();
    apply_settings_update(
        &settings,
        &database,
        Settings {
            connection_mode: "lan_only".into(),
            ..stale
        },
        &persist,
        None,
    )
    .await
    .unwrap();

    let committed = settings.lock().await.clone();
    assert_eq!(
        committed.history_limit, 100,
        "a stale whole snapshot still overwrites an unrelated field"
    );
    assert_eq!(committed.connection_mode, "lan_only");
    drop(database);
    std::fs::remove_dir_all(root).unwrap();
}
