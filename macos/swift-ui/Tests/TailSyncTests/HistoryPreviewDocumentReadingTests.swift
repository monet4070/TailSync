import AppKit
import SwiftUI
import XCTest
@testable import TailSync

@MainActor
final class HistoryPreviewDocumentReadingTests: XCTestCase {
    func testTextSizePreservesHeadingRatiosLinksTablesAndImageAttachments() throws {
        let source = fixture()
        let original = NSAttributedString(attributedString: source)
        let options = HistoryPreviewDocumentReadingOptions(fontSize: 26, lineSpacing: .relaxed)
        let resized = HistoryPreviewDocumentReadingStyle.attributed(source, options: options)
        let body = (source.string as NSString).range(of: "阅读正文").location
        let table = (source.string as NSString).range(of: "表格").location
        let image = (source.string as NSString).range(of: "\u{fffc}").location
        let headingFont = try XCTUnwrap(resized.attribute(.font, at: 0, effectiveRange: nil) as? NSFont)
        XCTAssertEqual(headingFont.pointSize, 41.6, accuracy: 0.01)
        XCTAssertTrue(headingFont.fontDescriptor.symbolicTraits.contains(.bold))
        XCTAssertEqual((resized.attribute(.font, at: body, effectiveRange: nil) as? NSFont)?.pointSize, 26)
        XCTAssertEqual((resized.attribute(.link, at: body, effectiveRange: nil) as? URL)?.absoluteString, "https://example.com/article")
        let paragraph = try XCTUnwrap(resized.attribute(.paragraphStyle, at: table, effectiveRange: nil) as? NSParagraphStyle)
        XCTAssertEqual(paragraph.lineHeightMultiple, 1.5)
        XCTAssertEqual(paragraph.textBlocks.count, 1)
        let attachment = try XCTUnwrap(source.attribute(.attachment, at: image, effectiveRange: nil) as? NSTextAttachment)
        XCTAssertTrue((resized.attribute(.attachment, at: image, effectiveRange: nil) as? NSTextAttachment) === attachment)
        XCTAssertEqual((resized.attribute(.paragraphStyle, at: image, effectiveRange: nil) as? NSParagraphStyle)?.lineHeightMultiple, 1)
        XCTAssertEqual(attachment.bounds.size, NSSize(width: 320, height: 160))
        XCTAssertTrue(source.isEqual(to: original))
        let reset = HistoryPreviewDocumentReadingStyle.attributed(source, options: .init(fontSize: 13))
        XCTAssertEqual((reset.attribute(.font, at: body, effectiveRange: nil) as? NSFont)?.pointSize, 13)
        XCTAssertFalse(HistoryPreviewDocumentReadingStyle.plainText(source).contains("\u{fffc}"))
        XCTAssertTrue(HistoryPreviewDocumentReadingStyle.plainText(source).contains("阅读正文"))
    }

    func testSearchNavigatesUnicodeMatchesAndSurvivesReadingChanges() throws {
        let source = NSAttributedString(string: "🍊 Alpha\nalpha\nALPHA", attributes: [.font: NSFont.systemFont(ofSize: 13)])
        let document = material(source)
        let (scroll, coordinator) = reader(source, material: document)
        let text = try XCTUnwrap(scroll.documentView as? NSTextView)
        var summaries: [HistoryPreviewDocumentSearchSummary] = []
        coordinator.onSearchSummary = { summaries.append($0) }
        coordinator.updateSearch("alpha", revision: 0, direction: 1)
        let ranges = HistoryPreviewDocumentSearch.matches(in: source.string, query: "alpha").ranges
        XCTAssertEqual(ranges.count, 3)
        XCTAssertEqual(text.selectedRange(), ranges[0])
        coordinator.updateSearch("alpha", revision: 1, direction: -1)
        XCTAssertEqual(text.selectedRange(), ranges[2])
        coordinator.updateSearch("alpha", revision: 2, direction: 1)
        XCTAssertEqual(text.selectedRange(), ranges[0])
        coordinator.install(document, options: .init(fontSize: 24, comfortableWidth: true), in: text, scrollView: scroll)
        coordinator.updateSearch("alpha", revision: 2, direction: 1)
        XCTAssertEqual(text.selectedRange(), ranges[0])
        XCTAssertEqual(try XCTUnwrap(text.textContainer).containerSize.width, 740, accuracy: 1)
        coordinator.updateSearch("missing", revision: 3, direction: 1)
        RunLoop.main.run(until: Date(timeIntervalSinceNow: 0.02))
        XCTAssertEqual(summaries.last?.count, 0)
        XCTAssertEqual(text.string, source.string)
        XCTAssertNil(text.layoutManager?.temporaryAttribute(.backgroundColor, atCharacterIndex: ranges[0].location, effectiveRange: nil))
    }

