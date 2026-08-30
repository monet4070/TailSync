use super::*;

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
        let duplicate_ids = self.unfavorited_duplicate_ids(&self.entry_ids_by_hash(&data_hash)?)?;
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
        let duplicate_ids = self.unfavorited_duplicate_ids(&self.entry_ids_by_hash(data_hash)?)?;
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
}
