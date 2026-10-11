//! Owned read preparation. SQL and file-handle acquisition use HistoryDB;
//! decryption and matching operate without borrowing the database.

use super::*;
use crate::cancellation::Cancellation;
use rusqlite::{params_from_iter, types::Value};
use std::{
    fs::File,
    io::{Read, Seek},
    sync::Arc,
};

const CHUNK_BYTE_BUDGET: usize = 2 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum HistoryReadError {
    #[error("request cancelled")]
    Cancelled,
    #[error("history changed while reading; retry the query")]
    Changed,
    #[error("{0}")]
    Operation(String),
}

impl From<crate::cancellation::Cancelled> for HistoryReadError {
    fn from(_: crate::cancellation::Cancelled) -> Self {
        Self::Cancelled
    }
}

impl From<rusqlite::Error> for HistoryReadError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Operation(error.to_string())
    }
}

#[derive(Clone, Debug)]
pub struct HistoryReadRevision {
    identity: Arc<()>,
    changes: u64,
    external_version: i64,
}

impl HistoryReadRevision {
    pub fn authority(&self) -> usize {
        Arc::as_ptr(&self.identity) as usize
    }
    pub fn generation(&self) -> u64 {
        self.changes
    }
}

#[derive(Clone, Debug)]
pub struct HistoryReadCursor {
    timestamp: String,
    id: i64,
}

pub(super) enum PayloadSource {
    PlainBytes(Vec<u8>),
    Checked(Box<PayloadSource>, String, usize),
    CompositeFirstImage(Box<super::composite::PreparedContentAnalysis>),
    Inline(Vec<u8>),
    Encrypted(File),
    Container(File),
    Plain(File),
}

impl PayloadSource {
    pub(super) fn read(
        self,
        cancellation: &Cancellation,
        limit: u64,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        cancellation.check()?;
        let data = match self {
            Self::PlainBytes(bytes) => bytes,
            Self::CompositeFirstImage(prepared) => {
                let packed = prepared.read_first_image(cancellation)?;
                cancellation.check()?;
                return Ok(packed);
            }
            Self::Checked(source, hash, size) => {
                let bytes = source.read(cancellation, size as u64)?;
                if bytes.len() != size || blake3::hash(&bytes).to_hex().as_str() != hash {
                    return Err("Composite part digest mismatch".into());
                }
                bytes
            }
            Self::Inline(bytes) => {
                if bytes.len() as u64 > limit.saturating_add(28) {
                    return Err("encrypted payload exceeds read limit".into());
                }
                crypto::decrypt(&bytes)?
            }
            Self::Encrypted(mut file) => crypto::decrypt(&read_bounded(
                &mut file,
                limit.saturating_add(28),
                cancellation,
            )?)?,
            Self::Container(mut file) => {
                let mut bytes = Vec::with_capacity(usize::try_from(limit).unwrap_or(0));
                file_encryption::decrypt_open_file(&mut file, &mut bytes, limit, cancellation)?;
                bytes
            }
            Self::Plain(mut file) => read_bounded(&mut file, limit, cancellation)?,
        };
        cancellation.check()?;
        if data.len() as u64 > limit {
            return Err("payload exceeds read limit".into());
        }
        Ok(data)
    }
}

fn read_bounded(
    file: &mut File,
    limit: u64,
    cancellation: &Cancellation,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    if file.metadata()?.len() > limit {
        return Err("payload exceeds read limit".into());
    }
    let mut bytes = Vec::with_capacity(usize::try_from(file.metadata()?.len())?);
    let mut buffer = [0; 64 * 1024];
    loop {
        cancellation.check()?;
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        if (bytes.len() as u64).saturating_add(count as u64) > limit {
            return Err("payload exceeds read limit".into());
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    Ok(bytes)
}

fn open_payload(path: &Path) -> Result<File, Box<dyn std::error::Error>> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("history payload is not a regular file".into());
    }
    let file = File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err("history payload is not a regular file".into());
    }
    Ok(file)
}

struct Candidate {
    entry: HistoryEntry,
    text: Option<Result<PayloadSource, String>>,
    menu: Result<Option<super::composite::PreparedCompositeMenu>, String>,
}

pub struct HistoryReadChunk {
    candidates: Vec<Candidate>,
    pub cursor: Option<HistoryReadCursor>,
    pub exhausted: bool,
}

impl HistoryReadChunk {
    /// Number of candidate rows read from SQLite before payload filtering.
    /// Useful for diagnostic accounting without exposing row contents.
    pub fn candidate_count(&self) -> usize {
        self.candidates.len()
    }

    /// Sum of declared plaintext payload sizes for those candidate rows.
    /// This is logical read volume, not a measurement of physical disk I/O.
    pub fn candidate_payload_bytes(&self) -> u64 {
        self.candidates.iter().fold(0_u64, |sum, candidate| {
            sum.saturating_add(candidate.entry.size_bytes.max(0) as u64)
        })
    }