    func testReadingChangesKeepSelectionAndDoNotReturnLongDocumentsToTheTop() throws {
        let source = NSAttributedString(string: (0..<300).map { "第 \($0) 段 阅读正文\n" }.joined(), attributes: [.font: NSFont.systemFont(ofSize: 13)])
        let document = material(source)
        let (scroll, coordinator) = reader(source, material: document)
        let text = try XCTUnwrap(scroll.documentView as? NSTextView)
        let selection = (source.string as NSString).range(of: "第 180 段")
        text.setSelectedRange(selection)
        text.scrollRangeToVisible(selection)
        XCTAssertGreaterThan(scroll.contentView.bounds.minY, 0)
        coordinator.install(document, options: .init(fontSize: 26, lineSpacing: .relaxed), in: text, scrollView: scroll)
        XCTAssertEqual(text.selectedRange(), selection)
        XCTAssertGreaterThan(scroll.contentView.bounds.minY, 0)
        XCTAssertEqual(text.string, source.string)
    }

    func testComfortableReadingWidthTracksWindowResizing() {
        let text = HistoryPreviewDocumentReadingTextView(frame: NSRect(x: 0, y: 0, width: 1_100, height: 300))
        text.usesComfortableWidth = true
        XCTAssertEqual(text.textContainerInset.width, 180)
        text.setFrameSize(NSSize(width: 600, height: 300))
        XCTAssertEqual(text.textContainerInset.width, 20)
        text.setFrameSize(NSSize(width: 1_100, height: 300))
        XCTAssertEqual(text.textContainerInset.width, 180)
        text.usesComfortableWidth = false
        XCTAssertEqual(text.textContainerInset.width, 20)
    }

    func testDocumentSearchBoundsMatchRetention() {
        let text = String(repeating: "正文 ", count: 3_000)
        let found = HistoryPreviewDocumentSearch.matches(in: text, query: "正文")
        XCTAssertEqual(found.ranges.count, HistoryPreviewDocumentSearch.maximumMatches)
        XCTAssertTrue(found.truncated)
        XCTAssertTrue(HistoryPreviewDocumentSearch.matches(in: text, query: "").ranges.isEmpty)
    }

