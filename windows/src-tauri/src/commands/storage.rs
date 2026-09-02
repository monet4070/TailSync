use super::*;

/// Get current file transfer progress (for progress bar)
#[command]
pub async fn get_file_progress() -> Result<serde_json::Value, String> {
    let info = crate::api::get_file_progress();
    info.map_or_else(
        || Ok(serde_json::json!({"active": false})),
        |progress| serde_json::to_value(progress).map_err(|error| error.to_string()),
    )
}

#[command]
pub async fn cancel_file_batch(state: State<'_, AppState>, batch_id: String) -> Result<(), String> {
    let id = crate::protocol::TransferId::from_hex(&batch_id)?;
    cancel_file_batch_impl(&state.sync_engine, &state.pool, &state.settings, id).await;
    Ok(())
}

pub(crate) async fn cancel_file_batch_impl(
    sync_engine: &std::sync::Arc<tokio::sync::Mutex<crate::sync::SyncEngine>>,
    pool: &std::sync::Arc<tokio::sync::Mutex<network::ConnectionPool>>,
    settings: &std::sync::Arc<tokio::sync::Mutex<crate::crypto::Settings>>,
    batch_id: crate::protocol::TransferId,
) {
    let batch_id_hex = batch_id.as_hex();
    let source = crate::sync::SyncEngine::cancel_file_batch_local_shared(sync_engine, batch_id).await;
    crate::api::clear_file_progress_scope(Some(&batch_id_hex), None);
    if let Some(source) = source {
        if let Err(error) = network::send_file_batch_cancel(pool, settings, &source, batch_id).await
        {
            log::warn!("Could not notify {source} that file batch was cancelled: {error}");
        }
    } else {
        if let Err(error) = crate::sync::remove_outgoing_batch(batch_id) {
            log::warn!("Could not remove cancelled outgoing file batch {batch_id_hex}: {error}");
        }
        crate::api::request_file_batch_cancel(&batch_id_hex);
    }
}

#[command]
pub async fn get_storage_status(state: State<'_, AppState>) -> Result<db::StorageStatus, String> {
    Ok(db::storage_status_async(&state.db).await)
}

#[command]
pub async fn change_storage_location(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    parent: String,
) -> Result<db::StorageMigrationResult, String> {
    use tauri_plugin_notification::NotificationExt;
    let parent = std::path::PathBuf::from(parent);
    let notifications_enabled = state.settings.lock().await.notifications_enabled;
    let show = || {
        if notifications_enabled {
            let _ = app
                .notification()
                .builder()
                .title("TailSync")
                .body("File transfers finished. Moving TailSync data now.")
                .show();
        }
    };
    let notify: Option<&(dyn Fn() + Send + Sync)> = Some(&show);
    let result = db::migrate_storage_with_rollback(
        &state.db,
        &state.settings,
        &parent,
        db::StorageMigrationHooks {
            wait_timeout: std::time::Duration::from_secs(60),
            has_active_transfers: &crate::api::has_active_file_progress,
            notify,
            persist_settings: &|settings: &crate::crypto::Settings| {
                settings.save().map_err(|error| error.to_string())
            },
        },
    )
    .await
    .map_err(|failure| match failure {
        db::StorageMigrationFailure::TimedOutWaitingForTransfers => {
            "Timed out waiting for active file transfers to finish".to_string()
        }
        db::StorageMigrationFailure::Migrate(error) => error,
        db::StorageMigrationFailure::SaveFailedAfterRollback { save_error } => format!(
            "Could not save the new storage location; TailSync returned to the old location: {save_error}"
        ),
        db::StorageMigrationFailure::RollbackAlsoFailed { save_error, rollback_error } => {
            format!(
                "Could not save the new storage location ({save_error}); rollback also failed: {rollback_error}"
            )
        }
    })?;
    *state.pending_storage_cleanup.lock().await =
        Some(std::path::PathBuf::from(result.old_root.clone()));
    Ok(result)
}

#[command]
pub async fn set_history_pinned(
    state: State<'_, AppState>,
    id: i64,
    pinned: bool,
) -> Result<(), String> {
    state
        .db
        .lock()
        .await
        .set_favorite(id, pinned)
        .map_err(|error| error.to_string())?;
    crate::api::bump_clipboard_version();
    Ok(())
}

#[command]
pub async fn delete_old_storage(state: State<'_, AppState>, path: String) -> Result<(), String> {
    let requested = std::path::PathBuf::from(path);
    let authorized = state
        .pending_storage_cleanup
        .lock()
        .await
        .as_ref()
        .is_some_and(|expected| paths_equivalent(expected, &requested));
    if !authorized {
        return Err(
            "The requested storage directory was not issued by a completed migration".into(),
        );
    }
    let result = tokio::task::spawn_blocking(move || {
        db::delete_old_storage(&requested).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?;
    if result.is_ok() {
        *state.pending_storage_cleanup.lock().await = None;
    }
    result
}

pub(super) fn paths_equivalent(left: &std::path::Path, right: &std::path::Path) -> bool {
    let left = left.canonicalize().unwrap_or_else(|_| left.to_path_buf());
    let right = right.canonicalize().unwrap_or_else(|_| right.to_path_buf());
    if cfg!(windows) {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    } else {
        left == right
    }
}

#[command]
pub async fn restore_file_batch(
    state: State<'_, AppState>,
    batch_id: String,
) -> Result<(), String> {
    let paths = materialize_file_batch_paths(state.db.clone(), batch_id).await?;
    crate::clipboard_file::write_clipboard_files(&paths)?;
    crate::api::bump_clipboard_version();
    Ok(())
}

pub(crate) async fn materialize_file_batch_paths(
    database: std::sync::Arc<tokio::sync::Mutex<db::HistoryDB>>,
    batch_id: String,
) -> Result<Vec<std::path::PathBuf>, String> {
    tokio::task::spawn_blocking(move || {
        database
            .blocking_lock()
            .materialize_file_batch(&batch_id)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}
