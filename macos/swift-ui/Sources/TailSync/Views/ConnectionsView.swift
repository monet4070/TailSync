import AppKit
import SwiftUI

struct ConnectionsView: View {
    struct PreviewData {
        var remotePairingExpanded = false
        var remoteInvite: ApiClient.RemotePairingInvite?
    }

    typealias PeerRoute = ConnectionsPeerSection.PeerRoute
    typealias PeerConnectionTestResult = ConnectionsPeerSection.PeerConnectionTestResult

    @ObservedObject var loc = Loc.shared
    @Environment(\.colorScheme) var colorScheme
    @Environment(\.dynamicTypeSize) var dynamicTypeSize

    @State var settings = AppSettings()
    @State var persistedSettings = AppSettings()
    @State var applyingPersistedSettings = false

    @State var localDevice: ApiClient.DeviceSnapshot?
    @State var peers: [ApiClient.PeerSnapshot] = []
    @State var isLoading = true
    @State var peersLoading = false
    @State var saved = false
    @State var loadErrorMessage: String?
    @State var actionErrorMessage: String?
    @State var peerError: String?

    @State var pairingStatus: ApiClient.PairingStatus?
    @State var pairingMessage: String?
    @State var pairingInProgress = false
    @State var showPairingSheet = false
    @State var previousPairingPhase: String?

    @State var remotePairing = RemotePairingInputState()
    @State var remoteInvite: ApiClient.RemotePairingInvite?
    @State var remotePairingInProgress = false
    @State var remoteInviteCopied = false

    @State var testingPeers: Set<String> = []
    @State var removingPeers: Set<String> = []
    @State var testResults: [String: [String: PeerConnectionTestResult]] = [:]
    @State var peerTestGenerations: [String: Int] = [:]
    @State var peerLoadGeneration = 0
    @State var peerRequestInFlight = false

    @State var saveGeneration = 0
    @State var saveCoordinator = SettingsSaveCoordinator()

    init(preview: PreviewData? = nil) {
        guard let preview else { return }
        var pairing = RemotePairingInputState()
        pairing.expanded = preview.remotePairingExpanded
        _remotePairing = State(initialValue: pairing)
        _remoteInvite = State(initialValue: preview.remoteInvite)
    }

    var activeTheme: TailSyncThemeSelection {
        TailSyncThemeSelection(
            storedValue: loc.colorTheme,
            catalogue: loc.resolvedV2Themes,
            reduceTransparency: loc.reduceTransparency,
            interfaceScale: TailSyncThemeAccessibilityPolicy.interfaceScale(for: dynamicTypeSize)
        )
    }

    @ViewBuilder
    var actionToast: some View {
        if saved || actionErrorMessage != nil {
            toast(message: actionErrorMessage ?? Loc.t("settings.saved"))
        }
    }