    func testSavedTextSizeUpdatesTheNativeDocumentWithoutHidingTheToolbar() throws {
        _ = NSApplication.shared
        let suite = "TailSync.DocumentReadingTests.\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let document = material(fixture())
        let theme = TailSyncThemeSelection(builtin: .tailsync)
        let host = NSHostingView(rootView: HistoryPreviewDocumentView(material: document)
            .defaultAppStorage(defaults)
            .frame(width: 720, height: 580)
            .environment(\.tailSyncPalette, theme.palette(for: .light))
            .environment(\.colorScheme, .light))
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 720, height: 580), styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.contentView = host
        defer { window.contentView = nil; window.close() }
        host.layoutSubtreeIfNeeded()
        RunLoop.main.run(until: Date(timeIntervalSinceNow: 0.05))
        let text = try XCTUnwrap(descendants(host).compactMap { $0 as? HistoryPreviewDocumentReadingTextView }.first)
        let body = (text.string as NSString).range(of: "阅读正文").location
        XCTAssertEqual((text.textStorage?.attribute(.font, at: body, effectiveRange: nil) as? NSFont)?.pointSize, 18)
        defaults.set(19, forKey: HistoryPreviewDocumentReadingOptions.fontSizeKey)
        RunLoop.main.run(until: Date(timeIntervalSinceNow: 0.05))
        XCTAssertEqual((text.textStorage?.attribute(.font, at: body, effectiveRange: nil) as? NSFont)?.pointSize, 19)
        XCTAssertEqual(defaults.double(forKey: HistoryPreviewDocumentReadingOptions.fontSizeKey), 19)
        let scroll = try XCTUnwrap(text.enclosingScrollView)
        XCTAssertGreaterThan(scroll.frame.height, 300)
        XCTAssertLessThan(scroll.convert(scroll.bounds, to: host).height, host.bounds.height - 40)
        if let directory = ProcessInfo.processInfo.environment["TAILSYNC_DOCUMENT_PREVIEW_RENDER_DIR"] {
            try FileManager.default.createDirectory(atPath: directory, withIntermediateDirectories: true)
            for scheme: ColorScheme in [.light, .dark] {
                host.rootView = HistoryPreviewDocumentView(material: document)
                    .defaultAppStorage(defaults)
                    .frame(width: 720, height: 580)
                    .environment(\.tailSyncPalette, theme.palette(for: scheme))
                    .environment(\.colorScheme, scheme)
                window.appearance = NSAppearance(named: scheme == .dark ? .darkAqua : .aqua)
                RunLoop.main.run(until: Date(timeIntervalSinceNow: 0.05))
                host.layoutSubtreeIfNeeded()
                let bitmap = try XCTUnwrap(host.bitmapImageRepForCachingDisplay(in: host.bounds))
                host.cacheDisplay(in: host.bounds, to: bitmap)
                try XCTUnwrap(bitmap.representation(using: .png, properties: [:])).write(to: URL(fileURLWithPath: directory).appendingPathComponent("document-preview-\(scheme == .dark ? "dark" : "light").png"))
            }
        }
    }

    private func reader(_ source: NSAttributedString, material document: HistoryPreviewDocumentMaterial? = nil) -> (NSScrollView, HistoryPreviewDocumentTextView.Coordinator) {
        let scroll = NSScrollView(frame: NSRect(x: 0, y: 0, width: 900, height: 300))
        scroll.hasVerticalScroller = true
        let text = HistoryPreviewDocumentTextFactory.makeTextView()
        scroll.documentView = text
        let coordinator = HistoryPreviewDocumentTextView.Coordinator(opener: RecordingDocumentLinkOpener())
        coordinator.textView = text
        coordinator.install(document ?? material(source), in: text, scrollView: scroll)
        return (scroll, coordinator)
    }

    private func material(_ source: NSAttributedString) -> HistoryPreviewDocumentMaterial {
        HistoryPreviewDocumentMaterial(entryId: 7, owner: "history", generation: 1, requestId: "reading-test", stamp: "reading-test", attributed: source, notices: [], stylesPartial: false, imagePlacements: 1, imagePlaceholders: 0, tableCount: 1, paragraphCount: 3, retainedBytes: 0)
    }

    private func fixture() -> NSAttributedString {
        let text = NSMutableAttributedString(string: "文稿阅读预览\n", attributes: [.font: NSFont.boldSystemFont(ofSize: 20.8), .foregroundColor: NSColor.labelColor])
        text.append(NSAttributedString(string: "阅读正文：调整字号时，标题、链接和图文位置都会保留。\n", attributes: [.font: NSFont.systemFont(ofSize: 13), .foregroundColor: NSColor.labelColor, .link: URL(string: "https://example.com/article")!]))
        let attachment = NSTextAttachment()
        attachment.image = NSImage(size: NSSize(width: 320, height: 160), flipped: false) { bounds in
            NSColor.systemOrange.withAlphaComponent(0.12).setFill()
            bounds.fill()
            ("图文内容" as NSString).draw(at: NSPoint(x: 110, y: 70), withAttributes: [.font: NSFont.systemFont(ofSize: 20), .foregroundColor: NSColor.systemOrange])
            return true
        }
        attachment.bounds = NSRect(x: 0, y: 0, width: 320, height: 160)
        text.append(NSAttributedString(attachment: attachment))
        text.append(NSAttributedString(string: "\n"))
        let table = NSTextTable()
        table.numberOfColumns = 1
        let block = NSTextTableBlock(table: table, startingRow: 0, rowSpan: 1, startingColumn: 0, columnSpan: 1)
        block.setWidth(8, type: .absoluteValueType, for: .padding)
        let paragraph = NSMutableParagraphStyle()
        paragraph.textBlocks = [block]
        text.append(NSAttributedString(string: "表格内容也保留原有结构\n", attributes: [.font: NSFont.systemFont(ofSize: 13), .foregroundColor: NSColor.labelColor, .paragraphStyle: paragraph]))
        return text
    }

    private func descendants(_ view: NSView) -> [NSView] {
        view.subviews.flatMap { [$0] + descendants($0) }
    }

}
