import AppKit
import Foundation
import SwiftUI

private enum HistoryPendingSelection { case first, last }

private struct HistoryDateBounds {
    let start: Date?
    let end: Date?
}

enum HistoryBatchPresentation {
    static func isBatchStart(_ entry: HistoryEntry, in entries: [HistoryEntry]) -> Bool {
        guard let batchId = entry.batch_id,
              let index = entries.firstIndex(where: { $0.id == entry.id }) else { return false }
        let visibleCount = entries.filter { $0.batch_id == batchId }.count
        let hasMultipleFiles = max(entry.batch_count ?? 0, entry.batch_total ?? 0, visibleCount) > 1
        // Keep incomplete-batch warnings even if only one member is available.
        guard hasMultipleFiles || entry.batch_status != "complete" else { return false }
        return index == 0 || entries[index - 1].batch_id != batchId
    }
}

enum HistoryFavoriteMutationPolicy {
    static func logicalItemKey(_ entry: HistoryEntry) -> String {
        entry.batch_id.map { "batch:\($0)" } ?? "entry:\(entry.id)"
    }

    static func reconcile(
        _ entries: [HistoryEntry],
        pendingFavorites: [String: Bool]
    ) -> [HistoryEntry] {
        entries.map { entry in
            var reconciled = entry
            if let pendingFavorite = pendingFavorites[logicalItemKey(entry)] {
                reconciled.pinned = pendingFavorite
            }
            return reconciled
        }
    }
}

struct HistoryQuery: Equatable, Sendable {
    let keyword: String?
    let category: String?
    let startTime: String?
    let endTime: String?
    let limit: Int
    let offset: Int
    let collection: String
}

struct HistoryViewDependencies: Sendable {
    var load: @Sendable (HistoryQuery) async throws -> [HistoryEntry]
    var restore: @Sendable (Int64) async throws -> Void
    var delete: @Sendable (Int64, String) async throws -> Void = { id, collection in
        if collection == "favorites" { try await ApiClient.shared.deleteFavoriteEntry(id: id) }
        else { try await ApiClient.shared.deleteEntry(id: id) }
    }
    var restoreFormat: @Sendable (Int64, String) async throws -> Void = { try await ApiClient.shared.restoreEntry(id: $0, format: $1) }
    var archiveImages: @Sendable (Int64) async throws -> ContractArchiveEntryImagesReport = { try await ApiClient.shared.archiveEntryImages(id: $0) }
    var sourceContextGrant: @Sendable (Int64) async throws -> ContractSourceContextGrantReport = { try await ApiClient.shared.sourceContextGrantCreate(id: $0) }
    var sourceContextImport: @Sendable (Int64, String) async throws -> ContractSourceContextImportReport = { try await ApiClient.shared.sourceContextImport(id: $0, path: $1) }
    var sourceContextRevoke: @Sendable (String) async throws -> ContractSourceContextRevokeReport = { try await ApiClient.shared.sourceContextGrantRevoke(grantId: $0) }
    var restoreBatch: @Sendable (String) async throws -> Void = {
        try await ApiClient.shared.restoreFileBatch($0)
    }
    var restoreGuarded: (@Sendable (Int64, String?, Int64, ContractHistoryIdentity?) async throws -> Void)? = nil
    var restoreReported: (@Sendable (Int64, String?, Int64, ContractHistoryIdentity?) async throws -> ContractHistoryRestoreReport)? = nil
    var usesRuntimeUpdates = true

    static let live = HistoryViewDependencies(
        load: { query in
            try await ApiClient.shared.getHistory(
                keyword: query.keyword, category: query.category,
                startTime: query.startTime, endTime: query.endTime,
                limit: query.limit, offset: query.offset, collection: query.collection
            )
        },
        restore: { try await ApiClient.shared.restoreEntry(id: $0) },
        restoreGuarded: { id, format, originalID, identity in
            try await ApiClient.shared.restoreEntry(id: id, format: format, originalID: originalID, identity: identity)
        },
        restoreReported: { id, format, originalID, identity in
            try await ApiClient.shared.restoreEntryWithReport(id: id, format: format, originalID: originalID, identity: identity)
        }
    )
}

struct HistoryView: View {
    let collection: String
    private let visibilityNotification: Notification.Name
    private let dependencies: HistoryViewDependencies

    init(
        collection: String = "all",
        dependencies: HistoryViewDependencies = .live,
        keyboardSession: HistoryKeyboardSession? = nil
    ) {
        self.collection = collection
        self.dependencies = dependencies
        _keyboard = StateObject(wrappedValue: keyboardSession ?? HistoryKeyboardSession())
        visibilityNotification = .tailSyncHistoryCollectionVisibilityChanged(
            for: collection
        )
    }

    @ObservedObject private var loc = Loc.shared
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize
    @State private var entries: [HistoryEntry] = []
    @State private var keyword = ""
    @State private var selectedCategory = "all"
    @State private var selectedDateFilter = HistoryDateFilter.all
    @State private var customStartDate = Calendar.autoupdatingCurrent.startOfDay(for: Date())
    @State private var customEndDate = Calendar.autoupdatingCurrent.startOfDay(for: Date())
    @State private var page = 0
    @State private var hasNext = false
    @State private var isLoading = false
    @State private var windowIsVisible = true
    @State private var restoredId: Int64? = nil
    // Previewing is independent of restoration: selection chooses a row,
    // Space opens it in the reusable preview window, and double-click retains
    // the established clipboard-restore gesture.
    @StateObject private var keyboard: HistoryKeyboardSession
    private var focusedEntryId: Int64? {
        get { keyboard.selectedID }
        nonmutating set { keyboard.selectedID = newValue }
    }
    @State private var isRestoring = false
    @State private var isDeleting = false
    @State private var loadedQuery: HistoryQuery? = nil
    @State private var previewFollowTask: Task<Void, Never>? = nil
    @State private var interactionGeneration = 0
    @State private var queryTask: Task<Void, Never>? = nil
    @State private var loadTask: Task<Void, Never>? = nil
    @State private var pendingLoadPage: Int? = nil
    @State private var queryGeneration = 0
    @State private var pendingPageSelection: HistoryPendingSelection? = nil
    @State private var errorMsg: String? = nil
    @State private var lastVersion: UInt64 = 0
    @State private var runtimeRevision: UInt64 = 0
    @State private var daemonOnline = false
    @State private var fileProgress: ApiClient.FileProgress? = nil
    @State private var progressBarEnabled = true
    @State private var showingClearAlert = false
    @State private var lastSourceGrantId: String? = nil
    @State private var loadGeneration = 0
    @State private var supportedCategories: Set<String> = []
    @State private var multipleLabelsSupported = false
    @State private var dateRangeFilteringSupported = false
    @State private var historyCapabilitiesChecked = false
    @State private var historyCapabilitiesLoading = false
    @State private var unresolvedMigrationCount = 0
    @State private var syncWarning: String? = nil
    @State private var syncWarningTask: Task<Void, Never>? = nil
    // Persistent while storage is unavailable, unlike `syncWarning` which is a
    // transient toast. A user with an open History/Favorites window must be able
    // to see that new items may not be saved, not just after an unrelated copy.
    @State private var storageWarning: String? = nil
    @State private var actionMessage: String? = nil
    @State private var retryRestoreAction: (() -> Void)? = nil
    @State private var actionMessageTask: Task<Void, Never>? = nil
    @State private var restoreFeedbackTask: Task<Void, Never>? = nil
    @State private var expandedBatchIds: Set<String> = []
    @State private var favoriteMutations: [String: Bool] = [:]

