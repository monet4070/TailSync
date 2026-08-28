import AppKit
import SwiftUI

struct HistoryMarkdownDocument: Equatable {
    let blocks: [HistoryMarkdownBlock]
    let parsesInlineMarkdown: Bool
}

indirect enum HistoryMarkdownBlock: Equatable {
    case heading(level: Int, text: String)
    case paragraph(String)
    case list([HistoryMarkdownListItem])
    case blockQuote([HistoryMarkdownBlock])
    case code(language: String?, text: String)
    case thematicBreak
    case table(headers: [String], rows: [[String]])
}

struct HistoryMarkdownListItem: Equatable {
    enum Marker: Equatable {
        case bullet
        case number(Int)
        case task(Bool)
    }

    let depth: Int
    let marker: Marker
    var text: String
}

enum HistoryMarkdownRenderer {
    static let maximumRichTextBytes = 2 * 1024 * 1024

    private static let headingExpression = try! NSRegularExpression(
        pattern: #"^ {0,3}(#{1,6})(?:[ \t]+(.*?)[ \t]*#*[ \t]*|[ \t]*)$"#
    )
    private static let bulletExpression = try! NSRegularExpression(
        pattern: #"^([ \t]*)([-+*])[ \t]+(?:\[([ xX])\][ \t]+)?(.*)$"#
    )
    private static let numberExpression = try! NSRegularExpression(
        pattern: #"^([ \t]*)([0-9]{1,9})[.)][ \t]+(.*)$"#
    )
    private static let quoteExpression = try! NSRegularExpression(
        pattern: #"^ {0,3}>[ \t]?(.*)$"#
    )

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
        // Preview never loads remote or embedded resources. Keep image alt
        // text so the document remains understandable without network I/O.
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

    static func document(_ source: String) -> HistoryMarkdownDocument {
        guard source.utf8.count <= maximumRichTextBytes else {
            return HistoryMarkdownDocument(
                blocks: [.paragraph(source)],
                parsesInlineMarkdown: false
            )
        }
        let sanitized = sanitizedSource(source)
        return HistoryMarkdownDocument(
            blocks: parseBlocks(sanitized, depth: 0),
            parsesInlineMarkdown: true
        )
    }

    static func attributedArticle(_ source: String) -> AttributedString {
        guard source.utf8.count <= maximumRichTextBytes else {
            return AttributedString(source)
        }
        let sanitized = sanitizedSource(source)
        var article = (try? AttributedString(
            markdown: sanitized,
            options: AttributedString.MarkdownParsingOptions(interpretedSyntax: .full)
        )) ?? AttributedString(sanitized)
        removeUnsafeLinks(from: &article)
        return article
    }

    static func attributedInline(_ source: String, parseMarkdown: Bool) -> AttributedString {
        guard parseMarkdown else { return AttributedString(source) }
        var inline = (try? AttributedString(
            markdown: source,
            options: AttributedString.MarkdownParsingOptions(
                interpretedSyntax: .inlineOnlyPreservingWhitespace
            )
        )) ?? AttributedString(source)
        removeUnsafeLinks(from: &inline)
        return inline
    }

    static func isAllowedLink(_ url: URL) -> Bool {
        guard let scheme = url.scheme?.lowercased() else { return false }
        return scheme == "http" || scheme == "https"
    }

