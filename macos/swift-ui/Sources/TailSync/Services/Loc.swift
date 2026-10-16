import Foundation
import AppKit
import ImageIO
import SwiftUI

extension Notification.Name {
    static let tailSyncLocaleChanged = Notification.Name("TailSyncLocaleChanged")
    static let tailSyncSettingsChanged = Notification.Name("TailSyncSettingsChanged")
    static let tailSyncConnectionModeChanged = Notification.Name("TailSyncConnectionModeChanged")
    static let tailSyncThemeAssetsChanged = Notification.Name("TailSyncThemeAssetsChanged")
    static let tailSyncRemotePairingInviteReceived = Notification.Name(
        "TailSyncRemotePairingInviteReceived"
    )
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

    static func configurationURL(environment: [String: String], applicationSupport: URL) -> URL {
        let directory: URL
        if let isolatedRoot = environment["TAILSYNC_DATA_DIR"] {
            directory = URL(fileURLWithPath: isolatedRoot, isDirectory: true)
        } else {
            directory = applicationSupport.appendingPathComponent("com.tailsync.TailSync")
        }
        return directory.appendingPathComponent("config-v2.json")
    }

    private static let configURL = configurationURL(
        environment: ProcessInfo.processInfo.environment,
        applicationSupport: FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
    )