    func toast(message: String) -> some View {
        let tokens = component("toast")
        return Text(message)
            .font(.caption)
            .foregroundColor(tokens?.foregroundColor ?? palette.toastTextColor)
            .padding(.horizontal, tokens?.padding ?? 12)
            .padding(.vertical, 6)
            .background(tokens?.backgroundColor ?? palette.toastColor)
            .clipShape(RoundedRectangle(cornerRadius: tokens?.radius ?? 999, style: .continuous))
            .padding(.bottom, 8)
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
                    VStack(spacing: 16) {
                        connectionsCard
                    }
                    .padding(.vertical, 14)
                }
                .overlay(alignment: .bottom) {
                    actionToast
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
            pairingSection.pairingSheet
        }
        .onReceive(
            NotificationCenter.default.publisher(for: .tailSyncRemotePairingInviteReceived)
        ) { notification in
            guard let link = notification.object as? String else { return }
            handleRemotePairingLink(link)
        }
        .onReceive(
            NotificationCenter.default.publisher(for: GlobalShortcutController.syncStateChanged)
        ) { notification in
            if let enabled = notification.userInfo?["enabled"] as? Bool {
                settings.sync_enabled = enabled
                persistedSettings.sync_enabled = enabled
            }
        }
        .onReceive(
            NotificationCenter.default.publisher(for: .tailSyncSettingsChanged)
        ) { notification in
            if let updated = notification.object as? AppSettings {
                applyPersistedSettings(updated)
                persistedSettings = updated
            }
        }
        .onAppear {
            if let link = AppDelegate.takePendingRemotePairingLink() {
                handleRemotePairingLink(link)
            }
        }
        .tailSyncThemed()
    }

    var connectionsCard: some View {
        settingsCard(title: Loc.t("connections.title")) {
            // 1. Connection Mode Selection
            settingRow {
                VStack(alignment: .leading, spacing: 2) {
                    Text(Loc.t("settings.connectionMode"))
                        .foregroundColor(palette.primaryColor)
                    Text(Loc.t("settings.connectionModeDescription"))
                        .font(.caption2)
                        .foregroundColor(palette.tertiaryColor)
                        .fixedSize(horizontal: false, vertical: true)
                }
                Spacer(minLength: 12)
                Picker(Loc.t("settings.connectionMode"), selection: Binding(
                    get: { settings.connection_mode },
                    set: { mode in
                        settings.connection_mode = mode
                        changeConnectionMode()
                    }
                )) {
                    Text(Loc.t("settings.modeAuto")).tag("auto")
                    Text(Loc.t("settings.modeLan")).tag("lan_only")
                    Text(Loc.t("settings.modeIroh")).tag("iroh_only")
                    Text(Loc.t("settings.modeTailscale")).tag("tailscale_only")
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .accessibilityLabel(Loc.t("settings.connectionMode"))
                .frame(width: 280)
            }

            themedDivider.padding(.leading, 16)
            localIdentityRow

            themedDivider.padding(.leading, 16)
            peerSection

            themedDivider.padding(.leading, 16)
            pairingSection

            if ["auto", "iroh_only"].contains(settings.connection_mode) {
                themedDivider.padding(.leading, 16)
                remotePairingDrawer
            }
        }
    }

    var localIdentityRow: some View {
        settingRow {
            Image(systemName: "laptopcomputer")
                .foregroundColor(palette.accentColor)
                .frame(width: 24)
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(localDevice?.hostname ?? Loc.t("settings.localDevice"))
                        .font(.body.weight(.medium))
                        .foregroundColor(palette.primaryColor)
                    Text(Loc.t("settings.localDevice"))
                        .font(.caption2.weight(.medium))
                        .foregroundColor(palette.accentColor)
                        .padding(.horizontal, 5)
                        .padding(.vertical, 1)
                        .background(palette.accentSoftColor)
                        .clipShape(RoundedRectangle(cornerRadius: 3, style: .continuous))
                }
                Text(localDevice?.fingerprint ?? Loc.t("settings.identityLoading"))
                    .font(.caption2.monospaced())
                    .foregroundColor(palette.tertiaryColor)
                    .textSelection(.enabled)
                if let endpoint = localDevice?.iroh_endpoint_id {
                    Text(ConnectionsText.irohEndpoint(endpoint))
                        .font(.caption2.monospaced())
                        .foregroundColor(palette.tertiaryColor)
                        .textSelection(.enabled)
                }
            }
            Spacer()
        }
    }

    var pairingSection: ConnectionsPairingSection {
        ConnectionsPairingSection(
            activeTheme: activeTheme,
            pairingStatus: pairingStatus,
            pairingMessage: pairingMessage,
            pairingInProgress: pairingInProgress,
            onToggle: { togglePairingWindow() },
            onCancel: { cancelPairing() },
            onConfirm: { confirmPairing() }
        )
    }

    var peerSection: ConnectionsPeerSection {
        ConnectionsPeerSection(
            activeTheme: activeTheme,
            peers: peers,
            peersLoading: peersLoading,
            peerError: peerError,
            connectionMode: settings.connection_mode,
            pairingInProgress: pairingInProgress,
            testingPeers: testingPeers,
            removingPeers: removingPeers,
            testResults: testResults,
            onRefresh: { refreshPeers() },
            onTest: { hostname, routes in testPeer(hostname, routes: routes) },
            onForget: { forgetPeer($0) },
            onPair: { startPairing($0) },
            onToggle: { hostname, enabled in togglePeer(hostname, enabled: enabled) }
        )
    }

    func load() {
        isLoading = true
        loadErrorMessage = nil
        actionErrorMessage = nil
        Task { @MainActor in
            do {
                settings = try await ApiClient.shared.getSettings()
                persistedSettings = settings
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

    func loadPeers(clearExisting: Bool = false, showLoading: Bool = true) {
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

    func applyPeerResult(_ result: ApiClient.PeersResult, showLoading: Bool) {
        if result.requestSucceeded {
            localDevice = result.local
            peers = result.peers.filter { peer in
                peer.online || peer.trusted || peer.status == "discovered"
            }
        }
        peerError = result.error
        peerRequestInFlight = false
        if showLoading {
            peersLoading = false
        }
    }

    func refreshPeers() {
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

    func changeConnectionMode() {
        saveGeneration += 1
        let generation = saveGeneration
        peerLoadGeneration += 1
        peerRequestInFlight = false
        let requestedMode = settings.connection_mode
        peers = []
        testResults = [:]
        peerError = nil
        peersLoading = true

        Task { @MainActor in
            do {
                let outcome = await saveCoordinator.saveConnectionMode(requestedMode, fallback: persistedSettings)
                if let message = outcome.error {
                    guard generation == saveGeneration else { return }
                    settings.connection_mode = outcome.persisted.connection_mode
                    persistedSettings.connection_mode = outcome.persisted.connection_mode
                    throw ApiError.serverError(message)
                }
                guard generation == saveGeneration,
                      requestedMode == settings.connection_mode else { return }
                persistedSettings.connection_mode = outcome.persisted.connection_mode
                NotificationCenter.default.post(
                    name: .tailSyncConnectionModeChanged,
                    object: outcome.persisted.connection_mode
                )
                saved = true
                loadPeers()
                try? await Task.sleep(nanoseconds: 1_200_000_000)
                if generation == saveGeneration { saved = false }
            } catch {
                guard generation == saveGeneration else { return }
                peerRequestInFlight = false
                peersLoading = false
                actionErrorMessage = error.localizedDescription
            }
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

    func togglePairingWindow() {
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

    func startPairing(_ route: PeerRoute) {
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

    func confirmPairing() {
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

    func cancelPairing() {
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

    func pairingErrorDescription(_ error: Error) -> String {
        (error as? ApiError)?.pairingErrorDescription ?? error.localizedDescription
    }

    @MainActor
    func refreshPairingStatus() async {
        guard let status = try? await ApiClient.shared.getPairingStatus() else { return }
        pairingStatus = status
        if status.peer != nil && ["verification", "waiting_for_peer", "finalizing"].contains(status.phase) {
            showPairingSheet = true
        }
        if status.phase == "paired", previousPairingPhase != "paired" {
            showPairingSheet = false
            loadPeers()
        }
        previousPairingPhase = status.phase
    }

    func forgetPeer(_ hostname: String) {
        guard !removingPeers.contains(hostname) else { return }
        removingPeers.insert(hostname)
        peers.removeAll { $0.hostname == hostname }
        peerTestGenerations[hostname, default: 0] += 1
        testingPeers.remove(hostname)
        testResults.removeValue(forKey: hostname)

        Task { @MainActor in
            do {
                try await ApiClient.shared.forgetPeer(hostname: hostname)
                settings.enabled_peers.removeValue(forKey: hostname)
                persistedSettings.enabled_peers.removeValue(forKey: hostname)
                NotificationCenter.default.post(name: .tailSyncSettingsChanged, object: persistedSettings)
                loadPeers()
            } catch {
                peerError = error.localizedDescription
                loadPeers()
            }
            removingPeers.remove(hostname)
        }
    }

    func togglePeer(_ hostname: String, enabled: Bool) {
        Task { @MainActor in
            if await ApiClient.shared.togglePeer(hostname: hostname, enabled: enabled) {
                settings.enabled_peers[hostname] = enabled
                persistedSettings.enabled_peers[hostname] = enabled
                NotificationCenter.default.post(name: .tailSyncSettingsChanged, object: persistedSettings)
                loadPeers()
            }
        }
    }

    func testPeer(_ hostname: String, routes: [PeerRoute]) {
        guard !routes.isEmpty, !testingPeers.contains(hostname) else { return }
        let generation = peerTestGenerations[hostname, default: 0] + 1
        peerTestGenerations[hostname] = generation
        testingPeers.insert(hostname)
        testResults[hostname] = [:]
        Task { @MainActor in
            defer {
                if peerTestGenerations[hostname] == generation {
                    testingPeers.remove(hostname)
                }
            }
            for route in routes {
                let result = await ApiClient.shared.testConnection(address: route.address)
                    ?? (0, "", Loc.t("settings.connectionFailed"))
                guard peerTestGenerations[hostname] == generation else { return }
                var peerResults = testResults[hostname] ?? [:]
                peerResults[route.id] = PeerConnectionTestResult(
                    latencyMs: result.latencyMs,
                    path: result.path,
                    error: result.error
                )
                testResults[hostname] = peerResults
            }
        }
    }
}
