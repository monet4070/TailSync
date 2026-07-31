import Foundation
import SwiftUI

private enum HistoryDateFilter: String, CaseIterable, Identifiable {
    case all
    case today
    case yesterday
    case last7
    case last30
    case thisMonth
    case custom

    var id: String { rawValue }

    func label(language: String) -> String {
        let chinese = language == "zh-CN"
        switch self {
        case .all: return chinese ? "\u{5168}\u{90E8}\u{65E5}\u{671F}" : "All dates"
        case .today: return chinese ? "\u{4ECA}\u{5929}" : "Today"
        case .yesterday: return chinese ? "\u{6628}\u{5929}" : "Yesterday"
        case .last7: return chinese ? "\u{8FD1} 7 \u{5929}" : "Last 7 days"
        case .last30: return chinese ? "\u{8FD1} 30 \u{5929}" : "Last 30 days"
        case .thisMonth: return chinese ? "\u{672C}\u{6708}" : "This month"
        case .custom: return chinese ? "\u{81EA}\u{5B9A}\u{4E49}" : "Custom"
        }
    }
}

private struct HistoryDateBounds {
    let start: Date?
    let end: Date?
}

struct HistoryView: View {
    @ObservedObject private var loc = Loc.shared
    @Environment(\.colorScheme) private var colorScheme
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
    @State private var errorMsg: String? = nil
    @State private var lastVersion: UInt64 = 0
    @State private var daemonOnline = false
    @State private var fileProgress: (name: String, sent: UInt64, total: UInt64, active: Bool)? = nil
    @State private var showingClearAlert = false
    @State private var loadGeneration = 0
    @State private var supportedCategories: Set<String> = []
    @State private var multipleLabelsSupported = false
    @State private var dateRangeFilteringSupported = false
    @State private var historyCapabilitiesChecked = false
    @State private var historyCapabilitiesLoading = false

    private let pageSize = 30
    private let knownCategories = ["text", "website", "code", "command", "structured_data", "path", "image", "file"]

