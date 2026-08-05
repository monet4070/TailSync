import Foundation
import Darwin
import Security

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

    private func request(_ json: [String: Any]) async throws -> [String: Any] {
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
                var timeout = timeval(tv_sec: 3, tv_usec: 0)
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
                let maxResponseBytes = 4 * 1024 * 1024
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
              let data = response["data"] as? [String: Any],
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
        return StorageStatus(
            root: data["root"] as? String ?? "",
            usedBytes: (data["used_bytes"] as? NSNumber)?.uint64Value ?? 0,
            quotaBytes: (data["quota_bytes"] as? NSNumber)?.uint64Value ?? 0,
            available: data["available"] as? Bool ?? false,
            error: data["error"] as? String
        )
    }

    func changeStorageLocation(parent: String) async throws -> StorageMigrationResult {
        let response = try await request(["cmd": "change_storage_location", "parent": parent])
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
        let response = try await request(["cmd": "delete_old_storage", "path": path])
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

        init(from decoder: Decoder) throws {
            let values = try decoder.container(keyedBy: CodingKeys.self)
            hostname = try values.decodeIfPresent(String.self, forKey: .hostname) ?? "Unknown"
            tailscale_ip = try values.decodeIfPresent(String.self, forKey: .tailscale_ip) ?? ""
            connection_mode = try values.decodeIfPresent(String.self, forKey: .connection_mode) ?? "tailscale"
            public_key = try values.decodeIfPresent(String.self, forKey: .public_key) ?? ""
            fingerprint = try values.decodeIfPresent(String.self, forKey: .fingerprint) ?? ""
        }

        private enum CodingKeys: String, CodingKey { case hostname, tailscale_ip, connection_mode, public_key, fingerprint }
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
            }

            private enum CodingKeys: String, CodingKey {
                case interface, address, status, online, connected
                case latencyMs = "latency_ms"
                case legacyLatency = "latency"
                case pairingEndpoint = "pairing_endpoint"
            }
        }

        struct Candidate: Decodable {
            let interface: String
            let address: String
            let online: Bool
            let latency: Int?
            let status: String

            init(from decoder: Decoder) throws {
                let values = try decoder.container(keyedBy: CodingKeys.self)
                interface = try values.decodeIfPresent(String.self, forKey: .interface) ?? "lan"
                address = try values.decodeIfPresent(String.self, forKey: .address) ?? ""
                online = try values.decodeIfPresent(Bool.self, forKey: .online) ?? false
                latency = try values.decodeIfPresent(Int.self, forKey: .latency)
                status = try values.decodeIfPresent(String.self, forKey: .status)
                    ?? (online ? "online" : "discovered")
            }

            private enum CodingKeys: String, CodingKey {
                case interface, address, online, latency, status
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
        }

        private enum CodingKeys: String, CodingKey { case hostname, tailscale_ip, address, online, enabled, connection_mode, trusted, fingerprint, current_interface, current_address, candidates, routes, status }
    }

    typealias PeersResult = (
        local: DeviceSnapshot?,
        peers: [PeerSnapshot],
        pairedEndpoints: [String: String],
        error: String?
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
            data["discovery_error"] as? String
        )
    }

    func getPeers() async -> PeersResult {
        guard let response = try? await request(["cmd": "get_peers"]),
              let result = decodePeersResponse(response) else {
            return (nil, [], [:], responseError())
        }
        return result
    }

    func refreshPeers() async -> PeersResult {
        guard let response = try? await request(["cmd": "refresh_peers"]),
              let result = decodePeersResponse(response) else {
            return (nil, [], [:], responseError())
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

    func testConnection(address: String) async -> (latencyMs: Int, error: String)? {
        guard let response = try? await request(["cmd": "test_connection", "hostname": address]) else { return nil }
        if response["ok"] as? Bool == true,
           let data = response["data"] as? [String: Any],
           let latency = (data["latency_ms"] as? NSNumber)?.intValue {
            return (latency, "")
        }
        return (0, response["error"] as? String ?? "Connection failed")
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
        }
    }

    var pairingErrorDescription: String {
        guard case .serverError(let message) = self else {
            return localizedDescription
        }
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
