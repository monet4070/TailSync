import AppKit
import XCTest
@testable import TailSync

/// Real AppKit windows with injected text payloads and no daemon or clipboard
/// operations. Transparent hosts and takeFocus:false avoid app activation;
/// the preview is made transparent as soon as it is discovered.
@MainActor
final class HistoryPreviewKeyboardWindowTests: XCTestCase {
    func testPassivePresentationKeepsHostFirstResponderAcrossSelectionUpdates() async throws {
        let fixture = try PreviewKeyboardFixture()
        defer { fixture.dispose() }
        fixture.showHost(.history)
        XCTAssertTrue(fixture.history.firstResponder === fixture.historyResponder)

        let preview = try await fixture.present(id: 101)
        XCTAssertTrue(fixture.controller.isPreviewVisible(for: .history))
        XCTAssertTrue(preview.isVisible)
        XCTAssertFalse(preview.isKeyWindow)
        XCTAssertTrue(fixture.history.firstResponder === fixture.historyResponder)

        let reused = try await fixture.present(id: 102)
        XCTAssertTrue(reused === preview)
        XCTAssertEqual(fixture.model.currentEntryId, 102)
        XCTAssertTrue(fixture.history.firstResponder === fixture.historyResponder)
    }

    func testNativeKeyOwnershipAcrossPassivePresentationFocusAndEscape() async throws {
        guard ProcessInfo.processInfo.environment["TAILSYNC_GUI_FOCUS_TESTS"] == "1" else {
            throw XCTSkip("Native key-window ownership requires an active GUI test host; opt in with TAILSYNC_GUI_FOCUS_TESTS=1 for real-machine acceptance")
        }
        let fixture = try PreviewKeyboardFixture()
        defer { fixture.dispose() }
        fixture.showHost(.history, takeKeyFocus: true)
        XCTAssertTrue(fixture.history.isKeyWindow, "The native test host must acquire key status before exercising product code")
        XCTAssertTrue(NSApp.keyWindow === fixture.history)
        var returnCount = 0
        fixture.controller.registerKeyboardHandlers(
            for: .history, onRestore: nil, onReturn: { returnCount += 1 }
        )
        let preview = try await fixture.present(id: 151)
        XCTAssertTrue(fixture.history.isKeyWindow)
        XCTAssertTrue(NSApp.keyWindow === fixture.history)
        XCTAssertFalse(preview.isKeyWindow)
        XCTAssertTrue(fixture.history.firstResponder === fixture.historyResponder)
        _ = try await fixture.readOnlyContent(in: preview)
        fixture.controller.focus(for: .history)
        XCTAssertTrue(preview.isKeyWindow)
        XCTAssertTrue(NSApp.keyWindow === preview)
        preview.sendEvent(try keyEvent(53, in: preview))
        XCTAssertTrue(fixture.history.isKeyWindow)
        XCTAssertTrue(NSApp.keyWindow === fixture.history)
        XCTAssertTrue(fixture.history.firstResponder === fixture.historyResponder)
        XCTAssertEqual(returnCount, 1)
    }

    func testFocusShowsAllocatedWindowEvenWhenNotYetVisible() async throws {
        let fixture = try PreviewKeyboardFixture()
        defer { fixture.dispose() }
        let preview = try await fixture.present(id: 175)
        let content = try await fixture.readOnlyContent(in: preview)
        preview.orderOut(nil)
        XCTAssertFalse(preview.isVisible)
        XCTAssertTrue(fixture.controller.hasAllocatedWindow)
        fixture.controller.focus(for: .history)
        XCTAssertTrue(preview.isVisible)
        XCTAssertTrue(preview.firstResponder === content)
    }

