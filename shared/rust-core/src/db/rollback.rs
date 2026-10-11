//! Pre-upgrade rollback package (repair plan §4 R1-03).
//!
//! A database-only snapshot is not a rollback artefact: the v11→v12 upgrade also
//! touches external payload files, and the acceptance checklist already says so
//! (`docs/COMPOSITE-CLIPBOARD-NATIVE-ACCEPTANCE-2026-10-03.md`:35). This module
//! writes a **complete** package before the upgrade runs:
//!
//! ```text
//! <storage>/pre-v12-rollback/
//!   history-before-v12.db        SQLite consistent snapshot (VACUUM INTO)
//!   payloads/image-history/...   independent copies of every live payload
//!   payloads/file-history/...
//!   manifest.json                schema version, sizes, digests, modes
//! ```
//!
//! Guarantees, each matching a clause of the plan:
//!
//! - **Consistent snapshot, not a file copy.** `VACUUM INTO` reads through SQLite,
//!   so a `-wal`/`-shm` triple is never copied out from under a live connection.
//! - **Independent payload copies.** `copy_file_verified` writes new inodes and
//!   re-hashes them; a shared hardlink (which the original could mutate in place)
//!   is never used.
//! - **Frozen while snapshotting.** The storage maintenance lock is held for the
//!   whole operation, so a concurrent sweep or quota eviction cannot unlink a
//!   payload between enumeration and copy.
//! - **Fails closed.** Insufficient space, a copy mismatch or an unreadable
//!   reference returns an error, which stops the upgrade.
//! - **Out of quota and out of the sweeper's reach.** The directory is not in
//!   `bulk_storage_names`, and the orphan sweep only walks `file-history` /
//!   `image-history`. It is released only by an explicit [`retire`].
//! - **Never rolls back identity, trust or effect receipts.** Those live outside
//!   the history tree; `manifest.excludes` records that explicitly so a restore
//!   cannot be mistaken for a full rollback.

use super::*;

/// Directory name for the package, directly under the storage root.
pub const ROLLBACK_DIR_NAME: &str = "pre-v12-rollback";
/// Snapshot file name inside the package.
pub const SNAPSHOT_DB_NAME: &str = "history-before-v12.db";
/// Manifest file name inside the package.
pub const MANIFEST_NAME: &str = "manifest.json";
/// Manifest format version.
pub const MANIFEST_VERSION: u32 = 1;
/// Extra headroom required beyond the bytes to be written.
const SPACE_MARGIN_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RollbackFile {
    /// Path relative to the package directory, with `/` separators.
    pub relative_path: String,
    pub bytes: u64,
    /// Historical field name; the package digest algorithm is BLAKE3.
    pub sha256: String,
    /// Unix permission bits, or 0 on platforms without them.
    pub mode: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RollbackManifest {
    pub manifest_version: u32,
    /// Schema version the snapshot is at (the version we are upgrading *from*).
    pub source_schema_version: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_schema_version: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest_algorithm: Option<String>,
    pub created_at: String,
    pub database: RollbackFile,
    pub payloads: Vec<RollbackFile>,
    pub total_bytes: u64,
    /// What a restore deliberately does NOT bring back.
    pub excludes: Vec<String>,
}

/// The package directory for a storage root.
pub fn package_dir(storage_dir: &Path) -> PathBuf {
    storage_dir.join(ROLLBACK_DIR_NAME)
}

fn permission_mode(path: &Path) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|meta| meta.permissions().mode() & 0o777)
            .unwrap_or(0)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        0
    }
}

fn relative_string(package: &Path, path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let relative = path
        .strip_prefix(package)
        .map_err(|_| "rollback entry escaped the package directory")?;
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            std::path::Component::Normal(part) => parts.push(
                part.to_str()
                    .ok_or("rollback entry has a non-UTF-8 name")?
                    .to_string(),
            ),
            _ => return Err("rollback entry escaped the package directory".into()),
        }
    }
    Ok(parts.join("/"))
}

fn describe(package: &Path, path: &Path) -> Result<RollbackFile, Box<dyn std::error::Error>> {
    Ok(RollbackFile {
        relative_path: relative_string(package, path)?,
        bytes: std::fs::metadata(path)?.len(),
        sha256: super::storage::hash_file(path)?.to_hex().to_string(),
        mode: permission_mode(path),
    })
}

