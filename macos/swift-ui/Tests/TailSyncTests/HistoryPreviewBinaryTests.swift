import Foundation
import XCTest
@testable import TailSync

final class HistoryPreviewBinaryTests: XCTestCase {
  func testRejectsMalformedMetadataBeforeReceivingPayload() throws {
    let base: [String: Any] = ["entry_id": 7, "kind": "text", "name": "text.txt", "size_bytes": 5]
    for (key, value) in [("entry_id", true as Any), ("size_bytes", 1.5),
      ("size_bytes", -1), ("size_bytes", 67108865), ("kind", "unknown"),
      ("batch", ["batch_id": "incomplete"])] {
      var metadata = base
      metadata[key] = value
      let bytes = try JSONSerialization.data(withJSONObject: metadata)
      var frame = Data([0x54, 0x53, 0x50, 0x56, 1])
      frame.append(contentsOf: UInt32(bytes.count).littleEndianBytes)
      frame.append(bytes)
      XCTAssertThrowsError(try ApiClient.previewFrameHeader(frame), "\(key)=\(value)")
    }
    XCTAssertFalse(ApiClient.isUnsupportedCapabilitiesResponse("unauthorized: unknown command: get_local_capabilities"))
  }

  func testRequestIdentityAndTrailingBytesAreRejected() throws {
    let metadata = Data(#"{"entry_id":7,"request_id":"current","kind":"text","name":"t","size_bytes":0}"#.utf8)
    var frame = Data([0x54, 0x53, 0x50, 0x56, 1])
    frame.append(contentsOf: UInt32(metadata.count).littleEndianBytes)
    frame.append(metadata)
    XCTAssertNoThrow(try ApiClient.decodeBinaryPreview(frame, expectedRequestID: "current"))
    XCTAssertThrowsError(try ApiClient.decodeBinaryPreview(frame, expectedRequestID: "stale"))
    frame.append(0)
    XCTAssertThrowsError(try ApiClient.decodeBinaryPreview(frame))
  }

  func testLegacyPreviewFallbackOnlyAcceptsAnExplicitUnknownCommand() {
    XCTAssertTrue(
      ApiClient.isUnsupportedCapabilitiesResponse("unknown command: get_local_capabilities")
    )
    XCTAssertFalse(
      ApiClient.isUnsupportedCapabilitiesResponse("invalid capability token")
    )
    XCTAssertFalse(
      ApiClient.isUnsupportedCapabilitiesResponse("Invalid local capabilities response")
    )
  }

  func testDecodesAValidTextFrame() throws {
    let payload = Data("hello".utf8)
    let metadata: [String: Any] = [
      "entry_id": 7,
      "kind": "text",
      "name": "text.txt",
      "size_bytes": payload.count,
      "width": NSNull(),
      "height": NSNull(),
      "batch": NSNull(),
    ]
    let metadataBytes = try JSONSerialization.data(withJSONObject: metadata)
    var frame = Data([0x54, 0x53, 0x50, 0x56, 1])
    frame.append(contentsOf: UInt32(metadataBytes.count).littleEndianBytes)
    frame.append(metadataBytes)
    frame.append(payload)

    let decoded = try ApiClient.decodeBinaryPreview(frame)
    XCTAssertEqual(decoded.kind, "text")
    XCTAssertEqual(decoded.name, "text.txt")
    XCTAssertEqual(decoded.sizeBytes, 5)
    XCTAssertEqual(decoded.data, payload)
    XCTAssertEqual(decoded.entryId, 7)
  }

  func testRejectsAFrameWithMismatchedPayloadLength() throws {
    let metadata: [String: Any] = [
      "entry_id": 7,
      "kind": "text",
      "name": "text.txt",
      "size_bytes": 5,
      "width": NSNull(),
      "height": NSNull(),
      "batch": NSNull(),
    ]
    let metadataBytes = try JSONSerialization.data(withJSONObject: metadata)
    var frame = Data([0x54, 0x53, 0x50, 0x56, 1])
    frame.append(contentsOf: UInt32(metadataBytes.count).littleEndianBytes)
    frame.append(metadataBytes)
    frame.append(Data("no".utf8))

    XCTAssertThrowsError(try ApiClient.decodeBinaryPreview(frame))
  }

  func testDecodesRawRgbaDimensionsForAnImageFrame() throws {
    let payload = Data([1, 2, 3, 4, 5, 6, 7, 8])
    let metadata: [String: Any] = [
      "entry_id": 8,
      "kind": "image",
      "name": "image",
      "size_bytes": payload.count,
      "width": 2,
      "height": 1,
      "batch": NSNull(),
    ]
    let metadataBytes = try JSONSerialization.data(withJSONObject: metadata)
    var frame = Data([0x54, 0x53, 0x50, 0x56, 1])
    frame.append(contentsOf: UInt32(metadataBytes.count).littleEndianBytes)
    frame.append(metadataBytes)
    frame.append(payload)

    let decoded = try ApiClient.decodeBinaryPreview(frame)
    XCTAssertEqual(decoded.imageWidth, 2)
    XCTAssertEqual(decoded.imageHeight, 1)
    XCTAssertEqual(decoded.data, payload)
  }
}

private extension UInt32 {
  var littleEndianBytes: [UInt8] {
    withUnsafeBytes(of: littleEndian) { Array($0) }
  }
}
