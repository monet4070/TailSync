import SwiftUI
import AppKit
import UserNotifications

private func routeInterfaceLabel(_ interface: String) -> String {
    switch interface {
    case "lan": return "LAN"
    case "iroh": return "Iroh"
    default: return "Tailscale"
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

struct SettingsView: View {
    private struct PeerRoute: Identifiable {
        let peer: ApiClient.PeerSnapshot
        let address: String
        let interface: String?
        let online: Bool
        let connected: Bool
        let status: String
        let latencyMs: Int?
        let isPairingEndpoint: Bool

        var id: String { "\(peer.hostname)-\(interface ?? "unknown")-\(address)" }
    }

    @ObservedObject private var loc = Loc.shared
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.scenePhase) private var scenePhase
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
    @State private var testResults: [String: (latencyMs: Int, error: String)] = [:]
    @State private var peerLoadGeneration = 0
    @State private var peerRequestInFlight = false
    @State private var saveGeneration = 0
    @State private var saveCoordinator = SettingsSaveCoordinator()
    @State private var storageStatus: ApiClient.StorageStatus?
    @State private var storageBusy = false
    @State private var oldStorage: ApiClient.StorageMigrationResult?

    private var activeTheme: TailSyncColorTheme {
        TailSyncColorTheme(storedValue: loc.colorTheme)
    }

    private var palette: TailSyncThemePalette {
        activeTheme.palette(for: colorScheme)
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
                        Text(actionErrorMessage ?? Loc.t("settings.saved"))
                            .font(.caption)
                            .foregroundColor(palette.toastTextColor)
                            .padding(.horizontal, 12)
                            .padding(.vertical, 6)
                            .background(palette.toastColor)
                            .clipShape(Capsule())
                            .padding(.bottom, 8)
                    }
                }
            }
        }
        .task {
            load()
            var peerRefreshTicks = 0
            while !Task.isCancelled {
                if scenePhase == .active {
                    await refreshPairingStatus()
                    peerRefreshTicks += 1
                    if peerRefreshTicks >= 5 {
                        peerRefreshTicks = 0
                        if !isLoading && !peerRequestInFlight {
                            loadPeers(showLoading: false)
                        }
                    }
                }
                try? await Task.sleep(nanoseconds: 1_000_000_000)
            }
        }
        .sheet(isPresented: $showPairingSheet) {
            pairingSheet
        }
        .tailSyncThemed()
    }

    private var generalSection: some View {
        settingsCard(title: Loc.t("settings.general")) {
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
                            && ($0.pairingEndpoint || $0.address == pairedAddress)
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
                        isPairingEndpoint: peer.trusted && $0.address == pairedAddress
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
                isPairingEndpoint: peer.trusted && peer.address == pairedAddress
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
                        Text(result.error.isEmpty ? "\(result.latencyMs) ms" : result.error)
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
                .help(Loc.t("settings.testConnection"))
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
                Picker("", selection: $settings.theme) {
                    Text(Loc.t("settings.themeSystem")).tag("system")
                    Text(Loc.t("settings.themeLight")).tag("light")
                    Text(Loc.t("settings.themeDark")).tag("dark")
                }
                .pickerStyle(.menu)
                .frame(width: 130)
                .onChange(of: settings.theme) { theme in
                    loc.theme = theme
                    loc.applyTheme()
                    save()
                }
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
                loc.theme = settings.theme
                loc.colorTheme = TailSyncColorTheme(storedValue: settings.color_theme).rawValue
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
        localDevice = result.local
        peers = result.peers.filter { peer in
            peer.online || peer.trusted || peer.status == "discovered"
        }
        pairedPeerEndpoints = result.pairedEndpoints
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
        loc.theme = value.theme
        loc.colorTheme = TailSyncColorTheme(storedValue: value.color_theme).rawValue
        loc.notificationsEnabled = value.notifications_enabled
        loc.applyTheme()
        Task { @MainActor in
            await Task.yield()
            applyingPersistedSettings = false
        }
    }

    private func selectColorTheme(_ theme: TailSyncColorTheme) {
        guard settings.color_theme != theme.rawValue else { return }
        settings.color_theme = theme.rawValue
        loc.colorTheme = theme.rawValue
        save()
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
                ?? (0, Loc.t("settings.connectionFailed"))
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
                .background(palette.accentColor.opacity(0.10))
                .clipShape(RoundedRectangle(cornerRadius: activeTheme.metrics.controlRadius, style: .continuous))
        }
    }

    private func adjustHistoryLimit(by delta: Int) {
        settings.history_limit = min(500, max(10, settings.history_limit + delta))
        save()
    }

    private func settingsCard<Content: View>(title: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(title)
                .font(activeTheme.displayFont(
                    size: activeTheme.typography.sectionTitleSize,
                    weight: activeTheme == .tailsync ? .regular : .semibold
                ))
                .textCase(activeTheme.typography.uppercasesSectionTitles ? .uppercase : nil)
                .foregroundColor(palette.secondaryColor)
                .padding(.horizontal, 16)
                .padding(.bottom, 6)
            VStack(spacing: 0) { content() }
                .background(palette.surfaceColor)
                .clipShape(RoundedRectangle(cornerRadius: activeTheme.metrics.cardRadius, style: .continuous))
                .overlay {
                    RoundedRectangle(cornerRadius: activeTheme.metrics.cardRadius, style: .continuous)
                        .stroke(palette.borderColor, lineWidth: activeTheme == .highContrast ? 2 : 1)
                }
                .shadow(
                    color: palette.primaryColor.opacity(activeTheme.metrics.shadowRadius == 0 ? 0 : 0.08),
                    radius: activeTheme.metrics.shadowRadius,
                    y: activeTheme.metrics.shadowRadius > 0 ? 3 : 0
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
            .frame(height: activeTheme == .highContrast ? 2 : 1)
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

            Grid(horizontalSpacing: 8, verticalSpacing: 8) {
                GridRow {
                    colorThemeButton(.tailsync)
                    colorThemeButton(.ocean)
                }
                GridRow {
                    colorThemeButton(.forest)
                    colorThemeButton(.rose)
                }
                GridRow {
                    colorThemeButton(.highContrast)
                        .gridCellColumns(2)
                }
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
    }

    private func colorThemeButton(_ theme: TailSyncColorTheme) -> some View {
        let optionPalette = theme.palette(for: colorScheme)
        let selected = settings.color_theme == theme.rawValue
        let radius = theme.metrics.cardRadius

        return Button {
            selectColorTheme(theme)
        } label: {
            VStack(alignment: .leading, spacing: 0) {
                HStack(spacing: 0) {
                    Rectangle()
                        .fill(optionPalette.accentColor)
                        .frame(width: 6)
                    VStack(alignment: .leading, spacing: 5) {
                        Text("TailSync")
                            .font(theme.displayFont(size: 12, weight: .semibold))
                            .foregroundColor(optionPalette.primaryColor)
                        Capsule()
                            .fill(optionPalette.textTertiary == optionPalette.textPrimary
                                  ? optionPalette.primaryColor
                                  : optionPalette.tertiaryColor)
                            .frame(maxWidth: 68, minHeight: 3, maxHeight: 3)
                        Capsule()
                            .fill(optionPalette.dividerColor)
                            .frame(maxWidth: 96, minHeight: 3, maxHeight: 3)
                    }
                    .padding(9)
                    Spacer(minLength: 0)
                }
                .frame(maxWidth: .infinity, minHeight: 54, alignment: .leading)
                .background(optionPalette.windowColor)

                HStack(spacing: 6) {
                    Image(systemName: theme.symbolName)
                        .font(.system(size: 11, weight: .medium))
                    Text(Loc.t(theme.localizationKey))
                        .font(theme.readingFont(size: 11, weight: .medium))
                        .lineLimit(1)
                    Spacer(minLength: 4)
                    if selected {
                        Image(systemName: "checkmark.circle.fill")
                            .font(.system(size: 12, weight: .semibold))
                    }
                }
                .foregroundColor(selected ? palette.accentColor : palette.secondaryColor)
                .padding(.horizontal, 9)
                .frame(height: 30)
                .background(palette.raisedColor)
            }
            .clipShape(RoundedRectangle(cornerRadius: radius, style: .continuous))
            .overlay {
                RoundedRectangle(cornerRadius: radius, style: .continuous)
                    .stroke(selected ? palette.accentColor : palette.borderColor,
                            lineWidth: selected || activeTheme == .highContrast ? 2 : 1)
            }
        }
        .buttonStyle(.plain)
        .accessibilityLabel(Loc.t(theme.localizationKey))
        .accessibilityValue(selected ? Loc.t("settings.selected") : "")
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}
