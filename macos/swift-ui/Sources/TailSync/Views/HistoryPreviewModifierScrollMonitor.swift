import AppKit
import SwiftUI

/// Converts Command/Control + wheel into a local zoom action without
/// interfering with ordinary document scrolling elsewhere in the window.
struct HistoryPreviewModifierScrollMonitor: NSViewRepresentable {
    let onScroll: (CGFloat) -> Void

    func makeCoordinator() -> Coordinator {
        Coordinator(onScroll: onScroll)
    }

    func makeNSView(context: Context) -> NSView {
        let view = NSView(frame: .zero)
        context.coordinator.attach(to: view)
        return view
    }

    func updateNSView(_ view: NSView, context: Context) {
        context.coordinator.onScroll = onScroll
        context.coordinator.attach(to: view)
    }

    static func dismantleNSView(_ view: NSView, coordinator: Coordinator) {
        coordinator.stop()
    }

    @MainActor
    final class Coordinator {
        var onScroll: (CGFloat) -> Void

        private weak var view: NSView?
        private var monitor: Any?

        init(onScroll: @escaping (CGFloat) -> Void) {
            self.onScroll = onScroll
        }

        func attach(to view: NSView) {
            self.view = view
            guard monitor == nil else { return }
            monitor = NSEvent.addLocalMonitorForEvents(matching: .scrollWheel) { [weak self] event in
                guard let self,
                      let view = self.view,
                      let window = view.window,
                      event.window === window else { return event }

                let modifiers = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
                guard !modifiers.isDisjoint(with: [.command, .control]) else { return event }
                let localPoint = view.convert(event.locationInWindow, from: nil)
                guard view.bounds.contains(localPoint) else { return event }

                let delta = event.scrollingDeltaY
                guard delta != 0 else { return event }
                self.onScroll(delta)
                return nil
            }
        }

        func stop() {
            if let monitor { NSEvent.removeMonitor(monitor) }
            monitor = nil
            view = nil
        }

        deinit {
            if let monitor { NSEvent.removeMonitor(monitor) }
        }
    }
}
