import SwiftUI

struct HistoryPreviewTextToolbar: View {
    @Binding var mode: HistoryPreviewTextMode
    @Binding var query: String
    @Binding var wrapsLines: Bool
    @Binding var fontSize: Double

    let find: (Bool) -> Void
    let copyAll: () -> Void

    var body: some View {
        HStack(spacing: 8) {
            Picker("", selection: $mode) {
                Text(Loc.t("history.preview.plainText"))
                    .tag(HistoryPreviewTextMode.plain)
                Text(Loc.t("history.preview.code"))
                    .tag(HistoryPreviewTextMode.code)
            }
            .labelsHidden()
            .pickerStyle(.segmented)
            .frame(width: 150)

            searchControls
            Spacer(minLength: 8)

            Button { wrapsLines.toggle() } label: {
                Image(systemName: wrapsLines
                    ? "text.word.spacing"
                    : "arrow.left.and.right.text.vertical")
            }
            .help(Loc.t("history.preview.wrapLines"))
            Button {
                fontSize = HistoryPreviewPreferences.clampedTextFontSize(fontSize - 1)
            } label: {
                Image(systemName: "textformat.size.smaller")
            }
            .disabled(fontSize <= HistoryPreviewPreferences.minimumTextFontSize)
            .help(Loc.t("history.preview.decreaseFont"))
            Text("\(Int(fontSize))")
                .font(.caption.monospacedDigit())
                .frame(minWidth: 24)
            Button {
                fontSize = HistoryPreviewPreferences.clampedTextFontSize(fontSize + 1)
            } label: {
                Image(systemName: "textformat.size.larger")
            }
            .disabled(fontSize >= HistoryPreviewPreferences.maximumTextFontSize)
            .help(Loc.t("history.preview.increaseFont"))
            Button(action: copyAll) {
                Image(systemName: "doc.on.doc")
            }
            .help(Loc.t("history.preview.copyAll"))
        }
        .buttonStyle(.borderless)
        .controlSize(.small)
        .padding(.horizontal, 10)
        .frame(height: 42)
    }

    private var searchControls: some View {
        HStack(spacing: 5) {
            Image(systemName: "magnifyingglass")
                .foregroundStyle(.secondary)
            TextField(Loc.t("history.preview.search"), text: $query)
                .textFieldStyle(.plain)
                .onSubmit { find(true) }
            Button { find(false) } label: {
                Image(systemName: "chevron.up")
            }
            .disabled(query.isEmpty)
            .help(Loc.t("history.preview.previousMatch"))
            Button { find(true) } label: {
                Image(systemName: "chevron.down")
            }
            .disabled(query.isEmpty)
            .help(Loc.t("history.preview.nextMatch"))
        }
        .padding(.horizontal, 8)
        .frame(width: 260, height: 28)
        .background(.quaternary.opacity(0.45))
        .clipShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
    }
}
