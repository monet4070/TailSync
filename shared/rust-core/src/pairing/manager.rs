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
        Self::with_store(
            settings,
            identity,
            window_duration,
            max_failures,
            persist_trust,
            None,
        )
    }

    /// A manager whose pending-pairing store is the file at `path`. A restart is
    /// then testable by building a second manager over the same file, which is
    /// the only way to tell "the note survived" from "the note was in memory".
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub(crate) fn with_pending_store_at(
        settings: Arc<Mutex<Settings>>,
        identity: Arc<DeviceIdentity>,
        path: std::path::PathBuf,
    ) -> Arc<Self> {
        Self::with_store(
            settings,
            identity,
            DEFAULT_PAIRING_WINDOW,
            DEFAULT_MAX_FAILURES,
            false,
            Some(path),
        )
    }

    fn with_store(
        settings: Arc<Mutex<Settings>>,
        identity: Arc<DeviceIdentity>,
        window_duration: Duration,
        max_failures: u8,
        persist_trust: bool,
        store_path: Option<std::path::PathBuf>,
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
                session_direction: None,
            }),
            settings,
            identity,
            window_signal,
            window_duration,
            max_failures,
            persist_trust,
            pending: Mutex::new(match store_path {
                Some(path) => PendingTrustStore::load_from_path(&path),
                None if persist_trust => PendingTrustStore::load(),
                None => PendingTrustStore::in_memory(),
            }),
            #[cfg(test)]
            promotion_gate: Mutex::new(None),
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
            state.session_direction = None;
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
        mut pending: PendingPairing,
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
        let install = {
            let mut state = self.state.lock().await;
            let old_control = if !state.enabled {
                return Err(PairingError::WindowClosed);
            } else if let Some(existing_direction) = state.session_direction {
                if state.control.is_none() {
                    state.session_direction = None;
                    None
                } else if state.peer.as_ref().is_some_and(|peer| peer.local_confirmed) {
                    // Once the user confirmed this code, neither a same-
                    // direction request nor glare arbitration may replace it.
                    return Err(PairingError::AlreadyInProgress);
                } else if existing_direction == pending.direction {
                    state.control.take()
                } else if pending.direction == self.preferred_direction(&pending.remote_public_key)
                {
                    // Both peers may open an outbound and inbound session at
                    // the same time. Keep the globally deterministic side:
                    // the lexicographically smaller identity keeps outbound,
                    // the larger identity keeps inbound.
                    state.control.take()
                } else {
                    return Err(PairingError::GlareSuperseded);
                }
            } else if state.control.is_some() {
                // Older callers/tests may have installed a control channel
                // without direction metadata. Preserve the single-session
                // invariant rather than guessing at arbitration state.
                return Err(PairingError::AlreadyInProgress);
            } else {
                None
            };
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
            state.session_direction = Some(pending.direction);
            Ok((old_control, state.session_id))
        };
        let (old_control, session_id) = match install {
            Ok((old_control, session_id)) => (old_control, session_id),
            Err(error) => {
                let _ = pending.connection.shutdown().await;
                return Err(error);
            }
        };

        if let Some(old_control) = old_control {
            let _ = old_control.send(PairingAction::Cancel).await;
        }

        let manager = self.clone();
        tokio::spawn(async move {
            manager.run_session(session_id, pending, receiver).await;
        });
        Ok(())
    }

    fn preferred_direction(&self, remote_public_key: &[u8]) -> PairingDirection {
        if self.identity.public_key() < remote_public_key {
            PairingDirection::Outbound
        } else {
            PairingDirection::Inbound
        }
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
            state.session_direction = None;
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
        self.record_failure_for_session(None, error.into()).await;
    }

    async fn record_failure_for_session(&self, session_id: Option<u64>, error: String) {
        let mut close_window = false;
        {
            let mut state = self.state.lock().await;
            if !state.enabled || session_id.is_some_and(|id| state.session_id != id) {
                return;
            }
            state.failed_attempts = state.failed_attempts.saturating_add(1);
            state.peer = None;
            state.control = None;
            state.session_direction = None;
            let message = error;
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

    /// Record a recoverable pairing failure without consuming the lockout
    /// budget. Network unavailability, local cancellation, and a competing
    /// connection are not evidence of an invalid credential.
    pub async fn record_non_ban_failure(&self, error: impl Into<String>) {
        self.record_non_ban_failure_for_session(None, error.into())
            .await;
    }

    async fn record_non_ban_failure_for_session(&self, session_id: Option<u64>, message: String) {
        let mut state = self.state.lock().await;
        if !state.enabled || session_id.is_some_and(|id| state.session_id != id) {
            return;
        }
        state.peer = None;
        state.control = None;
        state.session_direction = None;
        state.phase = PairingPhase::Waiting;
        state.error = Some(message.clone());
        state.session_id = state.session_id.wrapping_add(1);
        if crate::diagnostics::is_collected() {
            crate::diagnostics::record(crate::diagnostics::Record {
                event: crate::diagnostics::Event::PairingFailed,
                peer: None,
                session: None,
                error: crate::diagnostics::error_ref(
                    "PairingError::record_non_ban_failure",
                    &message,
                ),
            });
        }
    }

    pub(super) async fn expire(self: &Arc<Self>, generation: u64) {
        let (control, close_window) = {
            let mut state = self.state.lock().await;
            if !state.enabled || state.generation != generation {
                return;
            }
            let control = state.control.take();
            state.enabled = false;
            state.phase = PairingPhase::TimedOut;
            state.deadline = None;
            state.expires_at = None;
            state.peer = None;
            state.error = Some(
                if control.is_some() {
                    "Pairing session timed out"
                } else {
                    "Pairing window timed out"
                }
                .to_string(),
            );
            state.generation = state.generation.wrapping_add(1);
            state.session_id = state.session_id.wrapping_add(1);
            state.session_direction = None;
            (control, true)
        };
        if close_window {
            self.window_signal.send_replace(false);
        }
        if let Some(control) = control {
            let _ = control.send(PairingAction::Cancel).await;
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
            mut remote_invite,
            ..
        } = pending;
        let deadline = {
            let state = self.state.lock().await;
            state.deadline.unwrap_or_else(Instant::now)
        };
        // The user-controlled pairing window may stay open for two minutes,
        // but an unauthenticated connection must not occupy the sole
        // verification slot for that entire time. The window expiration task
        // still owns the outer lifetime; this shorter deadline only releases
        // the current session and returns the window to Waiting.
        let session_deadline = Instant::now() + PAIRING_SESSION_TIMEOUT;
        let timeout = tokio::time::sleep_until(deadline.min(session_deadline));
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
                            self.fail_session_non_ban(session_id, "Could not construct pairing confirmation".to_string()).await;
                            return;
                        };
                        if let Err(error) = connection.write_frame(&frame).await {
                            self.fail_session_non_ban(session_id, format!("Could not confirm pairing: {error}")).await;
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
                            self.fail_session_on_protocol_error(
                                session_id,
                                local_confirmed,
                                "Received pairing completion before confirmation".to_string(),
                            )
                            .await;
                            return;
                        }
                        remote_persisted = true;
                    }
                    Ok(frame) if frame.command == Command::PairingCancel => {
                        self.fail_session_non_ban(session_id, "The other device cancelled pairing".to_string()).await;
                        return;
                    }
                    Ok(frame) if frame.command == Command::PeerError => {
                        self.fail_session_non_ban(
                            session_id,
                            String::from_utf8_lossy(&frame.payload).to_string(),
                        ).await;
                        return;
                    }
                    Ok(_) => {
                        self.fail_session_on_protocol_error(
                            session_id,
                            local_confirmed,
                            "Unexpected message during pairing".to_string(),
                        )
                        .await;
                        return;
                    }
                    Err(error) => {
                        self.fail_session_non_ban(session_id, format!("Pairing connection failed: {error}")).await;
                        return;
                    }
                },
                _ = &mut timeout => {
                    self.expire_current_session(session_id).await;
                    return;
                }
            }

            if local_confirmed && remote_confirmed && !local_persisted {
                // Both users confirmed, but the peer has not been observed to
                // have persisted its own half. Write a *pending* note instead of
                // trust: if the link dies before the peer's acknowledgement
                // arrives, this side must not be left holding a durable record
                // for a device the other side never accepted (S3-P1-2).
                if let Err(error) = self
                    .record_pending_pairing(
                        session_id,
                        &hostname,
                        &remote_public_key,
                        &interface,
                        &address,
                    )
                    .await
                {
                    self.fail_session_non_ban(session_id, error.to_string())
                        .await;
                    return;
                }
                let Ok(frame) = Frame::try_new(Command::PairingPersisted, 0, 0, Vec::new()) else {
                    self.fail_session_non_ban(
                        session_id,
                        "Could not construct pairing completion".to_string(),
                    )
                    .await;
                    return;
                };
                if let Err(error) = connection.write_frame(&frame).await {
                    self.fail_session_non_ban(
                        session_id,
                        format!("Could not confirm saved pairing: {error}"),
                    )
                    .await;
                    return;
                }
                local_persisted = true;
            }

            if local_persisted && remote_persisted {
                // The peer has now told us it persisted its own half over this
                // authenticated session. Only here does the local record become
                // authoritative, so a one-sided active trust cannot outlive a
                // pairing that the other device never completed.
                if let Err(error) = self
                    .promote_pairing(
                        session_id,
                        &hostname,
                        &remote_public_key,
                        &interface,
                        &address,
                    )
                    .await
                {
                    self.fail_session_non_ban(session_id, error.to_string())
                        .await;
                    return;
                }
                if let Some(claim) = remote_invite.take() {
                    claim.commit();
                }
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
                    self.fail_session_non_ban(session_id, error.to_string())
                        .await;
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

    /// Write the non-authoritative note that this side confirmed the pairing but
    /// has not yet seen the peer persist its own half. Nothing here grants
    /// access: while the note is the only record, `trusted_peer_keys` stays
    /// untouched, so admission, the connection pool, the peer directory and both
    /// UIs keep reporting the device as unpaired.
    async fn record_pending_pairing(
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
        let record = PendingTrustRecord {
            hostname: hostname.to_string(),
            public_key: encode_public_key(remote_public_key),
            interface: interface.to_string(),
            address: address.to_string(),
            recorded_at: unix_now(),
            reconciliations: 0,
        };
        let mut store = self.pending.lock().await;
        store
            .upsert(record)
            .map_err(|error| PairingError::Transport(format!("Could not record pairing: {error}")))
    }

    /// Make the pairing authoritative. Reached only after the peer sent
    /// `PairingPersisted` over the authenticated session, so both devices have
    /// now observed each other's persistence; the local record and the note that
    /// tracked its uncertainty change together.
    ///
    /// The session state is held across the write on purpose. Releasing it
    /// between the check and the write leaves a window in which `cancel()` or
    /// `expire()` can finish — both take only the state lock — and the trust
    /// record would still be written for a session the user had just ended or
    /// that had timed out. Holding the guard makes the check and the write
    /// indivisible; the cost is that a status query waits for one settings
    /// write, which is the same wait the settings lock already imposed.
    async fn promote_pairing(
        &self,
        session_id: u64,
        hostname: &str,
        remote_public_key: &[u8],
        interface: &str,
        address: &str,
    ) -> Result<(), PairingError> {
        let state = self.state.lock().await;
        if !state.enabled || state.session_id != session_id {
            return Err(PairingError::SessionClosed);
        }
        #[cfg(test)]
        if let Some(gate) = self.promotion_gate.lock().await.clone() {
            gate.entered.notify_one();
            gate.release.notified().await;
        }
        let public_key = encode_public_key(remote_public_key);
        let mut settings = self.settings.lock().await;
        let result = if self.persist_trust {
            settings.trust_peer(hostname, &public_key, interface, Some(address))
        } else {
            settings.trust_peer_without_save(hostname, &public_key, interface, Some(address))
        };
        result.map_err(|error| {
            PairingError::Transport(format!("Could not save paired device: {error}"))
        })?;
        drop(settings);
        drop(state);
        // The trust record is durable now; the note has served its purpose.
        // A failure to remove it is not fatal (the record is idempotent and
        // would be cleared by the next successful pairing), but it is worth
        // knowing about.
        let mut store = self.pending.lock().await;
        if let Err(error) = store.remove(hostname) {
            log::warn!("Could not clear the pending pairing record for {hostname}: {error}");
        }
        Ok(())
    }

    /// Pending records are exposed so a caller can tell the user which devices
    /// are half-paired, and so tests can assert that a failed pairing left a
    /// note rather than trust.
    pub async fn pending_trust(&self) -> Vec<PendingTrustRecord> {
        self.pending.lock().await.records().to_vec()
    }

    /// Drop the note for a device the user has forgotten. Otherwise the half
    /// pairing of a device that was explicitly removed would linger on disk
    /// forever with nothing in the UI to explain it.
    pub async fn forget_pending(&self, hostname: &str) {
        let mut store = self.pending.lock().await;
        if let Err(error) = store.remove(hostname) {
            log::warn!("Could not clear the pending pairing record for {hostname}: {error}");
        }
    }

    #[cfg(test)]
    pub(crate) async fn install_promotion_gate(&self, gate: Arc<PromotionGate>) {
        *self.promotion_gate.lock().await = Some(gate);
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
        state.session_direction = None;
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
        self.record_failure_for_session(Some(session_id), error)
            .await;
    }

    async fn fail_session_non_ban(&self, session_id: u64, error: String) {
        self.record_non_ban_failure_for_session(Some(session_id), error)
            .await;
    }

    /// Record a protocol anomaly from the peer. Before the local user confirms
    /// the verification code the peer is still anonymous, so an unexpected
    /// command only releases the slot. After local confirmation the session is
    /// attributed to a deliberate user action and the anomaly counts against
    /// the lockout budget.
    async fn fail_session_on_protocol_error(
        &self,
        session_id: u64,
        local_confirmed: bool,
        error: String,
    ) {
        if local_confirmed {
            self.fail_session(session_id, error).await;
        } else {
            self.fail_session_non_ban(session_id, error).await;
        }
    }

    async fn expire_current_session(self: &Arc<Self>, session_id: u64) {
        let (generation, session_timed_out) = {
            let state = self.state.lock().await;
            if state.session_id != session_id {
                return;
            }
            let window_deadline = state.deadline.unwrap_or_else(Instant::now);
            (
                state.generation,
                Instant::now() < window_deadline && state.control.is_some(),
            )
        };
        if session_timed_out {
            self.expire_session(session_id).await;
        } else {
            self.expire(generation).await;
        }
    }

    pub(super) async fn expire_session(&self, session_id: u64) {
        let (control, close_window) = {
            let mut state = self.state.lock().await;
            if !state.enabled || state.session_id != session_id || state.control.is_none() {
                return;
            }
            let control = state.control.take();
            let message = "Pairing session timed out".to_string();
            state.peer = None;
            state.error = Some(message.clone());
            if crate::diagnostics::is_collected() {
                crate::diagnostics::record(crate::diagnostics::Record {
                    event: crate::diagnostics::Event::PairingFailed,
                    peer: None,
                    session: None,
                    error: crate::diagnostics::error_ref("PairingError::session_timeout", &message),
                });
            }
            state.phase = PairingPhase::Waiting;
            state.session_id = state.session_id.wrapping_add(1);
            state.session_direction = None;
            (control, false)
        };
        if close_window {
            self.window_signal.send_replace(false);
        }
        if let Some(control) = control {
            let _ = control.send(PairingAction::Cancel).await;
        }
    }
}

fn unix_timestamp_after(duration: Duration) -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .saturating_add(duration)
        .as_secs()
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn encode_public_key(public_key: &[u8]) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    STANDARD.encode(public_key)
}
