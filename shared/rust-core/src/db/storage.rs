use super::*;
use crate::crypto::Settings;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{BufReader, Read, Write};

const OWNER_FILE_NAME: &str = "storage-owner-v1";
const STORAGE_MARKER_NAME: &str = ".tailsync-storage-v1";
const STORAGE_FREE_SPACE_MARGIN_BYTES: u64 = 64 * 1024 * 1024;

/// Platform hooks for [`migrate_storage_with_rollback`]. Everything that
/// lives outside the core crate (transfer-progress state, notifications,
/// settings persistence) is injected so the orchestration stays testable.
/// The hook references must be `Send + Sync` because the orchestration is
/// awaited from Tauri command futures.
pub struct StorageMigrationHooks<'a> {
    /// How long to wait for active transfers to become idle.
    pub wait_timeout: std::time::Duration,
    /// Reports whether file transfers are still active; polled until idle.
    pub has_active_transfers: &'a (dyn Fn() -> bool + Send + Sync),
    /// Called once transfers are idle, right before the migration starts.
    pub notify: Option<&'a (dyn Fn() + Send + Sync)>,
    /// Persists the settings after the new root has been assigned.
    pub persist_settings: &'a (dyn Fn(&Settings) -> Result<(), String> + Send + Sync),
}

/// Reasons a storage-parent migration can fail once the platform surfaces
/// have been peeled away. The platform formats these into user-facing
/// messages so each surface keeps its exact error strings.
#[derive(Debug)]
pub enum StorageMigrationFailure {
    /// Transfers never became idle within `wait_timeout`.
    TimedOutWaitingForTransfers,
    /// The migration itself failed.
    Migrate(String),
    /// The new root could not be persisted; storage was rolled back to the
    /// old root and the partially migrated data was removed.
    SaveFailedAfterRollback { save_error: String },
    /// Persisting the new root failed and the rollback also failed.
    RollbackAlsoFailed {
        save_error: String,
        rollback_error: String,
    },
}

