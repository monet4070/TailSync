use super::*;

impl SyncEngine {
    /// Open a new transfer or restore its durable `.part` + sidecar state.
    pub async fn begin_file_receive(
        &mut self,
        meta: FileMeta,
        file_path: &Path,
        source: String,
    ) -> Result<FileReceiveProgress, String> {
        let session_epoch = self.receive_epoch(&source);
        self.begin_file_receive_at_epoch(meta, file_path, source, session_epoch)
            .await
    }

    pub async fn begin_file_receive_at_epoch(
        &mut self,
        meta: FileMeta,
        file_path: &Path,
        source: String,
        session_epoch: u64,
    ) -> Result<FileReceiveProgress, String> {
        let transfer_id = meta.transfer_id.unwrap_or(TransferId([0; 16]));
        let receive_key = ReceiveKey::from(meta.transfer_id);
        let key = (source.clone(), receive_key);
        if let Some(batch_ref) = meta.batch {
            let batch_key = (source.clone(), batch_ref.batch_id);
            self.prune_cancelled_batches();
            if self.cancelled_batches.contains_key(&batch_key) {
                return Err("File batch was cancelled".to_string());
            }
            self.prune_completed_batches();
            if self.completed_batches.contains_key(&batch_key) {
                let completed_manifest = self
                    .completed_batch_manifests
                    .get(&batch_key)
                    .ok_or_else(|| "Completed file batch metadata is unavailable".to_string())?;
                let expected = completed_manifest
                    .files
                    .get(usize::from(batch_ref.index))
                    .ok_or_else(|| "File batch index is out of range".to_string())?;
                if expected.transfer_id != transfer_id
                    || expected.name != meta.name
                    || expected.size != meta.size
                    || expected.hash != meta.hash
                    || expected.chunk_size != meta.chunk_size
                {
                    return Err(
                        "File metadata does not match the completed batch manifest".to_string()
                    );
                }
                return Ok(FileReceiveProgress {
                    transfer_id,
                    next_offset: meta.size,
                    completed: None,
                });
            }
            let batch = self.incoming_batches.get(&batch_key).ok_or_else(|| {
                "File batch manifest must be accepted before file data".to_string()
            })?;
            let expected = batch
                .manifest
                .files
                .get(usize::from(batch_ref.index))
                .ok_or_else(|| "File batch index is out of range".to_string())?;
            if expected.transfer_id != transfer_id
                || expected.name != meta.name
                || expected.size != meta.size
                || expected.hash != meta.hash
                || expected.chunk_size != meta.chunk_size
            {
                return Err("File metadata does not match the accepted batch manifest".to_string());
            }
            if let Some(existing) = batch
                .files
                .get(usize::from(batch_ref.index))
                .and_then(Option::as_ref)
            {
                if existing.size == meta.size
                    && existing.hash == meta.hash
                    && existing.path.is_file()
                {
                    return Ok(FileReceiveProgress {
                        transfer_id,
                        next_offset: existing.size,
                        completed: None,
                    });
                }
            }
        }
        let now = chrono::Utc::now().timestamp();
        self.completed_transfers.retain(|_, completed| {
            now.saturating_sub(completed.completed_at) <= SEEN_MESSAGE_RETENTION_SECONDS
        });
        if let Some(completed) = self.completed_transfers.get(&key) {
            if completed.size == meta.size && completed.hash == meta.hash {
                return Ok(FileReceiveProgress {
                    transfer_id,
                    next_offset: completed.size,
                    completed: None,
                });
            }
            self.completed_transfers.remove(&key);
        }
        if let Some(state) = self.active_receives.get_mut(&key) {
            if state.meta.hash == meta.hash && state.meta.size == meta.size {
                state.session_epoch = state.session_epoch.max(session_epoch);
                return Ok(FileReceiveProgress {
                    transfer_id,
                    next_offset: state.received,
                    completed: None,
                });
            }
            return Err("transfer ID was reused with different metadata".to_string());
        }

        let active_for_peer = self
            .active_receives
            .keys()
            .filter(|(peer, _)| peer == &source)
            .count();
        if active_for_peer >= MAX_ACTIVE_RECEIVES_PER_PEER {
            return Err(format!(
                "peer {source} already has {MAX_ACTIVE_RECEIVES_PER_PEER} active file receives"
            ));
        }
        if self.active_receives.len() >= MAX_ACTIVE_RECEIVES_GLOBAL {
            return Err(format!(
                "global active file receive limit ({MAX_ACTIVE_RECEIVES_GLOBAL}) reached"
            ));
        }

        let resumable = meta.transfer_id.is_some();
        let parent = file_path
            .parent()
            .ok_or_else(|| "incoming file path has no parent".to_string())?;
        crate::private_fs::create_private_dir_all(parent).map_err(|error| error.to_string())?;
        let id = transfer_id.as_hex();
        let tmp_path = if resumable {
            parent.join(format!("{id}.part"))
        } else {
            file_path.with_extension("tmp")
        };
        let state_path = resumable.then(|| parent.join(format!("{id}.resume.json")));

        let mut final_path = file_path.to_path_buf();
        if let Some(ref path) = state_path {
            if let Ok(data) = fs::read(path) {
                if let Ok(saved) = serde_json::from_slice::<PersistedTransfer>(&data) {
                    if saved.source == source
                        && saved.meta.hash == meta.hash
                        && saved.meta.size == meta.size
                    {
                        if let Some(file_name) = saved.final_path.file_name() {
                            final_path = parent.join(file_name);
                        }
                    } else {
                        let _ = fs::remove_file(&tmp_path);
                    }
                }
            }
        }

        let mut file = crate::private_fs::open_private_file(&tmp_path, !resumable)
            .map_err(|error| format!("cannot open {}: {error}", tmp_path.display()))?;
        let received = file.metadata().map_err(|error| error.to_string())?.len();
        if received > meta.size {
            file.set_len(0).map_err(|error| error.to_string())?;
        }
        let received = file.metadata().map_err(|error| error.to_string())?.len();
        let requires_full_hash = received > 0;
        file.seek(SeekFrom::Start(received))
            .map_err(|error| error.to_string())?;
        let writer = BufWriter::with_capacity(FILE_CHUNK_SIZE, file);
        let state = FileReceiveState {
            meta: meta.clone(),
            session_epoch,
            tmp_path,
            final_path,
            state_path,
            writer,
            hasher: blake3::Hasher::new(),
            received,
            requires_full_hash,
        };
        persist_transfer_state(&state, &source).map_err(|error| error.to_string())?;

        info!(
            "File receive ready: {} from {} at {}/{} bytes",
            meta.name, source, received, meta.size
        );
        self.set_receive_progress(&source, &meta, received);
        self.active_receives.insert(key.clone(), state);
        if received == meta.size {
            let state = self
                .active_receives
                .remove(&key)
                .ok_or_else(|| "completed transfer state disappeared".to_string())?;
            let completed = self.finish_file_receive(state, &source).await?;
            return Ok(FileReceiveProgress {
                transfer_id,
                next_offset: received,
                completed,
            });
        }
        Ok(FileReceiveProgress {
            transfer_id,
            next_offset: received,
            completed: None,
        })
    }

