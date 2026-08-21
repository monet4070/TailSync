use super::*;

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
    let local_routes = local
        .candidates
        .iter()
        .map(|candidate| {
            serde_json::to_value(tailsync_core::peer::types::PeerRouteSnapshot {
                interface: candidate.interface,
                address: candidate.address.clone(),
                status: network::PeerStatus::Connected,
                online: true,
                connected: true,
                latency_ms: None,
                pairing_endpoint: false,
                rtt_capable: candidate.rtt_capable,
            })
            .expect("local route snapshot always serializes")
        })
        .collect::<Vec<_>>();

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

pub(super) async fn handle_cmd(req: Request, state: &ApiState) -> Response {
    match req.cmd.as_str() {
        "ping" => Response {
            ok: true,
            data: None,
            error: None,
        },

        "check_for_update" => {
            let result = match crate::updates::app_handle() {
                Ok(handle) => crate::updates::check_for_update(handle).await,
                Err(error) => Err(error),
            };
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

        "install_update" => {
            let result = match crate::updates::app_handle() {
                Ok(handle) => crate::updates::install_available_update(handle).await,
                Err(error) => Err(error),
            };
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
            data: Some(serde_json::json!({
                "tcp_server_healthy": network::TCP_SERVER_HEALTHY.load(Ordering::Acquire),
                "clipboard_monitor_healthy": crate::clipboard::monitor_is_healthy(),
                "clipboard_monitor_failures": crate::clipboard::monitor_failure_count(),
                "active_routes": network::active_routes_snapshot(),
            })),
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
                Ok(result) => Response {
                    ok: true,
                    data: serde_json::to_value(result).ok(),
                    error: None,
                },
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
            let result = req
                .path
                .as_deref()
                .ok_or_else(|| "missing path".to_string())
                .and_then(|path| {
                    db::delete_old_storage(std::path::Path::new(path))
                        .map_err(|error| error.to_string())
                });
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

        "refresh_peers" => {
            let mode = state.settings.lock().await.connection_mode.clone();
            match network::request_peer_refresh(&mode).await {
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
            }
        }

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

pub(crate) fn history_capabilities_data() -> Value {
    serde_json::json!({
        "classifier_version": crate::history_classifier::CLASSIFIER_VERSION,
        "categories": crate::history_classifier::CATEGORIES,
        "multiple_labels": true,
        "date_range_filter": true,
    })
}
