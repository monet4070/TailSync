//! Capability-gated pairing commit and authenticated replay. Settings remains
//! the sole trust authority; receipts bind prior user consent to one transaction.
use super::*;
use base64::{engine::general_purpose::STANDARD, Engine as _};

fn nonce_for_session(connection: &SecureConnection) -> [u8; 16] {
    let digest = blake3::derive_key(
        "TailSync pairing commit transaction v1",
        connection.session_id().as_bytes(),
    );
    digest[..16].try_into().expect("fixed-size nonce")
}

fn frame_nonce(frame: &Frame) -> Result<[u8; 16], PairingError> {
    if frame.sequence != 0 || frame.flags.bits() != 0 {
        return Err(PairingError::Transport(
            "Invalid pairing commit frame header".into(),
        ));
    }
    frame
        .payload
        .as_slice()
        .try_into()
        .map_err(|_| PairingError::Transport("Invalid pairing commit nonce".into()))
}

fn active_key(settings: &Settings, hostname: &str, key: &str) -> bool {
    settings
        .trusted_peer_keys
        .get(hostname)
        .is_some_and(|known| known == key)
        && settings
            .enabled_peers
            .get(hostname)
            .copied()
            .unwrap_or(true)
}

/// Send one commit-proof frame. The failure stays typed so a closed transport
/// keeps its structural kind instead of being flattened into `Transport(String)`.
async fn send_nonce(
    connection: &mut SecureConnection,
    command: Command,
    nonce: [u8; 16],
) -> Result<(), PairingFailure> {
    let frame = Frame::try_new(command, 0, 0, nonce.to_vec())
        .map_err(|error| PairingFailure::new(PairingFailureKind::Unknown, error.to_string()))?;
    connection
        .write_frame(&frame)
        .await
        .map_err(PairingFailure::from_boxed)
}

impl PairingManager {
    /// A proof may be replayed only while the authoritative Settings still
    /// contain this exact enabled key. A receipt alone never recreates trust.
    async fn local_commit_receipt(&self, hostname: &str, key: &str) -> Option<PendingTrustRecord> {
        let settings = self.settings.lock().await;
        let store = self.pending.lock().await;
        if !store.is_writable() || !active_key(&settings, hostname, key) {
            return None;
        }
        store
            .get(hostname)
            .filter(|record| record.public_key == key && record.commit_nonce.is_some())
            .cloned()
    }

    async fn has_commit_intent(&self, hostname: &str, key: &str, nonce: [u8; 16]) -> bool {
        let store = self.pending.lock().await;
        store.is_writable()
            && store.get(hostname).is_some_and(|record| {
                record.public_key == key && record.commit_nonce == Some(nonce)
            })
    }

    /// State -> Settings -> receipt is the same order as revoke. The nonce
    /// was saved before this write; a crash between Settings and phase update
    /// leaves a recoverable Pending note, not an unrecorded active commit.
    #[allow(clippy::too_many_arguments)]
    async fn commit_active(
        &self,
        session_id: u64,
        hostname: &str,
        key: &str,
        interface: &str,
        address: &str,
        nonce: [u8; 16],
        recovery: bool,
    ) -> Result<(), PairingError> {
        let mut state = self.state.lock().await;
        if !state.enabled || state.session_id != session_id {
            return Err(PairingError::SessionClosed);
        }
        #[cfg(test)]
        if let Some(gate) = self.promotion_gate.lock().await.clone() {
            gate.entered.notify_one();
            gate.release.notified().await;
        }
        let mut settings = self.settings.lock().await;
        let mut store = self.pending.lock().await;
        let mut record = store
            .get(hostname)
            .filter(|record| record.public_key == key && record.commit_nonce == Some(nonce))
            .cloned()
            .ok_or_else(|| PairingError::Transport("No matching durable pairing intent".into()))?;
        if recovery
            && (settings.enabled_peers.get(hostname) == Some(&false)
                || (record.phase != PendingTrustPhase::Pending
                    && !active_key(&settings, hostname, key)))
        {
            return Err(PairingError::Transport(
                "Pairing recovery trust was revoked or disabled".into(),
            ));
        }
        // Check the receipt store is writable before granting any new trust.
        store.upsert(record.clone()).map_err(|error| {
            PairingError::Transport(format!("Could not record pairing commit: {error}"))
        })?;
        if !recovery || !active_key(&settings, hostname, key) {
            let result = if self.persist_trust {
                settings.trust_peer(hostname, key, interface, Some(address))
            } else {
                settings.trust_peer_without_save(hostname, key, interface, Some(address))
            };
            result.map_err(|error| {
                PairingError::Transport(format!("Could not save paired device: {error}"))
            })?;
        }
        record.phase = PendingTrustPhase::CommittedAwaitingAck;
        store.upsert(record).map_err(|error| {
            PairingError::Transport(format!("Could not record pairing commit: {error}"))
        })?;
        state.phase = PairingPhase::Finalizing;
        if let Some(peer) = state.peer.as_mut() {
            peer.local_confirmed = true;
            peer.remote_confirmed = true;
        }
        Ok(())
    }

