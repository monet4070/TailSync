use log::{info, warn};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use thiserror::Error;

use crate::crypto;
use crate::history_classifier::{self, Classification};

mod composite;
pub use composite::RemoteArchiveOutcome;
mod display;
mod favorites;
mod file_encryption;
mod file_storage;
mod legacy_v1;
mod lifecycle;
mod migrations;
mod paths;
mod queries;
mod reads;
mod schema;
mod storage;
#[cfg(any(test, feature = "test-support"))]
mod test_support;
mod types;

pub(crate) use file_storage::materialize_remote_clipboard_file_at;
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
use file_storage::{materialize_clipboard_bytes_at, StoredFileReference, FILE_HISTORY_BYTE_LIMIT};
#[cfg(any(test, feature = "test-support"))]
pub use paths::default_user_data_dir_for_test;
pub use paths::{
    configure_storage_dir, configure_storage_parent, get_clipboard_files_dir, get_data_dir,
    get_file_history_dir, get_history_db_path, get_image_history_dir, get_incoming_dir,
    get_storage_dir, validate_storage_dir, STORAGE_DIRECTORY_NAME,
};
pub use reads::{
    HistoryReadChunk, HistoryReadCursor, HistoryReadError, HistoryReadRevision, PreparedPreview,
};
/// Pre-upgrade rollback package surface (repair plan §4 R1-03).
///
/// `prepare` runs automatically before a v11→v12 upgrade; `verify` and `retire`
/// are the explicit maintenance actions — the package is never released
/// implicitly, because it holds the only pre-upgrade state.
pub use rollback::{
    package_dir as rollback_package_dir, prepare as prepare_rollback_package,
    restore_into as restore_rollback_package, restore_v13_into as restore_v13_rollback_package,
    retire as retire_rollback_package, verify as verify_rollback_package,
    verify_v13 as verify_v13_rollback_package, RollbackFile, RollbackManifest,
};
pub use storage::{
    delete_old_storage, migrate_storage_with_rollback, storage_status_async,
    StorageMigrationFailure, StorageMigrationHooks,
};

pub use composite::{AnalysisHandle, ContentAnalysisInput, PreparedContentAnalysis};
pub use types::{
    CompositeInfo, FavoriteMutation, FileBatchWriteOptions, FileEncryptionMigrationBatch,
    HistoryCollection, HistoryDisplayInfo, HistoryEntry, HistoryFileInput, HistoryMutationError,
    HistoryQuery, HistoryQueryPage, MigrationDiagnostics, MigrationIssue, PreviewBatchNavigation,
    PreviewErrorCode, PreviewErrorInfo, PreviewKind, PreviewMetadata, PreviewPayload,
    StorageMigrationResult, StorageStatus, WithheldRestoreFormat,
};

/// Database schema version
const SCHEMA_VERSION: i64 = 13;
const COMPOSITE_SCHEMA_VERSION: i64 = 12;

/// Highest schema version a build that does not enable composite clipboard writes.
pub const LEGACY_SCHEMA_VERSION: i64 = 11;

/// How far one open is allowed to evolve the database schema.
///
/// The composite feature adds persistent structures, so "the feature is off" has
/// to mean "this database is not upgraded", not merely "negotiation is off".
/// Reading a database that already holds composite history is data preservation and
/// stays allowed under [`MigrationPolicy::LegacyOnly`]; *creating* composite
/// structures is not. See `docs/adr/ADR-004-composite-clipboard-contracts.md` (K8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationPolicy {
    /// Never create composite structures, and never migrate a legacy database past
    /// [`LEGACY_SCHEMA_VERSION`].
    LegacyOnly,
    /// Composite structures may be created and a legacy database may be upgraded.
    CompositeAllowed,
}

impl MigrationPolicy {
    /// The schema version this open may create from scratch.
    pub const fn write_target(self) -> i64 {
        match self {
            Self::LegacyOnly => LEGACY_SCHEMA_VERSION,
            Self::CompositeAllowed => SCHEMA_VERSION,
        }
    }

    /// The version this open must actually reach, given that the database — or the
    /// degraded store being carried over — already sits at `existing`.
    ///
    /// Adopting existing composite history needs a destination at least as new as
    /// the source, otherwise the carry-over would drop already-acknowledged
    /// content. That is a data-preservation requirement, not a decision to enable
    /// new capture, so it applies under `LegacyOnly` too.
    pub const fn effective_target(self, existing: i64) -> i64 {
        let target = self.write_target();
        if existing > target && existing <= SCHEMA_VERSION {
            existing
        } else {
            target
        }
    }
}

