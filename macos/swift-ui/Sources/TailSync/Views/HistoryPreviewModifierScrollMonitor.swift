import AppKit
import SwiftUI

enum HistoryPreviewModifierScrollPolicy {
    static func zoomDelta(
        scrollingDeltaY: CGFloat,
        modifiers: NSEvent.ModifierFlags,
        isInsidePreview: Bool
    ) -> CGFloat? {
        let deviceModifiers = modifiers.intersection(.deviceIndependentFlagsMask)
        guard isInsidePreview,
              !deviceModifiers.isDisjoint(with: [.command, .control]),
              scrollingDeltaY != 0 else { return nil }
        return scrollingDeltaY
    }
}

/// A local event monitor needs a view only as a geometry/window anchor. It
/// must never become the hit-test result itself, otherwise a representable
/// placed behind preview content can swallow button clicks.
final class HistoryPreviewModifierScrollView: NSView {
    override func hitTest(_ point: NSPoint) -> NSView? { nil }
}

/// Converts Command/Control + wheel into a local zoom action without
/// interfering with ordinary document scrolling elsewhere in the window.
struct HistoryPreviewModifierScrollMonitor: NSViewRepresentable {
    let onScroll: (CGFloat) -> Void

    func makeCoordinator() -> Coordinator {
        Coordinator(onScroll: onScroll)
    }

    func makeNSView(context: Context) -> HistoryPreviewModifierScrollView {
        let view = HistoryPreviewModifierScrollView(frame: .zero)
        context.coordinator.attach(to: view)
        return view
    }

    func updateNSView(_ view: HistoryPreviewModifierScrollView, context: Context) {
        context.coordinator.onScroll = onScroll
        context.coordinator.attach(to: view)
    }

    static func dismantleNSView(_ view: HistoryPreviewModifierScrollView, coordinator: Coordinator) {
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

                let localPoint = view.convert(event.locationInWindow, from: nil)
                guard let delta = HistoryPreviewModifierScrollPolicy.zoomDelta(
                    scrollingDeltaY: event.scrollingDeltaY,
                    modifiers: event.modifierFlags,
                    isInsidePreview: view.bounds.contains(localPoint)
                ) else { return event }
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
