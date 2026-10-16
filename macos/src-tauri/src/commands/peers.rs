use super::*;

/// Discover online peers using the configured transport.
#[command]
pub async fn get_peers(state: State<'_, AppState>) -> Result<serde_json::Value, CommandError> {
    let settings = state.settings.lock().await.clone();
    let mode = settings.connection_mode.clone();
    let discovery = network::cached_discover_peers(&mode).await;
    Ok(crate::api::peer_snapshot_data(
        &state.identity,
        &settings,
        discovery,
    ))
}

/// Ask the single background health monitor to run an early discovery round.
#[command]
pub async fn refresh_peers(state: State<'_, AppState>) -> Result<serde_json::Value, CommandError> {
    network::request_peer_refresh_and_wait(&state.settings).await?;
    get_peers(state).await
}

/// Pin a peer's Noise static public key after out-of-band verification.
#[command]
pub async fn trust_peer(
    state: State<'_, AppState>,
    hostname: String,
    public_key: String,
    address: Option<String>,
) -> Result<String, CommandError> {
    let fingerprint = crate::identity::trust_peer(
        &state.identity,
        &state.settings,
        &|settings: &crate::crypto::Settings| settings.save().map_err(|error| error.to_string()),
        &hostname,
        &public_key,
        address.as_deref(),
    )
    .await
    .map_err(|failure| match failure {
        crate::identity::TrustPeerFailure::InvalidHostname
        | crate::identity::TrustPeerFailure::SelfPairing => {
            CommandError::code(tailsync_runtime::contracts::StableErrorCode::InvalidArgument)
        }
        crate::identity::TrustPeerFailure::Key(_)
        | crate::identity::TrustPeerFailure::Interface(_)
        | crate::identity::TrustPeerFailure::Trust(_) => {
            CommandError::code(tailsync_runtime::contracts::StableErrorCode::InternalError)
        }
    })?;
    state.pool.lock().await.disconnect_hostname(hostname.trim());
    crate::network::clear_protocol_compatibility_error(hostname.trim());
    Ok(fingerprint)
}

/// Remove a pinned peer identity. New connections are rejected immediately.
#[command]
pub async fn forget_peer(state: State<'_, AppState>, hostname: String) -> Result<(), CommandError> {
    let hostname = hostname.trim();
    let result = state.pairing.revoke_peer(hostname).await;
    state.pool.lock().await.disconnect_hostname(hostname);
    crate::network::clear_protocol_compatibility_error(hostname);
    result.map_err(CommandError::from)
}

#[command]
pub async fn enable_pairing(
    state: State<'_, AppState>,
) -> Result<crate::pairing::PairingStatus, CommandError> {
    Ok(state.pairing.enable().await)
}

#[command]
pub async fn get_pairing_status(
    state: State<'_, AppState>,
) -> Result<crate::pairing::PairingStatus, CommandError> {
    Ok(state.pairing.status().await)
}

/// The real body of [`start_pairing`]: the command wrapper below calls exactly
/// this function, so the boundary keeps its production classification path.
pub(crate) async fn start_pairing_command(
    pairing: std::sync::Arc<crate::pairing::PairingManager>,
    identity: std::sync::Arc<crate::identity::DeviceIdentity>,
    settings: std::sync::Arc<tokio::sync::Mutex<crate::crypto::Settings>>,
    address: &str,
) -> Result<crate::pairing::PairingStatus, CommandError> {
    network::start_pairing_classified(pairing.clone(), identity, settings, address)
        .await
        .map_err(CommandError::from)?;
    Ok(pairing.status().await)
}

#[command]
pub async fn start_pairing(
    state: State<'_, AppState>,
    address: String,
) -> Result<crate::pairing::PairingStatus, CommandError> {
    start_pairing_command(
        state.pairing.clone(),
        state.identity.clone(),
        state.settings.clone(),
        &address,
    )
    .await
}

/// The real body of [`confirm_pairing`]; see [`start_pairing_command`].
pub(crate) async fn confirm_pairing_command(
    pairing: std::sync::Arc<crate::pairing::PairingManager>,
) -> Result<crate::pairing::PairingStatus, CommandError> {
    pairing.confirm().await.map_err(CommandError::from)
}

#[command]
pub async fn confirm_pairing(
    state: State<'_, AppState>,
) -> Result<crate::pairing::PairingStatus, CommandError> {
    confirm_pairing_command(state.pairing.clone()).await
}

#[command]
pub async fn cancel_pairing(
    state: State<'_, AppState>,
) -> Result<crate::pairing::PairingStatus, CommandError> {
    Ok(state.pairing.cancel().await)
}

/// Enable or disable a peer device
#[command]
pub async fn toggle_peer(
    state: State<'_, AppState>,
    hostname: String,
    enabled: bool,
) -> Result<(), CommandError> {
    let mut settings = state.settings.lock().await;
    settings
        .toggle_peer(&hostname, enabled)
        .map_err(|e| e.to_string())?;
    drop(settings);
    if !enabled {
        state.pool.lock().await.disconnect_hostname(&hostname);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    type PairingFixture = (
        std::sync::Arc<crate::pairing::PairingManager>,
        std::sync::Arc<crate::identity::DeviceIdentity>,
        std::sync::Arc<tokio::sync::Mutex<crate::crypto::Settings>>,
    );

    fn pairing_fixture() -> PairingFixture {
        let settings =
            std::sync::Arc::new(tokio::sync::Mutex::new(crate::crypto::Settings::default()));
        let identity = std::sync::Arc::new(crate::identity::DeviceIdentity::generate_for_test());
        let pairing = crate::pairing::PairingManager::new(settings.clone(), identity.clone());
        (pairing, identity, settings)
    }

    #[tokio::test]
    async fn start_pairing_command_serializes_the_typed_category() {
        let (pairing, identity, settings) = pairing_fixture();
        pairing.enable().await;
        // A valid IP outside the selected mode fails structurally, before any
        // socket work, and the manager records the legacy text alongside it.
        let error = start_pairing_command(pairing.clone(), identity, settings, "203.0.113.5")
            .await
            .expect_err("an out-of-mode target must be rejected");
        let encoded = serde_json::to_value(&error).unwrap();
        assert_eq!(encoded["code"], "invalid_argument");
        assert_eq!(encoded["message_key"], "error.invalid_argument");
        assert_eq!(encoded["retryable"], false);
        assert!(!encoded.to_string().contains("203.0.113.5"));

        // The legacy status keeps the original text and its typed kind.
        let status = pairing.status().await;
        assert_eq!(
            status.error.as_deref(),
            Some("Peer address is outside the selected network")
        );
        assert_eq!(
            status.failure_kind,
            Some(crate::pairing::PairingFailureKind::InvalidArgument)
        );
    }

    #[tokio::test]
    async fn confirm_pairing_command_serializes_a_finite_pairing_error() {
        let (pairing, _identity, _settings) = pairing_fixture();
        let error = confirm_pairing_command(pairing.clone())
            .await
            .expect_err("no verification is pending");
        let encoded = serde_json::to_value(&error).unwrap();
        assert_eq!(encoded["code"], "invalid_argument");
        assert_eq!(encoded["message_key"], "error.invalid_argument");
        // No status kind was involved: the manager never recorded a failure.
        assert!(pairing.status().await.failure_kind.is_none());
    }
}
