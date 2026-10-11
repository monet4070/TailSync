import XCTest
@testable import TailSync

///
final class CompositeRestoreMenuTests: XCTestCase {
    func testIncompleteRichItemOpensTextBackupRestore() {
        let subject = info(
            formats: ["html", "png", "text"],
            restorable: ["png", "text"],
            completeness: "external_references"
        )
        XCTAssertEqual(
            HistoryRowActionPolicy.explicitRestoreFormats(subject),
            ["png", "text"],
            "an incomplete rich item must let the user copy just the text"
        )
    }

    func testLiveRevokeRemainsAvailableAfterArchivalCompletes() {
        let subject = info(formats: ["text", "html"], restorable: [])
        XCTAssertTrue(HistoryRowActionPolicy.showsActionsMenu(
            subject,
            canRestoreFormat: false,
            hasArchiveHandler: false,
            hasAdvancedHandlers: true
        ))
        XCTAssertFalse(HistoryRowActionPolicy.showsActionsMenu(
            subject,
            canRestoreFormat: false,
            hasArchiveHandler: true,
            hasAdvancedHandlers: false
        ))
    }

    func testNativeOnlyRowDoesNotHideLiveRevoke() {
        let subject = info(nativeOnly: true, formats: ["text"], restorable: [])
        XCTAssertTrue(HistoryRowActionPolicy.showsActionsMenu(
            subject,
            canRestoreFormat: false,
            hasArchiveHandler: false,
            hasAdvancedHandlers: true
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

    func testAPlainTextRowOffersNoExplicitEntry() {
        let subject = info(formats: ["text"], restorable: ["text"])
        XCTAssertEqual(
            HistoryRowActionPolicy.explicitRestoreFormats(subject),
            [],
            "a plain-text row's whole restore already yields the text"
        )
    }

    func testARichTextRowOffersTheTextBackupEntry() {
        let subject = info(formats: ["text", "html"], restorable: ["text", "html"])
        XCTAssertEqual(
            HistoryRowActionPolicy.explicitRestoreFormats(subject),
            ["text"],
            "a rich item must expose the text-only backup entry"
        )
    }

    /// The case the server refuses: several images, no standalone PNG.
    func testAWithheldPngActionIsNotOffered() {
        let subject = info(
            formats: ["text", "html", "png"],
            restorable: ["html", "text"],
            withheld: [("png", "no_single_image")]
        )
        let actions = HistoryRowActionPolicy.explicitRestoreFormats(subject)
        XCTAssertEqual(
            actions,
            ["text"],
            "the withheld PNG must drop out while the text backup stays open"
        )
        XCTAssertFalse(
            actions.contains("png"),
            "the menu offered a PNG restore that the server withholds as no_single_image"
        )
        XCTAssertEqual(
            subject.withheld_formats.first?.code,
            "no_single_image",
            "the withheld action must carry the code the UI localises"
        )
    }

    /// A native-only item offers no explicit format at all.
    func testANativeOnlyItemOffersNoExplicitFormat() {
        let subject = info(
            nativeOnly: true,
            formats: ["text", "png"],
            restorable: [],
            withheld: [("text", "native_only"), ("png", "native_only")]
        )
        XCTAssertTrue(
            HistoryRowActionPolicy.explicitRestoreFormats(subject).isEmpty,
            "a native-only item must not offer an explicit format restore"
        )
        XCTAssertEqual(subject.withheld_formats.count, 2)
    }

    /// The lone-image case stays available: the guard must not make it unrestorable.
    func testALoneImageStillOffersPng() {
        let subject = info(formats: ["png"], restorable: ["png"])
        XCTAssertEqual(HistoryRowActionPolicy.explicitRestoreFormats(subject), ["png"])
        XCTAssertTrue(subject.withheld_formats.isEmpty)
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

    func testAMenuWithOnlyAdvancedCallbacksStillShows() {
        let subject = info(formats: ["text"], restorable: ["text"])
        XCTAssertTrue(HistoryRowActionPolicy.showsActionsMenu(
            subject,
            canRestoreFormat: false,
            hasArchiveHandler: false,
            hasAdvancedHandlers: true
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
