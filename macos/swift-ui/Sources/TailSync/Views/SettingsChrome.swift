import SwiftUI

/// Shared card and row styling for both settings windows.
protocol SettingsChrome: View {
    var activeTheme: TailSyncThemeSelection { get }
    var colorScheme: ColorScheme { get }
}

extension SettingsView: SettingsChrome {}
extension ConnectionsView: SettingsChrome {}

extension SettingsChrome {
    var palette: TailSyncThemePalette {
        activeTheme.palette(for: colorScheme)
    }

    func component(_ name: String, state: String = "default") -> TailSyncThemeComponentTokens? {
        activeTheme.component(name, state: state, scheme: colorScheme)
    }

    func settingsCard<Content: View>(title: String, @ViewBuilder content: () -> Content) -> some View {
        let section = component("section", state: "default")
        let panel = component("panel", state: "default")
        return VStack(alignment: .leading, spacing: 0) {
            Text(title)
                .font(activeTheme.displayFont(
                    size: activeTheme.typography.sectionTitleSize,
                    weight: activeTheme.builtin == .tailsync ? .regular : .semibold
                ))
                .textCase(activeTheme.typography.uppercasesSectionTitles ? .uppercase : nil)
                .foregroundColor(section?.foregroundColor ?? palette.secondaryColor)
                .padding(.horizontal, 16)
                .padding(.bottom, 6)
            VStack(spacing: 0) { content() }
                .background(panel?.backgroundColor ?? palette.surfaceColor)
                .clipShape(RoundedRectangle(cornerRadius: panel?.radius ?? activeTheme.metrics.cardRadius, style: .continuous))
                .overlay {
                    RoundedRectangle(cornerRadius: panel?.radius ?? activeTheme.metrics.cardRadius, style: .continuous)
                        .stroke(panel?.borderColor ?? palette.borderColor, lineWidth: activeTheme.builtin == .highContrast ? 2 : 1)
                }
                .shadow(
                    color: palette.primaryColor.opacity(panel?.shadowOpacity ?? (activeTheme.metrics.shadowRadius == 0 ? 0 : 0.08)),
                    radius: panel?.shadowRadius ?? activeTheme.metrics.shadowRadius,
                    y: panel?.shadowY ?? (activeTheme.metrics.shadowRadius > 0 ? 3 : 0)
                )
                .padding(.horizontal, 12)
        }
    }

    func settingRow<Content: View>(@ViewBuilder content: () -> Content) -> some View {
        HStack(spacing: 8) { content() }
            .font(activeTheme.readingFont(size: 13))
            .padding(.horizontal, 16)
            .padding(.vertical, activeTheme.metrics.rowPadding)
            .frame(minHeight: 36)
    }

    var themedDivider: some View {
        Rectangle()
            .fill(palette.dividerColor)
            .frame(height: activeTheme.builtin == .highContrast ? 2 : 1)
    }
}