/// Wait for active transfers to finish, migrate the storage to a new parent
/// directory, and persist the new root in settings — rolling back to the old
/// root (and deleting the new one) if persistence fails.
pub async fn migrate_storage_with_rollback(
    database: &std::sync::Arc<tokio::sync::Mutex<HistoryDB>>,
    settings: &std::sync::Arc<tokio::sync::Mutex<Settings>>,
    parent: &Path,
    hooks: StorageMigrationHooks<'_>,
) -> Result<StorageMigrationResult, StorageMigrationFailure> {
    let wait_deadline = tokio::time::Instant::now() + hooks.wait_timeout;
    while (hooks.has_active_transfers)() {
        if tokio::time::Instant::now() >= wait_deadline {
            return Err(StorageMigrationFailure::TimedOutWaitingForTransfers);
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    if let Some(notify) = hooks.notify {
        notify();
    }
    let previous_storage_root = settings.lock().await.storage_root.clone();
    let parent = parent.to_path_buf();
    let database_for_migration = database.clone();
    let migrated = tokio::task::spawn_blocking(move || {
        database_for_migration
            .blocking_lock()
            .migrate_storage_parent(&parent)
            .map_err(|error| error.to_string())
    })
    .await;
    let result = match migrated {
        Ok(Ok(result)) => result,
        Ok(Err(error)) => return Err(StorageMigrationFailure::Migrate(error)),
        Err(error) => return Err(StorageMigrationFailure::Migrate(error.to_string())),
    };
    let mut settings_guard = settings.lock().await;
    settings_guard.storage_root = Some(result.new_root.clone());
    if let Err(save_error) = (hooks.persist_settings)(&settings_guard) {
        settings_guard.storage_root = previous_storage_root;
        drop(settings_guard);
        let old_root = std::path::PathBuf::from(&result.old_root);
        let database_for_rollback = database.clone();
        let rollback = tokio::task::spawn_blocking(move || {
            database_for_rollback
                .blocking_lock()
                .reopen_storage_at(&old_root)
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| error.to_string())
        .and_then(|result| result);
        return match rollback {
            Ok(()) => {
                let _ = delete_old_storage(std::path::Path::new(&result.new_root));
                Err(StorageMigrationFailure::SaveFailedAfterRollback { save_error })
            }
            Err(rollback_error) => Err(StorageMigrationFailure::RollbackAlsoFailed {
                save_error,
                rollback_error,
            }),
        };
    }
    Ok(result)
}

#[derive(Serialize, Deserialize)]
struct StorageMarker {
    owner_id: String,
}

impl HistoryDB {
    pub fn storage_status(&self) -> StorageStatus {
        let root = get_storage_dir();
        if !self.storage_available {
            return StorageStatus {
                root: root.to_string_lossy().into_owned(),
                used_bytes: 0,
                quota_bytes: self.storage_quota_bytes,
                available: false,
                error: Some("Configured storage is unavailable".to_string()),
            };
        }
        match bulk_storage_size(&root) {
            Ok(used_bytes) => StorageStatus {
                root: root.to_string_lossy().into_owned(),
                used_bytes,
                quota_bytes: self.storage_quota_bytes,
                available: true,
                error: None,
            },
            Err(error) => StorageStatus {
                root: root.to_string_lossy().into_owned(),
                used_bytes: 0,
                quota_bytes: self.storage_quota_bytes,
                available: false,
                error: Some(error.to_string()),
            },
        }
    }

    /// Ensure a complete incoming batch can be retained without exceeding the
    /// configured quota. Oldest unpinned history is evicted first.
    pub fn reserve_for_file_batch(
        &mut self,
        incoming_bytes: u64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if !self.storage_available {
            return Err("Configured storage is unavailable; file transfer is paused".into());
        }
        let required_free = required_free_space_for_batch(incoming_bytes);
        let available = fs2::available_space(get_storage_dir())?;
        if available < required_free {
            return Err(format!(
                "Not enough free disk space for this file batch ({} bytes available, {} bytes required)",
                available, required_free
            )
            .into());
        }
        loop {
            let used = bulk_storage_size(&get_storage_dir())?;
            if used.saturating_add(incoming_bytes) <= self.storage_quota_bytes {
                return Ok(());
            }
            let oldest = self
                .conn
                .query_row(
                    "SELECT id FROM history WHERE pinned = 0
                     ORDER BY timestamp ASC, id ASC LIMIT 1",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?;
            let Some(id) = oldest else {
                return Err(format!(
                    "Storage quota is full; pinned history leaves no room for this {} byte batch",
                    incoming_bytes
                )
                .into());
            };
            let ids = self.expand_batch_groups(vec![id])?;
            self.delete_entries(&ids)?;
        }
    }

    pub fn set_pinned(&mut self, id: i64, pinned: bool) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.execute(
            "UPDATE history SET pinned = ?1 WHERE id = ?2",
            params![i64::from(pinned), id],
        )?;
        Ok(())
    }

    /// Copy all bulk data into `<parent>/TailSync Data`, verify every file and
    /// SQLite, then switch this open database to the new location. The old
    /// location is intentionally retained for explicit user cleanup.
    pub fn migrate_storage_parent(
        &mut self,
        parent: &Path,
    ) -> Result<StorageMigrationResult, Box<dyn std::error::Error>> {
        validate_storage_dir(parent)?;
        let old_root = get_storage_dir();
        let target = parent.join(STORAGE_DIRECTORY_NAME);
        if paths_equivalent(&old_root, &target) {
            return Ok(StorageMigrationResult {
                new_root: target.to_string_lossy().into_owned(),
                old_root: old_root.to_string_lossy().into_owned(),
                old_size_bytes: bulk_storage_size(&old_root).unwrap_or(0),
            });
        }

        fs::create_dir_all(parent)?;
        let owner_id = load_or_create_owner_id()?;
        validate_existing_target(&target, &owner_id)?;
        self.conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        let old_size_bytes = bulk_storage_size(&old_root)?;
        let required_free = old_size_bytes.saturating_add(STORAGE_FREE_SPACE_MARGIN_BYTES);
        let available = fs2::available_space(parent)?;
        if available < required_free {
            return Err(format!(
                "Not enough free disk space to move TailSync data ({} bytes available, {} bytes required)",
                available, required_free
            )
            .into());
        }
        let staging = parent.join(format!(
            ".tailsync-migrating-{:016x}",
            rand::random::<u64>()
        ));
        fs::create_dir(&staging)?;

        let migration = (|| -> Result<(), Box<dyn std::error::Error>> {
            copy_bulk_storage_verified(&old_root, &staging)?;
            write_marker(&staging, &owner_id)?;
            verify_sqlite(&staging.join("history-v2.db"))?;
            fs::rename(&staging, &target)?;
            Ok(())
        })();
        if let Err(error) = migration {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }

        if let Err(error) = self.reopen_storage_at(&target) {
            let _ = fs::remove_dir_all(&target);
            return Err(error);
        }

        Ok(StorageMigrationResult {
            new_root: target.to_string_lossy().into_owned(),
            old_root: old_root.to_string_lossy().into_owned(),
            old_size_bytes,
        })
    }

    pub fn reopen_storage_at(&mut self, root: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let previous_root = get_storage_dir();
        configure_storage_dir(Some(root))?;
        let mut replacement = match Self::open_at(root) {
            Ok(database) => database,
            Err(error) => {
                let _ = configure_storage_dir(Some(&previous_root));
                return Err(error);
            }
        };
        replacement.max_history = self.max_history;
        replacement.storage_quota_bytes = self.storage_quota_bytes;
        replacement.storage_available = true;
        *self = replacement;
        Ok(())
    }
}

fn required_free_space_for_batch(incoming_bytes: u64) -> u64 {
    incoming_bytes
        .saturating_mul(2)
        .saturating_add(STORAGE_FREE_SPACE_MARGIN_BYTES)
}

pub fn delete_old_storage(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let current = get_storage_dir();
    if paths_equivalent(path, &current) {
        return Err("Cannot delete the active TailSync storage".into());
    }
    if paths_equivalent(path, &get_data_dir()) {
        for name in bulk_storage_names() {
            let target = path.join(name);
            if target.is_dir() {
                match fs::remove_dir_all(&target) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
            } else {
                match fs::remove_file(&target) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
            }
        }
        return Ok(());
    }
    let owner_id = load_or_create_owner_id()?;
    let marker: StorageMarker = serde_json::from_slice(&fs::read(path.join(STORAGE_MARKER_NAME))?)?;
    if marker.owner_id != owner_id {
        return Err("The old storage directory belongs to another installation".into());
    }
    fs::remove_dir_all(path)?;
    Ok(())
}

fn load_or_create_owner_id() -> Result<String, Box<dyn std::error::Error>> {
    let path = get_data_dir().join(OWNER_FILE_NAME);
    match fs::read_to_string(&path) {
        Ok(value) if !value.trim().is_empty() => return Ok(value.trim().to_string()),
        Ok(_) => return Err("Storage owner marker is empty".into()),
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
        Err(_) => {}
    }
    let value = hex::encode(rand::random::<[u8; 16]>());
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, value.as_bytes())?;
    fs::rename(temporary, path)?;
    Ok(value)
}

fn validate_existing_target(
    target: &Path,
    owner_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if !target.exists() {
        return Ok(());
    }
    let marker_path = target.join(STORAGE_MARKER_NAME);
    let marker: StorageMarker = serde_json::from_slice(&fs::read(&marker_path).map_err(|_| {
        format!(
            "{} already exists and is not an owned TailSync storage directory",
            target.display()
        )
    })?)?;
    if marker.owner_id != owner_id {
        return Err("The selected TailSync storage belongs to another installation".into());
    }
    Err("The selected TailSync storage already contains data".into())
}

fn write_marker(root: &Path, owner_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let marker = serde_json::to_vec_pretty(&StorageMarker {
        owner_id: owner_id.to_string(),
    })?;
    fs::write(root.join(STORAGE_MARKER_NAME), marker)?;
    Ok(())
}

fn paths_equivalent(left: &Path, right: &Path) -> bool {
    let left = left.canonicalize().unwrap_or_else(|_| left.to_path_buf());
    let right = right.canonicalize().unwrap_or_else(|_| right.to_path_buf());
    if cfg!(windows) {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    } else {
        left == right
    }
}

fn copy_directory_verified(source: &Path, target: &Path) -> Result<(), Box<dyn std::error::Error>> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        if source_path.file_name().and_then(|name| name.to_str()) == Some(STORAGE_MARKER_NAME) {
            continue;
        }
        let target_path = target.join(entry.file_name());
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            fs::create_dir(&target_path)?;
            copy_directory_verified(&source_path, &target_path)?;
        } else if metadata.is_file() {
            copy_file_verified(&source_path, &target_path)?;
        }
    }
    Ok(())
}

