import SwiftUI
import AppKit
import UserNotifications
extension SettingsView {
    var historySection: some View {
        settingsCard(title: Loc.t("settings.history")) {
            settingRow {
                settingTitle("settings.limit", descriptionKey: "settings.limitDescription")
                Spacer()
                historyLimitControl
            }
        }
    }

    var storageSection: some View {
        settingsCard(title: Loc.t("settings.storage")) {
            settingRow {
                Image(systemName: "externaldrive")
                    .foregroundColor(palette.secondaryColor)
                VStack(alignment: .leading, spacing: 2) {
                    Text(storageStatus?.root ?? settings.storage_root ?? "")
                        .lineLimit(1)
                        .truncationMode(.middle)
                    if let status = storageStatus {
                        Text(status.available
                             ? "\(formatBytes(status.usedBytes)) / \(formatBytes(settings.storage_quota_bytes))"
                             : (status.error ?? Loc.t("settings.error")))
                            .font(.caption2)
                            .foregroundColor(status.available ? palette.secondaryColor : palette.warningColor)
                    }
                }
                Spacer()
                Button {
                    chooseStorageLocation()
                } label: {
                    Label(
                        storageBusy ? Loc.t("settings.storageMoving") : Loc.t("settings.storageChange"),
                        systemImage: "folder"
                    )
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
                .frame(minWidth: 54)
                .disabled(storageBusy)
            }
            themedDivider.padding(.leading, 16)
            settingRow {
                settingTitle("settings.storageQuota", descriptionKey: "settings.storageQuotaDescription")
                Spacer()
                Stepper(value: Binding(
                    get: { Int(settings.storage_quota_bytes / (1024 * 1024 * 1024)) },
                    set: { value in
                        settings.storage_quota_bytes = UInt64(max(1, min(16_384, value))) * 1024 * 1024 * 1024
                        save()
                    }
                ), in: 1...16_384) {
                    Text("\(settings.storage_quota_bytes / (1024 * 1024 * 1024)) GiB")
                        .monospacedDigit()
                }
            }
            if let oldStorage, oldStorage.oldRoot != oldStorage.newRoot {
                themedDivider.padding(.leading, 16)
                settingRow {
                    Text(formatBytes(oldStorage.oldSizeBytes))
                        .foregroundColor(palette.secondaryColor)
                    Spacer()
                    Button(Loc.t("settings.storageDeleteOld"), role: .destructive) {
                        deleteOldStorage(oldStorage)
                    }
                    .buttonStyle(.bordered)
                    .controlSize(.small)
                    .frame(minWidth: 54)
                    Button(Loc.t("settings.storageKeepOld")) { self.oldStorage = nil }
                        .buttonStyle(.bordered)
                        .controlSize(.small)
                        .frame(minWidth: 54)
                }
            }
        }
    }

    func formatBytes(_ bytes: UInt64) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(clamping: bytes), countStyle: .file)
    }

    func chooseStorageLocation() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        storageBusy = true
        Task { @MainActor in
            defer { storageBusy = false }
            do {
                let result = try await ApiClient.shared.changeStorageLocation(parent: url.path)
                oldStorage = result
                settings = try await ApiClient.shared.getSettings()
                persistedSettings = settings
                storageStatus = await ApiClient.shared.getStorageStatus()
                let notice = UNMutableNotificationContent()
                notice.title = "TailSync"
                notice.body = Loc.t("settings.storageMoving")
                try? await UNUserNotificationCenter.current().add(
                    UNNotificationRequest(identifier: UUID().uuidString, content: notice, trigger: nil)
                )
            } catch {
                actionErrorMessage = error.localizedDescription
            }
        }
    }

    func deleteOldStorage(_ storage: ApiClient.StorageMigrationResult) {
        Task { @MainActor in
            do {
                try await ApiClient.shared.deleteOldStorage(path: storage.oldRoot)
                oldStorage = nil
            } catch {
                actionErrorMessage = error.localizedDescription
            }
        }
    }

    var appearanceSection: some View {
        settingsCard(title: Loc.t("settings.appearance")) {
            settingRow {
                settingTitle("settings.theme", descriptionKey: "settings.themeDescription")
                Spacer()
                Picker("", selection: Binding(
                    get: { loc.localThemeSettings.appearance },
                    set: { appearance in Task { @MainActor in await loc.selectLocalTheme(id: loc.localThemeSettings.activeThemeId, appearance: appearance) } }
                )) {
                    Label(Loc.t("settings.themeSystem"), systemImage: "desktopcomputer").tag("system")
                    Label(Loc.t("settings.themeLight"), systemImage: "sun.max").tag("light")
                    Label(Loc.t("settings.themeDark"), systemImage: "moon").tag("dark")
                }
                .pickerStyle(.segmented)
                .frame(width: 190)
            }
            themedDivider.padding(.leading, 16)
            colorThemePicker
            themedDivider.padding(.leading, 16)
            settingRow {
                settingTitle("settings.language", descriptionKey: "settings.languageDescription")
                Spacer()
                Picker("", selection: $settings.language) {
                    Text("English").tag("en")
                    Text("简体中文").tag("zh-CN")
                }
                .pickerStyle(.menu)
                .frame(width: 130)
                .onChange(of: settings.language) { language in
                    loc.lang = language
                    save()
                }
            }
        }
    }

}
