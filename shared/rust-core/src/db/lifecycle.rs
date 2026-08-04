use super::*;

impl HistoryDB {
    /// Delete a history entry and its unreferenced external payload.
    pub fn delete(&mut self, id: i64) -> Result<(), Box<dyn std::error::Error>> {
        self.delete_entries(&[id])
    }

    /// Remove every history entry in one transaction.
    pub fn clear_all(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM history", [])?;
        tx.commit()?;
        if let Err(error) = std::fs::remove_dir_all(&self.file_history_dir) {
            if error.kind() != std::io::ErrorKind::NotFound {
                warn!("Could not remove file history folder: {error}");
            }
        }
        std::fs::create_dir_all(&self.file_history_dir)?;
        if let Err(error) = std::fs::remove_dir_all(&self.image_history_dir) {
            if error.kind() != std::io::ErrorKind::NotFound {
                warn!("Could not remove image history folder: {error}");
            }
        }
        std::fs::create_dir_all(&self.image_history_dir)?;
        // Reclaim pages after an explicit user-initiated clear operation.
        let _ = self
            .conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;");
        Ok(())
    }

    /// Trim entries of a given type beyond the configured count and byte limits.
    pub(super) fn trim(&mut self, entry_type: &str) -> Result<(), Box<dyn std::error::Error>> {
        let max = match entry_type {
            "text" => self.max_history,
            "image" => (self.max_history / 10).max(10),
            "file" => (self.max_history / 10).max(crate::sync::MAX_FILE_BATCH_COUNT as i64),
            _ => 100,
        };

        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM history WHERE type = ?1",
            params![entry_type],
            |row| row.get(0),
        )?;

        if count > max {
            let excess = count - max;
            let ids = self.expand_batch_groups(self.oldest_entry_ids(Some(entry_type), excess)?)?;
            self.delete_entries(&ids)?;
            info!("Trimmed {} {} entries", excess, entry_type);
        }

        let total: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM history", [], |row| row.get(0))?;
        if total > self.max_history {
            let excess = total - self.max_history;
            let ids = self.expand_batch_groups(self.oldest_entry_ids(None, excess)?)?;
            self.delete_entries(&ids)?;
            info!("Trimmed {} entries (total cap)", excess);
        }

        if entry_type == "file" {
            let quota = i64::try_from(self.storage_quota_bytes).unwrap_or(i64::MAX);
            let ids = self.expand_batch_groups(self.file_ids_over_byte_limit(quota)?)?;
            if !ids.is_empty() {
                let count = ids.len();
                self.delete_entries(&ids)?;
                info!("Trimmed {count} unpinned file entries (storage quota)");
            }
        }

        Ok(())
    }

    pub(super) fn file_ids_over_byte_limit(
        &self,
        limit: i64,
    ) -> Result<Vec<i64>, Box<dyn std::error::Error>> {
        let mut total = 0_i64;
        let mut statement = self.conn.prepare(
            "SELECT id, MAX(size_bytes, 0), pinned FROM history
             WHERE type IN ('file', 'image') ORDER BY timestamp DESC, id DESC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)? != 0,
            ))
        })?;
        let mut remove = Vec::new();
        for row in rows {
            let (id, size, pinned) = row?;
            total = total.saturating_add(size);
            if total > limit && !pinned {
                remove.push(id);
            }
        }
        Ok(remove)
    }

    fn oldest_entry_ids(
        &self,
        entry_type: Option<&str>,
        limit: i64,
    ) -> Result<Vec<i64>, Box<dyn std::error::Error>> {
        let ids = if let Some(entry_type) = entry_type {
            self.conn
                .prepare("SELECT id FROM history WHERE type = ?1 AND pinned = 0 ORDER BY timestamp ASC LIMIT ?2")?
                .query_map(params![entry_type, limit], |row| row.get(0))?
                .collect::<Result<Vec<_>, _>>()?
        } else {
            self.conn
                .prepare("SELECT id FROM history WHERE pinned = 0 ORDER BY timestamp ASC LIMIT ?1")?
                .query_map(params![limit], |row| row.get(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        Ok(ids)
    }

    pub(super) fn expand_batch_groups(
        &self,
        mut ids: Vec<i64>,
    ) -> Result<Vec<i64>, Box<dyn std::error::Error>> {
        if ids.is_empty() {
            return Ok(ids);
        }
        let mut batch_ids = Vec::new();
        for id in &ids {
            if let Some(batch_id) = self.conn.query_row(
                "SELECT batch_id FROM history WHERE id = ?1",
                params![id],
                |row| row.get::<_, Option<String>>(0),
            )? {
                batch_ids.push(batch_id);
            }
        }
        for batch_id in batch_ids {
            let group_ids = self
                .conn
                .prepare("SELECT id FROM history WHERE batch_id = ?1 AND pinned = 0")?
                .query_map(params![batch_id], |row| row.get::<_, i64>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            ids.extend(group_ids);
        }
        ids.sort_unstable();
        ids.dedup();
        Ok(ids)
    }

    pub(super) fn entry_ids_by_hash(
        &self,
        data_hash: &str,
    ) -> Result<Vec<i64>, Box<dyn std::error::Error>> {
        let ids = self
            .conn
            .prepare("SELECT id FROM history WHERE data_hash = ?1")?
            .query_map(params![data_hash], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ids)
    }

    pub(super) fn delete_entries(&mut self, ids: &[i64]) -> Result<(), Box<dyn std::error::Error>> {
        self.delete_entries_except(ids, None)
    }

    pub(super) fn delete_entries_except(
        &mut self,
        ids: &[i64],
        preserve_path: Option<&Path>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if ids.is_empty() {
            return Ok(());
        }
        let mut references = Vec::new();
        let mut affected_batches = Vec::new();
        for id in ids {
            let stored = self.conn.query_row(
                "SELECT type, data, batch_id FROM history WHERE id = ?1",
                params![id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Vec<u8>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                },
            );
            if let Ok((entry_type, stored, batch_id)) = stored {
                if let Some(batch_id) = batch_id {
                    affected_batches.push(batch_id);
                }
                let decoded = match entry_type.as_str() {
                    "file" => decode_file_reference(&stored)
                        .map(|reference| (self.file_history_dir.clone(), reference)),
                    "image" | "text" => decode_image_reference(&stored)
                        .map(|reference| (self.image_history_dir.clone(), reference)),
                    _ => None,
                };
                if let Some((directory, reference)) = decoded {
                    references.push((stored, directory, reference));
                }
            }
        }

        let tx = self.conn.transaction()?;
        for id in ids {
            tx.execute("DELETE FROM history WHERE id = ?1", params![id])?;
        }
        affected_batches.sort();
        affected_batches.dedup();
        for batch_id in affected_batches {
            tx.execute(
                "UPDATE history SET batch_status = 'incomplete'
                 WHERE batch_id = ?1
                   AND (SELECT COUNT(*) FROM history WHERE batch_id = ?1) < batch_total",
                params![batch_id],
            )?;
        }
        tx.commit()?;

        for (stored, directory, reference) in references {
            let remaining: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM history WHERE data = ?1",
                params![stored],
                |row| row.get(0),
            )?;
            if remaining == 0 {
                let path = resolve_file_reference_at(&directory, &reference)?;
                if preserve_path == Some(path.as_path()) {
                    continue;
                }
                if let Err(error) = std::fs::remove_file(&path) {
                    if error.kind() != std::io::ErrorKind::NotFound {
                        warn!("Could not remove history file {}: {error}", path.display());
                    }
                }
            }
        }
        Ok(())
    }
}
