import XCTest
@testable import TailSync

final class HistoryThumbnailCacheTests: XCTestCase {
    private func entry(
        id: Int64,
        dataHash: String,
        capture: String,
        content: String,
        revision: Int64,
        digest: String,
        classifier: Int64
    ) -> HistoryEntry {
        let json: [String: Any] = [
            "id": id,
            "timestamp": "2026-10-07T00:00:00Z",
            "type": "composite",
            "description": "x",
            "data_hash": dataHash,
            "size_bytes": 1,
            "source_peer": "self",
            "category": "text",
            "categories": ["text"],
            "category_confidence": 100,
            "classifier_version": classifier,
            "display": ["version": 1, "kind": "document", "thumbnail_eligible": true, "cache_key": "authority-\(revision)-\(digest)"],
            "pinned": false,
            "batch_status": "complete",
            "composite": [
                "completeness": "complete",
                "formats": ["text"],
                "identity": [
                    "archive_digest": digest,
                    "archive_revision": revision,
                    "capture_id": capture,
                    "content_id": content,
                    "dedupe_key": NSNull(),
                    "version": 1,
                ],
                "image_count": 1,
                "native_item_count": 0,
                "native_only": false,
                "representation_count": 1,
                "restorable_formats": ["text"],
                "withheld_formats": [],
            ],
        ]
        let data = try! JSONSerialization.data(withJSONObject: json)
        return try! JSONDecoder().decode(HistoryEntry.self, from: data)
    }

    func testThumbnailKeyBindsIdentityAndClassifierVersion() {
        let base = entry(id: 7, dataHash: "h", capture: "c1", content: "i1", revision: 1, digest: "d1", classifier: 1)
        let same = entry(id: 7, dataHash: "h", capture: "c1", content: "i1", revision: 1, digest: "d1", classifier: 1)
        let newClassifier = entry(id: 7, dataHash: "h", capture: "c1", content: "i1", revision: 1, digest: "d1", classifier: 2)
        let rearchived = entry(id: 7, dataHash: "h", capture: "c1", content: "i1", revision: 2, digest: "d2", classifier: 1)
        let newCapture = entry(id: 7, dataHash: "h", capture: "c2", content: "i1", revision: 1, digest: "d1", classifier: 1)
        XCTAssertEqual(HistoryThumbnailKey(entry: base), HistoryThumbnailKey(entry: same))
        XCTAssertNotEqual(HistoryThumbnailKey(entry: base), HistoryThumbnailKey(entry: newClassifier))
        XCTAssertNotEqual(HistoryThumbnailKey(entry: base), HistoryThumbnailKey(entry: rearchived))
        XCTAssertNotEqual(HistoryThumbnailKey(entry: base), HistoryThumbnailKey(entry: newCapture))
    }

    func testCacheDoesNotServeStalePixelsForANewRevision() {
        let first = HistoryThumbnailKey(entry: entry(id: 7, dataHash: "h", capture: "c1", content: "i1", revision: 1, digest: "d1", classifier: 1))
        let second = HistoryThumbnailKey(entry: entry(id: 7, dataHash: "h", capture: "c1", content: "i1", revision: 2, digest: "d2", classifier: 1))
        HistoryThumbnailCache.removeAll()
        defer { HistoryThumbnailCache.removeAll() }
        let image = NSImage(size: NSSize(width: 1, height: 1))
        image.addRepresentation(NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 1, pixelsHigh: 1, bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 4, bitsPerPixel: 32)!)
        HistoryThumbnailCache.insert(image, for: first)
        XCTAssertNotNil(HistoryThumbnailCache.image(for: first))
        XCTAssertNil(HistoryThumbnailCache.image(for: second))
    }

    func testOwnerIsolationAndRealBitmapBudgetIgnorePointSize() {
        let base = entry(id: 7, dataHash: "h", capture: "c1", content: "i1", revision: 1, digest: "d1", classifier: 1)
        let history = HistoryThumbnailKey(entry: base, owner: "all")
        let favorites = HistoryThumbnailKey(entry: base, owner: "favorites")
        XCTAssertNotEqual(history, favorites)
        HistoryThumbnailCache.removeAll()
        defer { HistoryThumbnailCache.removeAll() }
        let image = NSImage(size: NSSize(width: 1, height: 1))
        image.addRepresentation(NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 1024, pixelsHigh: 1024, bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 4096, bitsPerPixel: 32)!)
        HistoryThumbnailCache.insert(image, for: history)
        XCTAssertEqual(HistoryThumbnailCache.memoryCost, 4 * 1024 * 1024)
        XCTAssertNotNil(HistoryThumbnailCache.image(for: history))
        XCTAssertNil(HistoryThumbnailCache.image(for: favorites))
        HistoryThumbnailCache.removeAll()
        HistoryThumbnailCache.insert(image, for: history, encodedBytes: 1)
        XCTAssertEqual(HistoryThumbnailCache.memoryCost, 0)
        HistoryThumbnailCache.insert(image, for: history, encodedBytes: Int.max)
        XCTAssertEqual(HistoryThumbnailCache.memoryCost, 0)
    }

    func testDisplayMetadataRejectsUnknownVersionsAndKinds() throws {
        let base: [String: Any] = ["version": 1, "kind": "document", "thumbnail_eligible": true, "cache_key": "th1:bound"]
        let decoder = JSONDecoder()
        let metadata = try decoder.decode(ContractHistoryDisplayInfo.self, from: JSONSerialization.data(withJSONObject: base))
        XCTAssertEqual(metadata.kind, "document")
        var invalid = base
        invalid["version"] = 2
        XCTAssertThrowsError(try decoder.decode(ContractHistoryDisplayInfo.self, from: JSONSerialization.data(withJSONObject: invalid)))
        invalid = base
        invalid["kind"] = "future"
        XCTAssertThrowsError(try decoder.decode(ContractHistoryDisplayInfo.self, from: JSONSerialization.data(withJSONObject: invalid)))
    }
}
