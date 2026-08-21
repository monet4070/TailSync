import AppKit
import Foundation
import SwiftUI

private struct HistoryDateBounds {
    let start: Date?
    let end: Date?
}

struct HistoryView: View {
    @ObservedObject private var loc = Loc.shared
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize
    @Environment(\.scenePhase) private var scenePhase
    @State private var entries: [HistoryEntry] = []
    @State private var keyword = ""
    @State private var selectedCategory = "all"
    @State private var selectedDateFilter = HistoryDateFilter.all
    @State private var customStartDate = Calendar.autoupdatingCurrent.startOfDay(for: Date())
    @State private var customEndDate = Calendar.autoupdatingCurrent.startOfDay(for: Date())
    @State private var page = 0
    @State private var hasNext = false
    @State private var isLoading = false
    @State private var restoredId: Int64? = nil
    // Previewing is independent of restoration: selection chooses a row,
    // Space opens it in the reusable preview window, and double-click retains
    // the established clipboard-restore gesture.
    @State private var focusedEntryId: Int64? = nil
    @State private var errorMsg: String? = nil
    @State private var lastVersion: UInt64 = 0
    @State private var runtimeRevision: UInt64 = 0
    @State private var daemonOnline = false
    @State private var fileProgress: ApiClient.FileProgress? = nil
    @State private var progressBarEnabled = true
    @State private var showingClearAlert = false
    @State private var loadGeneration = 0
    @State private var supportedCategories: Set<String> = []
    @State private var multipleLabelsSupported = false
    @State private var dateRangeFilteringSupported = false
    @State private var historyCapabilitiesChecked = false
    @State private var historyCapabilitiesLoading = false
    @State private var unresolvedMigrationCount = 0
    @State private var syncWarning: String? = nil
    @State private var syncWarningTask: Task<Void, Never>? = nil
    @State private var restoreFeedbackTask: Task<Void, Never>? = nil
    @State private var expandedBatchIds: Set<String> = []

    private let pageSize = 30
    private let collapsedBatchFileLimit = 2
    private let knownCategories = ["text", "website", "code", "command", "structured_data", "path", "image", "file"]

