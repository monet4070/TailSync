import SwiftUI

/// Renders peer routes and actions from a snapshot supplied by ConnectionsView.
/// All mutations go back through explicit callbacks owned by the parent.
struct ConnectionsPeerSection: SettingsChrome {
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

    @Environment(\.colorScheme) var colorScheme
    let activeTheme: TailSyncThemeSelection
    let peers: [ApiClient.PeerSnapshot]
    let peersLoading: Bool
    let peerError: String?
    let connectionMode: String
    let pairingInProgress: Bool
    let testingPeers: Set<String>
    let removingPeers: Set<String>
    let testResults: [String: [String: PeerConnectionTestResult]]
    let onRefresh: () -> Void
    let onTest: (String, [PeerRoute]) -> Void
    let onForget: (String) -> Void
    let onPair: (PeerRoute) -> Void
    let onToggle: (String, Bool) -> Void

    var body: some View { peerList }

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
                Button { onRefresh() } label: { Image(systemName: "arrow.clockwise") }
                    .buttonStyle(.bordered)
                    .controlSize(.small)
                    .frame(minWidth: 54)
                    .help(Loc.t("settings.refresh"))
                    .accessibilityLabel(Loc.t("settings.refresh"))
            }
        } else {
            HStack {
                Text(ConnectionsText.deviceCount(peers.count))
                    .font(.caption2)
                    .foregroundColor(palette.tertiaryColor)
                Spacer()
                Button { onRefresh() } label: { Image(systemName: "arrow.clockwise") }
                    .buttonStyle(.bordered)
                    .controlSize(.small)
                    .frame(minWidth: 54)
                    .help(Loc.t("settings.refresh"))
                    .accessibilityLabel(Loc.t("settings.refresh"))
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
        if ["auto", "iroh_only"].contains(connectionMode),
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
            interface: routeInterface(for: connectionMode),
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
        switch connectionMode {
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
                Text(ConnectionsText.routeInterface(interface))
                    .font(.caption2.weight(.medium))
                    .foregroundColor(palette.accentColor)
            }
            Text(statusText(route.status))
                .font(.caption2.weight(route.connected ? .semibold : .regular))
                .foregroundColor(statusColor(route.status))
            if let result = testResult {
                let label = result.error.isEmpty
                    ? ConnectionsText.latency(result.latencyMs)
                        + (result.path == "relay" ? " · \(Loc.t("settings.relayPath"))" : "")
                    : result.error
                Text(label)
                    .font(.caption2.monospaced())
                    .foregroundColor(result.error.isEmpty ? palette.positiveColor : .red)
                    .lineLimit(1)
            } else if let latencyMs = route.latencyMs,
               ["online", "connected", "confirming"].contains(route.status) {
                Text(ConnectionsText.latency(latencyMs))
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
                    onTest(peer.hostname, testRoutes)
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
                .accessibilityLabel("\(Loc.t("settings.testAllConnections")) · \(peer.hostname)")
            }

            if peer.trusted, removingPeers.contains(peer.hostname) {
                ProgressView()
                    .controlSize(.small)
                    .frame(width: 48)
            } else if peer.trusted {
                Button { onForget(peer.hostname) } label: {
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
                        onPair(pairingRoute)
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
                .accessibilityLabel("\(Loc.t("settings.pair")) · \(peer.hostname)")
            }

            if peer.trusted {
                Toggle("", isOn: Binding(
                    get: { peer.enabled },
                    set: { onToggle(peer.hostname, $0) }
                ))
                .labelsHidden()
                .accessibilityLabel(
                    Loc.t("settings.deviceSync")
                        .replacingOccurrences(of: "{device}", with: peer.hostname)
                )
                .toggleStyle(.switch)
                .controlSize(.small)
            }
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
}
