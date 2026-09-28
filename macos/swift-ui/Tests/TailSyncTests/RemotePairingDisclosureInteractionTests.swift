import AppKit
import SwiftUI
import XCTest
@testable import TailSync

final class RemotePairingDisclosureInteractionTests: XCTestCase {
    @MainActor
    func testRemotePairingDisclosureOpensOnClick() throws {
        _ = NSApplication.shared
        let selection = TailSyncThemeSelection(builtin: .tailsync)
        let palette = selection.palette(for: .light)
        let state = DisclosureState()
        let disclosure = DisclosureHarness(state: state, palette: palette)
            .frame(width: 480)
        let host = NSHostingView(rootView: disclosure)
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 480, height: 120),
            styleMask: [.titled],
            backing: .buffered,
            defer: false
        )
        window.contentView = host
        host.frame = NSRect(x: 0, y: 0, width: 480, height: 120)
        window.makeKeyAndOrderFront(nil)
        defer { window.orderOut(nil) }
        host.layoutSubtreeIfNeeded()
        XCTAssertFalse(state.expanded)
        let collapsedHeight = host.fittingSize.height
        let point = NSPoint(x: 100, y: 30)
        for (type, eventNumber) in [(NSEvent.EventType.leftMouseDown, 1), (.leftMouseUp, 2)] {
            let event = try XCTUnwrap(NSEvent.mouseEvent(
                with: type,
                location: point,
                modifierFlags: [],
                timestamp: ProcessInfo.processInfo.systemUptime,
                windowNumber: window.windowNumber,
                context: nil,
                eventNumber: eventNumber,
                clickCount: 1,
                pressure: type == .leftMouseDown ? 1 : 0
            ))
            window.sendEvent(event)
        }
        RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        XCTAssertTrue(state.expanded)
        host.layoutSubtreeIfNeeded()
        XCTAssertGreaterThan(host.fittingSize.height, collapsedHeight)
    }
}

private final class DisclosureState: ObservableObject {
    @Published var expanded = false
}

private struct DisclosureHarness: View {
    @ObservedObject var state: DisclosureState
    let palette: TailSyncThemePalette

    var body: some View {
        RemotePairingDisclosure(isExpanded: $state.expanded, palette: palette) {
            Text("Pairing controls")
        }
    }
}
