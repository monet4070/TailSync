use super::*;

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
        ensure_supported_schema(&conn)?;
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

    /// Return the last known storage availability without walking the managed
    /// tree. UI status indicators should use this cheap health bit; callers
    /// that need quota accounting can still request [`storage_status`].
    pub fn is_storage_available(&self) -> bool {
        self.storage_available
    }
}

/// Refuse to let an older binary initialize or migrate a database created by
/// a newer binary. This check must run before `schema::initialize`, since that
/// function creates current-version tables and indexes with `IF NOT EXISTS`.
fn ensure_supported_schema(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    let has_schema_version: bool = conn.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'schema_version'
        )",
        [],
        |row| row.get(0),
    )?;
    if !has_schema_version {
        return Ok(());
    }

    let version: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_version",
        [],
        |row| row.get(0),
    )?;
    if version > SCHEMA_VERSION {
        return Err(format!(
            "database schema version {version} is newer than supported version {SCHEMA_VERSION}"
        )
        .into());
    }
    Ok(())
}
