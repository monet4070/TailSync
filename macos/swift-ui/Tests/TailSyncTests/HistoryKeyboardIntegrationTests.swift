import AppKit
import SwiftUI
import XCTest
@testable import TailSync

/// Exercises the real HistoryView and its native field editor with injected
/// history/restore operations. No daemon requests or clipboard mutations.
@MainActor
final class HistoryKeyboardIntegrationTests: XCTestCase {
    func testCommandDeleteDeletesSelectedRowOnceAndKeepsAdjacentSelection() async throws {
        let service = HistoryKeyboardTestService()
        let fixture = Fixture(service: service)
        defer { fixture.close() }
        try await wait { fixture.session.selectedID == 1 }
        fixture.session.focusList()
        fixture.window.sendEvent(event(125, window: fixture.window))
        XCTAssertEqual(fixture.session.selectedID, 2)
        fixture.window.sendEvent(event(51, window: fixture.window, modifiers: .command))
        fixture.window.sendEvent(event(51, window: fixture.window, modifiers: .command))
        fixture.window.sendEvent(event(36, window: fixture.window, modifiers: .command))
        try await wait { fixture.session.selectedID == 3 }
        let deletions = await service.deletedIDs
        let restores = await service.restoreCount
        XCTAssertEqual(deletions, [2])
        XCTAssertEqual(restores, 0, "restore must wait until deletion completes")
        XCTAssertTrue(fixture.window.firstResponder === fixture.session.listResponder)
    }

    func testCommandDeletePreservesSearchEditingCompositionAndStaleQuery() async throws {
        let service = HistoryKeyboardTestService()
        let fixture = Fixture(service: service)
        defer { fixture.close() }
        try await wait { fixture.session.selectedID == 1 }
        let editor = try fixture.searchEditor()
        editor.string = "typing"
        editor.setSelectedRange(NSRange(location: 6, length: 0))
        fixture.window.sendEvent(event(51, window: fixture.window, modifiers: .command))
        XCTAssertEqual(editor.string, "", "Command Delete keeps native delete-to-line-start editing")
        fixture.session.focusList()
        fixture.session.isComposing = true
        fixture.window.sendEvent(event(51, window: fixture.window, modifiers: .command))
        fixture.session.isComposing = false
        let currentEditor = try fixture.searchEditor()
        currentEditor.insertText("missing", replacementRange: NSRange(location: NSNotFound, length: 0))
        fixture.session.focusList()
        fixture.window.sendEvent(event(51, window: fixture.window, modifiers: .command))
        try await wait { await service.failedQueryCount == 1 }
        fixture.window.sendEvent(event(51, window: fixture.window, modifiers: .command))
        let deletions = await service.deletedIDs
        XCTAssertEqual(deletions, [])
    }

    func testCommandDeleteProtectsFavoritesAndUsesFavoritesDeletion() async throws {
        let protected = HistoryKeyboardTestService(pinned: true)
        let history = Fixture(service: protected)
        defer { history.close() }
        try await wait { history.session.selectedID == 1 }
        history.session.focusList()
        history.window.sendEvent(event(51, window: history.window, modifiers: .command))
        try await Task.sleep(for: .milliseconds(100))
        let protectedDeletions = await protected.deletedIDs
        XCTAssertEqual(protectedDeletions, [])

        let service = HistoryKeyboardTestService(pinned: true)
        let favorites = Fixture(service: service, collection: "favorites")
        defer { favorites.close() }
        try await wait { favorites.session.selectedID == 1 }
        favorites.session.focusList()
        favorites.window.sendEvent(event(51, window: favorites.window, modifiers: .command))
        try await wait { favorites.session.selectedID == 2 }
        let collections = await service.deletedCollections
        XCTAssertEqual(collections, ["favorites"])
    }

    func testCommandDeleteFailureKeepsSelectionAndAllowsRetry() async throws {
        let service = HistoryKeyboardTestService(failDelete: true)
        let fixture = Fixture(service: service)
        defer { fixture.close() }
        try await wait { fixture.session.selectedID == 1 }
        fixture.session.focusList()
        fixture.window.sendEvent(event(51, window: fixture.window, modifiers: .command))
        try await wait { await service.deleteFinishedCount == 1 }
        try await Task.sleep(for: .milliseconds(20))
        XCTAssertEqual(fixture.session.selectedID, 1)
        await service.allowDelete()
        fixture.window.sendEvent(event(51, window: fixture.window, modifiers: .command))
        try await wait { fixture.session.selectedID == 2 }
        let deletions = await service.deletedIDs
        XCTAssertEqual(deletions, [1, 1])
    }

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

