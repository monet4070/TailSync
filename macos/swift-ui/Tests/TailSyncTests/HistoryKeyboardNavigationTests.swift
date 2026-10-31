import AppKit
import SwiftUI
import XCTest
@testable import TailSync

/// These tests create only hidden, in-memory AppKit controls. They never launch
/// TailSync, install an event monitor, or restore anything to the pasteboard.
@MainActor
final class HistoryKeyboardNavigationTests: XCTestCase {
    func testNativeSearchDownTransfersFirstResponderWithoutSkippingFirstID() throws {
        let fixture = SearchFixture()
        defer { fixture.dispose() }
        let ids: [Int64] = [901, 407, 83]
        let session = fixture.session
        session.reconcile(ids: ids)
        var commands: [HistoryKeyboardCommand] = []
        var restoredIDs: [Int64] = []

        // Model the host's contract: the first down transfers focus; subsequent
        // downs move by stable ID. This does not claim to test HistoryView itself.
        session.onCommand = { command in
            commands.append(command)
            switch command {
            case .down:
                if session.focus == .search {
                    session.focusList()
                } else if let selectedID = session.selectedID,
                          let index = ids.firstIndex(of: selectedID) {
                    session.selectedID = ids[min(index + 1, ids.count - 1)]
                }
            case .restore:
                if let selectedID = session.selectedID { restoredIDs.append(selectedID) }
            default:
                break
            }
        }
        let editor = try fixture.focusSearch()
        XCTAssertEqual(session.selectedID, ids[0])
        XCTAssertTrue(fixture.window.firstResponder === editor)

        // Use the real field editor's key interpreter, not a delegate call.
        editor.interpretKeyEvents([try keyEvent(125, in: fixture.window)])
        XCTAssertEqual(commands, [.down])
        XCTAssertEqual(session.focus, .list)
        XCTAssertEqual(session.selectedID, ids[0], "The focus-transfer down must not advance to row 1")
        XCTAssertTrue(fixture.window.firstResponder === fixture.list)

        // Dispatch the remaining events through the window's responder chain.
        fixture.window.sendEvent(try keyEvent(125, in: fixture.window, repeatKey: true))
        XCTAssertEqual(session.selectedID, ids[1])
        fixture.window.sendEvent(try keyEvent(125, in: fixture.window, repeatKey: true))
        XCTAssertEqual(session.selectedID, ids[2])
        fixture.window.sendEvent(try keyEvent(36, in: fixture.window))
        XCTAssertEqual(commands, [.down, .down, .down, .restore(keepOpen: false)])
        XCTAssertEqual(restoredIDs, [ids[2]])
        XCTAssertTrue(fixture.window.firstResponder === fixture.list)
    }

    func testNativeSearchFieldUsesThemeColorForInsertionPoint() throws {
        let selection = TailSyncThemeSelection(builtin: .tailsync)
        let fixture = SearchFixture(selection: selection)
        defer { fixture.dispose() }
        let editor = try fixture.focusSearch()
        XCTAssertEqual(editor.insertionPointColor, NSColor(selection.palette(for: .light).accentColor))
    }

    func testNativeSearchSelectionUsesThemeAndRestoresSharedEditorDefaults() throws {
        let fixture = SearchFixture()
        defer { fixture.dispose() }
        // Inspect the native editor before it belongs to the search field.
        let nativeEditor = try XCTUnwrap(fixture.window.fieldEditor(true, for: fixture.field) as? NSTextView)
        let originalCaret = nativeEditor.insertionPointColor
        let originalSelection = nativeEditor.selectedTextAttributes as NSDictionary
        let editor = try fixture.focusSearch()
        XCTAssertTrue(nativeEditor === editor)
        let appearance = fixture.coordinator.parent.editorAppearance
        XCTAssertEqual(editor.selectedTextAttributes[.backgroundColor] as? NSColor, appearance.selectionBackgroundColor)
        XCTAssertEqual(editor.selectedTextAttributes[.foregroundColor] as? NSColor, appearance.selectionTextColor)
        fixture.session.focusList()
        XCTAssertEqual(editor.insertionPointColor, originalCaret)
        XCTAssertEqual(editor.selectedTextAttributes as NSDictionary, originalSelection)
        _ = try fixture.focusSearch()
        XCTAssertEqual(editor.insertionPointColor, appearance.insertionPointColor, "Refocusing reapplies the search theme")
    }

