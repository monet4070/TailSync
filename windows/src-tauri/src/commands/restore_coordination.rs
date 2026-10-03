//! One immediate (non-queued) restore lease for all history WebViews and types.
use std::sync::{Mutex, MutexGuard};
use tauri::{AppHandle, Emitter, Manager};

pub const RESTORE_STATE_EVENT: &str = "tailsync://restore-state";

#[derive(Default)]
pub(crate) struct RestoreState {
    pub active: bool,
    pub epoch: u64,
}

impl RestoreState {
    fn begin(&mut self) -> Result<(), &'static str> {
        if self.active {
            return Err("A clipboard restore is already in progress");
        }
        self.active = true;
        // Invalidates pending preview navigation/presentation even after failure.
        self.epoch = self.epoch.saturating_add(1);
        Ok(())
    }

    pub fn accepts_preview(&self, epoch: u64) -> bool {
        !self.active && self.epoch == epoch
    }
}

static RESTORE_STATE: Mutex<RestoreState> = Mutex::new(RestoreState {
    active: false,
    epoch: 0,
});

pub(crate) fn state() -> MutexGuard<'static, RestoreState> {
    RESTORE_STATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub(super) struct RestoreLease(AppHandle);

impl RestoreLease {
    pub fn begin(app: AppHandle) -> Result<Self, super::CommandError> {
        let mut state = state();
        state.begin()?;
        let preview_revision = app
            .try_state::<crate::preview_window::PreviewWindowController>()
            .map(|controller| controller.freeze_for_restore());
        let _ = app.emit(RESTORE_STATE_EVENT, serde_json::json!({"active": true, "epoch": state.epoch, "previewRevision": preview_revision}));
        Ok(Self(app))
    }
}

impl Drop for RestoreLease {
    fn drop(&mut self) {
        let mut state = state();
        state.active = false;
        let _ = self.0.emit(
            RESTORE_STATE_EVENT,
            serde_json::json!({"active": false, "epoch": state.epoch}),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_restore_sources_share_immediate_exclusion() {
        let state = Mutex::new(RestoreState::default());
        state.lock().unwrap().begin().unwrap();
        std::thread::scope(|scope| {
            let results: Vec<_> = (0..3)
                .map(|_| scope.spawn(|| state.lock().unwrap().begin().is_err()))
                .collect();
            for result in results {
                assert!(result.join().unwrap());
            }
        });
        // Release on success, error or cancellation permits an explicit retry,
        // never a queued second clipboard write.
        state.lock().unwrap().active = false;
        state.lock().unwrap().begin().unwrap();
    }

    #[test]
    fn restore_invalidates_old_preview_tickets_even_after_failure() {
        let mut state = RestoreState::default();
        let old_epoch = state.epoch;
        assert!(state.accepts_preview(old_epoch));
        state.begin().unwrap();
        assert!(!state.accepts_preview(state.epoch));
        state.active = false;
        assert!(!state.accepts_preview(old_epoch));
        assert!(state.accepts_preview(state.epoch));
    }
}
