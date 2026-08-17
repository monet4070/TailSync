import AppKit
import SwiftUI
import XCTest
@testable import TailSync

/// R001/R002 regression tests: the themed window background must be visible
/// behind History/Settings content, and the background loader must be free
/// of stale-result races. All timing is driven by checked continuations —
/// no sleeps, no real daemon.
///
/// Deliberately contains NO @MainActor annotations: their presence in any
/// test class of this bundle wedges the XCTest async runner (a plain async
/// test in another class never gets scheduled). Main-actor work is wrapped
/// in MainActor.run instead.
final class ThemeBackgroundTests: XCTestCase {
    /// Deterministic fetch gate: each call parks on a continuation the test
    /// resumes in the order it chooses.
    private final class FetchGate {
        var calls: [(id: String, mode: String)] = []
        private var pending: [CheckedContinuation<ApiClient.ThemeBackgroundPayload?, Never>] = []

        func fetch(_ id: String, _ mode: String) async -> ApiClient.ThemeBackgroundPayload? {
            calls.append((id, mode))
            return await withCheckedContinuation { pending.append($0) }
        }

        func resume(_ payload: ApiClient.ThemeBackgroundPayload?) {
            pending.removeFirst().resume(returning: payload)
        }

        /// Resume the most recently parked request (the "latest" one),
        /// leaving older requests parked for a later decision.
        func resumeLatest(_ payload: ApiClient.ThemeBackgroundPayload?) {
            pending.removeLast().resume(returning: payload)
        }

        var pendingCount: Int { pending.count }
    }

    /// Computed (not stored) so Loc.shared is only touched inside test
    /// bodies — touching it at class-instantiation time wedges the XCTest
    /// runner's main queue in this environment.
    private var loc: Loc { Loc.shared }
    private var gate: FetchGate!
    private let pngB64 =
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNgAAH//wMAAf8CAqU9iVkAAAAASUVORK5CYII="

    override func setUp() {
        _ = NSApplication.shared  // Loc.reload() -> applyTheme() touches NSApp
        gate = FetchGate()
        loc.backgroundFetch = { [weak self] id, mode in
            guard let self, let gate = self.gate else { return nil }
            return await gate.fetch(id, mode)
        }
        loc.themeBackgroundImage = nil
        loc.themeBackgroundScrim = nil
    }

    override func tearDown() {
        loc.backgroundFetch = { themeId, mode in
            await ApiClient.shared.getThemeBackground(themeId: themeId, mode: mode)
        }
        loc.themeBackgroundImage = nil
        loc.themeBackgroundScrim = nil
    }

    /// Deterministically wait until `count` fetch calls are parked on the
    /// gate (no sleeps).
    private func waitForPending(_ count: Int) async {
        var spins = 0
        while gate.pendingCount < count {
            spins += 1
            if spins > 10_000 { fatalError("gate never parked (got \(gate.pendingCount))") }
            await Task.yield()
        }
    }

    /// Compare a scrim colour numerically (the published scrim is built from
    /// a validated hex + opacity; its `description` format differs from a
    /// hand-built Color even for identical sRGB values).
    private func assertScrim(
        _ color: Color?,
        hex: UInt32,
        opacity: Double,
        file: StaticString = #filePath,
        line: UInt = #line
    ) {
        guard let color else {
            XCTFail("scrim missing", file: file, line: line)
            return
        }
        guard let rgb = NSColor(color).usingColorSpace(.sRGB) else {
            XCTFail("scrim has no sRGB conversion", file: file, line: line)
            return
        }
        let value = (UInt32((rgb.redComponent * 255).rounded()) << 16)
            | (UInt32((rgb.greenComponent * 255).rounded()) << 8)
            | UInt32((rgb.blueComponent * 255).rounded())
        XCTAssertEqual(value, hex, file: file, line: line)
        XCTAssertEqual(rgb.alphaComponent, opacity, accuracy: 0.01, file: file, line: line)
    }

    private func payload() -> ApiClient.ThemeBackgroundPayload {
        ApiClient.ThemeBackgroundPayload(
            mimeType: "image/png",
            data: Data(base64Encoded: pngB64)!)
    }

