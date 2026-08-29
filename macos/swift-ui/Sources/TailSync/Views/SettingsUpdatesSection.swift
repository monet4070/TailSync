import SwiftUI
import AppKit
extension SettingsView {
    var updatesSection: some View {
        settingsCard(title: Loc.t("settings.updates")) {
            settingRow {
                Image(systemName: appUpdatePhase == .available ? "arrow.down.circle.fill" : "arrow.down.circle")
                    .foregroundColor(appUpdateStatusColor)
                    .frame(width: 24)
                VStack(alignment: .leading, spacing: 2) {
                    Text("\(Loc.t("settings.currentVersion")): \(TailSyncAppVersion.current)")
                        .font(activeTheme.readingFont(size: 13, weight: .medium))
                    Text(appUpdateStatusText)
                        .font(activeTheme.readingFont(size: 10))
                        .foregroundColor(appUpdateStatusColor)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                Spacer(minLength: 8)
                Button {
                    if appUpdate != nil {
                        showingAppUpdateAlert = true
                    } else {
                        checkForAppUpdate()
                    }
                } label: {
                    Label(appUpdateActionTitle, systemImage: appUpdateActionIcon)
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
                .disabled(appUpdatePhase == .checking || appUpdatePhase == .installing)
            }
        }
    }

    var appUpdateStatusText: String {
        switch appUpdatePhase {
        case .ready:
            return Loc.t("settings.updateReady")
        case .checking:
            return Loc.t("settings.updateChecking")
        case .available:
            return Loc.t("settings.updateAvailable")
                .replacingOccurrences(of: "{version}", with: appUpdate?.version ?? "")
        case .current:
            return Loc.t("settings.updateCurrent")
        case .installing:
            return Loc.t("settings.updateInstalling")
        case .installed:
            return Loc.t("settings.updateInstalled")
        case .failed:
            return appUpdateErrorMessage ?? Loc.t("settings.updateFailed")
        }
    }

    var appUpdateStatusColor: Color {
        switch appUpdatePhase {
        case .current, .installed:
            return palette.positiveColor
        case .failed:
            return palette.warningColor
        default:
            return palette.secondaryColor
        }
    }

    var appUpdateActionTitle: String {
        appUpdate == nil ? Loc.t("settings.updateCheck") : Loc.t("settings.updateInstall")
    }

    var appUpdateActionIcon: String {
        appUpdate == nil ? "arrow.clockwise" : "arrow.down.circle"
    }

    var appUpdateAlertMessage: String {
        if let notes = appUpdate?.notes?.trimmingCharacters(in: .whitespacesAndNewlines), !notes.isEmpty {
            return notes
        }
        return Loc.t("update.installPrompt")
    }

    func checkForAppUpdate() {
        guard appUpdatePhase != .checking, appUpdatePhase != .installing else { return }
        appUpdate = nil
        appUpdateErrorMessage = nil
        appUpdatePhase = .checking
        Task { @MainActor in
            do {
                if let update = try await ApiClient.shared.checkForUpdate() {
                    appUpdate = update
                    appUpdatePhase = .available
                    showingAppUpdateAlert = true
                } else {
                    appUpdatePhase = .current
                }
            } catch {
                appUpdateErrorMessage = error.localizedDescription
                appUpdatePhase = .failed
            }
        }
    }

    func installAppUpdate() {
        guard appUpdate != nil else { return }
        showingAppUpdateAlert = false
        appUpdatePhase = .installing
        appUpdateErrorMessage = nil
        Task { @MainActor in
            do {
                guard try await AppDelegate.installAvailableUpdateAndRelaunch() else {
                    appUpdate = nil
                    appUpdatePhase = .current
                    return
                }
                appUpdatePhase = .installed
            } catch {
                appUpdateErrorMessage = error.localizedDescription
                appUpdatePhase = .failed
            }
        }
    }

}
