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
                        .onEnded { _ in save(.historyLimit(settings.history_limit)) }
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
        save(.historyLimit(settings.history_limit))
    }
}
