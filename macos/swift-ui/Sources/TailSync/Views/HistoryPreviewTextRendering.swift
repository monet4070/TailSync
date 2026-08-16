import AppKit

enum HistoryPreviewTextStyler {
    static func attributedString(
        _ text: String,
        isCode: Bool,
        fontSize: CGFloat,
        query: String
    ) -> NSAttributedString {
        let font = isCode
            ? NSFont.monospacedSystemFont(ofSize: fontSize, weight: .regular)
            : NSFont.systemFont(ofSize: fontSize)
        let result = NSMutableAttributedString(
            string: text,
            attributes: [.font: font, .foregroundColor: NSColor.labelColor]
        )
        let fullRange = NSRange(location: 0, length: result.length)
        if isCode, result.length <= 2_000_000 {
            apply(
                #"\b(?:class|struct|enum|func|let|var|if|else|for|while|return|import|from|def|fn|pub|async|await|throw|throws|try|catch|switch|case|break|continue|true|false|null|nil|SELECT|FROM|WHERE|INSERT|UPDATE|DELETE)\b"#,
                color: .systemPurple,
                to: result
            )
            apply(#"\b(?:0x[0-9A-Fa-f]+|\d+(?:\.\d+)?)\b"#, color: .systemOrange, to: result)
            apply(
                #"\b(?:String|Int|Double|Bool|Data|URL|Result|Option|Vec|Dictionary|Array|Set|Promise|Task)\b"#,
                color: .systemTeal,
                to: result
            )
            apply(
                #"\b[A-Za-z_][A-Za-z0-9_]*(?=\s*\()"#,
                color: .systemBlue,
                to: result
            )
            // Apply strings and comments last so keywords inside them do not
            // overwrite the token's more specific colour.
            apply(#"\"(?:\\.|[^\"\\])*\"|'(?:\\.|[^'\\])*'"#, color: .systemRed, to: result)
            apply(
                #"(?m)//.*$|/\*[\s\S]*?\*/|(?m)^\s*#(?![A-Fa-f0-9]).*$"#,
                color: .systemGreen,
                to: result
            )
        }
        if !query.isEmpty,
           result.length <= 8_000_000,
           let expression = try? NSRegularExpression(
               pattern: NSRegularExpression.escapedPattern(for: query),
               options: [.caseInsensitive]
           )
        {
            expression.enumerateMatches(in: text, range: fullRange) { match, _, _ in
                guard let range = match?.range else { return }
                result.addAttribute(
                    .backgroundColor,
                    value: NSColor.systemYellow.withAlphaComponent(0.45),
                    range: range
                )
            }
        }
        return result
    }

    private static func apply(
        _ pattern: String,
        color: NSColor,
        to text: NSMutableAttributedString
    ) {
        guard let expression = try? NSRegularExpression(pattern: pattern) else { return }
        let source = text.string
        expression.enumerateMatches(
            in: source,
            range: NSRange(location: 0, length: text.length)
        ) { match, _, _ in
            guard let range = match?.range else { return }
            text.addAttribute(.foregroundColor, value: color, range: range)
        }
    }
}

/// UTF-16 offsets match TextKit's character indices. The index is rebuilt only
/// when the text changes, then visible line lookups are O(log n).
struct HistoryPreviewLogicalLineIndex: Equatable {
    let lineStartOffsets: [Int]

    init(text: String) {
        let source = text as NSString
        var offsets = [0]
        offsets.reserveCapacity(max(1, HistoryPreviewTextMetrics.lineCount(in: text)))
        for index in 0..<source.length where source.character(at: index) == 10 {
            offsets.append(index + 1)
        }
        lineStartOffsets = offsets
    }

    func lineNumber(containing characterIndex: Int) -> Int {
        var lowerBound = 0
        var upperBound = lineStartOffsets.count
        while lowerBound < upperBound {
            let midpoint = lowerBound + (upperBound - lowerBound) / 2
            if lineStartOffsets[midpoint] <= characterIndex {
                lowerBound = midpoint + 1
            } else {
                upperBound = midpoint
            }
        }
        return max(1, lowerBound)
    }
}

final class HistoryPreviewLineNumberRulerView: NSRulerView {
    weak var textView: NSTextView?
    var lineIndex = HistoryPreviewLogicalLineIndex(text: "")

    init(textView: NSTextView) {
        self.textView = textView
        super.init(
            scrollView: textView.enclosingScrollView,
            orientation: .verticalRuler
        )
        clientView = textView
        ruleThickness = 52
    }

    required init(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    override func drawHashMarksAndLabels(in rect: NSRect) {
        guard let textView,
              let layoutManager = textView.layoutManager,
              let textContainer = textView.textContainer else { return }
        NSColor.textBackgroundColor.setFill()
        rect.fill()

        let visible = scrollView?.contentView.bounds ?? .zero
        let glyphRange = layoutManager.glyphRange(
            forBoundingRect: visible,
            in: textContainer
        )
        var glyphIndex = glyphRange.location
        var fragmentRange = NSRange()
        var lastDrawnLine = 0
        while glyphIndex < NSMaxRange(glyphRange),
              glyphIndex < layoutManager.numberOfGlyphs
        {
            let fragment = layoutManager.lineFragmentRect(
                forGlyphAt: glyphIndex,
                effectiveRange: &fragmentRange
            )
            let fragmentGlyphIndex = min(
                fragmentRange.location,
                max(0, layoutManager.numberOfGlyphs - 1)
            )
            let characterIndex = layoutManager.characterIndexForGlyph(
                at: fragmentGlyphIndex
            )
            let lineNumber = lineIndex.lineNumber(containing: characterIndex)
            if lineNumber != lastDrawnLine {
                drawLineNumber(
                    lineNumber,
                    fragment: fragment,
                    textView: textView,
                    visibleBounds: visible
                )
                lastDrawnLine = lineNumber
            }
            let nextGlyphIndex = NSMaxRange(fragmentRange)
            guard nextGlyphIndex > glyphIndex else { break }
            glyphIndex = nextGlyphIndex
        }
    }

    private func drawLineNumber(
        _ lineNumber: Int,
        fragment: NSRect,
        textView: NSTextView,
        visibleBounds: NSRect
    ) {
        let label = "\(lineNumber)" as NSString
        let attributes: [NSAttributedString.Key: Any] = [
            .font: NSFont.monospacedDigitSystemFont(ofSize: 10, weight: .regular),
            .foregroundColor: NSColor.secondaryLabelColor
        ]
        let size = label.size(withAttributes: attributes)
        let y = fragment.minY + textView.textContainerOrigin.y - visibleBounds.minY
        label.draw(
            at: NSPoint(x: ruleThickness - size.width - 8, y: y + 2),
            withAttributes: attributes
        )
    }
}
