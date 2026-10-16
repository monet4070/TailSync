use crate::pairing::{InviteError, PairingFailure, PairingFailureKind};

/// Retained `String` entry point. The classified implementation below is the
/// single production path; this wrapper only flattens the kind for callers that
/// still need the historical text. A host crate whose call sites all moved to
/// the typed entry no longer reaches it, so the compatibility surface is allowed
/// to stay unused there instead of being deleted.
#[allow(dead_code)]
pub async fn start_pairing(
    pairing: Arc<PairingManager>,
    identity: Arc<DeviceIdentity>,
    settings: Arc<Mutex<crypto::Settings>>,
    address: &str,
) -> Result<(), String> {
    start_pairing_classified(pairing, identity, settings, address)
        .await
        .map_err(PairingFailure::into_message)
}

/// Typed counterpart of [`start_pairing`]: identical text, order and counting,
/// with the failure kind preserved from its structural source.
pub async fn start_pairing_classified(
    pairing: Arc<PairingManager>,
    identity: Arc<DeviceIdentity>,
    settings: Arc<Mutex<crypto::Settings>>,
    address: &str,
) -> Result<(), PairingFailure> {
    start_pairing_with_connector(pairing, identity, settings, address, |address| async move {
        TcpStream::connect(address)
            .await
            .map_err(PairingFailure::from_io)
    })
    .await
}

/// The direct entry's implementation with the TCP connector injected. The
/// production wrapper above passes `TcpStream::connect`; a test can pass a
/// duplex stream instead so the entry's real failure handling (prefix, kind,
/// recorder, window and lockout-budget effects) runs without a live peer on
/// `TCP_PORT`. No behaviour differs between the two callers.
async fn start_pairing_with_connector<F, Fut, S>(
    pairing: Arc<PairingManager>,
    identity: Arc<DeviceIdentity>,
    settings: Arc<Mutex<crypto::Settings>>,
    address: &str,
    connect: F,
) -> Result<(), PairingFailure>
where
    F: FnOnce(SocketAddr) -> Fut,
    Fut: std::future::Future<Output = Result<S, PairingFailure>>,
    S: tailsync_core::secure::SessionIo + 'static,
{
    let target = tailsync_core::peer::directory::parse_pairing_target(address)
        .map_err(|message| PairingFailure::new(PairingFailureKind::InvalidArgument, message))?;
    let mode = settings.lock().await.connection_mode.clone();
    if let Err(message) = tailsync_core::peer::directory::validate_pairing_target(&target, &mode) {
        let failure = PairingFailure::new(PairingFailureKind::InvalidArgument, message);
        pairing
            .record_non_ban_failure_classified(failure.clone())
            .await;
        return Err(failure);
    }
    pairing
        .begin_handshake()
        .await
        .map_err(PairingFailure::from)?;

    let mut window = pairing.subscribe_window();
    let operation = timeout(HANDSHAKE_TIMEOUT, async move {
        match &target {
            PairingTarget::Tcp(ip) => {
                let stream = connect(SocketAddr::new(*ip, TCP_PORT)).await?;
                let accepted =
                    secure::connect_pairing(stream, &identity, local_peer_identity(&mode))
                        .await
                        .map_err(PairingFailure::from_boxed)?;
                let interface = infer_interface(&ip.to_string()).map_err(|message| {
                    PairingFailure::new(PairingFailureKind::InvalidArgument, message)
                })?;
                Ok::<_, PairingFailure>((accepted, ip.to_string(), interface))
            }
            PairingTarget::Iroh(endpoint_id) => {
                let endpoint = iroh::endpoint().await.map_err(transport_failure)?;
                let stream = endpoint
                    .connect(endpoint_id)
                    .await
                    .map_err(transport_failure)?;
                let accepted =
                    secure::connect_pairing(stream, &identity, local_peer_identity(&mode))
                        .await
                        .map_err(PairingFailure::from_boxed)?;
                iroh_identity_binding(
                    &accepted,
                    endpoint_id,
                    "Peer Iroh endpoint does not match its Noise identity",
                )?;
                iroh::remember_rtt_capability(endpoint_id);
                Ok::<_, PairingFailure>((accepted, endpoint_id.clone(), ConnectionInterface::Iroh))
            }
        }
    });
    tokio::pin!(operation);
    let (accepted, pairing_address, pairing_interface) = loop {
        tokio::select! {
            result = &mut operation => {
                break match result {
                    Ok(Ok(accepted)) => accepted,
                    Ok(Err(failure)) => {
                        let failure = failure.with_context("Pairing handshake failed: ");
                        pairing
                            .record_non_ban_failure_classified(failure.clone())
                            .await;
                        return Err(failure);
                    }
                    Err(_) => {
                        let failure = PairingFailure::new(
                            PairingFailureKind::TimedOut,
                            "Pairing handshake timed out",
                        );
                        pairing
                            .record_non_ban_failure_classified(failure.clone())
                            .await;
                        return Err(failure);
                    }
                };
            }
            changed = window.changed() => {
                if changed.is_err() || !*window.borrow() {
                    return Err(PairingFailure::new(
                        PairingFailureKind::WindowClosed,
                        "Pairing window was closed",
                    ));
                }
            }
        }
    };

    pairing
        .install_session(PendingPairing {
            connection: accepted.connection,
            hostname: accepted.peer_identity.hostname,
            remote_public_key: accepted.remote_public_key,
            handshake_hash: accepted.handshake_hash,
            address: pairing_address,
            interface: pairing_interface.as_str().to_string(),
            remote_invite: None,
            direction: PairingDirection::Outbound,
        })
        .await
        .map_err(PairingFailure::from)
}

