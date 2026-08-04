use super::*;

pub(super) struct ConnectionLimiter {
    total: Arc<Semaphore>,
    per_ip: StdMutex<HashMap<IpAddr, usize>>,
    max_per_ip: usize,
}

pub(super) struct ConnectionPermit {
    limiter: Arc<ConnectionLimiter>,
    ip: IpAddr,
    _total: OwnedSemaphorePermit,
}

impl ConnectionLimiter {
    pub(super) fn new(max_total: usize, max_per_ip: usize) -> Arc<Self> {
        Arc::new(Self {
            total: Arc::new(Semaphore::new(max_total)),
            per_ip: StdMutex::new(HashMap::new()),
            max_per_ip,
        })
    }

    pub(super) fn try_acquire(self: &Arc<Self>, ip: IpAddr) -> Option<ConnectionPermit> {
        let total = self.total.clone().try_acquire_owned().ok()?;
        let mut counts = self
            .per_ip
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let count = counts.entry(ip).or_default();
        if *count >= self.max_per_ip {
            return None;
        }
        *count += 1;
        drop(counts);
        Some(ConnectionPermit {
            limiter: self.clone(),
            ip,
            _total: total,
        })
    }
}

impl Drop for ConnectionPermit {
    fn drop(&mut self) {
        let mut counts = self
            .limiter
            .per_ip
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(count) = counts.get_mut(&self.ip) {
            *count -= 1;
            if *count == 0 {
                counts.remove(&self.ip);
            }
        }
    }
}

struct ReceiveSuspendGuard {
    sync_engine: Arc<Mutex<sync::SyncEngine>>,
    source: String,
}

