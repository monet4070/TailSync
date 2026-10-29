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

        var descriptionKey: String {
            self == .sync ? "settings.syncShortcutDescription" : "settings.historyShortcutDescription"
        }

        var recordKey: String {
            "settings.shortcutRecord"
        }

        var defaultValue: String {
            value(in: AppSettings())
        }

        func value(in settings: AppSettings) -> String {
            self == .sync ? settings.sync_shortcut : settings.history_shortcut
        }
    }

    @ObservedObject var loc = Loc.shared
    @StateObject var launchAtLogin = LaunchAtLoginController()
    @Environment(\.colorScheme) var colorScheme
    @State var settings = AppSettings()
    @State var persistedSettings = AppSettings()
    @State var applyingPersistedSettings = false
    @State var isLoading = true
    @State var saved = false
    @State var loadErrorMessage: String?
    @State var actionErrorMessage: String?
    @State var saveGeneration = 0
    @State var saveCoordinator = SettingsSaveCoordinator()
    @State var storageStatus: ApiClient.StorageStatus?
    @State var storageBusy = false
    @StateObject var thirdPartyImageAccess = ThirdPartyImageAccessSettings()
    @ObservedObject private var tour = GuidedTourController.shared
    @State var oldStorage: ApiClient.StorageMigrationResult?
    // True when the local service cannot be reached, so the last known storage
    // reading must not keep presenting itself as a healthy current state.
    @State var storageStatusUnconfirmed = false
    // Storage operations get their own error slot so a failed migration message
    // can be cleared by a later success instead of leaking into the generic
    // settings toast for the rest of the session.
    @State var storageErrorMessage: String?
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

    init() {
        _launchAtLogin = StateObject(wrappedValue: LaunchAtLoginController())
    }

    init(launchAtLogin: LaunchAtLoginController) {
        _launchAtLogin = StateObject(wrappedValue: launchAtLogin)
    }

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
                    VStack(spacing: 16) {
                        if !tour.suppressed {
                            Button { AppDelegate.showOnboarding() } label: {
                            HStack {
                                VStack(alignment: .leading, spacing: 5) {
                                    Text(OnboardingGuide.shared.label("title", language: loc.lang)).font(.headline)
                                    Text(OnboardingGuide.shared.label("reopen", language: loc.lang))
                                        .font(.caption).foregroundColor(palette.secondaryColor)
                                }
                                Spacer()
                                Image(systemName: "arrow.up.right")
                            }.padding(16)
                                .background(palette.surfaceColor)
                                .clipShape(RoundedRectangle(cornerRadius: 10))
                                .overlay(RoundedRectangle(cornerRadius: 10).stroke(palette.borderColor))
                            }.buttonStyle(.plain).padding(.horizontal, 16)
                        }
                        generalSection.tourTarget("general")
                        updatesSection.tourTarget("updates")
                        historySection.tourTarget("history-settings")
                        storageSection.tourTarget("storage")
                        networkSection
                        appearanceSection.tourTarget("appearance")
                    }
                    .padding(.vertical, 14)
                }
                .tourScrollable()
                .overlay(alignment: .bottom) {
                    actionToast
                }
            }
        }
        .task {
            load()
        }
        .task {
            await observeStorageStatus()
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
                persistedSettings.sync_enabled = enabled
            }
        }
        .onReceive(
            NotificationCenter.default.publisher(for: .tailSyncConnectionModeChanged)
        ) { notification in
            if let mode = notification.object as? String {
                settings.connection_mode = mode
                persistedSettings.connection_mode = mode
            }
        }
        .modifier(SettingsSnapshotObserver(
            settings: $settings, persisted: $persistedSettings,
            apply: applyPersistedSettings
        ))
        .onReceive(
            NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)
        ) { _ in
            launchAtLogin.refresh()
        }
        .onAppear {
            launchAtLogin.refresh()
        }
        .onDisappear {
            if let recordingShortcut {
                cancelShortcutRecording(recordingShortcut)
            }
        }
        .guidedTour(window: "settings")
        .tailSyncThemed()
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


}
