import AppKit
import SwiftUI
import XCTest
@testable import TailSync

@MainActor
final class SettingsSnapshotObserverTests: XCTestCase {
    func testOwnSaveNotificationDoesNotUndoThePreservedSliderDraft() async {
        let state = Draft()
        let center = NotificationCenter()
        let host = NSHostingView(rootView: Subscriber(state: state, center: center))
        host.frame = NSRect(x: 0, y: 0, width: 100, height: 100)
        host.layoutSubtreeIfNeeded()
        await settle()
        var submitted = state.settings
        submitted.notifications_enabled = false
        state.settings = submitted
        state.settings.history_limit = 230 // Still dragging when save returns.
        state.settings.sync_shortcut = "Alt+X" // A dedicated command finished.
        state.persisted = submitted
        state.settings = SettingsDraftReconciliation.merge(
            current: state.settings, submitted: submitted, canonical: submitted
        )
        await settle()
        center.post(name: .tailSyncSettingsChanged, object: submitted)
        await settle()
        XCTAssertEqual(state.settings.history_limit, 230)
        XCTAssertEqual(state.settings.sync_shortcut, "Alt+X")
        XCTAssertFalse(state.settings.notifications_enabled)
        XCTAssertEqual(state.persisted, submitted)
        withExtendedLifetime(host) {}
    }

    func testOtherWindowSnapshotUpdatesCleanFieldsAndPreservesPendingEdits() async {
        let state = Draft()
        let center = NotificationCenter()
        let host = NSHostingView(rootView: Subscriber(state: state, center: center))
        host.frame = NSRect(x: 0, y: 0, width: 100, height: 100)
        host.layoutSubtreeIfNeeded()
        await settle()
        state.settings.history_limit = 230
        state.settings.storage_quota_bytes *= 2
        var external = state.persisted
        external.language = "zh-CN"
        external.history_limit = 150
        await settle()
        center.post(name: .tailSyncSettingsChanged, object: external)
        await settle()
        XCTAssertEqual(state.settings.history_limit, 230)
        XCTAssertEqual(state.settings.storage_quota_bytes, 20 * 1024 * 1024 * 1024)
        XCTAssertEqual(state.settings.language, "zh-CN")
        XCTAssertEqual(state.persisted, external)

        // Once this window commits, a later external update must not remain
        // hidden behind a stale notion of a dirty draft.
        state.persisted = state.settings
        external = state.settings
        external.history_limit = 300
        await settle()
        center.post(name: .tailSyncSettingsChanged, object: external)
        await settle()
        XCTAssertEqual(state.settings.history_limit, 300)
        withExtendedLifetime(host) {}
    }

    private func settle() async {
        try? await Task.sleep(for: .milliseconds(30))
    }

    private final class Draft: ObservableObject {
        @Published var settings = AppSettings()
        @Published var persisted = AppSettings()
    }

    private struct Subscriber: View {
        @ObservedObject var state: Draft
        let center: NotificationCenter
        var body: some View {
            Text("Settings notification fixture")
                .modifier(SettingsSnapshotObserver(
                    settings: $state.settings, persisted: $state.persisted,
                    center: center, apply: { state.settings = $0 }
                ))
        }
    }
}
