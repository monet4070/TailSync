import SwiftUI

/// Date-range options for the history filter. Kept in the same order and
/// with the same business meaning as before the filter-bar rework.
enum HistoryDateFilter: String, CaseIterable, Identifiable {
    case all
    case today
    case yesterday
    case last7
    case last30
    case thisMonth
    case custom

    var id: String { rawValue }

    var label: String {
        switch self {
        case .all: return Loc.t("history.date.all")
        case .today: return Loc.t("history.date.today")
        case .yesterday: return Loc.t("history.date.yesterday")
        case .last7: return Loc.t("history.date.last7")
        case .last30: return Loc.t("history.date.last30")
        case .thisMonth: return Loc.t("history.date.thisMonth")
        case .custom: return Loc.t("history.date.custom")
        }
    }
}

/// Shared chrome for every control in the filter bar: one height, one corner
/// radius, one background/border pair, hover feedback and a disabled state.
/// Accent colour is intentionally not used here — emphasis is reserved for
/// selection, hover and focus (handled by the controls themselves).
struct FilterChip<Content: View>: View {
    var height: CGFloat = FilterBarMetrics.controlHeight
    var disabled = false
    @ViewBuilder var content: () -> Content

    @Environment(\.colorScheme) private var colorScheme
    @State private var hovering = false

    private var palette: TailSyncThemePalette {
        TailSyncThemeSelection(
            storedValue: Loc.shared.colorTheme,
            catalogue: Loc.shared.customThemes
        ).palette(for: colorScheme)
    }

    private var lineWidth: CGFloat {
        TailSyncThemeSelection(
            storedValue: Loc.shared.colorTheme,
            catalogue: Loc.shared.customThemes
        ).builtin == .highContrast ? 2 : 1
    }

    var body: some View {
        content()
            .frame(height: height)
            .padding(.horizontal, 10)
            .background(
                RoundedRectangle(cornerRadius: FilterBarMetrics.controlRadius, style: .continuous)
                    .fill(hovering && !disabled ? palette.accentColor.opacity(0.08) : palette.windowColor)
            )
            .overlay {
                RoundedRectangle(cornerRadius: FilterBarMetrics.controlRadius, style: .continuous)
                    .stroke(palette.borderColor, lineWidth: lineWidth)
            }
            .opacity(disabled ? 0.55 : 1)
            .onHover { hovering = $0 }
            .animation(.easeOut(duration: 0.12), value: hovering)
            .animation(.easeOut(duration: 0.12), value: disabled)
    }
}

/// Shared metrics for the filter bar so every control stays on the same grid.
enum FilterBarMetrics {
    static let controlHeight: CGFloat = 28
    static let controlRadius: CGFloat = 7
    static let rowSpacing: CGFloat = 8
}

/// A compact, anchored popover body for the filter menus. Each option is a
/// fixed-height row with hover, keyboard focus and a checkmark + light
/// background for the selected option. Width is content-driven with a cap so
/// the menu never approaches half the window. Generic over option ids so the
/// category and date filters share one implementation.
struct FilterMenuList: View {
    let options: [(id: String, label: String)]
    let selectedID: String
    let onSelect: (String) -> Void

    @Environment(\.colorScheme) private var colorScheme

    private var palette: TailSyncThemePalette {
        TailSyncThemeSelection(
            storedValue: Loc.shared.colorTheme,
            catalogue: Loc.shared.customThemes
        ).palette(for: colorScheme)
    }

    private var font: Font {
        let theme = TailSyncThemeSelection(
            storedValue: Loc.shared.colorTheme,
            catalogue: Loc.shared.customThemes
        )
        return theme.readingFont(size: 13)
    }

    var body: some View {
        VStack(spacing: 2) {
            ForEach(options, id: \.id) { option in
                FilterMenuRow(
                    label: option.label,
                    isSelected: option.id == selectedID,
                    font: font,
                    palette: palette,
                    onSelect: { onSelect(option.id) }
                )
            }
        }
        .padding(5)
        .frame(minWidth: 176, maxWidth: 220)
    }
}

