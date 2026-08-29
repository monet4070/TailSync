use super::*;

impl SyncEngine {
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
        self.clear_file_progress(None, Some(source));
    }

    /// Release open handles for a disconnected peer while retaining durable
    /// transfer state so the sender can resume after reconnecting.
    pub fn suspend_receive(&mut self, source: &str) {
        let epoch = self.receive_epoch(source);
        self.suspend_receive_epoch(source, epoch);
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
            if let Some(state) = self.active_receives.remove(&key) {
                drop(state.writer);
            }
        }
        self.incoming_batches
            .retain(|(peer, _), batch| peer != source || batch.session_epoch != epoch);
        if !self.active_receives.keys().any(|(peer, _)| peer == source)
            && !self.incoming_batches.keys().any(|(peer, _)| peer == source)
        {
            self.clear_file_progress(None, Some(source));
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

    pub(super) fn set_file_progress(&self, name: &str, received: u64, total: u64) {
        if let Some(platform) = self.platform.as_ref() {
            platform.set_file_progress(name, received, total);
        }
    }

    pub(super) fn set_receive_progress(&self, source: &str, meta: &FileMeta, received: u64) {
        let Some(batch_ref) = meta.batch else {
            self.set_file_progress(&meta.name, received, meta.size);
            return;
        };
        let Some(batch) = self
            .incoming_batches
            .get(&(source.to_string(), batch_ref.batch_id))
        else {
            return;
        };
        let completed_files = batch.files.iter().filter(|file| file.is_some()).count();
        let completed_bytes = batch
            .files
            .iter()
            .filter_map(|file| file.as_ref().map(|file| file.size))
            .sum::<u64>();
        if let Some(platform) = self.platform.as_ref() {
            platform.set_file_batch_progress(FileBatchProgress {
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
            });
        }
    }

    pub(super) fn clear_file_progress(&self, batch_id: Option<TransferId>, device: Option<&str>) {
        if let Some(platform) = self.platform.as_ref() {
            platform.clear_file_progress(batch_id, device);
        }
    }
}
