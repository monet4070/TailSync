import AppKit
import SwiftUI
import XCTest
@testable import TailSync

final class SettingsErrorRenderTests: XCTestCase {
    @MainActor
    func testProtocolErrorToastRendersInBothLanguages() throws {
        _ = NSApplication.shared
        let previousLanguage = Loc.shared.lang
        defer { Loc.shared.lang = previousLanguage }
        let output = URL(
            fileURLWithPath: ProcessInfo.processInfo.environment["TAILSYNC_ACCEPTANCE_OUTPUT_DIR"]
                ?? "/tmp/tailsync-settings-error-render",
            isDirectory: true
        )
        try FileManager.default.createDirectory(at: output, withIntermediateDirectories: true)

        for language in ["en", "zh-CN"] {
            Loc.shared.lang = language
            let settings = SettingsView(
                launchAtLogin: LaunchAtLoginController(service: AcceptanceLaunchAtLoginService())
            )
            let selection = TailSyncThemeSelection(builtin: .tailsync)
            let view = settings.toast(message: Loc.t("error.protocol_incompatible"))
                .environment(\.colorScheme, .light)
                .environment(\.tailSyncSelection, selection)
                .environment(\.tailSyncPalette, selection.palette(for: .light))
                .frame(width: 600, height: 120)
                .background(Color(nsColor: .windowBackgroundColor))

            let hosting = NSHostingView(rootView: view)
            hosting.frame = NSRect(x: 0, y: 0, width: 600, height: 120)
            hosting.layoutSubtreeIfNeeded()
            let representation = try XCTUnwrap(
                hosting.bitmapImageRepForCachingDisplay(in: hosting.bounds)
            )
            hosting.cacheDisplay(in: hosting.bounds, to: representation)
            let png = try XCTUnwrap(representation.representation(using: .png, properties: [:]))
            let file = output.appendingPathComponent("protocol-error-\(language).png")
            try png.write(to: file)
            XCTAssertGreaterThan(png.count, 1_000, language)
        }
    }

    @MainActor
    func testIncompatiblePeerRowRendersUpgradeNoticeInBothLanguages() throws {
        _ = NSApplication.shared
        let previousLanguage = Loc.shared.lang
        defer { Loc.shared.lang = previousLanguage }
        let output = URL(
            fileURLWithPath: ProcessInfo.processInfo.environment["TAILSYNC_ACCEPTANCE_OUTPUT_DIR"]
                ?? "/tmp/tailsync-settings-error-render",
            isDirectory: true
        )
        try FileManager.default.createDirectory(at: output, withIntermediateDirectories: true)
        let peer = try JSONDecoder().decode(
            ApiClient.PeerSnapshot.self,
            from: Data(
                #"{"hostname":"incompatible-test","trusted":true,"enabled":true,"status":"offline","required_protocol_version":5,"protocol_error":"private diagnostic"}"#.utf8
            )
        )
        let selection = TailSyncThemeSelection(builtin: .tailsync)

        for language in ["en", "zh-CN"] {
            Loc.shared.lang = language
            let settings = SettingsView(
                launchAtLogin: LaunchAtLoginController(service: AcceptanceLaunchAtLoginService())
            )
            let view = settings.peerRow(peer)
                .environment(\.colorScheme, .light)
                .environment(\.tailSyncSelection, selection)
                .environment(\.tailSyncPalette, selection.palette(for: .light))
                .frame(width: 600, height: 160)
                .background(Color(nsColor: .windowBackgroundColor))
            let hosting = NSHostingView(rootView: view)
            hosting.frame = NSRect(x: 0, y: 0, width: 600, height: 160)
            hosting.layoutSubtreeIfNeeded()
            let representation = try XCTUnwrap(
                hosting.bitmapImageRepForCachingDisplay(in: hosting.bounds)
            )
            hosting.cacheDisplay(in: hosting.bounds, to: representation)
            let png = try XCTUnwrap(representation.representation(using: .png, properties: [:]))
            try png.write(to: output.appendingPathComponent("protocol-peer-\(language).png"))
            XCTAssertGreaterThan(png.count, 1_000, language)
        }
    }
}

private final class AcceptanceLaunchAtLoginService: LaunchAtLoginServicing {
    var status: LaunchAtLoginRegistrationState = .notRegistered
    func register() throws { status = .enabled }
    func unregister() throws { status = .notRegistered }
    func openSystemSettings() {}
}
