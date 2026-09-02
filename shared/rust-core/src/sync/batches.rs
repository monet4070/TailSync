use super::*;

fn prepare_incoming_batch(
    manifest: FileBatchManifest,
    source: String,
    source_device_id: String,
    incoming_dir: &Path,
    session_epoch: u64,
    default_generation: u64,
) -> Result<(IncomingBatch, bool), String> {
    crate::private_fs::create_private_dir_all(incoming_dir).map_err(|error| error.to_string())?;
    let manifest_path = incoming_dir.join(format!("{}.batch.json", manifest.batch_id.as_hex()));
    let mut files = vec![None; manifest.files.len()];
    let mut local_generation = default_generation;
    let mut persisted_source_device_id = source_device_id;
    let mut restored_generation = false;
    if let Ok(data) = fs::read(&manifest_path) {
        if let Ok(saved) = serde_json::from_slice::<PersistedIncomingBatch>(&data) {
            if saved.source != source || saved.manifest != manifest {
                return Err("Batch ID was reused with a different source or manifest".to_string());
            }
            if saved.files.len() != manifest.files.len() {
                return Err("Persisted file batch state has an invalid file count".to_string());
            }
            local_generation = saved.local_generation;
            if !saved.source_device_id.is_empty() {
                persisted_source_device_id = saved.source_device_id.clone();
            }
            restored_generation = true;
            for (index, file) in saved.files.into_iter().enumerate() {
                if let Some(file) = file {
                    if let Some(file) =
                        restore_persisted_received_file(&file, &manifest.files[index], incoming_dir)
                    {
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
            source_device_id: persisted_source_device_id.clone(),
            manifest: manifest.clone(),
            files: files.clone(),
            local_generation,
        },
    )
    .map_err(|error| error.to_string())?;
    Ok((
        IncomingBatch {
            manifest,
            source,
            source_device_id: persisted_source_device_id,
            session_epoch,
            local_generation,
            files,
            manifest_path,
        },
        restored_generation,
    ))
}

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
        self.begin_file_batch_at_epoch_with_identity(
            manifest,
            source.clone(),
            source,
            incoming_dir,
            session_epoch,
        )
    }

    pub fn begin_file_batch_at_epoch(
        &mut self,
        manifest: FileBatchManifest,
        source: String,
        incoming_dir: &Path,
        session_epoch: u64,
    ) -> Result<(), String> {
        self.begin_file_batch_at_epoch_with_identity(
            manifest,
            source.clone(),
            source,
            incoming_dir,
            session_epoch,
        )
    }

    pub fn begin_file_batch_at_epoch_with_identity(
        &mut self,
        manifest: FileBatchManifest,
        source: String,
        source_device_id: String,
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
        self.prune_completed_batches();
        if self.completed_batches.contains_key(&key) {
            if self
                .completed_batch_manifests
                .get(&key)
                .is_some_and(|completed| completed != &manifest)
            {
                return Err("Batch ID was reused with a different manifest".to_string());
            }
            return Ok(());
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
        let mut persisted_source_device_id = source_device_id.clone();
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
                if !saved.source_device_id.is_empty() {
                    persisted_source_device_id = saved.source_device_id.clone();
                }
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
                source_device_id: persisted_source_device_id.clone(),
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
                source_device_id: persisted_source_device_id,
                session_epoch,
                local_generation,
                files,
                manifest_path,
            },
        );
        Ok(())
    }

    /// Open/restore a batch with only short state-mutex sections. The
    /// admission lock remains the quota boundary, while manifest I/O is
    /// isolated to this batch's operation lock.
    pub async fn begin_file_batch_shared(
        sync_engine: &Arc<tokio::sync::Mutex<SyncEngine>>,
        manifest: FileBatchManifest,
        source: String,
        source_device_id: String,
        incoming_dir: PathBuf,
        session_epoch: u64,
    ) -> Result<(), String> {
        manifest.validate().map_err(|error| error.to_string())?;
        let key = (source.clone(), manifest.batch_id);
        let operation_lock = {
            let mut engine = sync_engine.lock().await;
            engine.batch_operation_lock(&key)
        };
        let _operation_guard = operation_lock.lock().await;
        let local_generation = {
            let mut engine = sync_engine.lock().await;
            engine.prune_cancelled_batches();
            if engine.cancelled_batches.contains_key(&key) {
                return Err("File batch was cancelled; copy the files again to retry".to_string());
            }
            engine.prune_completed_batches();
            if engine.completed_batches.contains_key(&key) {
                if engine
                    .completed_batch_manifests
                    .get(&key)
                    .is_some_and(|completed| completed != &manifest)
                {
                    return Err("Batch ID was reused with a different manifest".to_string());
                }
                return Ok(());
            }
            if let Some(existing) = engine.incoming_batches.get_mut(&key) {
                if existing.manifest == manifest {
                    existing.session_epoch = existing.session_epoch.max(session_epoch);
                    return Ok(());
                }
                return Err("Batch ID was reused with a different manifest".to_string());
            }
            let active_for_peer = engine
                .incoming_batches
                .keys()
                .filter(|(peer, _)| peer == &source)
                .count();
            if active_for_peer >= MAX_ACTIVE_BATCHES_PER_PEER {
                return Err(format!(
                    "peer {source} already has {MAX_ACTIVE_BATCHES_PER_PEER} active file batches"
                ));
            }
            if engine.incoming_batches.len() >= MAX_ACTIVE_BATCHES_GLOBAL {
                return Err(format!(
                    "global active file batch limit ({MAX_ACTIVE_BATCHES_GLOBAL}) reached"
                ));
            }
            engine.clipboard_generation.wrapping_add(1).max(1)
        };

        let (batch, restored_generation) = prepare_incoming_batch(
            manifest,
            source.clone(),
            source_device_id,
            &incoming_dir,
            session_epoch,
            local_generation,
        )?;
        let mut engine = sync_engine.lock().await;
        if !restored_generation {
            engine.clipboard_generation = local_generation;
        }
        engine.incoming_batches.insert(key, batch);
        Ok(())
    }

    pub fn has_file_batch(&self, source: &str, batch_id: TransferId) -> bool {
        self.incoming_batches
            .contains_key(&(source.to_string(), batch_id))
    }

    pub fn is_file_batch_completed(&self, source: &str, batch_id: TransferId) -> bool {
        self.completed_batches
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

    pub async fn finish_file_batch(
        &mut self,
        source: &str,
        batch_id: TransferId,
    ) -> Result<(), String> {
        let key = (source.to_string(), batch_id);
        let now = chrono::Utc::now().timestamp();
        self.prune_completed_batches();
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
        let total = batch.manifest.files.len();
        let activate_clipboard = batch.local_generation == self.clipboard_generation;
        let source_device_id = batch.source_device_id.clone();
        let manifest_hash = Self::file_batch_manifest_hash(&batch.manifest)?;
        let platform = self
            .platform
            .clone()
            .ok_or_else(|| "Clipboard platform is unavailable".to_string())?;

        // The platform adapter owns the SQLite/file-history transaction. Do
        // not discard the durable receive manifest or acknowledge the batch
        // until that transaction has completed successfully. If it fails,
        // the sender keeps its journal and can retry the same batch.
        platform
            .files_received(FileReceiveCommit {
                batch_id: Some(batch_id),
                files,
                batch_total: total,
                batch_complete: true,
                activate_clipboard,
                device: source.to_string(),
                source_device_id,
                manifest_hash: Some(manifest_hash),
            })
            .await?;

        let batch = self
            .incoming_batches
            .remove(&key)
            .ok_or_else(|| "File batch state disappeared".to_string())?;
        let _ = fs::remove_file(batch.manifest_path);
        self.completed_batches.insert(key.clone(), now);
        self.completed_batch_manifests.insert(key, batch.manifest);
        Ok(())
    }

    /// Complete a batch without holding the engine mutex while the platform
    /// adapter persists history. The batch lock prevents a concurrent cancel
    /// from racing the durable commit.
    pub async fn finish_file_batch_shared(
        sync_engine: &Arc<tokio::sync::Mutex<SyncEngine>>,
        source: &str,
        batch_id: TransferId,
    ) -> Result<(), String> {
        let key = (source.to_string(), batch_id);
        let operation_lock = {
            let mut engine = sync_engine.lock().await;
            engine.batch_operation_lock(&key)
        };
        let _operation_guard = operation_lock.lock().await;
        let receive_operation_locks = {
            let mut engine = sync_engine.lock().await;
            let mut keys = engine
                .active_receives
                .iter()
                .filter_map(|((peer, receive_key), state)| {
                    (peer == source
                        && state
                            .meta
                            .batch
                            .is_some_and(|batch| batch.batch_id == batch_id))
                    .then_some((peer.clone(), *receive_key))
                })
                .collect::<Vec<_>>();
            keys.extend(
                engine
                    .inflight_receives
                    .iter()
                    .filter(|(peer, _)| peer == source)
                    .cloned(),
            );
            keys.extend(
                engine
                    .pending_receives
                    .iter()
                    .filter(|((peer, _), _)| peer == source)
                    .map(|(key, _)| key.clone()),
            );
            keys.sort_unstable_by_key(|(peer, receive_key)| {
                (peer.clone(), format!("{receive_key:?}"))
            });
            keys.dedup();
            keys.iter()
                .map(|key| engine.receive_operation_lock(key))
                .collect::<Vec<_>>()
        };
        let mut receive_operation_guards = Vec::with_capacity(receive_operation_locks.len());
        for receive_operation_lock in receive_operation_locks {
            receive_operation_guards.push(receive_operation_lock.lock_owned().await);
        }
        let (commit, platform, manifest_path, manifest, now) = {
            let mut engine = sync_engine.lock().await;
            let now = chrono::Utc::now().timestamp();
            engine.prune_completed_batches();
            let Some(batch) = engine.incoming_batches.get(&key) else {
                engine.prune_cancelled_batches();
                if engine.completed_batches.contains_key(&key)
                    || engine.cancelled_batches.contains_key(&key)
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
            let manifest = batch.manifest.clone();
            let commit = FileReceiveCommit {
                batch_id: Some(batch_id),
                files,
                batch_total: manifest.files.len(),
                batch_complete: true,
                activate_clipboard: batch.local_generation == engine.clipboard_generation,
                device: source.to_string(),
                source_device_id: batch.source_device_id.clone(),
                manifest_hash: Some(Self::file_batch_manifest_hash(&manifest)?),
            };
            let platform = engine
                .platform
                .clone()
                .ok_or_else(|| "Clipboard platform is unavailable".to_string())?;
            (commit, platform, batch.manifest_path.clone(), manifest, now)
        };

        platform.files_received(commit).await?;

        let mut engine = sync_engine.lock().await;
        let Some(_batch) = engine.incoming_batches.remove(&key) else {
            if engine.completed_batches.contains_key(&key) {
                return Ok(());
            }
            return Err("File batch state disappeared after durable commit".to_string());
        };
        let _ = fs::remove_file(manifest_path);
        engine.completed_batches.insert(key.clone(), now);
        engine.completed_batch_manifests.insert(key, manifest);
        drop(receive_operation_guards);
        Ok(())
    }

    pub(super) fn prune_completed_batches(&mut self) {
        let now = chrono::Utc::now().timestamp();
        self.completed_batches.retain(|_, completed_at| {
            now.saturating_sub(*completed_at) <= SEEN_MESSAGE_RETENTION_SECONDS
        });
        self.completed_batch_manifests
            .retain(|key, _| self.completed_batches.contains_key(key));
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
                if let Some(platform) = self.platform.clone() {
                    let source_device_id = batch.source_device_id.clone();
                    let manifest_hash = Self::file_batch_manifest_hash(&batch.manifest).ok();
                    if let Err(error) = platform
                        .files_received(FileReceiveCommit {
                            batch_id: Some(batch_id),
                            files: completed,
                            batch_total: batch.manifest.files.len(),
                            batch_complete: false,
                            activate_clipboard: false,
                            device: source.to_string(),
                            source_device_id,
                            manifest_hash,
                        })
                        .await
                    {
                        platform.file_batch_failed(Some(batch_id), &error);
                    }
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

    /// Cancel a batch while keeping platform persistence and temp-file
    /// cleanup outside the engine mutex. Returns whether any receive state was
    /// present for the batch.
    pub async fn cancel_file_batch_shared(
        sync_engine: &Arc<tokio::sync::Mutex<SyncEngine>>,
        source: &str,
        batch_id: TransferId,
    ) -> bool {
        let key = (source.to_string(), batch_id);
        let operation_lock = {
            let mut engine = sync_engine.lock().await;
            engine.batch_operation_lock(&key)
        };
        let _operation_guard = operation_lock.lock().await;
        let receive_operation_locks = {
            let mut engine = sync_engine.lock().await;
            let mut keys = engine
                .active_receives
                .iter()
                .filter_map(|((peer, receive_key), state)| {
                    (peer == source
                        && state
                            .meta
                            .batch
                            .is_some_and(|batch| batch.batch_id == batch_id))
                    .then_some((peer.clone(), *receive_key))
                })
                .collect::<Vec<_>>();
            // A chunk temporarily removes its state from `active_receives`
            // while doing disk I/O. Waiting for all in-flight receives from
            // this source closes that cancellation race without widening the
            // engine critical section.
            keys.extend(
                engine
                    .inflight_receives
                    .iter()
                    .filter(|(peer, _)| peer == source)
                    .cloned(),
            );
            keys.extend(
                engine
                    .pending_receives
                    .iter()
                    .filter(|((peer, _), _)| peer == source)
                    .map(|(key, _)| key.clone()),
            );
            keys.sort_unstable_by_key(|(peer, receive_key)| {
                (peer.clone(), format!("{receive_key:?}"))
            });
            keys.dedup();
            keys.iter()
                .map(|key| engine.receive_operation_lock(key))
                .collect::<Vec<_>>()
        };
        let mut receive_operation_guards = Vec::with_capacity(receive_operation_locks.len());
        for receive_operation_lock in receive_operation_locks {
            receive_operation_guards.push(receive_operation_lock.lock_owned().await);
        }
        let (batch, states, platform) = {
            let mut engine = sync_engine.lock().await;
            engine.prune_cancelled_batches();
            engine
                .cancelled_batches
                .insert(key.clone(), chrono::Utc::now().timestamp());
            let batch = engine.incoming_batches.remove(&key);
            let receive_keys = engine
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
            let states = receive_keys
                .into_iter()
                .filter_map(|receive_key| {
                    engine
                        .active_receives
                        .remove(&(source.to_string(), receive_key))
                })
                .collect::<Vec<_>>();
            let platform = engine.platform.clone();
            (batch, states, platform)
        };

        let mut was_receiving = !states.is_empty();
        for state in states {
            was_receiving = true;
            drop(state.writer);
            let _ = fs::remove_file(state.tmp_path);
            if let Some(path) = state.state_path {
                let _ = fs::remove_file(path);
            }
        }
        if let Some(batch) = batch {
            was_receiving = true;
            let _ = fs::remove_file(&batch.manifest_path);
            let completed = batch.files.into_iter().flatten().collect::<Vec<_>>();
            if !completed.is_empty() {
                if let Some(platform) = platform {
                    let source_device_id = batch.source_device_id.clone();
                    let manifest_hash = Self::file_batch_manifest_hash(&batch.manifest).ok();
                    if let Err(error) = platform
                        .files_received(FileReceiveCommit {
                            batch_id: Some(batch_id),
                            files: completed,
                            batch_total: batch.manifest.files.len(),
                            batch_complete: false,
                            activate_clipboard: false,
                            device: source.to_string(),
                            source_device_id,
                            manifest_hash,
                        })
                        .await
                    {
                        platform.file_batch_failed(Some(batch_id), &error);
                    }
                }
            } else if let Some(platform) = platform {
                platform.clear_file_progress(Some(batch_id), Some(source));
            }
        }
        drop(receive_operation_guards);
        was_receiving
    }

    pub fn file_batch_manifest_hash(manifest: &FileBatchManifest) -> Result<String, String> {
        let encoded = serde_json::to_vec(manifest).map_err(|error| error.to_string())?;
        Ok(blake3::hash(&encoded).to_hex().to_string())
    }

    /// Seed the in-memory completion cache from a durable receipt so a replay
    /// after process restart can answer every file with its final offset
    /// without recreating receive sidecars or rewriting the payload.
    pub fn remember_completed_file_batch(
        &mut self,
        manifest: FileBatchManifest,
        source: String,
    ) -> Result<(), String> {
        manifest.validate().map_err(|error| error.to_string())?;
        self.prune_completed_batches();
        let key = (source, manifest.batch_id);
        self.completed_batches
            .insert(key.clone(), chrono::Utc::now().timestamp());
        self.completed_batch_manifests.insert(key, manifest);
        Ok(())
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

    /// Locate and cancel a local incoming batch without awaiting platform
    /// persistence while the global engine mutex is held.
    pub async fn cancel_file_batch_local_shared(
        sync_engine: &Arc<tokio::sync::Mutex<SyncEngine>>,
        batch_id: TransferId,
    ) -> Option<String> {
        let source = {
            let engine = sync_engine.lock().await;
            engine
                .incoming_batches
                .keys()
                .find_map(|(source, id)| (*id == batch_id).then(|| source.clone()))
        };
        if let Some(source) = source.as_deref() {
            Self::cancel_file_batch_shared(sync_engine, source, batch_id).await;
        }
        source
    }
}
