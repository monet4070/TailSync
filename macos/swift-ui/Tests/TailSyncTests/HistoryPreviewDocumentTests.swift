import AppKit
import CryptoKit
import Foundation
import XCTest
@testable import TailSync

final class EventLog: @unchecked Sendable {
    private let lock = NSLock()
    private var storage: [String] = []

    func append(_ event: String) {
        lock.lock()
        storage.append(event)
        lock.unlock()
    }

    func snapshot() -> [String] {
        lock.lock()
        defer { lock.unlock() }
        return storage
    }

    func contains(_ event: String) -> Bool { snapshot().contains(event) }
}

actor DocumentReviewLatch {
    private var waiters: [CheckedContinuation<Void, Never>] = []
    private var opened = false

    func wait() async {
        if opened { return }
        await withCheckedContinuation { waiters.append($0) }
    }

    func open() {
        opened = true
        for waiter in waiters { waiter.resume() }
        waiters.removeAll()
    }
}

final class RecordingDocumentLinkOpener: HistoryPreviewDocumentLinkOpening {
    private(set) var opened: [URL] = []
    var result = true

    func open(_ url: URL) -> Bool {
        opened.append(url)
        return result
    }
}

@MainActor
final class HistoryPreviewDocumentTests: XCTestCase {
    private let style: [String: Any] = [
        "bold": false, "italic": false, "underline": false,
        "strikethrough": false, "subscript": false, "superscript": false,
    ]

    private func run(_ text: String, _ changes: [String: Any] = [:], link: String? = nil) -> [String: Any] {
        var style = self.style
        for (key, value) in changes { style[key] = value }
        var result: [String: Any] = ["text": text, "style": style, "unsupported": false]
        if let link { result["link"] = link }
        return result
    }

    private func paragraph(
        _ runs: [[String: Any]],
        list: String? = nil,
        heading: Int? = nil,
        quoteDepth: Int? = nil,
        codeBlock: Bool? = nil,
        listDepth: Int? = nil
    ) -> [String: Any] {
        var paragraph: [String: Any] = ["runs": runs]
        if let list { paragraph["list"] = list }
        if let heading { paragraph["heading"] = heading }
        if let quoteDepth { paragraph["quote_depth"] = quoteDepth }
        if let codeBlock { paragraph["code_block"] = codeBlock }
        if let listDepth { paragraph["list_depth"] = listDepth }
        return ["kind": "paragraph", "paragraph": paragraph]
    }

    private func image(_ asset: Int?, ordinal: Int, availability: String = "available") -> [String: Any] {
        var image: [String: Any] = ["ordinal": ordinal, "availability": availability]
        if let asset { image["asset"] = asset }
        return ["kind": "image", "image": image]
    }

    private func table(_ rows: [[[String: Any]]], truncated: Bool = false) -> [String: Any] {
        ["kind": "table", "table": ["rows": rows.map { ["cells": $0] }, "truncated": truncated]]
    }

    private func cell(_ header: Bool, _ runs: [[String: Any]]) -> [String: Any] {
        ["header": header, "runs": runs]
    }

    private func png(width: Int, height: Int, red: CGFloat, green: CGFloat, blue: CGFloat) throws -> Data {
        let colorSpace = CGColorSpaceCreateDeviceRGB()
        let context = try XCTUnwrap(CGContext(
            data: nil, width: width, height: height, bitsPerComponent: 8,
            bytesPerRow: width * 4, space: colorSpace,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
        ))
        context.setFillColor(CGColor(srgbRed: red, green: green, blue: blue, alpha: 1))
        context.fill(CGRect(x: 0, y: 0, width: width, height: height))
        let cgImage = try XCTUnwrap(context.makeImage())
        let representation = NSBitmapImageRep(cgImage: cgImage)
        return try XCTUnwrap(representation.representation(using: .png, properties: [:]))
    }

    private func frame(
        document: [String: Any],
        payload: Data,
        entryId: Int64 = 7,
        owner: String = "history",
        generation: UInt64 = 3,
        requestId: String = "document-test"
    ) throws -> Data {
        let model = try JSONSerialization.data(withJSONObject: document)
        let metadata: [String: Any] = [
            "request_id": requestId, "entry_id": entryId, "kind": "document",
            "name": "document", "stamp": "dc1:" + String(repeating: "0a", count: 32),
            "owner": owner, "generation": generation,
            "model_bytes": model.count, "size_bytes": model.count + payload.count,
        ]
        let header = try JSONSerialization.data(withJSONObject: metadata)
        var output = Data("TSDC".utf8)
        output.append(1)
        let length = UInt32(header.count)
        for shift in [0, 8, 16, 24] { output.append(UInt8((length >> shift) & 255)) }
        output.append(header)
        output.append(model)
        output.append(payload)
        return output
    }

    private func asset(index: Int, offset: Int, bytes: Data, width: UInt32, height: UInt32) -> [String: Any] {
        [
            "index": index, "offset": offset, "length": bytes.count,
            "width": width, "height": height,
            "sha256": SHA256.hash(data: bytes).map { String(format: "%02x", $0) }.joined(),
        ]
    }

    private func preview(
        document: [String: Any],
        payload: Data,
        entryId: Int64 = 7,
        owner: String = "history",
        generation: UInt64 = 3,
        requestId: String = "document-test"
    ) throws -> ApiClient.HistoryDocumentPreview {
        let bytes = try frame(
            document: document, payload: payload, entryId: entryId,
            owner: owner, generation: generation, requestId: requestId
        )
        return try ApiClient.decodeDocumentPreview(
            bytes,
            expectedRequestID: requestId,
            expectedEntryId: entryId,
            expectedOwner: owner,
            expectedGeneration: generation
        )
    }

    private func strings() -> HistoryPreviewDocumentStrings {
        HistoryPreviewDocumentStrings(
            imageUnavailable: "IMAGE-UNAVAILABLE",
            imageCannotDisplay: "IMAGE-CANNOT-DISPLAY",
            imageBudget: "IMAGE-BUDGET",
            availability: [
                "available": "IMAGE-AVAILABLE",
                "missing_resource": "MISSING-RESOURCE",
                "missing_source": "MISSING-SOURCE",
                "external_reference": "EXTERNAL-REFERENCE",
                "unsupported": "UNSUPPORTED",
                "pixel_budget": "PIXEL-BUDGET",
            ]
        )
    }

    private func attachments(_ attributed: NSAttributedString) -> [NSTextAttachment] {
        var result: [NSTextAttachment] = []
        attributed.enumerateAttribute(.attachment, in: NSRange(location: 0, length: attributed.length)) { value, _, _ in
            if let attachment = value as? NSTextAttachment { result.append(attachment) }
        }
        return result
    }

    private func font(at index: Int, in attributed: NSAttributedString) -> NSFont? {
        attributed.attribute(.font, at: index, effectiveRange: nil) as? NSFont
    }

    private func weight(_ font: NSFont?) -> CGFloat {
        guard let font,
              let traits = font.fontDescriptor.object(forKey: .traits) as? [NSFontDescriptor.TraitKey: Any],
              let weight = traits[.weight] as? NSNumber else { return 0 }
        return CGFloat(weight.doubleValue)
    }

    private func index(of needle: String, in attributed: NSAttributedString) -> Int {
        let location = (attributed.string as NSString).range(of: needle).location
        XCTAssertNotEqual(location, NSNotFound, "missing \(needle)")
        return location
    }

    private func srgbComponents(_ color: NSColor?) -> [CGFloat] {
        guard let converted = color?.usingColorSpace(.sRGB) else { return [] }
        return [converted.redComponent, converted.greenComponent, converted.blueComponent]
    }

    private func tableBlocks(_ attributed: NSAttributedString, startingRow: Int) -> [NSTextTableBlock] {
        var result: [NSTextTableBlock] = []
        attributed.enumerateAttribute(.paragraphStyle, in: NSRange(location: 0, length: attributed.length)) { value, _, _ in
            guard let paragraph = value as? NSParagraphStyle else { return }
            for block in paragraph.textBlocks {
                guard let table = block as? NSTextTableBlock, table.startingRow == startingRow else { continue }
                result.append(table)
            }
        }
        return result
    }

