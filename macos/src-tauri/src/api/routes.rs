use super::*;

const MAX_THEME_PACKAGE_BYTES: u64 = 64 * 1024 * 1024;

fn theme_path_error(
    code: &str,
    message: impl Into<String>,
) -> tailsync_core::themes_v2::ThemeError {
    tailsync_core::themes_v2::ThemeError {
        code: code.into(),
        message: message.into(),
        json_pointer: "/path".into(),
        platforms: vec!["macos".into()],
        severity: "error".into(),
        recoverable: true,
        fallback_applied: false,
    }
}

// Keep the structured ThemeError by value: these errors are immediately
// serialized into the JSON API response, and boxing them would add an
// allocation to every validation failure. The large error is intentional at
// this narrow boundary.
#[allow(clippy::result_large_err)]
fn read_theme_package(path: &str) -> Result<Vec<u8>, tailsync_core::themes_v2::ThemeError> {
    if !path.ends_with(".tailsync-theme") {
        return Err(theme_path_error(
            "THEME_EXTENSION",
            "theme package must end in .tailsync-theme",
        ));
    }
    let path = std::path::Path::new(path);
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| theme_path_error("THEME_IO", error.to_string()))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(theme_path_error(
            "THEME_PATH",
            "theme package must be a regular file, not a symbolic link",
        ));
    }
    if metadata.len() > MAX_THEME_PACKAGE_BYTES {
        return Err(theme_path_error(
            "THEME_TOO_LARGE",
            "theme package exceeds the 64 MiB import limit",
        ));
    }
    let canonical = path
        .canonicalize()
        .map_err(|error| theme_path_error("THEME_IO", error.to_string()))?;
    std::fs::read(canonical).map_err(|error| theme_path_error("THEME_IO", error.to_string()))
}

pub(crate) fn peer_snapshot_data(
    identity: &DeviceIdentity,
    settings: &crypto::Settings,
    discovery: Result<
        (
            network::tailscale::LocalInfo,
            Vec<network::tailscale::PeerInfo>,
        ),
        String,
    >,
) -> Value {
    let mode = settings.connection_mode.clone();
    let (local, peers, discovery_error) = match discovery {
        Ok((local, peers)) => (local, peers, None),
        Err(error) => (
            network::tailscale::LocalInfo {
                hostname: network::lan::local_hostname(),
                tailscale_ip: String::new(),
                candidates: Vec::new(),
            },
            Vec::new(),
            Some(error),
        ),
    };

    let mut peers = network::merge_paired_peers(settings, &mode, peers);
    network::apply_peer_health(&mut peers);
    let peers = peers
        .into_iter()
        .filter_map(|peer| {
            let paired_endpoint = settings.paired_peer_endpoints.get(&peer.hostname);
            let routes = peer
                .candidates
                .iter()
                .map(|candidate| {
                    let connected = peer.current_address.as_deref() == Some(&candidate.address);
                    serde_json::to_value(tailsync_core::peer::types::PeerRouteSnapshot {
                        interface: candidate.interface,
                        address: candidate.address.clone(),
                        status: if connected {
                            network::PeerStatus::Connected
                        } else {
                            candidate.status
                        },
                        online: candidate.online,
                        connected,
                        latency_ms: candidate.latency,
                        pairing_endpoint: paired_endpoint == Some(&candidate.address),
                        rtt_capable: candidate.rtt_capable,
                    })
                    .expect("peer route snapshot always serializes")
                })
                .collect::<Vec<_>>();
            let mut value = match serde_json::to_value(&peer) {
                Ok(value) => value,
                Err(error) => {
                    log::warn!("Could not serialize peer snapshot: {error}");
                    return None;
                }
            };
            value["routes"] = Value::Array(routes);
            let protocol_error = peer
                .trusted
                .then(|| network::protocol_compatibility_error(&peer.hostname))
                .flatten();
            value["protocol_error"] = serde_json::json!(protocol_error);
            value["required_protocol_version"] = protocol_error
                .as_ref()
                .map(|_| serde_json::json!(crate::protocol::VERSION))
                .unwrap_or(Value::Null);
            Some(value)
        })
        .collect::<Vec<_>>();
    let local_routes = network::mode_interface(&mode)
        .or_else(|| network::infer_interface(&local.tailscale_ip).ok())
        .filter(|_| !local.tailscale_ip.is_empty())
        .map(|interface| {
            let rtt_capable = interface != network::ConnectionInterface::Iroh;
            vec![
                serde_json::to_value(tailsync_core::peer::types::PeerRouteSnapshot {
                    interface,
                    address: local.tailscale_ip.clone(),
                    status: network::PeerStatus::Connected,
                    online: true,
                    connected: true,
                    latency_ms: None,
                    pairing_endpoint: false,
                    rtt_capable,
                })
                .expect("local route snapshot always serializes"),
            ]
        })
        .unwrap_or_default();

    serde_json::json!({
        "self": {
            "hostname": local.hostname,
            "tailscale_ip": local.tailscale_ip,
            "routes": local_routes,
            "connection_mode": mode,
            "public_key": identity.public_key_base64(),
            "fingerprint": identity.fingerprint(),
            "iroh_endpoint_id": network::local_iroh_endpoint_id(&mode),
        },
        "peers": peers,
        "paired_peer_endpoints": settings.paired_peer_endpoints,
        "discovery_error": discovery_error,
    })
}