    func testFocusEntersRenderedReadOnlyContentForDeepReading() async throws {
        let fixture = try PreviewKeyboardFixture()
        defer { fixture.dispose() }
        fixture.showHost(.history)
        let preview = try await fixture.present(id: 201)
        let content = try await fixture.readOnlyContent(in: preview)
        let originalResponder = preview.firstResponder
        fixture.controller.focus(for: .favorites)
        XCTAssertTrue(preview.firstResponder === originalResponder)
        fixture.controller.focus(for: .history)

        XCTAssertTrue(preview.firstResponder === content)
        XCTAssertFalse(content.isEditable)
        XCTAssertTrue(content.isSelectable)
        XCTAssertTrue(content.string.contains("preview body 201"))
        XCTAssertTrue(fixture.history.firstResponder === fixture.historyResponder)
    }

    func testFocusWhileLoadingEntersContentWhenItArrives() async throws {
        let fixture = try PreviewKeyboardFixture(loadDelayNanoseconds: 100_000_000)
        defer { fixture.dispose() }
        let preview = try await fixture.present(id: 251, waitUntilReady: false)
        fixture.controller.focus(for: .history)
        let content = try await fixture.readOnlyContent(in: preview)
        for _ in 0..<100 {
            if preview.firstResponder === content { break }
            try await Task.sleep(for: .milliseconds(10))
        }
        XCTAssertTrue(preview.firstResponder === content)
    }

    func testPendingDeepFocusDoesNotOverrideUserEditing() async throws {
        let fixture = try PreviewKeyboardFixture(loadDelayNanoseconds: 100_000_000)
        defer { fixture.dispose() }
        let preview = try await fixture.present(id: 252, waitUntilReady: false)
        fixture.controller.focus(for: .history)
        let editor = NSTextView(frame: NSRect(x: 0, y: 0, width: 100, height: 30))
        editor.isEditable = true
        preview.contentView?.addSubview(editor)
        XCTAssertTrue(preview.makeFirstResponder(editor))
        _ = try await fixture.readOnlyContent(in: preview)
        try await Task.sleep(for: .milliseconds(100))
        XCTAssertTrue(preview.firstResponder === editor)
    }

    func testPreviewReturnAndCommandReturnDispatchCurrentIDToHostRestore() async throws {
        let fixture = try PreviewKeyboardFixture()
        defer { fixture.dispose() }
        var calls: [PreviewRestoreCall] = []
        fixture.controller.registerKeyboardHandlers(
            for: .history,
            onRestore: { calls.append(.init(id: $0, keepOpen: $1)) }, onReturn: nil
        )
        let preview = try await fixture.present(id: 301)
        _ = try await fixture.readOnlyContent(in: preview)
        fixture.controller.focus(for: .history)
        for code: UInt16 in [36, 76] {
            preview.sendEvent(try keyEvent(code, in: preview))
            preview.sendEvent(try keyEvent(code, in: preview, modifiers: .command))
        }
        XCTAssertEqual(calls, [
            .init(id: 301, keepOpen: false), .init(id: 301, keepOpen: true),
            .init(id: 301, keepOpen: false), .init(id: 301, keepOpen: true)
        ])
        XCTAssertTrue(fixture.controller.hasAllocatedWindow)

        _ = try await fixture.present(id: 302)
        _ = try await fixture.readOnlyContent(in: preview)
        fixture.controller.focus(for: .history)
        preview.sendEvent(try keyEvent(36, in: preview))
        XCTAssertEqual(calls.last, .init(id: 302, keepOpen: false))
        let standaloneRestores = await fixture.dependencyRestores.calls()
        XCTAssertEqual(standaloneRestores, [], "Host callbacks must bypass the standalone restore dependency")
    }

