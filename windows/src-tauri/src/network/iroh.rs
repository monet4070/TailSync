use super::server::ConnectionLimiter;
use super::*;
use std::collections::HashSet;

use tailsync_core::iroh_transport::IrohEndpoint;

#[derive(Default)]
struct EndpointState {
    endpoint: Option<IrohEndpoint>,
    generation: u64,
}

static ENDPOINT_STATE: OnceLock<Mutex<EndpointState>> = OnceLock::new();
static LOCAL_ENDPOINT_ID: OnceLock<StdMutex<Option<String>>> = OnceLock::new();
static RTT_CAPABLE_ENDPOINTS: OnceLock<StdMutex<HashSet<String>>> = OnceLock::new();
static MODE_CHANGED: OnceLock<Notify> = OnceLock::new();

fn endpoint_state() -> &'static Mutex<EndpointState> {
    ENDPOINT_STATE.get_or_init(|| Mutex::new(EndpointState::default()))
}

fn local_endpoint_id_state() -> &'static StdMutex<Option<String>> {
    LOCAL_ENDPOINT_ID.get_or_init(|| StdMutex::new(None))
}

fn rtt_capable_endpoints() -> &'static StdMutex<HashSet<String>> {
    RTT_CAPABLE_ENDPOINTS.get_or_init(|| StdMutex::new(HashSet::new()))
}

pub(super) fn remember_rtt_capability(endpoint_id: &str) {
    rtt_capable_endpoints()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(endpoint_id.to_string());
}

pub(super) fn supports_rtt(endpoint_id: &str) -> bool {
    rtt_capable_endpoints()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .contains(endpoint_id)
}

fn mode_changed() -> &'static Notify {
    MODE_CHANGED.get_or_init(Notify::new)
}

pub(super) fn local_endpoint_id() -> Option<String> {
    let cached = local_endpoint_id_state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    if cached.is_some() {
        return cached;
    }
    match tailsync_core::iroh_transport::persistent_endpoint_id() {
        Ok(endpoint_id) => {
            *local_endpoint_id_state()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(endpoint_id.clone());
            Some(endpoint_id)
        }
        Err(error) => {
            warn!("Could not load local Iroh endpoint ID: {error}");
            None
        }
    }
}

async fn ensure_endpoint() -> Result<(IrohEndpoint, u64), String> {
    let mut state = endpoint_state().lock().await;
    if let Some(endpoint) = &state.endpoint {
        return Ok((endpoint.clone(), state.generation));
    }

    let endpoint = IrohEndpoint::bind().await?;
    let endpoint_id = endpoint.endpoint_id();
    state.generation = state.generation.wrapping_add(1);
    let generation = state.generation;
    state.endpoint = Some(endpoint.clone());
    *local_endpoint_id_state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(endpoint_id.clone());
    info!("Iroh endpoint started as {endpoint_id}");
    Ok((endpoint, generation))
}

pub(super) async fn endpoint() -> Result<IrohEndpoint, String> {
    ensure_endpoint().await.map(|(endpoint, _)| endpoint)
}

async fn invalidate_endpoint(generation: u64) {
    let endpoint = {
        let mut state = endpoint_state().lock().await;
        if state.generation != generation {
            return;
        }
        state.generation = state.generation.wrapping_add(1);
        state.endpoint.take()
    };
    *local_endpoint_id_state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    if let Some(endpoint) = endpoint {
        endpoint.close().await;
    }
}

async fn close_endpoint() {
    let endpoint = {
        let mut state = endpoint_state().lock().await;
        state.generation = state.generation.wrapping_add(1);
        state.endpoint.take()
    };
    *local_endpoint_id_state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    if let Some(endpoint) = endpoint {
        endpoint.close().await;
        info!("Iroh endpoint stopped");
    }
}

pub async fn refresh_for_mode(mode: &str) {
    if mode == "auto" {
        if let Err(error) = ensure_endpoint().await {
            warn!("Could not start Iroh endpoint: {error}");
        }
    } else {
        close_endpoint().await;
    }
    mode_changed().notify_waiters();
}

