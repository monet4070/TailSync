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