    func testHostedRestoreStateSuppressesReturnWhileRestoringAndAllowsRetry() async throws {
        let fixture = try PreviewKeyboardFixture()
        defer { fixture.dispose() }
        var calls: [PreviewRestoreCall] = []
        fixture.controller.registerKeyboardHandlers(
            for: .history,
            onRestore: { calls.append(.init(id: $0, keepOpen: $1)) }, onReturn: nil
        )
        let preview = try await fixture.present(id: 401)
        _ = try await fixture.readOnlyContent(in: preview)
        fixture.controller.focus(for: .history)
        fixture.controller.setRestoreState(.restoring, for: .history)
        fixture.controller.setRestoreState(.idle, for: .favorites)
        XCTAssertEqual(fixture.model.restoreState, .restoring)
        preview.sendEvent(try keyEvent(36, in: preview))
        preview.sendEvent(try keyEvent(36, in: preview, modifiers: .command))
        fixture.model.restoreCurrent()
        XCTAssertTrue(calls.isEmpty)

        fixture.controller.setRestoreState(.failed, for: .history)
        preview.sendEvent(try keyEvent(36, in: preview))
        XCTAssertEqual(calls, [.init(id: 401, keepOpen: false)])
        let standaloneRestores = await fixture.dependencyRestores.calls()
        XCTAssertEqual(standaloneRestores, [])
    }

    func testEscapeClosesPreviewReturnsKeyToHostAndInvokesOnReturn() async throws {
        let fixture = try PreviewKeyboardFixture()
        defer { fixture.dispose() }
        var returnCount = 0
        var restoreCount = 0
        fixture.controller.registerKeyboardHandlers(
            for: .history, onRestore: { _, _ in restoreCount += 1 },
            onReturn: {
                returnCount += 1
                fixture.history.makeFirstResponder(fixture.historyResponder)
            }
        )
        fixture.showHost(.history)
        let preview = try await fixture.present(id: 501)
        _ = try await fixture.readOnlyContent(in: preview)
        fixture.controller.focus(for: .history)
        preview.sendEvent(try keyEvent(53, in: preview))

        XCTAssertEqual(returnCount, 1)
        XCTAssertEqual(restoreCount, 0)
        XCTAssertTrue(fixture.history.firstResponder === fixture.historyResponder)
        XCTAssertFalse(fixture.controller.hasAllocatedWindow)
        XCTAssertFalse(preview.isVisible)
        XCTAssertNil(preview.contentViewController)
        fixture.assertSensitiveContentCleared()
    }

    func testEditableFieldKeepsPlainReturnButCommandReturnUsesHostRestore() async throws {
        let fixture = try PreviewKeyboardFixture()
        defer { fixture.dispose() }
        var calls: [PreviewRestoreCall] = []
        fixture.controller.registerKeyboardHandlers(
            for: .history,
            onRestore: { calls.append(.init(id: $0, keepOpen: $1)) }, onReturn: nil
        )
        let preview = try await fixture.present(id: 601)
        let field = NSTextField(frame: NSRect(x: 8, y: 8, width: 180, height: 24))
        let delegate = PreviewSearchCommandRecorder()
        field.stringValue = "in-preview query"
        field.delegate = delegate
        defer { field.delegate = nil; field.removeFromSuperview() }
        preview.contentView?.addSubview(field)
        XCTAssertTrue(preview.makeFirstResponder(field))
        let editor = try XCTUnwrap(field.currentEditor() as? NSTextView)
        XCTAssertTrue(editor.isEditable)
        XCTAssertTrue(preview.firstResponder === editor)
        preview.sendEvent(try keyEvent(36, in: preview))
        XCTAssertTrue(calls.isEmpty, "Plain Return belongs to the editable search field")
        XCTAssertTrue(delegate.commands.contains("insertNewline:"))
        XCTAssertTrue(preview.firstResponder === editor)

        preview.sendEvent(try keyEvent(36, in: preview, modifiers: .command))
        XCTAssertEqual(calls, [.init(id: 601, keepOpen: true)])
        XCTAssertTrue(fixture.controller.hasAllocatedWindow)
        let standaloneRestores = await fixture.dependencyRestores.calls()
        XCTAssertEqual(standaloneRestores, [])
    }