fn fsync_directory(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(unix)]
    {
        std::fs::File::open(path)?.sync_all()?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

/// Verify every file in an existing package against its manifest.
///
/// Used both as the last step of [`prepare`] and by the restore script, so a
/// tampered or truncated package is detected instead of being restored.
pub fn verify(storage_dir: &Path) -> Result<RollbackManifest, Box<dyn std::error::Error>> {
    verify_for(storage_dir, 12)
}
pub fn verify_v13(storage_dir: &Path) -> Result<RollbackManifest, Box<dyn std::error::Error>> {
    verify_for(storage_dir, 13)
}
fn directory_for(storage_dir: &Path, target: i64) -> PathBuf {
    storage_dir.join(format!("pre-v{target}-rollback"))
}
fn snapshot_for(target: i64) -> String {
    if target == 12 {
        SNAPSHOT_DB_NAME.into()
    } else {
        format!("history-before-v{target}.db")
    }
}
fn verify_for(
    storage_dir: &Path,
    target: i64,
) -> Result<RollbackManifest, Box<dyn std::error::Error>> {
    let package = directory_for(storage_dir, target);
    let manifest_path = package.join(MANIFEST_NAME);
    let bytes = std::fs::read(&manifest_path)
        .map_err(|error| format!("rollback manifest is unreadable: {error}"))?;
    let manifest: RollbackManifest = serde_json::from_slice(&bytes)
        .map_err(|error| format!("rollback manifest is invalid: {error}"))?;
    let expected_version = if target == 13 { 2 } else { MANIFEST_VERSION };
    if manifest.manifest_version != expected_version
        || (target == 13
            && (manifest.target_schema_version != Some(13)
                || manifest.digest_algorithm.as_deref() != Some("blake3")))
        || manifest.source_schema_version >= target
    {
        return Err(format!(
            "rollback manifest version {} is not supported (expected {expected_version})",
            manifest.manifest_version
        )
        .into());
    }
    if manifest.database.relative_path != snapshot_for(target) {
        return Err("Invalid rollback database path".into());
    }
    let mut entries = vec![manifest.database.clone()];
    entries.extend(manifest.payloads.iter().cloned());
    let mut expected_paths = std::collections::BTreeSet::from([MANIFEST_NAME.to_string()]);
    let total = entries
        .iter()
        .try_fold(0u64, |n, file| n.checked_add(file.bytes))
        .ok_or("Rollback size overflow")?;
    if total != manifest.total_bytes {
        return Err("Rollback total size mismatch".into());
    }
    for entry in entries {
        if !expected_paths.insert(entry.relative_path.clone()) {
            return Err("Duplicate rollback file path".into());
        }
        let path = package.join(
            entry
                .relative_path
                .replace('/', std::path::MAIN_SEPARATOR_STR),
        );
        let relative = Path::new(&entry.relative_path);
        if relative.is_absolute()
            || relative
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return Err("Invalid rollback relative path".into());
        }
        if entry != manifest.database
            && !(entry.relative_path.starts_with("payloads/image-history/")
                || entry.relative_path.starts_with("payloads/file-history/"))
        {
            return Err("Invalid rollback payload path".into());
        }
        let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
            format!("rollback entry {} is missing: {error}", entry.relative_path)
        })?;
        if !metadata.is_file()
            || std::fs::canonicalize(&path)?
                .strip_prefix(std::fs::canonicalize(&package)?)
                .is_err()
        {
            return Err("Invalid rollback file type or location".into());
        }
        #[cfg(unix)]
        if permission_mode(&path) != entry.mode || entry.mode & 0o077 != 0 {
            return Err("Rollback permissions mismatch".into());
        }
        if metadata.len() != entry.bytes {
            return Err(format!(
                "rollback entry {} is {} bytes, manifest says {}",
                entry.relative_path,
                metadata.len(),
                entry.bytes
            )
            .into());
        }
        let actual = super::storage::hash_file(&path)?.to_hex().to_string();
        if actual != entry.sha256 {
            return Err(format!(
                "rollback entry {} does not match its recorded digest",
                entry.relative_path
            )
            .into());
        }
    }
    fn inventory(
        root: &Path,
        current: &Path,
        files: &mut std::collections::BTreeSet<String>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        for entry in std::fs::read_dir(current)? {
            let path = entry?.path();
            let kind = std::fs::symlink_metadata(&path)?;
            if kind.is_symlink() {
                return Err("Rollback tree contains a symbolic link".into());
            }
            if kind.is_dir() {
                inventory(root, &path, files)?;
            } else if kind.is_file() {
                files.insert(relative_string(root, &path)?);
            } else {
                return Err("Rollback tree contains a special file".into());
            }
        }
        Ok(())
    }
    let mut actual_paths = std::collections::BTreeSet::new();
    inventory(&package, &package, &mut actual_paths)?;
    if expected_paths != actual_paths {
        return Err("Rollback file inventory mismatch".into());
    }
    Ok(manifest)
}

