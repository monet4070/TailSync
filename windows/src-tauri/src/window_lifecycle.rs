use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Manager, Runtime};

pub const HISTORY_WINDOW_LABEL: &str = "history";
pub const FAVORITES_WINDOW_LABEL: &str = "favorites";
pub const SETTINGS_WINDOW_LABEL: &str = "settings";
pub const TRANSIENT_WINDOW_IDLE_RELEASE: Duration = Duration::from_secs(5);

pub(crate) fn configure_transparent_window<'a, R, M>(
    builder: tauri::WebviewWindowBuilder<'a, R, M>,
) -> tauri::WebviewWindowBuilder<'a, R, M>
where
    R: Runtime,
    M: Manager<R>,
{
    #[cfg(not(target_os = "macos"))]
    {
        builder.transparent(true)
    }

    #[cfg(target_os = "macos")]
    {
        builder
    }
}

pub(crate) trait WindowActivation {
    fn move_to_current_desktop(&self) -> Result<(), String>;
    fn unminimize_window(&self) -> tauri::Result<()>;
    fn show_window(&self) -> tauri::Result<()>;
    fn focus_window(&self) -> tauri::Result<()>;
}

impl<R: tauri::Runtime> WindowActivation for tauri::WebviewWindow<R> {
    fn move_to_current_desktop(&self) -> Result<(), String> {
        #[cfg(target_os = "windows")]
        {
            if let Err(error) = move_window_to_current_virtual_desktop(self) {
                // Virtual desktop APIs are optional on older Windows builds
                // and may reject a window owned by another security context.
                // Failing this best-effort step must not prevent the normal
                // show/focus path from restoring the history window.
                log::debug!("Could not move window to the current virtual desktop: {error}");
            }
        }
        Ok(())
    }

    fn unminimize_window(&self) -> tauri::Result<()> {
        self.unminimize()
    }

    fn show_window(&self) -> tauri::Result<()> {
        self.show()
    }

    fn focus_window(&self) -> tauri::Result<()> {
        self.set_focus()
    }
}

pub(crate) fn restore_and_focus_window<W: WindowActivation>(window: &W) -> Result<(), String> {
    window.move_to_current_desktop()?;
    window
        .unminimize_window()
        .map_err(|error| error.to_string())?;
    window.show_window().map_err(|error| error.to_string())?;
    window.focus_window().map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn move_window_to_current_virtual_desktop<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
) -> Result<(), String> {
    use windows::Win32::System::Com::CoGetApartmentType;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{IVirtualDesktopManager, VirtualDesktopManager};
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    let hwnd =
        windows::Win32::Foundation::HWND(window.hwnd().map_err(|error| error.to_string())?.0);
    let foreground = unsafe { GetForegroundWindow() };
    if foreground.0.is_null() || foreground == hwnd {
        return Ok(());
    }

    // The Tauri callback can run on a worker thread. Initialise COM only for
    // this call when the thread has not already joined an apartment.
    let mut apartment_type = windows::Win32::System::Com::APTTYPE(0);
    let mut apartment_qualifier = windows::Win32::System::Com::APTTYPEQUALIFIER(0);
    let apartment_result =
        unsafe { CoGetApartmentType(&mut apartment_type, &mut apartment_qualifier) };
    let initialized_here = apartment_result.is_err();
    if initialized_here {
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
            .ok()
            .map_err(|error| error.to_string())?;
    }

    let result = (|| {
        let manager: IVirtualDesktopManager =
            unsafe { CoCreateInstance(&VirtualDesktopManager, None, CLSCTX_ALL) }
                .map_err(|error| error.to_string())?;
        let on_current = unsafe {
            manager
                .IsWindowOnCurrentVirtualDesktop(hwnd)
                .map_err(|error| error.to_string())?
        };
        if on_current.as_bool() {
            return Ok(());
        }
        let desktop_id = unsafe {
            manager
                .GetWindowDesktopId(foreground)
                .map_err(|error| error.to_string())?
        };
        unsafe { manager.MoveWindowToDesktop(hwnd, &desktop_id) }.map_err(|error| error.to_string())
    })();

    if initialized_here {
        unsafe { CoUninitialize() };
    }
    result
}

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

    #[derive(Default)]
    struct FakeWindow {
        calls: Mutex<Vec<&'static str>>,
    }

    impl WindowActivation for FakeWindow {
        fn move_to_current_desktop(&self) -> Result<(), String> {
            self.calls.lock().unwrap().push("desktop");
            Ok(())
        }

        fn unminimize_window(&self) -> tauri::Result<()> {
            self.calls.lock().unwrap().push("unminimize");
            Ok(())
        }

        fn show_window(&self) -> tauri::Result<()> {
            self.calls.lock().unwrap().push("show");
            Ok(())
        }

        fn focus_window(&self) -> tauri::Result<()> {
            self.calls.lock().unwrap().push("focus");
            Ok(())
        }
    }

    #[test]
    fn restoring_a_window_unminimizes_shows_and_focuses_in_order() {
        let window = FakeWindow::default();

        restore_and_focus_window(&window).unwrap();

        assert_eq!(
            *window.calls.lock().unwrap(),
            vec!["desktop", "unminimize", "show", "focus"]
        );
    }

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