    private static func parseBlocks(_ source: String, depth: Int) -> [HistoryMarkdownBlock] {
        let normalized = source
            .replacingOccurrences(of: "\r\n", with: "\n")
            .replacingOccurrences(of: "\r", with: "\n")
        let lines = normalized.components(separatedBy: "\n")
        var blocks: [HistoryMarkdownBlock] = []
        var index = 0

        while index < lines.count {
            let line = lines[index]
            if line.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                index += 1
                continue
            }

            if let fence = fenceOpening(line) {
                var codeLines: [String] = []
                index += 1
                while index < lines.count, !isFenceClosing(lines[index], fence: fence) {
                    codeLines.append(lines[index])
                    index += 1
                }
                if index < lines.count { index += 1 }
                blocks.append(.code(
                    language: fence.language.isEmpty ? nil : fence.language,
                    text: codeLines.joined(separator: "\n")
                ))
                continue
            }

            if let indented = indentedCodeContent(line) {
                var codeLines = [indented]
                index += 1
                while index < lines.count {
                    if let continuation = indentedCodeContent(lines[index]) {
                        codeLines.append(continuation)
                        index += 1
                    } else if lines[index].isEmpty {
                        codeLines.append("")
                        index += 1
                    } else {
                        break
                    }
                }
                while codeLines.last == "" { codeLines.removeLast() }
                blocks.append(.code(language: nil, text: codeLines.joined(separator: "\n")))
                continue
            }

            if let heading = heading(line) {
                blocks.append(.heading(level: heading.level, text: heading.text))
                index += 1
                continue
            }

            if index + 1 < lines.count,
               let level = setextHeadingLevel(lines[index + 1])
            {
                blocks.append(.heading(
                    level: level,
                    text: line.trimmingCharacters(in: .whitespaces)
                ))
                index += 2
                continue
            }

            if isThematicBreak(line) {
                blocks.append(.thematicBreak)
                index += 1
                continue
            }

            if index + 1 < lines.count,
               let headers = tableHeaders(line, separator: lines[index + 1])
            {
                index += 2
                var rows: [[String]] = []
                while index < lines.count,
                      !lines[index].trimmingCharacters(in: .whitespaces).isEmpty,
                      lines[index].contains("|")
                {
                    rows.append(splitTableRow(lines[index]))
                    index += 1
                }
                blocks.append(.table(headers: headers, rows: rows))
                continue
            }

            if quoteContent(line) != nil {
                var quoteLines: [String] = []
                while index < lines.count, let content = quoteContent(lines[index]) {
                    quoteLines.append(content)
                    index += 1
                }
                let quoteSource = quoteLines.joined(separator: "\n")
                let quoteBlocks = depth < 6
                    ? parseBlocks(quoteSource, depth: depth + 1)
                    : [.paragraph(quoteSource)]
                blocks.append(.blockQuote(quoteBlocks))
                continue
            }

            if listItem(line) != nil {
                var items: [HistoryMarkdownListItem] = []
                while index < lines.count {
                    if let item = listItem(lines[index]) {
                        items.append(item)
                        index += 1
                        continue
                    }
                    guard !items.isEmpty,
                          let continuation = listContinuation(lines[index]) else { break }
                    items[items.count - 1].text += "\n" + continuation
                    index += 1
                }
                blocks.append(.list(items))
                continue
            }

            var paragraphLines = [line]
            index += 1
            while index < lines.count,
                  !lines[index].trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
                  !startsBlock(lines, at: index)
            {
                paragraphLines.append(lines[index])
                index += 1
            }
            blocks.append(.paragraph(paragraphLines.joined(separator: "\n")))
        }
        return blocks
    }

    private static func startsBlock(_ lines: [String], at index: Int) -> Bool {
        let line = lines[index]
        if fenceOpening(line) != nil
            || indentedCodeContent(line) != nil
            || heading(line) != nil
            || isThematicBreak(line)
            || quoteContent(line) != nil
            || listItem(line) != nil
        {
            return true
        }
        guard index + 1 < lines.count else { return false }
        return setextHeadingLevel(lines[index + 1]) != nil
            || tableHeaders(line, separator: lines[index + 1]) != nil
    }

    private static func heading(_ line: String) -> (level: Int, text: String)? {
        guard let match = firstMatch(headingExpression, line),
              let markers = capture(match, in: line, at: 1) else { return nil }
        return (markers.count, capture(match, in: line, at: 2) ?? "")
    }

    private static func setextHeadingLevel(_ line: String) -> Int? {
        let compact = line.trimmingCharacters(in: .whitespaces)
        guard compact.count >= 3, let marker = compact.first,
              marker == "=" || marker == "-",
              compact.allSatisfy({ $0 == marker }) else { return nil }
        return marker == "=" ? 1 : 2
    }

    private static func isThematicBreak(_ line: String) -> Bool {
        let compact = line.filter { !$0.isWhitespace }
        guard compact.count >= 3, let marker = compact.first,
              marker == "-" || marker == "*" || marker == "_" else { return false }
        return compact.allSatisfy { $0 == marker }
    }

    private static func listItem(_ line: String) -> HistoryMarkdownListItem? {
        if let match = firstMatch(bulletExpression, line),
           let indentation = capture(match, in: line, at: 1),
           let text = capture(match, in: line, at: 4)
        {
            let task = capture(match, in: line, at: 3)
            return HistoryMarkdownListItem(
                depth: listDepth(indentation),
                marker: task.map { .task($0.lowercased() == "x") } ?? .bullet,
                text: text
            )
        }
        if let match = firstMatch(numberExpression, line),
           let indentation = capture(match, in: line, at: 1),
           let ordinalText = capture(match, in: line, at: 2),
           let ordinal = Int(ordinalText),
           let text = capture(match, in: line, at: 3)
        {
            return HistoryMarkdownListItem(
                depth: listDepth(indentation),
                marker: .number(ordinal),
                text: text
            )
        }
        return nil
    }

    private static func listDepth(_ indentation: String) -> Int {
        min(8, indentation.reduce(into: 0) { count, character in
            count += character == "\t" ? 2 : 1
        } / 2)
    }

    private static func listContinuation(_ line: String) -> String? {
        guard line.hasPrefix("  ") || line.hasPrefix("\t") else { return nil }
        return line.trimmingCharacters(in: .whitespaces)
    }

    private static func quoteContent(_ line: String) -> String? {
        guard let match = firstMatch(quoteExpression, line) else { return nil }
        return capture(match, in: line, at: 1) ?? ""
    }

    private static func indentedCodeContent(_ line: String) -> String? {
        if line.hasPrefix("\t") { return String(line.dropFirst()) }
        guard line.hasPrefix("    ") else { return nil }
        return String(line.dropFirst(4))
    }

    private static func fenceOpening(
        _ line: String
    ) -> (marker: Character, count: Int, language: String)? {
        let indentation = line.prefix { $0 == " " }.count
        guard indentation <= 3 else { return nil }
        let content = line.dropFirst(indentation)
        guard let marker = content.first, marker == "`" || marker == "~" else { return nil }
        let count = content.prefix { $0 == marker }.count
        guard count >= 3 else { return nil }
        let remainder = content.dropFirst(count)
        if marker == "`", remainder.contains("`") { return nil }
        return (
            marker,
            count,
            remainder.trimmingCharacters(in: .whitespaces)
        )
    }

    private static func isFenceClosing(
        _ line: String,
        fence: (marker: Character, count: Int, language: String)
    ) -> Bool {
        let indentation = line.prefix { $0 == " " }.count
        guard indentation <= 3 else { return false }
        let content = line.dropFirst(indentation)
        let markerCount = content.prefix { $0 == fence.marker }.count
        guard markerCount >= fence.count else { return false }
        return content.dropFirst(markerCount).allSatisfy(\.isWhitespace)
    }

    private static func tableHeaders(_ line: String, separator: String) -> [String]? {
        guard line.contains("|"), separator.contains("|") else { return nil }
        let headers = splitTableRow(line)
        let separators = splitTableRow(separator)
        guard !headers.isEmpty, separators.count == headers.count,
              separators.allSatisfy(isTableSeparator) else { return nil }
        return headers
    }

    private static func isTableSeparator(_ value: String) -> Bool {
        var compact = value.trimmingCharacters(in: .whitespaces)
        if compact.first == ":" { compact.removeFirst() }
        if compact.last == ":" { compact.removeLast() }
        return compact.count >= 3 && compact.allSatisfy { $0 == "-" }
    }

    private static func splitTableRow(_ line: String) -> [String] {
        let sentinel = "\u{E000}"
        var value = line.trimmingCharacters(in: .whitespaces)
        if value.first == "|" { value.removeFirst() }
        if value.last == "|" { value.removeLast() }
        return value
            .replacingOccurrences(of: #"\|"#, with: sentinel)
            .components(separatedBy: "|")
            .map {
                $0.replacingOccurrences(of: sentinel, with: "|")
                    .trimmingCharacters(in: .whitespaces)
            }
    }

    private static func removeUnsafeLinks(from article: inout AttributedString) {
        let unsafeRanges: [Range<AttributedString.Index>] = article.runs.compactMap { run in
            guard let link = run.link, !isAllowedLink(link) else { return nil }
            return run.range
        }
        for range in unsafeRanges { article[range].link = nil }
    }

    private static func firstMatch(
        _ expression: NSRegularExpression,
        _ source: String
    ) -> NSTextCheckingResult? {
        expression.firstMatch(
            in: source,
            range: NSRange(source.startIndex..<source.endIndex, in: source)
        )
    }

    private static func capture(
        _ match: NSTextCheckingResult,
        in source: String,
        at index: Int
    ) -> String? {
        let range = match.range(at: index)
        guard range.location != NSNotFound, let swiftRange = Range(range, in: source) else {
            return nil
        }
        return String(source[swiftRange])
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
    private let showsToolbar: Bool
    private let document: HistoryMarkdownDocument

    @Environment(\.tailSyncPalette) private var palette
    @AppStorage(HistoryPreviewPreferences.textFontSizeKey)
    private var fontSize = HistoryPreviewPreferences.defaultTextFontSize

    init(source: String, showsToolbar: Bool = true) {
        self.source = source
        self.showsToolbar = showsToolbar
        document = HistoryMarkdownRenderer.document(source)
    }

    var body: some View {
        VStack(spacing: 0) {
            if showsToolbar { markdownToolbar }
            ScrollView {
                HistoryMarkdownBlocksView(
                    blocks: document.blocks,
                    fontSize: CGFloat(HistoryPreviewPreferences.clampedTextFontSize(fontSize)),
                    parsesInlineMarkdown: document.parsesInlineMarkdown
                )
                .frame(maxWidth: 860, alignment: .topLeading)
                .frame(maxWidth: .infinity, alignment: .top)
                .padding(.horizontal, 40)
                .padding(.vertical, 34)
            }
        }
        .background(palette.surfaceColor)
        .background(HistoryPreviewModifierScrollMonitor { delta in
            fontSize = HistoryPreviewPreferences.textFontSize(
                afterModifierScroll: delta,
                current: fontSize
            )
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

private struct HistoryMarkdownBlocksView: View {
    let blocks: [HistoryMarkdownBlock]
    let fontSize: CGFloat
    let parsesInlineMarkdown: Bool

    var body: some View {
        LazyVStack(alignment: .leading, spacing: max(12, fontSize * 0.8)) {
            ForEach(blocks.indices, id: \.self) { index in
                HistoryMarkdownBlockView(
                    block: blocks[index],
                    fontSize: fontSize,
                    parsesInlineMarkdown: parsesInlineMarkdown
                )
            }
        }
    }
}

private struct HistoryMarkdownBlockView: View {
    let block: HistoryMarkdownBlock
    let fontSize: CGFloat
    let parsesInlineMarkdown: Bool

    @Environment(\.tailSyncPalette) private var palette

    @ViewBuilder
    var body: some View {
        switch block {
        case .heading(let level, let text):
            Text(inline(text))
                .font(.system(
                    size: fontSize * headingScale(level),
                    weight: level <= 2 ? .bold : .semibold
                ))
                .foregroundColor(palette.primaryColor)
                .textSelection(.enabled)
                .fixedSize(horizontal: false, vertical: true)

        case .paragraph(let text):
            Text(inline(text))
                .font(.system(size: fontSize))
                .foregroundColor(palette.primaryColor)
                .lineSpacing(max(3, fontSize * 0.22))
                .textSelection(.enabled)
                .fixedSize(horizontal: false, vertical: true)

        case .list(let items):
            HistoryMarkdownListView(
                items: items,
                fontSize: fontSize,
                parsesInlineMarkdown: parsesInlineMarkdown
            )

        case .blockQuote(let blocks):
            HStack(alignment: .top, spacing: 12) {
                RoundedRectangle(cornerRadius: 1.5)
                    .fill(palette.accentColor.opacity(0.75))
                    .frame(width: 3)
                HistoryMarkdownBlocksView(
                    blocks: blocks,
                    fontSize: fontSize,
                    parsesInlineMarkdown: parsesInlineMarkdown
                )
                .foregroundColor(palette.secondaryColor)
            }
            .padding(.vertical, 3)

        case .code(let language, let text):
            VStack(alignment: .leading, spacing: 8) {
                if let language, !language.isEmpty {
                    Text(language.uppercased())
                        .font(.system(size: 10, weight: .bold).monospaced())
                        .foregroundColor(palette.tertiaryColor)
                }
                ScrollView(.horizontal, showsIndicators: true) {
                    Text(text)
                        .font(.system(size: max(11, fontSize * 0.88), design: .monospaced))
                        .foregroundColor(palette.primaryColor)
                        .textSelection(.enabled)
                        .fixedSize(horizontal: true, vertical: true)
                }
            }
            .padding(14)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(palette.softSurfaceColor)
            .clipShape(RoundedRectangle(cornerRadius: 9, style: .continuous))
            .overlay {
                RoundedRectangle(cornerRadius: 9, style: .continuous)
                    .stroke(palette.borderColor, lineWidth: 1)
            }

        case .thematicBreak:
            Rectangle()
                .fill(palette.dividerColor)
                .frame(maxWidth: .infinity, minHeight: 1, maxHeight: 1)
                .padding(.vertical, 4)

        case .table(let headers, let rows):
            HistoryMarkdownTableView(
                headers: headers,
                rows: rows,
                fontSize: fontSize,
                parsesInlineMarkdown: parsesInlineMarkdown
            )
        }
    }

    private func inline(_ source: String) -> AttributedString {
        HistoryMarkdownRenderer.attributedInline(
            source,
            parseMarkdown: parsesInlineMarkdown
        )
    }

    private func headingScale(_ level: Int) -> CGFloat {
        switch level {
        case 1: return 1.85
        case 2: return 1.55
        case 3: return 1.32
        case 4: return 1.15
        case 5: return 1.05
        default: return 1
        }
    }
}

private struct HistoryMarkdownListView: View {
    let items: [HistoryMarkdownListItem]
    let fontSize: CGFloat
    let parsesInlineMarkdown: Bool

    @Environment(\.tailSyncPalette) private var palette

    var body: some View {
        VStack(alignment: .leading, spacing: max(6, fontSize * 0.38)) {
            ForEach(items.indices, id: \.self) { index in
                let item = items[index]
                HStack(alignment: .firstTextBaseline, spacing: 9) {
                    marker(item.marker)
                        .frame(width: 24, alignment: .trailing)
                    Text(HistoryMarkdownRenderer.attributedInline(
                        item.text,
                        parseMarkdown: parsesInlineMarkdown
                    ))
                    .font(.system(size: fontSize))
                    .foregroundColor(palette.primaryColor)
                    .lineSpacing(max(3, fontSize * 0.2))
                    .textSelection(.enabled)
                    .fixedSize(horizontal: false, vertical: true)
                }
                .padding(.leading, CGFloat(item.depth) * 20)
            }
        }
    }

    @ViewBuilder
    private func marker(_ marker: HistoryMarkdownListItem.Marker) -> some View {
        switch marker {
        case .bullet:
            Text("•").foregroundColor(palette.accentColor)
        case .number(let value):
            Text("\(value).")
                .font(.system(size: max(11, fontSize * 0.85)).monospacedDigit())
                .foregroundColor(palette.secondaryColor)
        case .task(let checked):
            Image(systemName: checked ? "checkmark.square.fill" : "square")
                .foregroundColor(checked ? palette.positiveColor : palette.tertiaryColor)
        }
    }
}

private struct HistoryMarkdownTableView: View {
    let headers: [String]
    let rows: [[String]]
    let fontSize: CGFloat
    let parsesInlineMarkdown: Bool

    @Environment(\.tailSyncPalette) private var palette

    private var columnCount: Int {
        max(headers.count, rows.map(\.count).max() ?? 0)
    }

    var body: some View {
        ScrollView(.horizontal, showsIndicators: true) {
            Grid(alignment: .leading, horizontalSpacing: 0, verticalSpacing: 0) {
                GridRow {
                    ForEach(0..<columnCount, id: \.self) { column in
                        cell(headers.indices.contains(column) ? headers[column] : "", header: true)
                    }
                }
                ForEach(rows.indices, id: \.self) { row in
                    GridRow {
                        ForEach(0..<columnCount, id: \.self) { column in
                            cell(rows[row].indices.contains(column) ? rows[row][column] : "", header: false)
                        }
                    }
                }
            }
            .overlay {
                RoundedRectangle(cornerRadius: 7, style: .continuous)
                    .stroke(palette.borderColor, lineWidth: 1)
            }
            .clipShape(RoundedRectangle(cornerRadius: 7, style: .continuous))
        }
    }

    private func cell(_ text: String, header: Bool) -> some View {
        Text(HistoryMarkdownRenderer.attributedInline(
            text,
            parseMarkdown: parsesInlineMarkdown
        ))
        .font(.system(size: max(11, fontSize * 0.9), weight: header ? .semibold : .regular))
        .foregroundColor(palette.primaryColor)
        .textSelection(.enabled)
        .fixedSize(horizontal: true, vertical: true)
        .padding(.horizontal, 12)
        .padding(.vertical, 9)
        .frame(minWidth: 90, alignment: .leading)
        .background(header ? palette.softSurfaceColor : palette.surfaceColor)
        .overlay(alignment: .trailing) {
            Rectangle().fill(palette.dividerColor).frame(width: 1)
        }
        .overlay(alignment: .bottom) {
            Rectangle().fill(palette.dividerColor).frame(height: 1)
        }
    }
}
