import Darwin
import Foundation
import XCTest
@testable import TailSync

/// Regression coverage for the history "auto-save network images" switch.
/// The toggle is a plain `AppSettings` flag, so the important boundaries are
/// decoding compatibility, encoding back to the backend, and the save/rollback
/// path through the shared `SettingsSaveCoordinator`.
final class SettingsNetworkImagesTests: XCTestCase {
    func testDefaultIsDisabled() {
        XCTAssertFalse(AppSettings().auto_save_network_images)
    }

    func testLegacySettingsWithoutFieldDecodeAsDisabled() throws {
        let data = Data("""
        {
          "notifications_enabled": true,
          "progress_bar_enabled": true,
          "sync_enabled": true,
          "history_limit": 100,
          "enabled_peers": {},
          "language": "en",
          "connection_mode": "auto"
        }
        """.utf8)

        let settings = try JSONDecoder().decode(AppSettings.self, from: data)

        XCTAssertFalse(settings.auto_save_network_images)
    }

    func testExplicitValuesDecode() throws {
        let enabled = try JSONDecoder().decode(
            AppSettings.self, from: Data(#"{"auto_save_network_images":true}"#.utf8))
        XCTAssertTrue(enabled.auto_save_network_images)

        let disabled = try JSONDecoder().decode(
            AppSettings.self, from: Data(#"{"auto_save_network_images":false}"#.utf8))
        XCTAssertFalse(disabled.auto_save_network_images)
    }

    func testEncodingCarriesFieldForBackendAndRoundTrips() throws {
        var value = AppSettings()
        value.auto_save_network_images = true

        let encoded = try JSONEncoder().encode(value)
        let object = try XCTUnwrap(
            JSONSerialization.jsonObject(with: encoded) as? [String: Any])
        XCTAssertEqual(object["auto_save_network_images"] as? Bool, true)

        let decoded = try JSONDecoder().decode(AppSettings.self, from: encoded)
        XCTAssertTrue(decoded.auto_save_network_images)
        XCTAssertEqual(decoded, value)
    }

    func testToggleSaveReachesUpdateSettingsWithTheFlag() async throws {
        let server = try SettingsNetworkImagesSocket()
        let client = ApiClient(
            socketPath: server.path, capabilityToken: String(repeating: "a", count: 64))
        let coordinator = SettingsSaveCoordinator(client: client)
        var value = AppSettings()
        value.auto_save_network_images = true

        let responseSent = expectation(description: "settings response sent")
        let connectionClosed = expectation(description: "settings socket closed")
        let response = try JSONSerialization.data(
            withJSONObject: ["ok": true, "data": client.jsonDictionary(value)]) + Data([0x0A])
        DispatchQueue.global().async {
            server.respond(
                with: response,
                responseSent: responseSent,
                connectionClosed: connectionClosed
            ) { request in
                XCTAssertEqual(request["cmd"] as? String, "update_settings")
                let settings = request["settings"] as? [String: Any]
                XCTAssertEqual(settings?["auto_save_network_images"] as? Bool, true)
            }
        }

        let outcome = await coordinator.save(.autoSaveNetworkImages(true), fallback: AppSettings())

        XCTAssertNil(outcome.error)
        XCTAssertTrue(outcome.persisted.auto_save_network_images)
        await fulfillment(of: [responseSent, connectionClosed], timeout: 4)
    }

    func testLocalizedLabelsExistForBothLanguages() {
        let loc = Loc.shared
        let previousLanguage = loc.lang
        defer { loc.lang = previousLanguage }

        for language in ["en", "zh-CN"] {
            loc.lang = language
            for key in ["qqImageAccess", "qqImageAccessDescription", "qqImageAccessChoose",
                        "qqImageAccessChange", "qqImageAccessReady", "qqImageAccessInvalid", "qqImageAccessFailed"] {
                XCTAssertNotEqual(Loc.t("settings.\(key)"), "settings.\(key)", language)
            }
            XCTAssertNotEqual(
                Loc.t("settings.autoSaveNetworkImages"),
                "settings.autoSaveNetworkImages",
                language
            )
            XCTAssertNotEqual(
                Loc.t("settings.autoSaveNetworkImagesDescription"),
                "settings.autoSaveNetworkImagesDescription",
                language
            )
        }
    }

    func testFailedToggleSaveRollsBackToPersistedDisabledValue() async throws {
        let server = try SettingsNetworkImagesSocket()
        let client = ApiClient(
            socketPath: server.path, capabilityToken: String(repeating: "a", count: 64))
        let coordinator = SettingsSaveCoordinator(client: client)
        var value = AppSettings()
        value.auto_save_network_images = true

        let responseSent = expectation(description: "failure response sent")
        let connectionClosed = expectation(description: "failure socket closed")
        DispatchQueue.global().async {
            server.respond(
                with: Data(#"{"ok":false,"error":"save failed"}"#.utf8) + Data([0x0A]),
                responseSent: responseSent,
                connectionClosed: connectionClosed
            )
        }

        let outcome = await coordinator.save(.autoSaveNetworkImages(true), fallback: AppSettings())

        XCTAssertNotNil(outcome.error)
        XCTAssertFalse(outcome.persisted.auto_save_network_images)
        await fulfillment(of: [responseSent, connectionClosed], timeout: 4)
    }
}

/// Minimal one-shot Unix-domain socket used to observe the exact request the
/// settings save path writes to the local service.
private final class SettingsNetworkImagesSocket: @unchecked Sendable {
    let path = "/tmp/ts-auto-images-\(UUID().uuidString).sock"
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

    deinit {
        close(descriptor)
        unlink(path)
    }

    func respond(
        with response: Data,
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
        setsockopt(
            connection, SOL_SOCKET, SO_NOSIGPIPE, &noSignal,
            socklen_t(MemoryLayout<Int32>.size))
        var buffer = [UInt8](repeating: 0, count: 4096)
        var request = Data()
        while !request.contains(0x0A) {
            let count = recv(connection, &buffer, buffer.count, 0)
            guard count > 0 else { return }
            request.append(contentsOf: buffer.prefix(count))
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
    }
}
