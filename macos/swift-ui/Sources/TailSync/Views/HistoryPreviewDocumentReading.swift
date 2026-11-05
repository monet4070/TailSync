import AppKit

enum HistoryPreviewDocumentReadingStyle {
    /// Derive display attributes from the immutable material each time, so
    /// repeated size changes never compound and attachments stay untouched.
    static func attributed(
        _ source: NSAttributedString,
        options: HistoryPreviewDocumentReadingOptions
    ) -> NSAttributedString {
        let result = NSMutableAttributedString(attributedString: source)
        let scale = options.normalizedFontSize / HistoryPreviewDocumentBuilder.bodyFontSize
        source.enumerateAttributes(in: NSRange(location: 0, length: source.length)) { attributes, range, _ in
            if let font = attributes[.font] as? NSFont,
               let resized = NSFont(descriptor: font.fontDescriptor, size: font.pointSize * scale) {
                result.addAttribute(.font, value: resized, range: range)
            }
            if let baseline = attributes[.baselineOffset] as? NSNumber {
                result.addAttribute(.baselineOffset, value: baseline.doubleValue * scale, range: range)
            }
            let paragraph = (attributes[.paragraphStyle] as? NSParagraphStyle)?.mutableCopy() as? NSMutableParagraphStyle
                ?? NSMutableParagraphStyle()
            paragraph.lineHeightMultiple = attributes[.attachment] == nil ? options.lineSpacing.multiple : 1
            paragraph.paragraphSpacing *= scale
            paragraph.paragraphSpacingBefore *= scale
            paragraph.firstLineHeadIndent *= scale
            paragraph.headIndent *= scale
            paragraph.tailIndent *= scale
            result.addAttribute(.paragraphStyle, value: paragraph, range: range)
        }
        return result
    }

    static func plainText(_ source: NSAttributedString) -> String {
        source.string.replacingOccurrences(of: "\u{fffc}", with: "")
    }
}

struct HistoryPreviewDocumentSearchSummary: Equatable {
    var current = 0
    var count = 0
    var truncated = false

    var label: String { "\(current)/\(count)\(truncated ? "+" : "")" }
}

enum HistoryPreviewDocumentSearch {
    static let maximumMatches = 2_000

    static func matches(in text: String, query: String) -> (ranges: [NSRange], truncated: Bool) {
        guard !query.isEmpty else { return ([], false) }
        let source = text as NSString
        var ranges: [NSRange] = []
        var start = 0
        while start < source.length {
            let match = source.range(
                of: query, options: [.caseInsensitive],
                range: NSRange(location: start, length: source.length - start)
            )
            guard match.location != NSNotFound, match.length > 0 else { break }
            if ranges.count == maximumMatches { return (ranges, true) }
            ranges.append(match)
            start = NSMaxRange(match)
        }
        return (ranges, false)
    }
}

final class HistoryPreviewDocumentReadingTextView: NSTextView {
    var usesComfortableWidth = false {
        didSet { updateReadingInsets() }
    }

    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        updateReadingInsets()
    }

    private func updateReadingInsets() {
        let inset = usesComfortableWidth
            ? max(20, (frame.width - HistoryPreviewDocumentReadingOptions.maximumReadingWidth) / 2)
            : 20
        let updated = NSSize(width: inset, height: 16)
        if textContainerInset != updated { textContainerInset = updated }
    }
}
