import XCTest
@testable import TailSync

/// R5-04 / G-6 / T25 — the restore outcome codes are localised in both languages.
///
/// The server answers with a stable code and never with Rust prose, so the UI has to
/// own the wording. If a code has no entry, the user sees the raw identifier — which is
/// exactly the "don't pass Rust English through" failure the plan calls out. Only
/// `published` may be presented as a completed copy; every other code has to say that
/// the item was saved to history and let the user copy again.
final class CompositeRestoreLocalizationTests: XCTestCase {
    func testNegotiatedSocketErrorsKeepRestoreIdentityAndLocaliseIt() {
        withLanguages { _ in
            for code in Self.restoreCodes where code != "published" {
                let wire: [String: Any] = ["ok": false, "error": [
                    "schema_version": 1, "code": code, "retryable": false,
                    "message_key": "private/untrusted", "detail_class": "private",
                ]]
                let normalized = ApiClient.normalizedStableErrorResponse(wire)
                XCTAssertEqual(normalized["error"] as? String, code)
                XCTAssertEqual(ApiError.serverError(normalized["error"] as? String ?? "").localizedDescription,
                               Loc.t("history.restore.code.\(code)"))
            }
        }
    }
    /// Mirrors `RestoreOutcome::code()` in `shared/tailsync-runtime/src/history.rs`.
    private static let restoreCodes = [
        "published",
        "publish_partial",
        "publish_unknown",
        "publish_superseded",
        "native_only",
        "originals_incomplete",
        "resources_incomplete",
        "no_single_image",
        "format_unavailable",
        "unsupported_format",
    ]

    private func withLanguages(_ body: (String) -> Void) {
        let original = Loc.shared.lang
        defer { Loc.shared.lang = original }
        for language in ["en", "zh-CN"] {
            Loc.shared.lang = language
            body(language)
        }
    }

    func testEveryRestoreCodeIsLocalisedInBothLanguages() {
        withLanguages { language in
            let keys = Self.restoreCodes.map { "history.restore.code.\($0)" } + [
                "history.captureOriginUnknown", "history.captureNotShared", "history.captureNotSaved",
                "history.receiveNotSaved"
            ]
            for key in keys {
                let text = Loc.t(key)
                XCTAssertFalse(
                    text.isEmpty,
                    "\(language): \(key) resolved to nothing"
                )
                XCTAssertNotEqual(
                    text,
                    key,
                    "\(language): \(key) is missing, so the user would see the raw code"
                )
                // A localised string must not leak a Rust identifier or a wire marker.
                XCTAssertFalse(
                    text.contains("_"),
                    "\(language): \(key) looks like an identifier, not user text: \(text)"
                )
                XCTAssertFalse(
                    text.lowercased().contains("temporary-pressure"),
                    "\(language): \(key) leaked an internal marker"
                )
            }
        }
    }

    /// Only `published` says the content is on the clipboard. Everything else has to
    /// tell the user the item is in history and that copying again is possible.
    func testOnlyPublishedIsPresentedAsACompletedCopy() {
        withLanguages { language in
            let published = Loc.t("history.restore.code.published")
            XCTAssertFalse(
                published.isEmpty,
                "\(language): published must have wording"
            )

            for code in Self.restoreCodes where code != "published" {
                let text = Loc.t("history.restore.code.\(code)")
                XCTAssertNotEqual(
                    text,
                    published,
                    "\(language): \(code) must not reuse the success wording"
                )
            }

            // The outcomes that may have left history in a usable state say so, and
            // offer the explicit re-copy the plan requires.
            for code in ["publish_partial", "publish_unknown"] {
                let text = Loc.t("history.restore.code.\(code)")
                XCTAssertTrue(
                    text.contains("history") || text.contains("历史"),
                    "\(language): \(code) must explain that the item is saved in history: \(text)"
                )
                XCTAssertTrue(
                    text.contains("again") || text.contains("再"),
                    "\(language): \(code) must let the user copy again: \(text)"
                )
            }
        }
    }

    /// R5-01 — a capture whose history write failed has to be visible, in both
    /// languages. This is the wording half of the guarantee; the structural half lives
    /// in `scripts/check-capture-failure-visibility.sh`.
    func testACaptureThatCouldNotBeSavedIsExplained() {
        withLanguages { language in
            let key = "history.captureNotSaved"
            let text = Loc.t(key)
            XCTAssertFalse(text.contains("still sent") || text.contains("仍已发送"))
            XCTAssertFalse(text.isEmpty, "\(language): \(key) resolved to nothing")
            XCTAssertNotEqual(
                text,
                key,
                "\(language): \(key) is missing, so a full disk would lose the copy silently"
            )
            // Saving failure proves nothing about delivery: never claim that a peer received it.
            XCTAssertTrue(
                text.contains("history") || text.contains("历史"),
                "\(language): \(key) must say the item was not saved to history: \(text)"
            )
            XCTAssertTrue(
                text.contains("again") || text.contains("重新复制"),
                "\(language): \(key) must explain how to recover after storage is available: \(text)"
            )
        }
    }

    func testTheRecopyActionIsLocalised() {
        withLanguages { language in
            let text = Loc.t("history.restore.recopy")
            XCTAssertFalse(text.isEmpty, "\(language): the re-copy action needs a label")
            XCTAssertNotEqual(
                text,
                "history.restore.recopy",
                "\(language): the re-copy action is missing"
            )
        }
    }

    func testIncompleteResourcesPointsAtTheBackupCopyEntries() {
        withLanguages { language in
            let text = Loc.t("history.restore.code.resources_incomplete")
            XCTAssertFalse(
                text.contains("Choose an available format") || text.contains("选择可用格式"),
                "\(language): resources_incomplete still asks for a format that is not there: \(text)"
            )
            XCTAssertTrue(
                text.lowercased().contains("text") || text.contains("文字"),
                "\(language): resources_incomplete must mention copying the text: \(text)"
            )
            XCTAssertTrue(
                text.lowercased().contains("image") || text.contains("图片"),
                "\(language): resources_incomplete must mention copying the image: \(text)"
            )
        }
    }

    func testTheAdvancedSubmenuAndResourceStatusLabelsAreLocalised() {
        withLanguages { language in
            for key in [
                "history.composite.advanced",
                "history.composite.resourcesMissing",
                "history.composite.archiveImagesHint",
            ] {
                let text = Loc.t(key)
                XCTAssertFalse(text.isEmpty, "\(language): \(key) resolved to nothing")
                XCTAssertNotEqual(text, key, "\(language): \(key) is missing")
            }
        }
    }
}