    func testThemeChangeUpdatesActiveEditorWithoutChangingCompositionOrSelection() throws {
        let fixture = SearchFixture(text: "draft")
        defer { fixture.dispose() }
        let editor = try fixture.focusSearch()
        editor.setMarkedText("拼音", selectedRange: NSRange(location: 1, length: 0), replacementRange: editor.selectedRange())
        let text = editor.string
        let range = editor.selectedRange()
        let markedRange = editor.markedRange()
        let appearance = HistorySearchEditorAppearance(selection: TailSyncThemeSelection(builtin: .forest), scheme: .dark)
        fixture.field.editorAppearance = appearance
        XCTAssertEqual(editor.insertionPointColor, appearance.insertionPointColor)
        XCTAssertEqual(editor.selectedTextAttributes[.backgroundColor] as? NSColor, appearance.selectionBackgroundColor)
        XCTAssertEqual(editor.string, text)
        XCTAssertEqual(editor.selectedRange(), range)
        XCTAssertEqual(editor.markedRange(), markedRange)
        XCTAssertTrue(editor.hasMarkedText())
    }

    func testCaretAndRowFocusResolveBuiltInAndCustomThemeTokens() {
        for builtin in TailSyncColorTheme.allCases {
            for scheme: ColorScheme in [.light, .dark] {
                let selection = TailSyncThemeSelection(builtin: builtin)
                let appearance = HistorySearchEditorAppearance(selection: selection, scheme: scheme)
                let style = HistoryRowFocusStyle(selection: selection, scheme: scheme)
                XCTAssertEqual(appearance.insertionPointColor, NSColor(selection.palette(for: scheme).accentColor))
                XCTAssertEqual(NSColor(style.color), NSColor(selection.palette(for: scheme).accentColor))
                XCTAssertEqual(style.cornerRadius, selection.metrics(for: scheme).controlRadius)
                XCTAssertEqual(style.lineWidth, builtin == .highContrast ? 3 : 2)
            }
        }
        let definition = TailSyncThemeDefinition.resolvedV2(id: "custom:focus", light: [
            "components": [
                "history": ["focus": ["focusRing": "#287A55", "radius": 11]],
                "search": ["focus": ["focusRing": "#B83E60"]]
            ]
        ], dark: [
            "components": [
                "history": ["focus": ["focusRing": "#69CA91", "radius": 13]],
                "search": ["focus": ["focusRing": "#EF86A2"]]
            ]
        ])
        let selection = TailSyncThemeSelection(builtin: .tailsync, definition: definition)
        let lightStyle = HistoryRowFocusStyle(selection: selection, scheme: .light)
        let darkStyle = HistoryRowFocusStyle(selection: selection, scheme: .dark)
        XCTAssertEqual(lightStyle.color, definition.components["history"]?["focus"]?.focusRingColor)
        XCTAssertEqual(darkStyle.color, definition.darkComponents["history"]?["focus"]?.focusRingColor)
        XCTAssertEqual(lightStyle.cornerRadius, 11)
        XCTAssertEqual(darkStyle.cornerRadius, 13)
        XCTAssertEqual(
            HistorySearchEditorAppearance(selection: selection, scheme: .light).insertionPointColor,
            NSColor(definition.components["search"]?["focus"]?.focusRingColor ?? .clear)
        )
        XCTAssertEqual(
            HistorySearchEditorAppearance(selection: selection, scheme: .dark).insertionPointColor,
            NSColor(definition.darkComponents["search"]?["focus"]?.focusRingColor ?? .clear)
        )
    }

    func testNativeFieldEditorDispatchesUpDownReturnAndEscapeToHandler() throws {
        let fixture = SearchFixture()
        defer { fixture.dispose() }
        var commands: [HistoryKeyboardCommand] = []
        fixture.session.onCommand = { commands.append($0) }
        let editor = try fixture.focusSearch()

        for code: UInt16 in [125, 126, 36, 53] {
            editor.interpretKeyEvents([try keyEvent(code, in: fixture.window)])
        }

        XCTAssertEqual(commands, [.down, .up, .restore(keepOpen: false), .escape])
        XCTAssertTrue(fixture.window.firstResponder === editor)
    }

