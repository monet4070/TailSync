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

#[command]
pub async fn create_remote_pairing_invite(
    state: State<'_, AppState>,
) -> Result<RemotePairingInviteResponse, String> {
    let invite = network::create_remote_pairing_invite(
        state.pairing.clone(),
        state.settings.clone(),
        state.remote_invites.clone(),
    )
    .await?;
    Ok(RemotePairingInviteResponse {
        link: invite.as_link(),
        expires_at: invite.expires_at(),
        remaining_seconds: invite.remaining_seconds(),
    })
}

#[command]
pub fn inspect_remote_pairing_link(link: String) -> Result<RemotePairingInvitePreview, String> {
    let invite = RemotePairingInvite::parse(&link).map_err(|error| error.to_string())?;
    Ok(RemotePairingInvitePreview {
        endpoint_id: invite.endpoint_id_string(),
        expires_at: invite.expires_at(),
        remaining_seconds: invite.remaining_seconds(),
    })
}

#[command]
pub async fn start_remote_pairing(
    state: State<'_, AppState>,
    link: String,
) -> Result<crate::pairing::PairingStatus, String> {
    network::start_remote_pairing(
        state.pairing.clone(),
        state.identity.clone(),
        state.settings.clone(),
        &link,
    )
    .await?;
    Ok(state.pairing.status().await)
}

#[command]
pub fn get_remote_pairing_invite_status(
    state: State<'_, AppState>,
) -> Result<RemoteInviteStatus, String> {
    Ok(state.remote_invites.status())
}

#[command]
pub async fn cancel_remote_pairing_invite(
    state: State<'_, AppState>,
) -> Result<crate::pairing::PairingStatus, String> {
    state.remote_invites.cancel();
    Ok(state.pairing.cancel().await)
}

#[command]
pub fn take_pending_remote_pairing_link(
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
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
        if let Err(error) = crate::commands::open_settings_window(handle).await {
            log::debug!("Could not open settings for a remote pairing invite: {error}");
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
}
