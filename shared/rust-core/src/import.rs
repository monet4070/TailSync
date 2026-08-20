//! Local API import sessions (backup/restore ingestion).
//!
//! Shared by the macOS and Windows local JSON-lines API servers (T102
//! migration). A client opens an import session (`begin_import`), streams
//! base64 chunks into a `.part` file in the shared `incoming/` directory
//! (`append_import_chunk`), then finalizes and commits it into history via
//! the migration inserters (`finalize_import` + `commit_import`). All error
//! strings are part of the observable API contract; keep them stable.

use crate::db::HistoryDB;
use crate::protocol::{MAX_IMAGE_PAYLOAD_SIZE, MAX_TEXT_PAYLOAD_SIZE};
use base64::Engine;
use chrono::DateTime;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use thiserror::Error;

/// Import session failures (T354 migration). Display strings are the
/// observable JSON-lines API contract — the module header promises they are
/// stable — and must stay byte-for-byte identical.
#[derive(Debug, Error)]
pub enum ImportError {
    #[error("unknown import type")]
    UnknownType,
    #[error("import description exceeds 1024 bytes")]
    DescriptionTooLong,
    #[error("{0} import exceeds the {1} byte limit")]
    ImportTooLarge(String, u64),
    #[error("data_hash must be 64 hexadecimal characters")]
    InvalidDataHash,
    #[error("active import limit ({0}) reached")]
    ActiveLimitReached(usize),
    #[error("could not create import file: {0}")]
    CreateFileFailed(String),
    #[error("could not allocate a unique import session")]
    SessionAllocationFailed,
    #[error("invalid import_id")]
    InvalidImportId,
    #[error("import chunk exceeds the {0} byte limit")]
    ChunkLimitExceeded(usize),
    #[error("invalid chunk base64: {0}")]
    InvalidChunkBase64(String),
    #[error("import chunk must contain 1 to {0} bytes")]
    ChunkSizeRange(usize),
    #[error("unknown or expired import session")]
    UnknownOrExpiredSession,
    #[error("unexpected import offset {0}; expected {1}")]
    UnexpectedOffset(u64, u64),
    #[error("import offset overflow")]
    OffsetOverflow,
    #[error("import chunk exceeds declared total_size")]
    ChunkExceedsTotal,
    #[error("could not write import chunk: {0}")]
    WriteChunkFailed(String),
    #[error("could not finalize import file: {0}")]
    FinalizeFileFailed(String),
    #[error("incomplete import: received {0}, expected {1} bytes")]
    IncompleteImport(u64, u64),
    #[error("import data hash mismatch")]
    HashMismatch,
    #[error("could not read completed import: {0}")]
    ReadCompletedFailed(String),
    #[error("file import path is unavailable")]
    FilePathUnavailable,
    #[error("completed import data is unavailable")]
    DataUnavailable,
    #[error("invalid import timestamp")]
    InvalidTimestamp,
    #[error("{0}")]
    Io(String),
}

pub const IMPORT_CHUNK_MAX_BYTES: usize = 512 * 1024;
pub const API_MAX_IMPORTS: usize = 4;
pub const IMPORT_SESSION_TTL: Duration = Duration::from_secs(10 * 60);
pub const MAX_IMPORT_FILE_SIZE: u64 = 1024 * 1024 * 1024;

struct ImportSession {
    time: String,
    entry_type: String,
    description: String,
    expected_size: u64,
    expected_hash: Option<String>,
    received: u64,
    path: PathBuf,
    file: File,
    hasher: blake3::Hasher,
    updated_at: Instant,
}

/// Registry of in-flight import sessions for the local API server.
#[derive(Default)]
pub struct ImportRegistry {
    sessions: HashMap<String, ImportSession>,
}

impl ImportRegistry {
    fn prune(&mut self, now: Instant) {
        let expired = self
            .sessions
            .iter()
            .filter(|(_, session)| now.duration_since(session.updated_at) > IMPORT_SESSION_TTL)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for id in expired {
            if let Some(session) = self.sessions.remove(&id) {
                let _ = std::fs::remove_file(session.path);
            }
        }
    }
}

/// Maximum payload size accepted for an import of the given entry type.
pub fn import_size_limit(entry_type: &str) -> Result<u64, ImportError> {
    match entry_type {
        "text" => Ok(MAX_TEXT_PAYLOAD_SIZE as u64),
        "image" => Ok(MAX_IMAGE_PAYLOAD_SIZE as u64),
        "file" => Ok(MAX_IMPORT_FILE_SIZE),
        _ => Err(ImportError::UnknownType),
    }
}

/// Parameters for opening a new import session.
pub struct BeginImportParams {
    pub time: String,
    pub entry_type: String,
    pub description: String,
    pub expected_size: u64,
    pub data_hash: Option<String>,
}