    func testControlledDocumentKeepsTextStylesOrderAndEmptyTableCells() throws {
        let red = try png(width: 4, height: 2, red: 1, green: 0, blue: 0)
        let blue = try png(width: 2, height: 4, red: 0, green: 0, blue: 1)
        var payload = Data()
        let redOffset = 0
        payload.append(red)
        let blueOffset = red.count
        payload.append(blue)
        let document: [String: Any] = [
            "version": 1,
            "blocks": [
                paragraph([run("中文😀 ", ["bold": true, "color": ["red": 255, "green": 0, "blue": 0]])]),
                image(0, ordinal: 0),
                paragraph([run("两图之间🧪")]),
                image(1, ordinal: 1),
                table([[cell(true, [run("A")]), cell(false, []), cell(false, [run("B")])]]),
                paragraph([run("<script>never execute</script> https://bad.invalid file:///private/secret")]),
            ],
            "assets": [
                asset(index: 0, offset: redOffset, bytes: red, width: 4, height: 2),
                asset(index: 1, offset: blueOffset, bytes: blue, width: 2, height: 4),
            ],
            "partial": [],
            "styles_partial": false,
            "first_image": 0,
        ]
        let material = try HistoryPreviewDocumentBuilder.materialize(
            try preview(document: document, payload: payload),
            strings: strings()
        )
        let attributed = material.attributed
        let text = attributed.string

        XCTAssertTrue(text.contains("中文😀 "))
        XCTAssertTrue(text.contains("两图之间🧪"))
        XCTAssertTrue(text.contains("<script>never execute</script> https://bad.invalid file:///private/secret"))
        XCTAssertFalse(text.contains("MISSING"))

        let boldIndex = (text as NSString).range(of: "中文").location
        let plainIndex = (text as NSString).range(of: "两图之间").location
        XCTAssertGreaterThan(weight(font(at: boldIndex, in: attributed)), weight(font(at: plainIndex, in: attributed)))
        let color = attributed.attribute(.foregroundColor, at: boldIndex, effectiveRange: nil) as? NSColor
        var converted: [CGFloat] = []
        try XCTUnwrap(NSAppearance(named: .aqua)).performAsCurrentDrawingAppearance {
            converted = srgbComponents(color)
        }
        XCTAssertEqual(converted, [0, 0, 0])

        let attachments = self.attachments(attributed)
        XCTAssertEqual(attachments.count, 2)
        XCTAssertEqual(attachments[0].bounds.width, 4)
        XCTAssertEqual(attachments[0].bounds.height, 2)
        XCTAssertEqual(attachments[1].bounds.width, 2)
        XCTAssertEqual(attachments[1].bounds.height, 4)
        let redPixels = try XCTUnwrap(attachments[0].image?.cgImage(forProposedRect: nil, context: nil, hints: nil))
        let bluePixels = try XCTUnwrap(attachments[1].image?.cgImage(forProposedRect: nil, context: nil, hints: nil))
        let firstColor = try XCTUnwrap(NSBitmapImageRep(cgImage: redPixels).colorAt(x: 0, y: 0)?.usingColorSpace(.sRGB))
        let secondColor = try XCTUnwrap(NSBitmapImageRep(cgImage: bluePixels).colorAt(x: 0, y: 0)?.usingColorSpace(.sRGB))
        XCTAssertEqual(firstColor.redComponent, 1, accuracy: 0.02)
        XCTAssertEqual(firstColor.blueComponent, 0, accuracy: 0.02)
        XCTAssertEqual(secondColor.redComponent, 0, accuracy: 0.02)
        XCTAssertEqual(secondColor.blueComponent, 1, accuracy: 0.02)

        XCTAssertEqual(material.imagePlacements, 2)
        XCTAssertEqual(material.imagePlaceholders, 0)
        XCTAssertEqual(material.tableCount, 1)
        XCTAssertEqual(tableBlocks(attributed, startingRow: 0).count, 3)
    }

    func testUnavailableFirstImageKeepsItsPlaceholderWithoutPromotingTheSecond() throws {
        let blue = try png(width: 2, height: 4, red: 0, green: 0, blue: 1)
        let document: [String: Any] = [
            "version": 1,
            "blocks": [image(nil, ordinal: 0, availability: "external_reference"), image(0, ordinal: 1)],
            "assets": [asset(index: 0, offset: 0, bytes: blue, width: 2, height: 4)],
            "partial": [["code": "external_reference", "detail": "first image unavailable"]],
            "styles_partial": true,
            "first_image": NSNull(),
        ]
        let material = try HistoryPreviewDocumentBuilder.materialize(
            try preview(document: document, payload: blue),
            strings: strings()
        )

        XCTAssertEqual(material.imagePlacements, 1)
        XCTAssertEqual(material.imagePlaceholders, 1)
        XCTAssertTrue(material.attributed.string.contains("IMAGE-UNAVAILABLE (1): EXTERNAL-REFERENCE"))
        XCTAssertEqual(attachments(material.attributed).count, 1)
        XCTAssertEqual(attachments(material.attributed)[0].bounds.width, 2)
        XCTAssertTrue(material.notices.contains { $0.code == "external_reference" })
        XCTAssertFalse(material.notices.contains { $0.code == "native_styles_unavailable" })
        XCTAssertEqual(material.notices.filter { $0.code == "external_reference" }.count, 1)
    }

    func testCorruptAssetBytesDegradeToAPlaceholderWithADecodeWarning() throws {
        let valid = try png(width: 2, height: 2, red: 1, green: 1, blue: 1)
        let corrupt = Data(valid.prefix(33))
        let document: [String: Any] = [
            "version": 1,
            "blocks": [image(0, ordinal: 0)],
            "assets": [asset(index: 0, offset: 0, bytes: corrupt, width: 2, height: 2)],
            "partial": [],
            "styles_partial": false,
            "first_image": 0,
        ]
        let material = try HistoryPreviewDocumentBuilder.materialize(
            try preview(document: document, payload: corrupt),
            strings: strings()
        )

        XCTAssertEqual(material.imagePlacements, 0)
        XCTAssertEqual(material.imagePlaceholders, 1)
        XCTAssertFalse(material.attributed.string.contains("IMAGE-AVAILABLE"))
        XCTAssertTrue(material.attributed.string.contains("IMAGE-CANNOT-DISPLAY (1): UNSUPPORTED"))
        XCTAssertFalse(material.attributed.string.contains("IMAGE-UNAVAILABLE"))
        XCTAssertTrue(material.notices.contains { $0.code == "unsupported_image" && $0.level == .warning })
    }

    func testPlaceholderCopySeparatesUnsavedImagesFromUnrenderableOnes() throws {
        let document: [String: Any] = [
            "version": 1,
            "blocks": [
                image(nil, ordinal: 0, availability: "missing_source"),
                image(nil, ordinal: 1, availability: "external_reference"),
                image(nil, ordinal: 2, availability: "unsupported"),
            ],
            "assets": [],
            "partial": [],
            "styles_partial": false,
            "first_image": NSNull(),
        ]
        let material = try HistoryPreviewDocumentBuilder.materialize(
            try preview(document: document, payload: Data()),
            strings: strings()
        )

        XCTAssertTrue(material.attributed.string.contains("IMAGE-UNAVAILABLE (1): MISSING-SOURCE"))
        XCTAssertTrue(material.attributed.string.contains("IMAGE-UNAVAILABLE (2): EXTERNAL-REFERENCE"))
        XCTAssertTrue(material.attributed.string.contains("IMAGE-CANNOT-DISPLAY (3): UNSUPPORTED"))
        XCTAssertFalse(material.attributed.string.contains("IMAGE-CANNOT-DISPLAY (1)"))
        XCTAssertFalse(material.attributed.string.contains("IMAGE-CANNOT-DISPLAY (2)"))
    }

    func testUnreportedImageAvailabilityStillRaisesAWarning() throws {
        let document: [String: Any] = [
            "version": 1,
            "blocks": [image(nil, ordinal: 0, availability: "missing_resource")],
            "assets": [],
            "partial": [],
            "styles_partial": false,
            "first_image": NSNull(),
        ]
        let material = try HistoryPreviewDocumentBuilder.materialize(
            try preview(document: document, payload: Data()),
            strings: strings()
        )

        XCTAssertEqual(material.notices.filter { $0.code == "missing_image_source" }.count, 1)
        XCTAssertTrue(material.warningNotices.contains { $0.code == "missing_image_source" })
        XCTAssertFalse(material.attributed.string.contains("IMAGE-AVAILABLE"))
        XCTAssertTrue(material.attributed.string.contains("IMAGE-UNAVAILABLE (1): MISSING-RESOURCE"))
    }