    private let pageSize = 30
    private let collapsedBatchFileLimit = 2
    private let knownCategories = ["text", "website", "code", "path", "image", "file"]

    private var activeTheme: TailSyncThemeSelection {
        TailSyncThemeSelection(
            storedValue: loc.colorTheme,
            catalogue: loc.resolvedV2Themes,
            reduceTransparency: loc.reduceTransparency,
            interfaceScale: TailSyncThemeAccessibilityPolicy.interfaceScale(for: dynamicTypeSize)
        )
    }

    private var isFavoritesCollection: Bool { collection == "favorites" }

    private var previewOwner: HistoryPreviewOwner {
        HistoryPreviewOwner(collection: collection)
    }

    private var palette: TailSyncThemePalette {
        activeTheme.palette(for: colorScheme)
    }

    private var categories: [String] {
        ["all"] + knownCategories.filter { supportedCategories.contains($0) }
    }

    private var categoryFilteringSupported: Bool {
        !supportedCategories.isEmpty
    }

    private var displayedEntries: [HistoryEntry] {
        var batchPositions: [String: Int] = [:]
        return entries.filter { entry in
            guard let batchId = entry.batch_id,
                  batchCount(entry) > collapsedBatchFileLimit,
                  !expandedBatchIds.contains(batchId) else { return true }
            let position = batchPositions[batchId, default: 0]
            batchPositions[batchId] = position + 1
            return position < collapsedBatchFileLimit
        }
    }

    private func batchCount(_ entry: HistoryEntry) -> Int {
        if let count = entry.batch_count ?? entry.batch_total { return count }
        guard let batchId = entry.batch_id else { return 1 }
        return max(1, entries.filter { $0.batch_id == batchId }.count)
    }

    private func logicalItemKey(_ entry: HistoryEntry) -> String {
        HistoryFavoriteMutationPolicy.logicalItemKey(entry)
    }

    private func batchTitle(_ entry: HistoryEntry) -> String {
        let count = batchCount(entry)
        let total = entry.batch_total ?? count
        let value = entry.batch_status == "incomplete" && count != total
            ? "\(count)/\(total)"
            : "\(count)"
        return "\(value) \(Loc.t("history.files"))"
    }

