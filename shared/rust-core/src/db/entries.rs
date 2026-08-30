use super::*;
use rusqlite::Transaction;

pub(super) struct ExternalHistoryPayload {
    pub(super) stored: Vec<u8>,
    pub(super) path: PathBuf,
}

impl HistoryDB {
    /// Add a text entry to history. Duplicate: delete old, insert new at top.
    pub fn add_text(
        &mut self,
        text: &str,
        source_peer: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let data_hash = blake3::hash(text.as_bytes()).to_hex().to_string();
        // Replace the duplicate and insert the new row in one SQLite
        // transaction so a process exit cannot leave the history gap between
        // those two operations.
        let duplicate_ids = self.unfavorited_duplicate_ids(&self.entry_ids_by_hash(&data_hash)?)?;

        let encrypted = crypto::encrypt(text.as_bytes())?;
        let timestamp = chrono::Utc::now().to_rfc3339();
        let classification = history_classifier::classify_text(text);
        let categories = serde_json::to_string(&classification.categories())?;

        let tx = self.conn.transaction()?;
        Self::delete_rows_in_transaction(&tx, &duplicate_ids)?;
        tx.execute(
            "INSERT INTO history
                (timestamp, type, description, data, size_bytes, source_peer, data_hash,
                 category, categories, category_confidence, classifier_version)
             VALUES (?1, 'text', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                timestamp,
                TEXT_DESCRIPTION_PLACEHOLDER,
                encrypted,
                text.len() as i64,
                source_peer,
                data_hash,
                classification.category,
                categories,
                classification.confidence as i64,
                history_classifier::CLASSIFIER_VERSION,
            ],
        )?;
        tx.commit()?;

        self.trim("text")?;
        Ok(())
    }

    /// Add an image entry to history. Duplicate: delete old, insert new.
    pub fn add_image(
        &mut self,
        image_data: &[u8],
        source_peer: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        crate::protocol::PackedImage::try_from(image_data)?;
        let data_hash = blake3::hash(image_data).to_hex().to_string();
        // Keep the payload outside SQLite, but make row replacement and row
        // insertion atomic. Old payloads are removed only after commit.
        let duplicate_ids = self.unfavorited_duplicate_ids(&self.entry_ids_by_hash(&data_hash)?)?;
        let reference = persist_image_at(&self.image_history_dir, &data_hash, image_data)?;
        let new_path = decode_image_reference(&reference)
            .map(|reference| resolve_file_reference_at(&self.image_history_dir, &reference))
            .transpose()?
            .ok_or("Could not resolve persisted image reference")?;
        let old_payloads = self.external_payloads_for_ids(&duplicate_ids)?;
        let timestamp = chrono::Utc::now().to_rfc3339();
        let description = format!("Image {} bytes", image_data.len());

        let tx = self.conn.transaction()?;
        Self::delete_rows_in_transaction(&tx, &duplicate_ids)?;
        let write_result = tx.execute(
            "INSERT INTO history
                (timestamp, type, description, data, size_bytes, source_peer, data_hash,
                 category, categories, category_confidence, classifier_version)
             VALUES (?1, 'image', ?2, ?3, ?4, ?5, ?6, 'image', '[\"image\"]', 100, ?7)",
            params![
                timestamp,
                description,
                reference,
                image_data.len() as i64,
                source_peer,
                data_hash,
                history_classifier::CLASSIFIER_VERSION,
            ],
        );
        if let Err(error) = write_result {
            drop(tx);
            let mut payloads = old_payloads;
            payloads.push(ExternalHistoryPayload {
                stored: reference,
                path: new_path,
            });
            self.cleanup_external_payloads(&payloads, None);
            return Err(error.into());
        }
        if let Err(error) = tx.commit() {
            let mut payloads = old_payloads;
            payloads.push(ExternalHistoryPayload {
                stored: reference,
                path: new_path.clone(),
            });
            self.cleanup_external_payloads(&payloads, None);
            return Err(error.into());
        }
        self.cleanup_external_payloads(&old_payloads, Some(&new_path));

        self.trim("image")?;
        Ok(())
    }

