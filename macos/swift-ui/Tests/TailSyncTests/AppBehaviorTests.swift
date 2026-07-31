import AppKit
import SwiftUI
import XCTest
@testable import TailSync

@MainActor
final class AppBehaviorTests: XCTestCase {
    func testInteractiveContentDoesNotMakeTheWindowDraggable() {
        let window = NSWindow()

        TailSyncWindowPolicy.configure(window)

        XCTAssertFalse(window.isMovableByWindowBackground)
    }

    func testChangingLanguagePublishesTheLocaleChangeNotification() {
        _ = NSApplication.shared
        let loc = Loc.shared
        let originalLanguage = loc.lang
        let targetLanguage = originalLanguage == "zh-CN" ? "en" : "zh-CN"
        let changed = expectation(
            forNotification: .tailSyncLocaleChanged,
            object: nil
        ) { _ in
            Loc.shared.lang == targetLanguage
        }

        loc.lang = targetLanguage
        wait(for: [changed], timeout: 0.25)
        loc.lang = originalLanguage
    }

    func testCanvasUsesTheWindowsSemanticColorTokens() throws {
        let light = TailSyncColorTheme.tailsync.palette(for: .light)
        let dark = TailSyncColorTheme.tailsync.palette(for: .dark)

        try assertColor(light.softSurfaceColor, hex: 0x1A1916, alpha: 0.045)
        try assertColor(light.primaryColor, hex: 0x191918)
        try assertColor(light.secondaryColor, hex: 0x68665F)
        try assertColor(light.tertiaryColor, hex: 0x98958B)
        try assertColor(light.borderColor, hex: 0xD3CEC2)
        try assertColor(light.dividerColor, hex: 0xECE8DF)
        try assertColor(light.toastColor, hex: 0x171716)

        try assertColor(dark.softSurfaceColor, hex: 0xFFFDF5, alpha: 0.055)
        try assertColor(dark.primaryColor, hex: 0xF4F1E9)
        try assertColor(dark.secondaryColor, hex: 0xAAA69C)
        try assertColor(dark.tertiaryColor, hex: 0x77746C)
        try assertColor(dark.borderColor, hex: 0x48463F)
        try assertColor(dark.dividerColor, hex: 0x2C2B28)
        try assertColor(dark.toastColor, hex: 0xF8F5ED)
    }

    func testCanvasUsesTheWindowsEditorialFontFamilies() {
        XCTAssertEqual(TailSyncColorTheme.tailsync.displayFontName, "Songti SC")
        XCTAssertNil(TailSyncColorTheme.tailsync.readingFontName)
    }

    private func assertColor(
        _ color: Color,
        hex: UInt32,
        alpha: CGFloat = 1,
        file: StaticString = #filePath,
        line: UInt = #line
    ) throws {
        try assertColor(
            color,
            red: CGFloat((hex >> 16) & 0xFF) / 255,
            green: CGFloat((hex >> 8) & 0xFF) / 255,
            blue: CGFloat(hex & 0xFF) / 255,
            alpha: alpha,
            file: file,
            line: line
        )
    }

    private func assertColor(
        _ color: Color,
        red: CGFloat,
        green: CGFloat,
        blue: CGFloat,
        alpha: CGFloat,
        file: StaticString = #filePath,
        line: UInt = #line
    ) throws {
        let resolved = try XCTUnwrap(NSColor(color).usingColorSpace(.sRGB), file: file, line: line)
        XCTAssertEqual(resolved.redComponent, red, accuracy: 0.005, file: file, line: line)
        XCTAssertEqual(resolved.greenComponent, green, accuracy: 0.005, file: file, line: line)
        XCTAssertEqual(resolved.blueComponent, blue, accuracy: 0.005, file: file, line: line)
        XCTAssertEqual(resolved.alphaComponent, alpha, accuracy: 0.005, file: file, line: line)
    }
}