    /// Decrypt one candidate at a time and observe cancellation between rows.
    /// Corrupt text retains its metadata fallback, matching the legacy query.
    pub fn matching_entries(
        self,
        keyword: Option<&str>,
        cancellation: &Cancellation,
    ) -> Result<Vec<HistoryEntry>, HistoryReadError> {
        let keyword = keyword
            .filter(|s| !s.trim().is_empty())
            .map(str::to_lowercase);
        let mut entries = Vec::new();
        for candidate in self.candidates {
            let Candidate {
                mut entry,
                text,
                menu,
            } = candidate;
            cancellation.check()?;
            let mut full_match = false;
            if let Some(source) = text {
                let text = source
                    .and_then(|source| {
                        source
                            .read(cancellation, PREVIEW_MAX_BYTES)
                            .map_err(|e| e.to_string())
                    })
                    .and_then(|bytes| {
                        let bytes = if entry.entry_type == "composite" {
                            entry.composite = super::composite::info_bytes(&bytes).ok();
                            let body = if let Some(info) = &mut entry.composite {
                                super::composite::PreparedCompositeMenu::apply_prepared(
                                    menu,
                                    info,
                                    cancellation,
                                )
                            } else {
                                None
                            };
                            body.map(String::into_bytes).map_or_else(
                                || {
                                    super::composite::summary_bytes(&bytes)
                                        .map_err(|e| e.to_string())
                                },
                                Ok,
                            )?
                        } else {
                            bytes
                        };
                        String::from_utf8(bytes).map_err(|e| e.to_string())
                    });
                cancellation.check()?;
                if let Ok(text) = text {
                    full_match = keyword
                        .as_ref()
                        .is_some_and(|keyword| text.to_lowercase().contains(keyword));
                    entry.description = text_preview(&text);
                } else {
                    tracing::debug!(target: "tailsync.runtime.execution", operation = "history.text", outcome = "unavailable", "history text unavailable");
                }
            }
            if keyword.as_ref().is_none_or(|keyword| {
                full_match || HistoryDB::entry_metadata_matches_keyword(&entry, keyword)
            }) {
                entries.push(entry);
            }
        }
        Ok(entries)
    }
}

pub struct PreparedPreview {
    pub metadata: PreviewMetadata,
    pub(super) source: PayloadSource,
    pub(super) data_hash: String,
}

impl PreparedPreview {
    /// Includes decrypt copies, binary framing and bounded PNG workspace. The
    /// lease travels with the admitted result until the IPC worker finishes.
    pub fn read_admitted(
        self,
        cancellation: &Cancellation,
    ) -> Result<
        (
            PreviewMetadata,
            PreviewPayload,
            crate::composite::budgets::PipelineLease,
        ),
        PreviewError,
    > {
        let size = self.metadata.size_bytes;
        let png_extra = if matches!(&self.source, PayloadSource::CompositeFirstImage(_)) {
            192 * 1024 * 1024 - 1024 * 1024
        } else {
            0
        };
        let memory = crate::composite::budgets::global()
            .reserve_pipeline(
                size.saturating_mul(3)
                    .saturating_add(png_extra)
                    .saturating_add(1024 * 1024),
            )
            .map_err(|e| PreviewError::PayloadUnavailable {
                entry_id: self.metadata.entry_id,
                reason: e.to_string(),
            })?;
        let (metadata, payload) = self.read_inner(cancellation)?;
        Ok((metadata, payload, memory))
    }

    pub fn read(
        self,
        cancellation: &Cancellation,
    ) -> Result<(PreviewMetadata, PreviewPayload), PreviewError> {
        let (metadata, payload, _memory) = self.read_admitted(cancellation)?;
        Ok((metadata, payload))
    }

    fn read_inner(
        self,
        cancellation: &Cancellation,
    ) -> Result<(PreviewMetadata, PreviewPayload), PreviewError> {
        let id = self.metadata.entry_id;
        let self_sized = matches!(&self.source, PayloadSource::CompositeFirstImage(_));
        let mut metadata = self.metadata;
        let data = self
            .source
            .read(
                cancellation,
                if self_sized {
                    32 * 1024 * 1024
                } else {
                    metadata.size_bytes
                },
            )
            .map_err(|error| PreviewError::PayloadUnavailable {
                entry_id: id,
                reason: error.to_string(),
            })?;
        if !self_sized && data.len() as u64 != metadata.size_bytes {
            return Err(PreviewError::PayloadUnavailable {
                entry_id: id,
                reason: "payload length differs from history metadata".into(),
            });
        }
        if self_sized {
            metadata.size_bytes = data.len() as u64;
        }
        if self.data_hash.len() == 64 && blake3::hash(&data).to_hex().as_str() != self.data_hash {
            return Err(PreviewError::PayloadUnavailable {
                entry_id: id,
                reason: "payload hash differs from history metadata".into(),
            });
        }
        let payload = PreviewPayload {
            kind: metadata.kind.as_str().into(),
            name: metadata.name.clone(),
            size_bytes: data.len() as u64,
            data,
        };
        Ok((metadata, payload))
    }
}