pub async fn start_server(
    sync_engine: Arc<Mutex<sync::SyncEngine>>,
    database: Arc<Mutex<db::HistoryDB>>,
    settings: Arc<Mutex<crypto::Settings>>,
    identity: Arc<DeviceIdentity>,
    pairing: Arc<PairingManager>,
    remote_invites: Arc<tailsync_core::pairing::RemotePairingInviteManager>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let limiter = ConnectionLimiter::new(64, 8);
    let mut handlers = tokio::task::JoinSet::new();

    loop {
        if *shutdown.borrow() {
            break;
        }
        let mode = settings.lock().await.connection_mode.clone();
        if mode != "auto" {
            close_endpoint().await;
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                _ = mode_changed().notified() => {}
                _ = wait_for_shutdown(&mut shutdown) => break,
            }
            continue;
        }

        let (endpoint, generation) = match ensure_endpoint().await {
            Ok(endpoint) => endpoint,
            Err(error) => {
                warn!("Could not start Iroh endpoint: {error}");
                tokio::select! {
                    _ = tokio::time::sleep(RECONNECT_DELAY) => {}
                    _ = mode_changed().notified() => {}
                    _ = wait_for_shutdown(&mut shutdown) => break,
                }
                continue;
            }
        };

        let accepted = tokio::select! {
            accepted = timeout(HANDSHAKE_TIMEOUT, endpoint.accept()) => match accepted {
                Ok(accepted) => Some(accepted),
                Err(_) => continue,
            },
            joined = handlers.join_next(), if !handlers.is_empty() => {
                if let Some(Err(error)) = joined {
                    debug!("Inbound Iroh connection task ended unexpectedly: {error}");
                }
                continue;
            }
            _ = mode_changed().notified() => continue,
            _ = wait_for_shutdown(&mut shutdown) => None,
        };
        let Some(accepted) = accepted else {
            break;
        };

        match accepted {
            Ok(Some(accepted)) => {
                let remote_endpoint_id = accepted.remote_endpoint_id.clone();
                let connection_kind = accepted.kind();
                let Some(permit) = limiter.try_acquire_source(remote_endpoint_id.clone()) else {
                    warn!("Connection limit reached for an inbound Iroh peer");
                    continue;
                };
                if connection_kind == tailsync_core::iroh_transport::IrohConnectionKind::Rtt {
                    handlers.spawn(async move {
                        let _permit = permit;
                        accepted.wait_for_close().await;
                    });
                    continue;
                }
                let sync = sync_engine.clone();
                let db = database.clone();
                let settings = settings.clone();
                let identity = identity.clone();
                let pairing = pairing.clone();
                let remote_invites = remote_invites.clone();
                handlers.spawn(async move {
                    let _permit = permit;
                    let stream = match timeout(HANDSHAKE_TIMEOUT, accepted.accept_stream()).await {
                        Ok(Ok(stream)) => stream,
                        Ok(Err(error)) => {
                            warn!("Could not accept inbound Iroh stream: {error}");
                            return;
                        }
                        Err(_) => {
                            warn!("Inbound Iroh peer did not open a stream before timeout");
                            return;
                        }
                    };
                    let result = if connection_kind
                        == tailsync_core::iroh_transport::IrohConnectionKind::Invite
                    {
                        server::handle_iroh_invite_connection(
                            stream,
                            remote_endpoint_id.clone(),
                            remote_invites,
                            sync,
                            db,
                            settings,
                            identity,
                            pairing,
                        )
                        .await
                    } else {
                        server::handle_iroh_connection(
                            stream,
                            remote_endpoint_id.clone(),
                            sync,
                            db,
                            settings,
                            identity,
                            pairing,
                            None,
                        )
                        .await
                    };
                    if let Err(error) = result {
                        warn!("Inbound Iroh connection error: {error}");
                        debug!("Failed inbound Iroh endpoint: {remote_endpoint_id}");
                    }
                });
            }
            Ok(None) => {
                invalidate_endpoint(generation).await;
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            Err(error) => {
                debug!("Rejected inbound Iroh connection: {error}");
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(50)) => {}
                    _ = mode_changed().notified() => {}
                    _ = wait_for_shutdown(&mut shutdown) => break,
                }
            }
        }
    }

    close_endpoint().await;
    if timeout(Duration::from_secs(2), async {
        while handlers.join_next().await.is_some() {}
    })
    .await
    .is_err()
    {
        warn!("Timed out while draining inbound Iroh connections");
        handlers.abort_all();
        while handlers.join_next().await.is_some() {}
    }
    info!("Iroh server stopped for application shutdown");
    Ok(())
}