    private var activeTheme: TailSyncColorTheme {
        TailSyncColorTheme(storedValue: loc.colorTheme)
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

    private var dateFilterTitle: String {
        loc.lang == "zh-CN" ? "\u{6309}\u{65E5}\u{671F}\u{7B5B}\u{9009}" : "Filter by date"
    }

    private var customStartTitle: String {
        loc.lang == "zh-CN" ? "\u{5F00}\u{59CB}" : "Start"
    }

    private var customEndTitle: String {
        loc.lang == "zh-CN" ? "\u{7ED3}\u{675F}" : "End"
    }

    var body: some View {
        VStack(spacing: 0) {
            VStack(spacing: 6) {
                HStack {
                    Image(systemName: "magnifyingglass").foregroundColor(palette.tertiaryColor)
                    TextField(Loc.t("history.search"), text: $keyword)
                        .textFieldStyle(.plain)
                        .font(activeTheme.typography.searchUsesDisplayFont
                              ? activeTheme.displayFont(size: activeTheme.typography.searchSize)
                              : activeTheme.readingFont(size: activeTheme.typography.searchSize))
                        .onSubmit { page = 0; load(targetPage: 0) }
                    if !keyword.isEmpty {
                        Button { keyword = ""; page = 0; load(targetPage: 0) } label: {
                            Image(systemName: "xmark.circle.fill").foregroundColor(palette.tertiaryColor)
                        }.buttonStyle(.plain)
                    }
                    Circle()
                        .fill(daemonOnline ? palette.positiveColor : palette.warningColor)
                        .frame(width: 7, height: 7)
                }

                HStack(spacing: 8) {
                    Picker(Loc.t("history.categoryFilter"), selection: $selectedCategory) {
                        ForEach(categories, id: \.self) { category in
                            Text(Loc.t("history.category.\(category)")).tag(category)
                        }
                    }
                    .labelsHidden()
                    .pickerStyle(.menu)
                    .frame(maxWidth: .infinity)
                    .disabled(!categoryFilteringSupported)
                    .onChange(of: selectedCategory) { _ in page = 0; load(targetPage: 0) }

                    Picker(dateFilterTitle, selection: $selectedDateFilter) {
                        ForEach(HistoryDateFilter.allCases) { filter in
                            Text(filter.label(language: loc.lang)).tag(filter)
                        }
                    }
                    .labelsHidden()
                    .pickerStyle(.menu)
                    .frame(maxWidth: .infinity)
                    .disabled(!dateRangeFilteringSupported)
                    .onChange(of: selectedDateFilter) { _ in page = 0; load(targetPage: 0) }
                }

                if dateRangeFilteringSupported && selectedDateFilter == .custom {
                    HStack(spacing: 8) {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(customStartTitle).font(.caption2).foregroundColor(palette.tertiaryColor)
                            DatePicker(
                                customStartTitle,
                                selection: $customStartDate,
                                in: ...customEndDate,
                                displayedComponents: .date
                            )
                            .labelsHidden()
                            .onChange(of: customStartDate) { _ in page = 0; load(targetPage: 0) }
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)

                        VStack(alignment: .leading, spacing: 2) {
                            Text(customEndTitle).font(.caption2).foregroundColor(palette.tertiaryColor)
                            DatePicker(
                                customEndTitle,
                                selection: $customEndDate,
                                in: customStartDate...,
                                displayedComponents: .date
                            )
                            .labelsHidden()
                            .onChange(of: customEndDate) { _ in page = 0; load(targetPage: 0) }
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
            }
            .padding(8)
            .background(palette.softSurfaceColor)
            .clipShape(RoundedRectangle(cornerRadius: activeTheme.metrics.controlRadius, style: .continuous))
            .overlay {
                RoundedRectangle(cornerRadius: activeTheme.metrics.controlRadius, style: .continuous)
                    .stroke(palette.borderColor, lineWidth: activeTheme == .highContrast ? 2 : 1)
            }
            .padding(.horizontal, 12).padding(.top, 8)

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
                    Image(systemName: "doc.on.clipboard")
                        .font(.system(size: 32))
                        .foregroundColor(palette.tertiaryColor)
                    Text(Loc.t("history.empty"))
                        .font(activeTheme.displayFont(size: 15))
                        .foregroundColor(palette.secondaryColor)
                }
                Spacer()
            } else {
                List {
                    ForEach(entries) { entry in
                        HistoryRow(
                            entry: entry,
                            isRestored: restoredId == entry.id,
                            showsMultipleLabels: multipleLabelsSupported
                        )
                            .contentShape(Rectangle())
                            .onTapGesture(count: 2) { restore(entry.id) }
                            .overlay(RightClickNSView { delete(entry.id) })
                            .listRowBackground(palette.surfaceColor)
                            .listRowSeparatorTint(palette.dividerColor)
                            .transition(.opacity.combined(with: .scale(scale: 0.95)))
                    }
                }
                .listStyle(.plain)
                .scrollContentBackground(.hidden)
                .background(palette.windowColor)
                .animation(.spring(response: 0.35, dampingFraction: 0.9), value: entries.count)
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
                Rectangle().fill(palette.dividerColor).frame(height: activeTheme == .highContrast ? 2 : 1)
            }
            .alert(Loc.t("history.confirmClear"), isPresented: $showingClearAlert) {
                Button("Cancel", role: .cancel) {}
                Button(Loc.t("history.clearAll"), role: .destructive) { clearAll() }
            }
        }
        .onAppear {
            load()
            loadHistoryCapabilities()
        }
        .onReceive(Timer.publish(every: 0.5, on: .main, in: .common).autoconnect()) { _ in
            Task {
                if let version = await ApiClient.shared.getVersion() {
                    let reconnected = !daemonOnline
                    if reconnected || version != lastVersion {
                        lastVersion = version
                        load(targetPage: 0)
                    }
                    daemonOnline = true
                    if reconnected {
                        historyCapabilitiesChecked = false
                        loadHistoryCapabilities()
                    }
                } else {
                    daemonOnline = false
                }
                if !historyCapabilitiesChecked { loadHistoryCapabilities() }
                fileProgress = await ApiClient.shared.getFileProgress()
            }
        }
        .onReceive(NotificationCenter.default.publisher(for: .NSCalendarDayChanged)) { _ in
            reloadActiveDateFilter()
        }
        .onReceive(NotificationCenter.default.publisher(for: .NSSystemTimeZoneDidChange)) { _ in
            reloadActiveDateFilter()
        }
        .overlay(alignment: .bottom) {
            VStack(spacing: 6) {
                if let p = fileProgress, p.active {
                    VStack(spacing: 3) {
                        ProgressView(value: Double(p.sent), total: Double(p.total)).progressViewStyle(.linear)
                        Text("Sending \(p.name) — \(p.sent)/\(p.total)")
                            .font(.caption2)
                            .foregroundColor(palette.secondaryColor)
                    }
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
        .background(palette.windowColor)
        .tailSyncThemed()
    }

    private func load(targetPage: Int? = nil) {
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
        entries = []
        hasNext = false
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
        Task {
            try? await ApiClient.shared.restoreEntry(id: id)
            withAnimation(.easeOut(duration: 0.4)) { restoredId = id }
            try? await Task.sleep(nanoseconds: 1_500_000_000)
            withAnimation(.easeOut(duration: 0.4)) { restoredId = nil }
        }
    }

    private func delete(_ id: Int64) {
        Task { try? await ApiClient.shared.deleteEntry(id: id); entries.removeAll { $0.id == id } }
    }

    private func clearAll() {
        Task {
            let ok = await ApiClient.shared.clearAllHistory()
            if ok { entries = []; page = 0; hasNext = false }
        }
    }
}

// ── Row ──────────────────────────────────────────────────────────

struct HistoryRow: View {
    let entry: HistoryEntry
    let isRestored: Bool
    let showsMultipleLabels: Bool
    @State private var thumbnail: NSImage? = nil
    @Environment(\.tailSyncTheme) private var theme
    @Environment(\.tailSyncPalette) private var palette

    private var visibleCategoryLabels: [String] {
        showsMultipleLabels ? entry.categoryLabels : [entry.categoryLabel]
    }

    var body: some View {
        HStack(spacing: 10) {
            Group {
                if entry.type == "image", let thumb = thumbnail {
                    Image(nsImage: thumb).resizable().aspectRatio(contentMode: .fill)
                        .frame(width: 32, height: 32)
                        .clipShape(RoundedRectangle(cornerRadius: theme.metrics.controlRadius, style: .continuous))
                } else {
                    Image(systemName: entry.icon).frame(width: 24, height: 24)
                        .foregroundColor(palette.accentColor)
                        .background(palette.accentColor.opacity(0.1))
                        .clipShape(RoundedRectangle(cornerRadius: theme.metrics.controlRadius, style: .continuous))
                }
            }
            .frame(width: 32, height: 32)
            .task { if entry.type == "image", let img = await ApiClient.shared.getImageData(id: entry.id) { thumbnail = rgbaToImage(img) } }

            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(visibleCategoryLabels.map { $0.uppercased() }.joined(separator: "  \u{00B7}  "))
                        .font(theme.readingFont(size: 10, weight: .semibold))
                        .foregroundColor(palette.accentColor).padding(.horizontal, 4).padding(.vertical, 1)
                        .background(palette.accentColor.opacity(0.12))
                        .clipShape(RoundedRectangle(cornerRadius: theme.metrics.controlRadius, style: .continuous))
                        .fixedSize(horizontal: false, vertical: true)
                        .layoutPriority(1)
                    Text(entry.formattedTime).font(.caption).foregroundColor(palette.tertiaryColor)
                }
                Text(entry.description)
                    .font(theme == .tailsync
                          ? theme.displayFont(size: theme.typography.historyContentSize)
                          : theme == .forest
                              ? theme.displayFont(size: theme.typography.historyContentSize, weight: .medium)
                              : theme.readingFont(size: theme.typography.historyContentSize, weight: .medium))
                    .foregroundColor(palette.primaryColor)
                    .lineLimit(1)
                HStack(spacing: 6) {
                    Text(entry.formattedSize).font(.caption2).foregroundColor(palette.tertiaryColor).monospacedDigit()
                    Spacer()
                    Text(entry.source_peer).font(.caption2).foregroundColor(palette.tertiaryColor).lineLimit(1)
                }
            }
        }
        .padding(.vertical, max(2, (theme.metrics.rowPadding - 6) / 2))
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(isRestored ? palette.accentColor.opacity(0.12) : .clear)
        .clipShape(RoundedRectangle(cornerRadius: theme.metrics.controlRadius, style: .continuous))
        .contentShape(Rectangle())
        .animation(.easeOut(duration: 0.4), value: isRestored)
    }
}

// ── Helpers ──────────────────────────────────────────────────────

private func rgbaToImage(_ data: ApiClient.ImageData) -> NSImage? {
    let w = data.width, h = data.height
    guard w > 0, h > 0, data.rgba.count >= w * h * 4 else { return nil }
    guard let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: w, pixelsHigh: h,
                                      bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                                      colorSpaceName: .deviceRGB, bytesPerRow: w * 4, bitsPerPixel: 32) else { return nil }
    data.rgba.copyBytes(to: rep.bitmapData!, count: data.rgba.count)
    let img = NSImage(size: NSSize(width: w, height: h))
    img.addRepresentation(rep)
    return img
}

// ── Right-click capture ─────────────────────────────────────────

/// Overlays each row to intercept right-clicks directly via `rightMouseDown`.
/// Much safer than `NSEvent.addLocalMonitorForEvents` which is global and
/// crashes when the monitored view is deallocated during event propagation.
struct RightClickNSView: NSViewRepresentable {
    let action: () -> Void

    func makeNSView(context: Context) -> NSView {
        let view = RightClickView(frame: .zero)
        view.action = action
        return view
    }

    func updateNSView(_ nsView: NSView, context: Context) {
        if let v = nsView as? RightClickView {
            v.action = action
        }
    }

    class RightClickView: NSView {
        var action: (() -> Void)?

        override func hitTest(_ point: NSPoint) -> NSView? {
            // Only claim right-clicks; pass everything else through
            // to the SwiftUI gesture recognizers (double-tap, etc.).
            guard let event = NSApp.currentEvent else { return nil }
            if event.type == .rightMouseDown || event.type == .rightMouseUp {
                return self
            }
            return nil
        }

        override func rightMouseDown(with event: NSEvent) {
            // Defer to next run-loop so the view is still alive when the
            // action removes the row from the List.
            let act = action
            DispatchQueue.main.async { act?() }
        }
    }
}
