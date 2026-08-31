import AppKit
import SwiftUI

extension SettingsView {
    var networkSection: some View {
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
            remotePairingSection

            themedDivider.padding(.leading, 16)
            peerList
        }
    }

    var localIdentityRow: some View {
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

    var pairingPanel: some View {
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
                    .buttonStyle(.plain)
                    .help(Loc.t("settings.refresh"))
            }
        } else {
            HStack {
                Text("\(peers.count) \(Loc.t("settings.devices"))")
                    .font(.caption2)
                    .foregroundColor(palette.tertiaryColor)
                Spacer()
                Button { refreshPeers() } label: { Image(systemName: "arrow.clockwise") }
                    .buttonStyle(.plain)
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
        case "tailscale_only": return "tailscale"
        default: return nil
        }
    }

    func routeIsAllowed(_ route: PeerRoute) -> Bool {
        switch settings.connection_mode {
        case "lan_only": return route.interface == "lan"
        case "tailscale_only": return route.interface == "tailscale"
        default: return true
        }
    }

    func routePriority(_ interface: String?) -> Int {
        switch interface {
        case "lan": return 0
        case "iroh": return 1
        case "tailscale": return 2
        default: return 3
        }
    }

    func preferredTestRoute(in routes: [PeerRoute]) -> PeerRoute? {
        routes
            .filter { route in
                routeIsAllowed(route)
                    && !route.address.isEmpty
                    && (route.interface != "iroh" || route.rttCapable)
            }
            .sorted { lhs, rhs in
                let lhsIsCurrent = lhs.peer.current_address == lhs.address
                let rhsIsCurrent = rhs.peer.current_address == rhs.address
                if lhsIsCurrent != rhsIsCurrent { return lhsIsCurrent }
                if lhs.connected != rhs.connected { return lhs.connected }
                if lhs.online != rhs.online { return lhs.online }
                let lhsConfirming = lhs.status == "confirming"
                let rhsConfirming = rhs.status == "confirming"
                if lhsConfirming != rhsConfirming { return lhsConfirming }
                let lhsPriority = routePriority(lhs.interface)
                let rhsPriority = routePriority(rhs.interface)
                if lhsPriority != rhsPriority { return lhsPriority < rhsPriority }
                return lhs.address < rhs.address
            }
            .first
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
        }
    }

    func peerRow(_ peer: ApiClient.PeerSnapshot) -> some View {
        let routes = peerRoutes(for: peer)
        let status = peerStatus(peer, routes: routes)
        let testRoute = preferredTestRoute(in: routes)
        let pairingRoute = pairingRoute(in: routes, for: peer)
        let needsIrohRediscovery = testRoute == nil
            && routes.contains { route in
                routeIsAllowed(route) && route.interface == "iroh" && !route.rttCapable
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
                if let result = testResults[peer.hostname] {
                    let label = result.error.isEmpty
                        ? "\(result.latencyMs) ms"
                            + (result.path == "relay" ? " · \(Loc.t("settings.relayPath"))" : "")
                        : result.error
                    let interfaceLabel = result.interface.map { "\(routeInterfaceLabel($0)) · " } ?? ""
                    Text(interfaceLabel + label)
                        .font(.caption2)
                        .foregroundColor(result.error.isEmpty ? .green : .red)
                        .lineLimit(1)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            Spacer()

            if testingPeers.contains(peer.hostname) {
                ProgressView().controlSize(.small)
            } else {
                Button {
                    if let testRoute {
                        testPeer(peer.hostname, route: testRoute)
                    }
                } label: {
                    Image(systemName: "bolt.horizontal")
                        .frame(width: 22, height: 22)
                }
                .buttonStyle(.plain)
                .disabled(testRoute == nil)
                .help(needsIrohRediscovery
                    ? Loc.t("settings.testRouteRediscover")
                    : Loc.t("settings.testPreferredConnection"))
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
                .buttonStyle(.plain)
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

}
