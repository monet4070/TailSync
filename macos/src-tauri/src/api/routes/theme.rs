use super::*;

pub(super) fn handles(command: &str) -> bool {
    matches!(
        command,
        "list_themes_v2"
            | "get_local_theme_settings"
            | "set_local_theme_settings"
            | "validate_theme"
            | "install_theme"
            | "update_theme"
            | "rollback_theme"
            | "delete_theme_v2"
            | "resolve_theme"
            | "get_theme_asset_slot"
            | "preview_theme_asset_slot"
    )
}

pub(super) async fn handle(req: Request) -> Response {
    match req.cmd.as_str() {
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
        _ => unreachable!("theme command dispatch was checked before routing"),
    }
}

#[cfg(test)]
mod tests {
    use super::handles;

    #[test]
    fn every_theme_route_handler_command_is_dispatchable() {
        for command in [
            "list_themes_v2",
            "get_local_theme_settings",
            "set_local_theme_settings",
            "validate_theme",
            "install_theme",
            "update_theme",
            "rollback_theme",
            "delete_theme_v2",
            "resolve_theme",
            "get_theme_asset_slot",
            "preview_theme_asset_slot",
        ] {
            assert!(
                handles(command),
                "theme command {command} is not dispatchable"
            );
        }
    }
}
