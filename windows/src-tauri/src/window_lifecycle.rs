use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Manager};

pub const HISTORY_WINDOW_LABEL: &str = "history";
pub const SETTINGS_WINDOW_LABEL: &str = "settings";
pub const TRANSIENT_WINDOW_IDLE_RELEASE: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ReleaseTicket {
    label: &'static str,
    generation: u64,
}

#[derive(Default)]
pub struct TransientWindowController {
    generations: Mutex<HashMap<&'static str, u64>>,
}

impl TransientWindowController {
    fn mark_open(&self, label: &'static str) {
        let mut generations = self
            .generations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let generation = generations.entry(label).or_default();
        *generation = generation.wrapping_add(1).max(1);
    }

    fn begin_release<T>(
        &self,
        label: &'static str,
        hide: impl FnOnce() -> Result<T, String>,
    ) -> Result<ReleaseTicket, String> {
        let mut generations = self
            .generations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        hide()?;
        let generation = generations.entry(label).or_default();
        *generation = generation.wrapping_add(1).max(1);
        Ok(ReleaseTicket {
            label,
            generation: *generation,
        })
    }

    fn release_if_current<T>(
        &self,
        ticket: ReleaseTicket,
        release: impl FnOnce() -> T,
    ) -> Option<T> {
        let generations = self
            .generations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (generations.get(ticket.label) == Some(&ticket.generation)).then(release)
    }
}

pub fn mark_window_open(app: &AppHandle, label: &'static str) {
    if let Some(controller) = app.try_state::<TransientWindowController>() {
        controller.mark_open(label);
    }
}

pub fn hide_then_release_window(app: AppHandle, label: &'static str) -> Result<(), String> {
    let Some(window) = app.get_webview_window(label) else {
        return Ok(());
    };
    let controller = app
        .try_state::<TransientWindowController>()
        .ok_or_else(|| "Transient window controller is unavailable".to_string())?;
    let ticket =
        controller.begin_release(label, || window.hide().map_err(|error| error.to_string()))?;

    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(TRANSIENT_WINDOW_IDLE_RELEASE).await;
        let Some(controller) = app.try_state::<TransientWindowController>() else {
            return;
        };
        let result = controller.release_if_current(ticket, || {
            app.get_webview_window(label)
                .map(|window| window.destroy().map_err(|error| error.to_string()))
                .unwrap_or(Ok(()))
        });
        if let Some(Err(error)) = result {
            log::debug!("Could not release idle {label} window: {error}");
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reopening_cancels_an_older_release_ticket() {
        let controller = TransientWindowController::default();
        let ticket = controller
            .begin_release(HISTORY_WINDOW_LABEL, || Ok(()))
            .unwrap();
        controller.mark_open(HISTORY_WINDOW_LABEL);

        assert_eq!(controller.release_if_current(ticket, || "released"), None);
    }

    #[test]
    fn latest_release_ticket_can_destroy_its_window() {
        let controller = TransientWindowController::default();
        controller.mark_open(SETTINGS_WINDOW_LABEL);
        let ticket = controller
            .begin_release(SETTINGS_WINDOW_LABEL, || Ok(()))
            .unwrap();

        assert_eq!(
            controller.release_if_current(ticket, || "released"),
            Some("released")
        );
    }

    #[test]
    fn failed_hide_does_not_schedule_a_release() {
        let controller = TransientWindowController::default();
        controller.mark_open(HISTORY_WINDOW_LABEL);
        assert!(controller
            .begin_release(HISTORY_WINDOW_LABEL, || Err::<(), _>(
                "hide failed".to_string()
            ))
            .is_err());

        let ticket = controller
            .begin_release(HISTORY_WINDOW_LABEL, || Ok(()))
            .unwrap();
        assert_eq!(ticket.generation, 2);
    }
}
