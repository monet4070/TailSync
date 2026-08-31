mod api;
mod clipboard;
mod clipboard_change;
mod clipboard_file;
mod commands;
mod network;
mod preview_window;
mod sync_adapter;
mod tray;
mod updates;
mod window_lifecycle;

pub use tailsync_core::{
    crypto, db, history_classifier, identity, pairing, protocol, secure, sync,
};

use log::info;
use std::sync::{Arc, Mutex as StdMutex};
use tauri::Manager;
use tokio::sync::{watch, Mutex};

type BackgroundTasks = Arc<StdMutex<Vec<tauri::async_runtime::JoinHandle<()>>>>;

pub struct AppState {
    pub db: Arc<Mutex<db::HistoryDB>>,
    pub sync_engine: Arc<Mutex<sync::SyncEngine>>,
    pub settings: Arc<Mutex<crypto::Settings>>,
    pub identity: Arc<identity::DeviceIdentity>,
    pub pool: Arc<Mutex<network::ConnectionPool>>,
    pub pairing: Arc<pairing::PairingManager>,
    pub remote_invites: Arc<pairing::RemotePairingInviteManager>,
    pub pending_remote_pairing_link: Arc<StdMutex<Option<String>>>,
    pub shutdown: watch::Sender<bool>,
    /// The exact old root returned by the most recent successful migration.
    /// Cleanup is one-shot and cannot be redirected by an arbitrary IPC path.
    pub pending_storage_cleanup: Arc<Mutex<Option<std::path::PathBuf>>>,
}

fn track_task(tasks: &BackgroundTasks, task: tauri::async_runtime::JoinHandle<()>) {
    tasks
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(task);
}

fn should_prevent_implicit_exit(code: Option<i32>) -> bool {
    // Tauri uses no exit code when destroying the last window, and Some(code)
    // for explicit AppHandle::exit/restart requests.
    code.is_none()
}

async fn wait_for_shutdown(shutdown: &mut watch::Receiver<bool>) {
    if *shutdown.borrow() {
        return;
    }
    while shutdown.changed().await.is_ok() {
        if *shutdown.borrow() {
            return;
        }
    }
}

fn start_file_history_encryption_migration(
    shutdown: watch::Receiver<bool>,
) -> tauri::async_runtime::JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        const BATCH_SIZE: usize = 8;
        let mut after_id = 0_i64;
        let mut scanned = 0_usize;
        let mut migrated = 0_usize;
        let mut failed = 0_usize;

        loop {
            if *shutdown.borrow() {
                return;
            }
            let result = tauri::async_runtime::spawn_blocking(move || {
                db::HistoryDB::migrate_file_history_encryption_batch(after_id, BATCH_SIZE)
                    .map_err(|error| error.to_string())
            })
            .await;
            let batch = match result {
                Ok(Ok(batch)) => batch,
                Ok(Err(error)) => {
                    log::error!("File-history encryption migration failed: {error}");
                    return;
                }
                Err(error) => {
                    log::error!("File-history encryption worker stopped unexpectedly: {error}");
                    return;
                }
            };
            scanned += batch.scanned;
            migrated += batch.migrated;
            failed += batch.failed;
            if batch.complete {
                if scanned > 0 {
                    log::info!(
                        "File-history encryption scan completed: {scanned} checked, {migrated} migrated, {failed} pending"
                    );
                }
                return;
            }
            let Some(last_id) = batch.last_id else {
                log::error!("File-history encryption migration made no cursor progress");
                return;
            };
            after_id = last_id;
            tokio::task::yield_now().await;
        }
    })
}

async fn coordinate_shutdown(
    handle: tauri::AppHandle,
    pool: Arc<Mutex<network::ConnectionPool>>,
    mut shutdown: watch::Receiver<bool>,
    tasks: BackgroundTasks,
) {
    wait_for_shutdown(&mut shutdown).await;
    info!("Application shutdown coordinator started");
    let close_connections = async {
        if tokio::time::timeout(std::time::Duration::from_millis(250), async {
            pool.lock().await.disconnect_all();
        })
        .await
        .is_err()
        {
            log::warn!("Timed out while closing peer connections");
        }
    };
    tokio::join!(
        close_connections,
        stop_background_tasks(tasks, std::time::Duration::from_millis(750))
    );
    info!("Background services stopped");
    handle.exit(0);
}

