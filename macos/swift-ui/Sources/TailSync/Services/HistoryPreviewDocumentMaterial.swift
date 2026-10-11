import AppKit
import Foundation
import ImageIO

struct HistoryPreviewDocumentStrings: Sendable {
    let imageUnavailable: String
    let imageCannotDisplay: String
    let imageBudget: String
    let availability: [String: String]

    @MainActor
    static func live() -> HistoryPreviewDocumentStrings {
        var availability: [String: String] = [:]
        for code in [
            "available", "missing_resource", "missing_source",
            "external_reference", "unsupported", "pixel_budget",
        ] {
            availability[code] = Loc.t("history.preview.imageAvailability.\(code)")
        }
        return HistoryPreviewDocumentStrings(
            imageUnavailable: Loc.t("history.preview.imageUnavailable"),
            imageCannotDisplay: Loc.t("history.preview.imageCannotDisplay"),
            imageBudget: Loc.t("history.preview.imageBudget"),
            availability: availability
        )
    }

    func availabilityLabel(_ code: String) -> String {
        availability[code] ?? imageUnavailable
    }

    func placeholderPrefix(_ code: String) -> String {
        switch code {
        case "missing_source", "missing_resource", "external_reference": return imageUnavailable
        default: return imageCannotDisplay
        }
    }
}

enum HistoryPreviewDocumentError: LocalizedError, Equatable {
    case tooLarge
    case invalidImage
    case timedOut

    var errorDescription: String? {
        switch self {
        case .tooLarge: return "Document preview exceeds the renderer budget"
        case .invalidImage: return "Document image could not be decoded"
        case .timedOut: return "Document preparation timed out"
        }
    }
}

enum HistoryPreviewNoticeLevel: Sendable, Equatable {
    case warning
    case diagnostic
}

enum HistoryPreviewDocumentLink {
    static let maximumByteCount = 2048

    static func safeURL(_ raw: String) -> URL? {
        guard !raw.isEmpty, raw.utf8.count <= maximumByteCount else { return nil }
        guard !raw.contains("\\") else { return nil }
        guard raw.range(of: "^https?://[^/?#]+", options: [.regularExpression, .caseInsensitive]) != nil else { return nil }
        for scalar in raw.unicodeScalars {
            if CharacterSet.whitespacesAndNewlines.contains(scalar) { return nil }
            if CharacterSet.controlCharacters.contains(scalar) { return nil }
        }
        guard !authority(of: raw).contains("@") else { return nil }
        guard var components = URLComponents(string: raw) else { return nil }
        guard let scheme = components.scheme?.lowercased(), scheme == "http" || scheme == "https" else { return nil }
        guard components.user == nil, components.password == nil else { return nil }
        guard let host = components.host, !host.isEmpty else { return nil }
        components.scheme = scheme
        return components.url
    }

    static func safeURL(from link: Any) -> URL? {
        if let url = link as? URL { return safeURL(url.absoluteString) }
        if let text = link as? String { return safeURL(text) }
        if let attributed = link as? NSAttributedString { return safeURL(attributed.string) }
        return nil
    }

    private static func authority(of raw: String) -> String {
        let schemeCharacters = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789+.-"
        var remainder = Substring(raw)
        if let colon = remainder.firstIndex(of: ":"),
           remainder.first?.isLetter == true,
           remainder[remainder.startIndex..<colon].allSatisfy({ schemeCharacters.contains($0) }) {
            remainder = remainder[remainder.index(after: colon)...]
        }
        remainder = remainder.drop(while: { $0 == "/" })
        guard let end = remainder.firstIndex(where: { $0 == "/" || $0 == "?" || $0 == "#" }) else {
            return String(remainder)
        }
        return String(remainder[remainder.startIndex..<end])
    }
}

enum HistoryPreviewDocumentContrast {
    static func readableTextColor(on color: ContractDocumentColor) -> NSColor {
        relativeLuminance(color) > 0.1791 ? NSColor.black : NSColor.white
    }

    static func relativeLuminance(_ color: ContractDocumentColor) -> Double {
        0.2126 * linearChannel(Double(color.red) / 255)
            + 0.7152 * linearChannel(Double(color.green) / 255)
            + 0.0722 * linearChannel(Double(color.blue) / 255)
    }

