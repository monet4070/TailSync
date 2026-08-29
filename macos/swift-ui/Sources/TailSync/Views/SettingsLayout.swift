import SwiftUI
extension SettingsView {
    var historyLimitControl: some View {
        HStack(spacing: 12) {
            GeometryReader { geometry in
                let thumbSize: CGFloat = 18
                let travelWidth = max(1, geometry.size.width - thumbSize)
                let progress = CGFloat(settings.history_limit - 10) / 490

                ZStack(alignment: .leading) {
                    Capsule()
                        .fill(palette.borderColor.opacity(0.65))
                        .frame(height: 5)
                        .padding(.horizontal, thumbSize / 2)

                    Capsule()
                        .fill(palette.accentColor)
                        .frame(width: max(1, travelWidth * progress), height: 5)
                        .offset(x: thumbSize / 2)

                    Circle()
                        .fill(palette.raisedColor)
                        .overlay {
                            Circle()
                                .stroke(palette.accentColor.opacity(0.75), lineWidth: 1)
                        }
                        .shadow(color: .black.opacity(0.18), radius: 2.5, y: 1)
                        .frame(width: thumbSize, height: thumbSize)
                        .offset(x: travelWidth * progress)
                }
                .frame(maxHeight: .infinity)
                .contentShape(Rectangle())
                .gesture(
                    DragGesture(minimumDistance: 0)
                        .onChanged { value in
                            let position = min(travelWidth, max(0, value.location.x - thumbSize / 2))
                            let step = Int((position / travelWidth * 49).rounded())
                            settings.history_limit = 10 + step * 10
                        }
                        .onEnded { _ in save() }
                )
                .accessibilityElement()
                .accessibilityLabel(Loc.t("settings.limit"))
                .accessibilityValue("\(settings.history_limit)")
                .accessibilityAdjustableAction { direction in
                    switch direction {
                    case .increment: adjustHistoryLimit(by: 10)
                    case .decrement: adjustHistoryLimit(by: -10)
                    @unknown default: break
                    }
                }
            }
            .frame(width: 180, height: 28)

            Text("\(settings.history_limit)")
                .font(.system(.caption, design: .monospaced).weight(.medium))
                .foregroundColor(palette.accentColor)
                .frame(width: 46, height: 26)
                .background(palette.accentSoftColor)
                .clipShape(RoundedRectangle(cornerRadius: activeTheme.metrics.controlRadius, style: .continuous))
        }
    }

    func adjustHistoryLimit(by delta: Int) {
        settings.history_limit = min(500, max(10, settings.history_limit + delta))
        save()
    }

    func settingsCard<Content: View>(title: String, @ViewBuilder content: () -> Content) -> some View {
        let section = component("section")
        let panel = component("panel")
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
