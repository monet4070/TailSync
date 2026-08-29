use super::*;

impl SyncEngine {
    pub fn supersede_file_clipboard(&mut self) -> u64 {
        self.clipboard_generation = self.clipboard_generation.wrapping_add(1).max(1);
        self.clipboard_generation
    }

    pub fn begin_file_batch(
        &mut self,
        manifest: FileBatchManifest,
        source: String,
        incoming_dir: &Path,
    ) -> Result<(), String> {
        let session_epoch = self.receive_epoch(&source);
        self.begin_file_batch_at_epoch(manifest, source, incoming_dir, session_epoch)
    }

    pub fn begin_file_batch_at_epoch(
        &mut self,
        manifest: FileBatchManifest,
        source: String,
        incoming_dir: &Path,
        session_epoch: u64,
    ) -> Result<(), String> {
        // Validate before touching disk. The server also validates before its
        // quota preflight, but this remains the authoritative core check.
        manifest.validate().map_err(|error| error.to_string())?;
        let key = (source.clone(), manifest.batch_id);
        self.prune_cancelled_batches();
        if self.cancelled_batches.contains_key(&key) {
            return Err("File batch was cancelled; copy the files again to retry".to_string());
        }
        if let Some(existing) = self.incoming_batches.get_mut(&key) {
            if existing.manifest == manifest {
                existing.session_epoch = existing.session_epoch.max(session_epoch);
                return Ok(());
            }
            return Err("Batch ID was reused with a different manifest".to_string());
        }
        let active_for_peer = self
            .incoming_batches
            .keys()
            .filter(|(peer, _)| peer == &source)
            .count();
        if active_for_peer >= MAX_ACTIVE_BATCHES_PER_PEER {
            return Err(format!(
                "peer {source} already has {MAX_ACTIVE_BATCHES_PER_PEER} active file batches"
            ));
        }
        if self.incoming_batches.len() >= MAX_ACTIVE_BATCHES_GLOBAL {
            return Err(format!(
                "global active file batch limit ({MAX_ACTIVE_BATCHES_GLOBAL}) reached"
            ));
        }
        crate::private_fs::create_private_dir_all(incoming_dir)
            .map_err(|error| error.to_string())?;
        let manifest_path = incoming_dir.join(format!("{}.batch.json", manifest.batch_id.as_hex()));
        let mut files = vec![None; manifest.files.len()];
        let mut local_generation = self.clipboard_generation.wrapping_add(1).max(1);
        let mut restored_generation = false;
        if let Ok(data) = fs::read(&manifest_path) {
            if let Ok(saved) = serde_json::from_slice::<PersistedIncomingBatch>(&data) {
                if saved.source != source || saved.manifest != manifest {
                    return Err(
                        "Batch ID was reused with a different source or manifest".to_string()
                    );
                }
                if saved.files.len() != manifest.files.len() {
                    return Err("Persisted file batch state has an invalid file count".to_string());
                }
                local_generation = saved.local_generation;
                restored_generation = true;
                for (index, file) in saved.files.into_iter().enumerate() {
                    if let Some(file) = file {
                        if let Some(file) = restore_persisted_received_file(
                            &file,
                            &manifest.files[index],
                            incoming_dir,
                        ) {
                            files[index] = Some(file);
                        }
                    }
                }
            }
        }
        persist_incoming_batch(
            &manifest_path,
            &PersistedIncomingBatch {
                source: source.clone(),
                manifest: manifest.clone(),
                files: files.clone(),
                local_generation,
            },
        )
        .map_err(|error| error.to_string())?;
        if !restored_generation {
            self.clipboard_generation = local_generation;
        }
        self.incoming_batches.insert(
            key,
            IncomingBatch {
                manifest,
                source,
                session_epoch,
                local_generation,
                files,
                manifest_path,
            },
        );
        Ok(())
    }

    pub fn has_file_batch(&self, source: &str, batch_id: TransferId) -> bool {
        self.incoming_batches
            .contains_key(&(source.to_string(), batch_id))
    }

