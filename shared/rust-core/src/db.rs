use log::{info, warn};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use thiserror::Error;

use crate::crypto;
use crate::history_classifier::{self, Classification};

mod favorites;
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
    FavoriteMutation, FileEncryptionMigrationBatch, HistoryCollection, HistoryEntry,
    HistoryFileInput, HistoryMutationError, HistoryQuery, HistoryQueryPage, MigrationDiagnostics,
    MigrationIssue, PreviewBatchNavigation, PreviewErrorCode, PreviewErrorInfo, PreviewKind,
    PreviewMetadata, PreviewPayload, StorageMigrationResult, StorageStatus,
};

/// Database schema version
const SCHEMA_VERSION: i64 = 10;

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

mod classification;
mod entries;
mod files;
mod migration_entries;
mod open;
mod preview;

#[cfg(test)]
mod tests;
