use super::CommandError;
use crate::network;
use crate::pairing::{RemoteInviteStatus, RemotePairingInvite};
use crate::AppState;
use serde::Serialize;
use tauri::{command, State};
#[cfg(target_os = "windows")]
use tauri::{AppHandle, Emitter, Manager};

#[cfg(target_os = "windows")]
pub(crate) const REMOTE_PAIRING_LINK_EVENT: &str = "remote-pairing-link-received";

#[cfg(any(target_os = "windows", test))]
pub(crate) fn validated_remote_pairing_link(raw: &str) -> Result<String, String> {
    let link = raw.trim();
    RemotePairingInvite::parse(link).map_err(|error| error.to_string())?;
    Ok(link.to_string())
}

#[cfg(any(target_os = "windows", test))]
pub(crate) fn remote_pairing_link_from_args(args: &[String]) -> Option<&str> {
    args.iter()
        .map(String::as_str)
        .find(|argument| argument.trim_start().starts_with("tailsync://"))
}

#[derive(Debug, Serialize)]
pub struct RemotePairingInviteResponse {
    pub link: String,
    pub expires_at: u64,
    pub remaining_seconds: u64,
}

#[derive(Debug, Serialize)]
pub struct RemotePairingInvitePreview {
    pub endpoint_id: String,
    pub expires_at: u64,
    pub remaining_seconds: u64,
}

fn parse_invite_for_ipc(link: &str) -> Result<RemotePairingInvite, CommandError> {
    // The invite parser's finite error is classified through the same mapping
    // the pairing entry points use, so an expired invite stays a closed window
    // instead of collapsing into a generic invalid argument.
    RemotePairingInvite::parse(link)
        .map_err(|error| CommandError::from(network::invite_failure(error)))
}

/// The real body of [`create_remote_pairing_invite`]: the command wrapper below
/// calls exactly this function, so the boundary keeps its production
/// classification path.
pub(crate) async fn create_remote_pairing_invite_command(
    pairing: std::sync::Arc<crate::pairing::PairingManager>,
    settings: std::sync::Arc<tokio::sync::Mutex<crate::crypto::Settings>>,
    invites: std::sync::Arc<crate::pairing::RemotePairingInviteManager>,
) -> Result<RemotePairingInviteResponse, CommandError> {
    let invite = network::create_remote_pairing_invite_classified(pairing, settings, invites)
        .await
        .map_err(CommandError::from)?;
    Ok(RemotePairingInviteResponse {
        link: invite.as_link(),
        expires_at: invite.expires_at(),
        remaining_seconds: invite.remaining_seconds(),
    })
}

#[command]
pub async fn create_remote_pairing_invite(
    state: State<'_, AppState>,
) -> Result<RemotePairingInviteResponse, CommandError> {
    create_remote_pairing_invite_command(
        state.pairing.clone(),
        state.settings.clone(),
        state.remote_invites.clone(),
    )
    .await
}

#[command]
pub fn inspect_remote_pairing_link(
    link: String,
) -> Result<RemotePairingInvitePreview, CommandError> {
    let invite = parse_invite_for_ipc(&link)?;
    Ok(RemotePairingInvitePreview {
        endpoint_id: invite.endpoint_id_string(),
        expires_at: invite.expires_at(),
        remaining_seconds: invite.remaining_seconds(),
    })
}

/// The real body of [`start_remote_pairing`]; see
/// [`create_remote_pairing_invite_command`].
pub(crate) async fn start_remote_pairing_command(
    pairing: std::sync::Arc<crate::pairing::PairingManager>,
    identity: std::sync::Arc<crate::identity::DeviceIdentity>,
    settings: std::sync::Arc<tokio::sync::Mutex<crate::crypto::Settings>>,
    link: &str,
) -> Result<crate::pairing::PairingStatus, CommandError> {
    parse_invite_for_ipc(link)?;
    network::start_remote_pairing_classified(pairing.clone(), identity, settings, link)
        .await
        .map_err(CommandError::from)?;
    Ok(pairing.status().await)
}

#[command]
pub async fn start_remote_pairing(
    state: State<'_, AppState>,
    link: String,
) -> Result<crate::pairing::PairingStatus, CommandError> {
    start_remote_pairing_command(
        state.pairing.clone(),
        state.identity.clone(),
        state.settings.clone(),
        &link,
    )
    .await
}

#[command]
pub fn get_remote_pairing_invite_status(
    state: State<'_, AppState>,
) -> Result<RemoteInviteStatus, CommandError> {
    Ok(state.remote_invites.status())
}

#[command]
pub async fn cancel_remote_pairing_invite(
    state: State<'_, AppState>,
) -> Result<crate::pairing::PairingStatus, CommandError> {
    state.remote_invites.cancel();
    Ok(state.pairing.cancel().await)
}

#[command]
pub fn take_pending_remote_pairing_link(
    state: State<'_, AppState>,
) -> Result<Option<String>, CommandError> {
    Ok(state
        .pending_remote_pairing_link
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take())
}

