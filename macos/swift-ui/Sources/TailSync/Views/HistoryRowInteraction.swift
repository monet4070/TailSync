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
    let onFavorite: () -> Void
    let onFavoritePressStarted: () -> Void
    let onFavoritePressCancelled: () -> Void

    init(
        previewRequest: HistoryPreviewRequest?,
        onSelect: @escaping () -> Void,
        onRestore: @escaping () -> Void,
        onDelete: @escaping () -> Void,
        onPreview: @escaping (HistoryPreviewRequest) -> Void,
        onClosePreview: @escaping () -> Void,
        isPreviewVisible: @escaping () -> Bool,
        onFavorite: @escaping () -> Void = {},
        onFavoritePressStarted: @escaping () -> Void = {},
        onFavoritePressCancelled: @escaping () -> Void = {}
    ) {
        self.previewRequest = previewRequest
        self.onSelect = onSelect
        self.onRestore = onRestore
        self.onDelete = onDelete
        self.onPreview = onPreview
        self.onClosePreview = onClosePreview
        self.isPreviewVisible = isPreviewVisible
        self.onFavorite = onFavorite
        self.onFavoritePressStarted = onFavoritePressStarted
        self.onFavoritePressCancelled = onFavoritePressCancelled
    }

    func makeNSView(context: Context) -> HistoryRowInteractionNSView {
        HistoryRowInteractionNSView()
    }

    func updateNSView(_ nsView: HistoryRowInteractionNSView, context: Context) {
        nsView.previewRequest = previewRequest
        nsView.onSelect = onSelect
        nsView.onRestore = onRestore
        nsView.onDelete = onDelete
        nsView.onFavorite = onFavorite
        nsView.onFavoritePressStarted = onFavoritePressStarted
        nsView.onFavoritePressCancelled = onFavoritePressCancelled
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
    var onFavorite: (() -> Void)?
    var onFavoritePressStarted: (() -> Void)?
    var onFavoritePressCancelled: (() -> Void)?
    var onPreview: ((HistoryPreviewRequest) -> Void)?
    var onClosePreview: (() -> Void)?
    var isPreviewVisible: (() -> Bool)?

    private var pressStartLocation: NSPoint?
    private var favoriteGraceWork: DispatchWorkItem?
    private var favoriteCommitWork: DispatchWorkItem?
    private var favoritePressTriggered = false

    private static let favoriteGrace: TimeInterval = 0.22
    private static let favoriteCharge: TimeInterval = 0.42
    private static let favoriteMoveThreshold: CGFloat = 8

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

        cancelFavoritePress(notify: false)
        favoritePressTriggered = false
        pressStartLocation = event.locationInWindow
        let grace = DispatchWorkItem { [weak self] in
            guard let self, self.pressStartLocation != nil else { return }
            self.onFavoritePressStarted?()
            let commit = DispatchWorkItem { [weak self] in
                guard let self, self.pressStartLocation != nil else { return }
                self.favoritePressTriggered = true
                self.onFavorite?()
            }
            self.favoriteCommitWork = commit
            DispatchQueue.main.asyncAfter(
                deadline: .now() + Self.favoriteCharge,
                execute: commit
            )
        }
        favoriteGraceWork = grace
        DispatchQueue.main.asyncAfter(
            deadline: .now() + Self.favoriteGrace,
            execute: grace
        )
    }

    override func mouseUp(with event: NSEvent) {
        guard event.buttonNumber == 0 else {
            super.mouseUp(with: event)
            return
        }
        let wasFavoritePress = favoritePressTriggered
        cancelFavoritePress(notify: !wasFavoritePress)
        if wasFavoritePress {
            // The completion already consumed this gesture. Do not let the
            // same mouse sequence fall through to the double-click restore.
            favoritePressTriggered = false
            return
        }
        onSelect?()
        if event.clickCount == 2 {
            onRestore?()
        }
    }

    override func mouseDragged(with event: NSEvent) {
        guard let start = pressStartLocation else {
            super.mouseDragged(with: event)
            return
        }
        let current = event.locationInWindow
        if hypot(current.x - start.x, current.y - start.y) > Self.favoriteMoveThreshold {
            cancelFavoritePress(notify: true)
        }
    }

    override func rightMouseDown(with event: NSEvent) {
        let completedActivePress = pressStartLocation != nil && favoritePressTriggered
        cancelFavoritePress(notify: true)
        guard !completedActivePress else { return }
        onDelete?()
    }

    private func cancelFavoritePress(notify: Bool) {
        favoriteGraceWork?.cancel()
        favoriteCommitWork?.cancel()
        favoriteGraceWork = nil
        favoriteCommitWork = nil
        if notify, pressStartLocation != nil, !favoritePressTriggered {
            onFavoritePressCancelled?()
        }
        pressStartLocation = nil
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