    func testNativeAnalysisUnavailableIsABodyLossWarning() throws {
        let document: [String: Any] = [
            "version": 1,
            "blocks": [],
            "assets": [],
            "partial": [["code": "native_analysis_unavailable", "detail": "RTF body facts require the supervised helper"]],
            "styles_partial": false,
        ]
        let material = try HistoryPreviewDocumentBuilder.materialize(
            try preview(document: document, payload: Data()),
            strings: strings()
        )
        XCTAssertEqual(material.warningNotices.map(\.code), ["native_analysis_unavailable"])
        XCTAssertEqual(material.warningSummaryKeys, ["history.preview.notice.nativeUnavailable"])
        XCTAssertTrue(material.diagnosticNotices.isEmpty)
    }

    func testUnsupportedMarkupStaysAWarningEvenWhenStylesWereSimplified() throws {
        let structuralDocument: [String: Any] = [
            "version": 1,
            "blocks": [paragraph([run("正文")])],
            "assets": [],
            "partial": [["code": "unsupported_markup", "detail": "a nested table is flattened"]],
            "styles_partial": false,
        ]
        let structuralMaterial = try HistoryPreviewDocumentBuilder.materialize(
            try preview(document: structuralDocument, payload: Data()),
            strings: strings()
        )
        XCTAssertTrue(structuralMaterial.warningNotices.contains { $0.code == "unsupported_markup" })

        let decorativeDocument: [String: Any] = [
            "version": 1,
            "blocks": [paragraph([run("正文")])],
            "assets": [],
            "partial": [["code": "unsupported_markup", "detail": "decorative declarations were simplified for reading"]],
            "styles_partial": true,
        ]
        let decorativeMaterial = try HistoryPreviewDocumentBuilder.materialize(
            try preview(document: decorativeDocument, payload: Data()),
            strings: strings()
        )
        XCTAssertEqual(decorativeMaterial.warningNotices.map(\.code), ["unsupported_markup"])
        XCTAssertEqual(decorativeMaterial.warningSummaryKeys, ["history.preview.notice.unsupportedMarkup"])
        XCTAssertEqual(decorativeMaterial.diagnosticNotices.map(\.code), ["styles_simplified"])
    }

    func testRepeatedPlacementsAreChargedIndividuallyAndDegradeToPlaceholders() throws {
        let large = try png(width: 2048, height: 2048, red: 1, green: 0, blue: 0)
        let document: [String: Any] = [
            "version": 1,
            "blocks": (0..<5).map { image(0, ordinal: $0) },
            "assets": [asset(index: 0, offset: 0, bytes: large, width: 2048, height: 2048)],
            "partial": [],
            "styles_partial": false,
            "first_image": 0,
        ]
        let material = try HistoryPreviewDocumentBuilder.materialize(
            try preview(document: document, payload: large),
            strings: strings()
        )
        XCTAssertEqual(material.imagePlacements + material.imagePlaceholders, 5)
        XCTAssertEqual(material.imagePlacements, 3)
        XCTAssertEqual(material.imagePlaceholders, 2)
        XCTAssertTrue(material.notices.contains { $0.code == "pixel_budget" })
        XCTAssertTrue(material.attributed.string.contains("IMAGE-CANNOT-DISPLAY (4): IMAGE-BUDGET"))
        XCTAssertFalse(material.attributed.string.contains("IMAGE-UNAVAILABLE"))
    }

    func testUnsupportedRunsAndStylesPartialProduceHonestNotices() throws {
        let document: [String: Any] = [
            "version": 1,
            "blocks": [paragraph([["text": "styled", "style": style, "unsupported": true]])],
            "assets": [],
            "partial": [],
            "styles_partial": true,
        ]
        let material = try HistoryPreviewDocumentBuilder.materialize(
            try preview(document: document, payload: Data()),
            strings: strings()
        )
        XCTAssertTrue(material.warningNotices.isEmpty)
        XCTAssertEqual(material.diagnosticNotices.map(\.code), ["unsupported_run", "styles_simplified"])
        XCTAssertEqual(material.diagnosticNotices.first?.code, "unsupported_run")
        XCTAssertEqual(material.diagnosticNotices.first?.labelKey, "history.preview.partial.simplified")
        XCTAssertEqual(material.diagnosticNotices.first?.summaryKey, "history.preview.notice.simplified")
        XCTAssertFalse(material.notices.contains { $0.code == "native_styles_unavailable" })
        XCTAssertTrue(material.stylesPartial)
        let index = (material.attributed.string as NSString).range(of: "styled").location
        XCTAssertEqual(weight(font(at: index, in: material.attributed)), weight(NSFont.systemFont(ofSize: 13)))
    }

    func testStylesPartialAloneRaisesOnlyASimplificationDiagnostic() throws {
        let document: [String: Any] = [
            "version": 1,
            "blocks": [paragraph([run("正文")])],
            "assets": [],
            "partial": [],
            "styles_partial": true,
        ]
        let material = try HistoryPreviewDocumentBuilder.materialize(
            try preview(document: document, payload: Data()),
            strings: strings()
        )
        XCTAssertTrue(material.stylesPartial)
        XCTAssertTrue(material.warningNotices.isEmpty)
        XCTAssertTrue(material.warningSummaryKeys.isEmpty)
        XCTAssertEqual(material.diagnosticNotices.map(\.code), ["styles_simplified"])
        XCTAssertEqual(material.diagnosticNotices.first?.labelKey, "history.preview.partial.simplified")
        XCTAssertEqual(material.diagnosticNotices.first?.summaryKey, "history.preview.notice.simplified")
    }

    func testDuplicatePartialReasonsCollapseToASingleNotice() throws {
        let document: [String: Any] = [
            "version": 1,
            "blocks": [paragraph([run("正文")])],
            "assets": [],
            "partial": [
                ["code": "external_reference", "detail": "same detail"],
                ["code": "external_reference", "detail": "same detail"],
            ],
            "styles_partial": false,
        ]
        let material = try HistoryPreviewDocumentBuilder.materialize(
            try preview(document: document, payload: Data()),
            strings: strings()
        )
        XCTAssertEqual(material.notices.filter { $0.code == "external_reference" }.count, 1)
        XCTAssertEqual(material.notices.count, 1)
    }

    func testSamePartialCodeKeepsDistinctDetails() throws {
        let document: [String: Any] = [
            "version": 1,
            "blocks": [paragraph([run("正文")])],
            "assets": [],
            "partial": [
                ["code": "external_reference", "detail": "first reason"],
                ["code": "external_reference", "detail": "second reason"],
            ],
            "styles_partial": false,
        ]
        let material = try HistoryPreviewDocumentBuilder.materialize(
            try preview(document: document, payload: Data()),
            strings: strings()
        )
        XCTAssertEqual(material.notices.filter { $0.code == "external_reference" }.count, 2)
        XCTAssertTrue(material.notices.contains { $0.detail == "first reason" })
        XCTAssertTrue(material.notices.contains { $0.detail == "second reason" })
    }

    func testRealWarningsSurviveForMissingImageTruncationStructureBudgetAndProvenance() throws {
        let document: [String: Any] = [
            "version": 1,
            "blocks": [paragraph([run("正文")])],
            "assets": [],
            "partial": [
                ["code": "missing_image_source", "detail": "no image bytes"],
                ["code": "text_limit", "detail": "text budget reached"],
                ["code": "native_table_structure_unavailable", "detail": "native tables are counts"],
                ["code": "unverified_provenance", "detail": "origin could not be verified"],
                ["code": "pixel_budget", "detail": "placement budget reached"],
            ],
            "styles_partial": true,
        ]
        let material = try HistoryPreviewDocumentBuilder.materialize(
            try preview(document: document, payload: Data()),
            strings: strings()
        )
        XCTAssertTrue(material.notices.contains { $0.code == "missing_image_source" && $0.detail == "no image bytes" })
        XCTAssertTrue(material.notices.contains { $0.code == "text_limit" && $0.detail == "text budget reached" })
        XCTAssertTrue(material.notices.contains { $0.code == "native_table_structure_unavailable" })
        XCTAssertTrue(material.notices.contains { $0.code == "unverified_provenance" })
        XCTAssertTrue(material.notices.contains { $0.code == "pixel_budget" })
        XCTAssertFalse(material.notices.contains { $0.code == "native_styles_unavailable" })
        XCTAssertEqual(material.warningSummaryKeys, [
            "history.preview.notice.missingImage",
            "history.preview.notice.truncated",
            "history.preview.notice.structureUnavailable",
            "history.preview.notice.unverifiedSource",
            "history.preview.notice.pixelBudget",
        ])
        XCTAssertTrue(material.warningNotices.allSatisfy { $0.level == .warning })
        XCTAssertEqual(material.diagnosticNotices.map(\.code), ["styles_simplified"])
    }

