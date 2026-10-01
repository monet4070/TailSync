import Foundation
import XCTest
@testable import TailSync

final class PairingRecoveryTests: XCTestCase {
  func testOldDaemonPairingStatusStillDecodes() throws {
    let data = Data(#"{"pairing_enabled":false,"phase":"disabled","remaining_seconds":0,"failed_attempts":0,"max_failures":5}"#.utf8)
    let status = try JSONDecoder().decode(ApiClient.PairingStatus.self, from: data)
    XCTAssertNil(status.pending)
    XCTAssertNil(status.pending_store_unavailable)
  }
  func testPendingSummaryDoesNotImplyLocalTrust() throws {
    let data = Data(#"{"pairing_enabled":false,"phase":"disabled","remaining_seconds":0,"failed_attempts":0,"max_failures":5,"pending":[{"hostname":"peer","address":"192.168.1.2","interface":"lan","fingerprint":"key","locally_trusted":false}],"pending_store_unavailable":false}"#.utf8)
    let status = try JSONDecoder().decode(ApiClient.PairingStatus.self, from: data)
    XCTAssertEqual(status.pending?.count, 1)
    XCTAssertEqual(status.pending?.first?.locally_trusted, false)
    XCTAssertNil(status.peer)
    XCTAssertEqual(status.phase, "disabled")
  }
}
