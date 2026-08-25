import AppKit
import Foundation
import XCTest
@testable import TailSync

@MainActor
final class WindowLifecycleTests: XCTestCase {
    func testShowHistoryCallsCompletionAfterWindowIsPresented() {
        _ = NSApplication.shared
        let expectation = expectation(description: "history window presented")

        AppDelegate.showHistory { window in
            XCTAssertTrue(window.isVisible)
            window.close()
            expectation.fulfill()
        }

        wait(for: [expectation], timeout: 1)
    }

    func testHistoryWindowPolicyMovesNormalWindowToTheActiveSpace() {
        _ = NSApplication.shared
        let window = NSWindow()

        HistoryWindowPresentationPolicy.configure(window, isPinned: false)

        XCTAssertTrue(window.collectionBehavior.contains(.moveToActiveSpace))
        XCTAssertTrue(window.collectionBehavior.contains(.fullScreenAuxiliary))
        XCTAssertFalse(window.collectionBehavior.contains(.canJoinAllSpaces))
        XCTAssertEqual(window.level, .normal)
    }

    func testHistoryWindowPolicyPinsWindowAboveOtherWindowsAcrossSpaces() {
        _ = NSApplication.shared
        let window = NSWindow()

        HistoryWindowPresentationPolicy.configure(window, isPinned: true)

        XCTAssertTrue(window.collectionBehavior.contains(.canJoinAllSpaces))
        XCTAssertTrue(window.collectionBehavior.contains(.fullScreenAuxiliary))
        XCTAssertFalse(window.collectionBehavior.contains(.moveToActiveSpace))
        XCTAssertEqual(window.level, .floating)
    }

    func testHistoryWindowPinStatePersistsAndUpdatesAttachedWindow() {
        _ = NSApplication.shared
        let suiteName = "TailSync.WindowLifecycleTests.\(UUID().uuidString)"
        let defaults = try! XCTUnwrap(UserDefaults(suiteName: suiteName))
        defer { defaults.removePersistentDomain(forName: suiteName) }

        let controller = HistoryWindowController(defaults: defaults)
        let window = NSWindow()
        controller.attach(window)

        XCTAssertFalse(controller.isPinned)
        XCTAssertEqual(window.level, .normal)

        controller.togglePinned()

        XCTAssertTrue(controller.isPinned)
        XCTAssertEqual(window.level, .floating)
        XCTAssertTrue(defaults.bool(forKey: HistoryWindowController.isPinnedKey))

        let restored = HistoryWindowController(defaults: defaults)
        XCTAssertTrue(restored.isPinned)
        restored.attach(window)
        XCTAssertEqual(window.level, .floating)
    }

    func testTransientWindowDetachesOwnerAndContentWhenClosed() {
        _ = NSApplication.shared
        let window = NSWindow(contentViewController: NSViewController())
        var controller: TailSyncTransientWindowController?
        var closeCount = 0

        controller = TailSyncTransientWindowController(window: window) {
            closeCount += 1
            controller = nil
        }
        window.close()
        RunLoop.main.run(until: Date(timeIntervalSinceNow: 0.02))

        XCTAssertEqual(closeCount, 1)
        XCTAssertNil(controller)
        XCTAssertNil(window.contentViewController)
    }

    func testTransientWindowReportsVisibilityChangesOncePerState() {
        _ = NSApplication.shared
        let window = NSWindow(contentViewController: NSViewController())
        var states: [Bool] = []
        let controller = TailSyncTransientWindowController(
            window: window,
            onVisibilityChange: { states.append($0) },
            onClose: {}
        )

        controller.windowDidMiniaturize(Notification(name: NSWindow.didMiniaturizeNotification,
                                                     object: window))
        controller.windowDidMiniaturize(Notification(name: NSWindow.didMiniaturizeNotification,
                                                     object: window))
        controller.windowDidDeminiaturize(Notification(name: NSWindow.didDeminiaturizeNotification,
                                                       object: window))

        XCTAssertEqual(states, [false, true])
    }

    func testPreviewWindowIsRecreatedAfterClose() {
        _ = NSApplication.shared
        let model = HistoryPreviewViewModel(
            dependencies: HistoryPreviewDependencies(
                load: { id, _ in
                    let data = Data("preview".utf8)
                    return HistoryPreviewData(
                        kind: "text",
                        name: "preview.txt",
                        sizeBytes: Int64(data.count),
                        data: data,
                        entryId: id
                    )
                },
                restore: { _ in }
            )
        )
        let controller = HistoryPreviewWindowController(viewModel: model)
        let request = HistoryPreviewRequest(
            items: [HistoryPreviewItem(
                id: 7,
                batchId: nil,
                batchIndex: nil,
                batchCount: nil,
                category: "text",
                type: "text",
                nameHint: "preview.txt",
                sizeBytes: 7
            )],
            selectedIndex: 0
        )

        XCTAssertFalse(controller.hasAllocatedWindow)
        controller.present(request)
        XCTAssertTrue(controller.hasAllocatedWindow)

        controller.close()
        XCTAssertFalse(controller.hasAllocatedWindow)

        controller.present(request)
        XCTAssertTrue(controller.hasAllocatedWindow)
        controller.shutdown()
        XCTAssertFalse(controller.hasAllocatedWindow)
    }
}