fn daemon_status_data() -> Value {
    serde_json::json!({
        "tcp_server_healthy": network::TCP_SERVER_HEALTHY.load(Ordering::Acquire),
        "clipboard_monitor_healthy": crate::clipboard::monitor_is_healthy(),
        "clipboard_monitor_failures": crate::clipboard::monitor_failure_count(),
        "active_routes": network::active_routes_snapshot(),
    })
}

async fn runtime_snapshot_data(state: &ApiState, since_notification_id: Option<u64>) -> Value {
    // A change during snapshot assembly remains observable: the response keeps
    // the starting revision, so the client's next wait returns immediately.
    let revision = get_runtime_revision();
    let sync_enabled = state.settings.lock().await.sync_enabled;
    let storage = state.db.lock().await.storage_status();
    serde_json::json!({
        "revision": revision,
        "history_version": get_clipboard_version(),
        "progress": get_file_progress(),
        "storage": storage,
        "sync_enabled": sync_enabled,
        "status": daemon_status_data(),
        "notifications": since_notification_id
            .map(get_runtime_notifications_since)
            .unwrap_or_default(),
    })
}

pub(super) async fn handle_cmd(req: Request, state: &ApiState) -> Response {
    match req.cmd.as_str() {
        "ping" => Response {
            ok: true,
            data: None,
            error: None,
        },

        "wait_runtime_snapshot" => {
            let since = req.since_revision.unwrap_or_default();
            let wait_ms = req.wait_ms.unwrap_or(2_500).clamp(50, 15_000);
            let _ = wait_for_runtime_revision(since, Duration::from_millis(wait_ms)).await;
            Response {
                ok: true,
                data: Some(runtime_snapshot_data(state, req.since_notification_id).await),
                error: None,
            }
        }

        "check_for_update" => {
            #[cfg(target_os = "macos")]
            {
                let result = crate::updates::check_for_update_headless().await;
                match result {
                    Ok(update) => Response {
                        ok: true,
                        data: Some(serde_json::to_value(update).unwrap_or(Value::Null)),
                        error: None,
                    },
                    Err(error) => Response {
                        ok: false,
                        data: None,
                        error: Some(error),
                    },
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                Response {
                    ok: false,
                    data: None,
                    error: Some("Headless updates are only available on macOS".to_string()),
                }
            }
        }

        "install_update" => {
            #[cfg(target_os = "macos")]
            {
                let result = crate::updates::install_available_update_headless().await;
                match result {
                    Ok(installed) => Response {
                        ok: true,
                        data: Some(serde_json::json!({ "installed": installed })),
                        error: None,
                    },
                    Err(error) => Response {
                        ok: false,
                        data: None,
                        error: Some(error),
                    },
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                Response {
                    ok: false,
                    data: None,
                    error: Some("Headless updates are only available on macOS".to_string()),
                }
            }
        }

        "get_file_progress" => {
            let info = get_file_progress();
            Response {
                ok: true,
                data: info.and_then(|progress| serde_json::to_value(progress).ok()),
                error: None,
            }
        }

        "cancel_file_batch" => {
            let result = match req.batch_id.as_deref() {
                Some(value) => match crate::protocol::TransferId::from_hex(value) {
                    Ok(id) => {
                        crate::commands::cancel_file_batch_impl(
                            &state.sync_engine,
                            &state.pool,
                            &state.settings,
                            id,
                        )
                        .await;
                        Ok(())
                    }
                    Err(error) => Err(error),
                },
                None => Err("missing batch_id".to_string()),
            };
            Response {
                ok: result.is_ok(),
                data: None,
                error: result.err(),
            }
        }

        "restore_file_batch" => {
            let result = match req.batch_id.as_deref() {
                Some(batch_id) => crate::commands::materialize_file_batch_paths(
                    state.db.clone(),
                    batch_id.to_string(),
                )
                .await
                .and_then(|paths| crate::clipboard_file::write_clipboard_files(&paths)),
                None => Err("missing batch_id".to_string()),
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

        "get_storage_status" => Response {
            ok: true,
            data: serde_json::to_value(state.db.lock().await.storage_status()).ok(),
            error: None,
        },

        "get_version" => Response {
            ok: true,
            data: Some(serde_json::json!(CLIPBOARD_VERSION.load(Ordering::Acquire))),
            error: None,
        },

        "get_sync_warning" => Response {
            ok: true,
            data: serde_json::to_value(tailsync_core::sync_warning::take()).ok(),
            error: None,
        },

        "get_history_capabilities" => Response {
            ok: true,
            data: Some(history_capabilities_data()),
            error: None,
        },

        "get_migration_diagnostics" => {
            let result = state
                .db
                .lock()
                .await
                .migration_diagnostics(50)
                .map_err(|error| error.to_string());
            match result {
                Ok(diagnostics) => Response {
                    ok: true,
                    data: Some(serde_json::to_value(diagnostics).unwrap_or_default()),
                    error: None,
                },
                Err(error) => Response {
                    ok: false,
                    data: None,
                    error: Some(error),
                },
            }
        }

        "get_status" => Response {
            ok: true,
            data: Some(daemon_status_data()),
            error: None,
        },

        "enable_pairing" => Response {
            ok: true,
            data: Some(serde_json::to_value(state.pairing.enable().await).unwrap_or_default()),
            error: None,
        },

        "get_pairing_status" => Response {
            ok: true,
            data: Some(serde_json::to_value(state.pairing.status().await).unwrap_or_default()),
            error: None,
        },

        "start_pairing" => {
            let address = req.address.as_deref().unwrap_or_default().trim();
            if address.is_empty() {
                return Response {
                    ok: false,
                    data: None,
                    error: Some("missing peer address".into()),
                };
            }
            match network::start_pairing(
                state.pairing.clone(),
                state.identity.clone(),
                state.settings.clone(),
                address,
            )
            .await
            {
                Ok(()) => Response {
                    ok: true,
                    data: Some(
                        serde_json::to_value(state.pairing.status().await).unwrap_or_default(),
                    ),
                    error: None,
                },
                Err(error) => Response {
                    ok: false,
                    data: Some(
                        serde_json::to_value(state.pairing.status().await).unwrap_or_default(),
                    ),
                    error: Some(error),
                },
            }
        }

        "confirm_pairing" => match state
            .pairing
            .confirm()
            .await
            .map_err(|error| error.to_string())
        {
            Ok(status) => Response {
                ok: true,
                data: Some(serde_json::to_value(status).unwrap_or_default()),
                error: None,
            },
            Err(error) => Response {
                ok: false,
                data: Some(serde_json::to_value(state.pairing.status().await).unwrap_or_default()),
                error: Some(error),
            },
        },

        "cancel_pairing" => Response {
            ok: true,
            data: Some(serde_json::to_value(state.pairing.cancel().await).unwrap_or_default()),
            error: None,
        },

        "get_history" => {
            let db = state.db.lock().await;
            // Consume before await for Send safety
            let result = db
                .get_all_filtered(
                    req.keyword.as_deref(),
                    req.category.as_deref(),
                    req.start_time.as_deref(),
                    req.end_time.as_deref(),
                    req.limit.unwrap_or(30),
                    req.offset.unwrap_or(0),
                )
                .map_err(|e| e.to_string());
            drop(db);
            match result {
                Ok(entries) => Response {
                    ok: true,
                    data: Some(serde_json::to_value(entries).unwrap_or_default()),
                    error: None,
                },
                Err(e) => Response {
                    ok: false,
                    data: None,
                    error: Some(e),
                },
            }
        }

        "get_preview_data" => {
            let Some(id) = req.id else {
                return Response {
                    ok: false,
                    data: None,
                    error: Some("missing id".into()),
                };
            };

            // Keep the decrypted payload in memory only.  The core preview
            // accessor deliberately avoids materialising file entries to a
            // plaintext path, which is important for the macOS Quick Look
            // caller and for text/image previews alike.
            let db = state.db.lock().await;
            let preview_id = match req.batch_id.as_deref() {
                Some(batch_id) => db
                    .get_preview_batch_navigation(batch_id, id)
                    .map(|navigation| navigation.first_entry_id),
                None => Ok(id),
            };
            let result = preview_id.and_then(|preview_id| {
                let metadata = db.get_preview_metadata(preview_id)?;
                let payload = db.get_preview_payload(preview_id)?;
                Ok((metadata, payload))
            });
            drop(db);

            let result = result.map(|(metadata, payload)| {
                use base64::Engine;
                serde_json::json!({
                    "entry_id": metadata.entry_id,
                    "kind": payload.kind,
                    "name": payload.name,
                    "size_bytes": payload.size_bytes,
                    "batch": metadata.batch,
                    "data_b64": base64::engine::general_purpose::STANDARD
                        .encode(payload.data),
                })
            });

            match result {
                Ok(data) => Response {
                    ok: true,
                    data: Some(data),
                    error: None,
                },
                Err(error) => {
                    let failure = db::PreviewErrorInfo::from(error);
                    let encoded =
                        serde_json::to_string(&failure).unwrap_or_else(|_| failure.message.clone());
                    Response {
                        ok: false,
                        data: None,
                        error: Some(encoded),
                    }
                }
            }
        }

        "delete_entry" => {
            let Some(id) = req.id else {
                return Response {
                    ok: false,
                    data: None,
                    error: Some("missing id".into()),
                };
            };
            let mut db = state.db.lock().await;
            match db.delete(id) {
                Ok(()) => {
                    bump_clipboard_version();
                    Response {
                        ok: true,
                        data: None,
                        error: None,
                    }
                }
                Err(e) => Response {
                    ok: false,
                    data: None,
                    error: Some(e.to_string()),
                },
            }
        }

        "clear_history" | "clear_all" => {
            let mut db = state.db.lock().await;
            match db.clear_all() {
                Ok(()) => {
                    bump_clipboard_version();
                    Response {
                        ok: true,
                        data: None,
                        error: None,
                    }
                }
                Err(e) => Response {
                    ok: false,
                    data: None,
                    error: Some(e.to_string()),
                },
            }
        }

        "restore_entry" => {
            let Some(id) = req.id else {
                return Response {
                    ok: false,
                    data: None,
                    error: Some("missing id".into()),
                };
            };
            let db = state.db.lock().await;
            let entry_type = db
                .get_type(id)
                .map_err(|e| e.to_string())
                .unwrap_or_default();
            let file_path = if entry_type == "file" {
                db.get_file_path(id).map_err(|e| e.to_string())
            } else {
                Ok(None)
            };
            let file_name = if entry_type == "file" {
                db.get_description(id)
                    .unwrap_or_else(|_| "restored_file".into())
            } else {
                String::new()
            };
            let data_result = match &file_path {
                Ok(Some(_)) => Ok(None),
                Ok(None) => db.get_data(id).map(Some).map_err(|e| e.to_string()),
                Err(error) => Err(error.clone()),
            };
            drop(db);
            match (data_result, file_path) {
                (Ok(data), Ok(file_path)) => {
                    if entry_type == "image" {
                        let Some(data) = data.as_ref() else {
                            return Response {
                                ok: false,
                                data: None,
                                error: Some("image history data is unavailable".into()),
                            };
                        };
                        if let Err(error) = state.sync_engine.lock().await.restore_image(data) {
                            return Response {
                                ok: false,
                                data: None,
                                error: Some(error),
                            };
                        }
                    } else if entry_type == "file" {
                        if let Some(path) = file_path {
                            if let Err(error) = restore_file_path_to_clipboard(&path, &file_name) {
                                return Response {
                                    ok: false,
                                    data: None,
                                    error: Some(error),
                                };
                            }
                        } else if let Some(data) = data.as_deref() {
                            if let Err(error) = restore_file_to_clipboard(data, &file_name) {
                                return Response {
                                    ok: false,
                                    data: None,
                                    error: Some(error),
                                };
                            }
                        } else {
                            return Response {
                                ok: false,
                                data: None,
                                error: Some("file history data is unavailable".into()),
                            };
                        }
                    } else {
                        let text = String::from_utf8_lossy(data.as_deref().unwrap_or_default())
                            .to_string();
                        if let Err(error) = state.sync_engine.lock().await.restore_text(&text) {
                            return Response {
                                ok: false,
                                data: None,
                                error: Some(error),
                            };
                        }
                    }

                    crate::api::bump_clipboard_version();
                    Response {
                        ok: true,
                        data: None,
                        error: None,
                    }
                }
                (Err(e), _) | (_, Err(e)) => Response {
                    ok: false,
                    data: None,
                    error: Some(e),
                },
            }
        }

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
            if result.is_ok() {
                bump_runtime_revision();
            }
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

        "set_sync_shortcut" => {
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

        "set_history_shortcut" => {
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

        "get_peers" => {
            let settings = state.settings.lock().await.clone();
            let mode = settings.connection_mode.clone();
            let discovery = network::cached_discover_peers(&mode).await;
            Response {
                ok: true,
                data: Some(peer_snapshot_data(&state.identity, &settings, discovery)),
                error: None,
            }
        }

        "refresh_peers" => match network::request_peer_refresh_and_wait().await {
            Ok(()) => {
                let settings = state.settings.lock().await.clone();
                let mode = settings.connection_mode.clone();
                let discovery = network::cached_discover_peers(&mode).await;
                Response {
                    ok: true,
                    data: Some(peer_snapshot_data(&state.identity, &settings, discovery)),
                    error: None,
                }
            }
            Err(error) => Response {
                ok: false,
                data: None,
                error: Some(error),
            },
        },

        "toggle_peer" => {
            let hostname = req.hostname.as_deref().unwrap_or_default().trim();
            if hostname.is_empty() {
                return Response {
                    ok: false,
                    data: None,
                    error: Some("missing hostname".into()),
                };
            }
            let enabled = req.enabled.unwrap_or(true);
            let result = state
                .settings
                .lock()
                .await
                .toggle_peer(hostname, enabled)
                .map_err(|error| error.to_string());
            if result.is_ok() && !enabled {
                state.pool.lock().await.disconnect_hostname(hostname);
            }
            match result {
                Ok(()) => Response {
                    ok: true,
                    data: None,
                    error: None,
                },
                Err(error) => Response {
                    ok: false,
                    data: None,
                    error: Some(error),
                },
            }
        }

        "trust_peer" => {
            let hostname = req.hostname.as_deref().unwrap_or_default().trim();
            let public_key = req.public_key.as_deref().unwrap_or_default();
            let address = req
                .address
                .as_deref()
                .filter(|value| !value.trim().is_empty());
            let result = identity::trust_peer(
                &state.identity,
                &state.settings,
                &|settings: &crate::crypto::Settings| {
                    settings.save().map_err(|error| error.to_string())
                },
                hostname,
                public_key,
                address,
            )
            .await
            .map_err(|failure| match failure {
                identity::TrustPeerFailure::InvalidHostname => "invalid hostname".to_string(),
                identity::TrustPeerFailure::SelfPairing => {
                    "cannot pair this device with itself".to_string()
                }
                identity::TrustPeerFailure::Key(error)
                | identity::TrustPeerFailure::Interface(error)
                | identity::TrustPeerFailure::Trust(error) => error,
            });
            if result.is_ok() {
                state.pool.lock().await.disconnect_hostname(hostname);
                network::clear_protocol_compatibility_error(hostname);
            }
            match result {
                Ok(fingerprint) => Response {
                    ok: true,
                    data: Some(serde_json::json!({ "fingerprint": fingerprint })),
                    error: None,
                },
                Err(error) => Response {
                    ok: false,
                    data: None,
                    error: Some(error),
                },
            }
        }

        "forget_peer" => {
            let hostname = req.hostname.as_deref().unwrap_or_default().trim();
            if hostname.is_empty() {
                return Response {
                    ok: false,
                    data: None,
                    error: Some("missing hostname".into()),
                };
            }
            let result = state
                .settings
                .lock()
                .await
                .forget_peer(hostname)
                .map_err(|error| error.to_string());
            if result.is_ok() {
                state.pool.lock().await.disconnect_hostname(hostname);
                network::clear_protocol_compatibility_error(hostname);
            }
            match result {
                Ok(()) => Response {
                    ok: true,
                    data: None,
                    error: None,
                },
                Err(error) => Response {
                    ok: false,
                    data: None,
                    error: Some(error),
                },
            }
        }

        "test_connection" => {
            let hostname = req.hostname.as_deref().unwrap_or_default().trim();
            if hostname.is_empty() {
                return Response {
                    ok: false,
                    data: None,
                    error: Some("missing address".into()),
                };
            }
            match network::test_connection(hostname).await {
                Ok(route) => {
                    network::record_address_test_success(hostname, route.latency_ms);
                    Response {
                        ok: true,
                        data: Some(serde_json::json!({
                            "latency_ms": route.latency_ms,
                            "path": route.path,
                        })),
                        error: None,
                    }
                }
                Err(error) => {
                    network::record_address_test_failure(hostname);
                    Response {
                        ok: false,
                        data: None,
                        error: Some(error),
                    }
                }
            }
        }

        "reconnect_peers" => {
            state.pool.lock().await.disconnect_all();
            crate::clipboard::request_wake_recovery();
            network::clear_peer_cache().await;
            Response {
                ok: true,
                data: None,
                error: None,
            }
        }

        "get_image_data" => {
            let Some(id) = req.id else {
                return Response {
                    ok: false,
                    data: None,
                    error: Some("missing id".into()),
                };
            };
            let db = state.db.lock().await;
            let result = db.get_data(id).map_err(|e| e.to_string());
            drop(db);
            match result {
                Ok(data) => {
                    let image = match crate::protocol::PackedImage::try_from(data.as_slice()) {
                        Ok(image) => image,
                        Err(error) => {
                            return Response {
                                ok: false,
                                data: None,
                                error: Some(error.to_string()),
                            }
                        }
                    };
                    // Downsample to a recognizable thumbnail (longest edge).
                    let (tw, th, thumb) = thumbnail_rgba(image, THUMBNAIL_MAX_SIDE);
                    // The full-size RGBA (up to 32 MiB) is now dead; release it
                    // before base64-encoding the ~100 KB thumbnail so the large
                    // buffer and the encoded copy never coexist.
                    drop(data);
                    use base64::Engine;
                    let b64 = base64::engine::general_purpose::STANDARD.encode(&thumb);
                    Response {
                        ok: true,
                        data: Some(serde_json::json!({
                            "width": tw,
                            "height": th,
                            "rgba_b64": b64,
                        })),
                        error: None,
                    }
                }
                Err(e) => Response {
                    ok: false,
                    data: None,
                    error: Some(e),
                },
            }
        }

        "begin_import" => import_response(begin_import(&req, state).await),

        "import_chunk" => import_response(append_import_chunk(&req, state).await),

        "finish_import" => import_response(finish_import(&req, state).await),

        "migrate_entry" => {
            let (Some(time), Some(etype), Some(desc), Some(data_b64)) =
                (&req.time, &req.entry_type, &req.desc, &req.data_b64)
            else {
                return Response {
                    ok: false,
                    data: None,
                    error: Some("missing fields".into()),
                };
            };
            use base64::Engine;
            let data = match base64::engine::general_purpose::STANDARD.decode(data_b64) {
                Ok(d) => d,
                Err(e) => {
                    return Response {
                        ok: false,
                        data: None,
                        error: Some(e.to_string()),
                    }
                }
            };
            if chrono::DateTime::parse_from_rfc3339(time).is_err() {
                return Response {
                    ok: false,
                    data: None,
                    error: Some("invalid import timestamp".into()),
                };
            }
            let limit = match import_size_limit(etype) {
                Ok(limit) => limit,
                Err(error) => {
                    return Response {
                        ok: false,
                        data: None,
                        error: Some(error),
                    }
                }
            };
            if data.len() as u64 > limit {
                return Response {
                    ok: false,
                    data: None,
                    error: Some(format!("{etype} import exceeds the {limit} byte limit")),
                };
            }
            let mut db = state.db.lock().await;
            let result = match etype.as_str() {
                "text" => db.add_text_migrated(time, desc, &data),
                "image" => db.add_image_migrated(time, desc, &data),
                "file" => db.add_file_migrated(time, desc, &data),
                _ => Err("unknown type".into()),
            };
            match result {
                Ok(()) => {
                    crate::api::bump_clipboard_version();
                    Response {
                        ok: true,
                        data: None,
                        error: None,
                    }
                }
                Err(e) => Response {
                    ok: false,
                    data: None,
                    error: Some(e.to_string()),
                },
            }
        }

        "list_themes_v2" => Response {
            ok: true,
            data: serde_json::to_value(tailsync_core::themes_v2::list_themes_v2()).ok(),
            error: None,
        },

        "get_local_theme_settings" => Response {
            ok: true,
            data: serde_json::to_value(tailsync_core::themes_v2::get_local_theme_settings()).ok(),
            error: None,
        },

        "set_local_theme_settings" => {
            let result = req
                .settings
                .ok_or_else(|| tailsync_core::themes_v2::ThemeError {
                    code: "THEME_SETTINGS".into(),
                    message: "missing settings".into(),
                    json_pointer: "/settings".into(),
                    platforms: vec!["macos".into()],
                    severity: "error".into(),
                    recoverable: true,
                    fallback_applied: false,
                })
                .and_then(|value| {
                    serde_json::from_value(value).map_err(|error| {
                        tailsync_core::themes_v2::ThemeError {
                            code: "THEME_SETTINGS".into(),
                            message: error.to_string(),
                            json_pointer: "/settings".into(),
                            platforms: vec!["macos".into()],
                            severity: "error".into(),
                            recoverable: true,
                            fallback_applied: false,
                        }
                    })
                })
                .and_then(tailsync_core::themes_v2::set_local_theme_settings);
            match result {
                Ok(()) => Response {
                    ok: true,
                    data: None,
                    error: None,
                },
                Err(error) => Response {
                    ok: false,
                    data: Some(serde_json::to_value(&error).unwrap_or(Value::Null)),
                    error: Some(error.to_string()),
                },
            }
        }

        "validate_theme" => {
            let result: Result<
                tailsync_core::themes_v2::ThemeValidation,
                tailsync_core::themes_v2::ThemeError,
            > = req
                .path
                .as_deref()
                .ok_or_else(|| tailsync_core::themes_v2::ThemeError {
                    code: "THEME_PATH".into(),
                    message: "missing path".into(),
                    json_pointer: "/path".into(),
                    platforms: vec!["macos".into()],
                    severity: "error".into(),
                    recoverable: true,
                    fallback_applied: false,
                })
                .and_then(read_theme_package)
                .map(|bytes| {
                    tailsync_core::themes_v2::validate_theme_for_platform(
                        &bytes,
                        req.mode.as_deref().unwrap_or("light"),
                        "macos",
                        req.high_contrast.unwrap_or(false),
                    )
                });
            match result {
                Ok(value) => Response {
                    ok: true,
                    data: serde_json::to_value(value).ok(),
                    error: None,
                },
                Err(error) => Response {
                    ok: false,
                    data: Some(
                        serde_json::to_value(tailsync_core::themes_v2::ThemeError {
                            code: "THEME_IO".into(),
                            message: error.to_string(),
                            json_pointer: "/path".into(),
                            platforms: vec!["macos".into()],
                            severity: "error".into(),
                            recoverable: true,
                            fallback_applied: false,
                        })
                        .unwrap_or(Value::Null),
                    ),
                    error: Some(error.to_string()),
                },
            }
        }

        "install_theme" | "update_theme" => {
            let result: Result<
                tailsync_core::themes_v2::ThemeDescriptor,
                tailsync_core::themes_v2::ThemeError,
            > = (|| {
                let path =
                    req.path
                        .as_deref()
                        .ok_or_else(|| tailsync_core::themes_v2::ThemeError {
                            code: "THEME_IO".into(),
                            message: "missing path".into(),
                            json_pointer: "/path".into(),
                            platforms: vec!["macos".into()],
                            severity: "error".into(),
                            recoverable: true,
                            fallback_applied: false,
                        })?;
                let digest = req.expected_digest.as_deref().ok_or_else(|| {
                    tailsync_core::themes_v2::ThemeError {
                        code: "THEME_DIGEST".into(),
                        message: "missing expected digest".into(),
                        json_pointer: "/expectedDigest".into(),
                        platforms: vec!["macos".into()],
                        severity: "error".into(),
                        recoverable: true,
                        fallback_applied: false,
                    }
                })?;
                let bytes = read_theme_package(path)?;
                if req.cmd == "install_theme" {
                    tailsync_core::themes_v2::install_theme(&bytes, digest)
                } else {
                    let options = req
                        .options
                        .clone()
                        .map(serde_json::from_value)
                        .transpose()
                        .map_err(|e| tailsync_core::themes_v2::ThemeError {
                            code: "THEME_OPTIONS".into(),
                            message: e.to_string(),
                            json_pointer: "/options".into(),
                            platforms: vec!["macos".into()],
                            severity: "error".into(),
                            recoverable: true,
                            fallback_applied: false,
                        })?
                        .unwrap_or_default();
                    tailsync_core::themes_v2::update_theme(&bytes, digest, options)
                }
            })();
            match result {
                Ok(value) => Response {
                    ok: true,
                    data: serde_json::to_value(value).ok(),
                    error: None,
                },
                Err(error) => Response {
                    ok: false,
                    data: Some(serde_json::to_value(&error).unwrap_or(Value::Null)),
                    error: Some(error.to_string()),
                },
            }
        }

        "rollback_theme" => {
            let result = req
                .theme_id
                .as_deref()
                .ok_or_else(|| tailsync_core::themes_v2::ThemeError {
                    code: "THEME_ID".into(),
                    message: "missing theme id".into(),
                    json_pointer: "/themeId".into(),
                    platforms: vec!["macos".into()],
                    severity: "error".into(),
                    recoverable: true,
                    fallback_applied: false,
                })
                .and_then(tailsync_core::themes_v2::rollback_theme);
            match result {
                Ok(value) => Response {
                    ok: true,
                    data: serde_json::to_value(value).ok(),
                    error: None,
                },
                Err(error) => Response {
                    ok: false,
                    data: Some(serde_json::to_value(&error).unwrap_or(Value::Null)),
                    error: Some(error.to_string()),
                },
            }
        }

        "delete_theme_v2" => {
            let result = if let Some(handle) = req.storage_handle.as_deref() {
                tailsync_core::themes_v2::delete_theme_by_handle_for_theme(
                    handle,
                    req.theme_id.as_deref().unwrap_or(""),
                )
            } else if let Some(id) = req.theme_id.as_deref() {
                tailsync_core::themes_v2::delete_theme(id)
            } else {
                Err(tailsync_core::themes_v2::ThemeError {
                    code: "THEME_ID".into(),
                    message: "missing theme_id or storage_handle".into(),
                    json_pointer: "".into(),
                    platforms: vec!["macos".into()],
                    severity: "error".into(),
                    recoverable: true,
                    fallback_applied: false,
                })
            };
            match result {
                Ok(()) => Response {
                    ok: true,
                    data: None,
                    error: None,
                },
                Err(error) => Response {
                    ok: false,
                    data: Some(serde_json::to_value(&error).unwrap_or(Value::Null)),
                    error: Some(error.to_string()),
                },
            }
        }

        "resolve_theme" => {
            let theme_id = req
                .theme_id
                .as_deref()
                .unwrap_or(tailsync_core::themes_v2::CANVAS_ID);
            let mode = req.mode.as_deref().unwrap_or("light");
            match tailsync_core::themes_v2::resolve_theme(
                theme_id,
                mode,
                "macos",
                req.high_contrast.unwrap_or(false),
            ) {
                Ok(theme) => Response {
                    ok: true,
                    data: serde_json::to_value(theme).ok(),
                    error: None,
                },
                Err(error) => Response {
                    ok: false,
                    data: Some(serde_json::to_value(&error).unwrap_or(Value::Null)),
                    error: Some(error.to_string()),
                },
            }
        }

        "get_theme_asset_slot" => {
            use base64::Engine;
            let result = req
                .theme_id
                .as_deref()
                .ok_or_else(|| tailsync_core::themes_v2::ThemeError {
                    code: "THEME_ID".into(),
                    message: "missing theme id".into(),
                    json_pointer: "/themeId".into(),
                    platforms: vec!["macos".into()],
                    severity: "error".into(),
                    recoverable: true,
                    fallback_applied: false,
                })
                .and_then(|id| {
                    req.expected_digest
                        .as_deref()
                        .ok_or_else(|| tailsync_core::themes_v2::ThemeError {
                            code: "THEME_DIGEST".into(),
                            message: "missing theme digest".into(),
                            json_pointer: "/digest".into(),
                            platforms: vec!["macos".into()],
                            severity: "error".into(),
                            recoverable: true,
                            fallback_applied: false,
                        })
                        .map(|digest| (id, digest))
                })
                .and_then(|(id, digest)| {
                    req.asset_slot
                        .as_deref()
                        .ok_or_else(|| tailsync_core::themes_v2::ThemeError {
                            code: "THEME_ASSET_SLOT".into(),
                            message: "missing asset slot".into(),
                            json_pointer: "/slot".into(),
                            platforms: vec!["macos".into()],
                            severity: "error".into(),
                            recoverable: true,
                            fallback_applied: false,
                        })
                        .and_then(|slot| {
                            tailsync_core::themes_v2::get_theme_asset_slot(id, digest, slot)
                        })
                });
            match result {
                Ok((descriptor, bytes)) => Response {
                    ok: true,
                    data: Some(
                        serde_json::json!({"descriptor": descriptor, "data_b64": base64::engine::general_purpose::STANDARD.encode(bytes)}),
                    ),
                    error: None,
                },
                Err(error) => Response {
                    ok: false,
                    data: Some(serde_json::to_value(&error).unwrap_or(Value::Null)),
                    error: Some(error.to_string()),
                },
            }
        }

        "preview_theme_asset_slot" => {
            use base64::Engine;
            let result: Result<Vec<u8>, tailsync_core::themes_v2::ThemeError> = req
                .path
                .as_deref()
                .ok_or_else(|| tailsync_core::themes_v2::ThemeError {
                    code: "THEME_PATH".into(),
                    message: "missing path".into(),
                    json_pointer: "/path".into(),
                    platforms: vec!["macos".into()],
                    severity: "error".into(),
                    recoverable: true,
                    fallback_applied: false,
                })
                .and_then(read_theme_package)
                .and_then(|bytes| {
                    let digest = req.expected_digest.as_deref().ok_or_else(|| {
                        tailsync_core::themes_v2::ThemeError {
                            code: "THEME_DIGEST".into(),
                            message: "missing digest".into(),
                            json_pointer: "/digest".into(),
                            platforms: vec!["macos".into()],
                            severity: "error".into(),
                            recoverable: true,
                            fallback_applied: false,
                        }
                    })?;
                    let slot = req.asset_slot.as_deref().ok_or_else(|| {
                        tailsync_core::themes_v2::ThemeError {
                            code: "THEME_ASSET_SLOT".into(),
                            message: "missing asset slot".into(),
                            json_pointer: "/slot".into(),
                            platforms: vec!["macos".into()],
                            severity: "error".into(),
                            recoverable: true,
                            fallback_applied: false,
                        }
                    })?;
                    tailsync_core::themes_v2::get_theme_asset_slot_from_package(
                        &bytes, digest, slot,
                    )
                    .map(|(_descriptor, asset)| asset)
                });
            match result {
                Ok(bytes) => Response {
                    ok: true,
                    data: Some(
                        serde_json::json!({"data_b64": base64::engine::general_purpose::STANDARD.encode(bytes)}),
                    ),
                    error: None,
                },
                Err(error) => Response {
                    ok: false,
                    data: Some(serde_json::to_value(&error).unwrap_or(Value::Null)),
                    error: Some(error.to_string()),
                },
            }
        }

        "quit" => {
            info!("Quit via API");
            Response {
                ok: true,
                data: None,
                error: None,
            }
        }

        _ => Response {
            ok: false,
            data: None,
            error: Some(format!("unknown command: {}", req.cmd)),
        },
    }
}

fn paths_equivalent(left: &std::path::Path, right: &std::path::Path) -> bool {
    let left = left.canonicalize().unwrap_or_else(|_| left.to_path_buf());
    let right = right.canonicalize().unwrap_or_else(|_| right.to_path_buf());
    if cfg!(windows) {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    } else {
        left == right
    }
}

pub(crate) fn history_capabilities_data() -> Value {
    serde_json::json!({
        "classifier_version": crate::history_classifier::CLASSIFIER_VERSION,
        "categories": crate::history_classifier::CATEGORIES,
        "multiple_labels": true,
        "date_range_filter": true,
    })
}
