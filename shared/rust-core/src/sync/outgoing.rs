// Durable outbound file-batch journal.
//
// A prepared file batch used to live only in the clipboard task's heap. That
// made a process restart indistinguishable from a new clipboard event: the
// receiver could keep a partial `.part`, but the sender had lost the manifest,
// transfer IDs, and source paths needed to continue it. This module keeps the
// minimum private metadata required to replay the same transaction.

use super::*;
use crate::db;

const OUTGOING_BATCH_VERSION: u8 = 1;
const OUTGOING_BATCH_SUFFIX: &str = ".outgoing.json";
const OUTGOING_SELECTION_SUFFIX: &str = ".outgoing-pending.json";
const OUTGOING_RETRY_DELAYS_SECONDS: [i64; 5] = [2, 10, 30, 120, 600];
const OUTGOING_ERROR_MAX_BYTES: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum OutgoingTransferKey {
    Selection(TransferId),
    Batch(TransferId),
}

static ACTIVE_OUTGOING_TRANSFERS: LazyLock<std::sync::Mutex<HashSet<OutgoingTransferKey>>> =
    LazyLock::new(|| std::sync::Mutex::new(HashSet::new()));

/// In-process lease for one durable outgoing selection or prepared batch.
/// The journal remains the cross-process authority; this lease only prevents
/// the live sender and recovery worker from replaying the same work together.
#[derive(Debug)]
pub struct OutgoingTransferClaim {
    key: OutgoingTransferKey,
}

impl Drop for OutgoingTransferClaim {
    fn drop(&mut self) {
        ACTIVE_OUTGOING_TRANSFERS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&self.key);
    }
}

fn try_claim_outgoing(key: OutgoingTransferKey) -> Option<OutgoingTransferClaim> {
    let mut active = ACTIVE_OUTGOING_TRANSFERS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if active.insert(key) {
        Some(OutgoingTransferClaim { key })
    } else {
        None
    }
}

pub fn try_claim_outgoing_selection(selection_id: TransferId) -> Option<OutgoingTransferClaim> {
    try_claim_outgoing(OutgoingTransferKey::Selection(selection_id))
}

pub fn try_claim_outgoing_batch(batch_id: TransferId) -> Option<OutgoingTransferClaim> {
    try_claim_outgoing(OutgoingTransferKey::Batch(batch_id))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedOutgoingSelection {
    pub version: u8,
    pub selection_id: TransferId,
    pub paths: Vec<PathBuf>,
    pub generation: u64,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default)]
    pub attempt_count: u32,
    #[serde(default)]
    pub next_attempt_at: i64,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub last_notified_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedOutgoingFile {
    pub path: PathBuf,
    pub modified_nanos: u128,
    pub entry: FileBatchEntry,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedOutgoingBatch {
    pub version: u8,
    pub manifest: FileBatchManifest,
    pub files: Vec<PersistedOutgoingFile>,
    pub peers: Vec<String>,
    /// Stable authenticated peer identities aligned with `peers`.
    /// Empty entries are legacy hostname-only records.
    #[serde(default)]
    pub peer_fingerprints: Vec<String>,
    #[serde(default)]
    pub selection_id: Option<TransferId>,
    #[serde(default)]
    pub completed_peers: Vec<String>,
    /// Stable identities aligned with `completed_peers`.
    #[serde(default)]
    pub completed_peer_fingerprints: Vec<String>,
    #[serde(default)]
    pub local_history_saved: bool,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default)]
    pub attempt_count: u32,
    #[serde(default)]
    pub next_attempt_at: i64,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub last_notified_at: i64,
}

impl PersistedOutgoingBatch {
    pub fn batch_id(&self) -> TransferId {
        self.manifest.batch_id
    }

    pub fn pending_peers<'a>(&'a self, candidates: &'a [(String, String)]) -> Vec<&'a String> {
        candidates
            .iter()
            .filter_map(|(hostname, fingerprint)| {
                let index = self.peer_index(hostname, fingerprint)?;
                (!self.is_peer_completed_at(index)).then_some(hostname)
            })
            .collect()
    }

