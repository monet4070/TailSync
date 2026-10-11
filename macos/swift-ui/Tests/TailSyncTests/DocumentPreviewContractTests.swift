import Foundation
import XCTest
@testable import TailSync

final class DocumentPreviewContractTests: XCTestCase {
  private func model() -> [String: Any] {
    ["version": 1, "blocks": [["kind": "paragraph", "paragraph": ["runs": [["text": "中文😀", "style": ["bold": true, "italic": false, "underline": false, "strikethrough": false, "subscript": false, "superscript": false], "unsupported": false]]]]], "assets": [], "partial": [], "styles_partial": false]
  }

  private func frame(_ document: [String: Any], owner: String = "history", generation: UInt64 = 3) throws -> Data {
    let payload = try JSONSerialization.data(withJSONObject: document)
    let metadata: [String: Any] = ["request_id": "current", "entry_id": 7, "kind": "document", "name": "document", "stamp": "dc1:" + String(repeating: "0a", count: 32), "owner": owner, "generation": generation, "model_bytes": payload.count, "size_bytes": payload.count]
    let header = try JSONSerialization.data(withJSONObject: metadata)
    var output = Data("TSDC".utf8)
    output.append(1)
    let length = UInt32(header.count)
    for shift in [0, 8, 16, 24] { output.append(UInt8((length >> shift) & 255)) }
    output.append(header)
    output.append(payload)
    return output
  }

  func testControlledDocumentSurvivesBinaryTransportModelSplit() throws {
    let bytes = try frame(model())
    let preview = try ApiClient.decodeDocumentPreview(bytes, expectedRequestID: "current", expectedEntryId: 7, expectedOwner: "history", expectedGeneration: 3)
    XCTAssertEqual(preview.document.blocks[0].paragraph?.runs[0].text, "中文😀")
    XCTAssertTrue(preview.assets.isEmpty)
    let header = try XCTUnwrap(ApiClient.documentFrameHeader(bytes))
    let control = try JSONSerialization.jsonObject(with: bytes[9..<header.0]) as? [String: Any]
    XCTAssertNil(control?["document"])
  }

  func testOwnerGenerationRequestAndEntryMismatchFailClosed() throws {
    let bytes = try frame(model())
    XCTAssertThrowsError(try ApiClient.decodeDocumentPreview(bytes, expectedOwner: "favorites"))
    XCTAssertThrowsError(try ApiClient.decodeDocumentPreview(bytes, expectedGeneration: 4))
    XCTAssertThrowsError(try ApiClient.decodeDocumentPreview(bytes, expectedEntryId: 8))
    XCTAssertThrowsError(try ApiClient.decodeDocumentPreview(bytes, expectedRequestID: "late"))
  }

  func testUnknownModelVersionAndInvalidUnionFailClosed() throws {
    var document = model()
    document["version"] = 2
    XCTAssertThrowsError(try ApiClient.decodeDocumentPreview(frame(document)))
    document = model()
    document["blocks"] = [["kind": "image", "paragraph": ["runs": []]]]
    XCTAssertThrowsError(try ApiClient.decodeDocumentPreview(frame(document)))
  }

  func testUTF8ByteAndBlockCapsUseGeneratedProductionDecoder() throws {
    var document = model()
    let style: [String: Any] = ["bold": false, "italic": false, "underline": false, "strikethrough": false, "subscript": false, "superscript": false]
    document["blocks"] = [["kind": "paragraph", "paragraph": ["runs": [["text": String(repeating: "中", count: 400000), "style": style, "unsupported": false]]]]]
    XCTAssertThrowsError(try ApiClient.decodeDocumentPreview(frame(document)))
    document["blocks"] = Array(repeating: ["kind": "paragraph", "paragraph": ["runs": []]] as [String: Any], count: 8193)
    XCTAssertThrowsError(try ApiClient.decodeDocumentPreview(frame(document)))
  }

  func testUnclaimedAssetsAndTrailingBytesFailClosed() throws {
    var document = model()
    document["assets"] = [["index": 0, "offset": 0, "length": 3, "width": 1, "height": 1, "sha256": String(repeating: "00", count: 32)]]
    XCTAssertThrowsError(try ApiClient.decodeDocumentPreview(frame(document)))
    var bytes = try frame(model())
    bytes.append(0)
    XCTAssertThrowsError(try ApiClient.decodeDocumentPreview(bytes))
  }
}
