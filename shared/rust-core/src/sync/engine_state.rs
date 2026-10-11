use super::*;

impl SyncEngine {
    /// Drop a disconnected peer's open receive handles without keeping the
    /// engine mutex while closing files and deleting temporary artifacts.
    pub async fn cancel_receive_shared(
        sync_engine: &Arc<tokio::sync::Mutex<SyncEngine>>,
        source: &str,
    ) {
        let operation_locks = {
            let mut engine = sync_engine.lock().await;
            let mut keys = engine
                .active_receives
                .keys()
                .filter(|(peer, _)| peer == source)
                .cloned()
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
        let mut operation_guards = Vec::with_capacity(operation_locks.len());
        for operation_lock in operation_locks {
            operation_guards.push(operation_lock.lock_owned().await);
        }
        let (states, platform) = {
            let mut engine = sync_engine.lock().await;
            let keys = engine
                .active_receives
                .keys()
                .filter(|(peer, _)| peer == source)
                .cloned()
                .collect::<Vec<_>>();
            let states = keys
                .into_iter()
                .filter_map(|key| engine.active_receives.remove(&key))
                .collect::<Vec<_>>();
            (states, engine.platform.clone())
        };
        for state in states {
            drop(state.writer);
            let _ = fs::remove_file(state.tmp_path);
            if let Some(path) = state.state_path {
                let _ = fs::remove_file(path);
            }
        }
        if let Some(platform) = platform {
            platform.clear_file_progress(None, Some(source));
        }
        drop(operation_guards);
    }

    /// Cancel all active receives from a peer and clean up their temp state.
    pub async fn cancel_receive(&mut self, source: &str) {
        let keys = self
            .active_receives
            .keys()
            .filter(|(peer, _)| peer == source)
            .cloned()
            .collect::<Vec<_>>();
        for key in keys {
            if let Some(state) = self.active_receives.remove(&key) {
                drop(state.writer);
                let _ = fs::remove_file(&state.tmp_path);
                if let Some(path) = state.state_path {
                    let _ = fs::remove_file(path);
                }
            }
        }
        self.deliver_progress_now(SyncEngine::clear_progress_update(None, Some(source)));
    }

    /// Release open handles for a disconnected peer while retaining durable
    /// transfer state so the sender can resume after reconnecting.
    pub fn suspend_receive(&mut self, source: &str) {
        let epoch = self.receive_epoch(source);
        self.suspend_receive_epoch(source, epoch);
    }

    /// Suspend a receive epoch after waiting for any transfer-scoped disk
    /// operation that temporarily removed its state from `active_receives`.
    /// This closes the disconnect race without holding the engine mutex while
    /// a buffered writer is dropped.
    pub async fn suspend_receive_epoch_shared(
        sync_engine: &Arc<tokio::sync::Mutex<SyncEngine>>,
        source: &str,
        epoch: u64,
    ) {
        let (keys, batch_operation_locks, operation_locks) = {
            let mut engine = sync_engine.lock().await;
            let mut batch_keys = engine
                .incoming_batches
                .iter()
                .filter(|((peer, _), batch)| peer == source && batch.session_epoch == epoch)
                .map(|(key, _)| key.clone())
                .collect::<Vec<_>>();
            batch_keys.sort_unstable_by_key(|(peer, batch_id)| (peer.clone(), batch_id.as_hex()));
            batch_keys.dedup();
            let batch_operation_locks = batch_keys
                .iter()
                .map(|key| engine.batch_operation_lock(key))
                .collect::<Vec<_>>();
            let mut keys = engine
                .active_receives
                .iter()
                .filter(|((peer, _), state)| peer == source && state.session_epoch == epoch)
                .map(|(key, _)| key.clone())
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
                    .filter(|((peer, _), pending_epoch)| peer == source && **pending_epoch == epoch)
                    .map(|(key, _)| key.clone()),
            );
            keys.sort_unstable_by_key(|(peer, receive_key)| {
                (peer.clone(), format!("{receive_key:?}"))
            });
            keys.dedup();
            let operation_locks = keys
                .iter()
                .map(|key| engine.receive_operation_lock(key))
                .collect::<Vec<_>>();
            (keys, batch_operation_locks, operation_locks)
        };
        let mut batch_operation_guards = Vec::with_capacity(batch_operation_locks.len());
        for batch_operation_lock in batch_operation_locks {
            batch_operation_guards.push(batch_operation_lock.lock_owned().await);
        }
        let mut operation_guards = Vec::with_capacity(operation_locks.len());
        for operation_lock in operation_locks {
            operation_guards.push(operation_lock.lock_owned().await);
        }
        let (states, should_clear_progress, platform) = {
            let mut engine = sync_engine.lock().await;
            let active_keys = engine
                .active_receives
                .iter()
                .filter(|((peer, _), state)| peer == source && state.session_epoch == epoch)
                .map(|(key, _)| key.clone())
                .collect::<Vec<_>>();
            let states = active_keys
                .into_iter()
                .filter_map(|key| engine.active_receives.remove(&key))
                .collect::<Vec<_>>();
            engine
                .pending_receives
                .retain(|key, pending_epoch| !(keys.contains(key) && *pending_epoch == epoch));
            engine.inflight_receives.retain(|key| !keys.contains(key));
            engine
                .incoming_batches
                .retain(|(peer, _), batch| peer != source || batch.session_epoch != epoch);
            let should_clear_progress = !engine
                .active_receives
                .keys()
                .any(|(peer, _)| peer == source)
                && !engine
                    .incoming_batches
                    .keys()
                    .any(|(peer, _)| peer == source);
            (states, should_clear_progress, engine.platform.clone())
        };
        for mut state in states {
            if let Err(error) = flush_and_persist_transfer_state(&mut state, source) {
                tracing::warn!(
                    source = %source,
                    error = %error,
                    "Could not persist receive state before connection suspension"
                );
            }
            drop(state.writer);
        }
        if should_clear_progress {
            if let Some(platform) = platform {
                platform.clear_file_progress(None, Some(source));
            }
        }
        // Keep the owned guards alive until after the state transition and
        // writer drops so a new FileMeta/chunk cannot reopen a suspended key.
        drop(keys);
        drop(operation_guards);
        drop(batch_operation_guards);
    }