    func testWarningSummaryKeysDeduplicateRepeatedCategories() throws {
        let document: [String: Any] = [
            "version": 1,
            "blocks": [paragraph([run("正文")])],
            "assets": [],
            "partial": [
                ["code": "text_limit", "detail": "text budget"],
                ["code": "block_limit", "detail": "block budget"],
                ["code": "missing_image_source", "detail": "no image bytes"],
            ],
            "styles_partial": false,
        ]
        let material = try HistoryPreviewDocumentBuilder.materialize(
            try preview(document: document, payload: Data()),
            strings: strings()
        )
        XCTAssertEqual(material.warningNotices.count, 3)
        XCTAssertEqual(material.warningSummaryKeys, [
            "history.preview.notice.truncated",
            "history.preview.notice.missingImage",
        ])
    }

    func testUnknownPartialCodeStaysReachableAsADiagnostic() {
        let notice = HistoryPreviewDocumentBuilder.notice(code: "future_reason", detail: "why it happened")
        XCTAssertEqual(notice.code, "future_reason")
        XCTAssertEqual(notice.detail, "why it happened")
        XCTAssertEqual(notice.labelKey, "history.preview.partial.other")
        XCTAssertEqual(notice.summaryKey, "history.preview.notice.other")
        XCTAssertEqual(notice.level, .diagnostic)
    }

    func testKnownNoticeStylesMatchTheWindowsRenderer() {
        let simplified = HistoryPreviewDocumentBuilder.notice(code: "native_styles_unavailable", detail: nil)
        XCTAssertEqual(simplified.level, .diagnostic)
        XCTAssertEqual(simplified.summaryKey, "history.preview.notice.simplified")
        XCTAssertEqual(HistoryPreviewDocumentBuilder.notice(code: "native_analysis_unavailable", detail: nil).level, .warning)
        XCTAssertEqual(HistoryPreviewDocumentBuilder.notice(code: "unsupported_markup", detail: nil).level, .warning)
        XCTAssertEqual(HistoryPreviewDocumentBuilder.notice(code: "unsupported_image", detail: nil).level, .warning)
        XCTAssertEqual(
            HistoryPreviewDocumentBuilder.notice(code: "unsupported_image", detail: nil).summaryKey,
            "history.preview.notice.imageUnsupported"
        )
        let structure = HistoryPreviewDocumentBuilder.notice(code: "native_table_structure_unavailable", detail: nil)
        XCTAssertEqual(structure.level, .warning)
        XCTAssertEqual(structure.summaryKey, "history.preview.notice.structureUnavailable")
        XCTAssertEqual(HistoryPreviewDocumentBuilder.notice(code: "pixel_budget", detail: nil).level, .warning)
        XCTAssertEqual(HistoryPreviewDocumentBuilder.notice(code: "external_reference", detail: nil).summaryKey, "history.preview.notice.missingImage")
        XCTAssertEqual(HistoryPreviewDocumentBuilder.notice(code: "unverified_provenance", detail: nil).level, .warning)
        XCTAssertEqual(HistoryPreviewDocumentBuilder.notice(code: "text_limit", detail: nil).summaryKey, "history.preview.notice.truncated")
    }

    func testDocumentTextViewAndAccessibilityDeclarationsAreReadOnlyAndTestable() throws {
        let document: [String: Any] = [
            "version": 1,
            "blocks": [paragraph([run("正文")]), image(nil, ordinal: 0, availability: "missing_resource")],
            "assets": [],
            "partial": [],
            "styles_partial": false,
        ]
        let material = try HistoryPreviewDocumentBuilder.materialize(
            try preview(document: document, payload: Data()),
            strings: strings()
        )
        let summary = material.accessibilitySummary(template: "{paragraphs}|{tables}|{images}|{placeholders}")
        XCTAssertEqual(summary, "1|0|0|1")

        let textView = HistoryPreviewDocumentTextFactory.makeTextView()
        XCTAssertFalse(textView.isEditable)
        XCTAssertTrue(textView.isSelectable)
        XCTAssertTrue(textView.usesFindBar)
        XCTAssertTrue(textView.isIncrementalSearchingEnabled)
        XCTAssertFalse(textView.isHorizontallyResizable)
    }


    private func documentRequest(id: Int64) -> HistoryPreviewRequest {
        HistoryPreviewRequest(
            items: [HistoryPreviewItem(
                id: id,
                batchId: nil,
                batchIndex: nil,
                batchCount: nil,
                category: "text",
                type: "composite",
                nameHint: "entry-\(id)",
                sizeBytes: 12,
                displayKind: "document"
            )],
            selectedIndex: 0
        )
    }

    private func simpleDocument() -> [String: Any] {
        [
            "version": 1,
            "blocks": [paragraph([run("正文😀")])],
            "assets": [],
            "partial": [],
            "styles_partial": false,
        ]
    }

    func testHistoryRefreshChecksTheSavedStampBeforeReloadingWithoutRestoring() async throws {
        let log = EventLog()
        let saved = try preview(document: simpleDocument(), payload: Data(), entryId: 17)
        let model = HistoryPreviewViewModel(dependencies: HistoryPreviewDependencies(
            load: { _, _ in throw HistoryPreviewRemoteError(code: .unsupportedType, message: "no legacy") },
            restore: { _ in log.append("restore") },
            loadDocument: { _, _, _, _ in log.append("load"); return saved },
            validateDocument: { _ in true },
            validateDocumentStamp: { id, _, _, _, _ in
                log.append("validate:\(id)")
                return !log.contains("stale")
            },
            materializeDocument: { preview, strings in
                try HistoryPreviewDocumentBuilder.materialize(preview, strings: strings)
            }
        ))
        defer { model.close() }
        model.present(documentRequest(id: 17))
        try await waitUntil { self.isDocument(model, entryId: 17) }
        await model.refreshSavedDocument()
        XCTAssertEqual(log.snapshot().filter { $0 == "load" }.count, 1)
        log.append("stale")
        await model.refreshSavedDocument()
        try await waitUntil { log.snapshot().filter { $0 == "load" }.count == 2 && self.isDocument(model, entryId: 17) }
        model.setHostedRestoreState(.restoring)
        await model.refreshSavedDocument()
        XCTAssertEqual(log.snapshot().filter { $0 == "load" }.count, 2)
        XCTAssertFalse(log.contains("restore"))
    }

    func testLateSavedStampValidationDoesNotResurrectAClosedPreview() async throws {
        let log = EventLog()
        let latch = DocumentReviewLatch()
        let saved = try preview(document: simpleDocument(), payload: Data(), entryId: 18)
        let model = HistoryPreviewViewModel(dependencies: HistoryPreviewDependencies(
            load: { _, _ in throw HistoryPreviewRemoteError(code: .unsupportedType, message: "no legacy") },
            restore: { _ in },
            loadDocument: { _, _, _, _ in log.append("load"); return saved },
            validateDocument: { _ in true },
            validateDocumentStamp: { _, _, _, _, _ in
                log.append("checking")
                await latch.wait()
                return false
            },
            materializeDocument: { preview, strings in
                try HistoryPreviewDocumentBuilder.materialize(preview, strings: strings)
            }
        ))
        model.present(documentRequest(id: 18))
        try await waitUntil { self.isDocument(model, entryId: 18) }
        let refresh = Task { await model.refreshSavedDocument() }
        try await waitUntil { log.contains("checking") }
        model.close()
        await latch.open()
        await refresh.value
        XCTAssertEqual(log.snapshot().filter { $0 == "load" }.count, 1)
        guard case .idle = model.state else { return XCTFail("Late validation reopened the preview") }
    }