    pub fn is_peer_completed(&self, peer: &str) -> bool {
        self.peers
            .iter()
            .position(|known| known == peer)
            .is_some_and(|index| self.is_peer_completed_at(index))
    }

    pub fn all_peers_completed(&self) -> bool {
        !self.peers.is_empty()
            && (0..self.peers.len()).all(|index| self.is_peer_completed_at(index))
    }

    fn peer_index(&self, hostname: &str, fingerprint: &str) -> Option<usize> {
        self.peers.iter().enumerate().find_map(|(index, known)| {
            let known_fingerprint = self
                .peer_fingerprints
                .get(index)
                .filter(|fingerprint| !fingerprint.is_empty());
            if let Some(known_fingerprint) = known_fingerprint {
                (!fingerprint.is_empty() && known_fingerprint == fingerprint).then_some(index)
            } else {
                (known == hostname).then_some(index)
            }
        })
    }

    fn is_peer_completed_at(&self, index: usize) -> bool {
        let Some(hostname) = self.peers.get(index) else {
            return false;
        };
        let fingerprint = self
            .peer_fingerprints
            .get(index)
            .filter(|fingerprint| !fingerprint.is_empty());
        if let Some(fingerprint) = fingerprint {
            self.completed_peer_fingerprints
                .iter()
                .any(|completed| completed == fingerprint)
                || (self.completed_peer_fingerprints.is_empty()
                    && self
                        .completed_peers
                        .iter()
                        .any(|completed| completed == hostname))
        } else {
            self.completed_peers
                .iter()
                .any(|completed| completed == hostname)
        }
    }

