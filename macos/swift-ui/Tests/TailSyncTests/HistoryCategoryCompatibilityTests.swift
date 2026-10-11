import Foundation
import XCTest
@testable import TailSync

final class HistoryCategoryCompatibilityTests: XCTestCase {
    func testLegacyTechnicalLabelsDecodeAsOneCodeCategory() throws {
        for category in ["command", "structured_data", "code"] {
            let value: [String: Any] = [
                "id": 7, "timestamp": "2026-10-10T00:00:00Z", "type": "text",
                "description": "git status --short", "data_hash": "fixture",
                "size_bytes": 18, "source_peer": "self", "category": category,
                "categories": ["command", "structured_data", "code"],
            ]
            let entry = try JSONDecoder().decode(HistoryEntry.self, from: JSONSerialization.data(withJSONObject: value))
            XCTAssertEqual(entry.category, "code")
            XCTAssertEqual(entry.categories, ["code"])
            XCTAssertEqual(entry.icon, "chevron.left.forwardslash.chevron.right")
            XCTAssertEqual(entry.categoryLabel, Loc.t("history.category.code"))
        }
    }
}
