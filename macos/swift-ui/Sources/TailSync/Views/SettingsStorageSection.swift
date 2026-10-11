import SwiftUI
import AppKit
import UserNotifications
extension SettingsView {
    var historySection: some View {
        settingsCard(title: Loc.t("settings.history")) {
            settingRow {
                settingTitle(
                    "settings.limit",
                    descriptionKey: "settings.limitDescription",
                    placeholder: "{value}",
                    placeholderValue: "\(settings.history_limit)"
                )
                Spacer()
                historyLimitControl
            }
            themedDivider.padding(.leading, 16)
            settingRow {
                settingTitle(
                    "settings.autoSaveNetworkImages",
                    descriptionKey: "settings.autoSaveNetworkImagesDescription"
                )
                Spacer()
                Toggle("", isOn: $settings.auto_save_network_images)
                    .labelsHidden()
                    .toggleStyle(.switch)
                    .controlSize(.small)
                    .accessibilityLabel(Loc.t("settings.autoSaveNetworkImages"))
                    .onChange(of: settings.auto_save_network_images) { _ in save() }
            }
            themedDivider.padding(.leading, 16)
            settingRow {
                settingTitle("settings.qqImageAccess", descriptionKey: "settings.qqImageAccessDescription")
                Spacer()
                Button(Loc.t(qqImageAccessAuthorized ? "settings.qqImageAccessChange" : "settings.qqImageAccessChoose")) {
                    chooseQQImageCache()
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
                .disabled(qqImageAccessBusy)
                .accessibilityLabel(Loc.t("settings.qqImageAccessChoose"))
            }
            if let key = qqImageAccessMessageKey {
                Text(Loc.t(key))
                    .font(.caption)
                    .foregroundColor(palette.secondaryColor)
                    .fixedSize(horizontal: false, vertical: true)
                    .padding(.horizontal, 16)
                    .padding(.bottom, 10)
            }
        }
        .task {
            qqImageAccessAuthorized = await QQImageCacheAccessController.status() == .authorized
            if qqImageAccessAuthorized { qqImageAccessMessageKey = "settings.qqImageAccessReady" }
        }
    }

    func chooseQQImageCache() {
        qqImageAccessBusy = true
        Task { @MainActor in
            defer { qqImageAccessBusy = false }
            switch await QQImageCacheAccessController.chooseDirectory(language: loc.lang) {
            case .granted:
                qqImageAccessAuthorized = true
                qqImageAccessMessageKey = "settings.qqImageAccessReady"
            case .cancelled:
                break
            case .invalidDirectory:
                qqImageAccessMessageKey = "settings.qqImageAccessInvalid"
            default:
                qqImageAccessMessageKey = "settings.qqImageAccessFailed"
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
                    if storageStatusUnconfirmed {
                        Text(Loc.t("settings.storageStatusUnconfirmed"))
                            .font(.caption2)
                            .foregroundColor(palette.warningColor)
                    } else if let status = storageStatus {
                        Text(status.available
                             ? "\(formatBytes(status.usedBytes)) / \(formatBytes(settings.storage_quota_bytes))"
                             : Loc.t("settings.storageCapacityUnavailable"))
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
            if !storageStatusUnconfirmed, storageStatus?.available == false {
                themedDivider.padding(.leading, 16)
                storageNoticeRow(
                    "\(Loc.t("settings.storageUnavailable")) \(Loc.t("settings.storageRecoveryGuidance"))"
                )
            }
            if let storageErrorMessage {
                themedDivider.padding(.leading, 16)
                storageNoticeRow(storageErrorMessage)
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

    func storageNoticeRow(_ message: String) -> some View {
        HStack(alignment: .top, spacing: 8) {
            Image(systemName: "exclamationmark.triangle")
                .foregroundColor(palette.warningColor)
            Text(message)
                .font(.caption)
                .foregroundColor(palette.secondaryColor)
                .fixedSize(horizontal: false, vertical: true)
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 8)
        .accessibilityElement(children: .combine)
        .accessibilityLabel(message)
    }

    func chooseStorageLocation() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        storageBusy = true
        storageErrorMessage = nil
        Task { @MainActor in
            defer { storageBusy = false }
            do {
                let result = try await ApiClient.shared.changeStorageLocation(parent: url.path)
                oldStorage = result
                settings = try await ApiClient.shared.getSettings()
                persistedSettings = settings
                storageStatus = await ApiClient.shared.getStorageStatus()
                // The migration succeeded: drop any older failure message so the
                // settings page reflects the current state.
                storageErrorMessage = nil
                let notice = UNMutableNotificationContent()
                notice.title = "TailSync"
                notice.body = Loc.t("settings.storageChanged")
                try? await UNUserNotificationCenter.current().add(
                    UNNotificationRequest(identifier: UUID().uuidString, content: notice, trigger: nil)
                )
            } catch {
                storageErrorMessage = error.localizedDescription
            }
        }
    }

    func deleteOldStorage(_ storage: ApiClient.StorageMigrationResult) {
        Task { @MainActor in
            do {
                try await ApiClient.shared.deleteOldStorage(path: storage.oldRoot)
                oldStorage = nil
                storageErrorMessage = nil
            } catch {
                storageErrorMessage = error.localizedDescription
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
