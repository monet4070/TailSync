use super::*;

/// Read the latest peer-health snapshot maintained by the background monitor.
#[command]
pub async fn get_peers(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let settings = state.settings.lock().await.clone();
    let mode = settings.connection_mode.clone();
    let discovery = network::cached_discover_peers(&mode).await;
    Ok(crate::api::peer_snapshot_data(
        &state.identity,
        &settings,
        discovery,
    ))
}

/// Ask the background monitor to execute a health round immediately.
#[command]
pub async fn refresh_peers(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let mode = state.settings.lock().await.connection_mode.clone();
    network::request_peer_refresh(&mode).await?;
    get_peers(state).await
}

/// Measure latency over a peer's selected TailSync route.
#[command]
pub async fn test_connection(address: String) -> Result<serde_json::Value, String> {
    let address = address.trim();
    if address.is_empty() {
        return Err("Missing peer address".to_string());
    }
    match network::test_connection(address).await {
        Ok(route) => {
            network::record_address_test_success(address, route.latency_ms);
            Ok(serde_json::to_value(route).map_err(|error| error.to_string())?)
        }
        Err(error) => {
            network::record_address_test_failure(address);
            Err(error)
        }
    }
}

/// Pin a peer's Noise static public key after out-of-band verification.
#[command]
pub async fn trust_peer(
    state: State<'_, AppState>,
    hostname: String,
    public_key: String,
    address: Option<String>,
) -> Result<String, String> {
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
        crate::identity::TrustPeerFailure::InvalidHostname => "Invalid peer hostname".to_string(),
        crate::identity::TrustPeerFailure::SelfPairing => {
            "Cannot pair this device with itself".to_string()
        }
        crate::identity::TrustPeerFailure::Key(error)
        | crate::identity::TrustPeerFailure::Interface(error)
        | crate::identity::TrustPeerFailure::Trust(error) => error,
    })?;
    state.pool.lock().await.disconnect_hostname(hostname.trim());
    crate::network::clear_protocol_compatibility_error(hostname.trim());
    Ok(fingerprint)
}

/// Remove a pinned peer identity. New connections are rejected immediately.
#[command]
pub async fn forget_peer(state: State<'_, AppState>, hostname: String) -> Result<(), String> {
    let hostname = hostname.trim();
    state
        .settings
        .lock()
        .await
        .forget_peer(hostname)
        .map_err(|error| error.to_string())?;
    state.pool.lock().await.disconnect_hostname(hostname);
    crate::network::clear_protocol_compatibility_error(hostname);
    Ok(())
}

#[command]
pub async fn enable_pairing(
    state: State<'_, AppState>,
) -> Result<crate::pairing::PairingStatus, String> {
    Ok(state.pairing.enable().await)
}

#[command]
pub async fn get_pairing_status(
    state: State<'_, AppState>,
) -> Result<crate::pairing::PairingStatus, String> {
    Ok(state.pairing.status().await)
}

#[command]
pub async fn start_pairing(
    state: State<'_, AppState>,
    address: String,
) -> Result<crate::pairing::PairingStatus, String> {
    network::start_pairing(
        state.pairing.clone(),
        state.identity.clone(),
        state.settings.clone(),
        &address,
    )
    .await?;
    Ok(state.pairing.status().await)
}

#[command]
pub async fn confirm_pairing(
    state: State<'_, AppState>,
) -> Result<crate::pairing::PairingStatus, String> {
    state
        .pairing
        .confirm()
        .await
        .map_err(|error| error.to_string())
}

#[command]
pub async fn cancel_pairing(
    state: State<'_, AppState>,
) -> Result<crate::pairing::PairingStatus, String> {
    Ok(state.pairing.cancel().await)
}

/// Enable or disable a peer device
#[command]
pub async fn toggle_peer(
    state: State<'_, AppState>,
    hostname: String,
    enabled: bool,
) -> Result<(), String> {
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
