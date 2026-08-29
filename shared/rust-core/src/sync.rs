use crate::protocol::{FileChunkPayload, MessageId, TransferId, FILE_CHUNK_SIZE};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::LazyLock;
use std::time::Duration;

mod resume;
pub use resume::cleanup_expired_transfers;
use resume::{
    persist_incoming_batch, persist_transfer_state, restore_persisted_received_file,
    PersistedIncomingBatch, PersistedTransfer,
};
mod shadow;
use shadow::ShadowFilter;
mod prepare;
use prepare::hash_source_file;
pub use prepare::{
    clipboard_files_are_readable, normalize_transferred_file_name, prepare_file_batch,
    revalidate_prepared_file, validate_incoming_file_meta, FileBatchEntry, FileBatchManifest,
    FileBatchRef, FileMeta, PreparedFile, PreparedFileBatch, MAX_FILE_SIZE,
};

const SEEN_MESSAGE_RETENTION_SECONDS: i64 = 10 * 60;
const SEEN_MESSAGE_MAX_ENTRIES: usize = 1024;
pub const INCOMPLETE_TRANSFER_RETENTION_SECONDS: u64 = 24 * 60 * 60;
const CANCELLED_BATCH_RETENTION_SECONDS: i64 = 24 * 60 * 60;
const CANCELLED_BATCH_MAX_ENTRIES: usize = 1024;
pub const MAX_FILE_BATCH_COUNT: usize = 20;
pub const MAX_FILE_BATCH_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_ACTIVE_BATCHES_PER_PEER: usize = 2;
const MAX_ACTIVE_BATCHES_GLOBAL: usize = 8;
const MAX_ACTIVE_RECEIVES_PER_PEER: usize = 2;
const MAX_ACTIVE_RECEIVES_GLOBAL: usize = 8;

static FILE_BATCH_ADMISSION_LOCK: LazyLock<tokio::sync::Mutex<()>> =
    LazyLock::new(|| tokio::sync::Mutex::new(()));

