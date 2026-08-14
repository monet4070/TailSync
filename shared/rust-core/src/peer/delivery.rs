//! Peer Delivery: reliable frame delivery shared by both platforms.
//!
//! This module owns the delivery path end to end: frame construction and
//! validation (event envelopes, confirmable file/batch frames), the typed
//! acknowledgement machinery ([`DeliveryError`]), the actual delivery
//! execution over an authenticated connection (retries with exponential
//! backoff, timeouts, permanent-rejection classification), and the
//! connection race policy ([`race_connections`]). The platform connection
//! pool owns the channels, sockets, and the per-peer lifecycle loop;
//! everything else lives here so both platforms share one implementation
//! and one test suite.

use std::future::Future;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::{timeout, Duration};

use crate::peer::types::{
    ActiveRoute, ConnectionInterface, DeliveryReceipt, ResolvedCandidate, ResolvedTarget,
};
use crate::protocol::{Command, EventEnvelope, FileOffset, Frame, MessageId, TransferId};
use crate::secure::SecureConnection;

/// Timing for one reliable delivery attempt. Defaults match the shared
/// platform constants (750 ms event ACK window, 10 s file ACK window,
/// 250 ms base retry delay, 4 attempts). Fields are private: construct via
/// [`DeliveryConfig::try_new`] so invariants (at least one attempt, bounded
/// backoff) cannot be violated.
#[derive(Debug, Clone, Copy)]
pub struct DeliveryConfig {
    event_ack_timeout: Duration,
    file_ack_timeout: Duration,
    event_retry_base_delay: Duration,
    max_attempts: usize,
}

impl DeliveryConfig {
    /// The shared delivery timing used by both platforms.
    pub const DEFAULT: Self = Self {
        event_ack_timeout: Duration::from_millis(750),
        file_ack_timeout: Duration::from_secs(10),
        event_retry_base_delay: Duration::from_millis(250),
        max_attempts: 4,
    };

    /// Build a config with validated timing. `max_attempts` applies to every
    /// acknowledged delivery (event, file, and batch frames) and must be
    /// between 1 and 8: the exponential backoff shifts `1 << attempt`, so
    /// keeping attempts bounded also keeps the delay arithmetic in range.
    pub fn try_new(
        event_ack_timeout: Duration,
        file_ack_timeout: Duration,
        event_retry_base_delay: Duration,
        max_attempts: usize,
    ) -> Result<Self, String> {
        if !(1..=8).contains(&max_attempts) {
            return Err(format!(
                "max_attempts must be between 1 and 8, got {max_attempts}"
            ));
        }
        Ok(Self {
            event_ack_timeout,
            file_ack_timeout,
            event_retry_base_delay,
            max_attempts,
        })
    }

    /// Retry delay before attempt `attempt` (0-based) of a delivery.
    fn retry_delay(&self, attempt: usize) -> Duration {
        self.event_retry_base_delay * (1u32 << attempt)
    }
}

impl Default for DeliveryConfig {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// A connection that can exchange protocol frames. Implemented by the
/// real [`SecureConnection`] and by in-memory pairs in tests, so the whole
/// delivery path (and the connection worker) is testable without sockets.
pub trait DeliveryConnection: Send {
    fn write_frame(
        &mut self,
        frame: &Frame,
    ) -> impl std::future::Future<Output = Result<(), String>> + Send;
    fn read_frame(&mut self) -> impl std::future::Future<Output = Result<Frame, String>> + Send;
}

impl DeliveryConnection for SecureConnection {
    async fn write_frame(&mut self, frame: &Frame) -> Result<(), String> {
        SecureConnection::write_frame(self, frame)
            .await
            .map_err(|error| error.to_string())
    }

    async fn read_frame(&mut self) -> Result<Frame, String> {
        SecureConnection::read_frame(self)
            .await
            .map_err(|error| error.to_string())
    }
}

/// What a queued frame expects from the peer before it counts as delivered.
#[derive(Debug, Clone, Copy)]
pub enum AckExpectation {
    None,
    Event(MessageId),
    File(TransferId),
    Batch(TransferId),
}

/// A frame queued for one peer, with its expected acknowledgement and an
/// optional completion callback for the caller that enqueued it. Fields are
/// private: frames can only be built through the constructors, which keep
/// command, payload, and acknowledgement internally consistent.
pub struct QueuedFrame {
    command: Command,
    payload: Vec<u8>,
    acknowledgement: AckExpectation,
    completion: Option<oneshot::Sender<Result<DeliveryReceipt, DeliveryError>>>,
}

impl QueuedFrame {
    pub fn command(&self) -> Command {
        self.command
    }

    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    pub fn acknowledgement(&self) -> AckExpectation {
        self.acknowledgement
    }
}

impl QueuedFrame {
    /// Build a frame for a command that is either acknowledged implicitly
    /// (control frames) or wrapped in an event envelope with a message ID
    /// (text and image payloads). Payloads are validated against the command
    /// limit at enqueue time so oversized frames never reach the wire.
    pub fn new(command: Command, content: Vec<u8>) -> Result<Self, String> {
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

    /// Build a text/image frame from an already-constructed event envelope,
    /// preserving its message ID and timestamp (needed for re-delivery of
    /// events with a specific age, e.g. the sleep/wake regression tests).
    pub fn new_with_envelope(command: Command, envelope: EventEnvelope) -> Result<Self, String> {
        if !matches!(command, Command::TextPayload | Command::ImagePayload) {
            return Err(format!("{command:?} is not an envelope-wrapped command"));
        }
        let payload = envelope.encode();
        if payload.len() > command.payload_limit() {
            return Err(format!(
                "{command:?} reliable payload exceeds the {} byte limit",
                command.payload_limit()
            ));
        }
        Ok(Self {
            command,
            payload,
            acknowledgement: AckExpectation::Event(envelope.message_id),
            completion: None,
        })
    }

    /// Build a file-control frame whose delivery is confirmed with a file
    /// offset receipt from the peer.
    pub fn confirmed_file(
        command: Command,
        payload: Vec<u8>,
        transfer_id: TransferId,
        completion: oneshot::Sender<Result<DeliveryReceipt, DeliveryError>>,
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

    /// Build a file-batch frame whose delivery is confirmed with a batch
    /// receipt from the peer.
    pub fn confirmed_batch(
        command: Command,
        payload: Vec<u8>,
        batch_id: TransferId,
        completion: oneshot::Sender<Result<DeliveryReceipt, DeliveryError>>,
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

/// A queued frame paired with its send sequence number.
pub struct PendingFrame {
    queued: QueuedFrame,
    sequence: u32,
}

impl PendingFrame {
    pub fn new(queued: QueuedFrame, sequence: u32) -> Self {
        Self { queued, sequence }
    }

    pub fn sequence(&self) -> u32 {
        self.sequence
    }

    /// Report the delivery result to the enqueuer, if one is waiting.
    pub fn complete(mut self, result: Result<DeliveryReceipt, DeliveryError>) {
        if let Some(completion) = self.queued.completion.take() {
            let _ = completion.send(result);
        }
    }
}

/// Record a platform warning for permanent delivery failures that carry
/// domain meaning (currently: expired event timestamps).
pub fn record_permanent_delivery_warning(hostname: &str, error: &str) {
    if error.contains("event timestamp is outside the accepted window") {
        crate::sync_warning::record_expired_event(hostname);
    }
}

/// Typed outcome of a delivery attempt. Retry policy is expressed through
/// [`DeliveryError::is_retryable`] instead of string matching, so wording
/// changes can never alter delivery behavior.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryError {
    /// The peer rejected the payload outright (permanent, do not retry).
    Rejected(String),
    /// The acknowledgement window expired (retry with backoff).
    Timeout(String),
    /// The transport failed while writing or reading (reconnect and retry).
    Transport(String),
    /// The peer answered with an unexpected or mismatched frame.
    Protocol(String),
}

impl std::fmt::Display for DeliveryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rejected(message) => write!(formatter, "peer rejected: {message}"),
            Self::Timeout(message) => write!(formatter, "delivery timed out: {message}"),
            Self::Transport(message) => write!(formatter, "transport failed: {message}"),
            Self::Protocol(message) => write!(formatter, "protocol mismatch: {message}"),
        }
    }
}

impl DeliveryError {
    /// Whether a retry (after backoff or reconnect) can plausibly succeed.
    /// Only outright peer rejections are permanent.
    pub fn is_retryable(&self) -> bool {
        !matches!(self, Self::Rejected(_))
    }