    static func contrastRatio(_ foreground: ContractDocumentColor, _ background: ContractDocumentColor) -> Double {
        let foregroundLuminance = relativeLuminance(foreground)
        let backgroundLuminance = relativeLuminance(background)
        return (max(foregroundLuminance, backgroundLuminance) + 0.05)
            / (min(foregroundLuminance, backgroundLuminance) + 0.05)
    }

    static func readableTextColor(_ foreground: NSColor, on background: NSColor) -> NSColor {
        NSColor(name: nil) { appearance in
            var result = foreground
            appearance.performAsCurrentDrawingAppearance {
                guard let foreground = foreground.usingColorSpace(.sRGB),
                      let background = background.usingColorSpace(.sRGB) else { return }
                let foregroundLuminance = luminance(foreground)
                let backgroundLuminance = luminance(background)
                let contrast = (max(foregroundLuminance, backgroundLuminance) + 0.05)
                    / (min(foregroundLuminance, backgroundLuminance) + 0.05)
                if contrast < 4.5 {
                    result = backgroundLuminance > 0.1791 ? NSColor.black : NSColor.white
                }
            }
            return result
        }
    }

    private static func luminance(_ color: NSColor) -> Double {
        0.2126 * linearChannel(Double(color.redComponent))
            + 0.7152 * linearChannel(Double(color.greenComponent))
            + 0.0722 * linearChannel(Double(color.blueComponent))
    }

    private static func linearChannel(_ value: Double) -> Double {
        value <= 0.03928 ? value / 12.92 : pow((value + 0.055) / 1.055, 2.4)
    }
}

final class HistoryPreviewDocumentMaterial: @unchecked Sendable, Equatable {
    struct Notice: Equatable, Sendable {
        let code: String
        let labelKey: String
        let summaryKey: String
        let detail: String?
        let level: HistoryPreviewNoticeLevel
    }

    let entryId: Int64
    let owner: String
    let generation: UInt64
    let requestId: String
    let stamp: String
    let attributed: NSAttributedString
    let notices: [Notice]
    let stylesPartial: Bool
    let imagePlacements: Int
    let imagePlaceholders: Int
    let tableCount: Int
    let paragraphCount: Int
    let retainedBytes: Int

    init(
        entryId: Int64,
        owner: String,
        generation: UInt64,
        requestId: String,
        stamp: String,
        attributed: NSAttributedString,
        notices: [Notice],
        stylesPartial: Bool,
        imagePlacements: Int,
        imagePlaceholders: Int,
        tableCount: Int,
        paragraphCount: Int,
        retainedBytes: Int
    ) {
        self.entryId = entryId
        self.owner = owner
        self.generation = generation
        self.requestId = requestId
        self.stamp = stamp
        self.attributed = attributed
        self.notices = notices
        self.stylesPartial = stylesPartial
        self.imagePlacements = imagePlacements
        self.imagePlaceholders = imagePlaceholders
        self.tableCount = tableCount
        self.paragraphCount = paragraphCount
        self.retainedBytes = retainedBytes
    }

    static func == (lhs: HistoryPreviewDocumentMaterial, rhs: HistoryPreviewDocumentMaterial) -> Bool {
        lhs === rhs
    }

    func accessibilitySummary(template: String) -> String {
        template
            .replacingOccurrences(of: "{paragraphs}", with: "\(paragraphCount)")
            .replacingOccurrences(of: "{tables}", with: "\(tableCount)")
            .replacingOccurrences(of: "{images}", with: "\(imagePlacements)")
            .replacingOccurrences(of: "{placeholders}", with: "\(imagePlaceholders)")
    }

    var warningNotices: [Notice] { notices.filter { $0.level == .warning } }

    var diagnosticNotices: [Notice] { notices.filter { $0.level == .diagnostic } }

    var warningSummaryKeys: [String] {
        var keys: [String] = []
        for notice in warningNotices where !keys.contains(notice.summaryKey) {
            keys.append(notice.summaryKey)
        }
        return keys
    }
}

