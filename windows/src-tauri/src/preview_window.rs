use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::{command, AppHandle, Emitter, Manager, State};

pub const PREVIEW_WINDOW_LABEL: &str = "preview";
pub const PREVIEW_REQUEST_EVENT: &str = "tailsync://preview-request";
pub const PREVIEW_CLOSE_EVENT: &str = "tailsync://preview-close";

const MAX_BATCH_ID_CHARS: usize = 256;

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewWindowOwner {
    #[default]
    History,
    Favorites,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PreviewWindowRequest {
    pub entry_id: i64,
    pub batch_id: Option<String>,
    #[serde(default)]
    pub owner: PreviewWindowOwner,
    #[serde(default = "default_take_focus")]
    pub take_focus: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewWindowSnapshot {
    pub revision: u64,
    pub entry_id: i64,
    pub batch_id: Option<String>,
    pub owner: PreviewWindowOwner,
    pub take_focus: bool,
}

#[derive(Default)]
pub struct PreviewWindowController {
    inner: Mutex<PreviewWindowControllerInner>,
}

#[derive(Default)]
struct PreviewWindowControllerInner {
    revision: u64,
    current: Option<PreviewWindowSnapshot>,
    minimized_with_owner: bool,
    pending_owner: Option<PreviewWindowOwner>,
    restore_epoch: u64,
}

impl PreviewWindowControllerInner {
    fn accepts_revision(&self, revision: u64) -> bool {
        self.revision == revision
    }

    fn invalidate(
        &mut self,
        owner: Option<PreviewWindowOwner>,
    ) -> Option<(u64, Option<PreviewWindowOwner>)> {
        if owner.is_some_and(|owner| self.pending_owner != Some(owner)) {
            return None;
        }
        let previous_owner = self.pending_owner;
        self.revision = self.revision.saturating_add(1);
        self.current = None;
        self.pending_owner = None;
        self.minimized_with_owner = false;
        Some((self.revision, previous_owner))
    }
}

fn default_take_focus() -> bool {
    true
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
    /// Preserve the displayed target on failure/Ctrl+Enter, but tombstone every
    /// pending navigation and renderer presentation from before the restore.
    pub(crate) fn freeze_for_restore(&self) -> u64 {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.revision = inner.revision.saturating_add(1);
        inner.revision
    }

    #[cfg(test)]
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
            owner: request.owner,
            take_focus: request.take_focus,
        };
        inner.minimized_with_owner = false;
        inner.pending_owner = Some(snapshot.owner);
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

    fn owns(&self, owner: PreviewWindowOwner) -> bool {
        let inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.pending_owner == Some(owner)
    }

    fn reserve(&self, owner: PreviewWindowOwner, restore_epoch: u64) -> u64 {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.revision = inner.revision.saturating_add(1);
        inner.pending_owner = Some(owner);
        inner.restore_epoch = restore_epoch;
        inner.revision
    }

    fn set_minimized_with_owner(&self, value: bool) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.minimized_with_owner = value;
    }

    fn was_minimized_with_owner(&self) -> bool {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .minimized_with_owner
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
) -> Result<PreviewWindowSnapshot, crate::commands::CommandError> {
    request.validate()?;
    let revision = {
        let restore = crate::commands::restore_coordination::state();
        if restore.active {
            return Err("A clipboard restore is already in progress".into());
        }
        controller.reserve(request.owner, restore.epoch)
    };
    if let Some(batch_id) = request.batch_id.as_deref() {
        let navigation = app_state
            .db
            .lock()
            .await
            .get_preview_batch_navigation(batch_id, request.entry_id)
            .map_err(|error| error.to_string())?;
        request.entry_id = navigation.first_entry_id;
    }
    let (send, receive) = tokio::sync::oneshot::channel();
    let ui_app = app.clone();
    app.run_on_main_thread(move || {
        let result = finish_open_preview(ui_app, request, revision);
        let _ = send.send(result);
    })
    .map_err(|error| error.to_string())?;
    receive.await.map_err(|error| error.to_string())?
}

fn finish_open_preview(
    app: AppHandle,
    request: PreviewWindowRequest,
    revision: u64,
) -> Result<PreviewWindowSnapshot, crate::commands::CommandError> {
    // UI work is serialized on the main thread with close/presentation.
    // No worker holds this lock while waiting for main-thread window creation.
    let controller = app.state::<PreviewWindowController>();
    let restore = crate::commands::restore_coordination::state();
    let mut inner = controller
        .inner
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !restore.accepts_preview(inner.restore_epoch) || !inner.accepts_revision(revision) {
        return Err("Preview request was superseded".to_string().into());
    }
    let snapshot = PreviewWindowSnapshot {
        revision,
        entry_id: request.entry_id,
        batch_id: request.batch_id,
        owner: request.owner,
        take_focus: request.take_focus,
    };
    inner.current = Some(snapshot.clone());
    inner.minimized_with_owner = false;
    crate::window_lifecycle::mark_window_open(&app, PREVIEW_WINDOW_LABEL);

    if let Some(window) = app.get_webview_window(PREVIEW_WINDOW_LABEL) {
        window
            .emit(PREVIEW_REQUEST_EVENT, &snapshot)
            .map_err(|error| error.to_string())?;
    } else {
        crate::window_lifecycle::configure_transparent_window(tauri::WebviewWindowBuilder::new(
            &app,
            PREVIEW_WINDOW_LABEL,
            tauri::WebviewUrl::App("preview.html".into()),
        ))
        .title("TailSync - Preview")
        .inner_size(900.0, 680.0)
        .min_inner_size(520.0, 360.0)
        .decorations(false)
        .shadow(false)
        .resizable(true)
        .visible(false)
        .focused(false)
        .center()
        .build()
        .map_err(|error| error.to_string())?;
    }

    Ok(snapshot)
}

impl PreviewWindowOwner {
    fn label(self) -> &'static str {
        match self {
            Self::History => "history",
            Self::Favorites => "favorites",
        }
    }
}