/// Release the package. Only an explicit call frees the space.
pub fn retire(storage_dir: &Path) -> Result<bool, Box<dyn std::error::Error>> {
    let package = package_dir(storage_dir);
    if !package.exists() {
        return Ok(false);
    }
    std::fs::remove_dir_all(&package)?;
    Ok(true)
}

/// Restore a verified package into a **new** directory.
///
/// Restoring in place is deliberately not offered: the upgrade may already have
/// written new history, and the operator has to decide what to archive first (plan
/// §4 R1-03 step order). The target directory must not exist, so a restore can
/// never overwrite a live store.
///
/// `identity`, `trust` and `effect receipts` are **not** in the package and are
/// therefore never reverted — reverting them would make an old binary re-run system
/// writes whose receipts it had already lost.
pub fn restore_into(
    storage_dir: &Path,
    target_dir: &Path,
) -> Result<RollbackManifest, Box<dyn std::error::Error>> {
    restore_for(storage_dir, target_dir, 12)
}
pub fn restore_v13_into(
    storage_dir: &Path,
    target_dir: &Path,
) -> Result<RollbackManifest, Box<dyn std::error::Error>> {
    restore_for(storage_dir, target_dir, 13)
}
fn restore_for(
    storage_dir: &Path,
    target_dir: &Path,
    target_schema: i64,
) -> Result<RollbackManifest, Box<dyn std::error::Error>> {
    let manifest = verify_for(storage_dir, target_schema)?;
    if target_dir.exists() {
        return Err(format!(
            "refusing to restore: {} already exists",
            target_dir.display()
        )
        .into());
    }
    let package = directory_for(storage_dir, target_schema);
    crate::private_fs::create_private_dir_all(target_dir)?;
    crate::private_fs::create_private_dir_all(&target_dir.join("file-history"))?;
    crate::private_fs::create_private_dir_all(&target_dir.join("image-history"))?;

    let snapshot = package.join(snapshot_for(target_schema));
    let restored_db = target_dir.join("history-v2.db");
    super::storage::copy_file_verified(&snapshot, &restored_db)?;
    crate::private_fs::enforce_private_database(&restored_db)?;

    for entry in &manifest.payloads {
        let relative = entry
            .relative_path
            .replace('/', std::path::MAIN_SEPARATOR_STR);
        let source = package.join(&relative);
        // Manifest paths are package-relative (they include the `payloads/`
        // container so `verify` can hash them in place). The restored tree must
        // match the storage layout the reader expects, so the container is stripped.
        let container = format!("payloads{sep}", sep = std::path::MAIN_SEPARATOR_STR);
        let restored_relative = relative.strip_prefix(&container).unwrap_or(&relative);
        let target = target_dir.join(restored_relative);
        if let Some(parent) = target.parent() {
            crate::private_fs::create_private_dir_all(parent)?;
        }
        super::storage::copy_file_verified(&source, &target)?;
    }

    fsync_directory(&target_dir.join("file-history"))?;
    fsync_directory(&target_dir.join("image-history"))?;
    fsync_directory(target_dir)?;
    Ok(manifest)
}

