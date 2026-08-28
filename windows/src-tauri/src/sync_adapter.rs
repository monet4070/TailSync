use std::sync::Arc;

use tauri::{AppHandle, Manager};
use tokio::sync::Mutex;

use crate::{api, clipboard_file, crypto, db};
use tailsync_core::protocol::TransferId;
use tailsync_core::sync::{FileBatchProgress, ReceivedFile, SyncPlatform};
use tauri_plugin_notification::NotificationExt;

pub struct TauriSyncPlatform {
    app: AppHandle,
    db: Arc<Mutex<db::HistoryDB>>,
    settings: Arc<Mutex<crypto::Settings>>,
}

struct FileProgressCleanup {
    batch_id: Option<String>,
    device: String,
}

struct HistoryVersionBump;

impl Drop for HistoryVersionBump {
    fn drop(&mut self) {
        api::bump_clipboard_version();
    }
}

impl Drop for FileProgressCleanup {
    fn drop(&mut self) {
        if let Some(batch_id) = self.batch_id.as_deref() {
            api::clear_file_progress_scope(Some(batch_id), Some(&self.device));
        } else {
            api::clear_file_progress();
        }
    }
}

impl TauriSyncPlatform {
    pub fn new(
        app: AppHandle,
        db: Arc<Mutex<db::HistoryDB>>,
        settings: Arc<Mutex<crypto::Settings>>,
    ) -> Self {
        Self { app, db, settings }
    }

    fn clipboard(
        &self,
    ) -> Result<tauri::State<'_, tauri_plugin_clipboard_manager::Clipboard<tauri::Wry>>, String>
    {
        self.app
            .try_state::<tauri_plugin_clipboard_manager::Clipboard<tauri::Wry>>()
            .ok_or_else(|| "Clipboard plugin state is unavailable".to_string())
    }
}

impl SyncPlatform for TauriSyncPlatform {
    fn write_text(&self, text: &str) -> Result<(), String> {
        self.clipboard()?
            .write_text(text.to_string())
            .map_err(|error| format!("write_text failed: {error}"))
    }

    fn write_image(&self, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
        let image = tauri::image::Image::new(rgba, width, height);
        match self.clipboard()?.write_image(&image) {
            Ok(()) => Ok(()),
            Err(primary) => {
                #[cfg(target_os = "windows")]
                {
                    clipboard_file::write_clipboard_image(width, height, rgba).map_err(|fallback| {
                        format!(
                            "write_image failed ({primary}); CF_DIB fallback failed ({fallback})"
                        )
                    })
                }
                #[cfg(not(target_os = "windows"))]
                {
                    Err(format!("write_image failed: {primary}"))
                }
            }
        }
    }

    fn set_file_progress(&self, name: &str, received: u64, total: u64) {
        api::set_file_progress(name, received, total);
    }

    fn clear_file_progress(&self, batch_id: Option<TransferId>, device: Option<&str>) {
        let batch_id = batch_id.map(TransferId::as_hex);
        api::clear_file_progress_scope(batch_id.as_deref(), device);
    }

    fn set_file_batch_progress(&self, progress: FileBatchProgress) {
        api::set_file_batch_progress(api::FileProgress {
            batch_id: progress.batch_id,
            name: progress.current_file,
            sent: progress.transferred_bytes,
            total: progress.total_bytes,
            active: true,
            direction: progress.direction,
            device: progress.device,
            completed_files: progress.completed_files,
            total_files: progress.total_files,
            speed_bytes_per_second: 0,
            status: "transferring".into(),
            can_stop: true,
        });
    }

    fn files_received(
        &self,
        batch_id: Option<TransferId>,
        files: Vec<ReceivedFile>,
        batch_total: usize,
        batch_complete: bool,
        activate_clipboard: bool,
        device: String,
    ) {
        let db = self.db.clone();
        let app = self.app.clone();
        let settings = self.settings.clone();
        let activation_version = api::get_clipboard_version();
        tauri::async_runtime::spawn(async move {
            let _progress_cleanup = FileProgressCleanup {
                batch_id: batch_id.map(TransferId::as_hex),
                device: device.clone(),
            };
            let notifications_enabled = settings.lock().await.notifications_enabled;
            let history_files = files
                .iter()
                .map(|file| db::HistoryFileInput {
                    name: file.name.clone(),
                    path: file.path.clone(),
                    data_hash: file.hash.clone(),
                    size: file.size,
                })
                .collect::<Vec<_>>();
            let names = files
                .iter()
                .map(|file| file.name.clone())
                .collect::<Vec<_>>();
            let history_batch_id = batch_id.unwrap_or_else(TransferId::random).as_hex();
            let db_source_peer = device.clone();
            let stored_paths = match tokio::task::spawn_blocking(move || {
                db.blocking_lock()
                    .add_file_batch_with_status(
                        &history_batch_id,
                        &history_files,
                        batch_total,
                        &db_source_peer,
                        true,
                        batch_complete,
                    )
                    .map_err(|error| error.to_string())
            })
            .await
            {
                Ok(Ok(paths)) => paths,
                Ok(Err(error)) => {
                    log::error!("DB save file batch failed: {error}");
                    if notifications_enabled {
                        let _ = app
                            .notification()
                            .builder()
                            .title("TailSync")
                            .body(format!("File batch failed: {error}"))
                            .show();
                    }
                    return;
                }
                Err(error) => {
                    log::error!("DB file batch task failed: {error}");
                    return;
                }
            };

            let _history_version_bump = HistoryVersionBump;
            if activate_clipboard && batch_complete {
                let mut clipboard_paths = Vec::with_capacity(stored_paths.len());
                for (stored_path, name) in stored_paths.iter().zip(&names) {
                    match db::materialize_remote_clipboard_file(stored_path, name, &device) {
                        Ok(path) => clipboard_paths.push(path),
                        Err(error) => {
                            log::error!("Could not prepare received batch for clipboard: {error}");
                            if notifications_enabled {
                                let _ = app
                                    .notification()
                                    .builder()
                                    .title("TailSync")
                                    .body(format!(
                                        "Could not place received files on the clipboard: {error}"
                                    ))
                                    .show();
                            }
                            return;
                        }
                    }
                }
                if api::get_clipboard_version() != activation_version {
                    log::info!("Received file batch was superseded before clipboard activation");
                } else if let Err(error) = clipboard_file::write_clipboard_files(&clipboard_paths) {
                    log::error!("Could not restore file batch clipboard: {error}");
                    if notifications_enabled {
                        let _ = app
                            .notification()
                            .builder()
                            .title("TailSync")
                            .body(format!("Could not update the clipboard: {error}"))
                            .show();
                    }
                    return;
                }
            }
            if batch_complete && notifications_enabled {
                let body = format!("Received {} file(s)", names.len());
                let _ = app
                    .notification()
                    .builder()
                    .title("TailSync")
                    .body(body)
                    .show();
            }
        });
    }

    fn file_batch_failed(&self, _batch_id: Option<TransferId>, message: &str) {
        log::error!("File batch failed: {message}");
        let app = self.app.clone();
        let settings = self.settings.clone();
        let message = message.to_string();
        tauri::async_runtime::spawn(async move {
            if settings.lock().await.notifications_enabled {
                let _ = app
                    .notification()
                    .builder()
                    .title("TailSync")
                    .body(message)
                    .show();
            }
        });
    }
}
