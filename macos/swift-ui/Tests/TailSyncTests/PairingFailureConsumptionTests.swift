import Darwin
import Foundation
import XCTest
@testable import TailSync

/// Exercises the real socket → ApiClient.request → normalize → pairing/remote
/// methods → production presentation seam chain. No ApiClient mock is used.
final class PairingFailureConsumptionTests: XCTestCase {
    func testSocketFixtureDoesNotRetainAnUnownedListeningServer() async throws {
        weak var lifetime: PairingScriptedSocket?
        var server: PairingScriptedSocket? = try PairingScriptedSocket()
        let path = try XCTUnwrap(server?.path)
        server?.start()
        lifetime = server
        server = nil
        try await Task.sleep(nanoseconds: 100_000_000)
        XCTAssertNil(lifetime, "the accepting worker must not retain the server after its owner exits")
        XCTAssertFalse(FileManager.default.fileExists(atPath: path))
    }

    func testLateTypedRemoteErrorDoesNotReplaceANewInviteInput() async throws {
        let socket = try makeSocket()
        let client = client(for: socket)
        socket.respond("inspect_remote_pairing_link", ["ok": false, "error": envelope("pairing_window_closed")])
        var state = RemotePairingInputState()
        state.receive("tailsync://pair/v1/old")
        let operation = state.beginOperation()
        let oldLink = state.link
        let pending = Task { () -> Error? in
            do {
                _ = try await client.inspectRemotePairingLink(oldLink)
                return nil
            } catch { return error }
        }
        state.receive("tailsync://pair/v1/new")
        let responseError = await pending.value
        let error = try XCTUnwrap(responseError)
        state.fail(error, for: operation)
        XCTAssertNil(state.feedback, "an old request cannot publish an error into a replacement input")
        XCTAssertEqual(state.link, "tailsync://pair/v1/new")
        XCTAssertFalse(state.operationInProgress)
    }

    func testEveryPairingCommandKeepsFixedPolicyThroughTheRealSocket() async throws {
        let socket = try makeSocket()
        let client = client(for: socket)
        for (command, code, key) in [
            ("enable_pairing", "pairing_identity_mismatch", "error.pairing_identity_mismatch"),
            ("start_pairing", "pairing_window_closed", "error.pairing_window_closed"),
            ("confirm_pairing", "pairing_timed_out", "error.pairing_timed_out"),
            ("cancel_pairing", "pairing_connection_closed", "error.pairing_connection_closed"),
            ("create_remote_pairing_invite", "pairing_busy", "error.pairing_busy"),
            ("inspect_remote_pairing_link", "pairing_cancelled", "error.pairing_cancelled"),
            ("start_remote_pairing", "protocol_incompatible", "error.protocol_incompatible"),
            ("cancel_remote_pairing_invite", "invalid_argument", "error.invalid_argument"),
        ] {
            var failure = envelope(code, messageKey: "/private/message; token=secret")
            failure["retryable"] = true
            failure["detail_class"] = "/private/detail"
            socket.respond(command, ["ok": false, "error": failure])
            await forEachLanguage {
                do {
                    switch command {
                    case "enable_pairing": _ = try await client.enablePairing()
                    case "start_pairing": _ = try await client.startPairing(address: "10.0.0.9")
                    case "confirm_pairing": _ = try await client.confirmPairing()
                    case "cancel_pairing": _ = try await client.cancelPairing()
                    case "create_remote_pairing_invite": _ = try await client.createRemotePairingInvite()
                    case "inspect_remote_pairing_link": _ = try await client.inspectRemotePairingLink("tailsync://pair/v1/invite")
                    case "start_remote_pairing": _ = try await client.startRemotePairing(inviteLink: "tailsync://pair/v1/invite")
                    default: _ = try await client.cancelRemotePairingInvite()
                    }
                    XCTFail("expected a fixed error for \(command)")
                } catch {
                    guard let error = error as? ApiError,
                        case .stableServerError(let value) = error
                    else { return XCTFail("lost code for \(command): \(error)") }
                    XCTAssertEqual(value.rawValue, code)
                    XCTAssertEqual(PairingFailurePresentation.message(for: error), Loc.t(key))
                }
            }
        }
    }

