import Darwin
import Foundation
import Security

final class ApiClient: @unchecked Sendable {
  static let shared = ApiClient()
  private let socketPath: String
  let capabilityToken: String
  private let daemonPIDLock = NSLock()
  private var expectedDaemonPID: pid_t?

  // Dependency injection for isolated socket tests; production always uses shared.
  init(socketPath: String, capabilityToken: String, expectedDaemonPID: pid_t? = nil) {
    self.socketPath = socketPath
    self.capabilityToken = capabilityToken
    self.expectedDaemonPID = expectedDaemonPID
  }

  private init() {
    socketPath = Self.apiSocketPath()
    if let configured = ProcessInfo.processInfo.environment["TAILSYNC_API_TOKEN"],
      Self.isValidCapabilityToken(configured)
    {
      capabilityToken = configured.lowercased()
      return
    }

    var bytes = [UInt8](repeating: 0, count: 32)
    guard SecRandomCopyBytes(kSecRandomDefault, bytes.count, &bytes) == errSecSuccess else {
      fatalError("TailSync could not generate its local API capability token")
    }
    capabilityToken = bytes.map { String(format: "%02x", $0) }.joined()
  }

  static func apiSocketPathForDaemon() -> String {
    apiSocketPath()
  }

  func setExpectedDaemonPID(_ pid: pid_t?) {
    daemonPIDLock.lock()
    expectedDaemonPID = pid
    daemonPIDLock.unlock()
  }

  private func expectedDaemonProcessIdentifier() -> pid_t? {
    daemonPIDLock.lock()
    defer { daemonPIDLock.unlock() }
    return expectedDaemonPID
  }

  private static func apiSocketPath() -> String {
    if let configured = ProcessInfo.processInfo.environment["TAILSYNC_API_SOCKET"],
      !configured.isEmpty
    {
      return configured
    }
    let supportDirectory = FileManager.default
      .urls(for: .applicationSupportDirectory, in: .userDomainMask)
      .first?
      .appendingPathComponent("TailSync", isDirectory: true)
    return supportDirectory?
      .appendingPathComponent("tailsyncd.sock", isDirectory: false)
      .path
      ?? FileManager.default.temporaryDirectory
      .appendingPathComponent("TailSync/tailsyncd.sock", isDirectory: false)
      .path
  }

  private static func isValidCapabilityToken(_ value: String) -> Bool {
    value.count == 64
      && value.unicodeScalars.allSatisfy {
        (48...57).contains($0.value) || (65...70).contains($0.value)
          || (97...102).contains($0.value)
      }
  }

  func jsonDictionary<Value: Encodable>(_ value: Value) throws -> [String: Any] {
    let encoded = try JSONEncoder().encode(value)
    guard
      let dictionary = try JSONSerialization.jsonObject(with: encoded) as? [String: Any]
    else {
      throw ApiError.invalidJson
    }
    return dictionary
  }

  private enum ResponseFraming: Sendable {
    case jsonLine
    case eof
  }

