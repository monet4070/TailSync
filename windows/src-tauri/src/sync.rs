use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Manager};

use crate::protocol::{FileChunkPayload, MessageId, TransferId, FILE_CHUNK_SIZE};

const SEEN_MESSAGE_RETENTION_SECONDS: i64 = 10 * 60;
const INCOMPLETE_TRANSFER_RETENTION_SECONDS: u64 = 24 * 60 * 60;

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
    pub(crate) shadow_filter: Vec<String>,
    /// Shadow-packet filter for images
    pub(crate) image_shadow_filter: Vec<String>,
    /// Tauri handle for clipboard access
    pub app_handle: Option<AppHandle>,
    /// DB handle for saving received files
    pub db: Option<Arc<tokio::sync::Mutex<crate::db::HistoryDB>>>,
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
            shadow_filter: Vec::new(),
            image_shadow_filter: Vec::new(),
            app_handle: None,
            db: None,
        }
    }

    // ── Text ─────────────────────────────────────────────────────

    /// Handle incoming text from a remote peer.
    ///
    /// Shadow-filter → write system clipboard.
    pub async fn handle_incoming_text(&mut self, text: &str, source: String) -> Result<(), String> {
        let hash = blake3::hash(text.as_bytes()).to_hex().to_string();

        // Shadow BEFORE writing clipboard so the monitor won't re-broadcast
        self.shadow_filter.push(hash.clone());

        let result = self.with_clipboard(|cb| {
            cb.write_text(text.to_string())
                .map_err(|error| format!("write_text failed: {error}"))
        });
        if let Err(error) = result {
            self.shadow_filter.retain(|entry| entry != &hash);
            return Err(error);
        }
        info!(
            "Clipboard ← text from peer {} ({} chars)",
            source,
            text.len()
        );
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
        if image_data.len() < 8 {
            return Err(format!("Image data too short from {source}"));
        }
        let hash = blake3::hash(image_data).to_hex().to_string();

        // Shadow BEFORE writing clipboard
        self.image_shadow_filter.push(hash.clone());

        // Reconstruct Image from packed format
        let w = u32::from_le_bytes(image_data[0..4].try_into().unwrap());
        let h = u32::from_le_bytes(image_data[4..8].try_into().unwrap());
        let rgba = &image_data[8..];

        let img = tauri::image::Image::new(rgba, w, h);
        let result = self.with_clipboard(|cb| {
            cb.write_image(&img)
                .map_err(|error| format!("write_image failed: {error}"))
        });
        if let Err(error) = result {
            self.image_shadow_filter.retain(|entry| entry != &hash);
            return Err(error);
        }
        info!(
            "Clipboard ← image from peer {} ({}×{} {} bytes)",
            source,
            w,
            h,
            image_data.len()
        );
        Ok(())
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
        crate::api::set_file_progress(&meta.name, received, meta.size);
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
        crate::api::set_file_progress(&state.meta.name, state.received, state.meta.size);

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
            crate::api::clear_file_progress();
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
        crate::api::clear_file_progress();

        let db_opt = self.db.clone();
        let fname = state.meta.name.clone();
        let size = state.meta.size;
        let received_path = state.final_path.clone();
        tokio::spawn(async move {
            let mut stored_path = None;
            if let Some(ref db_arc) = db_opt {
                let db_arc = db_arc.clone();
                let db_fname = fname.clone();
                let db_path = received_path.clone();
                let db_hash = computed.clone();
                match tokio::task::spawn_blocking(move || {
                    db_arc
                        .blocking_lock()
                        .adopt_file(&db_fname, &db_path, &db_hash, size, "peer")
                        .map_err(|error| error.to_string())
                })
                .await
                {
                    Ok(Ok(path)) => stored_path = Some(path),
                    Ok(Err(error)) => error!("DB save file failed: {error}"),
                    Err(error) => error!("DB file task failed: {error}"),
                }
            }
            crate::api::bump_clipboard_version();
            crate::api::restore_file_path_to_clipboard(
                stored_path.as_deref().unwrap_or(&received_path),
                &fname,
            );
        });
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
        crate::api::clear_file_progress();
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
        self.shadow_filter.push(hash);
    }

    pub fn add_image_shadow_filter(&mut self, data: &[u8]) {
        let hash = blake3::hash(data).to_hex().to_string();
        self.image_shadow_filter.push(hash);
    }

    // ── Internal ─────────────────────────────────────────────────

    /// Run a closure with the clipboard plugin state.
    fn with_clipboard<T>(
        &self,
        f: impl FnOnce(&tauri_plugin_clipboard_manager::Clipboard<tauri::Wry>) -> Result<T, String>,
    ) -> Result<T, String> {
        let handle = self
            .app_handle
            .as_ref()
            .ok_or_else(|| "Clipboard app handle is unavailable".to_string())?;
        let state = handle
            .try_state::<tauri_plugin_clipboard_manager::Clipboard<tauri::Wry>>()
            .ok_or_else(|| "Clipboard plugin state is unavailable".to_string())?;
        f(&state)
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
    use super::{normalize_transferred_file_name, FileMeta, SyncEngine};
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
