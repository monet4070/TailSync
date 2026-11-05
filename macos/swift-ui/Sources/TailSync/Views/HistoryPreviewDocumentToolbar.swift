import SwiftUI

struct HistoryPreviewDocumentToolbar: View {
    @Binding var query: String
    @Binding var fontSize: Double
    @Binding var lineSpacing: HistoryPreviewDocumentReadingOptions.LineSpacing
    @Binding var comfortableWidth: Bool
    let searchSummary: HistoryPreviewDocumentSearchSummary
    let find: (Bool) -> Void
    let resetReading: () -> Void
    let copyText: () -> Void

    @Environment(\.tailSyncPalette) private var palette

    var body: some View {
        HStack(spacing: 10) {
            searchControls
            Spacer(minLength: 0)
            fontControls
            Menu {
                Picker(Loc.t("history.preview.document.lineSpacing"), selection: $lineSpacing) {
                    ForEach(HistoryPreviewDocumentReadingOptions.LineSpacing.allCases) { spacing in
                        Text(Loc.t(spacing.labelKey)).tag(spacing)
                    }
                }
                Toggle(Loc.t("history.preview.document.comfortableWidth"), isOn: $comfortableWidth)
                Divider()
                Button(Loc.t("history.preview.document.resetReading"), action: resetReading)
            } label: {
                Label(Loc.t("history.preview.document.reading"), systemImage: "slider.horizontal.3")
            }
            .controlSize(.regular)
            .fixedSize()

            Button(action: copyText) {
                Label(Loc.t("history.preview.document.copyText"), systemImage: "doc.on.doc")
                    .font(.system(size: 12, weight: .semibold))
                    .frame(minHeight: 30)
            }
            .buttonStyle(.bordered)
            .controlSize(.regular)
            .fixedSize()
        }
        .historyPreviewToolbarStyle()
    }

    private var fontControls: some View {
        HStack(spacing: 0) {
            HistoryPreviewToolbarIconButton(systemName: "minus") { fontSize -= 1 }
                .disabled(fontSize <= HistoryPreviewPreferences.minimumTextFontSize)
                .help(Loc.t("history.preview.decreaseFont"))
                .accessibilityLabel(Loc.t("history.preview.decreaseFont"))
            Text("\(Int(fontSize)) pt")
                .font(.system(size: 12, weight: .semibold).monospacedDigit())
                .foregroundColor(palette.primaryColor)
                .frame(width: 46)
                .accessibilityLabel(Loc.t("history.preview.document.fontSize").replacingOccurrences(of: "{size}", with: "\(Int(fontSize))"))
            HistoryPreviewToolbarIconButton(systemName: "plus") { fontSize += 1 }
                .disabled(fontSize >= HistoryPreviewPreferences.maximumTextFontSize)
                .help(Loc.t("history.preview.increaseFont"))
                .accessibilityLabel(Loc.t("history.preview.increaseFont"))
        }
        .padding(2)
        .background(palette.softSurfaceColor)
        .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
        .overlay {
            RoundedRectangle(cornerRadius: 8, style: .continuous)
                .stroke(palette.borderColor, lineWidth: 1)
        }
        .help(Loc.t("history.preview.document.fontSizeHelp"))
    }

    private var searchControls: some View {
        HStack(spacing: 4) {
            Image(systemName: "magnifyingglass")
                .font(.system(size: 13, weight: .medium))
                .foregroundColor(palette.tertiaryColor)
            TextField(Loc.t("history.preview.search"), text: $query)
                .textFieldStyle(.plain)
                .font(.system(size: 13))
                .onSubmit { find(true) }
                .accessibilityLabel(Loc.t("history.preview.search"))
            if !query.isEmpty {
                Text(searchSummary.label)
                    .font(.caption2.monospacedDigit())
                    .foregroundColor(palette.tertiaryColor)
                    .fixedSize()
                HistoryPreviewToolbarIconButton(systemName: "xmark", compact: true) { query = "" }
                    .help(Loc.t("history.preview.document.clearSearch"))
                    .accessibilityLabel(Loc.t("history.preview.document.clearSearch"))
            }
            HistoryPreviewToolbarIconButton(systemName: "chevron.up", compact: true) { find(false) }
                .disabled(searchSummary.count == 0)
                .help(Loc.t("history.preview.previousMatch"))
                .accessibilityLabel(Loc.t("history.preview.previousMatch"))
            HistoryPreviewToolbarIconButton(systemName: "chevron.down", compact: true) { find(true) }
                .disabled(searchSummary.count == 0)
                .help(Loc.t("history.preview.nextMatch"))
                .accessibilityLabel(Loc.t("history.preview.nextMatch"))
        }
        .padding(.leading, 10)
        .padding(.trailing, 4)
        .frame(minWidth: 190, maxWidth: .infinity, minHeight: 36, maxHeight: 36)
        .background(palette.softSurfaceColor)
        .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
        .overlay {
            RoundedRectangle(cornerRadius: 8, style: .continuous)
                .stroke(palette.borderColor, lineWidth: 1)
        }
    }
}
