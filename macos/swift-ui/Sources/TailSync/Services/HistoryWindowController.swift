import AppKit
import Combine

enum HistoryWindowPresentationPolicy {
    static func configure(_ window: NSWindow, isPinned: Bool) {
        window.collectionBehavior = isPinned
            ? [.canJoinAllSpaces, .fullScreenAuxiliary]
            : [.moveToActiveSpace, .fullScreenAuxiliary]
        window.level = isPinned ? .floating : .normal
    }
}

@MainActor
final class HistoryWindowController: ObservableObject {
    static let shared = HistoryWindowController()
    static let isPinnedKey = "HistoryWindow.isPinned"

    @Published private(set) var isPinned: Bool

    private let defaults: UserDefaults
    private weak var window: NSWindow?

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        self.isPinned = defaults.bool(forKey: Self.isPinnedKey)
    }

    func attach(_ window: NSWindow?) {
        self.window = window
        guard let window else { return }
        HistoryWindowPresentationPolicy.configure(window, isPinned: isPinned)
    }

    func detach(_ window: NSWindow? = nil) {
        guard window == nil || self.window === window else { return }
        self.window = nil
    }

    func togglePinned() {
        isPinned.toggle()
        defaults.set(isPinned, forKey: Self.isPinnedKey)
        applyPresentationPolicy()
    }

    func present() {
        guard let window else { return }
        applyPresentationPolicy()
        if window.isMiniaturized { window.deminiaturize(nil) }
        NSApp.activate(ignoringOtherApps: true)
        window.orderFrontRegardless()
        window.makeKey()
    }

    /// Toggle the history window for the global shortcut. Only a focused,
    /// visible window is hidden; invoking the shortcut while another app or a
    /// different TailSync window is focused restores history instead.
    func toggle() {
        guard let window else { return }
        if window.isVisible && window.isKeyWindow {
            window.orderOut(nil)
        } else {
            present()
        }
    }

    private func applyPresentationPolicy() {
        guard let window else { return }
        HistoryWindowPresentationPolicy.configure(window, isPinned: isPinned)
    }
}
