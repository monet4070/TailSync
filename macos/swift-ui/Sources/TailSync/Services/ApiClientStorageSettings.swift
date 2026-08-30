import Foundation

extension ApiClient {
  struct StorageStatus {
    let root: String
    let usedBytes: UInt64
    let quotaBytes: UInt64
    let available: Bool
    let error: String?
  }

  struct StorageMigrationResult {
    let newRoot: String
    let oldRoot: String
    let oldSizeBytes: UInt64
  }

  func getStorageStatus() async -> StorageStatus? {
    guard let response = try? await request(["cmd": "get_storage_status"]),
      response["ok"] as? Bool == true,
      let data = response["data"] as? [String: Any]
    else { return nil }
    return Self.decodeStorageStatus(data)
  }

  static func decodeStorageStatus(_ value: Any?) -> StorageStatus? {
    guard let data = value as? [String: Any] else { return nil }
    return StorageStatus(
      root: data["root"] as? String ?? "",
      usedBytes: (data["used_bytes"] as? NSNumber)?.uint64Value ?? 0,
      quotaBytes: (data["quota_bytes"] as? NSNumber)?.uint64Value ?? 0,
      available: data["available"] as? Bool ?? false,
      error: data["error"] as? String
    )
  }

  func changeStorageLocation(parent: String) async throws -> StorageMigrationResult {
    let response = try await request(
      ["cmd": "change_storage_location", "parent": parent], timeoutSeconds: 600)
    guard response["ok"] as? Bool == true,
      let data = response["data"] as? [String: Any]
    else {
      throw ApiError.serverError(response["error"] as? String ?? "unknown")
    }
    return StorageMigrationResult(
      newRoot: data["new_root"] as? String ?? "",
      oldRoot: data["old_root"] as? String ?? "",
      oldSizeBytes: (data["old_size_bytes"] as? NSNumber)?.uint64Value ?? 0
    )
  }

  func deleteOldStorage(path: String) async throws {
    let response = try await request(
      ["cmd": "delete_old_storage", "path": path],
      timeoutSeconds: 600
    )
    guard response["ok"] as? Bool == true else {
      throw ApiError.serverError(response["error"] as? String ?? "unknown")
    }
  }

  func getSettings() async throws -> AppSettings {
    let response = try await request(["cmd": "get_settings"])
    guard response["ok"] as? Bool == true,
      let data = response["data"] as? [String: Any]
    else {
      throw ApiError.serverError(response["error"] as? String ?? "unknown")
    }
    return try JSONDecoder().decode(
      AppSettings.self, from: JSONSerialization.data(withJSONObject: data))
  }

  func updateSettings(_ settings: AppSettings) async throws {
    let object = try jsonDictionary(settings)
    let response = try await request(["cmd": "update_settings", "settings": object])
    guard response["ok"] as? Bool == true else {
      throw ApiError.serverError(response["error"] as? String ?? "unknown")
    }
  }

  func setSyncEnabled(_ enabled: Bool) async -> Bool {
    guard let response = try? await request(["cmd": "set_sync_enabled", "enabled": enabled]) else {
      return false
    }
    return response["ok"] as? Bool == true
  }

  func setSyncShortcut(_ shortcut: String) async -> Bool {
    guard let response = try? await request(["cmd": "set_sync_shortcut", "shortcut": shortcut])
    else {
      return false
    }
    return response["ok"] as? Bool == true
  }

  func setHistoryShortcut(_ shortcut: String) async -> Bool {
    guard let response = try? await request(["cmd": "set_history_shortcut", "shortcut": shortcut])
    else {
      return false
    }
    return response["ok"] as? Bool == true
  }

  func toggleSync() async -> Bool? {
    guard let response = try? await request(["cmd": "toggle_sync"]),
      response["ok"] as? Bool == true,
      let data = response["data"] as? [String: Any],
      let enabled = data["enabled"] as? Bool
    else { return nil }
    return enabled
  }

  func reconnectPeers() async -> Bool {
    guard let response = try? await request(["cmd": "reconnect_peers"]) else { return false }
    return response["ok"] as? Bool == true
  }

  func clearAllHistory() async -> Bool {
    guard let response = try? await request(["cmd": "clear_all"]) else { return false }
    return response["ok"] as? Bool == true
  }
}
