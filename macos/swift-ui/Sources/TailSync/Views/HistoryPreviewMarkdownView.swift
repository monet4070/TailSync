import AppKit
import SwiftUI
import WebKit

struct HistoryMarkdownPreviewView: View {
    let source: String
    private let showsToolbar: Bool
    @Environment(\.tailSyncPalette) private var palette
    @AppStorage(HistoryPreviewPreferences.textFontSizeKey)
    private var fontSize = HistoryPreviewPreferences.defaultTextFontSize

    init(source: String, showsToolbar: Bool = true) {
        self.source = source
        self.showsToolbar = showsToolbar
    }

    var body: some View {
        VStack(spacing: 0) {
            if showsToolbar { markdownToolbar }
            HistoryMarkdownWebView(source: source,
                appearance: HistoryMarkdownWebRenderer.appearance(palette: palette, fontSize: fontSize))
        }
        .background(palette.surfaceColor)
        .background(HistoryPreviewModifierScrollMonitor { delta in
            fontSize = HistoryPreviewPreferences.textFontSize(afterModifierScroll: delta, current: fontSize)
        })
    }

    private var markdownToolbar: some View {
        HStack(spacing: 8) {
            Spacer()
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

    private func copyAll() {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(source, forType: .string)
    }
}


private struct HistoryMarkdownWebView: NSViewRepresentable {
    let source: String
    let appearance: [String: Any]
    func makeCoordinator() -> HistoryMarkdownWebRenderer { HistoryMarkdownWebRenderer() }
    func makeNSView(context: Context) -> WKWebView {
        context.coordinator.update(source: source, appearance: appearance)
        return context.coordinator.webView
    }
    func updateNSView(_ view: WKWebView, context: Context) {
        context.coordinator.update(source: source, appearance: appearance)
    }
    static func dismantleNSView(_ view: WKWebView, coordinator: HistoryMarkdownWebRenderer) {
        coordinator.cancel()
    }
}
