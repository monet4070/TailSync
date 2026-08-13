use super::*;

impl HistoryDB {
    pub fn migration_diagnostics(
        &self,
        limit: usize,
    ) -> Result<MigrationDiagnostics, Box<dyn std::error::Error>> {
        let unresolved_count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM migration_issues AS issue
             WHERE issue.resolved_at IS NULL
               AND EXISTS (SELECT 1 FROM history WHERE history.id = issue.history_id)",
            [],
            |row| row.get(0),
        )?;
        let issues = self
            .conn
            .prepare(
                "SELECT history_id, migration_version, issue_type, details, created_at
                 FROM migration_issues AS issue
                 WHERE issue.resolved_at IS NULL
                   AND EXISTS (SELECT 1 FROM history WHERE history.id = issue.history_id)
                 ORDER BY created_at DESC, id DESC LIMIT ?1",
            )?
            .query_map(params![limit.min(100) as i64], |row| {
                Ok(MigrationIssue {
                    history_id: row.get(0)?,
                    migration_version: row.get(1)?,
                    issue_type: row.get(2)?,
                    details: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(MigrationDiagnostics {
            unresolved_count: unresolved_count.max(0) as usize,
            issues,
        })
    }

    pub(super) fn migrate(
        conn: &Connection,
        file_history_dir: &Path,
        image_history_dir: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Self::ensure_migration_issue_schema(conn)?;
        let version: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_version",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if version < 1 {
            info!("Running database migration v1...");
            // Base schema already created above
            conn.execute("INSERT INTO schema_version (version) VALUES (1)", [])?;
        }

        if version < 2 {
            info!("Running database migration v2...");
            // Future: add full-text search, etc.
            conn.execute("INSERT INTO schema_version (version) VALUES (2)", [])?;
        }

        if version < 3 {
            info!("Running database migration v3 (files to local history folder)...");
            let legacy_files = {
                let mut statement = conn.prepare(
                    "SELECT id, data, data_hash, description FROM history WHERE type = 'file'",
                )?;
                let rows = statement
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, Vec<u8>>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                        ))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                rows
            };

            for (id, stored, stored_hash, description) in legacy_files {
                Self::migrate_legacy_file_entry(
                    conn,
                    file_history_dir,
                    id,
                    &stored,
                    &stored_hash,
                    &description,
                )?;
            }
            conn.execute(
                "INSERT INTO schema_version (version) VALUES (?1)",
                params![3_i64],
            )?;
        }

        if version < 4 {
            info!("Running database migration v4 (images to local history folder)...");
            let legacy_images = {
                let mut statement =
                    conn.prepare("SELECT id, data, data_hash FROM history WHERE type = 'image'")?;
                let rows = statement
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, Vec<u8>>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                rows
            };
            for (id, stored, stored_hash) in legacy_images {
                Self::migrate_legacy_image_entry(
                    conn,
                    image_history_dir,
                    id,
                    &stored,
                    &stored_hash,
                )?;
            }
            conn.execute(
                "INSERT INTO schema_version (version) VALUES (?1)",
                params![4_i64],
            )?;
            conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
            let page_count: i64 = conn.query_row("PRAGMA page_count", [], |row| row.get(0))?;
            let free_pages: i64 = conn.query_row("PRAGMA freelist_count", [], |row| row.get(0))?;
            if page_count > 0 && free_pages * 4 > page_count {
                info!("Reclaiming SQLite pages after image migration...");
                conn.execute_batch("VACUUM;")?;
            }
        }

        if version < 5 {
            info!("Running database migration v5 (history categories)...");
        }
        Self::add_column_if_missing(
            conn,
            "category",
            "ALTER TABLE history ADD COLUMN category TEXT NOT NULL DEFAULT 'text'",
        )?;
        Self::add_column_if_missing(
            conn,
            "category_confidence",
            "ALTER TABLE history ADD COLUMN category_confidence INTEGER NOT NULL DEFAULT 0",
        )?;
        Self::add_column_if_missing(
            conn,
            "classifier_version",
            "ALTER TABLE history ADD COLUMN classifier_version INTEGER NOT NULL DEFAULT 0",
        )?;
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_history_category_timestamp
             ON history(category, timestamp DESC, id DESC);",
        )?;
        if version < 5 {
            conn.execute(
                "INSERT INTO schema_version (version) VALUES (?1)",
                params![5_i64],
            )?;
        }