    pub fn prepared_file_batch(&self) -> Result<PreparedFileBatch, String> {
        if self.version != OUTGOING_BATCH_VERSION {
            return Err(format!(
                "unsupported outgoing file batch journal version {}",
                self.version
            ));
        }
        self.manifest
            .validate()
            .map_err(|error| error.to_string())?;
        if self.files.len() != self.manifest.files.len() {
            return Err("outgoing file batch journal has an invalid file count".to_string());
        }
        let files = self
            .files
            .iter()
            .enumerate()
            .map(|(index, saved)| {
                if saved.entry != self.manifest.files[index] {
                    return Err(format!(
                        "outgoing file batch journal entry {index} does not match its manifest"
                    ));
                }
                Ok(PreparedFile {
                    path: saved.path.clone(),
                    modified_nanos: saved.modified_nanos,
                    entry: saved.entry.clone(),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(PreparedFileBatch {
            manifest: self.manifest.clone(),
            files,
        })
    }
}

fn outgoing_dir() -> PathBuf {
    db::get_data_dir().join("outgoing-transfers")
}

fn batch_path(directory: &Path, batch_id: TransferId) -> PathBuf {
    directory.join(format!("{}{}", batch_id.as_hex(), OUTGOING_BATCH_SUFFIX))
}

fn selection_path(directory: &Path, selection_id: TransferId) -> PathBuf {
    directory.join(format!(
        "{}{}",
        selection_id.as_hex(),
        OUTGOING_SELECTION_SUFFIX
    ))
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn validate_for_persistence(prepared: &PreparedFileBatch) -> Result<(), String> {
    prepared
        .manifest
        .validate()
        .map_err(|error| error.to_string())?;
    if prepared.files.len() != prepared.manifest.files.len() {
        return Err("prepared file batch has an invalid file count".to_string());
    }
    if prepared
        .files
        .iter()
        .zip(&prepared.manifest.files)
        .any(|(file, entry)| file.entry != *entry)
    {
        return Err("prepared file batch entries do not match their manifest".to_string());
    }
    Ok(())
}

fn persist_outgoing_batch_at(
    directory: &Path,
    prepared: &PreparedFileBatch,
    peers: &[String],
    selection_id: Option<TransferId>,
) -> Result<(), String> {
    let targets = peers
        .iter()
        .map(|hostname| (hostname.clone(), String::new()))
        .collect::<Vec<_>>();
    persist_outgoing_batch_at_with_identities(directory, prepared, &targets, selection_id)
}

fn persist_outgoing_batch_at_with_identities(
    directory: &Path,
    prepared: &PreparedFileBatch,
    peers: &[(String, String)],
    selection_id: Option<TransferId>,
) -> Result<(), String> {
    validate_for_persistence(prepared)?;
    crate::private_fs::create_private_dir_all(directory).map_err(|error| error.to_string())?;
    let timestamp = now();
    let journal = PersistedOutgoingBatch {
        version: OUTGOING_BATCH_VERSION,
        manifest: prepared.manifest.clone(),
        files: prepared
            .files
            .iter()
            .map(|file| PersistedOutgoingFile {
                path: file.path.clone(),
                modified_nanos: file.modified_nanos,
                entry: file.entry.clone(),
            })
            .collect(),
        peers: peers
            .iter()
            .filter(|(hostname, _)| !hostname.is_empty())
            .map(|(hostname, _)| hostname.clone())
            .collect(),
        peer_fingerprints: peers
            .iter()
            .filter(|(hostname, _)| !hostname.is_empty())
            .map(|(_, fingerprint)| fingerprint.clone())
            .collect(),
        selection_id,
        completed_peers: Vec::new(),
        completed_peer_fingerprints: Vec::new(),
        local_history_saved: false,
        created_at: timestamp,
        updated_at: timestamp,
        attempt_count: 0,
        next_attempt_at: 0,
        last_error: None,
        last_notified_at: 0,
    };
    let data = serde_json::to_vec_pretty(&journal).map_err(|error| error.to_string())?;
    crate::private_fs::write_private_file(&batch_path(directory, prepared.manifest.batch_id), &data)
        .map_err(|error| error.to_string())
}

pub fn persist_outgoing_batch(
    prepared: &PreparedFileBatch,
    peers: &[String],
) -> Result<(), String> {
    persist_outgoing_batch_at(&outgoing_dir(), prepared, peers, None)
}

pub fn persist_outgoing_batch_for_selection(
    prepared: &PreparedFileBatch,
    peers: &[String],
    selection_id: TransferId,
) -> Result<(), String> {
    persist_outgoing_batch_at(&outgoing_dir(), prepared, peers, Some(selection_id))
}

pub fn persist_outgoing_batch_with_identities(
    prepared: &PreparedFileBatch,
    peers: &[(String, String)],
) -> Result<(), String> {
    persist_outgoing_batch_at_with_identities(&outgoing_dir(), prepared, peers, None)
}

pub fn persist_outgoing_batch_for_selection_with_identities(
    prepared: &PreparedFileBatch,
    peers: &[(String, String)],
    selection_id: TransferId,
) -> Result<(), String> {
    persist_outgoing_batch_at_with_identities(&outgoing_dir(), prepared, peers, Some(selection_id))
}

/// Populate the target list for a journal that was created while no eligible
/// peer was available. A batch with an explicit target list keeps that list
/// stable across retries; an empty list means the initial clipboard event had
/// nowhere to go and may adopt the currently eligible trusted peers on the
/// next recovery pass.
pub fn enroll_outgoing_batch_peers(
    batch_id: TransferId,
    peers: &[(String, String)],
) -> Result<(), String> {
    let directory = outgoing_dir();
    enroll_outgoing_batch_peers_at(&directory, batch_id, peers)
}

fn enroll_outgoing_batch_peers_at(
    directory: &Path,
    batch_id: TransferId,
    peers: &[(String, String)],
) -> Result<(), String> {
    let mut batch = read_outgoing_batch_at(directory, batch_id)?;
    if !batch.peers.is_empty() {
        return Ok(());
    }
    for (hostname, fingerprint) in peers {
        if hostname.is_empty() {
            continue;
        }
        batch.peers.push(hostname.clone());
        batch.peer_fingerprints.push(fingerprint.clone());
    }
    if !batch.peers.is_empty() {
        write_outgoing_batch_at(directory, &mut batch)?;
    }
    Ok(())
}

pub fn persist_outgoing_selection(
    paths: &[PathBuf],
    generation: u64,
) -> Result<TransferId, String> {
    if paths.is_empty() || paths.len() > MAX_FILE_BATCH_COUNT {
        return Err(format!(
            "outgoing file selection must contain between 1 and {MAX_FILE_BATCH_COUNT} files"
        ));
    }
    let directory = outgoing_dir();
    crate::private_fs::create_private_dir_all(&directory).map_err(|error| error.to_string())?;
    let selection_id = TransferId::random();
    let timestamp = now();
    let selection = PersistedOutgoingSelection {
        version: OUTGOING_BATCH_VERSION,
        selection_id,
        paths: paths.to_vec(),
        generation,
        created_at: timestamp,
        updated_at: timestamp,
        attempt_count: 0,
        next_attempt_at: 0,
        last_error: None,
        last_notified_at: 0,
    };
    let data = serde_json::to_vec_pretty(&selection).map_err(|error| error.to_string())?;
    crate::private_fs::write_private_file(&selection_path(&directory, selection_id), &data)
        .map_err(|error| error.to_string())?;
    Ok(selection_id)
}

pub fn remove_outgoing_selection(selection_id: TransferId) -> Result<(), String> {
    let path = selection_path(&outgoing_dir(), selection_id);
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

pub fn outgoing_retry_due(next_attempt_at: i64) -> bool {
    next_attempt_at <= now()
}

fn retry_delay_seconds(attempt_count: u32) -> i64 {
    let index = usize::try_from(attempt_count.saturating_sub(1))
        .unwrap_or(usize::MAX)
        .min(OUTGOING_RETRY_DELAYS_SECONDS.len() - 1);
    OUTGOING_RETRY_DELAYS_SECONDS[index]
}

fn normalized_outgoing_error(error: &str) -> String {
    error.chars().take(OUTGOING_ERROR_MAX_BYTES).collect()
}

fn retry_state(
    attempt_count: &mut u32,
    next_attempt_at: &mut i64,
    last_error: &mut Option<String>,
    last_notified_at: &mut i64,
    error: &str,
) -> bool {
    let now = now();
    let error = normalized_outgoing_error(error);
    let error_changed = last_error.as_deref() != Some(error.as_str());
    let previous_attempt = *attempt_count;
    *attempt_count = attempt_count.saturating_add(1);
    *next_attempt_at = now.saturating_add(retry_delay_seconds(*attempt_count));
    let notify = error_changed
        || now.saturating_sub(*last_notified_at)
            >= retry_delay_seconds(previous_attempt.saturating_add(1));
    *last_error = Some(error);
    if notify {
        *last_notified_at = now;
    }
    notify
}

fn read_outgoing_batch_at(
    directory: &Path,
    batch_id: TransferId,
) -> Result<PersistedOutgoingBatch, String> {
    let path = batch_path(directory, batch_id);
    let data = fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    serde_json::from_slice(&data).map_err(|error| format!("{}: {error}", path.display()))
}

fn read_outgoing_selection_at(
    directory: &Path,
    selection_id: TransferId,
) -> Result<PersistedOutgoingSelection, String> {
    let path = selection_path(directory, selection_id);
    let data = fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    serde_json::from_slice(&data).map_err(|error| format!("{}: {error}", path.display()))
}

fn write_outgoing_batch_at(
    directory: &Path,
    batch: &mut PersistedOutgoingBatch,
) -> Result<(), String> {
    batch.updated_at = now();
    let data = serde_json::to_vec_pretty(batch).map_err(|error| error.to_string())?;
    crate::private_fs::write_private_file(&batch_path(directory, batch.batch_id()), &data)
        .map_err(|error| error.to_string())
}

fn write_outgoing_selection_at(
    directory: &Path,
    selection: &mut PersistedOutgoingSelection,
) -> Result<(), String> {
    selection.updated_at = now();
    let data = serde_json::to_vec_pretty(selection).map_err(|error| error.to_string())?;
    crate::private_fs::write_private_file(&selection_path(directory, selection.selection_id), &data)
        .map_err(|error| error.to_string())
}

pub fn schedule_outgoing_selection_retry(
    selection_id: TransferId,
    error: &str,
) -> Result<bool, String> {
    let directory = outgoing_dir();
    let mut selection = read_outgoing_selection_at(&directory, selection_id)?;
    let notify = retry_state(
        &mut selection.attempt_count,
        &mut selection.next_attempt_at,
        &mut selection.last_error,
        &mut selection.last_notified_at,
        error,
    );
    write_outgoing_selection_at(&directory, &mut selection)?;
    Ok(notify)
}

pub fn schedule_outgoing_batch_retry(batch_id: TransferId, error: &str) -> Result<bool, String> {
    let directory = outgoing_dir();
    let mut batch = read_outgoing_batch_at(&directory, batch_id)?;
    let notify = retry_state(
        &mut batch.attempt_count,
        &mut batch.next_attempt_at,
        &mut batch.last_error,
        &mut batch.last_notified_at,
        error,
    );
    write_outgoing_batch_at(&directory, &mut batch)?;
    Ok(notify)
}

pub fn mark_outgoing_peer_completed(batch_id: TransferId, peer: &str) -> Result<(), String> {
    mark_outgoing_peer_completed_with_identity(batch_id, peer, "")
}

pub fn mark_outgoing_peer_completed_with_identity(
    batch_id: TransferId,
    peer: &str,
    fingerprint: &str,
) -> Result<(), String> {
    let directory = outgoing_dir();
    let mut batch = read_outgoing_batch_at(&directory, batch_id)?;
    let Some(index) = batch.peer_index(peer, fingerprint) else {
        return Err(format!("peer {peer} is not part of outgoing batch"));
    };
    if batch
        .peer_fingerprints
        .get(index)
        .is_some_and(|known| !known.is_empty() && known != fingerprint)
    {
        return Err(format!(
            "peer {peer} identity does not match outgoing batch"
        ));
    }
    if !batch.is_peer_completed_at(index) {
        batch.completed_peers.push(peer.to_string());
        batch
            .completed_peer_fingerprints
            .push(fingerprint.to_string());
    }
    write_outgoing_batch_at(&directory, &mut batch)
}

pub fn mark_outgoing_history_saved(batch_id: TransferId) -> Result<(), String> {
    let directory = outgoing_dir();
    let mut batch = read_outgoing_batch_at(&directory, batch_id)?;
    batch.local_history_saved = true;
    write_outgoing_batch_at(&directory, &mut batch)
}

pub fn load_outgoing_batches() -> Vec<PersistedOutgoingBatch> {
    load_outgoing_batches_at(&outgoing_dir())
}

pub fn load_outgoing_selections() -> Vec<PersistedOutgoingSelection> {
    load_outgoing_selections_at(&outgoing_dir())
}

pub(crate) fn cleanup_expired_outgoing_batches() {
    let _ = load_outgoing_batches();
    let _ = load_outgoing_selections();
}

fn load_outgoing_batches_at(directory: &Path) -> Vec<PersistedOutgoingBatch> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let timestamp = now();
    let mut batches = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !metadata.is_file()
            || path.extension().and_then(|extension| extension.to_str()) != Some("json")
            || !path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(OUTGOING_BATCH_SUFFIX))
        {
            continue;
        }
        let data = match fs::read(&path) {
            Ok(data) => data,
            Err(error) => {
                warn!(
                    "Could not read outgoing file batch journal {}: {error}",
                    path.display()
                );
                continue;
            }
        };
        let batch = match serde_json::from_slice::<PersistedOutgoingBatch>(&data) {
            Ok(batch) => batch,
            Err(error) => {
                warn!(
                    "Ignoring invalid outgoing file batch journal {}: {error}",
                    path.display()
                );
                continue;
            }
        };
        let retention_anchor = if batch.created_at > 0 {
            batch.created_at
        } else {
            batch.updated_at
        };
        if timestamp.saturating_sub(retention_anchor) > INCOMPLETE_TRANSFER_RETENTION_SECONDS as i64
        {
            let _ = fs::remove_file(&path);
            continue;
        }
        if let Err(error) = batch.prepared_file_batch() {
            warn!(
                "Ignoring unusable outgoing file batch journal {}: {error}",
                path.display()
            );
            continue;
        }
        batches.push(batch);
    }
    batches.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.batch_id().as_hex().cmp(&right.batch_id().as_hex()))
    });
    batches
}

