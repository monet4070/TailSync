import AppKit

/// Owns a window only while it is open. Closing the window tears down the
/// hosted SwiftUI tree so its tasks, images, and view caches do not stay alive.
final class TailSyncTransientWindowController: NSWindowController, NSWindowDelegate {
    private var onClose: (() -> Void)?

    init(window: NSWindow, onClose: @escaping () -> Void) {
        self.onClose = onClose
        super.init(window: window)
        window.delegate = self
        window.isReleasedWhenClosed = false
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    func windowWillClose(_ notification: Notification) {
        guard let closingWindow = notification.object as? NSWindow,
              closingWindow === window else { return }
        closingWindow.contentViewController = nil
        closingWindow.delegate = nil
        self.window = nil
        let callback = onClose
        onClose = nil
        callback?()
    }
}
