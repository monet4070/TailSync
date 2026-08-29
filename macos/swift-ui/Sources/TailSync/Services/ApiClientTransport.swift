import Darwin
import Foundation
import Security

final class ApiClient: @unchecked Sendable {
  static let shared = ApiClient()
  private let socketPath: String
  let capabilityToken: String
  private let daemonPIDLock = NSLock()
  private var expectedDaemonPID: pid_t?

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

  func request(
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
        let sock = socket(AF_UNIX, SOCK_STREAM, 0)
        guard sock >= 0 else {
          continuation.resume(throwing: ApiError.connectionFailed)
          return
        }
        defer { close(sock) }

        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)
        let pathBytes = Array(self.socketPath.utf8) + [0]
        guard pathBytes.count <= MemoryLayout.size(ofValue: address.sun_path) else {
          continuation.resume(throwing: ApiError.connectionFailed)
          return
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
          continuation.resume(throwing: ApiError.connectionFailed)
          return
        }

        if let expectedPID = self.expectedDaemonProcessIdentifier() {
          guard let peerPID = Self.peerProcessIdentifier(sock), peerPID == expectedPID else {
            continuation.resume(throwing: ApiError.connectionFailed)
            return
          }
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

  private static func peerProcessIdentifier(_ socket: Int32) -> pid_t? {
    var peerPID: pid_t = 0
    var length = socklen_t(MemoryLayout<pid_t>.size)
    let result = withUnsafeMutablePointer(to: &peerPID) { pointer in
      getsockopt(socket, SOL_LOCAL, LOCAL_PEERPID, pointer, &length)
    }
    return result == 0 ? peerPID : nil
  }
}
