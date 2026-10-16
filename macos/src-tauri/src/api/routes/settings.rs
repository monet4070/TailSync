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
            let result = {
                let mut settings = state.settings.lock().await;
                settings
                    .set_sync_enabled(enabled)
                    .map(|()| settings.clone())
                    .map_err(|error| error.to_string())
            };
            if result.is_ok() {
                bump_runtime_revision();
            }
            match result {
                Ok(settings) => Response {
                    ok: true,
                    data: Some(serde_json::to_value(settings).unwrap_or_default()),
                    error: None,
                },
                Err(error) => Response {
                    ok: false,
                    data: None,
                    error: Some(error.into()),
                },
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
                    error: Some(error.to_string().into()),
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
                error: result.err().map(Into::into),
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
                error: result.err().map(Into::into),
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
                    error: Some(
                        match error {
                            crypto::SettingsUpdateError::Validation(_) => {
                                "invalid connection_mode".into()
                            }
                            other => other.to_string(),
                        }
                        .into(),
                    ),
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
            // Windows send field intents, never a whole `get_settings`
            // snapshot, so one window cannot roll back another window's edit.
            match crate::crypto::SettingsPatch::from_json_value(settings_json) {
                Ok(patch) => {
                    match crate::crypto::apply_settings_patch(
                        &state.settings,
                        &state.db,
                        patch,
                        &|settings: &crate::crypto::Settings| {
                            crate::automatic_images::persist(settings, &state.image_archive)
                        },
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
                                data: Some(
                                    serde_json::to_value(outcome.persisted).unwrap_or_default(),
                                ),
                                error: None,
                            }
                        }
                        Err(error) => Response {
                            ok: false,
                            data: None,
                            error: Some(error.to_string().into()),
                        },
                    }
                }
                Err(error) => Response {
                    ok: false,
                    data: None,
                    error: Some(error.into()),
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
                    // A completed migration swaps the active store to the new root;
                    // history windows key their reload on `history_version`, so bump
                    // it to make open History/Favorites views query the new root
                    // immediately. This also bumps the runtime revision — calling
                    // both separately would wake long-pollers on a snapshot whose
                    // revision moved but whose history_version had not.
                    bump_clipboard_version();
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
                    }.into()),
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
                error: result.err().map(Into::into),
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
                error: result.err().map(Into::into),
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
        let database = Arc::new(Mutex::new(db::HistoryDB::new_unavailable().unwrap()));
        ApiState {
            documents: Default::default(),
            db: database.clone(),
            image_archive: Arc::new(crate::automatic_images::AutomaticImageArchive::new(
                database,
                false,
                Arc::new(tailsync_core::composite::resource_archive::ReqwestTransport::new()),
                Arc::new(crate::api::bump_clipboard_version),
            )),
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
    async fn sync_toggle_returns_the_committed_snapshot_and_preserves_other_fields() {
        let mut original = crypto::Settings {
            sync_shortcut: "Alt+X".into(),
            history_limit: 250,
            ..Default::default()
        };
        let state = state(original.clone());
        let response = handle(
            SettingsCommand::SetSyncEnabled,
            serde_json::from_value(serde_json::json!({
                "cmd": "set_sync_enabled", "enabled": false
            }))
            .unwrap(),
            &state,
        )
        .await;
        assert!(response.ok);
        original.sync_enabled = false;
        assert_eq!(
            response.data,
            Some(serde_json::to_value(&original).unwrap())
        );
        assert_eq!(*state.settings.lock().await, original);
    }

    #[tokio::test]
    async fn update_settings_merges_two_stale_windows_without_rollback() {
        let state = state(crypto::Settings::default());
        // Both windows read the same snapshot before either of them saves.
        let stale = state.settings.lock().await.clone();
        assert_eq!(stale.history_limit, 100);
        assert_eq!(stale.connection_mode, "auto");

        let first = super::super::handle_cmd(
            serde_json::from_value(serde_json::json!({
                "cmd": "update_settings", "settings": {"history_limit": 250}
            }))
            .unwrap(),
            &state,
        )
        .await;
        assert!(first.ok, "{:?}", first.error);
        let canonical: crypto::Settings =
            serde_json::from_value(first.data.clone().unwrap()).unwrap();
        assert_eq!(canonical.history_limit, 250);
        assert_eq!(canonical.connection_mode, "auto");

        let second = super::super::handle_cmd(
            serde_json::from_value(serde_json::json!({
                "cmd": "update_settings", "settings": {"connection_mode": "lan_only"}
            }))
            .unwrap(),
            &state,
        )
        .await;
        assert!(second.ok, "{:?}", second.error);
        let canonical: crypto::Settings = serde_json::from_value(second.data.unwrap()).unwrap();
        assert_eq!(
            canonical.history_limit, 250,
            "the second window must not roll back the first window's edit"
        );
        assert_eq!(canonical.connection_mode, "lan_only");
        assert_eq!(*state.settings.lock().await, canonical);
    }

    #[tokio::test]
    async fn update_settings_route_rejects_whole_snapshots_and_owned_fields() {
        let original = crypto::Settings::default();
        let state = state(original.clone());
        for settings in [
            serde_json::json!({ "sync_enabled": false }),
            serde_json::json!({ "sync_shortcut": "Control+Shift+Z" }),
            serde_json::json!({ "history_shortcut": "Control+Shift+Z" }),
            serde_json::json!({ "enabled_peers": { "peer": false } }),
            serde_json::json!({ "storage_root": "/tmp/elsewhere" }),
            serde_json::to_value(&original).unwrap(),
        ] {
            let response = super::super::handle_cmd(
                serde_json::from_value(serde_json::json!({
                    "cmd": "update_settings", "settings": settings
                }))
                .unwrap(),
                &state,
            )
            .await;
            assert!(!response.ok, "{settings} must be rejected");
            let envelope = tailsync_runtime::contracts::StableErrorEnvelope::from_legacy_message(
                "update_settings",
                response.error.as_deref().unwrap(),
            );
            assert_eq!(
                envelope.code,
                tailsync_runtime::contracts::StableErrorCode::InvalidArgument
            );
            assert_eq!(*state.settings.lock().await, original);
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
