use super::*;

pub(super) fn handles(command: &str) -> bool {
    matches!(
        command,
        "get_peers"
            | "refresh_peers"
            | "toggle_peer"
            | "trust_peer"
            | "forget_peer"
            | "test_connection"
            | "reconnect_peers"
    )
}

pub(super) async fn handle(req: Request, state: &ApiState) -> Response {
    match req.cmd.as_str() {
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

        _ => unreachable!("peers command dispatch was checked before routing"),
    }
}
