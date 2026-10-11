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
    validate_file_ack_sequence(ack, pending.sequence, transfer_id)
}

fn validate_file_ack_sequence(
    ack: &Frame,
    sequence: u32,
    transfer_id: TransferId,
) -> Result<DeliveryReceipt, DeliveryError> {
    let offset =
        FileOffset::decode(&ack.payload).map_err(|e| DeliveryError::protocol(e.to_string()))?;
    if ack.sequence != sequence || offset.transfer_id != transfer_id {
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
            if matches!(
                pending.queued.command,
                Command::CompositePayload | Command::ArchiveUpdatePayload
            ) {
                return deliver_composite(stream, pending, envelope, config).await;
            }
            if pending.queued.command == Command::ImagePayload
                && stream.negotiated_capabilities().image_compressed_chunks
            {
                let compressed =
                    tokio::task::spawn_blocking(move || crate::image_chunks::compress(&envelope))
                        .await
                        .map_err(|error| DeliveryError::protocol(error.to_string()))?
                        .map_err(|error| DeliveryError::protocol(error.to_string()))?;
                if let Some(chunks) = compressed {
                    tracing::info!(
                        session_id = ?stream.session_id(),
                        path = "compressed_chunks",
                        chunks = chunks.len(),
                        "image delivery path selected"
                    );
                    return deliver_compressed_image(stream, pending, chunks, message_id, config)
                        .await;
                }
            }
            if pending.queued.command == Command::ImagePayload {
                tracing::info!(
                    session_id = ?stream.session_id(),
                    path = "raw_image",
                    "image delivery path selected"
                );
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
            if let Some(chunks) = &pending.queued.file_window {
                return deliver_file_window(stream, pending, chunks, transfer_id, config).await;
            }
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

async fn deliver_compressed_image<T: DeliveryConnection>(
    stream: &mut T,
    pending: &PendingFrame,
    chunks: Vec<Vec<u8>>,
    message_id: MessageId,
    config: &DeliveryConfig,
) -> Result<DeliveryReceipt, DeliveryError> {
    for (index, payload) in chunks.into_iter().enumerate() {
        let sequence = pending.sequence.wrapping_add(index as u32);
        let frame = Frame::try_new(Command::ImageChunk, 0, sequence, payload)
            .map_err(|error| DeliveryError::protocol(error.to_string()))?;
        stream
            .write_frame(&frame)
            .await
            .map_err(|error| DeliveryError::transport(error.to_string()))?;
        match timeout(
            config.event_ack_timeout.max(Duration::from_secs(10)),
            stream.read_frame(),
        )
        .await
        {
            Ok(Ok(ack)) if ack.command == Command::EventAck => {
                let received_id = MessageId::from_ack_payload(&ack.payload)
                    .map_err(|error| DeliveryError::protocol(error.to_string()))?;
                if ack.sequence != sequence || received_id != message_id {
                    return Err(DeliveryError::protocol("image chunk ACK mismatch"));
                }
            }
            Ok(Ok(error)) if error.command == Command::PeerError => {
                return Err(DeliveryError::rejected(format!(
                    "image: {}",
                    String::from_utf8_lossy(&error.payload)
                )));
            }
            Ok(Ok(other)) => {
                return Err(DeliveryError::protocol(format!(
                    "expected image chunk ACK, received {:?}",
                    other.command
                )));
            }
            Ok(Err(error)) => return Err(DeliveryError::transport(error.to_string())),
            Err(_) => {
                // Reconnect before replay so no delayed ACK from the old
                // connection can be mistaken for an ACK of the new attempt.
                return Err(DeliveryError::Timeout(
                    "image chunk acknowledgement timed out".into(),
                ));
            }
        }
    }
    Ok(DeliveryReceipt::default())
}

async fn deliver_file_window<T: DeliveryConnection>(
    stream: &mut T,
    pending: &PendingFrame,
    chunks: &[Vec<u8>],
    transfer_id: TransferId,
    config: &DeliveryConfig,
) -> Result<DeliveryReceipt, DeliveryError> {
    let mut frames = Vec::with_capacity(chunks.len());
    let mut window_end = 0_u64;
    for (index, payload) in chunks.iter().enumerate() {
        let chunk = crate::protocol::FileChunkPayload::decode(payload)
            .map_err(|error| DeliveryError::protocol(error.to_string()))?;
        window_end = window_end.max(chunk.offset + chunk.data.len() as u64);
        frames.push(
            Frame::try_new(
                Command::FileChunk,
                0,
                pending.sequence.wrapping_add(index as u32),
                payload.clone(),
            )
            .map_err(|error| DeliveryError::protocol(error.to_string()))?,
        );
    }

    // A legacy or locally disabled peer gets the same ordered chunks with
    // stop-and-wait ACKs. The queue remains valid across a reconnect.
    if !stream.negotiated_capabilities().file_sliding_window {
        tracing::info!(
            session_id = ?stream.session_id(),
            path = "stop_and_wait",
            chunks = frames.len(),
            "file delivery path selected"
        );
        let mut receipt = DeliveryReceipt::default();
        for frame in &frames {
            receipt = deliver_file_frame(stream, pending, frame, transfer_id, config).await?;
            if receipt.resume_required {
                return Ok(receipt);
            }
        }
        return Ok(receipt);
    }

    tracing::info!(
        session_id = ?stream.session_id(),
        path = "sliding_window",
        chunks = frames.len(),
        window_end,
        "file delivery path selected"
    );
    for frame in &frames {
        stream
            .write_frame(frame)
            .await
            .map_err(|error| DeliveryError::transport(error.to_string()))?;
    }
    let mut confirmed = 0_u64;
    let mut resume_required = false;
    for frame in &frames {
        match timeout(config.file_ack_timeout, stream.read_frame()).await {
            Ok(Ok(ack)) if matches!(ack.command, Command::FileAck | Command::FileResume) => {
                if ack.sequence != frame.sequence {
                    return Err(DeliveryError::protocol("file window ACK sequence mismatch"));
                }
                let offset = crate::protocol::FileOffset::decode(&ack.payload)
                    .map_err(|error| DeliveryError::protocol(error.to_string()))?;
                if offset.transfer_id != transfer_id || offset.next_offset > window_end {
                    return Err(DeliveryError::protocol("invalid file window ACK offset"));
                }
                confirmed = confirmed.max(offset.next_offset);
                resume_required |= ack.command == Command::FileResume;
            }
            Ok(Ok(error)) if error.command == Command::PeerError => {
                return Err(DeliveryError::rejected(format!(
                    "file: {}",
                    String::from_utf8_lossy(&error.payload)
                )));
            }
            Ok(Ok(other)) => {
                return Err(DeliveryError::protocol(format!(
                    "expected file window ACK, received {:?}",
                    other.command
                )));
            }
            Ok(Err(error)) => return Err(DeliveryError::transport(error.to_string())),
            Err(_) => {
                // A TCP record can have reached durable storage even when
                // its ACK was lost. Report the last cumulative offset and
                // resume only the missing suffix on a fresh connection.
                if confirmed > 0 {
                    return Ok(DeliveryReceipt {
                        next_offset: Some(confirmed),
                        resume_required: true,
                    });
                }
                return Err(DeliveryError::Timeout(
                    "file window acknowledgement timed out".into(),
                ));
            }
        }
    }
    tracing::info!(
        session_id = ?stream.session_id(),
        path = "sliding_window",
        confirmed_offset = confirmed,
        resume_required = resume_required || confirmed < window_end,
        "file delivery window acknowledged"
    );
    Ok(DeliveryReceipt {
        next_offset: Some(confirmed),
        resume_required: resume_required || confirmed < window_end,
    })
}

async fn deliver_event_frame<T: DeliveryConnection>(
    stream: &mut T,
    pending: &PendingFrame,
    frame: &Frame,
    message_id: MessageId,
    config: &DeliveryConfig,
) -> Result<(), DeliveryError> {
    tracing::debug!(
        command = ?pending.queued.command,
        sequence = pending.sequence,
        session_id = ?stream.session_id(),
        message_id = %message_id.as_hex(),
        "event delivery attempt"
    );
    stream
        .write_frame(frame)
        .await
        .map_err(|error| DeliveryError::transport(error.to_string()))?;
    match timeout(config.event_ack_timeout, stream.read_frame()).await {
        Ok(Ok(ack)) if ack.command == Command::EventAck => {
            validate_event_ack(&ack, pending, message_id)?;
            Ok(())
        }
        Ok(Ok(frame)) if frame.command == Command::PeerError => {
            let message = String::from_utf8_lossy(&frame.payload).to_string();
            if message.contains("event timestamp is outside the accepted window") {
                return Err(DeliveryError::expired(message));
            }
            if super::peer_error_is_temporary(&message) {
                // Temporary pressure is retryable through the existing transport
                // path; discarding the content here is what made a full budget look
                // like a permanent rejection (R4-03).
                return Err(DeliveryError::transport(format!(
                    "peer under temporary pressure: {message}"
                )));
            }
            Err(DeliveryError::rejected(format!("event: {message}")))
        }
        Ok(Ok(frame)) => Err(DeliveryError::protocol(format!(
            "expected EventAck, received {:?}",
            frame.command
        ))),
        Ok(Err(error)) => Err(DeliveryError::transport(error.to_string())),
        // Never replay on this stream: a duplicate ACK left by the first
        // attempt would be read as the retry's ACK and mismatched into a
        // protocol error. End the attempt so the worker reconnects and replays
        // on a fresh authenticated session, where the receiver's dedup window
        // decides whether the event is applied again.
        Err(_) => Err(DeliveryError::Timeout(
            "event acknowledgement timed out; reconnecting on a fresh session".into(),
        )),
    }
}

async fn deliver_file_frame<T: DeliveryConnection>(
    stream: &mut T,
    pending: &PendingFrame,
    frame: &Frame,
    transfer_id: TransferId,
    config: &DeliveryConfig,
) -> Result<DeliveryReceipt, DeliveryError> {
    tracing::debug!(
        command = ?pending.queued.command,
        sequence = pending.sequence,
        session_id = ?stream.session_id(),
        transfer_id = %transfer_id.as_hex(),
        "file delivery attempt"
    );
    stream
        .write_frame(frame)
        .await
        .map_err(|error| DeliveryError::transport(error.to_string()))?;
    match timeout(config.file_ack_timeout, stream.read_frame()).await {
        Ok(Ok(ack)) if matches!(ack.command, Command::FileAck | Command::FileResume) => {
            if frame.sequence == pending.sequence {
                validate_file_ack(&ack, pending, transfer_id)
            } else {
                validate_file_ack_sequence(&ack, frame.sequence, transfer_id)
            }
        }
        Ok(Ok(frame)) => {
            if frame.command == Command::PeerError {
                return Err(DeliveryError::rejected(format!(
                    "file: {}",
                    String::from_utf8_lossy(&frame.payload)
                )));
            }
            Err(DeliveryError::protocol(format!(
                "expected file acknowledgement, received {:?}",
                frame.command
            )))
        }
        Ok(Err(error)) => Err(DeliveryError::transport(error.to_string())),
        Err(_) => Err(DeliveryError::Timeout(
            "file acknowledgement timed out; reconnecting on a fresh session".into(),
        )),
    }
}

async fn deliver_batch_frame<T: DeliveryConnection>(
    stream: &mut T,
    pending: &PendingFrame,
    frame: &Frame,
    batch_id: TransferId,
    config: &DeliveryConfig,
) -> Result<DeliveryReceipt, DeliveryError> {
    tracing::debug!(
        command = ?pending.queued.command,
        sequence = pending.sequence,
        session_id = ?stream.session_id(),
        batch_id = %batch_id.as_hex(),
        "batch delivery attempt"
    );
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
            Ok(DeliveryReceipt::default())
        }
        Ok(Ok(reject)) if reject.command == Command::FileBatchReject => {
            Err(DeliveryError::rejected(format!(
                "batch: {}",
                String::from_utf8_lossy(&reject.payload)
            )))
        }
        Ok(Ok(error)) if error.command == Command::PeerError => Err(DeliveryError::rejected(
            format!("batch: {}", String::from_utf8_lossy(&error.payload)),
        )),
        Ok(Ok(other)) => Err(DeliveryError::protocol(format!(
            "expected batch acknowledgement, received {:?}",
            other.command
        ))),
        Ok(Err(error)) => Err(DeliveryError::transport(error.to_string())),
        Err(_) => Err(DeliveryError::Timeout(
            "file batch acknowledgement timed out; reconnecting on a fresh session".to_string(),
        )),
    }
}

async fn authorize_archive_update(pending: &PendingFrame) -> Result<(), DeliveryError> {
    let Some(authorized) = pending.queued.archive_authorization.clone() else {
        return Ok(());
    };
    if !tokio::task::spawn_blocking(move || authorized())
        .await
        .map_err(|_| DeliveryError::rejected("Archive authorization worker unavailable"))?
    {
        return Err(DeliveryError::rejected("Archive authorization revoked"));
    }
    Ok(())
}

async fn deliver_composite<T: DeliveryConnection>(
    stream: &mut T,
    pending: &PendingFrame,
    envelope: EventEnvelope,
    config: &DeliveryConfig,
) -> Result<DeliveryReceipt, DeliveryError> {
    use crate::protocol::composite::{AckResult, CompositeAck};
    let is_archive_update = pending.queued.command == Command::ArchiveUpdatePayload;
    if is_archive_update {
        authorize_archive_update(pending).await?;
    }
    if is_archive_update
        && (!stream.negotiated_capabilities().archive_resource_updates
            || !stream.negotiated_capabilities().composite_clipboard)
    {
        return Err(DeliveryError::rejected(
            "Archive resource updates were not negotiated; no clipboard fallback is allowed",
        ));
    }
    // Per-peer envelope/model/scratch; the source wire is shared and admitted at
    // enqueue. Reject pressure without changing this event's identity.
    let scratch = Arc::new(
        crate::composite::budgets::global()
            .reserve_pipeline((envelope.content.len() as u64).saturating_mul(3))
            .map_err(|e| DeliveryError::protocol(e.to_string()))?,
    );
    if pending.selected_composite.get().is_none() {
        let choice = if is_archive_update || stream.negotiated_capabilities().composite_clipboard {
            CompositeSelection::Portable
        } else {
            let cache = pending.queued.composite_fallback.clone();
            let bytes = envelope.content.clone();
            let held = scratch.clone();
            let (command, content, _memory) = tokio::task::spawn_blocking(move || {
                let _held = held;
                let mut cache = cache.lock().unwrap_or_else(|p| p.into_inner());
                if let Some(cached) = cache.as_ref() { return cached.clone(); }
                let converted: Result<CompositeFallbackBody, String> = (|| {
                    let bundle = crate::composite::PortableBundle::decode(&bytes).map_err(|e| e.to_string())?;
                    let rgba_bytes = bundle.validate_with_metrics().map_err(|e| e.to_string())?.rgba_bytes as u64;
                    let mut memory = crate::composite::budgets::global().reserve_pipeline(rgba_bytes.saturating_mul(4).saturating_add(32 * 1024 * 1024)).map_err(|e| e.to_string())?;
                    let (command, bytes) = crate::composite::legacy_content(&bundle).map_err(|e| e.to_string())?;
                    memory.shrink_to(bytes.len() as u64);
                    Ok((command, Arc::from(bytes.into_boxed_slice()), Arc::new(memory)))
                })();
                if !matches!(&converted, Err(error) if error.contains(crate::protocol::TEMPORARY_PRESSURE_MARKER)) { *cache = Some(converted.clone()); }
                converted
            }).await.map_err(|_| DeliveryError::protocol("Composite fallback worker failed"))?
                .map_err(|error| if error.contains(crate::protocol::TEMPORARY_PRESSURE_MARKER) { DeliveryError::protocol(error) } else { DeliveryError::rejected(error) })?;
            CompositeSelection::Legacy {
                command,
                message_id: MessageId::random(),
                timestamp_ms: envelope.timestamp_ms,
                content,
            }
        };
        let _ = pending.selected_composite.set(choice);
    }
    let selected = pending
        .selected_composite
        .get()
        .ok_or_else(|| DeliveryError::protocol("Missing composite selection"))?;
    if let CompositeSelection::Legacy {
        command,
        message_id,
        timestamp_ms,
        content,
    } = selected
    {
        let frame = Frame::try_new(
            *command,
            0,
            pending.sequence,
            EventEnvelope::encode_content(*message_id, *timestamp_ms, content),
        )
        .map_err(|e| DeliveryError::protocol(e.to_string()))?;
        deliver_event_frame(stream, pending, &frame, *message_id, config).await?;
        if let Some(peer) = pending.undelivered_peer.as_deref() {
            crate::sync_warning::record_composite_result(peer, AckResult::StoredIncomplete);
        }
        return Ok(DeliveryReceipt::default());
    }
    if !stream.negotiated_capabilities().composite_clipboard {
        return Err(DeliveryError::rejected(
            "Composite capability lost after selection; payload identity cannot change",
        ));
    }
    // Always use bounded chunks; first implementation avoids the large-frame
    // admission race. Compression has raw fallback and never changes identity.
    let digest = *blake3::hash(&envelope.content).as_bytes();
    let message_id = envelope.message_id;
    // R4-04: encode and compress once. The event's identity — and therefore its
    // chosen encoding — is fixed for the life of the frame, so a retry re-sends the
    // same immutable bytes instead of re-compressing up to 32 MiB every attempt.
    // The cache lives on the shared event, so a fan-out to N peers also encodes once.
    let cache = pending.queued.composite_chunks.clone();
    let held = scratch.clone();
    let encoded = tokio::task::spawn_blocking(move || {
        let _held = held;
        let mut cache = cache.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(cached) = cache.as_ref() { return cached.clone(); }
        let encoded = (|| {
            let bound = CachedChunks::memory_bound(envelope.content.len());
            let mut memory = crate::composite::budgets::global().reserve_pipeline(bound).map_err(|e| e.to_string())?;
            let chunks = if is_archive_update {
                crate::composite::chunks::encode_archive(&envelope)
            } else {
                crate::composite::chunks::encode(&envelope)
            }.map_err(|e| e.to_string())?;
            let allocated = CachedChunks::allocated_bytes(&chunks, chunks.capacity());
            if allocated > bound {
                return Err(format!("{}Composite chunk allocation exceeds admitted cache memory", crate::protocol::TEMPORARY_PRESSURE_MARKER));
            }
            memory.shrink_to(allocated);
            Ok(Arc::new(CachedChunks { chunks, _memory: memory }))
        })();
        if !matches!(&encoded, Err(error) if error.contains(crate::protocol::TEMPORARY_PRESSURE_MARKER)) { *cache = Some(encoded.clone()); }
        encoded
    }).await.map_err(|_| DeliveryError::protocol("Composite encoding worker failed"))?;
    let cached = encoded.map_err(DeliveryError::protocol)?;
    let chunks = &cached.chunks;
    let count = chunks.len();
    let expires = pending.queued.enqueued_at
        + Duration::from_millis(crate::protocol::EVENT_TIMESTAMP_WINDOW_MS as u64);
    for (index, payload) in chunks.iter().cloned().enumerate() {
        if is_archive_update {
            authorize_archive_update(pending).await?;
        }
        let remaining = expires.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(DeliveryError::expired("Composite deadline reached"));
        }
        let sequence = pending.sequence.wrapping_add(index as u32);
        let chunk_command = if is_archive_update {
            Command::ArchiveUpdateChunk
        } else {
            Command::CompositeChunk
        };
        let frame = Frame::try_new(chunk_command, 0, sequence, payload)
            .map_err(|e| DeliveryError::protocol(e.to_string()))?;
        timeout(remaining, stream.write_frame(&frame))
            .await
            .map_err(|_| DeliveryError::expired("Composite deadline reached"))?
            .map_err(DeliveryError::transport)?;
        let remaining = expires.saturating_duration_since(Instant::now());
        let ack = timeout(
            config
                .event_ack_timeout
                .max(Duration::from_secs(10))
                .min(remaining),
            stream.read_frame(),
        )
        .await
        .map_err(|_| DeliveryError::Timeout("Composite acknowledgement timed out".into()))?
        .map_err(DeliveryError::transport)?;
        if ack.command == Command::PeerError {
            let message = String::from_utf8_lossy(&ack.payload).into_owned();
            if is_archive_update && message.contains(crate::protocol::TEMPORARY_PRESSURE_MARKER) {
                return Err(DeliveryError::protocol(message));
            }
            return Err(DeliveryError::rejected(message));
        }
        let ack_command = if is_archive_update {
            Command::ArchiveUpdateAck
        } else {
            Command::CompositeAck
        };
        if ack.command != ack_command || ack.sequence != sequence {
            return Err(DeliveryError::protocol(
                "Composite ACK command or sequence mismatch",
            ));
        }
        let ack = CompositeAck::decode(&ack.payload)
            .map_err(|e| DeliveryError::protocol(e.to_string()))?;
        if ack.message_id != message_id || ack.digest != digest || ack.chunk_index != index as u16 {
            return Err(DeliveryError::protocol("Composite ACK identity mismatch"));
        }
        if index + 1 < count {
            if ack.result != AckResult::Chunk {
                return Err(DeliveryError::protocol("Premature composite terminal ACK"));
            }
        } else if ack.result != AckResult::Applied
            && !(is_archive_update && ack.result == AckResult::Superseded)
        {
            return Err(DeliveryError::CompositeResult(ack.result));
        }
    }
    Ok(DeliveryReceipt::default())
}
