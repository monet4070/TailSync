import AppKit
import SwiftUI
import XCTest
@testable import TailSync

final class HistoryRowInteractionTests: XCTestCase {
    func testFirstClickSelectsAndSecondClickRestoresWithoutGestureCompetition() throws {
        _ = NSApplication.shared
        let window = makeWindow()
        let interaction = HistoryRowInteractionNSView(frame: NSRect(x: 0, y: 0, width: 240, height: 80))
        window.contentView?.addSubview(interaction)

        var selectionCount = 0
        var restoreCount = 0
        var deleteCount = 0
        interaction.onSelect = { selectionCount += 1 }
        interaction.onRestore = { restoreCount += 1 }
        interaction.onDelete = { deleteCount += 1 }

        interaction.mouseDown(with: mouseEvent(.leftMouseDown, window: window, clickCount: 1, eventNumber: 1))
        interaction.mouseUp(with: mouseEvent(.leftMouseUp, window: window, clickCount: 1, eventNumber: 2))

        XCTAssertTrue(window.firstResponder === interaction)
        XCTAssertEqual(selectionCount, 1)
        XCTAssertEqual(restoreCount, 0)

        interaction.mouseDown(with: mouseEvent(.leftMouseDown, window: window, clickCount: 2, eventNumber: 3))
        interaction.mouseUp(with: mouseEvent(.leftMouseUp, window: window, clickCount: 2, eventNumber: 4))

        XCTAssertEqual(selectionCount, 2)
        XCTAssertEqual(restoreCount, 1)

        interaction.rightMouseDown(with: mouseEvent(
            .rightMouseDown,
            window: window,
            clickCount: 1,
            eventNumber: 5
        ))
        XCTAssertEqual(deleteCount, 1)
        _ = window.makeFirstResponder(nil)
    }

    func testSwiftUIOverlayInstallsAFullSizeInteractiveResponder() throws {
        _ = NSApplication.shared
        let window = makeWindow()
        let request = Self.previewRequest(id: 40)
        var selectionCount = 0
        var previewCount = 0
        let root = Color.clear
            .frame(width: 240, height: 80)
            .overlay {
                HistoryRowInteraction(
                    previewRequest: request,
                    onSelect: { selectionCount += 1 },
                    onRestore: {},
                    onDelete: {},
                    onPreview: { _ in previewCount += 1 },
                    onClosePreview: {},
                    isPreviewVisible: { false }
                )
            }
        let host = NSHostingView(rootView: root)
        host.frame = NSRect(x: 0, y: 0, width: 240, height: 80)
        window.contentView?.addSubview(host)
        window.layoutIfNeeded()
        host.layoutSubtreeIfNeeded()

        let target = try XCTUnwrap(
            host.hitTest(NSPoint(x: 120, y: 40)) as? HistoryRowInteractionNSView
        )
        XCTAssertEqual(target.frame.size, NSSize(width: 240, height: 80))

        target.mouseDown(with: mouseEvent(.leftMouseDown, window: window, clickCount: 1, eventNumber: 1))
        target.mouseUp(with: mouseEvent(.leftMouseUp, window: window, clickCount: 1, eventNumber: 2))
        target.keyDown(with: keyEvent(window: window, keyCode: 49, characters: " "))

        XCTAssertEqual(selectionCount, 1)
        XCTAssertEqual(previewCount, 1)
    }

    func testSpaceOnSelectedRowOpensPreviewExactlyOnce() {
        _ = NSApplication.shared
        let window = makeWindow()
        let interaction = HistoryRowInteractionNSView(frame: NSRect(x: 0, y: 0, width: 240, height: 80))
        window.contentView?.addSubview(interaction)
        let request = Self.previewRequest(id: 41)
        var previewedRequests: [HistoryPreviewRequest] = []
        var closeCount = 0
        interaction.previewRequest = request
        interaction.onPreview = { previewedRequests.append($0) }
        interaction.onClosePreview = { closeCount += 1 }
        interaction.isPreviewVisible = { false }
        window.makeFirstResponder(interaction)

        interaction.keyDown(with: keyEvent(window: window, keyCode: 49, characters: " "))

        XCTAssertEqual(previewedRequests, [request])
        XCTAssertEqual(closeCount, 0, "the opening Space must not also close the new preview")
    }

    func testModifiedAndRepeatedSpaceDoNotOpenPreview() {
        _ = NSApplication.shared
        let window = makeWindow()
        let interaction = HistoryRowInteractionNSView(frame: NSRect(x: 0, y: 0, width: 240, height: 80))
        window.contentView?.addSubview(interaction)
        interaction.previewRequest = Self.previewRequest(id: 42)
        var previewCount = 0
        interaction.onPreview = { _ in previewCount += 1 }

        interaction.keyDown(with: keyEvent(
            window: window,
            keyCode: 49,
            characters: " ",
            modifiers: [.command]
        ))
        interaction.keyDown(with: keyEvent(
            window: window,
            keyCode: 49,
            characters: " ",
            isRepeat: true
        ))

        XCTAssertEqual(previewCount, 0)
    }

    func testEscapeClosesOnlyAnExistingPreview() {
        _ = NSApplication.shared
        let window = makeWindow()
        let interaction = HistoryRowInteractionNSView(frame: NSRect(x: 0, y: 0, width: 240, height: 80))
        window.contentView?.addSubview(interaction)
        var previewVisible = false
        var closeCount = 0
        interaction.isPreviewVisible = { previewVisible }
        interaction.onClosePreview = { closeCount += 1 }

        interaction.keyDown(with: keyEvent(window: window, keyCode: 53, characters: "\u{1b}"))
        XCTAssertEqual(closeCount, 0)

        previewVisible = true
        interaction.keyDown(with: keyEvent(window: window, keyCode: 53, characters: "\u{1b}"))
        XCTAssertEqual(closeCount, 1)
    }

    private func makeWindow() -> NSWindow {
        NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 320, height: 160),
            styleMask: [.titled],
            backing: .buffered,
            defer: false
        )
    }

    private func mouseEvent(
        _ type: NSEvent.EventType,
        window: NSWindow,
        clickCount: Int,
        eventNumber: Int
    ) -> NSEvent {
        NSEvent.mouseEvent(
            with: type,
            location: NSPoint(x: 120, y: 40),
            modifierFlags: [],
            timestamp: ProcessInfo.processInfo.systemUptime,
            windowNumber: window.windowNumber,
            context: nil,
            eventNumber: eventNumber,
            clickCount: clickCount,
            pressure: type == .leftMouseDown ? 1 : 0
        )!
    }

    private func keyEvent(
        window: NSWindow,
        keyCode: UInt16,
        characters: String,
        modifiers: NSEvent.ModifierFlags = [],
        isRepeat: Bool = false
    ) -> NSEvent {
        NSEvent.keyEvent(
            with: .keyDown,
            location: .zero,
            modifierFlags: modifiers,
            timestamp: ProcessInfo.processInfo.systemUptime,
            windowNumber: window.windowNumber,
            context: nil,
            characters: characters,
            charactersIgnoringModifiers: characters,
            isARepeat: isRepeat,
            keyCode: keyCode
        )!
    }

    private static func previewRequest(id: Int64) -> HistoryPreviewRequest {
        HistoryPreviewRequest(
            items: [
                HistoryPreviewItem(
                    id: id,
                    batchId: nil,
                    batchIndex: nil,
                    batchCount: nil,
                    category: "text",
                    type: "text",
                    nameHint: "preview",
                    sizeBytes: 7
                )
            ],
            selectedIndex: 0
        )
    }
}