async fn stop_background_tasks(tasks: BackgroundTasks, timeout: std::time::Duration) {
    let running = tasks
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .drain(..)
        .collect::<Vec<_>>();
    let deadline = tokio::time::Instant::now() + timeout;
    for mut task in running {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            task.abort();
            let _ = task.await;
            continue;
        }
        match tokio::time::timeout(remaining, &mut task).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => log::debug!("Background task ended with an error: {error}"),
            Err(_) => {
                task.abort();
                let _ = task.await;
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn start_parent_monitor(
    shutdown: tokio::sync::watch::Sender<bool>,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) -> Option<tauri::async_runtime::JoinHandle<()>> {
    let parent_pid = std::env::var("TAILSYNC_PARENT_PID")
        .ok()
        .and_then(|value| value.parse::<libc::pid_t>().ok())?;
    Some(tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {}
                _ = wait_for_shutdown(&mut shutdown_rx) => return,
            }
            let current_parent = unsafe { libc::getppid() };
            if current_parent != parent_pid {
                log::info!("TailSync UI parent exited; stopping daemon");
                let _ = shutdown.send(true);
                break;
            }
        }
    }))
}

#[cfg(not(target_os = "macos"))]
fn start_parent_monitor(
    _shutdown: tokio::sync::watch::Sender<bool>,
    _shutdown_rx: tokio::sync::watch::Receiver<bool>,
) -> Option<tauri::async_runtime::JoinHandle<()>> {
    None
}

#[cfg(target_os = "windows")]
fn start_background_notifications(
    app: tauri::AppHandle,
    db: Arc<Mutex<db::HistoryDB>>,
    settings: Arc<Mutex<crypto::Settings>>,
    mut shutdown: watch::Receiver<bool>,
) -> tauri::async_runtime::JoinHandle<()> {
    use tauri_winrt_notification::Toast;

    tauri::async_runtime::spawn(async move {
        let app_id = app.config().identifier.clone();
        let mut last_seen_id = db
            .lock()
            .await
            .get_all_filtered(None, None, None, None, 1, 0)
            .ok()
            .and_then(|entries| entries.first().map(|entry| entry.id))
            .unwrap_or_default();
        let mut last_version = api::get_clipboard_version();
        let mut last_revision = api::get_runtime_revision();

        loop {
            tokio::select! {
                revision = api::wait_for_runtime_revision(
                    last_revision,
                    std::time::Duration::from_secs(15),
                ) => {
                    last_revision = revision;
                }
                _ = wait_for_shutdown(&mut shutdown) => return,
            }
            let version = api::get_clipboard_version();
            if version == last_version {
                continue;
            }
            last_version = version;

            let latest = db
                .lock()
                .await
                .get_all_filtered(None, None, None, None, 1, 0)
                .ok()
                .and_then(|entries| entries.into_iter().next());
            let Some(entry) = latest else {
                continue;
            };
            if entry.id <= last_seen_id {
                continue;
            }
            last_seen_id = entry.id;
            if entry.source_peer == "self"
                || entry.batch_id.is_some()
                || !settings.lock().await.notifications_enabled
            {
                continue;
            }

            let body = match entry.entry_type.as_str() {
                "image" => format!("Image received from {}", entry.source_peer),
                "file" => format!("{} received from {}", entry.description, entry.source_peer),
                _ => entry.description,
            };
            let notification = Toast::new(&app_id).title("TailSync").text2(&body);
            if let Err(error) = notification.show() {
                log::debug!("Could not show background notification: {error}");
            }
        }
    })
}

