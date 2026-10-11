import Foundation

enum HistoryResourceArchiveFeedback {
  static func message(_ report: ContractArchiveEntryImagesReport) -> String {
    if report.complete { return Loc.t("history.composite.archiveDone") }
    let reasons = report.failures.prefix(8).map { failure in
      let key: String
      switch failure.code {
      case "archive_not_http", "archive_user_info", "archive_not_public": key = "history.composite.archiveBlocked"
      case "archive_decode", "archive_mime": key = "history.composite.archiveInvalid"
      case "archive_too_many", "archive_too_large", "archive_total_budget": key = "history.composite.archiveLimit"
      default: key = "history.composite.archiveUnavailable"
      }
      return "#\(failure.image_index + 1): \(Loc.t(key))"
    }
    return ([Loc.t("history.composite.archivePartial")] + reasons).joined(separator: " · ")
  }
}
