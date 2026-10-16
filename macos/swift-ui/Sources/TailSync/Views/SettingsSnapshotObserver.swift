import SwiftUI

/// One canonical-notification entry point for both settings windows.
struct SettingsSnapshotObserver: ViewModifier {
    @Binding var settings: AppSettings
    @Binding var persisted: AppSettings
    var center: NotificationCenter = .default
    var apply: (AppSettings) -> Void

    func body(content: Content) -> some View {
        content.onReceive(center.publisher(for: .tailSyncSettingsChanged)) { notification in
            guard let updated = notification.object as? AppSettings else { return }
            // Compare against the last committed values, not the current draft.
            // This also handles our own save notification after its direct
            // response has already preserved an unfinished slider/shortcut edit.
            let merged = SettingsDraftReconciliation.merge(
                current: settings, submitted: persisted, canonical: updated
            )
            persisted = updated
            apply(merged)
        }
    }
}
