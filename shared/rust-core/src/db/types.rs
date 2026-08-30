use thiserror::Error;

/// The collection used by a history query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryCollection {
    All,
    Favorites,
}

impl HistoryCollection {
    pub fn from_wire(value: Option<&str>) -> Result<Self, HistoryMutationError> {
        match value.unwrap_or("all") {
            "all" => Ok(Self::All),
            "favorites" => Ok(Self::Favorites),
            other => Err(HistoryMutationError::InvalidCollection {
                collection: other.to_string(),
            }),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Favorites => "favorites",
        }
    }
}

/// Stable errors for user-initiated history mutations.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum HistoryMutationError {
    #[error("history entry {id} is unavailable")]
    EntryNotFound { id: i64 },
    #[error("history entry {id} is protected because it is favorited")]
    FavoriteProtected { id: i64 },
    #[error("history entry {id} is not favorited")]
    NotFavorite { id: i64 },
    #[error("unsupported history collection: {collection}")]
    InvalidCollection { collection: String },
}

/// Result of changing the favorite state of a logical history item.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FavoriteMutation {
    pub affected_ids: Vec<i64>,
    pub favorite: bool,
}

/// A clipboard history entry.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HistoryEntry {
    pub id: i64,
    pub timestamp: String,
    #[serde(rename = "type")]
    pub entry_type: String,
    pub description: String,
    pub data_hash: String,
    pub size_bytes: i64,
    pub source_peer: String,
    pub category: String,
    pub categories: Vec<String>,
    pub category_confidence: i64,
    pub classifier_version: i64,
    pub pinned: bool,
    pub batch_id: Option<String>,
    pub batch_index: Option<i64>,
    pub batch_total: Option<i64>,
    pub batch_count: Option<i64>,
    pub batch_status: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct HistoryQueryPage {
    pub entries: Vec<HistoryEntry>,
    pub total: Option<usize>,
    pub has_more: bool,
}

/// Inputs for a paged history collection query.
#[derive(Debug, Clone)]
pub struct HistoryQuery<'a> {
    pub collection: HistoryCollection,
    pub keyword: Option<&'a str>,
    pub category: Option<&'a str>,
    pub start_time: Option<&'a str>,
    pub end_time: Option<&'a str>,
    pub limit: usize,
    pub offset: usize,
}

/// The storage-level kind of a history item that can be previewed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PreviewKind {
    Text,
    Image,
    File,
}

impl PreviewKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Image => "image",
            Self::File => "file",
        }
    }
}

/// Stable error categories exposed by platform preview adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewErrorCode {
    EntryNotFound,
    BatchNotFound,
    EntryNotInBatch,
    MetadataUnavailable,
    PayloadUnavailable,
    PreviewTooLarge,
    UnsupportedType,
    InvalidSize,
}

/// Stable, transport-neutral preview failure exposed by platform adapters.
///
/// Platform code serializes this value using its native IPC framing. Keeping
/// retryability and size details here prevents Windows and macOS from
/// interpreting free-form error messages differently.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PreviewErrorInfo {
    pub code: PreviewErrorCode,
    pub message: String,
    pub entry_id: Option<i64>,
    pub size_bytes: Option<u64>,
    pub limit_bytes: Option<u64>,
    pub retryable: bool,
}

/// Ordered navigation state for the current item in a file batch.
///
/// `item_index` is zero-based and `item_count` is derived from the rows that
/// are actually present. This keeps incomplete batches navigable without
/// trusting stale `batch_total` metadata.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PreviewBatchNavigation {
    pub batch_id: String,
    pub item_index: usize,
    pub item_count: usize,
    pub first_entry_id: i64,
    pub last_entry_id: i64,
    pub previous_entry_id: Option<i64>,
    pub next_entry_id: Option<i64>,
}

/// Metadata needed to select a renderer without decrypting the payload.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PreviewMetadata {
    pub entry_id: i64,
    pub kind: PreviewKind,
    pub name: String,
    pub size_bytes: u64,
    pub batch: Option<PreviewBatchNavigation>,
}

/// Decrypted bytes and metadata used by the history preview surfaces.
///
/// The payload is intentionally kept in memory by callers.  File previews
/// must not reuse the clipboard materialisation path, which creates a
/// plaintext file on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewPayload {
    pub kind: String,
    pub name: String,
    pub size_bytes: u64,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct HistoryFileInput {
    pub name: String,
    pub path: std::path::PathBuf,
    pub data_hash: String,
    pub size: u64,
}

/// Options shared by complete and partial file-batch writes.
///
/// The optional receipt fields are populated only for authenticated remote
/// receives. Keeping them with the write options makes it hard for callers to
/// persist history and its idempotency identity inconsistently.
#[derive(Debug, Clone, Copy)]
pub struct FileBatchWriteOptions<'a> {
    pub expected_total: usize,
    pub source_peer: &'a str,
    pub move_sources: bool,
    pub complete: bool,
    pub source_device_id: Option<&'a str>,
    pub manifest_hash: Option<&'a str>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct StorageStatus {
    pub root: String,
    pub used_bytes: u64,
    pub quota_bytes: u64,
    pub available: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct StorageMigrationResult {
    pub new_root: String,
    pub old_root: String,
    pub old_size_bytes: u64,
}

#[derive(Debug, serde::Serialize)]
pub struct MigrationIssue {
    pub history_id: i64,
    pub migration_version: i64,
    pub issue_type: String,
    pub details: String,
    pub created_at: String,
}

#[derive(Debug, serde::Serialize)]
pub struct MigrationDiagnostics {
    pub unresolved_count: usize,
    pub issues: Vec<MigrationIssue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileEncryptionMigrationBatch {
    pub scanned: usize,
    pub migrated: usize,
    pub failed: usize,
    pub last_id: Option<i64>,
    pub complete: bool,
}
