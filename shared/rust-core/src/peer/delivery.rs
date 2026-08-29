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
use crate::secure::SecureConnection;

/// Keep interactive clipboard traffic responsive without allowing a
/// continuously readable priority queue to starve file-transfer traffic.
const PRIORITY_BURST_LIMIT: usize = 8;

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
pub struct QueuedFrame {
    command: Command,
    payload: Payload,
    acknowledgement: AckExpectation,
    completion: Option<oneshot::Sender<Result<DeliveryReceipt, DeliveryError>>>,
    enqueued_at: Instant,
}

impl QueuedFrame {
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
            payload: Payload::Owned(payload),
            acknowledgement: AckExpectation::Event(envelope.message_id),
            completion: None,
            enqueued_at: Instant::now(),
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
    command: Command,
    payload: Arc<[u8]>,
    message_id: MessageId,
}

impl SharedEvent {
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
        if !matches!(command, Command::TextPayload | Command::ImagePayload) {
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
            command,
            payload,
            message_id,
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
            enqueued_at: Instant::now(),
        }
    }
}

/// A queued frame paired with its send sequence number.
pub struct PendingFrame {
    queued: QueuedFrame,
    sequence: u32,
    undelivered_peer: Option<String>,
}

impl PendingFrame {
    pub fn new(queued: QueuedFrame, sequence: u32) -> Self {
        Self {
            queued,
            sequence,
            undelivered_peer: None,
        }
    }

    fn new_for_peer(queued: QueuedFrame, sequence: u32, hostname: &str) -> Self {
        Self {
            queued,
            sequence,
            undelivered_peer: Some(hostname.to_string()),
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
        crate::sync_warning::record_delivery_shutdown(&hostname);
        log::warn!("Delivery to {hostname} ended before the in-flight frame completed");
    }
}

/// Record a platform warning for permanent delivery failures that carry
/// domain meaning (currently: expired event timestamps).
pub fn record_permanent_delivery_warning(hostname: &str, error: &DeliveryError) {
    if matches!(error, DeliveryError::Expired(_)) {
        crate::sync_warning::record_expired_event(hostname);
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
        !matches!(self, Self::Rejected(_) | Self::Expired(_))
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
pub use race::{candidate_delay, race_connections};
pub use worker::{run_connection_worker, ConnectionAdapter, WorkerConfig};

#[cfg(test)]
pub(crate) use executor::{validate_event_ack, validate_file_ack};
#[cfg(test)]
use worker::receive_scheduled_frame;

#[cfg(test)]
mod tests;
