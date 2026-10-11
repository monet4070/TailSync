import XCTest
@testable import TailSync

final class HistoryBatchPresentationTests: XCTestCase {
    private func file(_ id: Int, count: Int? = nil, total: Int? = nil,
                      status: String = "complete", batch: String? = "batch") throws -> HistoryEntry {
        var object: [String: Any] = [
            "id": id, "timestamp": "2026-10-10T00:00:00Z", "type": "file",
            "description": "example-\(id).md", "data_hash": "fixture-\(id)",
            "size_bytes": 100, "source_peer": "self", "batch_status": status,
        ]
        object["batch_id"] = batch
        object["batch_count"] = count
        object["batch_total"] = total
        return try JSONDecoder().decode(HistoryEntry.self, from: JSONSerialization.data(withJSONObject: object))
    }

    func testCompleteSingleFileHasNoGroupHeaderEvenWithBatchID() throws {
        let single = try file(2809, count: 1, total: 1)
        XCTAssertFalse(HistoryBatchPresentation.isBatchStart(single, in: [single]))
    }

    func testLegacySingleFileWithoutCountsHasNoGroupHeader() throws {
        let single = try file(1)
        XCTAssertFalse(HistoryBatchPresentation.isBatchStart(single, in: [single]))
    }

    func testMultiFileBatchHeaderAppearsOnlyAtStart() throws {
        let entries = try [file(1, count: 2, total: 2), file(2, count: 2, total: 2)]
        XCTAssertTrue(HistoryBatchPresentation.isBatchStart(entries[0], in: entries))
        XCTAssertFalse(HistoryBatchPresentation.isBatchStart(entries[1], in: entries))
    }

    func testFilteredMultiFileBatchKeepsHeaderWithOneVisibleFile() throws {
        let visible = try file(1, count: 3, total: 3)
        XCTAssertTrue(HistoryBatchPresentation.isBatchStart(visible, in: [visible]))
    }

    func testIncompleteBatchKeepsWarningWithOneReceivedFile() throws {
        let partial = try file(1, count: 1, total: 3, status: "incomplete")
        XCTAssertTrue(HistoryBatchPresentation.isBatchStart(partial, in: [partial]))
        let inconsistent = try file(2, count: 1, total: 1, status: "incomplete")
        XCTAssertTrue(HistoryBatchPresentation.isBatchStart(inconsistent, in: [inconsistent]))
    }

    func testLegacyMultiFileBatchUsesVisibleRowsWhenCountsAreMissing() throws {
        let entries = try [file(1), file(2)]
        XCTAssertTrue(HistoryBatchPresentation.isBatchStart(entries[0], in: entries))
    }
}
