import AppKit
import Combine
import PDFKit
import SwiftUI
import XCTest
@testable import TailSync

final class HistoryPreviewLayoutTests: XCTestCase {
    @MainActor
    func testSwitchingToCodeKeepsTextToolbarControlsVisible() throws {
        _ = NSApplication.shared
        let text = "let answer = 42\nprint(answer)"
        let previewSize = NSSize(
            width: HistoryPreviewWindowKind.text.minimumContentSize.width,
            height: 520
        )
        let host = NSHostingView(
            rootView: HistoryPreviewTextView(text: text, initiallyCode: false)
                .frame(width: previewSize.width, height: previewSize.height)
                .environment(\.colorScheme, .light)
        )
        host.frame = NSRect(origin: .zero, size: previewSize)
        let window = NSWindow(
            contentRect: host.frame,
            styleMask: [.titled, .closable, .resizable],
            backing: .buffered,
            defer: false
        )
        window.contentView = host
        host.layoutSubtreeIfNeeded()

        let segmented = try XCTUnwrap(descendants(of: host).compactMap { $0 as? NSSegmentedControl }.first)
        let scrollView = try XCTUnwrap(descendants(of: host).compactMap { $0 as? NSScrollView }.first)
        let textView = try XCTUnwrap(scrollView.documentView as? NSTextView)
        let scrollFrameBefore = scrollView.convert(scrollView.bounds, to: host)
        let editorFrameBefore = textView.convert(textView.bounds, to: host)
        let visibleBefore = visibleControlCount(in: host)
        XCTAssertGreaterThanOrEqual(
            visibleBefore,
            1,
            "the toolbar must expose at least one visible native control"
        )
        let beforeImage = try snapshot(host)
        let beforeToolbarInk = toolbarInkPixelCount(in: beforeImage)
        XCTAssertGreaterThan(beforeToolbarInk, 100, "the toolbar snapshot must contain visible controls")
        XCTAssertEqual(textView.string, text)
        XCTAssertFalse(textView.isHidden)
        XCTAssertTrue(editorFrameBefore.intersects(host.bounds))

        segmented.setSelected(true, forSegment: 1)
        let action = try XCTUnwrap(segmented.action)
        _ = NSApp.sendAction(action, to: segmented.target, from: segmented)
        RunLoop.main.run(until: Date(timeIntervalSinceNow: 0.05))
        host.layoutSubtreeIfNeeded()
        let afterImage = try snapshot(host)

        XCTAssertEqual(segmented.selectedSegment, 1)
        XCTAssertTrue(scrollView.hasVerticalRuler)
        XCTAssertTrue(scrollView.documentView === textView)
        XCTAssertEqual(textView.string, text)
        XCTAssertFalse(textView.isHidden)
        XCTAssertTrue(textView.convert(textView.bounds, to: host).intersects(host.bounds))
        XCTAssertEqual(
            scrollView.convert(scrollView.bounds, to: host),
            scrollFrameBefore,
            "enabling code mode must not let the AppKit editor escape its SwiftUI frame"
        )
        XCTAssertGreaterThanOrEqual(
            visibleControlCount(in: host),
            visibleBefore,
            "code mode must not move the toolbar controls outside the preview"
        )
        let afterToolbarInk = toolbarInkPixelCount(in: afterImage)
        XCTAssertGreaterThanOrEqual(
            afterToolbarInk,
            beforeToolbarInk / 2,
            "code mode must not paint over the visible toolbar"
        )

        segmented.setSelected(true, forSegment: 0)
        _ = NSApp.sendAction(action, to: segmented.target, from: segmented)
        RunLoop.main.run(until: Date(timeIntervalSinceNow: 0.05))
        host.layoutSubtreeIfNeeded()
        let roundTripImage = try snapshot(host)

        XCTAssertEqual(segmented.selectedSegment, 0)
        XCTAssertFalse(scrollView.hasVerticalRuler)
        XCTAssertTrue(scrollView.documentView === textView)
        XCTAssertEqual(textView.string, text)
        XCTAssertFalse(textView.isHidden)
        XCTAssertTrue(textView.convert(textView.bounds, to: host).intersects(host.bounds))
        XCTAssertEqual(scrollView.convert(scrollView.bounds, to: host), scrollFrameBefore)
        XCTAssertGreaterThanOrEqual(visibleControlCount(in: host), visibleBefore)
        XCTAssertGreaterThanOrEqual(
            toolbarInkPixelCount(in: roundTripImage),
            beforeToolbarInk / 2,
            "switching back to text must keep the toolbar visible"
        )
    }

