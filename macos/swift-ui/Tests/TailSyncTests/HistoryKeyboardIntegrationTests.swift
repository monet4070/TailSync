import AppKit
import SwiftUI
import XCTest
@testable import TailSync

/// Exercises the real HistoryView and its native field editor with injected
/// history/restore operations. No daemon requests or clipboard mutations.
@MainActor
final class HistoryKeyboardIntegrationTests: XCTestCase {
    func testRealHistoryViewFirstDownArrowSearchAndRestoreLock() async throws {
        let service = HistoryKeyboardTestService()
        let fixture = Fixture(service: service)
        defer { fixture.close() }
        try await wait { fixture.session.selectedID == 1 }
        let editor = try fixture.searchEditor()
        editor.interpretKeyEvents([event(125, window: fixture.window)])
        XCTAssertEqual(fixture.session.selectedID, 1)
        XCTAssertEqual(fixture.session.focus, .list)
        XCTAssertTrue(fixture.window.firstResponder === fixture.session.listResponder)
        fixture.window.sendEvent(event(125, window: fixture.window))
        XCTAssertEqual(fixture.session.selectedID, 2)
        fixture.window.sendEvent(event(36, window: fixture.window, modifiers: .command))
        fixture.window.sendEvent(event(36, window: fixture.window, modifiers: .command))
        try await wait { await service.restoreCount == 1 }
        fixture.window.sendEvent(event(125, window: fixture.window))
        XCTAssertEqual(fixture.session.selectedID, 2, "selection freezes while restoring")
        try await Task.sleep(for: .milliseconds(100))
        let restored = await service.restoreCount
        XCTAssertEqual(restored, 1)
        XCTAssertNotNil(fixture.session.selectedID)
        fixture.window.sendEvent(event(3, window: fixture.window, modifiers: .command))
        XCTAssertEqual(fixture.session.focus, .search)
    }

    func testTypedQueryCannotRestoreOldRowsDuringDebounceOrFailedRequest() async throws {
        let service = HistoryKeyboardTestService()
        let fixture = Fixture(service: service)
        defer { fixture.close() }
        try await wait { fixture.session.selectedID == 1 }
        let editor = try fixture.searchEditor()
        editor.insertText("missing", replacementRange: NSRange(location: NSNotFound, length: 0))
        editor.interpretKeyEvents([event(36, window: fixture.window)])
        let restoresBeforeLoad = await service.restoreCount
        XCTAssertEqual(restoresBeforeLoad, 0)
        try await wait { await service.failedQueryCount == 1 }
        editor.interpretKeyEvents([event(36, window: fixture.window)])
        try await Task.sleep(for: .milliseconds(20))
        let restoresAfterFailure = await service.restoreCount
        XCTAssertEqual(restoresAfterFailure, 0)
        XCTAssertEqual(fixture.session.selectedID, 1, "failure preserves stable selection")
        XCTAssertEqual(editor.string, "missing", "failure preserves query")
    }

    func testBoundaryPagingAndPageZeroUpReturnToSearch() async throws {
        let service = HistoryKeyboardTestService()
        let fixture = Fixture(service: service)
        defer { fixture.close() }
        try await wait { fixture.session.selectedID == 1 }
        let editor = try fixture.searchEditor()
        editor.interpretKeyEvents([event(125, window: fixture.window)])
        for _ in 0..<29 { fixture.window.sendEvent(event(125, window: fixture.window)) }
        XCTAssertEqual(fixture.session.selectedID, 30)
        fixture.window.sendEvent(event(125, window: fixture.window))
        try await wait { fixture.session.selectedID == 31 }
        fixture.window.sendEvent(event(126, window: fixture.window))
        try await wait { fixture.session.selectedID == 30 }
        for _ in 0..<29 { fixture.window.sendEvent(event(126, window: fixture.window)) }
        XCTAssertEqual(fixture.session.selectedID, 1)
        fixture.window.sendEvent(event(126, window: fixture.window))
        XCTAssertEqual(fixture.session.focus, .search)
        XCTAssertTrue(fixture.window.firstResponder is NSTextView)
    }