    func testNativeFieldEditorSpaceRemainsSearchTextRatherThanPreview() throws {
        let fixture = SearchFixture(text: "")
        defer { fixture.dispose() }
        var commands: [HistoryKeyboardCommand] = []
        fixture.session.onCommand = { commands.append($0) }
        let editor = try fixture.focusSearch()

        editor.interpretKeyEvents([try keyEvent(49, in: fixture.window)])

        XCTAssertEqual(editor.string, " ")
        XCTAssertTrue(commands.isEmpty)
        XCTAssertTrue(fixture.window.firstResponder === editor)
    }

    func testMarkedTextIsHardGateForEverySearchDelegateCommand() throws {
        var submissions = 0
        let fixture = SearchFixture(text: "", onSubmit: { submissions += 1 })
        defer { fixture.dispose() }
        var commands: [HistoryKeyboardCommand] = []
        fixture.session.onCommand = { commands.append($0) }
        fixture.session.selectedID = 901
        let editor = try fixture.focusSearch()
        editor.setMarkedText(
            "中文组词",
            selectedRange: NSRange(location: 2, length: 0),
            replacementRange: NSRange(location: NSNotFound, length: 0)
        )
        XCTAssertTrue(editor.hasMarkedText(), "Exercise real marked text, not a stub")
        // Deliberately stale: the delegate must consult the live field editor.
        fixture.session.isComposing = false
        let markedRange = editor.markedRange()
        let selection = editor.selectedRange()
        let text = editor.string

        for selector in [
            "moveDown:", "moveUp:", "insertNewline:", "cancelOperation:",
            "insertSpace:", "insertTab:"
        ] {
            XCTAssertFalse(fixture.command(selector, editor: editor), selector)
            XCTAssertTrue(editor.hasMarkedText(), selector)
            XCTAssertEqual(editor.markedRange(), markedRange, selector)
            XCTAssertEqual(editor.selectedRange(), selection, selector)
            XCTAssertEqual(editor.string, text, selector)
            XCTAssertEqual(fixture.session.focus, .search, selector)
            XCTAssertEqual(fixture.session.selectedID, 901, selector)
            XCTAssertTrue(fixture.window.firstResponder === editor, selector)
        }
        XCTAssertTrue(commands.isEmpty)
        XCTAssertEqual(submissions, 0)

        // Even the no-handler Return fallback must defer to composition.
        fixture.session.onCommand = nil
        XCTAssertFalse(fixture.command("insertNewline:", editor: editor))
        XCTAssertEqual(submissions, 0)

        editor.unmarkText()
        XCTAssertFalse(editor.hasMarkedText())
        fixture.session.isComposing = true
        fixture.session.onCommand = { commands.append($0) }
        XCTAssertTrue(fixture.command("moveDown:", editor: editor))
        XCTAssertEqual(commands, [.down], "An ended composition must not leave a stale keyboard lock")
    }

    func testSearchDelegateConsumesOnlyOwnedCommandsWithoutMarkedText() throws {
        let fixture = SearchFixture()
        defer { fixture.dispose() }
        var commands: [HistoryKeyboardCommand] = []
        fixture.session.onCommand = { commands.append($0) }
        let editor = try fixture.focusSearch()

        for (selector, expected) in [
            ("moveDown:", HistoryKeyboardCommand.down),
            ("moveUp:", .up),
            ("insertNewline:", .restore(keepOpen: false)),
            ("cancelOperation:", .escape)
        ] {
            XCTAssertTrue(fixture.command(selector, editor: editor), selector)
            XCTAssertEqual(commands.last, expected, selector)
        }
        let ownedCommands = commands
        for selector in ["insertSpace:", "insertTab:", "moveLeft:", "moveRight:", "deleteBackward:"] {
            XCTAssertFalse(fixture.command(selector, editor: editor), selector)
        }
        XCTAssertEqual(commands, ownedCommands)
    }

