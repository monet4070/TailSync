use std::sync::Arc;

use tauri::{AppHandle, Manager};
use tokio::sync::Mutex;

use crate::{api, db};
use tailsync_core::sync::{ReceivedFile, SyncPlatform};

pub struct TauriSyncPlatform {
    app: AppHandle,
    db: Arc<Mutex<db::HistoryDB>>,
}

impl TauriSyncPlatform {
    pub fn new(app: AppHandle, db: Arc<Mutex<db::HistoryDB>>) -> Self {
        Self { app, db }
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
        self.clipboard()?
            .write_image(&image)
            .map_err(|error| format!("write_image failed: {error}"))
    }

    fn set_file_progress(&self, name: &str, received: u64, total: u64) {
        api::set_file_progress(name, received, total);
    }

    fn clear_file_progress(&self) {
        api::clear_file_progress();
    }

    fn file_received(&self, file: ReceivedFile) {
        let db = self.db.clone();
        tauri::async_runtime::spawn(async move {
            let ReceivedFile {
                name,
                size,
                hash,
                path,
            } = file;
            let db_name = name.clone();
            let db_path = path.clone();
            let stored_path = match tokio::task::spawn_blocking(move || {
                db.blocking_lock()
                    .adopt_file(&db_name, &db_path, &hash, size, "peer")
                    .map_err(|error| error.to_string())
            })
            .await
            {
                Ok(Ok(path)) => Some(path),
                Ok(Err(error)) => {
                    log::error!("DB save file failed: {error}");
                    None
                }
                Err(error) => {
                    log::error!("DB file task failed: {error}");
                    None
                }
            };

            api::bump_clipboard_version();
            api::restore_file_path_to_clipboard(stored_path.as_deref().unwrap_or(&path), &name);
        });
    }
}
