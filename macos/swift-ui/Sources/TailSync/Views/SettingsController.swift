import SwiftUI
import AppKit
extension SettingsView {
    func load() {
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

    func save() {
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

    func applyPersistedSettings(_ value: AppSettings) {
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
        testingPeers.remove(hostname)
        testResults.removeValue(forKey: hostname)

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

    func togglePeer(_ hostname: String, enabled: Bool) {
        Task { @MainActor in
            if await ApiClient.shared.togglePeer(hostname: hostname, enabled: enabled) {
                settings.enabled_peers[hostname] = enabled
                persistedSettings.enabled_peers[hostname] = enabled
                loadPeers()
            }
        }
    }

    func testPeer(_ hostname: String, route: PeerRoute) {
        guard !route.address.isEmpty else { return }
        testingPeers.insert(hostname)
        Task { @MainActor in
            let result = await ApiClient.shared.testConnection(address: route.address)
                ?? (0, "", Loc.t("settings.connectionFailed"))
            testResults[hostname] = PeerConnectionTestResult(
                latencyMs: result.latencyMs,
                path: result.path,
                error: result.error,
                interface: route.interface
            )
            testingPeers.remove(hostname)
        }
    }

}