    pub async fn handle_resumable_file_chunk(
        &mut self,
        chunk: &FileChunkPayload,
        source: String,
    ) -> Result<FileReceiveProgress, FileReceiveError> {
        self.handle_file_chunk_with_key(chunk, source, ReceiveKey::Resumable(chunk.transfer_id))
            .await
    }

    pub(super) async fn handle_file_chunk_with_key(
        &mut self,
        chunk: &FileChunkPayload,
        source: String,
        receive_key: ReceiveKey,
    ) -> Result<FileReceiveProgress, FileReceiveError> {
        let key = (source.clone(), receive_key);
        if let Some(completed) = self.completed_transfers.get(&key) {
            return Ok(FileReceiveProgress {
                transfer_id: chunk.transfer_id,
                next_offset: completed.size,
                completed: None,
            });
        }
        let state =
            self.active_receives
                .get_mut(&key)
                .ok_or(FileReceiveError::MetadataUnavailable {
                    transfer_id: chunk.transfer_id,
                })?;

        if chunk.offset != state.received {
            return Ok(FileReceiveProgress {
                transfer_id: chunk.transfer_id,
                next_offset: state.received,
                completed: None,
            });
        }
        if state.received.saturating_add(chunk.data.len() as u64) > state.meta.size {
            return Err(FileReceiveError::Failed(format!(
                "file chunk exceeds declared size for {}",
                state.meta.name
            )));
        }
        state
            .writer
            .write_all(&chunk.data)
            .map_err(|error| FileReceiveError::Failed(error.to_string()))?;
        state
            .writer
            .flush()
            .map_err(|error| FileReceiveError::Failed(error.to_string()))?;
        state.hasher.update(&chunk.data);
        state.received += chunk.data.len() as u64;
        persist_transfer_state(state, &source)
            .map_err(|error| FileReceiveError::Failed(error.to_string()))?;
        let next_offset = state.received;
        let completed = state.received == state.meta.size;
        let progress_name = state.meta.name.clone();
        let progress_total = state.meta.size;
        let progress_meta = state.meta.clone();
        if progress_meta.batch.is_some() {
            self.set_receive_progress(&source, &progress_meta, next_offset);
        } else {
            self.set_file_progress(&progress_name, next_offset, progress_total);
        }

        if completed {
            let state = self.active_receives.remove(&key).ok_or_else(|| {
                FileReceiveError::Failed("completed transfer state disappeared".to_string())
            })?;
            let completed = self
                .finish_file_receive(state, &source)
                .await
                .map_err(FileReceiveError::Failed)?;
            return Ok(FileReceiveProgress {
                transfer_id: chunk.transfer_id,
                next_offset,
                completed,
            });
        }
        Ok(FileReceiveProgress {
            transfer_id: chunk.transfer_id,
            next_offset,
            completed: None,
        })
    }

