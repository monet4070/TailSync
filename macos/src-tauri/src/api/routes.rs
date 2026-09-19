use super::*;
mod registry;
use registry::{BuiltinCommand, LocalCommand};

mod history;
mod peers;
mod settings;
mod theme;

pub(crate) use history::preview_binary_response;

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
        .map(|peer| {
            let endpoint = settings.paired_peer_endpoints.get(&peer.hostname);
            let error = peer
                .trusted
                .then(|| network::protocol_compatibility_error(&peer.hostname))
                .flatten();
            tailsync_runtime::contracts::PeerSnapshot::new(
                peer,
                endpoint,
                error,
                crate::protocol::VERSION,
            )
        })
        .collect();
    let local_routes = network::mode_interface(&mode)
        .or_else(|| network::infer_interface(&local.tailscale_ip).ok())
        .filter(|_| !local.tailscale_ip.is_empty())
        .map(|interface| {
            let rtt_capable = interface != network::ConnectionInterface::Iroh;
            vec![tailsync_core::peer::types::PeerRouteSnapshot {
                interface,
                address: local.tailscale_ip.clone(),
                status: network::PeerStatus::Connected,
                online: true,
                connected: true,
                latency_ms: None,
                pairing_endpoint: false,
                rtt_capable,
            }]
        })
        .unwrap_or_default();

    serde_json::to_value(tailsync_runtime::contracts::PeersResponse {
        local: tailsync_runtime::contracts::LocalDeviceSnapshot {
            hostname: local.hostname,
            tailscale_ip: local.tailscale_ip,
            routes: local_routes,
            iroh_endpoint_id: network::local_iroh_endpoint_id(&mode),
            connection_mode: mode,
            public_key: identity.public_key_base64(),
            fingerprint: identity.fingerprint(),
        },
        peers,
        paired_peer_endpoints: settings.paired_peer_endpoints.clone(),
        discovery_error,
    })
    .expect("typed peer snapshot is JSON safe")
}

fn daemon_status() -> tailsync_runtime::contracts::DaemonStatus {
    tailsync_runtime::contracts::DaemonStatus {
        tcp_server_healthy: network::TCP_SERVER_HEALTHY.load(Ordering::Acquire),
        clipboard_monitor_healthy: crate::clipboard::monitor_is_healthy(),
        clipboard_monitor_failures: crate::clipboard::monitor_failure_count(),
        active_routes: network::active_routes_snapshot(),
    }
}

async fn runtime_snapshot_data(state: &ApiState, since_notification_id: Option<u64>) -> Value {
    // A change during snapshot assembly remains observable: the response keeps
    // the starting revision, so the client's next wait returns immediately.
    let revision = get_runtime_revision();
    let sync_enabled = state.settings.lock().await.sync_enabled;
    let storage = db::storage_status_async(&state.db).await;
    serde_json::to_value(tailsync_runtime::contracts::MacRuntimeSnapshot {
        revision,
        history_version: get_clipboard_version(),
        progress: get_file_progress(),
        storage,
        sync_enabled,
        status: daemon_status(),
        notifications: since_notification_id
            .map(get_runtime_notifications_since)
            .unwrap_or_default(),
    })
    .expect("typed runtime snapshot is JSON safe")
}

