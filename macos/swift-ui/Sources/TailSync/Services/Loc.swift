import Foundation
import AppKit
import ImageIO
import SwiftUI

extension Notification.Name {
    static let tailSyncLocaleChanged = Notification.Name("TailSyncLocaleChanged")
    static let tailSyncSettingsChanged = Notification.Name("TailSyncSettingsChanged")
    static let tailSyncThemeAssetsChanged = Notification.Name("TailSyncThemeAssetsChanged")
}

struct ThemeResolutionCacheIdentity: Equatable {
    let themeId: String
    let packageDigest: String
    let highContrast: Bool

    static func canReuse(
        _ cached: Self?,
        themeId: String,
        packageDigest: String,
        highContrast: Bool
    ) -> Bool {
        cached == Self(
            themeId: themeId,
            packageDigest: packageDigest,
            highContrast: highContrast
        )
    }
}

enum ThemeAssetImageDecoder {
    static func pixelLimit(for slot: String) -> Int {
        switch slot {
        case "logo": return 96
        case "emptyState": return 160
        case "previewPlaceholder": return 192
        default: return 192
        }
    }

    static func decode(_ data: Data, slot: String) -> NSImage? {
        guard let source = CGImageSourceCreateWithData(data as CFData, nil) else { return nil }
        let options: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceShouldCacheImmediately: true,
            kCGImageSourceThumbnailMaxPixelSize: pixelLimit(for: slot),
        ]
        guard let image = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary)
        else { return nil }
        return NSImage(
            cgImage: image,
            size: NSSize(width: image.width, height: image.height)
        )
    }
}

struct ThemeResolutionRequest: Equatable {
    let identity: ThemeResolutionCacheIdentity
    let generation: UInt64
}

struct ThemeAssetLoadRequest: Equatable {
    let themeId: String
    let packageDigest: String
    let generation: UInt64
}

enum ThemeLoadCommitPolicy {
    static func canCommitCanvasFallback(
        selectionGeneration: UInt64,
        latestSelectionGeneration: UInt64,
        colorTheme: String,
        intendedThemeId: String,
        canvasThemeId: String = "builtin:canvas@1"
    ) -> Bool {
        selectionGeneration == latestSelectionGeneration
            && colorTheme == canvasThemeId
            && intendedThemeId == canvasThemeId
    }

    static func canCommitThemeRefresh(
        requestGeneration: UInt64,
        latestGeneration: UInt64
    ) -> Bool {
        requestGeneration == latestGeneration
    }

    static func canCommitResolution(
        _ request: ThemeResolutionRequest,
        latestGeneration: UInt64?,
        currentPackageDigest: String?,
        effectiveHighContrast: Bool
    ) -> Bool {
        latestGeneration == request.generation
            && currentPackageDigest == request.identity.packageDigest
            && effectiveHighContrast == request.identity.highContrast
    }

    static func shouldFallbackAfterResolutionFailure(
        _ request: ThemeResolutionRequest,
        latestGeneration: UInt64?,
        activeThemeId: String
    ) -> Bool {
        latestGeneration == request.generation
            && activeThemeId == request.identity.themeId
    }

    static func canCommitAssets(
        _ request: ThemeAssetLoadRequest,
        latestGeneration: UInt64,
        activeThemeId: String,
        currentDefinitionDigest: String?,
        currentDescriptorDigest: String?
    ) -> Bool {
        latestGeneration == request.generation
            && activeThemeId == request.themeId
            && currentDefinitionDigest == request.packageDigest
            && currentDescriptorDigest == request.packageDigest
    }
}

enum ThemeStartupRetryPolicy {
    static let maximumAttempts = 20
    static let delayNanoseconds: UInt64 = 250_000_000

    static func run(
        maximumAttempts: Int = maximumAttempts,
        delayNanoseconds: UInt64 = delayNanoseconds,
        operation: () async -> Bool
    ) async -> Bool {
        let attempts = max(1, maximumAttempts)
        for attempt in 1...attempts {
            guard !Task.isCancelled else { return false }
            if await operation() { return true }
            guard attempt < attempts else { break }
            do {
                try await Task.sleep(nanoseconds: delayNanoseconds)
            } catch {
                return false
            }
        }
        return false
    }
}

enum ThemeCatalogueDisplayPolicy {
    static func shouldShowFallback(
        catalogueLoaded: Bool,
        activeThemeId: String,
        validThemeIds: [String]
    ) -> Bool {
        catalogueLoaded && !validThemeIds.contains(activeThemeId)
    }
}

private actor ThemeSettingsWriteCoordinator {
    private var tail: Task<Bool, Never>?

    func write(_ settings: ApiClient.LocalThemeSettings) async -> Bool {
        let predecessor = tail
        let job = Task<Bool, Never> {
            _ = await predecessor?.value
            do {
                try await ApiClient.shared.setLocalThemeSettings(settings)
                return true
            } catch {
                return false
            }
        }
        tail = job
        return await job.value
    }
}

/// Observable localization service.  Reads/watches the language and theme
/// settings saved by the Rust backend.
final class Loc: ObservableObject {
    static let shared = Loc()

