import SwiftUI
import AppKit
import UserNotifications
import UniformTypeIdentifiers

struct SettingsView: View {
    enum ThemePackageOperation {
        case install
        case update(themeId: String, installedVersion: String)

        var isInstall: Bool {
            if case .install = self { return true }
            return false
        }
    }

    struct PendingThemeImport: Identifiable {
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
    enum ShortcutKind: Equatable {
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
    }

    struct PeerConnectionTestResult {
        let latencyMs: Int
        let path: String
        let error: String
        let interface: String?
    }

    @ObservedObject var loc = Loc.shared
    @Environment(\.colorScheme) var colorScheme
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
    @State var testingPeers: Set<String> = []
    @State var removingPeers: Set<String> = []
    @State var testResults: [String: PeerConnectionTestResult] = [:]
    @State var peerLoadGeneration = 0
    @State var peerRequestInFlight = false
    @State var saveGeneration = 0
    @State var saveCoordinator = SettingsSaveCoordinator()
    @State var storageStatus: ApiClient.StorageStatus?
    @State var storageBusy = false
    @State var oldStorage: ApiClient.StorageMigrationResult?
    @State var recordingShortcut: ShortcutKind?
    @State var shortcutDraft = ""
    @State var shortcutError = ""
    @State var shortcutErrorKind: ShortcutKind?
    @State var shortcutBusy = false
    @State var shortcutMonitor: Any?
    @State var pendingThemeImport: PendingThemeImport?
    @State var appUpdatePhase = AppUpdatePhase.ready
    @State var appUpdate: ApiClient.UpdateInfo?
    @State var appUpdateErrorMessage: String?
    @State var showingAppUpdateAlert = false
    @Environment(\.dynamicTypeSize) var dynamicTypeSize

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
                        updatesSection
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
        .alert(Loc.t("update.title"), isPresented: $showingAppUpdateAlert) {
            Button(Loc.t("update.install")) {
                installAppUpdate()
            }
            Button(Loc.t("common.cancel"), role: .cancel) {}
        } message: {
            Text(appUpdateAlertMessage)
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


}
