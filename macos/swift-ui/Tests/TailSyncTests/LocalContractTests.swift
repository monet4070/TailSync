import Foundation
import XCTest
@testable import TailSync

final class LocalContractTests: XCTestCase {
  func testProductionRustFixturesUseTheGeneratedDecoder() throws {
    var root = URL(fileURLWithPath: #filePath)
    for _ in 0..<5 { root.deleteLastPathComponent() }
    let source = try Data(contentsOf: root.appendingPathComponent("shared/schema/fixtures/local-contracts.json"))
    let fixtures = try XCTUnwrap(JSONSerialization.jsonObject(with: source) as? [[String: Any]])
    for fixture in fixtures {
      let name = try XCTUnwrap(fixture["name"] as? String)
      let contract = try XCTUnwrap(fixture["contract"] as? String)
      let valid = try XCTUnwrap(fixture["valid"] as? Bool)
      let data = try JSONSerialization.data(withJSONObject: XCTUnwrap(fixture["value"]), options: .fragmentsAllowed)
      if valid { XCTAssertNoThrow(try validateGeneratedFixture(contract, data: data), name) }
      else { XCTAssertThrowsError(try validateGeneratedFixture(contract, data: data), name) }
    }
  }

  func testSwiftRetainsFullUInt64ButRejectsFractionalAndBooleanRevisions() throws {
    let prefix = #"{"history_version":0,"progress":null,"sync_warning":null,"notifications":[],"revision":"#
    let data = Data((prefix + "18446744073709551615}").utf8)
    XCTAssertEqual(try JSONDecoder().decode(ContractWindowsRuntimeSnapshot.self, from: data).revision, UInt64.max)
    for invalid in ["true", "1.5", "-1", "18446744073709551616"] {
      XCTAssertThrowsError(try JSONDecoder().decode(ContractWindowsRuntimeSnapshot.self, from: Data((prefix + invalid + "}").utf8)))
    }
  }

  func testUnknownStableErrorCodeMapsToInternalError() throws {
    let data = Data(#"{"schema_version":1,"code":"future_error","retryable":true,"message_key":"future.error","detail_class":"internal"}"#.utf8)
    let decoded = try JSONDecoder().decode(ContractStableErrorEnvelope.self, from: data)
    XCTAssertEqual(decoded.code, .internal_error)
    XCTAssertFalse(decoded.retryable)
    XCTAssertEqual(decoded.message_key, "error.internal")
    XCTAssertEqual(decoded.detail_class, .internal)
  }

}
