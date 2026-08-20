import AppKit
import Foundation
import PDFKit
import XCTest
@testable import TailSync

final class HistoryPreviewStoreTests: XCTestCase {
    private func temporaryDirectory() throws -> URL {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("tailsync-preview-test-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }

    private func docxFixture() -> Data {
        Data([0x50, 0x4B, 0x03, 0x04])
            + Data("[Content_Types].xml word/document.xml".utf8)
            + Data([0x50, 0x4B, 0x05, 0x06])
    }

    private func pptxFixture() -> Data {
        Data([0x50, 0x4B, 0x03, 0x04])
            + Data("[Content_Types].xml ppt/presentation.xml".utf8)
            + Data([0x50, 0x4B, 0x05, 0x06])
    }

    private func legacyPptFixture() -> Data {
        Data([0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1])
            + Data(repeating: 0, count: 32)
    }

    func testSanitizedNameCannotEscapeAsPathOrControlCharacter() {
        let unsafe = "../absolute/path\u{00}\u{1F}\\secret.txt"
        let sanitized = HistoryPreviewStore.sanitizedFileName(unsafe)

        XCTAssertFalse(sanitized.contains("/"))
        XCTAssertFalse(sanitized.contains("\\"))
        XCTAssertFalse(sanitized == "." || sanitized == "..")
        XCTAssertFalse(sanitized.unicodeScalars.contains {
            CharacterSet.controlCharacters.contains($0)
        })
        XCTAssertFalse(sanitized.hasPrefix("."))
    }

    func testSanitizedNameFitsFilesystemBudgetAndPreservesExtension() throws {
        let directory = try temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        let store = HistoryPreviewStore(directory: directory)
        let cases = [
            String(repeating: "a", count: 400) + ".pdf",
            String(repeating: "😀", count: 160) + ".docx",
        ]

        for name in cases {
            let sanitized = HistoryPreviewStore.sanitizedFileName(name)
            XCTAssertLessThanOrEqual(
                sanitized.decomposedStringWithCanonicalMapping.utf8.count,
                HistoryPreviewStore.maximumSanitizedNameBytes
            )
            XCTAssertTrue(sanitized.hasSuffix(name.hasSuffix(".pdf") ? ".pdf" : ".docx"))

            let url = try store.write(Data("preview".utf8), named: name)
            XCTAssertLessThanOrEqual(
                url.lastPathComponent.decomposedStringWithCanonicalMapping.utf8.count,
                255
            )
            try store.remove(url)
        }
    }

    func testWriteUsesPrivatePermissionsAndCloseRemovesOnlyPreviewFile() throws {
        let directory = try temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        let store = HistoryPreviewStore(directory: directory)

        let url = try store.write(Data("secret".utf8), named: "../../password.txt")
        XCTAssertTrue(url.path.hasPrefix(directory.standardizedFileURL.path + "/"))
        XCTAssertEqual(
            (try FileManager.default.attributesOfItem(atPath: directory.path)[.posixPermissions] as? NSNumber)?.intValue,
            0o700
        )
        XCTAssertEqual(
            (try FileManager.default.attributesOfItem(atPath: url.path)[.posixPermissions] as? NSNumber)?.intValue,
            0o600
        )

        try store.remove(url)
        XCTAssertFalse(FileManager.default.fileExists(atPath: url.path))
    }

    func testStartupCleanupRemovesHiddenAndVisibleStaleFiles() throws {
        let directory = try temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        let store = HistoryPreviewStore(directory: directory)
        _ = try store.write(Data("one".utf8), named: "one.txt")
        _ = try store.write(Data("two".utf8), named: "two.txt")
        let hidden = directory.appendingPathComponent(".stale")
        XCTAssertTrue(FileManager.default.createFile(atPath: hidden.path, contents: Data("old".utf8)))

        XCTAssertEqual(try store.cleanupStaleFiles(), 3)
        XCTAssertEqual(
            try FileManager.default.contentsOfDirectory(at: directory, includingPropertiesForKeys: nil).count,
            0
        )
    }

    func testTextMaterializationStaysInMemory() throws {
        let directory = try temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        let store = HistoryPreviewStore(directory: directory)
        let preview = HistoryPreviewData(
            kind: "text",
            name: "text.txt",
            sizeBytes: 5,
            data: Data("hello".utf8)
        )

        guard case .text(let text) = try store.materialize(preview) else {
            return XCTFail("text previews must not be materialized to disk")
        }
        XCTAssertEqual(text, "hello")
        XCTAssertEqual(
            try FileManager.default.contentsOfDirectory(
                at: directory,
                includingPropertiesForKeys: nil
            ).count,
            0
        )
    }

    func testSvgAndTextFilePathsStayAsEscapedSourceInMemory() throws {
        let directory = try temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        let store = HistoryPreviewStore(directory: directory)
        let svgSource = #"<svg onload="alert(1)"><script>alert(2)</script></svg>"#
        let cases = [
            ("../../unsafe/vector.svg", svgSource),
            (#"C:\Users\name\notes.TXT"#, "plain text"),
            ("folder/README.markdown", "# Heading"),
            ("folder/notes.md", "**source**")
        ]

        for (name, source) in cases {
            let bytes = Data(source.utf8)
            let preview = HistoryPreviewData(
                kind: "file",
                name: name,
                sizeBytes: Int64(bytes.count),
                data: bytes
            )
            guard case .text(let value) = try store.materialize(preview) else {
                return XCTFail("\(name) must stay in the in-memory source viewer")
            }
            XCTAssertEqual(value, source)
        }

        XCTAssertEqual(
            try FileManager.default.contentsOfDirectory(
                at: directory,
                includingPropertiesForKeys: nil
            ).count,
            0
        )
    }

    func testSessionCloseRemovesDocxQuickLookFileButKeepsTextInMemory() throws {
        let directory = try temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        let store = HistoryPreviewStore(directory: directory)
        let session = HistoryPreviewSession(store: store)

        let docx = docxFixture()
        let filePreview = HistoryPreviewData(
            kind: "file",
            name: "document.docx",
            sizeBytes: Int64(docx.count),
            data: docx
        )
        try session.open(filePreview)
        let fileURL = try XCTUnwrap(session.fileURL)
        XCTAssertTrue(FileManager.default.fileExists(atPath: fileURL.path))
        session.close()
        XCTAssertFalse(FileManager.default.fileExists(atPath: fileURL.path))

        let textPreview = HistoryPreviewData(
            kind: "text",
            name: "text.txt",
            sizeBytes: 9,
            data: Data("in memory".utf8)
        )
        try session.open(textPreview)
        XCTAssertEqual(session.text, "in memory")
        XCTAssertNil(session.fileURL)
        XCTAssertEqual(
            try FileManager.default.contentsOfDirectory(
                at: directory,
                includingPropertiesForKeys: nil
            ).count,
            0
        )
    }

    func testPackedImageMaterializesAsInMemoryPng() throws {
        let directory = try temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        let store = HistoryPreviewStore(directory: directory)
        let packed = Data([
            1, 0, 0, 0, // width
            1, 0, 0, 0, // height
            255, 0, 0, 255, // RGBA pixel
        ])
        let preview = HistoryPreviewData(
            kind: "image",
            name: "image",
            sizeBytes: Int64(packed.count),
            data: packed
        )

        guard case .image(let imageMaterial) = try store.materialize(preview) else {
            return XCTFail("image previews must remain in memory")
        }
        XCTAssertEqual(Array(imageMaterial.data.prefix(8)), [137, 80, 78, 71, 13, 10, 26, 10])
        XCTAssertFalse(imageMaterial.image.representations.isEmpty)
        XCTAssertEqual(
            try FileManager.default.contentsOfDirectory(
                at: directory,
                includingPropertiesForKeys: nil
            ).count,
            0
        )
    }

    func testPdfMaterializesInMemoryAndDocxUsesPrivateQuickLookFile() throws {
        let directory = try temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        let store = HistoryPreviewStore(directory: directory)

        let pdf = PDFDocument()
        // PDFPage(image:) requires actual pixels — a bare NSImage(size:)
        // has no representation and yields nil.
        let image = NSImage(size: NSSize(width: 2, height: 2))
        image.lockFocus()
        NSColor.black.setFill()
        NSRect(x: 0, y: 0, width: 2, height: 2).fill()
        image.unlockFocus()
        pdf.insert(try XCTUnwrap(PDFPage(image: image)), at: 0)
        let pdfData = try XCTUnwrap(pdf.dataRepresentation())
        let pdfPreview = HistoryPreviewData(
            kind: "file",
            name: "document.pdf",
            sizeBytes: Int64(pdfData.count),
            data: pdfData
        )
        guard case .pdf(let materializedPdf) = try store.materialize(pdfPreview) else {
            return XCTFail("PDF previews must remain in memory")
        }
        XCTAssertEqual(materializedPdf.data, pdfData)
        XCTAssertEqual(materializedPdf.document.pageCount, 1)
        // The PDF never touched disk: the store directory (created by the
        // fixture) still contains no files.
        XCTAssertTrue(try FileManager.default.contentsOfDirectory(atPath: directory.path).isEmpty)

        let docx = docxFixture()
        let docxPreview = HistoryPreviewData(
            kind: "file",
            name: "document.docx",
            sizeBytes: Int64(docx.count),
            data: docx
        )
        guard case .quickLook(let url) = try store.materialize(docxPreview) else {
            return XCTFail("DOCX previews need the Quick Look fallback")
        }
        XCTAssertTrue(FileManager.default.fileExists(atPath: url.path))
        XCTAssertEqual(
            (try FileManager.default.attributesOfItem(atPath: url.path)[.posixPermissions] as? NSNumber)?.intValue,
            0o600
        )
        try store.remove(url)

        let malformedDocx = HistoryPreviewData(
            kind: "file",
            name: "malformed.docx",
            sizeBytes: 4,
            data: Data([80, 75, 3, 4])
        )
        XCTAssertThrowsError(try store.materialize(malformedDocx)) { error in
            XCTAssertEqual(error as? HistoryPreviewStoreError, .invalidDocument)
        }
    }

    func testPowerPointMaterializesToPrivateQuickLookFilesAndRejectsWrongSignatures() throws {
        let directory = try temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        let store = HistoryPreviewStore(directory: directory)
        let fixtures = [
            ("slides.pptx", pptxFixture()),
            ("slides.ppt", legacyPptFixture())
        ]

        for (name, data) in fixtures {
            let preview = HistoryPreviewData(
                kind: "file",
                name: name,
                sizeBytes: Int64(data.count),
                data: data
            )
            guard case .quickLook(let url) = try store.materialize(preview) else {
                return XCTFail("\(name) should use the native Quick Look renderer")
            }
            XCTAssertEqual(url.pathExtension.lowercased(), (name as NSString).pathExtension)
            XCTAssertEqual(
                (try FileManager.default.attributesOfItem(atPath: url.path)[.posixPermissions]
                    as? NSNumber)?.intValue,
                0o600
            )
            try store.remove(url)
        }

        for name in ["malformed.ppt", "malformed.pptx"] {
            let malformed = HistoryPreviewData(
                kind: "file",
                name: name,
                sizeBytes: 4,
                data: Data([0x50, 0x4B, 0x03, 0x04])
            )
            XCTAssertThrowsError(try store.materialize(malformed)) { error in
                XCTAssertEqual(error as? HistoryPreviewStoreError, .invalidDocument)
            }
        }
    }

    func testRejectsOversizedAndOutsideDeletionTargets() throws {
        let directory = try temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        let store = HistoryPreviewStore(directory: directory)
        let oversized = HistoryPreviewData(
            kind: "file",
            name: "large.bin",
            sizeBytes: HistoryPreviewData.maxBytes + 1,
            data: Data()
        )
        XCTAssertThrowsError(try store.materialize(oversized)) { error in
            XCTAssertEqual(error as? HistoryPreviewStoreError, .tooLarge)
        }

        let outside = directory.deletingLastPathComponent().appendingPathComponent("outside")
        XCTAssertTrue(FileManager.default.createFile(atPath: outside.path, contents: Data()))
        defer { try? FileManager.default.removeItem(at: outside) }
        XCTAssertThrowsError(try store.remove(outside)) { error in
            XCTAssertEqual(error as? HistoryPreviewStoreError, .invalidPath)
        }
    }

    func testRejectsSymlinkPreviewRoot() throws {
        let parent = try temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: parent) }
        let realDirectory = parent.appendingPathComponent("real", isDirectory: true)
        let linkedDirectory = parent.appendingPathComponent("tailsync-preview", isDirectory: true)
        try FileManager.default.createDirectory(at: realDirectory, withIntermediateDirectories: true)
        try FileManager.default.createSymbolicLink(at: linkedDirectory, withDestinationURL: realDirectory)

        let store = HistoryPreviewStore(directory: linkedDirectory)
        XCTAssertThrowsError(try store.write(Data("secret".utf8), named: "preview.txt")) { error in
            XCTAssertEqual(error as? HistoryPreviewStoreError, .invalidPath)
        }
        XCTAssertThrowsError(try store.cleanupStaleFiles()) { error in
            XCTAssertEqual(error as? HistoryPreviewStoreError, .invalidPath)
        }
        XCTAssertTrue(FileManager.default.fileExists(atPath: realDirectory.path))
    }
}