    func testSavedResourceNotificationDuringLoadingIsAppliedAfterTheCandidateSettles() async throws {
        let log = EventLog()
        let latch = DocumentReviewLatch()
        let saved = try preview(document: simpleDocument(), payload: Data(), entryId: 19)
        let model = HistoryPreviewViewModel(dependencies: HistoryPreviewDependencies(
            load: { _, _ in throw HistoryPreviewRemoteError(code: .unsupportedType, message: "no legacy") },
            restore: { _ in },
            loadDocument: { _, _, _, _ in
                log.append("load")
                await latch.wait()
                return saved
            },
            validateDocument: { _ in true },
            validateDocumentStamp: { _, _, _, _, _ in false },
            materializeDocument: { preview, strings in
                try HistoryPreviewDocumentBuilder.materialize(preview, strings: strings)
            }
        ))
        defer { model.close() }
        model.present(documentRequest(id: 19))
        try await waitUntil { log.contains("load") }
        let refresh = Task { await model.refreshSavedDocument() }
        await latch.open()
        await refresh.value
        try await waitUntil { log.snapshot().filter { $0 == "load" }.count == 2 && self.isDocument(model, entryId: 19) }
    }

    private func waitUntil(
        attempts: Int = 300,
        _ condition: @MainActor () -> Bool
    ) async throws {
        for _ in 0..<attempts {
            if condition() { return }
            try await Task.sleep(nanoseconds: 10_000_000)
        }
        XCTFail("Timed out waiting for document preview state")
    }

    private func isDocument(_ model: HistoryPreviewViewModel, entryId: Int64? = nil) -> Bool {
        guard case .document(let material) = model.state else { return false }
        return entryId == nil || material.entryId == entryId
    }

    func testDocumentItemInstallsOnlyAfterBackendValidation() async throws {
        let log = EventLog()
        let preview = try preview(
            document: simpleDocument(), payload: Data(),
            entryId: 7, owner: "history", generation: 1, requestId: "vm-1"
        )
        let model = HistoryPreviewViewModel(dependencies: HistoryPreviewDependencies(
            load: { _, _ in
                log.append("legacy-load")
                throw HistoryPreviewRemoteError(code: .unsupportedType, message: "no legacy preview")
            },
            restore: { _ in log.append("restore") },
            loadDocument: { id, owner, _, _ in
                log.append("load:\(id):\(owner)")
                return preview
            },
            validateDocument: { _ in
                log.append("validate")
                return true
            },
            cancelDocument: { id, _, _, _ in log.append("cancel:\(id)") },
            materializeDocument: { preview, strings in
                log.append("materialize")
                return try HistoryPreviewDocumentBuilder.materialize(preview, strings: strings)
            }
        ))

        model.present(documentRequest(id: 7))
        try await waitUntil { self.isDocument(model, entryId: 7) }

        XCTAssertEqual(log.snapshot(), ["load:7:history", "materialize", "validate"])
        XCTAssertEqual(model.currentEntryId, 7)
        XCTAssertFalse(log.contains("legacy-load"))
        XCTAssertFalse(log.contains("restore"))
    }

    func testStaleDocumentResultNeverInstallsAndSendsCancel() async throws {
        let log = EventLog()
        let first = try preview(
            document: simpleDocument(), payload: Data(),
            entryId: 1, owner: "history", generation: 1, requestId: "vm-1"
        )
        let second = try preview(
            document: simpleDocument(), payload: Data(),
            entryId: 2, owner: "history", generation: 2, requestId: "vm-2"
        )
        let model = HistoryPreviewViewModel(dependencies: HistoryPreviewDependencies(
            load: { _, _ in throw HistoryPreviewRemoteError(code: .unsupportedType, message: "no legacy") },
            restore: { _ in },
            loadDocument: { id, _, _, _ in id == 1 ? first : second },
            validateDocument: { preview in
                if preview.entryId == 1 {
                    try? await Task.sleep(nanoseconds: 150_000_000)
                    return false
                }
                return true
            },
            cancelDocument: { id, _, _, _ in log.append("cancel:\(id)") },
            materializeDocument: { preview, strings in
                try HistoryPreviewDocumentBuilder.materialize(preview, strings: strings)
            }
        ))

        model.present(documentRequest(id: 1))
        try await Task.sleep(nanoseconds: 20_000_000)
        model.present(documentRequest(id: 2))

        try await waitUntil { self.isDocument(model, entryId: 2) }
        try await waitUntil { log.contains("cancel:1") }
        XCTAssertEqual(model.currentEntryId, 2)
        if case .failed = model.state { XCTFail("a stale candidate must not fail the newer preview") }
    }

    func testOwnerReachesTheDocumentRequest() async throws {
        let log = EventLog()
        let preview = try preview(
            document: simpleDocument(), payload: Data(),
            entryId: 5, owner: "favorites", generation: 1, requestId: "vm-fav"
        )
        let model = HistoryPreviewViewModel(dependencies: HistoryPreviewDependencies(
            load: { _, _ in throw HistoryPreviewRemoteError(code: .unsupportedType, message: "no legacy") },
            restore: { _ in },
            loadDocument: { id, owner, _, _ in
                log.append("load:\(id):\(owner)")
                return preview
            },
            validateDocument: { _ in true },
            materializeDocument: { preview, strings in
                try HistoryPreviewDocumentBuilder.materialize(preview, strings: strings)
            }
        ))

        model.present(documentRequest(id: 5), owner: .favorites)
        try await waitUntil { self.isDocument(model, entryId: 5) }
        XCTAssertTrue(log.contains("load:5:favorites"))
    }

    func testGenerationStaysMonotonicAcrossViewModelRecreation() async throws {
        let log = EventLog()
        let preview = try preview(
            document: simpleDocument(), payload: Data(),
            entryId: 3, owner: "history", generation: 1, requestId: "vm-gen"
        )
        func makeModel() -> HistoryPreviewViewModel {
            HistoryPreviewViewModel(dependencies: HistoryPreviewDependencies(
                load: { _, _ in throw HistoryPreviewRemoteError(code: .unsupportedType, message: "no legacy") },
                restore: { _ in },
                loadDocument: { _, _, generation, _ in
                    log.append("generation:\(generation)")
                    return preview
                },
                validateDocument: { _ in true },
                materializeDocument: { preview, strings in
                    try HistoryPreviewDocumentBuilder.materialize(preview, strings: strings)
                }
            ))
        }

        let first = makeModel()
        first.present(documentRequest(id: 3))
        try await waitUntil { self.isDocument(first, entryId: 3) }
        let second = makeModel()
        second.present(documentRequest(id: 3))
        try await waitUntil { self.isDocument(second, entryId: 3) }

        let generations = log.snapshot()
            .filter { $0.hasPrefix("generation:") }
            .compactMap { UInt64($0.dropFirst("generation:".count)) }
        XCTAssertEqual(generations.count, 2)
        XCTAssertGreaterThan(generations[1], generations[0])
        XCTAssertLessThanOrEqual(generations[1], 9_007_199_254_740_991)
    }

    func testInvalidBackendValidationFailsClosedWithoutInstall() async throws {
        let log = EventLog()
        let preview = try preview(
            document: simpleDocument(), payload: Data(),
            entryId: 7, owner: "history", generation: 1, requestId: "vm-7"
        )
        let model = HistoryPreviewViewModel(dependencies: HistoryPreviewDependencies(
            load: { _, _ in throw HistoryPreviewRemoteError(code: .unsupportedType, message: "no legacy") },
            restore: { _ in },
            loadDocument: { _, _, _, _ in preview },
            validateDocument: { _ in false },
            cancelDocument: { id, _, _, _ in log.append("cancel:\(id)") },
            materializeDocument: { preview, strings in
                try HistoryPreviewDocumentBuilder.materialize(preview, strings: strings)
            }
        ))

        model.present(documentRequest(id: 7))
        try await waitUntil {
            if case .failed = model.state { return true }
            return false
        }
        guard case .failed(let failure) = model.state else {
            return XCTFail("an invalidated document must fail closed")
        }
        XCTAssertEqual(failure.kind, .unavailable)
        XCTAssertTrue(log.contains("cancel:7"))
        XCTAssertFalse(isDocument(model))
    }

