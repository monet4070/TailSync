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

use std::sync::Arc;
use tokio::sync::oneshot;
use tokio::time::{Duration, Instant};

use crate::peer::types::DeliveryReceipt;
use crate::protocol::{
    Command, EventEnvelope, Frame, MessageId, TransferId, EVENT_ENVELOPE_HEADER_SIZE,
};
use crate::secure::{CapabilitySet, SecureConnection};

/// Keep interactive clipboard traffic responsive without allowing a
/// continuously readable priority queue to starve file-transfer traffic.
const PRIORITY_BURST_LIMIT: usize = 8;

/// Timing for one reliable delivery attempt. Defaults match the shared
/// platform constants (750 ms event ACK window, 10 s file ACK window).
/// Fields are private: construct via
/// [`DeliveryConfig::try_new`] so neither acknowledgement timeout can be zero.
#[derive(Debug, Clone, Copy)]
pub struct DeliveryConfig {
    event_ack_timeout: Duration,
    file_ack_timeout: Duration,
}

impl DeliveryConfig {
    /// The shared delivery timing used by both platforms.
    pub const DEFAULT: Self = Self {
        event_ack_timeout: Duration::from_millis(750),
        file_ack_timeout: Duration::from_secs(10),
    };

    /// Build a config with validated acknowledgement timeouts. A timeout ends
    /// the connection; the worker retries queued frames on a fresh session.
    pub fn try_new(
        event_ack_timeout: Duration,
        file_ack_timeout: Duration,
    ) -> Result<Self, String> {
        if event_ack_timeout.is_zero() || file_ack_timeout.is_zero() {
            return Err("acknowledgement timeouts must be positive".into());
        }
        Ok(Self {
            event_ack_timeout,
            file_ack_timeout,
        })
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

    /// A completed secure connection exposes a transcript-derived correlation
    /// ID. In-memory test connections and other adapters may omit it.
    fn session_id(&self) -> Option<&str> {
        None
    }

    /// Extensions negotiated for this connection. Test adapters and legacy
    /// transports fail closed unless they explicitly override this method.
    fn negotiated_capabilities(&self) -> CapabilitySet {
        CapabilitySet::disabled()
    }
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

    fn session_id(&self) -> Option<&str> {
        Some(SecureConnection::session_id(self))
    }

    fn negotiated_capabilities(&self) -> CapabilitySet {
        SecureConnection::negotiated_capabilities(self)
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

/// The wire bytes of a queued frame. Most frames uniquely own their payload;
/// a broadcast to several peers instead shares one reference-counted buffer
/// (see [`SharedEvent`]) so a large image is encoded once and never copied
/// per peer.
enum Payload {
    Owned(Vec<u8>),
    Shared(Arc<[u8]>),
}

impl Payload {
    fn as_slice(&self) -> &[u8] {
        match self {
            Payload::Owned(bytes) => bytes,
            Payload::Shared(bytes) => bytes,
        }
    }
}

/// A frame queued for one peer, with its expected acknowledgement and an
/// optional completion callback for the caller that enqueued it. Fields are
/// private: frames can only be built through the constructors, which keep
/// command, payload, and acknowledgement internally consistent.
type CompositeFallbackBody = (
    Command,
    Arc<[u8]>,
    Arc<crate::composite::budgets::PipelineLease>,
);
type CompositeFallback = Arc<std::sync::Mutex<Option<Result<CompositeFallbackBody, String>>>>;

/// Encoded composite chunks, computed once and reused.
///
/// R4-04: every delivery attempt used to re-run `chunks::encode`, which re-compresses
/// the whole payload. An event's identity — and therefore its chosen encoding — is
/// fixed for the life of the frame, so the compressed bytes are immutable and can be
/// shared across retries instead of recomputed. The cache is released with the frame,
/// which is also what bounds it: an expired, terminal or revoked event drops it.
struct CachedChunks {
    chunks: Vec<Vec<u8>>,
    _memory: crate::composite::budgets::PipelineLease,
}
impl CachedChunks {
    fn owner_bytes() -> usize {
        std::mem::size_of::<Self>() + 2 * std::mem::size_of::<usize>()
    }

    /// Compression can fall back to the original body. Admit all chunk headers
    /// and vector storage before the encoder allocates the retained cache.
    fn memory_bound(content_bytes: usize) -> u64 {
        let count = content_bytes.div_ceil(crate::composite::CHUNK_DATA_LIMIT);
        (content_bytes
            + count * (crate::composite::CHUNK_HEADER_SIZE + std::mem::size_of::<Vec<u8>>())
            + Self::owner_bytes()) as u64
    }

    fn allocated_bytes(chunks: &[Vec<u8>], outer_capacity: usize) -> u64 {
        (chunks.iter().map(Vec::capacity).sum::<usize>()
            + outer_capacity * std::mem::size_of::<Vec<u8>>()
            + Self::owner_bytes()) as u64
    }
}
type CompositeChunks = Arc<std::sync::Mutex<Option<Result<Arc<CachedChunks>, String>>>>;
fn admit_composite_wire(
    command: Command,
    bytes: usize,
) -> Result<Option<Arc<crate::composite::budgets::PipelineLease>>, String> {
    if !matches!(
        command,
        Command::CompositePayload | Command::ArchiveUpdatePayload
    ) {
        return Ok(None);
    }
    crate::composite::budgets::global()
        .reserve_pipeline(bytes as u64)
        .map(|lease| Some(Arc::new(lease)))
        .map_err(|e| e.to_string())
}

enum CompositeSelection {
    Portable,
    Legacy {
        command: Command,
        message_id: MessageId,
        timestamp_ms: i64,
        content: Arc<[u8]>,
    },
}

pub struct QueuedFrame {
    command: Command,
    payload: Payload,
    acknowledgement: AckExpectation,
    completion: Option<oneshot::Sender<Result<DeliveryReceipt, DeliveryError>>>,
    enqueued_at: Instant,
    file_window: Option<Vec<Vec<u8>>>,
    composite_fallback: CompositeFallback,
    composite_chunks: CompositeChunks,
    _composite_memory: Option<Arc<crate::composite::budgets::PipelineLease>>,
    archive_authorization: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
}

impl QueuedFrame {
    pub fn with_archive_authorization(
        mut self,
        authorized: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<Self, String> {
        if self.command != Command::ArchiveUpdatePayload {
            return Err("Only archive updates accept this authorization guard".into());
        }
        self.archive_authorization = Some(authorized);
        Ok(self)
    }

    pub fn sequence_span(&self) -> u32 {
        if matches!(
            self.command,
            Command::ImagePayload | Command::CompositePayload | Command::ArchiveUpdatePayload
        ) {
            64
        } else {
            self.file_window
                .as_ref()
                .map_or(1, |chunks| chunks.len() as u32)
        }
    }

    pub fn command(&self) -> Command {
        self.command
    }

    pub fn payload(&self) -> &[u8] {
        self.payload.as_slice()
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
        let composite_memory = admit_composite_wire(command, content.len())?;
        let (payload, acknowledgement) = if matches!(
            command,
            Command::TextPayload
                | Command::ImagePayload
                | Command::CompositePayload
                | Command::ArchiveUpdatePayload
        ) {
            let envelope = EventEnvelope::new(content);
            if envelope.encoded_len() > command.payload_limit() {
                return Err(format!(
                    "{:?} reliable payload exceeds the {} byte limit",
                    command,
                    command.payload_limit()
                ));
            }
            let message_id = envelope.message_id;
            (
                Payload::Owned(envelope.encode()),
                AckExpectation::Event(message_id),
            )
        } else {
            if content.len() > command.payload_limit() {
                return Err(format!(
                    "{:?} payload exceeds the {} byte limit",
                    command,
                    command.payload_limit()
                ));
            }
            (Payload::Owned(content), AckExpectation::None)
        };
        Ok(Self {
            command,
            payload,
            acknowledgement,
            completion: None,
            enqueued_at: Instant::now(),
            file_window: None,
            composite_fallback: Arc::new(std::sync::Mutex::new(None)),
            composite_chunks: Arc::new(std::sync::Mutex::new(None)),
            _composite_memory: composite_memory,
            archive_authorization: None,
        })
    }

    /// Build a text/image frame from an already-constructed event envelope,
    /// preserving its message ID and timestamp (needed for re-delivery of
    /// events with a specific age, e.g. the sleep/wake regression tests).
    pub fn new_with_envelope(command: Command, envelope: EventEnvelope) -> Result<Self, String> {
        let composite_memory = admit_composite_wire(command, envelope.content.len())?;
        if !matches!(
            command,
            Command::TextPayload
                | Command::ImagePayload
                | Command::CompositePayload
                | Command::ArchiveUpdatePayload
        ) {
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
            payload: Payload::Owned(payload),
            acknowledgement: AckExpectation::Event(envelope.message_id),
            completion: None,
            enqueued_at: Instant::now(),
            file_window: None,
            composite_fallback: Arc::new(std::sync::Mutex::new(None)),
            composite_chunks: Arc::new(std::sync::Mutex::new(None)),
            _composite_memory: composite_memory,
            archive_authorization: None,
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
            payload: Payload::Owned(payload),
            acknowledgement: AckExpectation::File(transfer_id),
            completion: Some(completion),
            enqueued_at: Instant::now(),
            file_window: None,
            composite_fallback: Arc::new(std::sync::Mutex::new(None)),
            composite_chunks: Arc::new(std::sync::Mutex::new(None)),
            _composite_memory: None,
            archive_authorization: None,
        })
    }

    /// A bounded four MiB file window. The worker sends these chunks on one
    /// authenticated connection and consumes one cumulative ACK per chunk.
    pub fn confirmed_file_window(
        chunks: Vec<Vec<u8>>,
        transfer_id: TransferId,
        completion: oneshot::Sender<Result<DeliveryReceipt, DeliveryError>>,
    ) -> Result<Self, String> {
        if chunks.is_empty() || chunks.len() > 4 {
            return Err("file window must contain one to four chunks".into());
        }
        let mut expected_offset = None;
        for payload in &chunks {
            if payload.len() > Command::FileChunk.payload_limit() {
                return Err("file window chunk exceeds payload limit".into());
            }
            let chunk = crate::protocol::FileChunkPayload::decode(payload)
                .map_err(|error| error.to_string())?;
            if chunk.transfer_id != transfer_id {
                return Err("file window contains another transfer".into());
            }
            if expected_offset.is_some_and(|offset| chunk.offset != offset) {
                return Err("file window chunks must be contiguous".into());
            }
            expected_offset = chunk.offset.checked_add(chunk.data.len() as u64);
            if expected_offset.is_none() {
                return Err("file window offset overflow".into());
            }
        }
        Ok(Self {
            command: Command::FileChunk,
            payload: Payload::Owned(Vec::new()),
            acknowledgement: AckExpectation::File(transfer_id),
            completion: Some(completion),
            enqueued_at: Instant::now(),
            file_window: Some(chunks),
            composite_fallback: Arc::new(std::sync::Mutex::new(None)),
            composite_chunks: Arc::new(std::sync::Mutex::new(None)),
            _composite_memory: None,
            archive_authorization: None,
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
            payload: Payload::Owned(payload),
            acknowledgement: AckExpectation::Batch(batch_id),
            completion: Some(completion),
            enqueued_at: Instant::now(),
            file_window: None,
            composite_fallback: Arc::new(std::sync::Mutex::new(None)),
            composite_chunks: Arc::new(std::sync::Mutex::new(None)),
            _composite_memory: None,
            archive_authorization: None,
        })
    }
}

/// A text or image event, encoded exactly once so a single broadcast can be
/// delivered to every peer without re-encoding or copying the payload per
/// peer. [`SharedEvent::queued`] hands each peer a [`QueuedFrame`] that shares
/// this buffer (an `Arc` bump, not a copy), so broadcasting a 32 MiB image to
/// N peers holds one buffer instead of N.
///
/// Every peer receives the same `message_id`. Receiver-side dedup is scoped to
/// `(source, message_id)` and each peer sees a given broadcast at most once
/// (peers do not relay), so the shared id is safe — it simply means one
/// clipboard event carries one identity across the fan-out.
#[derive(Clone)]
pub struct SharedEvent {
    composite_fallback: CompositeFallback,
    /// Shared so a fan-out to N peers encodes the event once, not N times (R4-04).
    composite_chunks: CompositeChunks,
    _composite_memory: Option<Arc<crate::composite::budgets::PipelineLease>>,
    command: Command,
    payload: Arc<[u8]>,
    message_id: MessageId,
    created_at: Instant,
}

impl SharedEvent {
    pub fn envelope_identity(&self) -> (MessageId, i64) {
        (
            self.message_id,
            i64::from_be_bytes(
                self.payload[20..28]
                    .try_into()
                    .expect("Shared event contains a validated envelope"),
            ),
        )
    }
    /// Encode a text/image payload once into a shared, reference-counted
    /// buffer, validated against the command limit exactly like
    /// [`QueuedFrame::new`] so a broadcast can never put an oversized frame on
    /// the wire.
    pub fn encode(command: Command, content: Vec<u8>) -> Result<Self, String> {
        Self::encode_shared(command, Arc::from(content.into_boxed_slice()))
    }

    /// Encode an event from a caller-owned shared content buffer. The content
    /// remains available to another consumer (for example, local history
    /// persistence) while the wire envelope is built once for fan-out.
    pub fn encode_shared(command: Command, content: Arc<[u8]>) -> Result<Self, String> {
        let composite_memory = admit_composite_wire(command, content.len())?;
        if !matches!(
            command,
            Command::TextPayload
                | Command::ImagePayload
                | Command::CompositePayload
                | Command::ArchiveUpdatePayload
        ) {
            return Err(format!("{command:?} is not an envelope-wrapped command"));
        }
        if EVENT_ENVELOPE_HEADER_SIZE + content.len() > command.payload_limit() {
            return Err(format!(
                "{:?} reliable payload exceeds the {} byte limit",
                command,
                command.payload_limit()
            ));
        }
        let (payload, message_id) = EventEnvelope::encode_shared(content);
        Ok(Self {
            composite_fallback: Arc::new(std::sync::Mutex::new(None)),
            composite_chunks: Arc::new(std::sync::Mutex::new(None)),
            _composite_memory: composite_memory,
            command,
            payload,
            message_id,
            created_at: Instant::now(),
        })
    }

    pub fn command(&self) -> Command {
        self.command
    }

    /// Build a per-peer queued frame that shares this event's encoded bytes
    /// (an `Arc` reference-count bump, no payload copy) and acknowledges the
    /// shared message ID.
    pub fn queued(&self) -> QueuedFrame {
        QueuedFrame {
            command: self.command,
            payload: Payload::Shared(self.payload.clone()),
            acknowledgement: AckExpectation::Event(self.message_id),
            completion: None,
            enqueued_at: self.created_at,
            file_window: None,
            composite_fallback: self.composite_fallback.clone(),
            composite_chunks: self.composite_chunks.clone(),
            _composite_memory: self._composite_memory.clone(),
            archive_authorization: None,
        }
    }
}

/// A queued frame paired with its send sequence number.
pub struct PendingFrame {
    queued: QueuedFrame,
    sequence: u32,
    undelivered_peer: Option<String>,
    selected_composite: std::sync::OnceLock<CompositeSelection>,
}

impl PendingFrame {
    pub fn new(queued: QueuedFrame, sequence: u32) -> Self {
        Self {
            queued,
            sequence,
            undelivered_peer: None,
            selected_composite: std::sync::OnceLock::new(),
        }
    }

    fn new_for_peer(queued: QueuedFrame, sequence: u32, hostname: &str) -> Self {
        Self {
            queued,
            sequence,
            undelivered_peer: Some(hostname.to_string()),
            selected_composite: std::sync::OnceLock::new(),
        }
    }

    pub fn sequence(&self) -> u32 {
        self.sequence
    }

    fn is_expired(&self, ttl: Duration) -> bool {
        self.queued.enqueued_at.elapsed() >= ttl
    }

    /// Report the delivery result to the enqueuer, if one is waiting.
    pub fn complete(mut self, result: Result<DeliveryReceipt, DeliveryError>) {
        self.undelivered_peer = None;
        if let Some(completion) = self.queued.completion.take() {
            let _ = completion.send(result);
        }
    }
}

/// Whether a queued frame carries clipboard content. Only these map to the
/// user-facing "clipboard delivery was stopped" warning; file transfers report
/// through their own progress/failure surface.
/// Whether a peer's error text describes temporary pressure rather than a rejection
/// of the content.
///
/// R4-03: a full assembly budget, a per-peer assembly already in flight, or an
/// exhausted raw/decode/pipeline budget are all *temporary*. Classifying them as a
/// permanent `Rejected` silently dropped the clipboard content for good; they are
/// transport failures that the existing retry path already handles.
pub fn peer_error_is_temporary(message: &str) -> bool {
    message.contains(crate::protocol::TEMPORARY_PRESSURE_MARKER)
}

fn carries_clipboard_content(command: Command) -> bool {
    matches!(
        command,
        Command::TextPayload
            | Command::ImagePayload
            | Command::ImageChunk
            | Command::CompositePayload
            | Command::CompositeChunk
    )
}

impl Drop for PendingFrame {
    fn drop(&mut self) {
        let Some(hostname) = self.undelivered_peer.take() else {
            return;
        };
        if let Some(completion) = self.queued.completion.take() {
            let _ = completion.send(Err(DeliveryError::Transport(
                "Connection task closed".to_string(),
            )));
        }
        // The warning is user-facing and names clipboard content, so it is
        // raised only when a clipboard frame is abandoned during a real
        // shutdown. A forced reconnect also abandons the frame, but the
        // durable retry resends it; an abandoned file/batch frame belongs to
        // the file-transfer failure surface instead.
        if carries_clipboard_content(self.queued.command())
            && crate::sync_warning::record_delivery_shutdown(&hostname)
        {
            log::warn!(
                "Clipboard delivery to {hostname} was abandoned while TailSync was shutting down"
            );
        } else {
            log::debug!(
                "Delivery to {hostname} was abandoned before completion; the durable retry or the file-transfer surface handles it"
            );
        }
    }
}

/// Record a platform warning for permanent delivery failures that carry
/// domain meaning (currently: expired event timestamps).
pub fn record_permanent_delivery_warning(hostname: &str, command: Command, error: &DeliveryError) {
    if let DeliveryError::CompositeResult(result) = error {
        crate::sync_warning::record_composite_result(hostname, *result);
    }
    if matches!(error, DeliveryError::Expired(_)) {
        crate::sync_warning::record_expired_event(hostname);
    }
    // R4-04 / D-5: a permanent rejection is only worth a user-visible warning when
    // it actually dropped clipboard content. A rejected file frame belongs to the
    // file-transfer surface instead.
    if matches!(error, DeliveryError::Rejected(_)) && carries_clipboard_content(command) {
        crate::sync_warning::record_delivery_rejected(hostname);
    }
}

fn complete_expired_frame(frame: PendingFrame, hostname: &str) {
    crate::sync_warning::record_delivery_expired(hostname);
    frame.complete(Err(DeliveryError::Timeout(
        "queued frame expired before delivery".to_string(),
    )));
}

/// Typed outcome of a delivery attempt. Retry policy is expressed through
/// [`DeliveryError::is_retryable`] instead of string matching, so wording
/// changes can never alter delivery behavior.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryError {
    /// The peer rejected the payload outright (permanent, do not retry).
    Rejected(String),
    CompositeResult(crate::protocol::composite::AckResult),
    /// The event was validly signed but outside the protocol's freshness
    /// window (permanent, do not retry).
    Expired(String),
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
            Self::CompositeResult(result) => {
                write!(formatter, "composite clipboard result: {result:?}")
            }
            Self::Rejected(message) => write!(formatter, "peer rejected: {message}"),
            Self::Expired(message) => write!(formatter, "event expired: {message}"),
            Self::Timeout(message) => write!(formatter, "delivery timed out: {message}"),
            Self::Transport(message) => write!(formatter, "transport failed: {message}"),
            Self::Protocol(message) => write!(formatter, "protocol mismatch: {message}"),
        }
    }
}

impl DeliveryError {
    /// Whether a retry (after backoff or reconnect) can plausibly succeed.
    /// Peer rejections and expired events are permanent; transport, timeout,
    /// and protocol failures are retried while the frame remains fresh.
    pub fn is_retryable(&self) -> bool {
        !matches!(
            self,
            Self::Rejected(_) | Self::Expired(_) | Self::CompositeResult(_)
        )
    }

    fn rejected(message: impl Into<String>) -> Self {
        Self::Rejected(message.into())
    }

    fn expired(message: impl Into<String>) -> Self {
        Self::Expired(message.into())
    }

    fn protocol(message: impl Into<String>) -> Self {
        Self::Protocol(message.into())
    }

    fn transport(message: impl Into<String>) -> Self {
        Self::Transport(message.into())
    }
}

mod executor;
mod race;
mod worker;

pub use executor::deliver_pending_frame;
pub use race::{candidate_delay, measured_candidate_delay, race_connections};
pub use worker::{run_connection_worker, ConnectionAdapter, WorkerConfig};

#[cfg(test)]
pub(crate) use executor::{validate_event_ack, validate_file_ack};
#[cfg(test)]
use worker::receive_scheduled_frame;

#[cfg(test)]
mod tests;
