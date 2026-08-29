use super::*;

pub(super) fn handles(command: &str) -> bool {
    matches!(
        command,
        "get_settings"
            | "get_sync_state"
            | "set_sync_enabled"
            | "toggle_sync"
            | "set_sync_shortcut"
            | "set_history_shortcut"
            | "update_settings"
            | "change_storage_location"
            | "delete_old_storage"
            | "set_history_pinned"
    )
}

pub(super) async fn handle(req: Request, state: &ApiState) -> Response {
    match req.cmd.as_str() {
        "get_settings" => {
            let settings = state.settings.lock().await.clone();
            Response {
                ok: true,
                data: Some(serde_json::to_value(settings).unwrap_or_default()),
                error: None,
            }
        }

        "get_sync_state" => {
            let settings = state.settings.lock().await;
            Response {
                ok: true,
                data: Some(serde_json::json!({
                    "enabled": settings.sync_enabled,
                    "shortcut": settings.sync_shortcut,
                    "history_shortcut": settings.history_shortcut,
                })),
                error: None,
            }
        }

        "set_sync_enabled" => {
            let enabled = req.enabled.unwrap_or(true);
            let result = state
                .settings
                .lock()
                .await
                .set_sync_enabled(enabled)
                .map_err(|error| error.to_string());
            Response {
                ok: result.is_ok(),
                data: None,
                error: result.err(),
            }
        }

        "toggle_sync" => {
            let mut settings = state.settings.lock().await;
            let enabled = !settings.sync_enabled;
            match settings.set_sync_enabled(enabled) {
                Ok(()) => Response {
                    ok: true,
                    data: Some(serde_json::json!({ "enabled": enabled })),
                    error: None,
                },
                Err(error) => Response {
                    ok: false,
                    data: None,
                    error: Some(error.to_string()),
                },
            }
        }

        "set_sync_shortcut" => {
            let shortcut = req.shortcut.unwrap_or_default();
            let result = state
                .settings
                .lock()
                .await
                .set_sync_shortcut(&shortcut)
                .map_err(|error| error.to_string());
            Response {
                ok: result.is_ok(),
                data: None,
                error: result.err(),
            }
        }

        "set_history_shortcut" => {
            let shortcut = req.shortcut.unwrap_or_default();
            let result = state
                .settings
                .lock()
                .await
                .set_history_shortcut(&shortcut)
                .map_err(|error| error.to_string());
            Response {
                ok: result.is_ok(),
                data: None,
                error: result.err(),
            }
        }

        "update_settings" => {
            let Some(settings_json) = req.settings else {
                return Response {
                    ok: false,
                    data: None,
                    error: Some("missing settings".into()),
                };
            };
            match serde_json::from_value::<crate::crypto::Settings>(settings_json) {
                Ok(mut requested_settings) => {
                    // The shortcut is registered through the dedicated
                    // set_sync_shortcut command; ignore any value arriving via
                    // generic settings so runtime and persisted state stay aligned.
                    requested_settings.sync_shortcut =
                        state.settings.lock().await.sync_shortcut.clone();
                    requested_settings.history_shortcut =
                        state.settings.lock().await.history_shortcut.clone();
                    match crate::crypto::apply_settings_update(
                        &state.settings,
                        &state.db,
                        requested_settings,
                        &|settings: &crate::crypto::Settings| {
                            settings.save().map_err(|error| error.to_string())
                        },
                        None,
                    )
                    .await
                    {
                        Ok(outcome) => {
                            if outcome.mode_changed {
                                state.pool.lock().await.disconnect_all();
                                network::clear_peer_cache().await;
                                network::refresh_iroh_for_mode(&outcome.connection_mode).await;
                            }
                            Response {
                                ok: true,
                                data: None,
                                error: None,
                            }
                        }
                        Err(error) => Response {
                            ok: false,
                            data: None,
                            error: Some(error.to_string()),
                        },
                    }
                }
                Err(e) => Response {
                    ok: false,
                    data: None,
                    error: Some(e.to_string()),
                },
            }
        }

        "change_storage_location" => {
            let Some(parent) = req.parent else {
                return Response {
                    ok: false,
                    data: None,
                    error: Some("missing parent".into()),
                };
            };
            let parent = std::path::PathBuf::from(parent);
            match db::migrate_storage_with_rollback(
                &state.db,
                &state.settings,
                &parent,
                db::StorageMigrationHooks {
                    wait_timeout: std::time::Duration::from_secs(60),
                    has_active_transfers: &has_active_file_progress,
                    notify: None,
                    persist_settings: &|settings: &crate::crypto::Settings| {
                        settings.save().map_err(|error| error.to_string())
                    },
                },
            )
            .await
            {
                Ok(result) => {
                    *state.pending_storage_cleanup.lock().await =
                        Some(std::path::PathBuf::from(result.old_root.clone()));
                    Response {
                        ok: true,
                        data: serde_json::to_value(result).ok(),
                        error: None,
                    }
                }
                Err(failure) => Response {
                    ok: false,
                    data: None,
                    error: Some(match failure {
                        db::StorageMigrationFailure::TimedOutWaitingForTransfers => {
                            "Timed out waiting for active file transfers to finish".to_string()
                        }
                        db::StorageMigrationFailure::Migrate(error) => error,
                        db::StorageMigrationFailure::SaveFailedAfterRollback { save_error } => {
                            format!(
                                "Could not save the new storage location; TailSync returned to the old location: {save_error}"
                            )
                        }
                        db::StorageMigrationFailure::RollbackAlsoFailed {
                            save_error,
                            rollback_error,
                        } => format!(
                            "Could not save the new storage location ({save_error}); rollback also failed: {rollback_error}"
                        ),
                    }),
                },
            }
        }

        "delete_old_storage" => {
            let result = match req.path.as_deref() {
                None => Err("missing path".to_string()),
                Some(path) => {
                    let requested = std::path::PathBuf::from(path);
                    let authorized = state
                        .pending_storage_cleanup
                        .lock()
                        .await
                        .as_ref()
                        .is_some_and(|expected| paths_equivalent(expected, &requested));
                    if !authorized {
                        Err("The requested storage directory was not issued by a completed migration".to_string())
                    } else {
                        let result =
                            db::delete_old_storage(&requested).map_err(|error| error.to_string());
                        if result.is_ok() {
                            *state.pending_storage_cleanup.lock().await = None;
                        }
                        result
                    }
                }
            };
            Response {
                ok: result.is_ok(),
                data: None,
                error: result.err(),
            }
        }

        "set_history_pinned" => {
            let result = match req.id {
                Some(id) => state
                    .db
                    .lock()
                    .await
                    .set_pinned(id, req.pinned.unwrap_or(true))
                    .map_err(|error| error.to_string()),
                None => Err("missing id".to_string()),
            };
            if result.is_ok() {
                bump_clipboard_version();
            }
            Response {
                ok: result.is_ok(),
                data: None,
                error: result.err(),
            }
        }

        _ => unreachable!("settings command dispatch was checked before routing"),
    }
}
