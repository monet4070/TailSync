import Foundation
import XCTest
@testable import TailSync

@MainActor
final class ThirdPartyImageAccessTests: XCTestCase {
    func testSwitchStaysOffUntilSystemGrantAndCanBeRevoked() async {
        let helper = ImageAccessHelperStub([.notAuthorized, .granted, .revoked])
        let setting = ThirdPartyImageAccessSettings { await helper.request($0) }
        XCTAssertFalse(setting.isEnabled)
        await setting.load()
        XCTAssertFalse(setting.isEnabled)
        await setting.setEnabled(true, language: "zh-CN")
        XCTAssertTrue(setting.isEnabled)
        await setting.setEnabled(false, language: "zh-CN")
        XCTAssertFalse(setting.isEnabled)
        XCTAssertFalse(setting.isBusy)
        let commands = await helper.commands
        XCTAssertEqual(commands, [
            ["--qq-image-access-status"], ["--grant-qq-image-access", "zh-CN"],
            ["--revoke-qq-image-access"]
        ])
    }

    func testCancelledOrMissingSourceGrantDoesNotEnableTheSwitch() async {
        let helper = ImageAccessHelperStub([.cancelled, .copyMessageFirst, .unavailable])
        let setting = ThirdPartyImageAccessSettings { await helper.request($0) }
        await setting.setEnabled(true, language: "en-US")
        XCTAssertFalse(setting.isEnabled)
        await setting.setEnabled(true, language: "en-US")
        XCTAssertFalse(setting.isEnabled)
        XCTAssertEqual(setting.messageKey, "settings.qqImageAccessCopyFirst")
        await setting.setEnabled(true, language: "en-US")
        XCTAssertFalse(setting.isEnabled)
        XCTAssertEqual(setting.messageKey, "settings.qqImageAccessFailed")
        let commands = await helper.commands
        XCTAssertTrue(commands.allSatisfy { $0 == ["--grant-qq-image-access", "en"] })
    }

    func testExistingGrantLoadsAsOnAndFailedRevocationKeepsItOn() async {
        let helper = ImageAccessHelperStub([.authorized, .unavailable])
        let setting = ThirdPartyImageAccessSettings { await helper.request($0) }
        await setting.load()
        XCTAssertTrue(setting.isEnabled)
        await setting.setEnabled(false, language: "en")
        XCTAssertTrue(setting.isEnabled)
        XCTAssertEqual(setting.messageKey, "settings.qqImageAccessFailed")
    }

    func testPermissionMessagesExplainAutomaticLocationInBothLanguages() {
        let loc = Loc.shared
        let originalLanguage = loc.lang
        defer { loc.lang = originalLanguage }
        for language in ["en", "zh-CN"] {
            loc.lang = language
            for key in ["settings.qqImageAccess", "settings.qqImageAccessDescription",
                        "settings.qqImageAccessCopyFirst", "settings.qqImageAccessDisabled",
                        "settings.thirdPartyImages", "settings.thirdPartyImagesDescription",
                        "settings.wechatImageAccess", "settings.wechatImageAccessDescription",
                        "settings.imageAccessAutomatic"] {
                XCTAssertNotEqual(Loc.t(key), key)
            }
        }
    }

    func testRevokedBookmarkRefreshClearsTheOldEnabledMessage() async {
        let helper = ImageAccessHelperStub([.authorized, .notAuthorized])
        let setting = ThirdPartyImageAccessSettings { await helper.request($0) }
        await setting.load()
        XCTAssertTrue(setting.isEnabled)
        XCTAssertEqual(setting.messageKey, "settings.qqImageAccessReady")
        await setting.load()
        XCTAssertFalse(setting.isEnabled)
        XCTAssertNil(setting.messageKey)
    }
}

private actor ImageAccessHelperStub {
    private var results: [QQImageCacheAccessController.Result]
    private(set) var commands: [[String]] = []
    init(_ results: [QQImageCacheAccessController.Result]) { self.results = results }
    func request(_ arguments: [String]) -> QQImageCacheAccessController.Result {
        commands.append(arguments)
        return results.removeFirst()
    }
}
