import AppKit
import Foundation
import SwiftUI
import XCTest
@testable import TailSync

final class HistoryPreviewModelTests: XCTestCase {
    func testTextFontPreferenceDefaultsToReadableSizeAndClampsValues() {
        XCTAssertEqual(HistoryPreviewPreferences.defaultTextFontSize, 18)
        XCTAssertEqual(HistoryPreviewPreferences.clampedTextFontSize(.nan), 18)
        XCTAssertEqual(HistoryPreviewPreferences.clampedTextFontSize(4), 12)
        XCTAssertEqual(HistoryPreviewPreferences.clampedTextFontSize(99), 32)
        XCTAssertEqual(HistoryPreviewPreferences.clampedTextFontSize(19.6), 20)
    }

    func testCodeStylerProducesDistinctSemanticColours() {
        let source = "let answer = 42 // important"
        let styled = HistoryPreviewTextStyler.attributedString(
            source,
            isCode: true,
            fontSize: 18,
            query: ""
        )
        let plain = HistoryPreviewTextStyler.attributedString(
            source,
            isCode: false,
            fontSize: 18,
            query: ""
        )
        let keywordIndex = (source as NSString).range(of: "let").location
        let commentIndex = (source as NSString).range(of: "//").location
        let codeKeyword = styled.attribute(.foregroundColor, at: keywordIndex, effectiveRange: nil) as? NSColor
        let codeComment = styled.attribute(.foregroundColor, at: commentIndex, effectiveRange: nil) as? NSColor
        let plainKeyword = plain.attribute(.foregroundColor, at: keywordIndex, effectiveRange: nil) as? NSColor

        XCTAssertNotEqual(codeKeyword, plainKeyword)
        XCTAssertNotEqual(codeComment, plainKeyword)
        XCTAssertNotEqual(codeKeyword, codeComment)
    }

    func testFormatDetectionSelectsDedicatedRenderers() {
        let cases: [(String, String, String?, HistoryPreviewFormat)] = [
            ("file", "README.md", nil, .markdown),
            ("file", "manual.pdf", nil, .pdf),
            ("file", "report.docx", nil, .docx),
            ("file", "photo.webp", nil, .image),
            ("file", "main.swift", nil, .code),
            ("text", "text.txt", "code", .code),
            ("text", "text.txt", nil, .text),
            ("file", "archive.zip", nil, .unsupported)
        ]

        for (kind, name, category, expected) in cases {
            let payload = HistoryPreviewData(
                kind: kind,
                name: name,
                sizeBytes: 0,
                data: Data()
            )
            XCTAssertEqual(
                HistoryPreviewFormat.detect(payload: payload, categoryHint: category),
                expected,
                name
            )
        }
    }

    func testMarkdownRemovesResourcesAndUnsafeLinkSchemes() {
        let source = """
        # Article
        ![remote image](https://example.com/image.png)
        <script>alert('x')</script>
        [safe](https://example.com/read)
        [unsafe](javascript:alert(1))
        [local](file:///Users/test/secret)
        [custom](tailsync://open/item)
        """

        let sanitized = HistoryMarkdownRenderer.sanitizedSource(source)
        XCTAssertTrue(sanitized.contains("remote image"))
        XCTAssertFalse(sanitized.contains("image.png"))
        XCTAssertFalse(sanitized.localizedCaseInsensitiveContains("script"))

        let article = HistoryMarkdownRenderer.attributedArticle(source)
        let links = article.runs.compactMap(\.link).map(\.absoluteString)
        XCTAssertEqual(links, ["https://example.com/read"])
        XCTAssertTrue(HistoryMarkdownRenderer.isAllowedLink(
            URL(string: "HTTP://example.com")!
        ))
        XCTAssertFalse(HistoryMarkdownRenderer.isAllowedLink(
            URL(string: "mailto:test@example.com")!
        ))
    }

    func testLogicalLineIndexUsesTextKitUtf16Offsets() {
        let index = HistoryPreviewLogicalLineIndex(text: "first\n😀 second\nthird")

        XCTAssertEqual(index.lineStartOffsets, [0, 6, 16])
        XCTAssertEqual(index.lineNumber(containing: 0), 1)
        XCTAssertEqual(index.lineNumber(containing: 5), 1)
        XCTAssertEqual(index.lineNumber(containing: 6), 2)
        XCTAssertEqual(index.lineNumber(containing: 15), 2)
        XCTAssertEqual(index.lineNumber(containing: 16), 3)
    }

    func testImageViewportFitsRotationAndAccumulatesDrags() {
        XCTAssertEqual(
            HistoryImageViewport.fittedScale(
                imageSize: CGSize(width: 1_000, height: 500),
                containerSize: CGSize(width: 500, height: 500),
                rotation: .zero
            ),
            0.5,
            accuracy: 0.0001
        )
        XCTAssertEqual(
            HistoryImageViewport.fittedScale(
                imageSize: CGSize(width: 1_000, height: 500),
                containerSize: CGSize(width: 500, height: 500),
                rotation: .degrees(90)
            ),
            0.5,
            accuracy: 0.0001
        )
        XCTAssertEqual(
            HistoryImageViewport.adding(
                CGSize(width: -3, height: 8),
                to: CGSize(width: 12, height: 4)
            ),
            CGSize(width: 9, height: 12)
        )
    }

    func testWindowSizePolicyMatchesRendererNeeds() {
        XCTAssertGreaterThan(
            HistoryPreviewWindowKind.document.defaultContentSize.width,
            HistoryPreviewWindowKind.text.defaultContentSize.width
        )
        for kind in HistoryPreviewWindowKind.allCases {
            XCTAssertGreaterThanOrEqual(
                kind.defaultContentSize.width,
                kind.minimumContentSize.width
            )
            XCTAssertGreaterThanOrEqual(
                kind.defaultContentSize.height,
                kind.minimumContentSize.height
            )
        }
    }

    func testTypedRemoteErrorsDriveFailureClassification() {
        let cases: [(HistoryPreviewRemoteErrorCode, HistoryPreviewFailure)] = [
            (.previewTooLarge, HistoryPreviewFailure(kind: .tooLarge, canRetry: false)),
            (.unsupportedType, HistoryPreviewFailure(kind: .unsupported, canRetry: false)),
            (.invalidSize, HistoryPreviewFailure(kind: .corrupt, canRetry: false)),
            (.payloadUnavailable, HistoryPreviewFailure(kind: .decryption, canRetry: true)),
            (.entryNotFound, HistoryPreviewFailure(kind: .unavailable, canRetry: false)),
            (.metadataUnavailable, HistoryPreviewFailure(kind: .unavailable, canRetry: true))
        ]

        for (code, expected) in cases {
            let error = HistoryPreviewRemoteError(code: code, message: "ignored wording")
            XCTAssertEqual(HistoryPreviewFailure.classify(error), expected, code.rawValue)
        }
    }
}