/// Write the complete pre-upgrade package, or return the existing verified one.
///
/// Idempotent by design: if a verified package already exists it is returned
/// untouched, because it holds the only pre-upgrade state and a partially migrated
/// database must never overwrite it. An existing but *unverifiable* package is an
/// error, which stops the upgrade rather than proceeding without a rollback path.
pub fn prepare(
    conn: &Connection,
    db_path: &Path,
    storage_dir: &Path,
    file_history_dir: &Path,
    image_history_dir: &Path,
    source_schema_version: i64,
) -> Result<RollbackManifest, Box<dyn std::error::Error>> {
    prepare_for(
        conn,
        db_path,
        storage_dir,
        file_history_dir,
        image_history_dir,
        source_schema_version,
        12,
    )
}
#[allow(clippy::too_many_arguments)]
pub(super) fn prepare_for(
    conn: &Connection,
    db_path: &Path,
    storage_dir: &Path,
    file_history_dir: &Path,
    image_history_dir: &Path,
    source_schema_version: i64,
    target_schema: i64,
) -> Result<RollbackManifest, Box<dyn std::error::Error>> {
    let package = directory_for(storage_dir, target_schema);
    if package.join(MANIFEST_NAME).exists() {
        return verify_for(storage_dir, target_schema).map_err(|error| {
            format!(
                "refusing to upgrade: the existing rollback package is not verifiable ({error})"
            )
            .into()
        });
    }

    // Freeze payload writes/deletes for the whole snapshot.
    let _maintenance = super::storage::STORAGE_MAINTENANCE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let payloads = super::lifecycle::live_payload_paths(
        conn,
        file_history_dir,
        image_history_dir,
        schema::composite_schema_present(conn)?,
    )?;

    // Size the requirement before writing anything.
    let database_bytes = std::fs::metadata(db_path)
        .map(|meta| meta.len())
        .unwrap_or(0);
    let payload_bytes: u64 = payloads
        .iter()
        .filter_map(|path| std::fs::metadata(path).ok())
        .map(|meta| meta.len())
        .sum();
    let required = database_bytes
        .saturating_add(payload_bytes)
        .saturating_add(SPACE_MARGIN_BYTES);
    let available = fs2::available_space(storage_dir)?;
    if available < required {
        return Err(format!(
            "refusing to upgrade: the pre-upgrade rollback package needs {required} bytes \
             (database {database_bytes} + payloads {payload_bytes} + {SPACE_MARGIN_BYTES} margin) \
             but only {available} bytes are free"
        )
        .into());
    }

    crate::private_fs::create_private_dir_all(&package)?;
    let payload_root = package.join("payloads");
    crate::private_fs::create_private_dir_all(&payload_root)?;
    crate::private_fs::create_private_dir_all(&payload_root.join("image-history"))?;
    crate::private_fs::create_private_dir_all(&payload_root.join("file-history"))?;

    // 1) Consistent SQLite snapshot.
    let snapshot_name = snapshot_for(target_schema);
    let snapshot = package.join(&snapshot_name);
    let temporary = package.join(format!(
        ".{snapshot_name}.{:016x}.tmp",
        rand::random::<u64>()
    ));
    let file = crate::private_fs::create_private_file(&temporary)?;
    drop(file);
    if let Err(error) = conn.execute(
        "VACUUM INTO ?1",
        params![temporary.to_string_lossy().as_ref()],
    ) {
        let _ = std::fs::remove_file(&temporary);
        return Err(
            format!("refusing to upgrade: the pre-upgrade snapshot failed ({error})").into(),
        );
    }
    crate::private_fs::enforce_private_database(&temporary)?;
    std::fs::File::open(&temporary)?.sync_all()?;
    std::fs::rename(&temporary, &snapshot)?;

    // 2) Independent copies of every live payload.
    let mut entries = Vec::with_capacity(payloads.len());
    for source in &payloads {
        let bucket = if source.starts_with(image_history_dir) {
            "image-history"
        } else if source.starts_with(file_history_dir) {
            "file-history"
        } else {
            return Err(format!(
                "refusing to upgrade: live payload {} is outside the managed payload directories",
                source.display()
            )
            .into());
        };
        let name = source.file_name().ok_or("live payload has no file name")?;
        let target = payload_root.join(bucket).join(name);
        super::storage::copy_file_verified(source, &target)?;
        entries.push(describe(&package, &target)?);
    }
    entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));

    let database = describe(&package, &snapshot)?;
    let total_bytes = database
        .bytes
        .saturating_add(entries.iter().map(|entry| entry.bytes).sum::<u64>());
    let manifest = RollbackManifest {
        manifest_version: if target_schema == 13 {
            2
        } else {
            MANIFEST_VERSION
        },
        source_schema_version,
        target_schema_version: (target_schema == 13).then_some(13),
        digest_algorithm: (target_schema == 13).then(|| "blake3".into()),
        created_at: chrono::Utc::now().to_rfc3339(),
        database,
        payloads: entries,
        total_bytes,
        excludes: vec![
            "identity keys (kept outside the history tree; a restore must not revert them)"
                .to_string(),
            "trusted-peer and pairing state".to_string(),
            "effect receipts / journals".to_string(),
            "history written after the upgrade (archived separately)".to_string(),
        ],
    };
    let encoded = serde_json::to_vec_pretty(&manifest)?;
    crate::private_fs::write_private_file(&package.join(MANIFEST_NAME), &encoded)?;

    fsync_directory(&payload_root.join("image-history"))?;
    fsync_directory(&payload_root.join("file-history"))?;
    fsync_directory(&payload_root)?;
    fsync_directory(&package)?;
    fsync_directory(storage_dir)?;

    // 3) Do not proceed on an unverified package.
    verify_for(storage_dir, target_schema).map_err(|error| {
        format!("refusing to upgrade: the rollback package did not verify ({error})").into()
    })
}
