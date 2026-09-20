use super::server::ConnectionLimiter;
use super::*;

use tailsync_core::iroh_transport::{IrohEndpoint, IrohEndpointRegistry};

static ENDPOINT_REGISTRY: OnceLock<IrohEndpointRegistry> = OnceLock::new();

fn endpoint_registry() -> &'static IrohEndpointRegistry {
    ENDPOINT_REGISTRY.get_or_init(IrohEndpointRegistry::default)
}
pub(super) fn remember_rtt_capability(endpoint_id: &str) {
    endpoint_registry().remember_rtt_capability(endpoint_id);
}

pub(super) fn supports_rtt(endpoint_id: &str) -> bool {
    endpoint_registry().supports_rtt(endpoint_id)
}

fn mode_changed() -> &'static Notify {
    endpoint_registry().mode_changed()
}

pub(super) fn local_endpoint_id() -> Option<String> {
    endpoint_registry().local_endpoint_id()
}

async fn ensure_endpoint() -> Result<(IrohEndpoint, u64), String> {
    endpoint_registry().ensure_endpoint().await
}

pub(super) async fn endpoint() -> Result<IrohEndpoint, String> {
    endpoint_registry().endpoint().await
}

async fn invalidate_endpoint(generation: u64) {
    endpoint_registry().invalidate_endpoint(generation).await;
}

async fn close_endpoint() {
    endpoint_registry().close_endpoint().await;
}

pub async fn refresh_for_mode(mode: &str) {
    if let Err(error) = endpoint_registry().refresh_for_mode(mode).await {
        warn!("Could not start Iroh endpoint: {error}");
    }
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
        if !tailsync_core::peer::types::ConnectionMode::parse(&mode).is_some_and(|mode| {
            mode.allows(tailsync_core::peer::types::ConnectionInterface::Iroh)
        }) {
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
