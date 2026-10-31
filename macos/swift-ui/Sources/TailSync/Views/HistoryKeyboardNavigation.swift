import AppKit
import SwiftUI

/// Commands originate in the actual first responder, never in a global event monitor.
enum HistoryKeyboardCommand: Equatable {
    case down, up, search, escape, preview, deepPreview, delete
    case restore(keepOpen: Bool)
    case number(Int), page(Int), quickSearch

    var isListShortcut: Bool {
        switch self {
        case .number, .page, .quickSearch: return true
        default: return false
        }
    }

    static func event(_ event: NSEvent) -> Self? {
        let modifiers = event.modifierFlags.intersection([.command, .control, .option, .shift])
        if modifiers == [.command], event.keyCode == 3 { return .search }
        if modifiers == [.command], event.keyCode == 51, !event.isARepeat { return .delete }
        if modifiers.isEmpty {
            if !event.isARepeat {
                if event.keyCode == 123 { return .page(-1) }
                if event.keyCode == 124 { return .page(1) }
                let text = event.charactersIgnoringModifiers ?? ""
                if text.lowercased() == "f" { return .quickSearch }
                if text.count == 1, "123456".contains(text), let number = Int(text) {
                    return .number(number)
                }
            }
            switch event.keyCode {
            case 125: return .down
            case 126: return .up
            case 53: return .escape
            case 49: return event.isARepeat ? nil : .preview
            case 48: return .deepPreview
            case 36, 76: return event.isARepeat ? nil : .restore(keepOpen: false)
            default: return nil
            }
        }
        if modifiers == [.command], !event.isARepeat, [36, 76].contains(event.keyCode) {
            return .restore(keepOpen: true)
        }
        return nil
    }
}

@MainActor
final class HistoryKeyboardSession: ObservableObject {
    enum Focus { case search, list }
    @Published var focus: Focus = .search
    @Published var selectedID: Int64?
    @Published var listHasFocus = false
    var quickShortcutsEnabled = false
    weak var searchField: NSTextField?
    weak var listResponder: NSView?
    var onCommand: ((HistoryKeyboardCommand) -> Void)?
    var isComposing = false

    func focusSearch(selectAll: Bool = true) {
        focus = .search
        guard let field = searchField, let window = field.window else { return }
        window.makeFirstResponder(field)
        if selectAll { field.currentEditor()?.selectAll(nil) }
    }

    func focusList() {
        focus = .list
        if let responder = listResponder { responder.window?.makeFirstResponder(responder) }
    }

    /// Stable IDs survive polling, filtering and failed page loads.
    func reconcile(ids: [Int64]) {
        if let selectedID, ids.contains(selectedID) { return }
        selectedID = ids.first
    }
}

struct HistoryKeyboardListResponder: NSViewRepresentable {
    @ObservedObject var session: HistoryKeyboardSession
    func makeNSView(context: Context) -> HistoryKeyboardListNSView {
        let view = HistoryKeyboardListNSView()
        view.session = session
        session.listResponder = view
        return view
    }
    func updateNSView(_ view: HistoryKeyboardListNSView, context: Context) {
        view.session = session
        session.listResponder = view
    }
    static func dismantleNSView(_ view: HistoryKeyboardListNSView, coordinator: ()) {
        if view.session?.listResponder === view { view.session?.listResponder = nil }
        view.session = nil
    }
}

final class HistoryKeyboardListNSView: NSView {
    weak var session: HistoryKeyboardSession?
    override var acceptsFirstResponder: Bool { true }
    override func becomeFirstResponder() -> Bool {
        let accepted = super.becomeFirstResponder()
        if accepted { session?.listHasFocus = true }
        return accepted
    }
    override func resignFirstResponder() -> Bool {
        let accepted = super.resignFirstResponder()
        if accepted { session?.listHasFocus = false }
        return accepted
    }
    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        guard window != nil else { return }
        Task { @MainActor [weak self] in
            guard let self, self.session?.focus == .list else { return }
            self.session?.focusList()
        }
    }
    override func hitTest(_ point: NSPoint) -> NSView? { nil }
    override func keyDown(with event: NSEvent) {
        if let command = HistoryKeyboardCommand.event(event),
           !command.isListShortcut || session?.quickShortcutsEnabled == true {
            session?.onCommand?(command)
        } else { super.keyDown(with: event) }
    }
    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        guard window?.firstResponder === self,
              let command = HistoryKeyboardCommand.event(event),
              command == .search || command == .restore(keepOpen: true) || command == .delete else { return false }
        if command != .delete { session?.isComposing = false }
        session?.onCommand?(command)
        return true
    }
}

/// Native text editing does not inherit SwiftUI's tint. Resolve the editor's
/// caret and selected-text colors explicitly from the same theme as its chrome.
struct HistorySearchEditorAppearance {
    let insertionPointColor: NSColor
    let selectionBackgroundColor: NSColor
    let selectionTextColor: NSColor

    init(selection: TailSyncThemeSelection, scheme: ColorScheme) {
        let palette = selection.palette(for: scheme)
        let search = selection.component("search", state: "focus", scheme: scheme)
        let input = selection.component("input", state: "focus", scheme: scheme)
        insertionPointColor = NSColor(search?.focusRingColor ?? input?.focusRingColor ?? palette.accentColor)
        selectionBackgroundColor = NSColor(palette.accentColor)
        selectionTextColor = NSColor(palette.accentContrastColor)
    }
}