    async fn finish_commit(
        &self,
        session_id: u64,
        hostname: &str,
        key: &str,
        nonce: [u8; 16],
    ) -> Result<(), PairingError> {
        let mut state = self.state.lock().await;
        if !state.enabled || state.session_id != session_id {
            return Err(PairingError::SessionClosed);
        }
        let settings = self.settings.lock().await;
        if !active_key(&settings, hostname, key) {
            return Err(PairingError::SessionClosed);
        }
        let mut store = self.pending.lock().await;
        let mut record = store
            .get(hostname)
            .filter(|record| record.public_key == key && record.commit_nonce == Some(nonce))
            .cloned()
            .ok_or_else(|| PairingError::Transport("No matching pairing commit receipt".into()))?;
        record.phase = PendingTrustPhase::Completed;
        store.upsert(record).map_err(|error| {
            PairingError::Transport(format!("Could not record completed pairing: {error}"))
        })?;
        Self::commit_pairing_state(&mut state);
        self.window_signal.send_replace(false);
        if crate::diagnostics::is_collected() {
            crate::diagnostics::record(crate::diagnostics::Record {
                event: crate::diagnostics::Event::PairingConfirmed,
                peer: Some(hostname.into()),
                session: None,
                error: None,
            });
        }
        Ok(())
    }

