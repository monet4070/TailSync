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
    pub document_eligible: bool,
}

pub struct PreviewWindowController {
    inner: Mutex<PreviewWindowControllerInner>,
    window_retiring: tokio::sync::watch::Sender<bool>,
}

impl Default for PreviewWindowController {
    fn default() -> Self {
        Self {
            inner: Mutex::default(),
            window_retiring: tokio::sync::watch::channel(false).0,
        }
    }
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
        if self.pending_owner.is_none() && self.current.is_none() {
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
    async fn wait_for_window_release(&self) {
        let mut retiring = self.window_retiring.subscribe();
        loop {
            if !*retiring.borrow_and_update() {
                return;
            }
            if retiring.changed().await.is_err() {
                return;
            }
        }
    }

    fn begin_window_release(&self) {
        self.window_retiring.send_replace(true);
    }

    fn finish_window_release(&self) {
        self.window_retiring.send_replace(false);
    }

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
            document_eligible: false,
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

    fn retire(
        &self,
        owner: Option<PreviewWindowOwner>,
    ) -> Option<(u64, Option<PreviewWindowOwner>)> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .invalidate(owner)
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
    let document_eligible = {
        let database = app_state.db.lock().await;
        if let Some(batch_id) = request.batch_id.as_deref() {
            let navigation = database
                .get_preview_batch_navigation(batch_id, request.entry_id)
                .map_err(|error| error.to_string())?;
            request.entry_id = navigation.first_entry_id;
        }
        database
            .display_kind(request.entry_id)
            .is_ok_and(|kind| kind == "document")
    };
    // Tauri queues native destruction. A fast reopen must wait asynchronously
    // for the old WebView/HWND to leave the manager, rather than reuse it.
    controller.wait_for_window_release().await;
    let (send, receive) = tokio::sync::oneshot::channel();
    let ui_app = app.clone();
    app.run_on_main_thread(move || {
        let result = finish_open_preview(ui_app, request, revision, document_eligible);
        let _ = send.send(result);
    })
    .map_err(|error| error.to_string())?;
    receive.await.map_err(|error| error.to_string())?
}

fn finish_open_preview(
    app: AppHandle,
    request: PreviewWindowRequest,
    revision: u64,
    document_eligible: bool,
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
        document_eligible,
    };
    inner.current = Some(snapshot.clone());
    inner.minimized_with_owner = false;
    // WebView2 creation/focus may dispatch nested IPC and window callbacks.
    // They must be able to read the controller while native UI work runs.
    drop(inner);
    if let Some(window) = app.get_webview_window(PREVIEW_WINDOW_LABEL) {
        window
            .emit(PREVIEW_REQUEST_EVENT, &snapshot)
            .map_err(|error| error.to_string())?;
    } else {
        let builder = tauri::WebviewWindowBuilder::new(
            &app,
            PREVIEW_WINDOW_LABEL,
            tauri::WebviewUrl::App("preview.html".into()),
        )
        .title("TailSync - Preview")
        .inner_size(900.0, 680.0)
        .min_inner_size(520.0, 360.0)
        .decorations(false)
        .shadow(false)
        .resizable(true)
        .visible(false)
        .focused(false);
        #[cfg(target_os = "windows")]
        let builder = builder.transparent(false);
        builder
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
/// and native show run on the UI thread, so a late renderer cannot reopen it.
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
        .cloned()
    else {
        return Ok(());
    };
    drop(inner);
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
pub async fn get_preview_window_request(
    app: AppHandle,
    ready_revision: Option<u64>,
) -> Result<Option<PreviewWindowSnapshot>, crate::commands::CommandError> {
    if let Some(revision) = ready_revision {
        let (send, receive) = tokio::sync::oneshot::channel();
        let ui_app = app.clone();
        app.run_on_main_thread(move || {
            let controller = ui_app.state::<PreviewWindowController>();
            let result = present_preview_window(ui_app.clone(), controller.clone(), revision)
                .map(|()| controller.current());
            let _ = send.send(result);
        })
        .map_err(|error| error.to_string())?;
        return receive.await.map_err(|error| error.to_string())?;
    }
    Ok(app.state::<PreviewWindowController>().current())
}

/// Withdraw the native preview and destroy its WebView immediately, matching
/// the macOS close lifecycle. A later preview creates a fresh window.
#[command]
pub async fn close_preview_window(
    app: AppHandle,
    owner: Option<PreviewWindowOwner>,
) -> Result<(), crate::commands::CommandError> {
    let started = std::time::Instant::now();
    let (send, receive) = tokio::sync::oneshot::channel();
    let ui_app = app.clone();
    app.run_on_main_thread(move || {
        let result = finish_close_preview(ui_app, owner);
        log::info!(
            "Preview native withdrawal completed in {} ms (success={})",
            started.elapsed().as_millis(),
            result.is_ok()
        );
        let _ = send.send(result);
    })
    .map_err(|error| error.to_string())?;
    receive.await.map_err(|error| error.to_string())?
}

fn finish_close_preview(
    app: AppHandle,
    owner: Option<PreviewWindowOwner>,
) -> Result<(), crate::commands::CommandError> {
    let controller = app.state::<PreviewWindowController>();
    let Some((revision, previous_owner)) = controller.retire(owner) else {
        return Ok(());
    };
    let notice = serde_json::json!({ "revision": revision, "owner": previous_owner });
    // Withdraw the HWND before changing renderer content or returning focus.
    // No transparent topmost host may survive a five-second idle grace period.
    if let Some(window) = app.get_webview_window(PREVIEW_WINDOW_LABEL) {
        let return_focus = window.is_focused().unwrap_or(false);
        controller.begin_window_release();
        let closed_app = app.clone();
        let started = std::time::Instant::now();
        window.on_window_event(move |event| {
            if matches!(event, tauri::WindowEvent::Destroyed) {
                closed_app
                    .state::<PreviewWindowController>()
                    .finish_window_release();
                log::info!(
                    "Preview native window destroyed in {} ms",
                    started.elapsed().as_millis()
                );
            }
        });
        if let Err(error) = crate::window_lifecycle::withdraw_and_destroy_preview(&window) {
            controller.finish_window_release();
            return Err(error.into());
        }
        if return_focus {
            if let Some(host) =
                previous_owner.and_then(|owner| app.get_webview_window(owner.label()))
            {
                // Activating the HWND alone does not reliably return keyboard
                // input from a hidden WebView2 controller to history's content.
                if let Err(error) = crate::window_lifecycle::focus_window_content(&host) {
                    log::warn!("Could not return preview focus to its owner: {error}");
                }
            }
        }
    }
    if let Some(owner) = previous_owner {
        let _ = app.emit_to(owner.label(), PREVIEW_CLOSE_EVENT, &notice);
    }
    Ok(())
}

/// Keep the reusable preview paired only with the active source window's
/// minimized state. The marker records only a minimize initiated here, so a
/// manually minimized preview is never undone later.
#[command]
pub async fn sync_preview_window_minimized(
    app: AppHandle,
    owner: PreviewWindowOwner,
    minimized: bool,
) -> Result<(), crate::commands::CommandError> {
    let (send, receive) = tokio::sync::oneshot::channel();
    let ui_app = app.clone();
    app.run_on_main_thread(move || {
        let result = finish_sync_preview_minimized(ui_app, owner, minimized);
        let _ = send.send(result);
    })
    .map_err(|error| error.to_string())?;
    receive.await.map_err(|error| error.to_string())?
}

fn finish_sync_preview_minimized(
    app: AppHandle,
    owner: PreviewWindowOwner,
    minimized: bool,
) -> Result<(), crate::commands::CommandError> {
    let controller = app.state::<PreviewWindowController>();
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
    fn snapshot_exposes_the_document_eligibility_flag_to_the_renderer() {
        let controller = PreviewWindowController::default();
        let snapshot = controller.replace(request(1));
        let json = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(json["documentEligible"], serde_json::json!(false));
        assert_eq!(json["entryId"], serde_json::json!(1));
        assert_eq!(json["takeFocus"], serde_json::json!(false));
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
    fn close_transition_releases_controller_before_focus_callbacks_reenter() {
        let controller = PreviewWindowController::default();
        let previous = controller.replace(request(1));
        let (revision, owner) = controller
            .retire(Some(PreviewWindowOwner::History))
            .unwrap();
        assert!(revision > previous.revision);
        assert_eq!(owner, Some(PreviewWindowOwner::History));
        // Native focus/hide can synchronously reenter through WebView2 IPC.
        // Simulate that callback without risking a hung test on lock regression.
        assert!(controller.inner.try_lock().is_ok());
        assert!(!controller.owns(PreviewWindowOwner::History));
        assert_eq!(controller.current(), None);
    }

    #[test]
    fn repeated_close_does_not_retire_a_window_twice() {
        let controller = PreviewWindowController::default();
        controller.replace(request(1));
        assert!(controller.retire(None).is_some());
        assert!(controller.retire(None).is_none());
    }

    #[tokio::test]
    async fn quick_reopen_waits_for_native_destruction_without_blocking_ui() {
        let controller = PreviewWindowController::default();
        controller.begin_window_release();
        assert!(tokio::time::timeout(
            std::time::Duration::from_millis(25),
            controller.wait_for_window_release()
        )
        .await
        .is_err());
        controller.finish_window_release();
        tokio::time::timeout(
            std::time::Duration::from_millis(25),
            controller.wait_for_window_release(),
        )
        .await
        .expect("native destruction must unblock the next preview");
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