enum HistoryPreviewDocumentBuilder {
    static let maximumPlacementPixels = 16_000_000
    static let maximumRetainedBytes = 64 * 1024 * 1024

    private static let bodyFontSize: CGFloat = 13
    private static let maximumImageWidth: CGFloat = 620
    private static let maximumImageHeight: CGFloat = 460

    private struct NoticeStyle: Sendable {
        let labelKey: String
        let summaryKey: String
        let level: HistoryPreviewNoticeLevel
    }

    private static let noticeStyles: [String: NoticeStyle] = [
        "text_limit": NoticeStyle(labelKey: "history.preview.partial.limit", summaryKey: "history.preview.notice.truncated", level: .warning),
        "block_limit": NoticeStyle(labelKey: "history.preview.partial.limit", summaryKey: "history.preview.notice.truncated", level: .warning),
        "run_limit": NoticeStyle(labelKey: "history.preview.partial.limit", summaryKey: "history.preview.notice.truncated", level: .warning),
        "cell_limit": NoticeStyle(labelKey: "history.preview.partial.limit", summaryKey: "history.preview.notice.truncated", level: .warning),
        "image_limit": NoticeStyle(labelKey: "history.preview.partial.limit", summaryKey: "history.preview.notice.truncated", level: .warning),
        "depth_limit": NoticeStyle(labelKey: "history.preview.partial.limit", summaryKey: "history.preview.notice.truncated", level: .warning),
        "pixel_budget": NoticeStyle(labelKey: "history.preview.partial.pixelBudget", summaryKey: "history.preview.notice.pixelBudget", level: .warning),
        "malformed_html": NoticeStyle(labelKey: "history.preview.partial.malformed", summaryKey: "history.preview.notice.malformed", level: .warning),
        "unsupported_dependency": NoticeStyle(labelKey: "history.preview.partial.malformed", summaryKey: "history.preview.notice.malformed", level: .warning),
        "unsupported_markup": NoticeStyle(labelKey: "history.preview.partial.unsupportedMarkup", summaryKey: "history.preview.notice.unsupportedMarkup", level: .warning),
        "missing_image_source": NoticeStyle(labelKey: "history.preview.partial.missingImage", summaryKey: "history.preview.notice.missingImage", level: .warning),
        "external_reference": NoticeStyle(labelKey: "history.preview.partial.missingImage", summaryKey: "history.preview.notice.missingImage", level: .warning),
        "native_analysis_unavailable": NoticeStyle(labelKey: "history.preview.partial.nativeAnalysis", summaryKey: "history.preview.notice.nativeUnavailable", level: .warning),
        "native_styles_unavailable": NoticeStyle(labelKey: "history.preview.partial.stylesSimplified", summaryKey: "history.preview.notice.simplified", level: .diagnostic),
        "native_table_structure_unavailable": NoticeStyle(labelKey: "history.preview.partial.nativeTableStructure", summaryKey: "history.preview.notice.structureUnavailable", level: .warning),
        "native_partial": NoticeStyle(labelKey: "history.preview.partial.nativePartial", summaryKey: "history.preview.notice.structureUnavailable", level: .warning),
        "other_native_items": NoticeStyle(labelKey: "history.preview.partial.nativeOther", summaryKey: "history.preview.notice.structureUnavailable", level: .warning),
        "unverified_provenance": NoticeStyle(labelKey: "history.preview.partial.unverified", summaryKey: "history.preview.notice.unverifiedSource", level: .warning),
        "unsupported_image": NoticeStyle(labelKey: "history.preview.partial.unsupportedImage", summaryKey: "history.preview.notice.imageUnsupported", level: .warning),
        "styles_simplified": NoticeStyle(labelKey: "history.preview.partial.simplified", summaryKey: "history.preview.notice.simplified", level: .diagnostic),
        "unsupported_run": NoticeStyle(labelKey: "history.preview.partial.simplified", summaryKey: "history.preview.notice.simplified", level: .diagnostic),
    ]

    private static let fallbackNoticeStyle = NoticeStyle(
        labelKey: "history.preview.partial.other",
        summaryKey: "history.preview.notice.other",
        level: .diagnostic
    )

