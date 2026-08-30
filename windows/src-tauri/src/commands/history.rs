use super::*;

#[derive(serde::Serialize)]
pub struct HistoryPage {
    pub entries: Vec<db::HistoryEntry>,
    pub total: Option<usize>,
    pub has_more: bool,
}

/// Get clipboard history entries
// Tauri exposes these named arguments as the stable frontend command contract.
#[allow(clippy::too_many_arguments)]
#[command]
pub async fn get_history(
    state: State<'_, AppState>,
    keyword: Option<String>,
    category: Option<String>,
    start_time: Option<String>,
    end_time: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
    collection: Option<String>,
) -> Result<Vec<db::HistoryEntry>, String> {
    let db = state.db.lock().await;
    let collection = db::HistoryCollection::from_wire(collection.as_deref())
        .map_err(|error| error.to_string())?;
    db.get_page_in_collection(db::HistoryQuery {
        collection,
        keyword: keyword.as_deref(),
        category: category.as_deref(),
        start_time: start_time.as_deref(),
        end_time: end_time.as_deref(),
        limit: limit.unwrap_or(50),
        offset: offset.unwrap_or(0),
    })
    .map(|page| page.entries)
    .map_err(|e| e.to_string())
}

#[allow(clippy::too_many_arguments)]
#[command]
pub async fn get_history_page(
    state: State<'_, AppState>,
    keyword: Option<String>,
    category: Option<String>,
    start_time: Option<String>,
    end_time: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
    collection: Option<String>,
) -> Result<HistoryPage, String> {
    let db = state.db.lock().await;
    let collection = db::HistoryCollection::from_wire(collection.as_deref())
        .map_err(|error| error.to_string())?;
    let page = db
        .get_page_in_collection(db::HistoryQuery {
            collection,
            keyword: keyword.as_deref(),
            category: category.as_deref(),
            start_time: start_time.as_deref(),
            end_time: end_time.as_deref(),
            limit: limit.unwrap_or(50),
            offset: offset.unwrap_or(0),
        })
        .map_err(|e| e.to_string())?;
    Ok(HistoryPage {
        entries: page.entries,
        total: page.total,
        has_more: page.has_more,
    })
}

#[command]
pub async fn get_history_capabilities() -> Result<serde_json::Value, String> {
    Ok(crate::api::history_capabilities_data())
}

#[command]
pub async fn get_migration_diagnostics(
    state: State<'_, AppState>,
) -> Result<db::MigrationDiagnostics, String> {
    state
        .db
        .lock()
        .await
        .migration_diagnostics(50)
        .map_err(|error| error.to_string())
}

/// Search history by keyword (searches description field)
#[command]
pub async fn search_history(
    state: State<'_, AppState>,
    keyword: String,
) -> Result<Vec<db::HistoryEntry>, String> {
    let db = state.db.lock().await;
    db.get_all(Some(&keyword), None, 100, 0)
        .map_err(|e| e.to_string())
}

/// Delete a history entry
#[command]
pub async fn delete_entry(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let mut db = state.db.lock().await;
    db.delete(id).map_err(|e| e.to_string())?;
    crate::api::bump_clipboard_version();
    Ok(())
}

/// Set or clear the favorite state for a logical history item.
#[command]
pub async fn set_history_favorite(
    state: State<'_, AppState>,
    id: i64,
    favorite: bool,
) -> Result<db::FavoriteMutation, String> {
    let mut db = state.db.lock().await;
    let mutation = db
        .set_favorite(id, favorite)
        .map_err(|error| error.to_string())?;
    crate::api::bump_clipboard_version();
    Ok(mutation)
}

/// Delete a logical history item from the favorites collection.
#[command]
pub async fn delete_favorite_entry(
    state: State<'_, AppState>,
    id: i64,
) -> Result<db::FavoriteMutation, String> {
    let mut db = state.db.lock().await;
    let mutation = db.delete_favorite(id).map_err(|error| error.to_string())?;
    crate::api::bump_clipboard_version();
    Ok(mutation)
}

/// Delete all clipboard history entries.
#[command]
pub async fn clear_history(state: State<'_, AppState>) -> Result<(), String> {
    let mut db = state.db.lock().await;
    db.clear_all().map_err(|e| e.to_string())?;
    crate::api::bump_clipboard_version();
    Ok(())
}

/// Restore a history entry back to clipboard.
/// Handles text (as text), images (as Image), and files (via CF_HDROP).
#[command]
pub async fn restore_entry(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let db = state.db.lock().await;
    let entry_type = db.get_type(id).map_err(|e| e.to_string())?;
    let file_path = if entry_type == "file" {
        db.get_file_path(id).map_err(|e| e.to_string())?
    } else {
        None
    };
    let file_name = if entry_type == "file" {
        Some(db.get_description(id).map_err(|e| e.to_string())?)
    } else {
        None
    };
    let data = if file_path.is_none() {
        Some(db.get_data(id).map_err(|e| e.to_string())?)
    } else {
        None
    };
    drop(db);

    if entry_type == "image" {
        let data = data.as_ref().ok_or("Image history data is unavailable")?;
        state.sync_engine.lock().await.restore_image(data)?;
    } else if entry_type == "file" {
        if let Some(path) = file_path {
            crate::api::restore_file_path_to_clipboard(
                &path,
                file_name.as_deref().unwrap_or("restored_file"),
            )?;
        } else {
            crate::api::restore_file_to_clipboard(
                data.as_deref().ok_or("File history data is unavailable")?,
                file_name.as_deref().unwrap_or("restored_file"),
            )?;
        }

        info!(
            "Restored entry {} (file: {}) to clipboard",
            id,
            file_name.as_deref().unwrap_or("restored_file")
        );
    } else {
        // Text entry (or fallback for unknown types)
        let text = String::from_utf8_lossy(data.as_deref().unwrap_or_default()).to_string();

        state.sync_engine.lock().await.restore_text(&text)?;

        info!(
            "Restored entry {} to clipboard ({} chars)",
            id,
            text.chars().count()
        );
    }

    crate::api::bump_clipboard_version();
    Ok(())
}