    var body: some View {
        VStack(spacing: 0) {
            HistoryFilterBar(
                keyword: $keyword,
                selectedCategory: $selectedCategory,
                selectedDateFilter: $selectedDateFilter,
                customStartDate: $customStartDate,
                customEndDate: $customEndDate,
                categories: categories,
                categoryFilteringSupported: categoryFilteringSupported,
                dateRangeFilteringSupported: dateRangeFilteringSupported,
                daemonOnline: daemonOnline,
                onSubmit: { page = 0; load(targetPage: 0) },
                onFilterChanged: {
                    restoredId = nil
                    interactionGeneration &+= 1
                    queryTask?.cancel()
                    queryTask = nil
                    pendingPageSelection = nil
                    page = 0
                    load(targetPage: 0)
                },
                isFavoritesCollection: isFavoritesCollection,
                onOpenFavorites: { AppDelegate.showFavorites() },
                keyboardSession: keyboard,
                onQueryChanged: scheduleSearch
            )
            .padding(.horizontal, 12).padding(.top, 8)

            if unresolvedMigrationCount > 0 {
                HStack(spacing: 7) {
                    Image(systemName: "exclamationmark.triangle")
                        .foregroundColor(palette.warningColor)
                    Text("\(Loc.t("history.migrationWarningPrefix")) \(unresolvedMigrationCount) \(Loc.t("history.migrationWarningSuffix"))")
                        .font(activeTheme.readingFont(size: 10))
                        .foregroundColor(palette.secondaryColor)
                        .fixedSize(horizontal: false, vertical: true)
                    Spacer(minLength: 0)
                }
                .padding(.horizontal, 10)
                .padding(.vertical, 7)
                .background(palette.warningSoftColor)
                .clipShape(RoundedRectangle(cornerRadius: activeTheme.metrics.controlRadius, style: .continuous))
                .overlay {
                    RoundedRectangle(cornerRadius: activeTheme.metrics.controlRadius, style: .continuous)
                        .stroke(palette.warningColor.opacity(0.3), lineWidth: 1)
                }
                .padding(.horizontal, 12)
                .padding(.top, 6)
                .accessibilityElement(children: .combine)
            }

            if daemonOnline, let storageWarning {
                HStack(spacing: 7) {
                    Image(systemName: "externaldrive.badge.exclamationmark")
                        .foregroundColor(palette.warningColor)
                    Text(storageWarning)
                        .font(activeTheme.readingFont(size: 10))
                        .foregroundColor(palette.secondaryColor)
                        .fixedSize(horizontal: false, vertical: true)
                    Spacer(minLength: 0)
                }
                .padding(.horizontal, 10)
                .padding(.vertical, 7)
                .background(palette.warningSoftColor)
                .clipShape(RoundedRectangle(cornerRadius: activeTheme.metrics.controlRadius, style: .continuous))
                .overlay {
                    RoundedRectangle(cornerRadius: activeTheme.metrics.controlRadius, style: .continuous)
                        .stroke(palette.warningColor.opacity(0.3), lineWidth: 1)
                }
                .padding(.horizontal, 12)
                .padding(.top, 6)
                .accessibilityElement(children: .combine)
                .accessibilityLabel(storageWarning)
            }

            // List
            if isLoading && entries.isEmpty {
                Spacer()
                ProgressView().controlSize(.small)
                Spacer()
            } else if let errorMsg {
                Spacer()
                VStack(spacing: 8) {
                    Image(systemName: "exclamationmark.triangle")
                        .font(.system(size: 28))
                        .foregroundColor(palette.warningColor)
                    Text(errorMsg)
                        .font(activeTheme.readingFont(size: 13))
                        .foregroundColor(palette.secondaryColor)
                }
                Spacer()
            } else if entries.isEmpty {
                Spacer()
                VStack(spacing: 8) {
                    if isFavoritesCollection {
                        Image(systemName: "star")
                            .font(.system(size: 32))
                            .foregroundColor(palette.tertiaryColor)
                    } else if let image = loc.themeAssetImages["emptyState"] {
                        Image(nsImage: image).resizable().aspectRatio(contentMode: .fit).frame(width: 64, height: 64)
                    } else {
                        Image(systemName: "doc.on.clipboard")
                            .font(.system(size: 32))
                            .foregroundColor(palette.tertiaryColor)
                    }
                    Text(Loc.t(isFavoritesCollection ? "favorites.emptyTitle" : "history.empty"))
                        .font(activeTheme.displayFont(size: 15))
                        .foregroundColor(palette.secondaryColor)
                    if isFavoritesCollection {
                        Text(Loc.t("favorites.emptyDescription"))
                            .font(activeTheme.readingFont(size: 11))
                            .foregroundColor(palette.tertiaryColor)
                    }
                }
                Spacer()
            } else {
                ScrollViewReader { scroll in
                List {
                    ForEach(displayedEntries) { entry in
                        VStack(spacing: 4) {
                        if isBatchStart(entry), let batchId = entry.batch_id {
                            HStack(spacing: 6) {
                                Image(systemName: "folder")
                                Text(batchTitle(entry))
                                Spacer()
                                if entry.batch_status == "complete" {
                                    Button {
                                        restoreBatch(batchId)
                                    } label: {
                                        Label(Loc.t("history.copyAll"), systemImage: "doc.on.doc")
                                    }
                                    .buttonStyle(.borderless)
                                } else {
                                    Text(Loc.t("history.incomplete"))
                                        .foregroundColor(palette.warningColor)
                                }
                                if batchCount(entry) > collapsedBatchFileLimit {
                                    Button {
                                        if expandedBatchIds.contains(batchId) {
                                            expandedBatchIds.remove(batchId)
                                        } else {
                                            expandedBatchIds.insert(batchId)
                                        }
                                    } label: {
                                        Label(
                                            expandedBatchIds.contains(batchId)
                                                ? Loc.t("history.showLess")
                                                : "\(Loc.t("history.showMore")) (\(batchCount(entry) - collapsedBatchFileLimit))",
                                            systemImage: expandedBatchIds.contains(batchId)
                                                ? "chevron.up"
                                                : "chevron.down"
                                        )
                                    }
                                    .buttonStyle(.borderless)
                                }
                            }
                            .font(.caption)
                            .foregroundColor(palette.secondaryColor)
                            .padding(.horizontal, 6)
                        }
                        HistoryInteractiveRow(
                            entry: entry,
                            owner: collection,
                            isRestored: restoredId == entry.id,
                            isFocused: focusedEntryId == entry.id,
                            showsMultipleLabels: multipleLabelsSupported,
                            previewRequest: historyPreviewRequest(
                                focusedId: entry.id,
                                entries: entries,
                                expandedBatchIds: expandedBatchIds
                            ),
                            onSelect: {
                                guard !isRestoring, !isLoading, queryTask == nil else { return }
                                focusedEntryId = entry.id
                                keyboard.focusList()
                            },
                            onRestore: {
                                restore(entry.id)
                            },
                            onRestoreFormat: { format in
                                performRestore(restoredEntryID: entry.id, dismiss: false) { resolvedID in
                                    if let reported = dependencies.restoreReported {
                                        return try await reported(resolvedID ?? entry.id, format, entry.id, entry.composite?.identity)
                                    }
                                    if let guarded = dependencies.restoreGuarded {
                                        try await guarded(resolvedID ?? entry.id, format, entry.id, entry.composite?.identity)
                                    } else { try await dependencies.restoreFormat(resolvedID ?? entry.id, format) }
                                    return nil
                                }
                            },
                            onArchiveImages: {
                                archiveImages(entry.id)
                            },
                            onAuthorizeSource: {
                                authorizeSourceContext(entry.id)
                            },
                            onImportSourcePacket: {
                                importSourcePacket(entry.id)
                            },
                            onRevokeSource: lastSourceGrantId == nil ? nil : {
                                revokeSourceContext()
                            },
                            onDelete: {
                                delete(entry)
                            },
                            onFavorite: {
                                toggleFavorite(entry)
                            },
                            onPreview: { request in
                                guard !isRestoring, !isDeleting, resultsAreCurrent else { return }
                                previewFollowTask?.cancel()
                                HistoryPreviewWindowController.shared.present(
                                    request,
                                    owner: previewOwner,
                                    takeFocus: false
                                )
                            },
                            onClosePreview: {
                                HistoryPreviewWindowController.shared.close(
                                    ifOwnedBy: previewOwner
                                )
                            },
                            isPreviewVisible: {
                                HistoryPreviewWindowController.shared.isPreviewVisible(
                                    for: previewOwner
                                )
                            }
                        )
                        }
                        .id(entry.id)
                        .listRowBackground(palette.surfaceColor)
                        .listRowSeparatorTint(palette.dividerColor)
                    }
                }
                .listStyle(.plain)
                .scrollContentBackground(.hidden)
                .onChange(of: keyboard.selectedID) { id in
                    guard let id else { return }
                    // Scroll only the minimum needed to keep the row visible, and
                    // animate it so the list glides instead of jumping. Once the
                    // row reaches an edge it stays put and the list slides under
                    // it, so the selection never lurches.
                    withAnimation(loc.reduceMotion ? nil : .easeOut(duration: 0.15)) {
                        scroll.scrollTo(id, anchor: nil)
                    }
                }
                .animation(loc.reduceMotion ? nil : .spring(response: 0.35, dampingFraction: 0.9), value: entries.count)
                }
            }

            // Pagination + clear
            HStack(spacing: 12) {
                Button {
                    let target = page - 1
                    load(targetPage: target)
                } label: { Image(systemName: "chevron.left") }
                    .disabled(page == 0 || isRestoring || !resultsAreCurrent)
                Text("\(page + 1)").font(.caption).foregroundColor(palette.tertiaryColor).monospacedDigit()
                Button {
                    let target = page + 1
                    load(targetPage: target)
                } label: { Image(systemName: "chevron.right") }
                    .disabled(!hasNext || isRestoring || !resultsAreCurrent)
                Spacer()
                if !isFavoritesCollection && !entries.isEmpty {
                    Button(Loc.t("history.clearAll"), role: .destructive) { showingClearAlert = true }
                        .font(.caption).buttonStyle(.plain)
                }
            }
            .padding(.vertical, 8)
            .padding(.horizontal, 12)
            .background(palette.surfaceColor)
            .overlay(alignment: .top) {
                Rectangle().fill(palette.dividerColor).frame(height: activeTheme.builtin == .highContrast ? 2 : 1)
            }
            .alert(Loc.t("history.confirmClear"), isPresented: $showingClearAlert) {
                Button(Loc.t("common.cancel"), role: .cancel) {}
                Button(Loc.t("history.clearAll"), role: .destructive) { clearAll() }
            }
        }
        .background(HistoryKeyboardListResponder(session: keyboard).frame(width: 0, height: 0))
        .onChange(of: keyboard.selectedID) { _ in followPreviewSelection() }
        .onAppear {
            keyboard.onCommand = handleKeyboard
            HistoryPreviewWindowController.shared.registerKeyboardHandlers(
                for: previewOwner,
                onRestore: { id, keepOpen in restore(id, dismiss: !keepOpen) },
                onReturn: { keyboard.focusList() }
            )
            if !isFavoritesCollection {
                HistoryWindowController.shared.onKeyboardActivation = {
                    interactionGeneration &+= 1
                    restoredId = nil
                    keyboard.selectedID = displayedEntries.first?.id
                    keyboard.focusSearch()
                }
            }
            keyboard.focusSearch(selectAll: false)
            load()
            if dependencies.usesRuntimeUpdates {
                loadProgressPreference()
                loadHistoryCapabilities()
                loadMigrationDiagnostics()
            }
        }
        .task(id: windowIsVisible) {
            guard windowIsVisible, dependencies.usesRuntimeUpdates else { return }
            await runtimeSnapshotLoop(forceRefresh: true)
        }
        .onReceive(NotificationCenter.default.publisher(
            for: visibilityNotification
        )) { notification in
            guard let isVisible = notification.userInfo?["visible"] as? Bool else { return }
            let wasVisible = windowIsVisible
            windowIsVisible = isVisible
            if !isVisible {
                interactionGeneration &+= 1
                previewFollowTask?.cancel()
                // An occluded or miniaturized window keeps this view alive, so
                // hiding it - not only `onDisappear` - must release the read.
                cancelPendingQueries()
                HistoryPreviewWindowController.shared.close(ifOwnedBy: previewOwner)
            } else if !wasVisible, let requestedPage = pendingLoadPage {
                load(targetPage: requestedPage)
            }
        }
        .onReceive(NotificationCenter.default.publisher(for: .NSCalendarDayChanged)) { _ in
            reloadActiveDateFilter()
        }
        .onReceive(NotificationCenter.default.publisher(for: .NSSystemTimeZoneDidChange)) { _ in
            reloadActiveDateFilter()
        }
        .onReceive(NotificationCenter.default.publisher(for: .tailSyncSettingsChanged)) { notification in
            guard let settings = notification.object as? AppSettings else { return }
            progressBarEnabled = settings.progress_bar_enabled
            if !progressBarEnabled { fileProgress = nil }
        }
        .onDisappear {
            keyboard.onCommand = nil
            if !isFavoritesCollection { HistoryWindowController.shared.onKeyboardActivation = nil }
            interactionGeneration &+= 1
            cancelPendingQueries()
            previewFollowTask?.cancel()
            HistoryPreviewWindowController.shared.registerKeyboardHandlers(
                for: previewOwner, onRestore: nil, onReturn: nil
            )
            HistoryThumbnailCache.removeAll()
            restoreFeedbackTask?.cancel()
            restoreFeedbackTask = nil
            syncWarningTask?.cancel()
            syncWarningTask = nil
            actionMessageTask?.cancel()
            actionMessageTask = nil
            focusedEntryId = nil
            HistoryPreviewWindowController.shared.close(ifOwnedBy: previewOwner)
        }
        .overlay(alignment: .bottom) {
            VStack(spacing: 6) {
                if let actionMessage {
                    VStack(spacing: 6) {
                    Text(actionMessage)
                        .font(activeTheme.readingFont(size: 11))
                        .foregroundColor(palette.toastTextColor)
                        .multilineTextAlignment(.center)
                        .padding(.horizontal, 12)
                        .padding(.vertical, 8)
                        .background(palette.toastColor)
                        .clipShape(RoundedRectangle(cornerRadius: activeTheme.metrics.controlRadius, style: .continuous))
                        .shadow(color: .black.opacity(0.16), radius: 8, y: 3)
                        .accessibilityLabel(actionMessage)
                    if let retryRestoreAction {
                        Button(Loc.t("history.restore.recopy"), action: retryRestoreAction)
                    }
                    }
                }
                if let syncWarning {
                    Text(syncWarning)
                        .font(activeTheme.readingFont(size: 11))
                        .foregroundColor(palette.primaryColor)
                        .multilineTextAlignment(.center)
                        .padding(.horizontal, 12)
                        .padding(.vertical, 8)
                        .background(palette.surfaceColor)
                        .clipShape(RoundedRectangle(cornerRadius: activeTheme.metrics.controlRadius, style: .continuous))
                        .shadow(color: .black.opacity(0.16), radius: 8, y: 3)
                        .accessibilityLabel(syncWarning)
                }
                if let p = fileProgress, p.active {
                    VStack(spacing: 5) {
                        ProgressView(value: Double(p.sent), total: Double(max(p.total, 1))).progressViewStyle(.linear)
                        HStack(spacing: 8) {
                            Text("\(min(p.completedFiles + 1, p.totalFiles))/\(p.totalFiles)")
                            Text(p.name).lineLimit(1).truncationMode(.middle)
                            if !p.device.isEmpty { Text(p.device) }
                            Spacer(minLength: 4)
                            Text("\(ByteCountFormatter.string(fromByteCount: Int64(p.speedBytesPerSecond), countStyle: .file))/s")
                            if p.canStop && !p.batchId.isEmpty {
                                Button {
                                    Task { await ApiClient.shared.cancelFileBatch(p.batchId) }
                                } label: {
                                    Label(Loc.t("history.stopTransfer"), systemImage: "stop.fill")
                                }
                                .buttonStyle(.bordered)
                                .controlSize(.small)
                            }
                        }
                        .font(.caption2)
                        .foregroundColor(palette.secondaryColor)
                    }
                    .tint(palette.infoColor)
                    .padding(.horizontal, 14).padding(.vertical, 8)
                    .background(palette.raisedColor)
                    .clipShape(RoundedRectangle(cornerRadius: activeTheme.metrics.cardRadius, style: .continuous))
                    .padding(.horizontal, 8)
                }
                if restoredId != nil {
                    Text(Loc.t("history.restored"))
                        .font(.caption)
                        .foregroundColor(palette.toastTextColor)
                        .padding(.horizontal, 12).padding(.vertical, 6)
                        .background(palette.toastColor)
                        .clipShape(Capsule())
                        .padding(.bottom, 8)
                }
            }
        }
        // The window background (window colour + custom background image +
        // scrim) is owned by tailSyncThemed(); painting an opaque root
        // background here would cover it.
        .tailSyncThemed()
    }

