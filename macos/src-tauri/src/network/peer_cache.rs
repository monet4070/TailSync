use super::*;

#[derive(Clone)]
struct PeerCacheEntry {
    local: tailscale::LocalInfo,
    peers: Vec<tailscale::PeerInfo>,
}

static PEER_CACHE: OnceLock<RwLock<HashMap<String, PeerCacheEntry>>> = OnceLock::new();
static PEER_REFRESH_NOTIFY: OnceLock<Notify> = OnceLock::new();
static PEER_REFRESH_GENERATION: OnceLock<watch::Sender<u64>> = OnceLock::new();

fn peer_cache() -> &'static RwLock<HashMap<String, PeerCacheEntry>> {
    PEER_CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

fn peer_refresh_generation() -> &'static watch::Sender<u64> {
    PEER_REFRESH_GENERATION.get_or_init(|| watch::channel(0).0)
}

pub(crate) async fn store_peer_cache(
    mode: &str,
    local: tailscale::LocalInfo,
    peers: Vec<tailscale::PeerInfo>,
) {
    peer_cache()
        .write()
        .await
        .insert(mode.to_string(), PeerCacheEntry { local, peers });
}

pub async fn clear_peer_cache() {
    peer_cache().write().await.clear();
    clear_peer_health();
    request_peer_refresh();
}

async fn refresh_peer_cache(
    mode: &str,
) -> Result<(tailscale::LocalInfo, Vec<tailscale::PeerInfo>), String> {
    let (local, peers) = discover_peers(mode).await?;
    store_peer_cache(mode, local.clone(), peers.clone()).await;
    Ok((local, peers))
}

pub async fn cached_discover_peers(
    mode: &str,
) -> Result<(tailscale::LocalInfo, Vec<tailscale::PeerInfo>), String> {
    if let Some(entry) = peer_cache().read().await.get(mode).cloned() {
        return Ok((entry.local, entry.peers));
    }
    let mut completion = peer_refresh_generation().subscribe();
    request_peer_refresh();
    let _ = timeout(Duration::from_secs(2), completion.changed()).await;
    if let Some(entry) = peer_cache().read().await.get(mode).cloned() {
        return Ok((entry.local, entry.peers));
    }
    Err(format!("Peer discovery is starting for {mode} mode"))
}

pub fn request_peer_refresh() {
    PEER_REFRESH_NOTIFY.get_or_init(Notify::new).notify_one();
}

pub async fn request_peer_refresh_and_wait() -> Result<(), String> {
    let mut completion = peer_refresh_generation().subscribe();
    let generation = *completion.borrow();
    request_peer_refresh();
    timeout(Duration::from_secs(3), async {
        while *completion.borrow() == generation {
            completion
                .changed()
                .await
                .map_err(|_| "Peer health monitor stopped".to_string())?;
        }
        Ok::<(), String>(())
    })
    .await
    .map_err(|_| "Peer refresh timed out".to_string())??;
    Ok(())
}

pub async fn peer_cache_refresh_loop(
    settings: Arc<Mutex<crypto::Settings>>,
    app_handle: Option<tauri::AppHandle>,
    mut shutdown: watch::Receiver<bool>,
) {
    loop {
        if *shutdown.borrow() {
            return;
        }
        let mode = settings.lock().await.connection_mode.clone();
        match refresh_peer_cache(&mode).await {
            Ok((_, peers)) => {
                update_peer_health(&mode, &peers);
                remember_peer_addresses(&settings, &mode, &peers).await;
            }
            Err(error) => {
                update_peer_health_for_failed_round(&mode);
                debug!("Peer cache refresh failed for {mode} mode: {error}");
            }
        }
        peer_refresh_generation().send_modify(|generation| {
            *generation = generation.wrapping_add(1);
        });
        if let Some(app_handle) = &app_handle {
            use tauri::Emitter;
            let _ = app_handle.emit("peer-health-changed", ());
        }
        tokio::select! {
            _ = tokio::time::sleep(PEER_CACHE_REFRESH_INTERVAL) => {}
            _ = PEER_REFRESH_NOTIFY.get_or_init(Notify::new).notified() => {}
            _ = wait_for_shutdown(&mut shutdown) => return,
        }
    }
}
