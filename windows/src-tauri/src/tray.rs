use log::info;
#[cfg(target_os = "macos")]
use std::process::{Child, Command};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    AppHandle, Manager, Runtime,
};

const TRAY_ID: &str = "tailsync-tray";
static TRANSPARENT_TRAY_RGBA: [u8; 32 * 32 * 4] = [0; 32 * 32 * 4];

fn request_shutdown<R: Runtime>(app: &AppHandle<R>) {
    if let Some(state) = app.try_state::<crate::AppState>() {
        let _ = state.shutdown.send(true);
    } else {
        app.exit(0);
    }
}

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

#[derive(Clone, Debug, PartialEq, Eq)]
struct TrayTransferState {
    summary: String,
    current_file: String,
    stop_label: &'static str,
    can_stop: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TrayMenuState {
    language: String,
    storage_unavailable: bool,
    transfer: Option<TrayTransferState>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TrayMenuStructure {
    language: String,
    storage_unavailable: bool,
    transfer_visible: bool,
}

impl TrayMenuState {
    fn from_current_progress(language: String, storage_unavailable: bool) -> Self {
        let transfer = crate::api::get_file_progress()
            .filter(|progress| progress.active)
            .map(|progress| {
                let percent = progress.sent.saturating_mul(100) / progress.total.max(1);
                TrayTransferState {
                    summary: format!(
                        "{} / {} files - {}%",
                        progress.completed_files, progress.total_files, percent
                    ),
                    current_file: format!("{}  {}", progress.device, progress.name),
                    stop_label: if language == "zh-CN" {
                        "停止传输"
                    } else {
                        "Stop transfer"
                    },
                    can_stop: progress.can_stop,
                }
            });

        Self {
            language,
            storage_unavailable,
            transfer,
        }
    }

    fn structure(&self) -> TrayMenuStructure {
        TrayMenuStructure {
            language: self.language.clone(),
            storage_unavailable: self.storage_unavailable,
            transfer_visible: self.transfer.is_some(),
        }
    }
}

struct BuiltTrayMenu<R: Runtime> {
    menu: Menu<R>,
    state: TrayMenuState,
    progress_item: Option<MenuItem<R>>,
    current_file_item: Option<MenuItem<R>>,
    stop_item: Option<MenuItem<R>>,
}

fn build_tray_menu<R: Runtime>(
    app: &AppHandle<R>,
    state: TrayMenuState,
) -> tauri::Result<BuiltTrayMenu<R>> {
    let labels = tray_labels(&state.language);
    let show = MenuItem::with_id(app, "show", labels.history, true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", labels.settings, true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", labels.quit, true, None::<&str>)?;
    let storage_warning = MenuItem::with_id(
        app,
        "storage_warning",
        if state.language == "zh-CN" {
            "存储不可用，文件传输已暂停"
        } else {
            "Storage unavailable - file transfer paused"
        },
        false,
        None::<&str>,
    )?;
    let warning_separator = PredefinedMenuItem::separator(app)?;

    let (menu, progress_item, current_file_item, stop_item) =
        if let Some(transfer) = &state.transfer {
            let progress_separator = PredefinedMenuItem::separator(app)?;
            let summary = MenuItem::with_id(
                app,
                "transfer_progress",
                &transfer.summary,
                false,
                None::<&str>,
            )?;
            let current = MenuItem::with_id(
                app,
                "transfer_file",
                &transfer.current_file,
                false,
                None::<&str>,
            )?;
            let stop = MenuItem::with_id(
                app,
                "stop_transfer",
                transfer.stop_label,
                transfer.can_stop,
                None::<&str>,
            )?;
            let menu = if state.storage_unavailable {
                Menu::with_items(
                    app,
                    &[
                        &storage_warning,
                        &warning_separator,
                        &summary,
                        &current,
                        &stop,
                        &progress_separator,
                        &show,
                        &settings,
                        &separator,
                        &quit,
                    ],
                )?
            } else {
                Menu::with_items(
                    app,
                    &[
                        &summary,
                        &current,
                        &stop,
                        &progress_separator,
                        &show,
                        &settings,
                        &separator,
                        &quit,
                    ],
                )?
            };
            (menu, Some(summary), Some(current), Some(stop))
        } else if state.storage_unavailable {
            (
                Menu::with_items(
                    app,
                    &[
                        &storage_warning,
                        &warning_separator,
                        &show,
                        &settings,
                        &separator,
                        &quit,
                    ],
                )?,
                None,
                None,
                None,
            )
        } else {
            (
                Menu::with_items(app, &[&show, &settings, &separator, &quit])?,
                None,
                None,
                None,
            )
        };

    Ok(BuiltTrayMenu {
        menu,
        state,
        progress_item,
        current_file_item,
        stop_item,
    })
}

fn refresh_tray_menu<R: Runtime>(
    app: &AppHandle<R>,
    built: &mut BuiltTrayMenu<R>,
    next_state: TrayMenuState,
) -> Result<(), String> {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return Ok(());
    };

    if built.state.structure() != next_state.structure() {
        let next_menu = build_tray_menu(app, next_state).map_err(|error| error.to_string())?;
        tray.set_menu(Some(next_menu.menu.clone()))
            .map_err(|error| error.to_string())?;
        *built = next_menu;
        return Ok(());
    }

    if let (Some(current), Some(next)) = (&built.state.transfer, &next_state.transfer) {
        if current.summary != next.summary {
            built
                .progress_item
                .as_ref()
                .ok_or("Missing tray progress menu item")?
                .set_text(&next.summary)
                .map_err(|error| error.to_string())?;
        }
        if current.current_file != next.current_file {
            built
                .current_file_item
                .as_ref()
                .ok_or("Missing tray current-file menu item")?
                .set_text(&next.current_file)
                .map_err(|error| error.to_string())?;
        }
        if current.stop_label != next.stop_label {
            built
                .stop_item
                .as_ref()
                .ok_or("Missing tray stop menu item")?
                .set_text(next.stop_label)
                .map_err(|error| error.to_string())?;
        }
        if current.can_stop != next.can_stop {
            built
                .stop_item
                .as_ref()
                .ok_or("Missing tray stop menu item")?
                .set_enabled(next.can_stop)
                .map_err(|error| error.to_string())?;
        }
    }

    built.state = next_state;
    Ok(())
}

fn initial_tray_menu_state(app: &AppHandle, language: String) -> TrayMenuState {
    let storage_unavailable = app
        .try_state::<crate::AppState>()
        .and_then(|state| {
            state
                .db
                .try_lock()
                .ok()
                .map(|db| !db.storage_status().available)
        })
        .unwrap_or(false);
    TrayMenuState::from_current_progress(language, storage_unavailable)
}

// ═══════════════════════════════════════════════════════════════════
// Public entry point
// ═══════════════════════════════════════════════════════════════════

/// Start the tray system on non-macOS platforms using Tauri's built-in tray.
/// macOS uses the SwiftUI menu bar instead, so this entry point is excluded there.
#[cfg(not(target_os = "macos"))]
#[cfg_attr(test, allow(dead_code))]
pub fn start_tray(app_handle: AppHandle) {
    let mut menu = match create_tauri_tray(&app_handle) {
        Ok(menu) => menu,
        Err(error) => {
            log::error!("Could not create tray icon: {error}");
            return;
        }
    };
    let updater = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            if updater.tray_by_id(TRAY_ID).is_none() {
                break;
            }
            let Some((settings, db)) = updater
                .try_state::<crate::AppState>()
                .map(|state| (state.settings.clone(), state.db.clone()))
            else {
                break;
            };
            let language = settings.lock().await.language.clone();
            let storage_unavailable = !db.lock().await.storage_status().available;
            let next_state = TrayMenuState::from_current_progress(language, storage_unavailable);
            if let Err(error) = refresh_tray_menu(&updater, &mut menu, next_state) {
                log::debug!("Could not refresh tray transfer state: {error}");
            }
        }
    });
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
fn create_tauri_tray(app: &AppHandle) -> Result<BuiltTrayMenu<tauri::Wry>, String> {
    use tauri::{
        image::Image,
        tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    };

    let language = crate::crypto::Settings::load()
        .map(|settings| settings.language)
        .unwrap_or_else(|_| "en".into());
    let state = initial_tray_menu_state(app, language);
    let menu = build_tray_menu(app, state).map_err(|error| error.to_string())?;

    // The tray is rendered at roughly 16-24 physical pixels on Windows.
    // Feeding it the 512px application artwork makes the shell resample the
    // fine document outlines too aggressively and produces a visibly soft
    // icon.  Use the dedicated small asset so the shell starts from already
    // pixel-aligned edges.
    let icon = Image::from_bytes(include_bytes!("../icons/32x32.png")).unwrap_or_else(|error| {
        log::error!("Bundled tray icon is invalid: {error}");
        Image::new(&TRANSPARENT_TRAY_RGBA, 32, 32)
    });

    let _tray = TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .menu(&menu.menu)
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
            "stop_transfer" => {
                let Some(progress) = crate::api::get_file_progress() else {
                    return;
                };
                if let Ok(id) = crate::protocol::TransferId::from_hex(&progress.batch_id) {
                    if let Some(state) = app.try_state::<crate::AppState>() {
                        let sync = state.sync_engine.clone();
                        let pool = state.pool.clone();
                        let settings = state.settings.clone();
                        tauri::async_runtime::spawn(async move {
                            crate::commands::cancel_file_batch_impl(&sync, &pool, &settings, id)
                                .await;
                        });
                    }
                }
            }
            "quit" => {
                request_shutdown(app);
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
        .map_err(|error| error.to_string())?;

    info!("Tauri tray created (non-macOS)");
    Ok(menu)
}

#[cfg(target_os = "macos")]
#[allow(dead_code)]
fn create_tauri_tray(app: &AppHandle) -> Result<(), String> {
    // On macOS this is only called as a fallback when the Swift helper
    // fails to start, so we do create a Tauri tray.
    use tauri::{image::Image, tray::TrayIconBuilder};

    let language = crate::crypto::Settings::load()
        .map(|settings| settings.language)
        .unwrap_or_else(|_| "en".into());
    let state = initial_tray_menu_state(app, language);
    let menu = build_tray_menu(app, state).map_err(|error| error.to_string())?;

    let icon = Image::from_bytes(include_bytes!("../icons/icon.png")).unwrap_or_else(|error| {
        log::error!("Bundled tray icon is invalid: {error}");
        Image::new(&TRANSPARENT_TRAY_RGBA, 32, 32)
    });

    let _tray = TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .menu(&menu.menu)
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
            "quit" => request_shutdown(app),
            _ => {}
        })
        .build(app)
        .map_err(|error| error.to_string())?;

    info!("Tauri tray created (fallback)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{tray_labels, TrayLabels, TrayMenuState, TrayTransferState};
    use tauri::image::Image;

    fn transfer(summary: &str, current_file: &str, can_stop: bool) -> TrayTransferState {
        TrayTransferState {
            summary: summary.into(),
            current_file: current_file.into(),
            stop_label: "Stop transfer",
            can_stop,
        }
    }

    #[test]
    fn bundled_tray_icons_decode() {
        Image::from_bytes(include_bytes!("../icons/32x32.png"))
            .expect("decode bundled 32px tray icon");
        Image::from_bytes(include_bytes!("../icons/icon.png"))
            .expect("decode bundled application icon");
    }

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

    #[test]
    fn transfer_content_updates_keep_the_same_menu_structure() {
        let current = TrayMenuState {
            language: "en".into(),
            storage_unavailable: false,
            transfer: Some(transfer("1 / 3 files - 10%", "Phone  a.zip", true)),
        };
        let updated = TrayMenuState {
            language: "en".into(),
            storage_unavailable: false,
            transfer: Some(transfer("2 / 3 files - 75%", "Phone  b.zip", false)),
        };

        assert_ne!(current, updated);
        assert_eq!(current.structure(), updated.structure());
    }

    #[test]
    fn structural_tray_changes_require_a_menu_rebuild() {
        let idle = TrayMenuState {
            language: "en".into(),
            storage_unavailable: false,
            transfer: None,
        };
        let active = TrayMenuState {
            transfer: Some(transfer("0 / 1 files - 0%", "Phone  a.zip", true)),
            ..idle.clone()
        };
        let storage_unavailable = TrayMenuState {
            storage_unavailable: true,
            ..idle.clone()
        };
        let chinese = TrayMenuState {
            language: "zh-CN".into(),
            ..idle.clone()
        };

        assert_ne!(idle.structure(), active.structure());
        assert_ne!(idle.structure(), storage_unavailable.structure());
        assert_ne!(idle.structure(), chinese.structure());
    }
}
