import Foundation

extension ApiClient {
  func getVersion() async -> UInt64? {
    guard let response = try? await request(["cmd": "get_version"]),
      response["ok"] as? Bool == true,
      let version = (response["data"] as? NSNumber)?.uint64Value
    else { return nil }
    return version
  }

  struct RuntimeSnapshot {
    struct Notification {
      let id: UInt64
      let level: String
      let message: String
    }

    let revision: UInt64
    let historyVersion: UInt64
    let progress: FileProgress?
    let storage: StorageStatus?
    let syncEnabled: Bool
    let status: DaemonStatus
    let notifications: [Notification]
  }

  /// Wait for a daemon-side state change instead of polling each subsystem.
  /// The server keeps the request bounded so reconnects and watchdog checks
  /// remain responsive when the daemon is stopped or upgraded.
  func waitForRuntimeSnapshot(
    since revision: UInt64,
    sinceNotificationId: UInt64? = nil
  ) async -> RuntimeSnapshot? {
    var requestPayload: [String: Any] = [
      "cmd": "wait_runtime_snapshot",
      "since_revision": revision,
      "wait_ms": 2_500,
    ]
    if let sinceNotificationId {
      requestPayload["since_notification_id"] = sinceNotificationId
    }
    guard
      let response = try? await request(
        requestPayload,
        timeoutSeconds: 4
      ),
      response["ok"] as? Bool == true,
      let data = response["data"] as? [String: Any],
      let nextRevision = (data["revision"] as? NSNumber)?.uint64Value,
      let historyVersion = (data["history_version"] as? NSNumber)?.uint64Value,
      let statusData = data["status"] as? [String: Any]
    else { return nil }

    let notifications: [RuntimeSnapshot.Notification] =
      (data["notifications"] as? [[String: Any]] ?? []).compactMap { value in
        guard let id = (value["id"] as? NSNumber)?.uint64Value,
          let message = value["message"] as? String
        else { return nil }
        return RuntimeSnapshot.Notification(
          id: id,
          level: value["level"] as? String ?? "error",
          message: message
        )
      }

    return RuntimeSnapshot(
      revision: nextRevision,
      historyVersion: historyVersion,
      progress: Self.decodeFileProgress(data["progress"]),
      storage: Self.decodeStorageStatus(data["storage"]),
      syncEnabled: data["sync_enabled"] as? Bool ?? true,
      status: Self.decodeDaemonStatus(statusData),
      notifications: notifications
    )
  }

  struct HistoryCapabilities {
    let classifierVersion: Int
    let categories: [String]
    let multipleLabels: Bool
    let dateRangeFilter: Bool
  }

  struct MigrationDiagnostics {
    let unresolvedCount: Int
  }

  func getHistoryCapabilities() async throws -> HistoryCapabilities? {
    let response = try await request(["cmd": "get_history_capabilities"])
    guard response["ok"] as? Bool == true,
      let data = response["data"] as? [String: Any],
      let classifierVersion = (data["classifier_version"] as? NSNumber)?.intValue,
      classifierVersion > 0,
      let categories = data["categories"] as? [String]
    else {
      return nil
    }
    return HistoryCapabilities(
      classifierVersion: classifierVersion,
      categories: categories,
      multipleLabels: data["multiple_labels"] as? Bool ?? false,
      dateRangeFilter: data["date_range_filter"] as? Bool ?? false
    )
  }

  func getMigrationDiagnostics() async throws -> MigrationDiagnostics {
    let response = try await request(["cmd": "get_migration_diagnostics"])
    guard response["ok"] as? Bool == true,
      let data = response["data"] as? [String: Any],
      let unresolvedCount = (data["unresolved_count"] as? NSNumber)?.intValue
    else {
      throw ApiError.serverError(response["error"] as? String ?? "unknown")
    }
    return MigrationDiagnostics(unresolvedCount: max(0, unresolvedCount))
  }

  func ping() async -> Bool {
    guard let response = try? await request(["cmd": "ping"]) else { return false }
    return response["ok"] as? Bool == true
  }

  struct SyncWarning {
    let kind: String
    let peer: String
  }

  func takeSyncWarning() async -> SyncWarning? {
    guard let response = try? await request(["cmd": "get_sync_warning"]),
      response["ok"] as? Bool == true,
      let data = response["data"] as? [String: Any],
      let kind = data["kind"] as? String,
      let peer = data["peer"] as? String
    else { return nil }
    return SyncWarning(kind: kind, peer: peer)
  }

  struct UpdateInfo: Decodable {
    let current_version: String
    let version: String
    let notes: String?
    let published_at: String?
  }

  func checkForUpdate() async throws -> UpdateInfo? {
    // The headless daemon starts its API socket just before Tauri finishes
    // registering the updater AppHandle. Retry only that short startup
    // window; feed/network failures must still surface immediately rather
    // than being hidden behind a long fixed delay.
    for attempt in 0..<8 {
      do {
        let response = try await request(["cmd": "check_for_update"], timeoutSeconds: 30)
        guard response["ok"] as? Bool == true else {
          throw ApiError.serverError(response["error"] as? String ?? "Update check failed")
        }
        guard let value = response["data"], !(value is NSNull) else { return nil }
        return try JSONDecoder().decode(
          UpdateInfo.self,
          from: JSONSerialization.data(withJSONObject: value)
        )
      } catch {
        guard attempt < 7, Self.shouldRetryUpdateCheck(error) else { throw error }
        try await Task.sleep(nanoseconds: 150_000_000)
      }
    }
    return nil
  }

  private static func shouldRetryUpdateCheck(_ error: Error) -> Bool {
    guard let error = error as? ApiError else { return false }
    switch error {
    case .connectionFailed:
      return true
    case .serverError(let message):
      return message.localizedCaseInsensitiveContains("still starting")
    default:
      return false
    }
  }

  func installUpdate() async throws -> Bool {
    let response = try await request(["cmd": "install_update"], timeoutSeconds: 600)
    guard response["ok"] as? Bool == true else {
      throw ApiError.serverError(response["error"] as? String ?? "Update installation failed")
    }
    return ((response["data"] as? [String: Any])?["installed"] as? Bool) ?? false
  }

  func requestShutdown() async -> Bool {
    guard let response = try? await request(["cmd": "quit"]) else { return false }
    return response["ok"] as? Bool == true
  }

  struct DaemonStatus {
    let alive: Bool
    let tcpServerHealthy: Bool
    let clipboardMonitorHealthy: Bool
    let activeInterfaces: Set<String>
  }

  func getStatus() async -> DaemonStatus {
    guard let response = try? await request(["cmd": "get_status"]),
      response["ok"] as? Bool == true,
      let data = response["data"] as? [String: Any]
    else {
      return DaemonStatus(
        alive: false,
        tcpServerHealthy: false,
        clipboardMonitorHealthy: false,
        activeInterfaces: []
      )
    }
    return Self.decodeDaemonStatus(data)
  }

  private static func decodeDaemonStatus(_ data: [String: Any]) -> DaemonStatus {
    let routes = data["active_routes"] as? [String: [String: Any]] ?? [:]
    let interfaces = Set(routes.values.compactMap { $0["interface"] as? String })
    return DaemonStatus(
      alive: true,
      tcpServerHealthy: data["tcp_server_healthy"] as? Bool ?? false,
      clipboardMonitorHealthy: data["clipboard_monitor_healthy"] as? Bool ?? false,
      activeInterfaces: interfaces
    )
  }
}
