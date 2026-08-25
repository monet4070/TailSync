use super::health::clear_peer_health;
use super::*;
use std::collections::HashSet;

#[derive(Clone)]
struct PeerCacheEntry {
    local: tailscale::LocalInfo,
    peers: Vec<tailscale::PeerInfo>,
}

static PEER_CACHE: OnceLock<RwLock<HashMap<String, PeerCacheEntry>>> = OnceLock::new();
static PEER_REFRESH_NOTIFY: OnceLock<Notify> = OnceLock::new();
static PEER_REFRESH_GENERATION: AtomicU64 = AtomicU64::new(0);
static PEER_REFRESH_COMPLETED: OnceLock<watch::Sender<u64>> = OnceLock::new();
static PEER_REFRESH_MODE: OnceLock<StdMutex<String>> = OnceLock::new();

fn peer_cache() -> &'static RwLock<HashMap<String, PeerCacheEntry>> {
    PEER_CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

fn peer_refresh_notify() -> &'static Notify {
    PEER_REFRESH_NOTIFY.get_or_init(Notify::new)
}

fn peer_refresh_completed() -> &'static watch::Sender<u64> {
    PEER_REFRESH_COMPLETED.get_or_init(|| watch::channel(0).0)
}

fn last_peer_refresh_mode() -> &'static StdMutex<String> {
    PEER_REFRESH_MODE.get_or_init(|| StdMutex::new(String::new()))
}

fn refresh_completed_for_mode(generation: u64, baseline: u64, mode: &str) -> bool {
    generation > baseline
        && last_peer_refresh_mode()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_str()
            == mode
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
    peer_refresh_notify().notify_one();
}

async fn cached_peer_entry(mode: &str) -> Option<PeerCacheEntry> {
    peer_cache().read().await.get(mode).cloned()
}

pub async fn cached_discover_peers(
    mode: &str,
) -> Result<(tailscale::LocalInfo, Vec<tailscale::PeerInfo>), String> {
    if let Some(entry) = cached_peer_entry(mode).await {
        return Ok((entry.local, entry.peers));
    }

    let generation = PEER_REFRESH_GENERATION.load(Ordering::Acquire);
    let mut completed = peer_refresh_completed().subscribe();
    peer_refresh_notify().notify_one();
    let _ = timeout(PEER_INITIAL_CACHE_WAIT, async {
        while !refresh_completed_for_mode(
            PEER_REFRESH_GENERATION.load(Ordering::Acquire),
            generation,
            mode,
        ) {
            if completed.changed().await.is_err() {
                break;
            }
        }
    })
    .await;

    cached_peer_entry(mode)
        .await
        .map(|entry| (entry.local, entry.peers))
        .ok_or_else(|| "Peer discovery is still starting".to_string())
}

pub async fn request_peer_refresh(mode: &str) -> Result<(), String> {
    let generation = PEER_REFRESH_GENERATION.load(Ordering::Acquire);
    let mut completed = peer_refresh_completed().subscribe();
    peer_refresh_notify().notify_one();
    timeout(PEER_MANUAL_REFRESH_WAIT, async {
        while !refresh_completed_for_mode(
            PEER_REFRESH_GENERATION.load(Ordering::Acquire),
            generation,
            mode,
        ) {
            completed
                .changed()
                .await
                .map_err(|_| "Peer health monitor stopped".to_string())?;
        }
        Ok::<(), String>(())
    })
    .await
    .map_err(|_| "Peer refresh timed out".to_string())?
}

async fn run_peer_health_round(
    settings: &Arc<Mutex<crypto::Settings>>,
) -> Result<(String, Vec<tailscale::PeerInfo>), String> {
    let snapshot = settings.lock().await.clone();
    let mode = snapshot.connection_mode.clone();
    let discovery = discover_peers(&mode).await;
    let discovered = match discovery {
        Ok((local, peers)) => {
            store_peer_cache(&mode, local, peers.clone()).await;
            remember_peer_addresses(settings, &mode, &peers).await;
            peers
        }
        Err(error) => {
            debug!("Peer discovery failed for {mode} mode: {error}");
            let mut peers = cached_peer_entry(&mode)
                .await
                .map(|entry| entry.peers)
                .unwrap_or_default();
            for peer in &mut peers {
                peer.online = false;
                for candidate in &mut peer.candidates {
                    candidate.latency = None;
                }
            }
            peers
        }
    };

    let peers = merge_paired_peers(&snapshot, &mode, discovered);
    let mut routes = HashMap::<PeerRouteKey, Option<u64>>::new();
    for peer in &peers {
        for candidate in &peer.candidates {
            let key = PeerRouteKey::new(&peer.hostname, candidate.interface, &candidate.address);
            let result = routes.entry(key).or_default();
            if candidate.latency.is_some() {
                *result = candidate.latency;
            }
        }
    }

    let addresses = routes
        .iter()
        .filter(|(_, latency)| latency.is_none())
        .filter_map(|(key, _)| key.address.parse::<IpAddr>().ok())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let probed = if addresses.is_empty() {
        HashMap::new()
    } else {
        tokio::task::spawn_blocking(move || lan::probe_hostnames(&addresses))
            .await
            .map_err(|error| format!("Peer health probe task failed: {error}"))?
    };

    let candidates = routes.keys().cloned().collect::<Vec<_>>();
    let observations = routes
        .into_iter()
        .filter_map(|(key, latency)| {
            let latency = latency.or_else(|| {
                key.address
                    .parse::<IpAddr>()
                    .ok()
                    .and_then(|address| probed.get(&address))
                    .map(|response| response.latency_ms)
            });
            latency.map(|latency_ms| (key, latency_ms))
        })
        .collect::<Vec<_>>();
    record_probe_round(&mode, candidates, observations);

    Ok((mode, peers))
}

pub async fn peer_health_monitor(
    settings: Arc<Mutex<crypto::Settings>>,
    pool: Arc<Mutex<ConnectionPool>>,
    app_handle: AppHandle,
    mut shutdown: watch::Receiver<bool>,
) {
    loop {
        if *shutdown.borrow() {
            return;
        }
        let mode = match run_peer_health_round(&settings).await {
            Ok((mode, peers)) => {
                prewarm_connections(pool.clone(), peers).await;
                mode
            }
            Err(error) => {
                debug!("Peer health round failed: {error}");
                settings.lock().await.connection_mode.clone()
            }
        };
        *last_peer_refresh_mode()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = mode.clone();
        let generation = PEER_REFRESH_GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
        let _ = peer_refresh_completed().send(generation);
        let _ = app_handle.emit(
            "peer-health-changed",
            serde_json::json!({ "generation": generation, "mode": mode }),
        );

        tokio::select! {
            () = tokio::time::sleep(PEER_CACHE_REFRESH_INTERVAL) => {}
            () = peer_refresh_notify().notified() => {}
            () = wait_for_shutdown(&mut shutdown) => return,
        }
    }
}