pub(super) async fn handle_cmd(req: Request, state: &ApiState) -> Response {
    let command = match LocalCommand::parse(&req.cmd) {
        Some(LocalCommand::Builtin(command)) => command,
        Some(LocalCommand::History(command)) => return history::handle(command, req, state).await,
        Some(LocalCommand::Peers(command)) => return peers::handle(command, req, state).await,
        Some(LocalCommand::Settings(command)) => {
            return settings::handle(command, req, state).await
        }
        Some(LocalCommand::Theme(command)) => return theme::handle(command, req).await,
        Some(LocalCommand::BinaryPreview) => {
            return Response {
                ok: false,
                data: None,
                error: Some("binary preview requires the binary transport".into()),
            }
        }
        None => {
            return Response {
                ok: false,
                data: None,
                error: Some(format!("unknown command: {}", req.cmd)),
            }
        }
    };
    match command {
        BuiltinCommand::Ping => Response {
            ok: true,
            data: None,
            error: None,
        },

        BuiltinCommand::GetLocalCapabilities => Response {
            ok: true,
            data: serde_json::to_value(tailsync_runtime::contracts::LocalCapabilities::current(
                "macos",
                crate::protocol::VERSION,
                true,
                true,
            ))
            .ok(),
            error: None,
        },

        BuiltinCommand::WaitRuntimeSnapshot => {
            let since = req.since_revision.unwrap_or_default();
            let wait_ms = req.wait_ms.unwrap_or(2_500).clamp(50, 15_000);
            let _ = wait_for_runtime_revision(since, Duration::from_millis(wait_ms)).await;
            Response {
                ok: true,
                data: Some(runtime_snapshot_data(state, req.since_notification_id).await),
                error: None,
            }
        }

        BuiltinCommand::CheckForUpdate => {
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

        BuiltinCommand::InstallUpdate => {
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

        BuiltinCommand::GetFileProgress => {
            let info = get_file_progress();
            Response {
                ok: true,
                data: info.and_then(|progress| serde_json::to_value(progress).ok()),
                error: None,
            }
        }

        BuiltinCommand::CancelFileBatch => {
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

        BuiltinCommand::RestoreFileBatch => {
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

        BuiltinCommand::GetStorageStatus => Response {
            ok: true,
            data: serde_json::to_value(db::storage_status_async(&state.db).await).ok(),
            error: None,
        },

        BuiltinCommand::GetVersion => Response {
            ok: true,
            data: Some(serde_json::json!(CLIPBOARD_VERSION.load(Ordering::Acquire))),
            error: None,
        },

        BuiltinCommand::GetSyncWarning => Response {
            ok: true,
            data: serde_json::to_value(tailsync_core::sync_warning::take()).ok(),
            error: None,
        },

        BuiltinCommand::GetHistoryCapabilities => Response {
            ok: true,
            data: Some(history_capabilities_data()),
            error: None,
        },

        BuiltinCommand::GetMigrationDiagnostics => {
            let result = tailsync_runtime::history::HistoryOperations::migration_diagnostics_async(
                state.db.clone(),
                50,
            )
            .await;
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

        BuiltinCommand::GetStatus => Response {
            ok: true,
            data: Some(
                serde_json::to_value(daemon_status()).expect("typed daemon status is JSON safe"),
            ),
            error: None,
        },

        BuiltinCommand::EnablePairing => Response {
            ok: true,
            data: Some(serde_json::to_value(state.pairing.enable().await).unwrap_or_default()),
            error: None,
        },

        BuiltinCommand::GetPairingStatus => Response {
            ok: true,
            data: Some(serde_json::to_value(state.pairing.status().await).unwrap_or_default()),
            error: None,
        },

        BuiltinCommand::StartPairing => {
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

        BuiltinCommand::ConfirmPairing => match state
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

        BuiltinCommand::CancelPairing => Response {
            ok: true,
            data: Some(serde_json::to_value(state.pairing.cancel().await).unwrap_or_default()),
            error: None,
        },

        BuiltinCommand::CreateRemotePairingInvite => match network::create_remote_pairing_invite(
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

        BuiltinCommand::InspectRemotePairingLink => {
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

        BuiltinCommand::StartRemotePairing => {
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

        BuiltinCommand::GetRemotePairingInviteStatus => Response {
            ok: true,
            data: Some(serde_json::to_value(state.remote_invites.status()).unwrap_or_default()),
            error: None,
        },

        BuiltinCommand::CancelRemotePairingInvite => {
            state.remote_invites.cancel();
            Response {
                ok: true,
                data: Some(serde_json::to_value(state.pairing.cancel().await).unwrap_or_default()),
                error: None,
            }
        }

        BuiltinCommand::GetImageData => {
            let Some(id) = req.id else {
                return Response {
                    ok: false,
                    data: None,
                    error: Some("missing id".into()),
                };
            };
            let result =
                tailsync_runtime::history::HistoryOperations::data_async(state.db.clone(), id)
                    .await;
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

        BuiltinCommand::BeginImport => import_response(begin_import(&req, state).await),

        BuiltinCommand::ImportChunk => import_response(append_import_chunk(&req, state).await),

        BuiltinCommand::FinishImport => import_response(finish_import(&req, state).await),

        BuiltinCommand::MigrateEntry => {
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
            let time = time.clone();
            let etype = etype.clone();
            let desc = desc.clone();
            let result = tailsync_runtime::execution::run_db(state.db.clone(), move |database| {
                let result = match etype.as_str() {
                    "text" => database
                        .add_text_migrated(&time, &desc, &data)
                        .map_err(|error| error.to_string()),
                    "image" => database
                        .add_image_migrated(&time, &desc, &data)
                        .map_err(|error| error.to_string()),
                    "file" => database
                        .add_file_migrated(&time, &desc, &data)
                        .map_err(|error| error.to_string()),
                    _ => Err("unknown type".into()),
                };
                if result.is_ok() {
                    crate::api::bump_clipboard_version();
                }
                result
            })
            .await
            .map_err(|error| error.to_string());
            match result {
                Ok(()) => Response {
                    ok: true,
                    data: None,
                    error: None,
                },
                Err(e) => Response {
                    ok: false,
                    data: None,
                    error: Some(e.to_string()),
                },
            }
        }

        BuiltinCommand::Quit => {
            info!("Quit via API");
            Response {
                ok: true,
                data: None,
                error: None,
            }
        }
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
