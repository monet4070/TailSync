use super::*;
use tailsync_runtime::history::{HistoryOperations, HistoryPageRequestOwned};

use super::registry::HistoryCommand;

struct CancelOnDrop(tailsync_core::cancellation::Cancellation);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

pub(super) async fn handle(command: HistoryCommand, req: Request, state: &ApiState) -> Response {
    match command {
        HistoryCommand::GetHistory | HistoryCommand::GetHistoryPage => {
            let paged = matches!(command, HistoryCommand::GetHistoryPage);
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
            let result = tailsync_runtime::history_display::page_display_async(
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
                crate::clipboard_composite::native_analyzer(),
            )
            .await;
            match result {
                Ok(page) if !paged && page.display_index_pending == Some(true) => Response {
                    ok: false,
                    data: None,
                    error: Some("content analysis busy".into()),
                },
                Ok(page) => Response {
                    ok: true,
                    data: Some(if paged {
                        serde_json::to_value(page).unwrap_or_default()
                    } else {
                        serde_json::to_value(page.entries).unwrap_or_default()
                    }),
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
            let image_archive = state.image_archive.clone();
            match HistoryOperations::delete_async_with_hook(state.db.clone(), id, move || {
                image_archive.cancel_entry(id);
                bump_clipboard_version();
            })
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
            let image_archive = state.image_archive.clone();
            match HistoryOperations::delete_favorite_async_with_hook(
                state.db.clone(),
                id,
                move || {
                    image_archive.cancel_entry(id);
                    bump_clipboard_version();
                },
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
            let image_archive = state.image_archive.clone();
            match HistoryOperations::clear_async_with_hook(state.db.clone(), move || {
                image_archive.cancel_pending();
                bump_clipboard_version();
            })
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
            if payload.entry_type == "composite" {
                let result = payload
                    .restore_composite_async(state.sync_engine.clone(), req.restore_format.clone())
                    .await;
                // R5-03: only `Published` is a success. An unsupported or partial
                // restore carries a stable code the UI localises, instead of a
                // generic success the user would read as "copied".
                return match result {
                    Ok(outcome) => {
                        if outcome.is_published() {
                            super::super::bump_clipboard_version();
                        }
                        Response {
                            ok: outcome.is_published(),
                            data: outcome.report().map(|report| serde_json::json!(report)),
                            error: (!outcome.is_published()).then(|| outcome.code().to_string()),
                        }
                    }
                    Err(error) => Response {
                        ok: false,
                        data: None,
                        error: Some(error),
                    },
                };
            }
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
                if let Err(error) =
                    sync::SyncEngine::restore_image_shared(state.sync_engine.clone(), data.to_vec())
                        .await
                {
                    return Response {
                        ok: false,
                        data: None,
                        error: Some(error),
                    };
                }
            } else if entry_type == "file" {
                state.sync_engine.lock().await.supersede_file_clipboard();
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
                if let Err(error) =
                    sync::SyncEngine::restore_text_shared(state.sync_engine.clone(), text).await
                {
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
        HistoryCommand::ArchiveEntryImages => {
            let Some(id) = req.id else {
                return Response {
                    ok: false,
                    data: None,
                    error: Some("missing id".into()),
                };
            };
            let cancellation = tailsync_core::cancellation::Cancellation::default();
            let _cancel_on_drop = CancelOnDrop(cancellation.clone());
            let transport: std::sync::Arc<
                dyn tailsync_core::composite::resource_archive::ResourceTransport,
            > = std::sync::Arc::new(
                tailsync_core::composite::resource_archive::ReqwestTransport::new(),
            );
            let image_archive = state.image_archive.clone();
            let result = HistoryOperations::archive_entry_images_with_commit_hook(
                state.db.clone(),
                id,
                transport,
                cancellation,
                move |commit| image_archive.committed(commit),
            )
            .await;
            match result {
                Ok(report) => Response {
                    ok: true,
                    data: Some(serde_json::json!({
                        "archived": report.archived,
                        "deduplicated": report.deduplicated,
                        "complete": report.complete,
                        "failures": report.failures,
                    })),
                    error: None,
                },
                Err(error) => Response {
                    ok: false,
                    data: None,
                    error: Some(error),
                },
            }
        }
        HistoryCommand::SourceContextGrantCreate => {
            let Some(id) = req.id else {
                return Response {
                    ok: false,
                    data: None,
                    error: Some("missing id".into()),
                };
            };
            match tailsync_runtime::source_context::create_grant(
                state.db.clone(),
                tailsync_runtime::source_context::default_registry(),
                id,
            )
            .await
            {
                Ok(report) => Response {
                    ok: true,
                    data: Some(serde_json::json!(report)),
                    error: None,
                },
                Err(error) => Response {
                    ok: false,
                    data: None,
                    error: Some(error),
                },
            }
        }
        HistoryCommand::SourceContextGrantRevoke => {
            let Some(grant_id) = req.grant_id else {
                return Response {
                    ok: false,
                    data: None,
                    error: Some("missing grant_id".into()),
                };
            };
            let report = tailsync_runtime::source_context::revoke_grant(
                tailsync_runtime::source_context::default_registry(),
                &grant_id,
            );
            Response {
                ok: true,
                data: Some(serde_json::json!(report)),
                error: None,
            }
        }
        HistoryCommand::SourceContextImport => {
            let Some(id) = req.id else {
                return Response {
                    ok: false,
                    data: None,
                    error: Some("missing id".into()),
                };
            };
            let Some(path) = req.path else {
                return Response {
                    ok: false,
                    data: None,
                    error: Some("missing path".into()),
                };
            };
            let cancellation = tailsync_core::cancellation::Cancellation::default();
            let _cancel_on_drop = CancelOnDrop(cancellation.clone());
            match tailsync_runtime::source_context::import_packet_checked(
                state.db.clone(),
                tailsync_runtime::source_context::default_registry(),
                path,
                Some(id),
                cancellation,
            )
            .await
            {
                Ok(report) => Response {
                    ok: true,
                    data: Some(serde_json::json!(report)),
                    error: None,
                },
                Err(error) => Response {
                    ok: false,
                    data: None,
                    error: Some(error),
                },
            }
        }
    }
}