    /// Cancels the in-flight page read and invalidates its result. Only the read
    /// that is still current may clear the loading flag or the task handle.
    private func cancelPendingLoad() {
        loadTask?.cancel()
        loadTask = nil
        loadGeneration &+= 1
        isLoading = false
    }

    private func cancelPendingQueries() {
        queryTask?.cancel()
        queryTask = nil
        queryGeneration &+= 1
        cancelPendingLoad()
    }

    private func load(targetPage: Int? = nil, clearExisting: Bool = true) {
        // Runtime refreshes must not supersede a typed query or keyboard page turn.
        if !clearExisting && (queryTask != nil || isLoading || isRestoring) { return }
        let requestedPage = targetPage ?? pendingLoadPage ?? page
        pendingLoadPage = requestedPage
        guard windowIsVisible else { return }
        // A replaced read must not keep the daemon working on a page nobody wants.
        loadTask?.cancel()
        loadGeneration += 1
        let generation = loadGeneration
        let requestedQuery = historyQuery(page: requestedPage)
        isLoading = true
        errorMsg = nil
        let task = Task { @MainActor in
            defer {
                if generation == loadGeneration {
                    loadTask = nil
                    pendingLoadPage = nil
                }
            }
            do {
                let result = try await dependencies.load(requestedQuery)
                guard generation == loadGeneration, !Task.isCancelled, windowIsVisible else { return }
                entries = HistoryFavoriteMutationPolicy.reconcile(
                    Array(result.prefix(pageSize)),
                    pendingFavorites: favoriteMutations
                )
                hasNext = result.count > pageSize
                page = requestedPage
                loadedQuery = requestedQuery
                if let pendingPageSelection {
                    keyboard.selectedID = pendingPageSelection == .first
                        ? displayedEntries.first?.id
                        : displayedEntries.last?.id
                    self.pendingPageSelection = nil
                } else {
                    pruneSelection()
                }
            } catch is CancellationError {
                // A superseded, hidden, or closed read is not a load failure.
                guard generation == loadGeneration else { return }
            } catch {
                guard generation == loadGeneration else { return }
                pendingPageSelection = nil
                showActionMessage(Loc.t("history.loadError"))
            }
            if generation == loadGeneration {
                isLoading = false
                followPreviewSelection()
            }
        }
        loadTask = task
    }

