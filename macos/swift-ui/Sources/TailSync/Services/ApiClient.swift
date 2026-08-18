import Foundation
import Darwin
import Security

enum ThemeResolvedPairDecoder {
    static func decode(
        themeId: String,
        highContrast: Bool,
        lightData: [String: Any],
        darkData: [String: Any]
    ) -> TailSyncThemeDefinition? {
        guard lightData["id"] as? String == themeId,
              darkData["id"] as? String == themeId,
              lightData["mode"] as? String == "light",
              darkData["mode"] as? String == "dark",
              lightData["highContrast"] as? Bool == highContrast,
              darkData["highContrast"] as? Bool == highContrast,
              let lightDigest = lightData["digest"] as? String,
              darkData["digest"] as? String == lightDigest,
              let lightTokens = lightData["tokens"] as? [String: Any],
              let darkTokens = darkData["tokens"] as? [String: Any],
              let lightSlots = assetSlots(lightData),
              let darkSlots = assetSlots(darkData),
              lightSlots == darkSlots else { return nil }
        return TailSyncThemeDefinition.resolvedV2(
            id: themeId,
            packageDigest: lightDigest,
            light: lightTokens,
            dark: darkTokens,
            assetSlots: lightSlots
        )
    }

    private static func assetSlots(
        _ data: [String: Any]
    ) -> [String: TailSyncThemeAssetDescriptor]? {
        let rawSlots = data["assetSlots"] as? [String: Any] ?? [:]
        var slots: [String: TailSyncThemeAssetDescriptor] = [:]
        for (key, raw) in rawSlots {
            guard let value = raw as? [String: Any],
                  let slot = value["slot"] as? String,
                  slot == key,
                  let assetKey = value["key"] as? String,
                  let digest = value["digest"] as? String,
                  let mime = value["mimeType"] as? String,
                  let bytes = (value["bytes"] as? NSNumber)?.intValue,
                  let width = (value["width"] as? NSNumber)?.intValue,
                  let height = (value["height"] as? NSNumber)?.intValue else { return nil }
            slots[key] = TailSyncThemeAssetDescriptor(
                slot: slot,
                key: assetKey,
                digest: digest,
                mimeType: mime,
                bytes: bytes,
                width: width,
                height: height
            )
        }
        return slots
    }
}

final class ApiClient: @unchecked Sendable {
    static let shared = ApiClient()
    private let port: UInt16 = 19889
    let capabilityToken: String

    private init() {
        if let configured = ProcessInfo.processInfo.environment["TAILSYNC_API_TOKEN"],
           Self.isValidCapabilityToken(configured) {
            capabilityToken = configured.lowercased()
            return
        }

        var bytes = [UInt8](repeating: 0, count: 32)
        guard SecRandomCopyBytes(kSecRandomDefault, bytes.count, &bytes) == errSecSuccess else {
            fatalError("TailSync could not generate its local API capability token")
        }
        capabilityToken = bytes.map { String(format: "%02x", $0) }.joined()
    }

    private static func isValidCapabilityToken(_ value: String) -> Bool {
        value.count == 64 && value.unicodeScalars.allSatisfy {
            (48...57).contains($0.value) || (65...70).contains($0.value) || (97...102).contains($0.value)
        }
    }

    private func request(
        _ json: [String: Any],
        timeoutSeconds: Int = 3,
        maxResponseBytes: Int = 4 * 1024 * 1024
    ) async throws -> [String: Any] {
        var authenticated = json
        authenticated["token"] = capabilityToken
        var data = try JSONSerialization.data(withJSONObject: authenticated)
        data.append(0x0A)

        return try await withCheckedThrowingContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                let sock = socket(AF_INET, SOCK_STREAM, 0)
                guard sock >= 0 else {
                    continuation.resume(throwing: ApiError.connectionFailed)
                    return
                }
                defer { close(sock) }

                var address = sockaddr_in()
                address.sin_family = sa_family_t(AF_INET)
                address.sin_port = CFSwapInt16HostToBig(self.port)
                inet_pton(AF_INET, "127.0.0.1", &address.sin_addr)
                var timeout = timeval(tv_sec: timeoutSeconds, tv_usec: 0)
                setsockopt(sock, SOL_SOCKET, SO_SNDTIMEO, &timeout, socklen_t(MemoryLayout<timeval>.size))
                setsockopt(sock, SOL_SOCKET, SO_RCVTIMEO, &timeout, socklen_t(MemoryLayout<timeval>.size))

                let connected = withUnsafePointer(to: &address) {
                    $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                        connect(sock, $0, socklen_t(MemoryLayout<sockaddr_in>.size))
                    }
                }
                guard connected == 0 else {
                    continuation.resume(throwing: ApiError.connectionFailed)
                    return
                }

                var sentTotal = 0
                while sentTotal < data.count {
                    let sent = data.withUnsafeBytes { bytes -> Int in
                        guard let base = bytes.baseAddress else { return -1 }
                        return send(sock, base.advanced(by: sentTotal), data.count - sentTotal, 0)
                    }
                    guard sent > 0 else {
                        continuation.resume(throwing: ApiError.sendFailed)
                        return
                    }
                    sentTotal += sent
                }

