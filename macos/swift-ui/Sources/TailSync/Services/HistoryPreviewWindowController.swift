import AppKit
import PDFKit
import QuickLookUI
import SwiftUI

/// Sole owner of the independent preview window and its sensitive content.
/// A process-wide instance is intentional: the product permits one reusable
/// preview window, and this object defines its complete creation/cleanup path.
@MainActor
final class HistoryPreviewWindowController: NSObject, NSWindowDelegate {
    static let shared = HistoryPreviewWindowController()

    let viewModel: HistoryPreviewViewModel

    var isPreviewVisible: Bool { window?.isVisible == true }
    var hasAllocatedWindow: Bool { window != nil }

    private weak var historyWindow: NSWindow?
    private var window: HistoryPreviewWindow?
    private var historyObservers: [NSObjectProtocol] = []
    private var activeWindowKind: HistoryPreviewWindowKind = .text
    private var isApplyingFrame = false
    private var restoreAfterHistoryMiniaturizes = false

    // The default is resolved inside the (isolated) initializer body:
    // `HistoryPreviewViewModel()` is main-actor-isolated and cannot be
    // evaluated as a default argument from a synchronous nonisolated call.
    init(viewModel: HistoryPreviewViewModel? = nil) {
        let viewModel = viewModel ?? HistoryPreviewViewModel()
        self.viewModel = viewModel
        super.init()
        viewModel.onFormatChange = { [weak self] kind in
            self?.applyWindowKind(kind)
        }
    }

    func attachHistoryWindow(_ historyWindow: NSWindow?) {
        guard self.historyWindow !== historyWindow else { return }
        removeHistoryObservers()
        self.historyWindow = historyWindow
        guard let historyWindow else {
            close()
            return
        }
        let center = NotificationCenter.default
        historyObservers = [
            center.addObserver(
                forName: NSWindow.willCloseNotification,
                object: historyWindow,
                queue: .main
            ) { [weak self] _ in
                Task { @MainActor in self?.close() }
            },
            center.addObserver(
                forName: NSWindow.didMiniaturizeNotification,
                object: historyWindow,
                queue: .main
            ) { [weak self] _ in
                Task { @MainActor in self?.historyDidMiniaturize() }
            },
            center.addObserver(
                forName: NSWindow.didDeminiaturizeNotification,
                object: historyWindow,
                queue: .main
            ) { [weak self] _ in
                Task { @MainActor in self?.historyDidDeminiaturize() }
            }
        ]
    }