    private func loadHistoryCapabilities() {
        guard !historyCapabilitiesLoading else { return }
        historyCapabilitiesLoading = true
        Task {
            do {
                let capabilities = try await ApiClient.shared.getHistoryCapabilities()
                supportedCategories = Set(
                    capabilities?.categories.filter { knownCategories.contains($0) } ?? []
                )
                multipleLabelsSupported = capabilities?.multipleLabels ?? false
                dateRangeFilteringSupported = capabilities?.dateRangeFilter ?? false
                if selectedCategory != "all" && !supportedCategories.contains(selectedCategory) {
                    selectedCategory = "all"
                }
                if !dateRangeFilteringSupported && selectedDateFilter != .all {
                    selectedDateFilter = .all
                }
                historyCapabilitiesChecked = true
            } catch {
                // Keep the capability unchecked so a daemon that is still starting is retried.
            }
            historyCapabilitiesLoading = false
        }
    }

    private func loadProgressPreference() {
        Task {
            if let settings = try? await ApiClient.shared.getSettings() {
                progressBarEnabled = settings.progress_bar_enabled
                if !progressBarEnabled { fileProgress = nil }
            }
        }
    }

    @MainActor
    private func runtimeSnapshotLoop(forceRefresh: Bool) async {
        var shouldRefreshImmediately = forceRefresh
        while !Task.isCancelled {
            // A resumed window should get a fresh consolidated snapshot right
            // away instead of waiting for the previous revision to change.
            let sinceRevision = shouldRefreshImmediately ? 0 : runtimeRevision
            shouldRefreshImmediately = false
            guard let snapshot = await ApiClient.shared.waitForRuntimeSnapshot(since: sinceRevision) else {
                daemonOnline = false
                try? await Task.sleep(for: .milliseconds(750))
                continue
            }

            let reconnected = !daemonOnline
            daemonOnline = true
            runtimeRevision = snapshot.revision
            // Persistent storage banner: keeps showing while the server
            // reports storage unavailable, and clears immediately when it
            // reports storage back; the background recovery merges the rows and
            // bumps the history version, which reloads the list below.
            if snapshot.storage?.available == false {
                if storageWarning == nil {
                    storageWarning = Loc.t("history.storageUnavailable")
                }
            } else {
                storageWarning = nil
            }
            if reconnected || snapshot.historyVersion != lastVersion {
                lastVersion = snapshot.historyVersion
                load(targetPage: page, clearExisting: false)
                await HistoryPreviewWindowController.shared.refreshSavedDocument(for: previewOwner)
                if reconnected {
                    historyCapabilitiesChecked = false
                    loadHistoryCapabilities()
                }
            }
            // Warnings are independent of history mutations. A hidden or cancelled
            // window leaves the warning pending for the next visible consumer.
            if syncWarning == nil, windowIsVisible, !Task.isCancelled,
               let warning = await ApiClient.shared.takeSyncWarning(),
               windowIsVisible, !Task.isCancelled {
                let messageKey: String?
                switch warning.kind {
                case "composite_incomplete": messageKey = "history.composite.syncIncomplete"
                case "composite_apply_unknown": messageKey = "history.composite.applyUnknown"
                case "composite_partial": messageKey = "history.composite.partial"
                case "composite_superseded": messageKey = "history.composite.superseded"
                case "file_selection_symlink": messageKey = "history.fileSelectionSymlink"
                case "file_selection_folder": messageKey = "history.fileSelectionFolder"
                case "delivery_rejected": messageKey = "history.deliveryRejected"
                case "expired_event": messageKey = "history.syncExpired"
                case "delivery_stalled": messageKey = "history.syncStalled"
                case "delivery_shutdown": messageKey = "history.syncShutdown"
                case "delivery_expired": messageKey = "history.syncDeliveryExpired"
                // Capture failure remains visible independently of delivery.
                case "clipboard_unavailable": messageKey = "history.clipboardUnavailable"
                case "capture_origin_unknown": messageKey = "history.captureOriginUnknown"
                case "capture_not_shared": messageKey = "history.captureNotShared"
                case "capture_failed": messageKey = "history.captureFailed"
                case "warning_overflow": messageKey = "history.warningOverflow"
                case "capture_not_saved": messageKey = "history.captureNotSaved"
                case "receive_not_saved": messageKey = "history.receiveNotSaved"
                default: messageKey = nil
                }
                if let messageKey {
                    syncWarning = Loc.t(messageKey)
                        .replacingOccurrences(of: "{peer}", with: warning.peer)
                    await ApiClient.shared.acknowledgeSyncWarning(warning.id)
                    syncWarningTask?.cancel()
                    syncWarningTask = Task { @MainActor in
                        try? await Task.sleep(for: .seconds(8))
                        guard !Task.isCancelled else { return }
                        syncWarning = nil
                    }
                }
            }
            if !historyCapabilitiesChecked { loadHistoryCapabilities() }
            fileProgress = progressBarEnabled ? snapshot.progress : nil

            // Progress may update for every transfer chunk. Coalesce those
            // events while keeping the visible bar responsive.
            if snapshot.progress?.active == true {
                try? await Task.sleep(for: .milliseconds(250))
            }
        }
    }