    /// Compatibility path for peers that send unframed v2 file chunks.
    pub async fn handle_file_chunk(&mut self, chunk: &[u8], source: String) {
        let key = (source.clone(), ReceiveKey::Legacy);
        let Some(offset) = self.active_receives.get(&key).map(|state| state.received) else {
            warn!("Legacy file chunk from unknown source: {}", source);
            return;
        };
        let payload = FileChunkPayload {
            transfer_id: TransferId([0; 16]),
            offset,
            data: chunk.to_vec(),
        };
        if let Err(error) = self
            .handle_file_chunk_with_key(&payload, source, ReceiveKey::Legacy)
            .await
        {
            error!("Legacy file chunk failed: {error}");
        }
    }

    pub(super) async fn finish_file_receive(
        &mut self,
        state: FileReceiveState,
        source: &str,
    ) -> Result<Option<PendingReceivedFile>, String> {
        let mut writer = state.writer;
        writer.flush().map_err(|error| error.to_string())?;
        drop(writer);
        let computed = state.hasher.finalize().to_hex().to_string();
        if !state.requires_full_hash && computed != state.meta.hash {
            let _ = fs::remove_file(&state.tmp_path);
            if let Some(path) = state.state_path {
                let _ = fs::remove_file(path);
            }
            self.clear_file_progress(state.meta.batch.map(|batch| batch.batch_id), Some(source));
            return Err(format!(
                "whole-file checksum mismatch for {}",
                state.meta.name
            ));
        }
        fs::rename(&state.tmp_path, &state.final_path).map_err(|error| error.to_string())?;
        if let Some(path) = state.state_path {
            let _ = fs::remove_file(path);
        }
        let pending = PendingReceivedFile {
            transfer_id: state.meta.transfer_id.unwrap_or(TransferId([0; 16])),
            file: ReceivedFile {
                name: state.meta.name.clone(),
                size: state.meta.size,
                hash: if state.requires_full_hash {
                    state.meta.hash.clone()
                } else {
                    computed
                },
                path: state.final_path,
            },
            hash_verified: !state.requires_full_hash,
            meta: state.meta,
        };
        if pending.hash_verified {
            self.commit_received_file(source, pending).await?;
            Ok(None)
        } else {
            info!(
                "File receive complete: {} ({} bytes, awaiting resumed-file verification)",
                pending.meta.name, pending.meta.size
            );
            Ok(Some(pending))
        }
    }

