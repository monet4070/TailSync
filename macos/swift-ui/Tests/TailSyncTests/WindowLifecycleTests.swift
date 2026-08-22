import AppKit
import Foundation
import XCTest
@testable import TailSync

@MainActor
final class WindowLifecycleTests: XCTestCase {
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
