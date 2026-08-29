use super::*;

impl HistoryDB {
    // ── Migration inserts (pre-set timestamp + description) ──────
    pub fn add_text_migrated(
        &mut self,
        time: &str,
        _desc: &str,
        data: &[u8],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let data_hash = blake3::hash(data).to_hex().to_string();
        if self.exists_by_hash(&data_hash)? {
            return Ok(());
        }
        let encrypted = crypto::encrypt(data)?;
        let classification = std::str::from_utf8(data)
            .map(history_classifier::classify_text)
            .unwrap_or(Classification {
                category: "text",
                confidence: 0,
                secondary_category: None,
            });
        let categories = serde_json::to_string(&classification.categories())?;
        self.conn.execute(
            "INSERT INTO history
                (timestamp, type, description, data, size_bytes, source_peer, data_hash,
                 category, categories, category_confidence, classifier_version)
             VALUES (?1, 'text', ?2, ?3, ?4, 'migrated', ?5, ?6, ?7, ?8, ?9)",
            params![
                time,
                TEXT_DESCRIPTION_PLACEHOLDER,
                encrypted,
                data.len() as i64,
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

    pub fn add_image_migrated(
        &mut self,
        time: &str,
        desc: &str,
        data: &[u8],
    ) -> Result<(), Box<dyn std::error::Error>> {
        crate::protocol::PackedImage::try_from(data)?;
        let data_hash = blake3::hash(data).to_hex().to_string();
        if self.exists_by_hash(&data_hash)? {
            return Ok(());
        }
        let encrypted = crypto::encrypt(data)?;
        self.conn.execute(
            "INSERT INTO history
                (timestamp, type, description, data, size_bytes, source_peer, data_hash,
                 category, categories, category_confidence, classifier_version)
             VALUES (?1, 'image', ?2, ?3, ?4, 'migrated', ?5, 'image', '[\"image\"]', 100, ?6)",
            params![
                time,
                desc,
                encrypted,
                data.len() as i64,
                data_hash,
                history_classifier::CLASSIFIER_VERSION,
            ],
        )?;
        self.trim("image")?;
        Ok(())
    }

    pub fn add_file_migrated(
        &mut self,
        time: &str,
        desc: &str,
        data: &[u8],
    ) -> Result<(), Box<dyn std::error::Error>> {
        validate_history_file_size(data.len() as u64)?;
        let data_hash = blake3::hash(data).to_hex().to_string();
        if self.exists_by_hash(&data_hash)? {
            return Ok(());
        }
        let (reference, _) =
            persist_history_file_at(&self.file_history_dir, &data_hash, desc, data)?;
        self.conn.execute(
            "INSERT INTO history
                (timestamp, type, description, data, size_bytes, source_peer, data_hash,
                 category, categories, category_confidence, classifier_version)
             VALUES (?1, 'file', ?2, ?3, ?4, 'migrated', ?5, 'file', '[\"file\"]', 100, ?6)",
            params![
                time,
                desc,
                reference,
                data.len() as i64,
                data_hash,
                history_classifier::CLASSIFIER_VERSION,
            ],
        )?;
        self.trim("file")?;
        Ok(())
    }

    pub fn add_file_migrated_from_path(
        &mut self,
        time: &str,
        desc: &str,
        source: &Path,
        data_hash: &str,
        size: u64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        validate_history_file_size(size)?;
        if self.exists_by_hash(data_hash)? {
            return Ok(());
        }
        let (reference, _) = persist_history_file_from_path_at(
            &self.file_history_dir,
            data_hash,
            desc,
            source,
            size,
            true,
        )?;
        self.conn.execute(
            "INSERT INTO history
                (timestamp, type, description, data, size_bytes, source_peer, data_hash,
                 category, categories, category_confidence, classifier_version)
             VALUES (?1, 'file', ?2, ?3, ?4, 'migrated', ?5, 'file', '[\"file\"]', 100, ?6)",
            params![
                time,
                desc,
                reference,
                size as i64,
                data_hash,
                history_classifier::CLASSIFIER_VERSION,
            ],
        )?;
        self.trim("file")?;
        Ok(())
    }
}