    func present(_ request: HistoryPreviewRequest) {
        guard let selected = request.selectedItem else { return }
        let window = previewWindow()
        applyWindowKind(selected.estimatedFormat.windowKind)
        viewModel.present(request)
        window.title = Loc.t("history.preview.title")
        if window.isMiniaturized { window.deminiaturize(nil) }
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    func close() {
        viewModel.close()
        restoreAfterHistoryMiniaturizes = false
        guard let window else { return }
        if window.isVisible { saveFrameIfNeeded() }
        window.delegate = nil
        window.onClosePreview = nil
        window.onNavigate = nil
        window.contentViewController = nil
        self.window = nil
        window.close()
    }

    func closeIfShowing(entryId: Int64) {
        if viewModel.contains(entryId: entryId) { close() }
    }

    func shutdown() {
        close()
        removeHistoryObservers()
    }

    func windowShouldClose(_ sender: NSWindow) -> Bool {
        if sender.isVisible { saveFrameIfNeeded() }
        viewModel.close()
        restoreAfterHistoryMiniaturizes = false
        return true
    }

    func windowWillClose(_ notification: Notification) {
        guard let closingWindow = notification.object as? HistoryPreviewWindow,
              closingWindow === window else { return }
        closingWindow.onClosePreview = nil
        closingWindow.onNavigate = nil
        closingWindow.contentViewController = nil
        window = nil
    }

    func windowDidMove(_ notification: Notification) {
        saveFrameIfNeeded()
    }

    func windowDidResize(_ notification: Notification) {
        saveFrameIfNeeded()
    }

    private func previewWindow() -> HistoryPreviewWindow {
        if let window { return window }
        let rootView = HistoryPreviewView(model: viewModel)
        let hosting = NSHostingController(rootView: rootView)
        let window = HistoryPreviewWindow(contentViewController: hosting)
        window.title = Loc.t("history.preview.title")
        // Keep programmatic miniaturization for history-window lifecycle
        // coupling while removing the unused visible yellow/green controls.
        window.styleMask = [.titled, .closable, .miniaturizable, .resizable]
        window.standardWindowButton(.miniaturizeButton)?.isHidden = true
        window.standardWindowButton(.zoomButton)?.isHidden = true
        window.titlebarAppearsTransparent = false
        window.isReleasedWhenClosed = false
        window.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        window.level = .normal
        window.delegate = self
        window.onClosePreview = { [weak self] in self?.close() }
        window.onNavigate = { [weak self] forward in
            if forward {
                self?.viewModel.navigateForward()
            } else {
                self?.viewModel.navigateBackward()
            }
        }
        self.window = window
        applyWindowKind(activeWindowKind, force: true)
        return window
    }

    private func applyWindowKind(_ kind: HistoryPreviewWindowKind, force: Bool = false) {
        guard force || kind != activeWindowKind || window == nil else { return }
        if window?.isVisible == true { saveFrameIfNeeded() }
        activeWindowKind = kind
        guard let window else { return }

        isApplyingFrame = true
        defer { isApplyingFrame = false }
        window.contentMinSize = kind.minimumContentSize
        let frame = restoredFrame(for: kind, window: window)
        window.setFrame(frame, display: true, animate: window.isVisible)
    }

    private func restoredFrame(
        for kind: HistoryPreviewWindowKind,
        window: NSWindow
    ) -> NSRect {
        let screenFrame = (historyWindow?.screen ?? window.screen ?? NSScreen.main)?.visibleFrame
            ?? NSRect(x: 0, y: 0, width: 1_440, height: 900)
        let maximumSize = NSSize(width: screenFrame.width * 0.9, height: screenFrame.height * 0.9)
        let minimumSize = kind.minimumContentSize

        let stored = UserDefaults.standard.string(forKey: frameKey(for: kind)).map(NSRectFromString)
        let desiredSize = stored?.size ?? kind.defaultContentSize
        let size = NSSize(
            width: min(max(desiredSize.width, min(minimumSize.width, maximumSize.width)), maximumSize.width),
            height: min(max(desiredSize.height, min(minimumSize.height, maximumSize.height)), maximumSize.height)
        )
        let desiredOrigin: NSPoint
        if let stored, stored.intersects(screenFrame) {
            desiredOrigin = stored.origin
        } else if let historyFrame = historyWindow?.frame {
            desiredOrigin = NSPoint(
                x: historyFrame.midX - size.width / 2,
                y: historyFrame.midY - size.height / 2
            )
        } else {
            desiredOrigin = NSPoint(
                x: screenFrame.midX - size.width / 2,
                y: screenFrame.midY - size.height / 2
            )
        }
        let origin = NSPoint(
            x: min(max(desiredOrigin.x, screenFrame.minX), screenFrame.maxX - size.width),
            y: min(max(desiredOrigin.y, screenFrame.minY), screenFrame.maxY - size.height)
        )
        return NSRect(origin: origin, size: size)
    }

    private func saveFrameIfNeeded() {
        guard !isApplyingFrame, let window, window.isVisible, !window.isMiniaturized else { return }
        UserDefaults.standard.set(NSStringFromRect(window.frame), forKey: frameKey(for: activeWindowKind))
    }

    private func frameKey(for kind: HistoryPreviewWindowKind) -> String {
        "HistoryPreviewWindow.frame.\(kind.rawValue)"
    }

    private func historyDidMiniaturize() {
        guard let window, window.isVisible else {
            restoreAfterHistoryMiniaturizes = false
            return
        }
        restoreAfterHistoryMiniaturizes = true
        if !window.isMiniaturized { window.miniaturize(nil) }
    }

    private func historyDidDeminiaturize() {
        guard restoreAfterHistoryMiniaturizes else { return }
        restoreAfterHistoryMiniaturizes = false
        if window?.isMiniaturized == true { window?.deminiaturize(nil) }
    }

    private func removeHistoryObservers() {
        let center = NotificationCenter.default
        historyObservers.forEach(center.removeObserver)
        historyObservers.removeAll()
    }

    deinit {
        // deinit is nonisolated; remove the observers inline instead of
        // calling the actor-isolated helper.
        let center = NotificationCenter.default
        historyObservers.forEach(center.removeObserver)
    }
}

enum HistoryPreviewWindowKeyPolicy {
    static func keepsSpaceKey(_ responder: NSResponder?) -> Bool {
        if let textView = responder as? NSTextView {
            // Search fields use an editable field editor; the read-only
            // source viewer intentionally retains Space-to-close.
            return textView.isEditable
        }
        return responder is NSTextField
            || responder is NSSearchField
            || responder is NSButton
            || responder is NSSlider
            || responder is NSStepper
            || responder is NSPopUpButton
            || responder is PDFView
            || responder is QLPreviewView
    }
}

private final class HistoryPreviewWindow: NSWindow {
    var onClosePreview: (() -> Void)?
    var onNavigate: ((Bool) -> Void)?

    override func keyDown(with event: NSEvent) {
        let modifiers = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        let shortcutModifiers = modifiers.intersection([.command, .control, .option, .shift])
        if shortcutModifiers == [.option], event.keyCode == 123 {
            onNavigate?(false)
            return
        }
        if shortcutModifiers == [.option], event.keyCode == 124 {
            onNavigate?(true)
            return
        }
        if event.keyCode == 53 {
            onClosePreview?()
            return
        }
        if event.keyCode == 49,
           modifiers.isDisjoint(with: [.command, .control, .option, .shift]),
           !event.isARepeat,
           !HistoryPreviewWindowKeyPolicy.keepsSpaceKey(firstResponder)
        {
            onClosePreview?()
            return
        }
        super.keyDown(with: event)
    }

}
