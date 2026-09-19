use super::*;
use tailsync_runtime::history::{HistoryOperations, HistoryPageRequestOwned};

use super::registry::HistoryCommand;

pub(super) async fn handle(command: HistoryCommand, req: Request, state: &ApiState) -> Response {
    match command {
        HistoryCommand::GetHistory => {
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
            let result = HistoryOperations::entries_async(
                state.db.clone(),
                HistoryPageRequestOwned::new(
                    collection,
                    req.keyword,
                    req.category,
                    req.start_time,
                    req.end_time,
                    req.limit,
                    req.offset,
                    30,
                ),
            )
            .await;
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

        HistoryCommand::DeleteEntry => {
            let Some(id) = req.id else {
                return Response {
                    ok: false,
                    data: None,
                    error: Some("missing id".into()),
                };
            };
            match HistoryOperations::delete_async_with_hook(
                state.db.clone(),
                id,
                bump_clipboard_version,
            )
            .await
            {
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

        HistoryCommand::SetHistoryFavorite => {
            let Some(id) = req.id else {
                return Response {
                    ok: false,
                    data: None,
                    error: Some("missing id".into()),
                };
            };
            match HistoryOperations::set_favorite_async_with_hook(
                state.db.clone(),
                id,
                req.favorite.unwrap_or(true),
                bump_clipboard_version,
            )
            .await
            {
                Ok(mutation) => Response {
                    ok: true,
                    data: serde_json::to_value(mutation).ok(),
                    error: None,
                },
                Err(error) => Response {
                    ok: false,
                    data: None,
                    error: Some(error.to_string()),
                },
            }
        }

        HistoryCommand::DeleteFavoriteEntry => {
            let Some(id) = req.id else {
                return Response {
                    ok: false,
                    data: None,
                    error: Some("missing id".into()),
                };
            };
            match HistoryOperations::delete_favorite_async_with_hook(
                state.db.clone(),
                id,
                bump_clipboard_version,
            )
            .await
            {
                Ok(mutation) => Response {
                    ok: true,
                    data: serde_json::to_value(mutation).ok(),
                    error: None,
                },
                Err(error) => Response {
                    ok: false,
                    data: None,
                    error: Some(error.to_string()),
                },
            }
        }

        HistoryCommand::ClearHistory | HistoryCommand::ClearAll => {
            match HistoryOperations::clear_async_with_hook(state.db.clone(), bump_clipboard_version)
                .await
            {
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

        HistoryCommand::RestoreEntry => {
            let Some(id) = req.id else {
                return Response {
                    ok: false,
                    data: None,
                    error: Some("missing id".into()),
                };
            };
            let payload = HistoryOperations::restore_payload_async(state.db.clone(), id).await;
            let payload = match payload {
                Ok(payload) => payload,
                Err(error) => {
                    return Response {
                        ok: false,
                        data: None,
                        error: Some(error),
                    };
                }
            };
            let entry_type = payload.entry_type;
            let file_path = payload.file_path;
            let file_name = payload.file_name.unwrap_or_else(|| "restored_file".into());
            let data = payload.data;

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
                let text = String::from_utf8_lossy(data.as_deref().unwrap_or_default()).to_string();
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
    }
}
