import Foundation

struct AppSettings: Codable, Equatable {
    var notifications_enabled: Bool = true
    var progress_bar_enabled: Bool = true
    var history_limit: Int = 100
    var enabled_peers: [String: Bool] = [:]
    var theme: String = "system"
    var language: String = "en"
    var connection_mode: String = "auto"
    var trusted_peer_keys: [String: String] = [:]
    var trusted_peer_addresses: [String: [String: String]] = [:]
    var paired_peer_endpoints: [String: String] = [:]

    enum CodingKeys: String, CodingKey {
        case notifications_enabled, progress_bar_enabled, history_limit
        case enabled_peers, theme, language, connection_mode
        case trusted_peer_keys, trusted_peer_addresses, paired_peer_endpoints
    }

    init() {}

    init(from decoder: Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        notifications_enabled = try values.decodeIfPresent(Bool.self, forKey: .notifications_enabled) ?? true
        progress_bar_enabled = try values.decodeIfPresent(Bool.self, forKey: .progress_bar_enabled) ?? true
        history_limit = try values.decodeIfPresent(Int.self, forKey: .history_limit) ?? 100
        enabled_peers = try values.decodeIfPresent([String: Bool].self, forKey: .enabled_peers) ?? [:]
        theme = try values.decodeIfPresent(String.self, forKey: .theme) ?? "system"
        language = try values.decodeIfPresent(String.self, forKey: .language) ?? "en"
        let mode = try values.decodeIfPresent(String.self, forKey: .connection_mode) ?? "auto"
        switch mode {
        case "manual", "lan": connection_mode = "lan_only"
        case "tailscale": connection_mode = "tailscale_only"
        default: connection_mode = mode
        }
        trusted_peer_keys = try values.decodeIfPresent([String: String].self, forKey: .trusted_peer_keys) ?? [:]
        trusted_peer_addresses = try values.decodeIfPresent([String: [String: String]].self, forKey: .trusted_peer_addresses) ?? [:]
        paired_peer_endpoints = try values.decodeIfPresent([String: String].self, forKey: .paired_peer_endpoints) ?? [:]
    }
}