    pub(super) async fn run_commit_session(
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
        let key = STANDARD.encode(&remote_public_key);
        let fresh_nonce = nonce_for_session(&connection);
        let mut nonce = fresh_nonce;
        let mut local_confirmed = false;
        let mut remote_confirmed = false;
        let mut local_persisted = false;
        let mut remote_persisted = false;
        let mut local_committed = false;
        let mut remote_committed = false;
        let mut acknowledged = false;
        let mut proof_sent = false;
        let deadline = {
            let state = self.state.lock().await;
            if !state.enabled || state.session_id != session_id {
                return;
            }
            self.pending.lock().await.retry_unavailable();
            state.deadline.unwrap_or_else(Instant::now)
        };
        let timeout =
            tokio::time::sleep_until(deadline.min(Instant::now() + PAIRING_SESSION_TIMEOUT));
        tokio::pin!(timeout);
        macro_rules! attempt {
            ($operation:expr) => {
                match $operation {
                    Ok(value) => value,
                    Err(error) => {
                        self.fail_session_non_ban_classified(
                            session_id,
                            PairingFailure::from(error),
                        )
                        .await;
                        return;
                    }
                }
            };
        }

        if let Some(record) = self.local_commit_receipt(&hostname, &key).await {
            nonce = record.commit_nonce.expect("receipt filter checked nonce");
            local_confirmed = true;
            local_persisted = true;
            local_committed = true;
            attempt!(send_nonce(&mut connection, Command::PairingCommitted, nonce).await);
            proof_sent = true;
            let mut state = self.state.lock().await;
            if !state.enabled || state.session_id != session_id {
                return;
            }
            state.phase = PairingPhase::Finalizing;
        }

        loop {
            tokio::select! {
                action = receiver.recv() => match action {
                    Some(PairingAction::Confirm) if !local_confirmed => {
                        let frame = attempt!(Frame::try_new(Command::PairingConfirm, 0, 0, Vec::new()));
                        attempt!(connection.write_frame(&frame).await);
                        local_confirmed = true;
                        self.set_confirmation(session_id, true, remote_confirmed).await;
                    }
                    Some(PairingAction::Confirm) => {}
                    Some(PairingAction::Cancel) => {
                        if let Ok(frame) = Frame::try_new(Command::PairingCancel, 0, 0, Vec::new()) { let _ = connection.write_frame(&frame).await; }
                        return;
                    }
                    None => return,
                },
                result = connection.read_frame() => {
                    let frame = attempt!(result);
                    match frame.command {
                        Command::PairingConfirm => {
                            remote_confirmed = true;
                            self.set_confirmation(session_id, local_confirmed, true).await;
                        }
                        Command::PairingPersisted => {
                            let received = attempt!(frame_nonce(&frame));
                            if !remote_confirmed || received != nonce {
                                self.fail_session_on_protocol_error(session_id, local_confirmed, "Received pairing persistence without matching confirmation".into()).await;
                                return;
                            }
                            remote_persisted = true;
                        }
                        Command::PairingCommitted => {
                            let received = attempt!(frame_nonce(&frame));
                            let recovery = received != fresh_nonce;
                            if !self.has_commit_intent(&hostname, &key, received).await || (!recovery && !remote_persisted) {
                                self.fail_session_on_protocol_error(session_id, local_confirmed, "Received pairing commit without matching durable intent".into()).await;
                                return;
                            }
                            if received != nonce && local_committed {
                                self.fail_session_non_ban_classified(session_id, PairingFailure::new(PairingFailureKind::Unknown, "Pairing commit transaction changed")).await;
                                return;
                            }
                            nonce = received;
                            if !local_committed {
                                attempt!(self.commit_active(session_id, &hostname, &key, &interface, &address, nonce, recovery).await);
                                local_committed = true;
                            }
                            local_confirmed = true; remote_confirmed = true;
                            local_persisted = true; remote_persisted = true;
                            remote_committed = true;
                            // Send our own durable proof before ACK. An ACK is
                            // never accepted as a substitute for peer commit.
                            if !proof_sent { attempt!(send_nonce(&mut connection, Command::PairingCommitted, nonce).await); proof_sent = true; }
                            attempt!(send_nonce(&mut connection, Command::PairingCommittedAck, nonce).await);
                        }
                        Command::PairingCommittedAck => {
                            let received = attempt!(frame_nonce(&frame));
                            if local_committed && received == nonce { acknowledged = true; }
                        }
                        Command::PairingCancel => {
                            self.fail_session_non_ban_classified(session_id, PairingFailure::new(PairingFailureKind::Cancelled, "The other device cancelled pairing")).await;
                            return;
                        }
                        Command::PeerError => {
                            // Peer-supplied bytes: classified only by the handshake
                            // layer's wire-boundary helper.
                            self.fail_session_non_ban_classified(session_id, PairingFailure::from_peer_error(&String::from_utf8_lossy(&frame.payload))).await;
                            return;
                        }
                        _ => { self.fail_session_on_protocol_error(session_id, local_confirmed, "Unexpected message during pairing".into()).await; return; }
                    }
                }
                _ = &mut timeout => { self.expire_current_session(session_id).await; return; }
            }

            if local_confirmed && remote_confirmed && !local_persisted {
                attempt!(
                    self.record_pairing_intent(
                        session_id,
                        &hostname,
                        &remote_public_key,
                        &interface,
                        &address,
                        Some(nonce)
                    )
                    .await
                );
                attempt!(send_nonce(&mut connection, Command::PairingPersisted, nonce).await);
                local_persisted = true;
            }
            // Bootstrap is permitted only after BOTH users and BOTH durable
            // pending proofs. Recovery does not use this local inference.
            if local_persisted && remote_persisted && !local_committed {
                attempt!(
                    self.commit_active(
                        session_id, &hostname, &key, &interface, &address, nonce, false
                    )
                    .await
                );
                local_committed = true;
                attempt!(send_nonce(&mut connection, Command::PairingCommitted, nonce).await);
                proof_sent = true;
            }
            if local_committed && remote_committed && acknowledged {
                attempt!(self.finish_commit(session_id, &hostname, &key, nonce).await);
                if let Some(claim) = remote_invite.take() {
                    claim.commit();
                }
                match tokio::time::timeout(PAIRING_FINALIZE_TIMEOUT, connection.shutdown()).await {
                    Ok(Ok(())) => {}
                    result => {
                        log::warn!("Pairing transport cleanup after committed proof: {result:?}")
                    }
                }
                return;
            }
        }
    }
}
