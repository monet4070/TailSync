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
use tokio::sync::{mpsc, oneshot};
use tokio::time::{timeout, Duration};

use crate::peer::types::{ConnectionInterface, DeliveryReceipt, ResolvedCandidate, ResolvedTarget};
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
pub async fn deliver_pending_frame(
    stream: &mut SecureConnection,
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

async fn deliver_event_frame(
    stream: &mut SecureConnection,
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

async fn deliver_file_frame(
    stream: &mut SecureConnection,
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

async fn deliver_batch_frame(
    stream: &mut SecureConnection,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::peer::types::PeerCandidate;

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
}
