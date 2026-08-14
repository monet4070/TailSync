use crate::protocol::{FileChunkPayload, MessageId, TransferId, FILE_CHUNK_SIZE};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

const SEEN_MESSAGE_RETENTION_SECONDS: i64 = 10 * 60;
pub const INCOMPLETE_TRANSFER_RETENTION_SECONDS: u64 = 24 * 60 * 60;
const CANCELLED_BATCH_RETENTION_SECONDS: i64 = 24 * 60 * 60;
const CANCELLED_BATCH_MAX_ENTRIES: usize = 1024;
pub const MAX_FILE_BATCH_COUNT: usize = 20;
pub const MAX_FILE_BATCH_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_ACTIVE_BATCHES_PER_PEER: usize = 2;
const MAX_ACTIVE_BATCHES_GLOBAL: usize = 8;
const MAX_ACTIVE_RECEIVES_PER_PEER: usize = 2;
const MAX_ACTIVE_RECEIVES_GLOBAL: usize = 8;
const SHADOW_FILTER_TTL: Duration = Duration::from_secs(30);
const SHADOW_FILTER_MAX_ENTRIES: usize = 1024;

static FILE_BATCH_ADMISSION_LOCK: LazyLock<tokio::sync::Mutex<()>> =
    LazyLock::new(|| tokio::sync::Mutex::new(()));

/// Serializes the quota preflight and core batch admission across network
/// connections. Without this seam, two peers could both pass the active-batch
/// check before either one inserts its batch state.
pub fn file_batch_admission_lock() -> &'static tokio::sync::Mutex<()> {
    &FILE_BATCH_ADMISSION_LOCK
}

struct ShadowEntry {
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
                expires_at: now + SHADOW_FILTER_TTL,
            },
        );
    }

    /// Shadow entries intentionally remain sticky for the full TTL. Clipboard
    /// backends can emit several events for one programmatic write, and every
    /// one of those echoes must be suppressed. The accepted trade-off is that
    /// a user copying identical content during the TTL is suppressed as well.
    fn contains(&mut self, hash: &str) -> bool {
        self.prune(Instant::now());
        self.entries.contains_key(hash)
    }

    fn remove(&mut self, hash: &str) -> bool {
        self.entries.remove(hash).is_some()
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
    #[serde(default)]
    pub batch: Option<FileBatchRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileBatchRef {
    pub batch_id: TransferId,
    pub index: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileBatchEntry {
    pub transfer_id: TransferId,
    pub index: u16,
    pub name: String,
    #[serde(default)]
    pub source_parent: String,
    pub size: u64,
    pub hash: String,
    #[serde(default = "default_file_chunk_size")]
    pub chunk_size: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileBatchManifest {
    pub batch_id: TransferId,
    pub generation: u64,
    pub total_bytes: u64,
    pub files: Vec<FileBatchEntry>,
}

#[derive(Debug, Clone)]
pub struct PreparedFile {
    pub path: PathBuf,
    pub modified_nanos: u128,
    pub entry: FileBatchEntry,
}

#[derive(Debug, Clone)]
pub struct PreparedFileBatch {
    pub manifest: FileBatchManifest,
    pub files: Vec<PreparedFile>,
}

impl FileBatchManifest {
    pub fn validate(&self) -> Result<(), String> {
        if self.files.is_empty() || self.files.len() > MAX_FILE_BATCH_COUNT {
            return Err(format!(
                "A file batch must contain between 1 and {MAX_FILE_BATCH_COUNT} files"
            ));
        }
        if self.total_bytes > MAX_FILE_BATCH_BYTES {
            return Err("File batch exceeds the 1 GiB transfer limit".to_string());
        }
        let mut transfer_ids = HashSet::new();
        let mut total = 0_u64;
        for (expected_index, file) in self.files.iter().enumerate() {
            if usize::from(file.index) != expected_index {
                return Err("File batch indexes must be contiguous".to_string());
            }
            if !transfer_ids.insert(file.transfer_id) {
                return Err("File batch contains a duplicate transfer ID".to_string());
            }
            if file.name.is_empty()
                || Path::new(&file.name)
                    .file_name()
                    .is_none_or(|name| name != file.name.as_str())
            {
                return Err("File batch contains an invalid file name".to_string());
            }
            if file.chunk_size == 0 || file.chunk_size as usize > FILE_CHUNK_SIZE {
                return Err("File batch contains an invalid chunk size".to_string());
            }
            total = total
                .checked_add(file.size)
                .ok_or_else(|| "File batch byte count overflowed".to_string())?;
        }
        if total != self.total_bytes {
            return Err("File batch total does not match its manifest".to_string());
        }
        Ok(())
    }
}

pub fn prepare_file_batch(
    paths: Vec<PathBuf>,
    generation: u64,
) -> Result<PreparedFileBatch, String> {
    if paths.is_empty() || paths.len() > MAX_FILE_BATCH_COUNT {
        return Err(format!(
            "Select between 1 and {MAX_FILE_BATCH_COUNT} ordinary files"
        ));
    }
    let mut candidates = Vec::with_capacity(paths.len());
    let mut total_bytes = 0_u64;
    for path in paths {
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("Cannot inspect {}: {error}", path.display()))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "The selection contains a symbolic link: {}",
                path.display()
            ));
        }
        if !metadata.is_file() {
            return Err(format!(
                "The selection contains a folder or non-file item: {}",
                path.display()
            ));
        }
        total_bytes = total_bytes
            .checked_add(metadata.len())
            .ok_or_else(|| "The selected file sizes overflowed".to_string())?;
        if total_bytes > MAX_FILE_BATCH_BYTES {
            return Err("The selected files exceed the 1 GiB batch limit".to_string());
        }
        let original_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .ok_or_else(|| format!("{} does not have a valid file name", path.display()))?
            .to_string();
        let source_parent = path
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .map(short_parent_label)
            .unwrap_or_default();
        let modified_nanos = modified_nanos(&metadata)?;
        candidates.push((
            path,
            original_name,
            source_parent,
            metadata.len(),
            modified_nanos,
        ));
    }

    let mut used_names = HashSet::new();
    let mut entries = Vec::with_capacity(candidates.len());
    let mut files = Vec::with_capacity(candidates.len());
    for (index, (path, original_name, source_parent, size, modified_nanos)) in
        candidates.into_iter().enumerate()
    {
        let display_name = collision_safe_name(&original_name, &source_parent, &mut used_names);
        let hash = hash_source_file(&path)?;
        let entry = FileBatchEntry {
            transfer_id: TransferId::random(),
            index: u16::try_from(index).map_err(|_| "File batch index overflowed")?,
            name: display_name,
            source_parent,
            size,
            hash,
            chunk_size: FILE_CHUNK_SIZE as u32,
        };
        entries.push(entry.clone());
        files.push(PreparedFile {
            path,
            modified_nanos,
            entry,
        });
    }
    let manifest = FileBatchManifest {
        batch_id: TransferId::random(),
        generation,
        total_bytes,
        files: entries,
    };
    manifest.validate()?;
    Ok(PreparedFileBatch { manifest, files })
}