    func testPreviewControlsUseConsistentMinimumSizes() {
        XCTAssertGreaterThanOrEqual(HistoryPreviewLayoutMetrics.regularControlSize, 32)
        XCTAssertGreaterThanOrEqual(HistoryPreviewLayoutMetrics.compactControlSize, 28)
        XCTAssertGreaterThanOrEqual(
            HistoryPreviewLayoutMetrics.toolbarHeight,
            HistoryPreviewLayoutMetrics.regularControlSize + 16
        )
    }

    @MainActor
    func testModifierScrollOverlayIsMouseTransparent() {
        let view = HistoryPreviewModifierScrollView(
            frame: NSRect(x: 0, y: 0, width: 320, height: 200)
        )

        XCTAssertNil(
            view.hitTest(NSPoint(x: 160, y: 100)),
            "the wheel monitor must never sit in front of preview controls"
        )
    }

    @MainActor
    func testTextPreviewWrapsLongLinesAtTheViewportWidthByDefault() throws {
        _ = NSApplication.shared
        let text = String(repeating: "automaticWrappingMustKeepLongTokensVisible", count: 30)
        let size = NSSize(width: 420, height: 320)
        let host = NSHostingView(
            rootView: HistoryPreviewTextView(text: text, initiallyCode: false)
                .frame(width: size.width, height: size.height)
        )
        host.frame = NSRect(origin: .zero, size: size)
        let window = NSWindow(
            contentRect: host.frame,
            styleMask: [.titled, .closable, .resizable],
            backing: .buffered,
            defer: false
        )
        window.contentView = host
        host.layoutSubtreeIfNeeded()

        let scrollView = try XCTUnwrap(descendants(of: host).compactMap { $0 as? NSScrollView }.first)
        let textView = try XCTUnwrap(scrollView.documentView as? NSTextView)
        let textContainer = try XCTUnwrap(textView.textContainer)
        let layoutManager = try XCTUnwrap(textView.layoutManager)
        layoutManager.ensureLayout(for: textContainer)
        var fragmentCount = 0
        layoutManager.enumerateLineFragments(
            forGlyphRange: NSRange(location: 0, length: layoutManager.numberOfGlyphs)
        ) { _, _, _, _, _ in
            fragmentCount += 1
        }

        XCTAssertFalse(scrollView.hasHorizontalScroller)
        XCTAssertTrue(textContainer.widthTracksTextView)
        XCTAssertGreaterThan(textView.frame.width, 0)
        XCTAssertGreaterThan(fragmentCount, 1, "long text should wrap into visible line fragments")
    }

    @MainActor
    func testTextPreviewCanRestoreWrappingAfterHorizontalLayout() throws {
        _ = NSApplication.shared
        let text = String(repeating: "restoreWrappingAfterHorizontalLayout", count: 30)
        let size = NSSize(width: 420, height: 260)
        let host = NSHostingView(rootView: textEditor(text: text, wrapsLines: false, size: size))
        host.frame = NSRect(origin: .zero, size: size)
        let window = NSWindow(
            contentRect: host.frame,
            styleMask: [.titled, .resizable],
            backing: .buffered,
            defer: false
        )
        window.contentView = host
        host.layoutSubtreeIfNeeded()
        let scrollView = try XCTUnwrap(descendants(of: host).compactMap { $0 as? NSScrollView }.first)
        XCTAssertTrue(scrollView.hasHorizontalScroller)

        host.rootView = textEditor(text: text, wrapsLines: true, size: size)
        host.layoutSubtreeIfNeeded()
        RunLoop.main.run(until: Date(timeIntervalSinceNow: 0.02))
        let textView = try XCTUnwrap(scrollView.documentView as? NSTextView)
        let textContainer = try XCTUnwrap(textView.textContainer)
        let layoutManager = try XCTUnwrap(textView.layoutManager)
        layoutManager.ensureLayout(for: textContainer)
        var fragmentCount = 0
        layoutManager.enumerateLineFragments(
            forGlyphRange: NSRange(location: 0, length: layoutManager.numberOfGlyphs)
        ) { _, _, _, _, _ in
            fragmentCount += 1
        }

        XCTAssertFalse(scrollView.hasHorizontalScroller)
        XCTAssertLessThanOrEqual(textView.frame.width, scrollView.contentSize.width + 1)
        XCTAssertGreaterThan(fragmentCount, 1)
    }