    func testSearchDelegateWithoutHandlerFallsBackOnlyForReturn() throws {
        var submissions = 0
        let fixture = SearchFixture(onSubmit: { submissions += 1 })
        defer { fixture.dispose() }
        let editor = try fixture.focusSearch()

        for selector in ["moveDown:", "moveUp:", "cancelOperation:"] {
            XCTAssertFalse(fixture.command(selector, editor: editor), selector)
        }
        XCTAssertEqual(submissions, 0)
        XCTAssertTrue(fixture.command("insertNewline:", editor: editor))
        XCTAssertEqual(submissions, 1)
    }

    func testFocusSearchAndListChangeActualWindowFirstResponder() throws {
        let text = "历史 👩🏽‍💻 query"
        let fixture = SearchFixture(text: text)
        defer { fixture.dispose() }
        fixture.session.selectedID = 407
        fixture.session.focusList()
        XCTAssertEqual(fixture.session.focus, .list)
        XCTAssertTrue(fixture.window.firstResponder === fixture.list)

        let editor = try fixture.focusSearch()
        XCTAssertEqual(fixture.session.focus, .search)
        XCTAssertTrue(fixture.window.firstResponder === editor)
        XCTAssertEqual(editor.selectedRange(), NSRange(location: 0, length: (text as NSString).length))
        XCTAssertEqual(fixture.session.selectedID, 407)

        editor.setSelectedRange(NSRange(location: 1, length: 0))
        fixture.session.focusSearch(selectAll: false)
        XCTAssertTrue(fixture.window.firstResponder === editor)
        // AppKit may select the field-editor contents while making the field
        // first responder; the contract here is the responder transition, not
        // preservation of the caret when selectAll is disabled.
        XCTAssertEqual(editor.string, text)

        fixture.session.focusList()
        XCTAssertEqual(fixture.session.focus, .list)
        XCTAssertTrue(fixture.window.firstResponder === fixture.list)
        XCTAssertEqual(fixture.session.selectedID, 407)
    }

    func testReconcileInitiallySelectsFirstStableIDWithoutChangingFocus() {
        let session = HistoryKeyboardSession()
        XCTAssertEqual(session.focus, .search)
        XCTAssertNil(session.selectedID)
        session.reconcile(ids: [901, 407, 83])
        XCTAssertEqual(session.selectedID, 901)
        XCTAssertEqual(session.focus, .search)
    }

    func testReconcilePreservesStableIDAcrossRefreshReorderAndFiltering() {
        let session = HistoryKeyboardSession()
        session.selectedID = 407
        session.focus = .list
        for ids: [Int64] in [[901, 407, 83], [1001, 901, 407, 83], [83, 407, 901], [407, 83]] {
            session.reconcile(ids: ids)
            XCTAssertEqual(session.selectedID, 407, "Selection must follow the ID, not its old row index")
            XCTAssertEqual(session.focus, .list)
        }
        // A failed refresh that keeps the host's last page must keep selection.
        session.reconcile(ids: [407, 83])
        XCTAssertEqual(session.selectedID, 407)
    }

    func testReconcileFallsBackWhenSelectedIDDisappearsAndClearsEmptyResults() {
        let session = HistoryKeyboardSession()
        session.selectedID = 407
        session.reconcile(ids: [901, 83])
        XCTAssertEqual(session.selectedID, 901)
        session.reconcile(ids: [])
        XCTAssertNil(session.selectedID)
        session.reconcile(ids: [Int64.max, Int64.min])
        XCTAssertEqual(session.selectedID, Int64.max)
        session.selectedID = Int64.min
        session.reconcile(ids: [Int64.min, Int64.max])
        XCTAssertEqual(session.selectedID, Int64.min)
    }

    func testSessionDoesNotRetainNativeResponders() {
        _ = NSApplication.shared
        let session = HistoryKeyboardSession()
        autoreleasepool {
            let field = NSTextField()
            let list = HistoryKeyboardListNSView()
            session.searchField = field
            session.listResponder = list
            XCTAssertNotNil(session.searchField)
            XCTAssertNotNil(session.listResponder)
        }
        XCTAssertNil(session.searchField)
        XCTAssertNil(session.listResponder)
        session.focusSearch()
        XCTAssertEqual(session.focus, .search)
        session.focusList()
        XCTAssertEqual(session.focus, .list)
    }

