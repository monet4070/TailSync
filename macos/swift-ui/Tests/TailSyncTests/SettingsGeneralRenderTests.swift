import AppKit
import SwiftUI
import XCTest
@testable import TailSync

final class SettingsGeneralRenderTests: XCTestCase {
    @MainActor
    func testRenderLaunchAtLoginSettingInChinese() throws {
        _ = NSApplication.shared
        let previousLanguage = Loc.shared.lang
        Loc.shared.lang = "zh-CN"
        defer { Loc.shared.lang = previousLanguage }

        let controller = LaunchAtLoginController(
            service: SettingsPreviewLaunchAtLoginService()
        )
        let settings = SettingsView(launchAtLogin: controller)
        let selection = TailSyncThemeSelection(builtin: .tailsync)
        let view = settings.generalSection
            .environment(\.colorScheme, .light)
            .environment(\.tailSyncSelection, selection)
            .environment(\.tailSyncPalette, selection.palette(for: .light))
            .frame(width: 480)
            .padding(12)
            .background(Color(nsColor: .windowBackgroundColor))

        let hosting = NSHostingView(rootView: view)
        hosting.frame = NSRect(x: 0, y: 0, width: 504, height: 420)
        hosting.layoutSubtreeIfNeeded()
        let height = max(240, hosting.fittingSize.height)
        hosting.frame = NSRect(x: 0, y: 0, width: 504, height: height)
        hosting.layoutSubtreeIfNeeded()
        let representation = try XCTUnwrap(
            hosting.bitmapImageRepForCachingDisplay(in: hosting.bounds)
        )
        hosting.cacheDisplay(in: hosting.bounds, to: representation)
        let png = try XCTUnwrap(
            representation.representation(using: .png, properties: [:])
        )
        let output = URL(fileURLWithPath: "/tmp/tailsync-settings-render", isDirectory: true)
        try FileManager.default.createDirectory(
            at: output,
            withIntermediateDirectories: true
        )
        let file = output.appendingPathComponent("launch-at-login-zh.png")
        try png.write(to: file)
        XCTAssertGreaterThan(png.count, 1_000)
    }
}

private final class SettingsPreviewLaunchAtLoginService: LaunchAtLoginServicing {
    var status: LaunchAtLoginRegistrationState = .notRegistered
    func register() throws { status = .enabled }
    func unregister() throws { status = .notRegistered }
    func openSystemSettings() {}
}