    func testEditableMarkedTextDefersReturnCommandReturnEscapeAndNavigation() async throws {
        let fixture = try PreviewKeyboardFixture()
        defer { fixture.dispose() }
        var calls: [PreviewRestoreCall] = []
        var returnCount = 0
        fixture.controller.registerKeyboardHandlers(
            for: .history,
            onRestore: { calls.append(.init(id: $0, keepOpen: $1)) },
            onReturn: { returnCount += 1 }
        )
        let preview = try await fixture.present(id: 701)
        // Real marked text; consume forwarded keys in a recording native editor
        // to isolate the window gate from the installed keyboard layout / IME.
        let editor = PreviewMarkedTextRecorder(frame: NSRect(x: 8, y: 8, width: 200, height: 40))
        editor.isEditable = true
        editor.isSelectable = true
        preview.contentView?.addSubview(editor)
        defer { editor.unmarkText(); editor.removeFromSuperview() }
        XCTAssertTrue(preview.makeFirstResponder(editor))
        editor.setMarkedText(
            "中文组词", selectedRange: NSRange(location: 2, length: 0),
            replacementRange: NSRange(location: NSNotFound, length: 0)
        )
        XCTAssertTrue(editor.hasMarkedText())
        let markedRange = editor.markedRange()
        let events: [(UInt16, NSEvent.ModifierFlags)] = [
            (36, []), (76, []), (36, .command), (76, .command),
            (53, []), (49, []), (125, []), (126, []), (123, .option), (124, .option)
        ]
        for (code, modifiers) in events {
            preview.sendEvent(try keyEvent(code, in: preview, modifiers: modifiers))
            XCTAssertTrue(editor.hasMarkedText(), "keyCode \(code)")
            XCTAssertEqual(editor.markedRange(), markedRange)
            XCTAssertTrue(preview.firstResponder === editor)
            XCTAssertTrue(fixture.controller.isPreviewVisible(for: .history))
            XCTAssertEqual(fixture.model.currentEntryId, 701)
        }
        XCTAssertEqual(editor.receivedKeyCodes, events.map { $0.0 }, "Every marked-text event must reach the editor")
        XCTAssertTrue(calls.isEmpty)
        XCTAssertEqual(returnCount, 0)
        let standaloneRestores = await fixture.dependencyRestores.calls()
        XCTAssertEqual(standaloneRestores, [])

        editor.unmarkText()
        XCTAssertFalse(editor.hasMarkedText())
        preview.sendEvent(try keyEvent(36, in: preview, modifiers: .command))
        XCTAssertEqual(calls, [.init(id: 701, keepOpen: true)], "Ending composition releases the keyboard gate")
    }