    pub async fn commit_received_file(
        &mut self,
        source: &str,
        pending: PendingReceivedFile,
    ) -> Result<(), String> {
        if !pending.hash_verified {
            return Err("Received file must be hash-verified before commit".to_string());
        }
        info!(
            "File receive complete: {} ({} bytes, hash verified)",
            pending.meta.name, pending.meta.size
        );
        let PendingReceivedFile {
            meta,
            file: received_file,
            ..
        } = pending;
        if let Some(batch_ref) = meta.batch {
            let batch_key = (source.to_string(), batch_ref.batch_id);
            if !self.incoming_batches.contains_key(&batch_key) {
                // A receive task may finish its off-thread hash after the
                // connection guard has suspended in-memory state. Rehydrate
                // the durable manifest so verified bytes remain resumable.
                let incoming_dir = received_file
                    .path
                    .parent()
                    .ok_or_else(|| "Received file path has no parent".to_string())?;
                let manifest_path =
                    incoming_dir.join(format!("{}.batch.json", batch_ref.batch_id.as_hex()));
                let data = fs::read(&manifest_path)
                    .map_err(|error| format!("File batch state disappeared: {error}"))?;
                let saved: PersistedIncomingBatch = serde_json::from_slice(&data)
                    .map_err(|error| format!("Invalid persisted file batch state: {error}"))?;
                if saved.source != source || saved.manifest.batch_id != batch_ref.batch_id {
                    return Err(
                        "Persisted file batch state belongs to another source or batch".to_string(),
                    );
                }
                if saved.files.len() != saved.manifest.files.len() {
                    return Err("Persisted file batch state has an invalid file count".to_string());
                }
                let manifest = saved.manifest;
                let saved_source = saved.source;
                let local_generation = saved.local_generation;
                let source_device_id = if saved.source_device_id.is_empty() {
                    self.peer_device_ids
                        .get(source)
                        .cloned()
                        .unwrap_or_else(|| source.to_string())
                } else {
                    saved.source_device_id
                };
                let mut files = vec![None; manifest.files.len()];
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
                let session_epoch = self.receive_epoch(source);
                self.incoming_batches.insert(
                    batch_key.clone(),
                    IncomingBatch {
                        manifest,
                        source: saved_source,
                        source_device_id,
                        session_epoch,
                        local_generation,
                        files,
                        manifest_path,
                    },
                );
            }
            let batch = self
                .incoming_batches
                .get_mut(&batch_key)
                .ok_or_else(|| "File batch state disappeared before completion".to_string())?;
            let mut files = batch.files.clone();
            let slot = files
                .get_mut(usize::from(batch_ref.index))
                .ok_or_else(|| "File batch index is out of range".to_string())?;
            *slot = Some(received_file);
            let persisted = PersistedIncomingBatch {
                source: batch.source.clone(),
                source_device_id: batch.source_device_id.clone(),
                manifest: batch.manifest.clone(),
                files: files.clone(),
                local_generation: batch.local_generation,
            };
            persist_incoming_batch(&batch.manifest_path, &persisted)
                .map_err(|error| error.to_string())?;
            batch.files = files;
            let completed_files = batch.files.iter().filter(|file| file.is_some()).count();
            let completed_bytes = batch
                .files
                .iter()
                .filter_map(|file| file.as_ref().map(|file| file.size))
                .sum();
            if let Some(platform) = self.platform.as_ref() {
                platform.set_file_batch_progress(FileBatchProgress {
                    batch_id: batch_ref.batch_id.as_hex(),
                    direction: "receiving".to_string(),
                    device: source.to_string(),
                    current_file: meta.name.clone(),
                    completed_files,
                    total_files: batch.manifest.files.len(),
                    transferred_bytes: completed_bytes,
                    total_bytes: batch.manifest.total_bytes,
                });
            }
        } else {
            let platform = self
                .platform
                .clone()
                .ok_or_else(|| "Clipboard platform is unavailable".to_string())?;
            platform
                .files_received(FileReceiveCommit {
                    batch_id: None,
                    files: vec![received_file],
                    batch_total: 1,
                    batch_complete: true,
                    activate_clipboard: true,
                    device: source.to_string(),
                    source_device_id: self
                        .peer_device_ids
                        .get(source)
                        .cloned()
                        .unwrap_or_else(|| source.to_string()),
                    manifest_hash: None,
                })
                .await?;
        }
        self.completed_transfers.insert(
            (source.to_string(), ReceiveKey::from(meta.transfer_id)),
            CompletedTransfer {
                size: meta.size,
                hash: meta.hash,
                completed_at: chrono::Utc::now().timestamp(),
            },
        );
        Ok(())
    }

    pub fn discard_received_file(&self, source: &str, batch_id: Option<TransferId>) {
        self.clear_file_progress(batch_id, Some(source));
    }
}
