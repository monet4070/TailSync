import AppKit
import SwiftUI

enum HistoryMarkdownRenderer {
    static func sanitizedSource(_ source: String) -> String {
        var sanitized = source
        let blockTags = [
            "script", "style", "iframe", "video", "audio", "object", "embed"
        ]
        for tag in blockTags {
            sanitized = replacing(
                #"<\s*\#(tag)\b[^>]*>[\s\S]*?<\s*/\s*\#(tag)\s*>"#,
                in: sanitized,
                with: ""
            )
            sanitized = replacing(
                #"<\s*\#(tag)\b[^>]*/?\s*>"#,
                in: sanitized,
                with: ""
            )
        }
        // Preview renders prose only. Remote and embedded image resources are
        // reduced to alt text before Foundation parses the Markdown.
        sanitized = replacing(
            #"!\[([^\]]*)\]\([^\)]*\)"#,
            in: sanitized,
            with: "$1"
        )
        sanitized = replacing(
            #"!\[([^\]]*)\]\[[^\]]*\]"#,
            in: sanitized,
            with: "$1"
        )
        sanitized = replacing(#"<[^>]+>"#, in: sanitized, with: "")
        return sanitized
    }

    static func attributedArticle(_ source: String) -> AttributedString {
        let sanitized = sanitizedSource(source)
        var article = (try? AttributedString(
            markdown: sanitized,
            options: AttributedString.MarkdownParsingOptions(interpretedSyntax: .full)
        )) ?? AttributedString(sanitized)

        let unsafeLinkRanges = article.runs.compactMap { run in
            guard let link = run.link, !isAllowedLink(link) else { return nil }
            return run.range
        }
        for range in unsafeLinkRanges {
            article[range].link = nil
        }
        return article
    }

    static func isAllowedLink(_ url: URL) -> Bool {
        guard let scheme = url.scheme?.lowercased() else { return false }
        return scheme == "http" || scheme == "https"
    }

    private static func replacing(
        _ pattern: String,
        in source: String,
        with template: String
    ) -> String {
        guard let expression = try? NSRegularExpression(
            pattern: pattern,
            options: [.caseInsensitive]
        ) else { return source }
        let range = NSRange(source.startIndex..<source.endIndex, in: source)
        return expression.stringByReplacingMatches(
            in: source,
            range: range,
            withTemplate: template
        )
    }
}

struct HistoryMarkdownPreviewView: View {
    let source: String
    @AppStorage(HistoryPreviewPreferences.textFontSizeKey)
    private var fontSize = HistoryPreviewPreferences.defaultTextFontSize

    private var article: AttributedString {
        HistoryMarkdownRenderer.attributedArticle(source)
    }

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                Spacer()
                Button {
                    fontSize = HistoryPreviewPreferences.clampedTextFontSize(fontSize - 1)
                } label: {
                    Image(systemName: "textformat.size.smaller")
                }
                .disabled(fontSize <= 12)
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
            .padding(.horizontal, 12)
            .frame(height: 42)
            Divider()

            ScrollView {
                Text(article)
                    .font(.system(size: CGFloat(
                        HistoryPreviewPreferences.clampedTextFontSize(fontSize)
                    )))
                    .textSelection(.enabled)
                    .frame(maxWidth: 820, alignment: .topLeading)
                    .frame(maxWidth: .infinity, alignment: .top)
                    .padding(.horizontal, 36)
                    .padding(.vertical, 30)
            }
        }
    }

    private func copyAll() {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(source, forType: .string)
    }
}
