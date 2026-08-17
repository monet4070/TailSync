import AppKit
import SwiftUI

enum HistoryPreviewTextMode: String, CaseIterable, Identifiable {
    case plain
    case code

    var id: String { rawValue }
}

struct HistoryPreviewTextView: View {
    let text: String

    @Environment(\.tailSyncPalette) private var palette
    @State private var mode: HistoryPreviewTextMode
    @State private var query = ""
    @State private var wrapsLines = true
    @AppStorage(HistoryPreviewPreferences.textFontSizeKey)
    private var fontSize = HistoryPreviewPreferences.defaultTextFontSize
    @State private var searchRevision = 0
    @State private var searchDirection = 1

    private let lineCount: Int
    private let characterCount: Int

    init(text: String, initiallyCode: Bool) {
        self.text = text
        _mode = State(initialValue: initiallyCode ? .code : .plain)
        lineCount = HistoryPreviewTextMetrics.lineCount(in: text)
        characterCount = text.count
    }

    var body: some View {
        VStack(spacing: 0) {
            HistoryPreviewTextToolbar(
                mode: $mode,
                query: $query,
                wrapsLines: $wrapsLines,
                fontSize: normalizedFontSizeBinding,
                find: find,
                copyAll: copyAll
            )
            HistoryPreviewTextEditor(
                text: text,
                isCode: mode == .code,
                wrapsLines: wrapsLines,
                fontSize: CGFloat(HistoryPreviewPreferences.clampedTextFontSize(fontSize)),
                searchQuery: query,
                searchRevision: searchRevision,
                searchDirection: searchDirection
            )
            .clipped()
            Divider().overlay(palette.dividerColor)
            HStack(spacing: 12) {
                Text("\(lineCount) \(Loc.t("history.preview.lines"))")
                Text("\(characterCount) \(Loc.t("history.preview.characters"))")
                Spacer()
            }
            .font(.caption2)
            .foregroundColor(palette.tertiaryColor)
            .padding(.horizontal, 16)
            .frame(height: 30)
            .background(palette.raisedColor)
        }
        .background(palette.surfaceColor)
        .background(HistoryPreviewModifierScrollMonitor { delta in
            fontSize = HistoryPreviewPreferences.textFontSize(
                afterModifierScroll: delta,
                current: fontSize
            )
        })
    }

    private var normalizedFontSizeBinding: Binding<Double> {
        Binding(
            get: { HistoryPreviewPreferences.clampedTextFontSize(fontSize) },
            set: { fontSize = HistoryPreviewPreferences.clampedTextFontSize($0) }
        )
    }

    private func find(forward: Bool) {
        guard !query.isEmpty else { return }
        searchDirection = forward ? 1 : -1
        searchRevision += 1
    }

    private func copyAll() {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
    }
}

enum HistoryPreviewTextMetrics {
    static func lineCount(in text: String) -> Int {
        guard !text.isEmpty else { return 0 }
        return text.reduce(into: 1) { count, character in
            if character == "\n" { count += 1 }
        }
    }
}
