use crate::protocol::{FileChunkPayload, MessageId, TransferId, FILE_CHUNK_SIZE};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

const SEEN_MESSAGE_RETENTION_SECONDS: i64 = 10 * 60;
const INCOMPLETE_TRANSFER_RETENTION_SECONDS: u64 = 24 * 60 * 60;
const MAX_ACTIVE_RECEIVES_PER_PEER: usize = 2;
const MAX_ACTIVE_RECEIVES_GLOBAL: usize = 8;
const SHADOW_FILTER_TTL: Duration = Duration::from_secs(30);
const SHADOW_FILTER_MAX_ENTRIES: usize = 1024;

struct ShadowEntry {
    remaining: u16,
    expires_at: Instant,
}

struct ShadowFilter {
    entries: HashMap<String, ShadowEntry>,
}

impl ShadowFilter {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    fn insert(&mut self, hash: String) {
        let now = Instant::now();
        self.prune(now);
        if let Some(entry) = self.entries.get_mut(&hash) {
            entry.remaining = entry.remaining.saturating_add(1);
            entry.expires_at = now + SHADOW_FILTER_TTL;
            return;
        }
        if self.entries.len() >= SHADOW_FILTER_MAX_ENTRIES {
            if let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.expires_at)
                .map(|(hash, _)| hash.clone())
            {
                self.entries.remove(&oldest);
            }
        }
        self.entries.insert(
            hash,
            ShadowEntry {
                remaining: 1,
                expires_at: now + SHADOW_FILTER_TTL,
            },
        );
    }

    fn consume(&mut self, hash: &str) -> bool {
        self.prune(Instant::now());
        let should_remove = match self.entries.get_mut(hash) {
            Some(entry) => {
                entry.remaining -= 1;
                entry.remaining == 0
            }
            None => return false,
        };
        if should_remove {
            self.entries.remove(hash);
        }
        true
    }

    fn prune(&mut self, now: Instant) {
        self.entries.retain(|_, entry| entry.expires_at > now);
    }
}

fn default_file_chunk_size() -> u32 {
    FILE_CHUNK_SIZE as u32
}

pub fn normalize_transferred_file_name(name: &str, data_hash: &str) -> String {
    let full_prefix = format!("{data_hash}-");
    let legacy_prefix = data_hash.get(..12).map(|hash| format!("{hash}_"));
    let mut normalized = name;
    loop {
        if let Some(stripped) = normalized.strip_prefix(&full_prefix) {
            normalized = stripped;
            continue;
        }
        if let Some(stripped) = legacy_prefix
            .as_deref()
            .and_then(|prefix| normalized.strip_prefix(prefix))
        {
            normalized = stripped;
            continue;
        }
        break;
    }
    if normalized.is_empty() {
        name.to_string()
    } else {
        normalized.to_string()
    }
}

/// Metadata for incoming file transfers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMeta {
    #[serde(default)]
    pub transfer_id: Option<TransferId>,
    pub name: String,
    pub size: u64,
    pub hash: String,
    #[serde(default = "default_file_chunk_size")]
    pub chunk_size: u32,
}

#[derive(Debug, Clone)]
pub struct ReceivedFile {
    pub name: String,
    pub size: u64,
    pub hash: String,
    pub path: PathBuf,
}

pub trait SyncPlatform: Send + Sync {
    fn write_text(&self, text: &str) -> Result<(), String>;
    fn write_image(&self, width: u32, height: u32, rgba: &[u8]) -> Result<(), String>;
    fn set_file_progress(&self, name: &str, received: u64, total: u64);
    fn clear_file_progress(&self);
    fn file_received(&self, file: ReceivedFile);
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedTransfer {
    meta: FileMeta,
    source: String,
    final_path: PathBuf,
    updated_at: i64,
}

/// State for an in-progress file receive — streamed to disk.
struct FileReceiveState {
    meta: FileMeta,
    /// Temp file path; renamed to final on success.
    tmp_path: PathBuf,
    final_path: PathBuf,
    state_path: Option<PathBuf>,
    writer: BufWriter<File>,
    hasher: blake3::Hasher,
    received: u64,
}

pub struct SyncEngine {
    /// Reliable event IDs retained across reconnects so ACK loss cannot apply
    /// the same clipboard event twice.
    seen_messages: HashMap<(String, MessageId), i64>,
    /// Active file receives: authenticated peer + transfer ID → receive state.
    active_receives: HashMap<(String, TransferId), FileReceiveState>,
    completed_transfers: HashMap<(String, TransferId), (u64, i64)>,
    /// Shadow-packet filter for text (echo suppression)
    shadow_filter: ShadowFilter,
    /// Shadow-packet filter for images
    image_shadow_filter: ShadowFilter,
    platform: Option<Arc<dyn SyncPlatform>>,
}

impl Default for SyncEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl SyncEngine {
    pub fn new() -> Self {
        SyncEngine {
            seen_messages: HashMap::new(),
            active_receives: HashMap::new(),
            completed_transfers: HashMap::new(),
            shadow_filter: ShadowFilter::new(),
            image_shadow_filter: ShadowFilter::new(),
            platform: None,
        }
    }

