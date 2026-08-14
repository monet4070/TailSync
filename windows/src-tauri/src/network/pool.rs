use super::*;

#[derive(Clone)]
pub(super) struct PoolSender {
    pub(super) priority: mpsc::Sender<QueuedFrame>,
    pub(super) bulk: mpsc::Sender<QueuedFrame>,
    pub(super) shutdown: watch::Sender<bool>,
}

pub(super) use tailsync_core::peer::delivery::QueuedFrame;
pub use tailsync_core::peer::types::DeliveryReceipt;

impl PoolSender {
    #[cfg(test)]
    pub(super) fn same_channel(&self, other: &Self) -> bool {
        self.priority.same_channel(&other.priority) && self.bulk.same_channel(&other.bulk)
    }

    pub(super) fn channel_for(&self, command: Command) -> &mpsc::Sender<QueuedFrame> {
        if command == Command::FileChunk {
            &self.bulk
        } else {
            &self.priority
        }
    }

    fn request_shutdown(&self) {
        let _ = self.shutdown.send(true);
    }
}

pub struct ConnectionPool {
    pub(super) senders: HashMap<(ResolvedTarget, String), PoolSender>,
    batch_serializers: HashMap<String, Arc<Mutex<()>>>,
    identity: Arc<DeviceIdentity>,
    settings: Arc<Mutex<crypto::Settings>>,
}

pub(super) use tailsync_core::peer::types::{ResolvedCandidate, ResolvedTarget};

fn resolve_candidates(peer: &tailscale::PeerInfo) -> Result<Vec<ResolvedCandidate>, String> {
    let mut candidates = peer.candidates.clone();
    if candidates.is_empty() {
        let address = if peer.address.is_empty() {
            &peer.tailscale_ip
        } else {
            &peer.address
        };
        let interface = mode_interface(&peer.connection_mode)
            .or_else(|| infer_interface(address).ok())
            .ok_or_else(|| format!("Peer {} has no connection candidates", peer.hostname))?;
        candidates.push(PeerCandidate::new(interface, address));
    }
    candidates.sort_by_key(|candidate| candidate.priority);
    candidates
        .into_iter()
        .map(|candidate| {
            let target = match candidate.interface {
                ConnectionInterface::Iroh => ResolvedTarget::Iroh(
                    tailsync_core::iroh_transport::canonical_endpoint_id(&candidate.address)?,
                ),
                ConnectionInterface::Lan | ConnectionInterface::Tailscale => {
                    let ip: IpAddr = candidate.address.parse().map_err(|error| {
                        format!("Invalid peer address {}: {error}", candidate.address)
                    })?;
                    ResolvedTarget::Tcp(SocketAddr::new(ip, TCP_PORT))
                }
            };
            Ok(ResolvedCandidate { candidate, target })
        })
        .collect()
}

impl ConnectionPool {
    pub fn new(identity: Arc<DeviceIdentity>, settings: Arc<Mutex<crypto::Settings>>) -> Self {
        ConnectionPool {
            senders: HashMap::new(),
            batch_serializers: HashMap::new(),
            identity,
            settings,
        }
    }

    pub(super) fn sender_for(
        &mut self,
        addr: SocketAddr,
        hostname: String,
    ) -> Result<PoolSender, String> {
        let interface = infer_interface(&addr.ip().to_string()).unwrap_or(ConnectionInterface::Lan);
        self.sender_for_candidates(
            hostname,
            vec![ResolvedCandidate {
                candidate: PeerCandidate::new(interface, addr.ip().to_string()),
                target: ResolvedTarget::Tcp(addr),
            }],
        )
    }

    fn sender_for_peer(&mut self, peer: &tailscale::PeerInfo) -> Result<PoolSender, String> {
        self.sender_for_candidates(peer.hostname.clone(), resolve_candidates(peer)?)
    }