  private func requestData(
    _ json: [String: Any],
    timeoutSeconds: Int,
    maxResponseBytes: Int,
    framing: ResponseFraming
  ) async throws -> Data {
    var authenticated = json
    authenticated["token"] = capabilityToken
    if ["get_history", "get_preview_data"].contains(json["cmd"] as? String ?? "") {
      authenticated["request_id"] = UUID().uuidString
    }
    var data = try JSONSerialization.data(withJSONObject: authenticated)
    data.append(0x0A)

    let ownership = RequestSocketOwnership()
    return try await withTaskCancellationHandler(operation: {
      try Task.checkCancellation()
      return try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Data, Error>) in
      DispatchQueue.global(qos: .userInitiated).async {
        do {
        try ownership.checkCancellation()
        let sock = socket(AF_UNIX, SOCK_STREAM, 0)
        guard sock >= 0 else {
          throw ApiError.connectionFailed
        }
        try ownership.install(sock)
        defer { ownership.finish() }
        var noSignal: Int32 = 1
        setsockopt(sock, SOL_SOCKET, SO_NOSIGPIPE, &noSignal, socklen_t(MemoryLayout<Int32>.size))

        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)
        let pathBytes = Array(self.socketPath.utf8) + [0]
        guard pathBytes.count <= MemoryLayout.size(ofValue: address.sun_path) else {
          throw ApiError.connectionFailed
        }
        withUnsafeMutableBytes(of: &address.sun_path) { destination in
          destination.initializeMemory(as: UInt8.self, repeating: 0)
          pathBytes.withUnsafeBytes { source in
            destination.copyBytes(from: source)
          }
        }
        var timeout = timeval(tv_sec: timeoutSeconds, tv_usec: 0)
        setsockopt(sock, SOL_SOCKET, SO_SNDTIMEO, &timeout, socklen_t(MemoryLayout<timeval>.size))
        setsockopt(sock, SOL_SOCKET, SO_RCVTIMEO, &timeout, socklen_t(MemoryLayout<timeval>.size))

        let connected = withUnsafePointer(to: &address) {
          $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
            connect(sock, $0, socklen_t(MemoryLayout<sockaddr_un>.size))
          }
        }
        guard connected == 0 else {
          throw ApiError.connectionFailed
        }

        if let expectedPID = self.expectedDaemonProcessIdentifier() {
          guard let peerPID = Self.peerProcessIdentifier(sock), peerPID == expectedPID else {
            throw ApiError.connectionFailed
          }
        }

        var sentTotal = 0
        while sentTotal < data.count {
          try ownership.checkCancellation()
          let sent = data.withUnsafeBytes { bytes -> Int in
            guard let base = bytes.baseAddress else { return -1 }
            return send(sock, base.advanced(by: sentTotal), data.count - sentTotal, 0)
          }
          guard sent > 0 else {
            if errno == EINTR { continue }
            throw ApiError.sendFailed
          }
          sentTotal += sent
        }

        // JSON responses end at the first newline. Binary preview responses
        // end at their declared frame length; EOF remains a truncation/error
        // signal because the server closes this one-shot connection afterward.
        var responseData = Data()
        var buffer = [UInt8](repeating: 0, count: 64 * 1024)
        var newlineIndex: Data.Index?
        var expectedFrameBytes: Int?
        while responseData.count <= maxResponseBytes {
          try ownership.checkCancellation()
          let received = recv(sock, &buffer, buffer.count, 0)
          if received == 0 { break }
          if received < 0 {
            if errno == EINTR { continue }
            throw ApiError.noResponse
          }
          responseData.append(contentsOf: buffer.prefix(received))
          if responseData.count > maxResponseBytes {
            throw ApiError.noResponse
          }
          if case .eof = framing {
            // Validate the bounded header and metadata as soon as they arrive,
            // before accepting a potentially large payload from the daemon.
            if responseData.first == 0x7B {
              guard responseData.count <= 1024 * 1024 else { throw ApiError.noResponse }
              // Binary-preview failures retain the legacy JSON-line envelope.
              // Do not keep a completed error waiting for the peer to close.
              if responseData.contains(0x0A) { break }
            } else {
              if expectedFrameBytes == nil, let (end, metadata) = try Self.previewFrameHeader(responseData) {
                expectedFrameBytes = end + Int(metadata.size_bytes)
              }
              if let expectedFrameBytes, responseData.count > expectedFrameBytes {
                throw ApiError.serverError("Trailing binary preview bytes")
              }
              // The frame declares its exact total length. Completing at that
              // boundary avoids an otherwise unbounded loading state if a
              // successfully written one-shot response is slow to close.
              if let expectedFrameBytes, responseData.count == expectedFrameBytes { break }
            }
          }
          if case .jsonLine = framing, let index = responseData.firstIndex(of: 0x0A) {
            newlineIndex = index
            break
          }
        }
        try ownership.checkCancellation()
        switch framing {
        case .jsonLine:
          guard let newlineIndex, newlineIndex > responseData.startIndex else {
            throw ApiError.noResponse
          }
          continuation.resume(returning: Data(responseData[..<newlineIndex]))
        case .eof:
          guard !responseData.isEmpty else {
            throw ApiError.noResponse
          }
          continuation.resume(returning: responseData)
        }
        } catch {
          continuation.resume(throwing: ownership.isCancelled ? CancellationError() : error)
        }
      }
      }
    }, onCancel: { ownership.cancel() })
  }

  func request(
    _ json: [String: Any],
    timeoutSeconds: Int = 3,
    maxResponseBytes: Int = 4 * 1024 * 1024
  ) async throws -> [String: Any] {
    var request = json
    // Preview failures have a separate, richer error code used by navigation.
    // Other JSON commands opt into the fixed local error envelope.
    if json["cmd"] as? String != "get_preview_data" {
      request["error_schema_version"] = 1
    }
    let line = try await requestData(
      request,
      timeoutSeconds: timeoutSeconds,
      maxResponseBytes: maxResponseBytes,
      framing: .jsonLine
    )
    guard let response = try? JSONSerialization.jsonObject(with: line) as? [String: Any] else {
      throw ApiError.invalidJson
    }
    return Self.normalizedStableErrorResponse(response)
  }

  static func normalizedStableErrorResponse(_ response: [String: Any]) -> [String: Any] {
    guard let error = response["error"] as? [String: Any] else { return response }
    var normalized = response
    let envelope = try? JSONDecoder().decode(
      ContractStableErrorEnvelope.self,
      from: JSONSerialization.data(withJSONObject: error)
    )
    // The generated decoder derives the display key from the code. Never use
    // untrusted server message_key or detail fields as visible text.
    normalized["error"] = Loc.t(envelope?.message_key ?? "error.internal")
    return normalized
  }

  /// Send an authenticated one-shot command and retain the raw response.
  /// This is used only for the negotiated binary preview capability.
  func requestBytes(
    _ json: [String: Any],
    timeoutSeconds: Int = 3,
    maxResponseBytes: Int = 4 * 1024 * 1024
  ) async throws -> Data {
    try await requestData(
      json,
      timeoutSeconds: timeoutSeconds,
      maxResponseBytes: maxResponseBytes,
      framing: .eof
    )
  }

  private static func peerProcessIdentifier(_ socket: Int32) -> pid_t? {
    var peerPID: pid_t = 0
    var length = socklen_t(MemoryLayout<pid_t>.size)
    let result = withUnsafeMutablePointer(to: &peerPID) { pointer in
      getsockopt(socket, SOL_LOCAL, LOCAL_PEERPID, pointer, &length)
    }
    return result == 0 ? peerPID : nil
  }
}