    func testReplacedRemoteOperationCannotClearBusyOrPublishAnOldError() {
        var state = RemotePairingInputState()
        state.receive("tailsync://pair/v1/invite")
        let first = state.beginOperation()
        let second = state.beginOperation()
        state.finishOperation(first)
        state.fail(ApiError.stableServerError(.pairing_window_closed), for: first)
        XCTAssertTrue(state.operationInProgress)
        XCTAssertNil(state.feedback)
        state.fail(ApiError.stableServerError(.pairing_busy), for: second)
        XCTAssertEqual(state.feedback, .error(Loc.t("error.pairing_busy")))
        state.finishOperation(second)
        XCTAssertFalse(state.operationInProgress)
        state.receive("tailsync://pair/v1/invite")
        XCTAssertFalse(state.isCurrentOperation(second), "receiving the same link still supersedes the old operation")
        XCTAssertNil(state.feedback)
    }

    func testLegacyWireCannotForgeTheLocallyNormalizedCodeCarrier() async throws {
        let socket = try makeSocket()
        let client = client(for: socket)
        socket.respond("start_pairing", [
            "ok": false, "error": "Pairing window is closed", "error_code": "pairing_busy",
        ])
        await forEachLanguage {
            do {
                _ = try await client.startPairing(address: "10.0.0.9")
                XCTFail("expected legacy failure")
            } catch {
                guard let error = error as? ApiError,
                    case .serverError(let message) = error
                else { return XCTFail("unverified wire carrier became a typed error: \(error)") }
                XCTAssertEqual(message, "Pairing window is closed")
                XCTAssertEqual(error.pairingErrorDescription, Loc.t("error.pairingWindowClosed"))
            }
        }
    }
    private func envelope(_ code: String, messageKey: String = "error.identityMismatch") -> [String: Any] {
        [
            "schema_version": 1,
            "code": code,
            "retryable": false,
            "message_key": messageKey,
            "detail_class": "resource",
        ]
    }

    private func makeSocket() throws -> PairingScriptedSocket {
        let socket = try PairingScriptedSocket()
        socket.start()
        addTeardownBlock { socket.stop() }
        return socket
    }

    private func client(for socket: PairingScriptedSocket) -> ApiClient {
        ApiClient(socketPath: socket.path, capabilityToken: String(repeating: "a", count: 64))
    }

    /// Re-runs the whole flow for both UI languages so localized text is
    /// resolved in the language it is asserted against.
    private func forEachLanguage(_ body: () async throws -> Void) async rethrows {
        let loc = Loc.shared
        let previous = loc.lang
        defer { loc.lang = previous }
        for language in ["en", "zh-CN"] {
            loc.lang = language
            try await body()
        }
    }

    private func statusResponse(_ suffix: String) -> Data {
        let json =
            #"{"ok":true,"data":{"pairing_enabled":false,"phase":"disabled","remaining_seconds":0,"failed_attempts":0,"max_failures":5,"error":"Connection reset by peer""#
            + suffix + "}}"
        return Data(json.utf8) + Data([0x0A])
    }

    // MARK: - Direct typed pairing request errors

    func testStartPairingKeepsTypedCodeAndIgnoresContradictoryText() async throws {
        let socket = try makeSocket()
        let client = client(for: socket)
        // The envelope's message_key contradicts the code on purpose.
        socket.respond("start_pairing", [
            "ok": false,
            "error": envelope("pairing_window_closed", messageKey: "error.pairing_identity_mismatch"),
        ])

        do {
            _ = try await client.startPairing(address: "10.0.0.9")
            XCTFail("expected pairing failure")
        } catch {
            guard let apiError = error as? ApiError,
                case .stableServerError(let code) = apiError
            else { return XCTFail("unexpected error: \(error)") }
            XCTAssertEqual(code, .pairing_window_closed)
        }
    }

