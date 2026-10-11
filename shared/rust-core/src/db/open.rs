use super::*;

impl HistoryDB {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        Self::open_at_with_policy(&get_storage_dir(), production_migration_policy())
    }

    /// Open an existing store without ever creating composite structures.
    ///
    /// A test/dev entry point: production opens go through [`Self::new`] and the
    /// reopen paths, which pass [`production_migration_policy`]. A database that
    /// already holds composite history still opens at v12 (adoption); only the
    /// *creation* of composite structures is withheld.
    ///
    /// `cfg(test)` rather than `test-support`: every caller is an in-crate test
    /// module, and a `test-support`-only build would otherwise see it as dead code.
    #[cfg(test)]
    pub(crate) fn open_at(storage_dir: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        Self::open_at_with_policy(storage_dir, MigrationPolicy::LegacyOnly)
    }

    pub fn new_unavailable() -> Result<Self, Box<dyn std::error::Error>> {
        Self::new_unavailable_with_policy(production_migration_policy())
    }

    /// Degraded in-memory store. The policy decides whether it carries composite
    /// tables, so a degraded store can still accept the composite history the
    /// running product is configured to receive.
    pub fn new_unavailable_with_policy(
        policy: MigrationPolicy,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        schema::initialize(&conn)?;
        let target = policy.write_target();
        if target >= COMPOSITE_SCHEMA_VERSION {
            schema::initialize_composite(&conn)?;
        }
        Self::migrate_to(
            &conn,
            &get_file_history_dir(),
            &get_image_history_dir(),
            target,
        )?;
        let has_composite_schema = schema::composite_schema_present(&conn)?;
        Ok(Self {
            conn,
            read_identity: std::sync::Arc::new(()),
            display_authority: rand::random(),
            #[cfg(any(test, feature = "test-support"))]
            storage_work_observer: None,
            deferred_checkpoint: false,
            max_history: 1000,
            storage_quota_bytes: crypto::DEFAULT_STORAGE_QUOTA_BYTES,
            storage_available: false,
            file_history_dir: get_file_history_dir(),
            image_history_dir: get_image_history_dir(),
            has_composite_schema,
            protected_history: std::collections::HashSet::new(),
        })
    }

    pub(crate) fn open_at_with_policy(
        storage_dir: &Path,
        policy: MigrationPolicy,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Self::open_at_with_minimum_policy(storage_dir, policy, 0)
    }

    pub(super) fn open_at_with_minimum_policy(
        storage_dir: &Path,
        policy: MigrationPolicy,
        minimum_schema: i64,
    ) -> Result<Self, Box<dyn std::error::Error>> {
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
        // The version already on disk bounds what this open must reach: adopting an
        // existing v12 history is data preservation, not enabling new capture.
        let existing = schema::schema_version(&conn)?;
        let target = policy.effective_target(existing).max(minimum_schema);
        let file_history_dir = storage_dir.join("file-history");
        let image_history_dir = storage_dir.join("image-history");
        // A real v11 -> v12 upgrade must first produce a *complete* rollback package
        // (consistent SQLite snapshot + every live payload + digests), and must not
        // proceed without one. R1-03: a database-only snapshot is not a rollback
        // artefact because the payload files are not in it.
        if target >= COMPOSITE_SCHEMA_VERSION && existing > 0 && existing < COMPOSITE_SCHEMA_VERSION
        {
            rollback::prepare(
                &conn,
                &db_path,
                storage_dir,
                &file_history_dir,
                &image_history_dir,
                existing,
            )?;
        }

        // Enable WAL mode for concurrent read/write. secure_delete overwrites
        // deleted cells in the main database; explicit deletes also truncate WAL.
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA secure_delete = ON;")?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;

        schema::initialize(&conn)?;

        // SQLite creates -wal/-shm under the umask; lock them down now that
        // they exist (and again on every later open via enforce_private_tree).
        crate::private_fs::enforce_private_database(&db_path)?;

        // Save a v12 snapshot before v13; do not overwrite the pre-v12 package.
        if target >= 13 && existing > 0 && existing < 13 {
            Self::migrate_to(&conn, &file_history_dir, &image_history_dir, 12)?;
            rollback::prepare_for(
                &conn,
                &db_path,
                storage_dir,
                &file_history_dir,
                &image_history_dir,
                12,
                13,
            )?;
        }
        Self::migrate_to(&conn, &file_history_dir, &image_history_dir, target)?;

        // A database that already reports v12 keeps its composite structures; make
        // sure they exist before any composite query is allowed to run.
        if target >= COMPOSITE_SCHEMA_VERSION && !schema::composite_schema_present(&conn)? {
            schema::initialize_composite(&conn)?;
        }
        let has_composite_schema = schema::composite_schema_present(&conn)?;

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
            display_authority: rand::random(),
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
            has_composite_schema,
            protected_history: std::collections::HashSet::new(),
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
        let replacement = Self::open_at_with_minimum_policy(
            &get_storage_dir(),
            production_migration_policy(),
            schema::schema_version(&self.conn)?,
        )?;
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
            let migrated = migrate_degraded_history_into(
                &self.conn,
                &replacement.conn,
                self.has_composite_schema(),
                replacement.has_composite_schema(),
            )?;
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
    source_has_composite: bool,
    target_has_composite: bool,
) -> Result<usize, Box<dyn std::error::Error>> {
    // A v11 store has no `history_parts` / `history_composite_events`; querying
    // them unconditionally would fail the whole carry-over on a legacy database.
    let carry_composite = source_has_composite && target_has_composite;
    if source_has_composite && !target_has_composite {
        return Err("Composite recovery requires a compatible destination".into());
    }
    let source_has_identity = schema::identity_schema_present(source)?;
    let target_has_identity = schema::identity_schema_present(target)?;
    if source_has_identity && !target_has_identity {
        return Err("v13 recovery requires an identity-capable destination".into());
    }
    let mut select = source.prepare(
        "SELECT timestamp, type, description, data, size_bytes, source_peer, data_hash,
                category, categories, category_confidence, classifier_version, pinned,
                batch_id, batch_index, batch_total, batch_status, created_at, id
         FROM history
         ORDER BY id",
    )?;
    let transaction = target.unchecked_transaction()?;
    let mut insert = transaction.prepare(
        "INSERT INTO history
            (timestamp, type, description, data, size_bytes, source_peer, data_hash,
             category, categories, category_confidence, classifier_version, pinned,
             batch_id, batch_index, batch_total, batch_status, created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
    )?;
    let mut rows = select.query([])?;
    let mut migrated = 0usize;
    let mut identities = std::collections::BTreeMap::new();
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
        let source_id: i64 = row.get(17)?;
        let capture: Option<String> = if source_has_identity {
            source
                .query_row(
                    "SELECT capture_id FROM history_composite_identity WHERE history_id=?1",
                    [source_id],
                    |r| r.get(0),
                )
                .optional()?
        } else {
            None
        };
        let existing: Option<i64> = if entry_type == "composite" {
            if let Some(capture) = &capture {
                transaction
                    .query_row(
                        "SELECT history_id FROM history_composite_identity WHERE capture_id=?1",
                        [capture],
                        |r| r.get(0),
                    )
                    .optional()?
            } else {
                transaction.query_row("SELECT id FROM history WHERE type='composite' AND data_hash=?1 AND data=?2 ORDER BY id LIMIT 1", params![data_hash,data], |r| r.get(0)).optional()?
            }
        } else {
            transaction
                .query_row(
                    "SELECT id FROM history WHERE data_hash=?1 AND type=?2 ORDER BY id LIMIT 1",
                    params![data_hash, entry_type],
                    |r| r.get(0),
                )
                .optional()?
        };
        if let Some(target_id) = existing {
            if entry_type == "composite" && capture.is_some() {
                let (persisted, persisted_hash): (Vec<u8>, String) = transaction.query_row(
                    "SELECT data,data_hash FROM history WHERE id=?1",
                    [target_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?;
                if persisted != data || persisted_hash != data_hash {
                    return Err(
                        "Composite recovery archive identity conflict: preserve both stores".into(),
                    );
                }
            }
            transaction.execute(
                "UPDATE history SET pinned=MAX(pinned,?1) WHERE id=?2",
                params![pinned, target_id],
            )?;
            identities.insert(source_id, target_id);
            continue;
        }
        let inserted = insert.execute(params![
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
        migrated += inserted;
        let target_id = transaction.last_insert_rowid();
        if capture.is_some() {
            let values = source.query_row("SELECT version,capture_id,content_id,dedupe_key,archive_revision,archive_digest FROM history_composite_identity WHERE history_id=?1", [source_id], |r| Ok(crate::composite::identity::HistoryIdentity {
                version:r.get(0)?,capture_id:r.get(1)?,content_id:r.get(2)?,dedupe_key:r.get(3)?,archive_revision:r.get(4)?,archive_digest:r.get(5)?
            }))?;
            composite::insert_identity(&transaction, target_id, &values)?;
        }
        identities.insert(row.get::<_, i64>(17)?, target_id);
        if inserted != 0 && entry_type == "composite" && carry_composite {
            let mut parts = source.prepare("SELECT part_id,kind,mime,data_hash,size_bytes,stored FROM history_parts WHERE history_id=?1")?;
            let mut part_rows = parts.query([row.get::<_, i64>(17)?])?;
            while let Some(part) = part_rows.next()? {
                transaction.execute("INSERT INTO history_parts(history_id,part_id,kind,mime,data_hash,size_bytes,stored) VALUES (?1,?2,?3,?4,?5,?6,?7)",params![target_id,part.get::<_,i64>(0)?,part.get::<_,String>(1)?,part.get::<_,String>(2)?,part.get::<_,String>(3)?,part.get::<_,i64>(4)?,part.get::<_,Vec<u8>>(5)?])?;
            }
        }
    }
    if !carry_composite {
        drop(insert);
        transaction.commit()?;
        return Ok(migrated);
    }
    let mut events=source.prepare("SELECT source_device_id,event_id,digest,history_id,timestamp_ms FROM history_composite_events")?;
    let mut event_rows = events.query([])?;
    while let Some(row) = event_rows.next()? {
        let source_id: String = row.get(0)?;
        let event_id: String = row.get(1)?;
        let digest: String = row.get(2)?;
        let previous:Option<String>=transaction.query_row("SELECT digest FROM history_composite_events WHERE source_device_id=?1 AND event_id=?2",params![source_id,event_id],|r|r.get(0)).optional()?;
        if previous.as_ref().is_some_and(|old| old != &digest) {
            return Err("Composite recovery identity collision".into());
        }
        let old: Option<i64> = row.get(3)?;
        let new = old.and_then(|id| identities.get(&id).copied());
        transaction.execute("INSERT OR IGNORE INTO history_composite_events(source_device_id,event_id,digest,history_id,timestamp_ms) VALUES (?1,?2,?3,?4,?5)",params![source_id,event_id,digest,new,row.get::<_,i64>(4)?])?;
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
