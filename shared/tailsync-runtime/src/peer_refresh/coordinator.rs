//! One worker, a coalesced wake and an epoch fence for all refresh side effects.
use std::{
    collections::HashSet,
    future::Future,
    sync::{Arc, Mutex as StdMutex},
    time::Duration,
};
use tailsync_core::{
    cancellation::Cancellation,
    crypto::Settings,
    peer::{
        directory,
        health::RouteKey,
        types::{LocalInfo, PeerInfo, PeerStatus},
    },
};
use tokio::sync::{watch, Mutex, Notify};

pub type Discovery = (LocalInfo, Vec<PeerInfo>);
pub type Observations = Vec<(RouteKey, u64)>;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RefreshResult {
    Idle,
    Completed { used_cache: bool },
    Failed,
    Superseded,
    Stopped,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use tailsync_core::peer::types::{ConnectionInterface, PeerCandidate};

    #[derive(Default)]
    struct Adapter {
        fail: AtomicBool,
        calls: AtomicUsize,
        health: AtomicUsize,
        plans: StdMutex<Vec<Vec<RouteKey>>>,
    }
    fn discovery() -> Discovery {
        (
            LocalInfo {
                hostname: "local".into(),
                tailscale_ip: "127.0.0.1".into(),
                candidates: vec![],
            },
            vec![PeerInfo {
                hostname: "peer".into(),
                tailscale_ip: "192.0.2.1".into(),
                online: true,
                enabled: true,
                address: "192.0.2.1".into(),
                connection_mode: "lan".into(),
                trusted: false,
                fingerprint: String::new(),
                candidates: vec![PeerCandidate {
                    latency: Some(42),
                    ..PeerCandidate::new(ConnectionInterface::Lan, "192.0.2.1")
                }],
                current_interface: Some(ConnectionInterface::Lan),
                current_address: Some("192.0.2.1".into()),
                status: PeerStatus::Online,
            }],
        )
    }
    impl PeerRefreshAdapter for Adapter {
        async fn discover(&self, _: &str) -> Result<Discovery, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.fail.load(Ordering::SeqCst) {
                Err("offline fixture".into())
            } else {
                Ok(discovery())
            }
        }
        async fn probe(&self, routes: Vec<RouteKey>) -> Result<Observations, String> {
            self.plans.lock().unwrap().push(routes);
            Ok(vec![])
        }
        fn supports_rtt(&self, _: &str) -> bool {
            false
        }
        fn record_health(&self, _: &str, _: &[PeerInfo], _: Observations) {
            self.health.fetch_add(1, Ordering::SeqCst);
        }
        fn remember(&self, _: &mut Settings, _: &str, _: &[PeerInfo]) {}
        async fn prewarm(&self, _: Vec<PeerInfo>) {}
        fn publish(&self, _: Completion) {}
    }

    #[test]
    fn stale_mode_and_same_mode_reset_cannot_commit_any_side_effect() {
        let worker = PeerRefresh::default();
        let old = worker.begin("auto");
        worker.begin("lan_only");
        assert!(
            !worker.commit(&old, Some(discovery()), RefreshResult::Failed, || panic!(
                "stale effect"
            ))
        );
        let old = worker.begin("lan_only");
        worker.invalidate(|| {});
        assert!(
            !worker.commit(&old, Some(discovery()), RefreshResult::Failed, || panic!(
                "stale wake effect"
            ))
        );
        assert!(worker.cached("auto").is_none());
        assert!(worker.cached("lan_only").is_none());
    }

    #[tokio::test]
    async fn failure_reuses_candidates_without_replaying_old_health() {
        let worker = Arc::new(PeerRefresh::default());
        let adapter = Arc::new(Adapter::default());
        let settings = Arc::new(Mutex::new(Settings::default()));
        worker.run_round(&settings, &adapter).await;
        adapter.fail.store(true, Ordering::SeqCst);
        worker.run_round(&settings, &adapter).await;
        let peers = worker.cached("auto").unwrap().1;
        assert!(!peers[0].online);
        assert!(peers[0].current_address.is_none());
        assert_eq!(peers[0].status, PeerStatus::Discovered);
        assert!(peers[0].candidates[0].latency.is_none());
        assert!(!adapter.plans.lock().unwrap()[1].is_empty());
        assert_eq!(
            worker.completed.borrow().result,
            RefreshResult::Completed { used_cache: true }
        );
        assert_eq!(adapter.health.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn remembered_routes_are_probed_even_when_discovery_fails() {
        let worker = Arc::new(PeerRefresh::default());
        let adapter = Arc::new(Adapter::default());
        adapter.fail.store(true, Ordering::SeqCst);
        let mut settings = Settings::default();
        settings
            .trusted_peer_keys
            .insert("remembered".into(), "11".repeat(32));
        settings.trusted_peer_addresses.insert(
            "remembered".into(),
            [("lan".into(), "192.0.2.9".into())].into(),
        );
        worker
            .run_round(&Arc::new(Mutex::new(settings)), &adapter)
            .await;
        assert!(adapter.plans.lock().unwrap()[0].contains(&RouteKey::new(
            "remembered",
            ConnectionInterface::Lan,
            "192.0.2.9"
        )));
        assert_eq!(worker.completed.borrow().result, RefreshResult::Failed);
    }

    #[tokio::test]
    async fn one_thousand_requests_have_one_pending_wake() {
        let worker = PeerRefresh::default();
        for _ in 0..1000 {
            worker.request();
        }
        tokio::time::timeout(Duration::from_millis(20), worker.wake.notified())
            .await
            .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(20), worker.wake.notified())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn reset_and_shutdown_resolve_waiters_without_false_success() {
        let worker = Arc::new(PeerRefresh::default());
        worker.begin("auto");
        for stop in [false, true] {
            let waiting = worker.clone();
            let task = tokio::spawn(async move {
                waiting
                    .refresh_and_wait("auto", Duration::from_secs(1))
                    .await
            });
            worker.wake.notified().await; // waiter subscribed before waking the worker
            if stop {
                worker.stop();
            } else {
                worker.invalidate(|| {});
            }
            assert!(task.await.unwrap().is_err());
            // invalidate's coalesced worker wake is consumed before the next waiter.
            if !stop {
                worker.wake.notified().await;
            }
        }
    }
}
#[derive(Clone, Debug)]
pub struct Completion {
    pub epoch: u64,
    pub generation: u64,
    pub mode: String,
    pub result: RefreshResult,
}
struct State {
    epoch: u64,
    mode: String,
    cache: Option<Discovery>,
    cancellation: Cancellation,
}
#[derive(Clone)]
struct Round {
    epoch: u64,
    mode: String,
    cancellation: Cancellation,
}
pub struct PeerRefresh {
    state: StdMutex<State>,
    wake: Notify,
    completed: watch::Sender<Completion>,
}

impl Default for PeerRefresh {
    fn default() -> Self {
        Self {
            state: StdMutex::new(State {
                epoch: 0,
                mode: String::new(),
                cache: None,
                cancellation: Cancellation::default(),
            }),
            wake: Notify::new(),
            completed: watch::channel(Completion {
                epoch: 0,
                generation: 0,
                mode: String::new(),
                result: RefreshResult::Idle,
            })
            .0,
        }
    }
}

impl PeerRefresh {
    fn notify(&self, state: &State, result: RefreshResult) {
        self.completed.send_modify(|value| {
            *value = Completion {
                epoch: state.epoch,
                generation: value.generation.wrapping_add(1),
                mode: state.mode.clone(),
                result,
            };
        });
    }
    fn reset(&self, state: &mut State) {
        state.cancellation.cancel();
        state.cancellation = Cancellation::default();
        state.epoch = state.epoch.wrapping_add(1);
        state.cache = None;
        self.notify(state, RefreshResult::Superseded);
    }
    /// Callback clears only in-memory health, atomically with invalidation.
    pub fn invalidate(&self, clear_health: impl FnOnce()) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        self.reset(&mut state);
        clear_health();
        self.wake.notify_one();
    }
    pub fn request(&self) {
        self.wake.notify_one();
    }
    pub fn cached(&self, mode: &str) -> Option<Discovery> {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        (state.mode == mode).then(|| state.cache.clone()).flatten()
    }
    fn begin(&self, mode: &str) -> Round {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.mode != mode {
            self.reset(&mut state);
            state.mode = mode.to_owned();
        }
        Round {
            epoch: state.epoch,
            mode: state.mode.clone(),
            cancellation: state.cancellation.clone(),
        }
    }
    fn commit(
        &self,
        round: &Round,
        cache: Option<Discovery>,
        result: RefreshResult,
        apply: impl FnOnce(),
    ) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.epoch != round.epoch
            || state.mode != round.mode
            || round.cancellation.is_cancelled()
        {
            return false;
        }
        apply();
        state.cache = cache;
        self.notify(&state, result);
        true
    }
    pub async fn refresh_and_wait(
        &self,
        mode: &str,
        budget: Duration,
    ) -> Result<Completion, String> {
        let mut completed = self.completed.subscribe();
        let generation = completed.borrow().generation;
        let epoch = {
            let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            (state.mode == mode).then_some(state.epoch)
        };
        self.request(); // One pending Notify permit; no per-request work queue.
        tokio::time::timeout(budget, async {
            loop {
                let value = completed.borrow_and_update().clone();
                if value.generation > generation {
                    if value.result == RefreshResult::Stopped {
                        return Err("Peer health monitor stopped".into());
                    }
                    if epoch.is_some_and(|epoch| epoch != value.epoch) {
                        return Err("Peer refresh superseded by settings or reset".into());
                    }
                    if value.mode == mode {
                        match value.result {
                            RefreshResult::Completed { .. } => return Ok(value),
                            RefreshResult::Failed => {
                                return Err("Peer discovery and cache are unavailable".into())
                            }
                            RefreshResult::Superseded if epoch.is_some() => {
                                return Err("Peer refresh superseded".into())
                            }
                            _ => {}
                        }
                    }
                }
                completed
                    .changed()
                    .await
                    .map_err(|_| "Peer health monitor stopped".to_string())?;
            }
        })
        .await
        .map_err(|_| "Peer refresh timed out".to_string())?
    }
    fn stop(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        self.reset(&mut state);
        self.notify(&state, RefreshResult::Stopped);
    }
    /// Supports platform fixtures without exposing a test-only discovery bypass.
    #[cfg(any(test, feature = "test-support"))]
    pub fn seed_candidates(&self, mode: &str, discovery: Discovery) {
        let round = self.begin(mode);
        self.commit(
            &round,
            Some(discovery),
            RefreshResult::Completed { used_cache: false },
            || {},
        );
    }
}

