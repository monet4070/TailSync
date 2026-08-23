// Outbound file-batch preparation (T323 extraction from sync.rs).
//
// Pure validation, naming, and hashing for building a prepared batch from a
// user's file selection: metadata checks, collision-safe display names,
// source hashing, and manifest validation. SyncEngine receive state stays
// in the parent module.

use super::*;
use thiserror::Error;

pub(crate) fn default_file_chunk_size() -> u32 {
    FILE_CHUNK_SIZE as u32
}

/// Batch preparation and inbound metadata validation errors (T352
/// migration). The `Display` strings are the observable wire contract —
/// they reach the remote peer and the UI verbatim — and must stay stable.
#[derive(Debug, Error)]
pub enum PrepareError {
    #[error("This TailSync version requires the file_batch_v1 protocol")]
    ProtocolRequired,
    #[error("File exceeds the 1 GiB receive limit")]
    FileTooLarge,
    #[error("Invalid file name")]
    InvalidFileName,
    #[error("Invalid file chunk size")]
    InvalidChunkSize,
    #[error("A file batch must contain between 1 and {0} files")]
    BadBatchSize(usize),
    #[error("File batch exceeds the 1 GiB transfer limit")]
    BatchTooLarge,
    #[error("File batch indexes must be contiguous")]
    NonContiguousIndexes,
    #[error("File batch contains a duplicate transfer ID")]
    DuplicateTransferId,
    #[error("File batch contains an invalid transfer ID")]
    InvalidTransferId,
    #[error("File batch contains an invalid file name")]
    InvalidBatchFileName,
    #[error("File batch contains an invalid chunk size")]
    InvalidBatchChunkSize,
    #[error("File batch byte count overflowed")]
    ByteCountOverflow,
    #[error("File batch total does not match its manifest")]
    TotalMismatch,
    #[error("Select between 1 and {0} ordinary files")]
    BadSelection(usize),
    #[error("Cannot inspect {0}: {1}")]
    InspectFailed(PathBuf, String),
    #[error("The selection contains a symbolic link: {0}")]
    SymlinkSelected(PathBuf),
    #[error("The selection contains a folder or non-file item: {0}")]
    NonFileSelected(PathBuf),
    #[error("The selected file sizes overflowed")]
    SizeOverflow,
    #[error("The selected files exceed the 1 GiB batch limit")]
    SelectionTooLarge,
    #[error("{0} does not have a valid file name")]
    InvalidPathName(PathBuf),
    #[error("File batch index overflowed")]
    IndexOverflow,
    #[error("Cannot re-open {0}: {1}")]
    ReopenFailed(PathBuf, String),
    #[error("{0} changed after the batch was copied")]
    ChangedAfterCopy(PathBuf),
    #[error("{0}")]
    Io(String),
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

/// Maximum accepted size for a single inbound file (1 GiB).
pub const MAX_FILE_SIZE: u64 = 1024 * 1024 * 1024;

/// Validates and normalizes an inbound `FileMeta` from an untrusted peer
/// (T107 migration). On success the file name has been reduced to its
/// basename and normalized; error strings are part of the observable wire
/// contract and must stay stable.
pub fn validate_incoming_file_meta(meta: &mut FileMeta) -> Result<(), PrepareError> {
    if meta.batch.is_none() {
        return Err(PrepareError::ProtocolRequired);
    }
    if meta.size > MAX_FILE_SIZE {
        return Err(PrepareError::FileTooLarge);
    }
    let Some(file_name) = std::path::Path::new(&meta.name).file_name() else {
        return Err(PrepareError::InvalidFileName);
    };
    meta.name = file_name.to_string_lossy().to_string();
    meta.name = normalize_transferred_file_name(&meta.name, &meta.hash);
    if meta.name.is_empty() || meta.name == "." || meta.name == ".." {
        return Err(PrepareError::InvalidFileName);
    }
    if meta.transfer_id.is_some()
        && (meta.chunk_size == 0 || meta.chunk_size as usize > crate::protocol::FILE_CHUNK_SIZE)
    {
        return Err(PrepareError::InvalidChunkSize);
    }
    Ok(())
}

/// Verify that clipboard paths can still be inspected and opened before a
/// transfer is admitted. Clipboard providers may advertise a path whose
/// backing file has already disappeared or become inaccessible.
pub fn clipboard_files_are_readable(paths: &[PathBuf]) -> Result<(), String> {
    for path in paths {
        let metadata = fs::symlink_metadata(path).map_err(|error| {
            format!("Cannot inspect clipboard file {}: {error}", path.display())
        })?;
        if !metadata.is_file() {
            continue;
        }
        File::open(path)
            .map_err(|error| format!("Cannot read clipboard file {}: {error}", path.display()))?;
    }
    Ok(())
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
    pub fn validate(&self) -> Result<(), PrepareError> {
        if self.files.is_empty() || self.files.len() > MAX_FILE_BATCH_COUNT {
            return Err(PrepareError::BadBatchSize(MAX_FILE_BATCH_COUNT));
        }
        if self.total_bytes > MAX_FILE_BATCH_BYTES {
            return Err(PrepareError::BatchTooLarge);
        }
        let mut transfer_ids = HashSet::new();
        let mut total = 0_u64;
        for (expected_index, file) in self.files.iter().enumerate() {
            if usize::from(file.index) != expected_index {
                return Err(PrepareError::NonContiguousIndexes);
            }
            if file.transfer_id.is_zero() {
                return Err(PrepareError::InvalidTransferId);
            }
            if !transfer_ids.insert(file.transfer_id) {
                return Err(PrepareError::DuplicateTransferId);
            }
            if file.name.is_empty()
                || Path::new(&file.name)
                    .file_name()
                    .is_none_or(|name| name != file.name.as_str())
            {
                return Err(PrepareError::InvalidBatchFileName);
            }
            if file.chunk_size == 0 || file.chunk_size as usize > FILE_CHUNK_SIZE {
                return Err(PrepareError::InvalidBatchChunkSize);
            }
            total = total
                .checked_add(file.size)
                .ok_or(PrepareError::ByteCountOverflow)?;
        }
        if total != self.total_bytes {
            return Err(PrepareError::TotalMismatch);
        }
        Ok(())
    }
}

pub fn prepare_file_batch(
    paths: Vec<PathBuf>,
    generation: u64,
) -> Result<PreparedFileBatch, PrepareError> {
    if paths.is_empty() || paths.len() > MAX_FILE_BATCH_COUNT {
        return Err(PrepareError::BadSelection(MAX_FILE_BATCH_COUNT));
    }
    let mut candidates = Vec::with_capacity(paths.len());
    let mut total_bytes = 0_u64;
    for path in paths {
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| PrepareError::InspectFailed(path.clone(), error.to_string()))?;
        if metadata.file_type().is_symlink() {
            return Err(PrepareError::SymlinkSelected(path.clone()));
        }
        if !metadata.is_file() {
            return Err(PrepareError::NonFileSelected(path.clone()));
        }
        total_bytes = total_bytes
            .checked_add(metadata.len())
            .ok_or(PrepareError::SizeOverflow)?;
        if total_bytes > MAX_FILE_BATCH_BYTES {
            return Err(PrepareError::SelectionTooLarge);
        }
        let original_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .ok_or_else(|| PrepareError::InvalidPathName(path.clone()))?
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
            index: u16::try_from(index).map_err(|_| PrepareError::IndexOverflow)?,
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

pub fn revalidate_prepared_file(file: &PreparedFile) -> Result<(), PrepareError> {
    let metadata = fs::symlink_metadata(&file.path)
        .map_err(|error| PrepareError::ReopenFailed(file.path.clone(), error.to_string()))?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() != file.entry.size
        || modified_nanos(&metadata)? != file.modified_nanos
    {
        return Err(PrepareError::ChangedAfterCopy(file.path.clone()));
    }
    let hash = hash_source_file(&file.path)?;
    if hash != file.entry.hash {
        return Err(PrepareError::ChangedAfterCopy(file.path.clone()));
    }
    Ok(())
}

pub(crate) fn hash_source_file(path: &Path) -> Result<String, PrepareError> {
    let file = File::open(path).map_err(|error| PrepareError::Io(error.to_string()))?;
    let mut reader = BufReader::with_capacity(64 * 1024, file);
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|error| PrepareError::Io(error.to_string()))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn modified_nanos(metadata: &fs::Metadata) -> Result<u128, PrepareError> {
    metadata
        .modified()
        .map_err(|error| PrepareError::Io(error.to_string()))?
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .map_err(|error| PrepareError::Io(error.to_string()))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_directory(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "tailsync-prepare-{label}-{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        ))
    }

    #[test]
    fn normalize_transferred_file_name_strips_full_and_legacy_prefixes() {
        assert_eq!(
            normalize_transferred_file_name("abc123-def.txt", "abc123"),
            "def.txt"
        );
        // The legacy prefix is the first 12 hash characters plus an
        // underscore; hashes shorter than 12 characters never match.
        assert_eq!(
            normalize_transferred_file_name("abcdef123456_ghi.txt", "abcdef123456"),
            "ghi.txt"
        );
        assert_eq!(
            normalize_transferred_file_name("abc123", "abc123"),
            "abc123",
            "a name that is entirely the prefix is kept"
        );
        assert_eq!(
            normalize_transferred_file_name("plain.txt", "hash"),
            "plain.txt"
        );
    }

    #[test]
    fn collision_safe_name_uniquifies_with_folder_labels() {
        let mut used = HashSet::new();
        let first = collision_safe_name("report.pdf", "taxes", &mut used);
        let second = collision_safe_name("report.pdf", "taxes", &mut used);
        let third = collision_safe_name("report.pdf", "taxes", &mut used);
        assert_eq!(first, "report.pdf");
        assert_eq!(second, "report (taxes).pdf");
        assert_eq!(third, "report (taxes 2).pdf");
        assert_ne!(first, second);
        assert_ne!(second, third);
    }

    #[test]
    fn manifest_validate_rejects_structural_violations() {
        let entry = |index: u16| FileBatchEntry {
            transfer_id: TransferId::random(),
            index,
            name: format!("file-{index}.bin"),
            source_parent: String::new(),
            size: 4,
            hash: "hash".to_string(),
            chunk_size: FILE_CHUNK_SIZE as u32,
        };
        let manifest = |files: Vec<FileBatchEntry>, total_bytes: u64| FileBatchManifest {
            batch_id: TransferId::random(),
            generation: 0,
            total_bytes,
            files,
        };

        assert!(
            manifest(Vec::new(), 0).validate().is_err(),
            "empty batches are rejected"
        );
        assert!(
            manifest(vec![entry(1)], 4).validate().is_err(),
            "non-contiguous indexes are rejected"
        );
        let duplicate = entry(0);
        assert!(
            manifest(vec![duplicate.clone(), duplicate], 8)
                .validate()
                .is_err(),
            "duplicate transfer IDs are rejected"
        );
        assert!(
            manifest(vec![entry(0)], 5).validate().is_err(),
            "total byte mismatch is rejected"
        );
        let mut zero_transfer = entry(0);
        zero_transfer.transfer_id = TransferId([0; 16]);
        assert!(
            manifest(vec![zero_transfer], 4).validate().is_err(),
            "all-zero transfer IDs are rejected"
        );
        assert!(
            manifest(vec![entry(0)], 4).validate().is_ok(),
            "a well-formed manifest validates"
        );
    }

    #[test]
    fn prepare_file_batch_builds_hashed_entries_for_real_files() {
        let directory = test_directory("real-files");
        fs::create_dir_all(directory.join("source")).unwrap();
        fs::write(directory.join("source").join("a.txt"), b"alpha").unwrap();
        fs::write(directory.join("source").join("b.txt"), b"beta").unwrap();

        let prepared = prepare_file_batch(
            vec![
                directory.join("source").join("a.txt"),
                directory.join("source").join("b.txt"),
            ],
            7,
        )
        .unwrap();

        assert_eq!(prepared.manifest.generation, 7);
        assert_eq!(prepared.manifest.total_bytes, 9);
        assert_eq!(prepared.manifest.files.len(), 2);
        assert_eq!(prepared.files[0].entry.name, "a.txt");
        assert_eq!(
            prepared.files[0].entry.hash,
            blake3::hash(b"alpha").to_hex().to_string()
        );
        assert_eq!(
            prepared.files[1].entry.hash,
            blake3::hash(b"beta").to_hex().to_string()
        );
        // The same selection prepared again gets fresh transfer IDs.
        let again = prepare_file_batch(vec![directory.join("source").join("a.txt")], 7).unwrap();
        assert_ne!(again.manifest.batch_id, prepared.manifest.batch_id);

        revalidate_prepared_file(&prepared.files[0]).unwrap();
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn prepare_file_batch_rejects_symlinks_and_folders() {
        let directory = test_directory("reject");
        fs::create_dir_all(directory.join("folder")).unwrap();
        fs::write(directory.join("real.txt"), b"data").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(directory.join("real.txt"), directory.join("link.txt")).unwrap();

        let folder_error = prepare_file_batch(vec![directory.join("folder")], 0).unwrap_err();
        assert!(matches!(folder_error, PrepareError::NonFileSelected(_)));
        assert!(folder_error
            .to_string()
            .contains("The selection contains a folder or non-file item:"));
        #[cfg(unix)]
        {
            let link_error = prepare_file_batch(vec![directory.join("link.txt")], 0).unwrap_err();
            assert!(matches!(link_error, PrepareError::SymlinkSelected(_)));
            assert!(link_error.to_string().contains("symbolic link"));
        }
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn clipboard_files_are_readable_rejects_missing_paths() {
        let missing = test_directory("missing-clipboard-file").join("missing.txt");
        let error = clipboard_files_are_readable(&[missing]).unwrap_err();
        assert!(error.contains("Cannot inspect clipboard file"));
    }

    #[test]
    fn clipboard_files_are_readable_accepts_regular_files() {
        let directory = test_directory("readable-clipboard-file");
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("clipboard.txt");
        fs::write(&path, b"clipboard file").unwrap();

        assert!(clipboard_files_are_readable(std::slice::from_ref(&path)).is_ok());

        fs::remove_dir_all(directory).unwrap();
    }
}
