import XCTest
@testable import TailSync

final class HistoryRowDiagnosticsTests: XCTestCase {
    private func info(
        nativeOnly: Bool = false,
        formats: [String],
        restorable: [String],
        completeness: String = "complete",
        imageCount: Int = 2,
        withheld: [(String, String)] = []
    ) -> ContractCompositeInfo {
        let json: [String: Any] = [
            "completeness": completeness,
            "formats": formats,
            "image_count": imageCount,
            "native_item_count": 1,
            "native_only": nativeOnly,
            "representation_count": formats.count,
            "restorable_formats": restorable,
            "withheld_formats": withheld.map { ["format": $0.0, "code": $0.1] },
        ]
        let data = try! JSONSerialization.data(withJSONObject: json)
        return try! JSONDecoder().decode(ContractCompositeInfo.self, from: data)
    }

    private func withLanguages(_ body: (String) -> Void) {
        let original = Loc.shared.lang
        defer { Loc.shared.lang = original }
        for language in ["en", "zh-CN"] {
            Loc.shared.lang = language
            body(language)
        }
    }

    private func value(_ label: String, in rows: [HistoryRowDiagnostics.Row]) -> String? {
        rows.first { $0.label == label }?.value
    }

    func testTheRowKeepsIntegrityWordingAndHidesCountsOnly() {
        withLanguages { _ in
            let nativeOnly = info(nativeOnly: true, formats: ["text"], restorable: [])
            XCTAssertEqual(HistoryRowStatusLabel.text(for: nativeOnly), Loc.t("history.composite.localOnly"))

            let incomplete = info(
                formats: ["text", "html"],
                restorable: ["text"],
                completeness: "external_references"
            )
            XCTAssertEqual(HistoryRowStatusLabel.text(for: incomplete), Loc.t("history.composite.resourcesMissing"))

            let complete = info(formats: ["text"], restorable: ["text"])
            XCTAssertNil(HistoryRowStatusLabel.text(for: complete))
            XCTAssertNil(HistoryRowStatusLabel.text(for: nil))
        }
    }

    func testDiagnosticsExposeCountsNativeOnlyCompletenessAndWithheld() {
        withLanguages { _ in
            let subject = info(
                formats: ["text", "html", "png"],
                restorable: ["text", "html"],
                completeness: "external_references",
                imageCount: 2,
                withheld: [("png", "no_single_image")]
            )
            let rows = HistoryRowDiagnostics.rows(for: subject)

            XCTAssertEqual(value(Loc.t("history.composite.diag.formats"), in: rows), "text, html, png")
            XCTAssertEqual(value(Loc.t("history.composite.diag.representations"), in: rows), "3")
            XCTAssertEqual(value(Loc.t("history.composite.diag.images"), in: rows), "2")
            XCTAssertEqual(value(Loc.t("history.composite.diag.nativeItems"), in: rows), "1")
            XCTAssertEqual(value(Loc.t("history.composite.diag.nativeOnly"), in: rows), Loc.t("history.composite.diag.no"))
            XCTAssertEqual(
                value(Loc.t("history.composite.diag.completeness"), in: rows),
                Loc.t("history.composite.completeness.external_references")
            )
            XCTAssertEqual(value(Loc.t("history.composite.diag.restorable"), in: rows), "text, html")

            let withheld = value(Loc.t("history.composite.diag.withheld"), in: rows)
            XCTAssertEqual(withheld, "png: \(Loc.t("history.restore.code.no_single_image"))")
        }
    }

    func testDiagnosticsNeverLeakRawIdentifiers() {
        withLanguages { language in
            let subject = info(
                nativeOnly: true,
                formats: ["text", "png"],
                restorable: [],
                completeness: "missing_formats",
                withheld: [("text", "native_only"), ("png", "no_single_image"), ("rtf", "future_code")]
            )
            let rows = HistoryRowDiagnostics.rows(for: subject)
            XCTAssertEqual(value(Loc.t("history.composite.diag.nativeOnly"), in: rows), Loc.t("history.composite.diag.yes"))
            XCTAssertEqual(
                value(Loc.t("history.composite.diag.completeness"), in: rows),
                Loc.t("history.composite.completeness.missing_formats")
            )
            let withheld = value(Loc.t("history.composite.diag.withheld"), in: rows) ?? ""
            XCTAssertFalse(withheld.contains("native_only"), "\(language): leaked a raw withheld code")
            XCTAssertFalse(withheld.contains("no_single_image"), "\(language): leaked a raw withheld code")
            XCTAssertTrue(withheld.contains(Loc.t("history.composite.diag.unavailable")))
        }
    }

    func testEveryDiagnosticsStringIsLocalisedInBothLanguages() {
        let keys = [
            "history.composite.moreActions",
            "history.composite.details",
            "history.composite.diag.formats",
            "history.composite.diag.representations",
            "history.composite.diag.images",
            "history.composite.diag.nativeItems",
            "history.composite.diag.nativeOnly",
            "history.composite.diag.completeness",
            "history.composite.diag.restorable",
            "history.composite.diag.withheld",
            "history.composite.diag.yes",
            "history.composite.diag.no",
            "history.composite.diag.none",
            "history.composite.diag.unavailable",
            "history.composite.diag.unknown",
            "history.composite.completeness.complete",
            "history.composite.completeness.external_references",
            "history.composite.completeness.missing_formats",
        ]
        withLanguages { language in
            for key in keys {
                let text = Loc.t(key)
                XCTAssertFalse(text.isEmpty, "\(language): \(key) resolved to nothing")
                XCTAssertNotEqual(text, key, "\(language): \(key) is missing")
            }
        }
    }
}