    pub fn set_platform(&mut self, platform: Arc<dyn SyncPlatform>) {
        self.platform = Some(platform);
    }

    // ── Text ─────────────────────────────────────────────────────

    /// Handle incoming text from a remote peer.
    ///
    /// Shadow-filter → write system clipboard.
    pub async fn handle_incoming_text(&mut self, text: &str, source: String) -> Result<(), String> {
        self.restore_text(text)?;
        info!(
            "Clipboard ← text from peer {} ({} chars)",
            source,
            text.len()
        );
        Ok(())
    }

    pub fn restore_text(&mut self, text: &str) -> Result<(), String> {
        let hash = blake3::hash(text.as_bytes()).to_hex().to_string();
        self.shadow_filter.insert(hash.clone());
        if let Err(error) = self.platform()?.write_text(text) {
            self.shadow_filter.consume(&hash);
            return Err(error);
        }
        Ok(())
    }

    // ── Images ───────────────────────────────────────────────────

    /// Handle incoming image from a remote peer.
    ///
    /// The packed format from clipboard.rs is [width:4 LE][height:4 LE][rgba].
    /// We reconstruct a `tauri::image::Image` and write it to the clipboard.
    pub async fn handle_incoming_image(
        &mut self,
        image_data: &[u8],
        source: String,
    ) -> Result<(), String> {
        let (width, height) = self.restore_image(image_data)?;
        info!(
            "Clipboard ← image from peer {} ({}×{} {} bytes)",
            source,
            width,
            height,
            image_data.len()
        );
        Ok(())
    }

    pub fn restore_image(&mut self, image_data: &[u8]) -> Result<(u32, u32), String> {
        let image = crate::protocol::PackedImage::try_from(image_data)
            .map_err(|error| format!("Invalid packed image: {error}"))?;
        let hash = blake3::hash(image_data).to_hex().to_string();
        self.image_shadow_filter.insert(hash.clone());
        if let Err(error) = self
            .platform()?
            .write_image(image.width, image.height, image.rgba)
        {
            self.image_shadow_filter.consume(&hash);
            return Err(error);
        }
        Ok((image.width, image.height))
    }

    // ── Files ────────────────────────────────────────────────────

    /// Open a new transfer or restore its durable `.part` + sidecar state.
    pub async fn begin_file_receive(
        &mut self,
        meta: FileMeta,
        file_path: &Path,
        source: String,
    ) -> Result<(TransferId, u64), String> {
        let transfer_id = meta.transfer_id.unwrap_or(TransferId([0; 16]));
        let key = (source.clone(), transfer_id);
        let now = chrono::Utc::now().timestamp();
        self.completed_transfers
            .retain(|_, (_, at)| now.saturating_sub(*at) <= SEEN_MESSAGE_RETENTION_SECONDS);
        if let Some((size, _)) = self.completed_transfers.get(&key) {
            return Ok((transfer_id, *size));
        }
        if let Some(state) = self.active_receives.get(&key) {
            if state.meta.hash == meta.hash && state.meta.size == meta.size {
                return Ok((transfer_id, state.received));
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
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
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
                        final_path = saved.final_path;
                    } else {
                        let _ = fs::remove_file(&tmp_path);
                    }
                }
            }
        }

        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(!resumable)
            .open(&tmp_path)
            .map_err(|error| format!("cannot open {}: {error}", tmp_path.display()))?;
        let received = file.metadata().map_err(|error| error.to_string())?.len();
        if received > meta.size {
            file.set_len(0).map_err(|error| error.to_string())?;
        }
        let received = file.metadata().map_err(|error| error.to_string())?.len();
        let mut hasher = blake3::Hasher::new();
        if received > 0 {
            file.seek(SeekFrom::Start(0))
                .map_err(|error| error.to_string())?;
            let mut reader = BufReader::new(&file);
            let mut buffer = vec![0u8; 64 * 1024];
            loop {
                let count = reader
                    .read(&mut buffer)
                    .map_err(|error| error.to_string())?;
                if count == 0 {
                    break;
                }
                hasher.update(&buffer[..count]);
            }
        }
        file.seek(SeekFrom::Start(received))
            .map_err(|error| error.to_string())?;
        let writer = BufWriter::with_capacity(FILE_CHUNK_SIZE, file);
        let state = FileReceiveState {
            meta: meta.clone(),
            tmp_path,
            final_path,
            state_path,
            writer,
            hasher,
            received,
        };
        persist_transfer_state(&state, &source)?;

