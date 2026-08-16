use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::{command, AppHandle, Emitter, Manager, State};

pub const PREVIEW_WINDOW_LABEL: &str = "preview";
pub const PREVIEW_REQUEST_EVENT: &str = "tailsync://preview-request";
pub const PREVIEW_CLOSE_EVENT: &str = "tailsync://preview-close";

const MAX_BATCH_ID_CHARS: usize = 256;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PreviewWindowRequest {
    pub entry_id: i64,
    pub batch_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewWindowSnapshot {
    pub revision: u64,
    pub entry_id: i64,
    pub batch_id: Option<String>,
}

#[derive(Default)]
pub struct PreviewWindowController {
    inner: Mutex<PreviewWindowControllerInner>,
}

#[derive(Default)]
struct PreviewWindowControllerInner {
    revision: u64,
    current: Option<PreviewWindowSnapshot>,
    minimized_with_history: bool,
}

impl PreviewWindowRequest {
    fn validate(&self) -> Result<(), String> {
        if self.entry_id <= 0 {
            return Err("preview entry id must be positive".to_string());
        }
        if self
            .batch_id
            .as_ref()
            .is_some_and(|batch_id| batch_id.chars().count() > MAX_BATCH_ID_CHARS)
        {
            return Err("preview batch id is too long".to_string());
        }
        Ok(())
    }
}

impl PreviewWindowController {
    fn replace(&self, request: PreviewWindowRequest) -> PreviewWindowSnapshot {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.revision = inner.revision.saturating_add(1);
        let snapshot = PreviewWindowSnapshot {
            revision: inner.revision,
            entry_id: request.entry_id,
            batch_id: request.batch_id,
        };
        inner.current = Some(snapshot.clone());
        snapshot
    }

    fn current(&self) -> Option<PreviewWindowSnapshot> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .current
            .clone()
    }

    fn set_minimized_with_history(&self, value: bool) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.minimized_with_history = value;
    }

    fn was_minimized_with_history(&self) -> bool {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .minimized_with_history
    }
}

/// Replace the singleton preview window's navigation context.
///
/// The controller stores the request before the window is created so the
/// preview page can always pull the latest state during its first mount. The
/// preview page restores its shared frame while hidden, then shows itself.
#[command]
pub async fn open_preview_window(
    app: AppHandle,
    controller: State<'_, PreviewWindowController>,
    app_state: State<'_, crate::AppState>,
    mut request: PreviewWindowRequest,
) -> Result<PreviewWindowSnapshot, String> {
    request.validate()?;
    if let Some(batch_id) = request.batch_id.as_deref() {
        let navigation = app_state
            .db
            .lock()
            .await
            .get_preview_batch_navigation(batch_id, request.entry_id)
            .map_err(|error| error.to_string())?;
        request.entry_id = navigation.first_entry_id;
    }
    let snapshot = controller.replace(request);

    if let Some(window) = app.get_webview_window(PREVIEW_WINDOW_LABEL) {
        window
            .emit(PREVIEW_REQUEST_EVENT, &snapshot)
            .map_err(|error| error.to_string())?;
        return Ok(snapshot);
    }

    tauri::WebviewWindowBuilder::new(
        &app,
        PREVIEW_WINDOW_LABEL,
        tauri::WebviewUrl::App("preview.html".into()),
    )
    .title("TailSync - Preview")
    .inner_size(900.0, 680.0)
    .min_inner_size(520.0, 360.0)
    .decorations(false)
    .resizable(true)
    .visible(false)
    .center()
    .build()
    .map_err(|error| error.to_string())?;

    Ok(snapshot)
}

/// Return the latest preview request. This closes the create/listen race for
/// the first render of a newly-created WebView.
#[command]
pub fn get_preview_window_request(
    controller: State<'_, PreviewWindowController>,
) -> Option<PreviewWindowSnapshot> {
    controller.current()
}

/// Hide the reusable preview window without discarding its WebView. Blob URLs
/// and decrypted buffers are released by the preview page before this command
/// is invoked.
#[command]
pub fn close_preview_window(app: AppHandle) -> Result<(), String> {
    if let Some(controller) = app.try_state::<PreviewWindowController>() {
        controller.set_minimized_with_history(false);
    }
    if let Some(window) = app.get_webview_window(PREVIEW_WINDOW_LABEL) {
        window
            .emit(PREVIEW_CLOSE_EVENT, ())
            .map_err(|error| error.to_string())?;
        window.hide().map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Keep the reusable preview window paired with the history window's
/// minimized state. The marker only records a minimize initiated by this
/// function, so manually minimizing the preview is never undone later.
#[command]
pub fn sync_preview_window_minimized(
    app: AppHandle,
    controller: State<'_, PreviewWindowController>,
    minimized: bool,
) -> Result<(), String> {
    let Some(window) = app.get_webview_window(PREVIEW_WINDOW_LABEL) else {
        return Ok(());
    };
    if minimized {
        if window.is_visible().map_err(|error| error.to_string())?
            && !window.is_minimized().map_err(|error| error.to_string())?
        {
            window.minimize().map_err(|error| error.to_string())?;
            controller.set_minimized_with_history(true);
        }
    } else if controller.was_minimized_with_history() {
        window.unminimize().map_err(|error| error.to_string())?;
        controller.set_minimized_with_history(false);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(id: i64) -> PreviewWindowRequest {
        PreviewWindowRequest {
            entry_id: id,
            batch_id: None,
        }
    }

    #[test]
    fn preview_request_validation_rejects_invalid_navigation() {
        assert!(request(0).validate().is_err());
        let mut oversized_batch = request(1);
        oversized_batch.batch_id = Some("x".repeat(MAX_BATCH_ID_CHARS + 1));
        assert!(oversized_batch.validate().is_err());
    }

    #[test]
    fn controller_revisions_make_initial_pull_and_events_orderable() {
        let controller = PreviewWindowController::default();
        let first = controller.replace(request(1));
        let second = controller.replace(request(2));

        assert_eq!(first.revision, 1);
        assert_eq!(second.revision, 2);
        assert_eq!(controller.current(), Some(second));
    }

    #[test]
    fn controller_tracks_only_history_owned_minimization() {
        let controller = PreviewWindowController::default();
        assert!(!controller.was_minimized_with_history());
        controller.set_minimized_with_history(true);
        assert!(controller.was_minimized_with_history());
        controller.set_minimized_with_history(false);
        assert!(!controller.was_minimized_with_history());
    }
}