    private init() {
        reload()
        NSWorkspace.shared.notificationCenter.addObserver(
            forName: NSWorkspace.accessibilityDisplayOptionsDidChangeNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            Task { @MainActor [weak self] in
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
            "history.composite.syncIncomplete": "Rich content was not fully applied on {peer}; history retains the complete captured item.",
            "history.composite.applyUnknown": "Clipboard application on {peer} is uncertain; retries only save history.",
            "history.composite.partial": "Only some formats were published on {peer}. Restore manually from history.",
            "history.composite.superseded": "Rich content was saved to history on {peer}; the clipboard keeps newer content.",

            "history.composite.formats": "formats",
            "history.composite.images": "images",
            "history.composite.localOnly": "Local restore only",
            "history.composite.incomplete": "Incomplete",
            "history.composite.resourcesMissing": "Some resources not saved",
            "history.sourceLocal": "This device",
            "history.composite.copy.text": "Plain text only",
            "history.composite.copy.menu": "Copy format",
            "history.composite.copy.png": "Image only",
            "history.composite.moreActions": "More actions",
            "history.composite.advanced": "Advanced",
            "history.composite.details": "Details",
            "history.composite.diag.formats": "Formats",
            "history.composite.diag.representations": "Representations",
            "history.composite.diag.images": "Images",
            "history.composite.diag.nativeItems": "Native items",
            "history.composite.diag.nativeOnly": "Native only",
            "history.composite.diag.completeness": "Completeness",
            "history.composite.diag.restorable": "Restorable formats",
            "history.composite.diag.withheld": "Withheld formats",
            "history.composite.diag.yes": "Yes",
            "history.composite.diag.no": "No",
            "history.composite.diag.none": "None",
            "history.composite.diag.unavailable": "Unavailable",
            "history.composite.diag.unknown": "Unknown",
            "history.composite.completeness.complete": "Complete",
            "history.composite.completeness.external_references": "External references",
            "history.composite.completeness.missing_formats": "Missing formats",
            "history.composite.archiveImages": "Complete network images (click to go online)",
            "history.composite.archiveImagesHint": "Completing network images can fail on its own; the rest of this item can still be restored.",
            "history.composite.archiveDone": "Images saved to local history",
            "history.composite.archivePartial": "Some images could not be saved; the original references are kept",
            "history.composite.archiveBlocked": "Reference is not an allowed public HTTP(S) address",
            "history.composite.archiveInvalid": "Not a supported static PNG image",
            "history.composite.archiveLimit": "Image archive limit exceeded",
            "history.composite.archiveUnavailable": "Image is unavailable or the request timed out",
            "history.composite.authorizeSource": "Authorize browser source (writes a ticket file)",
            "history.composite.importSourcePacket": "Import source packet (from the extension)",
            "history.composite.revokeSource": "Revoke source authorization",
            "history.composite.sourceAuthorized": "Source authorization saved to the chosen file",
            "history.composite.sourceImported": "Browser images saved to local history",
            "history.composite.sourcePartial": "Some browser images were not imported; the original references are kept",
            "history.composite.sourceUnavailable": "No matching same-origin images to authorize",
            "history.composite.sourceSaveFailed": "Could not write the authorization file",
            "history.composite.sourceRevoked": "Source authorization revoked",

            // Stable restore-outcome codes (repair plan §4 R5-04). The server returns a
            // code, never Rust prose, so both UIs localise the same failure. Only
            // `published` may be presented as a completed copy.
            "history.restore.code.published": "Copied to clipboard",
            "history.restore.code.publish_partial": "Saved to history, but only part of it was published. You can copy it again.",
            "history.restore.code.publish_unknown": "Saved to history, but the publish result is unknown. You can copy it again.",
            "history.restore.code.publish_superseded": "Saved to history; the clipboard keeps newer content.",
            "history.restore.code.native_only": "This item can only be restored on the platform that captured it.",
            "history.restore.code.originals_incomplete": "Some original formats were not captured. Choose an available format to restore.",
            "history.restore.code.resources_incomplete": "Some resources are not saved locally. Use the menu to copy just the text, or just the saved image.",
            "history.restore.code.no_single_image": "This item holds several images, so there is no single image to copy. Choose one image instead.",
            "history.restore.availableContent": "Copied saved content; {count} image(s) not saved",
            "history.restore.code.format_unavailable": "This item does not carry that format.",
            "history.restore.code.unsupported_format": "That restore format is not supported.",
            "history.restore.recopy": "Copy again",

            "history.restore": "Restore",
            "history.delete": "Delete",
            "history.clearAll": "Clear All History",
            "history.confirmClear": "Delete all un-favorited clipboard history? Favorites are preserved.",
            "history.deleteError": "Could not delete this history entry",
            "history.favoriteProtected": "Favorites can only be removed from the Favorites window",
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
            "history.storageUnavailable": "Storage is temporarily unavailable, so some new items may not be saved yet. Keep TailSync running, restore access to the current storage location or free space, and wait for recovery before moving it.",
            "history.fileSelectionSymlink": "The copied selection included a symbolic link, so nothing was added to file history. Copy the actual file instead.",
            "history.fileSelectionFolder": "The copied selection included a folder or non-file item, so nothing was added to file history. Copy ordinary files instead.",
            "history.sending": "Sending",
            "history.stopTransfer": "Stop",
            "history.files": "files",
            "history.copyAll": "Copy all",
            "history.showMore": "Show more",
            "history.showLess": "Show less",
            "history.incomplete": "Incomplete",
            "history.pin": "Favorite",
            "history.unpin": "Remove favorite",
            "favorites.open": "Open Favorites",
            "favorites.title": "Favorites",
            "favorites.emptyTitle": "No favorites yet",
            "favorites.emptyDescription": "Long-press a history item to save it here",
            "history.windowPin": "Keep history window on top",
            "history.windowUnpin": "Stop keeping history window on top",
            "history.syncExpired": "An older clipboard item was not sent to {peer}, preventing it from replacing newer clipboard content.",
            "history.syncStalled": "Clipboard sync to {peer} stalled — the connection was not ready. Reconnect the device if it persists.",
            "history.syncShutdown": "Clipboard delivery to {peer} was stopped while TailSync was shutting down.",
            "history.clipboardUnavailable": "Clipboard access is unavailable. Check the helper and try copying again.",
            "history.captureFailed": "This copy could not be captured completely. Available content is kept in history when possible.",
            "history.captureOriginUnknown": "This copy was saved to history, but its source could not be confirmed, so it was not synced automatically. Copy it again from the source app if you want to sync it.",
            "history.captureNotShared": "This copy was saved to history, but could not be prepared for sync. Restore it from history, or copy it again from the source app to sync it.",
            "history.deliveryRejected": "Clipboard content could not be delivered to {peer}. The peer may not support its formats; review history and copy it again if needed.",
            "history.warningOverflow": "Additional sync warnings occurred. Review recent history and delivery results.",
            "history.captureNotSaved": "This copy could not be saved to history (storage is full, unavailable, or the content is unsupported). Check storage, then copy again from the source app or try another copy format.",
            "history.receiveNotSaved": "Received content could not be saved to history. Check history storage, then copy the content again on the sending device.",
            "history.syncDeliveryExpired": "Clipboard delivery to {peer} expired before it could be sent.",
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
            "history.preview.documentTitle": "Document",
            "history.preview.documentAccessibilitySummary": "{paragraphs} paragraphs, {tables} tables, {images} images, {placeholders} unavailable images",
            "history.preview.documentImage": "Image",
            "history.preview.documentTable": "Table",
            "history.preview.imageUnavailable": "Image not saved",
            "history.preview.imageBudget": "Image omitted to stay within the preview memory budget",
            "history.preview.imageCannotDisplay": "Image cannot be displayed",
            "history.preview.imageAvailability.available": "Image available",
            "history.preview.imageAvailability.missing_resource": "The saved image data is missing",
            "history.preview.imageAvailability.missing_source": "The document did not provide image bytes",
            "history.preview.imageAvailability.external_reference": "The image points outside the saved content",
            "history.preview.imageAvailability.unsupported": "This image format is not supported in the preview",
            "history.preview.imageAvailability.pixel_budget": "The image exceeds the preview pixel budget",
            "history.preview.partialTitle": "Preview notice",
            "history.preview.partial.limit": "Only part of the content is shown because it reached a preview limit.",
            "history.preview.partial.pixelBudget": "Some images are placeholders because the preview reached its pixel budget.",
            "history.preview.partial.malformed": "Some markup was not displayed because it is malformed or unsupported.",
            "history.preview.partial.missingImage": "Some images are missing or referenced external content and were not loaded.",
            "history.preview.partial.unverified": "The relationship between the preview and the original clipboard formats could not be verified.",
            "history.preview.partial.other": "Part of this document could not be shown exactly as stored.",
            "history.preview.partial.simplified": "Some markup or text formatting was simplified for reading.",
            "history.preview.partial.nativeAnalysis": "The native body could not be analyzed, so its content is not shown.",
            "history.preview.partial.stylesSimplified": "Native text styles were simplified for reading.",
            "history.preview.partial.nativeTableStructure": "Table structure from the native document is not available.",
            "history.preview.partial.nativePartial": "Some native attributes could not be represented.",
            "history.preview.partial.nativeOther": "Some native items are not shown.",
            "history.preview.partial.unsupportedMarkup": "Some markup is not supported and was kept as plain text or simplified.",
            "history.preview.partial.unsupportedImage": "Some images could not be decoded and are not shown.",
            "history.preview.noticeSummary": "Preview notices: {reasons}",
            "history.preview.noticeSeparator": ", ",
            "history.preview.noticeDetails": "Details",
            "history.preview.noticeDiagnostics": "Preview simplifications ({count})",
            "history.preview.notice.missingImage": "Images not saved",
            "history.preview.notice.truncated": "Content truncated",
            "history.preview.notice.pixelBudget": "Image budget reached",
            "history.preview.notice.structureUnavailable": "Structure not fully shown",
            "history.preview.notice.unverifiedSource": "Format conversion not verified",
            "history.preview.notice.malformed": "Markup not shown",
            "history.preview.notice.simplified": "Formatting simplified",
            "history.preview.notice.other": "Other differences",
            "history.preview.notice.unsupportedMarkup": "Markup not shown as stored",
            "history.preview.notice.imageUnsupported": "Images not shown",
            "history.preview.notice.nativeUnavailable": "Content not shown",
            "history.preview.markdown": "MD",
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
            "history.preview.svgVisual": "Preview",
            "history.preview.svgSource": "Source",
            "history.preview.svgRendering": "Rendering preview…",
            "history.preview.svgTrustExternal": "Trust external links",
            "history.preview.svgTrustedExternal": "External links trusted",
            "history.preview.svgTrustExternalHelp": "Allow this preview to load external images and fonts over HTTPS only. Scripts stay disabled and non-public addresses are refused, but those servers will see the request.",
            "history.preview.svgTrustAlertTitle": "Allow external resources?",
            "history.preview.svgTrustAlertMessage": "This preview will contact the following HTTPS hosts. Scripts stay disabled, but these servers will see your request.",
            "history.preview.svgTrustAlertDeny": "Don’t allow",
            "history.preview.svgTrustAlertAllow": "Allow",
            "history.preview.svgTrustRejectedTitle": "Cannot trust external links",
            "history.preview.svgTrustRejectedMessage": "This SVG references non-HTTPS or non-public addresses. They are refused, and external resources cannot be enabled for it:",
            "history.preview.svgBlockedContent": "This SVG contains active or navigation markup, so it shows as source only.",
            "history.preview.svgTooLarge": "This SVG exceeds the preview size limit, so it shows as source only.",
            "history.preview.svgRenderFailed": "The SVG preview could not be rendered; showing source instead.",
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
            "history.display.document": "Document",
            "history.display.unknown": "Content pending",
            "history.category.file": "File",
            "settings.title": "Settings",
            "settings.general": "General",
            "settings.syncEnabled": "Clipboard sync",
            "settings.syncEnabledDescription": "Send newly copied items to paired devices that are enabled",
            "settings.launchAtLogin": "Launch at login",
            "settings.launchAtLoginDescription": "Start TailSync automatically after you log in",
            "settings.syncShortcut": "Sync shortcut",
            "settings.syncShortcutDescription": "Pause or resume clipboard sync from any app",
            "settings.historyShortcut": "History shortcut",
            "settings.historyShortcutDescription": "Open the history window from any app",
            "settings.launchAtLoginApproval": "Approval is required in System Settings.",
            "settings.launchAtLoginOpenSettings": "Open Settings",
            "settings.launchAtLoginError": "Could not update this setting.",
            "settings.historyShortcutRecord": "Record",
            "settings.shortcutRecord": "Record",
            "settings.shortcutChange": "Change",
            "settings.shortcutRecording": "Press a key combination…",
            "settings.shortcutSave": "Save",
            "settings.shortcutCancel": "Cancel",
            "settings.shortcutNone": "None",
            "settings.resetDefault": "Reset to default",
            "settings.clearShortcut": "Clear shortcut",
            "settings.notifications": "Notifications",
            "settings.notificationsDescription": "Show a notification when content arrives from a paired device",
            "settings.progressBar": "Progress bar",
            "settings.progressBarDescription": "Show transfer feedback while large files are being sent",
            "settings.history": "History",
            "settings.storage": "Storage",
            "settings.storageDescription": "History and file transfer data",
            "settings.storageChange": "Change",
            "settings.storageMoving": "Moving...",
            "settings.storageQuota": "Storage quota",
            "settings.storageQuotaDescription": "Automatically delete the oldest entries when this limit is exceeded",
            "settings.storageDeleteOld": "Delete old data",
            "settings.storageKeepOld": "Keep",
            "settings.storageChanged": "Storage location changed",
            "settings.storageUnavailable": "Storage is temporarily unavailable. Some new items may not be saved yet.",
            "settings.storageRecoveryGuidance": "Keep TailSync running, restore the current storage location's access or free space, and wait for recovery before moving it.",
            "settings.storageCapacityUnavailable": "Capacity unavailable while storage is offline",
            "settings.storageStatusUnconfirmed": "Storage status is temporarily unavailable",
            "settings.limit": "History limit",
            "settings.limitDescription": "Keep up to {value} clipboard history entries",
            "settings.autoSaveNetworkImages": "Auto-save network images",
            "settings.qqImageAccess": "QQ message images",
            "settings.qqImageAccessDescription": "QQ supplies only the first image directly. Allow access to this account's image cache to save all images in copied messages.",
            "settings.qqImageAccessChoose": "Allow access…",
            "settings.qqImageAccessChange": "Choose folder…",
            "settings.qqImageAccessReady": "Image cache access is saved. Copy the QQ message again to save its images. Existing history is unchanged.",
            "settings.qqImageAccessInvalid": "Choose this QQ account's nt_data/Pic directory. Copy a QQ message first to suggest its image cache.",
            "settings.qqImageAccessFailed": "Image cache access could not be saved. Select the Pic folder again and allow access in the system dialog.",
            "settings.autoSaveNetworkImagesDescription": "Download public images in new local copies only (uses data and storage). Existing or received history stays manual. Turning off cancels pending auto-downloads, not saved images.",
            "settings.appearance": "Appearance",
            "settings.theme": "Theme",
            "settings.themeDescription": "Follow the system appearance or choose manually",
            "settings.themeSystem": "System",
            "settings.themeLight": "Light",
            "settings.themeDark": "Dark",
            "settings.language": "Language",
            "settings.languageDescription": "Choose the interface language for TailSync",
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
            "settings.saved": "Settings saved",
            "settings.saveFailed": "Could not save settings",
            "settings.loading": "Connecting to backend...",
            "settings.error": "Failed to connect",
            "settings.retry": "Retry",
            "settings.updates": "Updates",
            "settings.currentVersion": "Current version",
            "settings.updateReady": "Ready to check for signed updates",
            "settings.updateChecking": "Checking for updates...",
            "settings.updateAvailable": "TailSync {version} is available",
            "settings.updateCurrent": "TailSync is up to date",
            "settings.updateInstalling": "Downloading and verifying the signed update...",
            "settings.updateInstalled": "Update installed. Restart TailSync to finish.",
            "settings.updateFailed": "Could not check for updates",
            "settings.updateCheck": "Check for updates",
            "settings.updateInstall": "Install update",
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
            "connections.title": "Devices & Connections",
            "settings.manageConnections": "Devices & Connections",
            "settings.manageConnectionsDescription": "View online devices, manage discovery and secure pairing",
            "settings.openConnections": "Open",
            "settings.network": "Network",
            "settings.connectionMode": "Connection",
            "settings.connectionModeDescription": "Choose how TailSync discovers devices and manage secure pairing",
            "settings.modeAuto": "Automatic",
            "settings.modeTailscale": "Tailscale",
            "settings.modeLan": "Local network",
            "settings.modeIroh": "Iroh",
            "settings.localDevice": "This device",
            "settings.identityLoading": "Loading identity...",
            "settings.copyPublicKey": "Copy public key",
            "notice.notificationGap": "{count} earlier notifications are no longer available. Check history and transfer status for the current state.",
            "settings.pairDevice": "Pair a device",
            "settings.pendingPairing": "Pairing is incomplete. Open pairing on both devices and compare the code again, or forget this device.",
            "settings.pendingPeerCompletion": "This device trusts the peer; the peer’s completion is unconfirmed. Pair again on both devices to recover.",
            "settings.resumePairing": "Pair again",
            "settings.pendingStoreUnavailable": "Pairing recovery data cannot be read. Use a compatible version to recover it.",
            "settings.peerHostname": "Device name",
            "settings.peerPublicKey": "Remote public key (Base64)",
            "settings.pair": "Pair",
            "settings.allowPairing": "Allow pairing",
            "settings.closePairing": "Close",
            "settings.pairingClosed": "Currently closed",
            "settings.remotePairing": "Remote Iroh pairing",
            "settings.remotePairingDescription": "Use a short-lived link when the devices are not on the same network",
            "settings.createRemoteInvite": "Create invite link",
            "settings.createRemoteInviteDescription": "Create a one-time link on this device, then send it to the other device",
            "settings.remoteInviteLink": "Remote pairing invite link",
            "settings.copyInvite": "Copy invite link",
            "settings.copied": "Copied",
            "settings.cancelRemoteInvite": "Cancel invite",
            "settings.remoteInviteCopyFailed": "Could not copy the invite link",
            "settings.remoteInviteExpires": "Expires in {seconds}s",
            "settings.useRemoteInvite": "Use invite link",
            "settings.useRemoteInviteDescription": "Paste the link from the other device to start secure pairing",
            "settings.remoteInvitePlaceholder": "tailsync://pair/v1/...",
            "settings.checkInvite": "Check link",
            "settings.startRemotePairing": "Start pairing",
            "settings.remoteInviteValid": "Link is valid for {seconds}s",
            "settings.waitingPairing": "Waiting for another device",
            "settings.pairingInstruction": "On the other device, click Allow pairing. Then click the pair icon next to it here.",
            "settings.secureHandshake": "Establishing a secure connection",
            "settings.compareCode": "Confirm that the other device shows the same code",
            "settings.waitingPeerConfirm": "Confirmed, waiting for the other device...",
            "settings.pairingFinalizing": "Finalizing pairing...",
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
            "settings.testAllConnections": "Test all available routes",
            "settings.relayPath": "Relay",
            "settings.testRouteRediscover": "Iroh latency testing requires rediscovering this device",
            "settings.connectionFailed": "Connection failed",
            "error.localServiceUnavailable": "TailSync's local service is unavailable. Please wait a moment and try again.",
            "error.localServiceSendFailed": "Could not send the request to TailSync's local service.",
            "error.localServiceNoResponse": "TailSync's local service did not respond.",
            "error.localServiceInvalidResponse": "TailSync's local service returned an invalid response.",
            "error.invalid_argument": "The request is invalid.",
            "error.not_found": "The requested item was not found.",
            "error.temporarily_busy": "TailSync is busy. Try again.",
            "error.storage_unavailable": "Storage is unavailable. Try again.",
            "error.unauthorized": "This action is not authorized.",
            "error.protocol_incompatible": "The connected device uses an incompatible protocol.",
            "error.internal": "TailSync could not complete this action.",
            "error.pairing_identity_mismatch": "The device identity does not match what this pairing expected. Pair it again or verify the fingerprint.",
            "error.pairing_window_closed": "This pairing request is no longer valid. Open pairing again on the other device and click Allow pairing.",
            "error.pairing_timed_out": "The other device did not answer the pairing request in time. Click Allow pairing on the other device and try again.",
            "error.pairing_connection_closed": "The pairing connection closed before the request completed. Click Allow pairing on the other device and try again.",
            "error.pairing_busy": "Another pairing is already in progress. Finish or cancel it and try again.",
            "error.pairing_cancelled": "The pairing request was cancelled.",
            "error.pairingWindowClosed": "The other device is not allowing pairing. Click Allow pairing on that device first.",
            "error.pairingHandshakeTimedOut": "The other device did not answer the pairing request. Update both devices and click Allow pairing on the other device first.",
            "error.pairingConnectionClosed": "The other device closed the pairing connection. Update both devices and click Allow pairing on the other device first.",
            "error.identityMismatch": "The device identity does not match the paired device. Pair it again or verify the fingerprint.",
        ],
        "zh-CN": [
            "history.loadError": "无法加载历史记录",
            "history.title": "历史记录",
            "history.search": "搜索历史...",
            "history.empty": "暂无记录",
            "history.restored": "历史已回溯至剪切板",
            "history.composite.syncIncomplete": "与 {peer} 的图文未完整应用；完整内容保留在历史中。",
            "history.composite.applyUnknown": "无法确认 {peer} 是否应用过图文；重试只保存历史。",
            "history.composite.partial": "{peer} 只发布了部分格式，请从历史手动恢复。",
            "history.composite.superseded": "图文已保存到 {peer} 的历史，当前剪贴板保留更新的内容。",

            "history.composite.formats": "格式",
            "history.composite.images": "图片",
            "history.composite.localOnly": "仅本机恢复",
            "history.composite.incomplete": "内容不完整",
            "history.composite.resourcesMissing": "部分资源未保存",
            "history.sourceLocal": "本机",
            "history.composite.copy.text": "仅复制文字",
            "history.composite.copy.menu": "选择复制格式",
            "history.composite.copy.png": "仅复制图片",
            "history.composite.moreActions": "更多操作",
            "history.composite.advanced": "高级",
            "history.composite.details": "详情",
            "history.composite.diag.formats": "格式",
            "history.composite.diag.representations": "表示数",
            "history.composite.diag.images": "图片数",
            "history.composite.diag.nativeItems": "原生条目",
            "history.composite.diag.nativeOnly": "仅原生",
            "history.composite.diag.completeness": "完整性",
            "history.composite.diag.restorable": "可恢复格式",
            "history.composite.diag.withheld": "保留格式",
            "history.composite.diag.yes": "是",
            "history.composite.diag.no": "否",
            "history.composite.diag.none": "无",
            "history.composite.diag.unavailable": "不可用",
            "history.composite.diag.unknown": "未知",
            "history.composite.completeness.complete": "完整",
            "history.composite.completeness.external_references": "含外部引用",
            "history.composite.completeness.missing_formats": "缺少格式",
            "history.composite.archiveImages": "补全网络图片（点击联网）",
            "history.composite.archiveImagesHint": "补全网络图片可能单独失败，不影响这条内容的其他恢复。",
            "history.composite.archiveDone": "图片已保存到本地历史",
            "history.composite.archivePartial": "部分图片未能保存，原始引用已保留",
            "history.composite.archiveBlocked": "引用不是允许的公开 HTTP(S) 地址",
            "history.composite.archiveInvalid": "不是支持的静态 PNG 图片",
            "history.composite.archiveLimit": "超出图片归档限制",
            "history.composite.archiveUnavailable": "图片不可用或请求超时",
            "history.composite.authorizeSource": "授权浏览器来源（另存授权文件）",
            "history.composite.importSourcePacket": "导入来源资源包（来自扩展）",
            "history.composite.revokeSource": "撤销来源授权",
            "history.composite.sourceAuthorized": "来源授权已保存到所选文件",
            "history.composite.sourceImported": "浏览器图片已保存到本地历史",
            "history.composite.sourcePartial": "部分浏览器图片未能导入，原始引用已保留",
            "history.composite.sourceUnavailable": "没有可授权的同源图片",
            "history.composite.sourceSaveFailed": "无法写入授权文件",
            "history.composite.sourceRevoked": "来源授权已撤销",

            // 稳定恢复结果码（方案 §4 R5-04）。服务端只返回码，不透传 Rust 英文，
            // 两个 UI 各自本地化同一失败。只有 published 可以显示为"已复制"。
            "history.restore.code.published": "已复制到剪贴板",
            "history.restore.code.publish_partial": "已保存到历史，但只发布了部分内容。可再次复制。",
            "history.restore.code.publish_unknown": "已保存到历史，但发布结果未知。可再次复制。",
            "history.restore.code.publish_superseded": "已保存到历史；剪贴板保留更新的内容。",
            "history.restore.code.native_only": "此条目只能在采集它的平台上恢复。",
            "history.restore.code.originals_incomplete": "部分原始格式未采集成功。请选择可用格式恢复。",
            "history.restore.code.resources_incomplete": "部分资源尚未保存到本地。可在菜单中仅复制文字，或仅复制已保存的图片。",
            "history.restore.code.no_single_image": "此条目包含多张图片，没有可单独复制的图片。请改选单张图片。",
            "history.restore.availableContent": "已复制已保存内容，{count} 张图片未保存",
            "history.restore.code.format_unavailable": "此条目不含该格式。",
            "history.restore.code.unsupported_format": "不支持该恢复格式。",
            "history.restore.recopy": "再次复制",

            "history.restore": "回溯",
            "history.delete": "删除",
            "history.clearAll": "清空所有记录",
            "history.confirmClear": "确定要删除所有未收藏记录吗？收藏内容会保留。",
            "history.deleteError": "无法删除这条历史记录",
            "history.favoriteProtected": "收藏内容只能在收藏窗口中删除",
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
            "history.storageUnavailable": "存储暂时不可用，部分新记录可能尚未保存。请保持 TailSync 运行，恢复当前存储位置的访问权限或可用空间，等待恢复后再迁移。",
            "history.fileSelectionSymlink": "复制的选择中包含符号链接，未加入文件历史。请改为复制实际文件。",
            "history.fileSelectionFolder": "复制的选择中包含文件夹或非文件项目，未加入文件历史。请改为复制普通文件。",
            "history.sending": "正在发送",
            "history.stopTransfer": "停止",
            "history.files": "个文件",
            "history.copyAll": "全部复制",
            "history.showMore": "展开剩余",
            "history.showLess": "收起",
            "history.incomplete": "未完成",
            "history.pin": "收藏",
            "history.unpin": "取消收藏",
            "favorites.open": "打开收藏",
            "favorites.title": "收藏",
            "favorites.emptyTitle": "还没有收藏",
            "favorites.emptyDescription": "长按历史记录即可收藏到这里",
            "history.windowPin": "历史窗口置顶",
            "history.windowUnpin": "取消历史窗口置顶",
            "history.syncExpired": "一条较早的剪贴板内容未发送到 {peer}，以避免覆盖对方较新的剪贴板内容。",
            "history.syncStalled": "向 {peer} 同步剪贴板受阻——连接尚未就绪。若持续出现，请重新连接该设备。",
            "history.syncShutdown": "TailSync 关闭时停止了向 {peer} 发送剪贴板内容。",
            "history.clipboardUnavailable": "剪贴板访问不可用。请检查辅助程序后重新复制。",
            "history.captureFailed": "本次复制未能完整采集。可读取的内容会尽可能保存在历史记录中。",
            "history.captureOriginUnknown": "本次复制已保存到历史，但来源无法确认，未自动同步。需要同步时，请从来源应用重新复制。",
            "history.captureNotShared": "本次复制已保存到历史，但无法准备同步内容。可从历史手动恢复；需要再次同步时，请从来源应用重新复制。",
            "history.deliveryRejected": "剪贴板内容未能投递到 {peer}。对方可能不支持其格式；请检查历史记录，按需重新复制。",
            "history.warningOverflow": "还有更多同步警告。请检查最近的历史记录和投递结果。",
            "history.captureNotSaved": "这次复制未能保存到历史（存储已满、不可用，或内容不受支持）。请检查存储状态后从来源应用重新复制，或尝试另一种复制格式。",
            "history.receiveNotSaved": "接收的内容未能保存到历史记录。请检查历史存储，再到发送设备重新复制该内容。",
            "history.syncDeliveryExpired": "向 {peer} 发送剪贴板内容已过期，未能完成发送。",
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
            "history.preview.documentTitle": "文稿",
            "history.preview.documentAccessibilitySummary": "{paragraphs} 个段落、{tables} 个表格、{images} 张图片、{placeholders} 张不可用图片",
            "history.preview.documentImage": "图片",
            "history.preview.documentTable": "表格",
            "history.preview.imageUnavailable": "图片未保存",
            "history.preview.imageBudget": "为保持预览内存预算，已省略该图片",
            "history.preview.imageCannotDisplay": "图片无法显示",
            "history.preview.imageAvailability.available": "图片可用",
            "history.preview.imageAvailability.missing_resource": "图片数据缺失",
            "history.preview.imageAvailability.missing_source": "文稿未提供图片字节",
            "history.preview.imageAvailability.external_reference": "图片指向已保存内容之外",
            "history.preview.imageAvailability.unsupported": "预览不支持此图片格式",
            "history.preview.imageAvailability.pixel_budget": "图片超出预览像素预算",
            "history.preview.partialTitle": "预览提示",
            "history.preview.partial.limit": "内容达到预览上限，仅显示一部分。",
            "history.preview.partial.pixelBudget": "预览达到像素预算，部分图片以占位显示。",
            "history.preview.partial.malformed": "部分标记格式错误或不受支持，未显示。",
            "history.preview.partial.missingImage": "部分图片缺失或引用外部内容，未加载。",
            "history.preview.partial.unverified": "预览内容与原始剪贴板格式的对应关系无法验证。",
            "history.preview.partial.other": "此文稿的一部分无法按原样显示。",
            "history.preview.partial.simplified": "部分标记或文字格式已简化为统一排版。",
            "history.preview.partial.nativeAnalysis": "原生正文无法分析，未显示其内容。",
            "history.preview.partial.stylesSimplified": "原生文字样式已简化为统一排版。",
            "history.preview.partial.nativeTableStructure": "原生文稿的表格结构不可用。",
            "history.preview.partial.nativePartial": "部分原生属性无法表示。",
            "history.preview.partial.nativeOther": "部分原生内容未显示。",
            "history.preview.partial.unsupportedMarkup": "部分标记不受支持，已按纯文本或简化方式显示。",
            "history.preview.partial.unsupportedImage": "部分图片无法解码，未显示。",
            "history.preview.noticeSummary": "预览提示：{reasons}",
            "history.preview.noticeSeparator": "、",
            "history.preview.noticeDetails": "详情",
            "history.preview.noticeDiagnostics": "预览简化项（{count}）",
            "history.preview.notice.missingImage": "图片未保存",
            "history.preview.notice.truncated": "内容被截断",
            "history.preview.notice.pixelBudget": "图片超出预览预算",
            "history.preview.notice.structureUnavailable": "结构未完整显示",
            "history.preview.notice.unverifiedSource": "格式转换关系未验证",
            "history.preview.notice.malformed": "标记未显示",
            "history.preview.notice.simplified": "格式已简化",
            "history.preview.notice.other": "其他差异",
            "history.preview.notice.unsupportedMarkup": "标记未按原样显示",
            "history.preview.notice.imageUnsupported": "图片未显示",
            "history.preview.notice.nativeUnavailable": "正文未显示",
            "history.preview.markdown": "MD",
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
            "history.preview.svgVisual": "预览",
            "history.preview.svgSource": "源码",
            "history.preview.svgRendering": "正在渲染预览…",
            "history.preview.svgTrustExternal": "信任外链",
            "history.preview.svgTrustedExternal": "已信任外链",
            "history.preview.svgTrustExternalHelp": "仅允许此预览通过 HTTPS 加载外部图片和字体。脚本保持禁用,非公网地址将被拒绝,但对应服务器将会看到该请求。",
            "history.preview.svgTrustAlertTitle": "允许加载外部资源?",
            "history.preview.svgTrustAlertMessage": "此预览将联系以下 HTTPS 域名。脚本保持禁用,但这些服务器将会看到你的请求。",
            "history.preview.svgTrustAlertDeny": "不允许",
            "history.preview.svgTrustAlertAllow": "允许",
            "history.preview.svgTrustRejectedTitle": "无法信任外链",
            "history.preview.svgTrustRejectedMessage": "此 SVG 引用了非 HTTPS 或非公网地址,这些地址已被拒绝,且无法为其启用外部资源:",
            "history.preview.svgBlockedContent": "此 SVG 包含活动或导航标记,仅显示源码。",
            "history.preview.svgTooLarge": "此 SVG 超出预览大小限制,仅显示源码。",
            "history.preview.svgRenderFailed": "SVG 预览渲染失败,已回退为源码显示。",
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
            "history.display.document": "文稿",
            "history.display.unknown": "内容待识别",
            "history.category.file": "文件",
            "settings.title": "设置",
            "settings.general": "通用",
            "settings.syncEnabled": "剪贴板同步",
            "settings.syncEnabledDescription": "把本机新复制的内容发送到已启用且已配对的设备",
            "settings.launchAtLogin": "开机时自动启动",
            "settings.launchAtLoginDescription": "登录后自动启动 TailSync",
            "settings.syncShortcut": "同步快捷键",
            "settings.syncShortcutDescription": "全局暂停或恢复剪贴板同步",
            "settings.historyShortcut": "历史记录快捷键",
            "settings.historyShortcutDescription": "从任意应用全局打开历史记录窗口",
            "settings.launchAtLoginApproval": "需要在系统设置中批准。",
            "settings.launchAtLoginOpenSettings": "打开系统设置",
            "settings.launchAtLoginError": "无法更新此设置。",
            "settings.historyShortcutRecord": "录制",
            "settings.shortcutRecord": "录制",
            "settings.shortcutChange": "更改",
            "settings.shortcutRecording": "请按下键组合…",
            "settings.shortcutSave": "保存",
            "settings.shortcutCancel": "取消",
            "settings.shortcutNone": "未设置",
            "settings.resetDefault": "恢复默认",
            "settings.clearShortcut": "清除快捷键",
            "settings.notifications": "通知",
            "settings.notificationsDescription": "远端设备发来内容时显示到达提示",
            "settings.progressBar": "进度条",
            "settings.progressBarDescription": "大文件传输过程中显示传输反馈",
            "settings.history": "历史记录",
            "settings.storage": "存储",
            "settings.storageDescription": "历史记录和文件传输数据",
            "settings.storageChange": "更改",
            "settings.storageMoving": "正在迁移...",
            "settings.storageQuota": "存储配额",
            "settings.storageQuotaDescription": "超出配额时自动清理最旧记录",
            "settings.storageDeleteOld": "删除旧数据",
            "settings.storageKeepOld": "保留",
            "settings.storageChanged": "存储位置已更改",
            "settings.storageUnavailable": "存储暂时不可用，部分新记录可能尚未保存。",
            "settings.storageRecoveryGuidance": "请保持 TailSync 运行，恢复当前存储位置的访问权限或可用空间，等待恢复后再迁移。",
            "settings.storageCapacityUnavailable": "存储离线，容量暂不可用",
            "settings.storageStatusUnconfirmed": "存储状态暂不可确认",
            "settings.limit": "历史记录上限",
            "settings.limitDescription": "最多保留 {value} 条剪贴板历史",
            "settings.autoSaveNetworkImages": "自动保存网络图片",
            "settings.qqImageAccess": "QQ 消息图片",
            "settings.qqImageAccessDescription": "QQ 只直接提供第一张图。允许读取当前账号的图片缓存后，可保存复制消息中的全部图片。",
            "settings.qqImageAccessChoose": "授权图片目录…",
            "settings.qqImageAccessChange": "更换图片目录…",
            "settings.qqImageAccessReady": "已保存图片目录授权。请重新复制 QQ 消息以保存图片；已有历史记录不会自动修改。",
            "settings.qqImageAccessInvalid": "请选择当前 QQ 账号的 nt_data/Pic 目录。先复制 QQ 消息，可自动定位到该图片目录。",
            "settings.qqImageAccessFailed": "未能保存图片目录授权。请重新选择 Pic 目录，并在系统对话框中允许访问。",
            "settings.autoSaveNetworkImagesDescription": "仅本机新复制内容会下载公开图片，消耗流量与存储；旧记录和接收内容不会自动下载。关闭会停止未完成的自动下载，已保存图片保留。旧记录可手动保存。",
            "settings.appearance": "外观",
            "settings.theme": "主题",
            "settings.themeDescription": "跟随系统外观或手动选择浅色/深色",
            "settings.themeSystem": "跟随系统",
            "settings.themeLight": "浅色",
            "settings.themeDark": "深色",
            "settings.language": "语言",
            "settings.languageDescription": "选择 TailSync 界面显示语言",
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
            "settings.saved": "设置已保存",
            "settings.saveFailed": "未能保存设置",
            "settings.loading": "连接后端中...",
            "settings.error": "连接失败",
            "settings.retry": "重试",
            "settings.updates": "更新",
            "settings.currentVersion": "当前版本",
            "settings.updateReady": "可检查签名更新",
            "settings.updateChecking": "正在检查更新...",
            "settings.updateAvailable": "TailSync {version} 已可用",
            "settings.updateCurrent": "TailSync 已是最新版",
            "settings.updateInstalling": "正在下载并验证签名更新...",
            "settings.updateInstalled": "更新已安装，重启 TailSync 后生效。",
            "settings.updateFailed": "无法检查更新",
            "settings.updateCheck": "检查更新",
            "settings.updateInstall": "安装更新",
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
            "connections.title": "连接与设备",
            "settings.manageConnections": "连接与设备",
            "settings.manageConnectionsDescription": "查看在线设备、管理网络发现与安全配对",
            "settings.openConnections": "打开",
            "settings.network": "网络",
            "settings.connectionMode": "连接方式",
            "settings.connectionModeDescription": "选择 TailSync 发现设备的方式并管理安全配对",
            "settings.modeAuto": "自动",
            "settings.modeTailscale": "Tailscale",
            "settings.modeLan": "局域网",
            "settings.modeIroh": "Iroh",
            "settings.localDevice": "本机设备",
            "settings.identityLoading": "正在加载设备身份…",
            "settings.copyPublicKey": "复制公钥",
            "notice.notificationGap": "有 {count} 条较早的通知已不可读取，请查看历史记录和传输状态以确认当前情况。",
            "settings.pairDevice": "配对设备",
            "settings.pendingPairing": "配对尚未完成。请在两端打开配对并重新核对验证码，或遗忘此设备。",
            "settings.pendingPeerCompletion": "本机已信任此设备，对端完成情况待确认。请在两端重新配对以恢复。",
            "settings.resumePairing": "重新配对",
            "settings.pendingStoreUnavailable": "无法读取配对恢复记录。请使用兼容版本恢复。",
            "settings.peerHostname": "设备名称",
            "settings.peerPublicKey": "远端公钥（Base64）",
            "settings.pair": "配对",
            "settings.allowPairing": "允许配对",
            "settings.closePairing": "关闭",
            "settings.pairingClosed": "当前关闭",
            "settings.remotePairing": "远程 Iroh 配对",
            "settings.remotePairingDescription": "两台设备不在同一网络时，可使用短时有效的链接",
            "settings.createRemoteInvite": "创建邀请链接",
            "settings.createRemoteInviteDescription": "在本机创建一次性链接，再发送给另一台设备",
            "settings.remoteInviteLink": "远程配对邀请链接",
            "settings.copyInvite": "复制邀请链接",
            "settings.copied": "已复制",
            "settings.cancelRemoteInvite": "取消邀请",
            "settings.remoteInviteCopyFailed": "无法复制邀请链接",
            "settings.remoteInviteExpires": "将在 {seconds} 秒后失效",
            "settings.useRemoteInvite": "使用邀请链接",
            "settings.useRemoteInviteDescription": "粘贴另一台设备发来的链接，开始安全配对",
            "settings.remoteInvitePlaceholder": "tailsync://pair/v1/...",
            "settings.checkInvite": "检查链接",
            "settings.startRemotePairing": "开始配对",
            "settings.remoteInviteValid": "链接有效期还剩 {seconds} 秒",
            "settings.waitingPairing": "等待另一台设备",
            "settings.pairingInstruction": "请先在另一台设备点击“允许配对”，再点击此处该设备旁的配对图标。",
            "settings.secureHandshake": "正在建立安全连接",
            "settings.compareCode": "请确认另一台设备显示相同验证码",
            "settings.waitingPeerConfirm": "已确认，等待对端确认...",
            "settings.pairingFinalizing": "正在完成配对...",
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
            "settings.testAllConnections": "依次测试所有可用路径",
            "settings.relayPath": "中继",
            "settings.testRouteRediscover": "Iroh 延迟测试需要先重新发现该设备",
            "settings.connectionFailed": "连接失败",
            "error.localServiceUnavailable": "TailSync 本地服务暂时不可用，请稍后重试。",
            "error.localServiceSendFailed": "无法向 TailSync 本地服务发送请求。",
            "error.localServiceNoResponse": "TailSync 本地服务没有响应。",
            "error.localServiceInvalidResponse": "TailSync 本地服务返回了无效响应。",
            "error.invalid_argument": "请求参数无效。",
            "error.not_found": "未找到请求的项目。",
            "error.temporarily_busy": "TailSync 正忙，请重试。",
            "error.storage_unavailable": "存储不可用，请重试。",
            "error.unauthorized": "无权执行此操作。",
            "error.protocol_incompatible": "连接的设备使用不兼容的协议。",
            "error.internal": "TailSync 无法完成此操作。",
            "error.pairing_identity_mismatch": "设备身份与本次配对预期的身份不匹配。请重新配对或核对指纹。",
            "error.pairing_window_closed": "该配对请求已失效。请在另一台设备上重新打开配对并点击“允许配对”。",
            "error.pairing_timed_out": "另一台设备未及时响应配对请求。请在另一台设备上点击“允许配对”后重试。",
            "error.pairing_connection_closed": "配对连接在请求完成前已关闭。请在另一台设备上点击“允许配对”后重试。",
            "error.pairing_busy": "已有配对正在进行。请先完成或取消后再试。",
            "error.pairing_cancelled": "配对请求已取消。",
            "error.pairingWindowClosed": "另一台设备尚未允许配对，请先在该设备上点击“允许配对”。",
            "error.pairingHandshakeTimedOut": "另一台设备未响应配对请求。请先将两端更新到最新版，并在另一台设备上点击“允许配对”。",
            "error.pairingConnectionClosed": "另一台设备关闭了配对连接。请先将两端更新到最新版，并在另一台设备上点击“允许配对”。",
            "error.identityMismatch": "设备身份与已配对设备不匹配。请重新配对或核对指纹。",
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
