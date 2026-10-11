use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Manager, Runtime};

pub const HISTORY_WINDOW_LABEL: &str = "history";
pub const FAVORITES_WINDOW_LABEL: &str = "favorites";
pub const SETTINGS_WINDOW_LABEL: &str = "settings";
pub const CONNECTIONS_WINDOW_LABEL: &str = "connections";
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
    fn focus_webview(&self) -> tauri::Result<()>;
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
        // WebView2 owns a separate surface. Restore it before the host window
        // after an explicit surface hide, including a quick reopen while idle.
        self.as_ref().show()?;
        self.show()
    }

    fn focus_window(&self) -> tauri::Result<()> {
        self.set_focus()
    }

    fn focus_webview(&self) -> tauri::Result<()> {
        self.as_ref().set_focus()
    }
}

pub(crate) fn focus_window_content<W: WindowActivation>(window: &W) -> Result<(), String> {
    window.focus_window().map_err(|error| error.to_string())?;
    window.focus_webview().map_err(|error| error.to_string())
}

pub(crate) fn restore_and_focus_window<W: WindowActivation>(window: &W) -> Result<(), String> {
    window.move_to_current_desktop()?;
    window
        .unminimize_window()
        .map_err(|error| error.to_string())?;
    window.show_window().map_err(|error| error.to_string())?;
    focus_window_content(window)?;
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
    #[cfg(any(target_os = "windows", test))]
    original_foreground: Mutex<Option<(usize, u32, u32)>>,
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

/// Capture on the hotkey callback thread, before spawning or activating UI.
pub fn capture_history_foreground(app: &AppHandle) {
    capture_foreground(app, true);
}

/// An explicit/menu open starts a fresh session, never inherits a previous HWND.
pub fn capture_history_foreground_for_open(app: &AppHandle) {
    capture_foreground(app, false);
}

fn capture_foreground(app: &AppHandle, preserve_active: bool) {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowThreadProcessId,
        };
        let Some(controller) = app.try_state::<TransientWindowController>() else {
            return;
        };
        let foreground = unsafe { GetForegroundWindow() };
        let owned = app
            .webview_windows()
            .values()
            .any(|window| window.hwnd().is_ok_and(|hwnd| hwnd.0 == foreground.0));
        // Toggling an already-active history must retain its original target.
        if owned
            && preserve_active
            && app
                .get_webview_window(HISTORY_WINDOW_LABEL)
                .is_some_and(|window| window.is_visible().unwrap_or(false))
        {
            return;
        }
        let mut original = controller
            .original_foreground
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *original = None;
        if foreground.0.is_null() || owned {
            return;
        }
        let mut pid = 0;
        let tid = unsafe { GetWindowThreadProcessId(foreground, Some(&mut pid)) };
        if tid != 0 && pid != 0 {
            *original = Some((foreground.0 as usize, pid, tid));
        }
    }
    #[cfg(not(target_os = "windows"))]
    let _ = (app, preserve_active);
}

pub fn return_history_foreground(app: &AppHandle) {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{
            GetWindowThreadProcessId, IsIconic, IsWindow, SetForegroundWindow, ShowWindow,
            SW_RESTORE,
        };
        let Some(controller) = app.try_state::<TransientWindowController>() else {
            return;
        };
        // Consume even on invalid/denied activation: no stale target next session.
        let Some((original, original_pid, original_tid)) = controller
            .original_foreground
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        else {
            return;
        };
        let hwnd = HWND(original as *mut std::ffi::c_void);
        unsafe {
            if !IsWindow(Some(hwnd)).as_bool() {
                return;
            }
            let mut pid = 0;
            let tid = GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid != original_pid || tid != original_tid {
                return;
            }
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }
            // Must precede hide(), while TailSync still owns the foreground grant.
            if !SetForegroundWindow(hwnd).as_bool() {
                log::debug!("Windows denied returning focus to the original HWND");
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    let _ = app;
}

pub fn show_without_activation(window: &tauri::WebviewWindow) -> Result<(), String> {
    window.as_ref().show().map_err(|error| error.to_string())?;
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_SHOWNOACTIVATE};
        let hwnd =
            windows::Win32::Foundation::HWND(window.hwnd().map_err(|error| error.to_string())?.0);
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    window.show().map_err(|error| error.to_string())
}

pub fn mark_window_open(app: &AppHandle, label: &'static str) {
    if let Some(controller) = app.try_state::<TransientWindowController>() {
        controller.mark_open(label);
    }
}

/// Preview has a close/recreate lifecycle, separate from the idle-cached
/// history/settings windows. Hide its native host before touching WebView2.
pub(crate) fn withdraw_and_destroy_preview(window: &tauri::WebviewWindow) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    withdraw_native_preview(windows::Win32::Foundation::HWND(
        window.hwnd().map_err(|error| error.to_string())?.0,
    ))?;
    // Keep Tauri/Tao's visibility state in sync. Do not hide just the WebView:
    // that leaves a transparent HWND in the window stack until destruction.
    window.hide().map_err(|error| error.to_string())?;
    window.destroy().map_err(|error| error.to_string())
}

