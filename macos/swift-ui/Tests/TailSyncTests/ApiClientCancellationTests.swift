import Darwin
import Foundation
import XCTest
@testable import TailSync

final class ApiClientCancellationTests: XCTestCase {
  func testConnectionModeSaveDoesNotSendStaleSettingsAndUsesServerSnapshot() async throws {
    let server = try LocalTestSocket()
    let client = ApiClient(socketPath: server.path, capabilityToken: String(repeating: "a", count: 64))
    let coordinator = SettingsSaveCoordinator(client: client)
    var stale = AppSettings()
    stale.sync_enabled = true
    var actual = stale
    actual.sync_enabled = false
    actual.connection_mode = "lan_only"
    actual.history_limit = 250
    let response = try JSONSerialization.data(withJSONObject: [
      "ok": true, "data": client.jsonDictionary(actual),
    ]) + Data([0x0A])
    let responseSent = expectation(description: "connection mode saved")
    let connectionClosed = expectation(description: "connection mode socket closed")
    DispatchQueue.global().async {
      server.respond(with: response, closeDelay: 0, responseSent: responseSent, connectionClosed: connectionClosed) { request in
        XCTAssertEqual(request["cmd"] as? String, "set_connection_mode")
        XCTAssertEqual(request["connection_mode"] as? String, "lan_only")
        XCTAssertNil(request["settings"])
        XCTAssertNil(request["sync_enabled"])
      }
    }
    let outcome = await coordinator.saveConnectionMode("lan_only", fallback: stale)
    XCTAssertNil(outcome.error)
    XCTAssertFalse(outcome.persisted.sync_enabled)
    XCTAssertEqual(outcome.persisted.history_limit, 250)
    XCTAssertEqual(outcome.persisted.connection_mode, "lan_only")
    await fulfillment(of: [responseSent, connectionClosed], timeout: 4)
  }

  func testConnectionModeFailedSavePreservesLastServerSnapshot() async throws {
    let server = try LocalTestSocket()
    let client = ApiClient(socketPath: server.path, capabilityToken: String(repeating: "a", count: 64))
    let coordinator = SettingsSaveCoordinator(client: client)
    var actual = AppSettings()
    actual.sync_enabled = false
    actual.connection_mode = "lan_only"
    let success = try JSONSerialization.data(withJSONObject: ["ok": true, "data": client.jsonDictionary(actual)]) + Data([0x0A])
    let done = expectation(description: "two save responses")
    done.expectedFulfillmentCount = 4
    DispatchQueue.global().async {
      server.respond(with: success, closeDelay: 0, responseSent: done, connectionClosed: done)
      server.respond(with: Data("{\"ok\":false,\"error\":\"save failed\"}\n".utf8), closeDelay: 0, responseSent: done, connectionClosed: done)
    }
    _ = await coordinator.saveConnectionMode("lan_only", fallback: AppSettings())
    let failed = await coordinator.saveConnectionMode("iroh_only", fallback: AppSettings())
    XCTAssertNotNil(failed.error)
    XCTAssertFalse(failed.persisted.sync_enabled)
    XCTAssertEqual(failed.persisted.connection_mode, "lan_only")
    await fulfillment(of: [done], timeout: 4)
  }

  func testAuthenticatedSocketStableErrorsReachTheLocalizedClientBoundary() async throws {
    let loc = Loc.shared
    let previousLanguage = loc.lang
    defer { loc.lang = previousLanguage }
    for language in ["en", "zh-CN"] {
      loc.lang = language
      for code in ["protocol_incompatible", "storage_unavailable", "internal_error"] {
        let server = try LocalTestSocket()
        let client = ApiClient(socketPath: server.path, capabilityToken: String(repeating: "a", count: 64))
        let responseSent = expectation(description: "\(language) \(code) response sent")
        let connectionClosed = expectation(description: "\(language) \(code) connection closed")
        let response = Data(
          "{\"ok\":false,\"error\":{\"schema_version\":1,\"code\":\"\(code)\",\"retryable\":false,\"message_key\":\"private/path\",\"detail_class\":\"private\"}}\n".utf8
        )
        DispatchQueue.global().async {
          server.respond(
            with: response,
            closeDelay: 0,
            responseSent: responseSent,
            connectionClosed: connectionClosed
          )
        }
        let result = try await client.request(["cmd": "get_peers"])
        let key = code == "internal_error" ? "error.internal" : "error.\(code)"
        XCTAssertEqual(result["error"] as? String, Loc.t(key), "\(language) \(code)")
        XCTAssertFalse((result["error"] as? String ?? "").contains("private/path"))
        await fulfillment(of: [responseSent, connectionClosed], timeout: 4)
      }
    }
  }

