import SwiftUI

struct HistoryPreviewToolbarIconButton: View {
    @Environment(\.tailSyncPalette) private var palette

    let systemName: String
    var selected = false
    var compact = false
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Image(systemName: systemName)
                .font(.system(size: compact ? 11 : 12, weight: .semibold))
                .foregroundColor(selected ? palette.accentColor : palette.secondaryColor)
                .frame(width: controlSize, height: controlSize)
                .background(selected ? palette.accentColor.opacity(0.12) : Color.clear)
                .clipShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    private var controlSize: CGFloat {
        compact
            ? HistoryPreviewLayoutMetrics.compactControlSize
            : HistoryPreviewLayoutMetrics.regularControlSize
    }
}

extension View {
    func historyPreviewToolbarStyle() -> some View {
        modifier(HistoryPreviewToolbarStyle())
    }
}

private struct HistoryPreviewToolbarStyle: ViewModifier {
    @Environment(\.tailSyncPalette) private var palette

    func body(content: Content) -> some View {
        content
            .padding(.horizontal, 14)
            .frame(height: HistoryPreviewLayoutMetrics.toolbarHeight)
            .background(palette.raisedColor)
            .overlay(alignment: .bottom) {
                Rectangle().fill(palette.dividerColor).frame(height: 1)
            }
    }
}