    func testTypedDirectErrorDescriptionFollowsTheCodeInBothLanguages() async throws {
        let socket = try makeSocket()
        let client = client(for: socket)
        socket.respond("confirm_pairing", [
            "ok": false,
            "error": envelope("pairing_busy", messageKey: "error.pairing_timed_out"),
        ])

        try await forEachLanguage {
            do {
                _ = try await client.confirmPairing()
                XCTFail("expected pairing failure")
            } catch {
                XCTAssertEqual(
                    PairingFailurePresentation.message(for: error),
                    Loc.t("error.pairing_busy"))
            }
        }
    }

    func testLegacyStringPairingErrorKeepsTheTextFallback() async throws {
        let socket = try makeSocket()
        let client = client(for: socket)
        socket.respond("start_pairing", ["ok": false, "error": "Pairing handshake timed out"])

        try await forEachLanguage {
            do {
                _ = try await client.startPairing(address: "10.0.0.9")
                XCTFail("expected pairing failure")
            } catch {
                guard let apiError = error as? ApiError,
                    case .serverError(let message) = apiError
                else { return XCTFail("unexpected error: \(error)") }
                XCTAssertEqual(message, "Pairing handshake timed out")
                XCTAssertEqual(
                    PairingFailurePresentation.message(for: error),
                    Loc.t("error.pairingHandshakeTimedOut"))
            }
        }
    }

    func testFutureStableCodeFallsBackToInternalPolicy() async throws {
        let socket = try makeSocket()
        let client = client(for: socket)
        socket.respond("cancel_pairing", [
            "ok": false,
            "error": envelope("a_brand_new_code", messageKey: "error.pairing_busy"),
        ])

        try await forEachLanguage {
            do {
                _ = try await client.cancelPairing()
                XCTFail("expected failure")
            } catch {
                guard let apiError = error as? ApiError,
                    case .stableServerError(let code) = apiError
                else { return XCTFail("unexpected error: \(error)") }
                XCTAssertEqual(code, .internal_error)
                XCTAssertEqual(
                    PairingFailurePresentation.message(for: error),
                    Loc.t("error.internal"))
            }
        }
    }

    // MARK: - Polled status kind precedence