    static func notice(code: String, detail: String?) -> HistoryPreviewDocumentMaterial.Notice {
        let style = noticeStyles[code] ?? fallbackNoticeStyle
        let normalizedDetail = (detail?.isEmpty ?? true) ? nil : detail
        return HistoryPreviewDocumentMaterial.Notice(
            code: code,
            labelKey: style.labelKey,
            summaryKey: style.summaryKey,
            detail: normalizedDetail,
            level: style.level
        )
    }

    private static func imageNoticeCode(for availability: ContractDocumentImageAvailability) -> String? {
        switch availability {
        case .missing_source, .missing_resource: return "missing_image_source"
        case .external_reference: return "external_reference"
        case .pixel_budget: return "pixel_budget"
        case .unsupported: return "unsupported_image"
        case .available: return nil
        }
    }

    static func materialize(
        _ preview: ApiClient.HistoryDocumentPreview,
        strings: HistoryPreviewDocumentStrings,
        maxRetainedBytes: Int = maximumRetainedBytes
    ) throws -> HistoryPreviewDocumentMaterial {
        let document = preview.document
        let deadline = ProcessInfo.processInfo.systemUptime + 2
        try checkWork(deadline)
        let modelBytes = try JSONEncoder().encode(document).count
        var retainedBytes = preview.assets.count * 2 + modelBytes * 8 + 4 * 1024 * 1024
        guard retainedBytes <= maxRetainedBytes else {
            throw HistoryPreviewDocumentError.tooLarge
        }
        let result = NSMutableAttributedString()
        var placementPixels = 0
        var imagePlacements = 0
        var imagePlaceholders = 0
        var tableCount = 0
        var paragraphCount = 0
        var unsupportedRunCount = 0
        var listCounters: [Int: (key: String, count: Int)] = [:]
        var pixelBudgetExceeded = false
        var synthesizedImageNotices: [String] = []

        for block in document.blocks {
            try checkWork(deadline)
            switch block.kind {
            case .paragraph:
                guard let paragraph = block.paragraph else { break }
                paragraphCount += 1
                let heading = normalizedHeading(paragraph.heading)
                let quoteDepth = Int(min(paragraph.quote_depth ?? 0, 8))
                let listDepth = Int(min(paragraph.list_depth ?? 0, 8))
                let codeBlock = paragraph.code_block ?? false
                let fontSize = bodyFontSize * headingScale(heading)
                var paragraphAttributes: [NSAttributedString.Key: Any] = [:]
                if let style = paragraphStyle(quoteDepth: quoteDepth, listDepth: listDepth, heading: heading) {
                    paragraphAttributes[.paragraphStyle] = style
                }
                if quoteDepth > 0 || codeBlock {
                    paragraphAttributes[.backgroundColor] = NSColor.windowBackgroundColor
                }
                let attributes = runAttributes(
                    nil,
                    header: heading != nil,
                    fontSize: fontSize,
                    monospaced: codeBlock
                ).merging(paragraphAttributes) { _, replacement in replacement }
                if let list = paragraph.list {
                    for depth in listCounters.keys.filter({ $0 > listDepth }) {
                        listCounters.removeValue(forKey: depth)
                    }
                    let key = "\(list.rawValue):\(quoteDepth):\(codeBlock)"
                    let previous = listCounters[listDepth]
                    let count = previous?.key == key ? previous?.count ?? 0 : 0
                    if list == .ordered {
                        listCounters[listDepth] = (key, count + 1)
                        result.append(NSAttributedString(string: "\(count + 1).\t", attributes: attributes))
                    } else {
                        listCounters[listDepth] = (key, 0)
                        result.append(NSAttributedString(string: "•\t", attributes: attributes))
                    }
                } else {
                    listCounters.removeAll(keepingCapacity: true)
                }
                unsupportedRunCount += try appendRuns(
                    paragraph.runs,
                    into: result,
                    header: heading != nil,
                    fontSize: fontSize,
                    monospaced: codeBlock,
                    baseAttributes: paragraphAttributes.isEmpty ? nil : paragraphAttributes,
                    deadline: deadline
                )
                result.append(NSAttributedString(string: "\n", attributes: attributes))
            case .table:
                guard let table = block.table else { break }
                tableCount += 1
                listCounters.removeAll(keepingCapacity: true)
                unsupportedRunCount += try appendTable(table, into: result, deadline: deadline)
                result.append(NSAttributedString(string: "\n", attributes: runAttributes(nil, header: false)))
            case .image:
                guard let image = block.image else { break }
                listCounters.removeAll(keepingCapacity: true)
                var budgetLimited = false
                var placeholderReason = image.availability.rawValue
                var imageNotice: String?
                if image.availability == .available {
                    if let assetIndex = image.asset,
                       assetIndex < UInt64(document.assets.count),
                       let bytes = preview.assetBytes(document.assets[Int(assetIndex)]) {
                        let declared = document.assets[Int(assetIndex)]
                        let cost = Int(declared.width) * Int(declared.height)
                        budgetLimited = placementPixels + cost > maximumPlacementPixels || cost * 4 > maxRetainedBytes - retainedBytes
                        pixelBudgetExceeded = pixelBudgetExceeded || budgetLimited
                        if !budgetLimited,
                           let source = CGImageSourceCreateWithData(bytes as CFData, nil),
                           CGImageSourceGetCount(source) == 1,
                           CGImageSourceGetType(source) as String? == "public.png",
                           let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [String: Any],
                           (properties[kCGImagePropertyPixelWidth as String] as? NSNumber)?.uint32Value == declared.width,
                           (properties[kCGImagePropertyPixelHeight as String] as? NSNumber)?.uint32Value == declared.height,
                           let pixels = CGImageSourceCreateImageAtIndex(source, 0, [kCGImageSourceShouldCacheImmediately: true] as CFDictionary),
                           CGImageSourceGetStatus(source) == .statusComplete,
                           pixels.width == Int(declared.width), pixels.height == Int(declared.height) {
                            try checkWork(deadline)
                            let decoded = NSImage(cgImage: pixels, size: NSSize(width: pixels.width, height: pixels.height))
                            placementPixels += cost
                            retainedBytes += cost * 4
                            imagePlacements += 1
                            result.append(NSAttributedString(attachment: boundedAttachment(decoded)))
                            result.append(NSAttributedString(string: "\n", attributes: runAttributes(nil, header: false)))
                            continue
                        }
                        if budgetLimited {
                            imageNotice = "pixel_budget"
                        } else {
                            placeholderReason = ContractDocumentImageAvailability.unsupported.rawValue
                            imageNotice = "unsupported_image"
                        }
                    } else {
                        placeholderReason = ContractDocumentImageAvailability.missing_resource.rawValue
                        imageNotice = "missing_image_source"
                    }
                } else {
                    imageNotice = imageNoticeCode(for: image.availability)
                }
                imagePlaceholders += 1
                if let imageNotice { synthesizedImageNotices.append(imageNotice) }
                let reason = budgetLimited
                    ? strings.imageBudget
                    : strings.availabilityLabel(placeholderReason)
                let placeholder = "\(strings.placeholderPrefix(placeholderReason)) (\(image.ordinal + 1)): \(reason)"
                let placeholderAttributes = runAttributes(nil, header: false).merging(
                    [.foregroundColor: NSColor.secondaryLabelColor]
                ) { _, replacement in replacement }
                result.append(NSAttributedString(string: placeholder, attributes: placeholderAttributes))
                result.append(NSAttributedString(string: "\n", attributes: runAttributes(nil, header: false)))
            }
        }

        try checkWork(deadline)

        var collectedNotices: [HistoryPreviewDocumentMaterial.Notice] = []
        var seenNoticeKeys: Set<String> = []
        func recordNotice(_ code: String, detail: String?) {
            let notice = notice(code: code, detail: detail)
            let key = "\(notice.code)\u{1F}\(notice.detail ?? "")"
            guard seenNoticeKeys.insert(key).inserted else { return }
            collectedNotices.append(notice)
        }
        for reason in document.partial {
            recordNotice(reason.code.rawValue, detail: reason.detail)
        }
        let reportedCodes = Set(document.partial.map(\.code.rawValue))
        for code in synthesizedImageNotices where !reportedCodes.contains(code) {
            recordNotice(code, detail: nil)
        }
        if pixelBudgetExceeded, !reportedCodes.contains("pixel_budget") {
            recordNotice("pixel_budget", detail: nil)
        }
        if unsupportedRunCount > 0,
           !reportedCodes.contains("unsupported_markup"),
           !reportedCodes.contains("malformed_html") {
            recordNotice("unsupported_run", detail: nil)
        }
        if document.styles_partial, !reportedCodes.contains("native_styles_unavailable") {
            recordNotice("styles_simplified", detail: nil)
        }
        let notices = collectedNotices.filter { $0.level == .warning }
            + collectedNotices.filter { $0.level == .diagnostic }

        return HistoryPreviewDocumentMaterial(
            entryId: preview.entryId,
            owner: preview.owner,
            generation: preview.generation,
            requestId: preview.requestId,
            stamp: preview.stamp,
            attributed: result,
            notices: notices,
            stylesPartial: document.styles_partial,
            imagePlacements: imagePlacements,
            imagePlaceholders: imagePlaceholders,
            tableCount: tableCount,
            paragraphCount: paragraphCount,
            retainedBytes: retainedBytes
        )
    }

