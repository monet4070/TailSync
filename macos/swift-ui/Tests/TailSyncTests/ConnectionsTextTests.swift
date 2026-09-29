import XCTest
@testable import TailSync

final class ConnectionsTextTests: XCTestCase {
    @MainActor
    func testConnectionDetailsUseTheSelectedLanguage() {
        let previousLanguage = Loc.shared.lang
        defer { Loc.shared.lang = previousLanguage }

        Loc.shared.lang = "en"
        XCTAssertEqual(ConnectionsText.irohEndpoint("peer-id"), "Iroh endpoint: peer-id")
        XCTAssertEqual(ConnectionsText.routeInterface("lan"), "LAN route")
        XCTAssertEqual(ConnectionsText.latency(42), "42 ms")
        XCTAssertEqual(ConnectionsText.deviceCount(1), "1 device")
        XCTAssertEqual(ConnectionsText.deviceCount(2), "2 devices")
        XCTAssertEqual(
            ConnectionsText.pairingWindow(remaining: 120, failed: 1, maximum: 3),
            "120s left · 1/3 failed attempts"
        )

        Loc.shared.lang = "zh-CN"
        XCTAssertEqual(ConnectionsText.irohEndpoint("peer-id"), "Iroh 端点：peer-id")
        XCTAssertEqual(ConnectionsText.routeInterface("lan"), "LAN 路由")
        XCTAssertEqual(ConnectionsText.latency(42), "42 毫秒")
        XCTAssertEqual(ConnectionsText.deviceCount(1), "1 台设备")
        XCTAssertEqual(ConnectionsText.deviceCount(2), "2 台设备")
        XCTAssertEqual(
            ConnectionsText.pairingWindow(remaining: 120, failed: 1, maximum: 3),
            "剩余 120 秒 · 失败 1/3 次"
        )
    }
}