    fn sender_for_candidates(
        &mut self,
        hostname: String,
        candidates: Vec<ResolvedCandidate>,
    ) -> Result<PoolSender, String> {
        let target = candidates
            .first()
            .ok_or_else(|| format!("Peer {hostname} has no usable connection candidates"))?
            .target
            .clone();
        let key = (target, hostname.clone());
        if let Some(tx) = self.senders.get(&key) {
            return Ok(tx.clone());
        }

        self.senders.retain(|(_, peer_hostname), sender| {
            let keep = peer_hostname != &hostname;
            if !keep {
                sender.request_shutdown();
            }
            keep
        });

        let (priority, priority_rx) = mpsc::channel::<QueuedFrame>(POOL_CHANNEL_SIZE);
        let (bulk, bulk_rx) = mpsc::channel::<QueuedFrame>(POOL_CHANNEL_SIZE);
        let (shutdown, shutdown_rx) = watch::channel(false);
        let tx = PoolSender {
            priority,
            bulk,
            shutdown,
        };
        self.senders.insert(key, tx.clone());
        tokio::spawn(connection_task(
            candidates,
            hostname,
            priority_rx,
            bulk_rx,
            self.identity.clone(),
            self.settings.clone(),
            shutdown_rx,
        ));
        Ok(tx)
    }

    /// Push a frame to a TCP peer. Creates a persistent background connection
    /// on first use.
    pub async fn send(
        &mut self,
        addr: SocketAddr,
        hostname: String,
        cmd: Command,
        payload: Vec<u8>,
    ) -> Result<(), String> {
        let trusted_key = self
            .settings
            .lock()
            .await
            .trusted_peer_keys
            .get(&hostname)
            .cloned()
            .ok_or_else(|| format!("Peer {hostname} is not paired"))?;
        secure::decode_trusted_key(&trusted_key)
            .map_err(|error| format!("Peer {hostname} has an invalid pinned key: {error}"))?;
        if payload.len() > cmd.payload_limit() {
            return Err(format!(
                "{:?} payload exceeds the {} byte limit",
                cmd,
                cmd.payload_limit()
            ));
        }
        let tx = self.sender_for(addr, hostname)?;

        enqueue_pool_frame(tx, ResolvedTarget::Tcp(addr), cmd, payload).await
    }

    /// Remove a peer from the pool (e.g. when user disables it).
    pub fn disconnect_hostname(&mut self, hostname: &str) {
        self.senders.retain(|(_, peer_hostname), sender| {
            let keep = peer_hostname != hostname;
            if !keep {
                sender.request_shutdown();
            }
            keep
        });
    }

    pub fn disconnect_all(&mut self) {
        for sender in self.senders.values() {
            sender.request_shutdown();
        }
        self.senders.clear();
    }
}

