use super::*;
use tailsync_core::peer::health::RouteKey;
use tailsync_runtime::peer_refresh::{
    Completion, Discovery, Observations, PeerRefresh, PeerRefreshAdapter,
};

static PEER_REFRESH: OnceLock<Arc<PeerRefresh>> = OnceLock::new();
fn refresh() -> &'static Arc<PeerRefresh> {
    PEER_REFRESH.get_or_init(|| Arc::new(PeerRefresh::default()))
}

#[cfg(test)]
pub(crate) async fn store_peer_cache(
    mode: &str,
    local: tailscale::LocalInfo,
    peers: Vec<tailscale::PeerInfo>,
) {
    refresh().seed_candidates(mode, (local, peers));
}

pub async fn clear_peer_cache() {
    refresh().invalidate(super::health::clear_peer_health);
}

pub async fn cached_discover_peers(mode: &str) -> Result<Discovery, String> {
    if let Some(cached) = refresh().cached(mode) {
        return Ok(cached);
    }
    refresh()
        .refresh_and_wait(mode, Duration::from_secs(2))
        .await?;
    refresh()
        .cached(mode)
        .ok_or_else(|| "Peer discovery is still starting".into())
}

struct Adapter {
    pool: Arc<Mutex<ConnectionPool>>,
    app_handle: Option<tauri::AppHandle>,
}
impl PeerRefreshAdapter for Adapter {
    async fn discover(&self, mode: &str) -> Result<Discovery, String> {
        discover_peers(mode).await
    }
    fn supports_rtt(&self, endpoint: &str) -> bool {
        iroh::supports_rtt(endpoint)
    }
    fn remember(
        &self,
        settings: &mut crypto::Settings,
        _mode: &str,
        peers: &[tailscale::PeerInfo],
    ) {
        let routes = peers.iter().flat_map(|peer| {
            peer.candidates.iter().map(|candidate| {
                (
                    peer.hostname.as_str(),
                    candidate.interface.as_str(),
                    candidate.address.as_str(),
                )
            })
        });
        if let Err(error) = settings.remember_peer_addresses(routes) {
            debug!("Could not remember peer routes: {error}");
        }
    }
    async fn prewarm(&self, peers: Vec<tailscale::PeerInfo>) {
        prewarm_connections(self.pool.clone(), peers).await;
    }
    fn publish(&self, completion: Completion) {
        if let Some(app) = &self.app_handle {
            use tauri::Emitter;
            let _ = app.emit("peer-health-changed", serde_json::json!({"epoch": completion.epoch, "generation": completion.generation, "mode": completion.mode}));
        }
    }
    async fn probe(&self, routes: Vec<RouteKey>) -> Result<Observations, String> {
        let mut observations = Vec::new();
        for interface in [ConnectionInterface::Lan, ConnectionInterface::Tailscale] {
            let addresses = routes
                .iter()
                .filter(|route| route.interface == interface)
                .map(|route| route.address.clone())
                .collect::<std::collections::HashSet<_>>();
            if addresses.is_empty() {
                continue;
            }
            for response in lan::probe_addresses(addresses, interface).await? {
                for candidate in response.candidates {
                    let key =
                        RouteKey::new(&response.hostname, candidate.interface, &candidate.address);
                    if routes.contains(&key) {
                        if let Some(latency) = candidate.latency {
                            observations.push((key, latency));
                        }
                    }
                }
            }
        }
        Ok(observations)
    }
    fn record_health(&self, mode: &str, peers: &[tailscale::PeerInfo], observations: Observations) {
        super::health::record_probe_round(mode, peers, observations);
    }
}

pub async fn request_peer_refresh_and_wait(
    settings: &Arc<Mutex<crypto::Settings>>,
) -> Result<(), String> {
    let mode = settings.lock().await.connection_mode.clone();
    refresh()
        .refresh_and_wait(&mode, Duration::from_secs(7))
        .await
        .map(|_| ())
}
pub async fn peer_cache_refresh_loop(
    settings: Arc<Mutex<crypto::Settings>>,
    pool: Arc<Mutex<ConnectionPool>>,
    app_handle: Option<tauri::AppHandle>,
    shutdown: watch::Receiver<bool>,
) {
    refresh()
        .clone()
        .run(
            settings,
            Arc::new(Adapter { pool, app_handle }),
            shutdown,
            PEER_CACHE_REFRESH_INTERVAL,
        )
        .await;
}