impl HistoryDB {
    pub fn read_revision(&self) -> Result<HistoryReadRevision, HistoryReadError> {
        Ok(HistoryReadRevision {
            identity: self.read_identity.clone(),
            changes: self.conn.total_changes(),
            external_version: self
                .conn
                .query_row("PRAGMA data_version", [], |r| r.get(0))?,
        })
    }

    pub fn validate_read_revision(
        &self,
        revision: &HistoryReadRevision,
    ) -> Result<(), HistoryReadError> {
        let current = self.read_revision()?;
        if !Arc::ptr_eq(&current.identity, &revision.identity)
            || current.changes != revision.changes
            || current.external_version != revision.external_version
        {
            Err(HistoryReadError::Changed)
        } else {
            Ok(())
        }
    }

    pub fn validate_storage_authority(
        &self,
        revision: &HistoryReadRevision,
    ) -> Result<(), HistoryReadError> {
        if Arc::ptr_eq(&self.read_identity, &revision.identity) {
            Ok(())
        } else {
            Err(HistoryReadError::Changed)
        }
    }

    pub(super) fn prepare_text(
        &self,
        stored: Vec<u8>,
    ) -> Result<PayloadSource, Box<dyn std::error::Error>> {
        if let Some(reference) = decode_image_reference(&stored) {
            Ok(PayloadSource::Encrypted(open_payload(
                &resolve_file_reference_at(&self.image_history_dir, &reference)?,
            )?))
        } else {
            Ok(PayloadSource::Inline(stored))
        }
    }

