import SwiftUI

struct SingleKeycapView: View {
    let key: String
    let palette: PreviewThemePalette

    var body: some View {
        Text(key)
            .font(.system(size: 11, weight: .semibold, design: .rounded))
            .foregroundColor(palette.primaryColor)
            .padding(.horizontal, 6)
            .padding(.vertical, 3)
            .background(
                RoundedRectangle(cornerRadius: 4, style: .continuous)
                    .fill(palette.surfaceColor)
            )
            .overlay(
                RoundedRectangle(cornerRadius: 4, style: .continuous)
                    .stroke(palette.borderColor, lineWidth: 1)
            )
            .shadow(color: Color.black.opacity(0.08), radius: 1, y: 1)
    }
}

struct KeycapsGroupView: View {
    let keys: [String]
    let palette: PreviewThemePalette

    var body: some View {
        HStack(spacing: 3) {
            ForEach(Array(keys.enumerated()), id: \.offset) { _, key in
                SingleKeycapView(key: key, palette: palette)
            }
        }
    }
}