/// Create a self-contained, one-time invite. The endpoint is started here so
/// the displayed ID is backed by the live listener even when the server was
/// just switched to an Iroh-capable mode.
///
/// Retained `String` wrapper; see [`start_pairing`].
#[allow(dead_code)]
pub async fn create_remote_pairing_invite(
    pairing: Arc<PairingManager>,
    settings: Arc<Mutex<crypto::Settings>>,
    invites: Arc<RemotePairingInviteManager>,
) -> Result<RemotePairingInvite, String> {
    create_remote_pairing_invite_classified(pairing, settings, invites)
        .await
        .map_err(PairingFailure::into_message)
}

/// Typed counterpart of [`create_remote_pairing_invite`].
pub async fn create_remote_pairing_invite_classified(
    pairing: Arc<PairingManager>,
    settings: Arc<Mutex<crypto::Settings>>,
    invites: Arc<RemotePairingInviteManager>,
) -> Result<RemotePairingInvite, PairingFailure> {
    let mode = settings.lock().await.connection_mode.clone();
    if !mode_allows_iroh(&mode) {
        return Err(PairingFailure::new(
            PairingFailureKind::InvalidArgument,
            "Remote Iroh pairing is unavailable in the selected connection mode",
        ));
    }
    let endpoint = iroh::endpoint().await.map_err(transport_failure)?;
    let invite = invites
        .create_from_endpoint_id(&endpoint.endpoint_id(), DEFAULT_INVITE_TTL)
        .map_err(invite_failure)?;
    pairing.enable().await;
    Ok(invite)
}

/// Consume a remote invite on the new device, then enter the existing Noise
/// XX verification flow. The invite only authorizes reaching this pairing
/// handshake; it never replaces the fingerprint/code confirmation step.
///
/// Retained `String` wrapper; see [`start_pairing`].
#[allow(dead_code)]
pub async fn start_remote_pairing(
    pairing: Arc<PairingManager>,
    identity: Arc<DeviceIdentity>,
    settings: Arc<Mutex<crypto::Settings>>,
    link: &str,
) -> Result<(), String> {
    start_remote_pairing_classified(pairing, identity, settings, link)
        .await
        .map_err(PairingFailure::into_message)
}

/// Typed counterpart of [`start_remote_pairing`].
pub async fn start_remote_pairing_classified(
    pairing: Arc<PairingManager>,
    identity: Arc<DeviceIdentity>,
    settings: Arc<Mutex<crypto::Settings>>,
    link: &str,
) -> Result<(), PairingFailure> {
    let invite = RemotePairingInvite::parse(link).map_err(invite_failure)?;
    let mode = settings.lock().await.connection_mode.clone();
    if !mode_allows_iroh(&mode) {
        return Err(PairingFailure::new(
            PairingFailureKind::InvalidArgument,
            "Remote Iroh pairing is unavailable in the selected connection mode",
        ));
    }
    pairing.enable().await;
    pairing
        .begin_handshake()
        .await
        .map_err(PairingFailure::from)?;

    let mut window = pairing.subscribe_window();
    let operation = async {
        let endpoint = iroh::endpoint().await.map_err(transport_failure)?;
        let stream = timeout(
            REMOTE_INVITE_CONNECT_TIMEOUT,
            endpoint.connect_invite(&invite.endpoint_id_string()),
        )
        .await
        .map_err(|_| {
            PairingFailure::new(
                PairingFailureKind::TimedOut,
                "Remote Iroh invite connection timed out",
            )
        })?
        .map_err(transport_failure)?;
        let accepted = complete_remote_invite(stream, &invite, &identity, &mode).await?;
        Ok::<_, PairingFailure>((accepted, invite.endpoint_id_string()))
    };
    tokio::pin!(operation);
    let (accepted, pairing_address) = loop {
        tokio::select! {
            result = &mut operation => {
                break match result {
                    Ok(accepted) => accepted,
                    Err(failure) => {
                        let failure = failure.with_context("Remote pairing failed: ");
                        pairing
                            .record_non_ban_failure_classified(failure.clone())
                            .await;
                        return Err(failure);
                    }
                };
            }
            changed = window.changed() => {
                if changed.is_err() || !*window.borrow() {
                    return Err(PairingFailure::new(
                        PairingFailureKind::WindowClosed,
                        "Pairing window was closed",
                    ));
                }
            }
        }
    };

    pairing
        .install_session(PendingPairing {
            connection: accepted.connection,
            hostname: accepted.peer_identity.hostname,
            remote_public_key: accepted.remote_public_key,
            handshake_hash: accepted.handshake_hash,
            address: pairing_address,
            interface: ConnectionInterface::Iroh.as_str().to_string(),
            remote_invite: None,
            direction: PairingDirection::Outbound,
        })
        .await
        .map_err(PairingFailure::from)
}