    /// Stable (timestamp, id) cursor under a bounded optimistic revision.
    /// Callers must validate again before publishing the page. On a mutation
    /// they discard the whole page and retry at most a fixed number of times.
    pub fn prepare_read_chunk(
        &self,
        query: HistoryQuery<'_>,
        cursor: Option<&HistoryReadCursor>,
        revision: &HistoryReadRevision,
        chunk_size: usize,
    ) -> Result<HistoryReadChunk, HistoryReadError> {
        self.validate_read_revision(revision)?;
        let keyword = query.keyword.filter(|value| !value.trim().is_empty());
        let (mut filters, mut values) = Self::history_filter_clause(
            query.collection,
            query.category,
            query.start_time,
            query.end_time,
        )
        .map_err(|e| HistoryReadError::Operation(e.to_string()))?;
        let mut append_condition = |condition: &str| {
            filters.push_str(if filters.is_empty() {
                " WHERE "
            } else {
                " AND "
            });
            filters.push_str(condition);
        };
        if let Some(keyword) = keyword {
            append_condition("(type IN ('text', 'composite') OR description LIKE ? ESCAPE '\\' OR source_peer LIKE ? ESCAPE '\\' OR type LIKE ? ESCAPE '\\' OR category LIKE ? ESCAPE '\\' OR EXISTS (SELECT 1 FROM json_each(history.categories) AS label WHERE label.value LIKE ? ESCAPE '\\'))");
            for _ in 0..5 {
                values.push(Value::Text(format!(
                    "%{}%",
                    super::queries::escape_like_literal(keyword)
                )));
            }
        }
        if let Some(cursor) = cursor {
            append_condition("(timestamp, id) < (?, ?)");
            values.push(Value::Text(cursor.timestamp.clone()));
            values.push(Value::Integer(cursor.id));
        }
        let count = chunk_size.clamp(1, 256);
        let sql = format!("SELECT id, timestamp, type, description, data_hash, size_bytes, source_peer,
            category, category_confidence, classifier_version, categories, pinned, batch_id, batch_index, batch_total, batch_status,
            CASE WHEN batch_id IS NULL THEN NULL ELSE (SELECT COUNT(*) FROM history AS b WHERE b.batch_id = history.batch_id) END,
            CASE WHEN type IN ('text', 'composite') THEN data ELSE NULL END
            FROM history{filters} ORDER BY timestamp DESC, id DESC LIMIT ? OFFSET ?");
        values.push(Value::Integer(count as i64));
        // With no keyword the public offset still belongs to SQL. With a
        // keyword it counts decrypted matches in the runtime instead.
        values.push(Value::Integer(if keyword.is_none() && cursor.is_none() {
            i64::try_from(query.offset).map_err(|e| HistoryReadError::Operation(e.to_string()))?
        } else {
            0
        }));
        let mut statement = self.conn.prepare(&sql)?;
        let mut rows = statement.query(params_from_iter(values))?;
        let mut candidates = Vec::new();
        let mut bytes = 0usize;
        let mut exhausted = false;
        while candidates.len() < count && bytes < CHUNK_BYTE_BUDGET {
            let Some(row) = rows.next()? else {
                exhausted = true;
                break;
            };
            let entry = Self::row_to_entry(row)?;
            let stored = row.get::<_, Option<Vec<u8>>>(17)?;
            let text = stored.map(|stored| {
                bytes = bytes.saturating_add(stored.len());
                if entry.entry_type == "composite" {
                    // Menu preparation already needs the manifest while SQL is
                    // available. Retain that plaintext for the owned read worker
                    // instead of decrypting the same manifest twice.
                    self.read_text_payload_compat(&stored)
                        .map(PayloadSource::PlainBytes)
                        .map_err(|error| error.to_string())
                } else {
                    self.prepare_text(stored).map_err(|error| error.to_string())
                }
            });
            let menu = if entry.entry_type == "composite" {
                match &text {
                    Some(Ok(PayloadSource::PlainBytes(decoded))) => self
                        .prepare_composite_menu(entry.id, decoded)
                        .map_err(|e| e.to_string()),
                    Some(Err(_)) => Err("Composite manifest unavailable".into()),
                    _ => Ok(None),
                }
            } else {
                Ok(None)
            };
            bytes = bytes.saturating_add(
                menu.as_ref()
                    .ok()
                    .and_then(|m| m.as_ref())
                    .map_or(0, |m| m.declared_bytes()),
            );
            candidates.push(Candidate { entry, text, menu });
        }
        let cursor = candidates.last().map(|candidate| HistoryReadCursor {
            timestamp: candidate.entry.timestamp.clone(),
            id: candidate.entry.id,
        });
        drop(rows);
        drop(statement);
        self.validate_read_revision(revision)?;
        Ok(HistoryReadChunk {
            candidates,
            cursor,
            exhausted,
        })
    }

    pub fn prepare_preview(
        &self,
        id: i64,
        batch_id: Option<&str>,
    ) -> Result<PreparedPreview, PreviewError> {
        let id = match batch_id {
            Some(batch) => self.get_preview_batch_navigation(batch, id)?.first_entry_id,
            None => id,
        };
        let mut metadata = self.get_preview_metadata(id)?;
        if self
            .get_type(id)
            .map_err(|_| PreviewError::EntryNotFound { entry_id: id })?
            == "composite"
        {
            let display_kind =
                self.display_kind(id)
                    .map_err(|error| PreviewError::MetadataUnavailable {
                        entry_id: id,
                        reason: error.to_string(),
                    })?;
            if display_kind == "image" {
                return self.prepare_composite_image(id);
            }
            let stored: Vec<u8> = self
                .conn
                .query_row("SELECT data FROM history WHERE id=?1", [id], |r| r.get(0))
                .map_err(|e| PreviewError::PayloadUnavailable {
                    entry_id: id,
                    reason: e.to_string(),
                })?;
            let summary =
                super::composite::summary_bytes(&crypto::decrypt(&stored).map_err(|e| {
                    PreviewError::PayloadUnavailable {
                        entry_id: id,
                        reason: e.to_string(),
                    }
                })?)
                .map_err(|e| PreviewError::PayloadUnavailable {
                    entry_id: id,
                    reason: e.to_string(),
                })?;
            metadata.size_bytes = summary.len() as u64;
            return Ok(PreparedPreview {
                metadata,
                data_hash: blake3::hash(&summary).to_hex().to_string(),
                source: PayloadSource::PlainBytes(summary),
            });
        }
        let source = (|| -> Result<_, Box<dyn std::error::Error>> {
            let (stored, data_hash): (Vec<u8>, String) = self.conn.query_row(
                "SELECT data, data_hash FROM history WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let source = match metadata.kind {
                PreviewKind::Text | PreviewKind::Image => self.prepare_text(stored)?,
                PreviewKind::File => match decode_file_reference(&stored) {
                    Some(reference) => {
                        let mut file = open_payload(&resolve_file_reference_at(
                            &self.file_history_dir,
                            &reference,
                        )?)?;
                        let mut magic = [0; 8];
                        let count = file.read(&mut magic)?;
                        file.rewind()?;
                        if reference.version == 2 || (count == 8 && &magic == b"TSFENC1\0") {
                            PayloadSource::Container(file)
                        } else {
                            PayloadSource::Plain(file)
                        }
                    }
                    None => PayloadSource::Inline(stored),
                },
            };
            Ok((source, data_hash))
        })()
        .map_err(|error| PreviewError::PayloadUnavailable {
            entry_id: id,
            reason: error.to_string(),
        })?;
        Ok(PreparedPreview {
            metadata,
            source: source.0,
            data_hash: source.1,
        })
    }
}