    func testMissingCapabilityFallsBackToTheLegacyTextPath() async throws {
        let model = HistoryPreviewViewModel(dependencies: HistoryPreviewDependencies(
            load: { id, _ in
                HistoryPreviewData(
                    kind: "text", name: "text.txt", sizeBytes: 4,
                    data: Data("note".utf8), entryId: id
                )
            },
            restore: { _ in },
            loadDocument: { _, _, _, _ in nil }
        ))

        model.present(documentRequest(id: 9))
        try await waitUntil {
            if case .ready = model.state { return true }
            return false
        }
        guard case .ready(let payload, _, _) = model.state else {
            return XCTFail("the legacy path should remain for older daemons")
        }
        XCTAssertEqual(String(data: payload.data, encoding: .utf8), "note")
    }

    func testCloseCancelsTheActiveDocumentRequestAndClearsState() async throws {
        let log = EventLog()
        let preview = try preview(
            document: simpleDocument(), payload: Data(),
            entryId: 7, owner: "history", generation: 1, requestId: "vm-close"
        )
        let model = HistoryPreviewViewModel(dependencies: HistoryPreviewDependencies(
            load: { _, _ in throw HistoryPreviewRemoteError(code: .unsupportedType, message: "no legacy") },
            restore: { _ in },
            loadDocument: { _, _, _, _ in preview },
            validateDocument: { _ in true },
            cancelDocument: { id, _, _, _ in log.append("cancel:\(id)") },
            materializeDocument: { preview, strings in
                try HistoryPreviewDocumentBuilder.materialize(preview, strings: strings)
            }
        ))

        model.present(documentRequest(id: 7))
        try await waitUntil { self.isDocument(model, entryId: 7) }
        model.close()
        try await waitUntil { log.contains("cancel:7") }
        XCTAssertEqual(model.state, .idle)
    }

    func testLateMissingCapabilityCannotLoadLegacyOrClearNewRequest() async throws {
        let latch = DocumentReviewLatch()
        let log = EventLog()
        let current = try preview(document: simpleDocument(), payload: Data(), entryId: 2)
        let model = HistoryPreviewViewModel(dependencies: HistoryPreviewDependencies(
            load: { id, _ in
                log.append("legacy:\(id)")
                throw CancellationError()
            },
            restore: { _ in },
            loadDocument: { id, _, _, _ in
                log.append("load:\(id)")
                if id == 1 { await latch.wait(); return nil }
                return current
            },
            validateDocument: { _ in true },
            cancelDocument: { id, _, _, _ in log.append("cancel:\(id)") },
            materializeDocument: { preview, strings in
                try HistoryPreviewDocumentBuilder.materialize(preview, strings: strings)
            }
        ))
        model.present(documentRequest(id: 1))
        try await waitUntil { log.contains("load:1") }
        model.present(documentRequest(id: 2))
        try await waitUntil { self.isDocument(model, entryId: 2) }
        await latch.open()
        try await Task.sleep(for: .milliseconds(50))
        model.close()
        try await Task.sleep(for: .milliseconds(50))
        XCTAssertFalse(log.contains("legacy:1"))
        XCTAssertTrue(log.contains("cancel:2"))
    }

    func testCancellationBurstCoalescesInsteadOfBuildingAnUnboundedTaskChain() async throws {
        let latch = DocumentReviewLatch()
        let log = EventLog()
        let current = try preview(document: simpleDocument(), payload: Data())
        let model = HistoryPreviewViewModel(dependencies: HistoryPreviewDependencies(
            load: { _, _ in throw CancellationError() }, restore: { _ in },
            loadDocument: { id, _, _, _ in log.append("load:\(id)"); return current },
            validateDocument: { _ in true },
            cancelDocument: { id, _, _, _ in
                log.append("cancel:\(id)")
                await latch.wait()
            },
            materializeDocument: { preview, strings in
                try HistoryPreviewDocumentBuilder.materialize(preview, strings: strings)
            }
        ))
        model.present(documentRequest(id: 1))
        try await waitUntil { log.contains("load:1") }
        model.present(documentRequest(id: 2))
        try await waitUntil { log.contains("cancel:1") }
        for entryId in 3...35 {
            model.present(documentRequest(id: Int64(entryId)))
            try await waitUntil { log.contains("load:\(entryId)") }
        }
        model.close()
        await latch.open()
        try await Task.sleep(for: .milliseconds(200))
        XCTAssertLessThanOrEqual(log.snapshot().filter { $0.hasPrefix("cancel:") }.count, 3)
    }

    func testCancelledMaterializerHoldsItsAdmissionUntilActualWorkerExit() async throws {
        let blocker = DispatchSemaphore(value: 0)
        let log = EventLog()
        let current = try preview(document: simpleDocument(), payload: Data())
        let model = HistoryPreviewViewModel(dependencies: HistoryPreviewDependencies(
            load: { _, _ in throw CancellationError() }, restore: { _ in },
            loadDocument: { _, _, _, _ in current }, validateDocument: { _ in true },
            materializeDocument: { preview, strings in
                log.append("worker")
                blocker.wait()
                return try HistoryPreviewDocumentBuilder.materialize(preview, strings: strings)
            }
        ))
        model.present(documentRequest(id: 1))
        try await waitUntil { log.contains("worker") }
        for entryId in 2...8 {
            model.present(documentRequest(id: Int64(entryId)))
            try await Task.sleep(for: .milliseconds(15))
        }
        model.close()
        let count = log.snapshot().filter { $0 == "worker" }.count
        for _ in 0..<count { blocker.signal() }
        try await Task.sleep(for: .milliseconds(100))
        XCTAssertEqual(count, 1)
        XCTAssertEqual(model.state, .idle)
    }

    func testRetainedQuotaIncludesModelAndDecodedPlacements() throws {
        let bytes = try png(width: 4, height: 4, red: 1, green: 0, blue: 0)
        let document: [String: Any] = [
            "version": 1, "blocks": [image(0, ordinal: 0)],
            "assets": [asset(index: 0, offset: 0, bytes: bytes, width: 4, height: 4)],
            "partial": [], "styles_partial": false, "first_image": 0,
        ]
        let current = try preview(document: document, payload: bytes)
        XCTAssertThrowsError(try HistoryPreviewDocumentBuilder.materialize(current, strings: strings(), maxRetainedBytes: bytes.count))
        let material = try HistoryPreviewDocumentBuilder.materialize(current, strings: strings())
        XCTAssertGreaterThan(material.retainedBytes, bytes.count + 4 * 4 * 4)
    }

    func testBudgetNoticeSurvivesLaterSmallImageAndUnavailablePlacement() throws {
        let large = try png(width: 2048, height: 2048, red: 1, green: 0, blue: 0)
        let small = try png(width: 2, height: 2, red: 0, green: 0, blue: 1)
        var payload = large
        payload.append(small)
        let document: [String: Any] = [
            "version": 1,
            "blocks": (0..<4).map { image(0, ordinal: $0) } + [image(1, ordinal: 4), image(nil, ordinal: 5, availability: "external_reference")],
            "assets": [asset(index: 0, offset: 0, bytes: large, width: 2048, height: 2048), asset(index: 1, offset: large.count, bytes: small, width: 2, height: 2)],
            "partial": [], "styles_partial": false, "first_image": 0,
        ]
        let material = try HistoryPreviewDocumentBuilder.materialize(try preview(document: document, payload: payload), strings: strings())
        XCTAssertEqual(material.imagePlacements, 4)
        XCTAssertEqual(material.imagePlaceholders, 2)
        XCTAssertTrue(material.notices.contains { $0.code == "pixel_budget" })
        XCTAssertTrue(material.attributed.string.contains("EXTERNAL-REFERENCE"))
        XCTAssertLessThanOrEqual(material.retainedBytes, HistoryPreviewDocumentBuilder.maximumRetainedBytes)
    }

    func testOrderedListRestartsAfterBulletGroupLikeTheWindowsRenderer() throws {
        let document: [String: Any] = [
            "version": 1,
            "blocks": [paragraph([run("first")], list: "ordered"), paragraph([run("bullet")], list: "bullet"), paragraph([run("new list")], list: "ordered")],
            "assets": [], "partial": [], "styles_partial": false,
        ]
        let material = try HistoryPreviewDocumentBuilder.materialize(try preview(document: document, payload: Data()), strings: strings())
        XCTAssertEqual(material.attributed.string, "1.\tfirst\n•\tbullet\n1.\tnew list\n")
    }