    @MainActor
    func testReattachingPDFViewDoesNotRepublishUnchangedState() throws {
        let data = try makePDFData()
        let controller = HistoryPDFPreviewController(document: try XCTUnwrap(PDFDocument(data: data)))
        let pdfView = PDFView(frame: NSRect(x: 0, y: 0, width: 640, height: 480))
        controller.attach(pdfView)
        var changeCount = 0
        let observation = controller.objectWillChange.sink { changeCount += 1 }

        for _ in 0..<100 {
            controller.attach(pdfView)
        }

        withExtendedLifetime(observation) {
            XCTAssertEqual(
                changeCount,
                0,
                "NSViewRepresentable updates must not create an ObservableObject feedback loop"
            )
        }
    }

    @MainActor
    func testEveryCustomPreviewToolbarKeepsControlsUsableAtMinimumWidth() throws {
        _ = NSApplication.shared
        let pdfData = try makePDFData()
        let image = makeImage()
        let previews: [(AnyView, NSSize)] = [
            (
                AnyView(HistoryMarkdownPreviewView(source: "# Article\nBody")),
                HistoryPreviewWindowKind.document.minimumContentSize
            ),
            (
                AnyView(HistoryImagePreviewView(material: HistoryPreviewImageMaterial(
                    data: try XCTUnwrap(image.tiffRepresentation),
                    image: image
                ))),
                HistoryPreviewWindowKind.image.minimumContentSize
            ),
            (
                AnyView(HistoryPDFPreviewView(material: HistoryPreviewPDFMaterial(
                    data: pdfData,
                    document: try XCTUnwrap(PDFDocument(data: pdfData))
                ))),
                HistoryPreviewWindowKind.pdf.minimumContentSize
            )
        ]

        for (preview, size) in previews {
            let host = NSHostingView(
                rootView: preview.frame(width: size.width, height: size.height)
            )
            host.frame = NSRect(origin: .zero, size: size)
            let window = NSWindow(
                contentRect: host.frame,
                styleMask: [.titled, .resizable],
                backing: .buffered,
                defer: false
            )
            window.contentView = host
            host.layoutSubtreeIfNeeded()
            RunLoop.main.run(until: Date(timeIntervalSinceNow: 0.02))
            let image = try snapshot(host)
            XCTAssertGreaterThan(
                toolbarInkPixelCount(in: image),
                100,
                "the format toolbar must remain visibly rendered at minimum width"
            )
        }
    }

    @MainActor
    func testPDFPreviewReusesPreparedDocumentAndDefersThumbnailRendering() throws {
        _ = NSApplication.shared
        let data = try makePDFData()
        let document = try XCTUnwrap(PDFDocument(data: data))
        let material = HistoryPreviewPDFMaterial(data: data, document: document)
        let size = NSSize(width: 760, height: 560)
        let host = NSHostingView(
            rootView: HistoryPDFPreviewView(material: material)
                .frame(width: size.width, height: size.height)
        )
        host.frame = NSRect(origin: .zero, size: size)
        let window = NSWindow(
            contentRect: host.frame,
            styleMask: [.titled, .resizable],
            backing: .buffered,
            defer: false
        )
        window.contentView = host
        host.layoutSubtreeIfNeeded()
        RunLoop.main.run(until: Date(timeIntervalSinceNow: 0.05))

        let pdfView = try XCTUnwrap(descendants(of: host).compactMap { $0 as? PDFView }.first)
        let thumbnails = try XCTUnwrap(
            descendants(of: host).compactMap { $0 as? PDFThumbnailView }.first
        )
        XCTAssertTrue(pdfView.document === document)
        XCTAssertTrue(thumbnails.isHidden)
        XCTAssertNil(thumbnails.pdfView)
    }