fn load_outgoing_selections_at(directory: &Path) -> Vec<PersistedOutgoingSelection> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let timestamp = now();
    let mut selections = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !metadata.is_file()
            || path.extension().and_then(|extension| extension.to_str()) != Some("json")
            || !path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(OUTGOING_SELECTION_SUFFIX))
        {
            continue;
        }
        let data = match fs::read(&path) {
            Ok(data) => data,
            Err(error) => {
                warn!(
                    "Could not read pending outgoing file selection {}: {error}",
                    path.display()
                );
                continue;
            }
        };
        let selection = match serde_json::from_slice::<PersistedOutgoingSelection>(&data) {
            Ok(selection) => selection,
            Err(error) => {
                warn!(
                    "Ignoring invalid pending outgoing file selection {}: {error}",
                    path.display()
                );
                continue;
            }
        };
        if selection.version != OUTGOING_BATCH_VERSION
            || selection.paths.is_empty()
            || selection.paths.len() > MAX_FILE_BATCH_COUNT
        {
            warn!(
                "Ignoring malformed pending outgoing file selection {}",
                path.display()
            );
            continue;
        }
        let retention_anchor = if selection.created_at > 0 {
            selection.created_at
        } else {
            selection.updated_at
        };
        if timestamp.saturating_sub(retention_anchor) > INCOMPLETE_TRANSFER_RETENTION_SECONDS as i64
        {
            let _ = fs::remove_file(&path);
            continue;
        }
        selections.push(selection);
    }
    selections.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.selection_id.as_hex().cmp(&right.selection_id.as_hex()))
    });
    selections
}

