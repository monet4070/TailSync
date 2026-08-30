import AppKit
import PDFKit
import QuickLookUI
import SwiftUI

enum HistoryPreviewOwner: Equatable {
    case history
    case favorites

    init(collection: String) {
        self = collection == "favorites" ? .favorites : .history
    }
}

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
    private weak var favoritesWindow: NSWindow?
    private weak var activeHostWindow: NSWindow?
    private var activeOwner: HistoryPreviewOwner?
    private var window: HistoryPreviewWindow?
    private var hostObservers: [NSObjectProtocol] = []
    private var activeWindowKind: HistoryPreviewWindowKind = .text
    private var isApplyingFrame = false
    private var restoreAfterHostMiniaturizes = false

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

    func registerHostWindow(_ window: NSWindow?, for owner: HistoryPreviewOwner) {
        switch owner {
        case .history:
            historyWindow = window
        case .favorites:
            favoritesWindow = window
        }
        guard activeOwner == owner else { return }

        removeHostObservers()
        activeHostWindow = window
        restoreAfterHostMiniaturizes = false
        guard let window else {
            close()
            activeOwner = nil
            return
        }
        observeHostWindow(window, owner: owner)
    }

    func isPreviewVisible(for owner: HistoryPreviewOwner) -> Bool {
        activeOwner == owner && isPreviewVisible
    }

    func present(_ request: HistoryPreviewRequest, owner: HistoryPreviewOwner = .history) {
        activateHost(for: owner)
        guard let selected = request.selectedItem else { return }
        let window = previewWindow()
        applyWindowKind(selected.estimatedFormat.windowKind)
        viewModel.present(request)
        window.title = Loc.t("history.preview.title")
        if window.isMiniaturized { window.deminiaturize(nil) }
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    func close(ifOwnedBy owner: HistoryPreviewOwner) {
        guard activeOwner == owner else { return }
        close()
    }

    private func activateHost(for owner: HistoryPreviewOwner) {
        let hostWindow = registeredWindow(for: owner)
        guard activeOwner != owner || activeHostWindow !== hostWindow else { return }
        removeHostObservers()
        activeOwner = owner
        activeHostWindow = hostWindow
        restoreAfterHostMiniaturizes = false
        if let hostWindow {
            observeHostWindow(hostWindow, owner: owner)
        }
    }

    private func registeredWindow(for owner: HistoryPreviewOwner) -> NSWindow? {
        switch owner {
        case .history: historyWindow
        case .favorites: favoritesWindow
        }
    }

    private func observeHostWindow(_ hostWindow: NSWindow, owner: HistoryPreviewOwner) {
        let center = NotificationCenter.default
        hostObservers = [
            center.addObserver(
                forName: NSWindow.willCloseNotification,
                object: hostWindow,
                queue: .main
            ) { [weak self] _ in
                Task { @MainActor [weak self] in self?.hostWillClose(owner: owner) }
            },
            center.addObserver(
                forName: NSWindow.didMiniaturizeNotification,
                object: hostWindow,
                queue: .main
            ) { [weak self] _ in
                Task { @MainActor [weak self] in self?.hostDidMiniaturize() }
            },
            center.addObserver(
                forName: NSWindow.didDeminiaturizeNotification,
                object: hostWindow,
                queue: .main
            ) { [weak self] _ in
                Task { @MainActor [weak self] in self?.hostDidDeminiaturize() }
            }
        ]
    }

    private func hostWillClose(owner: HistoryPreviewOwner) {
        guard activeOwner == owner else { return }
        close()
        removeHostObservers()
        activeHostWindow = nil
        activeOwner = nil
    }

    func close() {
        viewModel.close()
        restoreAfterHostMiniaturizes = false
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
        removeHostObservers()
        historyWindow = nil
        favoritesWindow = nil
        activeHostWindow = nil
        activeOwner = nil
    }

    func windowShouldClose(_ sender: NSWindow) -> Bool {
        if sender.isVisible { saveFrameIfNeeded() }
        viewModel.close()
        restoreAfterHostMiniaturizes = false
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
        let screenFrame = (activeHostWindow?.screen ?? window.screen ?? NSScreen.main)?.visibleFrame
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
        } else if let historyFrame = activeHostWindow?.frame {
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

    private func hostDidMiniaturize() {
        guard let window, window.isVisible else {
            restoreAfterHostMiniaturizes = false
            return
        }
        restoreAfterHostMiniaturizes = true
        if !window.isMiniaturized { window.miniaturize(nil) }
    }

    private func hostDidDeminiaturize() {
        guard restoreAfterHostMiniaturizes else { return }
        restoreAfterHostMiniaturizes = false
        if window?.isMiniaturized == true { window?.deminiaturize(nil) }
    }

    private func removeHostObservers() {
        let center = NotificationCenter.default
        hostObservers.forEach(center.removeObserver)
        hostObservers.removeAll()
    }

    deinit {
        // deinit is nonisolated; remove the observers inline instead of
        // calling the actor-isolated helper.
        let center = NotificationCenter.default
        hostObservers.forEach(center.removeObserver)
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
