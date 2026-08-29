use super::*;

impl HistoryDB {
    pub fn backfill_classifications(
        &mut self,
        limit: usize,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        let media_updated = self.conn.execute(
            "UPDATE history
             SET category = type,
                 categories = json_array(type),
                 category_confidence = 100,
                 classifier_version = ?1
             WHERE type IN ('image', 'file')
               AND (classifier_version < ?1 OR category != type
                    OR categories != json_array(type) OR category_confidence != 100)",
            params![history_classifier::CLASSIFIER_VERSION],
        )?;
        let rows = self
            .conn
            .prepare(
                "SELECT id, data FROM history
                 WHERE type = 'text' AND classifier_version < ?1
                 ORDER BY id ASC LIMIT ?2",
            )?
            .query_map(
                params![history_classifier::CLASSIFIER_VERSION, limit as i64],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?)),
            )?
            .collect::<Result<Vec<_>, _>>()?;
        if rows.is_empty() {
            return Ok(media_updated);
        }

        let mut updates = Vec::with_capacity(rows.len());
        for (id, stored) in rows {
            let legacy_reference = decode_image_reference(&stored);
            let (classification, replacement) = match self.read_text_payload_compat(&stored) {
                Ok(data) => match std::str::from_utf8(&data) {
                    Ok(text) => (
                        history_classifier::classify_text(text),
                        if legacy_reference.is_some() {
                            Some(crypto::encrypt(&data)?)
                        } else {
                            None
                        },
                    ),
                    Err(error) => {
                        warn!("History text entry {id} is not UTF-8: {error}");
                        (
                            Classification {
                                category: "text",
                                confidence: 0,
                                secondary_category: None,
                            },
                            None,
                        )
                    }
                },
                Err(error) => {
                    if error
                        .downcast_ref::<std::io::Error>()
                        .is_some_and(|io_error| io_error.kind() != std::io::ErrorKind::NotFound)
                    {
                        return Err(error);
                    }
                    warn!("History text entry {id} could not be classified: {error}");
                    (
                        Classification {
                            category: "text",
                            confidence: 0,
                            secondary_category: None,
                        },
                        None,
                    )
                }
            };
            updates.push((id, classification, replacement, legacy_reference));
        }

        let tx = self.conn.transaction()?;
        for (id, classification, replacement, _) in &updates {
            let categories = serde_json::to_string(&classification.categories())?;
            if let Some(encrypted) = replacement {
                tx.execute(
                    "UPDATE history
                     SET data = ?1, category = ?2, categories = ?3,
                         category_confidence = ?4, classifier_version = ?5
                     WHERE id = ?6",
                    params![
                        encrypted,
                        classification.category,
                        categories,
                        classification.confidence as i64,
                        history_classifier::CLASSIFIER_VERSION,
                        id,
                    ],
                )?;
            } else {
                tx.execute(
                    "UPDATE history
                     SET category = ?1, categories = ?2,
                         category_confidence = ?3, classifier_version = ?4
                     WHERE id = ?5",
                    params![
                        classification.category,
                        categories,
                        classification.confidence as i64,
                        history_classifier::CLASSIFIER_VERSION,
                        id,
                    ],
                )?;
            }
        }
        tx.commit()?;

        for (_, _, replacement, reference) in &updates {
            if replacement.is_none() {
                continue;
            }
            if let Some(reference) = reference {
                let path = resolve_file_reference_at(&self.image_history_dir, reference)?;
                let remaining: i64 = self.conn.query_row(
                    "SELECT COUNT(*) FROM history WHERE data = ?1",
                    params![encode_image_reference(&reference.file_name)?],
                    |row| row.get(0),
                )?;
                if remaining == 0 {
                    if let Err(error) = std::fs::remove_file(&path) {
                        if error.kind() != std::io::ErrorKind::NotFound {
                            warn!(
                                "Could not remove migrated text reference {}: {error}",
                                path.display()
                            );
                        }
                    }
                }
            }
        }
        Ok(media_updated + updates.len())
    }
}
