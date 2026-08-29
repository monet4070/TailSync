use std::time::{SystemTime, UNIX_EPOCH};

use super::*;

impl PairingManager {
    pub fn new(settings: Arc<Mutex<Settings>>, identity: Arc<DeviceIdentity>) -> Arc<Self> {
        Self::with_policy(
            settings,
            identity,
            DEFAULT_PAIRING_WINDOW,
            DEFAULT_MAX_FAILURES,
            true,
        )
    }

    pub(crate) fn with_policy(
        settings: Arc<Mutex<Settings>>,
        identity: Arc<DeviceIdentity>,
        window_duration: Duration,
        max_failures: u8,
        persist_trust: bool,
    ) -> Arc<Self> {
        let (window_signal, _) = watch::channel(false);
        Arc::new(Self {
            state: Mutex::new(PairingState {
                enabled: false,
                phase: PairingPhase::Disabled,
                deadline: None,
                expires_at: None,
                failed_attempts: 0,
                peer: None,
                error: None,
                generation: 0,
                session_id: 0,
                control: None,
            }),
            settings,
            identity,
            window_signal,
            window_duration,
            max_failures,
            persist_trust,
        })
    }

    pub async fn enable(self: &Arc<Self>) -> PairingStatus {
        let (old_control, generation, deadline) = {
            let mut state = self.state.lock().await;
            let old_control = state.control.take();
            let deadline = Instant::now() + self.window_duration;
            state.enabled = true;
            state.phase = PairingPhase::Waiting;
            state.deadline = Some(deadline);
            state.expires_at = Some(unix_timestamp_after(self.window_duration));
            state.failed_attempts = 0;
            state.peer = None;
            state.error = None;
            state.generation = state.generation.wrapping_add(1);
            state.session_id = state.session_id.wrapping_add(1);
            (old_control, state.generation, deadline)
        };
        if let Some(control) = old_control {
            let _ = control.send(PairingAction::Cancel).await;
        }
        self.window_signal.send_replace(true);
        if crate::diagnostics::is_collected() {
            crate::diagnostics::record(crate::diagnostics::Record {
                event: crate::diagnostics::Event::PairingWindowOpened,
                peer: None,
                session: None,
                error: None,
            });
        }

        self.schedule_expiration(generation, deadline);
        self.status().await
    }

    fn schedule_expiration(self: &Arc<Self>, generation: u64, deadline: Instant) {
        let manager = Arc::downgrade(self);
        tokio::spawn(async move {
            tokio::time::sleep_until(deadline).await;
            if let Some(manager) = manager.upgrade() {
                manager.expire(generation).await;
            }
        });
    }

    pub async fn status(&self) -> PairingStatus {
        let state = self.state.lock().await;
        PairingStatus {
            pairing_enabled: state.enabled,
            phase: state.phase,
            expires_at: state.expires_at,
            remaining_seconds: state
                .deadline
                .map(|deadline| deadline.saturating_duration_since(Instant::now()).as_secs())
                .unwrap_or(0),
            failed_attempts: state.failed_attempts,
            max_failures: self.max_failures,
            peer: state.peer.clone(),
            error: state.error.clone(),
        }
    }

    pub async fn is_enabled(&self) -> bool {
        self.state.lock().await.enabled
    }

    pub fn subscribe_window(&self) -> watch::Receiver<bool> {
        self.window_signal.subscribe()
    }

    pub async fn begin_handshake(&self) -> Result<(), PairingError> {
        let mut state = self.state.lock().await;
        if !state.enabled {
            return Err(PairingError::WindowClosed);
        }
        if state.control.is_some() {
            return Err(PairingError::AlreadyInProgress);
        }
        state.phase = PairingPhase::Handshaking;
        state.error = None;
        if crate::diagnostics::is_collected() {
            crate::diagnostics::record(crate::diagnostics::Record {
                event: crate::diagnostics::Event::PairingHandshakeStarted,
                peer: None,
                session: None,
                error: None,
            });
        }
        Ok(())
    }

