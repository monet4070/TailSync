import AppKit
import SwiftUI
import XCTest
@testable import TailSync

/// Snapshots the redesigned settings sections (descriptions, keycap badges,
/// collapsed remote pairing drawer) for visual review in both languages.
final class SettingsUIPreviewTests: XCTestCase {
    @MainActor
    private func render(
        name: String,
        lang: String,
        scheme: ColorScheme,
        makeView: @MainActor () -> some View
    ) throws {
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
        let height = max(240, hosting.fittingSize.height)
        hosting.frame = NSRect(x: 0, y: 0, width: 504, height: height)
        hosting.layoutSubtreeIfNeeded()
        let rep = try XCTUnwrap(hosting.bitmapImageRepForCachingDisplay(in: hosting.bounds))
        hosting.cacheDisplay(in: hosting.bounds, to: rep)
        let png = try XCTUnwrap(rep.representation(using: .png, properties: [:]))
        let output = URL(fileURLWithPath: "/tmp/tailsync-settings-render", isDirectory: true)
        try FileManager.default.createDirectory(at: output, withIntermediateDirectories: true)
        try png.write(to: output.appendingPathComponent(name))
        XCTAssertGreaterThan(png.count, 1_000)
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
        try render(name: "network-en-light.png", lang: "en", scheme: .light) {
            SettingsView().networkSection
        }
        try render(name: "network-zh-dark.png", lang: "zh-CN", scheme: .dark) {
            SettingsView().networkSection
        }
    }
}
