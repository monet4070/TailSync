import SwiftUI
import AppKit
import UserNotifications
import UniformTypeIdentifiers

private func routeInterfaceLabel(_ interface: String) -> String {
    switch interface {
    case "lan": return "LAN"
    case "iroh": return "Iroh"
    default: return "Tailscale"
    }
}

struct SettingsPollingPlan {
    let refreshPairingStatus: Bool
    let refreshPeers: Bool
}

enum SettingsPollingPolicy {
    static func next(
        applicationIsActive: Bool,
        peerRefreshTicks: inout Int
    ) -> SettingsPollingPlan {
        var refreshPeers = false
        if applicationIsActive {
            peerRefreshTicks += 1
            if peerRefreshTicks >= 5 {
                peerRefreshTicks = 0
                refreshPeers = true
            }
        }
        return SettingsPollingPlan(
            refreshPairingStatus: true,
            refreshPeers: refreshPeers
        )
    }
}

private actor SettingsSaveCoordinator {
    private enum SaveResult: Sendable {
        case success(AppSettings)
        case failure(String, AppSettings)
    }

    private var tail: Task<SaveResult, Never>?

    func save(
        _ settings: AppSettings,
        fallback: AppSettings
    ) async -> (error: String?, persisted: AppSettings) {
        let predecessor = tail
        let job = Task<SaveResult, Never> {
            let lastPersisted: AppSettings
            if let result = await predecessor?.value {
                switch result {
                case .success(let settings): lastPersisted = settings
                case .failure(_, let settings): lastPersisted = settings
                }
            } else {
                lastPersisted = fallback
            }
            do {
                try await ApiClient.shared.updateSettings(settings)
                return .success(settings)
            } catch {
                return .failure(error.localizedDescription, lastPersisted)
            }
        }
        tail = job
        switch await job.value {
        case .success(let settings): return (nil, settings)
        case .failure(let message, let settings): return (message, settings)
        }
    }
}

enum ThemeV2CardLayout {
    static let previewHeight: CGFloat = 68
    static let minimumCardHeight: CGFloat = 108
    static let minimumActionHitSize: CGFloat = 28
}

enum ThemeV2CardPreviewPolicy {
    static func selection(
        themeId: String,
        catalogue: [TailSyncThemeDefinition],
        reduceTransparency: Bool,
        interfaceScale: CGFloat
    ) -> TailSyncThemeSelection {
        TailSyncThemeSelection(
            storedValue: themeId,
            catalogue: catalogue,
            reduceTransparency: reduceTransparency,
            interfaceScale: interfaceScale
        )
    }
}

struct ThemeV2CardView: View {
    let descriptor: ApiClient.ThemeV2Descriptor
    let name: String
    let selected: Bool
    let selection: TailSyncThemeSelection
    let colorScheme: ColorScheme
    let onSelect: () -> Void
    let onUpdate: () -> Void
    let onRollback: () -> Void
    let onDelete: () -> Void

    @State private var hovering = false

    private var palette: TailSyncThemePalette {
        selection.palette(for: colorScheme)
    }

    private var metrics: TailSyncThemeMetrics {
        selection.metrics(for: colorScheme)
    }

    private var typography: TailSyncThemeTypography {
        selection.typography(for: colorScheme)
    }

    private var cardRadius: CGFloat {
        min(14, max(3, metrics.cardRadius))
    }

    private var sourceLabel: String {
        if descriptor.status != "valid" {
            return descriptor.diagnostics.map(\.message).joined(separator: " ")
        }
        return descriptor.source == "builtin"
            ? Loc.t("settings.themePackageBuiltIn")
            : descriptor.version
    }

    private var cardBackground: Color {
        if selected { return palette.activeColor }
        if hovering { return palette.hoverColor }
        return palette.surfaceColor
    }

    private var previewTitle: String {
        typography.uppercasesSectionTitles ? "TAILSYNC" : "TailSync"
    }

    var body: some View {
        ZStack(alignment: .topTrailing) {
            Button(action: onSelect) {
                VStack(alignment: .leading, spacing: 0) {
                    previewBand
                        .frame(height: ThemeV2CardLayout.previewHeight)
                    HStack(spacing: 8) {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(name)
                                .font(selection.displayFont(size: 11, weight: .semibold))
                                .foregroundColor(palette.primaryColor)
                                .lineLimit(1)
                                .minimumScaleFactor(0.75)
                            Text(sourceLabel)
                                .font(selection.readingFont(size: 9))
                                .foregroundColor(palette.tertiaryColor)
                                .lineLimit(1)
                                .truncationMode(.tail)
                        }
                        Spacer(minLength: 4)
                        if selected {
                            Image(systemName: "checkmark")
                                .font(.system(size: 9, weight: .bold))
                                .foregroundColor(palette.accentContrastColor)
                                .frame(width: 20, height: 20)
                                .background(palette.accentColor)
                                .clipShape(RoundedRectangle(cornerRadius: min(7, cardRadius), style: .continuous))
                        }
                    }
                    .padding(.horizontal, 10)
                    .padding(.vertical, 7)
                }
                .frame(maxWidth: .infinity, minHeight: ThemeV2CardLayout.minimumCardHeight, alignment: .topLeading)
                .background(cardBackground)
                .contentShape(Rectangle())
                .clipShape(RoundedRectangle(cornerRadius: cardRadius, style: .continuous))
                .overlay {
                    RoundedRectangle(cornerRadius: cardRadius, style: .continuous)
                        .stroke(
                            selected || hovering ? palette.accentColor : palette.borderColor,
                            lineWidth: selected ? 2 : 1
                        )
                }
            }
            .buttonStyle(.plain)
            .frame(maxWidth: .infinity)
            .disabled(descriptor.status != "valid")
            .accessibilityLabel("\(name), \(sourceLabel)")

            actionButtons
                .padding(6)
        }
        .frame(maxWidth: .infinity, minHeight: ThemeV2CardLayout.minimumCardHeight)
        .opacity(descriptor.status == "valid" ? 1 : 0.7)
        .onHover { hovering = $0 }
    }

    private var previewBand: some View {
        ZStack(alignment: .leading) {
            palette.windowColor
            Rectangle()
                .fill(palette.accentColor)
                .frame(width: 4)
            VStack(alignment: .leading, spacing: 7) {
                HStack(spacing: 6) {
                    Text(previewTitle)
                        .font(selection.displayFont(size: 10, weight: .semibold))
                        .foregroundColor(palette.primaryColor)
                        .lineLimit(1)
                        .minimumScaleFactor(0.7)
                    Spacer(minLength: 4)
                    previewSwatch(palette.accentColor)
                    previewSwatch(palette.secondaryColor)
                    previewSwatch(palette.borderStrongColor)
                }
                RoundedRectangle(cornerRadius: 1)
                    .fill(palette.secondaryColor.opacity(0.55))
                    .frame(maxWidth: .infinity, minHeight: 2, maxHeight: 2)
                HStack(spacing: 6) {
                    RoundedRectangle(cornerRadius: min(5, max(1, metrics.controlRadius)))
                        .fill(palette.inputColor)
                        .overlay(alignment: .leading) {
                            RoundedRectangle(cornerRadius: 1)
                                .fill(palette.tertiaryColor.opacity(0.55))
                                .frame(width: 44, height: 2)
                                .padding(.leading, 7)
                        }
                    RoundedRectangle(cornerRadius: min(5, max(1, metrics.controlRadius)))
                        .fill(palette.accentSoftColor)
                        .frame(width: 34)
                        .overlay {
                            RoundedRectangle(cornerRadius: 1)
                                .fill(palette.accentColor)
                                .frame(width: 13, height: 2)
                        }
                }
                .frame(height: 16)
            }
            .padding(.leading, 12)
            .padding(.trailing, descriptor.source == "builtin" ? 10 : 72)
            .padding(.vertical, 9)
        }
        .clipped()
    }

    private func previewSwatch(_ color: Color) -> some View {
        RoundedRectangle(cornerRadius: 2, style: .continuous)
            .fill(color)
            .frame(width: 9, height: 9)
    }

    @ViewBuilder
    private var actionButtons: some View {
        if descriptor.source != "builtin" {
            HStack(spacing: 4) {
                if descriptor.status == "valid" {
                    actionButton(
                        systemName: "arrow.triangle.2.circlepath",
                        label: Loc.t("settings.themePackageUpdate"),
                        action: onUpdate
                    )
                    actionButton(
                        systemName: "arrow.uturn.backward",
                        label: Loc.t("settings.themePackageRollback"),
                        action: onRollback
                    )
                }
                actionButton(
                    systemName: "trash",
                    label: Loc.t("settings.themePackageDelete"),
                    action: onDelete
                )
            }
        }
    }

    private func actionButton(
        systemName: String,
        label: String,
        action: @escaping () -> Void
    ) -> some View {
        Button(action: action) {
            Image(systemName: systemName)
                .font(.system(size: 11, weight: .medium))
                .foregroundColor(palette.primaryColor)
                .frame(
                    width: ThemeV2CardLayout.minimumActionHitSize,
                    height: ThemeV2CardLayout.minimumActionHitSize
                )
                .background(palette.raisedColor.opacity(0.96))
                .clipShape(RoundedRectangle(cornerRadius: min(7, cardRadius), style: .continuous))
                .overlay {
                    RoundedRectangle(cornerRadius: min(7, cardRadius), style: .continuous)
                        .stroke(palette.borderColor, lineWidth: 1)
                }
        }
        .buttonStyle(.plain)
        .accessibilityLabel(label)
        .help(label)
    }
}