    pub fn start_receive_session(&mut self, source: &str) -> u64 {
        let epoch = self.receive_epochs.entry(source.to_string()).or_insert(0);
        *epoch = epoch.wrapping_add(1).max(1);
        *epoch
    }

    pub(super) fn receive_epoch(&self, source: &str) -> u64 {
        self.receive_epochs.get(source).copied().unwrap_or(0)
    }

    pub fn suspend_receive_epoch(&mut self, source: &str, epoch: u64) {
        let keys = self
            .active_receives
            .iter()
            .filter(|((peer, _), state)| peer == source && state.session_epoch == epoch)
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        for key in keys {
            if let Some(mut state) = self.active_receives.remove(&key) {
                if let Err(error) = flush_and_persist_transfer_state(&mut state, source) {
                    tracing::warn!(
                        source = %source,
                        error = %error,
                        "Could not persist receive state before connection suspension"
                    );
                }
                drop(state.writer);
            }
        }
        self.incoming_batches
            .retain(|(peer, _), batch| peer != source || batch.session_epoch != epoch);
        if !self.active_receives.keys().any(|(peer, _)| peer == source)
            && !self.incoming_batches.keys().any(|(peer, _)| peer == source)
        {
            self.deliver_progress_now(SyncEngine::clear_progress_update(None, Some(source)));
        }
    }