    /// F5: a keystroke must cancel the in-flight page read immediately. Waiting
    /// for the 180 ms debounce to issue the replacement query leaves the old
    /// daemon read (and its connection permit) running.
    func testTypingCancelsTheInFlightReadBeforeTheDebounceReplacement() async throws {
        let service = HistoryKeyboardTestService()
        let fixture = Fixture(service: service)
        defer { fixture.close() }
        try await wait { fixture.session.selectedID == 1 }
        let editor = try fixture.searchEditor()
        editor.insertText("slow", replacementRange: NSRange(location: NSNotFound, length: 0))
        try await wait { await service.slowQueryCount == 1 }

        let typedAt = ContinuousClock.now
        editor.insertText("fast", replacementRange: NSRange(location: 0, length: editor.string.utf16.count))
        try await wait { await service.cancelledSlowQueryCount == 1 }
        let cancelledAfter = typedAt.duration(to: ContinuousClock.now)
        let issuedReplacement = await service.fastQueryCount

        XCTAssertLessThan(
            cancelledAfter, .milliseconds(150),
            "the typed query must cancel the read instead of waiting out the debounce"
        )
        XCTAssertEqual(issuedReplacement, 0, "the read was cancelled before the replacement query was issued")
        try await wait { fixture.session.selectedID == 200 }
    }

    /// F5: an occluded or miniaturized window keeps its SwiftUI view alive, so
    /// the visibility notification - not only `onDisappear` - must cancel the
    /// in-flight read.
    func testHiddenWindowCancelsTheInFlightRead() async throws {
        let service = HistoryKeyboardTestService()
        let window = HistoryOcclusionTestWindow(
            contentRect: NSRect(x: 0, y: 0, width: 450, height: 600),
            styleMask: [.titled], backing: .buffered, defer: false
        )
        let fixture = Fixture(service: service, hostWindow: window)
        let controller = TailSyncTransientWindowController(
            window: window,
            onVisibilityChange: { visible in
                NotificationCenter.default.post(
                    name: .tailSyncHistoryCollectionVisibilityChanged(for: "all"),
                    object: nil, userInfo: ["visible": visible]
                )
            },
            onClose: {}
        )
        defer {
            fixture.close()
            withExtendedLifetime(controller) {}
        }
        window.alphaValue = 0
        window.orderFront(nil)
        try await wait { fixture.session.selectedID == 1 }
        let editor = try fixture.searchEditor()
        editor.insertText("slow", replacementRange: NSRange(location: NSNotFound, length: 0))
        try await wait { await service.slowQueryCount == 1 }

        window.orderOut(nil)
        controller.windowDidChangeOcclusionState(Notification(
            name: NSWindow.didChangeOcclusionStateNotification, object: window
        ))
        try await wait { await service.cancelledSlowQueryCount == 1 }
    }

    func testHiddenDebounceDoesNotStartAReadAndReopensAtPageZero() async throws {
        let service = HistoryKeyboardTestService()
        let fixture = Fixture(service: service)
        defer { fixture.close() }
        try await wait { fixture.session.selectedID == 1 }
        fixture.session.focusList()
        for _ in 0..<30 { fixture.window.sendEvent(event(125, window: fixture.window)) }
        try await wait { fixture.session.selectedID == 31 }
        let editor = try fixture.searchEditor()
        editor.insertText("fast", replacementRange: NSRange(location: NSNotFound, length: 0))
        try await Task.sleep(for: .milliseconds(50))
        NotificationCenter.default.post(
            name: .tailSyncHistoryCollectionVisibilityChanged(for: "all"),
            object: nil, userInfo: ["visible": false]
        )
        try await Task.sleep(for: .milliseconds(300))
        let hiddenQueries = await service.loadQueries.filter { $0.keyword == "fast" }
        XCTAssertTrue(hiddenQueries.isEmpty, "the debounce must not start a daemon read in a hidden window")

        NotificationCenter.default.post(
            name: .tailSyncHistoryCollectionVisibilityChanged(for: "all"),
            object: nil, userInfo: ["visible": true]
        )
        try await wait { fixture.session.selectedID == 200 }
        let resumedQueries = await service.loadQueries.filter { $0.keyword == "fast" }
        XCTAssertEqual(resumedQueries.map(\.offset), [0], "the new search must retain its page-zero intent")
    }

