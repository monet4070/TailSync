use super::*;

impl HistoryDB {
    /// Set the favorite state for one logical history item. File batches are
    /// always mutated atomically, so every visible row agrees on the state.
    pub fn set_favorite(
        &mut self,
        id: i64,
        favorite: bool,
    ) -> Result<FavoriteMutation, HistoryMutationError> {
        let ids = self.logical_item_ids(id)?;
        let tx = self
            .conn
            .transaction()
            .map_err(|_| HistoryMutationError::EntryNotFound { id })?;
        for item_id in &ids {
            tx.execute(
                "UPDATE history SET pinned = ?1 WHERE id = ?2",
                rusqlite::params![i64::from(favorite), item_id],
            )
            .map_err(|_| HistoryMutationError::EntryNotFound { id })?;
        }
        tx.commit()
            .map_err(|_| HistoryMutationError::EntryNotFound { id })?;
        Ok(FavoriteMutation {
            affected_ids: ids,
            favorite,
        })
    }

    /// Delete a logical history item from the favorites collection.
    pub fn delete_favorite(
        &mut self,
        id: i64,
    ) -> Result<FavoriteMutation, Box<dyn std::error::Error>> {
        let ids = self.logical_item_ids(id)?;
        let favorite = self.is_logical_item_favorite(&ids)?;
        if !favorite {
            return Err(Box::new(HistoryMutationError::NotFavorite { id }));
        }
        self.delete_entries_with_batch_policy(&ids, None, false)?;
        Ok(FavoriteMutation {
            affected_ids: ids,
            favorite: false,
        })
    }

    pub(super) fn ensure_logical_item_unfavorited(
        &self,
        id: i64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let ids = self.logical_item_ids(id)?;
        if self.is_logical_item_favorite(&ids)? {
            return Err(Box::new(HistoryMutationError::FavoriteProtected { id }));
        }
        Ok(())
    }

    pub(super) fn logical_item_ids(&self, id: i64) -> Result<Vec<i64>, HistoryMutationError> {
        let batch_id = self
            .conn
            .query_row(
                "SELECT batch_id FROM history WHERE id = ?1",
                rusqlite::params![id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(|_| HistoryMutationError::EntryNotFound { id })?
            .ok_or(HistoryMutationError::EntryNotFound { id })?;
        match batch_id {
            Some(batch_id) => self
                .conn
                .prepare(
                    "SELECT id FROM history WHERE batch_id = ?1
                     ORDER BY batch_index ASC, id ASC",
                )
                .map_err(|_| HistoryMutationError::EntryNotFound { id })?
                .query_map(rusqlite::params![batch_id], |row| row.get(0))
                .map_err(|_| HistoryMutationError::EntryNotFound { id })?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| HistoryMutationError::EntryNotFound { id }),
            None => Ok(vec![id]),
        }
    }

    pub(super) fn is_logical_item_favorite(
        &self,
        ids: &[i64],
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let Some(first) = ids.first() else {
            return Ok(false);
        };
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM history WHERE id = ?1 AND pinned <> 0",
            rusqlite::params![first],
            |row| row.get(0),
        )?;
        if count != 0 {
            return Ok(true);
        }
        let mut statement = self.conn.prepare(
            "SELECT COUNT(*) FROM history
             WHERE id IN (SELECT id FROM history WHERE batch_id =
                          (SELECT batch_id FROM history WHERE id = ?1))
               AND pinned <> 0",
        )?;
        let count: i64 = statement.query_row(rusqlite::params![first], |row| row.get(0))?;
        Ok(count != 0)
    }

    /// Duplicate replacement is an internal history maintenance operation,
    /// but it must not become a hidden deletion path for a favorite item.
    pub(super) fn unfavorited_duplicate_ids(
        &self,
        ids: &[i64],
    ) -> Result<Vec<i64>, Box<dyn std::error::Error>> {
        let mut deletable = Vec::with_capacity(ids.len());
        for id in ids {
            let logical_ids = self.logical_item_ids(*id)?;
            if !self.is_logical_item_favorite(&logical_ids)? {
                deletable.push(*id);
            }
        }
        Ok(deletable)
    }
}
