use super::*;

impl HistoryDB {
    /// Read the renderer and navigation metadata without decrypting or
    /// materialising the entry payload.
    pub fn get_preview_metadata(&self, id: i64) -> Result<PreviewMetadata, PreviewError> {
        let row = self
            .conn
            .query_row(
                "SELECT type, description, size_bytes, batch_id
                 FROM history WHERE id = ?1",
                params![id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| PreviewError::MetadataUnavailable {
                entry_id: id,
                reason: error.to_string(),
            })?
            .ok_or(PreviewError::EntryNotFound { entry_id: id })?;

        let (entry_type, description, declared_size, batch_id) = row;
        let size_bytes = checked_preview_size(declared_size)?;
        let (kind, name) = preview_kind_and_name(&entry_type, &description)?;
        let batch = batch_id
            .as_deref()
            .map(|batch_id| self.get_preview_batch_navigation(batch_id, id))
            .transpose()?;

        Ok(PreviewMetadata {
            entry_id: id,
            kind,
            name,
            size_bytes,
            batch,
        })
    }

    /// Return navigation for one file in a batch without loading any payload.
    pub fn get_preview_batch_navigation(
        &self,
        batch_id: &str,
        entry_id: i64,
    ) -> Result<PreviewBatchNavigation, PreviewError> {
        let load_ids = || -> rusqlite::Result<Vec<i64>> {
            self.conn
                .prepare(
                    "SELECT id FROM history
                     WHERE batch_id = ?1 AND type = 'file'
                     ORDER BY batch_index ASC, id ASC",
                )?
                .query_map(params![batch_id], |row| row.get(0))?
                .collect()
        };
        let entry_ids = load_ids().map_err(|error| PreviewError::MetadataUnavailable {
            entry_id,
            reason: error.to_string(),
        })?;
        if entry_ids.is_empty() {
            return Err(PreviewError::BatchNotFound {
                batch_id: batch_id.to_string(),
            });
        }
        let item_index = entry_ids
            .iter()
            .position(|candidate| *candidate == entry_id)
            .ok_or_else(|| PreviewError::EntryNotInBatch {
                entry_id,
                batch_id: batch_id.to_string(),
            })?;

        Ok(PreviewBatchNavigation {
            batch_id: batch_id.to_string(),
            item_index,
            item_count: entry_ids.len(),
            first_entry_id: entry_ids[0],
            last_entry_id: entry_ids[entry_ids.len() - 1],
            previous_entry_id: item_index
                .checked_sub(1)
                .and_then(|index| entry_ids.get(index).copied()),
            next_entry_id: entry_ids.get(item_index + 1).copied(),
        })
    }

    /// Load a bounded, decrypted payload for an in-memory history preview.
    ///
    /// This deliberately does not call `get_file_path` or any materialisation
    /// helper: preview callers must never create a plaintext file on disk.
    /// The typed error is part of the platform IPC contract; callers should
    /// never have to infer retryability from an error string.
    pub fn get_preview_payload(&self, id: i64) -> Result<PreviewPayload, PreviewError> {
        let metadata = self.get_preview_metadata(id)?;
        let data = self
            .get_data(id)
            .map_err(|error| PreviewError::PayloadUnavailable {
                entry_id: id,
                reason: error.to_string(),
            })?;
        let size_bytes = u64::try_from(data.len()).unwrap_or(u64::MAX);
        if size_bytes > PREVIEW_MAX_BYTES {
            return Err(PreviewError::PreviewTooLarge {
                size: size_bytes,
                limit: PREVIEW_MAX_BYTES,
            });
        }

        Ok(PreviewPayload {
            kind: metadata.kind.as_str().to_string(),
            name: metadata.name,
            size_bytes,
            data,
        })
    }

    /// Reads current encrypted text and the short-lived v3 format that stored
    /// migrated text payloads behind an image-reference envelope.
    pub(super) fn read_text_payload_compat(
        &self,
        stored: &[u8],
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        if let Some(reference) = decode_image_reference(stored) {
            let encrypted = std::fs::read(resolve_file_reference_at(
                &self.image_history_dir,
                &reference,
            )?)?;
            return crypto::decrypt(&encrypted);
        }
        crypto::decrypt(stored)
    }
}