fn start_storage_monitor(
    database: Arc<Mutex<db::HistoryDB>>,
    app: tauri::AppHandle,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) -> tauri::async_runtime::JoinHandle<()> {
    use tauri_plugin_notification::NotificationExt;
    tauri::async_runtime::spawn(async move {
        let mut warned = false;
        loop {
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {}
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() { return; }
                    continue;
                }
            }
            let mut history = database.lock().await;
            if history.storage_status().available {
                warned = false;
                continue;
            }
            history.mark_storage_unavailable();
            match history.reopen_configured_storage() {
                Ok(()) => {
                    warned = false;
                    let _ = app
                        .notification()
                        .builder()
                        .title("TailSync")
                        .body("TailSync storage is available again. File transfer resumed.")
                        .show();
                }
                Err(error) if !warned => {
                    warned = true;
                    log::warn!("TailSync storage unavailable: {error}");
                    let _ = app
                        .notification()
                        .builder()
                        .title("TailSync")
                        .body("TailSync storage is unavailable. File transfer is paused.")
                        .show();
                }
                Err(_) => {}
            }
        }
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> i32 {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    match run_app() {
        Ok(()) => 0,
        Err(error) => report_startup_failure(&db::get_data_dir(), error.as_ref()),
    }
}

/// Record a fatal startup failure and return the process exit code.
///
/// The message goes to the logger and stderr (visible in `tauri:dev`), and
/// is appended to `{data dir}/startup-error.log` so release builds — which
/// have no console — still leave a reliable, documented failure trace.
fn report_startup_failure(data_dir: &std::path::Path, error: &dyn std::error::Error) -> i32 {
    let message = format!("TailSync failed to start: {error}");
    log::error!("{message}");
    eprintln!("{message}");
    if let Err(log_error) = write_startup_error_log(data_dir, &message) {
        eprintln!("Could not write startup error log: {log_error}");
    }
    1
}

/// Append one startup-failure line to `{data dir}/startup-error.log`,
/// creating the data directory when needed.
fn write_startup_error_log(data_dir: &std::path::Path, message: &str) -> std::io::Result<()> {
    use std::io::Write;
    std::fs::create_dir_all(data_dir)?;
    let path = data_dir.join("startup-error.log");
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(
        file,
        "[{}] {message}",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
    )
}

fn run_app() -> Result<(), Box<dyn std::error::Error>> {
    info!("TailSync v2 starting...");

    // Encryption is foundational: never start with a new key after an existing
    // key could not be read, because that would make history permanently
    // undecryptable.
    crypto::initialize()?;
    let loaded_settings = crypto::Settings::load()?;
    let storage_available = db::configure_storage_dir(
        loaded_settings
            .storage_root
            .as_deref()
            .map(std::path::Path::new),
    )
    .map(|_| true)
    .unwrap_or_else(|error| {
        log::error!("Configured storage is unavailable: {error}");
        false
    });
    sync::cleanup_expired_transfers();

    let mut history_db = if storage_available {
        db::HistoryDB::new()?
    } else {
        db::HistoryDB::new_unavailable()?
    };
    history_db.set_storage_quota(loaded_settings.storage_quota_bytes);
    let db = Arc::new(Mutex::new(history_db));
    let db_for_classification = db.clone();
    let classification_task = tauri::async_runtime::spawn(async move {
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
    let sync_engine = Arc::new(Mutex::new(sync::SyncEngine::new()));
    let settings = Arc::new(Mutex::new(loaded_settings));
    let identity = Arc::new(identity::DeviceIdentity::load_or_create()?);
    let api_token = api::load_api_token().map_err(std::io::Error::other)?;
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let shutdown_for_parent = shutdown_tx.clone();
    let background_tasks: BackgroundTasks = Arc::new(StdMutex::new(Vec::new()));
    track_task(&background_tasks, classification_task);
    track_task(
        &background_tasks,
        start_file_history_encryption_migration(shutdown_rx.clone()),
    );
    db.blocking_lock()
        .set_max_history(settings.blocking_lock().history_limit as i64);
    let pool = Arc::new(Mutex::new(network::ConnectionPool::new(
        identity.clone(),
        settings.clone(),
    )));
    let pairing = pairing::PairingManager::new(settings.clone(), identity.clone());
    let remote_invites = Arc::new(pairing::RemotePairingInviteManager::new());
    let pending_remote_pairing_link = Arc::new(StdMutex::new(None));
    let pending_storage_cleanup = Arc::new(Mutex::new(None));
    let settings_for_monitor = settings.clone();
    #[cfg(target_os = "windows")]
    let settings_for_notifications = settings.clone();
    let settings_for_server = settings.clone();
    let settings_for_iroh = settings.clone();
    let settings_for_discovery = settings.clone();
    let identity_for_server = identity.clone();
    let identity_for_iroh = identity.clone();
    let identity_for_discovery = identity.clone();

    // Start JSON API server
    let api_state = Arc::new(api::ApiState {
        db: db.clone(),
        sync_engine: sync_engine.clone(),
        settings: settings.clone(),
        identity: identity.clone(),
        pool: pool.clone(),
        pairing: pairing.clone(),
        remote_invites: remote_invites.clone(),
        token: api_token,
        shutdown: shutdown_tx.clone(),
        imports: Mutex::new(api::ImportRegistry::default()),
        pending_storage_cleanup: pending_storage_cleanup.clone(),
    });
    let api_shutdown = shutdown_rx.clone();
    let api_task = tauri::async_runtime::spawn(async move {
        if let Err(e) = api::start(api_state, api_shutdown).await {
            log::error!("API server error: {}", e);
        }
    });
    track_task(&background_tasks, api_task);

    let db_for_setup = db.clone();
    let sync_for_setup = sync_engine.clone();
    let pool_for_setup = pool.clone();
    let tasks_for_setup = background_tasks.clone();
    let shutdown_for_setup = shutdown_rx.clone();
    let shutdown_for_state = shutdown_tx.clone();

    let app = tauri::Builder::default()
        // This must remain the first plugin so secondary launches are rejected
        // before app setup creates another tray or registers global shortcuts.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            let handle = app.clone();
            #[cfg(not(target_os = "windows"))]
            let _ = &args;
            #[cfg(target_os = "windows")]
            if let Some(link) = commands::remote_pairing_link_from_args(&args) {
                #[cfg(target_os = "windows")]
                if let Err(error) = commands::queue_remote_pairing_link(&handle, link) {
                    log::debug!(
                        "Ignoring invalid TailSync deep link from a repeated launch: {error}"
                    );
                }
                #[cfg(target_os = "windows")]
                return;
            }
            tauri::async_runtime::spawn(async move {
                if let Err(error) = commands::open_history_window(handle).await {
                    log::warn!("Could not focus TailSync after a repeated launch: {error}");
                }
            });
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(updates::plugin_builder().build())
        .setup(move |app| {
            let handle = app.handle().clone();
            updates::register_app_handle(handle.clone());
            #[cfg(target_os = "windows")]
            updates::spawn_automatic_update_check(handle.clone());
            if let Some(task) =
                start_parent_monitor(shutdown_for_parent, shutdown_for_setup.clone())
            {
                track_task(&tasks_for_setup, task);
            }

            // Inject platform clipboard, progress, and file-history services.
            {
                let mut sync = sync_for_setup.blocking_lock();
                sync.set_platform(Arc::new(sync_adapter::TauriSyncPlatform::new(
                    handle.clone(),
                    db_for_setup.clone(),
                    settings.clone(),
                )));
            }

            // The macOS SwiftUI app owns the native menu bar. Other platforms
            // use the shared Tauri tray implementation.
            let state = AppState {
                db: db_for_setup.clone(),
                sync_engine: sync_for_setup.clone(),
                settings,
                identity,
                pool: pool_for_setup.clone(),
                pairing: pairing.clone(),
                remote_invites: remote_invites.clone(),
                pending_remote_pairing_link: pending_remote_pairing_link.clone(),
                shutdown: shutdown_for_state,
                pending_storage_cleanup,
            };
            let initial_shortcuts = state.settings.blocking_lock().clone();
            app.manage(state);
            #[cfg(target_os = "windows")]
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                if let Err(error) = app.deep_link().register_all() {
                    log::debug!("Could not register the TailSync deep-link scheme: {error}");
                }
                let deep_link_handle = handle.clone();
                app.deep_link().on_open_url(move |event| {
                    for url in event.urls() {
                        if let Err(error) =
                            commands::queue_remote_pairing_link(&deep_link_handle, url.as_str())
                        {
                            log::debug!("Ignoring invalid TailSync deep link: {error}");
                        }
                    }
                });
                // On Windows the initial protocol launch is exposed through
                // `get_current`, while later launches arrive as events. Read
                // the current value after the listener is installed so a
                // cold-start invite is not lost before the settings window
                // exists.
                if let Ok(Some(urls)) = app.deep_link().get_current() {
                    for url in urls {
                        if let Err(error) =
                            commands::queue_remote_pairing_link(&handle, url.as_str())
                        {
                            log::debug!("Ignoring invalid TailSync deep link: {error}");
                        }
                    }
                }
            }
            app.manage(preview_window::PreviewWindowController::default());
            app.manage(window_lifecycle::TransientWindowController::default());
            if let Err(error) = commands::register_saved_shortcuts(&handle, &initial_shortcuts) {
                log::warn!("Could not register saved global shortcuts: {error}");
            }
            #[cfg(all(not(target_os = "macos"), not(test)))]
            tray::start_tray(handle.clone());
            if std::env::var_os("TAILSYNC_OPEN_SETTINGS_ON_START").is_some() {
                let settings_handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = commands::open_settings_window(settings_handle).await {
                        log::warn!("Could not open settings test window: {error}");
                    }
                });
            }
            if std::env::var_os("TAILSYNC_OPEN_HISTORY_ON_START").is_some() {
                let history_handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = commands::open_history_window(history_handle).await {
                        log::warn!("Could not open history test window: {error}");
                    }
                });
            }
            let pool_for_health = pool_for_setup.clone();

            // Start clipboard monitor (file → text → image)
            let clipboard_task = clipboard::start_monitor(
                handle.clone(),
                db_for_setup.clone(),
                sync_for_setup.clone(),
                pool_for_setup.clone(),
                settings_for_monitor,
                shutdown_for_setup.clone(),
            );
            track_task(&tasks_for_setup, clipboard_task);
            #[cfg(target_os = "windows")]
            let notification_task = start_background_notifications(
                handle.clone(),
                db_for_setup.clone(),
                settings_for_notifications,
                shutdown_for_setup.clone(),
            );
            #[cfg(target_os = "windows")]
            track_task(&tasks_for_setup, notification_task);

            // Start P2P network server
            let db_for_storage_monitor = db_for_setup.clone();
            let db_for_iroh = db_for_setup.clone();
            let sync_for_iroh = sync_for_setup.clone();
            let pairing_for_server = pairing.clone();
            let pairing_for_iroh = pairing.clone();
            let remote_invites_for_iroh = remote_invites.clone();
            let server_shutdown = shutdown_for_setup.clone();
            let server_task = tauri::async_runtime::spawn(async move {
                if let Err(e) = network::start_server(
                    sync_for_setup,
                    db_for_setup,
                    settings_for_server,
                    identity_for_server,
                    pairing_for_server,
                    server_shutdown,
                )
                .await
                {
                    log::error!("Network server error: {}", e);
                }
            });
            track_task(&tasks_for_setup, server_task);
            let iroh_shutdown = shutdown_for_setup.clone();
            let iroh_task = tauri::async_runtime::spawn(async move {
                if let Err(error) = network::start_iroh_server(
                    sync_for_iroh,
                    db_for_iroh,
                    settings_for_iroh,
                    identity_for_iroh,
                    pairing_for_iroh,
                    remote_invites_for_iroh,
                    iroh_shutdown,
                )
                .await
                {
                    log::error!("Iroh server error: {error}");
                }
            });
            track_task(&tasks_for_setup, iroh_task);
            let discovery_task = tauri::async_runtime::spawn(network::start_discovery_responder(
                identity_for_discovery,
                shutdown_for_setup.clone(),
            ));
            track_task(&tasks_for_setup, discovery_task);
            let health_task = tauri::async_runtime::spawn(network::peer_health_monitor(
                settings_for_discovery,
                pool_for_health,
                handle.clone(),
                shutdown_for_setup.clone(),
            ));
            track_task(&tasks_for_setup, health_task);
            track_task(
                &tasks_for_setup,
                start_storage_monitor(
                    db_for_storage_monitor,
                    handle.clone(),
                    shutdown_for_setup.clone(),
                ),
            );

            tauri::async_runtime::spawn(coordinate_shutdown(
                handle.clone(),
                pool_for_setup,
                shutdown_for_setup,
                tasks_for_setup,
            ));

            info!("TailSync v2 initialized successfully");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_history,
            commands::get_history_page,
            commands::get_history_capabilities,
            commands::get_migration_diagnostics,
            commands::search_history,
            commands::delete_entry,
            commands::set_history_favorite,
            commands::delete_favorite_entry,
            commands::clear_history,
            commands::restore_entry,
            commands::get_peers,
            commands::refresh_peers,
            commands::test_connection,
            commands::toggle_peer,
            commands::get_sync_state,
            commands::set_sync_enabled,
            commands::toggle_sync,
            commands::suspend_sync_shortcut,
            commands::resume_sync_shortcut,
            commands::set_sync_shortcut,
            commands::set_history_shortcut,
            commands::trust_peer,
            commands::forget_peer,
            commands::enable_pairing,
            commands::get_pairing_status,
            commands::start_pairing,
            commands::confirm_pairing,
            commands::cancel_pairing,
            commands::create_remote_pairing_invite,
            commands::inspect_remote_pairing_link,
            commands::start_remote_pairing,
            commands::get_remote_pairing_invite_status,
            commands::cancel_remote_pairing_invite,
            commands::take_pending_remote_pairing_link,
            commands::get_settings,
            commands::update_settings,
            commands::open_history_window,
            commands::open_favorites_window,
            commands::open_settings_window,
            commands::close_history_window,
            commands::close_favorites_window,
            commands::close_settings_window,
            preview_window::open_preview_window,
            preview_window::get_preview_window_request,
            preview_window::close_preview_window,
            preview_window::sync_preview_window_minimized,
            commands::get_image_data,
            commands::get_preview,
            commands::validate_theme,
            commands::install_theme,
            commands::update_theme,
            commands::rollback_theme,
            commands::delete_theme_v2,
            commands::list_themes_v2,
            commands::get_local_theme_settings,
            commands::set_local_theme_settings,
            commands::resolve_theme,
            commands::get_theme_asset,
            commands::get_theme_asset_slot,
            commands::preview_theme_asset_slot,
            commands::get_file_progress,
            commands::cancel_file_batch,
            commands::get_storage_status,
            commands::change_storage_location,
            commands::set_history_pinned,
            commands::delete_old_storage,
            commands::restore_file_batch,
            commands::get_version,
            commands::wait_runtime_snapshot,
            commands::get_sync_warning,
            commands::get_update_status,
            commands::check_for_update,
            commands::install_update,
        ])
        .build(tauri::generate_context!())?;

    app.run(|_handle, event| {
        if let tauri::RunEvent::ExitRequested { code, api, .. } = event {
            if should_prevent_implicit_exit(code) {
                log::debug!("Keeping TailSync resident after the last window was released");
                api.prevent_exit();
            }
        }
    });
    Ok(())
}

