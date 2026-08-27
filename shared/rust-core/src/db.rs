use log::{info, warn};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use thiserror::Error;

use crate::crypto;
use crate::history_classifier::{self, Classification};

mod file_encryption;
mod file_storage;
mod legacy_v1;
mod lifecycle;
mod migrations;
mod paths;
mod queries;
mod schema;
mod storage;
mod types;

pub use file_storage::{
    cleanup_clipboard_files, materialize_clipboard_bytes, materialize_clipboard_file,
    materialize_remote_clipboard_file,
};
use file_storage::{
    decode_file_reference, decode_image_reference, encode_file_reference_version,
    encode_image_reference, materialize_clipboard_file_at, persist_history_file_at,
    persist_history_file_from_path_at, persist_image_at, resolve_file_reference_at,
    validate_history_file_size,
};
#[cfg(test)]
use file_storage::{
    materialize_clipboard_bytes_at, materialize_remote_clipboard_file_at, StoredFileReference,
    FILE_HISTORY_BYTE_LIMIT,
};
pub use paths::{
    configure_storage_dir, configure_storage_parent, get_clipboard_files_dir, get_data_dir,
    get_file_history_dir, get_history_db_path, get_image_history_dir, get_incoming_dir,
    get_storage_dir, validate_storage_dir, STORAGE_DIRECTORY_NAME,
};
pub use storage::{
    delete_old_storage, migrate_storage_with_rollback, StorageMigrationFailure,
    StorageMigrationHooks,
};
pub use types::{
    FileEncryptionMigrationBatch, HistoryEntry, HistoryFileInput, HistoryQueryPage,
    MigrationDiagnostics, MigrationIssue, PreviewBatchNavigation, PreviewErrorCode,
    PreviewErrorInfo, PreviewKind, PreviewMetadata, PreviewPayload, StorageMigrationResult,
    StorageStatus,
};

/// Database schema version
const SCHEMA_VERSION: i64 = 9;

/// Maximum amount of decrypted data a preview request may materialise.
pub const PREVIEW_MAX_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PreviewError {
    #[error("history entry {entry_id} is unavailable")]
    EntryNotFound { entry_id: i64 },
    #[error("history batch {batch_id} is unavailable")]
    BatchNotFound { batch_id: String },
    #[error("history entry {entry_id} does not belong to batch {batch_id}")]
    EntryNotInBatch { entry_id: i64, batch_id: String },
    #[error("preview metadata for history entry {entry_id} is unavailable: {reason}")]
    MetadataUnavailable { entry_id: i64, reason: String },
    #[error("preview payload for history entry {entry_id} is unavailable: {reason}")]
    PayloadUnavailable { entry_id: i64, reason: String },
    #[error("preview payload is too large: {size} bytes (limit {limit} bytes)")]
    PreviewTooLarge { size: u64, limit: u64 },
    #[error("unsupported history preview type: {kind}")]
    UnsupportedType { kind: String },
    #[error("history entry has an invalid size: {size}")]
    InvalidSize { size: i64 },
}

impl PreviewError {
    pub const fn code(&self) -> PreviewErrorCode {
        match self {
            Self::EntryNotFound { .. } => PreviewErrorCode::EntryNotFound,
            Self::BatchNotFound { .. } => PreviewErrorCode::BatchNotFound,
            Self::EntryNotInBatch { .. } => PreviewErrorCode::EntryNotInBatch,
            Self::MetadataUnavailable { .. } => PreviewErrorCode::MetadataUnavailable,
            Self::PayloadUnavailable { .. } => PreviewErrorCode::PayloadUnavailable,
            Self::PreviewTooLarge { .. } => PreviewErrorCode::PreviewTooLarge,
            Self::UnsupportedType { .. } => PreviewErrorCode::UnsupportedType,
            Self::InvalidSize { .. } => PreviewErrorCode::InvalidSize,
        }
    }
}

impl From<PreviewError> for PreviewErrorInfo {
    fn from(error: PreviewError) -> Self {
        let (entry_id, size_bytes, limit_bytes, retryable) = match &error {
            PreviewError::EntryNotFound { entry_id }
            | PreviewError::MetadataUnavailable { entry_id, .. }
            | PreviewError::PayloadUnavailable { entry_id, .. }
            | PreviewError::EntryNotInBatch { entry_id, .. } => (
                Some(*entry_id),
                None,
                None,
                matches!(
                    &error,
                    PreviewError::MetadataUnavailable { .. }
                        | PreviewError::PayloadUnavailable { .. }
                ),
            ),
            PreviewError::PreviewTooLarge { size, limit } => {
                (None, Some(*size), Some(*limit), false)
            }
            PreviewError::BatchNotFound { .. }
            | PreviewError::UnsupportedType { .. }
            | PreviewError::InvalidSize { .. } => (None, None, None, false),
        };
        Self {
            code: error.code(),
            message: error.to_string(),
            entry_id,
            size_bytes,
            limit_bytes,
            retryable,
        }
    }
}

impl PreviewErrorInfo {
    pub fn payload_unavailable(entry_id: i64, message: impl Into<String>) -> Self {
        Self {
            code: PreviewErrorCode::PayloadUnavailable,
            message: message.into(),
            entry_id: Some(entry_id),
            size_bytes: None,
            limit_bytes: None,
            retryable: false,
        }
    }
}

const TEXT_DESCRIPTION_PLACEHOLDER: &str = "Encrypted text";

fn text_preview(text: &str) -> String {
    text.chars().take(100).collect()
}

fn preview_kind_and_name(
    entry_type: &str,
    description: &str,
) -> Result<(PreviewKind, String), PreviewError> {
    match entry_type {
        "text" => Ok((PreviewKind::Text, "text.txt".to_string())),
        "image" => Ok((PreviewKind::Image, "image".to_string())),
        "file" => Ok((PreviewKind::File, description.to_string())),
        other => Err(PreviewError::UnsupportedType {
            kind: other.to_string(),
        }),
    }
}

fn checked_preview_size(size: i64) -> Result<u64, PreviewError> {
    let size = u64::try_from(size).map_err(|_| PreviewError::InvalidSize { size })?;
    if size > PREVIEW_MAX_BYTES {
        return Err(PreviewError::PreviewTooLarge {
            size,
            limit: PREVIEW_MAX_BYTES,
        });
    }
    Ok(size)
}

fn remove_unreferenced_persisted_files(conn: &Connection, persisted: &[(Vec<u8>, PathBuf)]) {
    for (reference, path) in persisted {
        let remaining = conn.query_row(
            "SELECT COUNT(*) FROM history WHERE data = ?1",
            params![reference],
            |row| row.get::<_, i64>(0),
        );
        if matches!(remaining, Ok(0)) {
            if let Err(error) = std::fs::remove_file(path) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    warn!(
                        "Could not remove unreferenced batch file {}: {error}",
                        path.display()
                    );
                }
            }
        }
    }
}

pub struct HistoryDB {
    conn: Connection,
    max_history: i64,
    storage_quota_bytes: u64,
    storage_available: bool,
    file_history_dir: PathBuf,
    image_history_dir: PathBuf,
}