/// Result of opening a session: the id the client must use for chunks and
/// the first offset to send.
#[derive(Debug, PartialEq)]
pub struct BeginImportResult {
    pub import_id: String,
    pub next_offset: u64,
}

/// Opens an import session and allocates its `.part` file in `incoming_dir`.
pub fn begin_import(
    registry: &mut ImportRegistry,
    incoming_dir: &Path,
    params: &BeginImportParams,
) -> Result<BeginImportResult, ImportError> {
    DateTime::parse_from_rfc3339(&params.time).map_err(|_| ImportError::InvalidTimestamp)?;
    if params.description.len() > 1024 {
        return Err(ImportError::DescriptionTooLong);
    }
    let limit = import_size_limit(&params.entry_type)?;
    if params.expected_size > limit {
        return Err(ImportError::ImportTooLarge(
            params.entry_type.clone(),
            limit,
        ));
    }
    let expected_hash = params
        .data_hash
        .as_deref()
        .map(str::trim)
        .filter(|hash| !hash.is_empty())
        .map(|hash| {
            if hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                Ok(hash.to_ascii_lowercase())
            } else {
                Err(ImportError::InvalidDataHash)
            }
        })
        .transpose()?;

    std::fs::create_dir_all(incoming_dir).map_err(|error| ImportError::Io(error.to_string()))?;
    let now = Instant::now();
    registry.prune(now);
    if registry.sessions.len() >= API_MAX_IMPORTS {
        return Err(ImportError::ActiveLimitReached(API_MAX_IMPORTS));
    }

    for _ in 0..8 {
        let import_id = hex::encode(rand::random::<[u8; 16]>());
        let path = incoming_dir.join(format!("api-import-{import_id}.part"));
        let file = match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(ImportError::CreateFileFailed(error.to_string())),
        };
        registry.sessions.insert(
            import_id.clone(),
            ImportSession {
                time: params.time.clone(),
                entry_type: params.entry_type.clone(),
                description: params.description.clone(),
                expected_size: params.expected_size,
                expected_hash,
                received: 0,
                path,
                file,
                hasher: blake3::Hasher::new(),
                updated_at: now,
            },
        );
        return Ok(BeginImportResult {
            import_id,
            next_offset: 0,
        });
    }
    Err(ImportError::SessionAllocationFailed)
}

/// Appends one base64-encoded chunk to a session; returns the next offset.
pub fn append_import_chunk(
    registry: &mut ImportRegistry,
    import_id: &str,
    offset: u64,
    chunk_b64: &str,
) -> Result<u64, ImportError> {
    if import_id.len() != 32 || !import_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ImportError::InvalidImportId);
    }
    let max_encoded = IMPORT_CHUNK_MAX_BYTES.div_ceil(3) * 4;
    if chunk_b64.len() > max_encoded {
        return Err(ImportError::ChunkLimitExceeded(IMPORT_CHUNK_MAX_BYTES));
    }
    let chunk = base64::engine::general_purpose::STANDARD
        .decode(chunk_b64)
        .map_err(|error| ImportError::InvalidChunkBase64(error.to_string()))?;
    if chunk.is_empty() || chunk.len() > IMPORT_CHUNK_MAX_BYTES {
        return Err(ImportError::ChunkSizeRange(IMPORT_CHUNK_MAX_BYTES));
    }

    let now = Instant::now();
    registry.prune(now);
    let session = registry
        .sessions
        .get_mut(import_id)
        .ok_or(ImportError::UnknownOrExpiredSession)?;
    if offset != session.received {
        return Err(ImportError::UnexpectedOffset(offset, session.received));
    }
    let next_offset = offset
        .checked_add(chunk.len() as u64)
        .ok_or(ImportError::OffsetOverflow)?;
    if next_offset > session.expected_size {
        return Err(ImportError::ChunkExceedsTotal);
    }
    session
        .file
        .write_all(&chunk)
        .map_err(|error| ImportError::WriteChunkFailed(error.to_string()))?;
    session.hasher.update(&chunk);
    session.received = next_offset;
    session.updated_at = now;
    Ok(next_offset)
}

/// Finished session content, ready to be committed into history.
#[derive(Debug, PartialEq)]
pub struct FinalizedImport {
    pub time: String,
    pub entry_type: String,
    pub description: String,
    /// Inline payload for text/image imports; `None` for file imports.
    pub inline_data: Option<Vec<u8>>,
    /// On-disk payload for file imports; the caller removes it after commit.
    pub path: Option<PathBuf>,
    pub data_hash: String,
    pub size: u64,
}