    private func isBatchStart(_ entry: HistoryEntry) -> Bool {
        HistoryBatchPresentation.isBatchStart(entry, in: entries)
    }

    private func restoreBatch(_ batchId: String) {
        performRestore(restoredEntryID: nil, dismiss: false) { _ in
            try await dependencies.restoreBatch(batchId)
            return nil
        }
    }

    private func loadMigrationDiagnostics() {
        Task {
            do {
                unresolvedMigrationCount = (try await ApiClient.shared
                    .getMigrationDiagnostics()).unresolvedCount
            } catch {
                // Diagnostics are supplementary; normal history remains usable.
            }
        }
    }

    private func dateBounds(for filter: HistoryDateFilter) -> HistoryDateBounds {
        let calendar = Calendar.autoupdatingCurrent
        let today = calendar.startOfDay(for: Date())
        let tomorrow = calendar.date(byAdding: .day, value: 1, to: today)

        switch filter {
        case .all:
            return HistoryDateBounds(start: nil, end: nil)
        case .today:
            return HistoryDateBounds(start: today, end: tomorrow)
        case .yesterday:
            return HistoryDateBounds(
                start: calendar.date(byAdding: .day, value: -1, to: today),
                end: today
            )
        case .last7, .last30:
            let daysBeforeToday = filter == .last7 ? 6 : 29
            return HistoryDateBounds(
                start: calendar.date(byAdding: .day, value: -daysBeforeToday, to: today),
                end: tomorrow
            )
        case .thisMonth:
            let interval = calendar.dateInterval(of: .month, for: today)
            return HistoryDateBounds(start: interval?.start, end: interval?.end)
        case .custom:
            let start = calendar.startOfDay(for: customStartDate)
            let inclusiveEnd = calendar.startOfDay(for: customEndDate)
            return HistoryDateBounds(
                start: start,
                end: calendar.date(byAdding: .day, value: 1, to: inclusiveEnd)
            )
        }
    }

    private func utcRFC3339(_ date: Date?) -> String? {
        guard let date else { return nil }
        let formatter = ISO8601DateFormatter()
        formatter.timeZone = TimeZone(secondsFromGMT: 0)
        formatter.formatOptions = [.withInternetDateTime]
        return formatter.string(from: date)
    }

    private func reloadActiveDateFilter() {
        guard dateRangeFilteringSupported && selectedDateFilter != .all else { return }
        page = 0
        load(targetPage: 0)
    }

    private func scheduleSearch() {
        restoredId = nil
        interactionGeneration &+= 1
        queryTask?.cancel()
        queryTask = nil
        queryGeneration &+= 1
        let generation = queryGeneration
        // Cancel and invalidate the previous request immediately, not after the debounce.
        cancelPendingLoad()
        pendingLoadPage = 0
        pendingPageSelection = nil
        previewFollowTask?.cancel()
        HistoryPreviewWindowController.shared.close(ifOwnedBy: previewOwner)
        guard windowIsVisible else { return }
        queryTask = Task { @MainActor in
            // `queryTask != nil` is read as "a search is still pending" by
            // `resultsAreCurrent` and by the runtime-refresh early return, so
            // every exit path must clear it — including the composing bail-out
            // below. Only the newest generation may clear the slot, otherwise a
            // superseded task would clear its replacement's flag. Clearing does
            // not make stale results look current: `resultsAreCurrent` still
            // compares the loaded query against the live keyword, which has
            // already changed by the time this runs.
            defer { if generation == queryGeneration { queryTask = nil } }
            try? await Task.sleep(for: .milliseconds(180))
            guard !Task.isCancelled, generation == queryGeneration, !keyboard.isComposing else { return }
            load(targetPage: 0)
        }
    }

    private func handleKeyboard(_ command: HistoryKeyboardCommand) {
        guard !keyboard.isComposing, !isRestoring, !isDeleting, !showingClearAlert else { return }
        switch command {
        case .down:
            guard resultsAreCurrent else { return }
            keyboard.reconcile(ids: displayedEntries.map(\.id))
            if keyboard.focus == .search { keyboard.focusList(); return }
            if let id = focusedEntryId,
               let index = displayedEntries.firstIndex(where: { $0.id == id }),
               index + 1 < displayedEntries.count {
                focusedEntryId = displayedEntries[index + 1].id
            } else if hasNext {
                pendingPageSelection = .first
                load(targetPage: page + 1)
            }
        case .up:
            guard resultsAreCurrent else { return }
            if keyboard.focus == .search { return }
            if let id = focusedEntryId,
               let index = displayedEntries.firstIndex(where: { $0.id == id }), index > 0 {
                focusedEntryId = displayedEntries[index - 1].id
            } else if page > 0 {
                pendingPageSelection = .last
                load(targetPage: page - 1)
            } else { keyboard.focusSearch() }
        case .search: keyboard.focusSearch()
        case .restore(let keepOpen):
            guard resultsAreCurrent, let id = focusedEntryId else { return }
            restore(id, dismiss: !keepOpen)
        case .delete:
            guard resultsAreCurrent, keyboard.focus == .list,
                  let entry = displayedEntries.first(where: { $0.id == focusedEntryId }) else { return }
            delete(entry)
        case .escape:
            if HistoryPreviewWindowController.shared.isPreviewVisible(for: previewOwner) {
                HistoryPreviewWindowController.shared.close(ifOwnedBy: previewOwner)
            } else if isFavoritesCollection {
                HistoryPreviewWindowController.shared.dismissHost(for: previewOwner)
            } else { HistoryWindowController.shared.dismissAndReturnToOriginatingApp() }
        case .preview, .deepPreview:
            guard resultsAreCurrent, keyboard.focus == .list, let request = selectedPreviewRequest else { return }
            previewFollowTask?.cancel()
            if command == .deepPreview,
               HistoryPreviewWindowController.shared.isPreviewVisible(for: previewOwner),
               HistoryPreviewWindowController.shared.viewModel.currentItem?.id == request.selectedItem?.id {
                HistoryPreviewWindowController.shared.focus(for: previewOwner)
            } else {
                HistoryPreviewWindowController.shared.present(
                    request, owner: previewOwner, takeFocus: command == .deepPreview
                )
            }
        }
    }