  func testLegacyDaemonTextPreviewErrorUsesJsonCompatibilityPath() async throws {
    let server = try LocalTestSocket()
    let client = ApiClient(socketPath: server.path, capabilityToken: String(repeating: "a", count: 64))
    let capabilitySent = expectation(description: "legacy capability response sent")
    let capabilityClosed = expectation(description: "legacy capability connection closed")
    let previewSent = expectation(description: "legacy preview response sent")
    let previewClosed = expectation(description: "legacy preview connection closed")
    DispatchQueue.global().async {
      server.respond(
        with: Data(#"{"ok":false,"error":"unknown command: get_local_capabilities"}"#.utf8) + Data([0x0A]),
        closeDelay: 0,
        responseSent: capabilitySent,
        connectionClosed: capabilityClosed
      )
      server.respond(
        with: Data(#"{"ok":false,"error":"preview is too large"}"#.utf8) + Data([0x0A]),
        closeDelay: 0,
        responseSent: previewSent,
        connectionClosed: previewClosed
      )
    }

    do {
      _ = try await client.getPreviewData(id: 7)
      XCTFail("legacy preview error must be surfaced")
    } catch let error as HistoryPreviewRemoteError {
      XCTAssertNil(error.code)
      XCTAssertEqual(
        HistoryPreviewFailure.classify(error),
        HistoryPreviewFailure(kind: .tooLarge, canRetry: false)
      )
    }
    await fulfillment(of: [capabilitySent, capabilityClosed, previewSent, previewClosed], timeout: 4)
  }

  func testSixtyFourMiBBinaryPreviewCompletesAtDeclaredLengthWithoutWaitingForEOF() async throws {
    let server = try LocalTestSocket()
    let client = ApiClient(socketPath: server.path, capabilityToken: String(repeating: "a", count: 64))
    let width = 4_096
    let height = 4_096
    let payload = Data(repeating: 0x7F, count: width * height * 4)
    XCTAssertEqual(payload.count, 64 * 1024 * 1024)
    let metadata = try JSONSerialization.data(withJSONObject: [
      "entry_id": 7,
      "kind": "image",
      "name": "image",
      "size_bytes": payload.count,
      "width": width,
      "height": height,
      "batch": NSNull(),
    ])
    var frame = Data([0x54, 0x53, 0x50, 0x56, 1])
    frame.append(contentsOf: UInt32(metadata.count).littleEndianBytes)
    frame.append(metadata)
    frame.append(payload)

    let responseSent = expectation(description: "large preview frame sent")
    let connectionClosed = expectation(description: "test server closed delayed connection")
    DispatchQueue.global().async {
      server.respond(
        with: frame,
        closeDelay: 2,
        responseSent: responseSent,
        connectionClosed: connectionClosed
      )
    }

    let started = Date()
    let received = try await client.requestBytes(
      ["cmd": "get_preview_binary", "id": 7],
      timeoutSeconds: 10,
      maxResponseBytes: frame.count
    )
    let elapsed = Date().timeIntervalSince(started)

    XCTAssertEqual(received, frame)
    XCTAssertLessThan(elapsed, 1.75, "A complete frame must not wait for EOF")
    await fulfillment(of: [responseSent, connectionClosed], timeout: 4)
  }

  func testFiftyPreviewSwitchesReleaseTheirRealSocket() async throws {
    let server = try LocalTestSocket()
    let client = ApiClient(socketPath: server.path, capabilityToken: String(repeating: "a", count: 64))
    for _ in 0..<50 {
      let received = expectation(description: "authenticated request received")
      let disconnected = expectation(description: "cancelled socket disconnected")
      DispatchQueue.global().async {
        server.waitForRequestThenDisconnect(received: received, disconnected: disconnected)
      }
      let task = Task { try await client.requestBytes(["cmd": "get_preview_binary", "id": 7], timeoutSeconds: 30) }
      await fulfillment(of: [received], timeout: 2)
      let started = Date()
      task.cancel()
      do {
        _ = try await task.value
        XCTFail("A cancelled preview must throw")
      } catch is CancellationError {
        XCTAssertLessThan(Date().timeIntervalSince(started), 1)
      } catch { XCTFail("Unexpected cancellation error: \(error)") }
      await fulfillment(of: [disconnected], timeout: 1)
    }
  }

  func testCancelledBeforeConnectionDoesNotCreateASocket() async throws {
    let client = ApiClient(socketPath: "/tmp/tailsync-no-such-test-socket", capabilityToken: "unused")
    let task = Task {
      withUnsafeCurrentTask { $0?.cancel() }
      return try await client.request(["cmd": "get_history"])
    }
    do { _ = try await task.value; XCTFail("expected cancellation") }
    catch is CancellationError {} catch { XCTFail("Unexpected error: \(error)") }
  }

  func testLateCancellationCannotShutDownAReusedDescriptor() throws {
    var pair: [Int32] = [-1, -1]
    XCTAssertEqual(socketpair(AF_UNIX, SOCK_STREAM, 0, &pair), 0)
    let owner = RequestSocketOwnership()
    try owner.install(pair[0])
    owner.finish()
    var replacement: [Int32] = [-1, -1]
    XCTAssertEqual(socketpair(AF_UNIX, SOCK_STREAM, 0, &replacement), 0)
    XCTAssertEqual(dup2(replacement[0], pair[0]), pair[0])
    defer {
      for descriptor in Set([pair[0], pair[1], replacement[0], replacement[1]]) { close(descriptor) }
    }
    owner.cancel()
    var noSignal: Int32 = 1
    setsockopt(replacement[1], SOL_SOCKET, SO_NOSIGPIPE, &noSignal, socklen_t(MemoryLayout<Int32>.size))
    var byte: UInt8 = 42
    XCTAssertEqual(send(replacement[1], &byte, 1, 0), 1)
    XCTAssertEqual(recv(pair[0], &byte, 1, 0), 1)
    XCTAssertEqual(byte, 42)
  }
}

private final class LocalTestSocket: @unchecked Sendable {
  let path = "/tmp/ts-test-\(UUID().uuidString).sock"
  let descriptor: Int32

  init() throws {
    descriptor = socket(AF_UNIX, SOCK_STREAM, 0)
    guard descriptor >= 0 else { throw ApiError.connectionFailed }
    var address = sockaddr_un()
    address.sun_family = sa_family_t(AF_UNIX)
    let bytes = Array(path.utf8) + [0]
    withUnsafeMutableBytes(of: &address.sun_path) { $0.copyBytes(from: bytes) }
    let bound = withUnsafePointer(to: &address) {
      $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
        bind(descriptor, $0, socklen_t(MemoryLayout<sockaddr_un>.size))
      }
    }
    guard bound == 0, listen(descriptor, 8) == 0 else {
      close(descriptor)
      throw ApiError.connectionFailed
    }
  }

  deinit { close(descriptor); unlink(path) }

  func waitForRequestThenDisconnect(received: XCTestExpectation, disconnected: XCTestExpectation) {
    let connection = accept(descriptor, nil, nil)
    guard connection >= 0 else { return }
    defer { close(connection) }
    var timeout = timeval(tv_sec: 2, tv_usec: 0)
    setsockopt(connection, SOL_SOCKET, SO_RCVTIMEO, &timeout, socklen_t(MemoryLayout<timeval>.size))
    var bytes = [UInt8](repeating: 0, count: 4096)
    var request = Data()
    while !request.contains(0x0A) {
      let count = recv(connection, &bytes, bytes.count, 0)
      guard count > 0 else { return }
      request.append(contentsOf: bytes.prefix(count))
    }
    received.fulfill()
    if recv(connection, &bytes, bytes.count, 0) == 0 { disconnected.fulfill() }
  }

  func respond(
    with response: Data,
    closeDelay: TimeInterval,
    responseSent: XCTestExpectation,
    connectionClosed: XCTestExpectation,
    inspectRequest: (([String: Any]) -> Void)? = nil
  ) {
    let connection = accept(descriptor, nil, nil)
    guard connection >= 0 else { return }
    defer {
      close(connection)
      connectionClosed.fulfill()
    }
    var noSignal: Int32 = 1
    setsockopt(connection, SOL_SOCKET, SO_NOSIGPIPE, &noSignal, socklen_t(MemoryLayout<Int32>.size))
    var bytes = [UInt8](repeating: 0, count: 4096)
    var request = Data()
    while !request.contains(0x0A) {
      let count = recv(connection, &bytes, bytes.count, 0)
      guard count > 0 else { return }
      request.append(contentsOf: bytes.prefix(count))
    }
    if let object = try? JSONSerialization.jsonObject(with: request) as? [String: Any] {
      inspectRequest?(object)
    }
    var sent = 0
    while sent < response.count {
      let count = response.withUnsafeBytes { rawBytes -> Int in
        guard let base = rawBytes.baseAddress else { return -1 }
        return send(connection, base.advanced(by: sent), response.count - sent, 0)
      }
      guard count > 0 else { return }
      sent += count
    }
    responseSent.fulfill()
    Thread.sleep(forTimeInterval: closeDelay)
  }
}

private extension UInt32 {
  var littleEndianBytes: [UInt8] {
    withUnsafeBytes(of: littleEndian) { Array($0) }
  }
}
