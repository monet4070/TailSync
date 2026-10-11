use std::sync::Arc;
use tokio::sync::{watch, Mutex};

pub use tailsync_runtime::automatic_image_archive::AutomaticImageArchive;

pub fn start(
    database: Arc<Mutex<crate::db::HistoryDB>>,
    enabled: bool,
    pool: Arc<Mutex<crate::network::ConnectionPool>>,
    settings: Arc<Mutex<crate::crypto::Settings>>,
    shutdown: watch::Receiver<bool>,
) -> (
    Arc<AutomaticImageArchive>,
    tauri::async_runtime::JoinHandle<()>,
) {
    let outgoing = Arc::new(tailsync_runtime::archive_sync::ArchiveSyncQueue::new(
        database.clone(),
    ));
    let on_commit = outgoing.clone();
    let queue = Arc::new(AutomaticImageArchive::with_commit_hook(
        database,
        enabled,
        Arc::new(tailsync_core::composite::resource_archive::ReqwestTransport::new()),
        Arc::new(move |commit| {
            crate::api::bump_clipboard_version();
            if on_commit.enqueue(commit) == tailsync_runtime::archive_sync::EnqueueResult::Full {
                log::warn!("Archive update queue full; the local images remain saved");
            }
        }),
    ));
    let worker_queue = queue.clone();
    let worker = tauri::async_runtime::spawn(async move {
        tokio::join!(
            worker_queue.run(shutdown.clone()),
            run_outgoing(outgoing, pool, settings, shutdown)
        );
    });
    (queue, worker)
}

async fn run_outgoing(
    outgoing: Arc<tailsync_runtime::archive_sync::ArchiveSyncQueue>,
    pool: Arc<Mutex<crate::network::ConnectionPool>>,
    settings: Arc<Mutex<crate::crypto::Settings>>,
    mut shutdown: watch::Receiver<bool>,
) {
    loop {
        if *shutdown.borrow() {
            return;
        }
        tokio::select! {
            biased;
            _ = shutdown.changed() => { if *shutdown.borrow() || shutdown.has_changed().is_err() { return; } }
            _ = async {
                match outgoing.next().await {
                    Ok(Some(dispatch)) => send_update(&outgoing, &pool, &settings, dispatch).await,
                    Ok(None) => {},
                    Err(error) => log::warn!("Archive update export skipped: {error}"),
                }
            } => {}
        }
    }
}

async fn send_update(
    outgoing: &tailsync_runtime::archive_sync::ArchiveSyncQueue,
    pool: &Arc<Mutex<crate::network::ConnectionPool>>,
    settings: &Arc<Mutex<crate::crypto::Settings>>,
    dispatch: tailsync_runtime::archive_sync::ArchiveDispatch,
) {
    let snapshot = settings.lock().await.clone();
    if !snapshot.sync_enabled {
        return;
    }
    let mode = snapshot.connection_mode.clone();
    let discovered = crate::network::cached_discover_peers(&mode)
        .await
        .map(|(_, peers)| peers)
        .unwrap_or_default();
    let peers = crate::network::merge_paired_peers(&snapshot, &mode, discovered);
    for peer in peers {
        if !peer.enabled || !peer.trusted || !settings.lock().await.sync_enabled {
            continue;
        }
        let current = outgoing.current(dispatch.commit.clone());
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            crate::network::queue_peer_shared_event(pool, &peer, &dispatch.event, current),
        )
        .await
        {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => log::warn!(
                "Archive update to {} was not queued: {error}",
                peer.hostname
            ),
            Err(_) => log::warn!("Archive update queue timed out for {}", peer.hostname),
        }
    }
}

pub fn persist(
    settings: &crate::crypto::Settings,
    queue: &AutomaticImageArchive,
) -> Result<(), String> {
    settings.save().map_err(|error| error.to_string())?;
    queue.set_enabled(settings.auto_save_network_images);
    Ok(())
}