    func testOwnerSwitchIsolatesFocusRestoreReturnStateAndClose() async throws {
        let fixture = try PreviewKeyboardFixture()
        defer { fixture.dispose() }
        var historyCalls: [PreviewRestoreCall] = []
        var favoriteCalls: [PreviewRestoreCall] = []
        var historyReturns = 0
        var favoriteReturns = 0
        fixture.controller.registerKeyboardHandlers(
            for: .history, onRestore: { historyCalls.append(.init(id: $0, keepOpen: $1)) },
            onReturn: { historyReturns += 1 }
        )
        fixture.controller.registerKeyboardHandlers(
            for: .favorites, onRestore: { favoriteCalls.append(.init(id: $0, keepOpen: $1)) },
            onReturn: {
                favoriteReturns += 1
                fixture.favorites.makeFirstResponder(fixture.favoritesResponder)
            }
        )
        let preview = try await fixture.present(id: 801, owner: .history)
        fixture.showHost(.favorites)
        let reused = try await fixture.present(id: 802, owner: .favorites)
        XCTAssertTrue(reused === preview)
        XCTAssertTrue(fixture.controller.isPreviewVisible(for: .favorites))
        XCTAssertFalse(fixture.controller.isPreviewVisible(for: .history))
        let originalResponder = preview.firstResponder
        fixture.controller.close(ifOwnedBy: .history)
        fixture.controller.registerHostWindow(nil, for: .history)
        fixture.controller.focus(for: .history)
        XCTAssertTrue(preview.firstResponder === originalResponder)
        fixture.controller.setRestoreState(.restoring, for: .history)
        XCTAssertTrue(fixture.controller.hasAllocatedWindow)
        XCTAssertEqual(fixture.model.restoreState, .idle)
        fixture.model.restoreCurrent()
        XCTAssertEqual(favoriteCalls, [.init(id: 802, keepOpen: true)])
        XCTAssertTrue(historyCalls.isEmpty)

        _ = try await fixture.readOnlyContent(in: preview)
        fixture.controller.focus(for: .favorites)
        preview.sendEvent(try keyEvent(36, in: preview))
        XCTAssertEqual(favoriteCalls.last, .init(id: 802, keepOpen: false))
        XCTAssertTrue(historyCalls.isEmpty)
        preview.sendEvent(try keyEvent(53, in: preview))
        XCTAssertEqual(favoriteReturns, 1)
        XCTAssertEqual(historyReturns, 0)
        XCTAssertTrue(fixture.favorites.firstResponder === fixture.favoritesResponder)
        fixture.assertSensitiveContentCleared()
        let standaloneRestores = await fixture.dependencyRestores.calls()
        XCTAssertEqual(standaloneRestores, [])
    }

    func testProgrammaticCloseClearsWindowContentCallbacksAndSessionAndRecreatesWindow() async throws {
        let fixture = try PreviewKeyboardFixture()
        defer { fixture.dispose() }
        var calls: [PreviewRestoreCall] = []
        fixture.controller.registerKeyboardHandlers(
            for: .history,
            onRestore: { calls.append(.init(id: $0, keepOpen: $1)) }, onReturn: nil
        )
        let oldWindow = try await fixture.present(id: 901)
        XCTAssertNotNil(fixture.session.text)
        XCTAssertNotNil(fixture.model.onHostedRestore)
        fixture.controller.close()
        fixture.controller.close()
        XCTAssertFalse(fixture.controller.hasAllocatedWindow)
        XCTAssertFalse(fixture.controller.isPreviewVisible)
        XCTAssertFalse(oldWindow.isVisible)
        XCTAssertNil(oldWindow.delegate)
        XCTAssertNil(oldWindow.contentViewController)
        fixture.assertSensitiveContentCleared()
        oldWindow.sendEvent(try keyEvent(36, in: oldWindow, modifiers: .command))
        XCTAssertTrue(calls.isEmpty, "A retained closed window must not keep a usable restore callback")

        let recreated = try await fixture.present(id: 902)
        XCTAssertFalse(recreated === oldWindow)
        _ = try await fixture.readOnlyContent(in: recreated)
        fixture.controller.focus(for: .history)
        recreated.sendEvent(try keyEvent(36, in: recreated))
        XCTAssertEqual(calls, [.init(id: 902, keepOpen: false)])
        let standaloneRestores = await fixture.dependencyRestores.calls()
        XCTAssertEqual(standaloneRestores, [])
    }

    func testNativeWindowCloseDisposesSensitiveContentAndHostedRestore() async throws {
        let fixture = try PreviewKeyboardFixture()
        defer { fixture.dispose() }
        let preview = try await fixture.present(id: 1001)
        XCTAssertNotNil(fixture.session.payload)
        preview.performClose(nil)
        await Task.yield()
        XCTAssertFalse(fixture.controller.hasAllocatedWindow)
        XCTAssertFalse(preview.isVisible)
        XCTAssertNil(preview.contentViewController)
        fixture.assertSensitiveContentCleared()
    }