enum ThemePackageVersionRelation: Equatable {
    case upgrade
    case same
    case downgrade
}

struct ThemePackageUpdateOptions: Equatable {
    let allowSameVersion: Bool
    let allowDowngrade: Bool

    static func forRelation(_ relation: ThemePackageVersionRelation?) -> Self {
        Self(
            allowSameVersion: relation == .same,
            allowDowngrade: relation == .downgrade
        )
    }
}

struct ThemePackageSemanticVersion: Comparable {
    let core: [UInt32]
    let prerelease: [String]?

    init?(_ value: String) {
        let buildParts = value.split(separator: "+", maxSplits: 1, omittingEmptySubsequences: false)
        guard buildParts.count <= 2,
              buildParts.count == 1 || Self.validIdentifiers(String(buildParts[1]), numericLeadingZeroesAllowed: true) else { return nil }
        let versionParts = buildParts[0].split(separator: "-", maxSplits: 1, omittingEmptySubsequences: false)
        let numbers = versionParts[0].split(separator: ".", omittingEmptySubsequences: false)
        guard numbers.count == 3 else { return nil }
        var parsed: [UInt32] = []
        for number in numbers {
            let text = String(number)
            guard !text.isEmpty, text == "0" || !text.hasPrefix("0"), let value = UInt32(text) else { return nil }
            parsed.append(value)
        }
        if versionParts.count == 2 {
            let value = String(versionParts[1])
            guard Self.validIdentifiers(value, numericLeadingZeroesAllowed: false) else { return nil }
            prerelease = value.split(separator: ".").map(String.init)
        } else {
            prerelease = nil
        }
        core = parsed
    }

    private static func validIdentifiers(_ value: String, numericLeadingZeroesAllowed: Bool) -> Bool {
        let identifiers = value.split(separator: ".", omittingEmptySubsequences: false)
        return !identifiers.isEmpty && identifiers.allSatisfy { identifier in
            let text = String(identifier)
            guard !text.isEmpty,
                  text.unicodeScalars.allSatisfy({ CharacterSet.alphanumerics.contains($0) || $0 == "-" }) else { return false }
            return numericLeadingZeroesAllowed || !text.allSatisfy(\.isNumber) || text == "0" || !text.hasPrefix("0")
        }
    }

    static func < (lhs: Self, rhs: Self) -> Bool {
        for index in lhs.core.indices where lhs.core[index] != rhs.core[index] {
            return lhs.core[index] < rhs.core[index]
        }
        switch (lhs.prerelease, rhs.prerelease) {
        case (nil, nil): return false
        case (nil, .some): return false
        case (.some, nil): return true
        case let (.some(left), .some(right)):
            for index in 0..<max(left.count, right.count) {
                guard index < left.count else { return true }
                guard index < right.count else { return false }
                let a = left[index], b = right[index]
                if a == b { continue }
                let aNumeric = a.allSatisfy(\.isNumber)
                let bNumeric = b.allSatisfy(\.isNumber)
                if aNumeric && bNumeric {
                    if a.count != b.count { return a.count < b.count }
                    return a < b
                }
                if aNumeric != bNumeric { return aNumeric }
                return a < b
            }
            return false
        }
    }

    static func relation(candidate: String, installed: String) -> ThemePackageVersionRelation? {
        guard let candidate = Self(candidate), let installed = Self(installed) else { return nil }
        if candidate == installed { return .same }
        return candidate < installed ? .downgrade : .upgrade
    }
}

struct SettingsView: View {
    private enum ThemePackageOperation {
        case install
        case update(themeId: String, installedVersion: String)

        var isInstall: Bool {
            if case .install = self { return true }
            return false
        }
    }

    private struct PendingThemeImport: Identifiable {
        let id = UUID()
        let path: String
        let digest: String
        let standard: TailSyncThemeDefinition
        let highContrast: TailSyncThemeDefinition
        let diagnostics: [ApiClient.ThemeDiagnostic]
        let assetImages: [String: NSImage]
        let candidateVersion: String
        let versionRelation: ThemePackageVersionRelation?
        let operation: ThemePackageOperation
    }
    private enum ShortcutKind: Equatable {
        case sync
        case history

        var titleKey: String {
            self == .sync ? "settings.syncShortcut" : "settings.historyShortcut"
        }

        var recordKey: String {
            self == .sync ? "settings.shortcutRecord" : "settings.historyShortcutRecord"
        }

        func value(in settings: AppSettings) -> String {
            self == .sync ? settings.sync_shortcut : settings.history_shortcut
        }
    }

    private struct PeerRoute: Identifiable {
        let peer: ApiClient.PeerSnapshot
        let address: String
        let interface: String?
        let online: Bool
        let connected: Bool
        let status: String
        let latencyMs: Int?
        let isPairingEndpoint: Bool
        let rttCapable: Bool

        var id: String { "\(peer.hostname)-\(interface ?? "unknown")-\(address)" }
    }

    @ObservedObject private var loc = Loc.shared
    @Environment(\.colorScheme) private var colorScheme
    @State private var settings = AppSettings()
    @State private var persistedSettings = AppSettings()
    @State private var applyingPersistedSettings = false
    @State private var localDevice: ApiClient.DeviceSnapshot?
    @State private var peers: [ApiClient.PeerSnapshot] = []
    @State private var pairedPeerEndpoints: [String: String] = [:]
    @State private var isLoading = true
    @State private var peersLoading = false
    @State private var saved = false
    @State private var loadErrorMessage: String?
    @State private var actionErrorMessage: String?
    @State private var peerError: String?
    @State private var pairingStatus: ApiClient.PairingStatus?
    @State private var pairingMessage: String?
    @State private var pairingInProgress = false
    @State private var showPairingSheet = false
    @State private var previousPairingPhase: String?
    @State private var testingPeers: Set<String> = []
    @State private var removingPeers: Set<String> = []
    @State private var testResults: [String: (latencyMs: Int, path: String, error: String)] = [:]
    @State private var peerLoadGeneration = 0
    @State private var peerRequestInFlight = false
    @State private var saveGeneration = 0
    @State private var saveCoordinator = SettingsSaveCoordinator()
    @State private var storageStatus: ApiClient.StorageStatus?
    @State private var storageBusy = false
    @State private var oldStorage: ApiClient.StorageMigrationResult?
    @State private var recordingShortcut: ShortcutKind?
    @State private var shortcutDraft = ""
    @State private var shortcutError = ""
    @State private var shortcutErrorKind: ShortcutKind?
    @State private var shortcutBusy = false
    @State private var shortcutMonitor: Any?
    @State private var pendingThemeImport: PendingThemeImport?
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize

    private var activeTheme: TailSyncThemeSelection {
        TailSyncThemeSelection(
            storedValue: loc.colorTheme,
            catalogue: loc.resolvedV2Themes,
            reduceTransparency: loc.reduceTransparency,
            interfaceScale: TailSyncThemeAccessibilityPolicy.interfaceScale(for: dynamicTypeSize)
        )
    }

    private var palette: TailSyncThemePalette {
        activeTheme.palette(for: colorScheme)
    }

    private func component(_ name: String, state: String = "default") -> TailSyncThemeComponentTokens? {
        activeTheme.component(name, state: state, scheme: colorScheme)
    }

    var body: some View {
        Group {
            if let loadErrorMessage {
                VStack(spacing: 12) {
                    Image(systemName: "exclamationmark.triangle")
                        .font(.system(size: 28))
                        .foregroundColor(palette.warningColor)
                    Text(Loc.t("settings.error"))
                        .font(activeTheme.displayFont(size: 17, weight: .semibold))
                    Text(loadErrorMessage).font(.caption).foregroundColor(palette.secondaryColor)
                    Button(Loc.t("settings.retry")) { load() }
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if isLoading {
                ProgressView().controlSize(.small)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                ScrollView {
                    VStack(spacing: 14) {
                        generalSection
                        historySection
                        storageSection
                        networkSection
                        appearanceSection
                    }
                    .padding(.vertical, 12)
                }
                .overlay(alignment: .bottom) {
                    if saved || actionErrorMessage != nil {
                        let toast = component("toast")
                        Text(actionErrorMessage ?? Loc.t("settings.saved"))
                            .font(.caption)
                            .foregroundColor(toast?.foregroundColor ?? palette.toastTextColor)
                            .padding(.horizontal, toast?.padding ?? 12)
                            .padding(.vertical, 6)
                            .background(toast?.backgroundColor ?? palette.toastColor)
                            .clipShape(RoundedRectangle(cornerRadius: toast?.radius ?? 999, style: .continuous))
                            .padding(.bottom, 8)
                    }
                }
            }
        }
        .task {
            load()
            var peerRefreshTicks = 0
            while !Task.isCancelled {
                let pollingPlan = SettingsPollingPolicy.next(
                    applicationIsActive: NSApp.isActive,
                    peerRefreshTicks: &peerRefreshTicks
                )
                if pollingPlan.refreshPairingStatus {
                    await refreshPairingStatus()
                }
                if pollingPlan.refreshPeers && !isLoading && !peerRequestInFlight {
                    loadPeers(showLoading: false)
                }
                try? await Task.sleep(nanoseconds: 1_000_000_000)
            }
        }
        .sheet(isPresented: $showPairingSheet) {
            pairingSheet
        }
        .sheet(item: $pendingThemeImport) { preview in
            themeImportPreview(preview)
        }
        .onReceive(
            NotificationCenter.default.publisher(for: GlobalShortcutController.syncStateChanged)
        ) { notification in
            if let enabled = notification.userInfo?["enabled"] as? Bool {
                settings.sync_enabled = enabled
            }
        }
        .onDisappear {
            if let recordingShortcut {
                cancelShortcutRecording(recordingShortcut)
            }
        }
        .tailSyncThemed()
    }

    private var generalSection: some View {
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

    private func shortcutRow(_ kind: ShortcutKind) -> some View {
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

    private func startShortcutRecording(_ kind: ShortcutKind) {
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

    private func cancelShortcutRecording(_ kind: ShortcutKind) {
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

    private func finishShortcutRecording() {
        if let monitor = shortcutMonitor {
            NSEvent.removeMonitor(monitor)
            shortcutMonitor = nil
        }
        recordingShortcut = nil
        shortcutDraft = ""
        shortcutError = ""
        shortcutErrorKind = nil
    }

    private func confirmShortcut(_ kind: ShortcutKind) {
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

    private static func capturedShortcut(from event: NSEvent) -> String? {
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

    private var historySection: some View {
        settingsCard(title: Loc.t("settings.history")) {
            settingRow {
                Text(Loc.t("settings.limit"))
                Spacer()
                historyLimitControl
            }
        }
    }

    private var storageSection: some View {
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
                .disabled(storageBusy)
            }
            themedDivider.padding(.leading, 16)
            settingRow {
                Text(Loc.t("settings.storageQuota"))
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
                    Button(Loc.t("settings.storageKeepOld")) { self.oldStorage = nil }
                }
            }
        }
    }

    private func formatBytes(_ bytes: UInt64) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(clamping: bytes), countStyle: .file)
    }

    private func chooseStorageLocation() {
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

    private func deleteOldStorage(_ storage: ApiClient.StorageMigrationResult) {
        Task { @MainActor in
            do {
                try await ApiClient.shared.deleteOldStorage(path: storage.oldRoot)
                oldStorage = nil
            } catch {
                actionErrorMessage = error.localizedDescription
            }
        }
    }

    private var networkSection: some View {
        settingsCard(title: Loc.t("settings.network")) {
            settingRow {
                Text(Loc.t("settings.connectionMode"))
                Spacer()
                Picker("", selection: $settings.connection_mode) {
                    Text(Loc.t("settings.modeAuto")).tag("auto")
                    Text(Loc.t("settings.modeLan")).tag("lan_only")
                    Text(Loc.t("settings.modeTailscale")).tag("tailscale_only")
                }
                .pickerStyle(.segmented)
                .frame(width: 240)
                .onChange(of: settings.connection_mode) { _ in
                    changeConnectionMode()
                }
            }

            themedDivider.padding(.leading, 16)
            localIdentityRow

            themedDivider.padding(.leading, 16)
            pairingPanel

            themedDivider.padding(.leading, 16)
            peerList
        }
    }

    private var localIdentityRow: some View {
        settingRow {
            Image(systemName: "laptopcomputer")
                .foregroundColor(palette.accentColor)
                .frame(width: 24)
            VStack(alignment: .leading, spacing: 2) {
                Text(localDevice?.hostname ?? Loc.t("settings.localDevice"))
                    .font(.body.weight(.medium))
                Text(localDevice?.fingerprint ?? Loc.t("settings.identityLoading"))
                    .font(.caption2.monospaced())
                    .foregroundColor(palette.tertiaryColor)
                    .textSelection(.enabled)
                if let endpoint = localDevice?.iroh_endpoint_id {
                    Text("iroh: \(endpoint)")
                        .font(.caption2.monospaced())
                        .foregroundColor(palette.tertiaryColor)
                        .textSelection(.enabled)
                }
            }
            Spacer()
        }
    }

    private var pairingPanel: some View {
        settingRow {
            Image(systemName: "link.badge.plus")
                .foregroundColor(palette.accentColor)
                .frame(width: 24)
            VStack(alignment: .leading, spacing: 2) {
                Text(Loc.t("settings.pairDevice"))
                    .font(.body.weight(.medium))
                Text(pairingWindowSummary)
                    .font(.caption2)
                    .foregroundColor(palette.tertiaryColor)
            }
            Spacer()
            Button(pairingStatus?.pairing_enabled == true
                   ? Loc.t("settings.closePairing")
                   : Loc.t("settings.allowPairing")) {
                togglePairingWindow()
            }
            .buttonStyle(.bordered)
            .disabled(pairingInProgress)
            if let pairingMessage {
                Text(pairingMessage)
                    .font(.caption2)
                    .foregroundColor(.red)
                    .textSelection(.enabled)
            }
        }
    }

    private var pairingWindowSummary: String {
        guard let status = pairingStatus, status.pairing_enabled else {
            return Loc.t("settings.pairingClosed")
        }
        return "\(Loc.t("settings.waitingPairing")) · \(status.remaining_seconds)s · \(status.failed_attempts)/\(status.max_failures)"
    }

    private var pairingSheet: some View {
        VStack(spacing: 14) {
            Image(systemName: "lock.shield")
                .font(.system(size: 28))
                .foregroundColor(palette.accentColor)
            Text(pairingStatus?.peer?.hostname ?? Loc.t("settings.pairDevice"))
                .font(activeTheme.displayFont(size: 17, weight: .semibold))

            if let peer = pairingStatus?.peer {
                Text(peer.verification_code)
                    .font(.system(size: 34, weight: .bold, design: .monospaced))
                    .textSelection(.enabled)
                Text(Loc.t("settings.compareCode"))
                    .font(.caption)
                    .foregroundColor(palette.secondaryColor)
                    .multilineTextAlignment(.center)
                Text(peer.fingerprint)
                    .font(.caption2.monospaced())
                    .foregroundColor(palette.tertiaryColor)
                    .textSelection(.enabled)
                if peer.local_confirmed {
                    Text(Loc.t("settings.waitingPeerConfirm"))
                        .font(.caption)
                        .foregroundColor(palette.accentColor)
                }
            } else if pairingInProgress || pairingStatus?.phase == "handshaking" {
                ProgressView(Loc.t("settings.secureHandshake"))
                    .controlSize(.small)
            } else {
                VStack(spacing: 7) {
                    Text(Loc.t("settings.waitingPairing"))
                        .font(.caption.weight(.medium))
                    Text(Loc.t("settings.pairingInstruction"))
                        .font(.caption2)
                        .foregroundColor(palette.secondaryColor)
                        .multilineTextAlignment(.center)
                }
            }

            if let message = pairingMessage ?? pairingStatus?.error {
                Text(message)
                    .font(.caption2)
                    .foregroundColor(.red)
                    .multilineTextAlignment(.center)
            }

            HStack(spacing: 10) {
                Button(Loc.t("settings.cancel")) { cancelPairing() }
                    .keyboardShortcut(.cancelAction)
                Button(
                    pairingStatus?.peer?.local_confirmed == true
                    ? Loc.t("settings.confirmed")
                    : Loc.t("settings.codesMatch")
                ) { confirmPairing() }
                    .buttonStyle(.borderedProminent)
                    .disabled(
                        pairingInProgress
                        || pairingStatus?.peer == nil
                        || pairingStatus?.peer?.local_confirmed == true
                    )
            }
        }
        .padding(24)
        .frame(width: 340)
        .frame(minHeight: 300)
        .background(palette.windowColor)
        .interactiveDismissDisabled(pairingStatus?.pairing_enabled == true)
    }

    @ViewBuilder
    private var peerList: some View {
        if peersLoading {
            settingRow {
                ProgressView().controlSize(.small)
                Text(Loc.t("settings.loadingDevices"))
                    .font(.caption)
                    .foregroundColor(palette.secondaryColor)
                Spacer()
            }
        } else if peers.isEmpty {
            settingRow {
                Text(peerError ?? Loc.t("settings.noDevices"))
                    .font(.caption)
                    .foregroundColor(peerError == nil ? palette.secondaryColor : palette.warningColor)
                Spacer()
                Button { refreshPeers() } label: { Image(systemName: "arrow.clockwise") }
                    .buttonStyle(.plain)
                    .help(Loc.t("settings.refresh"))
            }
        } else {
            HStack {
                Text("\(peerRoutes.count) \(Loc.t("settings.devices"))")
                    .font(.caption2)
                    .foregroundColor(palette.tertiaryColor)
                Spacer()
                Button { refreshPeers() } label: { Image(systemName: "arrow.clockwise") }
                    .buttonStyle(.plain)
                    .help(Loc.t("settings.refresh"))
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 6)

            ForEach(Array(peerRoutes.enumerated()), id: \.element.id) { index, route in
                if index > 0 { themedDivider.padding(.leading, 48) }
                peerRow(route)
            }
        }
    }

    private var peerRoutes: [PeerRoute] {
        peers.flatMap { peer in
            let availableAddresses = Set(peer.candidates.map(\.address) + [peer.address])
            let savedPairingAddress = pairedPeerEndpoints[peer.hostname]
            let pairedAddress = savedPairingAddress
                .flatMap { availableAddresses.contains($0) ? $0 : nil }
                ?? (peer.trusted ? peer.candidates.first?.address ?? peer.address : nil)
            if !peer.routes.isEmpty {
                return peer.routes.map {
                    PeerRoute(
                        peer: peer,
                        address: $0.address,
                        interface: $0.interface,
                        online: $0.online,
                        connected: $0.connected,
                        status: $0.connected ? "connected" : $0.status,
                        latencyMs: $0.latencyMs,
                        isPairingEndpoint: peer.trusted
                            && ($0.pairingEndpoint || $0.address == pairedAddress),
                        rttCapable: $0.rttCapable
                    )
                }
            }
            if settings.connection_mode == "auto", !peer.candidates.isEmpty {
                return peer.candidates.map {
                    PeerRoute(
                        peer: peer,
                        address: $0.address,
                        interface: $0.interface,
                        online: $0.online,
                        connected: peer.current_address == $0.address,
                        status: peer.current_address == $0.address ? "connected" : $0.status,
                        latencyMs: $0.latency,
                        isPairingEndpoint: peer.trusted && $0.address == pairedAddress,
                        rttCapable: $0.rttCapable
                    )
                }
            }
            return [PeerRoute(
                peer: peer,
                address: peer.address,
                interface: routeInterface(for: settings.connection_mode),
                online: peer.candidates.first?.online ?? peer.online,
                connected: peer.current_address == peer.address,
                status: peer.current_address == peer.address
                    ? "connected"
                    : peer.candidates.first?.status ?? peer.status,
                latencyMs: peer.candidates.first?.latency,
                isPairingEndpoint: peer.trusted && peer.address == pairedAddress,
                rttCapable: true
            )]
        }
    }

    private func routeInterface(for mode: String) -> String? {
        switch mode {
        case "lan_only": return "lan"
        case "tailscale_only": return "tailscale"
        default: return nil
        }
    }

    private func peerRow(_ route: PeerRoute) -> some View {
        let peer = route.peer
        return settingRow {
            Circle()
                .fill(statusColor(route.status))
                .frame(width: 8, height: 8)
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(peer.hostname).font(.body.weight(.medium))
                    Image(systemName: peer.trusted ? "checkmark.shield.fill" : "exclamationmark.shield")
                        .font(.caption2)
                        .foregroundColor(peer.trusted ? palette.positiveColor : palette.warningColor)
                    Text(peer.trusted ? Loc.t("settings.paired") : Loc.t("settings.notPaired"))
                        .font(.caption2)
                        .foregroundColor(peer.trusted ? palette.positiveColor : palette.warningColor)
                }
                if let version = peer.requiredProtocolVersion {
                    Text(
                        Loc.t("settings.protocolUpgradeRequired")
                            .replacingOccurrences(of: "{version}", with: String(version))
                    )
                    .font(.caption2)
                    .foregroundColor(palette.warningColor)
                    .fixedSize(horizontal: false, vertical: true)
                }
                HStack(spacing: 7) {
                    Text(route.address.isEmpty ? Loc.t("settings.pairedOffline") : route.address)
                        .font(.caption.monospaced())
                        .foregroundColor(palette.secondaryColor)
                        .lineLimit(1)
                        .truncationMode(.middle)
                        .help(route.address)
                    if let interface = route.interface {
                        Text("· \(routeInterfaceLabel(interface))")
                            .font(.caption2.weight(.medium))
                            .foregroundColor(palette.accentColor)
                    }
                    Text(statusText(route.status))
                        .font(.caption2.weight(route.connected ? .semibold : .regular))
                        .foregroundColor(statusColor(route.status))
                    if let latencyMs = route.latencyMs,
                       ["online", "connected", "confirming"].contains(route.status) {
                        Text("\(latencyMs) ms")
                            .font(.caption2.monospaced())
                            .foregroundColor(palette.tertiaryColor)
                    }
                    if peer.trusted, !peer.fingerprint.isEmpty {
                        Text(peer.fingerprint)
                            .font(.caption2.monospaced())
                            .foregroundColor(palette.tertiaryColor)
                    }
                    if let result = testResults[route.id] {
                        let label = result.error.isEmpty
                            ? "\(result.latencyMs) ms"
                                + (result.path == "relay" ? " · \(Loc.t("settings.relayPath"))" : "")
                            : result.error
                        Text(label)
                            .font(.caption2)
                            .foregroundColor(result.error.isEmpty ? .green : .red)
                            .lineLimit(1)
                    }
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            Spacer()

            if route.address.isEmpty {
                EmptyView()
            } else if testingPeers.contains(route.id) {
                ProgressView().controlSize(.small)
            } else {
                Button { testPeer(route) } label: {
                    Image(systemName: "bolt.horizontal")
                        .frame(width: 22, height: 22)
                }
                .buttonStyle(.plain)
                .disabled(route.interface == "iroh" && !route.rttCapable)
                .help(route.interface == "iroh" && !route.rttCapable
                    ? Loc.t("settings.testRouteRediscover")
                    : Loc.t("settings.testConnection"))
            }

            if route.isPairingEndpoint, removingPeers.contains(peer.hostname) {
                ProgressView()
                    .controlSize(.small)
                    .frame(width: 48)
            } else if route.isPairingEndpoint {
                Button { forgetPeer(peer.hostname) } label: {
                    Text(Loc.t("settings.removeDevice"))
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
                .tint(.red)
                .help(Loc.t("settings.unpair"))
            } else if !peer.trusted {
                Button {
                    startPairing(route)
                } label: {
                    Image(systemName: "link.badge.plus")
                        .frame(width: 22, height: 22)
                }
                .buttonStyle(.plain)
                .help(Loc.t("settings.pair"))
            }

            if route.isPairingEndpoint {
                Toggle("", isOn: Binding(
                    get: { peer.enabled },
                    set: { togglePeer(peer.hostname, enabled: $0) }
                ))
                .labelsHidden()
                .toggleStyle(.switch)
                .controlSize(.small)
            }
        }
    }

    private var appearanceSection: some View {
        settingsCard(title: Loc.t("settings.appearance")) {
            settingRow {
                Text(Loc.t("settings.theme"))
                Spacer()
                Picker("", selection: Binding(
                    get: { loc.localThemeSettings.appearance },
                    set: { appearance in Task { @MainActor in await loc.selectLocalTheme(id: loc.localThemeSettings.activeThemeId, appearance: appearance) } }
                )) {
                    Text(Loc.t("settings.themeSystem")).tag("system")
                    Text(Loc.t("settings.themeLight")).tag("light")
                    Text(Loc.t("settings.themeDark")).tag("dark")
                }
                .pickerStyle(.menu)
                .frame(width: 130)
            }
            themedDivider.padding(.leading, 16)
            colorThemePicker
            themedDivider.padding(.leading, 16)
            settingRow {
                Text(Loc.t("settings.language"))
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

    private func load() {
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
                loadPeers()
            } catch {
                loadErrorMessage = error.localizedDescription
                isLoading = false
            }
        }
    }

    private func loadPeers(clearExisting: Bool = false, showLoading: Bool = true) {
        peerLoadGeneration += 1
        let generation = peerLoadGeneration
        let requestedMode = settings.connection_mode
        if clearExisting {
            peers = []
            testResults = [:]
        }
        peerRequestInFlight = true
        if showLoading {
            peersLoading = true
        }
        peerError = nil
        Task { @MainActor in
            let result = await ApiClient.shared.getPeers()
            guard generation == peerLoadGeneration,
                  requestedMode == settings.connection_mode else { return }
            applyPeerResult(result, showLoading: showLoading)
        }
    }

    private func applyPeerResult(_ result: ApiClient.PeersResult, showLoading: Bool) {
        if result.requestSucceeded {
            localDevice = result.local
            peers = result.peers.filter { peer in
                peer.online || peer.trusted || peer.status == "discovered"
            }
            pairedPeerEndpoints = result.pairedEndpoints
        }
        peerError = result.error
        peerRequestInFlight = false
        if showLoading {
            peersLoading = false
        }
    }

    private func refreshPeers() {
        guard !peerRequestInFlight else { return }
        peerLoadGeneration += 1
        let generation = peerLoadGeneration
        let requestedMode = settings.connection_mode
        peerRequestInFlight = true
        peersLoading = true
        Task { @MainActor in
            let result = await ApiClient.shared.refreshPeers()
            guard generation == peerLoadGeneration,
                  requestedMode == settings.connection_mode else { return }
            applyPeerResult(result, showLoading: true)
        }
    }

    private func statusText(_ status: String) -> String {
        switch status {
        case "connected": return Loc.t("settings.connected")
        case "online": return Loc.t("settings.online")
        case "confirming": return Loc.t("settings.confirming")
        case "discovered": return Loc.t("settings.discovered")
        default: return Loc.t("settings.offline")
        }
    }

    private func statusColor(_ status: String) -> Color {
        switch status {
        case "connected", "online": return palette.positiveColor
        case "confirming", "discovered": return palette.warningColor
        default: return palette.tertiaryColor
        }
    }

    private func changeConnectionMode() {
        peerLoadGeneration += 1
        let generation = peerLoadGeneration
        let requestedMode = settings.connection_mode
        let value = settings
        peers = []
        testResults = [:]
        peerError = nil
        peersLoading = true

        Task { @MainActor in
            do {
                let outcome = await saveCoordinator.save(value, fallback: persistedSettings)
                if let message = outcome.error {
                    guard generation == peerLoadGeneration else { return }
                    persistedSettings = outcome.persisted
                    applyPersistedSettings(outcome.persisted)
                    throw ApiError.serverError(message)
                }
                persistedSettings = outcome.persisted
                guard generation == peerLoadGeneration,
                      requestedMode == settings.connection_mode else { return }
                saved = true
                loadPeers()
                try? await Task.sleep(nanoseconds: 1_200_000_000)
                saved = false
            } catch {
                guard generation == peerLoadGeneration else { return }
                peerRequestInFlight = false
                peersLoading = false
                actionErrorMessage = error.localizedDescription
            }
        }
    }

    private func save() {
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

    private func applyPersistedSettings(_ value: AppSettings) {
        applyingPersistedSettings = true
        settings = value
        loc.lang = value.language
        // Theme is local-only V2 state; never hydrate it from synced
        // AppSettings.color_theme/theme.
        loc.notificationsEnabled = value.notifications_enabled
        loc.applyTheme()
        Task { @MainActor in
            await Task.yield()
            applyingPersistedSettings = false
        }
    }

    private func togglePairingWindow() {
        if pairingStatus?.pairing_enabled == true {
            cancelPairing()
            return
        }
        pairingInProgress = true
        pairingMessage = nil
        showPairingSheet = true
        Task { @MainActor in
            do {
                pairingStatus = try await ApiClient.shared.enablePairing()
            } catch {
                pairingMessage = pairingErrorDescription(error)
            }
            pairingInProgress = false
        }
    }

    private func startPairing(_ route: PeerRoute) {
        guard !route.address.isEmpty else { return }
        pairingInProgress = true
        pairingMessage = nil
        showPairingSheet = true
        Task { @MainActor in
            do {
                if pairingStatus?.pairing_enabled != true {
                    pairingStatus = try await ApiClient.shared.enablePairing()
                }
                pairingStatus = try await ApiClient.shared.startPairing(address: route.address)
            } catch {
                pairingMessage = pairingErrorDescription(error)
                pairingStatus = try? await ApiClient.shared.getPairingStatus()
            }
            pairingInProgress = false
        }
    }

    private func confirmPairing() {
        pairingInProgress = true
        pairingMessage = nil
        Task { @MainActor in
            do {
                pairingStatus = try await ApiClient.shared.confirmPairing()
            } catch {
                pairingMessage = pairingErrorDescription(error)
            }
            pairingInProgress = false
        }
    }

    private func cancelPairing() {
        pairingInProgress = true
        Task { @MainActor in
            do {
                pairingStatus = try await ApiClient.shared.cancelPairing()
                showPairingSheet = false
            } catch {
                pairingMessage = pairingErrorDescription(error)
            }
            pairingInProgress = false
        }
    }

    private func pairingErrorDescription(_ error: Error) -> String {
        (error as? ApiError)?.pairingErrorDescription ?? error.localizedDescription
    }

    @MainActor
    private func refreshPairingStatus() async {
        guard let status = try? await ApiClient.shared.getPairingStatus() else { return }
        pairingStatus = status
        if status.peer != nil && ["verification", "waiting_for_peer"].contains(status.phase) {
            showPairingSheet = true
        }
        if status.phase == "paired", previousPairingPhase != "paired" {
            showPairingSheet = false
            loadPeers()
        }
        previousPairingPhase = status.phase
    }

    private func forgetPeer(_ hostname: String) {
        guard !removingPeers.contains(hostname) else { return }
        removingPeers.insert(hostname)
        peers.removeAll { $0.hostname == hostname }
        testResults = testResults.filter { !$0.key.hasPrefix("\(hostname)-") }

        Task { @MainActor in
            do {
                try await ApiClient.shared.forgetPeer(hostname: hostname)
                settings.enabled_peers.removeValue(forKey: hostname)
                persistedSettings.enabled_peers.removeValue(forKey: hostname)
                loadPeers()
            } catch {
                peerError = error.localizedDescription
                loadPeers()
            }
            removingPeers.remove(hostname)
        }
    }

    private func togglePeer(_ hostname: String, enabled: Bool) {
        Task { @MainActor in
            if await ApiClient.shared.togglePeer(hostname: hostname, enabled: enabled) {
                settings.enabled_peers[hostname] = enabled
                persistedSettings.enabled_peers[hostname] = enabled
                loadPeers()
            }
        }
    }

    private func testPeer(_ route: PeerRoute) {
        guard !route.address.isEmpty else { return }
        testingPeers.insert(route.id)
        Task { @MainActor in
            testResults[route.id] = await ApiClient.shared.testConnection(address: route.address)
                ?? (0, "", Loc.t("settings.connectionFailed"))
            testingPeers.remove(route.id)
        }
    }

    private var historyLimitControl: some View {
        HStack(spacing: 12) {
            GeometryReader { geometry in
                let thumbSize: CGFloat = 18
                let travelWidth = max(1, geometry.size.width - thumbSize)
                let progress = CGFloat(settings.history_limit - 10) / 490

                ZStack(alignment: .leading) {
                    Capsule()
                        .fill(palette.borderColor.opacity(0.65))
                        .frame(height: 5)
                        .padding(.horizontal, thumbSize / 2)

                    Capsule()
                        .fill(palette.accentColor)
                        .frame(width: max(1, travelWidth * progress), height: 5)
                        .offset(x: thumbSize / 2)

                    Circle()
                        .fill(palette.raisedColor)
                        .overlay {
                            Circle()
                                .stroke(palette.accentColor.opacity(0.75), lineWidth: 1)
                        }
                        .shadow(color: .black.opacity(0.18), radius: 2.5, y: 1)
                        .frame(width: thumbSize, height: thumbSize)
                        .offset(x: travelWidth * progress)
                }
                .frame(maxHeight: .infinity)
                .contentShape(Rectangle())
                .gesture(
                    DragGesture(minimumDistance: 0)
                        .onChanged { value in
                            let position = min(travelWidth, max(0, value.location.x - thumbSize / 2))
                            let step = Int((position / travelWidth * 49).rounded())
                            settings.history_limit = 10 + step * 10
                        }
                        .onEnded { _ in save() }
                )
                .accessibilityElement()
                .accessibilityLabel(Loc.t("settings.limit"))
                .accessibilityValue("\(settings.history_limit)")
                .accessibilityAdjustableAction { direction in
                    switch direction {
                    case .increment: adjustHistoryLimit(by: 10)
                    case .decrement: adjustHistoryLimit(by: -10)
                    @unknown default: break
                    }
                }
            }
            .frame(width: 180, height: 28)

            Text("\(settings.history_limit)")
                .font(.system(.caption, design: .monospaced).weight(.medium))
                .foregroundColor(palette.accentColor)
                .frame(width: 46, height: 26)
                .background(palette.accentSoftColor)
                .clipShape(RoundedRectangle(cornerRadius: activeTheme.metrics.controlRadius, style: .continuous))
        }
    }

    private func adjustHistoryLimit(by delta: Int) {
        settings.history_limit = min(500, max(10, settings.history_limit + delta))
        save()
    }

    private func settingsCard<Content: View>(title: String, @ViewBuilder content: () -> Content) -> some View {
        let section = component("section")
        let panel = component("panel")
        return VStack(alignment: .leading, spacing: 0) {
            Text(title)
                .font(activeTheme.displayFont(
                    size: activeTheme.typography.sectionTitleSize,
                    weight: activeTheme.builtin == .tailsync ? .regular : .semibold
                ))
                .textCase(activeTheme.typography.uppercasesSectionTitles ? .uppercase : nil)
                .foregroundColor(section?.foregroundColor ?? palette.secondaryColor)
                .padding(.horizontal, 16)
                .padding(.bottom, 6)
            VStack(spacing: 0) { content() }
                .background(panel?.backgroundColor ?? palette.surfaceColor)
                .clipShape(RoundedRectangle(cornerRadius: panel?.radius ?? activeTheme.metrics.cardRadius, style: .continuous))
                .overlay {
                    RoundedRectangle(cornerRadius: panel?.radius ?? activeTheme.metrics.cardRadius, style: .continuous)
                        .stroke(panel?.borderColor ?? palette.borderColor, lineWidth: activeTheme.builtin == .highContrast ? 2 : 1)
                }
                .shadow(
                    color: palette.primaryColor.opacity(panel?.shadowOpacity ?? (activeTheme.metrics.shadowRadius == 0 ? 0 : 0.08)),
                    radius: panel?.shadowRadius ?? activeTheme.metrics.shadowRadius,
                    y: panel?.shadowY ?? (activeTheme.metrics.shadowRadius > 0 ? 3 : 0)
                )
                .padding(.horizontal, 12)
        }
    }

    private func settingRow<Content: View>(@ViewBuilder content: () -> Content) -> some View {
        HStack(spacing: 8) { content() }
            .font(activeTheme.readingFont(size: 13))
            .padding(.horizontal, 16)
            .padding(.vertical, activeTheme.metrics.rowPadding)
            .frame(minHeight: 36)
    }

    private var themedDivider: some View {
        Rectangle()
            .fill(palette.dividerColor)
            .frame(height: activeTheme.builtin == .highContrast ? 2 : 1)
    }

    private var colorThemePicker: some View {
        VStack(alignment: .leading, spacing: 10) {
            VStack(alignment: .leading, spacing: 2) {
                Text(Loc.t("settings.colorTheme"))
                    .font(activeTheme.readingFont(size: 13, weight: .medium))
                Text(Loc.t("settings.colorThemeDescription"))
                    .font(activeTheme.readingFont(size: 10))
                    .foregroundColor(palette.tertiaryColor)
            }

            if !loc.themeDescriptors.isEmpty {
                LazyVGrid(
                    columns: [GridItem(.adaptive(minimum: 190), spacing: 8)],
                    spacing: 8
                ) {
                    ForEach(loc.themeDescriptors) { descriptor in
                        themeV2Card(descriptor)
                    }
                }
            }

            if !loc.themeCatalogueLoaded {
                HStack(spacing: 6) {
                    if loc.themeCatalogueLoadFailed {
                        Label(Loc.t("error.localServiceUnavailable"), systemImage: "exclamationmark.triangle")
                            .foregroundColor(palette.warningColor)
                        Spacer()
                        Button(Loc.t("settings.retry")) {
                            Task { @MainActor in await loc.retryThemeCatalogueLoading() }
                        }
                        .buttonStyle(.bordered)
                        .controlSize(.small)
                    } else {
                        ProgressView().controlSize(.small)
                        Text(Loc.t("settings.loading"))
                            .foregroundColor(palette.tertiaryColor)
                    }
                }
                .font(activeTheme.readingFont(size: 10))
            } else if ThemeCatalogueDisplayPolicy.shouldShowFallback(
                catalogueLoaded: loc.themeCatalogueLoaded,
                activeThemeId: loc.colorTheme,
                validThemeIds: loc.themeDescriptors
                    .filter { $0.status == "valid" }
                    .map(\.id)
            ) {
                Label(Loc.t("settings.themePackageFallback"), systemImage: "exclamationmark.triangle")
                    .font(activeTheme.readingFont(size: 10))
                    .foregroundColor(palette.warningColor)
            }

            HStack(spacing: 8) {
                Button {
                    selectThemePackage(for: .install)
                } label: {
                    Label(Loc.t("settings.themePackageImport"), systemImage: "square.and.arrow.down")
                }
            }
            .buttonStyle(.bordered)
            .controlSize(.small)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
    }

    private func themeV2Card(_ descriptor: ApiClient.ThemeV2Descriptor) -> some View {
        let selected = loc.colorTheme == descriptor.id
        let name: String = switch descriptor.id {
        case "builtin:canvas@1": Loc.t("settings.colorTheme.tailsync")
        case "builtin:flux@1": Loc.t("settings.colorTheme.ocean")
        case "builtin:ledger@1": Loc.t("settings.colorTheme.forest")
        case "builtin:aura@1": Loc.t("settings.colorTheme.rose")
        case "builtin:mono@1": Loc.t("settings.colorTheme.high-contrast")
        default: descriptor.name[loc.lang] ?? descriptor.name["en"] ?? descriptor.id
        }
        let previewSelection = ThemeV2CardPreviewPolicy.selection(
            themeId: descriptor.id,
            catalogue: loc.resolvedV2Themes,
            reduceTransparency: loc.reduceTransparency,
            interfaceScale: min(activeTheme.interfaceScale, 1.25)
        )
        return ThemeV2CardView(
            descriptor: descriptor,
            name: name,
            selected: selected,
            selection: previewSelection,
            colorScheme: colorScheme,
            onSelect: { Task { @MainActor in await loc.selectLocalTheme(id: descriptor.id) } },
            onUpdate: { selectThemePackage(for: .update(themeId: descriptor.id, installedVersion: descriptor.version)) },
            onRollback: { rollbackThemeV2(descriptor) },
            onDelete: { deleteThemeV2(descriptor) }
        )
    }

    private func selectThemePackage(for operation: ThemePackageOperation) {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [UTType(filenameExtension: "tailsync-theme")!]
        panel.allowsMultipleSelection = false
        panel.canChooseDirectories = false
        panel.title = Loc.t(operation.isInstall ? "settings.themePackageImportTitle" : "settings.themePackageUpdateTitle")
        guard panel.runModal() == .OK, let url = panel.url else { return }
        Task { @MainActor in
            do {
                let light = try await ApiClient.shared.validateThemeV2(path: url.path, mode: "light")
                let dark = try await ApiClient.shared.validateThemeV2(path: url.path, mode: "dark")
                let highLight = try await ApiClient.shared.validateThemeV2(path: url.path, mode: "light", highContrast: true)
                let highDark = try await ApiClient.shared.validateThemeV2(path: url.path, mode: "dark", highContrast: true)
                let validations = [light, dark, highLight, highDark]
                let diagnostics = validations.flatMap(\.diagnostics)
                guard validations.allSatisfy(\.valid),
                      let digest = light.digest,
                      let candidateVersion = light.candidateVersion,
                      validations.allSatisfy({ $0.digest == digest }),
                      validations.allSatisfy({ $0.candidateVersion == candidateVersion }),
                      let lightTokens = light.previewTokens,
                      let darkTokens = dark.previewTokens,
                      let highLightTokens = highLight.previewTokens,
                      let highDarkTokens = highDark.previewTokens else {
                    throw ApiError.serverError(diagnostics.map(\.message).joined(separator: "\n"))
                }
                if case .update(let themeId, _) = operation,
                   validations.contains(where: { $0.previewId != themeId }) {
                    throw ApiError.serverError(Loc.t("settings.themePackageIdMismatch"))
                }
                var images: [String: NSImage] = [:]
                for slot in light.previewAssetSlots.keys where ["logo", "emptyState", "previewPlaceholder"].contains(slot) {
                    if let data = try? await ApiClient.shared.previewThemeAssetSlot(path: url.path, digest: digest, slot: slot),
                       let image = ThemeAssetImageDecoder.decode(data, slot: slot) {
                        images[slot] = image
                    }
                }
                pendingThemeImport = PendingThemeImport(
                    path: url.path,
                    digest: digest,
                    standard: TailSyncThemeDefinition.resolvedV2(id: "preview", light: lightTokens, dark: darkTokens),
                    highContrast: TailSyncThemeDefinition.resolvedV2(id: "preview-high", light: highLightTokens, dark: highDarkTokens),
                    diagnostics: diagnostics,
                    assetImages: images,
                    candidateVersion: candidateVersion,
                    versionRelation: {
                        if case .update(_, let installedVersion) = operation {
                            return ThemePackageSemanticVersion.relation(
                                candidate: candidateVersion,
                                installed: installedVersion
                            )
                        }
                        return nil
                    }(),
                    operation: operation
                )
            } catch {
                actionErrorMessage = error.localizedDescription
            }
        }
    }

    private func themeImportPreview(_ preview: PendingThemeImport) -> some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(Loc.t("settings.themePackagePreview")).font(.headline)
            LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], spacing: 10) {
                previewSwatch(Loc.t("settings.themePreviewLight"), theme: preview.standard, scheme: .light)
                previewSwatch(Loc.t("settings.themePreviewDark"), theme: preview.standard, scheme: .dark)
                previewSwatch(Loc.t("settings.themePreviewHighLight"), theme: preview.highContrast, scheme: .light)
                previewSwatch(Loc.t("settings.themePreviewHighDark"), theme: preview.highContrast, scheme: .dark)
            }
            if let logo = preview.assetImages["logo"] {
                Image(nsImage: logo).resizable().aspectRatio(contentMode: .fit).frame(width: 42, height: 42).frame(maxWidth: .infinity, alignment: .center)
            }
            HStack(spacing: 12) {
                if let empty = preview.assetImages["emptyState"] {
                    Image(nsImage: empty).resizable().aspectRatio(contentMode: .fit).frame(width: 46, height: 34)
                }
                if let placeholder = preview.assetImages["previewPlaceholder"] {
                    Image(nsImage: placeholder).resizable().aspectRatio(contentMode: .fit).frame(width: 58, height: 34)
                }
            }
            Text(themeVersionDescription(preview))
                .font(.caption.monospacedDigit())
                .foregroundColor(preview.versionRelation == .downgrade ? palette.warningColor : palette.secondaryColor)
            if !preview.diagnostics.isEmpty {
                VStack(alignment: .leading, spacing: 4) {
                    ForEach(preview.diagnostics.indices, id: \.self) { index in
                        Text(preview.diagnostics[index].message)
                            .font(.caption)
                            .foregroundColor(preview.diagnostics[index].severity == "error" ? .red : .orange)
                    }
                }
            }
            HStack {
                Spacer()
                Button(Loc.t("common.cancel")) { pendingThemeImport = nil }
                Button(Loc.t(preview.operation.isInstall ? "settings.themePackageInstall" : "settings.themePackageUpdate")) {
                    applyPreviewedTheme(preview)
                }
                .buttonStyle(.borderedProminent)
            }
        }
        .padding(20)
        .frame(width: 640)
    }

    private func previewSwatch(
        _ title: String,
        theme: TailSyncThemeDefinition,
        scheme: ColorScheme
    ) -> some View {
        let palette = scheme == .light ? theme.lightPalette : theme.darkPalette
        let components = scheme == .light ? theme.components : theme.darkComponents
        let search = components["search"]?["focus"]
        let hover = components["history"]?["hover"]
        let selected = components["history"]?["selected"]
        let buttonStates = ["default", "hover", "active", "selected", "disabled", "focus"]
        return VStack(alignment: .leading, spacing: 8) {
            Text(title).font(.caption.weight(.semibold)).foregroundColor(palette.primaryColor)
            VStack(alignment: .leading, spacing: 7) {
                Text(Loc.t("settings.themePreviewSearch"))
                    .font(.caption)
                    .foregroundColor(search?.foregroundColor ?? palette.primaryColor)
                    .padding(search?.padding ?? 7)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(search?.backgroundColor ?? palette.softSurfaceColor)
                    .clipShape(RoundedRectangle(cornerRadius: search?.radius ?? 6))
                    .overlay(RoundedRectangle(cornerRadius: search?.radius ?? 6).stroke(search?.focusRingColor ?? palette.accentColor, lineWidth: 2))
                Text(Loc.t("settings.themePreviewHover"))
                    .font(.caption)
                    .foregroundColor(hover?.foregroundColor ?? palette.primaryColor)
                    .padding(hover?.padding ?? 7)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(hover?.backgroundColor ?? palette.hoverColor)
                Text(Loc.t("settings.themePreviewSelected"))
                    .font(.caption.weight(.medium))
                    .foregroundColor(selected?.foregroundColor ?? palette.accentContrastColor)
                    .padding(selected?.padding ?? 7)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(selected?.backgroundColor ?? palette.accentColor)
            }
            .padding(8)
            .background(palette.surfaceColor)
            .clipShape(RoundedRectangle(cornerRadius: 7))
            LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible()), GridItem(.flexible())], spacing: 5) {
                ForEach(buttonStates, id: \.self) { state in
                    let token = components["button"]?[state]
                    VStack(alignment: .leading, spacing: 3) {
                        Text(previewStateLabel(state))
                            .font(.system(size: 9))
                            .foregroundColor(palette.tertiaryColor)
                        Text(previewStateLabel(state))
                            .font(.system(size: 10, weight: .medium))
                            .foregroundColor(token?.foregroundColor ?? palette.primaryColor)
                            .frame(maxWidth: .infinity)
                            .padding(.vertical, 5)
                            .padding(.horizontal, token?.padding ?? 5)
                            .background(token?.backgroundColor ?? palette.raisedColor)
                            .clipShape(RoundedRectangle(cornerRadius: token?.radius ?? 5, style: .continuous))
                            .overlay {
                                RoundedRectangle(cornerRadius: token?.radius ?? 5, style: .continuous)
                                    .stroke(token?.borderColor ?? palette.borderColor, lineWidth: state == "focus" ? 2 : 1)
                            }
                            .opacity(state == "disabled" ? 0.55 : 1)
                    }
                }
            }
        }.padding(10).frame(maxWidth: .infinity, alignment: .leading).background(palette.windowColor).clipShape(RoundedRectangle(cornerRadius: 8))
    }

    private func previewStateLabel(_ state: String) -> String {
        switch state {
        case "default": return Loc.t("settings.themePreviewStateDefault")
        case "hover": return Loc.t("settings.themePreviewStateHover")
        case "active": return Loc.t("settings.themePreviewStateActive")
        case "selected": return Loc.t("settings.themePreviewStateSelected")
        case "disabled": return Loc.t("settings.themePreviewStateDisabled")
        case "focus": return Loc.t("settings.themePreviewStateFocus")
        default: return state
        }
    }

    private func themeVersionDescription(_ preview: PendingThemeImport) -> String {
        if case .update(_, let installedVersion) = preview.operation {
            return "\(installedVersion) → \(preview.candidateVersion)"
        }
        return "\(Loc.t("settings.themePackageCandidateVersion")): \(preview.candidateVersion)"
    }

    private func applyPreviewedTheme(_ preview: PendingThemeImport) {
        if case .update(_, let installedVersion) = preview.operation,
           preview.versionRelation == .same || preview.versionRelation == .downgrade {
            let alert = NSAlert()
            alert.messageText = Loc.t(preview.versionRelation == .same
                ? "settings.themePackageReplaceTitle"
                : "settings.themePackageDowngradeTitle")
            alert.informativeText = "\(installedVersion) → \(preview.candidateVersion)"
            alert.addButton(withTitle: Loc.t("settings.themePackageUpdate"))
            alert.addButton(withTitle: Loc.t("common.cancel"))
            guard alert.runModal() == .alertFirstButtonReturn else { return }
        }
        let updateOptions = ThemePackageUpdateOptions.forRelation(preview.versionRelation)
        Task { @MainActor in
            do {
                switch preview.operation {
                case .install:
                    _ = try await ApiClient.shared.installThemeV2(path: preview.path, digest: preview.digest)
                case .update:
                    _ = try await ApiClient.shared.updateThemeV2(
                        path: preview.path,
                        digest: preview.digest,
                        allowSameVersion: updateOptions.allowSameVersion,
                        allowDowngrade: updateOptions.allowDowngrade
                    )
                }
                pendingThemeImport = nil
                await loc.refreshThemesV2()
                await loc.reloadActiveThemeAfterPackageChange()
                saved = true
                try? await Task.sleep(nanoseconds: 1_200_000_000)
                saved = false
            } catch { actionErrorMessage = error.localizedDescription }
        }
    }

    private func rollbackThemeV2(_ descriptor: ApiClient.ThemeV2Descriptor) {
        let alert = NSAlert()
        alert.messageText = Loc.t("settings.themePackageRollbackTitle")
        alert.informativeText = descriptor.name[loc.lang] ?? descriptor.name["en"] ?? descriptor.id
        alert.addButton(withTitle: Loc.t("settings.themePackageRollback"))
        alert.addButton(withTitle: Loc.t("common.cancel"))
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        Task { @MainActor in
            do {
                _ = try await ApiClient.shared.rollbackThemeV2(id: descriptor.id)
                await loc.refreshThemesV2()
                await loc.reloadActiveThemeAfterPackageChange()
            } catch {
                actionErrorMessage = error.localizedDescription
            }
        }
    }

    private func deleteThemeV2(_ descriptor: ApiClient.ThemeV2Descriptor) {
        let alert = NSAlert()
        alert.messageText = Loc.t("settings.themePackageDeleteTitle")
        alert.informativeText = descriptor.name[loc.lang] ?? descriptor.name["en"] ?? descriptor.id
        alert.addButton(withTitle: Loc.t("settings.themePackageDelete"))
        alert.addButton(withTitle: Loc.t("common.cancel"))
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        Task { @MainActor in
            do {
                try await ApiClient.shared.deleteThemeV2(id: descriptor.id, storageHandle: descriptor.storageHandle)
                await loc.refreshThemesV2()
                await loc.syncLocalThemeSettings()
            } catch {
                actionErrorMessage = error.localizedDescription
            }
        }
    }

}