    @Published var lang: String = "en" {
        didSet {
            guard lang != oldValue else { return }
            NotificationCenter.default.post(name: .tailSyncLocaleChanged, object: nil)
        }
    }
    @Published var theme: String = "system"
    @Published var colorTheme: String = TailSyncColorTheme.tailsync.rawValue
    @Published var notificationsEnabled: Bool = true
    /// V2 descriptors, including built-in, custom, and invalid packages.
    @Published var themeDescriptors: [ApiClient.ThemeV2Descriptor] = []
    @Published var resolvedV2Themes: [TailSyncThemeDefinition] = []
    @Published var themeAssetImages: [String: NSImage] = [:]
    @Published var localThemeSettings = ApiClient.LocalThemeSettings(activeThemeId: "builtin:canvas@1", appearance: "system", highContrast: false)
    @Published private(set) var themeCatalogueLoaded = false
    @Published private(set) var themeCatalogueLoadFailed = false
    @Published private(set) var reduceMotion = NSWorkspace.shared.accessibilityDisplayShouldReduceMotion
    @Published private(set) var reduceTransparency = NSWorkspace.shared.accessibilityDisplayShouldReduceTransparency
    private var resolvedV2ThemeCacheIdentities: [String: ThemeResolutionCacheIdentity] = [:]
    private var nextThemeResolutionGeneration: UInt64 = 0
    private var latestThemeResolutionGenerations: [String: UInt64] = [:]
    private var latestThemeAssetGeneration: UInt64 = 0
    private var nextThemeRefreshGeneration: UInt64 = 0
    private var latestThemeRefreshGeneration: UInt64 = 0
    private var nextThemeSelectionGeneration: UInt64 = 0
    private var latestThemeSelectionGeneration: UInt64 = 0
    private var intendedThemeId = "builtin:canvas@1"
    private let themeSettingsWriter = ThemeSettingsWriteCoordinator()

    private static let configURL: URL = {
        let dir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
            .appendingPathComponent("com.tailsync.TailSync")
        return dir.appendingPathComponent("config-v2.json")
    }()

    private init() {
        reload()
        NotificationCenter.default.addObserver(
            forName: NSWorkspace.accessibilityDisplayOptionsDidChangeNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            Task { @MainActor in
                self?.refreshAccessibilityPreferences()
                await self?.reloadActiveResolvedTheme()
            }
        }
        Task { @MainActor [weak self] in
            await self?.loadThemeStateWithRetry()
        }
    }

