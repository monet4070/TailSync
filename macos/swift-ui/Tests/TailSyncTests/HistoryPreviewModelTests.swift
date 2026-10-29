import AppKit
import Foundation
import PDFKit
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
        XCTAssertEqual(
            HistoryPreviewPreferences.textFontSize(afterModifierScroll: 2, current: 18),
            19
        )
        XCTAssertEqual(
            HistoryPreviewPreferences.textFontSize(afterModifierScroll: -2, current: 18),
            17
        )
    }

    func testModifierScrollPolicyConsumesOnlyLocalCommandOrControlWheelEvents() {
        XCTAssertEqual(
            HistoryPreviewModifierScrollPolicy.zoomDelta(
                scrollingDeltaY: 4,
                modifiers: [.control],
                isInsidePreview: true
            ),
            4
        )
        XCTAssertEqual(
            HistoryPreviewModifierScrollPolicy.zoomDelta(
                scrollingDeltaY: -3,
                modifiers: [.command, .shift],
                isInsidePreview: true
            ),
            -3
        )
        XCTAssertNil(HistoryPreviewModifierScrollPolicy.zoomDelta(
            scrollingDeltaY: 4,
            modifiers: [],
            isInsidePreview: true
        ))
        XCTAssertNil(HistoryPreviewModifierScrollPolicy.zoomDelta(
            scrollingDeltaY: 4,
            modifiers: [.control],
            isInsidePreview: false
        ))
    }

    @MainActor
    func testInteractivePDFAndSearchControlsKeepTheirSpaceKey() {
        let editable = NSTextView()
        editable.isEditable = true
        let readOnly = NSTextView()
        readOnly.isEditable = false

        XCTAssertTrue(HistoryPreviewWindowKeyPolicy.keepsSpaceKey(editable))
        XCTAssertFalse(HistoryPreviewWindowKeyPolicy.keepsSpaceKey(readOnly))
        XCTAssertTrue(HistoryPreviewWindowKeyPolicy.keepsSpaceKey(PDFView()))
        XCTAssertFalse(HistoryPreviewWindowKeyPolicy.keepsSpaceKey(nil))
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
            ("file", "slides.ppt", nil, .presentation),
            ("file", "slides.pptx", nil, .presentation),
            ("file", "photo.webp", nil, .image),
            ("file", "vector.svg", nil, .svg),
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

    func testPresentationFilesUseTheDocumentPreviewWindow() {
        for name in ["slides.ppt", "slides.pptx", "SLIDES.PPTX"] {
            let payload = HistoryPreviewData(
                kind: "file",
                name: name,
                sizeBytes: 0,
                data: Data()
            )

            XCTAssertEqual(
                HistoryPreviewFormat.detect(payload: payload).windowKind,
                .document,
                "\(name) should be routed to the native document preview"
            )
        }
    }

    @MainActor
    func testMarkdownOfflineResourcesAndNavigationPolicy() {
        XCTAssertFalse(HistoryMarkdownResources.engine.isEmpty)
        XCTAssertTrue(HistoryMarkdownResources.styles.contains("data:font/woff2;base64,"))
        XCTAssertTrue(HistoryMarkdownResources.shell.contains("default-src 'none'"))
        XCTAssertTrue(HistoryMarkdownResources.shell.contains("script-src 'none'"))
        XCTAssertTrue(HistoryMarkdownWebRenderer.isAllowedLink(URL(string: "https://example.com")!))
        XCTAssertFalse(HistoryMarkdownWebRenderer.isAllowedLink(URL(string: "file:///secret")!))
        XCTAssertFalse(HistoryMarkdownWebRenderer.isAllowedLink(URL(string: "javascript:alert(1)")!))
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
        let initialLayout = HistoryImageViewport.layout(
            imageSize: CGSize(width: 300, height: 1_200),
            containerSize: CGSize(width: 640, height: 394),
            rotation: .zero
        )
        XCTAssertEqual(initialLayout.fittedScale, 394 / 1_200, accuracy: 0.0001)
        XCTAssertEqual(initialLayout.imageSize.width, 98.5, accuracy: 0.0001)
        XCTAssertEqual(initialLayout.imageSize.height, 394, accuracy: 0.0001)
        XCTAssertEqual(initialLayout.center, CGPoint(x: 320, y: 197))
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
        XCTAssertTrue(HistoryPreviewImageLimits.accepts(
            frameDimensions: [(width: 4_096, height: 4_096)]
        ))
        XCTAssertFalse(HistoryPreviewImageLimits.accepts(
            frameDimensions: [(width: 10_000, height: 10_000)]
        ))
        XCTAssertFalse(HistoryPreviewImageLimits.accepts(
            frameDimensions: Array(repeating: (width: 1_024, height: 1_024), count: 65)
        ))
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
            (.payloadUnavailable, HistoryPreviewFailure(kind: .unavailable, canRetry: true)),
            (.entryNotFound, HistoryPreviewFailure(kind: .unavailable, canRetry: false)),
            (.metadataUnavailable, HistoryPreviewFailure(kind: .unavailable, canRetry: true))
        ]

        for (code, expected) in cases {
            let error = HistoryPreviewRemoteError(code: code, message: "ignored wording")
            XCTAssertEqual(HistoryPreviewFailure.classify(error), expected, code.rawValue)
        }

        XCTAssertEqual(
            HistoryPreviewFailure.classify(HistoryPreviewStoreError.invalidDocument),
            HistoryPreviewFailure(kind: .corrupt, canRetry: false)
        )
    }

    func testLegacyTextPreviewErrorsUseSafeFailureClassification() {
        let cases: [(String, HistoryPreviewFailure)] = [
            ("preview is too large", HistoryPreviewFailure(kind: .tooLarge, canRetry: false)),
            ("unsupported preview type", HistoryPreviewFailure(kind: .unsupported, canRetry: false)),
            ("decryption failed", HistoryPreviewFailure(kind: .decryption, canRetry: true)),
            ("corrupt preview data", HistoryPreviewFailure(kind: .corrupt, canRetry: true)),
            ("unknown private path /tmp/secret", HistoryPreviewFailure(kind: .unavailable, canRetry: true))
        ]
        for (message, expected) in cases {
            let error = HistoryPreviewRemoteError(code: nil, message: message)
            XCTAssertEqual(HistoryPreviewFailure.classify(error), expected, message)
        }
    }
}
