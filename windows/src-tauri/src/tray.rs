use log::info;
#[cfg(target_os = "macos")]
use std::process::{Child, Command};
use tauri::AppHandle;

// ═══════════════════════════════════════════════════════════════════
// Public entry point
// ═══════════════════════════════════════════════════════════════════

/// Start the tray system on non-macOS platforms using Tauri's built-in tray.
/// macOS uses the SwiftUI menu bar instead, so this entry point is excluded there.
#[cfg(not(target_os = "macos"))]
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
        menu::{Menu, MenuItem},
        tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    };

    let show = MenuItem::with_id(app, "show", "History", true, None::<&str>).unwrap();
    let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>).unwrap();
    let sep = tauri::menu::PredefinedMenuItem::separator(app).unwrap();
    let quit = MenuItem::with_id(app, "quit", "Quit TailSync", true, None::<&str>).unwrap();
    let menu = Menu::with_items(app, &[&show, &settings, &sep, &quit]).unwrap();

    // The tray is rendered at roughly 16-24 physical pixels on Windows.
    // Feeding it the 512px application artwork makes the shell resample the
    // fine document outlines too aggressively and produces a visibly soft
    // icon.  Use the dedicated small asset so the shell starts from already
    // pixel-aligned edges.
    let icon = Image::from_bytes(include_bytes!("../icons/32x32.png"))
        .unwrap_or_else(|_| Image::new(&[0u8; 1024], 32, 32));

    let _tray = TrayIconBuilder::new()
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
    use tauri::{
        image::Image,
        menu::{Menu, MenuItem},
        tray::TrayIconBuilder,
    };

    let is_zh = crate::crypto::Settings::load()
        .map(|s| s.language == "zh-CN")
        .unwrap_or(false);
    let (hl, sl, ql) = if is_zh {
        ("历史记录", "设置", "退出")
    } else {
        ("History", "Settings", "Quit")
    };

    let show = MenuItem::with_id(app, "show", hl, true, None::<&str>).unwrap();
    let settings = MenuItem::with_id(app, "settings", sl, true, None::<&str>).unwrap();
    let sep = tauri::menu::PredefinedMenuItem::separator(app).unwrap();
    let quit = MenuItem::with_id(app, "quit", ql, true, None::<&str>).unwrap();
    let menu = Menu::with_items(app, &[&show, &settings, &sep, &quit]).unwrap();

    let icon = Image::from_bytes(include_bytes!("../icons/icon.png"))
        .unwrap_or_else(|_| Image::new(&[0u8; 1024], 32, 32));

    let _tray = TrayIconBuilder::new()
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
