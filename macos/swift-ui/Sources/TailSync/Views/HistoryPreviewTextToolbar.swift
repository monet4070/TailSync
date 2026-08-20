import SwiftUI

struct HistoryPreviewTextToolbar: View {
    @Environment(\.tailSyncPalette) private var palette

    @Binding var mode: HistoryPreviewTextMode
    @Binding var query: String
    @Binding var wrapsLines: Bool
    @Binding var fontSize: Double

    let find: (Bool) -> Void
    let copyAll: () -> Void

    var body: some View {
        HStack(spacing: 10) {
            Picker("", selection: $mode) {
                Text(Loc.t("history.preview.plainText"))
                    .tag(HistoryPreviewTextMode.plain)
                Text(Loc.t("history.preview.code"))
                    .tag(HistoryPreviewTextMode.code)
            }
            .labelsHidden()
            .pickerStyle(.segmented)
            .frame(
                width: HistoryPreviewLayoutMetrics.segmentedControlWidth,
                height: HistoryPreviewLayoutMetrics.regularControlSize
            )

            searchControls
                .layoutPriority(1)
            Spacer(minLength: 4)

            HistoryPreviewToolbarIconButton(
                systemName: wrapsLines
                    ? "text.word.spacing"
                    : "arrow.left.and.right.text.vertical",
                selected: wrapsLines,
                action: { wrapsLines.toggle() }
            )
            .help(Loc.t("history.preview.wrapLines"))

            HStack(spacing: 0) {
                HistoryPreviewToolbarIconButton(systemName: "minus", action: {
                    fontSize = HistoryPreviewPreferences.clampedTextFontSize(fontSize - 1)
                })
                .disabled(fontSize <= HistoryPreviewPreferences.minimumTextFontSize)
                .help(Loc.t("history.preview.decreaseFont"))

                Text("\(Int(fontSize))")
                    .font(.system(size: 12, weight: .semibold).monospacedDigit())
                    .foregroundColor(palette.primaryColor)
                    .frame(width: 34)

                HistoryPreviewToolbarIconButton(systemName: "plus", action: {
                    fontSize = HistoryPreviewPreferences.clampedTextFontSize(fontSize + 1)
                })
                .disabled(fontSize >= HistoryPreviewPreferences.maximumTextFontSize)
                .help(Loc.t("history.preview.increaseFont"))
            }
            .padding(2)
            .background(palette.softSurfaceColor)
            .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
            .overlay {
                RoundedRectangle(cornerRadius: 8, style: .continuous)
                    .stroke(palette.borderColor, lineWidth: 1)
            }

            Button(action: copyAll) {
                Label(Loc.t("history.preview.copyAll"), systemImage: "doc.on.doc")
                    .font(.system(size: 12, weight: .semibold))
                    .frame(minHeight: 30)
            }
            .buttonStyle(.bordered)
            .controlSize(.regular)
            .help(Loc.t("history.preview.copyAll"))
        }
        .historyPreviewToolbarStyle()
    }

    private var searchControls: some View {
        HStack(spacing: 6) {
            Image(systemName: "magnifyingglass")
                .font(.system(size: 13, weight: .medium))
                .foregroundColor(palette.tertiaryColor)
            TextField(Loc.t("history.preview.search"), text: $query)
                .textFieldStyle(.plain)
                .font(.system(size: 13))
                .onSubmit { find(true) }
            HistoryPreviewToolbarIconButton(
                systemName: "chevron.up",
                compact: true,
                action: { find(false) }
            )
            .disabled(query.isEmpty)
            .help(Loc.t("history.preview.previousMatch"))
            HistoryPreviewToolbarIconButton(
                systemName: "chevron.down",
                compact: true,
                action: { find(true) }
            )
            .disabled(query.isEmpty)
            .help(Loc.t("history.preview.nextMatch"))
        }
        .padding(.leading, 10)
        .padding(.trailing, 4)
        .frame(
            minWidth: 160,
            maxWidth: 300,
            minHeight: HistoryPreviewLayoutMetrics.regularControlSize,
            maxHeight: HistoryPreviewLayoutMetrics.regularControlSize
        )
        .background(palette.softSurfaceColor)
        .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
        .overlay {
            RoundedRectangle(cornerRadius: 8, style: .continuous)
                .stroke(palette.borderColor, lineWidth: 1)
        }
    }

}
