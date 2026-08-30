import AppKit

extension Notification.Name {
    static let tailSyncHistoryWindowVisibilityChanged = Notification.Name(
        "tailSyncHistoryWindowVisibilityChanged"
    )
    static let tailSyncFavoritesWindowVisibilityChanged = Notification.Name(
        "tailSyncFavoritesWindowVisibilityChanged"
    )

    static func tailSyncHistoryCollectionVisibilityChanged(
        for collection: String
    ) -> Notification.Name {
        collection == "favorites"
            ? .tailSyncFavoritesWindowVisibilityChanged
            : .tailSyncHistoryWindowVisibilityChanged
    }
}

/// Owns a window only while it is open. Closing the window tears down the
/// hosted SwiftUI tree so its tasks, images, and view caches do not stay alive.
final class TailSyncTransientWindowController: NSWindowController, NSWindowDelegate {
    private var onClose: (() -> Void)?
    private var onVisibilityChange: ((Bool) -> Void)?
    private var lastReportedVisibility = true

    init(
        window: NSWindow,
        onVisibilityChange: ((Bool) -> Void)? = nil,
        onClose: @escaping () -> Void
    ) {
        self.onVisibilityChange = onVisibilityChange
        self.onClose = onClose
        super.init(window: window)
        window.delegate = self
        window.isReleasedWhenClosed = false
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    func windowDidMiniaturize(_ notification: Notification) {
        reportVisibility(false, for: notification)
    }

    func windowDidDeminiaturize(_ notification: Notification) {
        reportVisibility(true, for: notification)
    }

    func windowDidChangeOcclusionState(_ notification: Notification) {
        guard let eventWindow = notification.object as? NSWindow,
              eventWindow === window else { return }
        reportVisibility(
            eventWindow.occlusionState.contains(.visible),
            for: notification
        )
    }

    func windowWillClose(_ notification: Notification) {
        guard let closingWindow = notification.object as? NSWindow,
              closingWindow === window else { return }
        closingWindow.contentViewController = nil
        closingWindow.delegate = nil
        self.window = nil
        let callback = onClose
        onVisibilityChange = nil
        onClose = nil
        callback?()
    }

    private func reportVisibility(_ visible: Bool, for notification: Notification) {
        guard let eventWindow = notification.object as? NSWindow,
              eventWindow === window,
              lastReportedVisibility != visible else { return }
        lastReportedVisibility = visible
        onVisibilityChange?(visible)
    }
}