/// The renderer calls this only after frame restoration. Revision validation
/// and native show share the close lock, so a late renderer cannot reopen it.
fn present_preview_window(
    app: AppHandle,
    controller: State<'_, PreviewWindowController>,
    revision: u64,
) -> Result<(), crate::commands::CommandError> {
    let restore = crate::commands::restore_coordination::state();
    let inner = controller
        .inner
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !restore.accepts_preview(inner.restore_epoch) {
        return Ok(());
    }
    let Some(snapshot) = inner
        .current
        .as_ref()
        .filter(|snapshot| snapshot.revision == revision && inner.accepts_revision(revision))
    else {
        return Ok(());
    };
    if let Some(window) = app.get_webview_window(PREVIEW_WINDOW_LABEL) {
        if let Some(host) = app.get_webview_window(snapshot.owner.label()) {
            window
                .set_always_on_top(host.is_always_on_top().map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?;
        }
        if snapshot.take_focus {
            crate::window_lifecycle::restore_and_focus_window(&window)?;
        } else {
            crate::window_lifecycle::show_without_activation(&window)?;
        }
    }
    Ok(())
}

/// Return the latest preview request. This closes the create/listen race for
/// the first render of a newly-created WebView.
#[command]
pub fn get_preview_window_request(
    app: AppHandle,
    controller: State<'_, PreviewWindowController>,
    ready_revision: Option<u64>,
) -> Result<Option<PreviewWindowSnapshot>, crate::commands::CommandError> {
    if let Some(revision) = ready_revision {
        present_preview_window(app, controller.clone(), revision)?;
    }
    Ok(controller.current())
}

/// Hide the reusable preview immediately, then destroy its WebView after a
/// short idle grace period. Reopening during the grace period cancels release.
#[command]
pub fn close_preview_window(
    app: AppHandle,
    controller: State<'_, PreviewWindowController>,
    owner: Option<PreviewWindowOwner>,
) -> Result<(), crate::commands::CommandError> {
    let mut inner = controller
        .inner
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some((revision, previous_owner)) = inner.invalidate(owner) else {
        return Ok(());
    };
    let notice = serde_json::json!({ "revision": revision, "owner": previous_owner });
    if let Some(window) = app.get_webview_window(PREVIEW_WINDOW_LABEL) {
        if window.is_focused().unwrap_or(false) {
            if let Some(host) =
                previous_owner.and_then(|owner| app.get_webview_window(owner.label()))
            {
                let _ = host.set_focus(); // Esc returns to host, never the external app.
            }
        }
        let _ = window.emit(PREVIEW_CLOSE_EVENT, &notice);
    }
    if let Some(owner) = previous_owner {
        let _ = app.emit_to(owner.label(), PREVIEW_CLOSE_EVENT, &notice);
    }
    crate::window_lifecycle::hide_then_release_window(app, PREVIEW_WINDOW_LABEL).map_err(Into::into)
}

/// Keep the reusable preview paired only with the active source window's
/// minimized state. The marker records only a minimize initiated here, so a
/// manually minimized preview is never undone later.
#[command]
pub fn sync_preview_window_minimized(
    app: AppHandle,
    controller: State<'_, PreviewWindowController>,
    owner: PreviewWindowOwner,
    minimized: bool,
) -> Result<(), crate::commands::CommandError> {
    if !controller.owns(owner) {
        return Ok(());
    }
    let Some(window) = app.get_webview_window(PREVIEW_WINDOW_LABEL) else {
        return Ok(());
    };
    if let Some(host) = app.get_webview_window(owner.label()) {
        window
            .set_always_on_top(host.is_always_on_top().map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    }
    if minimized {
        if window.is_visible().map_err(|error| error.to_string())?
            && !window.is_minimized().map_err(|error| error.to_string())?
        {
            window.minimize().map_err(|error| error.to_string())?;
            controller.set_minimized_with_owner(true);
        }
    } else if controller.was_minimized_with_owner() {
        window.unminimize().map_err(|error| error.to_string())?;
        controller.set_minimized_with_owner(false);
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
            owner: PreviewWindowOwner::History,
            take_focus: false,
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
    fn controller_tracks_preview_owner_and_owned_minimization() {
        let controller = PreviewWindowController::default();
        assert!(!controller.was_minimized_with_owner());
        controller.replace(request(1));
        assert!(controller.owns(PreviewWindowOwner::History));
        assert!(!controller.owns(PreviewWindowOwner::Favorites));

        let mut favorites = request(2);
        favorites.owner = PreviewWindowOwner::Favorites;
        controller.replace(favorites);
        assert!(controller.owns(PreviewWindowOwner::Favorites));
        assert!(!controller.owns(PreviewWindowOwner::History));

        controller.set_minimized_with_owner(true);
        assert!(controller.was_minimized_with_owner());
        controller.set_minimized_with_owner(false);
        assert!(!controller.was_minimized_with_owner());
    }
    #[test]
    fn closing_invalidates_pending_navigation_and_late_presentation() {
        let controller = PreviewWindowController::default();
        let first = controller.replace(request(1));
        let pending = controller.reserve(PreviewWindowOwner::History, 0);
        let mut inner = controller.inner.lock().unwrap();
        assert!(!inner.accepts_revision(first.revision));
        let (closed_revision, _) = inner.invalidate(Some(PreviewWindowOwner::History)).unwrap();
        assert!(closed_revision > pending);
        assert!(!inner.accepts_revision(pending));
        assert!(inner.current.is_none());
        assert!(inner.pending_owner.is_none());
    }

    #[test]
    fn newer_open_supersedes_pending_open_but_wrong_owner_close_does_not() {
        let controller = PreviewWindowController::default();
        let older = controller.reserve(PreviewWindowOwner::History, 0);
        let newer = controller.reserve(PreviewWindowOwner::History, 0);
        let mut inner = controller.inner.lock().unwrap();
        assert!(!inner.accepts_revision(older));
        assert!(inner.accepts_revision(newer));
        assert!(inner
            .invalidate(Some(PreviewWindowOwner::Favorites))
            .is_none());
        assert!(inner.accepts_revision(newer));
    }

    #[test]
    fn preview_focus_intent_is_preserved_and_defaults_to_deep_read() {
        let request: PreviewWindowRequest =
            serde_json::from_str(r#"{"entryId":1,"batchId":null,"owner":"history"}"#).unwrap();
        assert!(request.take_focus);
        let controller = PreviewWindowController::default();
        assert!(!controller.replace(super::tests::request(1)).take_focus);
    }
}
