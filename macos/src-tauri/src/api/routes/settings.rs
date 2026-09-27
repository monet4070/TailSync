use super::*;
use tailsync_runtime::history::HistoryOperations;

use super::registry::SettingsCommand;

pub(super) async fn handle(command: SettingsCommand, req: Request, state: &ApiState) -> Response {
    match command {
        SettingsCommand::GetSettings => {
            let settings = state.settings.lock().await.clone();
            Response {
                ok: true,
                data: Some(serde_json::to_value(settings).unwrap_or_default()),
                error: None,
            }
        }

        SettingsCommand::GetSyncState => {
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

        SettingsCommand::SetSyncEnabled => {
            let enabled = req.enabled.unwrap_or(true);
            let result = state
                .settings
                .lock()
                .await
                .set_sync_enabled(enabled)
                .map_err(|error| error.to_string());
            if result.is_ok() {
                bump_runtime_revision();
            }
            Response {
                ok: result.is_ok(),
                data: None,
                error: result.err(),
            }
        }

        SettingsCommand::ToggleSync => {
            let mut settings = state.settings.lock().await;
            let enabled = !settings.sync_enabled;
            match settings.set_sync_enabled(enabled) {
                Ok(()) => {
                    drop(settings);
                    bump_runtime_revision();
                    Response {
                        ok: true,
                        data: Some(serde_json::json!({ "enabled": enabled })),
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

        SettingsCommand::SetSyncShortcut => {
            let shortcut = req.shortcut.unwrap_or_default();
            let result = state
                .settings
                .lock()
                .await
                .set_sync_shortcut(&shortcut)
                .map_err(|error| error.to_string());
            if result.is_ok() {
                bump_runtime_revision();
            }
            Response {
                ok: result.is_ok(),
                data: None,
                error: result.err(),
            }
        }

        SettingsCommand::SetHistoryShortcut => {
            let shortcut = req.shortcut.unwrap_or_default();
            let result = state
                .settings
                .lock()
                .await
                .set_history_shortcut(&shortcut)
                .map_err(|error| error.to_string());
            if result.is_ok() {
                bump_runtime_revision();
            }
            Response {
                ok: result.is_ok(),
                data: None,
                error: result.err(),
            }
        }

        SettingsCommand::SetConnectionMode => {
            let Some(mode) = req.connection_mode else {
                return Response {
                    ok: false,
                    data: None,
                    error: Some("missing connection_mode".into()),
                };
            };
            match crypto::apply_connection_mode_update(&state.settings, &mode, &|settings| {
                settings.save().map_err(|error| error.to_string())
            })
            .await
            {
                Ok((settings, changed)) => {
                    if changed {
                        state.pool.lock().await.disconnect_all();
                        network::clear_peer_cache().await;
                        network::refresh_iroh_for_mode(&settings.connection_mode).await;
                        bump_runtime_revision();
                    }
                    Response {
                        ok: true,
                        data: Some(serde_json::to_value(settings).unwrap_or_default()),
                        error: None,
                    }
                }
                Err(error) => Response {
                    ok: false,
                    data: None,
                    error: Some(match error {
                        crypto::SettingsUpdateError::Validation(_) => {
                            "invalid connection_mode".into()
                        }
                        other => other.to_string(),
                    }),
                },
            }
        }

        SettingsCommand::UpdateSettings => {
            let Some(settings_json) = req.settings else {
                return Response {
                    ok: false,
                    data: None,
                    error: Some("missing settings".into()),
                };
            };
            match serde_json::from_value::<crate::crypto::Settings>(settings_json) {
                Ok(mut requested_settings) => {
                    // The shortcut is registered in the GUI process, so it can
                    // only be changed through the dedicated set_sync_shortcut
                    // route; ignore any value coming from generic settings.
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
                            bump_runtime_revision();
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

        SettingsCommand::ChangeStorageLocation => {
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
                    bump_runtime_revision();
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

        SettingsCommand::DeleteOldStorage => {
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

        SettingsCommand::SetHistoryPinned => {
            let result = match req.id {
                Some(id) => {
                    HistoryOperations::set_favorite_async_with_hook(
                        state.db.clone(),
                        id,
                        req.pinned.unwrap_or(true),
                        bump_clipboard_version,
                    )
                    .await
                }
                None => Err("missing id".to_string()),
            };
            Response {
                ok: result.is_ok(),
                data: None,
                error: result.err(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(settings: crypto::Settings) -> ApiState {
        let settings = Arc::new(Mutex::new(settings));
        let identity = Arc::new(DeviceIdentity::generate_for_test());
        ApiState {
            db: Arc::new(Mutex::new(db::HistoryDB::new_unavailable().unwrap())),
            sync_engine: Arc::new(Mutex::new(sync::SyncEngine::new())),
            pool: Arc::new(Mutex::new(network::ConnectionPool::new(
                identity.clone(),
                settings.clone(),
            ))),
            pairing: crate::pairing::PairingManager::new(settings.clone(), identity.clone()),
            remote_invites: Arc::new(crate::pairing::RemotePairingInviteManager::default()),
            identity,
            settings,
            token: ApiToken::parse(&"a".repeat(64)).unwrap(),
            shutdown: watch::channel(false).0,
            pending_storage_cleanup: Arc::new(Mutex::new(None)),
            imports: Mutex::new(ImportRegistry::default()),
        }
    }

    #[tokio::test]
    async fn connection_mode_route_preserves_paused_sync_and_returns_live_settings() {
        let mut original = crypto::Settings {
            sync_enabled: false,
            connection_mode: "lan_only".into(),
            history_limit: 250,
            language: "zh-CN".into(),
            ..crypto::Settings::default()
        };
        original
            .trusted_peer_keys
            .insert("paired-device".into(), "pinned-key".into());
        original.enabled_peers.insert("paired-device".into(), false);
        let state = state(original.clone());
        let request: Request = serde_json::from_value(serde_json::json!({
            "cmd": "set_connection_mode", "connection_mode": "tailscale_only"
        }))
        .unwrap();
        let response = super::super::handle_cmd(request, &state).await;
        assert!(response.ok, "{:?}", response.error);
        original.connection_mode = "tailscale_only".into();
        assert_eq!(*state.settings.lock().await, original);
        assert_eq!(
            response.data.unwrap(),
            serde_json::to_value(&original).unwrap()
        );
    }

    #[tokio::test]
    async fn connection_mode_route_rejects_missing_and_invalid_modes_without_mutation() {
        let original = crypto::Settings {
            connection_mode: "lan_only".into(),
            ..crypto::Settings::default()
        };
        let state = state(original.clone());
        for request in [
            serde_json::json!({"cmd": "set_connection_mode"}),
            serde_json::json!({"cmd": "set_connection_mode", "connection_mode": "unexpected"}),
        ] {
            let response =
                super::super::handle_cmd(serde_json::from_value(request).unwrap(), &state).await;
            assert!(!response.ok);
            let error = tailsync_runtime::contracts::StableErrorEnvelope::from_legacy_message(
                "set_connection_mode",
                response.error.as_deref().unwrap(),
            );
            assert_eq!(
                error.code,
                tailsync_runtime::contracts::StableErrorCode::InvalidArgument
            );
            assert_eq!(*state.settings.lock().await, original);
        }
    }
}