    private func historyQuery(page: Int) -> HistoryQuery {
        let bounds = dateRangeFilteringSupported
            ? dateBounds(for: selectedDateFilter)
            : HistoryDateBounds(start: nil, end: nil)
        return HistoryQuery(
            keyword: keyword.isEmpty ? nil : keyword,
            category: categoryFilteringSupported && selectedCategory != "all" ? selectedCategory : nil,
            startTime: utcRFC3339(bounds.start), endTime: utcRFC3339(bounds.end),
            limit: pageSize + 1, offset: page * pageSize, collection: collection
        )
    }

    private var resultsAreCurrent: Bool {
        !isLoading && queryTask == nil && loadedQuery == historyQuery(page: page)
    }

    private var selectedPreviewRequest: HistoryPreviewRequest? {
        guard let id = focusedEntryId else { return nil }
        return historyPreviewRequest(focusedId: id, entries: entries, expandedBatchIds: expandedBatchIds)
    }

    private func followPreviewSelection() {
        previewFollowTask?.cancel()
        guard HistoryPreviewWindowController.shared.isPreviewVisible(for: previewOwner) else { return }
        guard focusedEntryId != nil else {
            HistoryPreviewWindowController.shared.close(ifOwnedBy: previewOwner)
            return
        }
        let selected = focusedEntryId
        let generation = interactionGeneration
        previewFollowTask = Task { @MainActor in
            try? await Task.sleep(for: .milliseconds(100))
            guard !Task.isCancelled, windowIsVisible, resultsAreCurrent, !isRestoring,
                  generation == interactionGeneration, selected == focusedEntryId,
                  HistoryPreviewWindowController.shared.isPreviewVisible(for: previewOwner),
                  let request = selectedPreviewRequest else { return }
            HistoryPreviewWindowController.shared.present(request, owner: previewOwner, takeFocus: false)
        }
    }

    private func restore(_ id: Int64, dismiss: Bool = false) {
        let identity = entries.first(where: { $0.id == id })?.composite?.identity
        performRestore(restoredEntryID: id, dismiss: dismiss) { resolvedID in
            if let reported = dependencies.restoreReported {
                return try await reported(resolvedID ?? id, nil, id, identity)
            }
            if let guarded = dependencies.restoreGuarded {
                try await guarded(resolvedID ?? id, nil, id, identity)
            } else { try await dependencies.restore(resolvedID ?? id) }
            return nil
        }
    }

    // Keyboard, row, batch and hosted-preview restores share one lock and
    // completion path, so no entry point can mutate the clipboard concurrently.
    private func archiveImages(_ id: Int64) {
        guard !isRestoring, !isDeleting, resultsAreCurrent else { return }
        isRestoring = true
        let generation = interactionGeneration
        restoreFeedbackTask?.cancel()
        restoreFeedbackTask = Task { @MainActor in
            do {
                let report = try await dependencies.archiveImages(id)
                guard !Task.isCancelled, generation == interactionGeneration else { isRestoring = false; return }
                actionMessage = HistoryResourceArchiveFeedback.message(report)
            } catch {
                guard !Task.isCancelled, generation == interactionGeneration else { isRestoring = false; return }
                actionMessage = Loc.t("history.composite.archivePartial")
            }
            isRestoring = false
            load(clearExisting: false)
        }
    }

    private func authorizeSourceContext(_ id: Int64) {
        guard !isRestoring, !isDeleting, resultsAreCurrent else { return }
        isRestoring = true
        let generation = interactionGeneration
        restoreFeedbackTask?.cancel()
        restoreFeedbackTask = Task { @MainActor in
            defer { if generation == interactionGeneration { isRestoring = false } }
            do {
                let panel = NSSavePanel()
                panel.nameFieldStringValue = "tailsync-source-ticket.json"
                panel.canCreateDirectories = true
                guard panel.runModal() == .OK, let url = panel.url else { return }
                guard !Task.isCancelled, generation == interactionGeneration else { return }
                let createGrant = dependencies.sourceContextGrant
                let grantTask = Task.detached { try await createGrant(id) }
                let report = try await grantTask.value
                guard !Task.isCancelled, generation == interactionGeneration else {
                    Task { _ = try? await dependencies.sourceContextRevoke(report.grant_id) }
                    return
                }
                do {
                    try Data(report.ticket.utf8).write(to: url, options: .withoutOverwriting)
                } catch {
                    _ = try? await dependencies.sourceContextRevoke(report.grant_id)
                    actionMessage = Loc.t("history.composite.sourceSaveFailed")
                    return
                }
                lastSourceGrantId = report.grant_id
                actionMessage = Loc.t("history.composite.sourceAuthorized")
            } catch {
                guard !Task.isCancelled, generation == interactionGeneration else { return }
                actionMessage = Loc.t("history.composite.sourceUnavailable")
            }
        }
    }

    private func importSourcePacket(_ id: Int64) {
        guard !isRestoring, !isDeleting, resultsAreCurrent else { return }
        isRestoring = true
        let generation = interactionGeneration
        let panel = NSOpenPanel()
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = false
        guard panel.runModal() == .OK, let url = panel.url,
              generation == interactionGeneration else {
            if generation == interactionGeneration { isRestoring = false }
            return
        }
        restoreFeedbackTask?.cancel()
        restoreFeedbackTask = Task { @MainActor in
            defer { if generation == interactionGeneration { isRestoring = false } }
            do {
                let report = try await dependencies.sourceContextImport(id, url.path)
                guard !Task.isCancelled, generation == interactionGeneration else { return }
                actionMessage = report.complete
                    ? Loc.t("history.composite.sourceImported")
                    : Loc.t("history.composite.sourcePartial")
            } catch {
                guard !Task.isCancelled, generation == interactionGeneration else { return }
                actionMessage = Loc.t("history.composite.sourcePartial")
            }
            load(clearExisting: false)
        }
    }

    private func revokeSourceContext() {
        guard let grantId = lastSourceGrantId else { return }
        let generation = interactionGeneration
        Task { @MainActor in
            guard let report = try? await dependencies.sourceContextRevoke(grantId), report.revoked,
                  generation == interactionGeneration else { return }
            lastSourceGrantId = nil
            actionMessage = Loc.t("history.composite.sourceRevoked")
        }
    }

