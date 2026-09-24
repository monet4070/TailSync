import SwiftUI
import AppKit

/// A single keycap badge, matching the macOS native keycap design from the prototype.
struct KeycapBadge: View {
    let palette: TailSyncThemePalette
    let token: String

    var body: some View {
        Text(token)
            .font(.system(size: 11, weight: .semibold, design: .rounded))
            .foregroundColor(palette.primaryColor)
            .padding(.horizontal, 6)
            .padding(.vertical, 3)
            .background(
                RoundedRectangle(cornerRadius: 4, style: .continuous)
                    .fill(palette.surfaceColor)
            )
            .overlay(
                RoundedRectangle(cornerRadius: 4, style: .continuous)
                    .stroke(palette.borderColor, lineWidth: 1)
            )
            .shadow(color: Color.black.opacity(0.08), radius: 1, y: 1)
    }
}

/// Renders a full shortcut ("Shift+CommandOrControl+S") as a row of keycap badges.
struct ShortcutKeycapRow: View {
    let palette: TailSyncThemePalette
    let shortcut: String

    var body: some View {
        let tokens = ShortcutDisplayFormatter.tokens(for: shortcut)
        HStack(spacing: 3) {
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
        let currentValue = kind.value(in: settings)
        let defaultValue = kind == .sync ? "Shift+CommandOrControl+S" : "CommandOrControl+Shift+V"
        let isRecording = recordingShortcut == kind

        return settingRow {
            VStack(alignment: .leading, spacing: 2) {
                Text(Loc.t(kind.titleKey))
                    .foregroundColor(palette.primaryColor)
                Text(Loc.t(kind.descriptionKey))
                    .font(.caption2)
                    .foregroundColor(palette.tertiaryColor)
                    .fixedSize(horizontal: false, vertical: true)
                if !shortcutError.isEmpty, shortcutErrorKind == kind {
                    Text(shortcutError)
                        .font(.caption2)
                        .foregroundColor(.red)
                        .lineLimit(2)
                }
            }
            Spacer(minLength: 16)

            HStack(spacing: 6) {
                // Integrated capsule keyboard control
                Button {
                    if isRecording {
                        if !shortcutDraft.isEmpty {
                            confirmShortcut(kind)
                        } else {
                            cancelShortcutRecording(kind)
                        }
                    } else {
                        startShortcutRecording(kind)
                    }
                } label: {
                    HStack(spacing: 6) {
                        Image(systemName: "keyboard")
                            .font(.system(size: 11))
                            .foregroundColor(isRecording ? palette.accentColor : palette.secondaryColor)

                        if isRecording {
                            Text(shortcutDraft.isEmpty ? Loc.t("settings.shortcutRecording") : ShortcutDisplayFormatter.string(for: shortcutDraft))
                                .font(.system(size: 11, weight: .medium))
                                .foregroundColor(palette.accentColor)
                        } else if currentValue.isEmpty {
                            Text(Loc.t("settings.shortcutNone"))
                                .font(.system(size: 11))
                                .foregroundColor(palette.tertiaryColor)
                        } else {
                            ShortcutKeycapRow(palette: palette, shortcut: currentValue)
                        }

                        Image(systemName: isRecording ? "checkmark" : "pencil")
                            .font(.system(size: 10))
                            .foregroundColor(isRecording ? palette.accentColor : palette.tertiaryColor)
                    }
                    .padding(.horizontal, 8)
                    .padding(.vertical, 4)
                    .background(palette.surfaceColor)
                    .cornerRadius(6)
                    .overlay(
                        RoundedRectangle(cornerRadius: 6)
                            .stroke(isRecording ? palette.accentColor : palette.borderColor, lineWidth: 1)
                    )
                }
                .buttonStyle(.plain)
                .disabled(shortcutBusy)

                // Reset to default button
                Button {
                    applyShortcut(kind, value: defaultValue)
                } label: {
                    Image(systemName: "arrow.counterclockwise")
                        .font(.system(size: 11))
                        .foregroundColor(currentValue == defaultValue ? palette.tertiaryColor.opacity(0.4) : palette.secondaryColor)
                        .frame(width: 24, height: 24)
                        .background(palette.surfaceColor)
                        .cornerRadius(5)
                        .overlay(RoundedRectangle(cornerRadius: 5).stroke(palette.borderColor, lineWidth: 1))
                }
                .buttonStyle(.plain)
                .disabled(currentValue == defaultValue || shortcutBusy || isRecording)
                .help(Loc.t("settings.resetDefault"))

                // Clear shortcut button
                Button {
                    applyShortcut(kind, value: "")
                } label: {
                    Image(systemName: "xmark")
                        .font(.system(size: 11))
                        .foregroundColor(currentValue.isEmpty ? palette.tertiaryColor.opacity(0.4) : palette.secondaryColor)
                        .frame(width: 24, height: 24)
                        .background(palette.surfaceColor)
                        .cornerRadius(5)
                        .overlay(RoundedRectangle(cornerRadius: 5).stroke(palette.borderColor, lineWidth: 1))
                }
                .buttonStyle(.plain)
                .disabled(currentValue.isEmpty || shortcutBusy || isRecording)
                .help(Loc.t("settings.clearShortcut"))
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
                confirmShortcut(kind)
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
        applyShortcut(kind, value: shortcutDraft)
    }

    func applyShortcut(_ kind: ShortcutKind, value: String) {
        let previous = kind.value(in: settings)
        let controller = GlobalShortcutController.shared
        shortcutBusy = true
        shortcutError = ""
        shortcutErrorKind = kind
        Task { @MainActor in
            let error = await GlobalShortcutController.apply(
                previous: previous,
                next: value,
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
                settings.sync_shortcut = value
                persistedSettings.sync_shortcut = value
            } else {
                settings.history_shortcut = value
                persistedSettings.history_shortcut = value
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
