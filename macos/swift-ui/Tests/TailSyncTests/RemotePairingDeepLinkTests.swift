import Foundation
import XCTest
@testable import TailSync

final class RemotePairingDeepLinkTests: XCTestCase {
    func testReceivedInviteExpandsThePairingInput() {
        var state = RemotePairingInputState()
        state.receive("  tailsync://pair/v1/invite  ")
        XCTAssertTrue(state.expanded)
        XCTAssertEqual(state.link, "tailsync://pair/v1/invite")
        XCTAssertNil(state.feedback)
    }

    func testPairingFailureReplacesSuccessfulPreview() {
        var state = RemotePairingInputState()
        state.preview = ApiClient.RemotePairingInvitePreview(endpoint_id: "peer", expires_at: 123, remaining_seconds: 60)
        state.fail("Invite has expired")
        XCTAssertEqual(state.feedback, .error("Invite has expired"))
    }

    func testNewOperationInvalidatesOldPreview() {
        var state = RemotePairingInputState()
        state.preview = ApiClient.RemotePairingInvitePreview(endpoint_id: "peer", expires_at: 123, remaining_seconds: 60)
        state.beginOperation()
        XCTAssertNil(state.preview)
        XCTAssertNil(state.feedback)
    }

    func testValidInviteIsStoredAndTakenExactlyOnce() throws {
        var inbox = RemotePairingDeepLinkInbox()
        let url = try XCTUnwrap(URL(string: "tailsync://pair/v1/abc_DEF-123"))

        XCTAssertEqual(inbox.receive(url), url.absoluteString)
        XCTAssertEqual(inbox.takePending(), url.absoluteString)
        XCTAssertNil(inbox.takePending())
    }

    func testUnrelatedOrIncompleteURLsNeverEnterTheInbox() throws {
        var inbox = RemotePairingDeepLinkInbox()
        let rejected = [
            "https://example.invalid/pair/v1/abc",
            "tailsync://settings/v1/abc",
            "tailsync://pair/v2/abc",
            "tailsync://pair/v1/",
        ]

        for value in rejected {
            XCTAssertNil(inbox.receive(try XCTUnwrap(URL(string: value))))
        }
        XCTAssertNil(inbox.takePending())
    }

    func testNewValidInviteReplacesAnOlderUnconsumedInvite() throws {
        var inbox = RemotePairingDeepLinkInbox()
        let first = try XCTUnwrap(URL(string: "tailsync://pair/v1/first"))
        let second = try XCTUnwrap(URL(string: "tailsync://pair/v1/second"))

        XCTAssertEqual(inbox.receive(first), first.absoluteString)
        XCTAssertEqual(inbox.receive(second), second.absoluteString)
        XCTAssertEqual(inbox.takePending(), second.absoluteString)
    }
}