    func testHiddenPagingReadResumesTheRequestedPage() async throws {
        let service = HistoryKeyboardTestService(holdPageTurns: true)
        let fixture = Fixture(service: service)
        defer { fixture.close() }
        try await wait { fixture.session.selectedID == 1 }
        fixture.session.focusList()
        for _ in 0..<30 { fixture.window.sendEvent(event(125, window: fixture.window)) }
        try await wait { await service.pendingPageQueryCount == 1 }
        NotificationCenter.default.post(
            name: .tailSyncHistoryCollectionVisibilityChanged(for: "all"),
            object: nil, userInfo: ["visible": false]
        )
        try await wait { await service.cancelledPageQueryCount == 1 }
        await service.allowPageTurns()
        NotificationCenter.default.post(
            name: .tailSyncHistoryCollectionVisibilityChanged(for: "all"),
            object: nil, userInfo: ["visible": true]
        )
        try await wait { fixture.session.selectedID == 31 }
        let offsets = await service.loadQueries.filter { $0.keyword == nil }.map(\.offset)
        XCTAssertEqual(offsets, [0, 30, 30], "reopening must resume the cancelled page, not the old displayed page")
    }

    /// F5: tearing the hosting view down must cancel the in-flight read instead
    /// of letting it finish into a discarded generation.
    func testDisappearingViewCancelsTheInFlightRead() async throws {
        let service = HistoryKeyboardTestService()
        let fixture = Fixture(service: service)
        defer { fixture.close() }
        try await wait { fixture.session.selectedID == 1 }
        let editor = try fixture.searchEditor()
        editor.insertText("slow", replacementRange: NSRange(location: NSNotFound, length: 0))
        try await wait { await service.slowQueryCount == 1 }

        fixture.close()
        try await wait { await service.cancelledSlowQueryCount == 1 }
    }

    /// The generation guard must keep rejecting a late answer from a dependency
    /// that ignores cancellation.
    func testLateNonCooperativeQueryCannotInstallResults() async throws {
        let service = HistoryKeyboardTestService()
        let fixture = Fixture(service: service)
        defer { fixture.close() }
        try await wait { fixture.session.selectedID == 1 }
        let editor = try fixture.searchEditor()
        editor.insertText("stubborn", replacementRange: NSRange(location: NSNotFound, length: 0))
        try await wait { await service.stubbornQueryCount == 1 }
        editor.insertText("fast", replacementRange: NSRange(location: 0, length: editor.string.utf16.count))
        try await wait { fixture.session.selectedID == 200 }

        try await Task.sleep(for: .milliseconds(700))
        XCTAssertEqual(fixture.session.selectedID, 200)
        XCTAssertEqual(editor.string, "fast")
    }

    /// Space opens the preview for the selected row without taking the responder
    /// from the history list, the preview follows a later selection change after
    /// its follow debounce, and Escape closes the preview first.
    func testPreviewFollowsSelectionWithoutStealingFocusAndEscapeClosesItFirst() async throws {
        let service = HistoryKeyboardTestService()
        let fixture = Fixture(service: service)
        let preview = HistoryPreviewWindowController.shared
        preview.close()
        defer {
            preview.close(ifOwnedBy: .history)
            fixture.close()
        }
        try await wait { fixture.session.selectedID == 1 }

        // First Down hands the responder to the list while row 1 stays selected.
        let editor = try fixture.searchEditor()
        editor.interpretKeyEvents([event(125, window: fixture.window)])
        XCTAssertEqual(fixture.session.selectedID, 1)
        XCTAssertTrue(fixture.window.firstResponder === fixture.session.listResponder)

        // Space opens the preview for row 1. A passive preview must not take the
        // responder away from the history list.
        fixture.window.sendEvent(event(49, window: fixture.window))
        try await wait { preview.isPreviewVisible(for: .history) }
        XCTAssertEqual(preview.viewModel.currentEntryId, 1)
        XCTAssertTrue(
            fixture.window.firstResponder === fixture.session.listResponder,
            "a passive preview must not steal the history responder"
        )

        // Moving the selection is followed after the debounce, not immediately.
        fixture.window.sendEvent(event(125, window: fixture.window))
        XCTAssertEqual(fixture.session.selectedID, 2)
        try await wait { preview.viewModel.currentEntryId == 2 }

        // Escape layers: the preview closes first and the history stays put.
        fixture.window.sendEvent(event(53, window: fixture.window))
        XCTAssertFalse(preview.isPreviewVisible(for: .history))
        XCTAssertEqual(fixture.session.selectedID, 2)
    }

