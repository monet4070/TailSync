use super::*;
use tailsync_core::peer::admission::peer_is_allowed;
use tailsync_core::peer::event_receiver::process_reliable_event;
use tailsync_core::peer::inbound_source::InboundSource;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::Instrument;

pub(super) use tailsync_core::peer::connection_limiter::ConnectionLimiter;

/// Start the async TCP server.  Runs until the app shuts down.
pub async fn start_server(
    sync_engine: Arc<Mutex<sync::SyncEngine>>,
    database: Arc<Mutex<db::HistoryDB>>,
    settings: Arc<Mutex<crypto::Settings>>,
    identity: Arc<DeviceIdentity>,
    pairing: Arc<PairingManager>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let addr = SocketAddr::from(([0, 0, 0, 0], TCP_PORT));
    let limiter = ConnectionLimiter::new(64, 8);
    let mut handlers = tokio::task::JoinSet::new();

    loop {
        if *shutdown.borrow() {
            break;
        }
        TCP_SERVER_HEALTHY.store(false, Ordering::Release);
        let listener = match bind_tcp_listener(addr) {
            Ok(listener) => listener,
            Err(error) => {
                error!("TCP server bind error: {}", error);
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                    _ = wait_for_shutdown(&mut shutdown) => break,
                }
                continue;
            }
        };
        TCP_SERVER_HEALTHY.store(true, Ordering::Release);
        info!("TCP server listening on port {}", TCP_PORT);

        loop {
            let accepted = tokio::select! {
                accepted = listener.accept() => Some(accepted),
                joined = handlers.join_next(), if !handlers.is_empty() => {
                    if let Some(Err(error)) = joined {
                        debug!("Inbound connection task ended unexpectedly: {error}");
                    }
                    continue;
                }
                _ = wait_for_shutdown(&mut shutdown) => None,
            };
            let Some(accepted) = accepted else {
                break;
            };
            match accepted {
                Ok((stream, peer_addr)) => {
                    let Some(permit) = limiter.try_acquire(peer_addr.ip()) else {
                        warn!("Connection limit reached for an inbound peer");
                        debug!("Rejected inbound address: {peer_addr}");
                        continue;
                    };
                    debug!("New connection from {}", peer_addr);
                    let sync = sync_engine.clone();
                    let db = database.clone();
                    let settings = settings.clone();
                    let identity = identity.clone();
                    let pairing = pairing.clone();
                    handlers.spawn(async move {
                        let _permit = permit;
                        if let Err(e) = handle_connection(
                            stream, peer_addr, sync, db, settings, identity, pairing,
                        )
                        .await
                        {
                            warn!("Inbound peer connection error: {e}");
                            debug!("Failed inbound address: {peer_addr}");
                        }
                    });
                }
                Err(error) => {
                    error!("TCP accept error: {}; rebuilding listener", error);
                    TCP_SERVER_HEALTHY.store(false, Ordering::Release);
                    break;
                }
            }
        }
        if *shutdown.borrow() {
            break;
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(1)) => {}
            _ = wait_for_shutdown(&mut shutdown) => break,
        }
    }

    TCP_SERVER_HEALTHY.store(false, Ordering::Release);
    if timeout(Duration::from_secs(2), async {
        while handlers.join_next().await.is_some() {}
    })
    .await
    .is_err()
    {
        warn!("Timed out while draining inbound peer connections");
        handlers.abort_all();
        while handlers.join_next().await.is_some() {}
    }
    info!("TCP server stopped for application shutdown");
    Ok(())
}
async fn handle_connection(
    stream: TcpStream,
    peer_addr: SocketAddr,
    sync_engine: Arc<Mutex<sync::SyncEngine>>,
    database: Arc<Mutex<db::HistoryDB>>,
    settings: Arc<Mutex<crypto::Settings>>,
    identity: Arc<DeviceIdentity>,
    pairing: Arc<PairingManager>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (source_allowed, mode) = {
        let settings = settings.lock().await;
        (
            source_matches_mode(peer_addr.ip(), &settings.connection_mode),
            settings.connection_mode.clone(),
        )
    };
    if !source_allowed {
        return Err("Connection source is outside the selected network".into());
    }

    let accepted = timeout(
        HANDSHAKE_TIMEOUT,
        secure::accept_with_pairing_window_and_capabilities(
            stream,
            &identity,
            local_peer_identity(&mode),
            pairing.subscribe_window(),
            secure::CapabilitySwitches::runtime_defaults(),
        ),
    )
    .await
    .map_err(|_| "Handshake timed out")??;
    handle_accepted_connection(
        accepted,
        InboundSource::Tcp(peer_addr),
        sync_engine,
        database,
        settings,
        Some(pairing),
        None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_iroh_connection(
    stream: tailsync_core::iroh_transport::IrohBiStream,
    remote_endpoint_id: String,
    sync_engine: Arc<Mutex<sync::SyncEngine>>,
    database: Arc<Mutex<db::HistoryDB>>,
    settings: Arc<Mutex<crypto::Settings>>,
    identity: Arc<DeviceIdentity>,
    pairing: Arc<PairingManager>,
    remote_invite: Option<tailsync_core::pairing::InviteClaim>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mode = settings.lock().await.connection_mode.clone();
    if !tailsync_core::peer::types::ConnectionMode::parse(&mode).is_some_and(|mode| {
        mode.allows(tailsync_core::peer::types::ConnectionInterface::Iroh)
    }) {
        return Err("Iroh connections are unavailable in the selected connection mode".into());
    }
    let accepted = timeout(
        HANDSHAKE_TIMEOUT,
        secure::accept_with_pairing_window_and_capabilities(
            stream,
            &identity,
            local_peer_identity(&mode),
            pairing.subscribe_window(),
            secure::CapabilitySwitches::runtime_defaults(),
        ),
    )
    .await
    .map_err(|_| "Handshake timed out")??;
    let claimed_endpoint_id = accepted
        .peer_identity
        .iroh_endpoint_id
        .as_deref()
        .ok_or("Peer did not bind its Noise identity to an Iroh endpoint")?;
    let claimed_endpoint_id =
        tailsync_core::iroh_transport::canonical_endpoint_id(claimed_endpoint_id)?;
    if claimed_endpoint_id != remote_endpoint_id {
        return Err("Peer Iroh endpoint does not match its Noise identity".into());
    }
    if accepted.purpose == secure::HandshakePurpose::Pairing {
        super::iroh::remember_rtt_capability(&remote_endpoint_id);
    }
    handle_accepted_connection(
        accepted,
        InboundSource::Iroh(remote_endpoint_id),
        sync_engine,
        database,
        settings,
        Some(pairing),
        remote_invite,
    )
    .await
}

/// Validate the application-level capability before entering the normal Noise
/// pairing handshake.  An invalid capability receives only a generic reject
/// byte and never consumes a pairing failure budget.
#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_iroh_invite_connection(
    mut stream: tailsync_core::iroh_transport::IrohBiStream,
    remote_endpoint_id: String,
    invite_manager: Arc<tailsync_core::pairing::RemotePairingInviteManager>,
    sync_engine: Arc<Mutex<sync::SyncEngine>>,
    database: Arc<Mutex<db::HistoryDB>>,
    settings: Arc<Mutex<crypto::Settings>>,
    identity: Arc<DeviceIdentity>,
    pairing: Arc<PairingManager>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    const INVITE_PREFACE_TIMEOUT: Duration = Duration::from_secs(3);
    let mut encoded = [0u8; tailsync_core::pairing::invite::INVITE_HELLO_LENGTH];
    let read_result = timeout(INVITE_PREFACE_TIMEOUT, stream.read_exact(&mut encoded)).await;
    let hello = match read_result {
        Ok(Ok(_)) => tailsync_core::pairing::InviteHello::decode(&encoded),
        _ => Err(tailsync_core::pairing::InviteError::InvalidFormat),
    };
    let claim = match hello {
        Ok(hello) => match invite_manager.claim(&hello) {
            Ok(claim) => claim,
            Err(error) => {
                let _ = stream
                    .write_all(&[tailsync_core::pairing::invite::INVITE_ACK_REJECTED])
                    .await;
                let _ = stream.flush().await;
                return Err(error.to_string().into());
            }
        },
        Err(error) => {
            let _ = stream
                .write_all(&[tailsync_core::pairing::invite::INVITE_ACK_REJECTED])
                .await;
            let _ = stream.flush().await;
            return Err(error.to_string().into());
        }
    };
    timeout(
        INVITE_PREFACE_TIMEOUT,
        stream.write_all(&[tailsync_core::pairing::invite::INVITE_ACK_ACCEPTED]),
    )
    .await??;
    timeout(INVITE_PREFACE_TIMEOUT, stream.flush()).await??;
    handle_iroh_connection(
        stream,
        remote_endpoint_id,
        sync_engine,
        database,
        settings,
        identity,
        pairing,
        Some(claim),
    )
    .await
}

async fn handle_accepted_connection(
    accepted: secure::AcceptedConnection,
    source: InboundSource,
    sync_engine: Arc<Mutex<sync::SyncEngine>>,
    database: Arc<Mutex<db::HistoryDB>>,
    settings: Arc<Mutex<crypto::Settings>>,
    pairing: Option<Arc<PairingManager>>,
    remote_invite: Option<tailsync_core::pairing::InviteClaim>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let session_id = accepted.connection.session_id().to_string();
    let peer = accepted.peer_identity.hostname.clone();
    let peer_id = tailsync_core::observability::peer_id(&accepted.remote_public_key);
    let interface = source
        .interface()
        .map(|interface| interface.as_str().to_string())
        .unwrap_or_else(|_| "unknown".to_string());
    handle_accepted_connection_inner(
        accepted,
        source,
        sync_engine,
        database,
        settings,
        pairing,
        remote_invite,
    )
    .instrument(tracing::info_span!(
        "secure.session",
        session_id = %session_id,
        peer = %peer,
        peer_id = %peer_id,
        interface = %interface,
    ))
    .await
}

async fn handle_accepted_connection_inner(
    accepted: secure::AcceptedConnection,
    source: InboundSource,
    sync_engine: Arc<Mutex<sync::SyncEngine>>,
    database: Arc<Mutex<db::HistoryDB>>,
    settings: Arc<Mutex<crypto::Settings>>,
    pairing: Option<Arc<PairingManager>>,
    remote_invite: Option<tailsync_core::pairing::InviteClaim>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let purpose = accepted.purpose;
    let handshake_hash = accepted.handshake_hash;
    let mut stream = accepted.connection;
    let session_id = stream.session_id().to_string();
    let peer_info = accepted.peer_identity;
    let peer_public_key = accepted.remote_public_key;
    let source_device_id = secure::fingerprint(&peer_public_key);
    let peer_addr = source.description();
    let source_address = source.address();
    let source_interface = source.interface()?;

    if purpose == secure::HandshakePurpose::Pairing {
        return crate::pairing::install_pairing_session_with_invite(
            pairing.as_ref(),
            stream,
            peer_info.hostname,
            peer_public_key,
            handshake_hash,
            source_address,
            source_interface.as_str().to_string(),
            remote_invite,
        )
        .await
        .map_err(Into::into);
    }

    let source_allowed = {
        let settings = settings.lock().await;
        peer_is_allowed(&settings, &peer_info.hostname, &peer_public_key, &source)
    };
    if !source_allowed {
        secure::write_error(&mut stream, "Peer is not paired or is disabled").await?;
        return Ok(());
    }

    info!(
        "Authenticated peer {} ({}) connected as {} [{}]",
        peer_addr, peer_info.tailscale_ip, peer_info.hostname, source_device_id
    );
    {
        let mut settings = settings.lock().await;
        if let Err(error) = settings.remember_peer_address(
            &peer_info.hostname,
            source_interface.as_str(),
            &source_address,
        ) {
            warn!(
                "Could not remember address for {}: {error}",
                peer_info.hostname
            );
        }
        if source_interface != ConnectionInterface::Iroh {
            if let Some(endpoint_id) = &peer_info.iroh_endpoint_id {
                if let Err(error) =
                    settings.remember_peer_address(&peer_info.hostname, "iroh", endpoint_id)
                {
                    warn!(
                        "Could not remember Iroh endpoint for {}: {error}",
                        peer_info.hostname
                    );
                }
            }
        }
    }
    secure::write_ready(&mut stream).await?;
    let _active_guard =
        register_active_session(&peer_info.hostname, source_interface, &source_address, 0);
    let receive_epoch = {
        let mut sync = sync_engine.lock().await;
        sync.set_peer_device_identity(&peer_info.hostname, source_device_id.clone());
        sync.start_receive_session(&peer_info.hostname)
    };
    let _receive_guard = sync::ReceiveSuspendGuard::new(
        sync_engine.clone(),
        peer_info.hostname.clone(),
        receive_epoch,
    );

    // ── Receive loop ─────────────────────────────────────────────
    let mut last_activity = tokio::time::Instant::now();
    let mut last_reliable_sequence = None;
    let mut image_assembler = tailsync_core::image_chunks::ImageAssembler::default();
    let mut image_chunks_enabled = stream.negotiated_capabilities().image_compressed_chunks;

    loop {
        let frame = match timeout(
            CONNECTION_TIMEOUT,
            stream.read_frame_with_admission(|command, payload_length| match command {
                Command::ImageChunk => {
                    tailsync_core::peer::rate_limit::check_peer_chunk_bytes(
                        &peer_info.hostname,
                        payload_length,
                    )
                    .map_err(ProtocolError::AdmissionRejected)
                }
                Command::TextPayload | Command::ImagePayload | Command::FileBatchStart => {
                    check_peer_event_budget(&peer_info.hostname, payload_length)
                        .map_err(ProtocolError::AdmissionRejected)
                }
                Command::FileChunk
                    if payload_length < crate::protocol::MIN_FILE_CHUNK_PAYLOAD_SIZE =>
                {
                    Err(ProtocolError::AdmissionRejected(
                        "empty file chunk is not valid".to_string(),
                    ))
                }
                _ => Ok(()),
            }),
        )
        .await
        {
            Ok(Ok(f)) => f,
            Ok(Err(ProtocolError::IncompleteFrame { .. })) => continue,
            Ok(Err(e)) => {
                warn!("Inbound peer protocol error: {e}");
                debug!("Protocol error address: {peer_addr}");
                break;
            }
            Err(_) => {
                if last_activity.elapsed() > IDLE_TIMEOUT {
                    debug!("Connection {} idle timeout", peer_addr);
                    break;
                }
                continue;
            }
        };

        last_activity = tokio::time::Instant::now();

        let still_authorized = {
            let settings = settings.lock().await;
            peer_is_allowed(&settings, &peer_info.hostname, &peer_public_key, &source)
        };
        if !still_authorized {
            secure::write_error(&mut stream, "Peer authorization was revoked").await?;
            break;
        }

        match frame.command {
            Command::Heartbeat => {
                let ack = Frame::try_new(Command::HeartbeatAck, 0, frame.sequence, vec![])?;
                stream.write_frame(&ack).await?;
            }
            Command::TextPayload => {
                if let Err(error) = process_reliable_event(
                    &mut stream,
                    &frame,
                    &peer_info.hostname,
                    &sync_engine,
                    &database,
                    &mut last_reliable_sequence,
                    crate::api::bump_clipboard_version,
                )
                .await
                {
                    if error.is_retryable() {
                        warn!("Temporarily unable to apply text event from remote peer: {error}");
                        debug!("Transient text event failure address: {peer_addr}");
                        continue;
                    }
                    warn!("Rejected text event from remote peer: {error}");
                    debug!("Rejected text event address: {peer_addr}");
                    secure::write_error(&mut stream, &error.to_string()).await?;
                }
            }
            Command::ImagePayload => {
                if let Err(error) = process_reliable_event(
                    &mut stream,
                    &frame,
                    &peer_info.hostname,
                    &sync_engine,
                    &database,
                    &mut last_reliable_sequence,
                    crate::api::bump_clipboard_version,
                )
                .await
                {
                    if error.is_retryable() {
                        warn!("Temporarily unable to apply image event from remote peer: {error}");
                        debug!("Transient image event failure address: {peer_addr}");
                        continue;
                    }
                    warn!("Rejected image event from remote peer: {error}");
                    debug!("Rejected image event address: {peer_addr}");
                    secure::write_error(&mut stream, &error.to_string()).await?;
                }
            }
            Command::ImageChunk => {
                if !image_chunks_enabled {
                    secure::write_image_chunk_error(&mut stream).await?;
                    continue;
                }
                if frame.payload.get(28..30) == Some(&[0, 0]) {
                    if let Err(error) = check_peer_event_budget(&peer_info.hostname, 0) {
                        secure::write_error(&mut stream, &error).await?;
                        continue;
                    }
                }
                match image_assembler.accept(&frame.payload) {
                    Ok((message_id, None)) => {
                        let ack = Frame::try_new(
                            Command::EventAck,
                            0,
                            frame.sequence,
                            message_id.ack_payload(),
                        )?;
                        stream.write_frame(&ack).await?;
                    }
                    Ok((_, Some(envelope))) => {
                        let image_frame = Frame::try_new(
                            Command::ImagePayload,
                            0,
                            frame.sequence,
                            envelope.encode(),
                        )?;
                        if let Err(error) = process_reliable_event(
                            &mut stream,
                            &image_frame,
                            &peer_info.hostname,
                            &sync_engine,
                            &database,
                            &mut last_reliable_sequence,
                            crate::api::bump_clipboard_version,
                        )
                        .await
                        {
                            if error.is_retryable() {
                                warn!("Temporarily unable to apply compressed image: {error}");
                            } else {
                                secure::write_error(&mut stream, &error.to_string()).await?;
                            }
                        }
                    }
                    Err(error) => {
                        warn!("Disabling compressed image chunks for this session: {error}");
                        image_chunks_enabled = false;
                        secure::write_image_chunk_error(&mut stream).await?;
                    }
                }
            }
            Command::FileBatchStart => {
                let manifest: sync::FileBatchManifest = serde_json::from_slice(&frame.payload)?;
                tracing::info!(
                    session_id = %session_id,
                    peer = %peer_info.hostname,
                    peer_fingerprint = %source_device_id,
                    batch_id = %manifest.batch_id.as_hex(),
                    sequence = frame.sequence,
                    "file batch start received"
                );
                let result = {
                    let _admission_guard = sync::file_batch_admission_lock().lock().await;
                    match manifest.validate() {
                        Err(error) => Err(error.to_string()),
                        Ok(()) => {
                            let manifest_hash =
                                sync::SyncEngine::file_batch_manifest_hash(&manifest)
                                    .map_err(|error| error.to_string())?;
                            let (already_active, pending_bytes) = {
                                let engine = sync_engine.lock().await;
                                (
                                    engine.has_file_batch(&peer_info.hostname, manifest.batch_id)
                                        || engine.is_file_batch_completed(
                                            &peer_info.hostname,
                                            manifest.batch_id,
                                        ),
                                    engine.pending_file_batch_bytes(),
                                )
                            };
                            let receipt = {
                                let database = database.lock().await;
                                database
                                    .received_file_batch_receipt(
                                        &source_device_id,
                                        &manifest.batch_id.as_hex(),
                                    )
                                    .map_err(|error| error.to_string())?
                            };
                            let durable_complete = match receipt {
                                Some((stored_hash, _status)) if stored_hash != manifest_hash => {
                                    Err("Batch ID was reused with a different manifest".to_string())
                                }
                                Some((_, status)) => Ok(status == "complete"),
                                None => Ok(false),
                            };
                            match durable_complete {
                                Err(error) => Err(error),
                                Ok(durable_complete) => {
                                    let preflight = if !already_active && !durable_complete {
                                        database
                                            .lock()
                                            .await
                                            .reserve_for_file_batch(
                                                manifest.total_bytes.saturating_add(pending_bytes),
                                            )
                                            .map_err(|error| error.to_string())
                                    } else {
                                        Ok(())
                                    };
                                    match preflight {
                                        Ok(()) if durable_complete => {
                                            sync_engine.lock().await.remember_completed_file_batch(
                                                manifest.clone(),
                                                peer_info.hostname.clone(),
                                            )
                                        }
                                        Ok(()) => {
                                            sync::SyncEngine::begin_file_batch_shared(
                                                &sync_engine,
                                                manifest.clone(),
                                                peer_info.hostname.clone(),
                                                source_device_id.clone(),
                                                db::get_incoming_dir(),
                                                receive_epoch,
                                            )
                                            .await
                                        }
                                        Err(error) => Err(error),
                                    }
                                }
                            }
                        }
                    }
                };
                if let Err(error) = &result {
                    sync_engine
                        .lock()
                        .await
                        .notify_file_batch_failed(Some(manifest.batch_id), error);
                }
                let response = match result {
                    Ok(()) => Frame::try_new(
                        Command::FileBatchAccept,
                        0,
                        frame.sequence,
                        manifest.batch_id.0.to_vec(),
                    )?,
                    Err(error) => Frame::try_new(
                        Command::FileBatchReject,
                        0,
                        frame.sequence,
                        error.into_bytes(),
                    )?,
                };
                stream.write_frame(&response).await?;
            }
            Command::FileBatchComplete => {
                let bytes: [u8; 16] = frame
                    .payload
                    .as_slice()
                    .try_into()
                    .map_err(|_| "Invalid file batch completion ID")?;
                let batch_id = TransferId(bytes);
                tracing::info!(
                    session_id = %session_id,
                    peer = %peer_info.hostname,
                    peer_fingerprint = %source_device_id,
                    batch_id = %batch_id.as_hex(),
                    sequence = frame.sequence,
                    "file batch completion received"
                );
                let result = sync::SyncEngine::finish_file_batch_shared(
                    &sync_engine,
                    &peer_info.hostname,
                    batch_id,
                )
                .await;
                if let Err(error) = &result {
                    sync_engine
                        .lock()
                        .await
                        .notify_file_batch_failed(Some(batch_id), error);
                }
                let acknowledged = result.is_ok();
                let response = match result {
                    Ok(()) => Frame::try_new(
                        Command::FileBatchAccept,
                        0,
                        frame.sequence,
                        batch_id.0.to_vec(),
                    )?,
                    Err(error) => Frame::try_new(
                        Command::FileBatchReject,
                        0,
                        frame.sequence,
                        error.into_bytes(),
                    )?,
                };
                stream.write_frame(&response).await?;
                if acknowledged {
                    if let Err(error) = sync::SyncEngine::acknowledge_file_batch_shared(
                        &sync_engine,
                        &peer_info.hostname,
                        batch_id,
                        &db::get_incoming_dir(),
                    )
                    .await
                    {
                        warn!(
                            "File batch {batch_id:?} was acknowledged but cleanup remains pending: {error}"
                        );
                    }
                }
            }
            Command::FileBatchCancel => {
                let bytes: [u8; 16] = frame
                    .payload
                    .as_slice()
                    .try_into()
                    .map_err(|_| "Invalid file batch cancellation ID")?;
                let batch_id = TransferId(bytes);
                tracing::info!(
                    session_id = %session_id,
                    peer = %peer_info.hostname,
                    peer_fingerprint = %source_device_id,
                    batch_id = %batch_id.as_hex(),
                    sequence = frame.sequence,
                    "file batch cancellation received"
                );
                let was_receiving = sync::SyncEngine::cancel_file_batch_shared(
                    &sync_engine,
                    &peer_info.hostname,
                    batch_id,
                )
                .await;
                if !was_receiving {
                    crate::api::request_file_batch_cancel(&batch_id.as_hex());
                }
            }
            Command::FileMeta => {
                let mut meta: sync::FileMeta = serde_json::from_slice(&frame.payload)?;
                if let Err(error) = sync::validate_incoming_file_meta(&mut meta) {
                    let message = error.to_string();
                    secure::write_error(&mut stream, &message).await?;
                    continue;
                }
                let resumable = meta.transfer_id.is_some();
                tracing::info!(
                    session_id = %session_id,
                    peer = %peer_info.hostname,
                    peer_fingerprint = %source_device_id,
                    transfer_id = ?meta.transfer_id.map(|id| id.as_hex()),
                    batch_id = ?meta.batch.map(|batch| batch.batch_id.as_hex()),
                    sequence = frame.sequence,
                    size = meta.size,
                    resumable,
                    "file metadata received"
                );
                info!(
                    "Receiving file from {}: {} ({} bytes)",
                    peer_addr, meta.name, meta.size
                );
                let incoming_dir = db::get_incoming_dir();
                std::fs::create_dir_all(&incoming_dir)?;
                let file_path =
                    incoming_dir.join(format!("{:016x}-{}", rand::random::<u64>(), meta.name));
                let meta_batch_id = meta.batch.map(|batch| batch.batch_id);
                let result = sync::SyncEngine::begin_file_receive_shared(
                    &sync_engine,
                    meta,
                    &file_path,
                    peer_info.hostname.clone(),
                    receive_epoch,
                )
                .await;
                let result = match result {
                    Ok(mut progress) => {
                        if let Some(pending) = progress.completed.take() {
                            sync::verify_and_commit_received_file(
                                &sync_engine,
                                &peer_info.hostname,
                                pending,
                            )
                            .await
                            .map(|()| progress)
                        } else {
                            Ok(progress)
                        }
                    }
                    Err(error) => Err(error),
                };
                match result {
                    Ok(progress) if resumable => {
                        let response = Frame::try_new(
                            Command::FileResume,
                            0,
                            frame.sequence,
                            FileOffset {
                                transfer_id: progress.transfer_id,
                                next_offset: progress.next_offset,
                            }
                            .encode(),
                        )?;
                        stream.write_frame(&response).await?;
                    }
                    Ok(_) => {}
                    Err(error) => {
                        sync_engine
                            .lock()
                            .await
                            .notify_file_batch_failed(meta_batch_id, &error);
                        if let Some(batch_id) = meta_batch_id {
                            sync::SyncEngine::cancel_file_batch_shared(
                                &sync_engine,
                                &peer_info.hostname,
                                batch_id,
                            )
                            .await;
                        }
                        secure::write_error(&mut stream, &error).await?;
                    }
                }
            }
            Command::FileChunk => {
                if frame.payload.starts_with(b"FCH1") {
                    match FileChunkPayload::decode(&frame.payload) {
                        Ok(chunk) => {
                            tracing::debug!(
                                session_id = %session_id,
                                peer = %peer_info.hostname,
                                peer_fingerprint = %source_device_id,
                                transfer_id = %chunk.transfer_id.as_hex(),
                                offset = chunk.offset,
                                bytes = chunk.data.len(),
                                sequence = frame.sequence,
                                "file chunk received"
                            );
                            let expected_end = chunk.offset.saturating_add(chunk.data.len() as u64);
                            let chunk_batch_id = sync_engine
                                .lock()
                                .await
                                .batch_for_transfer(&peer_info.hostname, chunk.transfer_id);
                            let result = sync::SyncEngine::handle_resumable_file_chunk_shared(
                                &sync_engine,
                                &chunk,
                                peer_info.hostname.clone(),
                            )
                            .await;
                            let result = match result {
                                Ok(mut progress) => {
                                    if let Some(pending) = progress.completed.take() {
                                        sync::verify_and_commit_received_file(
                                            &sync_engine,
                                            &peer_info.hostname,
                                            pending,
                                        )
                                        .await
                                        .map(|()| progress)
                                        .map_err(sync::FileReceiveError::from)
                                    } else {
                                        Ok(progress)
                                    }
                                }
                                Err(error) => Err(error),
                            };
                            match result {
                                Ok(progress) => {
                                    let command = if progress.next_offset >= expected_end {
                                        Command::FileAck
                                    } else {
                                        Command::FileResume
                                    };
                                    let response = Frame::try_new(
                                        command,
                                        0,
                                        frame.sequence,
                                        FileOffset {
                                            transfer_id: chunk.transfer_id,
                                            next_offset: progress.next_offset,
                                        }
                                        .encode(),
                                    )?;
                                    tracing::debug!(
                                        session_id = %session_id,
                                        peer = %peer_info.hostname,
                                        peer_fingerprint = %source_device_id,
                                        transfer_id = %chunk.transfer_id.as_hex(),
                                        next_offset = progress.next_offset,
                                        sequence = frame.sequence,
                                        command = ?command,
                                        "file transfer acknowledgement sent"
                                    );
                                    stream.write_frame(&response).await?;
                                }
                                Err(error) if error.is_metadata_unavailable() => {
                                    let next_offset = sync::persisted_file_resume_offset(
                                        &peer_info.hostname,
                                        chunk.transfer_id,
                                        &db::get_incoming_dir(),
                                    )
                                    .unwrap_or(0);
                                    let response = Frame::try_new(
                                        Command::FileResume,
                                        0,
                                        frame.sequence,
                                        FileOffset {
                                            transfer_id: chunk.transfer_id,
                                            next_offset,
                                        }
                                        .encode(),
                                    )?;
                                    stream.write_frame(&response).await?;
                                }
                                Err(error) => {
                                    let error_message = error.to_string();
                                    sync_engine
                                        .lock()
                                        .await
                                        .notify_file_batch_failed(chunk_batch_id, &error_message);
                                    if let Some(batch_id) = chunk_batch_id {
                                        sync::SyncEngine::cancel_file_batch_shared(
                                            &sync_engine,
                                            &peer_info.hostname,
                                            batch_id,
                                        )
                                        .await;
                                    }
                                    secure::write_error(&mut stream, &error_message).await?;
                                }
                            }
                        }
                        Err(error) => {
                            secure::write_error(&mut stream, &error.to_string()).await?;
                            return Err(error.into());
                        }
                    }
                } else {
                    sync::SyncEngine::handle_file_chunk_shared(
                        &sync_engine,
                        &frame.payload,
                        peer_info.hostname.clone(),
                    )
                    .await;
                }
            }
            Command::CancelTransfer => {
                warn!("Transfer cancelled by remote peer");
                debug!("Transfer cancellation address: {peer_addr}");
                sync::SyncEngine::cancel_receive_shared(&sync_engine, &peer_info.hostname).await;
            }
            Command::PeerError => {
                let msg = String::from_utf8_lossy(&frame.payload);
                warn!("Remote peer error: {msg}");
                debug!("Remote error address: {peer_addr}");
            }
            _ => {
                debug!("Unhandled command {:?} from {}", frame.command, peer_addr);
            }
        }
    }

    debug!("Connection {peer_addr} closed");
    Ok(())
}

pub(super) use tailsync_core::peer::directory::source_matches_mode;

pub(super) fn local_peer_identity(mode: &str) -> secure::PeerIdentity {
    // Peer authentication is bound to the Noise static key and hostname. The
    // socket address is recorded separately, so handshakes must not block on
    // spawning `tailscale status` for every connection attempt.
    secure::PeerIdentity {
        hostname: lan::local_hostname(),
        tailscale_ip: String::new(),
        iroh_endpoint_id: tailsync_core::peer::types::ConnectionMode::parse(mode)
            .is_some_and(|mode| {
                mode.allows(tailsync_core::peer::types::ConnectionInterface::Iroh)
            })
            .then(iroh::local_endpoint_id)
            .flatten(),
    }
}

#[cfg(test)]
mod acceptance_tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD, Engine};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct RecordingPlatform {
        image_writes: AtomicUsize,
    }

    impl sync::SyncPlatform for RecordingPlatform {
        fn write_text(&self, _text: &str) -> Result<(), String> {
            Ok(())
        }

        fn write_image(&self, _width: u32, _height: u32, _rgba: &[u8]) -> Result<(), String> {
            self.image_writes.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn set_file_progress(&self, _name: &str, _received: u64, _total: u64) {}

        fn clear_file_progress(&self, _batch_id: Option<TransferId>, _device: Option<&str>) {}

        fn set_file_batch_progress(&self, _progress: sync::FileBatchProgress) {}

        fn files_received(&self, _commit: sync::FileReceiveCommit) -> sync::PlatformResultFuture {
            Box::pin(async { Ok(()) })
        }

        fn file_batch_failed(&self, _batch_id: Option<TransferId>, _message: &str) {}
    }

    async fn open_session(
        server_identity: Arc<DeviceIdentity>,
        client_identity: &DeviceIdentity,
        settings: Arc<Mutex<crypto::Settings>>,
        sync_engine: Arc<Mutex<sync::SyncEngine>>,
        database: Arc<Mutex<db::HistoryDB>>,
    ) -> (Result<secure::SecureConnection, String>, tokio::task::JoinHandle<()>) {
        let (client_io, server_io) = tokio::io::duplex(256 * 1024);
        let expected_server_key = server_identity.public_key().to_vec();
        let server = tokio::spawn(async move {
            let (_, pairing_window) = watch::channel(false);
            let accepted = secure::accept_with_pairing_window_and_capabilities(
                server_io,
                &server_identity,
                secure::PeerIdentity {
                    hostname: "server".into(),
                    tailscale_ip: String::new(),
                    iroh_endpoint_id: None,
                },
                pairing_window,
                secure::CapabilitySwitches {
                    file_sliding_window: true,
                    image_compressed_chunks: true,
                },
            )
            .await
            .expect("fixture handshake");
            handle_accepted_connection(
                accepted,
                InboundSource::Tcp("127.0.0.1:49152".parse().unwrap()),
                sync_engine,
                database,
                settings,
                None,
                None,
            )
            .await
            .expect("fixture receive session");
        });
        let client = secure::connect_with_capabilities(
            client_io,
            client_identity,
            secure::PeerIdentity {
                hostname: "client".into(),
                tailscale_ip: String::new(),
                iroh_endpoint_id: None,
            },
            "server",
            &expected_server_key,
            secure::CapabilitySwitches {
                file_sliding_window: true,
                image_compressed_chunks: true,
            },
        )
        .await
        .map_err(|error| error.to_string());
        (client, server)
    }

    fn image_fixture() -> Vec<Vec<u8>> {
        let mut rgba = vec![0_u8; 1024 * 1024 * 4];
        let mut state = 0x7ace_b00c_u32;
        for byte in rgba.iter_mut().take(1024 * 1024) {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            *byte = state as u8;
        }
        let packed = tailsync_core::protocol::pack_rgba_image(1024, 1024, &rgba).unwrap();
        let envelope = tailsync_core::protocol::EventEnvelope::new(packed);
        let chunks = tailsync_core::image_chunks::compress(&envelope)
            .unwrap()
            .expect("fixture must use compressed chunks");
        assert!(chunks.len() >= 2, "fixture must exercise a partial assembly");
        chunks
    }

    #[tokio::test]
    async fn malformed_image_chunk_is_rejected_without_partial_apply_and_next_session_recovers() {
        let server_identity = Arc::new(DeviceIdentity::generate_for_test());
        let client_identity = DeviceIdentity::generate_for_test();
        let mut trusted = crypto::Settings {
            connection_mode: "auto".into(),
            ..crypto::Settings::default()
        };
        trusted.trusted_peer_keys.insert(
            "client".into(),
            STANDARD.encode(client_identity.public_key()),
        );
        let settings = Arc::new(Mutex::new(trusted));
        let platform = Arc::new(RecordingPlatform::default());
        let mut engine = sync::SyncEngine::new();
        engine.set_platform(platform.clone());
        let sync_engine = Arc::new(Mutex::new(engine));
        let database = Arc::new(Mutex::new(db::HistoryDB::new_unavailable().unwrap()));
        let chunks = image_fixture();
        let first = chunks[0].clone();
        let second = chunks[1].clone();

        let (client, server) = open_session(
            server_identity.clone(),
            &client_identity,
            settings.clone(),
            sync_engine.clone(),
            database.clone(),
        )
        .await;
        let mut client = client.expect("trusted fixture must connect");
        assert!(client.negotiated_capabilities().image_compressed_chunks);
        client
            .write_frame(&Frame::try_new(Command::ImageChunk, 0, 1, first.clone()).unwrap())
            .await
            .unwrap();
        assert_eq!(client.read_frame().await.unwrap().command, Command::EventAck);
        assert!(database.lock().await.get_all(None, None, 10, 0).unwrap().is_empty());
        assert_eq!(platform.image_writes.load(Ordering::SeqCst), 0);

        // A duplicate/late first chunk is invalid while the second is due.
        client
            .write_frame(&Frame::try_new(Command::ImageChunk, 0, 2, first.clone()).unwrap())
            .await
            .unwrap();
        let error = client.read_frame().await.unwrap();
        assert_eq!(error.command, Command::PeerError);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&error.payload).unwrap()["code"],
            "invalid_image_chunk"
        );
        client
            .write_frame(&Frame::try_new(Command::ImageChunk, 0, 3, second.clone()).unwrap())
            .await
            .unwrap();
        let disabled = client.read_frame().await.unwrap();
        assert_eq!(disabled.command, Command::PeerError);
        assert_eq!(disabled.payload, error.payload);
        assert!(database.lock().await.get_all(None, None, 10, 0).unwrap().is_empty());
        assert_eq!(platform.image_writes.load(Ordering::SeqCst), 0);
        drop(client);
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap();

        let mut forged_raw_length = first.clone();
        forged_raw_length[36..40].copy_from_slice(&u32::MAX.to_be_bytes());
        let mut bad_digest = chunks.clone();
        for chunk in &mut bad_digest {
            chunk[40] ^= 1;
        }
        let mut inflated = chunks.clone();
        for chunk in &mut inflated {
            chunk[36..40].copy_from_slice(&12_u32.to_be_bytes());
        }
        for (case, payloads) in [
            ("out_of_order", vec![second.clone()]),
            ("forged_raw_length", vec![forged_raw_length]),
            ("bad_digest", bad_digest),
            ("inflated_image", inflated),
        ] {
            let (candidate, task) = open_session(
                server_identity.clone(),
                &client_identity,
                settings.clone(),
                sync_engine.clone(),
                database.clone(),
            )
            .await;
            let mut candidate = candidate.expect("trusted fixture must connect");
            for (index, payload) in payloads.iter().enumerate() {
                candidate
                    .write_frame(
                        &Frame::try_new(Command::ImageChunk, 0, index as u32 + 1, payload.clone())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                let response = candidate.read_frame().await.unwrap();
                if index + 1 == payloads.len() {
                    assert_eq!(response.command, Command::PeerError, "{case}");
                    assert_eq!(response.payload, error.payload, "{case}");
                } else {
                    assert_eq!(response.command, Command::EventAck, "{case}");
                }
            }
            assert!(database.lock().await.get_all(None, None, 10, 0).unwrap().is_empty());
            assert_eq!(platform.image_writes.load(Ordering::SeqCst), 0, "{case}");
            drop(candidate);
            tokio::time::timeout(Duration::from_secs(2), task)
                .await
                .unwrap()
                .unwrap();
        }

        let (fresh, fresh_server) = open_session(
            server_identity,
            &client_identity,
            settings,
            sync_engine,
            database.clone(),
        )
        .await;
        let mut fresh = fresh.expect("fresh trusted fixture must connect");
        fresh
            .write_frame(&Frame::try_new(Command::ImageChunk, 0, 1, first).unwrap())
            .await
            .unwrap();
        assert_eq!(fresh.read_frame().await.unwrap().command, Command::EventAck);
        assert!(database.lock().await.get_all(None, None, 10, 0).unwrap().is_empty());
        assert_eq!(platform.image_writes.load(Ordering::SeqCst), 0);
        drop(fresh);
        tokio::time::timeout(Duration::from_secs(2), fresh_server)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn authenticated_but_unpaired_peer_cannot_enter_receive_loop() {
        let server_identity = Arc::new(DeviceIdentity::generate_for_test());
        let client_identity = DeviceIdentity::generate_for_test();
        let settings = Arc::new(Mutex::new(crypto::Settings::default()));
        let database = Arc::new(Mutex::new(db::HistoryDB::new_unavailable().unwrap()));
        let (client, server) = open_session(
            server_identity,
            &client_identity,
            settings,
            Arc::new(Mutex::new(sync::SyncEngine::new())),
            database.clone(),
        )
        .await;
        assert!(client.err().expect("unpaired peer must be rejected").contains("not paired"));
        assert!(database.lock().await.get_all(None, None, 10, 0).unwrap().is_empty());
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn disabling_a_paired_peer_revokes_its_active_receive_session() {
        let server_identity = Arc::new(DeviceIdentity::generate_for_test());
        let client_identity = DeviceIdentity::generate_for_test();
        let mut trusted = crypto::Settings {
            connection_mode: "auto".into(),
            ..crypto::Settings::default()
        };
        trusted.trusted_peer_keys.insert(
            "client".into(),
            STANDARD.encode(client_identity.public_key()),
        );
        let settings = Arc::new(Mutex::new(trusted));
        let database = Arc::new(Mutex::new(db::HistoryDB::new_unavailable().unwrap()));
        let (client, server) = open_session(
            server_identity,
            &client_identity,
            settings.clone(),
            Arc::new(Mutex::new(sync::SyncEngine::new())),
            database.clone(),
        )
        .await;
        let mut client = client.expect("paired peer must connect");
        settings
            .lock()
            .await
            .enabled_peers
            .insert("client".into(), false);
        client
            .write_frame(&Frame::try_new(Command::Heartbeat, 0, 1, Vec::new()).unwrap())
            .await
            .unwrap();
        let rejection = client.read_frame().await.unwrap();
        assert_eq!(rejection.command, Command::PeerError);
        assert!(String::from_utf8_lossy(&rejection.payload).contains("revoked"));
        assert!(database.lock().await.get_all(None, None, 10, 0).unwrap().is_empty());
        drop(client);
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap();
    }
}