    /// Build a custom selection whose definition carries background metadata
    /// with distinct scrim colours per mode (scrim marks which request won).
    private func selection(lightScrim: String, darkScrim: String) -> TailSyncThemeSelection {
        let json = """
        { "id": "studio", "name": { "en": "Studio" },
          "palette": { "light": { "brand": "#d5684b", "brandText": "#ffffff",
            "bgWindow": "#faf9f5", "bgCard": "#fffefa", "bgInput": "#1a1916",
            "bgRaised": "#fffefa", "textPrimary": "#191918", "textSecondary": "#68665f",
            "textTertiary": "#98958b", "borderStrong": "#d3cec2", "divider": "#ece8df",
            "green": "#44745a", "orange": "#b96536", "bgToast": "#171716", "textToast": "#ffffff" },
            "dark": { "brand": "#ec8668", "brandText": "#181412", "bgWindow": "#191918",
            "bgCard": "#232321", "bgInput": "#fffdf5", "bgRaised": "#262522",
            "textPrimary": "#f4f1e9", "textSecondary": "#aaa69c", "textTertiary": "#77746c",
            "borderStrong": "#48463f", "divider": "#2c2b28", "green": "#75aa86",
            "orange": "#dc9163", "bgToast": "#f8f5ed", "textToast": "#171716" } },
          "metrics": { "cardRadius": 10, "controlRadius": 9, "rowPadding": 13, "shadowRadius": 8 },
          "typography": { "sectionTitleSize": 25, "uppercasesSectionTitles": false,
            "searchSize": 18, "searchUsesDisplayFont": true, "historyContentSize": 15 },
          "fonts": { "display": null, "reading": null },
          "background": {
            "light": { "hasImage": true, "scrim": { "hex": "\(lightScrim)", "opacity": 0.8 }, "mimeType": "image/png" },
            "dark": { "hasImage": true, "scrim": { "hex": "\(darkScrim)", "opacity": 0.8 }, "mimeType": "image/png" }
          } }
        """
        let definition = try! JSONDecoder().decode(
            TailSyncThemeDefinition.self, from: Data(json.utf8))
        return TailSyncThemeSelection(builtin: .tailsync, definition: definition)
    }

    // ── R002 race scenarios ────────────────────────────────────────────

    func testInFlightRequestCannotResurrectBackgroundAfterBuiltinSwitch() async {
        let selectionA = selection(lightScrim: "#aa0000", darkScrim: "#aa0000")
        let taskA = Task { await loc.loadThemeBackground(for: selectionA, light: true) }
        await waitForPending(1)
        await loc.loadThemeBackground(for: TailSyncThemeSelection(builtin: .tailsync), light: true)
        XCTAssertNil(loc.themeBackgroundImage)
        XCTAssertNil(loc.themeBackgroundScrim)

        // A's result arrives late — it must not resurrect the background.
        gate.resume(payload())
        await taskA.value
        XCTAssertNil(loc.themeBackgroundImage, "late A result must not publish")
        XCTAssertNil(loc.themeBackgroundScrim)
    }

    func testOutOfOrderRequestsPublishOnlyTheLatest() async {
        let selectionA = selection(lightScrim: "#aa0000", darkScrim: "#aa0000")
        let selectionB = selection(lightScrim: "#0000aa", darkScrim: "#0000aa")
        let taskA = Task { await loc.loadThemeBackground(for: selectionA, light: true) }
        await waitForPending(1)
        let taskB = Task { await loc.loadThemeBackground(for: selectionB, light: true) }
        await waitForPending(2)

        // B (the latest request) completes first and publishes.
        gate.resumeLatest(payload())
        await taskB.value
        XCTAssertNotNil(loc.themeBackgroundImage)
        assertScrim(loc.themeBackgroundScrim, hex: 0x0000AA, opacity: 0.8)

        // A completes late — must be dropped.
        gate.resume(payload())
        await taskA.value
        XCTAssertNotNil(loc.themeBackgroundImage)
        assertScrim(loc.themeBackgroundScrim, hex: 0x0000AA, opacity: 0.8)
    }

