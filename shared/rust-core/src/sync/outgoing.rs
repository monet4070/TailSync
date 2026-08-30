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
    #[serde(default)]
    pub selection_id: Option<TransferId>,
    #[serde(default)]
    pub completed_peers: Vec<String>,
    #[serde(default)]
    pub local_history_saved: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

impl PersistedOutgoingBatch {
    pub fn batch_id(&self) -> TransferId {
        self.manifest.batch_id
    }

    pub fn pending_peers<'a>(&'a self, candidates: &'a [String]) -> Vec<&'a String> {
        candidates
            .iter()
            .filter(|peer| self.peers.iter().any(|known| known == *peer))
            .filter(|peer| !self.completed_peers.iter().any(|done| done == *peer))
            .collect()
    }

    pub fn is_peer_completed(&self, peer: &str) -> bool {
        self.completed_peers
            .iter()
            .any(|completed| completed == peer)
    }

    pub fn all_peers_completed(&self) -> bool {
        self.peers.iter().all(|peer| self.is_peer_completed(peer))
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
            .filter(|peer| !peer.is_empty())
            .cloned()
            .collect(),
        selection_id,
        completed_peers: Vec::new(),
        local_history_saved: false,
        created_at: timestamp,
        updated_at: timestamp,
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

fn read_outgoing_batch_at(
    directory: &Path,
    batch_id: TransferId,
) -> Result<PersistedOutgoingBatch, String> {
    let path = batch_path(directory, batch_id);
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

pub fn mark_outgoing_peer_completed(batch_id: TransferId, peer: &str) -> Result<(), String> {
    let directory = outgoing_dir();
    let mut batch = read_outgoing_batch_at(&directory, batch_id)?;
    if !batch.peers.iter().any(|known| known == peer) {
        return Err(format!("peer {peer} is not part of outgoing batch"));
    }
    if !batch.is_peer_completed(peer) {
        batch.completed_peers.push(peer.to_string());
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
        if timestamp.saturating_sub(batch.updated_at) > INCOMPLETE_TRANSFER_RETENTION_SECONDS as i64
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
        if timestamp.saturating_sub(selection.updated_at)
            > INCOMPLETE_TRANSFER_RETENTION_SECONDS as i64
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
                .pending_peers(&["peer-a".into(), "peer-b".into()])
                .len(),
            2
        );

        restored.completed_peers.push("peer-a".into());
        write_outgoing_batch_at(&journal_dir, &mut restored).unwrap();
        let restored = load_outgoing_batches_at(&journal_dir).pop().unwrap();
        assert!(restored.is_peer_completed("peer-a"));
        assert_eq!(
            restored.pending_peers(&["peer-a".into(), "peer-b".into()]),
            vec![&"peer-b".to_string()]
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
        };
        let path = selection_path(&journal_dir, selection_id);
        crate::private_fs::write_private_file(&path, &serde_json::to_vec(&selection).unwrap())
            .unwrap();
        let restored = load_outgoing_selections_at(&journal_dir);
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].selection_id, selection_id);
        assert_eq!(restored[0].generation, 9);

        selection.updated_at = now() - INCOMPLETE_TRANSFER_RETENTION_SECONDS as i64 - 1;
        fs::write(&path, serde_json::to_vec(&selection).unwrap()).unwrap();
        assert!(load_outgoing_selections_at(&journal_dir).is_empty());
        assert!(!path.exists());
        let _ = fs::remove_dir_all(journal_dir);
    }
}