    func reload() {
        if let data = try? Data(contentsOf: Self.configURL),
           let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
            lang = obj["language"] as? String ?? fallbackLang()
            // Theme selection is V2 local state and is loaded from core.
            theme = "system"
            colorTheme = "builtin:canvas@1"
            notificationsEnabled = obj["notifications_enabled"] as? Bool ?? true
        } else {
            lang = fallbackLang()
            theme = "system"
            colorTheme = "builtin:canvas@1"
            notificationsEnabled = true
        }
        applyTheme()
    }

    @MainActor
    func retryThemeCatalogueLoading() async {
        themeCatalogueLoadFailed = false
        await loadThemeStateWithRetry()
    }

    @MainActor
    private func loadThemeStateWithRetry() async {
        let loaded = await ThemeStartupRetryPolicy.run {
            guard await self.refreshThemesV2() else { return false }
            do {
                let local = try await ApiClient.shared.getLocalThemeSettings()
                _ = self.beginThemeSelectionIntent(local.activeThemeId)
                self.localThemeSettings = local
                self.colorTheme = local.activeThemeId
                self.theme = local.appearance
                self.applyTheme()
                await self.loadResolvedV2Theme(local.activeThemeId)
                await self.loadThemeAssets(local.activeThemeId)
                return true
            } catch {
                return false
            }
        }
        themeCatalogueLoadFailed = !loaded
    }

    /// Refresh the V2 descriptor list and resolve all selectable entries.
    @discardableResult
    @MainActor
    func refreshThemesV2() async -> Bool {
        nextThemeRefreshGeneration &+= 1
        let refreshGeneration = nextThemeRefreshGeneration
        latestThemeRefreshGeneration = refreshGeneration
        guard let descriptors = try? await ApiClient.shared.listThemesV2() else { return false }
        guard ThemeLoadCommitPolicy.canCommitThemeRefresh(
            requestGeneration: refreshGeneration,
            latestGeneration: latestThemeRefreshGeneration
        ) else { return themeCatalogueLoaded }
        let validIds = Set(descriptors.filter { $0.status == "valid" }.map(\.id))
        themeDescriptors = descriptors
        themeCatalogueLoaded = true
        themeCatalogueLoadFailed = false
        resolvedV2Themes.removeAll { !validIds.contains($0.id) }
        resolvedV2ThemeCacheIdentities = resolvedV2ThemeCacheIdentities.filter {
            validIds.contains($0.key)
        }
        latestThemeResolutionGenerations = latestThemeResolutionGenerations.filter {
            validIds.contains($0.key)
        }
        for descriptor in descriptors where descriptor.status == "valid" {
            guard ThemeLoadCommitPolicy.canCommitThemeRefresh(
                requestGeneration: refreshGeneration,
                latestGeneration: latestThemeRefreshGeneration
            ) else { return themeCatalogueLoaded }
            let hasStaleDefinition = resolvedV2Themes.contains {
                $0.id == descriptor.id && $0.packageDigest != descriptor.digest
            }
            if hasStaleDefinition {
                resolvedV2Themes.removeAll { $0.id == descriptor.id }
                resolvedV2ThemeCacheIdentities.removeValue(forKey: descriptor.id)
                if descriptor.id == colorTheme {
                    invalidateThemeAssetLoads()
                }
            }
            await loadResolvedV2Theme(descriptor.id)
        }
        return true
    }

    @MainActor
    func loadResolvedV2Theme(_ id: String) async {
        let requestedHighContrast = effectiveHighContrast
        guard let descriptor = themeDescriptors.first(where: { $0.id == id }) else { return }
        guard descriptor.status == "valid" else {
            if id == colorTheme,
               id == intendedThemeId,
               id != "builtin:canvas@1" {
                await persistCanvasFallback()
            }
            return
        }
        if resolvedV2Themes.contains(where: { $0.id == id && $0.packageDigest == descriptor.digest }),
           ThemeResolutionCacheIdentity.canReuse(
               resolvedV2ThemeCacheIdentities[id],
               themeId: id,
               packageDigest: descriptor.digest,
               highContrast: requestedHighContrast
           ) { return }
        let request = beginThemeResolution(
            themeId: id,
            packageDigest: descriptor.digest,
            highContrast: requestedHighContrast
        )
        guard let definition = await ApiClient.shared.resolveThemeV2(themeId: id, highContrast: requestedHighContrast) else {
            if id != "builtin:canvas@1",
               ThemeLoadCommitPolicy.shouldFallbackAfterResolutionFailure(
                   request,
                   latestGeneration: latestThemeResolutionGenerations[id],
                   activeThemeId: intendedThemeId
               ) {
                await persistCanvasFallback()
            }
            return
        }
        let currentDigest = themeDescriptors.first {
            $0.id == id && $0.status == "valid"
        }?.digest
        guard definition.id == id,
              definition.packageDigest == request.identity.packageDigest,
              ThemeLoadCommitPolicy.canCommitResolution(
                  request,
                  latestGeneration: latestThemeResolutionGenerations[id],
                  currentPackageDigest: currentDigest,
                  effectiveHighContrast: effectiveHighContrast
              ) else { return }
        resolvedV2Themes.removeAll { $0.id == id }
        resolvedV2Themes.append(definition)
        resolvedV2ThemeCacheIdentities[id] = request.identity
    }

    @MainActor
    func syncLocalThemeSettings() async {
        let selectionGeneration = latestThemeSelectionGeneration
        guard let settings = try? await ApiClient.shared.getLocalThemeSettings() else { return }
        guard latestThemeSelectionGeneration == selectionGeneration else { return }
        _ = beginThemeSelectionIntent(settings.activeThemeId)
        localThemeSettings = settings
        colorTheme = settings.activeThemeId
        theme = settings.appearance
        applyTheme()
        await loadResolvedV2Theme(settings.activeThemeId)
        await loadThemeAssets(settings.activeThemeId)
    }

    @MainActor
    private var effectiveHighContrast: Bool {
        localThemeSettings.highContrast || NSWorkspace.shared.accessibilityDisplayShouldIncreaseContrast
    }

    @MainActor
    private func refreshAccessibilityPreferences() {
        reduceMotion = NSWorkspace.shared.accessibilityDisplayShouldReduceMotion
        reduceTransparency = NSWorkspace.shared.accessibilityDisplayShouldReduceTransparency
    }

    @MainActor
    private func reloadActiveResolvedTheme() async {
        let activeId = colorTheme
        resolvedV2Themes.removeAll { $0.id == activeId }
        resolvedV2ThemeCacheIdentities.removeValue(forKey: activeId)
        invalidateThemeAssetLoads()
        await loadResolvedV2Theme(activeId)
        await loadThemeAssets(activeId)
    }

    @MainActor
    func reloadActiveThemeAfterPackageChange() async {
        await reloadActiveResolvedTheme()
    }

    @MainActor
    func selectLocalTheme(id: String, appearance: String? = nil) async {
        let next = ApiClient.LocalThemeSettings(activeThemeId: id, appearance: appearance ?? localThemeSettings.appearance, highContrast: localThemeSettings.highContrast)
        let selectionGeneration = beginThemeSelectionIntent(id)
        let persisted = await themeSettingsWriter.write(next)
        guard latestThemeSelectionGeneration == selectionGeneration else { return }
        guard persisted else {
            await persistCanvasFallback()
            return
        }
        localThemeSettings = next; colorTheme = id; theme = next.appearance; applyTheme()
        await loadResolvedV2Theme(id)
        await loadThemeAssets(id)
    }

    @MainActor
    private func persistCanvasFallback() async {
        let canvas = ApiClient.LocalThemeSettings(activeThemeId: "builtin:canvas@1", appearance: localThemeSettings.appearance, highContrast: localThemeSettings.highContrast)
        let selectionGeneration = beginThemeSelectionIntent(canvas.activeThemeId)
        _ = await themeSettingsWriter.write(canvas)
        guard latestThemeSelectionGeneration == selectionGeneration else { return }
        localThemeSettings = canvas; colorTheme = canvas.activeThemeId
        await loadResolvedV2Theme(canvas.activeThemeId)
        guard ThemeLoadCommitPolicy.canCommitCanvasFallback(
            selectionGeneration: selectionGeneration,
            latestSelectionGeneration: latestThemeSelectionGeneration,
            colorTheme: colorTheme,
            intendedThemeId: intendedThemeId
        ) else { return }
        themeAssetImages = [:]
        NotificationCenter.default.post(name: .tailSyncThemeAssetsChanged, object: nil)
    }

    @MainActor
    private func loadThemeAssets(_ id: String) async {
        guard id == colorTheme, id == intendedThemeId else { return }
        latestThemeAssetGeneration &+= 1
        let generation = latestThemeAssetGeneration
        guard let definition = resolvedV2Themes.first(where: { $0.id == id }) else {
            clearThemeAssetsIfCurrent(themeId: id, generation: generation)
            return
        }
        var loaded: [String: NSImage] = [:]
        guard let packageDigest = definition.packageDigest else {
            clearThemeAssetsIfCurrent(themeId: id, generation: generation)
            return
        }
        let request = ThemeAssetLoadRequest(
            themeId: id,
            packageDigest: packageDigest,
            generation: generation
        )
        for (slot, descriptor) in definition.assetSlots {
            guard ["logo", "emptyState", "previewPlaceholder"].contains(slot), descriptor.bytes <= 10 * 1024 * 1024 else { continue }
            if let data = try? await ApiClient.shared.getThemeAssetSlot(themeId: id, digest: packageDigest, slot: slot),
               data.count == descriptor.bytes,
               let image = ThemeAssetImageDecoder.decode(data, slot: slot) {
                loaded[slot] = image
            }
            guard latestThemeAssetGeneration == generation, colorTheme == id else { return }
        }
        let currentDefinitionDigest = resolvedV2Themes.first { $0.id == id }?.packageDigest
        let currentDescriptorDigest = themeDescriptors.first {
            $0.id == id && $0.status == "valid"
        }?.digest
        guard ThemeLoadCommitPolicy.canCommitAssets(
            request,
            latestGeneration: latestThemeAssetGeneration,
            activeThemeId: intendedThemeId,
            currentDefinitionDigest: currentDefinitionDigest,
            currentDescriptorDigest: currentDescriptorDigest
        ) else { return }
        themeAssetImages = loaded
        NotificationCenter.default.post(name: .tailSyncThemeAssetsChanged, object: nil)
    }

    @MainActor
    private func beginThemeResolution(
        themeId: String,
        packageDigest: String,
        highContrast: Bool
    ) -> ThemeResolutionRequest {
        nextThemeResolutionGeneration &+= 1
        latestThemeResolutionGenerations[themeId] = nextThemeResolutionGeneration
        return ThemeResolutionRequest(
            identity: ThemeResolutionCacheIdentity(
                themeId: themeId,
                packageDigest: packageDigest,
                highContrast: highContrast
            ),
            generation: nextThemeResolutionGeneration
        )
    }

    @MainActor
    private func invalidateThemeAssetLoads() {
        latestThemeAssetGeneration &+= 1
    }

    @MainActor
    private func beginThemeSelectionIntent(_ themeId: String) -> UInt64 {
        nextThemeSelectionGeneration &+= 1
        latestThemeSelectionGeneration = nextThemeSelectionGeneration
        intendedThemeId = themeId
        latestThemeResolutionGenerations.removeValue(forKey: colorTheme)
        latestThemeResolutionGenerations.removeValue(forKey: themeId)
        invalidateThemeAssetLoads()
        return nextThemeSelectionGeneration
    }

    @MainActor
    private func clearThemeAssetsIfCurrent(themeId: String, generation: UInt64) {
        guard latestThemeAssetGeneration == generation,
              colorTheme == themeId,
              intendedThemeId == themeId else { return }
        themeAssetImages = [:]
        NotificationCenter.default.post(name: .tailSyncThemeAssetsChanged, object: nil)
    }

    private func fallbackLang() -> String {
        Locale.current.language.languageCode?.identifier == "zh" ? "zh-CN" : "en"
    }

    // ── Dictionary ──────────────────────────────────────────────

    private static let strings: [String: [String: String]] = [
        "en": [
            "history.loadError": "Could not load history",
            "history.title": "History",
            "history.search": "Search history...",
            "history.empty": "No entries",
            "history.restored": "Restored to clipboard",
            "history.restore": "Restore",
            "history.delete": "Delete",
            "history.clearAll": "Clear All History",
            "history.confirmClear": "Delete all clipboard history?",
            "history.deleteError": "Could not delete this history entry",
            "history.dateFilter": "Filter by date",
            "history.date.all": "All dates",
            "history.date.today": "Today",
            "history.date.yesterday": "Yesterday",
            "history.date.last7": "Last 7 days",
            "history.date.last30": "Last 30 days",
            "history.date.thisMonth": "This month",
            "history.date.custom": "Custom",
            "history.date.start": "Start",
            "history.date.end": "End",
            "history.migrationWarningPrefix": "Migration could not complete for",
            "history.migrationWarningSuffix": "history entries. The original data is preserved and will be retried at the next start.",
            "history.sending": "Sending",
            "history.stopTransfer": "Stop",
            "history.files": "files",
            "history.copyAll": "Copy all",
            "history.showMore": "Show more",
            "history.showLess": "Show less",
            "history.incomplete": "Incomplete",
            "history.pin": "Pin",
            "history.unpin": "Unpin",
            "history.syncExpired": "An older clipboard item was not sent to {peer}, preventing it from replacing newer clipboard content.",
            "history.preview.title": "History Preview",
            "history.preview.loading": "Loading preview...",
            "history.preview.error": "Could not load this preview",
            "history.preview.close": "Close preview",
            "history.preview.previousItem": "Previous file",
            "history.preview.nextItem": "Next file",
            "history.preview.restore": "Restore to clipboard",
            "history.preview.restored": "Restored to clipboard",
            "history.preview.restoreFailed": "Restore failed",
            "history.preview.retry": "Retry",
            "history.preview.tooLargeTitle": "File is too large to preview",
            "history.preview.tooLargeMessage": "The file exceeds the 64 MiB preview limit. You can still restore it to the clipboard.",
            "history.preview.unsupportedTitle": "Preview is not available",
            "history.preview.unsupportedMessage": "TailSync cannot preview {type} files yet. Restore the file to use it in another app.",
            "history.preview.corruptTitle": "File appears to be damaged",
            "history.preview.corruptMessage": "The stored bytes could not be decoded as this file type. You can retry or restore the original item.",
            "history.preview.decryptTitle": "Could not decrypt this file",
            "history.preview.decryptMessage": "TailSync could not authenticate the encrypted history data. Retry before restoring the item.",
            "history.preview.unavailableTitle": "Preview could not be loaded",
            "history.preview.unavailableMessage": "The local service or preview renderer is temporarily unavailable.",
            "history.preview.unknownType": "this",
            "history.preview.plainText": "Text",
            "history.preview.code": "Code",
            "history.preview.search": "Search",
            "history.preview.previousMatch": "Previous match",
            "history.preview.nextMatch": "Next match",
            "history.preview.wrapLines": "Toggle line wrapping",
            "history.preview.decreaseFont": "Decrease text size",
            "history.preview.increaseFont": "Increase text size",
            "history.preview.copyAll": "Copy all",
            "history.preview.lines": "lines",
            "history.preview.characters": "characters",
            "history.preview.fit": "Fit to window",
            "history.preview.actualSize": "Actual size",
            "history.preview.rotate": "Rotate view",
            "history.preview.transparency": "Toggle transparency background",
            "history.preview.thumbnails": "Toggle page thumbnails",
            "common.cancel": "Cancel",
            "history.categoryFilter": "Filter by category",
            "history.category.all": "All categories",
            "history.category.text": "Text",
            "history.category.website": "Website",
            "history.category.code": "Code",
            "history.category.command": "Command",
            "history.category.structured_data": "Structured data",
            "history.category.path": "Path",
            "history.category.image": "Image",
            "history.category.file": "File",
            "settings.title": "Settings",
            "settings.general": "General",
            "settings.syncEnabled": "Clipboard sync",
            "settings.syncShortcut": "Sync shortcut",
            "settings.historyShortcut": "History shortcut",
            "settings.historyShortcutRecord": "Record history shortcut",
            "settings.shortcutRecord": "Record",
            "settings.shortcutRecording": "Press a key combination…",
            "settings.shortcutSave": "Save",
            "settings.shortcutCancel": "Cancel",
            "settings.shortcutNone": "None",
            "settings.notifications": "Notifications",
            "settings.progressBar": "Progress bar",
            "settings.history": "History",
            "settings.storage": "Storage",
            "settings.storageDescription": "History and file transfer data",
            "settings.storageChange": "Change",
            "settings.storageMoving": "Moving...",
            "settings.storageQuota": "Storage quota",
            "settings.storageDeleteOld": "Delete old data",
            "settings.storageKeepOld": "Keep",
            "settings.limit": "Limit",
            "settings.appearance": "Appearance",
            "settings.theme": "Theme",
            "settings.themeSystem": "System",
            "settings.themeLight": "Light",
            "settings.themeDark": "Dark",
            "settings.colorTheme": "Visual theme",
            "settings.colorThemeDescription": "Type, material, colour, and density",
            "settings.colorTheme.tailsync": "Canvas",
            "settings.colorTheme.ocean": "Flux",
            "settings.colorTheme.forest": "Ledger",
            "settings.colorTheme.rose": "Aura",
            "settings.colorTheme.high-contrast": "Mono",
            "settings.themePackages": "Theme packages",
            "settings.themePackagesDescription": "Built-in and installed Theme V2 packages",
            "settings.themePackageImport": "Import theme",
            "settings.themePackageImportTitle": "Select a theme package",
            "settings.themePackageInstall": "Install",
            "settings.themePackageUpdate": "Update",
            "settings.themePackageUpdateTitle": "Select an updated theme package",
            "settings.themePackageCandidateVersion": "Candidate version",
            "settings.themePackageReplaceTitle": "Replace the installed theme with the same version?",
            "settings.themePackageDowngradeTitle": "Install an older version of this theme?",
            "settings.themePackageRollback": "Roll back",
            "settings.themePackageRollbackTitle": "Roll back this theme?",
            "settings.themePackagePreview": "Theme package preview",
            "settings.themePreviewLight": "Light",
            "settings.themePreviewDark": "Dark",
            "settings.themePreviewHighLight": "High contrast light",
            "settings.themePreviewHighDark": "High contrast dark",
            "settings.themePreviewSearch": "Focused search",
            "settings.themePreviewHover": "Hovered history row",
            "settings.themePreviewSelected": "Selected history row",
            "settings.themePreviewStateDefault": "Default",
            "settings.themePreviewStateHover": "Hover",
            "settings.themePreviewStateActive": "Active",
            "settings.themePreviewStateSelected": "Selected",
            "settings.themePreviewStateDisabled": "Disabled",
            "settings.themePreviewStateFocus": "Focus",
            "settings.themePackageIdMismatch": "The update package belongs to a different theme.",
            "settings.themePackageDelete": "Delete",
            "settings.themePackageDeleteTitle": "Delete this theme?",
            "settings.themePackageBuiltIn": "Built-in",
            "settings.themePackageFallback": "This theme package is not available on this device; using the default theme",
            "settings.selected": "Selected",
            "settings.language": "Language",
            "settings.saved": "Settings saved",
            "settings.loading": "Connecting to backend...",
            "settings.error": "Failed to connect",
            "settings.retry": "Retry",
            "menu.history": "History",
            "menu.settings": "Settings",
            "menu.checkForUpdates": "Check for Updates",
            "menu.quit": "Quit TailSync",
            "update.title": "TailSync Update",
            "update.available": "TailSync {version} is available",
            "update.installPrompt": "Install the signed update now?",
            "update.install": "Install Update",
            "update.current": "TailSync is up to date.",
            "update.failed": "Update Failed",
            "settings.network": "Network",
            "settings.connectionMode": "Connection",
            "settings.modeAuto": "Automatic",
            "settings.modeTailscale": "Tailscale",
            "settings.modeLan": "Local network",
            "settings.localDevice": "This device",
            "settings.identityLoading": "Loading identity...",
            "settings.copyPublicKey": "Copy public key",
            "settings.pairDevice": "Pair a device",
            "settings.peerHostname": "Device name",
            "settings.peerPublicKey": "Remote public key (Base64)",
            "settings.pair": "Pair",
            "settings.allowPairing": "Allow pairing",
            "settings.closePairing": "Close",
            "settings.pairingClosed": "Currently closed",
            "settings.waitingPairing": "Waiting for another device",
            "settings.pairingInstruction": "On the other device, click Allow pairing. Then click the pair icon next to it here.",
            "settings.secureHandshake": "Establishing a secure connection",
            "settings.compareCode": "Confirm that the other device shows the same code",
            "settings.waitingPeerConfirm": "Confirmed, waiting for the other device...",
            "settings.codesMatch": "Codes match",
            "settings.confirmed": "Confirmed",
            "settings.cancel": "Cancel",
            "settings.unpair": "Revoke pairing",
            "settings.removeDevice": "Remove",
            "settings.loadingDevices": "Discovering devices...",
            "settings.noDevices": "No devices found",
            "settings.devices": "devices",
            "settings.pairedOffline": "Paired · waiting for device",
            "settings.connected": "Connected",
            "settings.disconnected": "Not connected",
            "settings.online": "Online",
            "settings.offline": "Offline",
            "settings.protocolUpgradeRequired": "Incompatible version. Update both devices to a release using protocol v{version}.",
            "settings.confirming": "Confirming…",
            "settings.discovered": "Discovered",
            "settings.paired": "Paired",
            "settings.notPaired": "Not paired",
            "settings.refresh": "Refresh",
            "settings.testConnection": "Test connection",
            "settings.relayPath": "Relay",
            "settings.testRouteRediscover": "Iroh latency testing requires rediscovering this device",
            "settings.connectionFailed": "Connection failed",
            "error.localServiceUnavailable": "TailSync's local service is unavailable. Please wait a moment and try again.",
            "error.localServiceSendFailed": "Could not send the request to TailSync's local service.",
            "error.localServiceNoResponse": "TailSync's local service did not respond.",
            "error.localServiceInvalidResponse": "TailSync's local service returned an invalid response.",
            "error.pairingWindowClosed": "The other device is not allowing pairing. Click Allow pairing on that device first.",
            "error.pairingHandshakeTimedOut": "The other device did not answer the pairing request. Update both devices and click Allow pairing on the other device first.",
            "error.pairingConnectionClosed": "The other device closed the pairing connection. Update both devices and click Allow pairing on the other device first.",
        ],
        "zh-CN": [
            "history.loadError": "无法加载历史记录",
            "history.title": "历史记录",
            "history.search": "搜索历史...",
            "history.empty": "暂无记录",
            "history.restored": "历史已回溯至剪切板",
            "history.restore": "回溯",
            "history.delete": "删除",
            "history.clearAll": "清空所有记录",
            "history.confirmClear": "确定要删除所有记录吗？",
            "history.deleteError": "无法删除这条历史记录",
            "history.dateFilter": "按日期筛选",
            "history.date.all": "全部日期",
            "history.date.today": "今天",
            "history.date.yesterday": "昨天",
            "history.date.last7": "近 7 天",
            "history.date.last30": "近 30 天",
            "history.date.thisMonth": "本月",
            "history.date.custom": "自定义",
            "history.date.start": "开始",
            "history.date.end": "结束",
            "history.migrationWarningPrefix": "有",
            "history.migrationWarningSuffix": "条历史记录未完成迁移。原数据已保留，并会在下次启动时重试。",
            "history.sending": "正在发送",
            "history.stopTransfer": "停止",
            "history.files": "个文件",
            "history.copyAll": "全部复制",
            "history.showMore": "展开剩余",
            "history.showLess": "收起",
            "history.incomplete": "未完成",
            "history.pin": "置顶",
            "history.unpin": "取消置顶",
            "history.syncExpired": "一条较早的剪贴板内容未发送到 {peer}，以避免覆盖对方较新的剪贴板内容。",
            "history.preview.title": "历史预览",
            "history.preview.loading": "正在加载预览…",
            "history.preview.error": "无法加载此预览",
            "history.preview.close": "关闭预览",
            "history.preview.previousItem": "上一个文件",
            "history.preview.nextItem": "下一个文件",
            "history.preview.restore": "回溯到剪贴板",
            "history.preview.restored": "已恢复到剪贴板",
            "history.preview.restoreFailed": "回溯失败",
            "history.preview.retry": "重试",
            "history.preview.tooLargeTitle": "文件过大，无法预览",
            "history.preview.tooLargeMessage": "文件超过 64 MiB 预览上限，仍可将其回溯到剪贴板。",
            "history.preview.unsupportedTitle": "暂不支持预览",
            "history.preview.unsupportedMessage": "TailSync 暂时无法预览 {type} 文件，可回溯后使用其他应用打开。",
            "history.preview.corruptTitle": "文件可能已损坏",
            "history.preview.corruptMessage": "存储内容无法按此文件类型解码，可重试或回溯原始记录。",
            "history.preview.decryptTitle": "无法解密此文件",
            "history.preview.decryptMessage": "TailSync 无法验证这份加密历史数据，请先重试再回溯。",
            "history.preview.unavailableTitle": "无法加载预览",
            "history.preview.unavailableMessage": "本地服务或预览组件暂时不可用。",
            "history.preview.unknownType": "此类型",
            "history.preview.plainText": "文本",
            "history.preview.code": "代码",
            "history.preview.search": "搜索",
            "history.preview.previousMatch": "上一个匹配项",
            "history.preview.nextMatch": "下一个匹配项",
            "history.preview.wrapLines": "切换自动换行",
            "history.preview.decreaseFont": "缩小字号",
            "history.preview.increaseFont": "放大字号",
            "history.preview.copyAll": "复制全部",
            "history.preview.lines": "行",
            "history.preview.characters": "字符",
            "history.preview.fit": "适应窗口",
            "history.preview.actualSize": "实际大小",
            "history.preview.rotate": "旋转查看",
            "history.preview.transparency": "切换透明背景",
            "history.preview.thumbnails": "切换页面缩略图",
            "common.cancel": "取消",
            "history.categoryFilter": "按分类筛选",
            "history.category.all": "全部分类",
            "history.category.text": "文本",
            "history.category.website": "网站",
            "history.category.code": "代码",
            "history.category.command": "命令",
            "history.category.structured_data": "结构化数据",
            "history.category.path": "路径",
            "history.category.image": "图片",
            "history.category.file": "文件",
            "settings.title": "设置",
            "settings.syncEnabled": "剪贴板同步",
            "settings.syncShortcut": "同步快捷键",
            "settings.historyShortcut": "历史记录快捷键",
            "settings.historyShortcutRecord": "录制历史记录快捷键",
            "settings.shortcutRecord": "录制",
            "settings.shortcutRecording": "请按下键组合…",
            "settings.shortcutSave": "保存",
            "settings.shortcutCancel": "取消",
            "settings.shortcutNone": "无",
            "settings.general": "通用",
            "settings.notifications": "通知",
            "settings.progressBar": "进度条",
            "settings.history": "历史记录",
            "settings.storage": "存储",
            "settings.storageDescription": "历史记录和文件传输数据",
            "settings.storageChange": "更改",
            "settings.storageMoving": "正在迁移...",
            "settings.storageQuota": "存储配额",
            "settings.storageDeleteOld": "删除旧数据",
            "settings.storageKeepOld": "保留",
            "settings.limit": "上限",
            "settings.appearance": "外观",
            "settings.theme": "主题",
            "settings.themeSystem": "跟随系统",
            "settings.themeLight": "浅色",
            "settings.themeDark": "深色",
            "settings.colorTheme": "视觉主题",
            "settings.colorThemeDescription": "排版、材质、色彩与界面密度",
            "settings.colorTheme.tailsync": "画布 Canvas",
            "settings.colorTheme.ocean": "流光 Flux",
            "settings.colorTheme.forest": "书页 Ledger",
            "settings.colorTheme.rose": "柔光 Aura",
            "settings.colorTheme.high-contrast": "单色 Mono",
            "settings.themePackages": "主题包",
            "settings.themePackagesDescription": "内置与已安装的 Theme V2 主题包",
            "settings.themePackageImport": "导入主题",
            "settings.themePackageImportTitle": "选择主题包",
            "settings.themePackageInstall": "安装",
            "settings.themePackageUpdate": "更新",
            "settings.themePackageUpdateTitle": "选择更新后的主题包",
            "settings.themePackageCandidateVersion": "候选版本",
            "settings.themePackageReplaceTitle": "确认使用同版本主题包替换当前版本？",
            "settings.themePackageDowngradeTitle": "确认安装此主题的较旧版本？",
            "settings.themePackageRollback": "回滚",
            "settings.themePackageRollbackTitle": "确定回滚此主题？",
            "settings.themePackagePreview": "主题包预览",
            "settings.themePreviewLight": "浅色",
            "settings.themePreviewDark": "深色",
            "settings.themePreviewHighLight": "高对比度浅色",
            "settings.themePreviewHighDark": "高对比度深色",
            "settings.themePreviewSearch": "搜索框聚焦",
            "settings.themePreviewHover": "历史记录悬停",
            "settings.themePreviewSelected": "历史记录选中",
            "settings.themePreviewStateDefault": "默认",
            "settings.themePreviewStateHover": "悬停",
            "settings.themePreviewStateActive": "按下",
            "settings.themePreviewStateSelected": "选中",
            "settings.themePreviewStateDisabled": "禁用",
            "settings.themePreviewStateFocus": "聚焦",
            "settings.themePackageIdMismatch": "更新包属于另一个主题。",
            "settings.themePackageDelete": "删除",
            "settings.themePackageDeleteTitle": "确定删除此主题？",
            "settings.themePackageBuiltIn": "内置",
            "settings.themePackageFallback": "此主题包在本设备不可用，已使用默认主题",
            "settings.selected": "已选择",
            "settings.language": "语言",
            "settings.saved": "设置已保存",
            "settings.loading": "连接后端中...",
            "settings.error": "连接失败",
            "settings.retry": "重试",
            "menu.history": "历史记录",
            "menu.settings": "设置",
            "menu.checkForUpdates": "检查更新",
            "menu.quit": "退出 TailSync",
            "update.title": "TailSync 更新",
            "update.available": "TailSync {version} 已可用",
            "update.installPrompt": "现在安装已签名的更新吗？",
            "update.install": "安装更新",
            "update.current": "TailSync 已是最新版。",
            "update.failed": "更新失败",
            "settings.network": "网络",
            "settings.connectionMode": "连接方式",
            "settings.modeAuto": "自动",
            "settings.modeTailscale": "Tailscale",
            "settings.modeLan": "局域网",
            "settings.localDevice": "本机设备",
            "settings.identityLoading": "正在加载设备身份…",
            "settings.copyPublicKey": "复制公钥",
            "settings.pairDevice": "配对设备",
            "settings.peerHostname": "设备名称",
            "settings.peerPublicKey": "远端公钥（Base64）",
            "settings.pair": "配对",
            "settings.allowPairing": "允许配对",
            "settings.closePairing": "关闭",
            "settings.pairingClosed": "当前关闭",
            "settings.waitingPairing": "等待另一台设备",
            "settings.pairingInstruction": "请先在另一台设备点击“允许配对”，再点击此处该设备旁的配对图标。",
            "settings.secureHandshake": "正在建立安全连接",
            "settings.compareCode": "请确认另一台设备显示相同验证码",
            "settings.waitingPeerConfirm": "已确认，等待对端确认...",
            "settings.codesMatch": "验证码一致",
            "settings.confirmed": "已确认",
            "settings.cancel": "取消",
            "settings.unpair": "撤销配对",
            "settings.removeDevice": "删除",
            "settings.loadingDevices": "正在发现设备…",
            "settings.noDevices": "未发现设备",
            "settings.devices": "台设备",
            "settings.pairedOffline": "已配对 · 等待设备上线",
            "settings.connected": "已连接",
            "settings.disconnected": "未连接",
            "settings.online": "在线",
            "settings.offline": "离线",
            "settings.protocolUpgradeRequired": "版本不兼容，请将两台设备都更新到使用协议 v{version} 的版本。",
            "settings.confirming": "正在确认…",
            "settings.discovered": "已发现",
            "settings.paired": "已配对",
            "settings.notPaired": "未配对",
            "settings.refresh": "刷新",
            "settings.testConnection": "测试连接",
            "settings.relayPath": "中继",
            "settings.testRouteRediscover": "Iroh 延迟测试需要先重新发现该设备",
            "settings.connectionFailed": "连接失败",
            "error.localServiceUnavailable": "TailSync 本地服务暂时不可用，请稍后重试。",
            "error.localServiceSendFailed": "无法向 TailSync 本地服务发送请求。",
            "error.localServiceNoResponse": "TailSync 本地服务没有响应。",
            "error.localServiceInvalidResponse": "TailSync 本地服务返回了无效响应。",
            "error.pairingWindowClosed": "另一台设备尚未允许配对，请先在该设备上点击“允许配对”。",
            "error.pairingHandshakeTimedOut": "另一台设备未响应配对请求。请先将两端更新到最新版，并在另一台设备上点击“允许配对”。",
            "error.pairingConnectionClosed": "另一台设备关闭了配对连接。请先将两端更新到最新版，并在另一台设备上点击“允许配对”。",
        ],
    ]

    static func t(_ key: String) -> String {
        strings[shared.lang]?[key] ?? strings["en"]?[key] ?? key
    }

    // ── Theme ───────────────────────────────────────────────────

    func applyTheme() {
        DispatchQueue.main.async {
            // Headless contexts (tests, early launch) may have no
            // NSApplication yet; the appearance is irrelevant there.
            guard let app = NSApp else { return }
            switch self.theme {
            case "dark":  app.appearance = NSAppearance(named: .darkAqua)
            case "light": app.appearance = NSAppearance(named: .aqua)
            default:      app.appearance = nil
            }
        }
    }
}