    private func performRestore(
        restoredEntryID: Int64?, dismiss: Bool,
        operation: @escaping @Sendable (Int64?) async throws -> ContractHistoryRestoreReport?
    ) {
        guard !isRestoring, !isDeleting, resultsAreCurrent else { return }
        isRestoring = true
        retryRestoreAction = nil
        actionMessageTask?.cancel()
        actionMessageTask = nil
        actionMessage = nil
        restoredId = nil
        previewFollowTask?.cancel()
        restoreFeedbackTask?.cancel()
        let generation = interactionGeneration
        let retryIdentity = restoredEntryID.flatMap { id in
            entries.first(where: { $0.id == id }).map(HistoryRestoreRetryIdentity.init)
        }
        HistoryPreviewWindowController.shared.setRestoreState(.restoring, for: previewOwner)
        restoreFeedbackTask = Task { @MainActor in
            let report: ContractHistoryRestoreReport?
            do {
                report = try await operation(restoredEntryID)
            } catch {
                isRestoring = false
                guard !Task.isCancelled, generation == interactionGeneration else {
                    HistoryPreviewWindowController.shared.setRestoreState(.idle, for: previewOwner)
                    return
                }
                HistoryPreviewWindowController.shared.setRestoreState(.failed, for: previewOwner)
                let needsExplicitRetry: Bool
                if case ApiError.serverError(let code) = error {
                    needsExplicitRetry = code == "publish_partial" || code == "publish_unknown"
                } else {
                    needsExplicitRetry = false
                }
                showActionMessage(error.localizedDescription, persistent: needsExplicitRetry)
                if needsExplicitRetry {
                    retryRestoreAction = {
                        var retryID = restoredEntryID
                        if let retryIdentity {
                            guard let resolvedID = retryIdentity.resolve(in: entries) else {
                                showActionMessage(Loc.t("error.not_found"))
                                return
                            }
                            retryID = resolvedID
                        }
                        performRestore(restoredEntryID: retryID, dismiss: dismiss, operation: operation)
                    }
                }
                return
            }
            isRestoring = false
            guard !Task.isCancelled, generation == interactionGeneration else {
                HistoryPreviewWindowController.shared.setRestoreState(.idle, for: previewOwner)
                return
            }
            HistoryPreviewWindowController.shared.setRestoreState(.restored, for: previewOwner)
            if let report, !report.complete {
                showActionMessage(Loc.t("history.restore.availableContent").replacingOccurrences(of: "{count}", with: "\(report.missing_images)"))
            }
            if dismiss && report?.complete != false {
                if isFavoritesCollection {
                    HistoryPreviewWindowController.shared.dismissHost(for: previewOwner)
                } else {
                    HistoryWindowController.shared.dismissAndReturnToOriginatingApp()
                }
            } else {
                restoredId = restoredEntryID
                try? await Task.sleep(for: .seconds(1.5))
                guard !Task.isCancelled, generation == interactionGeneration else { return }
                restoredId = nil
                HistoryPreviewWindowController.shared.setRestoreState(.idle, for: previewOwner)
            }
        }
    }

    private func delete(_ entry: HistoryEntry) {
        guard !isRestoring, !isDeleting, resultsAreCurrent,
              favoriteMutations[logicalItemKey(entry)] == nil else { return }
        if entry.pinned && !isFavoritesCollection {
            showActionMessage(Loc.t("history.favoriteProtected"))
            return
        }
        isDeleting = true
        let generation = interactionGeneration
        let ids = displayedEntries.map(\.id)
        let removedIDs = Set(entries.filter {
            $0.id == entry.id || (isFavoritesCollection && logicalItemKey($0) == logicalItemKey(entry))
        }.map(\.id))
        let index = ids.firstIndex(of: entry.id) ?? 0
        let nextID = ids.dropFirst(index + 1).first { !removedIDs.contains($0) }
            ?? ids.prefix(index).last { !removedIDs.contains($0) }
        Task {
            defer { isDeleting = false }
            do {
                try await dependencies.delete(entry.id, collection)
                guard generation == interactionGeneration else { return }
                entries.removeAll { removedIDs.contains($0.id) }
                if let selectedID = focusedEntryId, removedIDs.contains(selectedID) { focusedEntryId = nextID }
                HistoryPreviewWindowController.shared.closeIfShowing(entryId: entry.id)
                pruneSelection()
                if dependencies.usesRuntimeUpdates { loadMigrationDiagnostics() }
                if isFavoritesCollection {
                    // A favorite file batch is one logical item. Refresh the
                    // collection so sibling rows disappear immediately too.
                    load(targetPage: page, clearExisting: false)
                }
            } catch {
                guard generation == interactionGeneration else { return }
                showActionMessage(Loc.t("history.deleteError"))
            }
        }
    }

    private func clearAll() {
        Task {
            let ok = await ApiClient.shared.clearAllHistory()
            if ok {
                page = 0
                focusedEntryId = nil
                HistoryPreviewWindowController.shared.close(ifOwnedBy: previewOwner)
                load(targetPage: 0)
                loadMigrationDiagnostics()
            } else {
                errorMsg = Loc.t("history.deleteError")
            }
        }
    }

    private func toggleFavorite(_ entry: HistoryEntry) {
        guard !isDeleting else { return }
        let mutationKey = logicalItemKey(entry)
        let favorite = !entry.pinned
        guard favoriteMutations[mutationKey] == nil else { return }
        favoriteMutations[mutationKey] = favorite
        withAnimation(loc.reduceMotion ? nil : .easeOut(duration: 0.18)) {
            for index in entries.indices
                where logicalItemKey(entries[index]) == mutationKey
            {
                entries[index].pinned = favorite
            }
        }
        Task {
            defer { favoriteMutations.removeValue(forKey: mutationKey) }
            do {
                // The daemon applies the mutation to the complete logical
                // item, including every member of a file batch.
                try await ApiClient.shared.setHistoryFavorite(id: entry.id, favorite: favorite)
                // Reload instead of changing only the pressed row: file
                // batches are one logical favorite and the daemon mutates
                // every member atomically.
                load(targetPage: page, clearExisting: false)
            } catch {
                withAnimation(loc.reduceMotion ? nil : .easeOut(duration: 0.18)) {
                    for index in entries.indices
                        where logicalItemKey(entries[index]) == mutationKey
                    {
                        entries[index].pinned = entry.pinned
                    }
                }
                showActionMessage(Loc.t("history.actionFailed"))
            }
        }
    }

    private func showActionMessage(_ message: String, persistent: Bool = false) {
        retryRestoreAction = nil
        actionMessageTask?.cancel()
        actionMessage = message
        // An uncertain native effect needs a deliberate recovery decision.
        // Keep its explanation and action available for slower/assistive input.
        guard !persistent else { return }
        actionMessageTask = Task { @MainActor in
            try? await Task.sleep(for: .seconds(3))
            guard !Task.isCancelled else { return }
            actionMessage = nil
            retryRestoreAction = nil
        }
    }

    /// Remove a selection that no longer refers to a loaded row so a delayed
    /// key event cannot preview deleted or stale content.
    private func pruneSelection() {
        keyboard.reconcile(ids: displayedEntries.map(\.id))
    }
}

// ── Row ──────────────────────────────────────────────────────────