    func testHeadingQuoteCodeAndListDepthProduceReadingSemantics() throws {
        let document: [String: Any] = [
            "version": 1,
            "blocks": [
                paragraph([run("一级标题")], heading: 1),
                paragraph([run("三级标题")], heading: 3),
                paragraph([run("引用一")], quoteDepth: 1),
                paragraph([run("引用二")], quoteDepth: 2),
                paragraph([run(" 保留   空白\n第二行 ")], codeBlock: true),
                paragraph([run("顶层")], list: "ordered", listDepth: 0),
                paragraph([run("嵌套")], list: "ordered", listDepth: 1),
            ],
            "assets": [], "partial": [], "styles_partial": false,
        ]
        let material = try HistoryPreviewDocumentBuilder.materialize(try preview(document: document, payload: Data()), strings: strings())
        let attributed = material.attributed

        XCTAssertEqual(attributed.string, "一级标题\n三级标题\n引用一\n引用二\n 保留   空白\n第二行 \n1.\t顶层\n1.\t嵌套\n")

        let headingFont = font(at: index(of: "一级标题", in: attributed), in: attributed)
        XCTAssertEqual(headingFont?.pointSize ?? 0, 13 * 1.6, accuracy: 0.01)
        XCTAssertEqual(headingFont?.fontName, NSFont.systemFont(ofSize: 13 * 1.6, weight: .semibold).fontName)

        let thirdHeadingFont = font(at: index(of: "三级标题", in: attributed), in: attributed)
        XCTAssertEqual(thirdHeadingFont?.pointSize ?? 0, 13 * 1.18, accuracy: 0.01)

        let quoteIndex = index(of: "引用一", in: attributed)
        let quoteStyle = attributed.attribute(.paragraphStyle, at: quoteIndex, effectiveRange: nil) as? NSParagraphStyle
        XCTAssertEqual(quoteStyle?.headIndent ?? 0, 16)
        XCTAssertEqual(quoteStyle?.firstLineHeadIndent ?? 0, 16)
        XCTAssertEqual(
            srgbComponents(attributed.attribute(.backgroundColor, at: quoteIndex, effectiveRange: nil) as? NSColor),
            srgbComponents(NSColor.windowBackgroundColor)
        )

        let deepQuoteStyle = attributed.attribute(.paragraphStyle, at: index(of: "引用二", in: attributed), effectiveRange: nil) as? NSParagraphStyle
        XCTAssertEqual(deepQuoteStyle?.headIndent ?? 0, 32)

        let codeIndex = index(of: "保留", in: attributed)
        XCTAssertEqual(font(at: codeIndex, in: attributed)?.fontName, NSFont.monospacedSystemFont(ofSize: 13, weight: .regular).fontName)
        XCTAssertEqual(
            srgbComponents(attributed.attribute(.backgroundColor, at: codeIndex, effectiveRange: nil) as? NSColor),
            srgbComponents(NSColor.windowBackgroundColor)
        )
        XCTAssertTrue(attributed.string.contains(" 保留   空白\n第二行 "))

        let nestedIndex = index(of: "嵌套", in: attributed)
        let nestedStyle = attributed.attribute(.paragraphStyle, at: nestedIndex, effectiveRange: nil) as? NSParagraphStyle
        XCTAssertEqual(nestedStyle?.headIndent ?? 0, 16)
        XCTAssertNil(attributed.attribute(.paragraphStyle, at: index(of: "顶层", in: attributed), effectiveRange: nil))
    }

    func testSafeLinkIsAttachedAndOpensOnlyThroughTheInjectedOpener() throws {
        let document: [String: Any] = [
            "version": 1,
            "blocks": [
                paragraph([run("安全", link: "https://example.com/docs")]),
                paragraph([run("纯文本")]),
            ],
            "assets": [], "partial": [], "styles_partial": false,
        ]
        let material = try HistoryPreviewDocumentBuilder.materialize(try preview(document: document, payload: Data()), strings: strings())
        let attributed = material.attributed
        let safeIndex = index(of: "安全", in: attributed)
        XCTAssertEqual((attributed.attribute(.link, at: safeIndex, effectiveRange: nil) as? URL)?.absoluteString, "https://example.com/docs")
        XCTAssertEqual(attributed.attribute(.toolTip, at: safeIndex, effectiveRange: nil) as? String, "https://example.com/docs")
        XCTAssertNil(attributed.attribute(.link, at: index(of: "纯文本", in: attributed), effectiveRange: nil))

        let textView = HistoryPreviewDocumentTextFactory.makeTextView()
        XCTAssertFalse(textView.isAutomaticLinkDetectionEnabled)

        let materialOpener = RecordingDocumentLinkOpener()
        let representable = HistoryPreviewDocumentTextView(material: material, opener: materialOpener)
        XCTAssertTrue(materialOpener.opened.isEmpty)
        let coordinator = representable.makeCoordinator()
        XCTAssertTrue(materialOpener.opened.isEmpty)
        XCTAssertTrue(coordinator.textView(textView, clickedOnLink: "https://example.com/docs", at: safeIndex))
        XCTAssertEqual(materialOpener.opened.map(\.absoluteString), ["https://example.com/docs"])

        let delegateOpener = RecordingDocumentLinkOpener()
        let delegate = HistoryPreviewDocumentLinkDelegate(opener: delegateOpener)
        XCTAssertTrue(delegate.textView(textView, clickedOnLink: "javascript:alert(1)", at: 0))
        XCTAssertTrue(delegate.textView(textView, clickedOnLink: URL(string: "file:///private/secret")!, at: 0))
        XCTAssertTrue(delegate.textView(textView, clickedOnLink: "https://user:pass@example.com/", at: 0))
        XCTAssertTrue(delegate.textView(textView, clickedOnLink: "https://@example.com/", at: 0))
        XCTAssertTrue(delegate.textView(textView, clickedOnLink: "/relative/path", at: 0))
        XCTAssertTrue(delegateOpener.opened.isEmpty)
    }

    func testDocumentLinkDelegateConsumesAnOpenerFailureWithoutDefaultRetry() {
        let opener = RecordingDocumentLinkOpener()
        opener.result = false
        let delegate = HistoryPreviewDocumentLinkDelegate(opener: opener)
        let textView = HistoryPreviewDocumentTextFactory.makeTextView()
        XCTAssertTrue(delegate.textView(textView, clickedOnLink: "https://example.com/docs", at: 0))
        XCTAssertEqual(opener.opened.map(\.absoluteString), ["https://example.com/docs"])
    }

    func testNestedOrderedListResumesItsParentCounter() throws {
        let document: [String: Any] = [
            "version": 1,
            "blocks": [
                paragraph([run("parent")], list: "ordered", listDepth: 0),
                paragraph([run("child")], list: "ordered", listDepth: 1),
                paragraph([run("bullet")], list: "bullet", listDepth: 2),
                paragraph([run("child two")], list: "ordered", listDepth: 1),
                paragraph([run("parent two")], list: "ordered", listDepth: 0),
                paragraph([run("new child")], list: "ordered", listDepth: 1),
                paragraph([run("parent three")], list: "ordered", listDepth: 0),
                paragraph([run("plain")]),
                paragraph([run("new list")], list: "ordered", listDepth: 0),
            ],
            "assets": [], "partial": [], "styles_partial": false,
        ]
        let material = try HistoryPreviewDocumentBuilder.materialize(try preview(document: document, payload: Data()), strings: strings())
        XCTAssertEqual(material.attributed.string, "1.\tparent\n1.\tchild\n•\tbullet\n2.\tchild two\n2.\tparent two\n1.\tnew child\n3.\tparent three\nplain\n1.\tnew list\n")
    }

    func testOrdinaryExplicitColoursRemainReadableAcrossAppearanceChanges() throws {
        let document: [String: Any] = [
            "version": 1,
            "blocks": [
                paragraph([run("dark", ["color": ["red": 0, "green": 0, "blue": 0]])]),
                paragraph([run("light", ["color": ["red": 255, "green": 255, "blue": 255]])]),
            ],
            "assets": [], "partial": [], "styles_partial": false,
        ]
        let source = try preview(document: document, payload: Data())
        let material = try HistoryPreviewDocumentBuilder.materialize(source, strings: strings())
        let darkColor = try XCTUnwrap(material.attributed.attribute(.foregroundColor, at: index(of: "dark", in: material.attributed), effectiveRange: nil) as? NSColor)
        let lightColor = try XCTUnwrap(material.attributed.attribute(.foregroundColor, at: index(of: "light", in: material.attributed), effectiveRange: nil) as? NSColor)
        try XCTUnwrap(NSAppearance(named: .darkAqua)).performAsCurrentDrawingAppearance {
            XCTAssertEqual(srgbComponents(darkColor), [1, 1, 1])
            XCTAssertEqual(srgbComponents(lightColor), [1, 1, 1])
        }
        try XCTUnwrap(NSAppearance(named: .aqua)).performAsCurrentDrawingAppearance {
            XCTAssertEqual(srgbComponents(darkColor), [0, 0, 0])
            XCTAssertEqual(srgbComponents(lightColor), [0, 0, 0])
        }
        XCTAssertEqual(source.document.blocks[0].paragraph?.runs[0].style.color?.red, 0)
        XCTAssertEqual(source.document.blocks[1].paragraph?.runs[0].style.color?.red, 255)
    }