    func testCommandEventMappingForNavigationAndRestoreKeys() throws {
        for (code, command): (UInt16, HistoryKeyboardCommand) in [
            (125, .down), (126, .up), (53, .escape), (49, .preview),
            (48, .deepPreview), (36, .restore(keepOpen: false)), (76, .restore(keepOpen: false))
        ] {
            XCTAssertEqual(HistoryKeyboardCommand.event(try keyEvent(code)), command, "keyCode \(code)")
        }
        XCTAssertEqual(HistoryKeyboardCommand.event(try keyEvent(3, modifiers: .command)), .search)
        for code: UInt16 in [36, 76] {
            XCTAssertEqual(
                HistoryKeyboardCommand.event(try keyEvent(code, modifiers: .command)),
                .restore(keepOpen: true)
            )
        }
    }

    func testCommandEventMappingKeepsArrowRepeatButSuppressesRepeatedSpace() throws {
        for (code, command): (UInt16, HistoryKeyboardCommand) in [(125, .down), (126, .up)] {
            XCTAssertEqual(HistoryKeyboardCommand.event(try keyEvent(code, repeatKey: true)), command)
        }
        XCTAssertNil(HistoryKeyboardCommand.event(try keyEvent(49, repeatKey: true)))
        XCTAssertEqual(
            HistoryKeyboardCommand.event(try keyEvent(125, modifiers: [.numericPad, .function, .capsLock])),
            .down,
            "AppKit's non-shortcut flags must not disable arrow navigation"
        )
    }

    func testCommandDeleteRequiresOnlyCommandAndSuppressesHeldKey() throws {
        XCTAssertEqual(HistoryKeyboardCommand.event(try keyEvent(51, modifiers: .command)), .delete)
        XCTAssertNil(HistoryKeyboardCommand.event(try keyEvent(51, modifiers: .command, repeatKey: true)))
        for modifiers: NSEvent.ModifierFlags in [[], .control, .option, .shift, [.command, .shift], [.command, .option], [.command, .control]] {
            XCTAssertNil(HistoryKeyboardCommand.event(try keyEvent(51, modifiers: modifiers)))
        }
        XCTAssertNil(HistoryKeyboardCommand.event(try keyEvent(117)), "Forward Delete is not the requested Mac shortcut")
    }

    func testCommandEventMappingRejectsTypingAndUnownedModifierCombinations() throws {
        for code: UInt16 in [0, 51] {
            XCTAssertNil(HistoryKeyboardCommand.event(try keyEvent(code)), "keyCode \(code)")
        }
        for modifiers: NSEvent.ModifierFlags in [.shift, .control, .option, [.command, .shift], [.command, .option]] {
            for code: UInt16 in [125, 126, 53, 49, 48, 36, 76, 3, 18, 123, 124] {
                XCTAssertNil(
                    HistoryKeyboardCommand.event(try keyEvent(code, modifiers: modifiers)),
                    "keyCode \(code), modifiers \(modifiers)"
                )
            }
        }
    }

    func testListShortcutsParseDigitsNumpadSearchAndPagesWithoutRepeats() throws {
        for (code, command): (UInt16, HistoryKeyboardCommand) in [
            (18, .number(1)), (22, .number(6)), (83, .number(1)),
            (3, .quickSearch), (123, .page(-1)), (124, .page(1))
        ] {
            XCTAssertEqual(HistoryKeyboardCommand.event(try keyEvent(code)), command)
            XCTAssertNil(HistoryKeyboardCommand.event(try keyEvent(code, repeatKey: true)))
        }
        XCTAssertEqual(HistoryKeyboardCommand.event(try keyEvent(18, modifiers: .capsLock)), .number(1))
    }