    #[doc(hidden)]
    pub async fn install_session(
        self: &Arc<Self>,
        pending: PendingPairing,
    ) -> Result<(), PairingError> {
        if pending.remote_public_key == self.identity.public_key() {
            return Err(PairingError::SelfPairing);
        }
        if !matches!(pending.interface.as_str(), "lan" | "iroh" | "tailscale") {
            return Err(PairingError::InvalidInterface);
        }
        let verification_code = derive_verification_code(
            &pending.handshake_hash,
            self.identity.public_key(),
            &pending.remote_public_key,
        )
        .map_err(PairingError::Verification)?;
        let fingerprint = crate::identity::fingerprint(&pending.remote_public_key);
        let (control, receiver) = mpsc::channel(4);
        let session_id = {
            let mut state = self.state.lock().await;
            if !state.enabled {
                return Err(PairingError::WindowClosed);
            }
            if state.control.is_some() {
                return Err(PairingError::AlreadyInProgress);
            }
            state.session_id = state.session_id.wrapping_add(1);
            state.phase = PairingPhase::Verification;
            state.peer = Some(PairingPeerStatus {
                hostname: pending.hostname.clone(),
                address: pending.address.clone(),
                fingerprint,
                verification_code,
                local_confirmed: false,
                remote_confirmed: false,
            });
            state.error = None;
            state.control = Some(control);
            state.session_id
        };

        let manager = self.clone();
        tokio::spawn(async move {
            manager.run_session(session_id, pending, receiver).await;
        });
        Ok(())
    }

    pub async fn confirm(&self) -> Result<PairingStatus, PairingError> {
        let control = {
            let state = self.state.lock().await;
            if !matches!(
                state.phase,
                PairingPhase::Verification | PairingPhase::WaitingForPeer
            ) {
                return Err(PairingError::NoVerification);
            }
            state.control.clone().ok_or(PairingError::SessionInactive)?
        };
        control
            .send(PairingAction::Confirm)
            .await
            .map_err(|_| PairingError::SessionInactive)?;
        Ok(self.status().await)
    }

    pub async fn cancel(&self) -> PairingStatus {
        let control = {
            let mut state = self.state.lock().await;
            let control = state.control.take();
            state.enabled = false;
            state.phase = PairingPhase::Cancelled;
            state.deadline = None;
            state.expires_at = None;
            state.error = Some("Pairing was cancelled".to_string());
            state.generation = state.generation.wrapping_add(1);
            state.session_id = state.session_id.wrapping_add(1);
            control
        };
        self.window_signal.send_replace(false);
        if crate::diagnostics::is_collected() {
            crate::diagnostics::record(crate::diagnostics::Record {
                event: crate::diagnostics::Event::PairingWindowClosed,
                peer: None,
                session: None,
                error: None,
            });
        }
        if let Some(control) = control {
            let _ = control.send(PairingAction::Cancel).await;
        }
        self.status().await
    }

    pub async fn record_failure(&self, error: impl Into<String>) {
        let mut close_window = false;
        {
            let mut state = self.state.lock().await;
            if !state.enabled {
                return;
            }
            state.failed_attempts = state.failed_attempts.saturating_add(1);
            state.peer = None;
            state.control = None;
            let message = error.into();
            state.error = Some(message.clone());
            if crate::diagnostics::is_collected() {
                crate::diagnostics::record(crate::diagnostics::Record {
                    event: crate::diagnostics::Event::PairingFailed,
                    peer: None,
                    session: None,
                    error: crate::diagnostics::error_ref("PairingError::record_failure", &message),
                });
            }
            if state.failed_attempts >= self.max_failures {
                state.enabled = false;
                state.phase = PairingPhase::Locked;
                state.deadline = None;
                state.expires_at = None;
                state.generation = state.generation.wrapping_add(1);
                close_window = true;
            } else {
                state.phase = PairingPhase::Waiting;
            }
        }
        if close_window {
            self.window_signal.send_replace(false);
        }
    }