/// NSTextField's field editor delegates interpreted keys here. IME marked text
/// is a hard gate, including Return/Escape; literal spaces always remain text.
struct HistoryNativeSearchField: NSViewRepresentable {
    @Binding var text: String
    let session: HistoryKeyboardSession
    let placeholder: String
    let font: NSFont
    let color: NSColor
    let editorAppearance: HistorySearchEditorAppearance
    let onSubmit: () -> Void
    let onQueryChanged: () -> Void

    func makeCoordinator() -> Coordinator { Coordinator(self) }
    func makeNSView(context: Context) -> HistorySearchNSTextField {
        let field = HistorySearchNSTextField()
        field.session = session
        field.isBordered = false
        field.drawsBackground = false
        field.focusRingType = .none
        field.isEditable = true
        field.isSelectable = true
        field.lineBreakMode = .byTruncatingTail
        field.cell?.usesSingleLineMode = false
        field.cell?.wraps = false
        field.cell?.isScrollable = true
        field.cell?.truncatesLastVisibleLine = true
        field.delegate = context.coordinator
        field.editorAppearance = editorAppearance
        field.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        session.searchField = field
        return field
    }
    func updateNSView(_ field: HistorySearchNSTextField, context: Context) {
        context.coordinator.parent = self
        session.searchField = field
        field.placeholderString = placeholder
        field.font = font
        field.textColor = color
        field.editorAppearance = editorAppearance
        // Never replace an active input-method composition with a SwiftUI update.
        if (field.currentEditor() as? NSTextView)?.hasMarkedText() != true,
           field.stringValue != text { field.stringValue = text }
    }
    static func dismantleNSView(_ field: HistorySearchNSTextField, coordinator: Coordinator) {
        field.restoreEditorAppearance()
        field.delegate = nil
        if coordinator.parent.session.searchField === field {
            coordinator.parent.session.searchField = nil
        }
    }
    final class Coordinator: NSObject, NSTextFieldDelegate {
        var parent: HistoryNativeSearchField
        init(_ parent: HistoryNativeSearchField) { self.parent = parent }
        func controlTextDidBeginEditing(_ notification: Notification) {
            parent.session.focus = .search
            (notification.object as? HistorySearchNSTextField)?.applyEditorAppearance()
        }
        func controlTextDidChange(_ notification: Notification) {
            guard let field = notification.object as? NSTextField else { return }
            parent.session.isComposing = (field.currentEditor() as? NSTextView)?.hasMarkedText() == true
            parent.text = field.stringValue
            if !parent.session.isComposing { parent.onQueryChanged() }
        }
        func controlTextDidEndEditing(_ notification: Notification) {
            parent.session.isComposing = false
            (notification.object as? HistorySearchNSTextField)?.restoreEditorAppearance()
        }
        func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
            guard !textView.hasMarkedText() else { return false }
            parent.session.isComposing = false
            let command: HistoryKeyboardCommand
            switch NSStringFromSelector(selector) {
            case "moveDown:": command = .down
            case "moveUp:": command = .up
            case "insertNewline:":
                if NSApp.currentEvent?.isARepeat == true { return true }
                command = .restore(keepOpen: NSApp.currentEvent?.modifierFlags.contains(.command) == true)
            case "cancelOperation:": command = .escape
            default: return false
            }
            guard let handler = parent.session.onCommand else {
                if case .restore = command { parent.onSubmit(); return true }
                return false
            }
            handler(command)
            return true
        }
    }
}

final class HistorySearchNSTextField: NSTextField {
    weak var session: HistoryKeyboardSession?
    var editorAppearance: HistorySearchEditorAppearance? {
        didSet { applyEditorAppearance() }
    }
    private weak var themedEditor: NSTextView?
    private var originalInsertionPointColor: NSColor?
    private var originalSelectedTextAttributes: [NSAttributedString.Key: Any]?

    override func becomeFirstResponder() -> Bool {
        let becameFirstResponder = super.becomeFirstResponder()
        if becameFirstResponder { applyEditorAppearance() }
        return becameFirstResponder
    }

    override func selectText(_ sender: Any?) {
        super.selectText(sender)
        applyEditorAppearance()
    }

    func applyEditorAppearance() {
        guard let appearance = editorAppearance,
              let editor = currentEditor() as? NSTextView else { return }
        if themedEditor !== editor {
            restoreEditorAppearance()
            themedEditor = editor
            originalInsertionPointColor = editor.insertionPointColor
            originalSelectedTextAttributes = editor.selectedTextAttributes
        }
        editor.insertionPointColor = appearance.insertionPointColor
        var attributes = editor.selectedTextAttributes
        attributes[.backgroundColor] = appearance.selectionBackgroundColor
        attributes[.foregroundColor] = appearance.selectionTextColor
        editor.selectedTextAttributes = attributes
    }

    // NSWindow may reuse its field editor for another control. Do not let the
    // search theme leak into settings/other text fields after editing ends.
    func restoreEditorAppearance() {
        if let editor = themedEditor {
            if let originalInsertionPointColor { editor.insertionPointColor = originalInsertionPointColor }
            if let originalSelectedTextAttributes { editor.selectedTextAttributes = originalSelectedTextAttributes }
        }
        themedEditor = nil
        originalInsertionPointColor = nil
        originalSelectedTextAttributes = nil
    }
    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        guard window != nil else { return }
        Task { @MainActor [weak self] in
            guard let self, self.session?.focus == .search else { return }
            self.session?.focusSearch(selectAll: false)
        }
    }
    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        guard currentEditor() != nil,
              (currentEditor() as? NSTextView)?.hasMarkedText() != true,
              let command = HistoryKeyboardCommand.event(event),
              command == .search || command == .restore(keepOpen: true) else {
            return super.performKeyEquivalent(with: event)
        }
        session?.isComposing = false
        session?.onCommand?(command)
        return true
    }
}