    pub(super) fn prune_cancelled_batches(&mut self) {
        let now = chrono::Utc::now().timestamp();
        self.cancelled_batches.retain(|_, cancelled_at| {
            now.saturating_sub(*cancelled_at) <= CANCELLED_BATCH_RETENTION_SECONDS
        });
        if self.cancelled_batches.len() <= CANCELLED_BATCH_MAX_ENTRIES {
            return;
        }
        let mut oldest = self
            .cancelled_batches
            .iter()
            .map(|(key, at)| (key.clone(), *at))
            .collect::<Vec<_>>();
        oldest.sort_by_key(|(_, at)| *at);
        let remove = oldest.len().saturating_sub(CANCELLED_BATCH_MAX_ENTRIES);
        for (key, _) in oldest.into_iter().take(remove) {
            self.cancelled_batches.remove(&key);
        }
    }

    // ── Shadow filter helpers ────────────────────────────────────

    pub fn has_seen_message(&self, source: &str, message_id: MessageId) -> bool {
        let now = chrono::Utc::now().timestamp();
        self.seen_messages
            .get(source)
            .and_then(|messages| messages.get(&message_id))
            .is_some_and(|seen_at| now.saturating_sub(*seen_at) <= SEEN_MESSAGE_RETENTION_SECONDS)
    }

    pub fn record_message(&mut self, source: &str, message_id: MessageId) {
        let now = chrono::Utc::now().timestamp();
        self.prune_seen_messages(now);
        let source: Arc<str> = if let Some((source, _)) = self.seen_messages.get_key_value(source) {
            source.clone()
        } else {
            let source: Arc<str> = Arc::from(source);
            self.seen_messages.insert(source.clone(), HashMap::new());
            source
        };
        let messages = self
            .seen_messages
            .get_mut(source.as_ref())
            .expect("seen-message source key was just inserted or found");
        if messages.contains_key(&message_id) {
            return;
        }
        messages.insert(message_id, now);
        self.seen_message_order.push_back((source, message_id, now));
        self.prune_seen_messages(now);
    }

    pub(super) fn prune_seen_messages(&mut self, now: i64) {
        while let Some((source, message_id, seen_at)) = self.seen_message_order.front().cloned() {
            let current = self
                .seen_messages
                .get(source.as_ref())
                .and_then(|messages| messages.get(&message_id))
                .copied();
            let expired = now.saturating_sub(seen_at) > SEEN_MESSAGE_RETENTION_SECONDS;
            let over_capacity = self.seen_message_order.len() > SEEN_MESSAGE_MAX_ENTRIES;
            if current != Some(seen_at) || expired || over_capacity {
                self.seen_message_order.pop_front();
                if self
                    .seen_messages
                    .get(source.as_ref())
                    .and_then(|messages| messages.get(&message_id))
                    .copied()
                    == Some(seen_at)
                {
                    let remove_peer =
                        self.seen_messages
                            .get_mut(source.as_ref())
                            .is_some_and(|messages| {
                                messages.remove(&message_id);
                                messages.is_empty()
                            });
                    if remove_peer {
                        self.seen_messages.remove(source.as_ref());
                    }
                }
            } else {
                break;
            }
        }
    }

    /// Record that an inbound event's clipboard effect has been applied while
    /// its history write is still outstanding. A retry of this event must not
    /// re-apply the clipboard effect, because newer clipboard intent (local or
    /// remote) may have superseded it in the meantime.
    pub fn mark_event_applied(&mut self, source: &str, message_id: MessageId) {
        let now = chrono::Utc::now().timestamp();
        self.prune_applied_events(now);
        let source: Arc<str> = if let Some((source, _)) = self.applied_events.get_key_value(source)
        {
            source.clone()
        } else {
            let source: Arc<str> = Arc::from(source);
            self.applied_events.insert(source.clone(), HashMap::new());
            source
        };
        let events = self
            .applied_events
            .get_mut(source.as_ref())
            .expect("applied-event source key was just inserted or found");
        if events.contains_key(&message_id) {
            return;
        }
        events.insert(message_id, now);
        self.applied_event_order
            .push_back((source, message_id, now));
        self.prune_applied_events(now);
    }

