import AppKit
import SwiftUI
import XCTest
@testable import TailSync

/// Snapshots the redesigned settings sections (descriptions, keycap badges,
/// collapsed remote pairing drawer) for visual review in both languages.
final class SettingsUIPreviewTests: XCTestCase {
    @MainActor
    @discardableResult
    private func render(
        name: String,
        lang: String,
        scheme: ColorScheme,
        makeView: @MainActor () -> some View
    ) throws -> CGFloat {
        _ = NSApplication.shared
        let previousLanguage = Loc.shared.lang
        Loc.shared.lang = lang
        defer { Loc.shared.lang = previousLanguage }

        let selection = TailSyncThemeSelection(builtin: .tailsync)
        let view = makeView()
            .environment(\.colorScheme, scheme)
            .environment(\.tailSyncSelection, selection)
            .environment(\.tailSyncPalette, selection.palette(for: scheme))
            .frame(width: 480)
            .padding(12)
            .background(Color(nsColor: .windowBackgroundColor))

        let hosting = NSHostingView(rootView: view)
        hosting.frame = NSRect(x: 0, y: 0, width: 504, height: 900)
        hosting.layoutSubtreeIfNeeded()
        let contentHeight = hosting.fittingSize.height
        let height = max(240, contentHeight)
        hosting.frame = NSRect(x: 0, y: 0, width: 504, height: height)
        hosting.layoutSubtreeIfNeeded()
        let rep = try XCTUnwrap(hosting.bitmapImageRepForCachingDisplay(in: hosting.bounds))
        hosting.cacheDisplay(in: hosting.bounds, to: rep)
        let png = try XCTUnwrap(rep.representation(using: .png, properties: [:]))
        let output = URL(fileURLWithPath: "/tmp/tailsync-settings-render", isDirectory: true)
        try FileManager.default.createDirectory(at: output, withIntermediateDirectories: true)
        try png.write(to: output.appendingPathComponent(name))
        XCTAssertGreaterThan(png.count, 1_000)
        return contentHeight
    }

    @MainActor
    func testPreviewGeneralSection() throws {
        try render(name: "general-zh-light.png", lang: "zh-CN", scheme: .light) {
            SettingsView().generalSection
        }
        try render(name: "general-en-light.png", lang: "en", scheme: .light) {
            SettingsView().generalSection
        }
        try render(name: "general-zh-dark.png", lang: "zh-CN", scheme: .dark) {
            SettingsView().generalSection
        }
    }

    @MainActor
    func testPreviewNetworkSection() throws {
        try render(name: "network-zh-light.png", lang: "zh-CN", scheme: .light) {
            SettingsView().networkSection
        }
        try render(name: "network-lan-zh-light.png", lang: "zh-CN", scheme: .light) {
            let view = SettingsView()
            view.settings.connection_mode = "lan_only"
            return view.networkSection
        }
        try render(name: "network-en-light.png", lang: "en", scheme: .light) {
            SettingsView().networkSection
        }
        try render(name: "network-zh-dark.png", lang: "zh-CN", scheme: .dark) {
            SettingsView().networkSection
        }
    }

    @MainActor
    func testPreviewConnectionsView() throws {
        let collapsedHeight = try render(name: "connections-zh-light.png", lang: "zh-CN", scheme: .light) {
            ConnectionsView().connectionsCard
        }
        try render(name: "connections-en-light.png", lang: "en", scheme: .light) {
            ConnectionsView().connectionsCard
        }
        try render(name: "connections-zh-dark.png", lang: "zh-CN", scheme: .dark) {
            ConnectionsView().connectionsCard
        }
        let expandedHeight = try render(name: "connections-expanded-zh-light.png", lang: "zh-CN", scheme: .light) {
            ConnectionsView(preview: .init(remotePairingExpanded: true)).connectionsCard
        }
        XCTAssertGreaterThan(expandedHeight, collapsedHeight)
        try render(name: "connections-with-invite-zh-light.png", lang: "zh-CN", scheme: .light) {
            ConnectionsView(preview: .init(
                remotePairingExpanded: true,
                remoteInvite: ApiClient.RemotePairingInvite(
                    link: "tailsync://pair/v1/mock-invitation-token-12345",
                    expires_at: 1800000000,
                    remaining_seconds: 240
                )
            )).connectionsCard
        }
    }
}