pub fn revalidate_prepared_file(file: &PreparedFile) -> Result<(), String> {
    let metadata = fs::symlink_metadata(&file.path)
        .map_err(|error| format!("Cannot re-open {}: {error}", file.path.display()))?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() != file.entry.size
        || modified_nanos(&metadata)? != file.modified_nanos
    {
        return Err(format!(
            "{} changed after the batch was copied",
            file.path.display()
        ));
    }
    let hash = hash_source_file(&file.path)?;
    if hash != file.entry.hash {
        return Err(format!(
            "{} changed after the batch was copied",
            file.path.display()
        ));
    }
    Ok(())
}

fn modified_nanos(metadata: &fs::Metadata) -> Result<u128, String> {
    metadata
        .modified()
        .map_err(|error| error.to_string())?
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .map_err(|error| error.to_string())
}

fn hash_source_file(path: &Path) -> Result<String, String> {
    let file = File::open(path).map_err(|error| error.to_string())?;
    let mut reader = BufReader::with_capacity(64 * 1024, file);
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn short_parent_label(value: &str) -> String {
    value.chars().take(24).collect()
}

fn collision_safe_name(original: &str, parent_label: &str, used: &mut HashSet<String>) -> String {
    if used.insert(original.to_string()) {
        return original.to_string();
    }
    let path = Path::new(original);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(original);
    let extension = path.extension().and_then(|value| value.to_str());
    let label = if parent_label.is_empty() {
        "Folder"
    } else {
        parent_label
    };
    for suffix in 1..=9999 {
        let annotation = if suffix == 1 {
            label.to_string()
        } else {
            format!("{label} {suffix}")
        };
        let candidate = match extension {
            Some(extension) => format!("{stem} ({annotation}).{extension}"),
            None => format!("{stem} ({annotation})"),
        };
        if used.insert(candidate.clone()) {
            return candidate;
        }
    }
    format!("{stem} ({})", TransferId::random().as_hex())
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
        let computed = hash_source_file(&self.file.path)?;
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

struct IncomingBatch {
    manifest: FileBatchManifest,
    source: String,
    local_generation: u64,
    files: Vec<Option<ReceivedFile>>,
    manifest_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedIncomingBatch {
    source: String,
    manifest: FileBatchManifest,
    files: Vec<Option<ReceivedFile>>,
}

pub struct SyncEngine {
    /// Reliable event IDs retained across reconnects so ACK loss cannot apply
    /// the same clipboard event twice.
    seen_messages: HashMap<(String, MessageId), i64>,
    /// Active file receives: authenticated peer + transfer ID → receive state.
    active_receives: HashMap<(String, TransferId), FileReceiveState>,
    completed_transfers: HashMap<(String, TransferId), CompletedTransfer>,
    incoming_batches: HashMap<(String, TransferId), IncomingBatch>,
    cancelled_batches: HashMap<(String, TransferId), i64>,
    completed_batches: HashMap<(String, TransferId), i64>,
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

impl SyncEngine {
    pub fn new() -> Self {
        SyncEngine {
            seen_messages: HashMap::new(),
            active_receives: HashMap::new(),
            completed_transfers: HashMap::new(),
            incoming_batches: HashMap::new(),
            cancelled_batches: HashMap::new(),
            completed_batches: HashMap::new(),
            clipboard_generation: 0,
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
        self.supersede_file_clipboard();
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
            self.shadow_filter.remove(&hash);
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
        self.supersede_file_clipboard();
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
            self.image_shadow_filter.remove(&hash);
            return Err(error);
        }
        Ok((image.width, image.height))
    }

    // ── Files ────────────────────────────────────────────────────

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
        // Validate before touching disk. The server also validates before its
        // quota preflight, but this remains the authoritative core check.
        manifest.validate()?;
        let key = (source.clone(), manifest.batch_id);
        self.prune_cancelled_batches();
        if self.cancelled_batches.contains_key(&key) {
            return Err("File batch was cancelled; copy the files again to retry".to_string());
        }
        if let Some(existing) = self.incoming_batches.get(&key) {
            return if existing.manifest == manifest {
                Ok(())
            } else {
                Err("Batch ID was reused with a different manifest".to_string())
            };
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
        fs::create_dir_all(incoming_dir).map_err(|error| error.to_string())?;
        let manifest_path = incoming_dir.join(format!("{}.batch.json", manifest.batch_id.as_hex()));
        let mut files = vec![None; manifest.files.len()];
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
            },
        )?;
        let local_generation = self.supersede_file_clipboard();
        self.incoming_batches.insert(
            key,
            IncomingBatch {
                manifest,
                source,
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
            .get(&(source.to_string(), transfer_id))
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
        let transfer_ids = self
            .active_receives
            .iter()
            .filter_map(|((peer, transfer_id), state)| {
                (peer == source
                    && state
                        .meta
                        .batch
                        .is_some_and(|batch| batch.batch_id == batch_id))
                .then_some(*transfer_id)
            })
            .collect::<Vec<_>>();
        for transfer_id in transfer_ids {
            if let Some(state) = self
                .active_receives
                .remove(&(source.to_string(), transfer_id))
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

    /// Open a new transfer or restore its durable `.part` + sidecar state.
    pub async fn begin_file_receive(
        &mut self,
        meta: FileMeta,
        file_path: &Path,
        source: String,
    ) -> Result<FileReceiveProgress, String> {
        let transfer_id = meta.transfer_id.unwrap_or(TransferId([0; 16]));
        let key = (source.clone(), transfer_id);
        if let Some(batch_ref) = meta.batch {
            let batch_key = (source.clone(), batch_ref.batch_id);
            self.prune_cancelled_batches();
            if self.cancelled_batches.contains_key(&batch_key) {
                return Err("File batch was cancelled".to_string());
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
        if let Some(state) = self.active_receives.get(&key) {
            if state.meta.hash == meta.hash && state.meta.size == meta.size {
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
                        if let Some(file_name) = saved.final_path.file_name() {
                            final_path = parent.join(file_name);
                        }
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
        let requires_full_hash = received > 0;
        file.seek(SeekFrom::Start(received))
            .map_err(|error| error.to_string())?;
        let writer = BufWriter::with_capacity(FILE_CHUNK_SIZE, file);
        let state = FileReceiveState {
            meta: meta.clone(),
            tmp_path,
            final_path,
            state_path,
            writer,
            hasher: blake3::Hasher::new(),
            received,
            requires_full_hash,
        };
        persist_transfer_state(&state, &source)?;

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
    ) -> Result<FileReceiveProgress, String> {
        let key = (source.clone(), chunk.transfer_id);
        if let Some(completed) = self.completed_transfers.get(&key) {
            return Ok(FileReceiveProgress {
                transfer_id: chunk.transfer_id,
                next_offset: completed.size,
                completed: None,
            });
        }
        let state = self
            .active_receives
            .get_mut(&key)
            .ok_or_else(|| "file transfer metadata is not available".to_string())?;

        if chunk.offset != state.received {
            return Ok(FileReceiveProgress {
                transfer_id: chunk.transfer_id,
                next_offset: state.received,
                completed: None,
            });
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
        let progress_meta = state.meta.clone();
        if progress_meta.batch.is_some() {
            self.set_receive_progress(&source, &progress_meta, next_offset);
        } else {
            self.set_file_progress(&progress_name, next_offset, progress_total);
        }

        if completed {
            let state = self
                .active_receives
                .remove(&key)
                .ok_or_else(|| "completed transfer state disappeared".to_string())?;
            let completed = self.finish_file_receive(state, &source).await?;
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

    async fn finish_file_receive(
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
            self.commit_received_file(source, pending)?;
            Ok(None)
        } else {
            info!(
                "File receive complete: {} ({} bytes, awaiting resumed-file verification)",
                pending.meta.name, pending.meta.size
            );
            Ok(Some(pending))
        }
    }

    pub fn commit_received_file(
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
            transfer_id,
            meta,
            file: received_file,
            ..
        } = pending;
        if let Some(batch_ref) = meta.batch {
            let batch = self
                .incoming_batches
                .get_mut(&(source.to_string(), batch_ref.batch_id))
                .ok_or_else(|| "File batch state disappeared before completion".to_string())?;
            let mut files = batch.files.clone();
            let slot = files
                .get_mut(usize::from(batch_ref.index))
                .ok_or_else(|| "File batch index is out of range".to_string())?;
            *slot = Some(received_file);
            let persisted = PersistedIncomingBatch {
                source: batch.source.clone(),
                manifest: batch.manifest.clone(),
                files: files.clone(),
            };
            persist_incoming_batch(&batch.manifest_path, &persisted)?;
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
        } else if let Some(platform) = self.platform.as_ref() {
            platform.files_received(None, vec![received_file], 1, true, true, source.to_string());
        }
        self.completed_transfers.insert(
            (source.to_string(), transfer_id),
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
        let keys = self
            .active_receives
            .keys()
            .filter(|(peer, _)| peer == source)
            .cloned()
            .collect::<Vec<_>>();
        for key in keys {
            if let Some(state) = self.active_receives.remove(&key) {
                drop(state.writer);
            }
        }
        self.incoming_batches.retain(|(peer, _), _| peer != source);
        self.cancelled_batches.retain(|(peer, _), _| peer != source);
        self.clear_file_progress(None, Some(source));
    }

    fn prune_cancelled_batches(&mut self) {
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

    fn set_receive_progress(&self, source: &str, meta: &FileMeta, received: u64) {
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

    fn clear_file_progress(&self, batch_id: Option<TransferId>, device: Option<&str>) {
        if let Some(platform) = self.platform.as_ref() {
            platform.clear_file_progress(batch_id, device);
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

fn persist_incoming_batch(path: &Path, batch: &PersistedIncomingBatch) -> Result<(), String> {
    let temp = path.with_extension("json.tmp");
    fs::write(
        &temp,
        serde_json::to_vec_pretty(batch).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    fs::rename(&temp, path).map_err(|error| error.to_string())
}

fn restore_persisted_received_file(
    saved: &ReceivedFile,
    expected: &FileBatchEntry,
    incoming_dir: &Path,
) -> Option<ReceivedFile> {
    if saved.name != expected.name || saved.size != expected.size || saved.hash != expected.hash {
        return None;
    }
    let file_name = saved.path.file_name()?;
    let candidate = incoming_dir.join(file_name);
    let incoming_dir = incoming_dir.canonicalize().ok()?;
    let canonical = candidate.canonicalize().ok()?;
    if canonical.parent() != Some(incoming_dir.as_path()) {
        return None;
    }
    let metadata = canonical.metadata().ok()?;
    if !metadata.is_file() || metadata.len() != expected.size {
        return None;
    }
    if hash_source_file(&canonical).ok()? != expected.hash {
        return None;
    }
    Some(ReceivedFile {
        name: expected.name.clone(),
        size: expected.size,
        hash: expected.hash.clone(),
        path: canonical,
    })
}

pub fn cleanup_expired_transfers() {
    let incoming = crate::db::get_incoming_dir();
    cleanup_expired_transfers_in(
        &incoming,
        Duration::from_secs(INCOMPLETE_TRANSFER_RETENTION_SECONDS),
    );
}

fn cleanup_expired_transfers_in(incoming: &Path, retention: Duration) {
    let Ok(entries) = fs::read_dir(incoming) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        // Only the incoming root is inspected. Managed subdirectories such as
        // clipboard-files have their own lifecycle and must never be touched.
        if !metadata.is_file() {
            continue;
        }
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let is_partial = file_name.ends_with(".part")
            || file_name.ends_with(".resume.json")
            || file_name.ends_with(".batch.json");
        let expired = metadata
            .modified()
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age > retention);
        if expired {
            if is_partial && file_name.ends_with(".batch.json") {
                if let Ok(data) = fs::read(&path) {
                    if let Ok(batch) = serde_json::from_slice::<PersistedIncomingBatch>(&data) {
                        for (index, saved) in batch.files.iter().enumerate() {
                            let Some(saved) = saved else {
                                continue;
                            };
                            let Some(expected) = batch.manifest.files.get(index) else {
                                continue;
                            };
                            if let Some(file) =
                                restore_persisted_received_file(saved, expected, incoming)
                            {
                                let _ = fs::remove_file(file.path);
                            }
                        }
                    }
                }
            }
            // Non-partial root files only exist briefly between a successful
            // rename and history import. Once stale, they are orphaned
            // plaintext left by an import failure or process crash.
            let _ = fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_transferred_file_name, prepare_file_batch, FileBatchEntry, FileBatchManifest,
        FileBatchProgress, FileBatchRef, FileMeta, ReceivedFile, ShadowFilter, SyncEngine,
        SyncPlatform, CANCELLED_BATCH_MAX_ENTRIES, CANCELLED_BATCH_RETENTION_SECONDS,
        MAX_ACTIVE_BATCHES_GLOBAL, MAX_ACTIVE_BATCHES_PER_PEER, MAX_FILE_BATCH_BYTES,
        MAX_FILE_BATCH_COUNT, SHADOW_FILTER_MAX_ENTRIES,
    };
    use crate::protocol::{FileChunkPayload, MessageId, TransferId, FILE_CHUNK_SIZE};
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant, SystemTime};

    #[derive(Debug)]
    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("tailsync-{label}-{:016x}", rand::random::<u64>()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn cancelled_batch_cache_expires_and_caps_entries() {
        let mut engine = SyncEngine::new();
        let now = chrono::Utc::now().timestamp();
        let expired_key = ("expired-peer".to_string(), TransferId([0xFE; 16]));
        engine.cancelled_batches.insert(
            expired_key.clone(),
            now - CANCELLED_BATCH_RETENTION_SECONDS - 1,
        );
        for index in 0..(CANCELLED_BATCH_MAX_ENTRIES + 32) {
            let mut id = [0_u8; 16];
            id[..8].copy_from_slice(&(index as u64).to_le_bytes());
            engine
                .cancelled_batches
                .insert(("peer".to_string(), TransferId(id)), now);
        }

        engine.prune_cancelled_batches();

        assert_eq!(engine.cancelled_batches.len(), CANCELLED_BATCH_MAX_ENTRIES);
        assert!(!engine.cancelled_batches.contains_key(&expired_key));
    }

    #[derive(Debug, Clone)]
    struct ReceivedBatchEvent {
        batch_id: Option<TransferId>,
        files: Vec<ReceivedFile>,
        batch_total: usize,
        batch_complete: bool,
        activate_clipboard: bool,
        device: String,
    }

    #[derive(Default)]
    struct TestPlatform {
        received: Mutex<Vec<ReceivedBatchEvent>>,
    }

    impl TestPlatform {
        fn received(&self) -> Vec<ReceivedBatchEvent> {
            self.received.lock().unwrap().clone()
        }
    }

    impl SyncPlatform for TestPlatform {
        fn write_text(&self, _text: &str) -> Result<(), String> {
            Ok(())
        }

        fn write_image(&self, _width: u32, _height: u32, _rgba: &[u8]) -> Result<(), String> {
            Ok(())
        }

        fn set_file_progress(&self, _name: &str, _received: u64, _total: u64) {}

        fn clear_file_progress(&self, _batch_id: Option<TransferId>, _device: Option<&str>) {}

        fn set_file_batch_progress(&self, _progress: FileBatchProgress) {}

        fn files_received(
            &self,
            batch_id: Option<TransferId>,
            files: Vec<ReceivedFile>,
            batch_total: usize,
            batch_complete: bool,
            activate_clipboard: bool,
            device: String,
        ) {
            self.received.lock().unwrap().push(ReceivedBatchEvent {
                batch_id,
                files,
                batch_total,
                batch_complete,
                activate_clipboard,
                device,
            });
        }

        fn file_batch_failed(&self, _batch_id: Option<TransferId>, _message: &str) {}
    }

    fn manifest_with_sizes(sizes: &[u64]) -> FileBatchManifest {
        FileBatchManifest {
            batch_id: TransferId([200; 16]),
            generation: 1,
            total_bytes: sizes.iter().sum(),
            files: sizes
                .iter()
                .enumerate()
                .map(|(index, size)| FileBatchEntry {
                    transfer_id: TransferId([u8::try_from(index + 1).unwrap(); 16]),
                    index: u16::try_from(index).unwrap(),
                    name: format!("file-{index}.bin"),
                    source_parent: "Source".to_string(),
                    size: *size,
                    hash: blake3::hash(&[]).to_hex().to_string(),
                    chunk_size: FILE_CHUNK_SIZE as u32,
                })
                .collect(),
        }
    }

    async fn receive_empty_file(
        sync: &mut SyncEngine,
        manifest: &FileBatchManifest,
        index: usize,
        incoming: &Path,
        source: &str,
    ) {
        let entry = manifest.files[index].clone();
        let meta = FileMeta {
            transfer_id: Some(entry.transfer_id),
            name: entry.name.clone(),
            size: entry.size,
            hash: entry.hash,
            chunk_size: entry.chunk_size,
            batch: Some(FileBatchRef {
                batch_id: manifest.batch_id,
                index: entry.index,
            }),
        };
        sync.begin_file_receive(meta, &incoming.join(&entry.name), source.to_string())
            .await
            .unwrap();
    }

    #[test]
    fn file_batch_count_and_size_boundaries_are_exact() {
        assert!(manifest_with_sizes(&[0; MAX_FILE_BATCH_COUNT])
            .validate()
            .is_ok());
        assert!(manifest_with_sizes(&[0; MAX_FILE_BATCH_COUNT + 1])
            .validate()
            .unwrap_err()
            .contains("between 1 and"));
        assert!(manifest_with_sizes(&[MAX_FILE_BATCH_BYTES])
            .validate()
            .is_ok());
        assert!(manifest_with_sizes(&[MAX_FILE_BATCH_BYTES + 1])
            .validate()
            .unwrap_err()
            .contains("1 GiB"));
    }

    #[test]
    fn active_file_batch_limits_are_enforced_without_affecting_other_peers() {
        let directory = TestDirectory::new("batch-limits");
        let mut sync = SyncEngine::new();

        for batch_index in 0..MAX_ACTIVE_BATCHES_PER_PEER {
            let mut manifest = manifest_with_sizes(&[0]);
            manifest.batch_id = TransferId([batch_index as u8; 16]);
            sync.begin_file_batch(manifest, "peer".to_string(), directory.path())
                .unwrap();
        }

        let mut rejected = manifest_with_sizes(&[0]);
        rejected.batch_id = TransferId([99; 16]);
        assert!(sync
            .begin_file_batch(rejected, "peer".to_string(), directory.path())
            .unwrap_err()
            .contains("active file batches"));

        let mut other_peer = manifest_with_sizes(&[0]);
        other_peer.batch_id = TransferId([100; 16]);
        sync.begin_file_batch(other_peer, "other-peer".to_string(), directory.path())
            .unwrap();
    }

    #[test]
    fn pending_file_batch_bytes_exclude_partial_data_already_on_disk() {
        let directory = TestDirectory::new("pending-batch-bytes");
        let manifest = manifest_with_sizes(&[10, 20]);
        let first_transfer = manifest.files[0].transfer_id;
        let mut sync = SyncEngine::new();
        sync.begin_file_batch(manifest, "peer-a".into(), directory.path())
            .unwrap();
        std::fs::write(
            directory
                .path()
                .join(format!("{}.part", first_transfer.as_hex())),
            [0_u8; 4],
        )
        .unwrap();

        assert_eq!(sync.pending_file_batch_bytes(), 26);
    }

    #[test]
    fn global_active_file_batch_limit_is_enforced() {
        let directory = TestDirectory::new("batch-global-limit");
        let mut sync = SyncEngine::new();

        for batch_index in 0..MAX_ACTIVE_BATCHES_GLOBAL {
            let mut manifest = manifest_with_sizes(&[0]);
            manifest.batch_id = TransferId([batch_index as u8; 16]);
            sync.begin_file_batch(manifest, format!("peer-{batch_index}"), directory.path())
                .unwrap();
        }

        let mut rejected = manifest_with_sizes(&[0]);
        rejected.batch_id = TransferId([99; 16]);
        assert!(sync
            .begin_file_batch(rejected, "peer-final".to_string(), directory.path())
            .unwrap_err()
            .contains("global active file batch limit"));
    }

    #[test]
    fn folder_rejects_the_entire_selection() {
        let directory = TestDirectory::new("folder-batch");
        let file = directory.path().join("ordinary.txt");
        let folder = directory.path().join("folder");
        std::fs::write(&file, b"ordinary").unwrap();
        std::fs::create_dir(&folder).unwrap();

        let error = prepare_file_batch(vec![file, folder], 1).unwrap_err();
        assert!(error.contains("folder or non-file"));
    }

    #[test]
    fn symbolic_link_rejects_the_entire_selection_when_supported() {
        let directory = TestDirectory::new("symlink-batch");
        let target = directory.path().join("target.txt");
        let link = directory.path().join("link.txt");
        std::fs::write(&target, b"target").unwrap();

        #[cfg(windows)]
        if std::os::windows::fs::symlink_file(&target, &link).is_err() {
            return;
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let error = prepare_file_batch(vec![target, link], 1).unwrap_err();
        assert!(error.contains("symbolic link"));
    }

    #[test]
    fn duplicate_names_get_a_short_source_directory_label() {
        let directory = TestDirectory::new("duplicate-names");
        let finance = directory.path().join("Finance");
        let legal = directory.path().join("Legal");
        std::fs::create_dir_all(&finance).unwrap();
        std::fs::create_dir_all(&legal).unwrap();
        let first = finance.join("report.pdf");
        let second = legal.join("report.pdf");
        std::fs::write(&first, b"finance").unwrap();
        std::fs::write(&second, b"legal").unwrap();

        let batch = prepare_file_batch(vec![first, second], 1).unwrap();
        assert_eq!(batch.manifest.files[0].name, "report.pdf");
        assert_eq!(batch.manifest.files[1].name, "report (Legal).pdf");
        assert_eq!(batch.manifest.files[1].source_parent, "Legal");
    }

    #[tokio::test]
    async fn batch_updates_clipboard_only_after_all_files_and_completion() {
        let directory = TestDirectory::new("batch-complete");
        let manifest = manifest_with_sizes(&[0, 0]);
        let platform = Arc::new(TestPlatform::default());
        let mut sync = SyncEngine::new();
        sync.set_platform(platform.clone());
        sync.begin_file_batch(manifest.clone(), "peer".to_string(), directory.path())
            .unwrap();

        receive_empty_file(&mut sync, &manifest, 0, directory.path(), "peer").await;
        receive_empty_file(&mut sync, &manifest, 1, directory.path(), "peer").await;
        assert!(platform.received().is_empty());

        sync.finish_file_batch("peer", manifest.batch_id).unwrap();
        let events = platform.received();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].batch_id, Some(manifest.batch_id));
        assert_eq!(events[0].files.len(), 2);
        assert_eq!(events[0].batch_total, 2);
        assert!(events[0].batch_complete);
        assert!(events[0].activate_clipboard);
        assert_eq!(events[0].device, "peer");
    }

    #[tokio::test]
    async fn completed_file_batch_is_idempotent() {
        let directory = TestDirectory::new("batch-idempotent");
        let manifest = manifest_with_sizes(&[0]);
        let platform = Arc::new(TestPlatform::default());
        let mut sync = SyncEngine::new();
        sync.set_platform(platform.clone());
        sync.begin_file_batch(manifest.clone(), "peer".to_string(), directory.path())
            .unwrap();
        receive_empty_file(&mut sync, &manifest, 0, directory.path(), "peer").await;

        sync.finish_file_batch("peer", manifest.batch_id).unwrap();
        sync.finish_file_batch("peer", manifest.batch_id).unwrap();
        assert_eq!(platform.received().len(), 1);
    }

    #[tokio::test]
    async fn newer_clipboard_generation_prevents_old_batch_activation() {
        let directory = TestDirectory::new("superseded-batch");
        let manifest = manifest_with_sizes(&[0]);
        let platform = Arc::new(TestPlatform::default());
        let mut sync = SyncEngine::new();
        sync.set_platform(platform.clone());
        sync.begin_file_batch(manifest.clone(), "peer".to_string(), directory.path())
            .unwrap();
        receive_empty_file(&mut sync, &manifest, 0, directory.path(), "peer").await;

        sync.supersede_file_clipboard();
        sync.finish_file_batch("peer", manifest.batch_id).unwrap();
        let events = platform.received();
        assert_eq!(events.len(), 1);
        assert!(events[0].batch_complete);
        assert!(!events[0].activate_clipboard);
    }

    #[tokio::test]
    async fn cancellation_exposes_only_completed_files_as_incomplete_history() {
        let directory = TestDirectory::new("cancelled-batch");
        let manifest = manifest_with_sizes(&[0, 0]);
        let platform = Arc::new(TestPlatform::default());
        let mut sync = SyncEngine::new();
        sync.set_platform(platform.clone());
        sync.begin_file_batch(manifest.clone(), "peer".to_string(), directory.path())
            .unwrap();
        receive_empty_file(&mut sync, &manifest, 0, directory.path(), "peer").await;

        sync.cancel_file_batch("peer", manifest.batch_id).await;
        let events = platform.received();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].files.len(), 1);
        assert_eq!(events[0].batch_total, 2);
        assert!(!events[0].batch_complete);
        assert!(!events[0].activate_clipboard);
    }

    #[tokio::test]
    async fn completed_batch_files_resume_after_engine_restart() {
        let directory = TestDirectory::new("batch-restart");
        let manifest = manifest_with_sizes(&[0, 0]);
        let mut initial = SyncEngine::new();
        initial
            .begin_file_batch(manifest.clone(), "peer".to_string(), directory.path())
            .unwrap();
        receive_empty_file(&mut initial, &manifest, 0, directory.path(), "peer").await;
        drop(initial);

        let platform = Arc::new(TestPlatform::default());
        let mut restored = SyncEngine::new();
        restored.set_platform(platform.clone());
        restored
            .begin_file_batch(manifest.clone(), "peer".to_string(), directory.path())
            .unwrap();
        receive_empty_file(&mut restored, &manifest, 1, directory.path(), "peer").await;
        restored
            .finish_file_batch("peer", manifest.batch_id)
            .unwrap();

        let events = platform.received();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].files.len(), 2);
        assert!(events[0].batch_complete);
    }

    #[tokio::test]
    async fn connection_suspend_releases_handles_and_preserves_partial_offset() {
        let directory = TestDirectory::new("suspend-receive");
        let transfer_id = TransferId([44; 16]);
        let data = b"first-second";
        let meta = FileMeta {
            transfer_id: Some(transfer_id),
            name: "received.bin".to_string(),
            size: data.len() as u64,
            hash: blake3::hash(data).to_hex().to_string(),
            chunk_size: FILE_CHUNK_SIZE as u32,
            batch: None,
        };
        let mut initial = SyncEngine::new();
        initial
            .begin_file_receive(
                meta.clone(),
                &directory.path().join("received.bin"),
                "peer".to_string(),
            )
            .await
            .unwrap();
        initial
            .handle_resumable_file_chunk(
                &FileChunkPayload {
                    transfer_id,
                    offset: 0,
                    data: b"first-".to_vec(),
                },
                "peer".to_string(),
            )
            .await
            .unwrap();

        initial.suspend_receive("peer");
        assert!(directory
            .path()
            .join(format!("{}.part", transfer_id.as_hex()))
            .is_file());
        assert!(directory
            .path()
            .join(format!("{}.resume.json", transfer_id.as_hex()))
            .is_file());

        let mut restored = SyncEngine::new();
        let offset = restored
            .begin_file_receive(
                meta,
                &directory.path().join("different-name.bin"),
                "peer".to_string(),
            )
            .await
            .unwrap()
            .next_offset;
        assert_eq!(offset, 6);
    }

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
    fn shadow_filter_stays_sticky_and_bounded() {
        let mut filter = ShadowFilter::new();
        filter.insert("same".into());
        filter.insert("same".into());
        assert!(filter.contains("same"));
        assert!(filter.contains("same"));
        assert_eq!(filter.entries.len(), 1);

        for index in 0..(SHADOW_FILTER_MAX_ENTRIES + 20) {
            filter.insert(format!("hash-{index}"));
        }
        assert_eq!(filter.entries.len(), SHADOW_FILTER_MAX_ENTRIES);
    }

    #[test]
    fn shadow_filter_remove_rolls_back_and_expired_entries_miss() {
        let mut filter = ShadowFilter::new();
        filter.insert("rollback".into());
        assert!(filter.remove("rollback"));
        assert!(!filter.contains("rollback"));

        filter.insert("expired".into());
        filter.entries.get_mut("expired").unwrap().expires_at = Instant::now();
        assert!(!filter.contains("expired"));
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
                batch: None,
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
            batch: None,
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
            batch: None,
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
                .next_offset,
            0
        );
        assert_eq!(
            initial
                .handle_resumable_file_chunk(&first, "peer".into())
                .await
                .unwrap()
                .next_offset,
            6
        );
        assert_eq!(
            initial
                .handle_resumable_file_chunk(&first, "peer".into())
                .await
                .unwrap()
                .next_offset,
            6
        );
        drop(initial);

        let mut restored = SyncEngine::new();
        assert_eq!(
            restored
                .begin_file_receive(meta, &directory.join("different-name.bin"), "peer".into())
                .await
                .unwrap()
                .next_offset,
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

    #[tokio::test]
    async fn completed_transfer_shortcut_requires_matching_hash() {
        let directory = TestDirectory::new("completed-transfer-hash");
        let transfer_id = TransferId([71; 16]);
        let original = b"original";
        let original_meta = FileMeta {
            transfer_id: Some(transfer_id),
            name: "original.bin".into(),
            size: original.len() as u64,
            hash: blake3::hash(original).to_hex().to_string(),
            chunk_size: FILE_CHUNK_SIZE as u32,
            batch: None,
        };
        let mut sync = SyncEngine::new();
        sync.begin_file_receive(
            original_meta,
            &directory.path().join("original.bin"),
            "peer".into(),
        )
        .await
        .unwrap();
        sync.handle_resumable_file_chunk(
            &FileChunkPayload {
                transfer_id,
                offset: 0,
                data: original.to_vec(),
            },
            "peer".into(),
        )
        .await
        .unwrap();

        let replacement = b"replaced";
        let progress = sync
            .begin_file_receive(
                FileMeta {
                    transfer_id: Some(transfer_id),
                    name: "replacement.bin".into(),
                    size: replacement.len() as u64,
                    hash: blake3::hash(replacement).to_hex().to_string(),
                    chunk_size: FILE_CHUNK_SIZE as u32,
                    batch: None,
                },
                &directory.path().join("replacement.bin"),
                "peer".into(),
            )
            .await
            .unwrap();
        assert_eq!(progress.next_offset, 0);
    }

    #[tokio::test]
    async fn resumed_file_is_verified_before_commit() {
        let directory = TestDirectory::new("resumed-file-verification");
        let transfer_id = TransferId([72; 16]);
        let data = b"first-second";
        let meta = FileMeta {
            transfer_id: Some(transfer_id),
            name: "received.bin".into(),
            size: data.len() as u64,
            hash: blake3::hash(data).to_hex().to_string(),
            chunk_size: FILE_CHUNK_SIZE as u32,
            batch: None,
        };
        let mut initial = SyncEngine::new();
        initial
            .begin_file_receive(
                meta.clone(),
                &directory.path().join("received.bin"),
                "peer".into(),
            )
            .await
            .unwrap();
        initial
            .handle_resumable_file_chunk(
                &FileChunkPayload {
                    transfer_id,
                    offset: 0,
                    data: b"first-".to_vec(),
                },
                "peer".into(),
            )
            .await
            .unwrap();
        initial.suspend_receive("peer");

        let platform = Arc::new(TestPlatform::default());
        let mut restored = SyncEngine::new();
        restored.set_platform(platform.clone());
        let progress = restored
            .begin_file_receive(meta, &directory.path().join("different.bin"), "peer".into())
            .await
            .unwrap();
        assert_eq!(progress.next_offset, 6);
        let progress = restored
            .handle_resumable_file_chunk(
                &FileChunkPayload {
                    transfer_id,
                    offset: 6,
                    data: b"second".to_vec(),
                },
                "peer".into(),
            )
            .await
            .unwrap();
        assert!(platform.received().is_empty());
        let verified = progress.completed.unwrap().verify_hash().unwrap();
        restored.commit_received_file("peer", verified).unwrap();
        assert_eq!(platform.received().len(), 1);
    }

    #[tokio::test]
    async fn corrupt_resumed_file_fails_deferred_verification() {
        let directory = TestDirectory::new("corrupt-resumed-file");
        let transfer_id = TransferId([73; 16]);
        let expected = b"first-second";
        let meta = FileMeta {
            transfer_id: Some(transfer_id),
            name: "received.bin".into(),
            size: expected.len() as u64,
            hash: blake3::hash(expected).to_hex().to_string(),
            chunk_size: FILE_CHUNK_SIZE as u32,
            batch: None,
        };
        let mut initial = SyncEngine::new();
        initial
            .begin_file_receive(
                meta.clone(),
                &directory.path().join("received.bin"),
                "peer".into(),
            )
            .await
            .unwrap();
        initial
            .handle_resumable_file_chunk(
                &FileChunkPayload {
                    transfer_id,
                    offset: 0,
                    data: b"wrong-".to_vec(),
                },
                "peer".into(),
            )
            .await
            .unwrap();
        initial.suspend_receive("peer");

        let mut restored = SyncEngine::new();
        restored
            .begin_file_receive(meta, &directory.path().join("different.bin"), "peer".into())
            .await
            .unwrap();
        let progress = restored
            .handle_resumable_file_chunk(
                &FileChunkPayload {
                    transfer_id,
                    offset: 6,
                    data: b"second".to_vec(),
                },
                "peer".into(),
            )
            .await
            .unwrap();
        let pending = progress.completed.unwrap();
        let path = pending.path().to_path_buf();
        assert!(pending
            .verify_hash()
            .unwrap_err()
            .contains("checksum mismatch"));
        assert!(path.is_file());
    }

    #[test]
    fn expired_transfer_cleanup_removes_orphans_but_preserves_recent_and_nested_files() {
        let directory = TestDirectory::new("expired-transfer-cleanup");
        let old_plaintext = directory.path().join("hash-old.txt");
        let recent_plaintext = directory.path().join("hash-recent.txt");
        let old_partial = directory.path().join("transfer.part");
        let nested_directory = directory.path().join("clipboard-files");
        let nested_file = nested_directory.join("managed.txt");
        std::fs::create_dir(&nested_directory).unwrap();
        for path in [
            &old_plaintext,
            &recent_plaintext,
            &old_partial,
            &nested_file,
        ] {
            std::fs::write(path, b"data").unwrap();
        }
        let old = SystemTime::now() - Duration::from_secs(2 * 60 * 60);
        for path in [&old_plaintext, &old_partial, &nested_file] {
            let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
            file.set_times(std::fs::FileTimes::new().set_modified(old))
                .unwrap();
        }

        super::cleanup_expired_transfers_in(directory.path(), Duration::from_secs(60 * 60));

        assert!(!old_plaintext.exists());
        assert!(!old_partial.exists());
        assert!(recent_plaintext.exists());
        assert!(nested_file.exists());
    }
}
