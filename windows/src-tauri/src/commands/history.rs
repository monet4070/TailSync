use super::*;
use tailsync_runtime::history::{HistoryOperations, HistoryPageRequestOwned};

pub use db::HistoryQueryPage as HistoryPage;

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
) -> Result<Vec<db::HistoryEntry>, CommandError> {
    let collection = db::HistoryCollection::from_wire(collection.as_deref()).map_err(|_| {
        CommandError::code(tailsync_runtime::contracts::StableErrorCode::InvalidArgument)
    })?;
    HistoryOperations::entries_async(
        state.db.clone(),
        HistoryPageRequestOwned::new(
            collection, keyword, category, start_time, end_time, limit, offset, 50,
        ),
    )
    .await
    .map_err(Into::into)
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
) -> Result<HistoryPage, CommandError> {
    let collection = db::HistoryCollection::from_wire(collection.as_deref()).map_err(|_| {
        CommandError::code(tailsync_runtime::contracts::StableErrorCode::InvalidArgument)
    })?;
    let page = HistoryOperations::page_async(
        state.db.clone(),
        HistoryPageRequestOwned::new(
            collection, keyword, category, start_time, end_time, limit, offset, 50,
        ),
    )
    .await?;
    Ok(HistoryPage {
        entries: page.entries,
        total: page.total,
        has_more: page.has_more,
    })
}

#[command]
pub async fn get_history_capabilities() -> Result<serde_json::Value, CommandError> {
    Ok(crate::api::history_capabilities_data())
}

#[command]
pub async fn get_migration_diagnostics(
    state: State<'_, AppState>,
) -> Result<db::MigrationDiagnostics, CommandError> {
    HistoryOperations::migration_diagnostics_async(state.db.clone(), 50)
        .await
        .map_err(Into::into)
}

/// Search history by keyword (searches description field)
#[command]
pub async fn search_history(
    state: State<'_, AppState>,
    keyword: String,
) -> Result<Vec<db::HistoryEntry>, CommandError> {
    HistoryOperations::entries_async(
        state.db.clone(),
        HistoryPageRequestOwned::new(
            db::HistoryCollection::All,
            Some(keyword),
            None,
            None,
            None,
            Some(100),
            Some(0),
            100,
        ),
    )
    .await
    .map_err(Into::into)
}

/// Delete a history entry
#[command]
pub async fn delete_entry(state: State<'_, AppState>, id: i64) -> Result<(), CommandError> {
    HistoryOperations::delete_async_with_hook(
        state.db.clone(),
        id,
        crate::api::bump_clipboard_version,
    )
    .await?;
    Ok(())
}

/// Set or clear the favorite state for a logical history item.
#[command]
pub async fn set_history_favorite(
    state: State<'_, AppState>,
    id: i64,
    favorite: bool,
) -> Result<db::FavoriteMutation, CommandError> {
    let mutation = HistoryOperations::set_favorite_async_with_hook(
        state.db.clone(),
        id,
        favorite,
        crate::api::bump_clipboard_version,
    )
    .await?;
    Ok(mutation)
}

/// Delete a logical history item from the favorites collection.
#[command]
pub async fn delete_favorite_entry(
    state: State<'_, AppState>,
    id: i64,
) -> Result<db::FavoriteMutation, CommandError> {
    let mutation = HistoryOperations::delete_favorite_async_with_hook(
        state.db.clone(),
        id,
        crate::api::bump_clipboard_version,
    )
    .await?;
    Ok(mutation)
}

/// Delete all clipboard history entries.
#[command]
pub async fn clear_history(state: State<'_, AppState>) -> Result<(), CommandError> {
    HistoryOperations::clear_async_with_hook(state.db.clone(), crate::api::bump_clipboard_version)
        .await?;
    Ok(())
}

/// Restore a history entry back to clipboard.
/// Handles text (as text), images (as Image), and files (via CF_HDROP).
#[command]
pub async fn restore_entry(state: State<'_, AppState>, id: i64) -> Result<(), CommandError> {
    let payload = HistoryOperations::restore_payload_async(state.db.clone(), id).await?;
    let entry_type = payload.entry_type;
    let file_path = payload.file_path;
    let file_name = payload.file_name;
    let data = payload.data;

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