#[cfg(test)]
mod startup_failure_tests {
    use super::{report_startup_failure, write_startup_error_log};

    fn temp_dir(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "tailsync-win-{label}-{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        ))
    }

    #[test]
    fn startup_failure_returns_nonzero_and_records_log() {
        let dir = temp_dir("startup-failure");
        let error = std::io::Error::other("legacy theme field");
        let code = report_startup_failure(&dir, &error);
        assert_eq!(code, 1, "a failed startup must exit with a non-zero code");
        let log = std::fs::read_to_string(dir.join("startup-error.log")).unwrap();
        assert!(log.contains("TailSync failed to start: legacy theme field"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn startup_error_log_appends_repeated_failures() {
        let dir = temp_dir("startup-append");
        write_startup_error_log(&dir, "first failure").unwrap();
        write_startup_error_log(&dir, "second failure").unwrap();
        let log = std::fs::read_to_string(dir.join("startup-error.log")).unwrap();
        assert!(log.contains("first failure"));
        assert!(log.contains("second failure"));
        assert_eq!(log.lines().count(), 2);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(test)]
mod shutdown_tests {
    use super::{should_prevent_implicit_exit, stop_background_tasks, track_task, BackgroundTasks};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex as StdMutex,
    };

    struct DropSignal(Arc<AtomicBool>);

    impl Drop for DropSignal {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    fn tasks() -> BackgroundTasks {
        Arc::new(StdMutex::new(Vec::new()))
    }

    #[test]
    fn only_implicit_last_window_exit_is_prevented() {
        assert!(should_prevent_implicit_exit(None));
        assert!(!should_prevent_implicit_exit(Some(0)));
        assert!(!should_prevent_implicit_exit(Some(1)));
        assert!(!should_prevent_implicit_exit(Some(
            tauri::RESTART_EXIT_CODE
        )));
    }

    #[tokio::test]
    async fn completed_shutdown_tasks_are_not_polled_twice() {
        let tasks = tasks();
        track_task(
            &tasks,
            tauri::async_runtime::JoinHandle::Tokio(tokio::spawn(async {})),
        );

        stop_background_tasks(tasks.clone(), std::time::Duration::from_secs(1)).await;

        assert!(tasks.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn overdue_shutdown_tasks_are_aborted() {
        let tasks = tasks();
        let dropped = Arc::new(AtomicBool::new(false));
        let dropped_for_task = dropped.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        track_task(
            &tasks,
            tauri::async_runtime::JoinHandle::Tokio(tokio::spawn(async move {
                let _drop_signal = DropSignal(dropped_for_task);
                let _ = started_tx.send(());
                std::future::pending::<()>().await;
            })),
        );
        started_rx.await.expect("background task should start");

        stop_background_tasks(tasks, std::time::Duration::from_millis(1)).await;

        assert!(dropped.load(Ordering::SeqCst));
    }
}
