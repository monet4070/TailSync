use super::*;
use rusqlite::{params_from_iter, types::Value};

impl HistoryDB {
    pub fn get_all(
        &self,
        keyword: Option<&str>,
        category: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<HistoryEntry>, Box<dyn std::error::Error>> {
        self.get_all_filtered(keyword, category, None, None, limit, offset)
    }

    /// Query history by keyword, any assigned label, and an optional UTC time range.
    /// The start is inclusive and the end is exclusive.
    pub fn get_all_filtered(
        &self,
        keyword: Option<&str>,
        category: Option<&str>,
        start_time: Option<&str>,
        end_time: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<HistoryEntry>, Box<dyn std::error::Error>> {
        Ok(self
            .get_page_filtered(keyword, category, start_time, end_time, limit, offset)?
            .entries)
    }

    pub fn get_page_filtered(
        &self,
        keyword: Option<&str>,
        category: Option<&str>,
        start_time: Option<&str>,
        end_time: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<HistoryQueryPage, Box<dyn std::error::Error>> {
        self.get_page_in_collection(HistoryQuery {
            collection: HistoryCollection::All,
            keyword,
            category,
            start_time,
            end_time,
            limit,
            offset,
        })
    }

    pub fn get_page_in_collection(
        &self,
        query: HistoryQuery<'_>,
    ) -> Result<HistoryQueryPage, Box<dyn std::error::Error>> {
        let HistoryQuery {
            collection,
            keyword,
            category,
            start_time,
            end_time,
            limit,
            offset,
        } = query;
        let keyword = keyword.filter(|value| !value.trim().is_empty());
        if let Some(keyword) = keyword {
            let (entries, _, has_more) = self.scan_keyword_matches(
                collection,
                keyword,
                category,
                start_time,
                end_time,
                Some((limit, offset)),
            )?;
            return Ok(HistoryQueryPage {
                entries,
                total: None,
                has_more,
            });
        }

        let (filter_clause, mut values) =
            Self::history_filter_clause(collection, category, start_time, end_time)?;

        let mut sql = String::from(
            "SELECT id, timestamp, type, description, data_hash, size_bytes, source_peer,
                    category, category_confidence, classifier_version, categories,
                    pinned, batch_id, batch_index, batch_total, batch_status,
                    CASE WHEN batch_id IS NULL THEN NULL ELSE
                        (SELECT COUNT(*) FROM history AS batch_entries
                         WHERE batch_entries.batch_id = history.batch_id)
                    END AS batch_count,
                    CASE WHEN type = 'text' THEN data ELSE NULL END AS text_data
             FROM history",
        );
        sql.push_str(&filter_clause);
        sql.push_str(" ORDER BY timestamp DESC, id DESC LIMIT ? OFFSET ?");
        values.push(Value::Integer(i64::try_from(limit)?));
        values.push(Value::Integer(i64::try_from(offset)?));

        let rows = self
            .conn
            .prepare(&sql)?
            .query_map(params_from_iter(values.iter()), |row| {
                Ok((Self::row_to_entry(row)?, row.get::<_, Option<Vec<u8>>>(17)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut entries = Vec::with_capacity(rows.len());
        for (mut entry, stored) in rows {
            if let Some(stored) = stored {
                self.hydrate_text_description(&mut entry, &stored);
            }
            entries.push(entry);
        }
        let total = self.count_in_collection(collection, None, category, start_time, end_time)?;
        let has_more = offset
            .checked_add(entries.len())
            .is_some_and(|consumed| consumed < total);
        Ok(HistoryQueryPage {
            entries,
            total: Some(total),
            has_more,
        })
    }

    pub fn count_all_filtered(
        &self,
        keyword: Option<&str>,
        category: Option<&str>,
        start_time: Option<&str>,
        end_time: Option<&str>,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        self.count_in_collection(
            HistoryCollection::All,
            keyword,
            category,
            start_time,
            end_time,
        )
    }

    pub fn count_in_collection(
        &self,
        collection: HistoryCollection,
        keyword: Option<&str>,
        category: Option<&str>,
        start_time: Option<&str>,
        end_time: Option<&str>,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        if let Some(keyword) = keyword.filter(|value| !value.trim().is_empty()) {
            let (_, count, _) = self
                .scan_keyword_matches(collection, keyword, category, start_time, end_time, None)?;
            return Ok(count);
        }
        let (filter_clause, values) =
            Self::history_filter_clause(collection, category, start_time, end_time)?;
        let sql = format!("SELECT COUNT(*) FROM history{filter_clause}");
        let count: i64 = self
            .conn
            .query_row(&sql, params_from_iter(values.iter()), |row| row.get(0))?;
        Ok(count.max(0) as usize)
    }

    fn scan_keyword_matches(
        &self,
        collection: HistoryCollection,
        keyword: &str,
        category: Option<&str>,
        start_time: Option<&str>,
        end_time: Option<&str>,
        page: Option<(usize, usize)>,
    ) -> Result<(Vec<HistoryEntry>, usize, bool), Box<dyn std::error::Error>> {
        let (filter_clause, mut values) =
            Self::history_filter_clause(collection, category, start_time, end_time)?;
        let escaped = format!("%{}%", escape_like_literal(keyword));
        let candidate =
            "(type = 'text' OR description LIKE ? ESCAPE '\\' OR source_peer LIKE ? ESCAPE '\\'
              OR type LIKE ? ESCAPE '\\' OR category LIKE ? ESCAPE '\\'
              OR EXISTS (SELECT 1 FROM json_each(history.categories) AS label
                         WHERE label.value LIKE ? ESCAPE '\\'))";
        let where_clause = if filter_clause.is_empty() {
            format!(" WHERE {candidate}")
        } else {
            format!("{filter_clause} AND {candidate}")
        };
        for _ in 0..5 {
            values.push(Value::Text(escaped.clone()));
        }
        let sql = format!(
            "SELECT id, timestamp, type, description, data_hash, size_bytes, source_peer,
                    category, category_confidence, classifier_version, categories,
                    pinned, batch_id, batch_index, batch_total, batch_status,
                    CASE WHEN batch_id IS NULL THEN NULL ELSE
                        (SELECT COUNT(*) FROM history AS batch_entries
                         WHERE batch_entries.batch_id = history.batch_id)
                    END AS batch_count,
                    CASE WHEN type = 'text' THEN data ELSE NULL END AS text_data
             FROM history{where_clause} ORDER BY timestamp DESC, id DESC"
        );
        let mut statement = self.conn.prepare(&sql)?;
        let mut rows = statement.query(params_from_iter(values.iter()))?;
        let (limit, offset) = page.unwrap_or((0, 0));
        let mut entries = Vec::with_capacity(limit);
        let mut count = 0usize;
        let mut has_more = false;

        while let Some(row) = rows.next()? {
            let mut entry = Self::row_to_entry(row)?;
            let mut full_text_matches = false;
            if let Some(stored) = row.get::<_, Option<Vec<u8>>>(17)? {
                match self
                    .read_text_payload_compat(&stored)
                    .and_then(|data| Ok(String::from_utf8(data)?))
                {
                    Ok(text) => {
                        full_text_matches = text.to_lowercase().contains(&keyword.to_lowercase());
                        entry.description = text_preview(&text);
                    }
                    Err(error) => warn!(
                        "Could not decrypt history text preview for entry {}: {error}",
                        entry.id
                    ),
                }
            }
            if !full_text_matches && !Self::entry_metadata_matches_keyword(&entry, keyword) {
                continue;
            }
            count = count.saturating_add(1);
            let Some((_, _)) = page else {
                continue;
            };
            if count <= offset {
                continue;
            }
            if entries.len() == limit {
                has_more = true;
                break;
            }
            entries.push(entry);
        }
        Ok((entries, count, has_more))
    }

    fn history_filter_clause(
        collection: HistoryCollection,
        category: Option<&str>,
        start_time: Option<&str>,
        end_time: Option<&str>,
    ) -> Result<(String, Vec<Value>), Box<dyn std::error::Error>> {
        let category = category.filter(|value| !value.is_empty() && *value != "all");
        if let Some(category) =
            category.filter(|value| !history_classifier::is_known_category(value))
        {
            return Err(format!("Unsupported history category: {category}").into());
        }

        let parse_bound = |value: Option<&str>, name: &str| {
            value
                .map(|value| {
                    chrono::DateTime::parse_from_rfc3339(value)
                        .map(|date| date.with_timezone(&chrono::Utc))
                        .map_err(|_| format!("Invalid {name}: expected an RFC 3339 timestamp"))
                })
                .transpose()
        };
        let start_time = parse_bound(start_time, "start_time")?;
        let end_time = parse_bound(end_time, "end_time")?;
        if start_time
            .as_ref()
            .zip(end_time.as_ref())
            .is_some_and(|(start, end)| start >= end)
        {
            return Err("start_time must be earlier than end_time".into());
        }

        let mut conditions = Vec::new();
        let mut values = Vec::<Value>::new();
        if collection == HistoryCollection::Favorites {
            conditions.push("history.pinned = 1");
        }
        if let Some(category) = category {
            conditions.push(
                "EXISTS (SELECT 1 FROM json_each(history.categories) AS label
                         WHERE label.value = ?)",
            );
            values.push(Value::Text(category.to_string()));
        }
        if let Some(start_time) = start_time {
            conditions.push("julianday(timestamp) >= julianday(?)");
            values.push(Value::Text(start_time.to_rfc3339()));
        }
        if let Some(end_time) = end_time {
            conditions.push("julianday(timestamp) < julianday(?)");
            values.push(Value::Text(end_time.to_rfc3339()));
        }
        let filter_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", conditions.join(" AND "))
        };
        Ok((filter_clause, values))
    }

    fn hydrate_text_description(&self, entry: &mut HistoryEntry, stored: &[u8]) {
        if entry.entry_type != "text" {
            return;
        }
        match self
            .read_text_payload_compat(stored)
            .and_then(|data| Ok(text_preview(std::str::from_utf8(&data)?)))
        {
            Ok(description) => entry.description = description,
            Err(error) => warn!(
                "Could not decrypt history text preview for entry {}: {error}",
                entry.id
            ),
        }
    }

    fn entry_metadata_matches_keyword(entry: &HistoryEntry, keyword: &str) -> bool {
        let keyword = keyword.to_lowercase();
        [
            entry.description.as_str(),
            entry.source_peer.as_str(),
            entry.entry_type.as_str(),
            entry.category.as_str(),
        ]
        .into_iter()
        .chain(entry.categories.iter().map(String::as_str))
        .any(|value| value.to_lowercase().contains(&keyword))
    }

    fn row_to_entry(row: &rusqlite::Row) -> Result<HistoryEntry, rusqlite::Error> {
        let category = row.get::<_, String>(7)?;
        let encoded_categories = row.get::<_, String>(10)?;
        let mut categories = serde_json::from_str::<Vec<String>>(&encoded_categories)
            .unwrap_or_else(|_| vec![category.clone()]);
        categories.retain(|label| history_classifier::is_known_category(label));
        categories.dedup();
        if !categories.iter().any(|label| label == &category) {
            categories.insert(0, category.clone());
        }
        Ok(HistoryEntry {
            id: row.get(0)?,
            timestamp: row.get(1)?,
            entry_type: row.get(2)?,
            description: row.get(3)?,
            data_hash: row.get(4)?,
            size_bytes: row.get(5)?,
            source_peer: row.get(6)?,
            category,
            categories,
            category_confidence: row.get(8)?,
            classifier_version: row.get(9)?,
            pinned: row.get::<_, i64>(11)? != 0,
            batch_id: row.get(12)?,
            batch_index: row.get(13)?,
            batch_total: row.get(14)?,
            batch_status: row.get(15)?,
            batch_count: row.get(16)?,
        })
    }
}

fn escape_like_literal(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}