/// Serializes the quota preflight and core batch admission across network
/// connections. Without this seam, two peers could both pass the active-batch
/// check before either one inserts its batch state.
pub fn file_batch_admission_lock() -> &'static tokio::sync::Mutex<()> {
    &FILE_BATCH_ADMISSION_LOCK
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceivedFile {
    pub name: String,
    pub size: u64,
    pub hash: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileBatchProgress {
    pub batch_id: String,
    pub direction: String,
    pub device: String,
    pub current_file: String,
    pub completed_files: usize,
    pub total_files: usize,
    pub transferred_bytes: u64,
    pub total_bytes: u64,
}

pub trait SyncPlatform: Send + Sync {
    fn write_text(&self, text: &str) -> Result<(), String>;
    fn write_image(&self, width: u32, height: u32, rgba: &[u8]) -> Result<(), String>;
    fn set_file_progress(&self, name: &str, received: u64, total: u64);
    fn clear_file_progress(&self, batch_id: Option<TransferId>, device: Option<&str>);
    fn set_file_batch_progress(&self, progress: FileBatchProgress);
    fn files_received(
        &self,
        batch_id: Option<TransferId>,
        files: Vec<ReceivedFile>,
        batch_total: usize,
        batch_complete: bool,
        activate_clipboard: bool,
        device: String,
    );
    fn file_batch_failed(&self, batch_id: Option<TransferId>, message: &str);
}

/// State for an in-progress file receive — streamed to disk.
struct FileReceiveState {
    meta: FileMeta,
    session_epoch: u64,
    /// Temp file path; renamed to final on success.
    tmp_path: PathBuf,
    final_path: PathBuf,
    state_path: Option<PathBuf>,
    writer: BufWriter<File>,
    hasher: blake3::Hasher,
    received: u64,
    requires_full_hash: bool,
}

#[derive(Debug, Clone)]
pub struct PendingReceivedFile {
    transfer_id: TransferId,
    meta: FileMeta,
    file: ReceivedFile,
    hash_verified: bool,
}

impl PendingReceivedFile {
    pub fn transfer_id(&self) -> TransferId {
        self.transfer_id
    }

    pub fn batch_id(&self) -> Option<TransferId> {
        self.meta.batch.map(|batch| batch.batch_id)
    }

    pub fn path(&self) -> &Path {
        &self.file.path
    }

    pub fn verify_hash(mut self) -> Result<Self, String> {
        if self.hash_verified {
            return Ok(self);
        }
        let computed = hash_source_file(&self.file.path).map_err(|error| error.to_string())?;
        if computed != self.meta.hash {
            return Err(format!(
                "whole-file checksum mismatch for {}",
                self.meta.name
            ));
        }
        self.file.hash = computed;
        self.hash_verified = true;
        Ok(self)
    }
}

#[derive(Debug)]
pub struct FileReceiveProgress {
    pub transfer_id: TransferId,
    pub next_offset: u64,
    pub completed: Option<PendingReceivedFile>,
}

#[derive(Debug, Clone)]
struct CompletedTransfer {
    size: u64,
    hash: String,
    completed_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ReceiveKey {
    Resumable(TransferId),
    Legacy,
}

impl From<Option<TransferId>> for ReceiveKey {
    fn from(transfer_id: Option<TransferId>) -> Self {
        transfer_id.map_or(Self::Legacy, Self::Resumable)
    }
}

struct IncomingBatch {
    manifest: FileBatchManifest,
    source: String,
    session_epoch: u64,
    local_generation: u64,
    files: Vec<Option<ReceivedFile>>,
    manifest_path: PathBuf,
}

pub struct SyncEngine {
    /// Reliable event IDs retained across reconnects so ACK loss cannot apply
    /// the same clipboard event twice.
    seen_messages: HashMap<Arc<str>, HashMap<MessageId, i64>>,
    seen_message_order: VecDeque<(Arc<str>, MessageId, i64)>,
    /// Active file receives: authenticated peer + transfer ID → receive state.
    active_receives: HashMap<(String, ReceiveKey), FileReceiveState>,
    completed_transfers: HashMap<(String, ReceiveKey), CompletedTransfer>,
    incoming_batches: HashMap<(String, TransferId), IncomingBatch>,
    cancelled_batches: HashMap<(String, TransferId), i64>,
    completed_batches: HashMap<(String, TransferId), i64>,
    receive_epochs: HashMap<String, u64>,
    clipboard_generation: u64,
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

mod batches;
mod clipboard;
mod engine_state;
mod receive_engine;

/// Verifies a completed inbound file off the async path and commits it
/// (T108 migration). On verification failure the completed file is removed;
/// a commit failure preserves verified bytes so a later reconnect can recover
/// from the durable batch manifest.
pub async fn verify_and_commit_received_file(
    sync_engine: &Arc<tokio::sync::Mutex<SyncEngine>>,
    source: &str,
    pending: PendingReceivedFile,
) -> Result<(), String> {
    let path = pending.path().to_path_buf();
    let batch_id = pending.batch_id();
    let verification = tokio::task::spawn_blocking(move || pending.verify_hash())
        .await
        .map_err(|error| format!("File verification task failed: {error}"))?;
    match verification {
        Ok(verified) => {
            let result = sync_engine
                .lock()
                .await
                .commit_received_file(source, verified);
            result
        }
        Err(error) => {
            let _ = std::fs::remove_file(&path);
            sync_engine
                .lock()
                .await
                .discard_received_file(source, batch_id);
            Err(error)
        }
    }
}

/// RAII guard that suspends all active receives from a peer on drop
/// (T109 migration). Used while a pairing session is being installed so a
/// later disconnect releases receive state asynchronously.
pub struct ReceiveSuspendGuard {
    sync_engine: Arc<tokio::sync::Mutex<SyncEngine>>,
    source: String,
    epoch: u64,
}

impl ReceiveSuspendGuard {
    pub fn new(
        sync_engine: Arc<tokio::sync::Mutex<SyncEngine>>,
        source: String,
        epoch: u64,
    ) -> Self {
        Self {
            sync_engine,
            source,
            epoch,
        }
    }
}

impl Drop for ReceiveSuspendGuard {
    fn drop(&mut self) {
        let sync_engine = self.sync_engine.clone();
        let source = self.source.clone();
        let epoch = self.epoch;
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                sync_engine
                    .lock()
                    .await
                    .suspend_receive_epoch(&source, epoch);
            });
        } else if let Ok(mut sync) = sync_engine.try_lock() {
            sync.suspend_receive_epoch(&source, epoch);
        }
    }
}

#[cfg(test)]
mod tests;
