// Resumable file-transfer persistence (T321 extraction from sync.rs).
//
// JSON state streamed next to `{id}.part` files, plus the stale-partial
// cleanup that removes orphaned incoming files after the retention window.
// Everything here is pure file I/O on explicit paths; SyncEngine state
// stays in the parent module.

use super::*;
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PersistedTransfer {
    pub(crate) meta: FileMeta,
    pub(crate) source: String,
    pub(crate) final_path: PathBuf,
    pub(crate) updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PersistedIncomingBatch {
    pub(crate) source: String,
    pub(crate) manifest: FileBatchManifest,
    pub(crate) files: Vec<Option<ReceivedFile>>,
    /// Generation captured when the batch was admitted. Older sidecars
    /// default to zero, which safely prevents activating stale batches.
    #[serde(default)]
    pub(crate) local_generation: u64,
}

/// Resume-persistence failures (T352 migration). Display strings match the
/// previous io/serde message passthroughs exactly.
#[derive(Debug, Error)]
pub enum ResumeError {
    #[error("{0}")]
    Io(String),
    #[error("{0}")]
    Serde(String),
}

pub(crate) fn persist_transfer_state(
    state: &FileReceiveState,
    source: &str,
) -> Result<(), ResumeError> {
    let Some(path) = state.state_path.as_ref() else {
        return Ok(());
    };
    let saved = PersistedTransfer {
        meta: state.meta.clone(),
        source: source.to_string(),
        final_path: state.final_path.clone(),
        updated_at: chrono::Utc::now().timestamp(),
    };
    let encoded =
        serde_json::to_vec_pretty(&saved).map_err(|error| ResumeError::Serde(error.to_string()))?;
    crate::private_fs::write_private_file(path, &encoded)
        .map_err(|error| ResumeError::Io(error.to_string()))
}

pub(crate) fn persist_incoming_batch(
    path: &Path,
    batch: &PersistedIncomingBatch,
) -> Result<(), ResumeError> {
    crate::private_fs::write_private_file(
        path,
        &serde_json::to_vec_pretty(batch).map_err(|error| ResumeError::Serde(error.to_string()))?,
    )
    .map_err(|error| ResumeError::Io(error.to_string()))
}

pub(crate) fn restore_persisted_received_file(
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

/// Read a durable resumable-transfer offset after the in-memory receive state
/// was dropped. Only sidecars created for the authenticated source and exact
/// transfer ID are accepted; the partial file must be a regular file and may
/// never advertise bytes beyond the declared size.
pub(crate) fn persisted_transfer_offset(
    source: &str,
    transfer_id: TransferId,
    incoming_dir: &Path,
) -> Option<u64> {
    let state_path = incoming_dir.join(format!("{}.resume.json", transfer_id.as_hex()));
    let data = fs::read(state_path).ok()?;
    let saved = serde_json::from_slice::<PersistedTransfer>(&data).ok()?;
    if saved.source != source || saved.meta.transfer_id != Some(transfer_id) {
        return None;
    }
    let part_path = incoming_dir.join(format!("{}.part", transfer_id.as_hex()));
    let metadata = fs::symlink_metadata(part_path).ok()?;
    if !metadata.is_file() || metadata.len() > saved.meta.size {
        return None;
    }
    Some(metadata.len())
}

pub fn cleanup_expired_transfers() {
    let incoming = crate::db::get_incoming_dir();
    cleanup_expired_transfers_in(
        &incoming,
        Duration::from_secs(INCOMPLETE_TRANSFER_RETENTION_SECONDS),
    );
    super::outgoing::cleanup_expired_outgoing_batches();
}

pub(crate) fn cleanup_expired_transfers_in(incoming: &Path, retention: Duration) {
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
    use super::*;

    fn test_directory(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "tailsync-resume-{label}-{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        ))
    }

    #[test]
    fn persist_incoming_batch_writes_atomically() {
        let directory = test_directory("atomic-batch");
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("batch.batch.json");
        let batch = PersistedIncomingBatch {
            source: "peer".to_string(),
            manifest: FileBatchManifest {
                batch_id: TransferId::random(),
                generation: 0,
                total_bytes: 0,
                files: Vec::new(),
            },
            files: Vec::new(),
            local_generation: 0,
        };

        persist_incoming_batch(&path, &batch).unwrap();

        assert!(path.exists());
        assert!(!path.with_extension("json.tmp").exists());
        let restored: PersistedIncomingBatch =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(restored.source, "peer");
        fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn persist_transfer_state_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let directory = test_directory("private-transfer-state");
        fs::create_dir_all(&directory).unwrap();
        let state_path = directory.join("payload.resume.json");
        let tmp_path = directory.join("payload.part");
        let state = FileReceiveState {
            meta: FileMeta {
                transfer_id: Some(TransferId::random()),
                name: "payload.bin".to_string(),
                size: 0,
                hash: blake3::hash(b"").to_hex().to_string(),
                chunk_size: FILE_CHUNK_SIZE as u32,
                batch: None,
            },
            session_epoch: 1,
            tmp_path: tmp_path.clone(),
            final_path: directory.join("payload.bin"),
            state_path: Some(state_path.clone()),
            writer: BufWriter::new(File::create(tmp_path).unwrap()),
            hasher: blake3::Hasher::new(),
            received: 0,
            requires_full_hash: false,
        };

        persist_transfer_state(&state, "peer").unwrap();

        let mode = fs::metadata(&state_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn restore_persisted_received_file_accepts_matching_files() {
        let directory = test_directory("restore-match");
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("payload.bin"), b"payload").unwrap();
        let incoming = directory.canonicalize().unwrap();
        let hash = hash_source_file(&incoming.join("payload.bin")).unwrap();
        let saved = ReceivedFile {
            name: "payload.bin".to_string(),
            size: 7,
            hash: hash.clone(),
            path: PathBuf::from("payload.bin"),
        };
        let expected = FileBatchEntry {
            transfer_id: TransferId::random(),
            index: 0,
            name: "payload.bin".to_string(),
            source_parent: String::new(),
            size: 7,
            hash,
            chunk_size: FILE_CHUNK_SIZE as u32,
        };

        let restored = restore_persisted_received_file(&saved, &expected, &incoming).unwrap();

        assert_eq!(restored.name, "payload.bin");
        assert_eq!(restored.path, incoming.join("payload.bin"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn restore_persisted_received_file_rejects_mismatches_and_missing_files() {
        let directory = test_directory("restore-mismatch");
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("payload.bin"), b"payload").unwrap();
        let incoming = directory.canonicalize().unwrap();
        let hash = hash_source_file(&incoming.join("payload.bin")).unwrap();
        let expected = FileBatchEntry {
            transfer_id: TransferId::random(),
            index: 0,
            name: "payload.bin".to_string(),
            source_parent: String::new(),
            size: 7,
            hash: hash.clone(),
            chunk_size: FILE_CHUNK_SIZE as u32,
        };

        // Wrong name.
        assert!(restore_persisted_received_file(
            &ReceivedFile {
                name: "other.bin".to_string(),
                size: 7,
                hash: hash.clone(),
                path: PathBuf::from("payload.bin"),
            },
            &expected,
            &incoming,
        )
        .is_none());
        // Wrong size.
        assert!(restore_persisted_received_file(
            &ReceivedFile {
                name: "payload.bin".to_string(),
                size: 99,
                hash: hash.clone(),
                path: PathBuf::from("payload.bin"),
            },
            &expected,
            &incoming,
        )
        .is_none());
        // Wrong hash.
        assert!(restore_persisted_received_file(
            &ReceivedFile {
                name: "payload.bin".to_string(),
                size: 7,
                hash: "0".repeat(64),
                path: PathBuf::from("payload.bin"),
            },
            &expected,
            &incoming,
        )
        .is_none());
        // Missing file.
        assert!(restore_persisted_received_file(
            &ReceivedFile {
                name: "missing.bin".to_string(),
                size: 7,
                hash,
                path: PathBuf::from("missing.bin"),
            },
            &expected,
            &incoming,
        )
        .is_none());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn restore_persisted_received_file_rejects_links_outside_incoming() {
        let directory = test_directory("restore-escape");
        fs::create_dir_all(&directory).unwrap();
        let outside = test_directory("restore-escape-outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("payload.bin"), b"payload").unwrap();
        let incoming = directory.canonicalize().unwrap();
        let outside_file = outside.canonicalize().unwrap().join("payload.bin");
        let hash = hash_source_file(&outside_file).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside_file, directory.join("payload.bin")).unwrap();

        let restored = restore_persisted_received_file(
            &ReceivedFile {
                name: "payload.bin".to_string(),
                size: 7,
                hash: hash.clone(),
                path: PathBuf::from("payload.bin"),
            },
            &FileBatchEntry {
                transfer_id: TransferId::random(),
                index: 0,
                name: "payload.bin".to_string(),
                source_parent: String::new(),
                size: 7,
                hash,
                chunk_size: FILE_CHUNK_SIZE as u32,
            },
            &incoming,
        );
        // The symlink resolves outside `incoming`, so the restore must refuse
        // even though name/size/hash all match.
        assert!(restored.is_none());

        fs::remove_dir_all(directory).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }
}
