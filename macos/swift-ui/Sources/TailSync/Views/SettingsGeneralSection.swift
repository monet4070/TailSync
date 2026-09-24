import SwiftUI
import AppKit

/// A three-dimensional macOS keycap badge for one shortcut token (⌘, ⇧, S…).
struct KeycapBadge: View {
    @Environment(\.colorScheme) private var colorScheme
    let palette: TailSyncThemePalette
    let token: String

    var body: some View {
        Text(token)
            .font(.caption2.monospaced().weight(.medium))
            .foregroundColor(palette.primaryColor)
            .frame(minWidth: token.count > 1 ? 24 : 20, minHeight: 20)
            .padding(.horizontal, 5)
            .background(
                RoundedRectangle(cornerRadius: 5, style: .continuous)
                    .fill(
                        LinearGradient(
                            colors: [
                                palette.raisedColor,
                                palette.surfaceColor,
                            ],
                            startPoint: .top,
                            endPoint: .bottom
                        )
                    )
            )
            .overlay {
                RoundedRectangle(cornerRadius: 5, style: .continuous)
                    .stroke(palette.borderColor.opacity(0.9), lineWidth: 0.8)
            }
            .shadow(color: .black.opacity(0.14), radius: 1, y: 1)
            .shadow(color: .white.opacity(colorScheme == .light ? 0.55 : 0.12), radius: 0.6, y: -0.6)
    }
}

/// Renders a full shortcut ("Shift+CommandOrControl+S") as a row of keycap badges.
struct ShortcutKeycapRow: View {
    let palette: TailSyncThemePalette
    let shortcut: String

    var body: some View {
        let tokens = ShortcutDisplayFormatter.tokens(for: shortcut)
        HStack(spacing: 4) {
            ForEach(Array(tokens.enumerated()), id: \.offset) { _, token in
                KeycapBadge(palette: palette, token: token)
            }
        }
    }
}

