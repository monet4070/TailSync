import AppKit
import WebKit
import XCTest
@testable import TailSync

final class HistoryMarkdownWebTests: XCTestCase {
    @MainActor
    func testNativeTablesMathWidthsAndAppearanceUpdates() async throws {
        guard ProcessInfo.processInfo.environment["TAILSYNC_MARKDOWN_WEB_TESTS"] == "1" else {
            throw XCTSkip("Opt-in real WebKit layout and formula acceptance")
        }
        _ = NSApplication.shared
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
            .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
            .deletingLastPathComponent()
        let source = try String(contentsOf: root.appendingPathComponent("shared/markdown-preview/acceptance.md"), encoding: .utf8)
        let renderer = HistoryMarkdownWebRenderer()
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 780, height: 860),
                              styleMask: [.titled, .resizable], backing: .buffered, defer: false)
        window.title = "TailSync Markdown acceptance"
        window.contentView = renderer.webView
        window.orderFront(nil)
        defer { renderer.cancel(); window.orderOut(nil) }
        let palette = TailSyncColorTheme.tailsync.definition.lightPalette
        var appearance = HistoryMarkdownWebRenderer.appearance(palette: palette, fontSize: 18)
        renderer.update(source: source, appearance: appearance)
        try await waitForRender(renderer)
        var stats = try await inspect(renderer)
        stats["renderDurationMs"] = renderer.lastRender?["durationMs"]
        XCTAssertEqual(stats["rows"] as? Int, 3)
        XCTAssertEqual(stats["columns"] as? [Int], [4, 4, 4])
        XCTAssertEqual(stats["escapedPipe"] as? Bool, true)
        XCTAssertEqual(stats["rightAligned"] as? Bool, true)
        XCTAssertEqual(stats["displayMath"] as? Int, 2)
        XCTAssertGreaterThan(stats["math"] as? Int ?? 0, 6)
        XCTAssertEqual(stats["codeLiteral"] as? Bool, true)
        XCTAssertEqual(stats["pageOverflow"] as? Bool, false)
        XCTAssertEqual(stats["fontsLoaded"] as? Bool, true)
        let initial = renderer.lastRender?["renders"] as? Int
        appearance["fontSize"] = 24
        renderer.update(source: source, appearance: appearance)
        let zoom = try await inspect(renderer)
        XCTAssertEqual(zoom["fontSize"] as? String, "24px")
        XCTAssertEqual(renderer.lastRender?["renders"] as? Int, initial)

        var benchmarks: [[String: Any]] = []
        for (name, large) in [
            ("prose-180k", String(repeating: "### Section\n\nReadable text with **emphasis** and `code`.\n\n", count: 3000)),
            ("table-500-rows", "|A|B|C|\n|---|---|---|\n" + String(repeating: "|中文说明|Long cell content|42|\n", count: 500)),
            ("math-128", String(repeating: "$\\frac{x^2+1}{\\sqrt{y}}$\n\n", count: 128))
        ] {
            renderer.update(source: large, appearance: appearance)
            try await waitForRender(renderer)
            benchmarks.append(["name": name, "bytes": large.utf8.count,
                               "durationMs": renderer.lastRender?["durationMs"] ?? -1,
                               "limited": renderer.lastRender?["truncated"] ?? true])
        }
        renderer.update(source: source, appearance: appearance)
        try await waitForRender(renderer)

        renderer.webView.frame.size = NSSize(width: 430, height: 860)
        let narrow = try await inspect(renderer)
        XCTAssertEqual(narrow["pageOverflow"] as? Bool, false)
        XCTAssertGreaterThan(narrow["tableScrollWidth"] as? Double ?? 0, narrow["tableClientWidth"] as? Double ?? 0)
        let unsafe = "<script>window.pwned=1</script><img src='https://bad.test/x'>\n\n$\\includegraphics{https://bad.test/x}$"
        renderer.update(source: unsafe, appearance: appearance)
        try await waitForRender(renderer)
        let active = try await evaluate(renderer,
            "return document.querySelectorAll('script,img,iframe,object,[onclick]').length",
            arguments: [:]) as? Int
        XCTAssertEqual(active, 0)

        if let path = ProcessInfo.processInfo.environment["TAILSYNC_MARKDOWN_EVIDENCE_DIR"] {
            renderer.update(source: source, appearance: HistoryMarkdownWebRenderer.appearance(palette: palette, fontSize: 18))
            renderer.webView.frame.size = NSSize(width: 780, height: 860)
            try await waitForRender(renderer)
            _ = try await inspect(renderer)
            let image = try await renderer.webView.takeSnapshot(configuration: nil)
            let png = try XCTUnwrap(NSBitmapImageRep(data: try XCTUnwrap(image.tiffRepresentation))?.representation(using: .png, properties: [:]))
            let output = URL(fileURLWithPath: path)
            try FileManager.default.createDirectory(at: output, withIntermediateDirectories: true)
            try png.write(to: output.appendingPathComponent("markdown-native.png"))
            try JSONSerialization.data(withJSONObject: stats, options: [.prettyPrinted, .sortedKeys])
                .write(to: output.appendingPathComponent("native-layout.json"))
            try JSONSerialization.data(withJSONObject: benchmarks, options: [.prettyPrinted, .sortedKeys])
                .write(to: output.appendingPathComponent("native-performance.json"))
            renderer.update(source: source, appearance: HistoryMarkdownWebRenderer.appearance(
                palette: TailSyncColorTheme.tailsync.definition.darkPalette, fontSize: 18))
            _ = try await inspect(renderer)
            let dark = try await renderer.webView.takeSnapshot(configuration: nil)
            let darkPNG = try XCTUnwrap(NSBitmapImageRep(data: try XCTUnwrap(dark.tiffRepresentation))?.representation(using: .png, properties: [:]))
            try darkPNG.write(to: output.appendingPathComponent("markdown-native-dark.png"))
        }
    }

    @MainActor
    private func waitForRender(_ renderer: HistoryMarkdownWebRenderer) async throws {
        for _ in 0..<400 {
            if renderer.lastRender != nil { return }
            try await Task.sleep(nanoseconds: 20_000_000)
        }
        XCTFail("Markdown WebKit preparation did not complete")
        throw NSError(domain: "MarkdownAcceptance", code: 1)
    }

    @MainActor
    private func inspect(_ renderer: HistoryMarkdownWebRenderer) async throws -> [String: Any] {
        let script = """
        await Promise.race([document.fonts.ready, new Promise(resolve => setTimeout(resolve, 1500))]);
        const article = document.getElementById('article');
        const table = article.querySelector('.md-table-scroll');
        return {rows: article.querySelectorAll('tbody tr').length,
          columns: [...article.querySelectorAll('tbody tr')].map(row=>row.children.length),
          escapedPipe: article.querySelector('tbody tr:nth-child(2) td:nth-child(2)')?.textContent.includes('a|b'),
          rightAligned: getComputedStyle(article.querySelector('th:nth-child(3)')).textAlign==='right',
          math: article.querySelectorAll('.katex').length, displayMath: article.querySelectorAll('.md-math-display').length,
          codeLiteral: article.querySelector('pre code')?.textContent.includes('$x^2$'),
          pageOverflow: document.documentElement.scrollWidth > innerWidth + 1,
          tableScrollWidth: table?.scrollWidth, tableClientWidth: table?.clientWidth,
          fontsLoaded: document.fonts.check('16px KaTeX_Main'), fontSize: getComputedStyle(article).fontSize};
        """
        let value = try await evaluate(renderer, script, arguments: [:])
        return try XCTUnwrap(value as? [String: Any])
    }

    @MainActor
    private func evaluate(_ renderer: HistoryMarkdownWebRenderer, _ script: String, arguments: [String: Any]) async throws -> Any {
        try await withCheckedThrowingContinuation { continuation in
            var finished = false
            Task { @MainActor in
                try? await Task.sleep(nanoseconds: 4_000_000_000)
                if !finished {
                    finished = true
                    continuation.resume(throwing: NSError(domain: "MarkdownEvaluationTimeout", code: 1))
                }
            }
            renderer.webView.callAsyncJavaScript(script, arguments: arguments, in: nil, in: .defaultClient) {
                guard !finished else { return }
                finished = true; continuation.resume(with: $0)
            }
        }
    }
}