    @MainActor
    func testTextAndMarkdownPreviewsInstallModifierWheelFontControls() {
        _ = NSApplication.shared
        let previews: [AnyView] = [
            AnyView(HistoryPreviewTextView(text: "text", initiallyCode: false)),
            AnyView(HistoryMarkdownPreviewView(source: "# Article"))
        ]
        for preview in previews {
            let host = NSHostingView(
                rootView: preview.frame(width: 640, height: 420)
            )
            host.frame = NSRect(x: 0, y: 0, width: 640, height: 420)
            let window = NSWindow(
                contentRect: host.frame,
                styleMask: [.titled],
                backing: .buffered,
                defer: false
            )
            window.contentView = host
            host.layoutSubtreeIfNeeded()
            XCTAssertEqual(
                descendants(of: host).compactMap { $0 as? HistoryPreviewModifierScrollView }.count,
                1
            )
        }
    }

    @MainActor
    func testPDFSearchStartsAsynchronouslyAfterDebounce() throws {
        let document = try XCTUnwrap(PDFDocument(data: makeSearchablePDFData()))
        let controller = HistoryPDFPreviewController(document: document)
        let pdfView = PDFView(frame: NSRect(x: 0, y: 0, width: 640, height: 480))
        controller.attach(pdfView)

        controller.updateSearch("needle")

        XCTAssertTrue(controller.isSearching)
        XCTAssertFalse(document.isFinding)
        XCTAssertEqual(controller.matchCount, 0)
        for _ in 0..<100 where controller.isSearching || controller.matchCount == 0 {
            RunLoop.main.run(until: Date(timeIntervalSinceNow: 0.02))
        }

        XCTAssertGreaterThan(controller.matchCount, 0)
        XCTAssertFalse(controller.isSearching)
    }

    @MainActor
    func testPDFControllerReleasesPendingSearchAndObservers() throws {
        let document = try XCTUnwrap(PDFDocument(data: makeSearchablePDFData()))
        weak var releasedController: HistoryPDFPreviewController?
        autoreleasepool {
            var controller: HistoryPDFPreviewController? = HistoryPDFPreviewController(
                document: document
            )
            controller?.updateSearch("needle")
            releasedController = controller
            controller = nil
        }
        RunLoop.main.run(until: Date(timeIntervalSinceNow: 0.22))

        XCTAssertNil(releasedController)
        XCTAssertFalse(document.isFinding)
    }

    func testThumbnailLayoutPreservesModerateAspectRatios() {
        let wide = HistoryThumbnailLayout.displaySize(pixelWidth: 200, pixelHeight: 100)
        XCTAssertEqual(wide.width, HistoryThumbnailLayout.maxSide, accuracy: 0.01)
        XCTAssertEqual(wide.width / wide.height, 2, accuracy: 0.01, "a 2:1 image must stay 2:1")

        let tall = HistoryThumbnailLayout.displaySize(pixelWidth: 100, pixelHeight: 200)
        XCTAssertEqual(tall.height, HistoryThumbnailLayout.maxSide, accuracy: 0.01)
        XCTAssertEqual(tall.height / tall.width, 2, accuracy: 0.01, "a 1:2 image must stay 1:2")
    }

    func testThumbnailLayoutRendersSquaresAsSquares() {
        let square = HistoryThumbnailLayout.displaySize(pixelWidth: 128, pixelHeight: 128)
        XCTAssertEqual(square.width, HistoryThumbnailLayout.maxSide, accuracy: 0.01)
        XCTAssertEqual(square.height, HistoryThumbnailLayout.maxSide, accuracy: 0.01)
    }