pub async fn acquire_peer_file_batch(
    pool: &Arc<Mutex<ConnectionPool>>,
    hostname: &str,
) -> tokio::sync::OwnedMutexGuard<()> {
    let serializer = {
        let mut pool = pool.lock().await;
        pool.batch_serializers
            .entry(hostname.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    };
    serializer.lock_owned().await
}

pub async fn queue_peer_frame(
    pool: &Arc<Mutex<ConnectionPool>>,
    peer: &tailscale::PeerInfo,
    cmd: Command,
    payload: Vec<u8>,
) -> Result<(), String> {
    if payload.len() > cmd.payload_limit() {
        return Err(format!(
            "{:?} payload exceeds the {} byte limit",
            cmd,
            cmd.payload_limit()
        ));
    }
    let settings = { pool.lock().await.settings.clone() };
    let trusted_key = settings
        .lock()
        .await
        .trusted_peer_keys
        .get(&peer.hostname)
        .cloned()
        .ok_or_else(|| format!("Peer {} is not paired", peer.hostname))?;
    secure::decode_trusted_key(&trusted_key)
        .map_err(|error| format!("Peer {} has an invalid pinned key: {error}", peer.hostname))?;

    let tx = { pool.lock().await.sender_for_peer(peer)? };
    let preferred = resolve_candidates(peer)?
        .first()
        .map(|candidate| candidate.target.clone())
        .ok_or_else(|| format!("Peer {} has no connection candidates", peer.hostname))?;
    enqueue_pool_frame(tx, preferred, cmd, payload).await
}

pub async fn queue_peer_file_frame(
    pool: &Arc<Mutex<ConnectionPool>>,
    peer: &tailscale::PeerInfo,
    command: Command,
    payload: Vec<u8>,
    transfer_id: TransferId,
) -> Result<DeliveryReceipt, String> {
    let settings = { pool.lock().await.settings.clone() };
    let trusted_key = settings
        .lock()
        .await
        .trusted_peer_keys
        .get(&peer.hostname)
        .cloned()
        .ok_or_else(|| format!("Peer {} is not paired", peer.hostname))?;
    secure::decode_trusted_key(&trusted_key)
        .map_err(|error| format!("Peer {} has an invalid pinned key: {error}", peer.hostname))?;

    let tx = { pool.lock().await.sender_for_peer(peer)? };
    let preferred = resolve_candidates(peer)?
        .first()
        .map(|candidate| candidate.target.clone())
        .ok_or_else(|| format!("Peer {} has no connection candidates", peer.hostname))?;
    let (completion_tx, completion_rx) = oneshot::channel();
    let queued = QueuedFrame::confirmed_file(command, payload, transfer_id, completion_tx)?;
    enqueue_queued_frame(tx, preferred, queued).await?;
    let completion = timeout(FILE_CONFIRM_TIMEOUT, completion_rx)
        .await
        .map_err(|_| format!("Timed out waiting for {:?} confirmation", command))?
        .map_err(|_| format!("Connection task for {} closed", peer.hostname))?;
    completion.map_err(|error| error.to_string())
}

pub async fn queue_peer_batch_frame(
    pool: &Arc<Mutex<ConnectionPool>>,
    peer: &tailscale::PeerInfo,
    command: Command,
    payload: Vec<u8>,
    batch_id: TransferId,
) -> Result<DeliveryReceipt, String> {
    let settings = { pool.lock().await.settings.clone() };
    let trusted_key = settings
        .lock()
        .await
        .trusted_peer_keys
        .get(&peer.hostname)
        .cloned()
        .ok_or_else(|| format!("Peer {} is not paired", peer.hostname))?;
    secure::decode_trusted_key(&trusted_key)
        .map_err(|error| format!("Peer {} has an invalid pinned key: {error}", peer.hostname))?;
    let tx = { pool.lock().await.sender_for_peer(peer)? };
    let preferred = resolve_candidates(peer)?
        .first()
        .map(|candidate| candidate.target.clone())
        .ok_or_else(|| format!("Peer {} has no connection candidates", peer.hostname))?;
    let (completion_tx, completion_rx) = oneshot::channel();
    let queued = QueuedFrame::confirmed_batch(command, payload, batch_id, completion_tx)?;
    enqueue_queued_frame(tx, preferred, queued).await?;
    let completion = timeout(FILE_CONFIRM_TIMEOUT, completion_rx)
        .await
        .map_err(|_| format!("Timed out waiting for {:?} confirmation", command))?
        .map_err(|_| format!("Connection task for {} closed", peer.hostname))?;
    completion.map_err(|error| error.to_string())
}

/// Start persistent connection tasks before the first clipboard payload so
/// copying does not pay TCP and Noise handshake latency.
pub async fn prewarm_connections(
    pool: Arc<Mutex<ConnectionPool>>,
    peers: Vec<tailscale::PeerInfo>,
) {
    for peer in peers
        .into_iter()
        .filter(|peer| peer.enabled && peer.trusted)
    {
        if let Err(error) = pool.lock().await.sender_for_peer(&peer) {
            debug!("Could not prewarm {}: {error}", peer.hostname);
        }
    }
}

async fn enqueue_pool_frame(
    tx: PoolSender,
    target: ResolvedTarget,
    cmd: Command,
    payload: Vec<u8>,
) -> Result<(), String> {
    let queued = QueuedFrame::new(cmd, payload)?;
    enqueue_queued_frame(tx, target, queued).await
}

async fn enqueue_queued_frame(
    tx: PoolSender,
    target: ResolvedTarget,
    queued: QueuedFrame,
) -> Result<(), String> {
    let command = queued.command();
    timeout(POOL_SEND_TIMEOUT, tx.channel_for(command).send(queued))
        .await
        .map_err(|_| format!("Timed out queueing frame for {target}"))?
        .map_err(|_| format!("Connection to {target} closed"))
}

/// Platform adapter for the shared connection worker.
///
/// Binds candidate resolution, the connect + handshake executor, session
/// registration, and protocol diagnostics to this crate's network module.
struct PoolAdapter {
    identity: Arc<DeviceIdentity>,
    settings: Arc<Mutex<crypto::Settings>>,
}

impl tailsync_core::peer::delivery::ConnectionAdapter for PoolAdapter {
    type Connection = secure::SecureConnection;
    type SessionLease = tailsync_core::peer::health::SessionGuard;

    async fn connect(
        &self,
        hostname: &str,
        candidates: &[ResolvedCandidate],
    ) -> Result<(secure::SecureConnection, ResolvedCandidate), String> {
        race_connect_and_handshake(candidates, hostname, &self.identity, &self.settings).await
    }

    fn register_session(
        &self,
        hostname: &str,
        interface: ConnectionInterface,
        address: &str,
        latency_ms: u64,
    ) -> Self::SessionLease {
        register_active_session(hostname, interface, address, latency_ms)
    }

    fn record_protocol_error(&self, hostname: &str, error: &str) {
        record_protocol_compatibility_error(hostname, error);
    }

    fn clear_protocol_error(&self, hostname: &str) {
        clear_protocol_compatibility_error(hostname);
    }

    async fn refresh_candidates(
        &self,
        hostname: &str,
        candidates: &mut Vec<ResolvedCandidate>,
    ) -> bool {
        refresh_remembered_iroh_candidate(candidates, hostname, &self.settings).await
    }
}

/// Background task for one pooled connection: delegates the entire lifecycle
/// loop (reconnect, heartbeat, keep-frame, queue priority) to the shared
/// worker in `tailsync_core`.
pub(super) async fn connection_task(
    candidates: Vec<ResolvedCandidate>,
    hostname: String,
    priority_rx: mpsc::Receiver<QueuedFrame>,
    bulk_rx: mpsc::Receiver<QueuedFrame>,
    identity: Arc<DeviceIdentity>,
    settings: Arc<Mutex<crypto::Settings>>,
    shutdown: watch::Receiver<bool>,
) {
    let adapter = PoolAdapter { identity, settings };
    tailsync_core::peer::delivery::run_connection_worker(
        &adapter,
        &tailsync_core::peer::delivery::WorkerConfig::default(),
        candidates,
        hostname,
        priority_rx,
        bulk_rx,
        shutdown,
    )
    .await
}

async fn refresh_remembered_iroh_candidate(
    candidates: &mut Vec<ResolvedCandidate>,
    hostname: &str,
    settings: &Arc<Mutex<crypto::Settings>>,
) -> bool {
    let endpoint_id = {
        let settings = settings.lock().await;
        if settings.connection_mode != "auto" {
            candidates
                .retain(|candidate| candidate.candidate.interface != ConnectionInterface::Iroh);
            return false;
        }
        settings
            .trusted_peer_addresses
            .get(hostname)
            .and_then(|addresses| addresses.get("iroh"))
            .cloned()
    };
    let Some(endpoint_id) = endpoint_id else {
        return false;
    };
    let Ok(endpoint_id) = tailsync_core::iroh_transport::canonical_endpoint_id(&endpoint_id) else {
        return false;
    };
    candidates.retain(|candidate| {
        candidate.candidate.interface != ConnectionInterface::Iroh
            || candidate.candidate.address == endpoint_id
    });
    if candidates.iter().any(|candidate| {
        candidate.candidate.interface == ConnectionInterface::Iroh
            && candidate.candidate.address == endpoint_id
    }) {
        return false;
    }
    candidates.push(ResolvedCandidate {
        candidate: PeerCandidate::new(ConnectionInterface::Iroh, endpoint_id.clone()),
        target: ResolvedTarget::Iroh(endpoint_id),
    });
    candidates.sort_by_key(|candidate| candidate.candidate.priority);
    true
}

pub(super) async fn wait_for_shutdown(shutdown: &mut watch::Receiver<bool>) {
    if *shutdown.borrow() {
        return;
    }
    while shutdown.changed().await.is_ok() {
        if *shutdown.borrow() {
            return;
        }
    }
}
pub(super) async fn race_connect_and_handshake(
    candidates: &[ResolvedCandidate],
    hostname: &str,
    identity: &Arc<DeviceIdentity>,
    settings: &Arc<Mutex<crypto::Settings>>,
) -> Result<(secure::SecureConnection, ResolvedCandidate), String> {
    let identity = identity.clone();
    let settings = settings.clone();
    let hostname = hostname.to_string();
    tailsync_core::peer::delivery::race_connections(
        candidates,
        HANDSHAKE_TIMEOUT,
        move |target, _candidate| {
            let identity = identity.clone();
            let settings = settings.clone();
            let hostname = hostname.clone();
            async move {
                connect_and_handshake(&target, &hostname, &identity, &settings)
                    .await
                    .map_err(|error| error.to_string())
            }
        },
    )
    .await
}

async fn connect_and_handshake(
    target: &ResolvedTarget,
    hostname: &str,
    identity: &DeviceIdentity,
    settings: &Arc<Mutex<crypto::Settings>>,
) -> Result<secure::SecureConnection, Box<dyn std::error::Error + Send + Sync>> {
    let (expected_key, mode) = {
        let settings = settings.lock().await;
        (
            settings
                .trusted_peer_keys
                .get(hostname)
                .cloned()
                .ok_or_else(|| format!("Peer {hostname} is not paired"))?,
            settings.connection_mode.clone(),
        )
    };
    let expected_key = secure::decode_trusted_key(&expected_key)?;
    if matches!(target, ResolvedTarget::Iroh(_)) && mode != "auto" {
        return Err(std::io::Error::other("Iroh is only available in automatic mode").into());
    }
    let connection = match target {
        ResolvedTarget::Tcp(address) => {
            let stream = timeout(CONNECTION_TIMEOUT, TcpStream::connect(address)).await??;
            secure::connect(
                stream,
                identity,
                local_peer_identity(&mode),
                hostname,
                &expected_key,
            )
            .await?
        }
        ResolvedTarget::Iroh(endpoint_id) => {
            let endpoint = iroh::endpoint().await.map_err(std::io::Error::other)?;
            let stream = endpoint
                .connect(endpoint_id)
                .await
                .map_err(std::io::Error::other)?;
            secure::connect(
                stream,
                identity,
                local_peer_identity(&mode),
                hostname,
                &expected_key,
            )
            .await?
        }
    };

    if let ResolvedTarget::Iroh(endpoint_id) = target {
        let claimed = connection
            .peer_identity()
            .iroh_endpoint_id
            .as_deref()
            .ok_or_else(|| {
                std::io::Error::other("Peer did not bind its Noise identity to an Iroh endpoint")
            })?;
        let claimed = tailsync_core::iroh_transport::canonical_endpoint_id(claimed)
            .map_err(std::io::Error::other)?;
        if &claimed != endpoint_id {
            return Err(std::io::Error::other(
                "Peer Iroh endpoint does not match its Noise identity",
            )
            .into());
        }
    }

    if let Some(endpoint_id) = &connection.peer_identity().iroh_endpoint_id {
        if let Err(error) =
            settings
                .lock()
                .await
                .remember_peer_address(hostname, "iroh", endpoint_id)
        {
            warn!("Could not remember Iroh endpoint for {hostname}: {error}");
        }
    }
    Ok(connection)
}

// ═══════════════════════════════════════════════════════════════════
// TCP server (inbound connections from peers)
// ═══════════════════════════════════════════════════════════════════