/// Finalizes a session: flushes and checks the `.part` file, verifies the
/// declared hash, reads inline payloads for text/image, and removes the
/// temporary file for non-file imports. File imports keep their temp file so
/// [`commit_import`] can persist it into file history; the caller removes it
/// after the commit attempt.
pub fn finalize_import(
    registry: &mut ImportRegistry,
    import_id: &str,
) -> Result<FinalizedImport, ImportError> {
    let mut session = registry
        .sessions
        .remove(import_id)
        .ok_or(ImportError::UnknownOrExpiredSession)?;

    let result = (|| -> Result<(String, u64), ImportError> {
        session
            .file
            .flush()
            .and_then(|_| session.file.sync_all())
            .map_err(|error| ImportError::FinalizeFileFailed(error.to_string()))?;
        if session.received != session.expected_size {
            return Err(ImportError::IncompleteImport(
                session.received,
                session.expected_size,
            ));
        }
        let actual_hash = session.hasher.finalize().to_hex().to_string();
        if session
            .expected_hash
            .as_deref()
            .is_some_and(|hash| hash != actual_hash)
        {
            return Err(ImportError::HashMismatch);
        }
        Ok((actual_hash, session.received))
    })();

    let (actual_hash, size) = match result {
        Ok(result) => result,
        Err(error) => {
            let _ = std::fs::remove_file(&session.path);
            return Err(error);
        }
    };
    drop(session.file);

    let inline_data = if session.entry_type == "file" {
        None
    } else {
        match std::fs::read(&session.path) {
            Ok(data) => Some(data),
            Err(error) => {
                let _ = std::fs::remove_file(&session.path);
                return Err(ImportError::ReadCompletedFailed(error.to_string()));
            }
        }
    };
    let path = if session.entry_type == "file" {
        Some(session.path.clone())
    } else {
        let _ = std::fs::remove_file(&session.path);
        None
    };
    Ok(FinalizedImport {
        time: session.time,
        entry_type: session.entry_type,
        description: session.description,
        inline_data,
        path,
        data_hash: actual_hash,
        size,
    })
}

