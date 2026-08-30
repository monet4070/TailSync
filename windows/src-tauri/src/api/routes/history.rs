use super::*;

pub(super) fn handles(command: &str) -> bool {
    matches!(
        command,
        "get_history"
            | "delete_entry"
            | "set_history_favorite"
            | "delete_favorite_entry"
            | "clear_history"
            | "clear_all"
            | "restore_entry"
    )
}

pub(super) async fn handle(req: Request, state: &ApiState) -> Response {
    match req.cmd.as_str() {
        "get_history" => {
            let collection = match db::HistoryCollection::from_wire(req.collection.as_deref()) {
                Ok(collection) => collection,
                Err(error) => {
                    return Response {
                        ok: false,
                        data: None,
                        error: Some(error.to_string()),
                    };
                }
            };
            let db = state.db.lock().await;
            // Consume before await for Send safety
            let result = db
                .get_page_in_collection(db::HistoryQuery {
                    collection,
                    keyword: req.keyword.as_deref(),
                    category: req.category.as_deref(),
                    start_time: req.start_time.as_deref(),
                    end_time: req.end_time.as_deref(),
                    limit: req.limit.unwrap_or(30),
                    offset: req.offset.unwrap_or(0),
                })
                .map(|page| page.entries)
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

        "set_history_favorite" => {
            let Some(id) = req.id else {
                return Response {
                    ok: false,
                    data: None,
                    error: Some("missing id".into()),
                };
            };
            let mut db = state.db.lock().await;
            match db.set_favorite(id, req.favorite.unwrap_or(true)) {
                Ok(mutation) => {
                    bump_clipboard_version();
                    Response {
                        ok: true,
                        data: serde_json::to_value(mutation).ok(),
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

        "delete_favorite_entry" => {
            let Some(id) = req.id else {
                return Response {
                    ok: false,
                    data: None,
                    error: Some("missing id".into()),
                };
            };
            let mut db = state.db.lock().await;
            match db.delete_favorite(id) {
                Ok(mutation) => {
                    bump_clipboard_version();
                    Response {
                        ok: true,
                        data: serde_json::to_value(mutation).ok(),
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

        _ => unreachable!("history command dispatch was checked before routing"),
    }
}

#[cfg(test)]
mod tests {
    use super::handles;

    #[test]
    fn every_history_handler_command_is_dispatchable() {
        for command in [
            "get_history",
            "delete_entry",
            "set_history_favorite",
            "delete_favorite_entry",
            "clear_history",
            "clear_all",
            "restore_entry",
        ] {
            assert!(
                handles(command),
                "history route does not dispatch {command}"
            );
        }
    }
}
