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
        let addresses = routes
            .iter()
            .filter_map(|route| route.address.parse::<IpAddr>().ok())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        if addresses.is_empty() {
            return Ok(Vec::new());
        }
        let probed = tokio::task::spawn_blocking(move || lan::probe_hostnames(&addresses))
            .await
            .map_err(|e| e.to_string())?;
        Ok(routes
            .into_iter()
            .filter_map(|route| {
                let response = probed.get(&route.address.parse::<IpAddr>().ok()?)?;
                // An address response for a different hostname is not evidence for this route.
                (response.hostname == route.hostname).then_some((route, response.latency_ms))
            })
            .collect())
    }
    fn record_health(&self, mode: &str, peers: &[tailscale::PeerInfo], observations: Observations) {
        let candidates = peers.iter().flat_map(|peer| {
            peer.candidates.iter().map(|candidate| {
                RouteKey::new(&peer.hostname, candidate.interface, &candidate.address)
            })
        });
        super::health::record_probe_round(mode, candidates, observations);
    }
}

pub async fn request_peer_refresh(mode: &str) -> Result<(), String> {
    refresh()
        .refresh_and_wait(mode, Duration::from_secs(7))
        .await
        .map(|_| ())
}
pub async fn peer_health_monitor(
    settings: Arc<Mutex<crypto::Settings>>,
    pool: Arc<Mutex<ConnectionPool>>,
    app_handle: tauri::AppHandle,
    shutdown: watch::Receiver<bool>,
) {
    refresh()
        .clone()
        .run(
            settings,
            Arc::new(Adapter {
                pool,
                app_handle: Some(app_handle),
            }),
            shutdown,
            PEER_CACHE_REFRESH_INTERVAL,
        )
        .await;
}