/// Commits a finalized import into history using the migration inserters.
///
/// The file branch persists the payload from [`FinalizedImport::path`] into
/// file history; text/image branches use the inline payload.
pub fn commit_import(db: &mut HistoryDB, finished: &FinalizedImport) -> Result<(), ImportError> {
    match finished.entry_type.as_str() {
        "file" => {
            let source = finished
                .path
                .as_deref()
                .ok_or(ImportError::FilePathUnavailable)?;
            db.add_file_migrated_from_path(
                &finished.time,
                &finished.description,
                source,
                &finished.data_hash,
                finished.size,
            )
            .map_err(|error| ImportError::Io(error.to_string()))
        }
        "text" | "image" => match finished.inline_data.as_deref() {
            Some(data) if finished.entry_type == "text" => db
                .add_text_migrated(&finished.time, &finished.description, data)
                .map_err(|error| ImportError::Io(error.to_string())),
            Some(data) => db
                .add_image_migrated(&finished.time, &finished.description, data)
                .map_err(|error| ImportError::Io(error.to_string())),
            None => Err(ImportError::DataUnavailable),
        },
        _ => Err(ImportError::UnknownType),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_incoming_dir(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "tailsync-import-{name}-{:016x}",
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn params(entry_type: &str, data: &[u8], hash: Option<&str>) -> BeginImportParams {
        BeginImportParams {
            time: "2026-08-15T12:00:00Z".to_string(),
            entry_type: entry_type.to_string(),
            description: "test import".to_string(),
            expected_size: data.len() as u64,
            data_hash: hash.map(str::to_string),
        }
    }

    #[test]
    fn import_size_limit_matches_protocol_and_file_limits() {
        assert_eq!(
            import_size_limit("text").unwrap(),
            MAX_TEXT_PAYLOAD_SIZE as u64
        );
        assert_eq!(
            import_size_limit("image").unwrap(),
            MAX_IMAGE_PAYLOAD_SIZE as u64
        );
        assert_eq!(import_size_limit("file").unwrap(), MAX_IMPORT_FILE_SIZE);
        assert!(matches!(
            import_size_limit("other"),
            Err(ImportError::UnknownType)
        ));
    }

    #[test]
    fn begin_import_validates_input_and_rejects_bad_hashes() {
        let dir = temp_incoming_dir("validate");
        let mut registry = ImportRegistry::default();

        let bad_hash = params("text", b"data", Some("not-a-hash"));
        assert!(matches!(
            begin_import(&mut registry, &dir, &bad_hash),
            Err(ImportError::InvalidDataHash)
        ));

        let bad_time = BeginImportParams {
            time: "not-a-timestamp".to_string(),
            ..params("text", b"data", None)
        };
        assert!(matches!(
            begin_import(&mut registry, &dir, &bad_time),
            Err(ImportError::InvalidTimestamp)
        ));

        let too_long = BeginImportParams {
            description: "x".repeat(1025),
            ..params("text", b"data", None)
        };
        assert!(matches!(
            begin_import(&mut registry, &dir, &too_long),
            Err(ImportError::DescriptionTooLong)
        ));
        assert_eq!(
            begin_import(&mut registry, &dir, &too_long)
                .unwrap_err()
                .to_string(),
            "import description exceeds 1024 bytes"
        );

        let oversized = BeginImportParams {
            expected_size: MAX_TEXT_PAYLOAD_SIZE as u64 + 1,
            ..params("text", b"data", None)
        };
        assert!(matches!(
            begin_import(&mut registry, &dir, &oversized).unwrap_err(),
            ImportError::ImportTooLarge(_, _)
        ));

        assert!(matches!(
            begin_import(&mut registry, &dir, &params("unknown-type", b"data", None)),
            Err(ImportError::UnknownType)
        ));

        assert!(registry.sessions.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn text_import_round_trip_commits_and_cleans_up() {
        let dir = temp_incoming_dir("text");
        let mut registry = ImportRegistry::default();
        let payload = b"hello import world";
        let hash = blake3::hash(payload).to_hex().to_string();

        let opened =
            begin_import(&mut registry, &dir, &params("text", payload, Some(&hash))).unwrap();
        assert_eq!(opened.next_offset, 0);

        let b64 = base64::engine::general_purpose::STANDARD.encode(payload);
        let next = append_import_chunk(&mut registry, &opened.import_id, 0, &b64).unwrap();
        assert_eq!(next, payload.len() as u64);
        assert!(append_import_chunk(&mut registry, &opened.import_id, next, &b64).is_err());

        let finished = finalize_import(&mut registry, &opened.import_id).unwrap();
        assert_eq!(finished.data_hash, hash);
        assert_eq!(finished.size, payload.len() as u64);
        assert_eq!(finished.inline_data.as_deref(), Some(payload.as_slice()));
        assert!(finished.path.is_none());

        let mut db = HistoryDB::new_unavailable().unwrap();
        commit_import(&mut db, &finished).unwrap();

        // The temporary .part file must be gone.
        assert!(std::fs::read_dir(&dir).unwrap().next().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_hash_mismatch_is_rejected_and_file_removed() {
        let dir = temp_incoming_dir("mismatch");
        let mut registry = ImportRegistry::default();
        let payload = b"payload";
        let wrong_hash = "0".repeat(64);

        let opened = begin_import(
            &mut registry,
            &dir,
            &params("text", payload, Some(&wrong_hash)),
        )
        .unwrap();
        let b64 = base64::engine::general_purpose::STANDARD.encode(payload);
        append_import_chunk(&mut registry, &opened.import_id, 0, &b64).unwrap();

        assert!(matches!(
            finalize_import(&mut registry, &opened.import_id),
            Err(ImportError::HashMismatch)
        ));
        assert!(std::fs::read_dir(&dir).unwrap().next().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_rejects_incomplete_and_misordered_chunks() {
        let dir = temp_incoming_dir("chunks");
        let mut registry = ImportRegistry::default();
        let payload = b"0123456789";

        let opened = begin_import(&mut registry, &dir, &params("text", payload, None)).unwrap();
        let b64 = base64::engine::general_purpose::STANDARD.encode(b"0123");
        append_import_chunk(&mut registry, &opened.import_id, 0, &b64).unwrap();

        // Wrong offset.
        assert!(matches!(
            append_import_chunk(&mut registry, &opened.import_id, 7, &b64).unwrap_err(),
            ImportError::UnexpectedOffset(7, _)
        ));
        // Over-declared size.
        let more = base64::engine::general_purpose::STANDARD.encode(b"4567890123456789");
        assert!(matches!(
            append_import_chunk(&mut registry, &opened.import_id, 4, &more),
            Err(ImportError::ChunkExceedsTotal)
        ));
        // Incomplete finalize.
        assert!(matches!(
            finalize_import(&mut registry, &opened.import_id).unwrap_err(),
            ImportError::IncompleteImport(4, 10)
        ));
        // Session was consumed by finalize; a second finalize fails.
        assert!(matches!(
            finalize_import(&mut registry, &opened.import_id),
            Err(ImportError::UnknownOrExpiredSession)
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_rejects_invalid_base64_and_bad_offsets() {
        let dir = temp_incoming_dir("b64");
        let mut registry = ImportRegistry::default();

        assert!(matches!(
            append_import_chunk(&mut registry, "not-a-valid-id", 0, "AAAA"),
            Err(ImportError::InvalidImportId)
        ));
        assert!(matches!(
            append_import_chunk(&mut registry, "abcd".repeat(8).as_str(), 0, "!!!"),
            Err(ImportError::InvalidChunkBase64(_))
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