fn bulk_storage_names() -> [&'static str; 7] {
    [
        "history-v2.db",
        "history-v2.db-wal",
        "history-v2.db-shm",
        "file-history",
        "image-history",
        "incoming",
        "clipboard-files",
    ]
}

fn copy_bulk_storage_verified(
    source: &Path,
    target: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    for name in bulk_storage_names() {
        let source_path = source.join(name);
        if !source_path.exists() {
            continue;
        }
        let target_path = target.join(name);
        if source_path.is_dir() {
            fs::create_dir(&target_path)?;
            copy_directory_verified(&source_path, &target_path)?;
        } else {
            copy_file_verified(&source_path, &target_path)?;
        }
    }
    Ok(())
}

fn bulk_storage_size(root: &Path) -> std::io::Result<u64> {
    let _ = fs::read_dir(root)?;
    let mut total = 0_u64;
    for name in bulk_storage_names() {
        let path = root.join(name);
        if !path.exists() {
            continue;
        }
        total = total.saturating_add(if path.is_dir() {
            directory_size(&path)?
        } else {
            path.metadata()?.len()
        });
    }
    Ok(total)
}

fn copy_file_verified(source: &Path, target: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut reader = BufReader::new(fs::File::open(source)?);
    let mut writer = fs::File::create(target)?;
    let mut source_hasher = blake3::Hasher::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        source_hasher.update(&buffer[..count]);
        writer.write_all(&buffer[..count])?;
    }
    writer.sync_all()?;
    let copied_hash = hash_file(target)?;
    if source_hasher.finalize() != copied_hash {
        return Err(format!(
            "Storage migration verification failed for {}",
            source.display()
        )
        .into());
    }
    Ok(())
}

