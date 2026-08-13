use super::*;

#[derive(Clone)]
pub(super) struct PoolSender {
    pub(super) priority: mpsc::Sender<QueuedFrame>,
    pub(super) bulk: mpsc::Sender<QueuedFrame>,
    pub(super) shutdown: watch::Sender<bool>,
}

pub(super) struct QueuedFrame {
    pub(super) command: Command,
    pub(super) payload: Vec<u8>,
    pub(super) acknowledgement: AckExpectation,
    pub(super) completion: Option<oneshot::Sender<Result<DeliveryReceipt, String>>>,
}

pub(super) struct PendingFrame {
    pub(super) queued: QueuedFrame,
    pub(super) sequence: u32,
}

impl PendingFrame {
    fn complete(mut self, result: Result<DeliveryReceipt, String>) {
        if let Some(completion) = self.queued.completion.take() {
            let _ = completion.send(result);
        }
    }
}

fn record_permanent_delivery_warning(hostname: &str, error: &str) {
    if error.contains("event timestamp is outside the accepted window") {
        tailsync_core::sync_warning::record_expired_event(hostname);
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum AckExpectation {
    None,
    Event(MessageId),
    File(TransferId),
    Batch(TransferId),
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DeliveryReceipt {
    pub next_offset: Option<u64>,
}

impl QueuedFrame {
    pub(super) fn new(command: Command, content: Vec<u8>) -> Result<Self, String> {
        let (payload, acknowledgement) =
            if matches!(command, Command::TextPayload | Command::ImagePayload) {
                let envelope = EventEnvelope::new(content);
                if envelope.encoded_len() > command.payload_limit() {
                    return Err(format!(
                        "{:?} reliable payload exceeds the {} byte limit",
                        command,
                        command.payload_limit()
                    ));
                }
                let message_id = envelope.message_id;
                (envelope.encode(), AckExpectation::Event(message_id))
            } else {
                if content.len() > command.payload_limit() {
                    return Err(format!(
                        "{:?} payload exceeds the {} byte limit",
                        command,
                        command.payload_limit()
                    ));
                }
                (content, AckExpectation::None)
            };
        Ok(Self {
            command,
            payload,
            acknowledgement,
            completion: None,
        })
    }

    fn confirmed_file(
        command: Command,
        payload: Vec<u8>,
        transfer_id: TransferId,
        completion: oneshot::Sender<Result<DeliveryReceipt, String>>,
    ) -> Result<Self, String> {
        if !matches!(
            command,
            Command::FileMeta | Command::FileChunk | Command::FileComplete
        ) {
            return Err(format!("{:?} is not a confirmable file command", command));
        }
        if payload.len() > command.payload_limit() {
            return Err(format!(
                "{:?} payload exceeds the {} byte limit",
                command,
                command.payload_limit()
            ));
        }
        Ok(Self {
            command,
            payload,
            acknowledgement: AckExpectation::File(transfer_id),
            completion: Some(completion),
        })
    }

    fn confirmed_batch(
        command: Command,
        payload: Vec<u8>,
        batch_id: TransferId,
        completion: oneshot::Sender<Result<DeliveryReceipt, String>>,
    ) -> Result<Self, String> {
        if !matches!(
            command,
            Command::FileBatchStart | Command::FileBatchComplete
        ) {
            return Err(format!("{:?} is not a confirmable batch command", command));
        }
        if payload.len() > command.payload_limit() {
            return Err(format!("{:?} payload exceeds the limit", command));
        }
        Ok(Self {
            command,
            payload,
            acknowledgement: AckExpectation::Batch(batch_id),
            completion: Some(completion),
        })
    }
}

impl PoolSender {
    fn request_shutdown(&self) {
        let _ = self.shutdown.send(true);
    }

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
}

pub struct ConnectionPool {
    pub(super) senders: HashMap<(ResolvedTarget, String), PoolSender>,
    batch_serializers: HashMap<String, Arc<Mutex<()>>>,
    identity: Arc<DeviceIdentity>,
    settings: Arc<Mutex<crypto::Settings>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum ResolvedTarget {
    Tcp(SocketAddr),
    Iroh(String),
}

impl std::fmt::Display for ResolvedTarget {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Tcp(address) => address.fmt(formatter),
            Self::Iroh(endpoint_id) => write!(formatter, "iroh:{endpoint_id}"),
        }
    }
}

#[derive(Clone)]
pub(super) struct ResolvedCandidate {
    pub(super) candidate: PeerCandidate,
    pub(super) target: ResolvedTarget,
}

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
    timeout(FILE_CONFIRM_TIMEOUT, completion_rx)
        .await
        .map_err(|_| format!("Timed out waiting for {:?} confirmation", command))?
        .map_err(|_| format!("Connection task for {} closed", peer.hostname))?
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
    timeout(FILE_CONFIRM_TIMEOUT, completion_rx)
        .await
        .map_err(|_| format!("Timed out waiting for {:?} confirmation", command))?
        .map_err(|_| format!("Connection task for {} closed", peer.hostname))?
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
    let command = queued.command;
    timeout(POOL_SEND_TIMEOUT, tx.channel_for(command).send(queued))
        .await
        .map_err(|_| format!("Timed out queueing frame for {target}"))?
        .map_err(|_| format!("Connection to {target} closed"))
}

/// Background task for one pooled connection.
///
/// - Connects + handshakes, then loops reading from `rx`.
/// - Each `(cmd, payload)` becomes a frame on the wire.
/// - Sends periodic heartbeats.
/// - Reconnects transparently on write errors.
pub(super) async fn connection_task(
    mut candidates: Vec<ResolvedCandidate>,
    hostname: String,
    mut priority_rx: mpsc::Receiver<QueuedFrame>,
    mut bulk_rx: mpsc::Receiver<QueuedFrame>,
    identity: Arc<DeviceIdentity>,
    settings: Arc<Mutex<crypto::Settings>>,
    mut shutdown: watch::Receiver<bool>,
) {
    let Some(preferred_target) = candidates.first().map(|candidate| candidate.target.clone())
    else {
        warn!("Connection task for {hostname} started without a route");
        return;
    };
    let mut pending: Option<PendingFrame> = None;
    let mut next_sequence = 1u32;
    loop {
        refresh_remembered_iroh_candidate(&mut candidates, &hostname, &settings).await;
        let connection = race_connect_and_handshake(&candidates, &hostname, &identity, &settings);
        let connection_result = tokio::select! {
            biased;
            _ = wait_for_shutdown(&mut shutdown) => return,
            result = connection => result,
        };
        let (mut stream, route) = match connection_result {
            Ok(result) => {
                clear_protocol_compatibility_error(&hostname);
                result
            }
            Err(e) => {
                if e.contains("Incompatible TailSync protocol:") {
                    record_protocol_compatibility_error(&hostname, &e);
                }
                warn!(
                    "Pool connect to {} ({}) failed: {} — retrying in {:?}",
                    preferred_target, hostname, e, RECONNECT_DELAY
                );
                tokio::select! {
                    biased;
                    _ = wait_for_shutdown(&mut shutdown) => return,
                    _ = tokio::time::sleep(RECONNECT_DELAY) => {}
                }
                continue;
            }
        };
        let learned_iroh =
            refresh_remembered_iroh_candidate(&mut candidates, &hostname, &settings).await;
        if learned_iroh
            && candidates
                .iter()
                .any(|candidate| candidate.candidate.priority < route.candidate.priority)
        {
            debug!("Learned a preferred Iroh route for {hostname}; reselecting path");
            continue;
        }
        let target = route.target.clone();
        let latency_ms = route.candidate.latency.unwrap_or_default();
        debug!(
            "Pool connected to {} via {} in {} ms",
            target,
            route.candidate.interface.as_str(),
            latency_ms
        );
        let _active_guard = register_active_session(
            &hostname,
            route.candidate.interface,
            &route.candidate.address,
            latency_ms,
        );

        let mut last_heartbeat = tokio::time::Instant::now();

        // A write can fail after the frame has been removed from the queue.
        // Keep that frame across reconnects so transient breaks do not lose
        // clipboard content silently.
        if let Some(frame) = pending.take() {
            let delivery = tokio::select! {
                biased;
                _ = wait_for_shutdown(&mut shutdown) => return,
                result = deliver_pending_frame(&mut stream, &frame) => result,
            };
            match delivery {
                Ok(receipt) => frame.complete(Ok(receipt)),
                Err(error) if is_permanent_delivery_error(&error) => {
                    record_permanent_delivery_warning(&hostname, &error);
                    warn!("Dropping event rejected by remote peer: {error}");
                    debug!("Rejected event route: {target}");
                    frame.complete(Err(error));
                }
                Err(error) => {
                    debug!(
                        "Pool delivery to {} failed: {error} — reselecting path",
                        target
                    );
                    pending = Some(frame);
                    continue;
                }
            }
        }

        // Inner loop: read from channel, write to wire
        loop {
            if last_heartbeat.elapsed() >= HEARTBEAT_INTERVAL {
                let Ok(hb) = Frame::try_new(Command::Heartbeat, 0, next_sequence, vec![]) else {
                    error!("Could not construct a heartbeat frame");
                    return;
                };
                next_sequence = next_sequence.wrapping_add(1).max(1);
                let heartbeat_ok = tokio::select! {
                    biased;
                    _ = wait_for_shutdown(&mut shutdown) => return,
                    result = async {
                        stream.write_frame(&hb).await.is_ok()
                            && matches!(
                                timeout(CONNECTION_TIMEOUT, stream.read_frame()).await,
                                Ok(Ok(Frame { command: Command::HeartbeatAck, .. }))
                            )
                    } => result,
                };
                if !heartbeat_ok {
                    debug!("Pool heartbeat to {} failed — reconnecting", target);
                    break;
                }
                last_heartbeat = tokio::time::Instant::now();
            }

            // Wait for next frame or heartbeat deadline
            let deadline = HEARTBEAT_INTERVAL.saturating_sub(last_heartbeat.elapsed());
            let next_frame = tokio::select! {
                biased;
                _ = wait_for_shutdown(&mut shutdown) => return,
                frame = priority_rx.recv() => Some(frame),
                frame = bulk_rx.recv() => Some(frame),
                _ = tokio::time::sleep(deadline) => None,
            };
            match next_frame {
                Some(Some(queued)) => {
                    let frame = PendingFrame {
                        queued,
                        sequence: next_sequence,
                    };
                    next_sequence = next_sequence.wrapping_add(1).max(1);
                    let delivery = tokio::select! {
                        biased;
                        _ = wait_for_shutdown(&mut shutdown) => return,
                        result = deliver_pending_frame(&mut stream, &frame) => result,
                    };
                    match delivery {
                        Ok(receipt) => frame.complete(Ok(receipt)),
                        Err(error) if is_permanent_delivery_error(&error) => {
                            record_permanent_delivery_warning(&hostname, &error);
                            warn!("Dropping event rejected by remote peer: {error}");
                            debug!("Rejected event route: {target}");
                            frame.complete(Err(error));
                        }
                        Err(error) => {
                            pending = Some(frame);
                            debug!(
                                "Pool delivery to {} failed: {error} — reselecting path",
                                target
                            );
                            break;
                        }
                    }
                }
                Some(None) => {
                    // All senders dropped — exit this connection for good
                    debug!("Pool channel for {} closed — shutting down", target);
                    return;
                }
                None => {
                    // Timeout — loop back to send heartbeat
                }
            }
        }
        // Outer loop: reconnect and try again
    }
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

pub(super) async fn deliver_pending_frame(
    stream: &mut secure::SecureConnection,
    pending: &PendingFrame,
) -> Result<DeliveryReceipt, String> {
    match pending.queued.acknowledgement {
        AckExpectation::None => {
            let frame = Frame::try_new(
                pending.queued.command,
                0,
                pending.sequence,
                pending.queued.payload.clone(),
            )
            .map_err(|error| error.to_string())?;
            stream
                .write_frame(&frame)
                .await
                .map_err(|error| error.to_string())?;
            Ok(DeliveryReceipt::default())
        }
        AckExpectation::Event(message_id) => {
            let envelope = EventEnvelope::decode(&pending.queued.payload)
                .map_err(|error| error.to_string())?;
            if envelope.message_id != message_id {
                return Err("queued event ID does not match its acknowledgement".to_string());
            }
            let frame = Frame::try_new(
                pending.queued.command,
                0,
                pending.sequence,
                pending.queued.payload.clone(),
            )
            .map_err(|error| error.to_string())?;
            deliver_event_frame(stream, pending, &frame, message_id).await?;
            Ok(DeliveryReceipt::default())
        }
        AckExpectation::File(transfer_id) => {
            let frame = Frame::try_new(
                pending.queued.command,
                0,
                pending.sequence,
                pending.queued.payload.clone(),
            )
            .map_err(|error| error.to_string())?;
            return deliver_file_frame(stream, pending, &frame, transfer_id).await;
        }
        AckExpectation::Batch(batch_id) => {
            let frame = Frame::try_new(
                pending.queued.command,
                0,
                pending.sequence,
                pending.queued.payload.clone(),
            )
            .map_err(|error| error.to_string())?;
            return deliver_batch_frame(stream, pending, &frame, batch_id).await;
        }
    }
}

async fn deliver_event_frame(
    stream: &mut secure::SecureConnection,
    pending: &PendingFrame,
    frame: &Frame,
    message_id: MessageId,
) -> Result<(), String> {
    for attempt in 0..EVENT_MAX_ATTEMPTS {
        stream
            .write_frame(frame)
            .await
            .map_err(|error| error.to_string())?;
        match timeout(EVENT_ACK_TIMEOUT, stream.read_frame()).await {
            Ok(Ok(ack)) if ack.command == Command::EventAck => {
                let acknowledged =
                    MessageId::from_ack_payload(&ack.payload).map_err(|error| error.to_string())?;
                if ack.sequence != pending.sequence || acknowledged != message_id {
                    return Err("received an acknowledgement for a different event".to_string());
                }
                return Ok(());
            }
            Ok(Ok(frame)) if frame.command == Command::PeerError => {
                return Err(format!(
                    "peer rejected event: {}",
                    String::from_utf8_lossy(&frame.payload)
                ));
            }
            Ok(Ok(frame)) => {
                return Err(format!("expected EventAck, received {:?}", frame.command));
            }
            Ok(Err(error)) => return Err(error.to_string()),
            Err(_) if attempt + 1 < EVENT_MAX_ATTEMPTS => {
                let multiplier = 1u32 << attempt;
                tokio::time::sleep(EVENT_RETRY_BASE_DELAY * multiplier).await;
            }
            Err(_) => {
                return Err(format!(
                    "event acknowledgement timed out after {EVENT_MAX_ATTEMPTS} attempts"
                ));
            }
        }
    }
    unreachable!("event retry loop always returns")
}

fn is_permanent_delivery_error(error: &str) -> bool {
    error.starts_with("peer rejected event:")
        || error.starts_with("peer rejected file:")
        || error.starts_with("peer rejected batch:")
}

async fn deliver_file_frame(
    stream: &mut secure::SecureConnection,
    pending: &PendingFrame,
    frame: &Frame,
    transfer_id: TransferId,
) -> Result<DeliveryReceipt, String> {
    for attempt in 0..EVENT_MAX_ATTEMPTS {
        stream
            .write_frame(frame)
            .await
            .map_err(|error| error.to_string())?;
        match timeout(FILE_ACK_TIMEOUT, stream.read_frame()).await {
            Ok(Ok(ack)) if matches!(ack.command, Command::FileAck | Command::FileResume) => {
                let offset = FileOffset::decode(&ack.payload).map_err(|error| error.to_string())?;
                if ack.sequence != pending.sequence || offset.transfer_id != transfer_id {
                    return Err("received a file acknowledgement for another transfer".to_string());
                }
                return Ok(DeliveryReceipt {
                    next_offset: Some(offset.next_offset),
                });
            }
            Ok(Ok(frame)) => {
                if frame.command == Command::PeerError {
                    return Err(format!(
                        "peer rejected file: {}",
                        String::from_utf8_lossy(&frame.payload)
                    ));
                }
                return Err(format!(
                    "expected file acknowledgement, received {:?}",
                    frame.command
                ));
            }
            Ok(Err(error)) => return Err(error.to_string()),
            Err(_) if attempt + 1 < EVENT_MAX_ATTEMPTS => {
                let multiplier = 1u32 << attempt;
                tokio::time::sleep(EVENT_RETRY_BASE_DELAY * multiplier).await;
            }
            Err(_) => {
                return Err(format!(
                    "file acknowledgement timed out after {EVENT_MAX_ATTEMPTS} attempts"
                ));
            }
        }
    }
    unreachable!("file retry loop always returns")
}

async fn deliver_batch_frame(
    stream: &mut secure::SecureConnection,
    pending: &PendingFrame,
    frame: &Frame,
    batch_id: TransferId,
) -> Result<DeliveryReceipt, String> {
    for attempt in 0..EVENT_MAX_ATTEMPTS {
        stream
            .write_frame(frame)
            .await
            .map_err(|error| error.to_string())?;
        match timeout(FILE_ACK_TIMEOUT, stream.read_frame()).await {
            Ok(Ok(ack)) if ack.command == Command::FileBatchAccept => {
                if ack.sequence != pending.sequence || ack.payload.as_slice() != batch_id.0 {
                    return Err("received an acknowledgement for another file batch".to_string());
                }
                return Ok(DeliveryReceipt::default());
            }
            Ok(Ok(reject)) if reject.command == Command::FileBatchReject => {
                return Err(format!(
                    "peer rejected batch: {}",
                    String::from_utf8_lossy(&reject.payload)
                ));
            }
            Ok(Ok(error)) if error.command == Command::PeerError => {
                return Err(format!(
                    "peer rejected batch: {}",
                    String::from_utf8_lossy(&error.payload)
                ));
            }
            Ok(Ok(other)) => {
                return Err(format!(
                    "expected batch acknowledgement, received {:?}",
                    other.command
                ));
            }
            Ok(Err(error)) => return Err(error.to_string()),
            Err(_) if attempt + 1 < EVENT_MAX_ATTEMPTS => {
                let multiplier = 1u32 << attempt;
                tokio::time::sleep(EVENT_RETRY_BASE_DELAY * multiplier).await;
            }
            Err(_) => return Err("file batch acknowledgement timed out".to_string()),
        }
    }
    unreachable!("batch retry loop always returns")
}

pub(super) async fn race_connect_and_handshake(
    candidates: &[ResolvedCandidate],
    hostname: &str,
    identity: &Arc<DeviceIdentity>,
    settings: &Arc<Mutex<crypto::Settings>>,
) -> Result<(secure::SecureConnection, ResolvedCandidate), String> {
    let has_lan = candidates
        .iter()
        .any(|candidate| candidate.candidate.interface == ConnectionInterface::Lan);
    let has_iroh = candidates
        .iter()
        .any(|candidate| candidate.candidate.interface == ConnectionInterface::Iroh);
    let (tx, mut rx) = mpsc::channel(candidates.len().max(1));
    let mut tasks = Vec::with_capacity(candidates.len());

    for candidate in candidates.iter().cloned() {
        let tx = tx.clone();
        let hostname = hostname.to_string();
        let identity = identity.clone();
        let settings = settings.clone();
        let delay = candidate_delay(candidate.candidate.interface, has_lan, has_iroh);
        tasks.push(tokio::spawn(async move {
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
            let started = tokio::time::Instant::now();
            let result = timeout(
                HANDSHAKE_TIMEOUT,
                connect_and_handshake(&candidate.target, &hostname, &identity, &settings),
            )
            .await
            .map_err(|_| "handshake timed out".to_string())
            .and_then(|result| result.map_err(|error| error.to_string()));
            let mut candidate = candidate;
            candidate.candidate.latency = Some(started.elapsed().as_millis() as u64);
            let _ = tx.send((candidate, result)).await;
        }));
    }
    drop(tx);

    let mut errors = Vec::new();
    while let Some((candidate, result)) = rx.recv().await {
        match result {
            Ok(stream) => {
                for task in tasks {
                    task.abort();
                }
                return Ok((stream, candidate));
            }
            Err(error) => errors.push(format!(
                "{} {}: {error}",
                candidate.candidate.interface.as_str(),
                candidate.target
            )),
        }
    }
    Err(errors.join("; "))
}

fn candidate_delay(interface: ConnectionInterface, has_lan: bool, has_iroh: bool) -> Duration {
    match interface {
        ConnectionInterface::Lan => Duration::ZERO,
        ConnectionInterface::Iroh if has_lan => Duration::from_millis(150),
        ConnectionInterface::Iroh => Duration::ZERO,
        ConnectionInterface::Tailscale if has_lan => Duration::from_millis(300),
        ConnectionInterface::Tailscale if has_iroh => Duration::from_millis(150),
        ConnectionInterface::Tailscale => Duration::ZERO,
    }
}

/// One-shot connect + handshake.  Returns an authenticated stream.
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