    func testRestoreFailureKeepsQueryAndStableSelectionAndAllowsRetry() async throws {
        let service = HistoryKeyboardTestService(failRestore: true)
        let fixture = Fixture(service: service)
        defer { fixture.close() }
        try await wait { fixture.session.selectedID == 1 }
        let editor = try fixture.searchEditor()
        editor.interpretKeyEvents([event(125, window: fixture.window)])
        fixture.window.sendEvent(event(125, window: fixture.window))
        fixture.window.sendEvent(event(36, window: fixture.window))
        try await wait { await service.restoreCount == 1 }
        try await Task.sleep(for: .milliseconds(100))
        XCTAssertEqual(fixture.session.selectedID, 2)
        XCTAssertEqual(fixture.session.focus, .list)
        fixture.window.sendEvent(event(36, window: fixture.window))
        try await wait { await service.restoreCount == 2 }
        XCTAssertEqual(fixture.session.selectedID, 2)
    }

    func testLateQueryResponseCannotReplaceNewerResults() async throws {
        let service = HistoryKeyboardTestService()
        let fixture = Fixture(service: service)
        defer { fixture.close() }
        try await wait { fixture.session.selectedID == 1 }
        let editor = try fixture.searchEditor()
        editor.insertText("slow", replacementRange: NSRange(location: NSNotFound, length: 0))
        try await wait { await service.slowQueryCount == 1 }
        editor.insertText("fast", replacementRange: NSRange(location: 0, length: editor.string.utf16.count))
        try await wait { fixture.session.selectedID == 200 }
        try await Task.sleep(for: .milliseconds(500))
        XCTAssertEqual(fixture.session.selectedID, 200)
        XCTAssertEqual(editor.string, "fast")
    }

    private func wait(_ condition: () async -> Bool) async throws {
        for _ in 0..<200 {
            if await condition() { return }
            try await Task.sleep(for: .milliseconds(10))
        }
        XCTFail("Timed out waiting for HistoryView")
    }

    private func event(_ code: UInt16, window: NSWindow, modifiers: NSEvent.ModifierFlags = []) -> NSEvent {
        let characters: String
        switch code {
        case 125: characters = String(UnicodeScalar(NSDownArrowFunctionKey)!)
        case 126: characters = String(UnicodeScalar(NSUpArrowFunctionKey)!)
        case 36: characters = "\r"
        case 3: characters = "f"
        default: characters = ""
        }
        return NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: modifiers,
            timestamp: 0, windowNumber: window.windowNumber, context: nil,
            characters: characters, charactersIgnoringModifiers: characters,
            isARepeat: false, keyCode: code)!
    }

    @MainActor
    private final class Fixture {
        let session = HistoryKeyboardSession()
        let window: NSWindow
        init(service: HistoryKeyboardTestService) {
            _ = NSApplication.shared
            let dependencies = HistoryViewDependencies(
                load: { try await service.load($0) },
                restore: { try await service.restore($0) },
                usesRuntimeUpdates: false
            )
            let view = HistoryView(dependencies: dependencies, keyboardSession: session)
            let hosting = NSHostingView(rootView: view)
            window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 450, height: 600),
                styleMask: [.titled], backing: .buffered, defer: false)
            window.isReleasedWhenClosed = false
            window.contentView = hosting
            hosting.layoutSubtreeIfNeeded()
        }
        func searchEditor() throws -> NSTextView {
            session.focusSearch(selectAll: false)
            return try XCTUnwrap(window.firstResponder as? NSTextView)
        }
        func close() {
            window.contentView = nil
            window.close()
        }
    }
}

private actor HistoryKeyboardTestService {
    private(set) var restoreCount = 0
    private(set) var failedQueryCount = 0
    private(set) var slowQueryCount = 0
    let failRestore: Bool
    init(failRestore: Bool = false) { self.failRestore = failRestore }
    func load(_ query: HistoryQuery) async throws -> [HistoryEntry] {
        if query.keyword == "slow" {
            slowQueryCount += 1
            try await Task.sleep(for: .milliseconds(600))
            return try [entry(100)]
        }
        if query.keyword == "fast" { return try [entry(200)] }
        if query.keyword != nil {
            failedQueryCount += 1
            throw ApiError.serverError("test query failed")
        }
        return try (query.offset + 1...min(query.offset + query.limit, 32)).map { try entry($0) }
    }
    private func entry(_ id: Int) throws -> HistoryEntry {
        let data = Data("""
        {"id":\(id),"timestamp":"2026-10-03T00:00:00Z","type":"text",
         "description":"row \(id)","data_hash":"hash\(id)","size_bytes":4,"source_peer":"local"}
        """.utf8)
        return try JSONDecoder().decode(HistoryEntry.self, from: data)
    }
    func restore(_ id: Int64) async throws {
        restoreCount += 1
        try await Task.sleep(for: .milliseconds(80))
        if failRestore { throw ApiError.serverError("test restore failed") }
    }
}