pub trait PeerRefreshAdapter: Send + Sync + 'static {
    fn discover(&self, mode: &str) -> impl Future<Output = Result<Discovery, String>> + Send;
    fn probe(
        &self,
        routes: Vec<RouteKey>,
    ) -> impl Future<Output = Result<Observations, String>> + Send;
    fn supports_rtt(&self, endpoint: &str) -> bool;
    fn record_health(&self, mode: &str, peers: &[PeerInfo], observations: Observations);
    /// Blocking worker owns Settings and the epoch fence. Do not reacquire them.
    fn remember(&self, settings: &mut Settings, mode: &str, peers: &[PeerInfo]);
    fn prewarm(&self, peers: Vec<PeerInfo>) -> impl Future<Output = ()> + Send;
    fn publish(&self, completion: Completion);
}

fn same_policy(a: &Settings, b: &Settings) -> bool {
    a.connection_mode == b.connection_mode
        && a.enabled_peers == b.enabled_peers
        && a.trusted_peer_keys == b.trusted_peer_keys
        && a.paired_peer_endpoints == b.paired_peer_endpoints
}

/// Cached candidates never count as new health evidence. Authenticated sessions
/// remain independently authoritative in the existing SessionRegistry.
pub fn strip_health(peers: &mut [PeerInfo]) {
    for peer in peers {
        peer.online = false;
        peer.status = PeerStatus::Discovered;
        peer.current_interface = None;
        peer.current_address = None;
        for candidate in &mut peer.candidates {
            candidate.online = false;
            candidate.status = PeerStatus::Discovered;
            candidate.latency = None;
        }
    }
}
pub fn probe_plan(peers: &[PeerInfo]) -> Vec<RouteKey> {
    peers
        .iter()
        .flat_map(|peer| {
            peer.candidates
                .iter()
                .filter(|candidate| {
                    candidate.latency.is_none()
                        && candidate.interface
                            != tailsync_core::peer::types::ConnectionInterface::Iroh
                })
                .map(|candidate| {
                    RouteKey::new(&peer.hostname, candidate.interface, &candidate.address)
                })
        })
        .collect::<HashSet<_>>()
        .into_iter()
        .collect()
}