    pub fn has_applied_event(&self, source: &str, message_id: MessageId) -> bool {
        let now = chrono::Utc::now().timestamp();
        self.applied_events
            .get(source)
            .and_then(|events| events.get(&message_id))
            .is_some_and(|applied_at| {
                now.saturating_sub(*applied_at) <= APPLIED_EVENT_RETENTION_SECONDS
            })
    }

    pub fn clear_applied_event(&mut self, source: &str, message_id: MessageId) {
        if let Some(events) = self.applied_events.get_mut(source) {
            events.remove(&message_id);
            if events.is_empty() {
                self.applied_events.remove(source);
            }
        }
    }

    pub(super) fn prune_applied_events(&mut self, now: i64) {
        while let Some((source, message_id, applied_at)) = self.applied_event_order.front().cloned()
        {
            let current = self
                .applied_events
                .get(source.as_ref())
                .and_then(|events| events.get(&message_id))
                .copied();
            let expired = now.saturating_sub(applied_at) > APPLIED_EVENT_RETENTION_SECONDS;
            let over_capacity = self.applied_event_order.len() > APPLIED_EVENT_MAX_ENTRIES;
            if current != Some(applied_at) || expired || over_capacity {
                self.applied_event_order.pop_front();
                if self
                    .applied_events
                    .get(source.as_ref())
                    .and_then(|events| events.get(&message_id))
                    .copied()
                    == Some(applied_at)
                {
                    let remove_peer =
                        self.applied_events
                            .get_mut(source.as_ref())
                            .is_some_and(|events| {
                                events.remove(&message_id);
                                events.is_empty()
                            });
                    if remove_peer {
                        self.applied_events.remove(source.as_ref());
                    }
                }
            } else {
                break;
            }
        }
    }

    pub fn add_shadow_filter(&mut self, text: &str) {
        let hash = blake3::hash(text.as_bytes()).to_hex().to_string();
        self.shadow_filter.insert(hash);
    }

    pub fn add_image_shadow_filter(&mut self, data: &[u8]) {
        let hash = blake3::hash(data).to_hex().to_string();
        self.image_shadow_filter.insert(hash);
    }

    pub fn contains_shadow_filter(&mut self, hash: &str) -> bool {
        self.shadow_filter.contains(hash)
    }

    pub fn contains_image_shadow_filter(&mut self, hash: &str) -> bool {
        self.image_shadow_filter.contains(hash)
    }

    /// Prefer a platform-native write receipt, falling back to the existing
    /// conservative shadow filter when the host has no sequence/change-count
    /// integration. A matched receipt removes the fallback entry so a later
    /// user copy of identical content is not swallowed by a long TTL.
    pub fn consume_text_echo(&mut self, hash: &str) -> bool {
        let receipt = self
            .platform
            .as_ref()
            .is_some_and(|platform| platform.consume_text_write_receipt(hash));
        if receipt {
            self.shadow_filter.remove(hash);
            true
        } else {
            self.shadow_filter.contains(hash)
        }
    }

    pub fn consume_image_echo(&mut self, hash: &str) -> bool {
        let receipt = self
            .platform
            .as_ref()
            .is_some_and(|platform| platform.consume_image_write_receipt(hash));
        if receipt {
            self.image_shadow_filter.remove(hash);
            true
        } else {
            self.image_shadow_filter.contains(hash)
        }
    }

    pub fn remove_shadow_filter(&mut self, text: &str) {
        let hash = blake3::hash(text.as_bytes()).to_hex().to_string();
        self.shadow_filter.remove(&hash);
    }

    pub fn remove_image_shadow_filter(&mut self, data: &[u8]) {
        let hash = blake3::hash(data).to_hex().to_string();
        self.image_shadow_filter.remove(&hash);
    }

    // ── Internal ─────────────────────────────────────────────────

    pub(super) fn platform(&self) -> Result<&dyn SyncPlatform, String> {
        self.platform
            .as_deref()
            .ok_or_else(|| "Clipboard platform is unavailable".to_string())
    }