#[cfg(target_os = "windows")]
fn withdraw_native_preview(hwnd: windows::Win32::Foundation::HWND) -> Result<(), String> {
    use windows::Win32::UI::WindowsAndMessaging::{
        IsWindowVisible, SetWindowPos, ShowWindow, HWND_NOTOPMOST, SWP_HIDEWINDOW, SWP_NOACTIVATE,
        SWP_NOMOVE, SWP_NOSIZE, SW_HIDE,
    };
    unsafe {
        // SW_HIDE returns the previous visibility, not an error code.
        let _ = ShowWindow(hwnd, SW_HIDE);
        SetWindowPos(
            hwnd,
            Some(HWND_NOTOPMOST),
            0,
            0,
            0,
            0,
            SWP_HIDEWINDOW | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
        )
        .map_err(|error| error.to_string())?;
        if IsWindowVisible(hwnd).as_bool() {
            return Err("Preview native host remained visible after withdrawal".to_string());
        }
    }
    Ok(())
}

pub fn hide_then_release_window(app: AppHandle, label: &'static str) -> Result<(), String> {
    let Some(window) = app.get_webview_window(label) else {
        return Ok(());
    };
    let controller = app
        .try_state::<TransientWindowController>()
        .ok_or_else(|| "Transient window controller is unavailable".to_string())?;
    let ticket = controller.begin_release(label, || {
        // Hiding the HWND alone can leave the transparent WebView2 surface
        // drawn until the five-second idle destruction. Hide both surfaces.
        window.as_ref().hide().map_err(|error| error.to_string())?;
        if let Err(error) = window.hide() {
            let _ = window.as_ref().show();
            return Err(error.to_string());
        }
        Ok(())
    })?;

    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(TRANSIENT_WINDOW_IDLE_RELEASE).await;
        let ui_app = app.clone();
        let _ = app.run_on_main_thread(move || {
            let Some(controller) = ui_app.try_state::<TransientWindowController>() else {
                return;
            };
            // Validate and destroy in the same UI turn: an already queued
            // destroy must never tear down a window that just reopened.
            let result = controller.release_if_current(ticket, || {
                ui_app
                    .get_webview_window(label)
                    .map(|window| window.destroy().map_err(|error| error.to_string()))
                    .unwrap_or(Ok(()))
            });
            if let Some(Err(error)) = result {
                log::debug!("Could not release idle {label} window: {error}");
            }
        });
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

        fn focus_webview(&self) -> tauri::Result<()> {
            self.calls.lock().unwrap().push("webview-focus");
            Ok(())
        }
    }

    #[test]
    fn foreground_target_is_consumed_once_even_when_return_cannot_activate() {
        let controller = TransientWindowController::default();
        *controller.original_foreground.lock().unwrap() = Some((123, 45, 67));
        assert_eq!(
            controller.original_foreground.lock().unwrap().take(),
            Some((123, 45, 67))
        );
        assert_eq!(controller.original_foreground.lock().unwrap().take(), None);
    }

    #[test]
    fn restoring_a_window_unminimizes_shows_and_focuses_in_order() {
        let window = FakeWindow::default();

        restore_and_focus_window(&window).unwrap();

        assert_eq!(
            *window.calls.lock().unwrap(),
            vec!["desktop", "unminimize", "show", "focus", "webview-focus"]
        );
    }

    #[test]
    fn returning_to_history_focuses_native_host_then_keyboard_content() {
        let window = FakeWindow::default();
        focus_window_content(&window).unwrap();
        assert_eq!(
            *window.calls.lock().unwrap(),
            vec!["focus", "webview-focus"]
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn withdrawing_preview_removes_visible_topmost_native_host() {
        use windows::core::w;
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, GetWindowLongPtrW, IsWindowVisible, GWL_EXSTYLE,
            WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
        };
        struct OwnedWindow(windows::Win32::Foundation::HWND);
        impl Drop for OwnedWindow {
            fn drop(&mut self) {
                let _ = unsafe { DestroyWindow(self.0) };
            }
        }
        // An actual Win32 host validates visibility/hit-test exclusion and
        // topmost removal, rather than just counting mocked hide calls.
        let host = OwnedWindow(unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST,
                w!("STATIC"),
                w!("TailSync preview lifecycle test"),
                WS_POPUP | WS_VISIBLE,
                0,
                0,
                1,
                1,
                None,
                None,
                None,
                None,
            )
            .expect("create native preview fixture")
        });
        assert!(unsafe { IsWindowVisible(host.0) }.as_bool());
        assert_ne!(
            unsafe { GetWindowLongPtrW(host.0, GWL_EXSTYLE) } & WS_EX_TOPMOST.0 as isize,
            0
        );
        withdraw_native_preview(host.0).unwrap();
        assert!(!unsafe { IsWindowVisible(host.0) }.as_bool());
        assert_eq!(
            unsafe { GetWindowLongPtrW(host.0, GWL_EXSTYLE) } & WS_EX_TOPMOST.0 as isize,
            0
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
