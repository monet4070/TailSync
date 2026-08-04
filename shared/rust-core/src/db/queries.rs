use super::*;

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
        let (filter_clause, mut values) =
            Self::history_filter_clause(keyword, category, start_time, end_time)?;

        let mut sql = String::from(
            "SELECT id, timestamp, type, description, data_hash, size_bytes, source_peer,
                    category, category_confidence, classifier_version, categories
             FROM history",
        );
        sql.push_str(&filter_clause);
        sql.push_str(" ORDER BY timestamp DESC, id DESC LIMIT ? OFFSET ?");
        values.push(Value::Integer(limit as i64));
        values.push(Value::Integer(offset as i64));

        let entries = self
            .conn
            .prepare(&sql)?
            .query_map(params_from_iter(values.iter()), Self::row_to_entry)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(entries)
    }

    pub fn count_all_filtered(
        &self,
        keyword: Option<&str>,
        category: Option<&str>,
        start_time: Option<&str>,
        end_time: Option<&str>,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        let (filter_clause, values) =
            Self::history_filter_clause(keyword, category, start_time, end_time)?;
        let sql = format!("SELECT COUNT(*) FROM history{filter_clause}");
        let count: i64 = self
            .conn
            .query_row(&sql, params_from_iter(values.iter()), |row| row.get(0))?;
        Ok(count.max(0) as usize)
    }

    fn history_filter_clause(
        keyword: Option<&str>,
        category: Option<&str>,
        start_time: Option<&str>,
        end_time: Option<&str>,
    ) -> Result<(String, Vec<Value>), Box<dyn std::error::Error>> {
        let keyword = keyword.filter(|value| !value.trim().is_empty());
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
        if let Some(keyword) = keyword {
            let pattern = format!("%{}%", escape_like_literal(keyword));
            conditions.push(
                "(description LIKE ? ESCAPE '\\' OR source_peer LIKE ? ESCAPE '\\'
                  OR type LIKE ? ESCAPE '\\' OR category LIKE ? ESCAPE '\\'
                  OR EXISTS (SELECT 1 FROM json_each(history.categories) AS label
                             WHERE label.value LIKE ? ESCAPE '\\'))",
            );
            for _ in 0..5 {
                values.push(Value::Text(pattern.clone()));
            }
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
        })
    }
}
