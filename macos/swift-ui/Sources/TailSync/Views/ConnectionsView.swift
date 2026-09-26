import AppKit
import SwiftUI

struct ConnectionsView: View {
    struct PeerRoute: Identifiable {
        let peer: ApiClient.PeerSnapshot
        let address: String
        let interface: String?
        let online: Bool
        let connected: Bool
        let status: String
        let latencyMs: Int?
        let rttCapable: Bool

        var id: String { "\(peer.hostname)-\(interface ?? "unknown")-\(address)" }

        var latencyTestTarget: PeerLatencyTestTarget {
            PeerLatencyTestTarget(
                id: id,
                address: address,
                interface: interface,
                rttCapable: rttCapable
            )
        }
    }

    struct PeerConnectionTestResult {
        let latencyMs: Int
        let path: String
        let error: String
    }

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

    var activeTheme: TailSyncThemeSelection {
        TailSyncThemeSelection(
            storedValue: loc.colorTheme,
            catalogue: loc.resolvedV2Themes,
            reduceTransparency: loc.reduceTransparency,
            interfaceScale: TailSyncThemeAccessibilityPolicy.interfaceScale(for: dynamicTypeSize)
        )
    }

    var palette: TailSyncThemePalette {
        activeTheme.palette(for: colorScheme)
    }

    func component(_ name: String, state: String = "default") -> TailSyncThemeComponentTokens? {
        activeTheme.component(name, state: state, scheme: colorScheme)
    }