private struct FilterMenuRow: View {
    let label: String
    let isSelected: Bool
    let font: Font
    let palette: TailSyncThemePalette
    let onSelect: () -> Void

    @State private var hovering = false

    var body: some View {
        Button(action: onSelect) {
            HStack(spacing: 8) {
                Text(label)
                    .font(font)
                    .foregroundColor(isSelected ? palette.accentColor : palette.primaryColor)
                    .lineLimit(1)
                    .truncationMode(.tail)
                Spacer(minLength: 8)
                Group {
                    if isSelected {
                        Image(systemName: "checkmark")
                            .font(.system(size: 11, weight: .semibold))
                            .foregroundColor(palette.accentColor)
                    } else {
                        Color.clear.frame(width: 12, height: 12)
                    }
                }
            }
            .frame(height: 26)
            .padding(.horizontal, 8)
            .contentShape(Rectangle())
            .background(
                RoundedRectangle(cornerRadius: 5, style: .continuous)
                    .fill(isSelected ? palette.accentColor.opacity(0.12) : (hovering ? palette.accentColor.opacity(0.08) : .clear))
            )
        }
        .buttonStyle(.plain)
        .onHover { hovering = $0 }
        .animation(.easeOut(duration: 0.1), value: hovering)
    }
}

/// Date-filter options for the popover (business meanings preserved).
private struct DateFilterOptions {
    static let all: [(id: String, label: String)] = HistoryDateFilter.allCases.map {
        (id: $0.rawValue, label: $0.label)
    }
}

/// The two-layer history filter bar:
///   row 1 — search field (with connection dot)
///   row 2 — category filter and date filter
/// plus the inline custom date range row when the date filter is `.custom`.
/// Behaviour (search submit, filter changes, custom dates) is passed back
/// through the callbacks; nothing here reaches into history data.
struct HistoryFilterBar: View {
    @Binding var keyword: String
    @Binding var selectedCategory: String
    @Binding var selectedDateFilter: HistoryDateFilter
    @Binding var customStartDate: Date
    @Binding var customEndDate: Date

    let categories: [String]
    let categoryFilteringSupported: Bool
    let dateRangeFilteringSupported: Bool
    let daemonOnline: Bool
    let onSubmit: () -> Void
    let onFilterChanged: () -> Void

    @Environment(\.colorScheme) private var colorScheme
    @State private var dateMenuOpen = false
    @State private var categoryMenuOpen = false

    private var activeTheme: TailSyncThemeSelection {
        TailSyncThemeSelection(storedValue: Loc.shared.colorTheme, catalogue: Loc.shared.customThemes)
    }

    private var palette: TailSyncThemePalette {
        activeTheme.palette(for: colorScheme)
    }

    private var searchFont: Font {
        activeTheme.typography.searchUsesDisplayFont
            ? activeTheme.displayFont(size: 13)
            : activeTheme.readingFont(size: 13)
    }

    private var selectedCategoryLabel: String {
        Loc.t("history.category.\(selectedCategory)")
    }