    pub(super) async fn expire(self: &Arc<Self>, generation: u64) {
        let (control, close_window, next_generation) = {
            let mut state = self.state.lock().await;
            if !state.enabled || state.generation != generation {
                return;
            }
            let control = state.control.take();
            if control.is_some() {
                state.failed_attempts = state.failed_attempts.saturating_add(1);
                state.peer = None;
                state.error = Some("Pairing session timed out".to_string());
                if state.failed_attempts >= self.max_failures {
                    state.enabled = false;
                    state.phase = PairingPhase::Locked;
                    state.deadline = None;
                    state.expires_at = None;
                    state.generation = state.generation.wrapping_add(1);
                    state.session_id = state.session_id.wrapping_add(1);
                    (control, true, None)
                } else {
                    state.phase = PairingPhase::Waiting;
                    let deadline = Instant::now() + self.window_duration;
                    state.deadline = Some(deadline);
                    state.expires_at = Some(unix_timestamp_after(self.window_duration));
                    state.generation = state.generation.wrapping_add(1);
                    state.session_id = state.session_id.wrapping_add(1);
                    (control, false, Some((state.generation, deadline)))
                }
            } else {
                state.enabled = false;
                state.phase = PairingPhase::TimedOut;
                state.deadline = None;
                state.expires_at = None;
                state.error = Some("Pairing window timed out".to_string());
                state.generation = state.generation.wrapping_add(1);
                state.session_id = state.session_id.wrapping_add(1);
                (control, true, None)
            }
        };
        if close_window {
            self.window_signal.send_replace(false);
        }
        if let Some(control) = control {
            let _ = control.send(PairingAction::Cancel).await;
        }
        if let Some((generation, deadline)) = next_generation {
            self.schedule_expiration(generation, deadline);
        }
    }

    async fn run_session(
        self: Arc<Self>,
        session_id: u64,
        pending: PendingPairing,
        mut receiver: mpsc::Receiver<PairingAction>,
    ) {
        let PendingPairing {
            mut connection,
            hostname,
            remote_public_key,
            address,
            interface,
            ..
        } = pending;
        let deadline = {
            let state = self.state.lock().await;
            state.deadline.unwrap_or_else(Instant::now)
        };
        let timeout = tokio::time::sleep_until(deadline);
        tokio::pin!(timeout);
        let mut local_confirmed = false;
        let mut remote_confirmed = false;
        let mut local_persisted = false;
        let mut remote_persisted = false;

        loop {
            tokio::select! {
                action = receiver.recv() => match action {
                    Some(PairingAction::Confirm) if !local_confirmed => {
                        let Ok(frame) = Frame::try_new(Command::PairingConfirm, 0, 0, Vec::new()) else {
                            self.fail_session(session_id, "Could not construct pairing confirmation".to_string()).await;
                            return;
                        };
                        if let Err(error) = connection.write_frame(&frame).await {
                            self.fail_session(session_id, format!("Could not confirm pairing: {error}")).await;
                            return;
                        }
                        local_confirmed = true;
                        self.set_confirmation(session_id, true, remote_confirmed).await;
                    }
                    Some(PairingAction::Confirm) => {}
                    Some(PairingAction::Cancel) => {
                        if let Ok(frame) = Frame::try_new(Command::PairingCancel, 0, 0, Vec::new()) {
                            let _ = connection.write_frame(&frame).await;
                        }
                        return;
                    }
                    None => return,
                },
                result = connection.read_frame() => match result {
                    Ok(frame) if frame.command == Command::PairingConfirm => {
                        remote_confirmed = true;
                        self.set_confirmation(session_id, local_confirmed, true).await;
                    }
                    Ok(frame) if frame.command == Command::PairingPersisted => {
                        if !remote_confirmed {
                            self.fail_session(
                                session_id,
                                "Received pairing completion before confirmation".to_string(),
                            )
                            .await;
                            return;
                        }
                        remote_persisted = true;
                    }
                    Ok(frame) if frame.command == Command::PairingCancel => {
                        self.fail_session(session_id, "The other device cancelled pairing".to_string()).await;
                        return;
                    }
                    Ok(frame) if frame.command == Command::PeerError => {
                        self.fail_session(
                            session_id,
                            String::from_utf8_lossy(&frame.payload).to_string(),
                        ).await;
                        return;
                    }
                    Ok(_) => {
                        self.fail_session(session_id, "Unexpected message during pairing".to_string()).await;
                        return;
                    }
                    Err(error) => {
                        self.fail_session(session_id, format!("Pairing connection failed: {error}")).await;
                        return;
                    }
                },
                _ = &mut timeout => {
                    self.expire_current_session(session_id).await;
                    return;
                }
            }

            if local_confirmed && remote_confirmed && !local_persisted {
                if let Err(error) = self
                    .persist_pairing(
                        session_id,
                        &hostname,
                        &remote_public_key,
                        &interface,
                        &address,
                    )
                    .await
                {
                    self.fail_session(session_id, error.to_string()).await;
                    return;
                }
                let Ok(frame) = Frame::try_new(Command::PairingPersisted, 0, 0, Vec::new()) else {
                    self.fail_session(
                        session_id,
                        "Could not construct pairing completion".to_string(),
                    )
                    .await;
                    return;
                };
                if let Err(error) = connection.write_frame(&frame).await {
                    self.fail_session(
                        session_id,
                        format!("Could not confirm saved pairing: {error}"),
                    )
                    .await;
                    return;
                }
                local_persisted = true;
            }

            if local_persisted && remote_persisted {
                self.set_finalizing(session_id).await;
                match tokio::time::timeout(PAIRING_FINALIZE_TIMEOUT, connection.shutdown()).await {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => {
                        log::warn!(
                            "Pairing transport close failed after both peers persisted: {error}"
                        );
                    }
                    Err(_) => {
                        log::warn!("Pairing transport close timed out after both peers persisted");
                    }
                }
                // Both trust records are already durable and both peers have
                // observed the other's PairingPersisted frame. A close error
                // here cannot undo the completed pairing and must not be
                // reported as a pairing failure.
                if let Err(error) = self.finish_success(session_id, &hostname).await {
                    self.fail_session(session_id, error.to_string()).await;
                }
                return;
            }
        }
    }