/// Consume the invite preface on an already-connected stream and complete the
/// Noise XX handshake, binding the authenticated peer to the invited Iroh
/// endpoint. Kept apart from endpoint creation so the preface and binding rules
/// run against any real stream: production passes the live Iroh stream.
async fn complete_remote_invite<S>(
    mut stream: S,
    invite: &RemotePairingInvite,
    identity: &DeviceIdentity,
    mode: &str,
) -> Result<secure::AcceptedConnection, PairingFailure>
where
    S: tailsync_core::secure::SessionIo + 'static,
{
    let hello = invite.hello().encode();
    timeout(REMOTE_INVITE_PREFACE_TIMEOUT, stream.write_all(&hello))
        .await
        .map_err(|_| {
            PairingFailure::new(
                PairingFailureKind::TimedOut,
                "Remote Iroh invite preface timed out",
            )
        })?
        .map_err(PairingFailure::from_io)?;
    timeout(REMOTE_INVITE_PREFACE_TIMEOUT, stream.flush())
        .await
        .map_err(|_| {
            PairingFailure::new(
                PairingFailureKind::TimedOut,
                "Remote Iroh invite preface timed out",
            )
        })?
        .map_err(PairingFailure::from_io)?;
    let mut ack = [0_u8; 1];
    timeout(REMOTE_INVITE_PREFACE_TIMEOUT, stream.read_exact(&mut ack))
        .await
        .map_err(|_| {
            PairingFailure::new(
                PairingFailureKind::TimedOut,
                "Remote Iroh invite acknowledgement timed out",
            )
        })?
        .map_err(PairingFailure::from_io)?;
    if ack[0] != tailsync_core::pairing::invite::INVITE_ACK_ACCEPTED {
        return Err(PairingFailure::new(
            PairingFailureKind::InvalidArgument,
            "The remote pairing invite was rejected or is no longer available",
        ));
    }
    let accepted = secure::connect_pairing(stream, identity, local_peer_identity(mode))
        .await
        .map_err(PairingFailure::from_boxed)?;
    iroh_identity_binding(
        &accepted,
        &invite.endpoint_id_string(),
        "Peer Iroh endpoint does not match the invite",
    )?;
    iroh::remember_rtt_capability(&invite.endpoint_id_string());
    Ok(accepted)
}

/// Bind the authenticated Noise identity to the expected Iroh endpoint. The
/// direct and invite paths share this rule; only the mismatch message differs.
fn iroh_identity_binding(
    accepted: &secure::AcceptedConnection,
    expected: &str,
    mismatch: &str,
) -> Result<(), PairingFailure> {
    let claimed = accepted
        .peer_identity
        .iroh_endpoint_id
        .as_deref()
        .ok_or_else(|| {
            PairingFailure::new(
                PairingFailureKind::IdentityMismatch,
                "Peer did not bind its Noise identity to an Iroh endpoint",
            )
        })?;
    let claimed = tailsync_core::iroh_transport::canonical_endpoint_id(claimed)
        .map_err(|error| PairingFailure::new(PairingFailureKind::InvalidAdvertisement, error))?;
    if claimed != expected {
        return Err(PairingFailure::new(
            PairingFailureKind::IdentityMismatch,
            mismatch,
        ));
    }
    Ok(())
}

/// An opaque error raised by the Iroh layer. The operation is known to be
/// transport-level, but the `String` carries no structure, so it stays in the
/// coarse transport bucket (the pooled adapter treats the same errors the same
/// way by wrapping them in `io::Error::other`).
fn transport_failure(message: String) -> PairingFailure {
    PairingFailure::new(PairingFailureKind::Transport, message)
}

/// Finite [`InviteError`] mapping. `Unavailable`/`Randomness` carry no finer
/// structural source and stay coarse.
///
/// Shared with the platform `inspect`/`preflight` boundaries, which parse an
/// invite directly and must classify it through the same single mapping.
pub(crate) fn invite_failure(error: InviteError) -> PairingFailure {
    let kind = match &error {
        InviteError::InvalidFormat | InviteError::AlreadyClaimed => {
            PairingFailureKind::InvalidArgument
        }
        InviteError::UnsupportedVersion => PairingFailureKind::ProtocolIncompatible,
        InviteError::Expired => PairingFailureKind::WindowClosed,
        InviteError::Unavailable | InviteError::Randomness => PairingFailureKind::Unknown,
    };
    PairingFailure::new(kind, error.to_string())
}