    private func keyEvent(
        _ code: UInt16, in window: NSWindow,
        modifiers: NSEvent.ModifierFlags = []
    ) throws -> NSEvent {
        let characters: String
        switch code {
        case 36, 76: characters = "\r"
        case 53: characters = "\u{1B}"
        case 49: characters = " "
        case 125: characters = "\u{F701}"
        case 126: characters = "\u{F700}"
        case 123: characters = "\u{F702}"
        case 124: characters = "\u{F703}"
        default: characters = ""
        }
        return try XCTUnwrap(NSEvent.keyEvent(
            with: .keyDown, location: .zero, modifierFlags: modifiers,
            timestamp: 0, windowNumber: window.windowNumber, context: nil,
            characters: characters, charactersIgnoringModifiers: characters,
            isARepeat: false, keyCode: code
        ))
    }
}

private struct PreviewRestoreCall: Equatable {
    let id: Int64
    let keepOpen: Bool
}

private actor PreviewDependencyRestoreRecorder {
    private var ids: [Int64] = []
    func record(_ id: Int64) { ids.append(id) }
    func calls() -> [Int64] { ids }
}

@MainActor
private final class PreviewKeyboardFixture {
    let directory: URL
    let session: HistoryPreviewSession
    let dependencyRestores = PreviewDependencyRestoreRecorder()
    let model: HistoryPreviewViewModel
    let controller: HistoryPreviewWindowController
    let history: NSWindow
    let favorites: NSWindow
    let historyResponder = PreviewHostFirstResponder()
    let favoritesResponder = PreviewHostFirstResponder()