impl HistoryDB {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        Self::open_at(&get_storage_dir())
    }

    pub fn new_unavailable() -> Result<Self, Box<dyn std::error::Error>> {
        let conn = Connection::open_in_memory()?;
        schema::initialize(&conn)?;
        Self::migrate(&conn, &get_file_history_dir(), &get_image_history_dir())?;
        Ok(Self {
            conn,
            max_history: 1000,
            storage_quota_bytes: crypto::DEFAULT_STORAGE_QUOTA_BYTES,
            storage_available: false,
            file_history_dir: get_file_history_dir(),
            image_history_dir: get_image_history_dir(),
        })
    }

    pub(crate) fn open_at(storage_dir: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        // The storage tree may predate the owner-only permission policy
        // (or hold files SQLite created under the process umask): repair it
        // on every open. App-created directories are locked by
        // create_private_dir_all; user-selected roots are left to
        // enforce_private_tree, which only touches app-managed content.
        crate::private_fs::create_private_dir_all(storage_dir)?;
        crate::private_fs::enforce_private_tree(storage_dir)?;
        let db_path = storage_dir.join("history-v2.db");
        info!("Opening database at {}", db_path.display());

        let conn = Connection::open(&db_path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let file_history_dir = storage_dir.join("file-history");
        let image_history_dir = storage_dir.join("image-history");

        // Enable WAL mode for concurrent read/write. secure_delete overwrites
        // deleted cells in the main database; explicit deletes also truncate WAL.
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA secure_delete = ON;")?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;

        schema::initialize(&conn)?;

        // SQLite creates -wal/-shm under the umask; lock them down now that
        // they exist (and again on every later open via enforce_private_tree).
        crate::private_fs::enforce_private_database(&db_path)?;

        // Run migrations
        Self::migrate(&conn, &file_history_dir, &image_history_dir)?;

        // Resumable transfers and clipboard materializations deliberately
        // survive restarts. Expired unreferenced files are cleaned separately.
        let incoming = storage_dir.join("incoming");
        let clipboard_files = storage_dir.join("clipboard-files");
        crate::private_fs::create_private_dir_all(&incoming)?;
        crate::private_fs::create_private_dir_all(&clipboard_files)?;
        crate::private_fs::create_private_dir_all(&file_history_dir)?;
        crate::private_fs::create_private_dir_all(&image_history_dir)?;

        let mut database = HistoryDB {
            conn,
            max_history: i64::MAX / 2,
            storage_quota_bytes: crypto::DEFAULT_STORAGE_QUOTA_BYTES,
            storage_available: true,
            file_history_dir,
            image_history_dir,
        };
        if let Err(error) = database.migrate_legacy_v1_if_present() {
            warn!("Could not automatically migrate TailSync v1 history: {error}");
        }
        database.max_history = 1000;
        Ok(database)
    }

    /// Update history limit from settings.
    pub fn set_max_history(&mut self, limit: i64) {
        self.max_history = limit;
    }

    pub fn set_storage_quota(&mut self, quota_bytes: u64) {
        self.storage_quota_bytes = quota_bytes;
    }

    pub fn reopen_configured_storage(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut replacement = Self::open_at(&get_storage_dir())?;
        replacement.max_history = self.max_history;
        replacement.storage_quota_bytes = self.storage_quota_bytes;
        *self = replacement;
        Ok(())
    }

    pub fn mark_storage_unavailable(&mut self) {
        self.storage_available = false;
    }

    /// Add a text entry to history. Duplicate: delete old, insert new at top.
    pub fn add_text(
        &mut self,
        text: &str,
        source_peer: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let data_hash = blake3::hash(text.as_bytes()).to_hex().to_string();
        // Delete old entry with same hash so the new one is at the top
        let duplicate_ids = self.entry_ids_by_hash(&data_hash)?;
        self.delete_entries(&duplicate_ids)?;

        let encrypted = crypto::encrypt(text.as_bytes())?;
        let timestamp = chrono::Utc::now().to_rfc3339();
        let classification = history_classifier::classify_text(text);
        let categories = serde_json::to_string(&classification.categories())?;

        self.conn.execute(
            "INSERT INTO history
                (timestamp, type, description, data, size_bytes, source_peer, data_hash,
                 category, categories, category_confidence, classifier_version)
             VALUES (?1, 'text', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                timestamp,
                TEXT_DESCRIPTION_PLACEHOLDER,
                encrypted,
                text.len() as i64,
                source_peer,
                data_hash,
                classification.category,
                categories,
                classification.confidence as i64,
                history_classifier::CLASSIFIER_VERSION,
            ],
        )?;

        self.trim("text")?;
        Ok(())
    }

    /// Add an image entry to history. Duplicate: delete old, insert new.
    pub fn add_image(
        &mut self,
        image_data: &[u8],
        source_peer: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        crate::protocol::PackedImage::try_from(image_data)?;
        let data_hash = blake3::hash(image_data).to_hex().to_string();
        // Delete old entry so the new copy appears at the top
        let duplicate_ids = self.entry_ids_by_hash(&data_hash)?;
        self.delete_entries(&duplicate_ids)?;

        let reference = persist_image_at(&self.image_history_dir, &data_hash, image_data)?;
        let timestamp = chrono::Utc::now().to_rfc3339();
        let description = format!("Image {} bytes", image_data.len());

        self.conn.execute(
            "INSERT INTO history
                (timestamp, type, description, data, size_bytes, source_peer, data_hash,
                 category, categories, category_confidence, classifier_version)
             VALUES (?1, 'image', ?2, ?3, ?4, ?5, ?6, 'image', '[\"image\"]', 100, ?7)",
            params![
                timestamp,
                description,
                reference,
                image_data.len() as i64,
                source_peer,
                data_hash,
                history_classifier::CLASSIFIER_VERSION,
            ],
        )?;

        self.trim("image")?;
        Ok(())
    }

    /// Check if an entry with the given hash already exists
    fn exists_by_hash(&self, hash: &str) -> Result<bool, Box<dyn std::error::Error>> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM history WHERE data_hash = ?1",
            params![hash],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Get history entries with optional keyword search
    /// Get entry bytes. File entries backed by the history folder are read
    /// from disk; legacy file blobs remain readable through decryption.
    pub fn get_data(&self, id: i64) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let (entry_type, stored): (String, Vec<u8>) = self.conn.query_row(
            "SELECT type, data FROM history WHERE id = ?1",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if entry_type == "text" {
            return self.read_text_payload_compat(&stored);
        }
        if entry_type == "file" {
            if let Some(reference) = decode_file_reference(&stored) {
                let path = resolve_file_reference_at(&self.file_history_dir, &reference)?;
                if reference.version == 2 || file_encryption::is_encrypted_file(&path)? {
                    return file_encryption::decrypt_file_to_vec(&path);
                }
                return Ok(std::fs::read(path)?);
            }
        }
        if entry_type == "image" {
            if let Some(reference) = decode_image_reference(&stored) {
                let encrypted = std::fs::read(resolve_file_reference_at(
                    &self.image_history_dir,
                    &reference,
                )?)?;
                return crypto::decrypt(&encrypted);
            }
        }
        crypto::decrypt(&stored)
    }

    /// Read the renderer and navigation metadata without decrypting or
    /// materialising the entry payload.
    pub fn get_preview_metadata(&self, id: i64) -> Result<PreviewMetadata, PreviewError> {
        let row = self
            .conn
            .query_row(
                "SELECT type, description, size_bytes, batch_id
                 FROM history WHERE id = ?1",
                params![id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| PreviewError::MetadataUnavailable {
                entry_id: id,
                reason: error.to_string(),
            })?
            .ok_or(PreviewError::EntryNotFound { entry_id: id })?;

        let (entry_type, description, declared_size, batch_id) = row;
        let size_bytes = checked_preview_size(declared_size)?;
        let (kind, name) = preview_kind_and_name(&entry_type, &description)?;
        let batch = batch_id
            .as_deref()
            .map(|batch_id| self.get_preview_batch_navigation(batch_id, id))
            .transpose()?;

        Ok(PreviewMetadata {
            entry_id: id,
            kind,
            name,
            size_bytes,
            batch,
        })
    }

    /// Return navigation for one file in a batch without loading any payload.
    pub fn get_preview_batch_navigation(
        &self,
        batch_id: &str,
        entry_id: i64,
    ) -> Result<PreviewBatchNavigation, PreviewError> {
        let load_ids = || -> rusqlite::Result<Vec<i64>> {
            self.conn
                .prepare(
                    "SELECT id FROM history
                     WHERE batch_id = ?1 AND type = 'file'
                     ORDER BY batch_index ASC, id ASC",
                )?
                .query_map(params![batch_id], |row| row.get(0))?
                .collect()
        };
        let entry_ids = load_ids().map_err(|error| PreviewError::MetadataUnavailable {
            entry_id,
            reason: error.to_string(),
        })?;
        if entry_ids.is_empty() {
            return Err(PreviewError::BatchNotFound {
                batch_id: batch_id.to_string(),
            });
        }
        let item_index = entry_ids
            .iter()
            .position(|candidate| *candidate == entry_id)
            .ok_or_else(|| PreviewError::EntryNotInBatch {
                entry_id,
                batch_id: batch_id.to_string(),
            })?;

        Ok(PreviewBatchNavigation {
            batch_id: batch_id.to_string(),
            item_index,
            item_count: entry_ids.len(),
            first_entry_id: entry_ids[0],
            last_entry_id: entry_ids[entry_ids.len() - 1],
            previous_entry_id: item_index
                .checked_sub(1)
                .and_then(|index| entry_ids.get(index).copied()),
            next_entry_id: entry_ids.get(item_index + 1).copied(),
        })
    }

    /// Load a bounded, decrypted payload for an in-memory history preview.
    ///
    /// This deliberately does not call `get_file_path` or any materialisation
    /// helper: preview callers must never create a plaintext file on disk.
    /// The typed error is part of the platform IPC contract; callers should
    /// never have to infer retryability from an error string.
    pub fn get_preview_payload(&self, id: i64) -> Result<PreviewPayload, PreviewError> {
        let metadata = self.get_preview_metadata(id)?;
        let data = self
            .get_data(id)
            .map_err(|error| PreviewError::PayloadUnavailable {
                entry_id: id,
                reason: error.to_string(),
            })?;
        let size_bytes = u64::try_from(data.len()).unwrap_or(u64::MAX);
        if size_bytes > PREVIEW_MAX_BYTES {
            return Err(PreviewError::PreviewTooLarge {
                size: size_bytes,
                limit: PREVIEW_MAX_BYTES,
            });
        }

        Ok(PreviewPayload {
            kind: metadata.kind.as_str().to_string(),
            name: metadata.name,
            size_bytes,
            data,
        })
    }

    /// Reads current encrypted text and the short-lived v3 format that stored
    /// migrated text payloads behind an image-reference envelope.
    fn read_text_payload_compat(
        &self,
        stored: &[u8],
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        if let Some(reference) = decode_image_reference(stored) {
            let encrypted = std::fs::read(resolve_file_reference_at(
                &self.image_history_dir,
                &reference,
            )?)?;
            return crypto::decrypt(&encrypted);
        }
        crypto::decrypt(stored)
    }

    pub fn backfill_classifications(
        &mut self,
        limit: usize,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        let media_updated = self.conn.execute(
            "UPDATE history
             SET category = type,
                 categories = json_array(type),
                 category_confidence = 100,
                 classifier_version = ?1
             WHERE type IN ('image', 'file')
               AND (classifier_version < ?1 OR category != type
                    OR categories != json_array(type) OR category_confidence != 100)",
            params![history_classifier::CLASSIFIER_VERSION],
        )?;
        let rows = self
            .conn
            .prepare(
                "SELECT id, data FROM history
                 WHERE type = 'text' AND classifier_version < ?1
                 ORDER BY id ASC LIMIT ?2",
            )?
            .query_map(
                params![history_classifier::CLASSIFIER_VERSION, limit as i64],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?)),
            )?
            .collect::<Result<Vec<_>, _>>()?;
        if rows.is_empty() {
            return Ok(media_updated);
        }

        let mut updates = Vec::with_capacity(rows.len());
        for (id, stored) in rows {
            let legacy_reference = decode_image_reference(&stored);
            let (classification, replacement) = match self.read_text_payload_compat(&stored) {
                Ok(data) => match std::str::from_utf8(&data) {
                    Ok(text) => (
                        history_classifier::classify_text(text),
                        if legacy_reference.is_some() {
                            Some(crypto::encrypt(&data)?)
                        } else {
                            None
                        },
                    ),
                    Err(error) => {
                        warn!("History text entry {id} is not UTF-8: {error}");
                        (
                            Classification {
                                category: "text",
                                confidence: 0,
                                secondary_category: None,
                            },
                            None,
                        )
                    }
                },
                Err(error) => {
                    if error
                        .downcast_ref::<std::io::Error>()
                        .is_some_and(|io_error| io_error.kind() != std::io::ErrorKind::NotFound)
                    {
                        return Err(error);
                    }
                    warn!("History text entry {id} could not be classified: {error}");
                    (
                        Classification {
                            category: "text",
                            confidence: 0,
                            secondary_category: None,
                        },
                        None,
                    )
                }
            };
            updates.push((id, classification, replacement, legacy_reference));
        }

        let tx = self.conn.transaction()?;
        for (id, classification, replacement, _) in &updates {
            let categories = serde_json::to_string(&classification.categories())?;
            if let Some(encrypted) = replacement {
                tx.execute(
                    "UPDATE history
                     SET data = ?1, category = ?2, categories = ?3,
                         category_confidence = ?4, classifier_version = ?5
                     WHERE id = ?6",
                    params![
                        encrypted,
                        classification.category,
                        categories,
                        classification.confidence as i64,
                        history_classifier::CLASSIFIER_VERSION,
                        id,
                    ],
                )?;
            } else {
                tx.execute(
                    "UPDATE history
                     SET category = ?1, categories = ?2,
                         category_confidence = ?3, classifier_version = ?4
                     WHERE id = ?5",
                    params![
                        classification.category,
                        categories,
                        classification.confidence as i64,
                        history_classifier::CLASSIFIER_VERSION,
                        id,
                    ],
                )?;
            }
        }
        tx.commit()?;

        for (_, _, replacement, reference) in &updates {
            if replacement.is_none() {
                continue;
            }
            if let Some(reference) = reference {
                let path = resolve_file_reference_at(&self.image_history_dir, reference)?;
                let remaining: i64 = self.conn.query_row(
                    "SELECT COUNT(*) FROM history WHERE data = ?1",
                    params![encode_image_reference(&reference.file_name)?],
                    |row| row.get(0),
                )?;
                if remaining == 0 {
                    if let Err(error) = std::fs::remove_file(&path) {
                        if error.kind() != std::io::ErrorKind::NotFound {
                            warn!(
                                "Could not remove migrated text reference {}: {error}",
                                path.display()
                            );
                        }
                    }
                }
            }
        }
        Ok(media_updated + updates.len())
    }

    /// Return the on-disk path for a folder-backed file history entry.
    /// Legacy BLOB entries return None and continue through get_data().
    pub fn get_file_path(&self, id: i64) -> Result<Option<PathBuf>, Box<dyn std::error::Error>> {
        let (entry_type, description, stored): (String, String, Vec<u8>) = self.conn.query_row(
            "SELECT type, description, data FROM history WHERE id = ?1",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        if entry_type != "file" {
            return Ok(None);
        }
        let Some(reference) = decode_file_reference(&stored) else {
            return Ok(None);
        };
        let path = resolve_file_reference_at(&self.file_history_dir, &reference)?;
        if !path.is_file() {
            return Err(format!("History file is missing: {}", path.display()).into());
        }
        let clipboard_files_dir = self
            .file_history_dir
            .parent()
            .map(|parent| parent.join("clipboard-files"))
            .unwrap_or_else(get_clipboard_files_dir);
        Ok(Some(materialize_clipboard_file_at(
            &clipboard_files_dir,
            &path,
            &description,
        )?))
    }

    /// Get the entry description for an entry (filename for file entries).
    pub fn get_description(&self, id: i64) -> Result<String, Box<dyn std::error::Error>> {
        let (entry_type, description, stored): (String, String, Vec<u8>) = self.conn.query_row(
            "SELECT type, description, data FROM history WHERE id = ?1",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        if entry_type == "text" {
            let plaintext = self.read_text_payload_compat(&stored)?;
            return Ok(text_preview(std::str::from_utf8(&plaintext)?));
        }
        Ok(description)
    }

    /// Get the entry type ("text" | "image" | "file") for an entry.
    pub fn get_type(&self, id: i64) -> Result<String, Box<dyn std::error::Error>> {
        let t: String = self.conn.query_row(
            "SELECT type FROM history WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )?;
        Ok(t)
    }

    pub fn materialize_file_batch(
        &self,
        batch_id: &str,
    ) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
        let rows = self
            .conn
            .prepare(
                "SELECT id, description, batch_total, batch_status
                 FROM history WHERE batch_id = ?1 AND type = 'file'
                 ORDER BY batch_index ASC, id ASC",
            )?
            .query_map(params![batch_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let expected = rows.first().map(|row| row.2).unwrap_or(0);
        if expected <= 0
            || rows.len() != expected as usize
            || rows.iter().any(|row| row.3 != "complete")
        {
            return Err("Only a complete file batch can be copied as a group".into());
        }
        rows.into_iter()
            .map(|(id, name, _, _)| {
                let source = self
                    .get_file_path(id)?
                    .ok_or("File batch entry is not backed by a history file")?;
                materialize_clipboard_file_at(&get_clipboard_files_dir(), &source, &name)
            })
            .collect()
    }

    /// Add a file entry to history
    pub fn add_file(
        &mut self,
        name: &str,
        data: &[u8],
        source_peer: &str,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        validate_history_file_size(data.len() as u64)?;
        let data_hash = blake3::hash(data).to_hex().to_string();
        let (reference, file_path) =
            persist_history_file_at(&self.file_history_dir, &data_hash, name, data)?;
        let duplicate_ids = self.entry_ids_by_hash(&data_hash)?;
        self.delete_entries_except(&duplicate_ids, Some(&file_path))?;
        let timestamp = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO history
                (timestamp, type, description, data, size_bytes, source_peer, data_hash,
                 category, categories, category_confidence, classifier_version)
             VALUES (?1, 'file', ?2, ?3, ?4, ?5, ?6, 'file', '[\"file\"]', 100, ?7)",
            params![
                timestamp,
                name,
                reference,
                data.len() as i64,
                source_peer,
                data_hash,
                history_classifier::CLASSIFIER_VERSION,
            ],
        )?;
        self.trim("file")?;
        Ok(file_path)
    }

    pub fn add_file_from_path(
        &mut self,
        name: &str,
        source: &Path,
        data_hash: &str,
        size: u64,
        source_peer: &str,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        self.add_file_path_inner(name, source, data_hash, size, source_peer, false)
    }

    pub fn adopt_file(
        &mut self,
        name: &str,
        source: &Path,
        data_hash: &str,
        size: u64,
        source_peer: &str,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        self.add_file_path_inner(name, source, data_hash, size, source_peer, true)
    }

    fn add_file_path_inner(
        &mut self,
        name: &str,
        source: &Path,
        data_hash: &str,
        size: u64,
        source_peer: &str,
        move_source: bool,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        validate_history_file_size(size)?;
        let (reference, file_path) = persist_history_file_from_path_at(
            &self.file_history_dir,
            data_hash,
            name,
            source,
            size,
            move_source,
        )?;
        let duplicate_ids = self.entry_ids_by_hash(data_hash)?;
        self.delete_entries_except(&duplicate_ids, Some(&file_path))?;
        let timestamp = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO history
                (timestamp, type, description, data, size_bytes, source_peer, data_hash,
                 category, categories, category_confidence, classifier_version)
             VALUES (?1, 'file', ?2, ?3, ?4, ?5, ?6, 'file', '[\"file\"]', 100, ?7)",
            params![
                timestamp,
                name,
                reference,
                size as i64,
                source_peer,
                data_hash,
                history_classifier::CLASSIFIER_VERSION,
            ],
        )?;
        self.trim("file")?;
        Ok(file_path)
    }

    /// Persist a batch as one history group. Entries are visible as incomplete
    /// while files are being adopted, so a partial failure remains recoverable
    /// without exposing a misleading "Copy all" action.
    pub fn add_file_batch(
        &mut self,
        batch_id: &str,
        files: &[HistoryFileInput],
        source_peer: &str,
        move_sources: bool,
    ) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
        self.add_file_batch_with_status(
            batch_id,
            files,
            files.len(),
            source_peer,
            move_sources,
            true,
        )
    }

    pub fn add_file_batch_with_status(
        &mut self,
        batch_id: &str,
        files: &[HistoryFileInput],
        expected_total: usize,
        source_peer: &str,
        move_sources: bool,
        complete: bool,
    ) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
        if files.is_empty() {
            return Err("A file batch cannot be empty".into());
        }
        if expected_total < files.len() {
            return Err("File batch total cannot be smaller than the completed file count".into());
        }
        let total = i64::try_from(expected_total).map_err(|_| "File batch is too large")?;
        let timestamp = chrono::Utc::now().to_rfc3339();
        let mut persisted = Vec::with_capacity(files.len());
        for file in files {
            validate_history_file_size(file.size)?;
            let persisted_file = persist_history_file_from_path_at(
                &self.file_history_dir,
                &file.data_hash,
                &file.name,
                &file.path,
                file.size,
                false,
            );
            match persisted_file {
                Ok(value) => persisted.push(value),
                Err(error) => {
                    remove_unreferenced_persisted_files(&self.conn, &persisted);
                    return Err(error);
                }
            }
        }

        let write_result = (|| -> Result<(), Box<dyn std::error::Error>> {
            let tx = self.conn.transaction()?;
            for (index, (file, (reference, _))) in files.iter().zip(&persisted).enumerate() {
                tx.execute(
                    "INSERT INTO history
                        (timestamp, type, description, data, size_bytes, source_peer, data_hash,
                         category, categories, category_confidence, classifier_version,
                         batch_id, batch_index, batch_total, batch_status)
                     VALUES (?1, 'file', ?2, ?3, ?4, ?5, ?6, 'file', '[\"file\"]', 100, ?7,
                             ?8, ?9, ?10, 'incomplete')",
                    params![
                        timestamp,
                        file.name,
                        reference,
                        file.size as i64,
                        source_peer,
                        file.data_hash,
                        history_classifier::CLASSIFIER_VERSION,
                        batch_id,
                        index as i64,
                        total,
                    ],
                )?;
            }
            if complete {
                tx.execute(
                    "UPDATE history SET batch_status = 'complete' WHERE batch_id = ?1",
                    params![batch_id],
                )?;
            }
            tx.commit()?;
            Ok(())
        })();
        if let Err(error) = write_result {
            remove_unreferenced_persisted_files(&self.conn, &persisted);
            return Err(error);
        }

        if move_sources {
            for (file, (_, stored_path)) in files.iter().zip(&persisted) {
                if file.path != *stored_path {
                    if let Err(error) = std::fs::remove_file(&file.path) {
                        if error.kind() != std::io::ErrorKind::NotFound {
                            warn!(
                                "Could not remove adopted batch source {}: {error}",
                                file.path.display()
                            );
                        }
                    }
                }
            }
        }
        self.trim("file")?;
        Ok(persisted.into_iter().map(|(_, path)| path).collect())
    }

    // ── Migration inserts (pre-set timestamp + description) ──────

    pub fn add_text_migrated(
        &mut self,
        time: &str,
        _desc: &str,
        data: &[u8],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let data_hash = blake3::hash(data).to_hex().to_string();
        if self.exists_by_hash(&data_hash)? {
            return Ok(());
        }
        let encrypted = crypto::encrypt(data)?;
        let classification = std::str::from_utf8(data)
            .map(history_classifier::classify_text)
            .unwrap_or(Classification {
                category: "text",
                confidence: 0,
                secondary_category: None,
            });
        let categories = serde_json::to_string(&classification.categories())?;
        self.conn.execute(
            "INSERT INTO history
                (timestamp, type, description, data, size_bytes, source_peer, data_hash,
                 category, categories, category_confidence, classifier_version)
             VALUES (?1, 'text', ?2, ?3, ?4, 'migrated', ?5, ?6, ?7, ?8, ?9)",
            params![
                time,
                TEXT_DESCRIPTION_PLACEHOLDER,
                encrypted,
                data.len() as i64,
                data_hash,
                classification.category,
                categories,
                classification.confidence as i64,
                history_classifier::CLASSIFIER_VERSION,
            ],
        )?;
        self.trim("text")?;
        Ok(())
    }

    pub fn add_image_migrated(
        &mut self,
        time: &str,
        desc: &str,
        data: &[u8],
    ) -> Result<(), Box<dyn std::error::Error>> {
        crate::protocol::PackedImage::try_from(data)?;
        let data_hash = blake3::hash(data).to_hex().to_string();
        if self.exists_by_hash(&data_hash)? {
            return Ok(());
        }
        let encrypted = crypto::encrypt(data)?;
        self.conn.execute(
            "INSERT INTO history
                (timestamp, type, description, data, size_bytes, source_peer, data_hash,
                 category, categories, category_confidence, classifier_version)
             VALUES (?1, 'image', ?2, ?3, ?4, 'migrated', ?5, 'image', '[\"image\"]', 100, ?6)",
            params![
                time,
                desc,
                encrypted,
                data.len() as i64,
                data_hash,
                history_classifier::CLASSIFIER_VERSION,
            ],
        )?;
        self.trim("image")?;
        Ok(())
    }

    pub fn add_file_migrated(
        &mut self,
        time: &str,
        desc: &str,
        data: &[u8],
    ) -> Result<(), Box<dyn std::error::Error>> {
        validate_history_file_size(data.len() as u64)?;
        let data_hash = blake3::hash(data).to_hex().to_string();
        if self.exists_by_hash(&data_hash)? {
            return Ok(());
        }
        let (reference, _) =
            persist_history_file_at(&self.file_history_dir, &data_hash, desc, data)?;
        self.conn.execute(
            "INSERT INTO history
                (timestamp, type, description, data, size_bytes, source_peer, data_hash,
                 category, categories, category_confidence, classifier_version)
             VALUES (?1, 'file', ?2, ?3, ?4, 'migrated', ?5, 'file', '[\"file\"]', 100, ?6)",
            params![
                time,
                desc,
                reference,
                data.len() as i64,
                data_hash,
                history_classifier::CLASSIFIER_VERSION,
            ],
        )?;
        self.trim("file")?;
        Ok(())
    }

    pub fn add_file_migrated_from_path(
        &mut self,
        time: &str,
        desc: &str,
        source: &Path,
        data_hash: &str,
        size: u64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        validate_history_file_size(size)?;
        if self.exists_by_hash(data_hash)? {
            return Ok(());
        }
        let (reference, _) = persist_history_file_from_path_at(
            &self.file_history_dir,
            data_hash,
            desc,
            source,
            size,
            true,
        )?;
        self.conn.execute(
            "INSERT INTO history
                (timestamp, type, description, data, size_bytes, source_peer, data_hash,
                 category, categories, category_confidence, classifier_version)
             VALUES (?1, 'file', ?2, ?3, ?4, 'migrated', ?5, 'file', '[\"file\"]', 100, ?6)",
            params![
                time,
                desc,
                reference,
                size as i64,
                data_hash,
                history_classifier::CLASSIFIER_VERSION,
            ],
        )?;
        self.trim("file")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packed_test_image(width: u32, height: u32, channel: u8) -> Vec<u8> {
        let length = 8 + width as usize * height as usize * 4;
        let mut data = Vec::with_capacity(length);
        data.extend_from_slice(&width.to_le_bytes());
        data.extend_from_slice(&height.to_le_bytes());
        data.resize(length, channel);
        data
    }

    fn test_database(root: &Path) -> HistoryDB {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                type TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                data BLOB NOT NULL,
                size_bytes INTEGER NOT NULL DEFAULT 0,
                source_peer TEXT NOT NULL DEFAULT '',
                data_hash TEXT NOT NULL DEFAULT '',
                category TEXT NOT NULL DEFAULT 'text',
                categories TEXT NOT NULL DEFAULT '[]',
                category_confidence INTEGER NOT NULL DEFAULT 0,
                classifier_version INTEGER NOT NULL DEFAULT 0,
                pinned INTEGER NOT NULL DEFAULT 0,
                batch_id TEXT,
                batch_index INTEGER,
                batch_total INTEGER,
                batch_status TEXT NOT NULL DEFAULT 'complete'
            );",
        )
        .unwrap();
        HistoryDB {
            conn,
            max_history: 100,
            storage_quota_bytes: crypto::DEFAULT_STORAGE_QUOTA_BYTES,
            storage_available: true,
            file_history_dir: root.join("file-history"),
            image_history_dir: root.join("image-history"),
        }
    }

    #[test]
    fn clipboard_bytes_are_materialized_below_the_controlled_directory() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-clipboard-bytes-{:016x}",
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&root).unwrap();
        for (untrusted_name, expected_name) in [
            (r"C:\outside\..\report.txt", "report.txt"),
            (r"\\server\share\private.pdf", "private.pdf"),
            ("../../escape.bin", "escape.bin"),
            ("/etc/passwd", "passwd"),
        ] {
            let path =
                materialize_clipboard_bytes_at(&root, b"legacy file", untrusted_name).unwrap();
            assert!(path.starts_with(&root));
            assert_eq!(
                path.file_name().and_then(|name| name.to_str()),
                Some(expected_name)
            );
            assert_eq!(std::fs::read(&path).unwrap(), b"legacy file");
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn malformed_images_are_rejected_before_database_insert() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-invalid-image-{:016x}",
            rand::random::<u64>()
        ));
        let mut db = test_database(&root);
        assert!(db
            .add_image_migrated("2026-01-01T00:00:00Z", "bad", &[0; 8])
            .is_err());
        let count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM history", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn preview_payload_loads_text_image_and_file_bytes() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-preview-payload-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let mut db = test_database(&root);

        let text = "preview text payload";
        db.add_text(text, "self").unwrap();
        let text_id = db.conn.last_insert_rowid();

        let image = packed_test_image(2, 1, 0x7b);
        db.add_image(&image, "self").unwrap();
        let image_id = db.conn.last_insert_rowid();

        let file = b"preview file payload";
        db.add_file("notes.md", file, "self").unwrap();
        let file_id = db.conn.last_insert_rowid();

        let text_preview = db.get_preview_payload(text_id).unwrap();
        assert_eq!(
            text_preview,
            PreviewPayload {
                kind: "text".to_string(),
                name: "text.txt".to_string(),
                size_bytes: text.len() as u64,
                data: text.as_bytes().to_vec(),
            }
        );

        let image_preview = db.get_preview_payload(image_id).unwrap();
        assert_eq!(image_preview.kind, "image");
        assert_eq!(image_preview.name, "image");
        assert_eq!(image_preview.size_bytes, image.len() as u64);
        assert_eq!(image_preview.data, image);

        let file_preview = db.get_preview_payload(file_id).unwrap();
        assert_eq!(file_preview.kind, "file");
        assert_eq!(file_preview.name, "notes.md");
        assert_eq!(file_preview.size_bytes, file.len() as u64);
        assert_eq!(file_preview.data, file);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn preview_metadata_describes_text_image_and_file_without_payloads() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-preview-metadata-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let mut db = test_database(&root);

        db.add_text("metadata text", "self").unwrap();
        let text_id = db.conn.last_insert_rowid();
        let image = packed_test_image(3, 2, 0x51);
        db.add_image(&image, "self").unwrap();
        let image_id = db.conn.last_insert_rowid();
        db.add_file("metadata.pdf", b"metadata file", "self")
            .unwrap();
        let file_id = db.conn.last_insert_rowid();

        assert_eq!(
            db.get_preview_metadata(text_id).unwrap(),
            PreviewMetadata {
                entry_id: text_id,
                kind: PreviewKind::Text,
                name: "text.txt".to_string(),
                size_bytes: 13,
                batch: None,
            }
        );
        assert_eq!(
            db.get_preview_metadata(image_id).unwrap(),
            PreviewMetadata {
                entry_id: image_id,
                kind: PreviewKind::Image,
                name: "image".to_string(),
                size_bytes: image.len() as u64,
                batch: None,
            }
        );
        assert_eq!(
            db.get_preview_metadata(file_id).unwrap(),
            PreviewMetadata {
                entry_id: file_id,
                kind: PreviewKind::File,
                name: "metadata.pdf".to_string(),
                size_bytes: 13,
                batch: None,
            }
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn preview_metadata_does_not_decrypt_payload_data() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-preview-metadata-only-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let db = test_database(&root);
        db.conn
            .execute(
                "INSERT INTO history
                    (timestamp, type, description, data, size_bytes, source_peer, data_hash)
                 VALUES ('2026-01-01T00:00:00Z', 'text', 'Encrypted text', X'00', 1,
                         'self', 'corrupt-text')",
                [],
            )
            .unwrap();
        let id = db.conn.last_insert_rowid();

        let metadata = db.get_preview_metadata(id).unwrap();
        assert_eq!(metadata.kind, PreviewKind::Text);
        assert_eq!(metadata.size_bytes, 1);
        assert!(matches!(
            db.get_preview_payload(id),
            Err(PreviewError::PayloadUnavailable { entry_id, .. }) if entry_id == id
        ));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn preview_batch_navigation_uses_actual_order_and_count() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-preview-navigation-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let db = test_database(&root);
        for (id, index) in [(41_i64, 2_i64), (42, 0), (43, 1)] {
            db.conn
                .execute(
                    "INSERT INTO history
                        (id, timestamp, type, description, data, size_bytes, source_peer,
                         data_hash, category, categories, category_confidence,
                         classifier_version, batch_id, batch_index, batch_total, batch_status)
                     VALUES (?1, '2026-01-01T00:00:00Z', 'file', ?2, X'00', 1, 'self',
                             ?3, 'file', '[\"file\"]', 100, 1, 'batch-nav', ?4, 99,
                             'receiving')",
                    params![id, format!("file-{id}"), format!("hash-{id}"), index],
                )
                .unwrap();
        }

        let navigation = db.get_preview_batch_navigation("batch-nav", 43).unwrap();
        assert_eq!(
            navigation,
            PreviewBatchNavigation {
                batch_id: "batch-nav".to_string(),
                item_index: 1,
                item_count: 3,
                first_entry_id: 42,
                last_entry_id: 41,
                previous_entry_id: Some(42),
                next_entry_id: Some(41),
            }
        );
        assert_eq!(db.get_preview_metadata(43).unwrap().batch, Some(navigation));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn preview_batch_navigation_reports_typed_membership_errors() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-preview-navigation-errors-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let db = test_database(&root);
        db.conn
            .execute(
                "INSERT INTO history
                    (id, timestamp, type, description, data, size_bytes, source_peer,
                     data_hash, batch_id, batch_index, batch_total)
                 VALUES (7, '2026-01-01T00:00:00Z', 'file', 'one.txt', X'00', 1,
                         'self', 'one', 'existing-batch', 0, 1)",
                [],
            )
            .unwrap();

        let missing = db
            .get_preview_batch_navigation("missing-batch", 7)
            .unwrap_err();
        assert_eq!(
            missing,
            PreviewError::BatchNotFound {
                batch_id: "missing-batch".to_string(),
            }
        );
        assert_eq!(missing.code(), PreviewErrorCode::BatchNotFound);

        let wrong_entry = db
            .get_preview_batch_navigation("existing-batch", 8)
            .unwrap_err();
        assert_eq!(
            wrong_entry,
            PreviewError::EntryNotInBatch {
                entry_id: 8,
                batch_id: "existing-batch".to_string(),
            }
        );
        assert_eq!(wrong_entry.code(), PreviewErrorCode::EntryNotInBatch);
        assert_eq!(
            serde_json::to_string(&wrong_entry.code()).unwrap(),
            "\"entry_not_in_batch\""
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn typed_preview_payload_reports_missing_entry() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-preview-typed-missing-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let db = test_database(&root);

        let error = db.get_preview_payload(404).unwrap_err();
        assert_eq!(error, PreviewError::EntryNotFound { entry_id: 404 });
        assert_eq!(error.code(), PreviewErrorCode::EntryNotFound);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn preview_payload_rejects_oversized_metadata_before_decryption() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-preview-too-large-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let db = test_database(&root);
        let declared_size = PREVIEW_MAX_BYTES + 1;
        db.conn
            .execute(
                "INSERT INTO history
                    (timestamp, type, description, data, size_bytes, source_peer, data_hash)
                 VALUES ('2026-01-01T00:00:00Z', 'file', 'large.bin', X'00', ?1, 'self', 'large')",
                params![declared_size as i64],
            )
            .unwrap();
        let id = db.conn.last_insert_rowid();

        let error = db.get_preview_payload(id).unwrap_err();
        assert!(matches!(
            error,
            PreviewError::PreviewTooLarge { size, limit }
                if size == declared_size && limit == PREVIEW_MAX_BYTES
        ));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn preview_payload_reports_missing_history_id() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-preview-missing-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let db = test_database(&root);

        let error = db.get_preview_payload(404).unwrap_err();
        assert_eq!(error, PreviewError::EntryNotFound { entry_id: 404 });

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn keyword_search_treats_like_metacharacters_as_literals() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-literal-search-{:016x}",
            rand::random::<u64>()
        ));
        let mut db = test_database(&root);
        for text in [
            "progress 100% done",
            "progress 1000 done",
            "value a_b",
            "value axb",
            r"path C:\Temp",
            "中文剪贴板",
        ] {
            db.add_text(text, "self").unwrap();
        }

        assert_eq!(db.get_all(Some("100%"), None, 10, 0).unwrap().len(), 1);
        assert_eq!(db.get_all(Some("a_b"), None, 10, 0).unwrap().len(), 1);
        assert_eq!(db.get_all(Some(r"C:\Temp"), None, 10, 0).unwrap().len(), 1);
        assert_eq!(db.get_all(Some("剪贴板"), None, 10, 0).unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn keyword_pages_are_bounded_and_do_not_read_non_text_payloads() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-bounded-search-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let mut db = test_database(&root);
        for index in 0..4 {
            db.add_text(&format!("needle text {index}"), "self")
                .unwrap();
        }
        db.conn
            .execute(
                "INSERT INTO history
                    (timestamp, type, description, data, size_bytes, source_peer, data_hash,
                     category, categories, category_confidence, classifier_version)
                 VALUES
                    ('2026-01-01T00:00:00Z', 'file', 'needle-large.bin', zeroblob(8388608),
                     8388608, 'self', 'large', 'file', '[\"file\"]', 100, 1)",
                [],
            )
            .unwrap();

        let first = db
            .get_page_filtered(Some("needle"), None, None, None, 2, 0)
            .unwrap();
        assert_eq!(first.entries.len(), 2);
        assert_eq!(first.total, None);
        assert!(first.has_more);

        let last = db
            .get_page_filtered(Some("needle"), None, None, None, 2, 4)
            .unwrap();
        assert_eq!(last.entries.len(), 1);
        assert_eq!(last.entries[0].description, "needle-large.bin");
        assert!(!last.has_more);
        assert_eq!(
            db.count_all_filtered(Some("needle"), None, None, None)
                .unwrap(),
            5
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn text_previews_are_decrypted_only_when_history_is_read() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-encrypted-preview-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let mut db = test_database(&root);
        let secret = "ts_live_super_sensitive_token";

        db.add_text(secret, "self").unwrap();

        let stored_description: String = db
            .conn
            .query_row("SELECT description FROM history", [], |row| row.get(0))
            .unwrap();
        assert_eq!(stored_description, TEXT_DESCRIPTION_PLACEHOLDER);
        assert!(!stored_description.contains(secret));

        let entries = db.get_all(Some("sensitive_token"), None, 10, 0).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].description, secret);
        assert_eq!(db.get_description(entries[0].id).unwrap(), secret);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn keyword_search_matches_text_beyond_the_display_preview() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-full-text-search-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let mut db = test_database(&root);
        let text = format!("{}deep_search_needle", "x".repeat(150));
        db.add_text(&text, "self").unwrap();

        let entries = db
            .get_page_filtered(Some("deep_search_needle"), None, None, None, 10, 0)
            .unwrap()
            .entries;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].description.chars().count(), 100);
        assert!(!entries[0].description.contains("deep_search_needle"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn migration_v9_scrubs_legacy_plaintext_previews_from_sqlite_files() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-preview-migration-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let db_path = root.join("history-v2.db");
        let secret = "legacy_plaintext_preview_should_disappear";
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute_batch("PRAGMA journal_mode = WAL;").unwrap();
            schema::initialize(&conn).unwrap();
            conn.execute("INSERT INTO schema_version (version) VALUES (8)", [])
                .unwrap();
            conn.execute(
                "INSERT INTO history
                    (timestamp, type, description, data, size_bytes, source_peer, data_hash)
                 VALUES ('2026-01-01T00:00:00Z', 'text', ?1, X'00', 1, 'self', 'hash')",
                params![secret],
            )
            .unwrap();
            HistoryDB::migrate(
                &conn,
                &root.join("file-history"),
                &root.join("image-history"),
            )
            .unwrap();
            let description: String = conn
                .query_row("SELECT description FROM history", [], |row| row.get(0))
                .unwrap();
            assert_eq!(description, TEXT_DESCRIPTION_PLACEHOLDER);
        }

        for path in [
            db_path.clone(),
            root.join("history-v2.db-wal"),
            root.join("history-v2.db-shm"),
        ] {
            if path.is_file() {
                let bytes = std::fs::read(path).unwrap();
                assert!(!bytes
                    .windows(secret.len())
                    .any(|window| window == secret.as_bytes()));
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn open_at_locks_the_managed_storage_tree() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-private-open-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let db = HistoryDB::open_at(&root).unwrap();
        drop(db);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&root), 0o700, "storage root");
            assert_eq!(mode(&root.join("incoming")), 0o700);
            assert_eq!(mode(&root.join("clipboard-files")), 0o700);
            assert_eq!(mode(&root.join("file-history")), 0o700);
            assert_eq!(mode(&root.join("image-history")), 0o700);
            assert_eq!(mode(&root.join("history-v2.db")), 0o600, "sqlite main file");
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn open_at_repairs_pre_existing_wide_permissions() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-private-repair-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(root.join("incoming")).unwrap();
        std::fs::write(root.join("history-v2.db"), b"legacy-wide-file").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
            std::fs::set_permissions(
                root.join("incoming"),
                std::fs::Permissions::from_mode(0o755),
            )
            .unwrap();
            std::fs::set_permissions(
                root.join("history-v2.db"),
                std::fs::Permissions::from_mode(0o644),
            )
            .unwrap();
        }
        // The wide "database" is not a valid SQLite file; opening fails after
        // the permission repair has already run, which is what we assert.
        assert!(HistoryDB::open_at(&root).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&root), 0o700);
            assert_eq!(mode(&root.join("incoming")), 0o700);
            assert_eq!(mode(&root.join("history-v2.db")), 0o600);
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn explicit_delete_truncates_the_write_ahead_log() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-delete-checkpoint-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let mut db = HistoryDB::open_at(&root).unwrap();
        db.add_text("delete me", "self").unwrap();
        let id: i64 = db
            .conn
            .query_row("SELECT id FROM history", [], |row| row.get(0))
            .unwrap();

        db.delete(id).unwrap();

        let wal_path = root.join("history-v2.db-wal");
        assert!(
            !wal_path.exists() || std::fs::metadata(&wal_path).unwrap().len() == 0,
            "explicit delete left data in {}",
            wal_path.display()
        );
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn main_database_connection_waits_for_transient_locks() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-busy-timeout-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let db = HistoryDB::open_at(&root).unwrap();

        let timeout_ms: i64 = db
            .conn
            .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
            .unwrap();

        assert_eq!(timeout_ms, 5_000);
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn file_byte_quota_selects_oldest_entries_for_removal() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-file-quota-{:016x}",
            rand::random::<u64>()
        ));
        let db = test_database(&root);
        db.conn
            .execute_batch(
                "INSERT INTO history (timestamp, type, description, data, size_bytes, data_hash)
                 VALUES
                   ('2026-01-01T00:00:00Z', 'file', 'old', X'00', 6, 'old'),
                   ('2026-01-02T00:00:00Z', 'file', 'middle', X'00', 6, 'middle'),
                   ('2026-01-03T00:00:00Z', 'file', 'new', X'00', 6, 'new');",
            )
            .unwrap();
        assert_eq!(db.file_ids_over_byte_limit(10).unwrap(), vec![2, 1]);
    }

    #[test]
    fn pinned_entries_are_never_selected_for_quota_cleanup() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-pinned-quota-{:016x}",
            rand::random::<u64>()
        ));
        let db = test_database(&root);
        db.conn
            .execute_batch(
                "INSERT INTO history
                    (timestamp, type, description, data, size_bytes, data_hash, pinned)
                 VALUES
                   ('2026-01-01T00:00:00Z', 'file', 'pinned', X'00', 8, 'pinned', 1),
                   ('2026-01-02T00:00:00Z', 'file', 'middle', X'00', 6, 'middle', 0),
                   ('2026-01-03T00:00:00Z', 'file', 'new', X'00', 6, 'new', 0);",
            )
            .unwrap();
        assert_eq!(db.file_ids_over_byte_limit(10).unwrap(), vec![2]);
    }

    #[test]
    fn image_writes_enforce_the_shared_file_byte_quota() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-image-quota-{:016x}",
            rand::random::<u64>()
        ));
        let mut db = test_database(&root);
        db.set_storage_quota(30);
        let mut image = [2_u32.to_le_bytes(), 2_u32.to_le_bytes()].concat();
        image.extend_from_slice(&[0x44; 16]);
        db.add_image(&image, "self").unwrap();
        let mut second = [2_u32.to_le_bytes(), 2_u32.to_le_bytes()].concat();
        second.extend_from_slice(&[0x55; 16]);
        db.add_image(&second, "self").unwrap();

        let count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM history WHERE type = 'image'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            count, 1,
            "image additions did not enforce the shared byte quota"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn changing_limits_can_be_enforced_immediately() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-immediate-limits-{:016x}",
            rand::random::<u64>()
        ));
        let mut db = test_database(&root);
        for index in 0..5 {
            db.add_text(&format!("entry-{index}"), "self").unwrap();
        }
        db.set_max_history(2);
        db.enforce_limits().unwrap();

        let count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM history", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 2);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn manually_deleting_from_a_complete_batch_rebases_survivors() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-partial-pinned-batch-{:016x}",
            rand::random::<u64>()
        ));
        let mut db = test_database(&root);
        db.conn
            .execute_batch(
                "INSERT INTO history
                    (id, timestamp, type, description, data, size_bytes, data_hash, pinned,
                     batch_id, batch_index, batch_total, batch_status)
                 VALUES
                   (1, '2026-01-01T00:00:00Z', 'file', 'keep.bin', X'00', 8, 'keep', 1,
                    'batch-pinned', 0, 2, 'complete'),
                   (2, '2026-01-01T00:00:00Z', 'file', 'remove.bin', X'00', 8, 'remove', 0,
                    'batch-pinned', 1, 2, 'complete');",
            )
            .unwrap();

        db.delete(2).unwrap();
        let survivor: (i64, i64, String) = db
            .conn
            .query_row(
                "SELECT batch_index, batch_total, batch_status FROM history WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(survivor, (0, 1, "complete".to_string()));
    }

    #[test]
    fn automatic_cleanup_keeps_partial_pinned_batches_incomplete() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-partial-cleanup-batch-{:016x}",
            rand::random::<u64>()
        ));
        let mut db = test_database(&root);
        db.conn
            .execute_batch(
                "INSERT INTO history
                    (id, timestamp, type, description, data, size_bytes, data_hash, pinned,
                     batch_id, batch_index, batch_total, batch_status)
                 VALUES
                   (1, '2026-01-01T00:00:00Z', 'file', 'keep.bin', X'00', 8, 'keep', 1,
                    'batch-cleanup', 0, 2, 'complete'),
                   (2, '2026-01-01T00:00:00Z', 'file', 'remove.bin', X'00', 8, 'remove', 0,
                    'batch-cleanup', 1, 2, 'complete');",
            )
            .unwrap();

        db.delete_entries(&[2]).unwrap();
        let survivor: (i64, String) = db
            .conn
            .query_row(
                "SELECT batch_total, batch_status FROM history WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(survivor, (2, "incomplete".to_string()));
    }

    #[test]
    fn incomplete_batch_is_retained_but_cannot_be_copied_as_a_group() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-incomplete-batch-{:016x}",
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("received.txt");
        std::fs::write(&source, b"received").unwrap();
        let hash = blake3::hash(b"received").to_hex().to_string();
        let mut db = test_database(&root);
        db.add_file_batch_with_status(
            "batch-incomplete",
            &[HistoryFileInput {
                name: "received.txt".into(),
                path: source,
                data_hash: hash,
                size: 8,
            }],
            2,
            "peer",
            false,
            false,
        )
        .unwrap();

        let (count, status, total): (i64, String, i64) = db
            .conn
            .query_row(
                "SELECT COUNT(*), MIN(batch_status), MIN(batch_total)
                 FROM history WHERE batch_id = 'batch-incomplete'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!((count, status.as_str(), total), (1, "incomplete", 2));
        let entries = db.get_all(None, None, 10, 0).unwrap();
        assert_eq!(entries[0].batch_count, Some(1));
        assert!(db.materialize_file_batch("batch-incomplete").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_batch_persistence_does_not_leave_partial_database_rows() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-atomic-batch-{:016x}",
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let first = root.join("first.bin");
        std::fs::write(&first, b"first").unwrap();
        let missing = root.join("missing.bin");
        let mut db = test_database(&root);
        let result = db.add_file_batch(
            "batch-atomic",
            &[
                HistoryFileInput {
                    name: "first.bin".into(),
                    path: first,
                    data_hash: blake3::hash(b"first").to_hex().to_string(),
                    size: 5,
                },
                HistoryFileInput {
                    name: "missing.bin".into(),
                    path: missing,
                    data_hash: blake3::hash(b"missing").to_hex().to_string(),
                    size: 7,
                },
            ],
            "self",
            false,
        );

        assert!(result.is_err());
        let count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM history WHERE batch_id = 'batch-atomic'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
        assert_eq!(std::fs::read_dir(&db.file_history_dir).unwrap().count(), 0);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn oversized_history_file_is_rejected_before_persistence() {
        assert!(validate_history_file_size(FILE_HISTORY_BYTE_LIMIT as u64).is_ok());
        let error = validate_history_file_size(FILE_HISTORY_BYTE_LIMIT as u64 + 1)
            .expect_err("oversized file must be rejected");
        assert!(error.to_string().contains("5 GiB history limit"));
    }

    #[test]
    fn file_reference_round_trip_uses_external_file() {
        let directory = std::env::temp_dir().join(format!(
            "tailsync-db-test-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let data = b"large file history content";
        let hash = blake3::hash(data).to_hex().to_string();

        let (encoded, path) =
            persist_history_file_at(&directory, &hash, "archive.bin", data).unwrap();
        let reference = decode_file_reference(&encoded).unwrap();

        assert_eq!(reference.version, 2);
        assert_eq!(reference.file_name, format!("{hash}-archive.bin"));
        assert_eq!(path, directory.join(reference.file_name));
        assert_eq!(file_encryption::decrypt_file_to_vec(&path).unwrap(), data);

        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn file_reference_rejects_parent_paths() {
        let directory = std::env::temp_dir();
        let reference = StoredFileReference {
            version: 1,
            file_name: "../outside.bin".to_string(),
        };
        assert!(resolve_file_reference_at(&directory, &reference).is_err());
    }

    #[test]
    fn file_reference_sanitizes_untrusted_original_name() {
        let directory = std::env::temp_dir().join(format!(
            "tailsync-file-name-test-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let data = b"file content";
        let hash = blake3::hash(data).to_hex().to_string();

        let (encoded, path) = persist_history_file_at(
            &directory,
            &hash,
            r#"C:\temp\bad<name>:"file"?.txt. "#,
            data,
        )
        .unwrap();
        let reference = decode_file_reference(&encoded).unwrap();

        assert_eq!(reference.file_name, format!("{hash}-bad_name___file__.txt"));
        assert_eq!(path.file_name().unwrap(), reference.file_name.as_str());
        assert_eq!(file_encryption::decrypt_file_to_vec(&path).unwrap(), data);

        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn clipboard_materialization_preserves_the_original_basename() {
        let directory = std::env::temp_dir().join(format!(
            "tailsync-clipboard-name-test-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let history_directory = directory.join("file-history");
        let clipboard_directory = directory.join("clipboard-files");
        let data = b"clipboard file content";
        let hash = blake3::hash(data).to_hex().to_string();
        let (_, history_path) =
            persist_history_file_at(&history_directory, &hash, "report.pdf", data).unwrap();

        let clipboard_path =
            materialize_clipboard_file_at(&clipboard_directory, &history_path, "report.pdf")
                .unwrap();

        assert_eq!(clipboard_path.file_name().unwrap(), "report.pdf");
        assert_eq!(std::fs::read(clipboard_path).unwrap(), data);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn plaintext_clipboard_materialization_does_not_change_the_source_inode() {
        use std::os::unix::fs::PermissionsExt;

        let directory = std::env::temp_dir().join(format!(
            "tailsync-clipboard-source-permissions-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let source = directory.join("source.txt");
        let clipboard_directory = directory.join("clipboard-files");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(&source, b"source").unwrap();
        std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o644)).unwrap();

        let target =
            materialize_clipboard_file_at(&clipboard_directory, &source, "source.txt").unwrap();

        let source_mode = std::fs::metadata(&source).unwrap().permissions().mode() & 0o777;
        let target_mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(source_mode, 0o644);
        assert_eq!(target_mode, 0o600);
        std::fs::write(&target, b"clipboard copy").unwrap();
        assert_eq!(std::fs::read(&source).unwrap(), b"source");

        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn remote_clipboard_materialization_copies_and_marks_plaintext_source() {
        let directory = std::env::temp_dir().join(format!(
            "tailsync-remote-clipboard-test-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let source = directory.join("source.bin");
        let clipboard_directory = directory.join("clipboard-files");
        let data = b"remote clipboard file content";
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(&source, data).unwrap();

        let target = materialize_remote_clipboard_file_at(
            &clipboard_directory,
            &source,
            "received.bin",
            "peer-with-untrusted-name",
        )
        .unwrap();
        std::fs::write(&target, b"changed clipboard copy").unwrap();
        assert_eq!(std::fs::read(&source).unwrap(), data);

        #[cfg(target_os = "macos")]
        {
            use std::os::unix::ffi::OsStrExt;

            let path = std::ffi::CString::new(target.as_os_str().as_bytes()).unwrap();
            let name = b"com.apple.quarantine\0";
            let size = unsafe {
                libc::getxattr(
                    path.as_ptr(),
                    name.as_ptr().cast(),
                    std::ptr::null_mut(),
                    0,
                    0,
                    0,
                )
            };
            assert!(
                size > 0,
                "remote file should carry macOS quarantine metadata"
            );
            let mut value = vec![0u8; usize::try_from(size).unwrap()];
            let read = unsafe {
                libc::getxattr(
                    path.as_ptr(),
                    name.as_ptr().cast(),
                    value.as_mut_ptr().cast(),
                    value.len(),
                    0,
                    0,
                )
            };
            assert_eq!(read, size);
            let value = String::from_utf8(value).unwrap();
            let fields = value.split(';').collect::<Vec<_>>();
            assert_eq!(fields.len(), 4, "quarantine value should have four fields");
            assert_eq!(fields[0], "0081");
            assert_eq!(fields[2], "TailSync");
            assert_eq!(fields[3].len(), 32);
        }

        #[cfg(target_os = "windows")]
        {
            let mut stream = target.as_os_str().to_os_string();
            stream.push(":Zone.Identifier");
            let marker = std::fs::read(stream).unwrap();
            assert_eq!(marker, b"[ZoneTransfer]\r\nZoneId=3\r\n");
        }

        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn file_history_lifecycle_keeps_bytes_out_of_sqlite() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-history-lifecycle-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let mut db = test_database(&root);
        let data = vec![0x5a; 1024 * 1024];

        let first_path = db.add_file("archive.bin", &data, "self").unwrap();
        assert!(first_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with("-archive.bin"));
        let first_id: i64 = db
            .conn
            .query_row("SELECT id FROM history", [], |row| row.get(0))
            .unwrap();
        let stored_size: i64 = db
            .conn
            .query_row("SELECT length(data) FROM history", [], |row| row.get(0))
            .unwrap();

        assert!(
            stored_size < 256,
            "SQLite should only contain a file reference"
        );
        assert_eq!(db.get_data(first_id).unwrap(), data);
        let restored_path = db.get_file_path(first_id).unwrap().unwrap();
        assert_ne!(restored_path, first_path);
        assert!(restored_path.starts_with(root.join("clipboard-files")));
        assert_eq!(std::fs::read(restored_path).unwrap(), data);

        db.delete(first_id).unwrap();
        assert!(!first_path.exists());

        db.add_file("one.bin", b"one", "self").unwrap();
        db.add_file("two.bin", b"two", "self").unwrap();
        db.clear_all().unwrap();
        assert_eq!(std::fs::read_dir(&db.file_history_dir).unwrap().count(), 0);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn image_history_lifecycle_keeps_bytes_out_of_sqlite() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-image-history-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let mut db = test_database(&root);
        let data = packed_test_image(512, 512, 0x7b);

        db.add_image(&data, "self").unwrap();
        let (id, stored_size): (i64, i64) = db
            .conn
            .query_row("SELECT id, length(data) FROM history", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();
        let image_path = std::fs::read_dir(&db.image_history_dir)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();

        assert!(
            stored_size < 256,
            "SQLite should only contain an image reference"
        );
        assert_eq!(db.get_data(id).unwrap(), data);
        db.delete(id).unwrap();
        assert!(!image_path.exists());

        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn duplicate_file_replacement_cleans_old_name_and_preserves_same_path() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-history-duplicate-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let mut db = test_database(&root);
        let data = b"same content";

        let old_path = db.add_file("old.bin", data, "self").unwrap();
        let new_path = db.add_file("new.bin", data, "self").unwrap();
        assert_ne!(old_path, new_path);
        assert!(!old_path.exists());
        assert!(new_path.exists());

        let same_path = db.add_file("new.bin", data, "self").unwrap();
        assert_eq!(same_path, new_path);
        assert!(same_path.exists());
        assert_eq!(
            file_encryption::decrypt_file_to_vec(&same_path).unwrap(),
            data
        );
        let count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM history", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn migration_v3_accepts_files_and_records_resolved_hash_mismatches() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-file-migration-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let file_dir = root.join("file-history");
        let image_dir = root.join("image-history");
        std::fs::create_dir_all(&file_dir).unwrap();
        std::fs::create_dir_all(&image_dir).unwrap();
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_version (version INTEGER NOT NULL);
             INSERT INTO schema_version (version) VALUES (2);
             CREATE TABLE history (
                id INTEGER PRIMARY KEY AUTOINCREMENT, timestamp TEXT NOT NULL,
                type TEXT NOT NULL, description TEXT NOT NULL DEFAULT '', data BLOB NOT NULL,
                size_bytes INTEGER NOT NULL DEFAULT 0, source_peer TEXT NOT NULL DEFAULT '',
                data_hash TEXT NOT NULL DEFAULT ''
             );",
        )
        .unwrap();
        let plaintext = b"ordinary legacy file bytes";
        let encrypted = crypto::encrypt(plaintext).unwrap();
        conn.execute(
            "INSERT INTO history (timestamp, type, description, data, data_hash)
             VALUES ('2026-01-01T00:00:00Z', 'file', 'report.bin', ?1, 'old-hash')",
            params![encrypted],
        )
        .unwrap();

        HistoryDB::migrate(&conn, &file_dir, &image_dir).unwrap();

        let stored: Vec<u8> = conn
            .query_row("SELECT data FROM history", [], |row| row.get(0))
            .unwrap();
        let reference = decode_file_reference(&stored).unwrap();
        assert_eq!(reference.version, 2);
        assert_eq!(
            file_encryption::decrypt_file_to_vec(
                &resolve_file_reference_at(&file_dir, &reference).unwrap()
            )
            .unwrap(),
            plaintext
        );
        let issue: (String, bool) = conn
            .query_row(
                "SELECT issue_type, resolved_at IS NOT NULL FROM migration_issues",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(issue, ("hash_mismatch".to_string(), true));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn migration_v4_preserves_invalid_images_and_exposes_diagnostics() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-image-migration-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let file_dir = root.join("file-history");
        let image_dir = root.join("image-history");
        std::fs::create_dir_all(&file_dir).unwrap();
        std::fs::create_dir_all(&image_dir).unwrap();
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_version (version INTEGER NOT NULL);
             INSERT INTO schema_version (version) VALUES (3);
             CREATE TABLE history (
                id INTEGER PRIMARY KEY AUTOINCREMENT, timestamp TEXT NOT NULL,
                type TEXT NOT NULL, description TEXT NOT NULL DEFAULT '', data BLOB NOT NULL,
                size_bytes INTEGER NOT NULL DEFAULT 0, source_peer TEXT NOT NULL DEFAULT '',
                data_hash TEXT NOT NULL DEFAULT ''
             );",
        )
        .unwrap();
        let encrypted = crypto::encrypt(&[0_u8; 8]).unwrap();
        conn.execute(
            "INSERT INTO history (timestamp, type, description, data, data_hash)
             VALUES ('2026-01-01T00:00:00Z', 'image', 'broken', ?1, 'hash')",
            params![encrypted.clone()],
        )
        .unwrap();

        HistoryDB::migrate(&conn, &file_dir, &image_dir).unwrap();

        let stored: Vec<u8> = conn
            .query_row("SELECT data FROM history", [], |row| row.get(0))
            .unwrap();
        assert_eq!(stored, encrypted);
        let db = HistoryDB {
            conn,
            max_history: 100,
            storage_quota_bytes: crypto::DEFAULT_STORAGE_QUOTA_BYTES,
            storage_available: true,
            file_history_dir: file_dir,
            image_history_dir: image_dir,
        };
        let diagnostics = db.migration_diagnostics(10).unwrap();
        assert_eq!(diagnostics.unresolved_count, 1);
        assert_eq!(diagnostics.issues[0].issue_type, "invalid_image");
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn migration_v5_is_idempotent_and_classifies_media() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-category-migration-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let file_dir = root.join("file-history");
        let image_dir = root.join("image-history");
        std::fs::create_dir_all(&file_dir).unwrap();
        std::fs::create_dir_all(&image_dir).unwrap();
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_version (version INTEGER NOT NULL);
             INSERT INTO schema_version (version) VALUES (4);
             CREATE TABLE history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                type TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                data BLOB NOT NULL,
                size_bytes INTEGER NOT NULL DEFAULT 0,
                source_peer TEXT NOT NULL DEFAULT '',
                data_hash TEXT NOT NULL DEFAULT ''
             );
             INSERT INTO history
                (timestamp, type, description, data, size_bytes, source_peer, data_hash)
             VALUES ('2026-01-01T00:00:00Z', 'image', 'image', X'00', 1, 'self', 'hash');",
        )
        .unwrap();

        HistoryDB::migrate(&conn, &file_dir, &image_dir).unwrap();
        HistoryDB::migrate(&conn, &file_dir, &image_dir).unwrap();

        let version: i64 = conn
            .query_row("SELECT MAX(version) FROM schema_version", [], |row| {
                row.get(0)
            })
            .unwrap();
        let media: (String, i64, i64) = conn
            .query_row(
                "SELECT category, category_confidence, classifier_version FROM history",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        let columns: Vec<String> = conn
            .prepare("PRAGMA table_info(history)")
            .unwrap()
            .query_map([], |row| row.get(1))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        assert_eq!(
            media,
            (
                "image".to_string(),
                100,
                crate::history_classifier::CLASSIFIER_VERSION,
            )
        );
        assert!(columns.contains(&"category".to_string()));
        assert!(columns.contains(&"category_confidence".to_string()));
        assert!(columns.contains(&"classifier_version".to_string()));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn migration_v5_repairs_a_partial_schema() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-category-partial-migration-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let file_dir = root.join("file-history");
        let image_dir = root.join("image-history");
        std::fs::create_dir_all(&file_dir).unwrap();
        std::fs::create_dir_all(&image_dir).unwrap();
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_version (version INTEGER NOT NULL);
             INSERT INTO schema_version (version) VALUES (5);
             CREATE TABLE history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                type TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                data BLOB NOT NULL,
                size_bytes INTEGER NOT NULL DEFAULT 0,
                source_peer TEXT NOT NULL DEFAULT '',
                data_hash TEXT NOT NULL DEFAULT ''
             );
             INSERT INTO history
                (timestamp, type, description, data, size_bytes, source_peer, data_hash)
             VALUES ('2026-01-01T00:00:00Z', 'file', 'file', X'00', 1, 'self', 'hash');",
        )
        .unwrap();

        HistoryDB::migrate(&conn, &file_dir, &image_dir).unwrap();

        let media: (String, i64, i64) = conn
            .query_row(
                "SELECT category, category_confidence, classifier_version FROM history",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        let index_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'index' AND name = 'idx_history_category_timestamp'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            media,
            (
                "file".to_string(),
                100,
                crate::history_classifier::CLASSIFIER_VERSION,
            )
        );
        assert_eq!(index_count, 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn opening_v7_database_adds_batch_columns_before_creating_batch_index() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-batch-v8-migration-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let db_path = root.join("history-v2.db");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_version (version INTEGER NOT NULL);
             INSERT INTO schema_version (version) VALUES (7);
             CREATE TABLE history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                type TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                data BLOB NOT NULL,
                size_bytes INTEGER NOT NULL DEFAULT 0,
                source_peer TEXT NOT NULL DEFAULT '',
                data_hash TEXT NOT NULL DEFAULT '',
                category TEXT NOT NULL DEFAULT 'text',
                categories TEXT NOT NULL DEFAULT '[]',
                category_confidence INTEGER NOT NULL DEFAULT 0,
                classifier_version INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
             );",
        )
        .unwrap();
        drop(conn);

        let db = HistoryDB::open_at(&root).unwrap();
        let columns = db
            .conn
            .prepare("PRAGMA table_info(history)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let batch_index_count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'index' AND name = 'idx_history_batch'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let version: i64 = db
            .conn
            .query_row("SELECT MAX(version) FROM schema_version", [], |row| {
                row.get(0)
            })
            .unwrap();

        for column in [
            "pinned",
            "batch_id",
            "batch_index",
            "batch_total",
            "batch_status",
        ] {
            assert!(columns.iter().any(|existing| existing == column));
        }
        assert_eq!(batch_index_count, 1);
        assert_eq!(version, SCHEMA_VERSION);
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn migration_v6_preserves_v5_primary_categories_as_json_and_is_idempotent() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-category-v6-migration-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let file_dir = root.join("file-history");
        let image_dir = root.join("image-history");
        std::fs::create_dir_all(&file_dir).unwrap();
        std::fs::create_dir_all(&image_dir).unwrap();
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_version (version INTEGER NOT NULL);
             INSERT INTO schema_version (version) VALUES (5);
             CREATE TABLE history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                type TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                data BLOB NOT NULL,
                size_bytes INTEGER NOT NULL DEFAULT 0,
                source_peer TEXT NOT NULL DEFAULT '',
                data_hash TEXT NOT NULL DEFAULT '',
                category TEXT NOT NULL DEFAULT 'text',
                category_confidence INTEGER NOT NULL DEFAULT 0,
                classifier_version INTEGER NOT NULL DEFAULT 0
             );
             INSERT INTO history
                (timestamp, type, description, data, size_bytes, source_peer, data_hash,
                 category, category_confidence, classifier_version)
             VALUES ('2026-01-01T00:00:00Z', 'text', 'legacy code', X'00', 1,
                     'self', 'legacy-code', 'code', 90, 2);",
        )
        .unwrap();

        HistoryDB::migrate(&conn, &file_dir, &image_dir).unwrap();
        HistoryDB::migrate(&conn, &file_dir, &image_dir).unwrap();

        let version: i64 = conn
            .query_row("SELECT MAX(version) FROM schema_version", [], |row| {
                row.get(0)
            })
            .unwrap();
        let migrated: (String, String, i64) = conn
            .query_row(
                "SELECT category, categories, classifier_version FROM history",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        let categories: Vec<String> = serde_json::from_str(&migrated.1).unwrap();
        let columns: Vec<String> = conn
            .prepare("PRAGMA table_info(history)")
            .unwrap()
            .query_map([], |row| row.get(1))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();

        assert_eq!(version, SCHEMA_VERSION);
        assert_eq!(migrated.0, "code");
        assert_eq!(categories, vec!["code"]);
        assert_eq!(migrated.2, 2);
        assert!(columns.contains(&"categories".to_string()));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn migration_v7_encrypts_plaintext_file_history_and_is_idempotent() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-file-encryption-migration-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let file_dir = root.join("file-history");
        let image_dir = root.join("image-history");
        std::fs::create_dir_all(&file_dir).unwrap();
        let conn = Connection::open_in_memory().unwrap();
        schema::initialize(&conn).unwrap();
        conn.execute("INSERT INTO schema_version (version) VALUES (6)", [])
            .unwrap();

        let plaintext = b"legacy plaintext file-history bytes";
        let data_hash = blake3::hash(plaintext).to_hex().to_string();
        let file_name = format!("{data_hash}-legacy.bin");
        let path = file_dir.join(&file_name);
        std::fs::write(&path, plaintext).unwrap();
        let reference = encode_file_reference_version(&file_name, 1).unwrap();
        conn.execute(
            "INSERT INTO history
                (timestamp, type, description, data, size_bytes, source_peer, data_hash)
             VALUES ('2026-01-01T00:00:00Z', 'file', 'legacy.bin', ?1, ?2, 'self', ?3)",
            params![reference, plaintext.len() as i64, data_hash],
        )
        .unwrap();

        HistoryDB::migrate(&conn, &file_dir, &image_dir).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), plaintext);
        let batch =
            HistoryDB::migrate_file_history_encryption_rows(&conn, &file_dir, 0, 16).unwrap();
        assert_eq!((batch.scanned, batch.migrated, batch.failed), (1, 1, 0));
        let stored_reference: Vec<u8> = conn
            .query_row("SELECT data FROM history", [], |row| row.get(0))
            .unwrap();
        assert_eq!(decode_file_reference(&stored_reference).unwrap().version, 2);
        assert_ne!(std::fs::read(&path).unwrap(), plaintext);
        assert_eq!(
            file_encryption::decrypt_file_to_vec(&path).unwrap(),
            plaintext
        );
        let encrypted_once = std::fs::read(&path).unwrap();

        let batch =
            HistoryDB::migrate_file_history_encryption_rows(&conn, &file_dir, 0, 16).unwrap();
        assert_eq!((batch.scanned, batch.migrated, batch.failed), (1, 0, 0));
        assert_eq!(std::fs::read(&path).unwrap(), encrypted_once);
        let version: i64 = conn
            .query_row("SELECT MAX(version) FROM schema_version", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn migration_v7_retries_missing_files_without_blocking_schema_upgrade() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-file-encryption-retry-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let file_dir = root.join("file-history");
        let image_dir = root.join("image-history");
        std::fs::create_dir_all(&file_dir).unwrap();
        let conn = Connection::open_in_memory().unwrap();
        schema::initialize(&conn).unwrap();
        conn.execute("INSERT INTO schema_version (version) VALUES (6)", [])
            .unwrap();

        let plaintext = b"file restored after a transient migration failure";
        let data_hash = blake3::hash(plaintext).to_hex().to_string();
        let file_name = format!("{data_hash}-missing.bin");
        let reference = encode_file_reference_version(&file_name, 1).unwrap();
        conn.execute(
            "INSERT INTO history
                (timestamp, type, description, data, size_bytes, source_peer, data_hash)
             VALUES ('2026-01-01T00:00:00Z', 'file', 'missing.bin', ?1, ?2, 'self', ?3)",
            params![reference, plaintext.len() as i64, data_hash],
        )
        .unwrap();

        HistoryDB::migrate(&conn, &file_dir, &image_dir).unwrap();
        let batch =
            HistoryDB::migrate_file_history_encryption_rows(&conn, &file_dir, 0, 16).unwrap();
        assert_eq!((batch.scanned, batch.migrated, batch.failed), (1, 0, 1));
        let state: (i64, i64) = conn
            .query_row(
                "SELECT (SELECT MAX(version) FROM schema_version),
                        (SELECT COUNT(*) FROM migration_issues WHERE resolved_at IS NULL)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(state, (SCHEMA_VERSION, 1));
        let stored_reference: Vec<u8> = conn
            .query_row("SELECT data FROM history", [], |row| row.get(0))
            .unwrap();
        assert_eq!(decode_file_reference(&stored_reference).unwrap().version, 1);

        let path = file_dir.join(&file_name);
        std::fs::write(&path, plaintext).unwrap();
        let batch =
            HistoryDB::migrate_file_history_encryption_rows(&conn, &file_dir, 0, 16).unwrap();
        assert_eq!((batch.scanned, batch.migrated, batch.failed), (1, 1, 0));
        let unresolved: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM migration_issues WHERE resolved_at IS NULL",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let stored_reference: Vec<u8> = conn
            .query_row("SELECT data FROM history", [], |row| row.get(0))
            .unwrap();
        assert_eq!(unresolved, 0);
        assert_eq!(decode_file_reference(&stored_reference).unwrap().version, 2);
        assert_eq!(
            file_encryption::decrypt_file_to_vec(&path).unwrap(),
            plaintext
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn migration_v7_recovers_when_container_was_installed_before_reference_update() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-file-encryption-crash-recovery-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let file_dir = root.join("file-history");
        let image_dir = root.join("image-history");
        std::fs::create_dir_all(&file_dir).unwrap();
        let conn = Connection::open_in_memory().unwrap();
        schema::initialize(&conn).unwrap();
        conn.execute("INSERT INTO schema_version (version) VALUES (6)", [])
            .unwrap();

        let plaintext = b"already encrypted before database update";
        let data_hash = blake3::hash(plaintext).to_hex().to_string();
        let file_name = format!("{data_hash}-interrupted.bin");
        let path = file_dir.join(&file_name);
        file_encryption::encrypt_bytes_atomic(plaintext, &data_hash, &path).unwrap();
        let encrypted_before = std::fs::read(&path).unwrap();
        let reference = encode_file_reference_version(&file_name, 1).unwrap();
        conn.execute(
            "INSERT INTO history
                (timestamp, type, description, data, size_bytes, source_peer, data_hash)
             VALUES ('2026-01-01T00:00:00Z', 'file', 'interrupted.bin', ?1, ?2, 'self', ?3)",
            params![reference, plaintext.len() as i64, data_hash],
        )
        .unwrap();

        HistoryDB::migrate(&conn, &file_dir, &image_dir).unwrap();
        let batch =
            HistoryDB::migrate_file_history_encryption_rows(&conn, &file_dir, 0, 16).unwrap();
        assert_eq!((batch.scanned, batch.migrated, batch.failed), (1, 1, 0));
        let stored_reference: Vec<u8> = conn
            .query_row("SELECT data FROM history", [], |row| row.get(0))
            .unwrap();
        assert_eq!(decode_file_reference(&stored_reference).unwrap().version, 2);
        assert_eq!(std::fs::read(&path).unwrap(), encrypted_before);
        assert_eq!(
            file_encryption::decrypt_file_to_vec(&path).unwrap(),
            plaintext
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn backfill_upgrades_old_classifier_and_persists_secondary_labels() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-category-classifier-backfill-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let mut db = test_database(&root);
        let text = b"example.com";
        let encrypted = crypto::encrypt(text).unwrap();
        let hash = blake3::hash(text).to_hex().to_string();
        db.conn
            .execute(
                "INSERT INTO history
                    (timestamp, type, description, data, size_bytes, source_peer, data_hash,
                     category, categories, category_confidence, classifier_version)
                 VALUES ('2026-02-01T10:00:00Z', 'text', 'example.com', ?1, ?2,
                         'self', ?3, 'text', '[\"text\"]', 75, 2)",
                params![encrypted, text.len() as i64, hash],
            )
            .unwrap();

        assert_eq!(db.backfill_classifications(1).unwrap(), 1);
        assert_eq!(db.backfill_classifications(1).unwrap(), 0);

        let entries = db.get_all(None, None, 10, 0).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].category, "website");
        assert_eq!(entries[0].categories, vec!["website", "text"]);
        assert_eq!(
            entries[0].classifier_version,
            crate::history_classifier::CLASSIFIER_VERSION
        );
        assert_eq!(db.get_all(None, Some("website"), 10, 0).unwrap().len(), 1);
        assert_eq!(db.get_all(None, Some("text"), 10, 0).unwrap().len(), 1);

        let encoded: String = db
            .conn
            .query_row("SELECT categories FROM history", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Vec<String>>(&encoded).unwrap(),
            vec!["website", "text"]
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn combined_filters_use_secondary_labels_and_validate_date_ranges() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-category-combined-filter-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let db = test_database(&root);
        db.conn
            .execute_batch(
                "INSERT INTO history
                    (timestamp, type, description, data, size_bytes, source_peer, data_hash,
                     category, categories, category_confidence, classifier_version)
                 VALUES
                    ('2026-02-01T18:00:00+08:00', 'text', 'needle at start', X'00', 1,
                     'Mac', 'start', 'website', '[\"website\",\"text\"]', 93, 3),
                    ('2026-02-01T19:00:00+08:00', 'text', 'needle at end', X'00', 1,
                     'Mac', 'end', 'website', '[\"website\",\"text\"]', 93, 3),
                    ('2026-02-01T18:30:00+08:00', 'text', 'unrelated', X'00', 1,
                     'Mac', 'other', 'website', '[\"website\",\"text\"]', 93, 3),
                    ('2026-02-01T18:30:00+08:00', 'text', 'needle primary only', X'00', 1,
                     'self', 'primary', 'website', '[\"website\"]', 99, 3);",
            )
            .unwrap();

        let results = db
            .get_all_filtered(
                Some("needle"),
                Some("text"),
                Some("2026-02-01T10:00:00Z"),
                Some("2026-02-01T11:00:00Z"),
                10,
                0,
            )
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].data_hash, "start");
        assert_eq!(results[0].categories, vec!["website", "text"]);
        assert_eq!(
            db.count_all_filtered(
                Some("needle"),
                Some("text"),
                Some("2026-02-01T10:00:00Z"),
                Some("2026-02-01T11:00:00Z"),
            )
            .unwrap(),
            1
        );

        let invalid = db
            .get_all_filtered(None, None, Some("2026-02-30T10:00:00Z"), None, 10, 0)
            .unwrap_err()
            .to_string();
        assert!(invalid.contains("Invalid start_time"));

        let reversed = db
            .get_all_filtered(
                None,
                None,
                Some("2026-02-01T11:00:00Z"),
                Some("2026-02-01T10:00:00Z"),
                10,
                0,
            )
            .unwrap_err()
            .to_string();
        assert_eq!(reversed, "start_time must be earlier than end_time");

        let equal = db
            .get_all_filtered(
                None,
                None,
                Some("2026-02-01T10:00:00Z"),
                Some("2026-02-01T10:00:00Z"),
                10,
                0,
            )
            .unwrap_err()
            .to_string();
        assert_eq!(equal, "start_time must be earlier than end_time");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn new_entries_persist_categories_and_category_queries_page_stably() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-category-query-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let mut db = test_database(&root);
        db.add_text("https://example.com/docs", "Mac").unwrap();
        db.add_text("const answer = 42;", "Mac").unwrap();
        db.add_text("ordinary note", "self").unwrap();
        db.add_image(&packed_test_image(1, 1, 0x7b), "Mac").unwrap();
        db.add_file("report.pdf", b"file bytes", "Mac").unwrap();
        db.conn
            .execute("UPDATE history SET timestamp = '2026-01-01T00:00:00Z'", [])
            .unwrap();

        let websites = db.get_all(None, Some("website"), 10, 0).unwrap();
        let code = db.get_all(Some("Mac"), Some("code"), 10, 0).unwrap();
        let first_page = db.get_all(None, None, 2, 0).unwrap();
        let second_page = db.get_all(None, None, 2, 2).unwrap();
        let image_category: String = db
            .conn
            .query_row(
                "SELECT category FROM history WHERE type = 'image'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let file_category: String = db
            .conn
            .query_row(
                "SELECT category FROM history WHERE type = 'file'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(websites.len(), 1);
        assert_eq!(websites[0].category, "website");
        assert_eq!(code.len(), 1);
        assert_eq!(code[0].category, "code");
        assert!(first_page[0].id > first_page[1].id);
        assert!(first_page[1].id > second_page[0].id);
        assert_eq!(image_category, "image");
        assert_eq!(file_category, "file");
        assert!(db.get_all(None, Some("unsupported"), 10, 0).is_err());
        db.conn
            .execute(
                "UPDATE history
                 SET category = 'text', category_confidence = 0, classifier_version = 0
                 WHERE type = 'image'",
                [],
            )
            .unwrap();
        assert_eq!(db.backfill_classifications(10).unwrap(), 1);
        let repaired_image_category: String = db
            .conn
            .query_row(
                "SELECT category FROM history WHERE type = 'image'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(repaired_image_category, "image");

        db.clear_all().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn backfill_repairs_legacy_text_references_and_marks_corrupt_rows() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-category-backfill-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let mut db = test_database(&root);
        let text = b"git status --short";
        let hash = blake3::hash(text).to_hex().to_string();
        let legacy_reference = persist_image_at(&db.image_history_dir, &hash, text).unwrap();
        let legacy_path = resolve_file_reference_at(
            &db.image_history_dir,
            &decode_image_reference(&legacy_reference).unwrap(),
        )
        .unwrap();
        db.conn
            .execute(
                "INSERT INTO history
                    (timestamp, type, description, data, size_bytes, source_peer, data_hash)
                 VALUES ('2026-01-01T00:00:00Z', 'text', 'git status --short', ?1, ?2, 'migrated', ?3)",
                params![legacy_reference, text.len() as i64, hash],
            )
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO history
                    (timestamp, type, description, data, size_bytes, source_peer, data_hash)
                 VALUES ('2026-01-02T00:00:00Z', 'text', 'broken', X'010203', 3, 'migrated', 'broken')",
                [],
            )
            .unwrap();
        let regular_text = b"example.com";
        let regular_hash = blake3::hash(regular_text).to_hex().to_string();
        let regular_encrypted = crypto::encrypt(regular_text).unwrap();
        db.conn
            .execute(
                "INSERT INTO history
                    (timestamp, type, description, data, size_bytes, source_peer, data_hash,
                     category, category_confidence, classifier_version)
                 VALUES ('2026-01-03T00:00:00Z', 'text', 'website', ?1, ?2, 'self', ?3,
                         'text', 75, 1)",
                params![regular_encrypted, regular_text.len() as i64, regular_hash],
            )
            .unwrap();

        assert_eq!(db.backfill_classifications(50).unwrap(), 3);
        assert_eq!(db.backfill_classifications(50).unwrap(), 0);

        let repaired: (i64, String, i64, i64) = db
            .conn
            .query_row(
                "SELECT id, category, category_confidence, classifier_version
                 FROM history WHERE data_hash = ?1",
                params![hash],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        let corrupt: (String, i64, i64) = db
            .conn
            .query_row(
                "SELECT category, category_confidence, classifier_version
                 FROM history WHERE data_hash = 'broken'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        let regular_category: String = db
            .conn
            .query_row(
                "SELECT category FROM history WHERE data_hash = ?1",
                params![regular_hash],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(repaired.1, "command");
        assert!(repaired.2 >= 90);
        assert_eq!(repaired.3, crate::history_classifier::CLASSIFIER_VERSION);
        assert_eq!(db.get_data(repaired.0).unwrap(), text);
        assert!(!legacy_path.exists());
        assert_eq!(regular_category, "website");
        assert_eq!(
            corrupt,
            (
                "text".to_string(),
                0,
                crate::history_classifier::CLASSIFIER_VERSION,
            )
        );

        let deletable_text = b"legacy text reference for deletion";
        let deletable_hash = blake3::hash(deletable_text).to_hex().to_string();
        let deletable_reference =
            persist_image_at(&db.image_history_dir, &deletable_hash, deletable_text).unwrap();
        let deletable_path = resolve_file_reference_at(
            &db.image_history_dir,
            &decode_image_reference(&deletable_reference).unwrap(),
        )
        .unwrap();
        db.conn
            .execute(
                "INSERT INTO history
                    (timestamp, type, description, data, size_bytes, source_peer, data_hash)
                 VALUES ('2026-01-04T00:00:00Z', 'text', 'deletable', ?1, ?2, 'migrated', ?3)",
                params![
                    deletable_reference,
                    deletable_text.len() as i64,
                    deletable_hash
                ],
            )
            .unwrap();
        let deletable_id = db.conn.last_insert_rowid();
        assert!(deletable_path.is_file());
        db.delete(deletable_id).unwrap();
        assert!(!deletable_path.exists());

        db.clear_all().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
