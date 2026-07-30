use log::info;
#[cfg(target_os = "macos")]
use std::process::{Child, Command};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    AppHandle, Runtime,
};

const TRAY_ID: &str = "tailsync-tray";

#[derive(Debug, PartialEq, Eq)]
struct TrayLabels {
    history: &'static str,
    settings: &'static str,
    quit: &'static str,
}

fn tray_labels(language: &str) -> TrayLabels {
    if language == "zh-CN" {
        TrayLabels {
            history: "历史记录",
            settings: "设置",
            quit: "退出 TailSync",
        }
    } else {
        TrayLabels {
            history: "History",
            settings: "Settings",
            quit: "Quit TailSync",
        }
    }
}

fn build_tray_menu<R: Runtime>(app: &AppHandle<R>, language: &str) -> tauri::Result<Menu<R>> {
    let labels = tray_labels(language);
    let show = MenuItem::with_id(app, "show", labels.history, true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", labels.settings, true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", labels.quit, true, None::<&str>)?;
    Menu::with_items(app, &[&show, &settings, &separator, &quit])
}

pub fn update_tray_menu<R: Runtime>(app: &AppHandle<R>, language: &str) -> Result<(), String> {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return Ok(());
    };
    let menu = build_tray_menu(app, language).map_err(|error| error.to_string())?;
    tray.set_menu(Some(menu)).map_err(|error| error.to_string())
}

// ═══════════════════════════════════════════════════════════════════
// Public entry point
// ═══════════════════════════════════════════════════════════════════

/// Start the tray system on non-macOS platforms using Tauri's built-in tray.
/// macOS uses the SwiftUI menu bar instead, so this entry point is excluded there.
#[cfg(not(target_os = "macos"))]
#[cfg_attr(test, allow(dead_code))]
pub fn start_tray(app_handle: AppHandle) {
    create_tauri_tray(&app_handle);
}

// ═══════════════════════════════════════════════════════════════════
// Swift helper spawn (macOS only)
// ═══════════════════════════════════════════════════════════════════

#[cfg(target_os = "macos")]
#[allow(dead_code)]
fn spawn_helper(app_handle: &AppHandle) -> Result<Child, Box<dyn std::error::Error>> {
    let _ = app_handle; // reserved for future use (e.g. writing config for helper)

    // Kill any stale helpers from previous runs
    let _ = Command::new("pkill").arg("-f").arg("TrayHelper").status();

    // Resolve the compiled binary.  In dev mode it lives in the SPM
    // build directory; in production it would be inside the .app bundle.
    let bin_path = resolve_helper_path()?;
    info!("Launching tray helper: {}", bin_path.display());

    // Write language setting so the helper can read it
    let lang = crate::crypto::Settings::load()
        .map(|s| s.language)
        .unwrap_or_else(|_| "en".into());

    let child = Command::new(&bin_path)
        .arg("--lang")
        .arg(&lang)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;

    Ok(child)
}

#[cfg(target_os = "macos")]
#[allow(dead_code)]
fn resolve_helper_path() -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    // Try several locations — SPM debug build, release build, alongside binary
    let candidates = [
        // Dev: SPM .build/debug/
        "src-tauri/tray-helper/.build/debug/TrayHelper",
        // Dev: absolute from project root
        "../src-tauri/tray-helper/.build/debug/TrayHelper",
        // Prod: inside .app bundle Resources/
        "TrayHelper",
    ];

    // Start from the current executable's directory
    if let Ok(exe) = std::env::current_exe() {
        let exe_dir = exe.parent().unwrap_or_else(|| std::path::Path::new("."));

        for rel in &candidates {
            let p = exe_dir.join(rel);
            if p.exists() {
                return Ok(p);
            }
        }

        // Also try relative to CWD
        for rel in &candidates {
            let p = std::path::Path::new(rel);
            if p.exists() {
                return Ok(p.to_path_buf());
            }
        }
    }

    Err("TrayHelper binary not found.  Run: cd src-tauri/tray-helper && swift build".into())
}

// ═══════════════════════════════════════════════════════════════════
// Tauri tray fallback (non-macOS)
// ═══════════════════════════════════════════════════════════════════

#[cfg(not(target_os = "macos"))]
#[allow(dead_code)]
fn create_tauri_tray(app: &AppHandle) {
    use tauri::{
        image::Image,
        tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    };

    let language = crate::crypto::Settings::load()
        .map(|settings| settings.language)
        .unwrap_or_else(|_| "en".into());
    let menu = build_tray_menu(app, &language).expect("failed to build tray menu");

    // The tray is rendered at roughly 16-24 physical pixels on Windows.
    // Feeding it the 512px application artwork makes the shell resample the
    // fine document outlines too aggressively and produces a visibly soft
    // icon.  Use the dedicated small asset so the shell starts from already
    // pixel-aligned edges.
    let icon = Image::from_bytes(include_bytes!("../icons/32x32.png"))
        .unwrap_or_else(|_| Image::new(&[0u8; 1024], 32, 32));

    let _tray = TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("TailSync")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                let h = app.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = crate::commands::open_history_window(h).await;
                });
            }
            "settings" => {
                let h = app.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = crate::commands::open_settings_window(h).await;
                });
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Down,
                ..
            } = event
            {
                let h = tray.app_handle().clone();
                tauri::async_runtime::spawn(async move {
                    let _ = crate::commands::open_history_window(h).await;
                });
            }
        })
        .build(app)
        .unwrap();

    info!("Tauri tray created (non-macOS)");
}

#[cfg(target_os = "macos")]
#[allow(dead_code)]
fn create_tauri_tray(app: &AppHandle) {
    // On macOS this is only called as a fallback when the Swift helper
    // fails to start, so we do create a Tauri tray.
    use tauri::{image::Image, tray::TrayIconBuilder};

    let language = crate::crypto::Settings::load()
        .map(|settings| settings.language)
        .unwrap_or_else(|_| "en".into());
    let menu = build_tray_menu(app, &language).expect("failed to build tray menu");

    let icon = Image::from_bytes(include_bytes!("../icons/icon.png"))
        .unwrap_or_else(|_| Image::new(&[0u8; 1024], 32, 32));

    let _tray = TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .menu(&menu)
        .tooltip("TailSync")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" | "history" => {
                let h = app.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = crate::commands::open_history_window(h).await;
                });
            }
            "settings" => {
                let h = app.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = crate::commands::open_settings_window(h).await;
                });
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)
        .unwrap();

    info!("Tauri tray created (fallback)");
}

#[cfg(test)]
mod tests {
    use super::{tray_labels, TrayLabels};

    #[test]
    fn tray_labels_follow_saved_language() {
        assert_eq!(
            tray_labels("zh-CN"),
            TrayLabels {
                history: "历史记录",
                settings: "设置",
                quit: "退出 TailSync",
            }
        );
        assert_eq!(
            tray_labels("en"),
            TrayLabels {
                history: "History",
                settings: "Settings",
                quit: "Quit TailSync",
            }
        );
        assert_eq!(tray_labels("unsupported"), tray_labels("en"));
    }
}