    @discardableResult
    private static func appendRuns(
        _ runs: [ContractDocumentRun],
        into result: NSMutableAttributedString,
        header: Bool,
        fontSize: CGFloat = bodyFontSize,
        monospaced: Bool = false,
        baseAttributes: [NSAttributedString.Key: Any]? = nil,
        deadline: TimeInterval
    ) throws -> Int {
        var unsupported = 0
        for run in runs {
            try checkWork(deadline)
            if run.unsupported { unsupported += 1 }
            var attributes = runAttributes(run.style, header: header, fontSize: fontSize, monospaced: monospaced, link: run.link, background: baseAttributes?[.backgroundColor] as? NSColor ?? .textBackgroundColor)
            if let baseAttributes {
                attributes = baseAttributes.merging(attributes) { _, runValue in runValue }
            }
            result.append(NSAttributedString(string: run.text, attributes: attributes))
        }
        return unsupported
    }

    private static func appendTable(_ table: ContractDocumentTable, into result: NSMutableAttributedString, deadline: TimeInterval) throws -> Int {
        var unsupported = 0
        let columns = table.rows.map(\.cells.count).max() ?? 0
        guard columns > 0 else { return 0 }
        let textTable = NSTextTable()
        textTable.numberOfColumns = columns
        textTable.layoutAlgorithm = .automaticLayoutAlgorithm
        textTable.collapsesBorders = true
        for (rowIndex, row) in table.rows.enumerated() {
            for (columnIndex, cell) in row.cells.enumerated() {
                try checkWork(deadline)
                let cellBlock = NSTextTableBlock(
                    table: textTable,
                    startingRow: rowIndex,
                    rowSpan: 1,
                    startingColumn: columnIndex,
                    columnSpan: 1
                )
                cellBlock.setBorderColor(NSColor.separatorColor)
                cellBlock.setWidth(1, type: .absoluteValueType, for: .border)
                cellBlock.setWidth(6, type: .absoluteValueType, for: .padding)
                if cell.header {
                    cellBlock.backgroundColor = NSColor.controlBackgroundColor
                }
                let paragraphStyle = NSMutableParagraphStyle()
                paragraphStyle.textBlocks = [cellBlock]
                let baseAttributes: [NSAttributedString.Key: Any] = [.paragraphStyle: paragraphStyle]
                unsupported += try appendRuns(cell.runs, into: result, header: cell.header, baseAttributes: baseAttributes, deadline: deadline)
                result.append(NSAttributedString(string: "\n", attributes: runAttributes(nil, header: cell.header).merging(
                    baseAttributes
                ) { _, replacement in replacement }))
            }
        }
        return unsupported
    }

