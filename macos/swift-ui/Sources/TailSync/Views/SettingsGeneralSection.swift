import SwiftUI
import AppKit
extension SettingsView {
    var generalSection: some View {
        settingsCard(title: Loc.t("settings.general")) {
            settingRow {
                Text(Loc.t("settings.syncEnabled"))
                Spacer()
                Toggle("", isOn: $settings.sync_enabled)
                    .labelsHidden()
                    .toggleStyle(.switch)
                    .controlSize(.small)
                    .onChange(of: settings.sync_enabled) { _ in save() }
            }
            themedDivider.padding(.leading, 16)
            shortcutRow(.sync)
            themedDivider.padding(.leading, 16)
            shortcutRow(.history)
            themedDivider.padding(.leading, 16)
            settingRow {
                Text(Loc.t("settings.notifications"))
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
                Text(Loc.t("settings.progressBar"))
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
            Text(Loc.t(kind.titleKey))
            Spacer()
            if recordingShortcut == kind {
                Text(shortcutDraft.isEmpty
                     ? Loc.t("settings.shortcutRecording")
                     : ShortcutDisplayFormatter.string(for: shortcutDraft))
                    .font(.caption2.monospaced())
                    .foregroundColor(palette.accentColor)
                Button(Loc.t("settings.shortcutCancel")) { cancelShortcutRecording(kind) }
                    .buttonStyle(.borderless)
                    .disabled(shortcutBusy)
                Button(Loc.t("settings.shortcutSave")) { confirmShortcut(kind) }
                    .buttonStyle(.borderedProminent)
                    .controlSize(.small)
                    .disabled(shortcutDraft.isEmpty || shortcutBusy)
            } else {
                Text(kind.value(in: settings).isEmpty
                     ? Loc.t("settings.shortcutNone")
                     : ShortcutDisplayFormatter.string(for: kind.value(in: settings)))
                    .font(.caption2.monospaced())
                    .foregroundColor(palette.tertiaryColor)
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
