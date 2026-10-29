import AppKit
import Foundation
import XCTest
@testable import TailSync

/// Consumes frames produced from the native capture and reopened SQLite matrix,
/// rather than constructing preview payloads independently of history.
final class HistoryFormatRoundtripTests: XCTestCase {
    private struct Row: Decodable {
        let name: String
        let frame: String
        let expected_format: String
        let category: String
        let display_kind: String?
    }

    @MainActor
    func testNativeHistoryFramesReachTheirActualRenderers() async throws {
        guard let directory = ProcessInfo.processInfo.environment["TAILSYNC_FORMAT_MATRIX_DIR"] else {
            throw XCTSkip("Opt-in acceptance: run the Rust native format matrix first")
        }
        let root = URL(fileURLWithPath: directory, isDirectory: true)
        let decoder = JSONDecoder()
        let native = try decoder.decode([Row].self, from: Data(contentsOf: root.appendingPathComponent("native-results.json")))
        let files = try decoder.decode([Row].self, from: Data(contentsOf: root.appendingPathComponent("file-results.json")))
        XCTAssertEqual(native.count, 24)
        let coveredExtensions = Set(files.map { HistoryPreviewFileTypes.fileExtension(for: $0.name) })
        XCTAssertTrue(HistoryPreviewFileTypes.textExtensions.isSubset(of: coveredExtensions))
        XCTAssertTrue(HistoryPreviewFileTypes.imageExtensions.isSubset(of: coveredExtensions))
        XCTAssertTrue(Set(["svg", "pdf", "docx", "ppt", "pptx", "xlsx", "zip"]).isSubset(of: coveredExtensions))
        let temp = FileManager.default.temporaryDirectory.appendingPathComponent("tailsync-format-render-\(UUID().uuidString)")
        let store = HistoryPreviewStore(directory: temp)
        addTeardownBlock { try store.cleanupStaleFiles() }
        var results: [[String: Any]] = []
        for row in native + files {
            let frame = try Data(contentsOf: root.appendingPathComponent(row.frame))
            if frame.starts(with: Data("TSDC".utf8)) {
                let preview = try ApiClient.decodeDocumentPreview(frame, expectedRequestID: row.name)
                let material = try HistoryPreviewDocumentBuilder.materialize(preview, strings: .live())
                XCTAssertEqual(row.expected_format, "document", row.name)
                XCTAssertGreaterThan(material.attributed.length, 0, row.name)
                if row.name == "html-two-images" { XCTAssertEqual(material.imagePlacements, 2) }
                if row.name == "rtfd-attachment" { XCTAssertEqual(material.imagePlacements, 1) }
                if row.name == "html-missing-image-with-text" {
                    XCTAssertEqual(material.imagePlaceholders, 1)
                    XCTAssertTrue(material.attributed.string.contains("saved words"))
                }
                results.append(["name": row.name, "renderer": "document", "images": material.imagePlacements,
                                "image_placeholders": material.imagePlaceholders, "characters": material.attributed.length])
                continue
            }
            let preview = try ApiClient.decodeBinaryPreview(frame, expectedRequestID: row.name)
            let format = HistoryPreviewFormat.detect(payload: preview, categoryHint: row.category, displayKind: row.display_kind)
            XCTAssertEqual(format.rawValue, row.expected_format, row.name)
            let material = try store.materialize(preview)
            switch material {
            case .text(let value):
                XCTAssertFalse(value.isEmpty, row.name)
                if format == .markdown {
                    let document = HistoryMarkdownRenderer.document(value)
                    XCTAssertFalse(document.blocks.isEmpty, row.name)
                } else if format == .svg {
                    let renderer = HistoryPreviewWebViewSVGRenderer()
                    let png = try await renderer.renderPNG(fromSVG: value, trustingExternalResources: false)
                    XCTAssertNotNil(NSImage(data: png), row.name)
                    renderer.cancel()
                }
            case .image(let image):
                XCTAssertGreaterThan(image.image.size.width, 0, row.name)
                XCTAssertGreaterThan(image.image.size.height, 0, row.name)
                XCTAssertNotNil(image.image.cgImage(forProposedRect: nil, context: nil, hints: nil), row.name)
            case .pdf(let pdf):
                XCTAssertEqual(pdf.document.pageCount, 2, row.name)
                XCTAssertTrue(pdf.document.string?.contains("Page two") == true, row.name)
            case .quickLook(let url):
                XCTAssertEqual(try Data(contentsOf: url), preview.data, row.name)
            case .unsupported:
                XCTAssertEqual(format, .unsupported, row.name)
            }
            results.append(["name": row.name, "renderer": format.rawValue, "bytes": preview.data.count])
        }
        try JSONSerialization.data(withJSONObject: results, options: [.prettyPrinted, .sortedKeys])
            .write(to: root.appendingPathComponent("swift-render-results.json"))
    }
}