    private var activeTheme: TailSyncThemeSelection {
        TailSyncThemeSelection(
            storedValue: loc.colorTheme,
            catalogue: loc.resolvedV2Themes,
            reduceTransparency: loc.reduceTransparency,
            interfaceScale: TailSyncThemeAccessibilityPolicy.interfaceScale(for: dynamicTypeSize)
        )
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
        entry.batch_count ?? entry.batch_total ?? 1
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
                onFilterChanged: { page = 0; load(targetPage: 0) }
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
                    if let image = loc.themeAssetImages["emptyState"] {
                        Image(nsImage: image).resizable().aspectRatio(contentMode: .fit).frame(width: 64, height: 64)
                    } else {
                        Image(systemName: "doc.on.clipboard")
                            .font(.system(size: 32))
                            .foregroundColor(palette.tertiaryColor)
                    }
                    Text(Loc.t("history.empty"))
                        .font(activeTheme.displayFont(size: 15))
                        .foregroundColor(palette.secondaryColor)
                }
                Spacer()
            } else {
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
                        HistoryRow(
                            entry: entry,
                            isRestored: restoredId == entry.id,
                            isFocused: focusedEntryId == entry.id,
                            showsMultipleLabels: multipleLabelsSupported
                        )
                        .contentShape(Rectangle())
                        .overlay {
                            HistoryRowInteraction(
                                previewRequest: historyPreviewRequest(
                                    focusedId: entry.id,
                                    entries: entries,
                                    expandedBatchIds: expandedBatchIds
                                ),
                                onSelect: {
                                    focusedEntryId = entry.id
                                },
                                onRestore: {
                                    restore(entry.id)
                                },
                                onDelete: {
                                    delete(entry.id)
                                },
                                onPreview: { request in
                                    HistoryPreviewWindowController.shared.present(request)
                                },
                                onClosePreview: {
                                    HistoryPreviewWindowController.shared.close()
                                },
                                isPreviewVisible: {
                                    HistoryPreviewWindowController.shared.isPreviewVisible
                                }
                            )
                        }
                        }
                        .listRowBackground(palette.surfaceColor)
                        .listRowSeparatorTint(palette.dividerColor)
                    }
                }
                .listStyle(.plain)
                .scrollContentBackground(.hidden)
                .animation(loc.reduceMotion ? nil : .spring(response: 0.35, dampingFraction: 0.9), value: entries.count)
            }

            // Pagination + clear
            HStack(spacing: 12) {
                Button {
                    let target = page - 1
                    load(targetPage: target)
                } label: { Image(systemName: "chevron.left") }
                    .disabled(page == 0 || isLoading)
                Text("\(page + 1)").font(.caption).foregroundColor(palette.tertiaryColor).monospacedDigit()
                Button {
                    let target = page + 1
                    load(targetPage: target)
                } label: { Image(systemName: "chevron.right") }
                    .disabled(!hasNext || isLoading)
                Spacer()
                if !entries.isEmpty {
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
        .onAppear {
            load()
            loadProgressPreference()
            loadHistoryCapabilities()
            loadMigrationDiagnostics()
        }
        .task { await runtimeSnapshotLoop() }
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
            HistoryThumbnailCache.removeAll()
            restoreFeedbackTask?.cancel()
            restoreFeedbackTask = nil
            syncWarningTask?.cancel()
            syncWarningTask = nil
            focusedEntryId = nil
            HistoryPreviewWindowController.shared.close()
        }
        .overlay(alignment: .bottom) {
            VStack(spacing: 6) {
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

    private func load(targetPage: Int? = nil, clearExisting: Bool = true) {
        loadGeneration += 1
        let generation = loadGeneration
        let requestedPage = targetPage ?? page
        let requestedKeyword = keyword.isEmpty ? nil : keyword
        let requestedCategory = categoryFilteringSupported && selectedCategory != "all"
            ? selectedCategory
            : nil
        let requestedBounds = dateRangeFilteringSupported
            ? dateBounds(for: selectedDateFilter)
            : HistoryDateBounds(start: nil, end: nil)
        let requestedStartTime = utcRFC3339(requestedBounds.start)
        let requestedEndTime = utcRFC3339(requestedBounds.end)
        isLoading = true
        errorMsg = nil
        if clearExisting {
            entries = []
            hasNext = false
            focusedEntryId = nil
        }
        Task {
            do {
                let result = try await ApiClient.shared.getHistory(
                    keyword: requestedKeyword,
                    category: requestedCategory,
                    startTime: requestedStartTime,
                    endTime: requestedEndTime,
                    limit: pageSize + 1,
                    offset: requestedPage * pageSize)
                guard generation == loadGeneration else { return }
                entries = Array(result.prefix(pageSize))
                hasNext = result.count > pageSize
                page = requestedPage
                pruneSelection()
            } catch {
                guard generation == loadGeneration else { return }
                errorMsg = Loc.t("history.loadError")
            }
            if generation == loadGeneration { isLoading = false }
        }
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
    private func runtimeSnapshotLoop() async {
        while !Task.isCancelled {
            guard scenePhase == .active else {
                try? await Task.sleep(for: .milliseconds(500))
                continue
            }
            guard let snapshot = await ApiClient.shared.waitForRuntimeSnapshot(since: runtimeRevision) else {
                daemonOnline = false
                try? await Task.sleep(for: .milliseconds(750))
                continue
            }

            let reconnected = !daemonOnline
            daemonOnline = true
            runtimeRevision = snapshot.revision
            if reconnected || snapshot.historyVersion != lastVersion {
                lastVersion = snapshot.historyVersion
                load(targetPage: page, clearExisting: false)
                if reconnected {
                    historyCapabilitiesChecked = false
                    loadHistoryCapabilities()
                }
                if let warning = await ApiClient.shared.takeSyncWarning() {
                    let messageKey: String?
                    switch warning.kind {
                    case "expired_event": messageKey = "history.syncExpired"
                    case "delivery_stalled": messageKey = "history.syncStalled"
                    default: messageKey = nil
                    }
                    if let messageKey {
                        syncWarning = Loc.t(messageKey)
                            .replacingOccurrences(of: "{peer}", with: warning.peer)
                        syncWarningTask?.cancel()
                        syncWarningTask = Task { @MainActor in
                            try? await Task.sleep(for: .seconds(8))
                            guard !Task.isCancelled else { return }
                            syncWarning = nil
                        }
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
        guard let batchId = entry.batch_id,
              let index = entries.firstIndex(where: { $0.id == entry.id }) else { return false }
        return index == 0 || entries[index - 1].batch_id != batchId
    }

    private func restoreBatch(_ batchId: String) {
        Task { @MainActor in
            do {
                try await ApiClient.shared.restoreFileBatch(batchId)
            } catch {
                errorMsg = error.localizedDescription
            }
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

    private func restore(_ id: Int64) {
        restoreFeedbackTask?.cancel()
        restoreFeedbackTask = Task {
            do {
                try await ApiClient.shared.restoreEntry(id: id)
            } catch {
                errorMsg = error.localizedDescription
                return
            }
            guard !Task.isCancelled else { return }
            withAnimation(loc.reduceMotion ? nil : .easeOut(duration: 0.4)) { restoredId = id }
            do {
                try await Task.sleep(nanoseconds: 1_500_000_000)
            } catch {
                return
            }
            guard !Task.isCancelled else { return }
            withAnimation(loc.reduceMotion ? nil : .easeOut(duration: 0.4)) { restoredId = nil }
        }
    }

    private func delete(_ id: Int64) {
        Task {
            do {
                try await ApiClient.shared.deleteEntry(id: id)
                entries.removeAll { $0.id == id }
                HistoryPreviewWindowController.shared.closeIfShowing(entryId: id)
                pruneSelection()
                loadMigrationDiagnostics()
            } catch {
                errorMsg = Loc.t("history.deleteError")
            }
        }
    }

    private func clearAll() {
        Task {
            let ok = await ApiClient.shared.clearAllHistory()
            if ok {
                entries = []
                page = 0
                hasNext = false
                focusedEntryId = nil
                HistoryPreviewWindowController.shared.close()
                loadMigrationDiagnostics()
            } else {
                errorMsg = Loc.t("history.deleteError")
            }
        }
    }

    /// Remove a selection that no longer refers to a loaded row so a delayed
    /// key event cannot preview deleted or stale content.
    private func pruneSelection() {
        let validIds = Set(entries.map(\.id))
        if let focusedEntryId, !validIds.contains(focusedEntryId) {
            self.focusedEntryId = nil
        }
    }
}

// ── Row ──────────────────────────────────────────────────────────

struct HistoryRow: View {
    let entry: HistoryEntry
    let isRestored: Bool
    let isFocused: Bool
    let showsMultipleLabels: Bool
    @State private var thumbnail: NSImage? = nil
    @State private var hovering = false
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.tailSyncSelection) private var selection
    @Environment(\.tailSyncPalette) private var palette

    private var component: TailSyncThemeComponentTokens? {
        let state = isRestored ? "selected" : (isFocused ? "focus" : (hovering ? "hover" : "default"))
        return selection.component("history", state: state, scheme: colorScheme)
    }

    private var visibleCategoryLabels: [String] {
        showsMultipleLabels ? entry.categoryLabels : [entry.categoryLabel]
    }

    var body: some View {
        HStack(spacing: 10) {
            Group {
                if entry.type == "image", let thumb = thumbnail {
                    thumbnailImage(thumb)
                } else {
                    Image(systemName: entry.icon).frame(width: 24, height: 24)
                        .foregroundColor(component?.iconColor ?? component?.accentColor ?? palette.accentColor)
                        .background((component?.accentColor ?? palette.accentColor).opacity(0.1))
                        .clipShape(RoundedRectangle(cornerRadius: component?.radius ?? selection.metrics.controlRadius, style: .continuous))
                }
            }
            .frame(width: HistoryThumbnailLayout.columnWidth)
            .task {
                guard entry.type == "image" else { return }
                if let cached = HistoryThumbnailCache.image(for: entry.id) {
                    thumbnail = cached
                } else if let img = await ApiClient.shared.getImageData(id: entry.id),
                          let image = rgbaToImage(img) {
                    HistoryThumbnailCache.insert(image, for: entry.id)
                    thumbnail = image
                }
            }

            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(visibleCategoryLabels.map { $0.uppercased() }.joined(separator: "  \u{00B7}  "))
                        .font(selection.readingFont(size: 10, weight: .semibold))
                        .foregroundColor(component?.accentColor ?? palette.accentColor).padding(.horizontal, 4).padding(.vertical, 1)
                        .background((component?.accentColor ?? palette.accentColor).opacity(0.12))
                        .clipShape(RoundedRectangle(cornerRadius: component?.radius ?? selection.metrics.controlRadius, style: .continuous))
                        .fixedSize(horizontal: false, vertical: true)
                        .layoutPriority(1)
                    Text(entry.formattedTime).font(.caption).foregroundColor(palette.tertiaryColor)
                }
                Text(entry.description)
                    .font(selection.builtin == .tailsync
                          ? selection.displayFont(size: selection.typography.historyContentSize)
                          : selection.builtin == .forest
                              ? selection.displayFont(size: selection.typography.historyContentSize, weight: .medium)
                              : selection.readingFont(size: selection.typography.historyContentSize, weight: .medium))
                    .foregroundColor(component?.foregroundColor ?? palette.primaryColor)
                    .lineLimit(1)
                HStack(spacing: 6) {
                    Text(entry.formattedSize).font(.caption2).foregroundColor(component?.secondaryTextColor ?? palette.tertiaryColor).monospacedDigit()
                    Spacer()
                    Text(entry.source_peer).font(.caption2).foregroundColor(component?.secondaryTextColor ?? palette.tertiaryColor).lineLimit(1)
                }
            }
        }
        .padding(.vertical, component?.padding ?? max(2, (selection.metrics.rowPadding - 6) / 2))
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(
            component?.backgroundColor ?? (isRestored
                ? palette.activeColor
                : isFocused ? palette.hoverColor : .clear)
        )
        .clipShape(RoundedRectangle(cornerRadius: component?.radius ?? selection.metrics.controlRadius, style: .continuous))
        .overlay {
            RoundedRectangle(cornerRadius: component?.radius ?? selection.metrics.controlRadius, style: .continuous)
                .stroke(
                    isFocused ? (component?.focusRingColor ?? palette.accentColor).opacity(0.7) : (component?.borderColor ?? .clear),
                    lineWidth: isFocused ? 1 : 0
                )
        }
        .accessibilityAddTraits(isFocused ? .isSelected : [])
        .contentShape(Rectangle())
        .onHover { hovering = $0 }
        .animation(Loc.shared.reduceMotion ? nil : .easeOut(duration: 0.4), value: isRestored)
        .animation(Loc.shared.reduceMotion ? nil : .easeOut(duration: 0.18), value: isFocused)
        .animation(Loc.shared.reduceMotion ? nil : .easeOut(duration: 0.12), value: hovering)
    }

    /// Render an image thumbnail at its natural aspect ratio inside the fixed
    /// column: `.fit` shows the whole image (a long screenshot is never cropped
    /// to an unrecognizable middle band), `.high` interpolation keeps the
    /// downscaled pixels crisp, and the frame comes from the ratio-clamped
    /// layout so extreme strips cannot distort the row.
    private func thumbnailImage(_ thumb: NSImage) -> some View {
        let size = HistoryThumbnailLayout.displaySize(
            pixelWidth: thumb.size.width,
            pixelHeight: thumb.size.height
        )
        return Image(nsImage: thumb)
            .resizable()
            .interpolation(.high)
            .aspectRatio(contentMode: .fit)
            .frame(width: size.width, height: size.height)
            .clipShape(RoundedRectangle(cornerRadius: selection.metrics.controlRadius, style: .continuous))
    }
}

// ── Helpers ──────────────────────────────────────────────────────

private func rgbaToImage(_ data: ApiClient.ImageData) -> NSImage? {
    let w = data.width, h = data.height
    let (pixelCount, pixelOverflow) = w.multipliedReportingOverflow(by: h)
    let (byteCount, byteOverflow) = pixelCount.multipliedReportingOverflow(by: 4)
    guard w > 0, h > 0, !pixelOverflow, !byteOverflow, data.rgba.count == byteCount else {
        return nil
    }
    guard let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: w, pixelsHigh: h,
                                      bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                                      colorSpaceName: .deviceRGB, bytesPerRow: w * 4, bitsPerPixel: 32) else { return nil }
    data.rgba.copyBytes(to: rep.bitmapData!, count: byteCount)
    let img = NSImage(size: NSSize(width: w, height: h))
    img.addRepresentation(rep)
    return img
}

/// Layout math for history-row image thumbnails.
///
/// Thumbnails render at their true aspect ratio inside a fixed-width column so
/// the text beside them stays left-aligned across rows. The ratio is clamped to
/// `maxAspect`:1 in both directions: without the clamp an extreme long-strip
/// (a full-page screenshot, a wide banner) would either blow the row height up
/// or shrink to an unreadable sliver.
enum HistoryThumbnailLayout {
    /// Longest displayed edge, in points.
    static let maxSide: CGFloat = 48
    /// Steepest displayed width:height (or height:width) ratio.
    static let maxAspect: CGFloat = 2.5
    /// Fixed column width so rows align regardless of thumbnail shape.
    static let columnWidth: CGFloat = maxSide

    /// Display size for a thumbnail of the given pixel dimensions.
    static func displaySize(pixelWidth: CGFloat, pixelHeight: CGFloat) -> CGSize {
        let w = max(pixelWidth, 1)
        let h = max(pixelHeight, 1)
        let clamped = min(max(w / h, 1 / maxAspect), maxAspect)
        if clamped >= 1 {
            // Wider than tall (or square): width drives the longest edge.
            return CGSize(width: maxSide, height: maxSide / clamped)
        } else {
            // Taller than wide: height drives the longest edge.
            return CGSize(width: maxSide * clamped, height: maxSide)
        }
    }
}

private enum HistoryThumbnailCache {
    private static let cache: NSCache<NSNumber, NSImage> = {
        let cache = NSCache<NSNumber, NSImage>()
        cache.countLimit = 30
        // 160px thumbnails cost ~100 KB each; 8 MB leaves headroom over the
        // 30-item count limit so the cache is bounded by count, not evictions.
        cache.totalCostLimit = 8 * 1024 * 1024
        return cache
    }()

    static func image(for id: Int64) -> NSImage? {
        cache.object(forKey: NSNumber(value: id))
    }

    static func insert(_ image: NSImage, for id: Int64) {
        let pixels = max(1, Int(image.size.width * image.size.height))
        cache.setObject(image, forKey: NSNumber(value: id), cost: pixels * 4)
    }

    static func removeAll() {
        cache.removeAllObjects()
    }
}