    /// Bytes promised by accepted file batches that have not reached disk yet.
    /// Existing `.part` and completed files are already included in storage
    /// usage, so only their remaining bytes count toward the next preflight.
    pub fn pending_file_batch_bytes(&self) -> u64 {
        self.incoming_batches.values().fold(0_u64, |total, batch| {
            let incoming_dir = batch.manifest_path.parent();
            let remaining = batch
                .manifest
                .files
                .iter()
                .zip(&batch.files)
                .filter_map(|(entry, completed)| completed.is_none().then_some(entry))
                .fold(0_u64, |remaining, entry| {
                    let received = incoming_dir
                        .map(|directory| {
                            directory.join(format!("{}.part", entry.transfer_id.as_hex()))
                        })
                        .and_then(|path| fs::metadata(path).ok())
                        .map(|metadata| metadata.len().min(entry.size))
                        .unwrap_or(0);
                    remaining.saturating_add(entry.size.saturating_sub(received))
                });
            total.saturating_add(remaining)
        })
    }

    pub fn batch_for_transfer(&self, source: &str, transfer_id: TransferId) -> Option<TransferId> {
        self.active_receives
            .get(&(source.to_string(), ReceiveKey::Resumable(transfer_id)))
            .and_then(|state| state.meta.batch.map(|batch| batch.batch_id))
    }

    pub fn notify_file_batch_failed(&self, batch_id: Option<TransferId>, message: &str) {
        if let Some(platform) = self.platform.as_ref() {
            platform.file_batch_failed(batch_id, message);
        }
    }

    pub fn finish_file_batch(&mut self, source: &str, batch_id: TransferId) -> Result<(), String> {
        let key = (source.to_string(), batch_id);
        let now = chrono::Utc::now().timestamp();
        self.completed_batches.retain(|_, completed_at| {
            now.saturating_sub(*completed_at) <= SEEN_MESSAGE_RETENTION_SECONDS
        });
        let Some(batch) = self.incoming_batches.get(&key) else {
            self.prune_cancelled_batches();
            if self.completed_batches.contains_key(&key)
                || self.cancelled_batches.contains_key(&key)
            {
                return Ok(());
            }
            return Err("File batch manifest is not available".to_string());
        };
        let files = batch
            .files
            .clone()
            .into_iter()
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| "File batch is incomplete".to_string())?;
        let batch = self
            .incoming_batches
            .remove(&key)
            .ok_or_else(|| "File batch state disappeared".to_string())?;
        let _ = fs::remove_file(batch.manifest_path);
        let activate_clipboard = batch.local_generation == self.clipboard_generation;
        if let Some(platform) = self.platform.as_ref() {
            platform.files_received(
                Some(batch_id),
                files,
                batch.manifest.files.len(),
                true,
                activate_clipboard,
                source.to_string(),
            );
        }
        self.completed_batches.insert(key, now);
        Ok(())
    }

    pub async fn cancel_file_batch(&mut self, source: &str, batch_id: TransferId) {
        let key = (source.to_string(), batch_id);
        self.prune_cancelled_batches();
        self.cancelled_batches
            .insert(key.clone(), chrono::Utc::now().timestamp());
        if let Some(batch) = self.incoming_batches.remove(&key) {
            let _ = fs::remove_file(batch.manifest_path);
            let completed = batch.files.into_iter().flatten().collect::<Vec<_>>();
            if !completed.is_empty() {
                if let Some(platform) = self.platform.as_ref() {
                    platform.files_received(
                        Some(batch_id),
                        completed,
                        batch.manifest.files.len(),
                        false,
                        false,
                        source.to_string(),
                    );
                }
            } else {
                self.clear_file_progress(Some(batch_id), Some(source));
            }
        }
        let receive_keys = self
            .active_receives
            .iter()
            .filter_map(|((peer, receive_key), state)| {
                (peer == source
                    && state
                        .meta
                        .batch
                        .is_some_and(|batch| batch.batch_id == batch_id))
                .then_some(*receive_key)
            })
            .collect::<Vec<_>>();
        for receive_key in receive_keys {
            if let Some(state) = self
                .active_receives
                .remove(&(source.to_string(), receive_key))
            {
                drop(state.writer);
                let _ = fs::remove_file(state.tmp_path);
                if let Some(path) = state.state_path {
                    let _ = fs::remove_file(path);
                }
            }
        }
    }

    pub async fn cancel_file_batch_local(&mut self, batch_id: TransferId) -> Option<String> {
        let source = self
            .incoming_batches
            .keys()
            .find_map(|(source, id)| (*id == batch_id).then(|| source.clone()));
        if let Some(source) = source {
            self.cancel_file_batch(&source, batch_id).await;
            return Some(source);
        }
        None
    }
}