    func testFailedLatestRequestClearsPreviousBackground() async {
        let selectionA = selection(lightScrim: "#aa0000", darkScrim: "#aa0000")
        let selectionB = selection(lightScrim: "#0000aa", darkScrim: "#0000aa")

        let taskA = Task { await loc.loadThemeBackground(for: selectionA, light: true) }
        await waitForPending(1)
        gate.resume(payload())
        await taskA.value
        XCTAssertNotNil(loc.themeBackgroundImage, "A published its background")

        // B starts and fails (nil payload) — A's image must be cleared.
        let taskB = Task { await loc.loadThemeBackground(for: selectionB, light: true) }
        await waitForPending(1)
        gate.resume(nil)
        await taskB.value
        XCTAssertNil(loc.themeBackgroundImage, "failed latest request must clear the stale image")
        XCTAssertNil(loc.themeBackgroundScrim)

        // Decode failure of the latest request also clears.
        let taskC = Task { await loc.loadThemeBackground(for: selectionA, light: true) }
        await waitForPending(1)
        gate.resume(ApiClient.ThemeBackgroundPayload(mimeType: "image/png", data: Data("garbage".utf8)))
        await taskC.value
        XCTAssertNil(loc.themeBackgroundImage, "decode failure must clear the stale image")
        XCTAssertNil(loc.themeBackgroundScrim)
    }

    func testModeSwitchOldModeResultCannotOverwriteNewMode() async {
        let selectionA = selection(lightScrim: "#aa0000", darkScrim: "#0000aa")
        let taskLight = Task { await loc.loadThemeBackground(for: selectionA, light: true) }
        await waitForPending(1)
        let taskDark = Task { await loc.loadThemeBackground(for: selectionA, light: false) }
        await waitForPending(2)

        // The dark request (the latest) completes first and publishes its
        // own scrim.
        gate.resumeLatest(payload())
        await taskDark.value
        assertScrim(loc.themeBackgroundScrim, hex: 0x0000AA, opacity: 0.8)

        // The stale light-mode request completes late — must be dropped.
        gate.resume(payload())
        await taskLight.value
        assertScrim(loc.themeBackgroundScrim, hex: 0x0000AA, opacity: 0.8)
    }

    // ── R001 rendering/composition regressions ─────────────────────────

    func testWindowBackgroundRendersImageVisibly() async throws {
        let renderedData = await Task { @MainActor in
            let image = NSImage(size: NSSize(width: 40, height: 30))
            image.lockFocus()
            NSColor.red.setFill()
            NSRect(x: 0, y: 0, width: 40, height: 30).fill()
            image.unlockFocus()
            let view = TailSyncWindowBackground(
                palette: TailSyncColorTheme.tailsync.palette(for: .light),
                image: image,
                scrim: nil
            )
                .frame(width: 120, height: 90)
            let renderer = ImageRenderer(content: view)
            renderer.scale = 1
            return renderer.nsImage?.tiffRepresentation
        }.value
        let rep = try XCTUnwrap(NSBitmapImageRep(data: try XCTUnwrap(renderedData)))

        var redPixels = 0
        for y in 0..<rep.pixelsHigh {
            for x in 0..<rep.pixelsWide {
                if let color = rep.colorAt(x: x, y: y),
                   color.redComponent > 0.8,
                   color.greenComponent < 0.2,
                   color.blueComponent < 0.2 {
                    redPixels += 1
                }
            }
        }
        XCTAssertGreaterThan(redPixels, 1000, "background image must be visible behind content")
    }

    func testHistoryViewDoesNotCoverThemedWindowBackground() throws {
        let sourceURL = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .appendingPathComponent("Sources/TailSync/Views/HistoryView.swift")
        let source = try String(contentsOf: sourceURL, encoding: .utf8)
        // The window background is owned by tailSyncThemed(). An opaque root
        // background painted by the view would sit in front of the themed
        // image and hide it — these exact patterns existed before the fix.
        XCTAssertFalse(
            source.contains("scrollContentBackground(.hidden)\n                .background(palette.windowColor)"),
            "HistoryView list must not paint an opaque window background over the themed layer")
        XCTAssertFalse(
            source.contains("\n        .background(palette.windowColor)\n        .tailSyncThemed()"),
            "HistoryView root must not paint an opaque window background over the themed layer")
    }
}
