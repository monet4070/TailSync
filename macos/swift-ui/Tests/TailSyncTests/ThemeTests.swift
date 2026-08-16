import SwiftUI
import XCTest
@testable import TailSync

final class ThemeTests: XCTestCase {
    func testSettingsRoundTripPreservesColorTheme() throws {
        var settings = AppSettings()
        settings.color_theme = TailSyncColorTheme.rose.rawValue

        let data = try JSONEncoder().encode(settings)
        let object = try XCTUnwrap(JSONSerialization.jsonObject(with: data) as? [String: Any])
        XCTAssertEqual(object["color_theme"] as? String, "rose")

        let decoded = try JSONDecoder().decode(AppSettings.self, from: data)
        XCTAssertEqual(decoded.color_theme, "rose")
    }

    func testMissingAndUnknownColorThemesFallBackToCanvas() throws {
        let missing = try JSONDecoder().decode(AppSettings.self, from: Data("{}".utf8))
        XCTAssertEqual(missing.color_theme, TailSyncColorTheme.tailsync.rawValue)
        XCTAssertEqual(missing.sync_shortcut, "CommandOrControl+Shift+S")
        XCTAssertEqual(missing.history_shortcut, "CommandOrControl+Shift+H")

        let unknown = TailSyncColorTheme(storedValue: "not-a-theme")
        XCTAssertEqual(unknown, .tailsync)
    }

    func testThemeIdentifiersMatchTheRustContract() {
        XCTAssertEqual(
            Set(TailSyncColorTheme.allCases.map(\.rawValue)),
            Set(["tailsync", "ocean", "forest", "rose", "high-contrast"])
        )
    }

    func testEveryThemeHasDistinctLightAndDarkPalettes() {
        let light = Set(TailSyncColorTheme.allCases.map { $0.palette(for: .light).signature })
        let dark = Set(TailSyncColorTheme.allCases.map { $0.palette(for: .dark).signature })

        XCTAssertEqual(light.count, TailSyncColorTheme.allCases.count)
        XCTAssertEqual(dark.count, TailSyncColorTheme.allCases.count)
    }

    func testKeyPaletteTokensMatchTheWindowsDesignSystem() {
        let expected: [TailSyncColorTheme: (lightAccent: UInt32, darkAccent: UInt32,
                                             lightWindow: UInt32, darkWindow: UInt32)] = [
            .tailsync: (0xD5684B, 0xEC8668, 0xFAF9F5, 0x191918),
            .ocean: (0x087F8C, 0x4CC9C0, 0xF1F7F7, 0x111E20),
            .forest: (0x287A55, 0x69CA91, 0xF3F7F3, 0x151F18),
            .rose: (0xB83E60, 0xEF86A2, 0xFAF4F6, 0x24191D),
            .highContrast: (0x005FCC, 0xFFD400, 0xFFFFFF, 0x000000),
        ]

        for theme in TailSyncColorTheme.allCases {
            let tokens = expected[theme]
            XCTAssertEqual(theme.palette(for: .light).accent, tokens?.lightAccent)
            XCTAssertEqual(theme.palette(for: .dark).accent, tokens?.darkAccent)
            XCTAssertEqual(theme.palette(for: .light).window, tokens?.lightWindow)
            XCTAssertEqual(theme.palette(for: .dark).window, tokens?.darkWindow)
        }
    }

    func testCanvasUsesTheWindowsEditorialMetrics() {
        let canvas = TailSyncColorTheme.tailsync

        XCTAssertEqual(canvas.metrics.cardRadius, 10)
        XCTAssertEqual(canvas.metrics.controlRadius, 9)
        XCTAssertEqual(canvas.metrics.rowPadding, 13)
        XCTAssertEqual(canvas.typography.sectionTitleSize, 25)
        XCTAssertFalse(canvas.typography.uppercasesSectionTitles)
        XCTAssertEqual(canvas.typography.searchSize, 18)
        XCTAssertTrue(canvas.typography.searchUsesDisplayFont)
        XCTAssertEqual(canvas.typography.historyContentSize, 15)
    }
}
