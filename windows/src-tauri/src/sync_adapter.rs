use std::sync::Arc;

use tauri::{AppHandle, Manager};
use tokio::sync::Mutex;

use crate::{api, clipboard_change, clipboard_file, crypto, db};
use tailsync_core::protocol::TransferId;
use tailsync_core::sync::{
    FileBatchProgress, FileReceiveCommit, PlatformResultFuture, SyncPlatform,
};
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
    fn restore_target(&self) -> tailsync_core::composite::restore::RestoreTarget {
        tailsync_core::composite::restore::RestoreTarget::Windows
    }
    fn write_restore_plan(
        &self,
        plan: &tailsync_core::composite::restore::RestorePlan,
        expected: Option<i64>,
        cancelled: Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<tailsync_core::composite::NativePublish, String> {
        if plan.target() != self.restore_target() {
            return Err("Restore plan target mismatch".into());
        }
        crate::clipboard_composite::write_native_cancellable(
            plan.native().ok_or("Native restore output unavailable")?,
            expected,
            Some(cancelled),
        )
    }
    fn clipboard_revision(&self) -> Option<i64> {
        crate::clipboard_composite::revision()
    }
    fn write_native_cancellable(
        &self,
        snapshot: &tailsync_core::protocol::native_clipboard::NativeSnapshot,
        cancelled: Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<tailsync_core::composite::NativePublish, String> {
        crate::clipboard_composite::write_native_cancellable(snapshot, None, Some(cancelled))
    }
    fn write_text_cancellable(
        &self,
        text: &str,
        cancelled: Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<(), String> {
        self.write_text_cancellable_typed(text, cancelled)
            .map_err(String::from)
    }

    fn write_text_cancellable_typed(
        &self,
        text: &str,
        cancelled: Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<(), tailsync_core::sync::WriteFailure> {
        tailsync_core::sync::run_cancellable_write(cancelled, |cancelled| {
            if !crate::clipboard_composite::capture_enabled() {
                return self.write_text_typed(text);
            }
            let bundle = tailsync_core::composite::PortableBundle {
                representations: vec![tailsync_core::composite::Representation {
                    mime: tailsync_core::composite::Mime::Text,
                    bytes: text.as_bytes().to_vec(),
                }],
                resources: vec![],
                bindings: vec![],
                completeness: tailsync_core::composite::Completeness::Complete,
            };
            require_composite_publish_typed(crate::clipboard_composite::write_cancellable(
                &bundle,
                None,
                Some(cancelled),
            )?)
        })
    }
    fn write_image_cancellable(
        &self,
        width: u32,
        height: u32,
        rgba: &[u8],
        cancelled: Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<(), String> {
        self.write_image_cancellable_typed(width, height, rgba, cancelled)
            .map_err(String::from)
    }

    fn write_image_cancellable_typed(
        &self,
        width: u32,
        height: u32,
        rgba: &[u8],
        cancelled: Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<(), tailsync_core::sync::WriteFailure> {
        tailsync_core::sync::run_cancellable_write(cancelled, |cancelled| {
            if !crate::clipboard_composite::capture_enabled() {
                return self.write_image_typed(width, height, rgba);
            }
            let png = tailsync_core::composite::packed_to_png(width, height, rgba)
                .map_err(|e| e.to_string())?;
            let bundle = tailsync_core::composite::PortableBundle {
                representations: vec![tailsync_core::composite::Representation {
                    mime: tailsync_core::composite::Mime::Png,
                    bytes: png,
                }],
                resources: vec![],
                bindings: vec![],
                completeness: tailsync_core::composite::Completeness::Complete,
            };
            require_composite_publish_typed(crate::clipboard_composite::write_cancellable(
                &bundle,
                None,
                Some(cancelled),
            )?)
        })
    }
    fn write_composite_cancellable(
        &self,
        bundle: &tailsync_core::composite::PortableBundle,
        expected: Option<i64>,
        cancelled: Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<tailsync_core::composite::NativePublish, String> {
        crate::clipboard_composite::write_cancellable(bundle, expected, Some(cancelled))
    }
    fn write_composite(
        &self,
        bundle: &tailsync_core::composite::PortableBundle,
        expected: Option<i64>,
    ) -> Result<tailsync_core::composite::NativePublish, String> {
        crate::clipboard_composite::write(bundle, expected)
    }
    fn supports_native_snapshot(
        &self,
        snapshot: &tailsync_core::protocol::native_clipboard::NativeSnapshot,
    ) -> bool {
        crate::clipboard_composite::supports_native(snapshot)
    }
    fn write_native_snapshot(
        &self,
        snapshot: &tailsync_core::protocol::native_clipboard::NativeSnapshot,
    ) -> Result<tailsync_core::composite::NativePublish, String> {
        crate::clipboard_composite::write_native(snapshot, None)
    }
    fn write_text(&self, text: &str) -> Result<(), String> {
        self.write_text_typed(text).map_err(String::from)
    }

    fn write_text_typed(&self, text: &str) -> Result<(), tailsync_core::sync::WriteFailure> {
        if crate::clipboard_composite::capture_enabled() {
            let bundle = tailsync_core::composite::PortableBundle {
                representations: vec![tailsync_core::composite::Representation {
                    mime: tailsync_core::composite::Mime::Text,
                    bytes: text.as_bytes().to_vec(),
                }],
                resources: vec![],
                bindings: vec![],
                completeness: tailsync_core::composite::Completeness::Complete,
            };
            return require_composite_publish_typed(crate::clipboard_composite::write(
                &bundle, None,
            )?);
        }
        self.clipboard()?
            .write_text(text.to_string())
            .map_err(|error| format!("write_text failed: {error}"))?;
        let hash = blake3::hash(text.as_bytes()).to_hex().to_string();
        clipboard_change::record_text_write_receipt(&hash);
        Ok(())
    }

    fn consume_text_write_receipt(&self, hash: &str) -> bool {
        clipboard_change::consume_text_write_receipt(hash)
    }

    fn write_image(&self, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
        self.write_image_typed(width, height, rgba)
            .map_err(String::from)
    }

    fn write_image_typed(
        &self,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> Result<(), tailsync_core::sync::WriteFailure> {
        if crate::clipboard_composite::capture_enabled() {
            let bytes = tailsync_core::composite::packed_to_png(width, height, rgba)
                .map_err(|e| e.to_string())?;
            let bundle = tailsync_core::composite::PortableBundle {
                representations: vec![tailsync_core::composite::Representation {
                    mime: tailsync_core::composite::Mime::Png,
                    bytes,
                }],
                resources: vec![],
                bindings: vec![],
                completeness: tailsync_core::composite::Completeness::Complete,
            };
            return require_composite_publish_typed(crate::clipboard_composite::write(
                &bundle, None,
            )?);
        }
        let image = tauri::image::Image::new(rgba, width, height);
        match self.clipboard()?.write_image(&image) {
            Ok(()) => {
                if let Ok(packed) = tailsync_core::protocol::pack_rgba_image(width, height, rgba) {
                    let hash = blake3::hash(&packed).to_hex().to_string();
                    clipboard_change::record_image_write_receipt(&hash);
                }
                Ok(())
            }
            Err(primary) => {
                // R5-07: this compatibility DIB is a response to an **observed** primary
                // failure, never a speculative "some consumers only know CF_DIB" fix.
                // The system synthesizes CF_DIB from CF_DIBV5 on request (Microsoft,
                // "Synthesized Clipboard Formats"), so the absence of a CF_DIB format is
                // not itself a reason to write one.
                // https://learn.microsoft.com/en-us/windows/win32/dataxchg/clipboard-formats
                #[cfg(target_os = "windows")]
                {
                    clipboard_file::write_clipboard_image(width, height, rgba)
                        .map_err(|fallback| {
                            format!(
                                "write_image failed ({primary}); CF_DIB fallback failed ({fallback})"
                            )
                        })?;
                    if let Ok(packed) =
                        tailsync_core::protocol::pack_rgba_image(width, height, rgba)
                    {
                        let hash = blake3::hash(&packed).to_hex().to_string();
                        clipboard_change::record_image_write_receipt(&hash);
                    }
                    Ok(())
                }
                #[cfg(not(target_os = "windows"))]
                {
                    // A primary write failure that is not a proven no-effect.
                    Err(format!("write_image failed: {primary}").into())
                }
            }
        }
    }

    fn consume_image_write_receipt(&self, hash: &str) -> bool {
        clipboard_change::consume_image_write_receipt(hash)
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

    fn files_received(&self, commit: FileReceiveCommit) -> PlatformResultFuture {
        let FileReceiveCommit {
            batch_id,
            files,
            batch_total,
            batch_complete,
            activate_clipboard,
            device,
            source_device_id,
            manifest_hash,
            clipboard_paths: staged_clipboard_paths,
        } = commit;
        let db = self.db.clone();
        let app = self.app.clone();
        let settings = self.settings.clone();
        let activation_version = api::get_clipboard_version();
        Box::pin(async move {
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
                let mut database = db.blocking_lock();
                let result = if let Some(manifest_hash) = manifest_hash.as_deref() {
                    database.add_file_batch_with_receipt(
                        &history_batch_id,
                        &history_files,
                        db::FileBatchWriteOptions {
                            expected_total: batch_total,
                            source_peer: &db_source_peer,
                            move_sources: true,
                            complete: batch_complete,
                            source_device_id: Some(&source_device_id),
                            manifest_hash: Some(manifest_hash),
                        },
                    )
                } else {
                    database.add_file_batch_with_status(
                        &history_batch_id,
                        &history_files,
                        batch_total,
                        &db_source_peer,
                        true,
                        batch_complete,
                    )
                };
                result.map_err(|error| error.to_string())
            })
            .await
            {
                Ok(Ok(paths)) => paths,
                Ok(Err(error)) => {
                    log::error!("DB save file batch failed: {error}");
                    return Err(error);
                }
                Err(error) => {
                    log::error!("DB file batch task failed: {error}");
                    return Err(error.to_string());
                }
            };

            let _history_version_bump = HistoryVersionBump;
            if activate_clipboard && batch_complete {
                let clipboard_paths = if let Some(paths) = staged_clipboard_paths {
                    if paths.len() != names.len() {
                        return Err("Received clipboard staging has an invalid file count".into());
                    }
                    paths
                } else {
                    let mut paths = Vec::with_capacity(stored_paths.len());
                    for (stored_path, name) in stored_paths.iter().zip(&names) {
                        match db::materialize_remote_clipboard_file(stored_path, name, &device) {
                            Ok(path) => paths.push(path),
                            Err(error) => {
                                log::error!(
                                    "Could not prepare received batch for clipboard: {error}"
                                );
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
                                return Ok(());
                            }
                        }
                    }
                    paths
                };
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
                    return Ok(());
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
            Ok(())
        })
    }

    fn file_batch_failed(&self, _batch_id: Option<TransferId>, message: &str) {
        log::error!("File batch failed: {message}");
        api::push_runtime_notification("error", message);
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

/// Project the five native publish outcomes onto the typed write result the claim
/// and receipt rules need (K2). `Published` succeeded; `NoEffect` is provable and
/// may release an at-most-once claim; `Partial`, `Superseded` and `Unknown` all
/// keep the claim, because the platform may already have been touched.
fn require_composite_publish_typed(
    outcome: tailsync_core::composite::NativePublish,
) -> Result<(), tailsync_core::sync::WriteFailure> {
    use tailsync_core::composite::NativePublish;
    use tailsync_core::sync::WriteFailure;
    match outcome {
        NativePublish::Published(_) => Ok(()),
        NativePublish::NoEffect { detail } => Err(WriteFailure::no_effect(detail)),
        other => Err(WriteFailure::may_have_effect(other.as_write_error())),
    }
}

#[cfg(test)]
mod composite_publish_projection_tests {
    use tailsync_core::composite::NativePublish;
    use tailsync_core::protocol::native_clipboard::{PublishReceipt, PublishState};

    fn receipt(state: PublishState) -> PublishReceipt {
        PublishReceipt {
            state,
            revision: 7,
            token: [3; 16],
        }
    }

    /// F2 — the five native outcomes keep their meaning across the projection onto
    /// the typed write result: only a provable no-effect may release an at-most-once
    /// claim, and `Partial` / `Superseded` / `Unknown` all keep it.
    #[test]
    fn the_five_native_outcomes_keep_their_claim_semantics() {
        assert!(
            super::require_composite_publish_typed(NativePublish::Published(receipt(
                PublishState::Published
            )))
            .is_ok(),
            "a published write is a success"
        );

        let no_effect = super::require_composite_publish_typed(NativePublish::NoEffect {
            detail: "clipboard helper never started".into(),
        })
        .unwrap_err();
        assert!(no_effect.is_proven_no_effect());

        for (label, outcome) in [
            (
                "partial",
                NativePublish::Partial(receipt(PublishState::Partial)),
            ),
            (
                "superseded",
                NativePublish::Superseded(receipt(PublishState::Superseded)),
            ),
            (
                "unknown",
                NativePublish::Unknown {
                    detail: "no-effect: clipboard helper returned no usable receipt".into(),
                },
            ),
        ] {
            let failure = super::require_composite_publish_typed(outcome).unwrap_err();
            assert!(
                !failure.is_proven_no_effect(),
                "{label} may already have touched the platform, so the claim stays"
            );
        }
    }
}