        info!(
            "File receive ready: {} from {} at {}/{} bytes",
            meta.name, source, received, meta.size
        );
        self.set_file_progress(&meta.name, received, meta.size);
        self.active_receives.insert(key.clone(), state);
        if received == meta.size {
            let state = self
                .active_receives
                .remove(&key)
                .ok_or_else(|| "completed transfer state disappeared".to_string())?;
            self.finish_file_receive(state).await?;
            self.completed_transfers
                .insert(key, (received, chrono::Utc::now().timestamp()));
        }
        Ok((transfer_id, received))
    }

    pub async fn handle_resumable_file_chunk(
        &mut self,
        chunk: &FileChunkPayload,
        source: String,
    ) -> Result<u64, String> {
        let key = (source.clone(), chunk.transfer_id);
        if let Some((size, _)) = self.completed_transfers.get(&key) {
            return Ok(*size);
        }
        let state = self
            .active_receives
            .get_mut(&key)
            .ok_or_else(|| "file transfer metadata is not available".to_string())?;

        if chunk.offset != state.received {
            return Ok(state.received);
        }
        if state.received.saturating_add(chunk.data.len() as u64) > state.meta.size {
            return Err(format!(
                "file chunk exceeds declared size for {}",
                state.meta.name
            ));
        }
        state
            .writer
            .write_all(&chunk.data)
            .map_err(|error| error.to_string())?;
        state.writer.flush().map_err(|error| error.to_string())?;
        state.hasher.update(&chunk.data);
        state.received += chunk.data.len() as u64;
        persist_transfer_state(state, &source)?;
        let next_offset = state.received;
        let completed = state.received == state.meta.size;
        let progress_name = state.meta.name.clone();
        let progress_total = state.meta.size;
        self.set_file_progress(&progress_name, next_offset, progress_total);

        if completed {
            let state = self
                .active_receives
                .remove(&key)
                .ok_or_else(|| "completed transfer state disappeared".to_string())?;
            self.finish_file_receive(state).await?;
            self.completed_transfers
                .insert(key, (next_offset, chrono::Utc::now().timestamp()));
        }
        Ok(next_offset)
    }

    /// Compatibility path for peers that send unframed v2 file chunks.
    pub async fn handle_file_chunk(&mut self, chunk: &[u8], source: String) {
        let key = (source.clone(), TransferId([0; 16]));
        let Some(offset) = self.active_receives.get(&key).map(|state| state.received) else {
            warn!("Legacy file chunk from unknown source: {}", source);
            return;
        };
        let payload = FileChunkPayload {
            transfer_id: TransferId([0; 16]),
            offset,
            data: chunk.to_vec(),
        };
        if let Err(error) = self.handle_resumable_file_chunk(&payload, source).await {
            error!("Legacy file chunk failed: {error}");
        }
    }

    async fn finish_file_receive(&mut self, state: FileReceiveState) -> Result<(), String> {
        let mut writer = state.writer;
        writer.flush().map_err(|error| error.to_string())?;
        drop(writer);
        let computed = state.hasher.finalize().to_hex().to_string();
        if computed != state.meta.hash {
            let _ = fs::remove_file(&state.tmp_path);
            if let Some(path) = state.state_path {
                let _ = fs::remove_file(path);
            }
            self.clear_file_progress();
            return Err(format!(
                "whole-file checksum mismatch for {}",
                state.meta.name
            ));
        }
        fs::rename(&state.tmp_path, &state.final_path).map_err(|error| error.to_string())?;
        if let Some(path) = state.state_path {
            let _ = fs::remove_file(path);
        }
        info!(
            "File receive complete: {} ({} bytes, hash verified)",
            state.meta.name, state.meta.size
        );
        self.clear_file_progress();
        if let Some(platform) = self.platform.as_ref() {
            platform.file_received(ReceivedFile {
                name: state.meta.name,
                size: state.meta.size,
                hash: computed,
                path: state.final_path,
            });
        }
        Ok(())
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
        self.clear_file_progress();
    }

    // ── Shadow filter helpers ────────────────────────────────────

    pub fn has_seen_message(&mut self, source: &str, message_id: MessageId) -> bool {
        let now = chrono::Utc::now().timestamp();
        self.seen_messages
            .retain(|_, seen_at| now.saturating_sub(*seen_at) <= SEEN_MESSAGE_RETENTION_SECONDS);
        self.seen_messages
            .contains_key(&(source.to_string(), message_id))
    }

    pub fn record_message(&mut self, source: &str, message_id: MessageId) {
        self.seen_messages.insert(
            (source.to_string(), message_id),
            chrono::Utc::now().timestamp(),
        );
    }

    pub fn add_shadow_filter(&mut self, text: &str) {
        let hash = blake3::hash(text.as_bytes()).to_hex().to_string();
        self.shadow_filter.insert(hash);
    }

    pub fn add_image_shadow_filter(&mut self, data: &[u8]) {
        let hash = blake3::hash(data).to_hex().to_string();
        self.image_shadow_filter.insert(hash);
    }

    pub fn consume_shadow_filter(&mut self, hash: &str) -> bool {
        self.shadow_filter.consume(hash)
    }

    pub fn consume_image_shadow_filter(&mut self, hash: &str) -> bool {
        self.image_shadow_filter.consume(hash)
    }

    // ── Internal ─────────────────────────────────────────────────

    fn platform(&self) -> Result<&dyn SyncPlatform, String> {
        self.platform
            .as_deref()
            .ok_or_else(|| "Clipboard platform is unavailable".to_string())
    }

    fn set_file_progress(&self, name: &str, received: u64, total: u64) {
        if let Some(platform) = self.platform.as_ref() {
            platform.set_file_progress(name, received, total);
        }
    }

    fn clear_file_progress(&self) {
        if let Some(platform) = self.platform.as_ref() {
            platform.clear_file_progress();
        }
    }
}