    /// Check if an entry with the given hash already exists
    pub(super) fn exists_by_hash(&self, hash: &str) -> Result<bool, Box<dyn std::error::Error>> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM history WHERE data_hash = ?1",
            params![hash],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Get history entries with optional keyword search
    /// Get entry bytes. File entries backed by the history folder are read
    /// from disk; legacy file blobs remain readable through decryption.
    pub fn get_data(&self, id: i64) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let (entry_type, stored): (String, Vec<u8>) = self.conn.query_row(
            "SELECT type, data FROM history WHERE id = ?1",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if entry_type == "text" {
            return self.read_text_payload_compat(&stored);
        }
        if entry_type == "file" {
            if let Some(reference) = decode_file_reference(&stored) {
                let path = resolve_file_reference_at(&self.file_history_dir, &reference)?;
                if reference.version == 2 || file_encryption::is_encrypted_file(&path)? {
                    return file_encryption::decrypt_file_to_vec(&path);
                }
                return Ok(std::fs::read(path)?);
            }
        }
        if entry_type == "image" {
            if let Some(reference) = decode_image_reference(&stored) {
                let encrypted = std::fs::read(resolve_file_reference_at(
                    &self.image_history_dir,
                    &reference,
                )?)?;
                return crypto::decrypt(&encrypted);
            }
        }
        crypto::decrypt(&stored)
    }

    pub(super) fn external_payloads_for_ids(
        &self,
        ids: &[i64],
    ) -> Result<Vec<ExternalHistoryPayload>, Box<dyn std::error::Error>> {
        let mut payloads = Vec::new();
        for id in ids {
            let stored = self.conn.query_row(
                "SELECT type, data FROM history WHERE id = ?1",
                params![id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
            );
            let Ok((entry_type, stored)) = stored else {
                continue;
            };
            let reference = match entry_type.as_str() {
                "file" => decode_file_reference(&stored),
                "image" => decode_image_reference(&stored),
                _ => None,
            };
            if let Some(reference) = reference {
                let directory = if entry_type == "file" {
                    &self.file_history_dir
                } else {
                    &self.image_history_dir
                };
                payloads.push(ExternalHistoryPayload {
                    stored,
                    path: resolve_file_reference_at(directory, &reference)?,
                });
            }
        }
        Ok(payloads)
    }

    pub(super) fn delete_rows_in_transaction(
        tx: &Transaction<'_>,
        ids: &[i64],
    ) -> Result<(), Box<dyn std::error::Error>> {
        if ids.is_empty() {
            return Ok(());
        }
        let mut affected_batches = std::collections::BTreeSet::new();
        for id in ids {
            let batch_id: Option<String> = tx.query_row(
                "SELECT batch_id FROM history WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )?;
            if let Some(batch_id) = batch_id {
                affected_batches.insert(batch_id);
            }
            tx.execute("DELETE FROM history WHERE id = ?1", params![id])?;
        }
        for batch_id in affected_batches {
            tx.execute(
                "UPDATE history SET batch_status = 'incomplete'
                 WHERE batch_id = ?1
                   AND (SELECT COUNT(*) FROM history WHERE batch_id = ?1) < batch_total",
                params![batch_id],
            )?;
        }
        Ok(())
    }

    pub(super) fn cleanup_external_payloads(
        &self,
        payloads: &[ExternalHistoryPayload],
        preserve_path: Option<&Path>,
    ) {
        for payload in payloads {
            if preserve_path == Some(payload.path.as_path()) {
                continue;
            }
            let remaining = self.conn.query_row(
                "SELECT COUNT(*) FROM history WHERE data = ?1",
                params![&payload.stored],
                |row| row.get::<_, i64>(0),
            );
            if matches!(remaining, Ok(0)) {
                if let Err(error) = std::fs::remove_file(&payload.path) {
                    if error.kind() != std::io::ErrorKind::NotFound {
                        warn!(
                            "Could not remove unreferenced history payload {}: {error}",
                            payload.path.display()
                        );
                    }
                }
            }
        }
    }
}