    private static func checkWork(_ deadline: TimeInterval) throws {
        try Task.checkCancellation()
        guard ProcessInfo.processInfo.systemUptime <= deadline else { throw HistoryPreviewDocumentError.timedOut }
    }

    private static func boundedAttachment(_ image: NSImage) -> NSTextAttachment {
        let attachment = NSTextAttachment()
        attachment.image = image
        let size = image.size
        guard size.width > 0, size.height > 0 else { return attachment }
        let scale = min(1, maximumImageWidth / size.width, maximumImageHeight / size.height)
        attachment.bounds = NSRect(
            x: 0,
            y: 0,
            width: (size.width * scale).rounded(),
            height: (size.height * scale).rounded()
        )
        return attachment
    }

    private static func runAttributes(
        _ style: ContractDocumentRunStyle?,
        header: Bool,
        fontSize: CGFloat = bodyFontSize,
        monospaced: Bool = false,
        link: String? = nil,
        background: NSColor = .textBackgroundColor
    ) -> [NSAttributedString.Key: Any] {
        let bold = (style?.bold ?? false) || header
        let weight: NSFont.Weight = bold ? .semibold : .regular
        var font = monospaced || style?.monospace == true
            ? NSFont.monospacedSystemFont(ofSize: fontSize, weight: weight)
            : NSFont.systemFont(ofSize: fontSize, weight: weight)
        if style?.italic == true {
            let traits = font.fontDescriptor.symbolicTraits.union(.italic)
            let descriptor = font.fontDescriptor.withSymbolicTraits(traits)
            if let italicFont = NSFont(descriptor: descriptor, size: fontSize) {
                font = italicFont
            }
        }
        var attributes: [NSAttributedString.Key: Any] = [
            .font: font,
            .foregroundColor: NSColor.labelColor,
        ]
        if style?.underline == true {
            attributes[.underlineStyle] = NSUnderlineStyle.single.rawValue
        }
        if style?.strikethrough == true {
            attributes[.strikethroughStyle] = NSUnderlineStyle.single.rawValue
        }
        if style?.superscript == true {
            attributes[.baselineOffset] = 3.0
        } else if style?.subscript == true {
            attributes[.baselineOffset] = -3.0
        }
        if let color = style?.color {
            attributes[.foregroundColor] = style?.highlight == nil
                ? HistoryPreviewDocumentContrast.readableTextColor(srgbColor(color), on: background)
                : srgbColor(color)
        }
        if let link, let url = HistoryPreviewDocumentLink.safeURL(link) {
            attributes[.link] = url
            attributes[.toolTip] = url.absoluteString
            attributes[.underlineStyle] = NSUnderlineStyle.single.rawValue
            if style?.color == nil {
                attributes[.foregroundColor] = NSColor.linkColor
            }
        }
        if let highlight = style?.highlight {
            attributes[.backgroundColor] = srgbColor(highlight)
            if style?.color.map({ HistoryPreviewDocumentContrast.contrastRatio($0, highlight) >= 4.5 }) != true {
                attributes[.foregroundColor] = HistoryPreviewDocumentContrast.readableTextColor(on: highlight)
            }
        }
        return attributes
    }

    private static func srgbColor(_ color: ContractDocumentColor) -> NSColor {
        NSColor(
            srgbRed: CGFloat(color.red) / 255,
            green: CGFloat(color.green) / 255,
            blue: CGFloat(color.blue) / 255,
            alpha: 1
        )
    }

    static func normalizedHeading(_ value: UInt8?) -> Int? {
        guard let value, value > 0 else { return nil }
        return value >= 3 ? 3 : Int(value)
    }

    private static func headingScale(_ heading: Int?) -> CGFloat {
        switch heading {
        case 1: return 1.6
        case 2: return 1.35
        case 3: return 1.18
        default: return 1
        }
    }

    private static func paragraphStyle(quoteDepth: Int, listDepth: Int, heading: Int?) -> NSParagraphStyle? {
        let indent = CGFloat(quoteDepth + listDepth) * 16
        guard indent > 0 || heading != nil else { return nil }
        let style = NSMutableParagraphStyle()
        style.headIndent = indent
        style.firstLineHeadIndent = indent
        if let heading {
            style.paragraphSpacingBefore = heading == 1 ? 10 : 6
            style.paragraphSpacing = 4
        }
        return style
    }
}
