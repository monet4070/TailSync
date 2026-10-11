import Foundation
import XCTest
@testable import TailSync

final class HistoryResourceArchiveFeedbackTests: XCTestCase {
  func testArchiveFeedbackIsLocalisedAndNeverPromotesPartialOrUnknownResources() throws {
    let previous = Loc.shared.lang
    defer { Loc.shared.lang = previous }
    for language in ["en", "zh-CN"] {
      Loc.shared.lang = language
      for code in ["archive_not_public", "archive_decode", "archive_total_budget", "future_code"] {
        let data = Data("{\"archived\":1,\"deduplicated\":0,\"complete\":false,\"failures\":[{\"image_index\":1,\"code\":\"\(code)\"}]}".utf8)
        let report = try JSONDecoder().decode(ContractArchiveEntryImagesReport.self, from: data)
        let message = HistoryResourceArchiveFeedback.message(report)
        XCTAssertTrue(message.contains("#2"))
        XCTAssertFalse(message.contains("history.composite."))
        XCTAssertFalse(message.contains(code))
        XCTAssertNotEqual(message, Loc.t("history.composite.archiveDone"))
      }
    }
  }
}
