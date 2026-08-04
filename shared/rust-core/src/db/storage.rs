use super::*;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{BufReader, Read, Write};

const OWNER_FILE_NAME: &str = "storage-owner-v1";
const STORAGE_MARKER_NAME: &str = ".tailsync-storage-v1";
const STORAGE_FREE_SPACE_MARGIN_BYTES: u64 = 64 * 1024 * 1024;

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
}