    async fn set_finalizing(&self, session_id: u64) {
        let mut state = self.state.lock().await;
        if state.session_id == session_id {
            state.phase = PairingPhase::Finalizing;
        }
    }

    async fn set_confirmation(&self, session_id: u64, local: bool, remote: bool) {
        let mut state = self.state.lock().await;
        if state.session_id != session_id {
            return;
        }
        if let Some(peer) = &mut state.peer {
            peer.local_confirmed = local;
            peer.remote_confirmed = remote;
        }
        state.phase = if local && !remote {
            PairingPhase::WaitingForPeer
        } else {
            PairingPhase::Verification
        };
    }

    async fn persist_pairing(
        &self,
        session_id: u64,
        hostname: &str,
        remote_public_key: &[u8],
        interface: &str,
        address: &str,
    ) -> Result<(), PairingError> {
        {
            let state = self.state.lock().await;
            if !state.enabled || state.session_id != session_id {
                return Err(PairingError::SessionClosed);
            }
        }
        let public_key = {
            use base64::{engine::general_purpose::STANDARD, Engine as _};
            STANDARD.encode(remote_public_key)
        };
        let mut settings = self.settings.lock().await;
        let result = if self.persist_trust {
            settings.trust_peer(hostname, &public_key, interface, Some(address))
        } else {
            settings.trust_peer_without_save(hostname, &public_key, interface, Some(address))
        };
        result.map_err(|error| {
            PairingError::Transport(format!("Could not save paired device: {error}"))
        })?;
        Ok(())
    }

    async fn finish_success(&self, session_id: u64, hostname: &str) -> Result<(), PairingError> {
        let mut state = self.state.lock().await;
        if state.session_id != session_id {
            return Ok(());
        }
        state.enabled = false;
        state.phase = PairingPhase::Paired;
        state.deadline = None;
        state.expires_at = None;
        state.error = None;
        state.control = None;
        state.generation = state.generation.wrapping_add(1);
        self.window_signal.send_replace(false);
        if crate::diagnostics::is_collected() {
            crate::diagnostics::record(crate::diagnostics::Record {
                event: crate::diagnostics::Event::PairingConfirmed,
                peer: Some(hostname.to_string()),
                session: None,
                error: None,
            });
        }
        Ok(())
    }

    async fn fail_session(&self, session_id: u64, error: String) {
        {
            let state = self.state.lock().await;
            if state.session_id != session_id {
                return;
            }
        }
        self.record_failure(error).await;
    }

    async fn expire_current_session(self: &Arc<Self>, session_id: u64) {
        let generation = {
            let state = self.state.lock().await;
            if state.session_id != session_id {
                return;
            }
            state.generation
        };
        self.expire(generation).await;
    }
}

fn unix_timestamp_after(duration: Duration) -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .saturating_add(duration)
        .as_secs()
}
