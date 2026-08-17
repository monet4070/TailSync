import AppKit
import Foundation
import PDFKit

/// Decrypted history bytes for the preview window, plus the metadata needed
/// to pick a renderer and to navigate a multi-entry batch. Produced by
/// `ApiClient.getPreviewData` (validated server response) or by test
/// fixtures; consumed by `HistoryPreviewFormat.detect` and the preview
/// services. The payload bytes themselves are bounded by `maxBytes` (the
/// shared 64 MiB payload cap); the Base64 envelope is rejected before
/// allocation.
struct HistoryPreviewData: Equatable, Sendable {
    /// Payload category from the daemon: "image" | "text" | "file".
    let kind: String
    /// Display/file name hint (never used as a path).
    let name: String
    /// Exact payload byte count reported by the daemon (validated to match
    /// the decoded bytes).
    let sizeBytes: Int64
    /// The decrypted payload bytes.
    let data: Data
    /// History entry id this payload belongs to (absent in fixtures).
    let entryId: Int64?
    /// Batch navigation context, when the entry is part of a batch.
    let batch: HistoryPreviewBatchNavigation?

    /// Shared payload cap across the preview pipeline (64 MiB).
    static let maxBytes: Int64 = 64 * 1024 * 1024

    init(
        kind: String,
        name: String,
        sizeBytes: Int64,
        data: Data,
        entryId: Int64? = nil,
        batch: HistoryPreviewBatchNavigation? = nil
    ) {
        self.kind = kind
        self.name = name
        self.sizeBytes = sizeBytes
        self.data = data
        self.entryId = entryId
        self.batch = batch
    }
}

/// Batch navigation context decoded from the preview response: the position
/// of the current item inside its batch plus the ids of the surrounding
/// entries (nil at the batch edges).
struct HistoryPreviewBatchNavigation: Equatable, Sendable {
    let batchId: String
    let itemIndex: Int
    let itemCount: Int
    let firstEntryId: Int64
    let lastEntryId: Int64
    let previousEntryId: Int64?
    let nextEntryId: Int64?
}

/// A validated, renderer-ready form of a preview payload, produced by
/// `HistoryPreviewStore.materialize`. Never writes plaintext except for the
/// Quick Look case (currently DOCX only), where a sanitized temporary file
/// URL is kept and removed when the session closes or the material is
/// discarded.
final class HistoryPreviewImageMaterial: @unchecked Sendable, Equatable {
    let data: Data
    let image: NSImage

    init(data: Data, image: NSImage) {
        self.data = data
        self.image = image
    }

    static func == (lhs: HistoryPreviewImageMaterial, rhs: HistoryPreviewImageMaterial) -> Bool {
        lhs === rhs
    }
}

final class HistoryPreviewPDFMaterial: @unchecked Sendable, Equatable {
    let data: Data
    let document: PDFDocument

    init(data: Data, document: PDFDocument) {
        self.data = data
        self.document = document
    }

    static func == (lhs: HistoryPreviewPDFMaterial, rhs: HistoryPreviewPDFMaterial) -> Bool {
        lhs === rhs
    }
}

enum HistoryPreviewMaterial: Equatable, Sendable {
    /// UTF-8 text (source or markdown), held in memory.
    case text(String)
    /// Validated image bytes and their decoded AppKit image, held in memory.
    case image(HistoryPreviewImageMaterial)
    /// Validated PDF bytes and the already-parsed PDFKit document, held in memory.
    case pdf(HistoryPreviewPDFMaterial)
    /// A temporary file for Quick Look; must be discarded when stale.
    case quickLook(URL)
    /// No renderer applies; the view shows a "cannot preview" state.
    case unsupported
}