    func testThumbnailLayoutClampsExtremeLongStrips() {
        let maxAspect = HistoryThumbnailLayout.maxAspect
        let maxSide = HistoryThumbnailLayout.maxSide

        let banner = HistoryThumbnailLayout.displaySize(pixelWidth: 2000, pixelHeight: 100)
        XCTAssertEqual(
            banner.width / banner.height, maxAspect, accuracy: 0.01,
            "a 20:1 banner must be clamped to \(maxAspect):1"
        )
        XCTAssertLessThanOrEqual(max(banner.width, banner.height), maxSide + 0.01)

        let column = HistoryThumbnailLayout.displaySize(pixelWidth: 100, pixelHeight: 2000)
        XCTAssertEqual(
            column.height / column.width, maxAspect, accuracy: 0.01,
            "a 1:20 long screenshot must be clamped to 1:\(maxAspect)"
        )
        XCTAssertLessThanOrEqual(max(column.width, column.height), maxSide + 0.01)
    }

    func testThumbnailLayoutSurvivesDegenerateSizes() {
        let zero = HistoryThumbnailLayout.displaySize(pixelWidth: 0, pixelHeight: 0)
        XCTAssertEqual(zero.width, HistoryThumbnailLayout.maxSide, accuracy: 0.01)
        XCTAssertEqual(zero.height, HistoryThumbnailLayout.maxSide, accuracy: 0.01)
        XCTAssertTrue(zero.width.isFinite && zero.height.isFinite)
    }

    @MainActor
    private func visibleControlCount(in host: NSView) -> Int {
        descendants(of: host).filter { view in
            guard view is NSButton || view is NSSegmentedControl || view is NSTextField,
                  !view.isHidden,
                  view.alphaValue > 0 else { return false }
            let frame = view.convert(view.bounds, to: host)
            return frame.intersects(host.bounds)
        }.count
    }

    @MainActor
    private func descendants(of view: NSView) -> [NSView] {
        view.subviews.flatMap { [$0] + descendants(of: $0) }
    }

    @MainActor
    private func snapshot(_ view: NSView) throws -> NSBitmapImageRep {
        let rep = try XCTUnwrap(view.bitmapImageRepForCachingDisplay(in: view.bounds))
        view.cacheDisplay(in: view.bounds, to: rep)
        return rep
    }

    private func toolbarInkPixelCount(in image: NSBitmapImageRep) -> Int {
        let toolbarPixelHeight = min(image.pixelsHigh, 100)
        var count = 0
        for y in 0..<toolbarPixelHeight {
            for x in 0..<image.pixelsWide {
                guard let color = image.colorAt(x: x, y: y)?.usingColorSpace(.sRGB) else { continue }
                let luminance = 0.2126 * color.redComponent
                    + 0.7152 * color.greenComponent
                    + 0.0722 * color.blueComponent
                if color.alphaComponent > 0.5, luminance < 0.72 {
                    count += 1
                }
            }
        }
        return count
    }

    private func textEditor(
        text: String,
        wrapsLines: Bool,
        size: NSSize
    ) -> some View {
        HistoryPreviewTextEditor(
            text: text,
            isCode: true,
            wrapsLines: wrapsLines,
            fontSize: 18,
            searchQuery: "",
            searchRevision: 0,
            searchDirection: 1
        )
        .frame(width: size.width, height: size.height)
    }

    @MainActor
    private func makePDFData() throws -> Data {
        let image = NSImage(size: NSSize(width: 16, height: 16))
        image.lockFocus()
        NSColor.black.setFill()
        NSRect(x: 0, y: 0, width: 16, height: 16).fill()
        image.unlockFocus()
        let document = PDFDocument()
        document.insert(try XCTUnwrap(PDFPage(image: image)), at: 0)
        return try XCTUnwrap(document.dataRepresentation())
    }

    @MainActor
    private func makeSearchablePDFData() -> Data {
        let label = NSTextField(labelWithString: "needle in a searchable PDF")
        label.frame = NSRect(x: 0, y: 0, width: 320, height: 40)
        return label.dataWithPDF(inside: label.bounds)
    }

    @MainActor
    private func makeImage() -> NSImage {
        let size = NSSize(width: 32, height: 32)
        let image = NSImage(size: size)
        image.lockFocus()
        NSColor.systemBlue.setFill()
        NSRect(origin: .zero, size: size).fill()
        image.unlockFocus()
        return image
    }
}