    func settingsCard<Content: View>(title: String, @ViewBuilder content: () -> Content) -> some View {
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

    func settingRow<Content: View>(@ViewBuilder content: () -> Content) -> some View {
        HStack(spacing: 8) { content() }
            .font(activeTheme.readingFont(size: 13))
            .padding(.horizontal, 16)
            .padding(.vertical, activeTheme.metrics.rowPadding)
            .frame(minHeight: 36)
    }

    var themedDivider: some View {
        Rectangle()
            .fill(palette.dividerColor)
            .frame(height: activeTheme.builtin == .highContrast ? 2 : 1)
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
            pairingSheet
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
                Picker("", selection: Binding(
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
                .frame(width: 280)
            }

            themedDivider.padding(.leading, 16)
            localIdentityRow

            themedDivider.padding(.leading, 16)
            peerList

            themedDivider.padding(.leading, 16)
            pairingPanel

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
                    Text("iroh: \(endpoint)")
                        .font(.caption2.monospaced())
                        .foregroundColor(palette.tertiaryColor)
                        .textSelection(.enabled)
                }
            }
            Spacer()
        }
    }

    var pairingPanel: some View {
        settingRow {
            Image(systemName: "link.badge.plus")
                .foregroundColor(palette.accentColor)
                .frame(width: 24)
            VStack(alignment: .leading, spacing: 2) {
                Text(Loc.t("settings.pairDevice"))
                    .font(.body.weight(.medium))
                    .foregroundColor(palette.primaryColor)
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
            .controlSize(.small)
            .disabled(pairingInProgress)
            if let pairingMessage {
                Text(pairingMessage)
                    .font(.caption2)
                    .foregroundColor(.red)
                    .textSelection(.enabled)
            }
        }
    }

    var pairingWindowSummary: String {
        guard let status = pairingStatus, status.pairing_enabled else {
            return Loc.t("settings.pairingClosed")
        }
        return "\(Loc.t("settings.waitingPairing")) · \(status.remaining_seconds)s · \(status.failed_attempts)/\(status.max_failures)"
    }

    var pairingSheet: some View {
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
                if pairingStatus?.phase == "finalizing" {
                    ProgressView(Loc.t("settings.pairingFinalizing"))
                        .controlSize(.small)
                } else if peer.local_confirmed {
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

            if let message = pairingMessage
                ?? pairingStatus?.error.map(ApiError.pairingErrorDescription)
            {
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
                        || pairingStatus?.phase == "finalizing"
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
    var peerList: some View {
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
                    .buttonStyle(.bordered)
                    .controlSize(.small)
                    .frame(minWidth: 54)
                    .help(Loc.t("settings.refresh"))
            }
        } else {
            HStack {
                Text("\(peers.count) \(Loc.t("settings.devices"))")
                    .font(.caption2)
                    .foregroundColor(palette.tertiaryColor)
                Spacer()
                Button { refreshPeers() } label: { Image(systemName: "arrow.clockwise") }
                    .buttonStyle(.bordered)
                    .controlSize(.small)
                    .frame(minWidth: 54)
                    .help(Loc.t("settings.refresh"))
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 6)

            ForEach(Array(peers.enumerated()), id: \.element.id) { index, peer in
                if index > 0 { themedDivider.padding(.leading, 48) }
                peerRow(peer)
            }
        }
    }

    func peerRoutes(for peer: ApiClient.PeerSnapshot) -> [PeerRoute] {
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
                    rttCapable: $0.rttCapable
                )
            }
        }
        if ["auto", "iroh_only"].contains(settings.connection_mode),
           !peer.candidates.isEmpty {
            return peer.candidates.map {
                PeerRoute(
                    peer: peer,
                    address: $0.address,
                    interface: $0.interface,
                    online: $0.online,
                    connected: peer.current_address == $0.address,
                    status: peer.current_address == $0.address ? "connected" : $0.status,
                    latencyMs: $0.latency,
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
            rttCapable: true
        )]
    }

    func routeInterface(for mode: String) -> String? {
        switch mode {
        case "lan_only": return "lan"
        case "iroh_only": return "iroh"
        case "tailscale_only": return "tailscale"
        default: return nil
        }
    }

    func routeIsAllowed(_ route: PeerRoute) -> Bool {
        switch settings.connection_mode {
        case "lan_only": return route.interface == "lan"
        case "iroh_only": return route.interface == "iroh"
        case "tailscale_only": return route.interface == "tailscale"
        default: return true
        }
    }

    func latencyTestRoutes(in routes: [PeerRoute]) -> [PeerRoute] {
        let routesByID = Dictionary(
            routes.map { ($0.id, $0) },
            uniquingKeysWith: { first, _ in first }
        )
        return PeerLatencyTestPlan
            .orderedTargets(routes.map(\.latencyTestTarget))
            .compactMap { routesByID[$0.id] }
    }

    func pairingRoute(in routes: [PeerRoute], for peer: ApiClient.PeerSnapshot) -> PeerRoute? {
        let availableRoutes = routes.filter { routeIsAllowed($0) && !$0.address.isEmpty }
        let tcpRoutes = availableRoutes.filter { $0.interface != "iroh" }
        if let route = tcpRoutes.first(where: \.connected) { return route }
        if let route = tcpRoutes.first(where: \.online) { return route }
        if let route = tcpRoutes.first(where: { $0.status == "confirming" }) { return route }
        if let route = availableRoutes.first(where: { $0.interface == "iroh" }) { return route }
        if let route = tcpRoutes.first(where: { $0.address == peer.current_address }) { return route }
        if let route = tcpRoutes.first(where: { $0.address == peer.address }) { return route }
        return tcpRoutes.first ?? availableRoutes.first
    }

    func peerStatus(_ peer: ApiClient.PeerSnapshot, routes: [PeerRoute]) -> String {
        let allowedRoutes = routes.filter(routeIsAllowed)
        if peer.current_address != nil || allowedRoutes.contains(where: \.connected) {
            return "connected"
        }
        for status in ["online", "confirming", "discovered"]
            where allowedRoutes.contains(where: { $0.status == status }) {
            return status
        }
        return peer.status
    }

    func peerRouteLine(_ route: PeerRoute) -> some View {
        let testResult = testResults[route.peer.hostname]?[route.id]
        return HStack(spacing: 7) {
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
            if let result = testResult {
                let label = result.error.isEmpty
                    ? "\(result.latencyMs) ms"
                        + (result.path == "relay" ? " · \(Loc.t("settings.relayPath"))" : "")
                    : result.error
                Text(label)
                    .font(.caption2.monospaced())
                    .foregroundColor(result.error.isEmpty ? palette.positiveColor : .red)
                    .lineLimit(1)
            } else if let latencyMs = route.latencyMs,
               ["online", "connected", "confirming"].contains(route.status) {
                Text("\(latencyMs) ms")
                    .font(.caption2.monospaced())
                    .foregroundColor(palette.tertiaryColor)
            }
        }
    }

    func peerRow(_ peer: ApiClient.PeerSnapshot) -> some View {
        let routes = peerRoutes(for: peer)
        let status = peerStatus(peer, routes: routes)
        let testRoutes = latencyTestRoutes(in: routes)
        let pairingRoute = pairingRoute(in: routes, for: peer)
        let needsIrohRediscovery = testRoutes.isEmpty
            && routes.contains { route in
                route.interface == "iroh" && !route.rttCapable
            }
        return settingRow {
            Circle()
                .fill(statusColor(status))
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
                ForEach(routes) { route in
                    peerRouteLine(route)
                }
                if peer.trusted, !peer.fingerprint.isEmpty {
                    Text(peer.fingerprint)
                        .font(.caption2.monospaced())
                        .foregroundColor(palette.tertiaryColor)
                        .lineLimit(1)
                        .truncationMode(.middle)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            Spacer()

            if testingPeers.contains(peer.hostname) {
                ProgressView().controlSize(.small)
            } else {
                Button {
                    testPeer(peer.hostname, routes: testRoutes)
                } label: {
                    Image(systemName: "bolt.horizontal")
                        .frame(width: 22, height: 22)
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
                .frame(minWidth: 54)
                .disabled(testRoutes.isEmpty)
                .help(needsIrohRediscovery
                    ? Loc.t("settings.testRouteRediscover")
                    : Loc.t("settings.testAllConnections"))
            }

            if peer.trusted, removingPeers.contains(peer.hostname) {
                ProgressView()
                    .controlSize(.small)
                    .frame(width: 48)
            } else if peer.trusted {
                Button { forgetPeer(peer.hostname) } label: {
                    Text(Loc.t("settings.removeDevice"))
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
                .frame(minWidth: 54)
                .tint(.red)
                .help(Loc.t("settings.unpair"))
            } else if !peer.trusted {
                Button {
                    if let pairingRoute {
                        startPairing(pairingRoute)
                    }
                } label: {
                    Image(systemName: "link.badge.plus")
                        .frame(width: 22, height: 22)
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
                .frame(minWidth: 54)
                .disabled(pairingRoute == nil || pairingInProgress)
                .help(Loc.t("settings.pair"))
            }

            if peer.trusted {
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

    func statusText(_ status: String) -> String {
        switch status {
        case "connected": return Loc.t("settings.connected")
        case "online": return Loc.t("settings.online")
        case "confirming": return Loc.t("settings.confirming")
        case "discovered": return Loc.t("settings.discovered")
        default: return Loc.t("settings.offline")
        }
    }

    func statusColor(_ status: String) -> Color {
        switch status {
        case "connected", "online": return palette.positiveColor
        case "confirming", "discovered": return palette.warningColor
        default: return palette.tertiaryColor
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