    func testListShortcutsDoNotInterceptSearchFieldEditing() throws {
        let fixture = SearchFixture(text: "")
        defer { fixture.dispose() }
        fixture.session.quickShortcutsEnabled = true
        var commands: [HistoryKeyboardCommand] = []
        fixture.session.onCommand = { commands.append($0) }
        let editor = try fixture.focusSearch()
        for code: UInt16 in [3, 18, 22] {
            let event = try keyEvent(code, in: fixture.window)
            XCTAssertFalse(fixture.field.performKeyEquivalent(with: event))
            editor.interpretKeyEvents([event])
        }
        XCTAssertEqual(editor.string, "f16")
        XCTAssertTrue(commands.isEmpty)
        XCTAssertFalse(fixture.session.listHasFocus)
        fixture.session.focusList()
        XCTAssertTrue(fixture.session.listHasFocus)
        fixture.window.sendEvent(try keyEvent(18, in: fixture.window))
        XCTAssertEqual(commands, [.number(1)])
    }

    private func keyEvent(
        _ code: UInt16,
        in window: NSWindow? = nil,
        modifiers: NSEvent.ModifierFlags = [],
        repeatKey: Bool = false
    ) throws -> NSEvent {
        let characters: String
        switch code {
        case 125: characters = "\u{F701}"
        case 126: characters = "\u{F700}"
        case 53: characters = "\u{1B}"
        case 49: characters = " "
        case 48: characters = "\t"
        case 36, 76: characters = "\r"
        case 3: characters = "f"
        case 18, 83: characters = "1"
        case 22: characters = "6"
        default: characters = "a"
        }
        return try XCTUnwrap(NSEvent.keyEvent(
            with: .keyDown,
            location: .zero,
            modifierFlags: modifiers,
            timestamp: 0,
            windowNumber: window?.windowNumber ?? 0,
            context: nil,
            characters: characters,
            charactersIgnoringModifiers: characters,
            isARepeat: repeatKey,
            keyCode: code
        ))
    }
}

@MainActor
private final class SearchFixture {
    let session = HistoryKeyboardSession()
    let window: NSWindow
    let field: HistorySearchNSTextField
    let list: HistoryKeyboardListNSView
    let coordinator: HistoryNativeSearchField.Coordinator

    init(text: String = "query", selection: TailSyncThemeSelection = TailSyncThemeSelection(builtin: .tailsync),
         scheme: ColorScheme = .light, onSubmit: @escaping () -> Void = {}) {
        _ = NSApplication.shared
        window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 360, height: 200),
            styleMask: [.titled], backing: .buffered, defer: false
        )
        window.isReleasedWhenClosed = false
        field = HistorySearchNSTextField(frame: NSRect(x: 20, y: 150, width: 300, height: 24))
        field.isEditable = true
        field.isSelectable = true
        field.stringValue = text
        field.session = session
        list = HistoryKeyboardListNSView(frame: NSRect(x: 20, y: 20, width: 300, height: 100))
        list.session = session
        coordinator = HistoryNativeSearchField(
            text: .constant(text), session: session, placeholder: "Search",
            font: .systemFont(ofSize: 13), color: .labelColor,
            editorAppearance: HistorySearchEditorAppearance(selection: selection, scheme: scheme),
            onSubmit: onSubmit, onQueryChanged: {}
        ).makeCoordinator()
        field.delegate = coordinator
        field.editorAppearance = coordinator.parent.editorAppearance
        window.contentView?.addSubview(field)
        window.contentView?.addSubview(list)
        session.searchField = field
        session.listResponder = list
    }

    func focusSearch() throws -> NSTextView {
        session.focusSearch(selectAll: true)
        let editor = try XCTUnwrap(field.currentEditor() as? NSTextView, "Expected NSTextField's real field editor")
        XCTAssertTrue(editor.isFieldEditor)
        XCTAssertTrue(window.firstResponder === editor)
        return editor
    }

    func command(_ selector: String, editor: NSTextView) -> Bool {
        coordinator.control(field, textView: editor, doCommandBy: NSSelectorFromString(selector))
    }

    func dispose() {
        session.onCommand = nil
        (field.currentEditor() as? NSTextView)?.unmarkText()
        window.makeFirstResponder(nil)
        field.delegate = nil
        session.searchField = nil
        session.listResponder = nil
        field.session = nil
        list.session = nil
        window.close()
    }
}
