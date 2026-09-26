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

    func save() {
        guard !isLoading, !applyingPersistedSettings else { return }
        let value = settings
        actionErrorMessage = nil
        saveGeneration += 1
        let generation = saveGeneration
        Task { @MainActor in
            let outcome = await saveCoordinator.save(value, fallback: persistedSettings)
            guard generation == saveGeneration else { return }
            if let error = outcome.error {
                persistedSettings = outcome.persisted
                applyPersistedSettings(outcome.persisted)
                actionErrorMessage = error
                return
            }
            persistedSettings = outcome.persisted
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
}