    init(loadDelayNanoseconds: UInt64 = 0) throws {
        _ = NSApplication.shared
        directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("tailsync-preview-keyboard-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        session = HistoryPreviewSession(store: HistoryPreviewStore(directory: directory))
        let recorder = dependencyRestores
        model = HistoryPreviewViewModel(
            session: session,
            dependencies: HistoryPreviewDependencies(
                load: { id, _ in
                    if loadDelayNanoseconds > 0 {
                        try await Task.sleep(nanoseconds: loadDelayNanoseconds)
                    }
                    let data = Data("preview body \(id)\nSecond line for deep reading.".utf8)
                    return HistoryPreviewData(
                        kind: "text", name: "keyboard-\(id).txt",
                        sizeBytes: Int64(data.count), data: data, entryId: id
                    )
                },
                restore: { id in await recorder.record(id) }
            )
        )
        controller = HistoryPreviewWindowController(viewModel: model)
        history = Self.host(title: "Keyboard test history", responder: historyResponder)
        favorites = Self.host(title: "Keyboard test favorites", responder: favoritesResponder)
        controller.registerHostWindow(history, for: .history)
        controller.registerHostWindow(favorites, for: .favorites)
    }

    func showHost(_ owner: HistoryPreviewOwner, takeKeyFocus: Bool = false) {
        let host = owner == .history ? history : favorites
        let responder = owner == .history ? historyResponder : favoritesResponder
        // Ordinary tests need a visible native window for return-to-host, not
        // foreground ownership. Only the opt-in GUI test asks it to become key.
        //
        // NOTE: `makeKeyAndOrderFront` can only make this window key while the
        // process is the ACTIVE application, and a plain `swift test` process
        // is not a bundled .app, so the WindowServer refuses activation
        // (`setActivationPolicy(.regular)` succeeds but `NSApp.isActive` stays
        // false). This opt-in test therefore requires a bundled GUI test host;
        // under `swift test` it fails its own precondition at line 35 rather
        // than exercising product code. See docs/acceptance/ for the real
        // end-to-end path used instead.
        if takeKeyFocus {
            host.makeKeyAndOrderFront(nil)
        } else {
            host.orderFront(nil)
        }
        XCTAssertTrue(host.makeFirstResponder(responder))
    }

    func present(id: Int64, owner: HistoryPreviewOwner = .history, waitUntilReady: Bool = true) async throws -> NSWindow {
        controller.present(HistoryPreviewRequest(items: [HistoryPreviewItem(
            id: id, batchId: nil, batchIndex: nil, batchCount: nil,
            category: "text", type: "text", nameHint: "keyboard-\(id).txt", sizeBytes: 48
        )], selectedIndex: 0), owner: owner, takeFocus: false)
        let preview = try XCTUnwrap(NSApp.windows.first {
            ($0.delegate as AnyObject?) === controller
        }, "Expected this controller's native preview window")
        preview.alphaValue = 0
        if waitUntilReady {
            try await waitUntil {
                if case .ready = self.model.state { return self.model.currentEntryId == id }
                return false
            }
        }
        return preview
    }

    func readOnlyContent(in window: NSWindow) async throws -> NSTextView {
        var content: NSTextView?
        try await waitUntil {
            window.contentView?.layoutSubtreeIfNeeded()
            content = Self.readOnlyText(in: window.contentView)
            return content != nil
        }
        return try XCTUnwrap(content, "Expected actual rendered NSTextView")
    }

    func assertSensitiveContentCleared(file: StaticString = #filePath, line: UInt = #line) {
        XCTAssertEqual(model.state, .idle, file: file, line: line)
        XCTAssertEqual(model.restoreState, .idle, file: file, line: line)
        XCTAssertTrue(model.items.isEmpty, file: file, line: line)
        XCTAssertNil(model.currentEntryId, file: file, line: line)
        XCTAssertNil(model.onHostedRestore, file: file, line: line)
        XCTAssertNil(session.text, file: file, line: line)
        XCTAssertNil(session.payload, file: file, line: line)
        XCTAssertNil(session.material, file: file, line: line)
        XCTAssertNil(session.fileURL, file: file, line: line)
        XCTAssertFalse(model.isRenderingSVG, file: file, line: line)
        XCTAssertFalse(model.svgExternalResourcesTrusted, file: file, line: line)
        XCTAssertNil(model.svgVisualFallback, file: file, line: line)
    }

    func dispose() {
        controller.registerKeyboardHandlers(for: .history, onRestore: nil, onReturn: nil)
        controller.registerKeyboardHandlers(for: .favorites, onRestore: nil, onReturn: nil)
        controller.shutdown()
        history.close()
        favorites.close()
        try? FileManager.default.removeItem(at: directory)
    }

    private func waitUntil(_ predicate: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(3)
        while !predicate() {
            if Date() >= deadline {
                throw NSError(domain: "PreviewKeyboardTests", code: 1,
                              userInfo: [NSLocalizedDescriptionKey: "Timed out waiting for the native preview"])
            }
            try await Task.sleep(nanoseconds: 10_000_000)
        }
    }

    private static func host(title: String, responder: NSView) -> NSWindow {
        let window = NSWindow(
            contentRect: NSRect(x: -10_000, y: -10_000, width: 320, height: 240),
            styleMask: [.titled], backing: .buffered, defer: false
        )
        window.isReleasedWhenClosed = false
        window.alphaValue = 0
        window.title = title
        responder.frame = NSRect(x: 0, y: 0, width: 300, height: 200)
        window.contentView?.addSubview(responder)
        return window
    }

    private static func readOnlyText(in root: NSView?) -> NSTextView? {
        guard let root else { return nil }
        if let text = root as? NSTextView, !text.isEditable { return text }
        return root.subviews.lazy.compactMap { readOnlyText(in: $0) }.first
    }
}

private final class PreviewHostFirstResponder: NSView {
    override var acceptsFirstResponder: Bool { true }
}

private final class PreviewSearchCommandRecorder: NSObject, NSTextFieldDelegate {
    var commands: [String] = []
    func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
        commands.append(NSStringFromSelector(selector))
        return true
    }
}

private final class PreviewMarkedTextRecorder: NSTextView {
    var receivedKeyCodes: [UInt16] = []
    override func keyDown(with event: NSEvent) {
        receivedKeyCodes.append(event.keyCode)
    }
}