        if version < 6 {
            info!("Running database migration v6 (multiple history labels)...");
        }
        let categories_added = Self::add_column_if_missing(
            conn,
            "categories",
            "ALTER TABLE history ADD COLUMN categories TEXT NOT NULL DEFAULT '[\"text\"]'",
        )?;
        if version < 6 || categories_added {
            conn.execute("UPDATE history SET categories = json_array(category)", [])?;
        }
        if version < 6 {
            conn.execute(
                "INSERT INTO schema_version (version) VALUES (?1)",
                params![6_i64],
            )?;
        }
        conn.execute(
            "UPDATE history
             SET category = type,
                 categories = json_array(type),
                 category_confidence = 100,
                 classifier_version = ?1
             WHERE type IN ('image', 'file')
               AND (classifier_version < ?1 OR category != type OR categories != json_array(type))",
            params![history_classifier::CLASSIFIER_VERSION],
        )?;

        if version >= 4 {
            Self::retry_unresolved_migration_issues(conn, file_history_dir, image_history_dir)?;
        }

        if version < 7 {
            info!("Running database migration v7 (enabling encrypted file history)...");
            conn.execute(
                "INSERT INTO schema_version (version) VALUES (?1)",
                params![7_i64],
            )?;
        }

        if version < 8 {
            info!("Running database migration v8 (file batches and pinned history)...");
        }
        Self::add_column_if_missing(
            conn,
            "pinned",
            "ALTER TABLE history ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0",
        )?;
        Self::add_column_if_missing(
            conn,
            "batch_id",
            "ALTER TABLE history ADD COLUMN batch_id TEXT",
        )?;
        Self::add_column_if_missing(
            conn,
            "batch_index",
            "ALTER TABLE history ADD COLUMN batch_index INTEGER",
        )?;
        Self::add_column_if_missing(
            conn,
            "batch_total",
            "ALTER TABLE history ADD COLUMN batch_total INTEGER",
        )?;
        Self::add_column_if_missing(
            conn,
            "batch_status",
            "ALTER TABLE history ADD COLUMN batch_status TEXT NOT NULL DEFAULT 'complete'",
        )?;
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_history_batch
             ON history(batch_id, batch_index);",
        )?;
        if version < 8 {
            conn.execute(
                "INSERT INTO schema_version (version) VALUES (?1)",
                params![8_i64],
            )?;
        }

        if version < 9 {
            info!("Running database migration v9 (removing plaintext text previews)...");
            conn.execute_batch("PRAGMA secure_delete = ON;")?;
            conn.execute(
                "UPDATE history SET description = ?1 WHERE type = 'text'",
                params![TEXT_DESCRIPTION_PLACEHOLDER],
            )?;
            conn.execute_batch(
                "DROP INDEX IF EXISTS idx_history_description;
                 DROP INDEX IF EXISTS idx_history_description_nontext;
                 CREATE INDEX idx_history_description_nontext
                    ON history(description) WHERE type <> 'text';",
            )?;
            conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;")?;
            // Mark v9 complete only after the residual-data cleanup succeeds.
            // If the process exits first, startup safely repeats this idempotent migration.
            conn.execute(
                "INSERT INTO schema_version (version) VALUES (?1)",
                params![SCHEMA_VERSION],
            )?;
        }

        Ok(())
    }

    fn ensure_migration_issue_schema(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS migration_issues (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                history_id INTEGER NOT NULL,
                migration_version INTEGER NOT NULL,
                issue_type TEXT NOT NULL,
                details TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                resolved_at TEXT,
                UNIQUE(history_id, migration_version, issue_type)
            );
            CREATE INDEX IF NOT EXISTS idx_migration_issues_unresolved
                ON migration_issues(resolved_at, history_id);",
        )?;
        Ok(())
    }

    fn record_migration_issue(
        conn: &Connection,
        history_id: i64,
        migration_version: i64,
        issue_type: &str,
        details: &str,
        resolved: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let details = details.chars().take(500).collect::<String>();
        conn.execute(
            "INSERT INTO migration_issues
                (history_id, migration_version, issue_type, details, created_at, resolved_at)
             VALUES (?1, ?2, ?3, ?4, datetime('now'),
                     CASE WHEN ?5 THEN datetime('now') ELSE NULL END)
             ON CONFLICT(history_id, migration_version, issue_type) DO UPDATE SET
                details = excluded.details,
                created_at = excluded.created_at,
                resolved_at = excluded.resolved_at",
            params![history_id, migration_version, issue_type, details, resolved],
        )?;
        Ok(())
    }

    fn resolve_migration_issues(
        conn: &Connection,
        history_id: i64,
        migration_version: i64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        conn.execute(
            "UPDATE migration_issues SET resolved_at = datetime('now')
             WHERE history_id = ?1 AND migration_version = ?2 AND resolved_at IS NULL",
            params![history_id, migration_version],
        )?;
        Ok(())
    }

    fn migrate_legacy_file_entry(
        conn: &Connection,
        file_history_dir: &Path,
        id: i64,
        stored: &[u8],
        stored_hash: &str,
        description: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if decode_file_reference(stored).is_some() {
            return Self::resolve_migration_issues(conn, id, 3);
        }
        let data = match crypto::decrypt(stored) {
            Ok(data) => data,
            Err(error) => {
                warn!("Legacy file entry {id} could not be decrypted during migration: {error}");
                return Self::record_migration_issue(
                    conn,
                    id,
                    3,
                    "decrypt_failed",
                    &error.to_string(),
                    false,
                );
            }
        };
        let actual_hash = blake3::hash(&data).to_hex().to_string();
        let reference =
            match persist_history_file_at(file_history_dir, &actual_hash, description, &data) {
                Ok((reference, _)) => reference,
                Err(error) => {
                    warn!(
                        "Legacy file entry {id} could not be persisted during migration: {error}"
                    );
                    return Self::record_migration_issue(
                        conn,
                        id,
                        3,
                        "persist_failed",
                        &error.to_string(),
                        false,
                    );
                }
            };
        if let Err(error) = conn.execute(
            "UPDATE history SET data = ?1, data_hash = ?2, size_bytes = ?3 WHERE id = ?4",
            params![reference, actual_hash, data.len() as i64, id],
        ) {
            warn!("Legacy file entry {id} database update failed: {error}");
            return Self::record_migration_issue(
                conn,
                id,
                3,
                "database_update_failed",
                &error.to_string(),
                false,
            );
        }
        Self::resolve_migration_issues(conn, id, 3)?;
        if !stored_hash.is_empty() && stored_hash != actual_hash {
            Self::record_migration_issue(
                conn,
                id,
                3,
                "hash_mismatch",
                &format!("stored hash {stored_hash}; actual hash {actual_hash}"),
                true,
            )?;
        }
        Ok(())
    }

    fn migrate_legacy_image_entry(
        conn: &Connection,
        image_history_dir: &Path,
        id: i64,
        stored: &[u8],
        stored_hash: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if decode_image_reference(stored).is_some() {
            return Self::resolve_migration_issues(conn, id, 4);
        }
        let data = match crypto::decrypt(stored) {
            Ok(data) => data,
            Err(error) => {
                warn!("Legacy image entry {id} could not be decrypted during migration: {error}");
                return Self::record_migration_issue(
                    conn,
                    id,
                    4,
                    "decrypt_failed",
                    &error.to_string(),
                    false,
                );
            }
        };
        if let Err(error) = crate::protocol::PackedImage::try_from(data.as_slice()) {
            warn!("Legacy image entry {id} is malformed and was not migrated: {error}");
            return Self::record_migration_issue(
                conn,
                id,
                4,
                "invalid_image",
                &error.to_string(),
                false,
            );
        }
        let actual_hash = blake3::hash(&data).to_hex().to_string();
        let reference = match persist_image_at(image_history_dir, &actual_hash, &data) {
            Ok(reference) => reference,
            Err(error) => {
                warn!("Legacy image entry {id} could not be persisted during migration: {error}");
                return Self::record_migration_issue(
                    conn,
                    id,
                    4,
                    "persist_failed",
                    &error.to_string(),
                    false,
                );
            }
        };
        if let Err(error) = conn.execute(
            "UPDATE history SET data = ?1, data_hash = ?2, size_bytes = ?3 WHERE id = ?4",
            params![reference, actual_hash, data.len() as i64, id],
        ) {
            warn!("Legacy image entry {id} database update failed: {error}");
            return Self::record_migration_issue(
                conn,
                id,
                4,
                "database_update_failed",
                &error.to_string(),
                false,
            );
        }
        Self::resolve_migration_issues(conn, id, 4)?;
        if !stored_hash.is_empty() && stored_hash != actual_hash {
            Self::record_migration_issue(
                conn,
                id,
                4,
                "hash_mismatch",
                &format!("stored hash {stored_hash}; actual hash {actual_hash}"),
                true,
            )?;
        }
        Ok(())
    }

    fn retry_unresolved_migration_issues(
        conn: &Connection,
        file_history_dir: &Path,
        image_history_dir: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let rows = conn
            .prepare(
                "SELECT DISTINCT history.id, issue.migration_version, history.data,
                                 history.data_hash, history.description
                 FROM migration_issues AS issue
                 JOIN history ON history.id = issue.history_id
                 WHERE issue.resolved_at IS NULL AND issue.migration_version IN (3, 4)
                 ORDER BY history.id",
            )?
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        for (id, migration_version, stored, stored_hash, description) in rows {
            match migration_version {
                3 => Self::migrate_legacy_file_entry(
                    conn,
                    file_history_dir,
                    id,
                    &stored,
                    &stored_hash,
                    &description,
                )?,
                4 => Self::migrate_legacy_image_entry(
                    conn,
                    image_history_dir,
                    id,
                    &stored,
                    &stored_hash,
                )?,
                _ => {}
            }
        }
        Ok(())
    }

    /// Encrypt a bounded batch of legacy file-history entries using a separate
    /// SQLite connection so application startup and normal history access are
    /// not blocked by large files.
    pub fn migrate_file_history_encryption_batch(
        after_id: i64,
        limit: usize,
    ) -> Result<FileEncryptionMigrationBatch, Box<dyn std::error::Error>> {
        if limit == 0 {
            return Err("file-encryption migration batch size must be positive".into());
        }
        let db_path = get_history_db_path();
        let conn = Connection::open(db_path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
        Self::ensure_migration_issue_schema(&conn)?;
        Self::migrate_file_history_encryption_rows(&conn, &get_file_history_dir(), after_id, limit)
    }

    pub(super) fn migrate_file_history_encryption_rows(
        conn: &Connection,
        file_history_dir: &Path,
        after_id: i64,
        limit: usize,
    ) -> Result<FileEncryptionMigrationBatch, Box<dyn std::error::Error>> {
        let limit = limit.min(64);
        let files = conn
            .prepare(
                "SELECT id, data, data_hash, size_bytes FROM history
                 WHERE type = 'file' AND id > ?1 ORDER BY id LIMIT ?2",
            )?
            .query_map(params![after_id, limit as i64], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let scanned = files.len();
        let last_id = files.last().map(|(id, _, _, _)| *id);
        let mut migrated = 0;
        let mut failed = 0;
        for (id, stored, data_hash, size_bytes) in files {
            let Some(reference) = decode_file_reference(&stored) else {
                continue;
            };
            let result = (|| -> Result<Option<Vec<u8>>, Box<dyn std::error::Error>> {
                let plaintext_size = u64::try_from(size_bytes)
                    .map_err(|_| "file-history size cannot be negative")?;
                validate_history_file_size(plaintext_size)?;
                let path = resolve_file_reference_at(file_history_dir, &reference)?;
                if reference.version == 1 {
                    file_encryption::ensure_file_encrypted(&path, plaintext_size, &data_hash)?;
                    return Ok(Some(encode_file_reference_version(
                        &reference.file_name,
                        2,
                    )?));
                }
                if !file_encryption::encrypted_file_matches(&path, plaintext_size, &data_hash)? {
                    return Err(
                        "encrypted file-history metadata does not match the database".into(),
                    );
                }
                Ok(None)
            })();

            match result {
                Ok(updated_reference) => {
                    if let Some(updated_reference) = updated_reference {
                        if let Err(error) = conn.execute(
                            "UPDATE history SET data = ?1 WHERE id = ?2",
                            params![updated_reference, id],
                        ) {
                            warn!("File-history entry {id} reference update failed: {error}");
                            Self::record_migration_issue(
                                conn,
                                id,
                                7,
                                "database_update_failed",
                                &error.to_string(),
                                false,
                            )?;
                            failed += 1;
                            continue;
                        }
                        migrated += 1;
                    }
                    Self::resolve_migration_issues(conn, id, 7)?;
                }
                Err(error) => {
                    failed += 1;
                    warn!("File-history entry {id} could not be encrypted: {error}");
                    Self::record_migration_issue(
                        conn,
                        id,
                        7,
                        "file_encryption_failed",
                        &error.to_string(),
                        false,
                    )?;
                }
            }
        }
        Ok(FileEncryptionMigrationBatch {
            scanned,
            migrated,
            failed,
            last_id,
            complete: scanned < limit,
        })
    }

    fn add_column_if_missing(
        conn: &Connection,
        column: &str,
        sql: &str,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let columns = conn
            .prepare("PRAGMA table_info(history)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<Result<Vec<_>, _>>()?;
        if !columns.iter().any(|existing| existing == column) {
            conn.execute(sql, [])?;
            return Ok(true);
        }
        Ok(false)
    }
}
