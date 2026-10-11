use super::*;
use crate::cancellation::Cancellation;
use crate::composite::display::{DisplayClassification, DISPLAY_CLASSIFIER_VERSION};

fn display_kind_for(entry_type: &str, classifier_version: i64, confidence: i64) -> &'static str {
    match entry_type {
        "image" => "image",
        "file" => "file",
        "text" => "text",
        "composite" if classifier_version == DISPLAY_CLASSIFIER_VERSION => match confidence {
            95 => "image",
            90 => "document",
            85 => "text",
            _ => "unknown",
        },
        _ => "unknown",
    }
}

impl HistoryDB {
    pub fn document_owner_current(&self, id: i64, owner: &str) -> bool {
        matches!(owner, "history" | "favorites")
            && (owner == "history" || self.is_logical_item_favorite(&[id]).unwrap_or(false))
    }
    pub fn display_rebuild_candidates(
        &self,
        limit: usize,
    ) -> Result<Vec<i64>, Box<dyn std::error::Error>> {
        Ok(self.conn.prepare("SELECT id FROM history WHERE type='composite' AND classifier_version != ?1 ORDER BY id LIMIT ?2")?
            .query_map(params![DISPLAY_CLASSIFIER_VERSION, limit.clamp(1, 4) as i64], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?)
    }

    pub fn commit_display_classification(
        &mut self,
        handle: &AnalysisHandle,
        classification: &DisplayClassification,
        cancellation: &Cancellation,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        cancellation.check()?;
        if !self.verify_content_analysis(handle)? {
            return Ok(false);
        }
        let expected: Vec<u8> = self.conn.query_row(
            "SELECT data FROM history WHERE id=?1 AND type='composite'",
            [handle.entry_id],
            |row| row.get(0),
        )?;
        if !self.verify_content_analysis(handle)? {
            return Ok(false);
        }
        let transaction = self.conn.transaction()?;
        let changed = transaction.execute("UPDATE history SET categories=?1,category_confidence=?2,classifier_version=?3 WHERE id=?4 AND type='composite' AND data=?5", params![classification.categories_json(), classification.confidence, classification.version, handle.entry_id, expected])?;
        cancellation.check()?;
        transaction.commit()?;
        Ok(changed == 1)
    }

    pub fn commit_unavailable_display(
        &mut self,
        id: i64,
        revision: &HistoryReadRevision,
        cancellation: &Cancellation,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        cancellation.check()?;
        if self.validate_read_revision(revision).is_err() {
            return Ok(false);
        }
        let transaction = self.conn.transaction()?;
        let changed = transaction.execute("UPDATE history SET categories='[\"text\",\"image\"]',category_confidence=0,classifier_version=?1 WHERE id=?2 AND type='composite' AND classifier_version != ?1", params![DISPLAY_CLASSIFIER_VERSION,id])?;
        cancellation.check()?;
        transaction.commit()?;
        Ok(changed == 1)
    }

    pub fn thumbnail_key(&self, id: i64) -> Result<String, Box<dyn std::error::Error>> {
        let (entry_type, timestamp, hash, version, confidence): (String, String, String, i64, i64) = self.conn.query_row("SELECT type,timestamp,data_hash,classifier_version,category_confidence FROM history WHERE id=?1", [id], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?)))?;
        let manifest = if entry_type == "composite" {
            let (size, stored): (i64, Option<Vec<u8>>) = self.conn.query_row("SELECT length(data), CASE WHEN length(data)<=8388736 THEN data ELSE NULL END FROM history WHERE id=?1", [id], |row| Ok((row.get(0)?,row.get(1)?)))?;
            Some((
                size,
                stored.map(|bytes| blake3::hash(&bytes).to_hex().to_string()),
            ))
        } else {
            None
        };
        let encoded = serde_json::to_vec(&(
            self.display_authority.to_string(),
            id,
            entry_type,
            timestamp,
            hash,
            manifest,
            version,
            confidence,
            crate::composite::content_analysis::ANALYSIS_VERSION,
            1u16,
            48u16,
        ))?;
        Ok(format!("th1:{}", blake3::hash(&encoded).to_hex()))
    }

    pub fn document_key(&self, id: i64) -> Result<String, Box<dyn std::error::Error>> {
        let (entry_type, timestamp, hash, version, confidence): (String, String, String, i64, i64) = self.conn.query_row("SELECT type,timestamp,data_hash,classifier_version,category_confidence FROM history WHERE id=?1", [id], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?)))?;
        let manifest = if entry_type == "composite" {
            let (size, stored): (i64, Option<Vec<u8>>) = self.conn.query_row("SELECT length(data), CASE WHEN length(data)<=8388736 THEN data ELSE NULL END FROM history WHERE id=?1", [id], |row| Ok((row.get(0)?,row.get(1)?)))?;
            Some((
                size,
                stored.map(|bytes| blake3::hash(&bytes).to_hex().to_string()),
            ))
        } else {
            None
        };
        let encoded = serde_json::to_vec(&(
            self.display_authority.to_string(),
            id,
            entry_type,
            timestamp,
            hash,
            manifest,
            version,
            confidence,
            crate::composite::content_analysis::ANALYSIS_VERSION,
            crate::composite::display_document::DOCUMENT_VERSION,
            1u16,
        ))?;
        Ok(format!("dc1:{}", blake3::hash(&encoded).to_hex()))
    }

    pub fn display_kind(&self, id: i64) -> Result<String, Box<dyn std::error::Error>> {
        let (entry_type, classifier_version, confidence): (String, i64, i64) =
            self.conn.query_row(
                "SELECT type,classifier_version,category_confidence FROM history WHERE id=?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?;
        Ok(display_kind_for(&entry_type, classifier_version, confidence).to_string())
    }

    pub fn attach_display_info(
        &self,
        entry: &mut HistoryEntry,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let kind = display_kind_for(
            &entry.entry_type,
            entry.classifier_version,
            entry.category_confidence,
        );
        // `category` in storage remains the retention bucket. Composite text
        // presents its derived primary label without changing quotas or trimming.
        if entry.entry_type == "composite" {
            match kind {
                "text" => {
                    entry.category = entry
                        .categories
                        .first()
                        .cloned()
                        .unwrap_or_else(|| "text".into());
                }
                "image" => entry.category = "image".into(),
                _ => {}
            }
        }
        let thumbnail_eligible = entry.entry_type == "image"
            || (entry.entry_type == "composite"
                && (kind == "unknown"
                    || entry.categories.iter().any(|category| category == "image")));
        entry.display = Some(HistoryDisplayInfo {
            version: 1,
            kind: kind.into(),
            thumbnail_eligible,
            cache_key: self.thumbnail_key(entry.id)?,
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composite::{Completeness, Mime, PortableBundle, Representation};

    fn store() -> (std::path::PathBuf, HistoryDB, i64) {
        let path =
            std::env::temp_dir().join(format!("tailsync-display-core-{}", rand::random::<u128>()));
        let mut database = HistoryDB::open_isolated_for_test_composite(&path).unwrap();
        let bundle = PortableBundle {
            representations: vec![Representation {
                mime: Mime::Html,
                bytes: b"<b>body</b>".to_vec(),
            }],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::Complete,
        };
        let id = database.add_composite(&bundle, None, "fixture").unwrap();
        (path, database, id)
    }

    #[test]
    fn unavailable_display_is_revision_checked_and_preserves_corrupt_original() {
        let (path, mut database, id) = store();
        let stamp = database.read_revision().unwrap();
        database
            .conn
            .execute("UPDATE history SET data=X'424144' WHERE id=?1", [id])
            .unwrap();
        assert!(!database
            .commit_unavailable_display(id, &stamp, &Cancellation::default())
            .unwrap());
        let stamp = database.read_revision().unwrap();
        assert!(database
            .commit_unavailable_display(id, &stamp, &Cancellation::default())
            .unwrap());
        let stored: Vec<u8> = database
            .conn
            .query_row("SELECT data FROM history WHERE id=?1", [id], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(stored, b"BAD");
        assert!(database.display_rebuild_candidates(4).unwrap().is_empty());
        let mut entry = database.get_all(None, None, 10, 0).unwrap().remove(0);
        database.attach_display_info(&mut entry).unwrap();
        assert_eq!(entry.display.unwrap().kind, "unknown");
        assert_eq!(database.display_kind(id).unwrap(), "unknown");
        let key = database.thumbnail_key(id).unwrap();
        database
            .conn
            .execute("UPDATE history SET data=X'424145' WHERE id=?1", [id])
            .unwrap();
        assert_ne!(key, database.thumbnail_key(id).unwrap());
        drop(database);
        std::fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn old_document_display_rebuilds_without_rewriting_the_capture() {
        let (path, mut database, id) = store();
        database.set_favorite(id, true).unwrap();
        let original = database.get_composite(id).unwrap();
        let identity = database.composite_identity(id).unwrap();
        database.conn.execute("UPDATE history SET categories='[\"text\"]',category_confidence=90,classifier_version=1002 WHERE id=?1", [id]).unwrap();
        assert_eq!(database.display_rebuild_candidates(4).unwrap(), vec![id]);
        let prepared = database.prepare_content_analysis(id).unwrap().unwrap();
        let classification = crate::composite::display::classify_bundle(&original, false, vec![]);
        assert!(database
            .commit_display_classification(
                &prepared.handle(),
                &classification,
                &Cancellation::default()
            )
            .unwrap());
        assert!(database.display_rebuild_candidates(4).unwrap().is_empty());
        let mut entry = database.get_all(None, None, 10, 0).unwrap().remove(0);
        database.attach_display_info(&mut entry).unwrap();
        assert_eq!(entry.display.unwrap().kind, "text");
        assert!(entry.pinned);
        assert_eq!(database.get_composite(id).unwrap(), original);
        assert_eq!(database.composite_identity(id).unwrap(), identity);
        drop(database);
        std::fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn derived_classification_never_changes_storage_bucket_or_trims() {
        let (path, mut database, id) = store();
        database.max_history = 0;
        let original = database.get_composite(id).unwrap();
        let prepared = database.prepare_content_analysis(id).unwrap().unwrap();
        let classification = crate::composite::display::classify_bundle(&original, false, vec![]);
        assert!(database
            .commit_display_classification(
                &prepared.handle(),
                &classification,
                &Cancellation::default()
            )
            .unwrap());
        assert_eq!(database.get_composite(id).unwrap(), original);
        let category: String = database
            .conn
            .query_row("SELECT category FROM history WHERE id=?1", [id], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(category, "text");
        assert_eq!(database.display_kind(id).unwrap(), "text");
        assert_eq!(database.get_all(None, None, 10, 0).unwrap().len(), 1);
        drop(database);
        std::fs::remove_dir_all(path).unwrap();
    }
}
