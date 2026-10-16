import Foundation

struct AppSettings: Codable, Equatable, Sendable {
    var notifications_enabled: Bool = true
    var progress_bar_enabled: Bool = true
    var sync_enabled: Bool = true
    var sync_shortcut: String = "CommandOrControl+Shift+S"
    var history_shortcut: String = "CommandOrControl+Shift+H"
    var history_limit: Int = 100
    var auto_save_network_images: Bool = false
    var storage_root: String? = nil
    var storage_quota_bytes: UInt64 = 10 * 1024 * 1024 * 1024
    var enabled_peers: [String: Bool] = [:]
    var language: String = "en"
    var connection_mode: String = "auto"
    var trusted_peer_keys: [String: String] = [:]
    var trusted_peer_addresses: [String: [String: String]] = [:]
    var paired_peer_endpoints: [String: String] = [:]

    enum CodingKeys: String, CodingKey {
        case notifications_enabled, progress_bar_enabled, sync_enabled, sync_shortcut, history_shortcut, history_limit
        case auto_save_network_images
        case storage_root, storage_quota_bytes
        case enabled_peers, language, connection_mode
        case trusted_peer_keys, trusted_peer_addresses, paired_peer_endpoints
    }

    init() {}

    init(from decoder: Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        notifications_enabled = try values.decodeIfPresent(Bool.self, forKey: .notifications_enabled) ?? true
        progress_bar_enabled = try values.decodeIfPresent(Bool.self, forKey: .progress_bar_enabled) ?? true
        sync_enabled = try values.decodeIfPresent(Bool.self, forKey: .sync_enabled) ?? true
        sync_shortcut = try values.decodeIfPresent(String.self, forKey: .sync_shortcut) ?? "CommandOrControl+Shift+S"
        history_shortcut = try values.decodeIfPresent(String.self, forKey: .history_shortcut) ?? "CommandOrControl+Shift+H"
        history_limit = try values.decodeIfPresent(Int.self, forKey: .history_limit) ?? 100
        auto_save_network_images = try values.decodeIfPresent(Bool.self, forKey: .auto_save_network_images) ?? false
        storage_root = try values.decodeIfPresent(String.self, forKey: .storage_root)
        storage_quota_bytes = try values.decodeIfPresent(UInt64.self, forKey: .storage_quota_bytes) ?? 10 * 1024 * 1024 * 1024
        enabled_peers = try values.decodeIfPresent([String: Bool].self, forKey: .enabled_peers) ?? [:]
        language = try values.decodeIfPresent(String.self, forKey: .language) ?? "en"
        let mode = try values.decodeIfPresent(String.self, forKey: .connection_mode) ?? "auto"
        switch mode {
        case "manual", "lan": connection_mode = "lan_only"
        case "iroh": connection_mode = "iroh_only"
        case "tailscale": connection_mode = "tailscale_only"
        case "auto", "lan_only", "iroh_only", "tailscale_only": connection_mode = mode
        default: connection_mode = "auto"
        }
        trusted_peer_keys = try values.decodeIfPresent([String: String].self, forKey: .trusted_peer_keys) ?? [:]
        trusted_peer_addresses = try values.decodeIfPresent([String: [String: String]].self, forKey: .trusted_peer_addresses) ?? [:]
        paired_peer_endpoints = try values.decodeIfPresent([String: String].self, forKey: .paired_peer_endpoints) ?? [:]
    }
}

/// One editable settings field change.
///
/// The generic settings command accepts only these fields. Sync state and the
/// global shortcuts have dedicated commands, peers and the storage root belong
/// to their own workflows, so they cannot travel through a patch at all.
/// Optional fields are omitted from the encoded JSON, which means a window only
/// ever sends the values it actually changed.
struct AppSettingsPatch: Encodable, Equatable, Sendable {
    var notifications_enabled: Bool?
    var progress_bar_enabled: Bool?
    var history_limit: Int?
    var storage_quota_bytes: UInt64?
    var language: String?
    var connection_mode: String?
    var auto_save_network_images: Bool?

    init() {}

    /// The patch that turns `current` into `updated`.
    init(changing current: AppSettings, to updated: AppSettings) {
        if current.notifications_enabled != updated.notifications_enabled {
            notifications_enabled = updated.notifications_enabled
        }
        if current.progress_bar_enabled != updated.progress_bar_enabled {
            progress_bar_enabled = updated.progress_bar_enabled
        }
        if current.history_limit != updated.history_limit {
            history_limit = updated.history_limit
        }
        if current.storage_quota_bytes != updated.storage_quota_bytes {
            storage_quota_bytes = updated.storage_quota_bytes
        }
        if current.language != updated.language {
            language = updated.language
        }
        if current.connection_mode != updated.connection_mode {
            connection_mode = updated.connection_mode
        }
        if current.auto_save_network_images != updated.auto_save_network_images {
            auto_save_network_images = updated.auto_save_network_images
        }
    }

    var isEmpty: Bool {
        notifications_enabled == nil
            && progress_bar_enabled == nil
            && history_limit == nil
            && storage_quota_bytes == nil
            && language == nil
            && connection_mode == nil
            && auto_save_network_images == nil
    }
}

/// The field the user edited, captured before entering the save queue.
/// A canonical reply can include another window's changes; those differences
/// must never be inferred as a new intent on the next save.
enum SettingsFieldChange: Sendable {
    case notificationsEnabled(Bool)
    case progressBarEnabled(Bool)
    case historyLimit(Int)
    case storageQuotaBytes(UInt64)
    case language(String)
    case connectionMode(String)
    case autoSaveNetworkImages(Bool)
    case syncEnabled(Bool)

    var patch: AppSettingsPatch {
        var patch = AppSettingsPatch()
        switch self {
        case .notificationsEnabled(let value): patch.notifications_enabled = value
        case .progressBarEnabled(let value): patch.progress_bar_enabled = value
        case .historyLimit(let value): patch.history_limit = value
        case .storageQuotaBytes(let value): patch.storage_quota_bytes = value
        case .language(let value): patch.language = value
        case .connectionMode(let value): patch.connection_mode = value
        case .autoSaveNetworkImages(let value): patch.auto_save_network_images = value
        case .syncEnabled: break // Dedicated command, never a generic patch.
        }
        return patch
    }
}

/// Install canonical values only where the draft has not changed since submit.
/// This also preserves edits such as a slider still being dragged, which have
/// not yet enqueued their own save.
enum SettingsDraftReconciliation {
    static func merge(current: AppSettings, submitted: AppSettings, canonical: AppSettings) -> AppSettings {
        var result = current
        func install<Value: Equatable>(_ key: WritableKeyPath<AppSettings, Value>) {
            if current[keyPath: key] == submitted[keyPath: key] {
                result[keyPath: key] = canonical[keyPath: key]
            }
        }
        install(\.notifications_enabled)
        install(\.progress_bar_enabled)
        install(\.history_limit)
        install(\.auto_save_network_images)
        install(\.storage_quota_bytes)
        install(\.language)
        install(\.connection_mode)
        install(\.sync_enabled)
        install(\.sync_shortcut)
        install(\.history_shortcut)
        install(\.storage_root)
        install(\.enabled_peers)
        install(\.trusted_peer_keys)
        install(\.trusted_peer_addresses)
        install(\.paired_peer_endpoints)
        return result
    }
}
