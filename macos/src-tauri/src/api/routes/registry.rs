// Generated from shared/schema/local-commands.json. Do not edit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HistoryCommand {
    GetHistory,
    GetPreviewData,
    DeleteEntry,
    SetHistoryFavorite,
    DeleteFavoriteEntry,
    ClearHistory,
    ClearAll,
    RestoreEntry,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PeersCommand {
    GetPeers,
    RefreshPeers,
    TogglePeer,
    TrustPeer,
    ForgetPeer,
    TestConnection,
    ReconnectPeers,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SettingsCommand {
    GetSettings,
    GetSyncState,
    SetSyncEnabled,
    ToggleSync,
    SetSyncShortcut,
    SetHistoryShortcut,
    SetConnectionMode,
    UpdateSettings,
    ChangeStorageLocation,
    DeleteOldStorage,
    SetHistoryPinned,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ThemeCommand {
    ListThemesV2,
    GetLocalThemeSettings,
    SetLocalThemeSettings,
    ValidateTheme,
    InstallTheme,
    UpdateTheme,
    RollbackTheme,
    DeleteThemeV2,
    ResolveTheme,
    GetThemeAssetSlot,
    PreviewThemeAssetSlot,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BuiltinCommand {
    Ping,
    GetLocalCapabilities,
    WaitRuntimeSnapshot,
    CheckForUpdate,
    InstallUpdate,
    GetFileProgress,
    CancelFileBatch,
    RestoreFileBatch,
    GetStorageStatus,
    GetVersion,
    GetSyncWarning,
    GetHistoryCapabilities,
    GetMigrationDiagnostics,
    GetStatus,
    EnablePairing,
    GetPairingStatus,
    StartPairing,
    ConfirmPairing,
    CancelPairing,
    CreateRemotePairingInvite,
    InspectRemotePairingLink,
    StartRemotePairing,
    GetRemotePairingInviteStatus,
    CancelRemotePairingInvite,
    GetImageData,
    BeginImport,
    ImportChunk,
    FinishImport,
    MigrateEntry,
    Quit,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LocalCommand {
    History(HistoryCommand),
    Peers(PeersCommand),
    Settings(SettingsCommand),
    Theme(ThemeCommand),
    Builtin(BuiltinCommand),
    BinaryPreview,
}
impl LocalCommand {
    pub(super) fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "get_history" => Self::History(HistoryCommand::GetHistory),
            "get_preview_data" => Self::History(HistoryCommand::GetPreviewData),
            "delete_entry" => Self::History(HistoryCommand::DeleteEntry),
            "set_history_favorite" => Self::History(HistoryCommand::SetHistoryFavorite),
            "delete_favorite_entry" => Self::History(HistoryCommand::DeleteFavoriteEntry),
            "clear_history" => Self::History(HistoryCommand::ClearHistory),
            "clear_all" => Self::History(HistoryCommand::ClearAll),
            "restore_entry" => Self::History(HistoryCommand::RestoreEntry),
            "get_peers" => Self::Peers(PeersCommand::GetPeers),
            "refresh_peers" => Self::Peers(PeersCommand::RefreshPeers),
            "toggle_peer" => Self::Peers(PeersCommand::TogglePeer),
            "trust_peer" => Self::Peers(PeersCommand::TrustPeer),
            "forget_peer" => Self::Peers(PeersCommand::ForgetPeer),
            "test_connection" => Self::Peers(PeersCommand::TestConnection),
            "reconnect_peers" => Self::Peers(PeersCommand::ReconnectPeers),
            "get_settings" => Self::Settings(SettingsCommand::GetSettings),
            "get_sync_state" => Self::Settings(SettingsCommand::GetSyncState),
            "set_sync_enabled" => Self::Settings(SettingsCommand::SetSyncEnabled),
            "toggle_sync" => Self::Settings(SettingsCommand::ToggleSync),
            "set_sync_shortcut" => Self::Settings(SettingsCommand::SetSyncShortcut),
            "set_history_shortcut" => Self::Settings(SettingsCommand::SetHistoryShortcut),
            "set_connection_mode" => Self::Settings(SettingsCommand::SetConnectionMode),
            "update_settings" => Self::Settings(SettingsCommand::UpdateSettings),
            "change_storage_location" => Self::Settings(SettingsCommand::ChangeStorageLocation),
            "delete_old_storage" => Self::Settings(SettingsCommand::DeleteOldStorage),
            "set_history_pinned" => Self::Settings(SettingsCommand::SetHistoryPinned),
            "list_themes_v2" => Self::Theme(ThemeCommand::ListThemesV2),
            "get_local_theme_settings" => Self::Theme(ThemeCommand::GetLocalThemeSettings),
            "set_local_theme_settings" => Self::Theme(ThemeCommand::SetLocalThemeSettings),
            "validate_theme" => Self::Theme(ThemeCommand::ValidateTheme),
            "install_theme" => Self::Theme(ThemeCommand::InstallTheme),
            "update_theme" => Self::Theme(ThemeCommand::UpdateTheme),
            "rollback_theme" => Self::Theme(ThemeCommand::RollbackTheme),
            "delete_theme_v2" => Self::Theme(ThemeCommand::DeleteThemeV2),
            "resolve_theme" => Self::Theme(ThemeCommand::ResolveTheme),
            "get_theme_asset_slot" => Self::Theme(ThemeCommand::GetThemeAssetSlot),
            "preview_theme_asset_slot" => Self::Theme(ThemeCommand::PreviewThemeAssetSlot),
            "ping" => Self::Builtin(BuiltinCommand::Ping),
            "get_local_capabilities" => Self::Builtin(BuiltinCommand::GetLocalCapabilities),
            "wait_runtime_snapshot" => Self::Builtin(BuiltinCommand::WaitRuntimeSnapshot),
            "check_for_update" => Self::Builtin(BuiltinCommand::CheckForUpdate),
            "install_update" => Self::Builtin(BuiltinCommand::InstallUpdate),
            "get_file_progress" => Self::Builtin(BuiltinCommand::GetFileProgress),
            "cancel_file_batch" => Self::Builtin(BuiltinCommand::CancelFileBatch),
            "restore_file_batch" => Self::Builtin(BuiltinCommand::RestoreFileBatch),
            "get_storage_status" => Self::Builtin(BuiltinCommand::GetStorageStatus),
            "get_version" => Self::Builtin(BuiltinCommand::GetVersion),
            "get_sync_warning" => Self::Builtin(BuiltinCommand::GetSyncWarning),
            "get_history_capabilities" => Self::Builtin(BuiltinCommand::GetHistoryCapabilities),
            "get_migration_diagnostics" => Self::Builtin(BuiltinCommand::GetMigrationDiagnostics),
            "get_status" => Self::Builtin(BuiltinCommand::GetStatus),
            "enable_pairing" => Self::Builtin(BuiltinCommand::EnablePairing),
            "get_pairing_status" => Self::Builtin(BuiltinCommand::GetPairingStatus),
            "start_pairing" => Self::Builtin(BuiltinCommand::StartPairing),
            "confirm_pairing" => Self::Builtin(BuiltinCommand::ConfirmPairing),
            "cancel_pairing" => Self::Builtin(BuiltinCommand::CancelPairing),
            "create_remote_pairing_invite" => {
                Self::Builtin(BuiltinCommand::CreateRemotePairingInvite)
            }
            "inspect_remote_pairing_link" => {
                Self::Builtin(BuiltinCommand::InspectRemotePairingLink)
            }
            "start_remote_pairing" => Self::Builtin(BuiltinCommand::StartRemotePairing),
            "get_remote_pairing_invite_status" => {
                Self::Builtin(BuiltinCommand::GetRemotePairingInviteStatus)
            }
            "cancel_remote_pairing_invite" => {
                Self::Builtin(BuiltinCommand::CancelRemotePairingInvite)
            }
            "get_image_data" => Self::Builtin(BuiltinCommand::GetImageData),
            "begin_import" => Self::Builtin(BuiltinCommand::BeginImport),
            "import_chunk" => Self::Builtin(BuiltinCommand::ImportChunk),
            "finish_import" => Self::Builtin(BuiltinCommand::FinishImport),
            "migrate_entry" => Self::Builtin(BuiltinCommand::MigrateEntry),
            "quit" => Self::Builtin(BuiltinCommand::Quit),
            "get_preview_binary" => Self::BinaryPreview,
            _ => return None,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_registered_commands_parse_and_unknown_is_rejected() {
        assert!(LocalCommand::parse("get_history").is_some());
        assert!(LocalCommand::parse("get_preview_data").is_some());
        assert!(LocalCommand::parse("delete_entry").is_some());
        assert!(LocalCommand::parse("set_history_favorite").is_some());
        assert!(LocalCommand::parse("delete_favorite_entry").is_some());
        assert!(LocalCommand::parse("clear_history").is_some());
        assert!(LocalCommand::parse("clear_all").is_some());
        assert!(LocalCommand::parse("restore_entry").is_some());
        assert!(LocalCommand::parse("get_peers").is_some());
        assert!(LocalCommand::parse("refresh_peers").is_some());
        assert!(LocalCommand::parse("toggle_peer").is_some());
        assert!(LocalCommand::parse("trust_peer").is_some());
        assert!(LocalCommand::parse("forget_peer").is_some());
        assert!(LocalCommand::parse("test_connection").is_some());
        assert!(LocalCommand::parse("reconnect_peers").is_some());
        assert!(LocalCommand::parse("get_settings").is_some());
        assert!(LocalCommand::parse("get_sync_state").is_some());
        assert!(LocalCommand::parse("set_sync_enabled").is_some());
        assert!(LocalCommand::parse("toggle_sync").is_some());
        assert!(LocalCommand::parse("set_sync_shortcut").is_some());
        assert!(LocalCommand::parse("set_history_shortcut").is_some());
        assert!(LocalCommand::parse("set_connection_mode").is_some());
        assert!(LocalCommand::parse("update_settings").is_some());
        assert!(LocalCommand::parse("change_storage_location").is_some());
        assert!(LocalCommand::parse("delete_old_storage").is_some());
        assert!(LocalCommand::parse("set_history_pinned").is_some());
        assert!(LocalCommand::parse("list_themes_v2").is_some());
        assert!(LocalCommand::parse("get_local_theme_settings").is_some());
        assert!(LocalCommand::parse("set_local_theme_settings").is_some());
        assert!(LocalCommand::parse("validate_theme").is_some());
        assert!(LocalCommand::parse("install_theme").is_some());
        assert!(LocalCommand::parse("update_theme").is_some());
        assert!(LocalCommand::parse("rollback_theme").is_some());
        assert!(LocalCommand::parse("delete_theme_v2").is_some());
        assert!(LocalCommand::parse("resolve_theme").is_some());
        assert!(LocalCommand::parse("get_theme_asset_slot").is_some());
        assert!(LocalCommand::parse("preview_theme_asset_slot").is_some());
        assert!(LocalCommand::parse("ping").is_some());
        assert!(LocalCommand::parse("get_local_capabilities").is_some());
        assert!(LocalCommand::parse("wait_runtime_snapshot").is_some());
        assert!(LocalCommand::parse("check_for_update").is_some());
        assert!(LocalCommand::parse("install_update").is_some());
        assert!(LocalCommand::parse("get_file_progress").is_some());
        assert!(LocalCommand::parse("cancel_file_batch").is_some());
        assert!(LocalCommand::parse("restore_file_batch").is_some());
        assert!(LocalCommand::parse("get_storage_status").is_some());
        assert!(LocalCommand::parse("get_version").is_some());
        assert!(LocalCommand::parse("get_sync_warning").is_some());
        assert!(LocalCommand::parse("get_history_capabilities").is_some());
        assert!(LocalCommand::parse("get_migration_diagnostics").is_some());
        assert!(LocalCommand::parse("get_status").is_some());
        assert!(LocalCommand::parse("enable_pairing").is_some());
        assert!(LocalCommand::parse("get_pairing_status").is_some());
        assert!(LocalCommand::parse("start_pairing").is_some());
        assert!(LocalCommand::parse("confirm_pairing").is_some());
        assert!(LocalCommand::parse("cancel_pairing").is_some());
        assert!(LocalCommand::parse("create_remote_pairing_invite").is_some());
        assert!(LocalCommand::parse("inspect_remote_pairing_link").is_some());
        assert!(LocalCommand::parse("start_remote_pairing").is_some());
        assert!(LocalCommand::parse("get_remote_pairing_invite_status").is_some());
        assert!(LocalCommand::parse("cancel_remote_pairing_invite").is_some());
        assert!(LocalCommand::parse("get_image_data").is_some());
        assert!(LocalCommand::parse("begin_import").is_some());
        assert!(LocalCommand::parse("import_chunk").is_some());
        assert!(LocalCommand::parse("finish_import").is_some());
        assert!(LocalCommand::parse("migrate_entry").is_some());
        assert!(LocalCommand::parse("quit").is_some());
        assert!(LocalCommand::parse("get_preview_binary").is_some());
        assert!(LocalCommand::parse("unknown_command").is_none());
    }
}
