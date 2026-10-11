use super::*;

#[cfg(test)]
fn remove_history_entry(path: &Path) -> std::io::Result<()> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let result = if metadata.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
    match result {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

#[cfg(test)]
fn clear_history_directory_with<F>(directory: &Path, remove_entry: &mut F) -> std::io::Result<()>
where
    F: FnMut(&Path) -> std::io::Result<()>,
{
    crate::private_fs::create_private_dir_all(directory)?;
    let mut first_error = None;
    for entry in std::fs::read_dir(directory)? {
        match entry {
            Ok(entry) => {
                if let Err(error) = remove_entry(&entry.path()) {
                    first_error.get_or_insert(error);
                }
            }
            Err(error) => {
                first_error.get_or_insert(error);
            }
        }
    }
    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

impl HistoryDB {
    /// Delete a history entry and its unreferenced external payload.
    pub fn delete(&mut self, id: i64) -> Result<(), Box<dyn std::error::Error>> {
        self.ensure_logical_item_unfavorited(id)?;
        let _freed = self.delete_entries_with_batch_policy(&[id], None, true)?;
        Ok(())
    }

    /// Remove every unfavorited history entry while preserving favorites.
    pub fn clear_all(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let ids = self.unfavorited_ids()?;
        self.delete_entries(&ids)?;
        Ok(())
    }

    fn unfavorited_ids(&self) -> Result<Vec<i64>, Box<dyn std::error::Error>> {
        self.conn
            .prepare(
                "SELECT id FROM history AS candidate
                 WHERE candidate.pinned = 0
                   AND (
                       candidate.batch_id IS NULL
                       OR NOT EXISTS (
                           SELECT 1 FROM history AS batch_entry
                           WHERE batch_entry.batch_id = candidate.batch_id
                             AND batch_entry.pinned <> 0
                       )
                   )",
            )?
            .query_map([], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    #[cfg(test)]
    fn clear_all_with<F>(&mut self, mut remove_entry: F) -> Result<(), Box<dyn std::error::Error>>
    where
        F: FnMut(&Path) -> std::io::Result<()>,
    {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM history", [])?;
        tx.commit()?;

        let mut first_error: Option<Box<dyn std::error::Error>> = None;
        for (label, directory) in [
            ("file", self.file_history_dir.clone()),
            ("image", self.image_history_dir.clone()),
        ] {
            if let Err(error) = clear_history_directory_with(&directory, &mut remove_entry) {
                warn!("Could not clear {label} history folder: {error}");
                if first_error.is_none() {
                    first_error = Some(Box::new(std::io::Error::new(
                        error.kind(),
                        format!(
                            "Could not clear {label} history folder {}: {error}",
                            directory.display()
                        ),
                    )));
                }
            }
        }

        // Reclaim pages after an explicit user-initiated clear operation.
        // Busy semantics: continue. A blocked TRUNCATE returns busy=1 (not an
        // error), so it only skips space reclamation; the logical clear has
        // already been committed. See docs/remediation/wal-checkpoint-semantics.md.
        if let Err(error) = self
            .conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;")
        {
            if first_error.is_none() {
                first_error = Some(Box::new(error));
            }
        }

        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// Trim entries of a given type beyond the configured count and byte limits.
    pub(super) fn trim(&mut self, entry_type: &str) -> Result<(), Box<dyn std::error::Error>> {
        if entry_type == "composite" {
            self.trim("text")?;
            return self.trim("image");
        }
        let max = match entry_type {
            "text" => self.max_history,
            "image" => (self.max_history / 10).max(10),
            "file" => (self.max_history / 10).max(crate::sync::MAX_FILE_BATCH_COUNT as i64),
            _ => 100,
        };

        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM history WHERE type = ?1 OR (type = 'composite' AND category = ?1)",
            params![entry_type],
            |row| row.get(0),
        )?;

        let count = count.saturating_sub(self.protected_count(Some(entry_type))?);
        if count > max {
            let excess = count - max;
            let ids = self.expand_batch_groups(self.oldest_entry_ids(Some(entry_type), excess)?)?;
            let _freed = self.delete_entries(&ids)?;
            info!("Trimmed {} {} entries", excess, entry_type);
        }

        let total: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM history", [], |row| row.get(0))?;
        let total = total.saturating_sub(self.protected_count(None)?);
        if total > self.max_history {
            let excess = total - self.max_history;
            let ids = self.expand_batch_groups(self.oldest_entry_ids(None, excess)?)?;
            let _freed = self.delete_entries(&ids)?;
            info!("Trimmed {} entries (total cap)", excess);
        }

        if matches!(entry_type, "file" | "image" | "composite") {
            let quota = i64::try_from(self.storage_quota_bytes).unwrap_or(i64::MAX);
            let ids = self.expand_batch_groups(self.file_ids_over_byte_limit(quota)?)?;
            if !ids.is_empty() {
                let count = ids.len();
                let _freed = self.delete_entries(&ids)?;
                info!("Trimmed {count} unpinned file entries (storage quota)");
            }
        }

        Ok(())
    }

    /// Apply current count and byte limits immediately after settings change.
    pub fn enforce_limits(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        for entry_type in ["text", "image", "file"] {
            self.trim(entry_type)?;
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
            if total > limit && !pinned && !self.protected_history.contains(&id) {
                remove.push(id);
            }
        }
        Ok(remove)
    }

    fn protected_count(&self, entry_type: Option<&str>) -> Result<i64, Box<dyn std::error::Error>> {
        let mut count = 0;
        let mut statement = self
            .conn
            .prepare("SELECT type, category FROM history WHERE id = ?1")?;
        for id in &self.protected_history {
            if let Some((kind, category)) = statement
                .query_row([id], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })
                .optional()?
            {
                if entry_type.is_none_or(|wanted| {
                    kind == wanted || (kind == "composite" && category == wanted)
                }) {
                    count += 1;
                }
            }
        }
        Ok(count)
    }

    fn oldest_entry_ids(
        &self,
        entry_type: Option<&str>,
        limit: i64,
    ) -> Result<Vec<i64>, Box<dyn std::error::Error>> {
        let ids = if let Some(entry_type) = entry_type {
            self.conn
                .prepare("SELECT id FROM history WHERE (type = ?1 OR (type = 'composite' AND category = ?1)) AND pinned = 0 ORDER BY timestamp ASC, id ASC")?
                .query_map(params![entry_type], |row| row.get(0))?
                .collect::<Result<Vec<_>, _>>()?
        } else {
            self.conn
                .prepare("SELECT id FROM history WHERE pinned = 0 ORDER BY timestamp ASC")?
                .query_map([], |row| row.get(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        Ok(ids
            .into_iter()
            .filter(|id| !self.protected_history.contains(id))
            .take(limit.max(0) as usize)
            .collect())
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
            let has_favorite: bool = self.conn.query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM history WHERE batch_id = ?1 AND pinned <> 0
                )",
                params![batch_id],
                |row| row.get::<_, i64>(0).map(|value| value != 0),
            )?;
            if has_favorite {
                ids.retain(|candidate| {
                    self.conn
                        .query_row(
                            "SELECT batch_id FROM history WHERE id = ?1",
                            params![candidate],
                            |row| row.get::<_, Option<String>>(0),
                        )
                        .ok()
                        .flatten()
                        .as_deref()
                        != Some(batch_id.as_str())
                });
            } else {
                let group_ids = self
                    .conn
                    .prepare("SELECT id FROM history WHERE batch_id = ?1")?
                    .query_map(params![batch_id], |row| row.get::<_, i64>(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                ids.extend(group_ids);
            }
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

    /// Returns the bytes of external payloads this deletion provably removed.
    pub(super) fn delete_entries(
        &mut self,
        ids: &[i64],
    ) -> Result<u64, Box<dyn std::error::Error>> {
        self.delete_entries_except(ids, None)
    }

    pub(super) fn delete_entries_except(
        &mut self,
        ids: &[i64],
        preserve_path: Option<&Path>,
    ) -> Result<u64, Box<dyn std::error::Error>> {
        self.delete_entries_with_batch_policy(ids, preserve_path, false)
    }

    pub(super) fn delete_entries_with_batch_policy(
        &mut self,
        ids: &[i64],
        preserve_path: Option<&Path>,
        rebase_complete_batches: bool,
    ) -> Result<u64, Box<dyn std::error::Error>> {
        if ids.is_empty() {
            return Ok(0);
        }
        let composite_payloads = self.composite_part_payloads(ids)?;
        let mut references = Vec::new();
        let mut affected_batches = std::collections::BTreeMap::<String, bool>::new();
        for id in ids {
            let stored = self.conn.query_row(
                "SELECT type, data, batch_id, batch_status FROM history WHERE id = ?1",
                params![id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Vec<u8>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            );
            if let Ok((entry_type, stored, batch_id, batch_status)) = stored {
                if let Some(batch_id) = batch_id {
                    let was_complete = batch_status == "complete";
                    affected_batches
                        .entry(batch_id)
                        .and_modify(|complete| *complete &= was_complete)
                        .or_insert(was_complete);
                }
                let decoded = match entry_type.as_str() {
                    "file" => decode_file_reference(&stored)
                        .map(|reference| (self.file_history_dir.clone(), reference)),
                    "image" | "text" => decode_image_reference(&stored)
                        .map(|reference| (self.image_history_dir.clone(), reference)),
                    _ => None,
                };
                if let Some((directory, reference)) = decoded {
                    references.push((directory, reference));
                }
            }
        }

        let has_composite_schema = self.has_composite_schema();
        let tx = self.conn.transaction()?;
        for id in ids {
            if has_composite_schema {
                tx.execute(
                    "UPDATE history_composite_events SET history_id=NULL WHERE history_id=?1",
                    params![id],
                )?;
                tx.execute("DELETE FROM history_parts WHERE history_id=?1", params![id])?;
            }
            tx.execute("DELETE FROM history WHERE id = ?1", params![id])?;
        }
        for (batch_id, was_complete) in affected_batches {
            if rebase_complete_batches && was_complete {
                let remaining_ids = {
                    let mut statement = tx.prepare(
                        "SELECT id FROM history WHERE batch_id = ?1
                         ORDER BY batch_index ASC, id ASC",
                    )?;
                    let ids = statement
                        .query_map(params![batch_id], |row| row.get::<_, i64>(0))?
                        .collect::<Result<Vec<_>, _>>()?;
                    ids
                };
                let remaining_total = i64::try_from(remaining_ids.len())?;
                for (index, remaining_id) in remaining_ids.into_iter().enumerate() {
                    tx.execute(
                        "UPDATE history
                         SET batch_index = ?1, batch_total = ?2, batch_status = 'complete'
                         WHERE id = ?3",
                        params![i64::try_from(index)?, remaining_total, remaining_id],
                    )?;
                }
            } else {
                tx.execute(
                    "UPDATE history SET batch_status = 'incomplete'
                     WHERE batch_id = ?1
                       AND (SELECT COUNT(*) FROM history WHERE batch_id = ?1) < batch_total",
                    params![batch_id],
                )?;
            }
        }
        tx.commit()?;

        let mut paths: Vec<_> = references
            .into_iter()
            .filter_map(|(directory, reference)| {
                match resolve_file_reference_at(&directory, &reference) {
                    Ok(path) => Some(path),
                    Err(error) => {
                        warn!("Could not resolve the path of a deleted payload: {error}");
                        None
                    }
                }
            })
            .collect();
        paths.extend(composite_payloads.into_iter().map(|p| p.path));
        let freed_bytes = self.remove_unreferenced_payload_paths(&paths, preserve_path);
        // Every deletion path, including quota trimming and duplicate
        // replacement, must remove deleted rows from the WAL as well. Perform
        // this after the external encrypted payloads have been handled so a
        // transient checkpoint failure cannot skip their deletion. A busy
        // checkpoint after a committed delete is not a delete failure.
        self.deferred_checkpoint = !self.try_checkpoint();
        Ok(freed_bytes)
    }

    /// Retry post-delete or pre-restart WAL cleanup without waiting for readers.
    /// Maintenance owns the retry; a successful delete stays successful.
    pub fn retry_deferred_checkpoint(&mut self) {
        if self.deferred_checkpoint {
            self.deferred_checkpoint = !self.try_checkpoint();
        }
    }

    fn try_checkpoint(&self) -> bool {
        match checkpoint_history_wal(&self.conn) {
            WalCheckpointOutcome::Completed => return true,
            WalCheckpointOutcome::Blocked {
                busy,
                log,
                checkpointed,
            } => {
                warn!(
                    "History WAL cleanup was blocked by a reader (busy={busy}, log={log}, checkpointed={checkpointed}); maintenance will retry"
                );
            }
            WalCheckpointOutcome::Failed(error) => {
                warn!("Could not checkpoint the history WAL: {error}");
            }
        }
        false
    }
}

/// Outcome of the post-delete WAL checkpoint.
///
/// `PRAGMA wal_checkpoint(TRUNCATE)` reports a reader-blocked attempt by
/// RETURNING a row with `busy = 1`, not by raising an error, so the result row
/// must be read: `execute_batch` discards it and leaves a blocked checkpoint
/// completely silent.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum WalCheckpointOutcome {
    Completed,
    Blocked {
        busy: i64,
        log: i64,
        checkpointed: i64,
    },
    Failed(String),
}

pub(crate) fn checkpoint_history_wal(conn: &rusqlite::Connection) -> WalCheckpointOutcome {
    let previous: i64 = match conn.pragma_query_value(None, "busy_timeout", |row| row.get(0)) {
        Ok(previous) => previous,
        Err(error) => return WalCheckpointOutcome::Failed(error.to_string()),
    };
    if let Err(error) = conn.busy_timeout(std::time::Duration::ZERO) {
        return WalCheckpointOutcome::Failed(error.to_string());
    }
    let outcome = match conn.query_row("PRAGMA wal_checkpoint(TRUNCATE);", [], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
        ))
    }) {
        Ok((0, _, _)) => WalCheckpointOutcome::Completed,
        Ok((busy, log, checkpointed)) => WalCheckpointOutcome::Blocked {
            busy,
            log,
            checkpointed,
        },
        Err(error) => WalCheckpointOutcome::Failed(error.to_string()),
    };
    // Restore the connection policy even when checkpointing failed.
    if let Err(error) = conn.busy_timeout(std::time::Duration::from_millis(previous.max(0) as u64))
    {
        return WalCheckpointOutcome::Failed(format!("Could not restore busy timeout: {error}"));
    }
    outcome
}

impl HistoryDB {
    /// The common unlink policy for deletion, eviction and duplicate replacement.
    /// Resolve the live set once, after the row transaction committed. Path or
    /// serialized-reference equality cannot establish file identity on APFS/NTFS.
    /// Arm a guard that owns the payload files written for one history entry until
    /// its transaction commits (repair plan §4 R5-02).
    ///
    /// Cleanup used to depend on an explicit call at each exit, so any `?` between
    /// writing a payload and committing the transaction leaked the file. The manifest
    /// encryption in `add_composite_event` did exactly that.
    ///
    /// The guard removes only payloads that no live row referenced **before** the
    /// transaction started. The transaction either commits (disarming the guard) or
    /// rolls back (leaving the live set unchanged), so the snapshot is exact — and a
    /// payload shared with another entry is never deleted. If the live set cannot be
    /// read, the guard removes nothing rather than guessing.
    pub(super) fn arm_pending_payloads(
        &self,
        written: &[super::entries::ExternalHistoryPayload],
    ) -> PendingPayloads {
        PendingPayloads::arm(self, written)
    }

    pub(super) fn remove_unreferenced_payload_paths(
        &self,
        paths: &[PathBuf],
        preserve_path: Option<&Path>,
    ) -> u64 {
        if paths.is_empty() {
            return 0;
        }
        let mut live = match self.referenced_payload_identities() {
            Ok(live) => live,
            Err(error) => {
                warn!("Cannot safely remove history payloads: {error}");
                return 0;
            }
        };
        if let Some(path) = preserve_path {
            match payload_file_identity(path) {
                Ok(identity) => {
                    live.insert(identity);
                }
                Err(error) => {
                    warn!("Cannot identify the preserved history payload: {error}");
                    return 0;
                }
            }
        }
        let mut freed = 0_u64;
        for path in paths {
            match payload_file_identity(path) {
                Ok(identity) if live.contains(&identity) => continue,
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    warn!(
                        "Cannot identify history payload {}: {error}",
                        path.display()
                    );
                    continue;
                }
            }
            let size = match std::fs::metadata(path) {
                Ok(metadata) => metadata.len(),
                Err(_) => continue,
            };
            match std::fs::remove_file(path) {
                Ok(()) => freed = freed.saturating_add(size),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => warn!(
                    "Could not remove history payload {}: {error}",
                    path.display()
                ),
            }
        }
        freed
    }

    /// Remove external payload files that no history row references and that are
    /// older than `grace`. Returns how many files were removed.
    ///
    /// Deleting a database row is not evidence that its payload left the disk:
    /// the post-commit `remove_file` can fail on a busy or read-only file, and the
    /// row is gone by then. This reconciles the payload directories against the
    /// rows that still exist instead of trusting the delete path, and the grace
    /// period keeps it from racing a transfer that is still being written.
    pub fn sweep_orphan_payloads(
        &mut self,
        grace: std::time::Duration,
    ) -> Result<u64, Box<dyn std::error::Error>> {
        #[cfg(any(test, feature = "test-support"))]
        if let Some(observer) = &self.storage_work_observer {
            observer("gc");
        }
        if grace.is_zero() {
            // Atomic writers create their temp files inside these directories
            // (`.{name}.{pid}-{rand}.tmp`, `{name}.tmp.{rand}`); a zero grace would
            // delete a transfer that is still in flight.
            return Err("orphan sweep requires a non-zero grace period".into());
        }
        // Path spelling is not file identity on APFS/NTFS: a reused payload
        // may have a reference with different case (or Unicode normalization).
        let live = self.referenced_payload_identities()?;
        let mut removed = 0_u64;
        for directory in [
            self.file_history_dir.clone(),
            self.image_history_dir.clone(),
        ] {
            let entries = match std::fs::read_dir(&directory) {
                Ok(entries) => entries,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            for entry in entries {
                let entry = entry?;
                let path = entry.path();
                let name = entry.file_name();
                let name = name.to_string_lossy();
                let is_atomic_temp = name.ends_with(".tmp")
                    || name.contains(".tmp.")
                    || (name.starts_with('.') && name.contains(".tmp"));
                if !entry.file_type()?.is_file() {
                    continue;
                }
                match payload_file_identity(&path) {
                    Ok(identity) if live.contains(&identity) => continue,
                    Ok(_) => {}
                    Err(error) => {
                        warn!(
                            "Could not identify payload {}: {error}; leaving it in place",
                            path.display()
                        );
                        continue;
                    }
                }
                let age = match entry.metadata().and_then(|meta| meta.modified()).ok() {
                    Some(modified) => match std::time::SystemTime::now().duration_since(modified) {
                        Ok(age) => age,
                        // Clock skew: treat as fresh rather than deleting on a guess.
                        Err(_) => continue,
                    },
                    None => {
                        warn!(
                            "Could not read the mtime of {}; leaving it in place",
                            path.display()
                        );
                        continue;
                    }
                };
                if age < grace {
                    continue;
                }
                if is_atomic_temp {
                    match crate::private_fs::collect_inactive_atomic_temp(&path) {
                        Ok(true) => removed += 1,
                        Ok(false) => {}
                        Err(error) => {
                            warn!("Could not collect atomic temp {}: {error}", path.display())
                        }
                    }
                    continue;
                }
                match std::fs::remove_file(&path) {
                    Ok(()) => removed += 1,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => {
                        warn!(
                            "Could not remove orphaned payload {}: {error}",
                            path.display()
                        );
                    }
                }
            }
        }
        Ok(removed)
    }

    /// File identities of every surviving external reference. Resolve the
    /// complete live set before deleting anything; uncertainty aborts the sweep.
    pub(super) fn validate_live_payload_references(
        &self,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.referenced_payload_identities().map(|_| ())
    }

    /// Visit every live external payload path, failing closed on an invalid or
    /// unreadable reference.
    fn for_each_live_payload_path<F>(&self, visit: F) -> Result<(), Box<dyn std::error::Error>>
    where
        F: FnMut(&std::path::Path) -> Result<(), Box<dyn std::error::Error>>,
    {
        for_each_live_payload_path(
            &self.conn,
            &self.file_history_dir,
            &self.image_history_dir,
            self.has_composite_schema(),
            visit,
        )
    }

    fn referenced_payload_identities(
        &self,
    ) -> Result<std::collections::HashSet<PayloadFileIdentity>, Box<dyn std::error::Error>> {
        let mut live = std::collections::HashSet::new();
        self.for_each_live_payload_path(|path| {
            match payload_file_identity(path) {
                Ok(identity) => {
                    live.insert(identity);
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            Ok(())
        })?;
        Ok(live)
    }
}

#[derive(Debug, Eq, PartialEq, Hash)]
struct PayloadFileIdentity {
    volume: u64,
    file_id: [u8; 16],
}

/// Owns freshly written payload files until their transaction commits.
///
/// See [`HistoryDB::arm_pending_payloads`] for why the live set is snapshotted rather
/// than read at drop time: the drop happens while the caller still holds `&mut self`
/// for the transaction, and the snapshot is exact because a rolled-back transaction
/// cannot have changed which payloads are live.
pub(super) struct PendingPayloads {
    written: Vec<PathBuf>,
    /// `None` when the live set could not be read; then nothing is removed.
    live: Option<std::collections::HashSet<PayloadFileIdentity>>,
    committed: bool,
}

impl PendingPayloads {
    fn arm(db: &HistoryDB, written: &[super::entries::ExternalHistoryPayload]) -> Self {
        let live = match db.referenced_payload_identities() {
            Ok(live) => Some(live),
            Err(error) => {
                warn!("Cannot safely remove history payloads after a failed write: {error}");
                None
            }
        };
        Self {
            written: written.iter().map(|payload| payload.path.clone()).collect(),
            live,
            committed: false,
        }
    }

    /// Disarm: the transaction committed, so the payloads are live.
    pub(super) fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for PendingPayloads {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        let Some(live) = self.live.as_ref() else {
            return;
        };
        for path in &self.written {
            match payload_file_identity(path) {
                Ok(identity) if live.contains(&identity) => continue,
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    warn!(
                        "Cannot identify history payload {}: {error}",
                        path.display()
                    );
                    continue;
                }
            }
            if let Err(error) = std::fs::remove_file(path) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    warn!("Cannot remove history payload {}: {error}", path.display());
                }
            }
        }
    }
}

/// Visit every live external payload path for a raw connection.
///
/// Free-function form so the pre-upgrade rollback package can enumerate payloads
/// *before* a `HistoryDB` exists (repair plan §4 R1-03). A legacy database has no
/// `history_parts`, so the live set is then just the history table.
pub(super) fn for_each_live_payload_path<F>(
    conn: &Connection,
    file_history_dir: &Path,
    image_history_dir: &Path,
    has_composite_schema: bool,
    mut visit: F,
) -> Result<(), Box<dyn std::error::Error>>
where
    F: FnMut(&std::path::Path) -> Result<(), Box<dyn std::error::Error>>,
{
    let sql = if has_composite_schema {
        "SELECT type, data FROM history UNION ALL SELECT 'image', stored FROM history_parts"
    } else {
        "SELECT type, data FROM history"
    };
    let mut statement = conn.prepare(sql)?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
    })?;
    for row in rows {
        let (entry_type, stored) = row?;
        let (directory, reference) = match entry_type.as_str() {
            "file" => (
                file_history_dir.to_path_buf(),
                decode_file_reference(&stored),
            ),
            "image" | "text" => (
                image_history_dir.to_path_buf(),
                decode_image_reference(&stored),
            ),
            _ => continue,
        };
        let Some(reference) = reference else {
            if entry_type == "file"
                || stored.starts_with(super::file_storage::IMAGE_REFERENCE_MAGIC)
            {
                return Err(
                    "Cannot safely sweep payloads: a live external reference is invalid".into(),
                );
            }
            continue; // Inline text/image data is not an external reference.
        };
        let path = resolve_file_reference_at(&directory, &reference)?;
        visit(&path)?;
    }
    Ok(())
}

/// Absolute paths of every live external payload for a raw connection.
pub(super) fn live_payload_paths(
    conn: &Connection,
    file_history_dir: &Path,
    image_history_dir: &Path,
    has_composite_schema: bool,
) -> Result<Vec<std::path::PathBuf>, Box<dyn std::error::Error>> {
    let mut paths = std::collections::BTreeSet::new();
    for_each_live_payload_path(
        conn,
        file_history_dir,
        image_history_dir,
        has_composite_schema,
        |path| {
            paths.insert(path.to_path_buf());
            Ok(())
        },
    )?;
    Ok(paths.into_iter().collect())
}

#[cfg(unix)]
fn payload_file_identity(path: &std::path::Path) -> std::io::Result<PayloadFileIdentity> {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::metadata(path)?;
    Ok(PayloadFileIdentity {
        volume: metadata.dev(),
        file_id: u128::from(metadata.ino()).to_le_bytes(),
    })
}

#[cfg(windows)]
fn payload_file_identity(path: &std::path::Path) -> std::io::Result<PayloadFileIdentity> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FileIdInfo, GetFileInformationByHandleEx, FILE_ID_INFO,
    };
    let file = std::fs::File::open(path)?;
    let mut info: FILE_ID_INFO = unsafe { std::mem::zeroed() };
    // Read the volume and full 128-bit file ID while the handle remains open.
    // Keep only the ID so sweeping many rows does not exhaust file handles.
    let success = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileIdInfo,
            (&mut info as *mut FILE_ID_INFO).cast(),
            std::mem::size_of::<FILE_ID_INFO>() as u32,
        )
    };
    if success == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(PayloadFileIdentity {
        volume: info.VolumeSerialNumber,
        file_id: info.FileId.Identifier,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_all_reports_removal_failure_and_continues_other_directories() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-clear-all-failure-{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let mut db = HistoryDB::open_at(&root).unwrap();
        let blocked = db.add_file("blocked.bin", b"blocked", "self").unwrap();
        db.add_image(&[1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0], "self")
            .unwrap();
        let image_entry = std::fs::read_dir(&db.image_history_dir)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();

        let result = db.clear_all_with(|path| {
            if path == blocked {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "injected removal failure",
                ));
            }
            remove_history_entry(path)
        });

        assert!(result.is_err());
        assert!(blocked.exists());
        assert!(!image_entry.exists());
        let row_count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM history", [], |row| row.get(0))
            .unwrap();
        assert_eq!(row_count, 0);

        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
}