fn persist_transfer_state(state: &FileReceiveState, source: &str) -> Result<(), String> {
    let Some(path) = state.state_path.as_ref() else {
        return Ok(());
    };
    let saved = PersistedTransfer {
        meta: state.meta.clone(),
        source: source.to_string(),
        final_path: state.final_path.clone(),
        updated_at: chrono::Utc::now().timestamp(),
    };
    let temp = path.with_extension("json.tmp");
    fs::write(
        &temp,
        serde_json::to_vec_pretty(&saved).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    fs::rename(&temp, path).map_err(|error| error.to_string())
}

pub fn cleanup_expired_transfers() {
    let incoming = crate::db::get_incoming_dir();
    let Ok(entries) = fs::read_dir(&incoming) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_partial = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".part") || name.ends_with(".resume.json"));
        if !is_partial {
            continue;
        }
        let expired = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age.as_secs() > INCOMPLETE_TRANSFER_RETENTION_SECONDS);
        if expired {
            let _ = fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_transferred_file_name, FileMeta, ShadowFilter, SyncEngine,
        SHADOW_FILTER_MAX_ENTRIES,
    };
    use crate::protocol::{FileChunkPayload, MessageId, TransferId, FILE_CHUNK_SIZE};

    #[test]
    fn reliable_message_dedup_is_scoped_to_the_peer() {
        let mut sync = SyncEngine::new();
        let message_id = MessageId([9; 16]);

        assert!(!sync.has_seen_message("peer-a", message_id));
        sync.record_message("peer-a", message_id);
        assert!(sync.has_seen_message("peer-a", message_id));
        assert!(!sync.has_seen_message("peer-b", message_id));
    }

    #[test]
    fn shadow_filter_counts_duplicates_and_stays_bounded() {
        let mut filter = ShadowFilter::new();
        filter.insert("same".into());
        filter.insert("same".into());
        assert!(filter.consume("same"));
        assert!(filter.consume("same"));
        assert!(!filter.consume("same"));

        for index in 0..(SHADOW_FILTER_MAX_ENTRIES + 20) {
            filter.insert(format!("hash-{index}"));
        }
        assert_eq!(filter.entries.len(), SHADOW_FILTER_MAX_ENTRIES);
    }

    #[tokio::test]
    async fn file_receive_limit_is_enforced_per_peer() {
        let directory = std::env::temp_dir().join(format!(
            "tailsync-receive-limit-{:016x}",
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let mut sync = SyncEngine::new();
        for index in 0..2_u8 {
            let meta = FileMeta {
                transfer_id: Some(TransferId([index + 1; 16])),
                name: format!("{index}.bin"),
                size: 1,
                hash: "unused".into(),
                chunk_size: FILE_CHUNK_SIZE as u32,
            };
            sync.begin_file_receive(meta, &directory.join(format!("{index}.bin")), "peer".into())
                .await
                .unwrap();
        }
        let third = FileMeta {
            transfer_id: Some(TransferId([3; 16])),
            name: "third.bin".into(),
            size: 1,
            hash: "unused".into(),
            chunk_size: FILE_CHUNK_SIZE as u32,
        };
        assert!(sync
            .begin_file_receive(third, &directory.join("third.bin"), "peer".into())
            .await
            .unwrap_err()
            .contains("active file receives"));
        sync.cancel_receive("peer").await;
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn transferred_file_name_removes_current_and_legacy_storage_prefixes() {
        let hash = "0b3a82ef490260f43fc7bf96477ca20ca44bd60eabd9ad94ae3114e7d8a974b1";
        assert_eq!(
            normalize_transferred_file_name(
                "0b3a82ef490260f43fc7bf96477ca20ca44bd60eabd9ad94ae3114e7d8a974b1-report.pdf",
                hash,
            ),
            "report.pdf"
        );
        assert_eq!(
            normalize_transferred_file_name("0b3a82ef4902_0b3a82ef4902_report.pdf", hash),
            "report.pdf"
        );
        assert_eq!(
            normalize_transferred_file_name("quarterly-report.pdf", hash),
            "quarterly-report.pdf"
        );
    }

    #[tokio::test]
    async fn file_receive_restores_the_confirmed_offset_from_disk() {
        let directory = std::env::temp_dir().join(format!(
            "tailsync-resume-test-{:016x}",
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let final_path = directory.join("received.bin");
        let transfer_id = TransferId([6; 16]);
        let full_data = b"first-second";
        let meta = FileMeta {
            transfer_id: Some(transfer_id),
            name: "received.bin".into(),
            size: full_data.len() as u64,
            hash: blake3::hash(full_data).to_hex().to_string(),
            chunk_size: FILE_CHUNK_SIZE as u32,
        };
        let first = FileChunkPayload {
            transfer_id,
            offset: 0,
            data: b"first-".to_vec(),
        };

        let mut initial = SyncEngine::new();
        assert_eq!(
            initial
                .begin_file_receive(meta.clone(), &final_path, "peer".into())
                .await
                .unwrap()
                .1,
            0
        );
        assert_eq!(
            initial
                .handle_resumable_file_chunk(&first, "peer".into())
                .await
                .unwrap(),
            6
        );
        assert_eq!(
            initial
                .handle_resumable_file_chunk(&first, "peer".into())
                .await
                .unwrap(),
            6
        );
        drop(initial);

        let mut restored = SyncEngine::new();
        assert_eq!(
            restored
                .begin_file_receive(meta, &directory.join("different-name.bin"), "peer".into())
                .await
                .unwrap()
                .1,
            6
        );
        drop(restored);
        assert_eq!(
            std::fs::metadata(directory.join(format!("{}.part", transfer_id.as_hex())))
                .unwrap()
                .len(),
            6
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
}
