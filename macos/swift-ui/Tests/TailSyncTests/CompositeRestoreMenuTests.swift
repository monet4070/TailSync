import XCTest
@testable import TailSync

final class CompositeRestoreMenuTests: XCTestCase {
    func testLiveRevokeRemainsAvailableAfterArchivalCompletes() {
        for subject in [
            info(formats: ["text", "html"], restorable: []),
            info(nativeOnly: true, formats: ["text"], restorable: []),
            info(formats: ["text"], restorable: ["text"]),
        ] {
            XCTAssertTrue(HistoryRowActionPolicy.showsActionsMenu(
                subject,
                canRestoreFormat: false,
                hasArchiveHandler: false,
                hasAdvancedHandlers: true
            ))
        }
        XCTAssertFalse(HistoryRowActionPolicy.showsActionsMenu(
            info(formats: ["text", "html"], restorable: []),
            canRestoreFormat: false,
            hasArchiveHandler: true,
            hasAdvancedHandlers: false
        ))
    }

    private func info(
        nativeOnly: Bool = false,
        formats: [String],
        restorable: [String],
        completeness: String = "complete",
        images: Int = 0,
        withheld: [(String, String)] = []
    ) -> ContractCompositeInfo {
        let json: [String: Any] = [
            "completeness": completeness,
            "formats": formats,
            "image_count": images,
            "native_item_count": 1,
            "native_only": nativeOnly,
            "representation_count": formats.count,
            "restorable_formats": restorable,
            "withheld_formats": withheld.map { ["format": $0.0, "code": $0.1] },
        ]
        let data = try! JSONSerialization.data(withJSONObject: json)
        return try! JSONDecoder().decode(ContractCompositeInfo.self, from: data)
    }

    func testExplicitRestoreFormatsFollowAvailableRepresentations() {
        let cases: [(ContractCompositeInfo, [String])] = [
            (info(formats: ["text"], restorable: ["text"]), []),
            (info(formats: ["text", "html"], restorable: ["text", "html"]), ["text"]),
            (info(formats: ["text", "html", "png"], restorable: ["html", "text"],
                  withheld: [("png", "no_single_image")]), ["text"]),
            (info(nativeOnly: true, formats: ["text", "png"], restorable: [],
                  withheld: [("text", "native_only"), ("png", "native_only")]), []),
            (info(formats: ["png"], restorable: ["png"]), ["png"]),
            (info(formats: ["html", "png", "text"], restorable: ["png", "text"],
                  completeness: "external_references"), ["png", "text"]),
        ]
        for (subject, expected) in cases {
            XCTAssertEqual(HistoryRowActionPolicy.explicitRestoreFormats(subject), expected,
                           "formats=\(subject.formats), nativeOnly=\(subject.native_only)")
        }
    }

    func testAnItemWithNothingToOfferShowsNoMenu() {
        let textOnly = info(formats: ["text"], restorable: ["text"])
        XCTAssertFalse(
            HistoryRowActionPolicy.showsActionsMenu(
                textOnly,
                canRestoreFormat: true,
                hasArchiveHandler: true,
                hasAdvancedHandlers: false
            )
        )
        XCTAssertFalse(
            HistoryRowActionPolicy.showsActionsMenu(
                textOnly,
                canRestoreFormat: false,
                hasArchiveHandler: true,
                hasAdvancedHandlers: false
            )
        )
    }

    func testMaintenanceEntriesStayAvailableForIncompleteHtml() {
        let subject = info(
            formats: ["text", "html"],
            restorable: ["text"],
            completeness: "external_references"
        )
        XCTAssertTrue(HistoryRowActionPolicy.showsMaintenanceActions(subject))
        XCTAssertTrue(
            HistoryRowActionPolicy.showsActionsMenu(
                subject,
                canRestoreFormat: true,
                hasArchiveHandler: true,
                hasAdvancedHandlers: false
            )
        )
    }

    func testMaintenanceEntriesStayUnavailableForNativeOnlyAndCompleteItems() {
        let nativeOnly = info(nativeOnly: true, formats: ["text", "html"], restorable: [], completeness: "missing_formats")
        XCTAssertFalse(HistoryRowActionPolicy.showsMaintenanceActions(nativeOnly))
        let complete = info(formats: ["text", "html"], restorable: ["html", "text"])
        XCTAssertFalse(HistoryRowActionPolicy.showsMaintenanceActions(complete))
    }

    func testAdvancedSubmenuStaysForIncompleteRichOrLiveRevoke() {
        let incomplete = info(formats: ["text", "html"], restorable: ["text"], completeness: "external_references")
        XCTAssertTrue(HistoryRowActionPolicy.showsAdvancedActions(
            incomplete,
            hasAuthorizeHandler: true,
            hasImportHandler: true,
            hasRevokeHandler: false
        ))
        let complete = info(formats: ["text", "html"], restorable: ["text", "html"])
        XCTAssertFalse(HistoryRowActionPolicy.showsAdvancedActions(
            complete,
            hasAuthorizeHandler: true,
            hasImportHandler: true,
            hasRevokeHandler: false
        ))
        XCTAssertTrue(HistoryRowActionPolicy.showsAdvancedActions(
            complete,
            hasAuthorizeHandler: false,
            hasImportHandler: false,
            hasRevokeHandler: true
        ))
    }

    func testExternalReferencesShowResourcesMissingInsteadOfGenericIncomplete() {
        let subject = info(
            formats: ["text", "html"],
            restorable: ["text"],
            completeness: "external_references"
        )
        XCTAssertEqual(HistoryRowStatusLabel.text(for: subject), Loc.t("history.composite.resourcesMissing"))
        XCTAssertNotEqual(HistoryRowStatusLabel.text(for: subject), Loc.t("history.composite.incomplete"))
    }

    func testDamagedOrTruncatedStatesAreNotMasked() {
        let subject = info(formats: ["text", "html"], restorable: [], completeness: "missing_formats")
        XCTAssertEqual(HistoryRowStatusLabel.text(for: subject), Loc.t("history.composite.incomplete"))
    }
}
