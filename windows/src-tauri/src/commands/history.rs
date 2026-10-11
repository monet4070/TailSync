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
    tailsync_runtime::history_display::entries_display_async(
        state.db.clone(),
        HistoryPageRequestOwned::new(
            collection, keyword, category, start_time, end_time, limit, offset, 50,
        ),
        crate::clipboard_composite::native_analyzer(),
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
    let page = tailsync_runtime::history_display::page_display_async(
        state.db.clone(),
        HistoryPageRequestOwned::new(
            collection, keyword, category, start_time, end_time, limit, offset, 50,
        ),
        crate::clipboard_composite::native_analyzer(),
    )
    .await?;
    Ok(HistoryPage {
        entries: page.entries,
        total: page.total,
        has_more: page.has_more,
        display_index_pending: page.display_index_pending,
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
    tailsync_runtime::history_display::entries_display_async(
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
        crate::clipboard_composite::native_analyzer(),
    )
    .await
    .map_err(Into::into)
}

/// Delete a history entry
#[command]
pub async fn delete_entry(state: State<'_, AppState>, id: i64) -> Result<(), CommandError> {
    let image_archive = state.image_archive.clone();
    HistoryOperations::delete_async_with_hook(state.db.clone(), id, move || {
        image_archive.cancel_entry(id);
        crate::api::bump_clipboard_version();
    })
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
    let image_archive = state.image_archive.clone();
    let mutation =
        HistoryOperations::delete_favorite_async_with_hook(state.db.clone(), id, move || {
            image_archive.cancel_entry(id);
            crate::api::bump_clipboard_version();
        })
        .await?;
    Ok(mutation)
}

/// Delete all clipboard history entries.
#[command]
pub async fn clear_history(state: State<'_, AppState>) -> Result<(), CommandError> {
    let image_archive = state.image_archive.clone();
    HistoryOperations::clear_async_with_hook(state.db.clone(), move || {
        image_archive.cancel_pending();
        crate::api::bump_clipboard_version();
    })
    .await?;
    Ok(())
}

#[command]
pub async fn archive_entry_images(
    state: State<'_, AppState>,
    id: i64,
) -> Result<tailsync_runtime::history::ArchiveEntryImagesReport, CommandError> {
    let transport: std::sync::Arc<
        dyn tailsync_core::composite::resource_archive::ResourceTransport,
    > = std::sync::Arc::new(tailsync_core::composite::resource_archive::ReqwestTransport::new());
    let image_archive = state.image_archive.clone();
    let report = HistoryOperations::archive_entry_images_with_commit_hook(
        state.db.clone(),
        id,
        transport,
        tailsync_core::cancellation::Cancellation::default(),
        move |commit| image_archive.committed(commit),
    )
    .await?;
    Ok(report)
}

#[command]
pub async fn source_context_grant_create(
    state: State<'_, AppState>,
    id: i64,
) -> Result<tailsync_runtime::source_context::SourceContextGrantReport, CommandError> {
    let report = tailsync_runtime::source_context::create_grant(
        state.db.clone(),
        tailsync_runtime::source_context::default_registry(),
        id,
    )
    .await?;
    Ok(report)
}

#[command]
pub async fn source_context_grant_revoke(
    grant_id: String,
) -> Result<tailsync_runtime::source_context::SourceContextRevokeReport, CommandError> {
    Ok(tailsync_runtime::source_context::revoke_grant(
        tailsync_runtime::source_context::default_registry(),
        &grant_id,
    ))
}

#[command]
pub async fn source_context_import(
    state: State<'_, AppState>,
    id: i64,
    path: String,
) -> Result<tailsync_runtime::source_context::SourceContextImportReport, CommandError> {
    let report = tailsync_runtime::source_context::import_packet_checked(
        state.db.clone(),
        tailsync_runtime::source_context::default_registry(),
        path,
        Some(id),
        tailsync_core::cancellation::Cancellation::default(),
    )
    .await?;
    Ok(report)
}

#[command]
pub async fn source_context_export_ticket(
    path: String,
    ticket: String,
) -> Result<(), CommandError> {
    tailsync_runtime::source_context::export_ticket(path, ticket)
        .await
        .map_err(Into::into)
}

/// Restore a history entry back to clipboard.
/// Handles text (as text), images (as Image), and files (via CF_HDROP).
#[command]
pub async fn restore_entry(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    restore_format: Option<String>,
    restore_original_id: Option<i64>,
    restore_identity: Option<tailsync_core::composite::identity::HistoryIdentity>,
) -> Result<tailsync_runtime::history::HistoryRestoreReport, CommandError> {
    let _restore = super::restore_coordination::RestoreLease::begin(app)?;
    let payload = HistoryOperations::restore_payload_checked_async(
        state.db.clone(),
        id,
        restore_original_id,
        restore_identity,
    )
    .await?;
    if payload.entry_type == "composite" {
        let outcome = payload
            .restore_composite_async(state.sync_engine.clone(), restore_format)
            .await?;
        // R5-03: only a fully published restore is a success. Anything else surfaces
        // the stable code, so the UI explains "saved to history, publish incomplete"
        // instead of showing a success the user would read as "copied".
        if !outcome.is_published() {
            return Err(CommandError::restore(&outcome));
        }
        crate::api::bump_clipboard_version();
        return outcome
            .report()
            .ok_or_else(|| CommandError::restore(&outcome));
    }
    if payload.entry_type == "text" || payload.entry_type == "image" {
        let bytes = payload.data.ok_or("History data unavailable")?;
        if payload.entry_type == "text" {
            let text = String::from_utf8(bytes).map_err(|_| "Invalid text history")?;
            tailsync_core::sync::SyncEngine::restore_text_shared(state.sync_engine.clone(), text)
                .await?;
        } else {
            tailsync_core::sync::SyncEngine::restore_image_shared(state.sync_engine.clone(), bytes)
                .await?;
        }
        crate::api::bump_clipboard_version();
        return Ok(tailsync_runtime::history::HistoryRestoreReport::default());
    }
    let entry_type = payload.entry_type;
    let file_path = payload.file_path;
    let file_name = payload.file_name;
    let data = payload.data;

    if entry_type == "image" {
        let data = data.as_ref().ok_or("Image history data is unavailable")?;
        state.sync_engine.lock().await.restore_image(data)?;
    } else if entry_type == "file" {
        state.sync_engine.lock().await.supersede_file_clipboard();
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
    Ok(tailsync_runtime::history::HistoryRestoreReport::default())
}