    /// Snapshot a single-file progress callback. The platform store is only
    /// touched when the snapshot is delivered outside the engine lock.
    pub(super) fn file_progress_update(name: &str, received: u64, total: u64) -> ProgressUpdate {
        ProgressUpdate::File {
            name: name.to_string(),
            received,
            total,
        }
    }

    /// Snapshot the progress callback for an in-flight receive.
    ///
    /// Batch aggregates are read from the incoming batch state, so this must run
    /// under the engine lock; the returned payload is delivered after the lock
    /// is released. `None` means the batch state disappeared and there is
    /// nothing to report.
    pub(super) fn receive_progress_update(
        &self,
        source: &str,
        meta: &FileMeta,
        received: u64,
    ) -> Option<ProgressUpdate> {
        let Some(batch_ref) = meta.batch else {
            return Some(Self::file_progress_update(&meta.name, received, meta.size));
        };
        let batch = self
            .incoming_batches
            .get(&(source.to_string(), batch_ref.batch_id))?;
        let completed_files = batch.files.iter().filter(|file| file.is_some()).count();
        let completed_bytes = batch
            .files
            .iter()
            .filter_map(|file| file.as_ref().map(|file| file.size))
            .sum::<u64>();
        Some(ProgressUpdate::Batch(FileBatchProgress {
            batch_id: batch_ref.batch_id.as_hex(),
            direction: "receiving".to_string(),
            device: source.to_string(),
            current_file: meta.name.clone(),
            completed_files,
            total_files: batch.manifest.files.len(),
            transferred_bytes: completed_bytes
                .saturating_add(received)
                .min(batch.manifest.total_bytes),
            total_bytes: batch.manifest.total_bytes,
        }))
    }

    /// Snapshot a progress-clear callback.
    pub(super) fn clear_progress_update(
        batch_id: Option<TransferId>,
        device: Option<&str>,
    ) -> ProgressUpdate {
        ProgressUpdate::Clear {
            batch_id,
            device: device.map(str::to_string),
        }
    }

    /// Pair a snapshot with the current platform handle so it can be delivered
    /// after the engine lock is released.
    pub(super) fn progress_dispatch(
        &self,
        update: Option<ProgressUpdate>,
    ) -> Option<ProgressDispatch> {
        update.map(|update| ProgressDispatch {
            platform: self.platform.clone(),
            update,
        })
    }

    /// Deliver a snapshot immediately. Only valid on `&mut self` compatibility
    /// paths, which never hold the engine lock while calling the platform.
    pub(super) fn deliver_progress_now(&self, update: ProgressUpdate) {
        if let Some(dispatch) = self.progress_dispatch(Some(update)) {
            dispatch.deliver();
        }
    }
}

/// A platform progress callback captured while the engine lock is held.
///
/// The platform store can block and broadcasts a UI revision, so the engine
/// lock must never be held while it runs. Receive paths snapshot the payload
/// under the lock and deliver it afterwards through [`ProgressDispatch`].
pub(super) enum ProgressUpdate {
    File {
        name: String,
        received: u64,
        total: u64,
    },
    Batch(FileBatchProgress),
    Clear {
        batch_id: Option<TransferId>,
        device: Option<String>,
    },
}

/// A progress snapshot plus the platform handle that must deliver it once the
/// engine lock has been released.
pub(super) struct ProgressDispatch {
    platform: Option<Arc<dyn SyncPlatform>>,
    update: ProgressUpdate,
}

impl ProgressDispatch {
    /// Invoke the platform progress callback. Callers must hold no engine lock.
    pub(super) fn deliver(self) {
        let Some(platform) = self.platform else {
            return;
        };
        match self.update {
            ProgressUpdate::File {
                name,
                received,
                total,
            } => platform.set_file_progress(&name, received, total),
            ProgressUpdate::Batch(progress) => platform.set_file_batch_progress(progress),
            ProgressUpdate::Clear { batch_id, device } => {
                platform.clear_file_progress(batch_id, device.as_deref())
            }
        }
    }
}