impl Drop for ReceiveSuspendGuard {
    fn drop(&mut self) {
        let sync_engine = self.sync_engine.clone();
        let source = self.source.clone();
        tokio::spawn(async move {
            sync_engine.lock().await.suspend_receive(&source);
        });
    }
}

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
    let purpose = accepted.purpose;
    let handshake_hash = accepted.handshake_hash;
    let mut stream = accepted.connection;
    let peer_info = accepted.peer_identity;
    let peer_public_key = accepted.remote_public_key;

    if purpose == secure::HandshakePurpose::Pairing {
        let address = peer_addr.ip().to_string();
        let interface = infer_interface(&address)?.as_str().to_string();
        secure::write_ready(&mut stream).await?;
        return pairing
            .install_session(PendingPairing {
                connection: stream,
                hostname: peer_info.hostname,
                remote_public_key: peer_public_key,
                handshake_hash,
                address,
                interface,
            })
            .await
            .map_err(Into::into);
    }

    let (trusted_key, peer_enabled, source_allowed) = {
        let settings = settings.lock().await;
        (
            settings
                .trusted_peer_keys
                .get(&peer_info.hostname)
                .and_then(|key| secure::decode_trusted_key(key).ok()),
            settings
                .enabled_peers
                .get(&peer_info.hostname)
                .copied()
                .unwrap_or(true),
            source_matches_mode(peer_addr.ip(), &settings.connection_mode),
        )
    };
    if !source_allowed
        || !peer_enabled
        || trusted_key.as_deref() != Some(peer_public_key.as_slice())
    {
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
    let source_interface = infer_interface(&peer_addr.ip().to_string())?;
    if let Err(error) = settings.lock().await.remember_peer_address(
        &peer_info.hostname,
        source_interface.as_str(),
        &peer_addr.ip().to_string(),
    ) {
        warn!(
            "Could not remember address for {}: {error}",
            peer_info.hostname
        );
    }
    secure::write_ready(&mut stream).await?;
    let _active_guard = authenticated_sessions().register(
        RouteKey {
            hostname: peer_info.hostname.clone(),
            interface: source_interface,
            address: peer_addr.ip().to_string(),
        },
        0,
    );
    let _receive_guard = ReceiveSuspendGuard {
        sync_engine: sync_engine.clone(),
        source: peer_info.hostname.clone(),
    };

    // ── Receive loop ─────────────────────────────────────────────
    let mut last_activity = tokio::time::Instant::now();
    let mut last_reliable_sequence = None;

    loop {
        let frame = match timeout(
            CONNECTION_TIMEOUT,
            stream.read_frame_with_admission(|command, payload_length| match command {
                Command::TextPayload | Command::ImagePayload => {
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
            settings
                .trusted_peer_keys
                .get(&peer_info.hostname)
                .and_then(|key| secure::decode_trusted_key(key).ok())
                .as_deref()
                == Some(peer_public_key.as_slice())
                && settings
                    .enabled_peers
                    .get(&peer_info.hostname)
                    .copied()
                    .unwrap_or(true)
                && source_matches_mode(peer_addr.ip(), &settings.connection_mode)
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
                if let Err(error) = receive_reliable_event(
                    &mut stream,
                    frame,
                    &peer_info.hostname,
                    &sync_engine,
                    &database,
                    &mut last_reliable_sequence,
                )
                .await
                {
                    warn!("Rejected text event from remote peer: {error}");
                    debug!("Rejected text event address: {peer_addr}");
                    secure::write_error(&mut stream, &error).await?;
                }
            }
            Command::ImagePayload => {
                if let Err(error) = receive_reliable_event(
                    &mut stream,
                    frame,
                    &peer_info.hostname,
                    &sync_engine,
                    &database,
                    &mut last_reliable_sequence,
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
                let already_active = sync_engine
                    .lock()
                    .await
                    .has_file_batch(&peer_info.hostname, manifest.batch_id);
                let preflight = if !already_active {
                    database
                        .lock()
                        .await
                        .reserve_for_file_batch(manifest.total_bytes)
                        .map_err(|error| error.to_string())
                } else {
                    Ok(())
                };
                let result = match preflight {
                    Ok(()) => sync_engine.lock().await.begin_file_batch(
                        manifest.clone(),
                        peer_info.hostname.clone(),
                        &db::get_incoming_dir(),
                    ),
                    Err(error) => Err(error),
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
                if meta.batch.is_none() {
                    secure::write_error(
                        &mut stream,
                        "This TailSync version requires the file_batch_v1 protocol",
                    )
                    .await?;
                    continue;
                }
                if meta.size > MAX_FILE_SIZE {
                    secure::write_error(&mut stream, "File exceeds the 1 GiB receive limit")
                        .await?;
                    continue;
                }
                let Some(file_name) = std::path::Path::new(&meta.name).file_name() else {
                    secure::write_error(&mut stream, "Invalid file name").await?;
                    continue;
                };
                meta.name = file_name.to_string_lossy().to_string();
                meta.name = sync::normalize_transferred_file_name(&meta.name, &meta.hash);
                if meta.name.is_empty() {
                    secure::write_error(&mut stream, "Invalid file name").await?;
                    continue;
                }
                if meta.transfer_id.is_some()
                    && (meta.chunk_size == 0 || meta.chunk_size as usize > FILE_CHUNK_SIZE)
                {
                    secure::write_error(&mut stream, "Invalid file chunk size").await?;
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
                let result = sync_engine
                    .lock()
                    .await
                    .begin_file_receive(meta, &file_path, peer_info.hostname.clone())
                    .await;
                match result {
                    Ok((transfer_id, next_offset)) if resumable => {
                        let response = Frame::try_new(
                            Command::FileResume,
                            0,
                            frame.sequence,
                            FileOffset {
                                transfer_id,
                                next_offset,
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
                        secure::write_error(&mut stream, &error).await?;
                    }
                }
            }
            Command::FileChunk => {
                if frame.payload.starts_with(b"FCH1") {
                    match FileChunkPayload::decode(&frame.payload) {
                        Ok(chunk) => {
                            let expected_end = chunk.offset.saturating_add(chunk.data.len() as u64);
                            match sync_engine
                                .lock()
                                .await
                                .handle_resumable_file_chunk(&chunk, peer_info.hostname.clone())
                                .await
                            {
                                Ok(next_offset) => {
                                    let command = if next_offset >= expected_end {
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
                                            next_offset,
                                        }
                                        .encode(),
                                    )?;
                                    stream.write_frame(&response).await?;
                                }
                                Err(error) => {
                                    let mut sync = sync_engine.lock().await;
                                    let batch_id = sync
                                        .batch_for_transfer(&peer_info.hostname, chunk.transfer_id);
                                    sync.notify_file_batch_failed(batch_id, &error);
                                    if let Some(batch_id) = batch_id {
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

async fn receive_reliable_event(
    stream: &mut secure::SecureConnection,
    frame: Frame,
    source: &str,
    sync_engine: &Arc<Mutex<sync::SyncEngine>>,
    database: &Arc<Mutex<db::HistoryDB>>,
    last_sequence: &mut Option<u32>,
) -> Result<(), String> {
    let envelope = EventEnvelope::decode(&frame.payload).map_err(|error| error.to_string())?;
    envelope
        .validate_timestamp(unix_timestamp_ms())
        .map_err(|error| error.to_string())?;

    let duplicate = sync_engine
        .lock()
        .await
        .has_seen_message(source, envelope.message_id);
    if last_sequence.is_some_and(|last| frame.sequence <= last) && !duplicate {
        return Err(format!(
            "replayed or out-of-order event sequence {}",
            frame.sequence
        ));
    }

    if !duplicate {
        let kind = match frame.command {
            Command::TextPayload => "text",
            Command::ImagePayload => "image",
            _ => {
                return Err(format!(
                    "{:?} is not a reliable event command",
                    frame.command
                ))
            }
        };
        process_event_content(
            frame.command,
            &envelope.content,
            source,
            sync_engine,
            database,
        )
        .await?;
        info!("{kind} event from {source} applied");
        sync_engine
            .lock()
            .await
            .record_message(source, envelope.message_id);
        crate::api::bump_clipboard_version();
    } else {
        debug!("Reliable event from {source} was already applied; acknowledging again");
    }

    if last_sequence.is_none_or(|last| frame.sequence > last) {
        *last_sequence = Some(frame.sequence);
    }
    let ack = Frame::try_new(
        Command::EventAck,
        0,
        frame.sequence,
        envelope.message_id.ack_payload(),
    )
    .map_err(|error| error.to_string())?;
    stream
        .write_frame(&ack)
        .await
        .map_err(|error| error.to_string())
}

async fn process_event_content(
    command: Command,
    content: &[u8],
    source: &str,
    sync_engine: &Arc<Mutex<sync::SyncEngine>>,
    database: &Arc<Mutex<db::HistoryDB>>,
) -> Result<(), String> {
    match command {
        Command::TextPayload => {
            let text = String::from_utf8(content.to_vec())
                .map_err(|_| "text event is not valid UTF-8".to_string())?;
            let db = database.clone();
            let db_text = text.clone();
            let db_source = source.to_string();
            tokio::task::spawn_blocking(move || {
                db.blocking_lock()
                    .add_text(&db_text, &db_source)
                    .map_err(|error| error.to_string())
            })
            .await
            .map_err(|error| error.to_string())??;

            info!("Received text from {}: {} chars", source, text.len());
            sync_engine
                .lock()
                .await
                .handle_incoming_text(&text, source.to_string())
                .await?;
        }
        Command::ImagePayload => {
            validate_packed_image(content)?;
            let db = database.clone();
            let image = content.to_vec();
            let db_source = source.to_string();
            tokio::task::spawn_blocking(move || {
                db.blocking_lock()
                    .add_image(&image, &db_source)
                    .map_err(|error| error.to_string())
            })
            .await
            .map_err(|error| error.to_string())??;

            info!("Received image from {}: {} bytes", source, content.len());
            sync_engine
                .lock()
                .await
                .handle_incoming_image(content, source.to_string())
                .await?;
        }
        _ => return Err(format!("{:?} is not a reliable event command", command)),
    }
    Ok(())
}

fn validate_packed_image(content: &[u8]) -> Result<(), String> {
    crate::protocol::PackedImage::try_from(content)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

pub(super) fn source_matches_mode(ip: std::net::IpAddr, mode: &str) -> bool {
    if mode == "auto" {
        return source_matches_mode(ip, "lan_only") || source_matches_mode(ip, "tailscale_only");
    }
    match (ip, mode) {
        (std::net::IpAddr::V4(ip), "tailscale" | "tailscale_only") => {
            let octets = ip.octets();
            octets[0] == 100 && (64..=127).contains(&octets[1])
        }
        (std::net::IpAddr::V6(ip), "tailscale" | "tailscale_only") => {
            let segments = ip.segments();
            segments[0] == 0xfd7a && segments[1] == 0x115c && segments[2] == 0xa1e0
        }
        (std::net::IpAddr::V4(ip), "lan" | "lan_only") => {
            ip.is_private() || ip.is_link_local() || ip.is_loopback()
        }
        (std::net::IpAddr::V6(ip), "lan" | "lan_only") => {
            let first = ip.segments()[0];
            (first & 0xfe00) == 0xfc00 || ip.is_unicast_link_local() || ip.is_loopback()
        }
        _ => false,
    }
}

pub(super) fn local_peer_identity(mode: &str) -> secure::PeerIdentity {
    // Peer authentication is bound to the Noise static key and hostname. The
    // socket address is recorded separately, so handshakes must not block on
    // spawning `tailscale status` for every connection attempt.
    let _ = mode;
    secure::PeerIdentity {
        hostname: lan::local_hostname(),
        tailscale_ip: String::new(),
    }
}
