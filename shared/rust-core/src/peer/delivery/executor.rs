use tokio::time::timeout;

use crate::protocol::FileOffset;

use super::*;

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
        resume_required: ack.command == Command::FileResume,
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
                pending.queued.payload.as_slice().to_vec(),
            )
            .map_err(|error| DeliveryError::protocol(error.to_string()))?;
            stream
                .write_frame(&frame)
                .await
                .map_err(|error| DeliveryError::transport(error.to_string()))?;
            Ok(DeliveryReceipt::default())
        }
        AckExpectation::Event(message_id) => {
            let envelope = EventEnvelope::decode(pending.queued.payload.as_slice())
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
                pending.queued.payload.as_slice().to_vec(),
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
                pending.queued.payload.as_slice().to_vec(),
            )
            .map_err(|error| DeliveryError::protocol(error.to_string()))?;
            deliver_file_frame(stream, pending, &frame, transfer_id, config).await
        }
        AckExpectation::Batch(batch_id) => {
            let frame = Frame::try_new(
                pending.queued.command,
                0,
                pending.sequence,
                pending.queued.payload.as_slice().to_vec(),
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
                let message = String::from_utf8_lossy(&frame.payload).to_string();
                if message.contains("event timestamp is outside the accepted window") {
                    return Err(DeliveryError::expired(message));
                }
                return Err(DeliveryError::rejected(format!("event: {message}")));
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