    /// Tab from the list enters deep reading for the selected row even when the
    /// preview had not been opened yet.
    func testTabFromListEntersDeepReadPreviewForTheSelectedRow() async throws {
        let service = HistoryKeyboardTestService()
        let fixture = Fixture(service: service)
        let preview = HistoryPreviewWindowController.shared
        preview.close()
        defer {
            preview.close(ifOwnedBy: .history)
            fixture.close()
        }
        try await wait { fixture.session.selectedID == 1 }

        let editor = try fixture.searchEditor()
        editor.interpretKeyEvents([event(125, window: fixture.window)])
        fixture.window.sendEvent(event(125, window: fixture.window))
        XCTAssertEqual(fixture.session.selectedID, 2)

        // The list responder claims Tab before the key-view loop can move focus.
        fixture.window.sendEvent(event(48, window: fixture.window))
        try await wait { preview.isPreviewVisible(for: .history) }
        XCTAssertEqual(preview.viewModel.currentEntryId, 2)
    }

    func testCoveredHostKeepsPreviewOpenAndFollowingSelectionForBothCollections() async throws {
        for collection in ["all", "favorites"] {
            let service = HistoryKeyboardTestService()
            let window = HistoryOcclusionTestWindow(
                contentRect: NSRect(x: 0, y: 0, width: 450, height: 600),
                styleMask: [.titled], backing: .buffered, defer: false
            )
            let fixture = Fixture(service: service, collection: collection, hostWindow: window)
            let preview = HistoryPreviewWindowController.shared
            let owner = HistoryPreviewOwner(collection: collection)
            preview.shutdown()
            preview.registerHostWindow(window, for: owner)
            let controller = TailSyncTransientWindowController(
                window: window,
                onVisibilityChange: { visible in
                    NotificationCenter.default.post(
                        name: .tailSyncHistoryCollectionVisibilityChanged(for: collection),
                        object: nil, userInfo: ["visible": visible]
                    )
                },
                onClose: {}
            )
            defer {
                preview.shutdown()
                fixture.close()
                withExtendedLifetime(controller) {}
            }
            window.alphaValue = 0
            window.orderFront(nil)
            try await wait { fixture.session.selectedID == 1 }
            let editor = try fixture.searchEditor()
            editor.interpretKeyEvents([event(125, window: window)])
            window.sendEvent(event(49, window: window))
            try await wait { preview.isPreviewVisible(for: owner) }

            window.reportedOcclusionState = []
            controller.windowDidChangeOcclusionState(Notification(
                name: NSWindow.didChangeOcclusionStateNotification, object: window
            ))
            try await Task.sleep(for: .milliseconds(150))

            XCTAssertTrue(window.isVisible)
            XCTAssertTrue(preview.isPreviewVisible(for: owner), "Covering an open host must not close its preview")
            window.sendEvent(event(125, window: window))
            try await wait { preview.viewModel.currentEntryId == 2 }
            XCTAssertTrue(preview.isPreviewVisible(for: owner))

            window.orderOut(nil)
            controller.windowDidChangeOcclusionState(Notification(
                name: NSWindow.didChangeOcclusionStateNotification, object: window
            ))
            try await wait { !preview.hasAllocatedWindow }
        }
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
        case 49: characters = " "
        case 48: characters = "\t"
        case 3: characters = "f"
        case 51: characters = "\u{7F}"
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
        init(service: HistoryKeyboardTestService, collection: String = "all", hostWindow: NSWindow? = nil) {
            _ = NSApplication.shared
            let dependencies = HistoryViewDependencies(
                load: { try await service.load($0) },
                restore: { try await service.restore($0) },
                delete: { try await service.delete($0, collection: $1) },
                usesRuntimeUpdates: false
            )
            let view = HistoryView(collection: collection, dependencies: dependencies, keyboardSession: session)
            let hosting = NSHostingView(rootView: view)
            window = hostWindow ?? NSWindow(contentRect: NSRect(x: 0, y: 0, width: 450, height: 600),
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

private final class HistoryOcclusionTestWindow: NSWindow {
    var reportedOcclusionState: NSWindow.OcclusionState = [.visible]

    override var occlusionState: NSWindow.OcclusionState { reportedOcclusionState }
}

private actor HistoryKeyboardTestService {
    private(set) var restoreCount = 0
    private(set) var failedQueryCount = 0
    private(set) var slowQueryCount = 0
    private(set) var cancelledSlowQueryCount = 0
    private(set) var fastQueryCount = 0
    private(set) var stubbornQueryCount = 0
    private(set) var loadQueries: [HistoryQuery] = []
    private(set) var pendingPageQueryCount = 0
    private(set) var cancelledPageQueryCount = 0
    private var holdPageTurns: Bool
    let failRestore: Bool
    let pinned: Bool
    var failDelete: Bool
    private(set) var deletedIDs: [Int64] = []
    private(set) var deletedCollections: [String] = []
    private(set) var deleteFinishedCount = 0
    private var removedIDs: Set<Int64> = []
    init(failRestore: Bool = false, pinned: Bool = false, failDelete: Bool = false, holdPageTurns: Bool = false) {
        self.failRestore = failRestore
        self.pinned = pinned
        self.failDelete = failDelete
        self.holdPageTurns = holdPageTurns
    }
    func allowPageTurns() { holdPageTurns = false }
    func allowDelete() { failDelete = false }
    func delete(_ id: Int64, collection: String) async throws {
        deletedIDs.append(id)
        deletedCollections.append(collection)
        try await Task.sleep(for: .milliseconds(80))
        deleteFinishedCount += 1
        if failDelete { throw ApiError.serverError("test delete failed") }
        removedIDs.insert(id)
    }
    func load(_ query: HistoryQuery) async throws -> [HistoryEntry] {
        loadQueries.append(query)
        if query.keyword == nil && query.offset > 0 && holdPageTurns {
            pendingPageQueryCount += 1
            do { try await Task.sleep(for: .seconds(10)) }
            catch { cancelledPageQueryCount += 1; throw error }
        }
        if query.keyword == "slow" {
            slowQueryCount += 1
            do {
                try await Task.sleep(for: .milliseconds(600))
            } catch {
                cancelledSlowQueryCount += 1
                throw error
            }
            return try [entry(100)]
        }
        if query.keyword == "stubborn" {
            stubbornQueryCount += 1
            // Ignores cancellation and answers late, like a dependency that
            // does not cooperate with task cancellation.
            await withCheckedContinuation { continuation in
                DispatchQueue.global().asyncAfter(deadline: .now() + 0.6) {
                    continuation.resume()
                }
            }
            return try [entry(300)]
        }
        if query.keyword == "fast" {
            fastQueryCount += 1
            return try [entry(200)]
        }
        if query.keyword != nil {
            failedQueryCount += 1
            throw ApiError.serverError("test query failed")
        }
        return try (query.offset + 1...min(query.offset + query.limit, 32))
            .filter { !removedIDs.contains(Int64($0)) }.map { try entry($0) }
    }
    private func entry(_ id: Int) throws -> HistoryEntry {
        let data = Data("""
        {"id":\(id),"timestamp":"2026-10-03T00:00:00Z","type":"text",
         "description":"row \(id)","data_hash":"hash\(id)","size_bytes":4,"source_peer":"local","pinned":\(pinned)}
        """.utf8)
        return try JSONDecoder().decode(HistoryEntry.self, from: data)
    }
    func restore(_ id: Int64) async throws {
        restoreCount += 1
        try await Task.sleep(for: .milliseconds(80))
        if failRestore { throw ApiError.serverError("test restore failed") }
    }
}
