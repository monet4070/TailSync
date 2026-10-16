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
            read_identity: std::sync::Arc::new(()),
            #[cfg(any(test, feature = "test-support"))]
            storage_work_observer: None,
            deferred_checkpoint: false,
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
            read_identity: std::sync::Arc::new(()),
            // A reader may have prevented a pre-restart delete checkpoint.
            // Reuse the maintenance retry once on every disk open rather
            // than persisting a second deletion ledger.
            #[cfg(any(test, feature = "test-support"))]
            storage_work_observer: None,
            deferred_checkpoint: true,
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
        let replacement = Self::open_at(&get_storage_dir())?;
        self.adopt_replacement(replacement)
    }

    /// Swap in a freshly opened persistent store, carrying over anything the
    /// current store holds that has no persistent home yet.
    ///
    /// When the daemon started while storage was unavailable it served a
    /// degraded in-memory database and may have accepted (and acknowledged)
    /// history that only exists there. Replacing the store must not silently
    /// drop that data, so any such rows are migrated into the persistent store
    /// first, keyed by content hash so the recovery is idempotent. If migration
    /// fails the current store is left untouched and the caller can retry, so a
    /// failed handoff never loses the degraded history or claims success.
    pub(super) fn adopt_replacement(
        &mut self,
        mut replacement: HistoryDB,
    ) -> Result<(), Box<dyn std::error::Error>> {
        replacement.max_history = self.max_history;
        replacement.storage_quota_bytes = self.storage_quota_bytes;
        // Only a store that lost availability needs its rows carried over. A
        // disk store that merely lost availability is reopened from the same
        // file, and the content-hash guard makes that copy a no-op, so the
        // migration is safe regardless of the underlying storage kind.
        if !self.storage_available {
            let migrated = migrate_degraded_history_into(&self.conn, &replacement.conn)?;
            if migrated > 0 {
                info!("Migrated {migrated} degraded history entr(ies) into persistent storage");
            }
        }
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

/// Copy history rows that only exist in a degraded in-memory store into a
/// persistent database. Rows whose content hash is already present in the
/// target are skipped, so the recovery is idempotent and a partial migration
/// can be retried without duplicating entries. Returns the number of rows
/// inserted; any SQL failure aborts before the store swap happens.
pub(super) fn migrate_degraded_history_into(
    source: &Connection,
    target: &Connection,
) -> Result<usize, Box<dyn std::error::Error>> {
    let mut select = source.prepare(
        "SELECT timestamp, type, description, data, size_bytes, source_peer, data_hash,
                category, categories, category_confidence, classifier_version, pinned,
                batch_id, batch_index, batch_total, batch_status, created_at
         FROM history
         ORDER BY id",
    )?;
    let transaction = target.unchecked_transaction()?;
    let mut insert = transaction.prepare(
        "INSERT INTO history
            (timestamp, type, description, data, size_bytes, source_peer, data_hash,
             category, categories, category_confidence, classifier_version, pinned,
             batch_id, batch_index, batch_total, batch_status, created_at)
         SELECT ?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17
         WHERE NOT EXISTS (SELECT 1 FROM history WHERE data_hash = ?7 AND type = ?2)",
    )?;
    let mut rows = select.query([])?;
    let mut migrated = 0usize;
    while let Some(row) = rows.next()? {
        let timestamp: String = row.get(0)?;
        let entry_type: String = row.get(1)?;
        let description: String = row.get(2)?;
        let data: Vec<u8> = row.get(3)?;
        let size_bytes: i64 = row.get(4)?;
        let source_peer: String = row.get(5)?;
        let data_hash: String = row.get(6)?;
        let category: String = row.get(7)?;
        let categories: String = row.get(8)?;
        let category_confidence: i64 = row.get(9)?;
        let classifier_version: i64 = row.get(10)?;
        let pinned: i64 = row.get(11)?;
        let batch_id: Option<String> = row.get(12)?;
        let batch_index: Option<i64> = row.get(13)?;
        let batch_total: Option<i64> = row.get(14)?;
        let batch_status: String = row.get(15)?;
        let created_at: String = row.get(16)?;
        // Existing payload wins; a favorite in either store remains protected.
        transaction.execute(
            "UPDATE history SET pinned = MAX(pinned, ?1) WHERE data_hash = ?2 AND type = ?3",
            params![pinned, data_hash, entry_type],
        )?;
        migrated += insert.execute(params![
            timestamp,
            entry_type,
            description,
            data,
            size_bytes,
            source_peer,
            data_hash,
            category,
            categories,
            category_confidence,
            classifier_version,
            pinned,
            batch_id,
            batch_index,
            batch_total,
            batch_status,
            created_at,
        ])?;
    }
    drop(insert);
    transaction.commit()?;
    Ok(migrated)
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