    var body: some View {
        VStack(spacing: FilterBarMetrics.rowSpacing) {
            // Row 1 — search
            HStack(spacing: 8) {
                FilterChip {
                    HStack(spacing: 6) {
                        Image(systemName: "magnifyingglass")
                            .font(.system(size: 11, weight: .medium))
                            .foregroundColor(palette.tertiaryColor)
                        TextField(Loc.t("history.search"), text: $keyword)
                            .textFieldStyle(.plain)
                            .font(searchFont)
                            .lineLimit(1)
                            .onSubmit(onSubmit)
                        if !keyword.isEmpty {
                            Button {
                                keyword = ""
                                onSubmit()
                            } label: {
                                Image(systemName: "xmark.circle.fill")
                                    .font(.system(size: 11))
                                    .foregroundColor(palette.tertiaryColor)
                            }
                            .buttonStyle(.plain)
                            .accessibilityLabel(Loc.t("history.search"))
                        }
                    }
                }
                .frame(maxWidth: .infinity)

                Circle()
                    .fill(daemonOnline ? palette.positiveColor : palette.warningColor)
                    .frame(width: 7, height: 7)
                    .accessibilityHidden(true)
            }

            // Row 2 — category + date filters
            HStack(spacing: FilterBarMetrics.rowSpacing) {
                Button {
                    categoryMenuOpen.toggle()
                } label: {
                    FilterChip(disabled: !categoryFilteringSupported) {
                        HStack(spacing: 6) {
                            Text(selectedCategoryLabel)
                                .font(activeTheme.readingFont(size: 12, weight: .medium))
                                .foregroundColor(palette.primaryColor)
                                .lineLimit(1)
                                .truncationMode(.tail)
                            Spacer(minLength: 4)
                            Image(systemName: "chevron.down")
                                .font(.system(size: 10, weight: .semibold))
                                .foregroundColor(palette.tertiaryColor)
                        }
                    }
                }
                .buttonStyle(.plain)
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity)
                .disabled(!categoryFilteringSupported)
                .accessibilityLabel(Loc.t("history.categoryFilter"))
                .popover(
                    isPresented: $categoryMenuOpen,
                    attachmentAnchor: .rect(.bounds),
                    arrowEdge: .bottom
                ) {
                    FilterMenuList(
                        options: categories.map { (id: $0, label: Loc.t("history.category.\($0)")) },
                        selectedID: selectedCategory
                    ) { category in
                        categoryMenuOpen = false
                        guard category != selectedCategory else { return }
                        selectedCategory = category
                        onFilterChanged()
                    }
                }

                Button {
                    dateMenuOpen.toggle()
                } label: {
                    FilterChip(disabled: !dateRangeFilteringSupported) {
                        HStack(spacing: 6) {
                            Text(Loc.t("history.dateFilter"))
                                .font(activeTheme.readingFont(size: 12, weight: .medium))
                                .foregroundColor(palette.primaryColor)
                                .lineLimit(1)
                                .truncationMode(.tail)
                            Spacer(minLength: 4)
                            Image(systemName: "chevron.down")
                                .font(.system(size: 10, weight: .semibold))
                                .foregroundColor(palette.tertiaryColor)
                        }
                    }
                }
                .buttonStyle(.plain)
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity)
                .disabled(!dateRangeFilteringSupported)
                .accessibilityLabel(Loc.t("history.dateFilter"))
                .popover(
                    isPresented: $dateMenuOpen,
                    attachmentAnchor: .rect(.bounds),
                    arrowEdge: .bottom
                ) {
                    FilterMenuList(
                        options: DateFilterOptions.all,
                        selectedID: selectedDateFilter.rawValue
                    ) { filterID in
                        dateMenuOpen = false
                        guard let filter = HistoryDateFilter(rawValue: filterID),
                              filter != selectedDateFilter else { return }
                        selectedDateFilter = filter
                        onFilterChanged()
                    }
                }
            }

            // Custom date range (inline, existing flow)
            if dateRangeFilteringSupported && selectedDateFilter == .custom {
                HStack(spacing: FilterBarMetrics.rowSpacing) {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(Loc.t("history.date.start"))
                            .font(.caption2)
                            .foregroundColor(palette.tertiaryColor)
                        FilterChip {
                            DatePicker(
                                Loc.t("history.date.start"),
                                selection: $customStartDate,
                                in: ...customEndDate,
                                displayedComponents: .date
                            )
                            .labelsHidden()
                            .datePickerStyle(.field)
                            .font(activeTheme.readingFont(size: 12))
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .onChange(of: customStartDate) { _ in onFilterChanged() }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)

                    VStack(alignment: .leading, spacing: 4) {
                        Text(Loc.t("history.date.end"))
                            .font(.caption2)
                            .foregroundColor(palette.tertiaryColor)
                        FilterChip {
                            DatePicker(
                                Loc.t("history.date.end"),
                                selection: $customEndDate,
                                in: customStartDate...,
                                displayedComponents: .date
                            )
                            .labelsHidden()
                            .datePickerStyle(.field)
                            .font(activeTheme.readingFont(size: 12))
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .onChange(of: customEndDate) { _ in onFilterChanged() }
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
                .stroke(palette.borderColor, lineWidth: activeTheme.builtin == .highContrast ? 2 : 1)
        }
    }
}
