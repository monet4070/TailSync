import SwiftUI
import AppKit

extension SettingsView {
    func load() {
        isLoading = true
        loadErrorMessage = nil
        actionErrorMessage = nil
        Task { @MainActor in
            do {
                settings = try await ApiClient.shared.getSettings()
                persistedSettings = settings
                storageStatus = await ApiClient.shared.getStorageStatus()
                loc.lang = settings.language
                loc.notificationsEnabled = settings.notifications_enabled
                loc.applyTheme()
                isLoading = false
            } catch {
                loadErrorMessage = error.localizedDescription
                isLoading = false
            }
        }
    }

    func save(_ change: SettingsFieldChange) {
        guard !isLoading, !applyingPersistedSettings else { return }
        let value = settings
        actionErrorMessage = nil
        saveGeneration += 1
        let generation = saveGeneration
        Task { @MainActor in
            let outcome = await saveCoordinator.save(change, fallback: persistedSettings)
            guard generation == saveGeneration else { return }
            if let error = outcome.error {
                persistedSettings = outcome.persisted
                applyPersistedSettings(SettingsDraftReconciliation.merge(
                    current: settings, submitted: value, canonical: outcome.persisted
                ))
                actionErrorMessage = error
                return
            }
            persistedSettings = outcome.persisted
            applyPersistedSettings(SettingsDraftReconciliation.merge(
                current: settings, submitted: value, canonical: outcome.persisted
            ))
            NotificationCenter.default.post(name: .tailSyncSettingsChanged, object: outcome.persisted)
            saved = true
            do {
                try await Task.sleep(nanoseconds: 1_200_000_000)
            } catch {
                return
            }
            if generation == saveGeneration { saved = false }
        }
    }

    func applyPersistedSettings(_ value: AppSettings) {
        applyingPersistedSettings = true
        settings = value
        loc.lang = value.language
        loc.notificationsEnabled = value.notifications_enabled
        loc.applyTheme()
        Task { @MainActor in
            await Task.yield()
            applyingPersistedSettings = false
        }
    }

    /// Keep the storage display current without touching any other setting the
    /// user may be editing. The consolidated runtime snapshot already carries
    /// storage status, so this reuses it rather than polling `get_storage_status`
    /// (which would race the migration that changes the root).
    @MainActor
    func observeStorageStatus() async {
        var since: UInt64 = 0
        while !Task.isCancelled {
            guard let snapshot = await ApiClient.shared.waitForRuntimeSnapshot(since: since) else {
                // The local service is unreachable: the last reading is no longer
                // a trustworthy "storage is healthy" claim, so mark it unconfirmed
                // instead of leaving a stale value on screen.
                storageStatusUnconfirmed = true
                try? await Task.sleep(for: .milliseconds(1_000))
                continue
            }
            since = snapshot.revision
            guard let storage = snapshot.storage else { continue }
            storageStatusUnconfirmed = false
            let changed = storage.available != storageStatus?.available
                || storage.usedBytes != storageStatus?.usedBytes
                || storage.error != storageStatus?.error
            if changed {
                storageStatus = storage
            }
        }
    }
}