/// Accept only a fully parsed invite. The event payload intentionally carries
/// no URL; the UI fetches the validated pending value through a Tauri command.
#[cfg(target_os = "windows")]
pub(crate) fn queue_remote_pairing_link(app: &AppHandle, raw: &str) -> Result<(), String> {
    let link = validated_remote_pairing_link(raw)?;
    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| "TailSync state is unavailable".to_string())?;
    *state
        .pending_remote_pairing_link
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(link);
    app.emit(REMOTE_PAIRING_LINK_EVENT, ())
        .map_err(|error| error.to_string())?;
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = crate::commands::open_connections_window(handle).await {
            log::debug!("Could not open connections for a remote pairing invite: {error}");
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    const ENDPOINT_ID: &str = "5866666666666666666666666666666666666666666666666666666666666666";

    fn invite_link() -> String {
        Arc::new(crate::pairing::RemotePairingInviteManager::new())
            .create_from_endpoint_id(ENDPOINT_ID, Duration::from_secs(60))
            .unwrap()
            .as_link()
    }

    #[test]
    fn native_inbox_accepts_only_a_complete_pairing_link() {
        let link = invite_link();
        assert_eq!(
            validated_remote_pairing_link(&format!("  {link}  ")),
            Ok(link)
        );
        assert!(validated_remote_pairing_link("tailsync://settings").is_err());
        assert!(validated_remote_pairing_link("https://example.invalid/pair/v1/x").is_err());
    }

    #[test]
    fn repeated_launch_finds_the_protocol_argument_among_regular_cli_values() {
        let link = invite_link();
        let args = vec![
            "TailSync.exe".to_string(),
            "--background".to_string(),
            link.clone(),
        ];
        assert_eq!(remote_pairing_link_from_args(&args), Some(link.as_str()));
        assert_eq!(
            remote_pairing_link_from_args(&[
                "TailSync.exe".to_string(),
                "--background".to_string()
            ]),
            None
        );
    }

    #[test]
    fn malformed_invite_is_an_invalid_argument_at_ipc_boundary() {
        let error = parse_invite_for_ipc("tailsync://settings").unwrap_err();
        assert_eq!(
            error.envelope().code,
            tailsync_runtime::contracts::StableErrorCode::InvalidArgument
        );
    }

    type PairingFixture = (
        std::sync::Arc<crate::pairing::PairingManager>,
        std::sync::Arc<crate::identity::DeviceIdentity>,
        std::sync::Arc<tokio::sync::Mutex<crate::crypto::Settings>>,
    );

    fn pairing_fixture(mode: &str) -> PairingFixture {
        let settings = crate::crypto::Settings {
            connection_mode: mode.to_string(),
            ..crate::crypto::Settings::default()
        };
        let settings = std::sync::Arc::new(tokio::sync::Mutex::new(settings));
        let identity = std::sync::Arc::new(crate::identity::DeviceIdentity::generate_for_test());
        let pairing = crate::pairing::PairingManager::new(settings.clone(), identity.clone());
        (pairing, identity, settings)
    }

    fn expired_invite_link() -> String {
        Arc::new(crate::pairing::RemotePairingInviteManager::new())
            .create_from_endpoint_id(ENDPOINT_ID, Duration::ZERO)
            .unwrap()
            .as_link()
    }

    #[test]
    fn expired_invite_is_a_closed_window_at_ipc_boundary() {
        let error = parse_invite_for_ipc(&expired_invite_link()).unwrap_err();
        let encoded = serde_json::to_value(&error).unwrap();
        assert_eq!(encoded["code"], "pairing_window_closed");
        assert_eq!(encoded["message_key"], "error.pairing_window_closed");
        assert_eq!(encoded["retryable"], false);
    }

    #[tokio::test]
    async fn create_remote_pairing_invite_command_serializes_the_typed_category() {
        // A mode without Iroh rejects the request structurally, before any
        // endpoint or network work happens.
        let (pairing, _identity, settings) = pairing_fixture("lan_only");
        let invites = Arc::new(crate::pairing::RemotePairingInviteManager::new());
        let error = create_remote_pairing_invite_command(pairing, settings, invites)
            .await
            .expect_err("lan_only has no remote invite transport");
        let encoded = serde_json::to_value(&error).unwrap();
        assert_eq!(encoded["code"], "invalid_argument");
        assert_eq!(encoded["message_key"], "error.invalid_argument");
    }

    #[tokio::test]
    async fn start_remote_pairing_command_serializes_a_finite_invite_error() {
        let (pairing, identity, settings) = pairing_fixture("lan_only");
        let error =
            start_remote_pairing_command(pairing, identity, settings, &expired_invite_link())
                .await
                .expect_err("an expired invite must be rejected");
        let encoded = serde_json::to_value(&error).unwrap();
        assert_eq!(encoded["code"], "pairing_window_closed");
        assert_eq!(encoded["message_key"], "error.pairing_window_closed");
    }
}