    fn rejected(message: impl Into<String>) -> Self {
        Self::Rejected(message.into())
    }

    fn protocol(message: impl Into<String>) -> Self {
        Self::Protocol(message.into())
    }

    fn transport(message: impl Into<String>) -> Self {
        Self::Transport(message.into())
    }
}

/// Delay before a candidate attempt starts, biased toward preferred
/// interfaces so LAN wins when available without serializing the race:
/// LAN starts immediately; Iroh and Tailscale give the faster route a
/// short head start.
pub fn candidate_delay(interface: ConnectionInterface, has_lan: bool, has_iroh: bool) -> Duration {
    match interface {
        ConnectionInterface::Lan => Duration::ZERO,
        ConnectionInterface::Iroh if has_lan => Duration::from_millis(150),
        ConnectionInterface::Iroh => Duration::ZERO,
        ConnectionInterface::Tailscale if has_lan => Duration::from_millis(300),
        ConnectionInterface::Tailscale if has_iroh => Duration::from_millis(150),
        ConnectionInterface::Tailscale => Duration::ZERO,
    }
}

/// Race connect attempts across all candidates in parallel, applying the
/// per-interface delay bias so preferred routes win without blocking
/// fallbacks. The first successful attempt wins and every remaining attempt
/// is cancelled; if all attempts fail, the collected errors are joined.
/// `connect` performs the actual connection and handshake for one route;
/// it receives owned route values so its future can be `'static`.
pub async fn race_connections<T, F, Fut>(
    candidates: &[ResolvedCandidate],
    handshake_timeout: Duration,
    connect: F,
) -> Result<(T, ResolvedCandidate), String>
where
    T: Send + 'static,
    F: Fn(ResolvedTarget, ResolvedCandidate) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<T, String>> + Send + 'static,
{
    if candidates.is_empty() {
        return Err("no connection candidates to race".to_string());
    }
    let has_lan = candidates
        .iter()
        .any(|candidate| candidate.candidate.interface == ConnectionInterface::Lan);
    let has_iroh = candidates
        .iter()
        .any(|candidate| candidate.candidate.interface == ConnectionInterface::Iroh);
    let (tx, mut rx) = mpsc::channel(candidates.len().max(1));
    // A JoinSet owns its tasks: dropping it (including early returns and
    // cancellation of the race future itself) aborts every outstanding
    // connect attempt, so no handshake outlives the race.
    let mut tasks = tokio::task::JoinSet::new();
    let connect = std::sync::Arc::new(connect);

    for candidate in candidates.iter().cloned() {
        let tx = tx.clone();
        let connect = connect.clone();
        let delay = candidate_delay(candidate.candidate.interface, has_lan, has_iroh);
        tasks.spawn(async move {
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
            let started = tokio::time::Instant::now();
            let result = timeout(
                handshake_timeout,
                connect(candidate.target.clone(), candidate.clone()),
            )
            .await
            .map_err(|_| "handshake timed out".to_string())
            .and_then(|result| result);
            let mut candidate = candidate;
            candidate.candidate.latency = Some(started.elapsed().as_millis() as u64);
            let _ = tx.send((candidate, result)).await;
        });
    }
    drop(tx);

    let mut errors = Vec::new();
    while let Some((candidate, result)) = rx.recv().await {
        match result {
            Ok(stream) => {
                tasks.abort_all();
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

/// Validate that an event ACK matches the pending frame's sequence and the
/// expected message ID. Rejects acknowledgements for other events so a stale
/// or cross-talk ACK can never complete the wrong delivery.
pub(crate) fn validate_event_ack(
    ack: &Frame,
    pending: &PendingFrame,
    message_id: MessageId,
) -> Result<(), DeliveryError> {
    let acknowledged = MessageId::from_ack_payload(&ack.payload)
        .map_err(|e| DeliveryError::protocol(e.to_string()))?;
    if ack.sequence != pending.sequence || acknowledged != message_id {
        return Err(DeliveryError::protocol(
            "received an acknowledgement for a different event",
        ));
    }
    Ok(())
}

/// Validate that a file ACK/resume matches the pending frame and transfer,
/// returning the next offset to continue from.
pub(crate) fn validate_file_ack(
    ack: &Frame,
    pending: &PendingFrame,
    transfer_id: TransferId,
) -> Result<DeliveryReceipt, DeliveryError> {
    let offset =
        FileOffset::decode(&ack.payload).map_err(|e| DeliveryError::protocol(e.to_string()))?;
    if ack.sequence != pending.sequence || offset.transfer_id != transfer_id {
        return Err(DeliveryError::protocol(
            "received a file acknowledgement for another transfer",
        ));
    }
    Ok(DeliveryReceipt {
        next_offset: Some(offset.next_offset),
    })
}

/// Deliver one pending frame over an authenticated connection, waiting for
/// and validating the expected acknowledgement. Event frames retry with an
/// exponential backoff; file and batch frames are retried on the file ACK
/// window. Peer rejections surface as permanent errors the caller must not
/// retry.
pub async fn deliver_pending_frame<T: DeliveryConnection>(
    stream: &mut T,
    pending: &PendingFrame,
    config: &DeliveryConfig,
) -> Result<DeliveryReceipt, DeliveryError> {
    match pending.queued.acknowledgement {
        AckExpectation::None => {
            let frame = Frame::try_new(
                pending.queued.command,
                0,
                pending.sequence,
                pending.queued.payload.clone(),
            )
            .map_err(|error| DeliveryError::protocol(error.to_string()))?;
            stream
                .write_frame(&frame)
                .await
                .map_err(|error| DeliveryError::transport(error.to_string()))?;
            Ok(DeliveryReceipt::default())
        }
        AckExpectation::Event(message_id) => {
            let envelope = EventEnvelope::decode(&pending.queued.payload)
                .map_err(|error| DeliveryError::protocol(error.to_string()))?;
            if envelope.message_id != message_id {
                return Err(DeliveryError::protocol(
                    "queued event ID does not match its acknowledgement",
                ));
            }
            let frame = Frame::try_new(
                pending.queued.command,
                0,
                pending.sequence,
                pending.queued.payload.clone(),
            )
            .map_err(|error| DeliveryError::protocol(error.to_string()))?;
            deliver_event_frame(stream, pending, &frame, message_id, config).await?;
            Ok(DeliveryReceipt::default())
        }
        AckExpectation::File(transfer_id) => {
            let frame = Frame::try_new(
                pending.queued.command,
                0,
                pending.sequence,
                pending.queued.payload.clone(),
            )
            .map_err(|error| DeliveryError::protocol(error.to_string()))?;
            deliver_file_frame(stream, pending, &frame, transfer_id, config).await
        }
        AckExpectation::Batch(batch_id) => {
            let frame = Frame::try_new(
                pending.queued.command,
                0,
                pending.sequence,
                pending.queued.payload.clone(),
            )
            .map_err(|error| DeliveryError::protocol(error.to_string()))?;
            deliver_batch_frame(stream, pending, &frame, batch_id, config).await
        }
    }
}

async fn deliver_event_frame<T: DeliveryConnection>(
    stream: &mut T,
    pending: &PendingFrame,
    frame: &Frame,
    message_id: MessageId,
    config: &DeliveryConfig,
) -> Result<(), DeliveryError> {
    for attempt in 0..config.max_attempts {
        stream
            .write_frame(frame)
            .await
            .map_err(|error| DeliveryError::transport(error.to_string()))?;
        match timeout(config.event_ack_timeout, stream.read_frame()).await {
            Ok(Ok(ack)) if ack.command == Command::EventAck => {
                validate_event_ack(&ack, pending, message_id)?;
                return Ok(());
            }
            Ok(Ok(frame)) if frame.command == Command::PeerError => {
                return Err(DeliveryError::rejected(format!(
                    "event: {}",
                    String::from_utf8_lossy(&frame.payload)
                )));
            }
            Ok(Ok(frame)) => {
                return Err(DeliveryError::protocol(format!(
                    "expected EventAck, received {:?}",
                    frame.command
                )));
            }
            Ok(Err(error)) => return Err(DeliveryError::transport(error.to_string())),
            Err(_) if attempt + 1 < config.max_attempts => {
                tokio::time::sleep(config.retry_delay(attempt)).await;
            }
            Err(_) => {
                return Err(DeliveryError::Timeout(format!(
                    "event acknowledgement timed out after {} attempts",
                    config.max_attempts
                )));
            }
        }
    }
    unreachable!("event retry loop always returns")
}

async fn deliver_file_frame<T: DeliveryConnection>(
    stream: &mut T,
    pending: &PendingFrame,
    frame: &Frame,
    transfer_id: TransferId,
    config: &DeliveryConfig,
) -> Result<DeliveryReceipt, DeliveryError> {
    for attempt in 0..config.max_attempts {
        stream
            .write_frame(frame)
            .await
            .map_err(|error| DeliveryError::transport(error.to_string()))?;
        match timeout(config.file_ack_timeout, stream.read_frame()).await {
            Ok(Ok(ack)) if matches!(ack.command, Command::FileAck | Command::FileResume) => {
                return validate_file_ack(&ack, pending, transfer_id);
            }
            Ok(Ok(frame)) => {
                if frame.command == Command::PeerError {
                    return Err(DeliveryError::rejected(format!(
                        "file: {}",
                        String::from_utf8_lossy(&frame.payload)
                    )));
                }
                return Err(DeliveryError::protocol(format!(
                    "expected file acknowledgement, received {:?}",
                    frame.command
                )));
            }
            Ok(Err(error)) => return Err(DeliveryError::transport(error.to_string())),
            Err(_) if attempt + 1 < config.max_attempts => {
                tokio::time::sleep(config.retry_delay(attempt)).await;
            }
            Err(_) => {
                return Err(DeliveryError::Timeout(format!(
                    "file acknowledgement timed out after {} attempts",
                    config.max_attempts
                )));
            }
        }
    }
    unreachable!("file retry loop always returns")
}

async fn deliver_batch_frame<T: DeliveryConnection>(
    stream: &mut T,
    pending: &PendingFrame,
    frame: &Frame,
    batch_id: TransferId,
    config: &DeliveryConfig,
) -> Result<DeliveryReceipt, DeliveryError> {
    for attempt in 0..config.max_attempts {
        stream
            .write_frame(frame)
            .await
            .map_err(|error| DeliveryError::transport(error.to_string()))?;
        match timeout(config.file_ack_timeout, stream.read_frame()).await {
            Ok(Ok(ack)) if ack.command == Command::FileBatchAccept => {
                if ack.sequence != pending.sequence || ack.payload.as_slice() != batch_id.0 {
                    return Err(DeliveryError::protocol(
                        "received an acknowledgement for another file batch",
                    ));
                }
                return Ok(DeliveryReceipt::default());
            }
            Ok(Ok(reject)) if reject.command == Command::FileBatchReject => {
                return Err(DeliveryError::rejected(format!(
                    "batch: {}",
                    String::from_utf8_lossy(&reject.payload)
                )));
            }
            Ok(Ok(error)) if error.command == Command::PeerError => {
                return Err(DeliveryError::rejected(format!(
                    "batch: {}",
                    String::from_utf8_lossy(&error.payload)
                )));
            }
            Ok(Ok(other)) => {
                return Err(DeliveryError::protocol(format!(
                    "expected batch acknowledgement, received {:?}",
                    other.command
                )));
            }
            Ok(Err(error)) => return Err(DeliveryError::transport(error.to_string())),
            Err(_) if attempt + 1 < config.max_attempts => {
                tokio::time::sleep(config.retry_delay(attempt)).await;
            }
            Err(_) => {
                return Err(DeliveryError::Timeout(
                    "file batch acknowledgement timed out".to_string(),
                ))
            }
        }
    }
    unreachable!("batch retry loop always returns")
}

/// Timing for the per-peer connection worker loop. Defaults match the shared
/// platform constants (30 s heartbeat, 10 s heartbeat ACK window, 5 s
/// reconnect delay).
#[derive(Debug, Clone, Copy)]
pub struct WorkerConfig {
    pub heartbeat_interval: Duration,
    pub heartbeat_ack_timeout: Duration,
    pub reconnect_delay: Duration,
    pub delivery: DeliveryConfig,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            heartbeat_interval: Duration::from_secs(30),
            heartbeat_ack_timeout: Duration::from_secs(10),
            reconnect_delay: Duration::from_secs(5),
            delivery: DeliveryConfig::DEFAULT,
        }
    }
}

/// Platform capabilities the per-peer connection worker needs. The adapter
/// owns candidate resolution, the connection + handshake executor, session
/// registration, protocol-compatibility diagnostics, and remembered-Iroh
/// refresh; the worker owns the lifecycle loop itself (reconnect, heartbeat,
/// keep-frame-across-reconnect, priority queue selection) so both platforms
/// run a single implementation.
#[allow(async_fn_in_trait)] // Concrete adapters; Send bounds are checked at the worker boundary.
pub trait ConnectionAdapter: Send + Sync {
    type Connection: DeliveryConnection;

    /// Connect and authenticate one route. Failures are retried by the
    /// worker after `reconnect_delay`.
    async fn connect(
        &self,
        hostname: &str,
        candidates: &[ResolvedCandidate],
    ) -> Result<(Self::Connection, ResolvedCandidate), String>;

    /// Record an authenticated session for the route (forces `connected`).
    fn register_session(
        &self,
        hostname: &str,
        interface: ConnectionInterface,
        address: &str,
        latency_ms: u64,
    );

    fn record_protocol_error(&self, hostname: &str, error: &str);
    fn clear_protocol_error(&self, hostname: &str);

    /// Refresh remembered Iroh candidates; returns true when a new preferred
    /// route was learned (the worker then reselects the path).
    async fn refresh_candidates(
        &self,
        hostname: &str,
        candidates: &mut Vec<ResolvedCandidate>,
    ) -> bool;
}

async fn wait_for_shutdown(shutdown: &mut watch::Receiver<bool>) {
    if *shutdown.borrow() {
        return;
    }
    while shutdown.changed().await.is_ok() {
        if *shutdown.borrow() {
            return;
        }
    }
}

/// Background worker for one pooled connection: connects and handshakes,
/// serves queued frames with heartbeat keepalive, reconnects transparently
/// on transient failures, and keeps the in-flight frame across reconnects so
/// clipboard content is never lost silently. Exits when all senders drop or
/// shutdown is signalled.
pub async fn run_connection_worker<A: ConnectionAdapter>(
    adapter: &A,
    config: &WorkerConfig,
    mut candidates: Vec<ResolvedCandidate>,
    hostname: String,
    mut priority_rx: mpsc::Receiver<QueuedFrame>,
    mut bulk_rx: mpsc::Receiver<QueuedFrame>,
    mut shutdown: watch::Receiver<bool>,
) {
    let Some(preferred_target) = candidates.first().map(|candidate| candidate.target.clone())
    else {
        log::warn!("Connection task for {hostname} started without a route");
        return;
    };
    let mut pending: Option<PendingFrame> = None;
    let mut next_sequence = 1u32;
    loop {
        adapter.refresh_candidates(&hostname, &mut candidates).await;
        let connection_result = {
            let connection = adapter.connect(&hostname, &candidates);
            tokio::pin!(connection);
            tokio::select! {
                biased;
                _ = wait_for_shutdown(&mut shutdown) => return,
                result = &mut connection => result,
            }
        };
        let (mut stream, route) = match connection_result {
            Ok(result) => {
                adapter.clear_protocol_error(&hostname);
                result
            }
            Err(error) => {
                if error.contains("Incompatible TailSync protocol:") {
                    adapter.record_protocol_error(&hostname, &error);
                }
                log::warn!(
                    "Pool connect to {} ({}) failed: {} — retrying in {:?}",
                    preferred_target,
                    hostname,
                    error,
                    config.reconnect_delay
                );
                tokio::select! {
                    biased;
                    _ = wait_for_shutdown(&mut shutdown) => return,
                    _ = tokio::time::sleep(config.reconnect_delay) => {}
                }
                continue;
            }
        };
        let learned_iroh = adapter.refresh_candidates(&hostname, &mut candidates).await;
        if learned_iroh
            && candidates
                .iter()
                .any(|candidate| candidate.candidate.priority < route.candidate.priority)
        {
            log::debug!("Learned a preferred Iroh route for {hostname}; reselecting path");
            continue;
        }
        let target = route.target.clone();
        let active = ActiveRoute {
            interface: route.candidate.interface,
            address: route.candidate.address.clone(),
            latency: route.candidate.latency.unwrap_or_default(),
        };
        log::debug!(
            "Pool connected to {} via {} in {} ms",
            target,
            active.interface.as_str(),
            active.latency
        );
        adapter.register_session(
            &hostname,
            active.interface,
            &route.candidate.address,
            active.latency,
        );

        let mut last_heartbeat = tokio::time::Instant::now();

        // A write can fail after the frame has been removed from the queue.
        // Keep that frame across reconnects so transient breaks do not lose
        // clipboard content silently.
        if let Some(frame) = pending.take() {
            let delivery = tokio::select! {
                biased;
                _ = wait_for_shutdown(&mut shutdown) => return,
                result = deliver_pending_frame(&mut stream, &frame, &config.delivery) => result,
            };
            match delivery {
                Ok(receipt) => frame.complete(Ok(receipt)),
                Err(error @ DeliveryError::Rejected(_)) => {
                    record_permanent_delivery_warning(&hostname, &error.to_string());
                    log::warn!("Dropping event rejected by remote peer: {error}");
                    log::debug!("Rejected event route: {target}");
                    frame.complete(Err(error));
                }
                Err(error) => {
                    log::debug!(
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
            if last_heartbeat.elapsed() >= config.heartbeat_interval {
                let Ok(hb) = Frame::try_new(Command::Heartbeat, 0, next_sequence, vec![]) else {
                    log::error!("Could not construct a heartbeat frame");
                    return;
                };
                next_sequence = next_sequence.wrapping_add(1).max(1);
                let heartbeat_ok = tokio::select! {
                    biased;
                    _ = wait_for_shutdown(&mut shutdown) => return,
                    result = async {
                        if stream.write_frame(&hb).await.is_err() {
                            return false;
                        }
                        matches!(
                            timeout(config.heartbeat_ack_timeout, stream.read_frame()).await,
                            Ok(Ok(Frame { command: Command::HeartbeatAck, .. }))
                        )
                    } => result,
                };
                if !heartbeat_ok {
                    log::debug!("Pool heartbeat to {} failed — reconnecting", target);
                    break;
                }
                last_heartbeat = tokio::time::Instant::now();
            }

            // Wait for next frame or heartbeat deadline
            let deadline = config
                .heartbeat_interval
                .saturating_sub(last_heartbeat.elapsed());
            let next_frame = async {
                tokio::select! {
                    biased;
                    frame = priority_rx.recv() => frame,
                    frame = bulk_rx.recv() => frame,
                }
            };
            let next = tokio::select! {
                biased;
                _ = wait_for_shutdown(&mut shutdown) => return,
                result = tokio::time::timeout(deadline, next_frame) => result,
            };
            match next {
                Ok(Some(queued)) => {
                    let frame = PendingFrame::new(queued, next_sequence);
                    next_sequence = next_sequence.wrapping_add(1).max(1);
                    let delivery = tokio::select! {
                        biased;
                        _ = wait_for_shutdown(&mut shutdown) => return,
                        result = deliver_pending_frame(&mut stream, &frame, &config.delivery) => result,
                    };
                    match delivery {
                        Ok(receipt) => frame.complete(Ok(receipt)),
                        Err(error @ DeliveryError::Rejected(_)) => {
                            record_permanent_delivery_warning(&hostname, &error.to_string());
                            log::warn!("Dropping event rejected by remote peer: {error}");
                            log::debug!("Rejected event route: {target}");
                            frame.complete(Err(error));
                        }
                        Err(error) => {
                            pending = Some(frame);
                            log::debug!(
                                "Pool delivery to {} failed: {error} — reselecting path",
                                target
                            );
                            break;
                        }
                    }
                }
                Ok(None) => {
                    // All senders dropped — exit this connection for good
                    log::debug!("Pool channel for {} closed — shutting down", target);
                    return;
                }
                Err(_) => {
                    // Timeout — loop back to send heartbeat
                }
            }
        }
        // Outer loop: reconnect and try again
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::DeviceIdentity;
    use crate::peer::types::PeerCandidate;
    use std::sync::Arc;

    fn transfer_id(byte: u8) -> TransferId {
        TransferId([byte; 16])
    }

    #[test]
    fn text_payload_is_wrapped_in_an_event_envelope() {
        let frame = QueuedFrame::new(Command::TextPayload, b"hello".to_vec()).unwrap();
        assert!(matches!(frame.acknowledgement, AckExpectation::Event(_)));
        let envelope = EventEnvelope::decode(&frame.payload).unwrap();
        assert_eq!(
            envelope.message_id,
            match frame.acknowledgement {
                AckExpectation::Event(id) => id,
                _ => unreachable!(),
            }
        );
    }

    #[test]
    fn control_payloads_are_acknowledged_implicitly() {
        let frame = QueuedFrame::new(Command::Heartbeat, b"ping".to_vec()).unwrap();
        assert!(matches!(frame.acknowledgement, AckExpectation::None));
        assert_eq!(frame.payload, b"ping");
    }

    #[test]
    fn oversized_payloads_are_rejected_at_enqueue_time() {
        let oversized = vec![0u8; Command::TextPayload.payload_limit() + 1];
        assert!(QueuedFrame::new(Command::TextPayload, oversized).is_err());
        let oversized_control = vec![0u8; Command::Heartbeat.payload_limit() + 1];
        assert!(QueuedFrame::new(Command::Heartbeat, oversized_control).is_err());
    }

    #[test]
    fn confirmed_file_rejects_non_file_commands() {
        let (tx, _rx) = oneshot::channel();
        assert!(QueuedFrame::confirmed_file(
            Command::TextPayload,
            vec![1, 2, 3],
            transfer_id(1),
            tx,
        )
        .is_err());
    }

    #[test]
    fn confirmed_batch_rejects_oversized_payloads() {
        let (tx, _rx) = oneshot::channel();
        let oversized = vec![0u8; Command::FileBatchStart.payload_limit() + 1];
        assert!(QueuedFrame::confirmed_batch(
            Command::FileBatchStart,
            oversized,
            transfer_id(2),
            tx,
        )
        .is_err());
    }

    #[test]
    fn completion_reports_the_result_to_the_enqueuer() {
        let (tx, mut rx) = oneshot::channel();
        let frame =
            QueuedFrame::confirmed_file(Command::FileChunk, vec![1, 2, 3], transfer_id(3), tx)
                .unwrap();
        let pending = PendingFrame {
            queued: frame,
            sequence: 7,
        };
        pending.complete(Ok(DeliveryReceipt {
            next_offset: Some(3),
        }));
        let receipt = rx.try_recv().unwrap().unwrap();
        assert_eq!(receipt.next_offset, Some(3));
    }

    #[test]
    fn delivery_error_retryability_is_typed() {
        assert!(!DeliveryError::Rejected("bad".into()).is_retryable());
        assert!(DeliveryError::Timeout("window".into()).is_retryable());
        assert!(DeliveryError::Transport("reset".into()).is_retryable());
        assert!(DeliveryError::Protocol("mismatch".into()).is_retryable());
        assert_eq!(
            DeliveryError::Rejected("bad".into()).to_string(),
            "peer rejected: bad"
        );
    }

    #[test]
    fn event_ack_must_match_sequence_and_message() {
        let message_id = MessageId([7u8; 16]);
        let frame = QueuedFrame::new(Command::TextPayload, b"hi".to_vec()).unwrap();
        let pending = PendingFrame {
            queued: frame,
            sequence: 3,
        };
        let good = Frame::try_new(Command::EventAck, 0, 3, message_id.ack_payload()).unwrap();
        validate_event_ack(&good, &pending, message_id).unwrap();

        let wrong_seq = Frame::try_new(Command::EventAck, 0, 4, message_id.ack_payload()).unwrap();
        assert!(validate_event_ack(&wrong_seq, &pending, message_id).is_err());

        let wrong_msg =
            Frame::try_new(Command::EventAck, 0, 3, MessageId([8u8; 16]).ack_payload()).unwrap();
        assert!(validate_event_ack(&wrong_msg, &pending, message_id).is_err());
    }

    #[test]
    fn file_ack_must_match_transfer_and_returns_offset() {
        let transfer = TransferId([9u8; 16]);
        let (tx, _rx) = oneshot::channel();
        let frame =
            QueuedFrame::confirmed_file(Command::FileChunk, vec![1, 2, 3], transfer, tx).unwrap();
        let pending = PendingFrame {
            queued: frame,
            sequence: 5,
        };
        let mut payload = Vec::new();
        payload.extend_from_slice(&transfer.0);
        payload.extend_from_slice(&42u64.to_be_bytes());
        let good = Frame::try_new(Command::FileAck, 0, 5, payload).unwrap();
        let receipt = validate_file_ack(&good, &pending, transfer).unwrap();
        assert_eq!(receipt.next_offset, Some(42));

        let mut wrong_payload = Vec::new();
        wrong_payload.extend_from_slice(&TransferId([10u8; 16]).0);
        wrong_payload.extend_from_slice(&42u64.to_be_bytes());
        let wrong = Frame::try_new(Command::FileAck, 0, 5, wrong_payload).unwrap();
        assert!(validate_file_ack(&wrong, &pending, transfer).is_err());
    }

    #[test]
    fn delivery_config_defaults_match_shared_constants() {
        let config = DeliveryConfig::default();
        assert_eq!(config.event_ack_timeout, Duration::from_millis(750));
        assert_eq!(config.file_ack_timeout, Duration::from_secs(10));
        assert_eq!(config.event_retry_base_delay, Duration::from_millis(250));
        assert_eq!(config.max_attempts, 4);
    }

    fn resolved_candidate(interface: ConnectionInterface, address: &str) -> ResolvedCandidate {
        ResolvedCandidate {
            candidate: PeerCandidate::new(interface, address),
            target: ResolvedTarget::Tcp(format!("{address}:19890").parse().unwrap()),
        }
    }
    #[test]
    fn candidate_delay_prefers_lan_without_serializing_fallbacks() {
        assert_eq!(
            candidate_delay(ConnectionInterface::Lan, true, true),
            Duration::ZERO
        );
        assert_eq!(
            candidate_delay(ConnectionInterface::Iroh, true, true),
            Duration::from_millis(150)
        );
        assert_eq!(
            candidate_delay(ConnectionInterface::Iroh, false, true),
            Duration::ZERO
        );
        assert_eq!(
            candidate_delay(ConnectionInterface::Tailscale, true, true),
            Duration::from_millis(300)
        );
        assert_eq!(
            candidate_delay(ConnectionInterface::Tailscale, false, true),
            Duration::from_millis(150)
        );
        assert_eq!(
            candidate_delay(ConnectionInterface::Tailscale, false, false),
            Duration::ZERO
        );
    }

    #[tokio::test]
    async fn race_wins_with_first_successful_attempt() {
        let candidates = vec![
            resolved_candidate(ConnectionInterface::Lan, "192.168.1.2"),
            resolved_candidate(ConnectionInterface::Tailscale, "100.64.0.2"),
        ];
        let (stream, winner) = race_connections(
            &candidates,
            Duration::from_secs(2),
            |target, _candidate| async move {
                if target.to_string().contains("192.168") {
                    Ok("lan-stream")
                } else {
                    Err("tailscale failed".to_string())
                }
            },
        )
        .await
        .unwrap();
        assert_eq!(stream, "lan-stream");
        assert_eq!(winner.candidate.interface, ConnectionInterface::Lan);
    }

    #[tokio::test]
    async fn race_joins_all_failures() {
        let candidates = vec![resolved_candidate(ConnectionInterface::Lan, "192.168.1.2")];
        let err = race_connections(
            &candidates,
            Duration::from_secs(1),
            |_target, _candidate| async move { Err::<(), String>("boom".to_string()) },
        )
        .await
        .unwrap_err();
        assert!(err.contains("lan") && err.contains("boom"));
    }

    #[tokio::test]
    async fn race_without_candidates_fails() {
        let err = race_connections::<(), _, _>(
            &[],
            Duration::from_secs(1),
            |_target, _candidate| async move { Ok(()) },
        )
        .await
        .unwrap_err();
        assert_eq!(err, "no connection candidates to race");
    }

    #[tokio::test]
    async fn race_applies_handshake_timeout() {
        let candidates = vec![resolved_candidate(ConnectionInterface::Lan, "192.168.1.2")];
        let err = race_connections(
            &candidates,
            Duration::from_millis(20),
            |_target, _candidate| async move {
                tokio::time::sleep(Duration::from_millis(200)).await;
                Ok::<&str, String>("late")
            },
        )
        .await
        .unwrap_err();
        assert!(err.contains("timed out"));
    }

    #[tokio::test]
    async fn race_aborts_remaining_attempts_on_first_success() {
        struct DropCounter(std::sync::Arc<std::sync::atomic::AtomicUsize>);
        impl Drop for DropCounter {
            fn drop(&mut self) {
                self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        }

        let dropped = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let completed = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let dropped_for_assert = dropped.clone();
        let completed_for_assert = completed.clone();
        let candidates = vec![
            resolved_candidate(ConnectionInterface::Lan, "192.168.1.2"),
            resolved_candidate(ConnectionInterface::Lan, "192.168.1.3"),
        ];
        let (stream, winner) = race_connections(
            &candidates,
            Duration::from_secs(2),
            move |target, _candidate| {
                let dropped = dropped.clone();
                let completed = completed.clone();
                async move {
                    if target.to_string().starts_with("192.168.1.2:") {
                        Ok("lan-stream")
                    } else {
                        // A slow losing attempt holding a drop guard: it must
                        // be aborted (and dropped) once the LAN attempt wins,
                        // before it could ever complete on its own.
                        let _guard = DropCounter(dropped);
                        tokio::time::sleep(Duration::from_millis(200)).await;
                        completed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        Ok::<&str, String>("tailscale-stream")
                    }
                }
            },
        )
        .await
        .unwrap();
        assert_eq!(stream, "lan-stream");
        assert_eq!(winner.candidate.interface, ConnectionInterface::Lan);

        // Give the abort a scheduling opportunity: the losing attempt must
        // have been dropped without ever completing.
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(
            dropped_for_assert.load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        assert_eq!(
            completed_for_assert.load(std::sync::atomic::Ordering::SeqCst),
            0
        );
    }

    // ------------------------------------------------------------------
    // Real delivery over an in-memory Noise connection pair. These tests
    // exercise the full delivery path (handshake, framing, ACK validation,
    // retry, rejection) without sockets, so both platforms share them.
    // ------------------------------------------------------------------

    fn server_identity() -> Arc<DeviceIdentity> {
        Arc::new(DeviceIdentity::generate_for_test())
    }

    fn server_peer_identity() -> crate::secure::PeerIdentity {
        crate::secure::PeerIdentity {
            hostname: "server".into(),
            tailscale_ip: String::new(),
            iroh_endpoint_id: None,
        }
    }

    fn client_peer_identity() -> crate::secure::PeerIdentity {
        crate::secure::PeerIdentity {
            hostname: "client".into(),
            tailscale_ip: String::new(),
            iroh_endpoint_id: None,
        }
    }

    /// Establish a Noise-authenticated pair over an in-memory duplex and
    /// return the client connection plus the server connection (the caller
    /// drives both sides from the test).
    async fn establish_pair(
        server_identity: &Arc<DeviceIdentity>,
        client_identity: &DeviceIdentity,
    ) -> (SecureConnection, SecureConnection) {
        let expected_key = server_identity.public_key().to_vec();
        let (client_io, server_io) = tokio::io::duplex(256 * 1024);
        let server_identity = server_identity.clone();
        let server = tokio::spawn(async move {
            let accepted =
                crate::secure::accept(server_io, &server_identity, server_peer_identity())
                    .await
                    .unwrap();
            let mut connection = accepted.connection;
            crate::secure::write_ready(&mut connection).await.unwrap();
            connection
        });
        let client = crate::secure::connect(
            client_io,
            client_identity,
            client_peer_identity(),
            "server",
            &expected_key,
        )
        .await
        .unwrap();
        let server = server.await.unwrap();
        (client, server)
    }

    #[tokio::test]
    async fn delivers_event_with_acknowledgement() {
        let server_identity = server_identity();
        let client_identity = DeviceIdentity::generate_for_test();
        let (mut client, mut server) = establish_pair(&server_identity, &client_identity).await;

        let server_task = tokio::spawn(async move {
            let frame = server.read_frame().await.unwrap();
            let message_id = EventEnvelope::decode(&frame.payload).unwrap().message_id;
            server
                .write_frame(
                    &Frame::try_new(
                        Command::EventAck,
                        0,
                        frame.sequence,
                        message_id.ack_payload(),
                    )
                    .unwrap(),
                )
                .await
                .unwrap();
        });

        let pending = PendingFrame::new(
            QueuedFrame::new(Command::TextPayload, b"hello".to_vec()).unwrap(),
            1,
        );
        let receipt = deliver_pending_frame(&mut client, &pending, &DeliveryConfig::DEFAULT)
            .await
            .unwrap();
        assert_eq!(receipt.next_offset, None);
        server_task.await.unwrap();
    }

    #[tokio::test]
    async fn event_rejection_is_permanent() {
        let server_identity = server_identity();
        let client_identity = DeviceIdentity::generate_for_test();
        let (mut client, mut server) = establish_pair(&server_identity, &client_identity).await;

        let server_task = tokio::spawn(async move {
            let frame = server.read_frame().await.unwrap();
            server
                .write_frame(
                    &Frame::try_new(Command::PeerError, 0, frame.sequence, b"bad event".to_vec())
                        .unwrap(),
                )
                .await
                .unwrap();
        });

        let pending = PendingFrame::new(
            QueuedFrame::new(Command::TextPayload, b"hello".to_vec()).unwrap(),
            2,
        );
        let err = deliver_pending_frame(&mut client, &pending, &DeliveryConfig::DEFAULT)
            .await
            .unwrap_err();
        assert!(matches!(err, DeliveryError::Rejected(_)));
        assert!(!err.is_retryable());
        server_task.await.unwrap();
    }

    #[tokio::test]
    async fn event_ack_for_another_message_is_a_protocol_error() {
        let server_identity = server_identity();
        let client_identity = DeviceIdentity::generate_for_test();
        let (mut client, mut server) = establish_pair(&server_identity, &client_identity).await;

        let server_task = tokio::spawn(async move {
            let frame = server.read_frame().await.unwrap();
            server
                .write_frame(
                    &Frame::try_new(
                        Command::EventAck,
                        0,
                        frame.sequence,
                        MessageId([0xEE; 16]).ack_payload(),
                    )
                    .unwrap(),
                )
                .await
                .unwrap();
        });

        let pending = PendingFrame::new(
            QueuedFrame::new(Command::TextPayload, b"hello".to_vec()).unwrap(),
            3,
        );
        let err = deliver_pending_frame(&mut client, &pending, &DeliveryConfig::DEFAULT)
            .await
            .unwrap_err();
        assert!(matches!(err, DeliveryError::Protocol(_)));
        assert!(err.is_retryable());
        server_task.await.unwrap();
    }

    #[tokio::test]
    async fn event_is_retried_with_the_same_sequence_until_acknowledged() {
        let server_identity = server_identity();
        let client_identity = DeviceIdentity::generate_for_test();
        let (mut client, mut server) = establish_pair(&server_identity, &client_identity).await;

        // The server reads the first attempt but stays silent; the client's
        // ACK window expires and it retries the identical frame. Only the
        // second attempt is acknowledged.
        let server_task = tokio::spawn(async move {
            let first = server.read_frame().await.unwrap();
            let retry = server.read_frame().await.unwrap();
            assert_eq!(retry.sequence, first.sequence);
            assert_eq!(retry.payload, first.payload);
            let message_id = EventEnvelope::decode(&retry.payload).unwrap().message_id;
            server
                .write_frame(
                    &Frame::try_new(
                        Command::EventAck,
                        0,
                        retry.sequence,
                        message_id.ack_payload(),
                    )
                    .unwrap(),
                )
                .await
                .unwrap();
        });

        let pending = PendingFrame::new(
            QueuedFrame::new(Command::TextPayload, b"reliable".to_vec()).unwrap(),
            42,
        );
        // Event ACK window is 750 ms; one silent round + ack must fit in the
        // 4 default attempts.
        deliver_pending_frame(&mut client, &pending, &DeliveryConfig::DEFAULT)
            .await
            .unwrap();
        server_task.await.unwrap();
    }

    #[tokio::test]
    async fn delivers_file_chunk_with_offset_ack() {
        let server_identity = server_identity();
        let client_identity = DeviceIdentity::generate_for_test();
        let (mut client, mut server) = establish_pair(&server_identity, &client_identity).await;
        let transfer = TransferId([0xAB; 16]);

        let server_task = tokio::spawn(async move {
            let frame = server.read_frame().await.unwrap();
            let ack = FileOffset {
                transfer_id: transfer,
                next_offset: 4096,
            };
            server
                .write_frame(
                    &Frame::try_new(Command::FileAck, 0, frame.sequence, ack.encode()).unwrap(),
                )
                .await
                .unwrap();
        });

        let (tx, _rx) = oneshot::channel();
        let pending = PendingFrame::new(
            QueuedFrame::confirmed_file(Command::FileChunk, vec![0u8; 8], transfer, tx).unwrap(),
            5,
        );
        let receipt = deliver_pending_frame(&mut client, &pending, &DeliveryConfig::DEFAULT)
            .await
            .unwrap();
        assert_eq!(receipt.next_offset, Some(4096));
        server_task.await.unwrap();
    }

    #[tokio::test]
    async fn delivers_batch_with_accept_ack() {
        let server_identity = server_identity();
        let client_identity = DeviceIdentity::generate_for_test();
        let (mut client, mut server) = establish_pair(&server_identity, &client_identity).await;
        let batch = TransferId([0xCD; 16]);

        let server_task = tokio::spawn(async move {
            let frame = server.read_frame().await.unwrap();
            server
                .write_frame(
                    &Frame::try_new(
                        Command::FileBatchAccept,
                        0,
                        frame.sequence,
                        batch.0.to_vec(),
                    )
                    .unwrap(),
                )
                .await
                .unwrap();
        });

        let (tx, _rx) = oneshot::channel();
        let pending = PendingFrame::new(
            QueuedFrame::confirmed_batch(Command::FileBatchStart, Vec::new(), batch, tx).unwrap(),
            6,
        );
        let receipt = deliver_pending_frame(&mut client, &pending, &DeliveryConfig::DEFAULT)
            .await
            .unwrap();
        assert_eq!(receipt.next_offset, None);
        server_task.await.unwrap();
    }

    // ------------------------------------------------------------------
    // Connection worker tests: the worker runs against in-memory frame
    // connections and a scripted fake adapter, so reconnect, keep-frame,
    // rejection, and shutdown behavior are testable without sockets.
    // ------------------------------------------------------------------

    struct MemoryConnection {
        io: tokio::io::DuplexStream,
    }

    impl DeliveryConnection for MemoryConnection {
        async fn write_frame(&mut self, frame: &Frame) -> Result<(), String> {
            let bytes = frame.encode();
            let len = (bytes.len() as u32).to_be_bytes();
            tokio::io::AsyncWriteExt::write_all(&mut self.io, &len)
                .await
                .map_err(|e| e.to_string())?;
            tokio::io::AsyncWriteExt::write_all(&mut self.io, &bytes)
                .await
                .map_err(|e| e.to_string())?;
            Ok(())
        }

        async fn read_frame(&mut self) -> Result<Frame, String> {
            let mut len = [0u8; 4];
            tokio::io::AsyncReadExt::read_exact(&mut self.io, &mut len)
                .await
                .map_err(|e| e.to_string())?;
            let mut buf = vec![0u8; u32::from_be_bytes(len) as usize];
            tokio::io::AsyncReadExt::read_exact(&mut self.io, &mut buf)
                .await
                .map_err(|e| e.to_string())?;
            let (frame, _) = Frame::decode(&buf).map_err(|e| e.to_string())?;
            Ok(frame)
        }
    }

    struct FakeAdapter {
        connects: tokio::sync::Mutex<
            std::collections::VecDeque<Result<(MemoryConnection, ResolvedCandidate), String>>,
        >,
        sessions: std::sync::Mutex<Vec<(String, ConnectionInterface, String, u64)>>,
        connect_calls: std::sync::atomic::AtomicUsize,
    }

    impl ConnectionAdapter for FakeAdapter {
        type Connection = MemoryConnection;

        async fn connect(
            &self,
            _hostname: &str,
            _candidates: &[ResolvedCandidate],
        ) -> Result<(MemoryConnection, ResolvedCandidate), String> {
            self.connect_calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.connects
                .lock()
                .await
                .pop_front()
                .unwrap_or_else(|| Err("no more connections scripted".to_string()))
        }

        fn register_session(
            &self,
            hostname: &str,
            interface: ConnectionInterface,
            address: &str,
            latency_ms: u64,
        ) {
            self.sessions.lock().unwrap().push((
                hostname.to_string(),
                interface,
                address.to_string(),
                latency_ms,
            ));
        }

        fn record_protocol_error(&self, _hostname: &str, _error: &str) {}
        fn clear_protocol_error(&self, _hostname: &str) {}

        async fn refresh_candidates(
            &self,
            _hostname: &str,
            _candidates: &mut Vec<ResolvedCandidate>,
        ) -> bool {
            false
        }
    }

    fn fast_worker_config() -> WorkerConfig {
        WorkerConfig {
            heartbeat_interval: Duration::from_secs(30),
            heartbeat_ack_timeout: Duration::from_millis(100),
            reconnect_delay: Duration::from_millis(10),
            delivery: DeliveryConfig::DEFAULT,
        }
    }

    fn fast_file_config() -> DeliveryConfig {
        DeliveryConfig::try_new(
            Duration::from_millis(50),
            Duration::from_millis(50),
            Duration::from_millis(10),
            1,
        )
        .unwrap()
    }

    fn scripted_adapter(
        connects: Vec<Result<(MemoryConnection, ResolvedCandidate), String>>,
    ) -> FakeAdapter {
        FakeAdapter {
            connects: tokio::sync::Mutex::new(connects.into()),
            sessions: std::sync::Mutex::new(Vec::new()),
            connect_calls: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    #[tokio::test]
    async fn worker_delivers_frames_and_registers_session() {
        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let candidate = resolved_candidate(ConnectionInterface::Lan, "192.168.1.2");
        let adapter = std::sync::Arc::new(scripted_adapter(vec![Ok((
            MemoryConnection { io: client_io },
            candidate.clone(),
        ))]));

        let server = tokio::spawn(async move {
            let mut server = MemoryConnection { io: server_io };
            let frame = server.read_frame().await.unwrap();
            let transfer = TransferId([0x11; 16]);
            let ack = FileOffset {
                transfer_id: transfer,
                next_offset: 1024,
            };
            server
                .write_frame(
                    &Frame::try_new(Command::FileAck, 0, frame.sequence, ack.encode()).unwrap(),
                )
                .await
                .unwrap();
        });

        let (priority_tx, priority_rx) = mpsc::channel(4);
        let (_bulk_tx, bulk_rx) = mpsc::channel(4);
        let (_shutdown_tx, shutdown_rx) = watch::channel(false);
        let adapter_for_worker = adapter.clone();
        let worker = tokio::spawn(async move {
            run_connection_worker(
                adapter_for_worker.as_ref(),
                &fast_worker_config(),
                vec![candidate],
                "peer".into(),
                priority_rx,
                bulk_rx,
                shutdown_rx,
            )
            .await
        });

        let (completion_tx, mut completion_rx) = oneshot::channel();
        let queued = QueuedFrame::confirmed_file(
            Command::FileChunk,
            vec![0u8; 8],
            TransferId([0x11; 16]),
            completion_tx,
        )
        .unwrap();
        priority_tx.send(queued).await.unwrap();

        let receipt = timeout(Duration::from_secs(2), &mut completion_rx)
            .await
            .expect("delivery completion")
            .unwrap()
            .unwrap();
        assert_eq!(receipt.next_offset, Some(1024));
        server.await.unwrap();

        // Worker keeps running after the delivery: drop senders to let it
        // exit, then verify the session was registered.
        drop(priority_tx);
        timeout(Duration::from_secs(2), worker)
            .await
            .unwrap()
            .unwrap();
        let sessions = adapter.sessions.lock().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].0, "peer");
        assert_eq!(sessions[0].1, ConnectionInterface::Lan);
    }

    #[tokio::test]
    async fn worker_keeps_pending_frame_across_reconnect() {
        let (client_a, server_a) = tokio::io::duplex(64 * 1024);
        let (client_b, server_b) = tokio::io::duplex(64 * 1024);
        let candidate = resolved_candidate(ConnectionInterface::Lan, "192.168.1.2");
        let adapter = std::sync::Arc::new(scripted_adapter(vec![
            Ok((MemoryConnection { io: client_a }, candidate.clone())),
            Ok((MemoryConnection { io: client_b }, candidate.clone())),
        ]));

        // First connection: the server reads the frame but stays silent, so
        // the delivery times out (file ACK window 50 ms, one attempt) and the
        // worker reconnects with the frame kept pending.
        let server_a_task = tokio::spawn(async move {
            let mut server = MemoryConnection { io: server_a };
            let _frame = server.read_frame().await.unwrap();
            tokio::time::sleep(Duration::from_millis(200)).await;
        });
        // Second connection: the server acknowledges the retried frame.
        let server_b_task = tokio::spawn(async move {
            let mut server = MemoryConnection { io: server_b };
            let frame = server.read_frame().await.unwrap();
            let ack = FileOffset {
                transfer_id: TransferId([0x22; 16]),
                next_offset: 2048,
            };
            server
                .write_frame(
                    &Frame::try_new(Command::FileAck, 0, frame.sequence, ack.encode()).unwrap(),
                )
                .await
                .unwrap();
        });

        let mut config = fast_worker_config();
        config.delivery = fast_file_config();
        let config = std::sync::Arc::new(config);
        let (priority_tx, priority_rx) = mpsc::channel(4);
        let (_bulk_tx, bulk_rx) = mpsc::channel(4);
        let (_shutdown_tx, shutdown_rx) = watch::channel(false);
        let adapter_for_worker = adapter.clone();
        let config_for_worker = config.clone();
        let worker = tokio::spawn(async move {
            run_connection_worker(
                adapter_for_worker.as_ref(),
                &config_for_worker,
                vec![candidate],
                "peer".into(),
                priority_rx,
                bulk_rx,
                shutdown_rx,
            )
            .await
        });

        let (completion_tx, mut completion_rx) = oneshot::channel();
        let queued = QueuedFrame::confirmed_file(
            Command::FileChunk,
            vec![0u8; 8],
            TransferId([0x22; 16]),
            completion_tx,
        )
        .unwrap();
        priority_tx.send(queued).await.unwrap();

        // The frame must be delivered on the second connection with the
        // same transfer, proving it survived the reconnect.
        let receipt = timeout(Duration::from_secs(3), &mut completion_rx)
            .await
            .expect("retried delivery completion")
            .unwrap()
            .unwrap();
        assert_eq!(receipt.next_offset, Some(2048));

        drop(priority_tx);
        timeout(Duration::from_secs(2), worker)
            .await
            .unwrap()
            .unwrap();
        server_a_task.await.unwrap();
        server_b_task.await.unwrap();
        assert_eq!(
            adapter
                .connect_calls
                .load(std::sync::atomic::Ordering::SeqCst),
            2
        );
    }

    #[tokio::test]
    async fn worker_drops_permanently_rejected_frames() {
        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let candidate = resolved_candidate(ConnectionInterface::Lan, "192.168.1.2");
        let adapter = std::sync::Arc::new(scripted_adapter(vec![Ok((
            MemoryConnection { io: client_io },
            candidate.clone(),
        ))]));

        let server = tokio::spawn(async move {
            let mut server = MemoryConnection { io: server_io };
            let frame = server.read_frame().await.unwrap();
            server
                .write_frame(
                    &Frame::try_new(
                        Command::PeerError,
                        0,
                        frame.sequence,
                        b"rejected batch: quota".to_vec(),
                    )
                    .unwrap(),
                )
                .await
                .unwrap();
        });

        let (priority_tx, priority_rx) = mpsc::channel(4);
        let (_bulk_tx, bulk_rx) = mpsc::channel(4);
        let (_shutdown_tx, shutdown_rx) = watch::channel(false);
        let adapter_for_worker = adapter.clone();
        let worker = tokio::spawn(async move {
            run_connection_worker(
                adapter_for_worker.as_ref(),
                &fast_worker_config(),
                vec![candidate],
                "peer".into(),
                priority_rx,
                bulk_rx,
                shutdown_rx,
            )
            .await
        });

        let (completion_tx, mut completion_rx) = oneshot::channel();
        let queued = QueuedFrame::confirmed_batch(
            Command::FileBatchStart,
            Vec::new(),
            TransferId([0x33; 16]),
            completion_tx,
        )
        .unwrap();
        priority_tx.send(queued).await.unwrap();

        let err = timeout(Duration::from_secs(2), &mut completion_rx)
            .await
            .expect("rejection completion")
            .unwrap()
            .expect_err("rejected delivery must fail");
        assert!(matches!(err, DeliveryError::Rejected(_)));
        server.await.unwrap();

        // The worker keeps serving (rejections are dropped, not fatal):
        // drop senders and confirm clean exit without any reconnect.
        drop(priority_tx);
        timeout(Duration::from_secs(2), worker)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            adapter
                .connect_calls
                .load(std::sync::atomic::Ordering::SeqCst),
            1
        );
    }

    #[tokio::test]
    async fn worker_exits_on_shutdown() {
        let (client_io, _server_io) = tokio::io::duplex(64 * 1024);
        let candidate = resolved_candidate(ConnectionInterface::Lan, "192.168.1.2");
        let adapter = std::sync::Arc::new(scripted_adapter(vec![Ok((
            MemoryConnection { io: client_io },
            candidate.clone(),
        ))]));

        let (priority_tx, priority_rx) = mpsc::channel(4);
        let (_bulk_tx, bulk_rx) = mpsc::channel(4);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let adapter_for_worker = adapter.clone();
        let worker = tokio::spawn(async move {
            run_connection_worker(
                adapter_for_worker.as_ref(),
                &fast_worker_config(),
                vec![candidate],
                "peer".into(),
                priority_rx,
                bulk_rx,
                shutdown_rx,
            )
            .await
        });

        // Let the worker connect first, then signal shutdown.
        tokio::time::sleep(Duration::from_millis(50)).await;
        shutdown_tx.send(true).unwrap();
        timeout(Duration::from_secs(2), worker)
            .await
            .expect("worker must exit on shutdown")
            .unwrap();
        drop(priority_tx);
    }
}