    func testPolledStatusKindWinsOverContradictoryText() async throws {
        let socket = try makeSocket()
        let client = client(for: socket)
        socket.respondRaw("get_pairing_status", statusResponse(#","failure_kind":"window_closed""#))

        try await forEachLanguage {
            let status = try await client.getPairingStatus()
            XCTAssertEqual(status.failure_kind, .window_closed)
            XCTAssertEqual(
                PairingFailurePresentation.statusMessage(status),
                Loc.t("error.pairing_window_closed"))
        }
    }

    func testDirectTypedErrorBeatsAStalePolledStatus() async throws {
        let socket = try makeSocket()
        let client = client(for: socket)
        socket.respond("start_pairing", [
            "ok": false,
            "error": envelope("pairing_busy"),
        ])
        socket.respondRaw("get_pairing_status", statusResponse(#","failure_kind":"window_closed""#))

        try await forEachLanguage {
            var directMessage: String?
            do {
                _ = try await client.startPairing(address: "10.0.0.9")
            } catch {
                directMessage = PairingFailurePresentation.message(for: error)
            }
            let status = try await client.getPairingStatus()
            XCTAssertEqual(
                PairingFailurePresentation.message(directError: directMessage, status: status),
                Loc.t("error.pairing_busy"))
        }
    }

    func testMissingNullAndFutureKindsFallBackToLegacyText() async throws {
        let socket = try makeSocket()
        let client = client(for: socket)
        let legacyText = #""error":"Pairing window is closed""#

        func status(_ suffix: String) async throws -> ApiClient.PairingStatus {
            let json =
                #"{"ok":true,"data":{"pairing_enabled":false,"phase":"disabled","remaining_seconds":0,"failed_attempts":0,"max_failures":5,"#
                + legacyText + suffix + "}}"
            socket.respondRaw("get_pairing_status", Data(json.utf8) + Data([0x0A]))
            return try await client.getPairingStatus()
        }

        try await forEachLanguage {
            let missing = try await status("")
            XCTAssertNil(missing.failure_kind)
            let null = try await status(#","failure_kind":null"#)
            XCTAssertNil(null.failure_kind)
            let future = try await status(#","failure_kind":"some_future_kind""#)
            XCTAssertEqual(future.failure_kind, .unknown)
            for value in [missing, null, future] {
                XCTAssertEqual(
                    PairingFailurePresentation.statusMessage(value),
                    Loc.t("error.pairingWindowClosed"))
            }
        }
    }

    func testTransportKindStaysConservative() async throws {
        let socket = try makeSocket()
        let client = client(for: socket)
        socket.respondRaw("get_pairing_status", statusResponse(#","failure_kind":"transport""#))

        try await forEachLanguage {
            let status = try await client.getPairingStatus()
            XCTAssertEqual(status.failure_kind, .transport)
            XCTAssertEqual(
                PairingFailurePresentation.statusMessage(status),
                Loc.t("error.internal"))
        }
    }

    func testNonStringKindIsRejected() async throws {
        let socket = try makeSocket()
        let client = client(for: socket)
        socket.respondRaw("get_pairing_status", statusResponse(#","failure_kind":42"#))

        do {
            _ = try await client.getPairingStatus()
            XCTFail("expected decode rejection for a non-string kind")
        } catch {
            XCTAssertTrue(error is DecodingError, "unexpected error: \(error)")
        }
    }

    // MARK: - Remote pairing request errors

    func testRemoteStartPairingUsesTheTypedDescription() async throws {
        let socket = try makeSocket()
        let client = client(for: socket)
        socket.respond("start_remote_pairing", [
            "ok": false,
            "error": envelope("pairing_busy", messageKey: "error.pairing_connection_closed"),
        ])

        try await forEachLanguage {
            do {
                _ = try await client.startRemotePairing(inviteLink: "tailsync://pair/v1/abc")
                XCTFail("expected remote pairing failure")
            } catch {
                var state = RemotePairingInputState()
                state.beginOperation()
                state.fail(error)
                XCTAssertEqual(state.feedback, .error(Loc.t("error.pairing_busy")))
            }
        }
    }

    func testRemoteInviteCreationUsesTheTypedDescription() async throws {
        let socket = try makeSocket()
        let client = client(for: socket)
        socket.respond("create_remote_pairing_invite", [
            "ok": false,
            "error": envelope("pairing_window_closed"),
        ])

        try await forEachLanguage {
            do {
                _ = try await client.createRemotePairingInvite()
                XCTFail("expected remote invite failure")
            } catch {
                var state = RemotePairingInputState()
                state.fail(error)
                XCTAssertEqual(state.feedback, .error(Loc.t("error.pairing_window_closed")))
            }
        }
    }

    func testRemoteFailureIsClearedByTheNextOperation() {
        var state = RemotePairingInputState()
        state.fail(ApiError.serverError("Invite has expired"))
        XCTAssertEqual(state.feedback, .error("Invite has expired"))
        state.beginOperation()
        XCTAssertNil(state.feedback)
        XCTAssertNil(state.preview)
    }

    // MARK: - Other services keep the legacy behaviour

    func testNonPairingServiceKeepsPlainServerError() async throws {
        let socket = try makeSocket()
        let client = client(for: socket)
        socket.respond("forget_peer", [
            "ok": false,
            "error": envelope("invalid_argument"),
        ])

        do {
            try await client.forgetPeer(hostname: "peer")
            XCTFail("expected failure")
        } catch {
            guard let apiError = error as? ApiError,
                case .serverError = apiError
            else { return XCTFail("unexpected error: \(error)") }
        }
    }
}

/// Minimal scripted Unix-domain socket: accepts connections and replies with
/// the queued response for the request's `cmd`.
private final class PairingScriptedSocket: @unchecked Sendable {
    let path = "/tmp/ts-f10c2-\(UUID().uuidString).sock"
    private let worker: Worker

    init() throws { worker = try Worker(path: path) }
    deinit { stop() }

    func stop() {
        worker.stop()
        unlink(path)
    }
    func start() { worker.start() }
    func respond(_ cmd: String, _ json: [String: Any]) {
        let data = (try? JSONSerialization.data(withJSONObject: json)) ?? Data()
        respondRaw(cmd, data + Data([0x0A]))
    }
    func respondRaw(_ cmd: String, _ data: Data) { worker.respondRaw(cmd, data) }

    // The queue retains only its worker. Releasing the owning server stops and
    // joins that worker before closing the listener, so descriptors cannot be
    // reused while a background accept/poll still refers to them.
    private final class Worker: @unchecked Sendable {
        private let descriptor: Int32
        private let queue = DispatchQueue(label: "f10c2-socket")
        private let group = DispatchGroup()
        private let lock = NSLock()
        private var stopped = false
        private var closed = false
        private var responses: [String: Data] = [:]

        init(path: String) throws {
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
                unlink(path)
                throw ApiError.connectionFailed
            }
        }

        func respondRaw(_ cmd: String, _ data: Data) {
            lock.lock()
            responses[cmd] = data
            lock.unlock()
        }


        func start() {
            group.enter()
            queue.async { [self] in
                defer { group.leave() }
                while true {
                    lock.lock()
                    let shouldStop = stopped
                    lock.unlock()
                    if shouldStop { return }
                    var ready = pollfd(fd: descriptor, events: Int16(POLLIN), revents: 0)
                    guard poll(&ready, 1, 25) > 0 else { continue }
                    lock.lock()
                    let cancelled = stopped
                    lock.unlock()
                    if cancelled { return }
                    let connection = accept(descriptor, nil, nil)
                    if connection < 0 { return }
                    handle(connection)
                }
            }
        }

        func stop() {
            lock.lock()
            stopped = true
            lock.unlock()
            XCTAssertEqual(group.wait(timeout: .now() + 3), .success, "socket worker did not stop")
            lock.lock()
            if !closed {
                closed = true
                close(descriptor)
            }
            lock.unlock()
        }

        private func handle(_ connection: Int32) {
            defer { close(connection) }
            var noSignal: Int32 = 1
            setsockopt(
                connection, SOL_SOCKET, SO_NOSIGPIPE, &noSignal,
                socklen_t(MemoryLayout<Int32>.size))
            var readTimeout = timeval(tv_sec: 1, tv_usec: 0)
            setsockopt(connection, SOL_SOCKET, SO_RCVTIMEO, &readTimeout, socklen_t(MemoryLayout<timeval>.size))
            var buffer = [UInt8](repeating: 0, count: 4096)
            var request = Data()
            while !request.contains(0x0A) {
                let count = recv(connection, &buffer, buffer.count, 0)
                guard count > 0 else { return }
                request.append(contentsOf: buffer.prefix(count))
            }
            let object = (try? JSONSerialization.jsonObject(with: request)) as? [String: Any]
            XCTAssertEqual(object?["token"] as? String, String(repeating: "a", count: 64))
            XCTAssertEqual(object?["error_schema_version"] as? Int, 1)
            let cmd = object?["cmd"] as? String ?? ""
            lock.lock()
            let response = responses[cmd]
            lock.unlock()
            guard let response else { return }
            var sent = 0
            while sent < response.count {
                let count = response.withUnsafeBytes { raw -> Int in
                    guard let base = raw.baseAddress else { return -1 }
                    return send(connection, base.advanced(by: sent), response.count - sent, 0)
                }
                guard count > 0 else { return }
                sent += count
            }
        }
    }
}
