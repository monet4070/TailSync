mod api;
mod clipboard;
mod clipboard_change;
mod clipboard_file;
mod commands;
mod network;
mod sync_adapter;
mod tray;

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
    pub shutdown: watch::Sender<bool>,
}

fn track_task(tasks: &BackgroundTasks, task: tauri::async_runtime::JoinHandle<()>) {
    tasks
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(task);
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
    if tokio::time::timeout(std::time::Duration::from_secs(2), async {
        pool.lock().await.disconnect_all();
    })
    .await
    .is_err()
    {
        log::warn!("Timed out while closing peer connections");
    }

    stop_background_tasks(tasks, std::time::Duration::from_secs(3)).await;
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
    let Some(parent_pid) = std::env::var("TAILSYNC_PARENT_PID")
        .ok()
        .and_then(|value| value.parse::<libc::pid_t>().ok())
    else {
        return None;
    };
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
fn prepare_notification_icon() -> Option<std::path::PathBuf> {
    const ICON_BYTES: &[u8] = include_bytes!("../icons/128x128.png");

    let path = db::get_data_dir().join("notification-logo-v1.png");
    if std::fs::read(&path).is_ok_and(|existing| existing == ICON_BYTES) {
        return Some(path);
    }
    if let Err(error) = std::fs::write(&path, ICON_BYTES) {
        log::warn!("Could not prepare the notification logo: {error}");
        return None;
    }
    Some(path)
}

#[cfg(target_os = "windows")]
fn start_background_notifications(
    app: tauri::AppHandle,
    db: Arc<Mutex<db::HistoryDB>>,
    settings: Arc<Mutex<crypto::Settings>>,
    mut shutdown: watch::Receiver<bool>,
) -> tauri::async_runtime::JoinHandle<()> {
    use tauri_winrt_notification::{IconCrop, Toast};

    tauri::async_runtime::spawn(async move {
        let app_id = app.config().identifier.clone();
        let notification_icon = prepare_notification_icon();
        let mut last_seen_id = db
            .lock()
            .await
            .get_all_filtered(None, None, None, None, 1, 0)
            .ok()
            .and_then(|entries| entries.first().map(|entry| entry.id))
            .unwrap_or_default();
        let mut last_version = api::get_clipboard_version();

        loop {
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_millis(500)) => {}
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
            if entry.source_peer == "self" || !settings.lock().await.notifications_enabled {
                continue;
            }

            let body = match entry.entry_type.as_str() {
                "image" => format!("Image received from {}", entry.source_peer),
                "file" => format!("{} received from {}", entry.description, entry.source_peer),
                _ => entry.description,
            };
            let mut notification = Toast::new(&app_id).title("TailSync").text2(&body);
            if let Some(icon) = notification_icon.as_deref() {
                notification = notification.icon(icon, IconCrop::Square, "TailSync");
            }
            if let Err(error) = notification.show() {
                log::debug!("Could not show background notification: {error}");
            }
        }
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    if let Err(error) = run_app() {
        log::error!("TailSync failed to start: {error}");
        eprintln!("TailSync failed to start: {error}");
    }
}

fn run_app() -> Result<(), Box<dyn std::error::Error>> {
    info!("TailSync v2 starting...");

    // Encryption is foundational: never start with a new key after an existing
    // key could not be read, because that would make history permanently
    // undecryptable.
    crypto::initialize()?;
    let loaded_settings = crypto::Settings::load()?;
    sync::cleanup_expired_transfers();

    let db = Arc::new(Mutex::new(db::HistoryDB::new()?));
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
    let settings_for_monitor = settings.clone();
    let settings_for_notifications = settings.clone();
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
        token: api_token,
        shutdown: shutdown_tx.clone(),
        imports: Mutex::new(api::ImportRegistry::default()),
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

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(move |app| {
            let handle = app.handle().clone();
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
                shutdown: shutdown_for_state,
            };
            app.manage(state);
            #[cfg(all(not(target_os = "macos"), not(test)))]
            tray::start_tray(handle.clone());
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
            let server_shutdown = shutdown_for_setup.clone();
            let server_task = tauri::async_runtime::spawn(async move {
                if let Err(e) = network::start_server(
                    sync_for_setup,
                    db_for_setup,
                    settings_for_server,
                    identity_for_server,
                    pairing,
                    server_shutdown,
                )
                .await
                {
                    log::error!("Network server error: {}", e);
                }
            });
            track_task(&tasks_for_setup, server_task);
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
            commands::clear_history,
            commands::restore_entry,
            commands::get_peers,
            commands::refresh_peers,
            commands::test_connection,
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
        .run(tauri::generate_context!())?;
    Ok(())
}

#[cfg(test)]
mod shutdown_tests {
    use super::{stop_background_tasks, track_task, BackgroundTasks};
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