/// The migration policy the running product uses.
///
/// Composite clipboard is enabled only when the build enables the feature **and**
/// the operator has not disabled it at runtime. That pair used to gate negotiation
/// and automatic capture while the schema migrated regardless; K8 makes it also
/// decide whether an open may create composite structures.
///
/// R1-04 extends this with the startup helper-health probe, so an unhealthy helper
/// keeps the database at v11 instead of advertising a capability the platform
/// cannot service.
pub fn production_migration_policy() -> MigrationPolicy {
    if crate::secure::composite_capture_serviceable() {
        MigrationPolicy::CompositeAllowed
    } else {
        MigrationPolicy::LegacyOnly
    }
}

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
        let code = error.code();
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
            code,
            message: public_preview_message(code).to_string(),
            entry_id,
            size_bytes,
            limit_bytes,
            retryable,
        }
    }
}

impl PreviewErrorInfo {
    pub fn payload_unavailable(entry_id: i64, _message: impl Into<String>) -> Self {
        Self {
            code: PreviewErrorCode::PayloadUnavailable,
            message: public_preview_message(PreviewErrorCode::PayloadUnavailable).to_string(),
            entry_id: Some(entry_id),
            size_bytes: None,
            limit_bytes: None,
            retryable: false,
        }
    }
}

/// The local UI receives this text. Source errors may contain private paths,
/// peer identities, SQL details, or clipboard content, so only fixed phrases
/// are allowed into the preview error contract.
const fn public_preview_message(code: PreviewErrorCode) -> &'static str {
    match code {
        PreviewErrorCode::EntryNotFound => "History entry is unavailable.",
        PreviewErrorCode::BatchNotFound => "History batch is unavailable.",
        PreviewErrorCode::EntryNotInBatch => "History entry is not in this batch.",
        PreviewErrorCode::MetadataUnavailable => "Preview metadata is unavailable.",
        PreviewErrorCode::PayloadUnavailable => "Preview payload is unavailable.",
        PreviewErrorCode::PreviewTooLarge => "Preview payload is too large.",
        PreviewErrorCode::UnsupportedType => "Preview type is unsupported.",
        PreviewErrorCode::InvalidSize => "History entry has an invalid size.",
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
        "composite" => Ok((PreviewKind::Text, "composite.txt".to_string())),
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

fn remove_unreferenced_persisted_files(db: &HistoryDB, persisted: &[(Vec<u8>, PathBuf)]) {
    // Rollback after partial batch write. Use file identity to protect live
    // references (a favorited file could share the same physical file as one
    // of these batch entries via case-insensitive aliasing).
    let paths: Vec<_> = persisted.iter().map(|(_, path)| path.clone()).collect();
    db.remove_unreferenced_payload_paths(&paths, None);
}

pub struct HistoryDB {
    protected_history: std::collections::HashSet<i64>,
    conn: Connection,
    read_identity: std::sync::Arc<()>,
    display_authority: u128,
    deferred_checkpoint: bool,
    #[cfg(any(test, feature = "test-support"))]
    storage_work_observer: Option<std::sync::Arc<dyn Fn(&'static str) + Send + Sync>>,
    max_history: i64,
    storage_quota_bytes: u64,
    storage_available: bool,
    file_history_dir: PathBuf,
    image_history_dir: PathBuf,
    /// Whether `history_parts` / `history_composite_events` exist in this database.
    ///
    /// A v11 database opened under [`MigrationPolicy::LegacyOnly`] has neither, so
    /// every composite-table query must be conditional. Cached at open time because
    /// delete, trim and GC paths consult it on hot paths.
    has_composite_schema: bool,
}

/// Owns a prepared history row's quota protection through blocking work. Dropping
/// an async receiver cannot leave a permanent in-memory migration/trim blocker.
pub struct PendingHistoryProtection {
    database: std::sync::Arc<tokio::sync::Mutex<HistoryDB>>,
    id: i64,
    runtime: tokio::runtime::Handle,
}
impl PendingHistoryProtection {
    pub fn new(
        database: std::sync::Arc<tokio::sync::Mutex<HistoryDB>>,
        id: i64,
    ) -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            database,
            id,
            runtime: tokio::runtime::Handle::current(),
        })
    }
}
impl Drop for PendingHistoryProtection {
    fn drop(&mut self) {
        let database = self.database.clone();
        let id = self.id;
        self.runtime.spawn(async move {
            database.lock().await.release_history(id);
        });
    }
}

impl HistoryDB {
    pub fn protect_history(&mut self, id: i64) {
        if id > 0 {
            self.protected_history.insert(id);
        }
    }
    pub fn release_history(&mut self, id: i64) {
        self.protected_history.remove(&id);
    }
    #[cfg(any(test, feature = "test-support"))]
    pub fn protected_history_count_for_test(&self) -> usize {
        self.protected_history.len()
    }

    /// Whether this database carries the composite tables.
    ///
    /// Composite-specific reads, deletes and GC must be skipped when this is false:
    /// on a legacy database those tables legitimately do not exist.
    pub(crate) const fn has_composite_schema(&self) -> bool {
        self.has_composite_schema
    }
}

mod classification;
mod entries;
mod files;
mod migration_entries;
mod open;
mod preview;
mod rollback;

#[cfg(test)]
mod tests;
