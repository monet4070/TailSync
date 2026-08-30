use super::*;

struct FileBatchCompleteness {
    row_count: i64,
    min_total: Option<i64>,
    max_total: Option<i64>,
    min_index: Option<i64>,
    max_index: Option<i64>,
    distinct_indexes: i64,
    complete_count: i64,
}

impl HistoryDB {
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
                "SELECT id, description, batch_index, batch_total, batch_status
                 FROM history WHERE batch_id = ?1 AND type = 'file'
                 ORDER BY batch_index ASC, id ASC",
            )?
            .query_map(params![batch_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let expected = rows.first().map(|row| row.3).unwrap_or(0);
        if expected <= 0
            || rows.len() != expected as usize
            || rows
                .iter()
                .enumerate()
                .any(|(index, row)| row.2 != index as i64 || row.4 != "complete")
        {
            return Err("Only a complete file batch can be copied as a group".into());
        }
        rows.into_iter()
            .map(|(id, name, _, _, _)| {
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
        let duplicate_ids = self.unfavorited_duplicate_ids(&self.entry_ids_by_hash(&data_hash)?)?;
        let old_payloads = self.external_payloads_for_ids(&duplicate_ids)?;
        let timestamp = chrono::Utc::now().to_rfc3339();
        let tx = self.conn.transaction()?;
        Self::delete_rows_in_transaction(&tx, &duplicate_ids)?;
        let write_result = tx.execute(
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
        );
        if let Err(error) = write_result {
            drop(tx);
            let mut payloads = old_payloads;
            payloads.push(super::entries::ExternalHistoryPayload {
                stored: reference,
                path: file_path.clone(),
            });
            self.cleanup_external_payloads(&payloads, None);
            return Err(error.into());
        }
        if let Err(error) = tx.commit() {
            let mut payloads = old_payloads;
            payloads.push(super::entries::ExternalHistoryPayload {
                stored: reference,
                path: file_path.clone(),
            });
            self.cleanup_external_payloads(&payloads, None);
            return Err(error.into());
        }
        self.cleanup_external_payloads(&old_payloads, Some(&file_path));
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
            false,
        )?;
        let duplicate_ids = self.unfavorited_duplicate_ids(&self.entry_ids_by_hash(data_hash)?)?;
        let old_payloads = self.external_payloads_for_ids(&duplicate_ids)?;
        let timestamp = chrono::Utc::now().to_rfc3339();
        let tx = self.conn.transaction()?;
        Self::delete_rows_in_transaction(&tx, &duplicate_ids)?;
        let write_result = tx.execute(
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
        );
        if let Err(error) = write_result {
            drop(tx);
            let mut payloads = old_payloads;
            payloads.push(super::entries::ExternalHistoryPayload {
                stored: reference,
                path: file_path.clone(),
            });
            self.cleanup_external_payloads(&payloads, None);
            return Err(error.into());
        }
        if let Err(error) = tx.commit() {
            let mut payloads = old_payloads;
            payloads.push(super::entries::ExternalHistoryPayload {
                stored: reference,
                path: file_path.clone(),
            });
            self.cleanup_external_payloads(&payloads, None);
            return Err(error.into());
        }
        self.cleanup_external_payloads(&old_payloads, Some(&file_path));
        if move_source && source != file_path {
            if let Err(error) = std::fs::remove_file(source) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    warn!(
                        "Could not remove adopted file source {}: {error}",
                        source.display()
                    );
                }
            }
        }
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

    /// Check whether a batch has already been committed as a complete history
    /// group. Outbound recovery can crash after this commit and before its
    /// private journal is marked, so callers must be able to retry idempotently.
    pub fn has_complete_file_batch(
        &self,
        batch_id: &str,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let stats: FileBatchCompleteness = self.conn.query_row(
            "SELECT COUNT(*),
                    MIN(batch_total), MAX(batch_total),
                    MIN(batch_index), MAX(batch_index),
                    COUNT(DISTINCT batch_index),
                    COALESCE(SUM(CASE WHEN batch_status = 'complete' THEN 1 ELSE 0 END), 0)
             FROM history
             WHERE batch_id = ?1 AND type = 'file'",
            params![batch_id],
            |row| {
                Ok(FileBatchCompleteness {
                    row_count: row.get(0)?,
                    min_total: row.get(1)?,
                    max_total: row.get(2)?,
                    min_index: row.get(3)?,
                    max_index: row.get(4)?,
                    distinct_indexes: row.get(5)?,
                    complete_count: row.get(6)?,
                })
            },
        )?;
        Ok(stats.row_count > 0
            && stats.min_total == Some(stats.row_count)
            && stats.max_total == Some(stats.row_count)
            && stats.min_index == Some(0)
            && stats.max_index == Some(stats.row_count - 1)
            && stats.distinct_indexes == stats.row_count
            && stats.complete_count == stats.row_count)
    }

    /// Check a durable completion receipt for one authenticated sender and
    /// exact manifest. A bare batch ID is intentionally insufficient because
    /// it is generated by the sender and is not the device identity.
    pub fn has_received_file_batch(
        &self,
        source_device_id: &str,
        batch_id: &str,
        manifest_hash: &str,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        Ok(self
            .received_file_batch_receipt(source_device_id, batch_id)?
            .is_some_and(|(stored_hash, status)| {
                stored_hash == manifest_hash && status == "complete"
            }))
    }

    /// Return the durable receipt for a sender and batch, including partial
    /// receipts. Callers must reject a different manifest before accepting any
    /// file bytes; a partial receipt with the same hash remains resumable.
    pub fn received_file_batch_receipt(
        &self,
        source_device_id: &str,
        batch_id: &str,
    ) -> Result<Option<(String, String)>, Box<dyn std::error::Error>> {
        self.conn
            .query_row(
                "SELECT manifest_hash, status
                 FROM received_file_batches
                 WHERE source_device_id = ?1 AND batch_id = ?2",
                params![source_device_id, batch_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(Into::into)
    }

    /// Reuse a previously persisted batch when a sender retries after losing
    /// the final acknowledgement. Matching the complete manifest is important:
    /// a batch ID is an idempotency key, not permission to append arbitrary rows
    /// to an existing history group.
    fn reuse_existing_file_batch(
        &mut self,
        batch_id: &str,
        files: &[HistoryFileInput],
        expected_total: usize,
        source_peer: &str,
        match_source_peer: bool,
        complete: bool,
    ) -> Result<Option<Vec<PathBuf>>, Box<dyn std::error::Error>> {
        let rows = self
            .conn
            .prepare(
                "SELECT type, description, data, size_bytes, source_peer, data_hash,
                        batch_index, batch_total, batch_status
                 FROM history WHERE batch_id = ?1 AND type = 'file'
                 ORDER BY batch_index ASC, id ASC",
            )?
            .query_map(params![batch_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                    row.get::<_, String>(8)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        if rows.is_empty() {
            return Ok(None);
        }

        let total = i64::try_from(expected_total).map_err(|_| "File batch is too large")?;
        let manifest_matches = rows.len() == files.len()
            && rows.iter().enumerate().all(|(index, row)| {
                let file = &files[index];
                row.0 == "file"
                    && row.1 == file.name
                    && row.3 == i64::try_from(file.size).unwrap_or(i64::MAX)
                    && (!match_source_peer || row.4 == source_peer)
                    && row.5 == file.data_hash
                    && row.6 == Some(index as i64)
                    && row.7 == Some(total)
            });
        if !manifest_matches {
            return Err("File batch already exists with a different or incomplete manifest".into());
        }
        if complete && (expected_total != files.len() || rows.len() != expected_total) {
            return Err("A complete file batch must contain every manifest file".into());
        }

        let paths = rows
            .iter()
            .map(|row| {
                let reference = decode_file_reference(&row.2)
                    .ok_or("Existing file batch has an invalid history reference")?;
                let path = resolve_file_reference_at(&self.file_history_dir, &reference)?;
                let metadata = std::fs::symlink_metadata(&path)?;
                if !metadata.is_file() || metadata.file_type().is_symlink() {
                    return Err(format!("Existing file batch is missing {}", path.display()).into());
                }
                Ok(path)
            })
            .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;

        if complete && rows.iter().any(|row| row.8 != "complete") {
            self.conn.execute(
                "UPDATE history SET batch_status = 'complete'
                 WHERE batch_id = ?1 AND type = 'file'",
                params![batch_id],
            )?;
        }
        if !match_source_peer {
            self.conn.execute(
                "UPDATE history SET source_peer = ?1
                 WHERE batch_id = ?2 AND type = 'file'",
                params![source_peer, batch_id],
            )?;
        }
        Ok(Some(paths))
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
        self.add_file_batch_with_status_and_receipt(
            batch_id,
            files,
            FileBatchWriteOptions {
                expected_total,
                source_peer,
                move_sources,
                complete,
                source_device_id: None,
                manifest_hash: None,
            },
        )
    }

    /// Persist a received file batch together with its durable completion
    /// receipt in the same SQLite transaction. The receipt identity is the
    /// authenticated public-key fingerprint, while `source_peer` remains the
    /// mutable display hostname shown in history.
    pub fn add_file_batch_with_receipt(
        &mut self,
        batch_id: &str,
        files: &[HistoryFileInput],
        options: FileBatchWriteOptions<'_>,
    ) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
        if options.source_device_id.is_none_or(str::is_empty)
            || options.manifest_hash.is_none_or(str::is_empty)
        {
            return Err("Received file batch receipt identity is missing".into());
        }
        self.add_file_batch_with_status_and_receipt(batch_id, files, options)
    }

    fn add_file_batch_with_status_and_receipt(
        &mut self,
        batch_id: &str,
        files: &[HistoryFileInput],
        options: FileBatchWriteOptions<'_>,
    ) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
        let FileBatchWriteOptions {
            expected_total,
            source_peer,
            move_sources,
            complete,
            source_device_id,
            manifest_hash,
        } = options;
        let receipt = source_device_id.zip(manifest_hash);
        if files.is_empty() {
            return Err("A file batch cannot be empty".into());
        }
        if expected_total < files.len() {
            return Err("File batch total cannot be smaller than the completed file count".into());
        }
        if complete && expected_total != files.len() {
            return Err("A complete file batch must contain every manifest file".into());
        }
        let existing_receipt = if let Some((source_device_id, manifest_hash)) = receipt {
            let existing_receipt: Option<String> = self
                .conn
                .query_row(
                    "SELECT manifest_hash FROM received_file_batches
                     WHERE source_device_id = ?1 AND batch_id = ?2",
                    params![source_device_id, batch_id],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(ref existing_hash) = existing_receipt {
                if existing_hash != manifest_hash {
                    return Err("File batch receipt exists with a different manifest".into());
                }
            }
            existing_receipt
        } else {
            None
        };
        if let Some(paths) = self.reuse_existing_file_batch(
            batch_id,
            files,
            expected_total,
            source_peer,
            existing_receipt.is_none(),
            complete,
        )? {
            if let Some((source_device_id, manifest_hash)) = receipt {
                self.record_received_file_batch_receipt(
                    source_device_id,
                    batch_id,
                    manifest_hash,
                    complete,
                )?;
            }
            return Ok(paths);
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
            if let Some((source_device_id, manifest_hash)) = receipt {
                let existing_receipt: Option<String> = tx
                    .query_row(
                        "SELECT manifest_hash FROM received_file_batches
                         WHERE source_device_id = ?1 AND batch_id = ?2",
                        params![source_device_id, batch_id],
                        |row| row.get(0),
                    )
                    .optional()?;
                if let Some(existing_hash) = existing_receipt {
                    if existing_hash != manifest_hash {
                        return Err("File batch receipt exists with a different manifest".into());
                    }
                    tx.execute(
                        "UPDATE received_file_batches
                         SET status = CASE WHEN ?3 THEN 'complete' ELSE status END,
                             completed_at = CASE WHEN ?3 THEN datetime('now') ELSE completed_at END
                         WHERE source_device_id = ?1 AND batch_id = ?2",
                        params![source_device_id, batch_id, complete],
                    )?;
                } else {
                    tx.execute(
                        "INSERT INTO received_file_batches
                            (source_device_id, batch_id, manifest_hash, status, completed_at)
                         VALUES (?1, ?2, ?3,
                                 CASE WHEN ?4 THEN 'complete' ELSE 'partial' END,
                                 CASE WHEN ?4 THEN datetime('now') ELSE NULL END)",
                        params![source_device_id, batch_id, manifest_hash, complete],
                    )?;
                }
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

    fn record_received_file_batch_receipt(
        &mut self,
        source_device_id: &str,
        batch_id: &str,
        manifest_hash: &str,
        complete: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let tx = self.conn.transaction()?;
        let existing_receipt: Option<String> = tx
            .query_row(
                "SELECT manifest_hash FROM received_file_batches
                 WHERE source_device_id = ?1 AND batch_id = ?2",
                params![source_device_id, batch_id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(existing_hash) = existing_receipt {
            if existing_hash != manifest_hash {
                return Err("File batch receipt exists with a different manifest".into());
            }
            if complete {
                tx.execute(
                    "UPDATE received_file_batches
                     SET status = 'complete', completed_at = datetime('now')
                     WHERE source_device_id = ?1 AND batch_id = ?2",
                    params![source_device_id, batch_id],
                )?;
            }
        } else {
            tx.execute(
                "INSERT INTO received_file_batches
                    (source_device_id, batch_id, manifest_hash, status, completed_at)
                 VALUES (?1, ?2, ?3,
                         CASE WHEN ?4 THEN 'complete' ELSE 'partial' END,
                         CASE WHEN ?4 THEN datetime('now') ELSE NULL END)",
                params![source_device_id, batch_id, manifest_hash, complete],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}
