use super::*;

mod history;
mod peers;
mod settings;
mod theme;

/// Filesystem policy is shared with Windows through the themes Core module.
/// This small Adapter only keeps the existing route-local call sites readable.
#[allow(clippy::result_large_err)]
fn read_theme_package(path: &str) -> Result<Vec<u8>, tailsync_core::themes_v2::ThemeError> {
    tailsync_core::themes_v2::read_theme_package_file(std::path::Path::new(path))
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
    let storage = db::storage_status_async(&state.db).await;
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
    let command = req.cmd.clone();
    match command.as_str() {
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
            data: serde_json::to_value(db::storage_status_async(&state.db).await).ok(),
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

        "create_remote_pairing_invite" => match network::create_remote_pairing_invite(
            state.pairing.clone(),
            state.settings.clone(),
            state.remote_invites.clone(),
        )
        .await
        {
            Ok(invite) => Response {
                ok: true,
                data: Some(serde_json::json!({
                    "link": invite.as_link(),
                    "expires_at": invite.expires_at(),
                    "remaining_seconds": invite.remaining_seconds(),
                })),
                error: None,
            },
            Err(error) => Response {
                ok: false,
                data: None,
                error: Some(error),
            },
        },

        "inspect_remote_pairing_link" => {
            let link = req
                .invite_link
                .as_deref()
                .or(req.address.as_deref())
                .unwrap_or_default();
            match crate::pairing::RemotePairingInvite::parse(link) {
                Ok(invite) => Response {
                    ok: true,
                    data: Some(serde_json::json!({
                        "endpoint_id": invite.endpoint_id_string(),
                        "expires_at": invite.expires_at(),
                        "remaining_seconds": invite.remaining_seconds(),
                    })),
                    error: None,
                },
                Err(error) => Response {
                    ok: false,
                    data: None,
                    error: Some(error.to_string()),
                },
            }
        }

        "start_remote_pairing" => {
            let link = req
                .invite_link
                .as_deref()
                .or(req.address.as_deref())
                .unwrap_or_default()
                .trim();
            if link.is_empty() {
                return Response {
                    ok: false,
                    data: None,
                    error: Some("missing invite_link".into()),
                };
            }
            match network::start_remote_pairing(
                state.pairing.clone(),
                state.identity.clone(),
                state.settings.clone(),
                link,
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

        "get_remote_pairing_invite_status" => Response {
            ok: true,
            data: Some(serde_json::to_value(state.remote_invites.status()).unwrap_or_default()),
            error: None,
        },

        "cancel_remote_pairing_invite" => {
            state.remote_invites.cancel();
            Response {
                ok: true,
                data: Some(serde_json::to_value(state.pairing.cancel().await).unwrap_or_default()),
                error: None,
            }
        }

        command if history::handles(command) => history::handle(req, state).await,
        command if settings::handles(command) => settings::handle(req, state).await,
        command if peers::handles(command) => peers::handle(req, state).await,
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

        command if theme::handles(command) => theme::handle(req).await,

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
