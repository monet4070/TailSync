use super::*;
use tailsync_runtime::history::{HistoryOperations, HistoryPageRequestOwned};

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
    let collection = db::HistoryCollection::from_wire(collection.as_deref())
        .map_err(|error| error.to_string())?;
    HistoryOperations::entries_async(
        state.db.clone(),
        HistoryPageRequestOwned::new(
            collection, keyword, category, start_time, end_time, limit, offset, 50,
        ),
    )
    .await
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
    let collection = db::HistoryCollection::from_wire(collection.as_deref())
        .map_err(|error| error.to_string())?;
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
pub async fn get_history_capabilities() -> Result<serde_json::Value, String> {
    Ok(crate::api::history_capabilities_data())
}

#[command]
pub async fn get_migration_diagnostics(
    state: State<'_, AppState>,
) -> Result<db::MigrationDiagnostics, String> {
    HistoryOperations::migration_diagnostics_async(state.db.clone(), 50).await
}

/// Search history by keyword (searches description field)
#[command]
pub async fn search_history(
    state: State<'_, AppState>,
    keyword: String,
) -> Result<Vec<db::HistoryEntry>, String> {
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
}

/// Delete a history entry
#[command]
pub async fn delete_entry(state: State<'_, AppState>, id: i64) -> Result<(), String> {
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
) -> Result<db::FavoriteMutation, String> {
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
) -> Result<db::FavoriteMutation, String> {
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
pub async fn clear_history(state: State<'_, AppState>) -> Result<(), String> {
    HistoryOperations::clear_async_with_hook(state.db.clone(), crate::api::bump_clipboard_version)
        .await?;
    Ok(())
}

/// Restore a history entry back to clipboard.
/// Handles text (as text), images (as Image), and files (via CF_HDROP).
#[command]
pub async fn restore_entry(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> Result<(), String> {
    let payload = HistoryOperations::restore_payload_async(state.db.clone(), id).await?;
    let entry_type = payload.entry_type;
    let file_path = payload.file_path;
    let file_name = payload.file_name;
    let data = payload.data;

    let clipboard = app
        .try_state::<tauri_plugin_clipboard_manager::Clipboard<tauri::Wry>>()
        .ok_or("Clipboard not available")?;

    if entry_type == "image" {
        let data = data.as_ref().ok_or("Image history data is unavailable")?;
        let image = crate::protocol::PackedImage::try_from(data.as_slice())
            .map_err(|error| error.to_string())?;
        let w = image.width;
        let h = image.height;
        let rgba = image.rgba;

        // Shadow filter to prevent re-broadcast
        {
            let mut sync = state.sync_engine.lock().await;
            sync.add_image_shadow_filter(data);
        }

        // Try arboard write_image first.
        // Fallback: bypass arboard and set CF_DIB directly via Win32 API.
        let img = tauri::image::Image::new(rgba, w, h);
        match clipboard.write_image(&img) {
            Ok(()) => {
                info!(
                    "Restored entry {} (image {}×{}) to clipboard via arboard",
                    id, w, h
                );
            }
            Err(_e) => {
                info!("write_image failed for entry {} — using raw CF_DIB", id);
                let bmp_dib = rgba_to_dib(rgba, w, h);
                if bmp_dib.is_empty() {
                    state
                        .sync_engine
                        .lock()
                        .await
                        .remove_image_shadow_filter(data);
                    return Err("DIB encode failed".into());
                }
                #[cfg(target_os = "windows")]
                {
                    set_clipboard_dib(&bmp_dib);
                    info!(
                        "Restored entry {} (image {}×{}) to clipboard via CF_DIB",
                        id, w, h
                    );
                }
                #[cfg(not(target_os = "windows"))]
                {
                    state
                        .sync_engine
                        .lock()
                        .await
                        .remove_image_shadow_filter(data);
                    return Err("write_image failed and no fallback on this platform".into());
                }
            }
        }
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

        // Shadow filter to prevent re-broadcast
        {
            let mut sync = state.sync_engine.lock().await;
            sync.add_shadow_filter(&text);
        }

        if let Err(error) = clipboard.write_text(text.clone()) {
            state.sync_engine.lock().await.remove_shadow_filter(&text);
            return Err(format!("Clipboard text write failed: {}", error));
        }

        info!(
            "Restored entry {} to clipboard ({} chars)",
            id,
            text.chars().count()
        );
    }

    crate::api::bump_clipboard_version();
    Ok(())
}