                // The daemon uses JSON-lines. A single recv() may contain only
                // part of a response, or several responses, so read until the
                // first newline and cap the buffer against a broken daemon.
                var responseData = Data()
                var buffer = [UInt8](repeating: 0, count: 64 * 1024)
                var newlineIndex: Data.Index?
                while responseData.count < maxResponseBytes {
                    let received = recv(sock, &buffer, buffer.count, 0)
                    guard received > 0 else { break }
                    responseData.append(contentsOf: buffer.prefix(received))
                    if let index = responseData.firstIndex(of: 0x0A) {
                        newlineIndex = index
                        break
                    }
                }
                guard let newlineIndex, newlineIndex > responseData.startIndex else {
                    continuation.resume(throwing: ApiError.noResponse)
                    return
                }
                let line = Data(responseData[..<newlineIndex])
                guard let response = try? JSONSerialization.jsonObject(with: line) as? [String: Any] else {
                    continuation.resume(throwing: ApiError.invalidJson)
                    return
                }
                continuation.resume(returning: response)
            }
        }
    }

    func getVersion() async -> UInt64? {
        guard let response = try? await request(["cmd": "get_version"]),
              response["ok"] as? Bool == true,
              let version = (response["data"] as? NSNumber)?.uint64Value else { return nil }
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
            "wait_ms": 2_500
        ]
        if let sinceNotificationId {
            requestPayload["since_notification_id"] = sinceNotificationId
        }
        guard let response = try? await request(
            requestPayload,
            timeoutSeconds: 4
        ),
        response["ok"] as? Bool == true,
        let data = response["data"] as? [String: Any],
        let nextRevision = (data["revision"] as? NSNumber)?.uint64Value,
        let historyVersion = (data["history_version"] as? NSNumber)?.uint64Value,
        let statusData = data["status"] as? [String: Any] else { return nil }

        let notifications: [RuntimeSnapshot.Notification] =
            (data["notifications"] as? [[String: Any]] ?? []).compactMap { value in
            guard let id = (value["id"] as? NSNumber)?.uint64Value,
                  let message = value["message"] as? String else { return nil }
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
              let categories = data["categories"] as? [String] else {
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
              let unresolvedCount = (data["unresolved_count"] as? NSNumber)?.intValue else {
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
              let peer = data["peer"] as? String else { return nil }
        return SyncWarning(kind: kind, peer: peer)
    }

    struct UpdateInfo: Decodable {
        let current_version: String
        let version: String
        let notes: String?
        let published_at: String?
    }

    func checkForUpdate() async throws -> UpdateInfo? {
        let response = try await request(["cmd": "check_for_update"], timeoutSeconds: 30)
        guard response["ok"] as? Bool == true else {
            throw ApiError.serverError(response["error"] as? String ?? "Update check failed")
        }
        guard let value = response["data"], !(value is NSNull) else { return nil }
        return try JSONDecoder().decode(
            UpdateInfo.self,
            from: JSONSerialization.data(withJSONObject: value)
        )
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
              let data = response["data"] as? [String: Any] else {
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

    struct ImageData {
        let width: Int
        let height: Int
        let rgba: Data
    }

    func getImageData(id: Int64) async -> ImageData? {
        guard let response = try? await request(["cmd": "get_image_data", "id": id]),
              response["ok"] as? Bool == true,
              let data = response["data"] as? [String: Any],
              let width = (data["width"] as? NSNumber)?.intValue,
              let height = (data["height"] as? NSNumber)?.intValue,
              let encoded = data["rgba_b64"] as? String,
              let rgba = Data(base64Encoded: encoded) else { return nil }
        return ImageData(width: width, height: height, rgba: rgba)
    }

    /// Decrypted history bytes for Quick Look/text preview.
    ///
    /// The daemon wraps the bytes in JSON/base64, so the response limit must
    /// account for base64's ~4/3 expansion over the shared 64 MiB payload cap.
    /// This command is deliberately given a longer socket timeout than normal
    /// metadata calls, while every other request retains the 3-second limit.
    func getPreviewData(id: Int64, batchId: String? = nil) async throws -> HistoryPreviewData {
        var command: [String: Any] = ["cmd": "get_preview_data", "id": id]
        if let batchId, !batchId.isEmpty {
            command["batch_id"] = batchId
        }
        let response = try await request(
            command,
            timeoutSeconds: 30,
            maxResponseBytes: 96 * 1024 * 1024
        )
        guard response["ok"] as? Bool == true else {
            throw Self.previewError(
                from: response,
                fallback: "Could not load history preview"
            )
        }
        guard let data = response["data"] as? [String: Any],
              let kind = data["kind"] as? String,
              ["image", "text", "file"].contains(kind),
              let name = data["name"] as? String,
              !name.isEmpty,
              let sizeNumber = data["size_bytes"] as? NSNumber,
              let encoded = data["data_b64"] as? String else {
            throw ApiError.serverError(response["error"] as? String ?? "Invalid history preview response")
        }

        let sizeBytes = sizeNumber.int64Value
        guard sizeBytes >= 0, sizeBytes <= HistoryPreviewData.maxBytes else {
            throw HistoryPreviewStoreError.tooLarge
        }
        // Reject an impossible Base64 envelope before allocating its decoded
        // buffer. Four encoded bytes represent at most three payload bytes.
        let maximumEncoded = Int((HistoryPreviewData.maxBytes + 2) / 3 * 4 + 4)
        guard encoded.utf8.count <= maximumEncoded else {
            throw HistoryPreviewStoreError.tooLarge
        }
        guard let bytes = Data(base64Encoded: encoded),
              Int64(bytes.count) <= HistoryPreviewData.maxBytes,
              Int64(bytes.count) == sizeBytes else {
            throw ApiError.serverError("Invalid history preview data")
        }
        return HistoryPreviewData(
            kind: kind,
            name: name,
            sizeBytes: sizeBytes,
            data: bytes,
            entryId: (data["entry_id"] as? NSNumber)?.int64Value,
            batch: Self.decodePreviewBatch(data["batch"])
        )
    }

    private static func decodePreviewBatch(_ value: Any?) -> HistoryPreviewBatchNavigation? {
        guard let batch = value as? [String: Any],
              let batchId = batch["batch_id"] as? String,
              let itemIndex = (batch["item_index"] as? NSNumber)?.intValue,
              let itemCount = (batch["item_count"] as? NSNumber)?.intValue,
              let firstEntryId = (batch["first_entry_id"] as? NSNumber)?.int64Value,
              let lastEntryId = (batch["last_entry_id"] as? NSNumber)?.int64Value else {
            return nil
        }
        return HistoryPreviewBatchNavigation(
            batchId: batchId,
            itemIndex: itemIndex,
            itemCount: itemCount,
            firstEntryId: firstEntryId,
            lastEntryId: lastEntryId,
            previousEntryId: (batch["previous_entry_id"] as? NSNumber)?.int64Value,
            nextEntryId: (batch["next_entry_id"] as? NSNumber)?.int64Value
        )
    }

    private static func previewError(
        from response: [String: Any],
        fallback: String
    ) -> HistoryPreviewRemoteError {
        let responseData = response["data"] as? [String: Any]
        let rawError = response["error"] as? String
        let encodedFailure: [String: Any]? = rawError
            .flatMap { $0.data(using: .utf8) }
            .flatMap { try? JSONSerialization.jsonObject(with: $0) as? [String: Any] }
        let codeValue = encodedFailure?["code"] as? String
            ?? responseData?["error_code"] as? String
            ?? response["error_code"] as? String
        let message = encodedFailure?["message"] as? String ?? rawError ?? fallback
        return HistoryPreviewRemoteError(
            code: codeValue.flatMap(HistoryPreviewRemoteErrorCode.init(rawValue:)),
            message: message,
            retryable: encodedFailure?["retryable"] as? Bool
        )
    }

    struct FileProgress: Equatable {
        let batchId: String
        let name: String
        let sent: UInt64
        let total: UInt64
        let active: Bool
        let direction: String
        let device: String
        let completedFiles: Int
        let totalFiles: Int
        let speedBytesPerSecond: UInt64
        let canStop: Bool
    }

    func getFileProgress() async -> FileProgress? {
        guard let response = try? await request(["cmd": "get_file_progress"]),
              response["ok"] as? Bool == true,
              let data = response["data"] as? [String: Any] else { return nil }
        return Self.decodeFileProgress(data)
    }

    private static func decodeFileProgress(_ value: Any?) -> FileProgress? {
        guard let data = value as? [String: Any],
              let name = data["name"] as? String,
              let sent = (data["sent"] as? NSNumber)?.uint64Value,
              let total = (data["total"] as? NSNumber)?.uint64Value,
              let active = data["active"] as? Bool else { return nil }
        return FileProgress(
            batchId: data["batch_id"] as? String ?? "",
            name: name,
            sent: sent,
            total: total,
            active: active,
            direction: data["direction"] as? String ?? "receiving",
            device: data["device"] as? String ?? "",
            completedFiles: (data["completed_files"] as? NSNumber)?.intValue ?? 0,
            totalFiles: (data["total_files"] as? NSNumber)?.intValue ?? 1,
            speedBytesPerSecond: (data["speed_bytes_per_second"] as? NSNumber)?.uint64Value ?? 0,
            canStop: data["can_stop"] as? Bool ?? false
        )
    }

    func cancelFileBatch(_ batchId: String) async {
        _ = try? await request(["cmd": "cancel_file_batch", "batch_id": batchId])
    }

    struct LocalThemeSettings: Codable, Equatable, Sendable {
        var activeThemeId: String
        var appearance: String
        var highContrast: Bool
    }
    struct ThemeV2Descriptor: Equatable, Identifiable {
        let id: String
        let storageHandle: String
        let source: String
        let version: String
        let digest: String
        let name: [String: String]
        let status: String
        let diagnostics: [ThemeDiagnostic]
    }
    struct ThemeDiagnostic: Equatable {
        let code: String
        let message: String
        let jsonPointer: String
        let severity: String
        let platforms: [String]
        let recoverable: Bool
        let fallbackApplied: Bool
    }
    struct ThemeValidation { let valid: Bool; let digest: String?; let candidateVersion: String?; let diagnostics: [ThemeDiagnostic]; let previewId: String?; let previewTokens: [String: Any]?; let previewAssetSlots: [String: TailSyncThemeAssetDescriptor] }

    func getLocalThemeSettings() async throws -> LocalThemeSettings {
        let response = try await request(["cmd": "get_local_theme_settings"])
        guard response["ok"] as? Bool == true, let data = response["data"] else { throw themeApiError(response, fallback: "unknown") }
        return try JSONDecoder().decode(LocalThemeSettings.self, from: JSONSerialization.data(withJSONObject: data))
    }
    func setLocalThemeSettings(_ settings: LocalThemeSettings) async throws {
        let data = try JSONSerialization.jsonObject(with: JSONEncoder().encode(settings)) as! [String: Any]
        let response = try await request(["cmd": "set_local_theme_settings", "settings": data])
        guard response["ok"] as? Bool == true else { throw themeApiError(response, fallback: "unknown") }
    }
    func listThemesV2() async throws -> [ThemeV2Descriptor] {
        let response = try await request(["cmd": "list_themes_v2"])
        guard response["ok"] as? Bool == true, let data = response["data"] as? [[String: Any]] else { throw themeApiError(response, fallback: "unknown") }
        return data.compactMap(themeV2Descriptor)
    }
    func validateThemeV2(path: String, mode: String = "light", highContrast: Bool = false) async throws -> ThemeValidation {
        let response = try await request(["cmd": "validate_theme", "path": path, "mode": mode, "high_contrast": highContrast])
        guard response["ok"] as? Bool == true, let data = response["data"] as? [String: Any] else { throw themeApiError(response, fallback: "unknown") }
        let previewData = data["preview"] as? [String: Any]
        let preview = previewData?["tokens"] as? [String: Any]
        let assets = (previewData?["assetSlots"] as? [String: Any] ?? [:]).compactMapValues { raw -> TailSyncThemeAssetDescriptor? in
            guard let value = raw as? [String: Any], let slot = value["slot"] as? String, let key = value["key"] as? String, let digest = value["digest"] as? String, let mime = value["mimeType"] as? String, let bytes = (value["bytes"] as? NSNumber)?.intValue, let width = (value["width"] as? NSNumber)?.intValue, let height = (value["height"] as? NSNumber)?.intValue else { return nil }
            return TailSyncThemeAssetDescriptor(slot: slot, key: key, digest: digest, mimeType: mime, bytes: bytes, width: width, height: height)
        }
        return ThemeValidation(valid: data["valid"] as? Bool ?? false, digest: data["digest"] as? String, candidateVersion: data["candidateVersion"] as? String, diagnostics: themeDiagnostics(data["diagnostics"]), previewId: previewData?["id"] as? String, previewTokens: preview, previewAssetSlots: assets)
    }
    func installThemeV2(path: String, digest: String) async throws -> ThemeV2Descriptor {
        let response = try await request(["cmd": "install_theme", "path": path, "expected_digest": digest])
        guard response["ok"] as? Bool == true,
              let data = response["data"] as? [String: Any],
              let descriptor = themeV2Descriptor(data) else {
            throw themeApiError(response, fallback: "invalid theme install response")
        }
        return descriptor
    }
    func updateThemeV2(
        path: String,
        digest: String,
        allowSameVersion: Bool = false,
        allowDowngrade: Bool = false
    ) async throws -> ThemeV2Descriptor {
        let response = try await request([
            "cmd": "update_theme",
            "path": path,
            "expected_digest": digest,
            "options": [
                "allowSameVersion": allowSameVersion,
                "allowDowngrade": allowDowngrade
            ]
        ])
        guard response["ok"] as? Bool == true,
              let data = response["data"] as? [String: Any],
              let descriptor = themeV2Descriptor(data) else {
            throw themeApiError(response, fallback: "invalid theme update response")
        }
        return descriptor
    }
    func rollbackThemeV2(id: String) async throws -> ThemeV2Descriptor {
        let response = try await request(["cmd": "rollback_theme", "theme_id": id])
        guard response["ok"] as? Bool == true,
              let data = response["data"] as? [String: Any],
              let descriptor = themeV2Descriptor(data) else {
            throw themeApiError(response, fallback: "invalid theme rollback response")
        }
        return descriptor
    }
    func deleteThemeV2(id: String, storageHandle: String) async throws {
        let response = try await request(["cmd": "delete_theme_v2", "theme_id": id, "storage_handle": storageHandle])
        guard response["ok"] as? Bool == true else { throw themeApiError(response, fallback: "unknown") }
    }

    /// Core resolves V2 inheritance/expressions; Swift maps only resolved
    /// semantic values into its native palette model.
    func resolveThemeV2(themeId: String, highContrast: Bool = false) async -> TailSyncThemeDefinition? {
        async let light = request(["cmd": "resolve_theme", "theme_id": themeId, "mode": "light", "platform": "macos", "high_contrast": highContrast])
        async let dark = request(["cmd": "resolve_theme", "theme_id": themeId, "mode": "dark", "platform": "macos", "high_contrast": highContrast])
        guard let l = try? await light, let d = try? await dark,
              l["ok"] as? Bool == true, d["ok"] as? Bool == true,
              let lightData = l["data"] as? [String: Any],
              let darkData = d["data"] as? [String: Any] else { return nil }
        return ThemeResolvedPairDecoder.decode(
            themeId: themeId,
            highContrast: highContrast,
            lightData: lightData,
            darkData: darkData
        )
    }

    func getThemeAssetSlot(themeId: String, digest: String, slot: String) async throws -> Data {
        let response = try await request(["cmd": "get_theme_asset_slot", "theme_id": themeId, "expected_digest": digest, "asset_slot": slot], maxResponseBytes: 16 * 1024 * 1024)
        guard response["ok"] as? Bool == true, let value = response["data"] as? [String: Any], let encoded = value["data_b64"] as? String, let bytes = Data(base64Encoded: encoded) else {
            throw themeApiError(response, fallback: "invalid theme asset response")
        }
        return bytes
    }

    func previewThemeAssetSlot(path: String, digest: String, slot: String) async throws -> Data {
        let response = try await request(["cmd": "preview_theme_asset_slot", "path": path, "expected_digest": digest, "asset_slot": slot], maxResponseBytes: 16 * 1024 * 1024)
        guard response["ok"] as? Bool == true, let value = response["data"] as? [String: Any], let encoded = value["data_b64"] as? String, let bytes = Data(base64Encoded: encoded) else {
            throw themeApiError(response, fallback: "invalid theme preview asset response")
        }
        return bytes
    }

    private func themeErrorMessage(_ response: [String: Any], fallback: String) -> String {
        if let data = response["data"] as? [String: Any], let code = data["code"] as? String, let message = data["message"] as? String {
            return "\(code): \(message)"
        }
        return response["error"] as? String ?? fallback
    }

    private func themeApiError(_ response: [String: Any], fallback: String) -> ApiError {
        if let data = response["data"] as? [String: Any],
           let diagnostic = themeDiagnostic(data) {
            return .themeError(diagnostic)
        }
        return .serverError(themeErrorMessage(response, fallback: fallback))
    }

    private func themeDiagnostic(_ value: [String: Any]) -> ThemeDiagnostic? {
        guard let code = value["code"] as? String,
              let message = value["message"] as? String else { return nil }
        return ThemeDiagnostic(
            code: code,
            message: message,
            jsonPointer: value["jsonPointer"] as? String ?? "",
            severity: value["severity"] as? String ?? "error",
            platforms: value["platforms"] as? [String] ?? [],
            recoverable: value["recoverable"] as? Bool ?? false,
            fallbackApplied: value["fallbackApplied"] as? Bool ?? false
        )
    }

    private func themeV2Descriptor(_ item: [String: Any]) -> ThemeV2Descriptor? {
        guard let id = item["id"] as? String,
              let handle = item["storageHandle"] as? String else { return nil }
        return ThemeV2Descriptor(
            id: id,
            storageHandle: handle,
            source: item["source"] as? String ?? "custom",
            version: item["version"] as? String ?? "",
            digest: item["digest"] as? String ?? "",
            name: item["name"] as? [String: String] ?? [:],
            status: item["status"] as? String ?? "invalid",
            diagnostics: themeDiagnostics(item["diagnostics"])
        )
    }

    private func themeDiagnostics(_ raw: Any?) -> [ThemeDiagnostic] {
        guard let values = raw as? [[String: Any]] else { return [] }
        return values.compactMap { value in
            themeDiagnostic(value)
        }
    }

    func restoreFileBatch(_ batchId: String) async throws {
        let response = try await request(["cmd": "restore_file_batch", "batch_id": batchId])
        guard response["ok"] as? Bool == true else {
            throw ApiError.serverError(response["error"] as? String ?? "unknown")
        }
    }


    func setHistoryPinned(id: Int64, pinned: Bool) async throws {
        let response = try await request(["cmd": "set_history_pinned", "id": id, "pinned": pinned])
        guard response["ok"] as? Bool == true else {
            throw ApiError.serverError(response["error"] as? String ?? "unknown")
        }
    }

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
              let data = response["data"] as? [String: Any] else { return nil }
        return Self.decodeStorageStatus(data)
    }

    private static func decodeStorageStatus(_ value: Any?) -> StorageStatus? {
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
        let response = try await request(["cmd": "change_storage_location", "parent": parent], timeoutSeconds: 600)
        guard response["ok"] as? Bool == true,
              let data = response["data"] as? [String: Any] else {
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

    func getHistory(
        keyword: String? = nil,
        category: String? = nil,
        startTime: String? = nil,
        endTime: String? = nil,
        limit: Int = 30,
        offset: Int = 0
    ) async throws -> [HistoryEntry] {
        var request: [String: Any] = ["cmd": "get_history", "limit": limit, "offset": offset]
        if let keyword { request["keyword"] = keyword }
        if let category { request["category"] = category }
        if let startTime { request["start_time"] = startTime }
        if let endTime { request["end_time"] = endTime }
        let response = try await self.request(request)
        guard response["ok"] as? Bool == true,
              let data = response["data"] as? [[String: Any]] else {
            throw ApiError.serverError(response["error"] as? String ?? "unknown")
        }
        return data.compactMap { item in
            guard let data = try? JSONSerialization.data(withJSONObject: item) else { return nil }
            return try? JSONDecoder().decode(HistoryEntry.self, from: data)
        }
    }

    func deleteEntry(id: Int64) async throws {
        let response = try await request(["cmd": "delete_entry", "id": id])
        guard response["ok"] as? Bool == true else {
            throw ApiError.serverError(response["error"] as? String ?? "unknown")
        }
    }

    func restoreEntry(id: Int64) async throws {
        let response = try await request(["cmd": "restore_entry", "id": id])
        guard response["ok"] as? Bool == true else {
            throw ApiError.serverError(response["error"] as? String ?? "unknown")
        }
    }

    func getSettings() async throws -> AppSettings {
        let response = try await request(["cmd": "get_settings"])
        guard response["ok"] as? Bool == true,
              let data = response["data"] as? [String: Any] else {
            throw ApiError.serverError(response["error"] as? String ?? "unknown")
        }
        return try JSONDecoder().decode(AppSettings.self, from: JSONSerialization.data(withJSONObject: data))
    }

    func updateSettings(_ settings: AppSettings) async throws {
        let encoded = try JSONEncoder().encode(settings)
        let object = try JSONSerialization.jsonObject(with: encoded) as! [String: Any]
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
        guard let response = try? await request(["cmd": "set_sync_shortcut", "shortcut": shortcut]) else {
            return false
        }
        return response["ok"] as? Bool == true
    }

    func setHistoryShortcut(_ shortcut: String) async -> Bool {
        guard let response = try? await request(["cmd": "set_history_shortcut", "shortcut": shortcut]) else {
            return false
        }
        return response["ok"] as? Bool == true
    }

    func toggleSync() async -> Bool? {
        guard let response = try? await request(["cmd": "toggle_sync"]),
              response["ok"] as? Bool == true,
              let data = response["data"] as? [String: Any],
              let enabled = data["enabled"] as? Bool else { return nil }
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

    struct DeviceSnapshot: Decodable {
        let hostname: String
        let tailscale_ip: String
        let connection_mode: String
        let public_key: String
        let fingerprint: String
        let iroh_endpoint_id: String?

        init(from decoder: Decoder) throws {
            let values = try decoder.container(keyedBy: CodingKeys.self)
            hostname = try values.decodeIfPresent(String.self, forKey: .hostname) ?? "Unknown"
            tailscale_ip = try values.decodeIfPresent(String.self, forKey: .tailscale_ip) ?? ""
            connection_mode = try values.decodeIfPresent(String.self, forKey: .connection_mode) ?? "tailscale"
            public_key = try values.decodeIfPresent(String.self, forKey: .public_key) ?? ""
            fingerprint = try values.decodeIfPresent(String.self, forKey: .fingerprint) ?? ""
            iroh_endpoint_id = try values.decodeIfPresent(String.self, forKey: .iroh_endpoint_id)
        }

        private enum CodingKeys: String, CodingKey { case hostname, tailscale_ip, connection_mode, public_key, fingerprint, iroh_endpoint_id }
    }

    struct PeerSnapshot: Decodable, Identifiable {
        struct Route: Decodable {
            let interface: String
            let address: String
            let status: String
            let online: Bool
            let connected: Bool
            let latencyMs: Int?
            let pairingEndpoint: Bool
            let rttCapable: Bool

            init(from decoder: Decoder) throws {
                let values = try decoder.container(keyedBy: CodingKeys.self)
                interface = try values.decodeIfPresent(String.self, forKey: .interface) ?? "lan"
                address = try values.decodeIfPresent(String.self, forKey: .address) ?? ""
                status = try values.decodeIfPresent(String.self, forKey: .status) ?? "discovered"
                online = try values.decodeIfPresent(Bool.self, forKey: .online) ?? false
                connected = try values.decodeIfPresent(Bool.self, forKey: .connected) ?? false
                latencyMs = try values.decodeIfPresent(Int.self, forKey: .latencyMs)
                    ?? values.decodeIfPresent(Int.self, forKey: .legacyLatency)
                pairingEndpoint = try values.decodeIfPresent(Bool.self, forKey: .pairingEndpoint) ?? false
                rttCapable = try values.decodeIfPresent(Bool.self, forKey: .rttCapable)
                    ?? (interface != "iroh")
            }

            private enum CodingKeys: String, CodingKey {
                case interface, address, status, online, connected
                case latencyMs = "latency_ms"
                case legacyLatency = "latency"
                case pairingEndpoint = "pairing_endpoint"
                case rttCapable = "rtt_capable"
            }
        }

        struct Candidate: Decodable {
            let interface: String
            let address: String
            let online: Bool
            let latency: Int?
            let status: String
            let rttCapable: Bool

            init(from decoder: Decoder) throws {
                let values = try decoder.container(keyedBy: CodingKeys.self)
                interface = try values.decodeIfPresent(String.self, forKey: .interface) ?? "lan"
                address = try values.decodeIfPresent(String.self, forKey: .address) ?? ""
                online = try values.decodeIfPresent(Bool.self, forKey: .online) ?? false
                latency = try values.decodeIfPresent(Int.self, forKey: .latency)
                status = try values.decodeIfPresent(String.self, forKey: .status)
                    ?? (online ? "online" : "discovered")
                rttCapable = try values.decodeIfPresent(Bool.self, forKey: .rttCapable)
                    ?? (interface != "iroh")
            }

            private enum CodingKeys: String, CodingKey {
                case interface, address, online, latency, status
                case rttCapable = "rtt_capable"
            }
        }

        let hostname: String
        let tailscale_ip: String
        let address: String
        let online: Bool
        let enabled: Bool
        let connection_mode: String
        let trusted: Bool
        let fingerprint: String
        let current_interface: String?
        let current_address: String?
        let candidates: [Candidate]
        let routes: [Route]
        let status: String
        let protocolError: String?
        let requiredProtocolVersion: Int?
        var id: String { hostname }

        init(from decoder: Decoder) throws {
            let values = try decoder.container(keyedBy: CodingKeys.self)
            hostname = try values.decodeIfPresent(String.self, forKey: .hostname) ?? "Unknown"
            tailscale_ip = try values.decodeIfPresent(String.self, forKey: .tailscale_ip) ?? ""
            address = try values.decodeIfPresent(String.self, forKey: .address) ?? tailscale_ip
            online = try values.decodeIfPresent(Bool.self, forKey: .online) ?? false
            enabled = try values.decodeIfPresent(Bool.self, forKey: .enabled) ?? true
            connection_mode = try values.decodeIfPresent(String.self, forKey: .connection_mode) ?? ""
            trusted = try values.decodeIfPresent(Bool.self, forKey: .trusted) ?? false
            fingerprint = try values.decodeIfPresent(String.self, forKey: .fingerprint) ?? ""
            current_interface = try values.decodeIfPresent(String.self, forKey: .current_interface)
            current_address = try values.decodeIfPresent(String.self, forKey: .current_address)
            candidates = try values.decodeIfPresent([Candidate].self, forKey: .candidates) ?? []
            routes = try values.decodeIfPresent([Route].self, forKey: .routes) ?? []
            status = try values.decodeIfPresent(String.self, forKey: .status)
                ?? (current_address != nil ? "connected" : online ? "online" : "offline")
            protocolError = try values.decodeIfPresent(String.self, forKey: .protocolError)
            requiredProtocolVersion = try values.decodeIfPresent(Int.self, forKey: .requiredProtocolVersion)
        }

        private enum CodingKeys: String, CodingKey {
            case hostname, tailscale_ip, address, online, enabled, connection_mode, trusted,
                 fingerprint, current_interface, current_address, candidates, routes, status
            case protocolError = "protocol_error"
            case requiredProtocolVersion = "required_protocol_version"
        }
    }

    typealias PeersResult = (
        local: DeviceSnapshot?,
        peers: [PeerSnapshot],
        pairedEndpoints: [String: String],
        error: String?,
        requestSucceeded: Bool
    )

    private func decodePeersResponse(_ response: [String: Any]) -> PeersResult? {
        guard response["ok"] as? Bool == true,
              let data = response["data"] as? [String: Any] else { return nil }
        let local: DeviceSnapshot?
        if let value = data["self"], let json = try? JSONSerialization.data(withJSONObject: value) {
            local = try? JSONDecoder().decode(DeviceSnapshot.self, from: json)
        } else {
            local = nil
        }
        let peers: [PeerSnapshot]
        if let value = data["peers"], let json = try? JSONSerialization.data(withJSONObject: value) {
            peers = (try? JSONDecoder().decode([PeerSnapshot].self, from: json)) ?? []
        } else {
            peers = []
        }
        return (
            local,
            peers,
            data["paired_peer_endpoints"] as? [String: String] ?? [:],
            data["discovery_error"] as? String,
            true
        )
    }

    func getPeers() async -> PeersResult {
        guard let response = try? await request(["cmd": "get_peers"]),
              let result = decodePeersResponse(response) else {
            return (nil, [], [:], responseError(), false)
        }
        return result
    }

    func refreshPeers() async -> PeersResult {
        guard let response = try? await request(["cmd": "refresh_peers"]),
              let result = decodePeersResponse(response) else {
            return (nil, [], [:], responseError(), false)
        }
        return result
    }

    func togglePeer(hostname: String, enabled: Bool) async -> Bool {
        guard let response = try? await request(["cmd": "toggle_peer", "hostname": hostname, "enabled": enabled]) else { return false }
        return response["ok"] as? Bool == true
    }

    func trustPeer(hostname: String, publicKey: String, address: String? = nil) async throws -> String {
        var payload: [String: Any] = ["cmd": "trust_peer", "hostname": hostname, "public_key": publicKey]
        if let address, !address.isEmpty { payload["address"] = address }
        let response = try await request(payload)
        guard response["ok"] as? Bool == true,
              let data = response["data"] as? [String: Any],
              let fingerprint = data["fingerprint"] as? String else {
            throw ApiError.serverError(response["error"] as? String ?? "Pairing failed")
        }
        return fingerprint
    }

    func forgetPeer(hostname: String) async throws {
        let response = try await request(["cmd": "forget_peer", "hostname": hostname])
        guard response["ok"] as? Bool == true else {
            throw ApiError.serverError(response["error"] as? String ?? "Unpair failed")
        }
    }

    struct PairingPeerStatus: Decodable {
        let hostname: String
        let address: String
        let fingerprint: String
        let verification_code: String
        let local_confirmed: Bool
        let remote_confirmed: Bool
    }

    struct PairingStatus: Decodable {
        let pairing_enabled: Bool
        let phase: String
        let expires_at: UInt64?
        let remaining_seconds: UInt64
        let failed_attempts: Int
        let max_failures: Int
        let peer: PairingPeerStatus?
        let error: String?
    }

    func getPairingStatus() async throws -> PairingStatus {
        try await pairingRequest(["cmd": "get_pairing_status"])
    }

    func enablePairing() async throws -> PairingStatus {
        try await pairingRequest(["cmd": "enable_pairing"])
    }

    func startPairing(address: String) async throws -> PairingStatus {
        try await pairingRequest(["cmd": "start_pairing", "address": address])
    }

    func confirmPairing() async throws -> PairingStatus {
        try await pairingRequest(["cmd": "confirm_pairing"])
    }

    func cancelPairing() async throws -> PairingStatus {
        try await pairingRequest(["cmd": "cancel_pairing"])
    }

    private func pairingRequest(_ payload: [String: Any]) async throws -> PairingStatus {
        let response = try await request(payload)
        guard response["ok"] as? Bool == true,
              let value = response["data"] as? [String: Any] else {
            throw ApiError.serverError(response["error"] as? String ?? "Pairing failed")
        }
        return try JSONDecoder().decode(
            PairingStatus.self,
            from: JSONSerialization.data(withJSONObject: value)
        )
    }

    func testConnection(address: String) async -> (latencyMs: Int, path: String, error: String)? {
        guard let response = try? await request(["cmd": "test_connection", "hostname": address]) else { return nil }
        if response["ok"] as? Bool == true,
           let data = response["data"] as? [String: Any],
           let latency = (data["latency_ms"] as? NSNumber)?.intValue {
            return (latency, data["path"] as? String ?? "", "")
        }
        return (0, "", response["error"] as? String ?? "Connection failed")
    }

    private func responseError() -> String {
        "Connection failed"
    }
}

enum ApiError: LocalizedError {
    case connectionFailed
    case sendFailed
    case noResponse
    case invalidJson
    case serverError(String)
    case themeError(ApiClient.ThemeDiagnostic)

    var errorDescription: String? {
        switch self {
        case .connectionFailed:
            return Loc.t("error.localServiceUnavailable")
        case .sendFailed:
            return Loc.t("error.localServiceSendFailed")
        case .noResponse:
            return Loc.t("error.localServiceNoResponse")
        case .invalidJson:
            return Loc.t("error.localServiceInvalidResponse")
        case .serverError(let message):
            return message
        case .themeError(let diagnostic):
            let pointer = diagnostic.jsonPointer.isEmpty ? "" : " (\(diagnostic.jsonPointer))"
            return "\(diagnostic.code): \(diagnostic.message)\(pointer)"
        }
    }

    var pairingErrorDescription: String {
        guard case .serverError(let message) = self else { return localizedDescription }
        if message.contains("Pairing window is closed") {
            return Loc.t("error.pairingWindowClosed")
        }
        if message.contains("Pairing handshake timed out") {
            return Loc.t("error.pairingHandshakeTimedOut")
        }
        if message.contains("Connection reset by peer") || message.contains("early eof") {
            return Loc.t("error.pairingConnectionClosed")
        }
        return message
    }
}
