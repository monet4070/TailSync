use super::*;
use tailsync_core::peer::admission::peer_is_allowed;
use tailsync_core::peer::event_receiver::process_reliable_event;
use tailsync_core::peer::inbound_source::InboundSource;

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
        secure::accept_with_pairing_window(
            stream,
            &identity,
            local_peer_identity(&mode),
            pairing.subscribe_window(),
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
    )
    .await
}

pub(super) async fn handle_iroh_connection(
    stream: tailsync_core::iroh_transport::IrohBiStream,
    remote_endpoint_id: String,
    sync_engine: Arc<Mutex<sync::SyncEngine>>,
    database: Arc<Mutex<db::HistoryDB>>,
    settings: Arc<Mutex<crypto::Settings>>,
    identity: Arc<DeviceIdentity>,
    pairing: Arc<PairingManager>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if settings.lock().await.connection_mode != "auto" {
        return Err("Iroh connections are only accepted in automatic mode".into());
    }
    let accepted = timeout(
        HANDSHAKE_TIMEOUT,
        secure::accept_with_pairing_window(
            stream,
            &identity,
            local_peer_identity("auto"),
            pairing.subscribe_window(),
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
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let purpose = accepted.purpose;
    let handshake_hash = accepted.handshake_hash;
    let mut stream = accepted.connection;
    let peer_info = accepted.peer_identity;
    let peer_public_key = accepted.remote_public_key;
    let peer_addr = source.description();
    let source_address = source.address();
    let source_interface = source.interface()?;

    if purpose == secure::HandshakePurpose::Pairing {
        return crate::pairing::install_pairing_session(
            pairing.as_ref(),
            stream,
            peer_info.hostname,
            peer_public_key,
            handshake_hash,
            source_address,
            source_interface.as_str().to_string(),
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
        peer_addr,
        peer_info.tailscale_ip,
        peer_info.hostname,
        secure::fingerprint(&peer_public_key)
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
    let receive_epoch = sync_engine
        .lock()
        .await
        .start_receive_session(&peer_info.hostname);
    let _receive_guard = sync::ReceiveSuspendGuard::new(
        sync_engine.clone(),
        peer_info.hostname.clone(),
        receive_epoch,
    );

    // ── Receive loop ─────────────────────────────────────────────
    let mut last_activity = tokio::time::Instant::now();
    let mut last_reliable_sequence = None;

    loop {
        let frame = match timeout(
            CONNECTION_TIMEOUT,
            stream.read_frame_with_admission(|command, payload_length| match command {
                Command::TextPayload | Command::ImagePayload | Command::FileBatchStart => {
                    check_peer_event_budget(&peer_info.hostname, payload_length)
                        .map_err(ProtocolError::AdmissionRejected)
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
                    warn!("Rejected text event from remote peer: {error}");
                    debug!("Rejected text event address: {peer_addr}");
                    secure::write_error(&mut stream, &error).await?;
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
                    warn!("Rejected image event from remote peer: {error}");
                    debug!("Rejected image event address: {peer_addr}");
                    secure::write_error(&mut stream, &error).await?;
                }
            }
            Command::FileBatchStart => {
                let manifest: sync::FileBatchManifest = serde_json::from_slice(&frame.payload)?;
                let result = {
                    let _admission_guard = sync::file_batch_admission_lock().lock().await;
                    match manifest.validate() {
                        Err(error) => Err(error.to_string()),
                        Ok(()) => {
                            let (already_active, pending_bytes) = {
                                let engine = sync_engine.lock().await;
                                (
                                    engine.has_file_batch(&peer_info.hostname, manifest.batch_id),
                                    engine.pending_file_batch_bytes(),
                                )
                            };
                            let preflight = if !already_active {
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
                                Ok(()) => sync_engine.lock().await.begin_file_batch_at_epoch(
                                    manifest.clone(),
                                    peer_info.hostname.clone(),
                                    &db::get_incoming_dir(),
                                    receive_epoch,
                                ),
                                Err(error) => Err(error),
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
                let result = sync_engine
                    .lock()
                    .await
                    .finish_file_batch(&peer_info.hostname, batch_id);
                if let Err(error) = &result {
                    sync_engine
                        .lock()
                        .await
                        .notify_file_batch_failed(Some(batch_id), error);
                }
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
            }
            Command::FileBatchCancel => {
                let bytes: [u8; 16] = frame
                    .payload
                    .as_slice()
                    .try_into()
                    .map_err(|_| "Invalid file batch cancellation ID")?;
                let batch_id = TransferId(bytes);
                let mut sync = sync_engine.lock().await;
                let was_receiving = sync.has_file_batch(&peer_info.hostname, batch_id);
                sync.cancel_file_batch(&peer_info.hostname, batch_id).await;
                drop(sync);
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
                info!(
                    "Receiving file from {}: {} ({} bytes)",
                    peer_addr, meta.name, meta.size
                );
                let incoming_dir = db::get_incoming_dir();
                std::fs::create_dir_all(&incoming_dir)?;
                let file_path =
                    incoming_dir.join(format!("{:016x}-{}", rand::random::<u64>(), meta.name));
                let meta_batch_id = meta.batch.map(|batch| batch.batch_id);
                let result = {
                    let mut sync = sync_engine.lock().await;
                    sync.begin_file_receive_at_epoch(
                        meta,
                        &file_path,
                        peer_info.hostname.clone(),
                        receive_epoch,
                    )
                    .await
                };
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
                        let mut sync = sync_engine.lock().await;
                        sync.notify_file_batch_failed(meta_batch_id, &error);
                        if let Some(batch_id) = meta_batch_id {
                            sync.cancel_file_batch(&peer_info.hostname, batch_id).await;
                        }
                        drop(sync);
                        secure::write_error(&mut stream, &error).await?;
                    }
                }
            }
            Command::FileChunk => {
                if frame.payload.starts_with(b"FCH1") {
                    match FileChunkPayload::decode(&frame.payload) {
                        Ok(chunk) => {
                            let expected_end = chunk.offset.saturating_add(chunk.data.len() as u64);
                            let (result, chunk_batch_id) = {
                                let mut sync = sync_engine.lock().await;
                                let batch_id =
                                    sync.batch_for_transfer(&peer_info.hostname, chunk.transfer_id);
                                let result = sync
                                    .handle_resumable_file_chunk(&chunk, peer_info.hostname.clone())
                                    .await;
                                (result, batch_id)
                            };
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
                                    stream.write_frame(&response).await?;
                                }
                                Err(error) => {
                                    let mut sync = sync_engine.lock().await;
                                    sync.notify_file_batch_failed(chunk_batch_id, &error);
                                    if let Some(batch_id) = chunk_batch_id {
                                        sync.cancel_file_batch(&peer_info.hostname, batch_id).await;
                                    }
                                    drop(sync);
                                    secure::write_error(&mut stream, &error).await?;
                                }
                            }
                        }
                        Err(error) => {
                            secure::write_error(&mut stream, &error.to_string()).await?;
                        }
                    }
                } else {
                    sync_engine
                        .lock()
                        .await
                        .handle_file_chunk(&frame.payload, peer_info.hostname.clone())
                        .await;
                }
            }
            Command::CancelTransfer => {
                warn!("Transfer cancelled by remote peer");
                debug!("Transfer cancellation address: {peer_addr}");
                sync_engine
                    .lock()
                    .await
                    .cancel_receive(&peer_info.hostname)
                    .await;
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
        iroh_endpoint_id: (mode == "auto").then(iroh::local_endpoint_id).flatten(),
    }
}