    func testUnreadableExplicitHighlightColoursChangeOnlyThePreview() throws {
        let document: [String: Any] = [
            "version": 1,
            "blocks": [
                paragraph([run("暗色", ["color": ["red": 0, "green": 0, "blue": 0], "highlight": ["red": 0, "green": 0, "blue": 0]])]),
                paragraph([run("亮色", ["color": ["red": 255, "green": 255, "blue": 255], "highlight": ["red": 255, "green": 255, "blue": 255]])]),
            ],
            "assets": [], "partial": [], "styles_partial": false,
        ]
        let source = try preview(document: document, payload: Data())
        let material = try HistoryPreviewDocumentBuilder.materialize(source, strings: strings())
        XCTAssertEqual(srgbComponents(material.attributed.attribute(.foregroundColor, at: index(of: "暗色", in: material.attributed), effectiveRange: nil) as? NSColor), [1, 1, 1])
        XCTAssertEqual(srgbComponents(material.attributed.attribute(.foregroundColor, at: index(of: "亮色", in: material.attributed), effectiveRange: nil) as? NSColor), [0, 0, 0])
        XCTAssertEqual(source.document.blocks[0].paragraph?.runs[0].style.color?.red, 0)
        XCTAssertEqual(source.document.blocks[1].paragraph?.runs[0].style.color?.red, 255)
    }

    func testSafeDocumentLinkPolicyRejectsUnsafeTargets() {
        XCTAssertEqual(HistoryPreviewDocumentLink.safeURL("https://example.com/docs")?.absoluteString, "https://example.com/docs")
        XCTAssertEqual(HistoryPreviewDocumentLink.safeURL("http://example.com/")?.absoluteString, "http://example.com/")
        XCTAssertEqual(HistoryPreviewDocumentLink.safeURL("HTTPS://example.com/")?.absoluteString, "https://example.com/")
        for rejected in [
            "", "   ", "https://example.com/a b", "https://example.com/a\tb", "https://example.com/a\nb",
            "https://example.com/a\rb", "https://example.com/a\u{0}b", "https://example.com/a\u{85}b",
            "https://example.com/a\u{3000}b", "https://example.com/a\\b", "https://user@example.com/",
            "https://user:pass@example.com/", "https://@example.com/", "ftp://example.com/",
            "file:///tmp/x", "javascript:void(0)", "/relative", "example.com", "https://",
            "https://example.com:notaport/",
        ] {
            XCTAssertNil(HistoryPreviewDocumentLink.safeURL(rejected), rejected)
        }
        XCTAssertNil(HistoryPreviewDocumentLink.safeURL("https://example.com/" + String(repeating: "a", count: 2048)))
        XCTAssertNotNil(HistoryPreviewDocumentLink.safeURL("https://example.com/" + String(repeating: "a", count: 2020)))
    }

    func testHighlightUsesReadableContrastAndKeepsExplicitColours() throws {
        let document: [String: Any] = [
            "version": 1,
            "blocks": [
                paragraph([run("亮底", ["highlight": ["red": 255, "green": 255, "blue": 255]])]),
                paragraph([run("暗底", ["highlight": ["red": 0, "green": 0, "blue": 0]])]),
                paragraph([run("原色", ["color": ["red": 255, "green": 0, "blue": 0], "highlight": ["red": 0, "green": 0, "blue": 0]])]),
            ],
            "assets": [], "partial": [], "styles_partial": false,
        ]
        let material = try HistoryPreviewDocumentBuilder.materialize(try preview(document: document, payload: Data()), strings: strings())
        let attributed = material.attributed

        let lightIndex = index(of: "亮底", in: attributed)
        XCTAssertEqual(srgbComponents(attributed.attribute(.foregroundColor, at: lightIndex, effectiveRange: nil) as? NSColor), [0, 0, 0])
        XCTAssertEqual(srgbComponents(attributed.attribute(.backgroundColor, at: lightIndex, effectiveRange: nil) as? NSColor), [1, 1, 1])

        let darkIndex = index(of: "暗底", in: attributed)
        XCTAssertEqual(srgbComponents(attributed.attribute(.foregroundColor, at: darkIndex, effectiveRange: nil) as? NSColor), [1, 1, 1])
        XCTAssertEqual(srgbComponents(attributed.attribute(.backgroundColor, at: darkIndex, effectiveRange: nil) as? NSColor), [0, 0, 0])

        let explicitIndex = index(of: "原色", in: attributed)
        XCTAssertEqual(srgbComponents(attributed.attribute(.foregroundColor, at: explicitIndex, effectiveRange: nil) as? NSColor), [1, 0, 0])
        XCTAssertEqual(srgbComponents(attributed.attribute(.backgroundColor, at: explicitIndex, effectiveRange: nil) as? NSColor), [0, 0, 0])
    }

    func testMissingReadingFieldsKeepTheLegacyLayout() throws {
        let document: [String: Any] = [
            "version": 1,
            "blocks": [paragraph([run("旧正文")]), paragraph([run("旧项目")], list: "bullet")],
            "assets": [], "partial": [], "styles_partial": false,
        ]
        let material = try HistoryPreviewDocumentBuilder.materialize(try preview(document: document, payload: Data()), strings: strings())
        let attributed = material.attributed

        XCTAssertEqual(attributed.string, "旧正文\n•\t旧项目\n")
        let bodyFont = font(at: index(of: "旧正文", in: attributed), in: attributed)
        XCTAssertEqual(bodyFont?.pointSize ?? 0, 13, accuracy: 0.01)
        XCTAssertEqual(bodyFont?.fontName, NSFont.systemFont(ofSize: 13, weight: .regular).fontName)
        XCTAssertNil(attributed.attribute(.paragraphStyle, at: index(of: "旧正文", in: attributed), effectiveRange: nil))
        XCTAssertNil(attributed.attribute(.backgroundColor, at: index(of: "旧正文", in: attributed), effectiveRange: nil))
        XCTAssertNil(attributed.attribute(.link, at: index(of: "旧正文", in: attributed), effectiveRange: nil))
        XCTAssertNil(attributed.attribute(.paragraphStyle, at: index(of: "旧项目", in: attributed), effectiveRange: nil))
    }

    func testCodeQuoteAndLinkRunsPreserveTheOriginalText() throws {
        let source = "第一行\n  缩进 与   多空格\n第三行"
        let document: [String: Any] = [
            "version": 1,
            "blocks": [
                paragraph([run(source)], codeBlock: true),
                paragraph([run("引用原文")], quoteDepth: 1),
                paragraph([run("链接文字", link: "https://example.com/")]),
            ],
            "assets": [], "partial": [], "styles_partial": false,
        ]
        let material = try HistoryPreviewDocumentBuilder.materialize(try preview(document: document, payload: Data()), strings: strings())
        XCTAssertTrue(material.attributed.string.contains(source))
        XCTAssertTrue(material.attributed.string.contains("引用原文"))
        XCTAssertTrue(material.attributed.string.contains("链接文字"))
    }

    func testHeadingLevelsClampIntoTheThreeReadingLevels() {
        XCTAssertEqual(HistoryPreviewDocumentBuilder.normalizedHeading(1), 1)
        XCTAssertEqual(HistoryPreviewDocumentBuilder.normalizedHeading(2), 2)
        XCTAssertEqual(HistoryPreviewDocumentBuilder.normalizedHeading(3), 3)
        XCTAssertEqual(HistoryPreviewDocumentBuilder.normalizedHeading(6), 3)
        XCTAssertNil(HistoryPreviewDocumentBuilder.normalizedHeading(0))
        XCTAssertNil(HistoryPreviewDocumentBuilder.normalizedHeading(nil))
    }
}
