import AppKit
import SwiftUI

/// One AppKit responder owns every pointer and keyboard interaction for a
/// history row. This avoids competing SwiftUI tap recognizers and gives Space
/// a stable first-responder target after selection.
struct HistoryRowInteraction: NSViewRepresentable {
    let previewRequest: HistoryPreviewRequest?
    let onSelect: () -> Void
    let onRestore: () -> Void
    let onDelete: () -> Void
    let onPreview: (HistoryPreviewRequest) -> Void
    let onClosePreview: () -> Void
    let isPreviewVisible: () -> Bool

    func makeNSView(context: Context) -> HistoryRowInteractionNSView {
        HistoryRowInteractionNSView()
    }

    func updateNSView(_ nsView: HistoryRowInteractionNSView, context: Context) {
        nsView.previewRequest = previewRequest
        nsView.onSelect = onSelect
        nsView.onRestore = onRestore
        nsView.onDelete = onDelete
        nsView.onPreview = onPreview
        nsView.onClosePreview = onClosePreview
        nsView.isPreviewVisible = isPreviewVisible
    }
}

final class HistoryRowInteractionNSView: NSView {
    var previewRequest: HistoryPreviewRequest?
    var onSelect: (() -> Void)?
    var onRestore: (() -> Void)?
    var onDelete: (() -> Void)?
    var onPreview: ((HistoryPreviewRequest) -> Void)?
    var onClosePreview: (() -> Void)?
    var isPreviewVisible: (() -> Bool)?

    override var acceptsFirstResponder: Bool { true }

    override func acceptsFirstMouse(for event: NSEvent?) -> Bool {
        true
    }

    override func mouseDown(with event: NSEvent) {
        guard event.buttonNumber == 0 else {
            super.mouseDown(with: event)
            return
        }
        _ = window?.makeFirstResponder(self)
        onSelect?()
    }

    override func mouseUp(with event: NSEvent) {
        guard event.buttonNumber == 0 else {
            super.mouseUp(with: event)
            return
        }
        if event.clickCount == 2 {
            onRestore?()
        }
    }

    override func rightMouseDown(with event: NSEvent) {
        onDelete?()
    }

    override func keyDown(with event: NSEvent) {
        let modifiers = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        let hasShortcutModifiers = !modifiers.isDisjoint(
            with: [.command, .control, .option, .shift]
        )
        guard !hasShortcutModifiers else {
            super.keyDown(with: event)
            return
        }

        if event.keyCode == 53, isPreviewVisible?() == true {
            onClosePreview?()
            return
        }
        if event.keyCode == 49,
           !event.isARepeat,
           let previewRequest
        {
            onPreview?(previewRequest)
            return
        }
        super.keyDown(with: event)
    }
}
