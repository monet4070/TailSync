import XCTest
@testable import TailSync

final class HistoryRestoreRetryIdentityTests: XCTestCase {
  private func entry(_ id: Int64, type: String = "composite", hash: String = "complete-content-hash", capture: String? = nil, content: String = "hcp1:content", key: String? = "hpd1:closed", revision: Int64 = 1) throws -> HistoryEntry {
    var wire: [String: Any] = [
      "id": id, "timestamp": "2026-10-05T11:00:00Z", "type": type,
      "description": "test", "data_hash": hash, "size_bytes": 84,
      "source_peer": "self", "pinned": false,
    ]
    if let capture {
      wire["composite"] = ["native_only": false, "native_item_count": 0, "formats": ["text"], "restorable_formats": ["text"], "withheld_formats": [], "representation_count": 1, "image_count": 0, "completeness": "complete",
        "identity": ["version": 1, "capture_id": capture, "content_id": content, "dedupe_key": key as Any? ?? NSNull(), "archive_revision": revision, "archive_digest": "archive"]] as [String: Any]
    }
    return try JSONDecoder().decode(HistoryEntry.self, from: JSONSerialization.data(withJSONObject: wire))
  }

  func testDuplicateCaptureRebindsTheRetryToTheExistingContent() throws {
    let original = try entry(138, capture: "first-capture")
    let replacement = try entry(140, capture: "second-capture")
    XCTAssertEqual(HistoryRestoreRetryIdentity(original).resolve(in: [replacement]), 140)
  }

  func testDeletedContentDoesNotProduceAStaleRestoreOrResurrectIt() throws {
    let original = try entry(138)
    let other = try entry(139, hash: "different-complete-content")
    XCTAssertNil(HistoryRestoreRetryIdentity(original).resolve(in: [other]))
    XCTAssertNil(HistoryRestoreRetryIdentity(original).resolve(in: []))
  }

  func testEmptyHashAndWrongTypeCannotGuessAReplacement() throws {
    XCTAssertNil(HistoryRestoreRetryIdentity(try entry(138, hash: "")).resolve(in: [try entry(140, hash: "")]))
    XCTAssertNil(HistoryRestoreRetryIdentity(try entry(138)).resolve(in: [try entry(140, type: "text")]))
  }

  func testAnExistingOriginalMustStillHaveTheSameContent() throws {
    let original = try entry(138)
    XCTAssertEqual(HistoryRestoreRetryIdentity(original).resolve(in: [original]), 138)
    XCTAssertNil(HistoryRestoreRetryIdentity(original).resolve(in: [try entry(138, hash: "changed")]))
  }
  func testLegacySamePortableHashCannotRebindAcrossRows() throws {
    XCTAssertNil(HistoryRestoreRetryIdentity(try entry(138)).resolve(in: [try entry(140)]))
  }
  func testDistinctOriginalsOrMissingClosureCannotRebind() throws {
    let original = try entry(138, capture: "first", content: "hcl1:1:a", key: nil)
    XCTAssertNil(HistoryRestoreRetryIdentity(original).resolve(in: [try entry(140, capture: "second", content: "hcl1:1:b", key: nil)]))
    let closed = try entry(138, capture: "first")
    XCTAssertNil(HistoryRestoreRetryIdentity(closed).resolve(in: [try entry(140, capture: "second", content: "hcp1:different")]))
  }
  func testSameCaptureRepreparesNewArchiveButRejectsChangedCapture() throws {
    let original = try entry(138, capture: "first")
    XCTAssertEqual(HistoryRestoreRetryIdentity(original).resolve(in: [try entry(138, capture: "first", revision: 2)]), 138)
    XCTAssertNil(HistoryRestoreRetryIdentity(original).resolve(in: [try entry(138, capture: "different")]))
  }

}
