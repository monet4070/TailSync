import Foundation
import XCTest
@testable import TailSync

final class ConfigurationIsolationTests: XCTestCase {
    func testIsolatedGUIReadsTheSameRootAsDaemon() {
        let root = "/tmp/tailsync-native-acceptance/data"
        let url = Loc.configurationURL(
            environment: ["TAILSYNC_DATA_DIR": root],
            applicationSupport: URL(fileURLWithPath: "/Users/example/Library/Application Support")
        )
        XCTAssertEqual(url.path, root + "/config-v2.json")
        XCTAssertFalse(url.path.contains("/Users/example/"))
    }

    func testNormalLaunchKeepsTheExistingConfigurationPath() {
        let support = URL(fileURLWithPath: "/Users/example/Library/Application Support")
        XCTAssertEqual(
            Loc.configurationURL(environment: [:], applicationSupport: support).path,
            support.appendingPathComponent("com.tailsync.TailSync/config-v2.json").path
        )
    }
}