extension SettingsView {
    func settingTitle(
        _ titleKey: String,
        descriptionKey: String? = nil
    ) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(Loc.t(titleKey))
            if let descriptionKey {
                Text(Loc.t(descriptionKey))
                    .font(.caption2)
                    .foregroundColor(palette.tertiaryColor)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    var generalSection: some View {
        settingsCard(title: Loc.t("settings.general")) {
            settingRow {
                settingTitle("settings.syncEnabled", descriptionKey: "settings.syncEnabledDescription")
                Spacer()
                Toggle("", isOn: $settings.sync_enabled)
                    .labelsHidden()
                    .toggleStyle(.switch)
                    .controlSize(.small)
                    .onChange(of: settings.sync_enabled) { _ in save() }
            }
            themedDivider.padding(.leading, 16)
            settingRow {
                VStack(alignment: .leading, spacing: 2) {
                    settingTitle("settings.launchAtLogin", descriptionKey: "settings.launchAtLoginDescription")
                    if launchAtLogin.requiresApproval {
                        Text(Loc.t("settings.launchAtLoginApproval"))
                            .font(.caption2)
                            .foregroundColor(palette.warningColor)
                    } else if let message = launchAtLogin.errorMessage {
                        Text("\(Loc.t("settings.launchAtLoginError")) \(message)")
                            .font(.caption2)
                            .foregroundColor(palette.warningColor)
                            .lineLimit(2)
                    }
                }
                Spacer()
                if launchAtLogin.requiresApproval {
                    Button(Loc.t("settings.launchAtLoginOpenSettings")) {
                        launchAtLogin.openSystemSettings()
                    }
                    .buttonStyle(.borderless)
                    .controlSize(.small)
                }
                Toggle("", isOn: Binding(
                    get: { launchAtLogin.isRequested },
                    set: { launchAtLogin.setEnabled($0) }
                ))
                .labelsHidden()
                .toggleStyle(.switch)
                .controlSize(.small)
                .accessibilityLabel(Loc.t("settings.launchAtLogin"))
            }
            themedDivider.padding(.leading, 16)
            shortcutRow(.sync)
            themedDivider.padding(.leading, 16)
            shortcutRow(.history)
            themedDivider.padding(.leading, 16)
            settingRow {
                settingTitle("settings.notifications", descriptionKey: "settings.notificationsDescription")
                Spacer()
                Toggle("", isOn: $settings.notifications_enabled)
                    .labelsHidden()
                    .toggleStyle(.switch)
                    .controlSize(.small)
                    .onChange(of: settings.notifications_enabled) { value in
                        loc.notificationsEnabled = value
                        save()
                    }
            }
            themedDivider.padding(.leading, 16)
            settingRow {
                settingTitle("settings.progressBar", descriptionKey: "settings.progressBarDescription")
                Spacer()
                Toggle("", isOn: $settings.progress_bar_enabled)
                    .labelsHidden()
                    .toggleStyle(.switch)
                    .controlSize(.small)
                    .onChange(of: settings.progress_bar_enabled) { _ in save() }
            }
        }
    }

    func shortcutRow(_ kind: ShortcutKind) -> some View {
        settingRow {
            settingTitle(kind.titleKey, descriptionKey: kind.descriptionKey)
            Spacer()
            if recordingShortcut == kind {
                HStack(spacing: 4) {
                    if shortcutDraft.isEmpty {
                        Text(Loc.t("settings.shortcutRecording"))
                            .font(.caption2.monospaced())
                            .foregroundColor(palette.accentColor)
                    } else {
                        ShortcutKeycapRow(palette: palette, shortcut: shortcutDraft)
                    }
                }
                Button(Loc.t("settings.shortcutCancel")) { cancelShortcutRecording(kind) }
                    .buttonStyle(.borderless)
                    .disabled(shortcutBusy)
                Button(Loc.t("settings.shortcutSave")) { confirmShortcut(kind) }
                    .buttonStyle(.borderedProminent)
                    .controlSize(.small)
                    .disabled(shortcutDraft.isEmpty || shortcutBusy)
            } else {
                Group {
                    if kind.value(in: settings).isEmpty {
                        Text(Loc.t("settings.shortcutNone"))
                            .font(.caption2.monospaced())
                            .foregroundColor(palette.tertiaryColor)
                    } else {
                        ShortcutKeycapRow(palette: palette, shortcut: kind.value(in: settings))
                    }
                }
                Button(Loc.t(kind.recordKey)) { startShortcutRecording(kind) }
                    .buttonStyle(.borderless)
                    .disabled(shortcutBusy || recordingShortcut != nil)
            }
            if !shortcutError.isEmpty, shortcutErrorKind == kind {
                Text(shortcutError)
                    .font(.caption2)
                    .foregroundColor(.red)
                    .lineLimit(2)
            }
        }
    }

    func startShortcutRecording(_ kind: ShortcutKind) {
        guard recordingShortcut == nil, !shortcutBusy else { return }
        GlobalShortcutController.shared.unregister()
        shortcutDraft = ""
        shortcutError = ""
        shortcutErrorKind = nil
        shortcutBusy = false
        recordingShortcut = kind
        shortcutMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { event in
            guard recordingShortcut == kind else { return event }
            if let shortcut = Self.capturedShortcut(from: event) {
                shortcutDraft = shortcut
                shortcutError = ""
            }
            return nil
        }
    }

    func cancelShortcutRecording(_ kind: ShortcutKind) {
        finishShortcutRecording()
        if case .failure(let error) =
            GlobalShortcutController.shared.register(
                syncShortcut: settings.sync_shortcut,
                historyShortcut: settings.history_shortcut
            ) {
            shortcutError = error.message
            shortcutErrorKind = kind
        }
    }

    func finishShortcutRecording() {
        if let monitor = shortcutMonitor {
            NSEvent.removeMonitor(monitor)
            shortcutMonitor = nil
        }
        recordingShortcut = nil
        shortcutDraft = ""
        shortcutError = ""
        shortcutErrorKind = nil
    }

    func confirmShortcut(_ kind: ShortcutKind) {
        guard recordingShortcut == kind, !shortcutDraft.isEmpty else { return }
        let next = shortcutDraft
        let previous = kind.value(in: settings)
        let controller = GlobalShortcutController.shared
        shortcutBusy = true
        shortcutError = ""
        shortcutErrorKind = kind
        Task { @MainActor in
            let error = await GlobalShortcutController.apply(
                previous: previous,
                next: next,
                register: { candidate in
                    controller.register(
                        syncShortcut: kind == .sync ? candidate : settings.sync_shortcut,
                        historyShortcut: kind == .history ? candidate : settings.history_shortcut
                    )
                },
                persist: { candidate in
                    if kind == .sync {
                        return await ApiClient.shared.setSyncShortcut(candidate)
                    }
                    return await ApiClient.shared.setHistoryShortcut(candidate)
                }
            )
            shortcutBusy = false
            if let error {
                shortcutError = error
                return
            }
            finishShortcutRecording()
            if kind == .sync {
                settings.sync_shortcut = next
                persistedSettings.sync_shortcut = next
            } else {
                settings.history_shortcut = next
                persistedSettings.history_shortcut = next
            }
        }
    }

    static func capturedShortcut(from event: NSEvent) -> String? {
        var modifiers: [String] = []
        if event.modifierFlags.contains(.command) { modifiers.append("CommandOrControl") }
        if event.modifierFlags.contains(.control) { modifiers.append("Control") }
        if event.modifierFlags.contains(.option) { modifiers.append("Alt") }
        if event.modifierFlags.contains(.shift) { modifiers.append("Shift") }
        guard !modifiers.isEmpty else { return nil }
        let keyCode = UInt32(event.keyCode)
        guard let name = ShortcutParser.keyCodeName(for: keyCode) else { return nil }
        return (modifiers + [name]).joined(separator: "+")
    }

}
