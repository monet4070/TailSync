import SwiftUI

struct ShortcutRowView: View {
    let title: String
    let description: String
    @Binding var keys: [String]
    let defaultKeys: [String]
    let palette: PreviewThemePalette
    @State private var isRecording = false

    var body: some View {
        HStack(alignment: .center, spacing: 8) {
            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                    .font(.system(size: 13, weight: .medium))
                    .foregroundColor(palette.primaryColor)
                Text(description)
                    .font(.system(size: 11))
                    .foregroundColor(palette.tertiaryColor)
            }
            Spacer(minLength: 16)

            HStack(spacing: 6) {
                Button(action: {
                    isRecording.toggle()
                }) {
                    HStack(spacing: 6) {
                        Image(systemName: "keyboard")
                            .font(.system(size: 11))
                            .foregroundColor(palette.secondaryColor)
                        if isRecording {
                            Text("按下新按键…")
                                .font(.system(size: 11))
                                .foregroundColor(palette.accentColor)
                        } else if keys.isEmpty {
                            Text("未设置")
                                .font(.system(size: 11))
                                .foregroundColor(palette.tertiaryColor)
                        } else {
                            KeycapsGroupView(keys: keys, palette: palette)
                        }
                        Image(systemName: "pencil")
                            .font(.system(size: 10))
                            .foregroundColor(palette.tertiaryColor)
                    }
                    .padding(.horizontal, 8)
                    .padding(.vertical, 4)
                    .background(palette.surfaceColor)
                    .cornerRadius(6)
                    .overlay(
                        RoundedRectangle(cornerRadius: 6)
                            .stroke(isRecording ? palette.accentColor : palette.borderColor, lineWidth: 1)
                    )
                }
                .buttonStyle(.plain)

                // 重置按钮
                Button(action: { keys = defaultKeys }) {
                    Image(systemName: "arrow.counterclockwise")
                        .font(.system(size: 11))
                        .foregroundColor(keys == defaultKeys ? palette.tertiaryColor.opacity(0.4) : palette.secondaryColor)
                        .frame(width: 24, height: 24)
                        .background(palette.surfaceColor)
                        .cornerRadius(5)
                        .overlay(RoundedRectangle(cornerRadius: 5).stroke(palette.borderColor, lineWidth: 1))
                }
                .buttonStyle(.plain)
                .disabled(keys == defaultKeys)

                // 清除按钮
                Button(action: { keys = [] }) {
                    Image(systemName: "xmark")
                        .font(.system(size: 11))
                        .foregroundColor(keys.isEmpty ? palette.tertiaryColor.opacity(0.4) : palette.secondaryColor)
                        .frame(width: 24, height: 24)
                        .background(palette.surfaceColor)
                        .cornerRadius(5)
                        .overlay(RoundedRectangle(cornerRadius: 5).stroke(palette.borderColor, lineWidth: 1))
                }
                .buttonStyle(.plain)
                .disabled(keys.isEmpty)
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 8)
    }
}
