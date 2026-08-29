use super::*;

impl HistoryDB {
    /// Add a text entry to history. Duplicate: delete old, insert new at top.
    pub fn add_text(
        &mut self,
        text: &str,
        source_peer: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let data_hash = blake3::hash(text.as_bytes()).to_hex().to_string();
        // Delete old entry with same hash so the new one is at the top
        let duplicate_ids = self.entry_ids_by_hash(&data_hash)?;
        self.delete_entries(&duplicate_ids)?;

        let encrypted = crypto::encrypt(text.as_bytes())?;
        let timestamp = chrono::Utc::now().to_rfc3339();
        let classification = history_classifier::classify_text(text);
        let categories = serde_json::to_string(&classification.categories())?;

        self.conn.execute(
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
        // Delete old entry so the new copy appears at the top
        let duplicate_ids = self.entry_ids_by_hash(&data_hash)?;
        self.delete_entries(&duplicate_ids)?;

        let reference = persist_image_at(&self.image_history_dir, &data_hash, image_data)?;
        let timestamp = chrono::Utc::now().to_rfc3339();
        let description = format!("Image {} bytes", image_data.len());

        self.conn.execute(
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
        )?;

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
}