impl PeerRefresh {
    async fn run_round<A: PeerRefreshAdapter>(
        self: &Arc<Self>,
        settings: &Arc<Mutex<Settings>>,
        adapter: &Arc<A>,
    ) {
        let snapshot = settings.lock().await.clone();
        let round = self.begin(&snapshot.connection_mode);
        let work = async {
            let discovered =
                tokio::time::timeout(Duration::from_secs(4), adapter.discover(&round.mode)).await;
            let (discovery, used_cache) = match discovered {
                Ok(Ok(discovery)) => (Some(discovery), false),
                _ => {
                    let mut cached = self.cached(&round.mode);
                    if let Some((_, peers)) = &mut cached {
                        strip_health(peers);
                    }
                    (cached, true)
                }
            };
            let raw = discovery
                .as_ref()
                .map(|(_, peers)| peers.clone())
                .unwrap_or_default();
            let peers = directory::merge_paired_peers(&snapshot, &round.mode, raw, |id| {
                adapter.supports_rtt(id)
            });
            let mut observations = peers
                .iter()
                .flat_map(|peer| {
                    peer.candidates.iter().filter_map(|candidate| {
                        candidate.latency.map(|latency| {
                            (
                                RouteKey::new(
                                    &peer.hostname,
                                    candidate.interface,
                                    &candidate.address,
                                ),
                                latency,
                            )
                        })
                    })
                })
                .collect::<Vec<_>>();
            if let Ok(Ok(probed)) =
                tokio::time::timeout(Duration::from_secs(2), adapter.probe(probe_plan(&peers)))
                    .await
            {
                observations.extend(probed);
            }
            let result = if discovery.is_some() {
                RefreshResult::Completed { used_cache }
            } else {
                RefreshResult::Failed
            };
            let coordinator = self.clone();
            let settings = settings.clone();
            let commit_adapter = adapter.clone();
            let commit_round = round.clone();
            let committed = tokio::task::spawn_blocking(move || {
                let mut latest = settings.blocking_lock();
                if !same_policy(&snapshot, &latest) {
                    coordinator.invalidate(|| {});
                    return None;
                }
                let accepted = coordinator.commit(&commit_round, discovery, result, || {
                    commit_adapter.record_health(&commit_round.mode, &peers, observations);
                    commit_adapter.remember(&mut latest, &commit_round.mode, &peers);
                });
                accepted.then_some(peers)
            })
            .await
            .ok()
            .flatten();
            if let Some(peers) = committed {
                adapter.publish(self.completed.borrow().clone());
                // Handshakes continue in connection workers; never delay waiters.
                let _ =
                    tokio::time::timeout(Duration::from_millis(500), adapter.prewarm(peers)).await;
            }
        };
        tokio::select! { biased; _ = round.cancellation.cancelled() => {}, _ = work => {} }
    }
    pub async fn run<A: PeerRefreshAdapter>(
        self: Arc<Self>,
        settings: Arc<Mutex<Settings>>,
        adapter: Arc<A>,
        mut shutdown: watch::Receiver<bool>,
        interval: Duration,
    ) {
        loop {
            if *shutdown.borrow() {
                break;
            }
            tokio::select! { biased; _ = shutdown.changed() => break, _ = self.run_round(&settings, &adapter) => {} }
            tokio::select! { _ = shutdown.changed() => break, _ = self.wake.notified() => {}, _ = tokio::time::sleep(interval) => {} }
        }
        self.stop();
    }
}
