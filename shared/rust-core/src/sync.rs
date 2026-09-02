use crate::protocol::{FileChunkPayload, MessageId, TransferId, FILE_CHUNK_SIZE};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::{self, File};
use std::future::Future;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::LazyLock;
use std::time::{Duration, Instant};
use thiserror::Error;

mod resume;
pub use resume::cleanup_expired_transfers;
use resume::{
    persist_incoming_batch, persist_transfer_state, persisted_transfer_offset,
    restore_persisted_received_file, PersistedIncomingBatch, PersistedTransfer,
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
mod outgoing;
pub use outgoing::{
    enroll_outgoing_batch_peers, load_outgoing_batches, load_outgoing_selections,
    mark_outgoing_history_saved, mark_outgoing_peer_completed,
    mark_outgoing_peer_completed_with_identity, outgoing_retry_due, persist_outgoing_batch,
    persist_outgoing_batch_for_selection, persist_outgoing_batch_for_selection_with_identities,
    persist_outgoing_batch_with_identities, persist_outgoing_selection, remove_outgoing_batch,
    remove_outgoing_selection, schedule_outgoing_batch_retry, schedule_outgoing_selection_retry,
    try_claim_outgoing_batch, try_claim_outgoing_selection, OutgoingTransferClaim,
    PersistedOutgoingBatch, PersistedOutgoingFile, PersistedOutgoingSelection,
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
const OPERATION_LOCK_MAX_ENTRIES: usize = 2048;
/// Resume metadata is advisory (the `.part` length is authoritative), so it
/// can be persisted at a bounded cadence instead of once per chunk. The
/// cadence keeps crash recovery loss bounded while avoiding one atomic JSON
/// rewrite and directory sync for every megabyte.
pub(crate) const RESUME_PERSIST_CHUNK_INTERVAL: u32 = 8;
pub(crate) const RESUME_PERSIST_INTERVAL: Duration = Duration::from_secs(1);

static FILE_BATCH_ADMISSION_LOCK: LazyLock<tokio::sync::Mutex<()>> =
    LazyLock::new(|| tokio::sync::Mutex::new(()));

/// Serializes the quota preflight and core batch admission across network
/// connections. Without this seam, two peers could both pass the active-batch
/// check before either one inserts its batch state.
pub fn file_batch_admission_lock() -> &'static tokio::sync::Mutex<()> {
    &FILE_BATCH_ADMISSION_LOCK
}

/// Return the last durable byte offset for a transfer whose in-memory receive
/// state disappeared. The sidecar is bound to the authenticated source and
/// transfer ID before its `.part` length is trusted.
pub fn persisted_file_resume_offset(
    source: &str,
    transfer_id: TransferId,
    incoming_dir: &Path,
) -> Option<u64> {
    persisted_transfer_offset(source, transfer_id, incoming_dir)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceivedFile {
    pub name: String,
    pub size: u64,
    pub hash: String,
    pub path: PathBuf,
}

/// Durable history work produced by the receive pipeline.
///
/// Keeping the batch identity, authenticated source, and post-commit UI
/// policy together prevents platform adapters from accidentally mixing the
/// values when a receive is retried after a process restart.
#[derive(Debug, Clone)]
pub struct FileReceiveCommit {
    pub batch_id: Option<TransferId>,
    pub files: Vec<ReceivedFile>,
    pub batch_total: usize,
    pub batch_complete: bool,
    pub activate_clipboard: bool,
    pub device: String,
    pub source_device_id: String,
    pub manifest_hash: Option<String>,
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

/// Result future returned by platform receive adapters.
///
/// The receive path awaits this future before sending a batch acceptance. This
/// keeps the network acknowledgement coupled to durable history persistence
/// without forcing the core crate to know which platform database is used.
pub type PlatformResultFuture = Pin<Box<dyn Future<Output = Result<(), String>> + Send>>;

pub trait SyncPlatform: Send + Sync {
    fn write_text(&self, text: &str) -> Result<(), String>;
    fn write_image(&self, width: u32, height: u32, rgba: &[u8]) -> Result<(), String>;
    fn set_file_progress(&self, name: &str, received: u64, total: u64);
    fn clear_file_progress(&self, batch_id: Option<TransferId>, device: Option<&str>);
    fn set_file_batch_progress(&self, progress: FileBatchProgress);
    /// Persist a completed receive before the network layer acknowledges it.
    ///
    /// Implementations may perform the database/file work on a blocking
    /// thread, but the future must not resolve successfully until the history
    /// record is durable. Clipboard activation and notifications may happen
    /// afterwards and must not turn a durable receive into a failed transfer.
    fn files_received(&self, commit: FileReceiveCommit) -> PlatformResultFuture;
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
    chunks_since_persist: u32,
    last_persist_at: Instant,
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

/// A resumable receive can lose its in-memory metadata when the authenticated
/// connection or daemon goes away. The sender must distinguish that recoverable
/// condition from a permanent validation or I/O failure so it can replay the
/// batch and file metadata instead of cancelling the transfer.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum FileReceiveError {
    #[error("file transfer metadata is not available")]
    MetadataUnavailable { transfer_id: TransferId },
    #[error("{0}")]
    Failed(String),
}

impl FileReceiveError {
    pub fn is_metadata_unavailable(&self) -> bool {
        matches!(self, Self::MetadataUnavailable { .. })
    }
}

impl From<String> for FileReceiveError {
    fn from(error: String) -> Self {
        Self::Failed(error)
    }
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

#[derive(Debug, Clone)]
struct IncomingBatch {
    manifest: FileBatchManifest,
    source: String,
    source_device_id: String,
    session_epoch: u64,
    local_generation: u64,
    files: Vec<Option<ReceivedFile>>,
    manifest_path: PathBuf,
}

type ReceiveOperationKey = (String, ReceiveKey);
type BatchOperationKey = (String, TransferId);

pub struct SyncEngine {
    /// Reliable event IDs retained across reconnects so ACK loss cannot apply
    /// the same clipboard event twice.
    seen_messages: HashMap<Arc<str>, HashMap<MessageId, i64>>,
    seen_message_order: VecDeque<(Arc<str>, MessageId, i64)>,
    /// Active file receives: authenticated peer + transfer ID → receive state.
    active_receives: HashMap<(String, ReceiveKey), FileReceiveState>,
    /// Per-transfer and per-batch locks are kept outside the state mutex's
    /// critical sections. Network handlers can therefore serialize one
    /// transfer while unrelated peers continue to inspect engine state.
    receive_operation_locks: HashMap<ReceiveOperationKey, Arc<tokio::sync::Mutex<()>>>,
    batch_operation_locks: HashMap<BatchOperationKey, Arc<tokio::sync::Mutex<()>>>,
    /// A FileMeta operation reserves its key before doing off-lock file I/O.
    /// This closes the race between two frames opening the same transfer.
    pending_receives: HashMap<ReceiveOperationKey, u64>,
    /// A chunk temporarily owns a receive state while it performs disk I/O.
    /// Cancellation includes these keys before waiting on their transfer lock.
    inflight_receives: HashSet<ReceiveOperationKey>,
    completed_transfers: HashMap<(String, ReceiveKey), CompletedTransfer>,
    incoming_batches: HashMap<(String, TransferId), IncomingBatch>,
    cancelled_batches: HashMap<(String, TransferId), i64>,
    completed_batches: HashMap<(String, TransferId), i64>,
    completed_batch_manifests: HashMap<(String, TransferId), FileBatchManifest>,
    peer_device_ids: HashMap<String, String>,
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

impl SyncEngine {
    fn prune_receive_operation_locks(&mut self) {
        if self.receive_operation_locks.len() <= OPERATION_LOCK_MAX_ENTRIES {
            return;
        }
        let remove_count = self
            .receive_operation_locks
            .len()
            .saturating_sub(OPERATION_LOCK_MAX_ENTRIES);
        let keys = self
            .receive_operation_locks
            .iter()
            .filter(|(key, lock)| {
                Arc::strong_count(lock) == 1
                    && !self.active_receives.contains_key(*key)
                    && !self.pending_receives.contains_key(*key)
                    && !self.inflight_receives.contains(*key)
                    && !self.completed_transfers.contains_key(*key)
            })
            .map(|(key, _)| key.clone())
            .take(remove_count)
            .collect::<Vec<_>>();
        for key in keys {
            self.receive_operation_locks.remove(&key);
        }
    }

    fn prune_batch_operation_locks(&mut self) {
        if self.batch_operation_locks.len() <= OPERATION_LOCK_MAX_ENTRIES {
            return;
        }
        let remove_count = self
            .batch_operation_locks
            .len()
            .saturating_sub(OPERATION_LOCK_MAX_ENTRIES);
        let keys = self
            .batch_operation_locks
            .iter()
            .filter(|(key, lock)| {
                Arc::strong_count(lock) == 1
                    && !self.incoming_batches.contains_key(*key)
                    && !self.cancelled_batches.contains_key(*key)
                    && !self.completed_batches.contains_key(*key)
            })
            .map(|(key, _)| key.clone())
            .take(remove_count)
            .collect::<Vec<_>>();
        for key in keys {
            self.batch_operation_locks.remove(&key);
        }
    }

    fn receive_operation_lock(&mut self, key: &ReceiveOperationKey) -> Arc<tokio::sync::Mutex<()>> {
        self.prune_receive_operation_locks();
        self.receive_operation_locks
            .entry(key.clone())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    fn batch_operation_lock(&mut self, key: &BatchOperationKey) -> Arc<tokio::sync::Mutex<()>> {
        self.prune_batch_operation_locks();
        self.batch_operation_locks
            .entry(key.clone())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }
}

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
            SyncEngine::commit_received_file_shared(sync_engine, source, verified).await
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
                SyncEngine::suspend_receive_epoch_shared(&sync_engine, &source, epoch).await;
            });
        } else if let Ok(mut sync) = sync_engine.try_lock() {
            sync.suspend_receive_epoch(&source, epoch);
        }
    }
}

#[cfg(test)]
mod tests;