pub fn remove_outgoing_batch(batch_id: TransferId) -> Result<(), String> {
    let path = batch_path(&outgoing_dir(), batch_id);
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_directory(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "tailsync-outgoing-{label}-{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        ))
    }

    fn prepared(directory: &Path) -> PreparedFileBatch {
        let path = directory.join("payload.bin");
        fs::create_dir_all(directory).unwrap();
        fs::write(&path, b"payload").unwrap();
        prepare_file_batch(vec![path], 7).unwrap()
    }

    #[test]
    fn live_claim_prevents_duplicate_recovery_until_the_sender_finishes() {
        let batch_id = TransferId::random();
        let first = try_claim_outgoing_batch(batch_id).expect("first sender should claim batch");
        assert!(try_claim_outgoing_batch(batch_id).is_none());
        drop(first);
        assert!(try_claim_outgoing_batch(batch_id).is_some());

        let selection_id = TransferId::random();
        let first = try_claim_outgoing_selection(selection_id)
            .expect("first sender should claim pending selection");
        assert!(try_claim_outgoing_selection(selection_id).is_none());
        drop(first);
        assert!(try_claim_outgoing_selection(selection_id).is_some());
    }

    #[test]
    fn journal_round_trip_preserves_ids_paths_and_peer_state() {
        let source = test_directory("round-trip-source");
        let journal_dir = test_directory("round-trip-journal");
        let batch = prepared(&source);
        let selection_id = TransferId::random();
        persist_outgoing_batch_at(
            &journal_dir,
            &batch,
            &["peer-a".into(), "peer-b".into()],
            Some(selection_id),
        )
        .unwrap();
        let mut restored = load_outgoing_batches_at(&journal_dir).pop().unwrap();
        assert_eq!(restored.batch_id(), batch.manifest.batch_id);
        assert_eq!(restored.selection_id, Some(selection_id));
        assert_eq!(
            restored.prepared_file_batch().unwrap().files[0].path,
            batch.files[0].path
        );
        assert_eq!(
            restored
                .pending_peers(&[
                    ("peer-a".into(), String::new()),
                    ("peer-b".into(), String::new()),
                ])
                .len(),
            2
        );

        restored.completed_peers.push("peer-a".into());
        write_outgoing_batch_at(&journal_dir, &mut restored).unwrap();
        let restored = load_outgoing_batches_at(&journal_dir).pop().unwrap();
        assert!(restored.is_peer_completed("peer-a"));
        assert_eq!(
            restored.pending_peers(&[
                ("peer-a".into(), String::new()),
                ("peer-b".into(), String::new()),
            ]),
            vec![&"peer-b".to_string()]
        );

        let _ = fs::remove_dir_all(source);
        let _ = fs::remove_dir_all(journal_dir);
    }

    #[test]
    fn peer_rename_is_matched_by_the_persisted_fingerprint() {
        let source = test_directory("peer-rename-source");
        let journal_dir = test_directory("peer-rename-journal");
        let batch = prepared(&source);
        let targets = vec![("old-name".into(), "peer-key".into())];
        persist_outgoing_batch_at_with_identities(&journal_dir, &batch, &targets, None).unwrap();

        let mut restored = load_outgoing_batches_at(&journal_dir).pop().unwrap();
        assert_eq!(
            restored.pending_peers(&[("new-name".into(), "peer-key".into())]),
            vec![&"new-name".to_string()]
        );
        assert!(restored
            .pending_peers(&[("new-name".into(), "different-key".into())])
            .is_empty());

        restored.completed_peers.push("new-name".into());
        restored.completed_peer_fingerprints.push("peer-key".into());
        assert!(restored.all_peers_completed());

        let _ = fs::remove_dir_all(source);
        let _ = fs::remove_dir_all(journal_dir);
    }

    #[test]
    fn an_outgoing_batch_without_peers_is_not_marked_complete() {
        let source = test_directory("no-peers-source");
        let batch = prepared(&source);
        let journal = PersistedOutgoingBatch {
            version: OUTGOING_BATCH_VERSION,
            manifest: batch.manifest,
            files: batch
                .files
                .into_iter()
                .map(|file| PersistedOutgoingFile {
                    path: file.path,
                    modified_nanos: file.modified_nanos,
                    entry: file.entry,
                })
                .collect(),
            peers: Vec::new(),
            peer_fingerprints: Vec::new(),
            selection_id: None,
            completed_peers: Vec::new(),
            completed_peer_fingerprints: Vec::new(),
            local_history_saved: false,
            created_at: now(),
            updated_at: now(),
            attempt_count: 0,
            next_attempt_at: 0,
            last_error: None,
            last_notified_at: 0,
        };
        assert!(!journal.all_peers_completed());
        let _ = fs::remove_dir_all(source);
    }

    #[test]
    fn an_outgoing_batch_without_peers_adopts_recovered_targets_once() {
        let source = test_directory("enroll-source");
        let journal_dir = test_directory("enroll-journal");
        let batch = prepared(&source);
        persist_outgoing_batch_at_with_identities(&journal_dir, &batch, &[], None).unwrap();

        enroll_outgoing_batch_peers_at(
            &journal_dir,
            batch.manifest.batch_id,
            &[
                ("peer-a".into(), "key-a".into()),
                ("peer-b".into(), "key-b".into()),
            ],
        )
        .unwrap();
        enroll_outgoing_batch_peers_at(
            &journal_dir,
            batch.manifest.batch_id,
            &[("peer-c".into(), "key-c".into())],
        )
        .unwrap();

        let restored = load_outgoing_batches_at(&journal_dir).pop().unwrap();
        assert_eq!(restored.peers, ["peer-a", "peer-b"]);
        assert_eq!(restored.peer_fingerprints, ["key-a", "key-b"]);
        assert_eq!(
            restored.pending_peers(&[
                ("peer-a".into(), "key-a".into()),
                ("peer-b".into(), "key-b".into()),
            ]),
            vec![&"peer-a".to_string(), &"peer-b".to_string()]
        );

        let _ = fs::remove_dir_all(source);
        let _ = fs::remove_dir_all(journal_dir);
    }

    #[test]
    fn stale_journal_is_removed_without_touching_source_file() {
        let source = test_directory("stale-source");
        let journal_dir = test_directory("stale-journal");
        let batch = prepared(&source);
        persist_outgoing_batch_at(&journal_dir, &batch, &["peer".into()], None).unwrap();
        let path = batch_path(&journal_dir, batch.manifest.batch_id);
        let mut saved: PersistedOutgoingBatch =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        saved.created_at = now() - INCOMPLETE_TRANSFER_RETENTION_SECONDS as i64 - 1;
        saved.updated_at = now() - INCOMPLETE_TRANSFER_RETENTION_SECONDS as i64 - 1;
        fs::write(&path, serde_json::to_vec(&saved).unwrap()).unwrap();

        assert!(load_outgoing_batches_at(&journal_dir).is_empty());
        assert!(source.join("payload.bin").is_file());
        assert!(!path.exists());

        let _ = fs::remove_dir_all(source);
        let _ = fs::remove_dir_all(journal_dir);
    }

    #[test]
    fn pending_selection_round_trip_and_expiry_are_bounded() {
        let journal_dir = test_directory("selection");
        fs::create_dir_all(&journal_dir).unwrap();
        let selection_id = TransferId::random();
        let mut selection = PersistedOutgoingSelection {
            version: OUTGOING_BATCH_VERSION,
            selection_id,
            paths: vec![PathBuf::from("/tmp/source.bin")],
            generation: 9,
            created_at: now(),
            updated_at: now(),
            attempt_count: 0,
            next_attempt_at: 0,
            last_error: None,
            last_notified_at: 0,
        };
        let path = selection_path(&journal_dir, selection_id);
        crate::private_fs::write_private_file(&path, &serde_json::to_vec(&selection).unwrap())
            .unwrap();
        let restored = load_outgoing_selections_at(&journal_dir);
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].selection_id, selection_id);
        assert_eq!(restored[0].generation, 9);

        selection.updated_at = now() - INCOMPLETE_TRANSFER_RETENTION_SECONDS as i64 - 1;
        selection.created_at = now() - INCOMPLETE_TRANSFER_RETENTION_SECONDS as i64 - 1;
        fs::write(&path, serde_json::to_vec(&selection).unwrap()).unwrap();
        assert!(load_outgoing_selections_at(&journal_dir).is_empty());
        assert!(!path.exists());
        let _ = fs::remove_dir_all(journal_dir);
    }

    #[test]
    fn retry_state_applies_backoff_and_throttles_duplicate_notifications() {
        let mut attempt_count = 0;
        let mut next_attempt_at = 0;
        let mut last_error = None;
        let mut last_notified_at = now();

        assert!(retry_state(
            &mut attempt_count,
            &mut next_attempt_at,
            &mut last_error,
            &mut last_notified_at,
            "peer unavailable"
        ));
        assert_eq!(attempt_count, 1);
        assert!(next_attempt_at > now());
        assert_eq!(last_error.as_deref(), Some("peer unavailable"));

        last_notified_at = now();
        assert!(!retry_state(
            &mut attempt_count,
            &mut next_attempt_at,
            &mut last_error,
            &mut last_notified_at,
            "peer unavailable"
        ));
        assert_eq!(attempt_count, 2);
        assert!(retry_state(
            &mut attempt_count,
            &mut next_attempt_at,
            &mut last_error,
            &mut last_notified_at,
            "storage unavailable"
        ));
    }
}
