mod api;
mod clipboard;
mod clipboard_change;
mod clipboard_file;
mod commands;
mod crypto;
mod db;
mod history_classifier;
mod identity;
mod network;
mod pairing;
mod protocol;
mod sync;
#[cfg(not(test))]
mod tray;

use log::info;
use std::sync::Arc;
use tauri::Manager;
use tokio::sync::Mutex;

pub struct AppState {
    pub db: Arc<Mutex<db::HistoryDB>>,
    pub sync_engine: Arc<Mutex<sync::SyncEngine>>,
    pub settings: Arc<Mutex<crypto::Settings>>,
    pub identity: Arc<identity::DeviceIdentity>,
    pub pool: Arc<Mutex<network::ConnectionPool>>,
    pub pairing: Arc<pairing::PairingManager>,
}

#[cfg(target_os = "macos")]
fn start_parent_monitor() {
    let Some(parent_pid) = std::env::var("TAILSYNC_PARENT_PID")
        .ok()
        .and_then(|value| value.parse::<libc::pid_t>().ok())
    else {
        return;
    };
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            let current_parent = unsafe { libc::getppid() };
            if current_parent != parent_pid {
                log::info!("TailSync UI parent exited; stopping daemon");
                std::process::exit(0);
            }
        }
    });
}

#[cfg(not(target_os = "macos"))]
fn start_parent_monitor() {}

#[cfg(target_os = "macos")]
fn hide_bundled_daemon_from_dock() {
    if std::env::var_os("TAILSYNC_PARENT_PID").is_none() {
        return;
    }
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

    let marker =
        MainThreadMarker::new().expect("TailSync must initialize AppKit on the main thread");
    let app = NSApplication::sharedApplication(marker);
    if !app.setActivationPolicy(NSApplicationActivationPolicy::Accessory) {
        eprintln!("TailSync daemon could not switch to accessory activation policy");
    }
}

#[cfg(not(target_os = "macos"))]
fn hide_bundled_daemon_from_dock() {}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    hide_bundled_daemon_from_dock();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    info!("TailSync v2 starting...");
    start_parent_monitor();

    // Pre-initialize the encryption key so Keychain access is requested at startup.
    let history_key_ready = match crypto::get_dek() {
        Ok(_) => true,
        Err(error) => {
            log::error!("Failed to initialize encryption key: {error}");
            false
        }
    };
    sync::cleanup_expired_transfers();

    let db = Arc::new(Mutex::new(
        db::HistoryDB::new().expect("Failed to initialize database"),
    ));
    if history_key_ready {
        let db_for_classification = db.clone();
        tauri::async_runtime::spawn(async move {
            const MAX_BACKFILL_RETRIES: u8 = 3;
            let mut total = 0_usize;
            let mut consecutive_failures = 0_u8;
            loop {
                let result = {
                    let mut db = db_for_classification.lock().await;
                    db.backfill_classifications(50)
                        .map_err(|error| error.to_string())
                };
                let processed = match result {
                    Ok(processed) => {
                        consecutive_failures = 0;
                        processed
                    }
                    Err(error) if consecutive_failures < MAX_BACKFILL_RETRIES => {
                        consecutive_failures += 1;
                        let delay_ms = 250 * u64::from(consecutive_failures);
                        log::warn!(
                            "History classification backfill failed; retrying {consecutive_failures}/{MAX_BACKFILL_RETRIES} in {delay_ms} ms: {error}"
                        );
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                        continue;
                    }
                    Err(error) => {
                        log::error!("History classification backfill failed: {error}");
                        break;
                    }
                };
                if processed == 0 {
                    break;
                }
                total += processed;
                tokio::task::yield_now().await;
            }
            if total > 0 {
                api::bump_clipboard_version();
                log::info!("Classified {total} existing history entries");
            }
        });
    }
    let sync_engine = Arc::new(Mutex::new(sync::SyncEngine::new()));
    let settings = Arc::new(Mutex::new(crypto::Settings::load().unwrap_or_default()));
    let identity = Arc::new(
        identity::DeviceIdentity::load_or_create().expect("Failed to initialize device identity"),
    );
    db.blocking_lock()
        .set_max_history(settings.blocking_lock().history_limit as i64);
    let pool = Arc::new(Mutex::new(network::ConnectionPool::new(
        identity.clone(),
        settings.clone(),
    )));
    let pairing = pairing::PairingManager::new(settings.clone(), identity.clone());
    let settings_for_monitor = settings.clone();
    let settings_for_server = settings.clone();
    let settings_for_discovery = settings.clone();
    let identity_for_server = identity.clone();
    let identity_for_discovery = identity.clone();

    // Start JSON API server
    let api_state = Arc::new(api::ApiState {
        db: db.clone(),
        sync_engine: sync_engine.clone(),
        settings: settings.clone(),
        identity: identity.clone(),
        pool: pool.clone(),
        pairing: pairing.clone(),
    });
    tauri::async_runtime::spawn(async move {
        if let Err(e) = api::start(api_state).await {
            log::error!("API server error: {}", e);
        }
    });

    let db_for_setup = db.clone();
    let sync_for_setup = sync_engine.clone();
    let pool_for_setup = pool.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(move |app| {
            hide_bundled_daemon_from_dock();
            let handle = app.handle().clone();

            // Inject AppHandle + DB into sync engine
            {
                let mut sync = sync_for_setup.blocking_lock();
                sync.app_handle = Some(handle.clone());
                sync.db = Some(db_for_setup.clone());
            }

            // The macOS SwiftUI app owns the native menu bar. Other platforms
            // use the shared Tauri tray implementation.
            #[cfg(all(not(target_os = "macos"), not(test)))]
            tray::start_tray(handle.clone());

            let state = AppState {
                db: db_for_setup.clone(),
                sync_engine: sync_for_setup.clone(),
                settings,
                identity,
                pool: pool_for_setup.clone(),
                pairing: pairing.clone(),
            };
            app.manage(state);

            // Start clipboard monitor (file → text → image)
            clipboard::start_monitor(
                handle.clone(),
                db_for_setup.clone(),
                sync_for_setup.clone(),
                pool_for_setup,
                settings_for_monitor,
            );

            // Start P2P network server
            tauri::async_runtime::spawn(async move {
                if let Err(e) = network::start_server(
                    sync_for_setup,
                    db_for_setup,
                    settings_for_server,
                    identity_for_server,
                    pairing,
                )
                .await
                {
                    log::error!("Network server error: {}", e);
                }
            });
            tauri::async_runtime::spawn(network::start_discovery_responder(identity_for_discovery));
            tauri::async_runtime::spawn(network::peer_cache_refresh_loop(
                settings_for_discovery,
                handle.clone(),
            ));

            info!("TailSync v2 initialized successfully");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_history,
            commands::search_history,
            commands::delete_entry,
            commands::clear_history,
            commands::restore_entry,
            commands::get_peers,
            commands::refresh_peers,
            commands::toggle_peer,
            commands::trust_peer,
            commands::forget_peer,
            commands::enable_pairing,
            commands::get_pairing_status,
            commands::start_pairing,
            commands::confirm_pairing,
            commands::cancel_pairing,
            commands::get_settings,
            commands::update_settings,
            commands::open_history_window,
            commands::open_settings_window,
            commands::get_image_data,
            commands::get_file_progress,
            commands::get_version,
        ])
        .run(tauri::generate_context!())
        .expect("error while running TailSync");
}