fn hash_file(path: &Path) -> std::io::Result<blake3::Hash> {
    let mut reader = BufReader::new(fs::File::open(path)?);
    let mut hasher = blake3::Hasher::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hasher.finalize())
}

fn verify_sqlite(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if !path.exists() {
        return Ok(());
    }
    let connection = Connection::open(path)?;
    let integrity: String = connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if integrity != "ok" {
        return Err(
            format!("Migrated history database failed integrity check: {integrity}").into(),
        );
    }
    Ok(())
}

fn directory_size(path: &Path) -> std::io::Result<u64> {
    let mut total = 0_u64;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        total = if metadata.is_dir() {
            total.saturating_add(directory_size(&entry.path())?)
        } else if metadata.is_file() {
            total.saturating_add(metadata.len())
        } else {
            total
        };
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_preflight_accounts_for_encrypted_history_copy_and_margin() {
        assert_eq!(
            required_free_space_for_batch(1024),
            2 * 1024 + STORAGE_FREE_SPACE_MARGIN_BYTES
        );
        assert_eq!(required_free_space_for_batch(u64::MAX), u64::MAX);
    }

    #[test]
    fn corrupt_sqlite_is_rejected_during_migration_verification() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-corrupt-migration-{:016x}",
            rand::random::<u64>()
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("history-v2.db");
        fs::write(&path, b"not a sqlite database").unwrap();
        assert!(verify_sqlite(&path).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn verified_storage_copy_preserves_files_and_valid_sqlite() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-valid-migration-{:016x}",
            rand::random::<u64>()
        ));
        let source = root.join("source");
        let target = root.join("target");
        fs::create_dir_all(source.join("file-history")).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(source.join("file-history").join("payload.bin"), b"payload").unwrap();
        let connection = Connection::open(source.join("history-v2.db")).unwrap();
        connection
            .execute_batch("CREATE TABLE history (id INTEGER PRIMARY KEY);")
            .unwrap();
        drop(connection);

        copy_bulk_storage_verified(&source, &target).unwrap();
        verify_sqlite(&target.join("history-v2.db")).unwrap();
        assert_eq!(
            fs::read(target.join("file-history").join("payload.bin")).unwrap(),
            b"payload"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn existing_target_requires_the_same_owner_marker() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-storage-owner-{:016x}",
            rand::random::<u64>()
        ));
        fs::create_dir_all(&root).unwrap();
        write_marker(&root, "another-owner").unwrap();
        let error = validate_existing_target(&root, "this-owner").unwrap_err();
        assert!(error.to_string().contains("another installation"));
        fs::remove_dir_all(root).unwrap();
    }

    fn temp_migration_base(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("tailsync-{name}-{:016x}", rand::random::<u64>()))
    }

    fn test_database(root: &Path) -> std::sync::Arc<tokio::sync::Mutex<HistoryDB>> {
        std::sync::Arc::new(tokio::sync::Mutex::new(HistoryDB::open_at(root).unwrap()))
    }

    /// Serializes the tests that reconfigure the process-global storage
    /// directory so concurrent migration tests cannot steal each other's
    /// storage root.
    fn migration_global_lock() -> &'static tokio::sync::Mutex<()> {
        static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
        LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
    }

    #[tokio::test]
    async fn storage_migration_times_out_when_transfers_never_become_idle() {
        let base = temp_migration_base("migration-timeout");
        let old_root = base.join("old");
        let database = test_database(&old_root);
        let settings = std::sync::Arc::new(tokio::sync::Mutex::new(Settings::default()));
        let busy = || true;
        let persist = |_settings: &Settings| -> Result<(), String> { Ok(()) };

        let error = migrate_storage_with_rollback(
            &database,
            &settings,
            &base.join("parent"),
            StorageMigrationHooks {
                wait_timeout: std::time::Duration::from_millis(20),
                has_active_transfers: &busy,
                notify: None,
                persist_settings: &persist,
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(
            error,
            StorageMigrationFailure::TimedOutWaitingForTransfers
        ));
        drop(database);
        drop(settings);
        fs::remove_dir_all(base).unwrap();
    }

    #[tokio::test]
    async fn storage_migration_succeeds_and_persists_the_new_root() {
        let _guard = migration_global_lock().lock().await;
        let original = get_storage_dir();
        let base = temp_migration_base("migration-success");
        let old_root = base.join("old");
        let parent = base.join("parent");
        configure_storage_dir(Some(&old_root)).unwrap();
        let database = test_database(&old_root);
        database
            .lock()
            .await
            .add_text("before migration", "self")
            .unwrap();
        let settings = std::sync::Arc::new(tokio::sync::Mutex::new(Settings::default()));
        let previous_root = settings.lock().await.storage_root.clone();
        let idle = || false;
        let persist = |_settings: &Settings| -> Result<(), String> { Ok(()) };

        let result = migrate_storage_with_rollback(
            &database,
            &settings,
            &parent,
            StorageMigrationHooks {
                wait_timeout: std::time::Duration::from_secs(60),
                has_active_transfers: &idle,
                notify: None,
                persist_settings: &persist,
            },
        )
        .await
        .unwrap();
        assert_eq!(
            PathBuf::from(&result.new_root),
            parent.join(STORAGE_DIRECTORY_NAME)
        );
        assert_ne!(
            settings.lock().await.storage_root,
            previous_root,
            "the new root must be persisted into settings"
        );
        assert_eq!(
            settings.lock().await.storage_root.as_deref(),
            Some(result.new_root.as_str())
        );
        database
            .lock()
            .await
            .add_text("after migration", "self")
            .unwrap();

        drop(database);
        drop(settings);
        configure_storage_dir(Some(&original)).unwrap();
        fs::remove_dir_all(base).unwrap();
    }

    #[tokio::test]
    async fn storage_migration_rolls_back_when_persistence_fails() {
        let _guard = migration_global_lock().lock().await;
        let original = get_storage_dir();
        let base = temp_migration_base("migration-rollback");
        let old_root = base.join("old");
        let parent = base.join("parent");
        configure_storage_dir(Some(&old_root)).unwrap();
        let database = test_database(&old_root);
        database
            .lock()
            .await
            .add_text("before migration", "self")
            .unwrap();
        let settings = std::sync::Arc::new(tokio::sync::Mutex::new(Settings::default()));
        let previous_root = settings.lock().await.storage_root.clone();
        let idle = || false;
        let fail_persist = |_settings: &Settings| -> Result<(), String> {
            Err("simulated save failure".to_string())
        };

        let error = migrate_storage_with_rollback(
            &database,
            &settings,
            &parent,
            StorageMigrationHooks {
                wait_timeout: std::time::Duration::from_secs(60),
                has_active_transfers: &idle,
                notify: None,
                persist_settings: &fail_persist,
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(
            error,
            StorageMigrationFailure::SaveFailedAfterRollback { ref save_error }
                if save_error == "simulated save failure"
        ));
        assert_eq!(settings.lock().await.storage_root, previous_root);
        assert_eq!(
            get_storage_dir(),
            old_root,
            "storage must be back at the old root"
        );
        assert!(
            !parent.join(STORAGE_DIRECTORY_NAME).exists(),
            "the partially migrated data must be deleted"
        );
        database
            .lock()
            .await
            .add_text("still works after rollback", "self")
            .unwrap();

        drop(database);
        drop(settings);
        configure_storage_dir(Some(&original)).unwrap();
        fs::remove_dir_all(base).unwrap();
    }

    #[tokio::test]
    async fn storage_migration_reports_when_rollback_also_fails() {
        let _guard = migration_global_lock().lock().await;
        let original = get_storage_dir();
        let base = temp_migration_base("migration-rollback-failed");
        let old_root = base.join("old");
        let parent = base.join("parent");
        configure_storage_dir(Some(&old_root)).unwrap();
        let database = test_database(&old_root);
        database
            .lock()
            .await
            .add_text("before migration", "self")
            .unwrap();
        let settings = std::sync::Arc::new(tokio::sync::Mutex::new(Settings::default()));
        let previous_root = settings.lock().await.storage_root.clone();
        let idle = || false;
        let old_root_for_hook = old_root.clone();
        let sabotage_persist = move |_settings: &Settings| -> Result<(), String> {
            // Replace the old root's database with a directory so the
            // rollback reopen cannot open it.
            fs::remove_file(old_root_for_hook.join("history-v2.db")).unwrap();
            fs::create_dir(old_root_for_hook.join("history-v2.db")).unwrap();
            Err("simulated save failure".to_string())
        };

        let error = migrate_storage_with_rollback(
            &database,
            &settings,
            &parent,
            StorageMigrationHooks {
                wait_timeout: std::time::Duration::from_secs(60),
                has_active_transfers: &idle,
                notify: None,
                persist_settings: &sabotage_persist,
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(
            error,
            StorageMigrationFailure::RollbackAlsoFailed { ref save_error, ref rollback_error }
                if save_error == "simulated save failure" && !rollback_error.is_empty()
        ));
        assert_eq!(settings.lock().await.storage_root, previous_root);
        database
            .lock()
            .await
            .add_text("still at the new root", "self")
            .unwrap();

        drop(database);
        drop(settings);
        configure_storage_dir(Some(&original)).unwrap();
        fs::remove_dir_all(base).unwrap();
    }

    #[tokio::test]
    async fn storage_migration_calls_the_notify_hook_once_transfers_are_idle() {
        let _guard = migration_global_lock().lock().await;
        let original = get_storage_dir();
        let base = temp_migration_base("migration-notify");
        let old_root = base.join("old");
        let parent = base.join("parent");
        configure_storage_dir(Some(&old_root)).unwrap();
        let database = test_database(&old_root);
        database
            .lock()
            .await
            .add_text("before migration", "self")
            .unwrap();
        let settings = std::sync::Arc::new(tokio::sync::Mutex::new(Settings::default()));
        use std::sync::atomic::{AtomicUsize, Ordering};
        let polls = std::sync::Arc::new(AtomicUsize::new(0));
        let polls_hook = polls.clone();
        let busy_once = move || {
            polls_hook.fetch_add(1, Ordering::SeqCst);
            polls_hook.load(Ordering::SeqCst) <= 1
        };
        let notified = std::sync::Arc::new(AtomicUsize::new(0));
        let notify_hook = notified.clone();
        let notify = move || {
            notify_hook.fetch_add(1, Ordering::SeqCst);
        };
        let persist = |_settings: &Settings| -> Result<(), String> { Ok(()) };

        migrate_storage_with_rollback(
            &database,
            &settings,
            &parent,
            StorageMigrationHooks {
                wait_timeout: std::time::Duration::from_secs(60),
                has_active_transfers: &busy_once,
                notify: Some(&notify),
                persist_settings: &persist,
            },
        )
        .await
        .unwrap();
        assert_eq!(
            notified.load(Ordering::SeqCst),
            1,
            "notify must fire exactly once"
        );

        drop(database);
        drop(settings);
        configure_storage_dir(Some(&original)).unwrap();
        fs::remove_dir_all(base).unwrap();
    }
}
