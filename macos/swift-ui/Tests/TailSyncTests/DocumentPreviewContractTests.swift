import Foundation
import XCTest
@testable import TailSync

final class DocumentPreviewContractTests: XCTestCase {
  private func model() -> [String: Any] {
    ["version": 1, "blocks": [["kind": "paragraph", "paragraph": ["runs": [["text": "中文😀", "style": ["bold": true, "italic": false, "underline": false, "strikethrough": false, "subscript": false, "superscript": false], "unsupported": false]]]]], "assets": [], "partial": [], "styles_partial": false]
  }

  private func frame(_ document: [String: Any], owner: String = "history", generation: UInt64 = 3, assets: Data = Data()) throws -> Data {
    let payload = try JSONSerialization.data(withJSONObject: document)
    let metadata: [String: Any] = ["request_id": "current", "entry_id": 7, "kind": "document", "name": "document", "stamp": "dc1:" + String(repeating: "0a", count: 32), "owner": owner, "generation": generation, "model_bytes": payload.count, "size_bytes": payload.count + assets.count]
    let header = try JSONSerialization.data(withJSONObject: metadata)
    var output = Data("TSDC".utf8)
    output.append(1)
    let length = UInt32(header.count)
    for shift in [0, 8, 16, 24] { output.append(UInt8((length >> shift) & 255)) }
    output.append(header)
    output.append(payload)
    output.append(assets)
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

  func testFrameHeaderRejectsMetadataBeyondTheSharedBudget() throws {
    var bytes = Data("TSDC".utf8)
    bytes.append(1)
    let length = UInt32(ContractDocumentPolicies.maxMetadataBytes)
    for shift in [0, 8, 16, 24] { bytes.append(UInt8((length >> shift) & 255)) }
    XCTAssertNil(try ApiClient.documentFrameHeader(bytes))
    var oversized = bytes
    let overLength = length + 1
    for (index, shift) in [0, 8, 16, 24].enumerated() {
      oversized[5 + index] = UInt8((overLength >> shift) & 255)
    }
    XCTAssertThrowsError(try ApiClient.documentFrameHeader(oversized))
  }

  private func expandDocument(_ expand: [String: Any]) -> [String: Any] {
    func run(_ text: String, _ link: String?) -> [String: Any] {
      var value: [String: Any] = [
        "text": text,
        "style": ["bold": false, "italic": false, "underline": false, "strikethrough": false, "subscript": false, "superscript": false],
        "unsupported": false,
      ]
      if let link { value["link"] = link }
      return value
    }
    func paragraph(_ runs: [[String: Any]]) -> [String: Any] { ["kind": "paragraph", "paragraph": ["runs": runs]] }
    func document(_ blocks: [[String: Any]]) -> [String: Any] {
      ["version": 1, "blocks": blocks, "assets": [], "partial": [], "styles_partial": false]
    }
    let count = expand["count"] as? Int ?? 0
    let groups = expand["groups"] as? Int ?? 1
    let counts = (0..<groups).map { count / groups + ($0 < count % groups ? 1 : 0) }
    switch expand["kind"] as? String {
    case "paragraphs":
      return document((0..<count).map { _ in paragraph([run("x", nil)]) })
    case "runs":
      return document(counts.map { count in paragraph((0..<count).map { _ in run("x", nil) }) })
    case "cells":
      return document([[
        "kind": "table",
        "table": ["rows": counts.map { count in ["cells": (0..<count).map { _ in ["header": false, "runs": [run("x", nil)]] as [String: Any] }] }, "truncated": false],
      ]])
    case "text":
      let bytes = expand["bytes"] as? Int ?? 0
      let link = (expand["linkBytes"] as? Int).map { "https://example.com/" + String(repeating: "a", count: $0 - 20) }
      let character = expand["character"] as? String ?? "x"
      let runs = (0..<groups).map { index in
        let size = bytes / groups + (index < bytes % groups ? 1 : 0)
        let text = String(repeating: character, count: size / character.utf8.count) + String(repeating: "x", count: size % character.utf8.count)
        return run(text, index == 0 ? link : nil)
      }
      return document([paragraph(runs)])
    case "assets":
      let length = expand["length"] as? Int ?? 0
      let width = expand["width"] as? Int ?? 0
      let height = expand["height"] as? Int ?? 0
      var value = document((0..<count).map { index in
        ["kind": "image", "image": ["asset": index, "availability": "available", "ordinal": index]] as [String: Any]
      })
      value["assets"] = (0..<count).map { index in
        ["index": index, "offset": index * length, "length": length, "width": width, "height": height, "sha256": String(repeating: "0a", count: 32)] as [String: Any]
      }
      value["first_image"] = 0
      return value
    case "partials":
      let detailBytes = expand["detailBytes"] as? Int ?? 0
      var value = document([paragraph([run("x", nil)])])
      value["partial"] = (0..<count).map { _ in
        ["code": "other_native_items", "detail": String(repeating: "d", count: detailBytes)] as [String: Any]
      }
      return value
    default:
      fatalError("unknown document corpus expand kind")
    }
  }

  func testSharedDocumentCorpusMatchesTheGeneratedDecoder() throws {
    var root = URL(fileURLWithPath: #filePath)
    for _ in 0..<5 { root.deleteLastPathComponent() }
    let source = try Data(contentsOf: root.appendingPathComponent("shared/schema/fixtures/document-corpus.json"))
    let cases = try XCTUnwrap(JSONSerialization.jsonObject(with: source) as? [[String: Any]])
    XCTAssertGreaterThanOrEqual(cases.count, 100)
    XCTAssertEqual(Set(cases.compactMap { $0["name"] as? String }).count, cases.count)
    for corpusCase in cases {
      let name = try XCTUnwrap(corpusCase["name"] as? String)
      let expected = ((corpusCase["expect"] as? [String: Any])?["swift"] as? Bool) ?? (corpusCase["valid"] as? Bool)!
      let data: Data
      if let raw = corpusCase["json"] as? String {
        data = Data(raw.utf8)
      } else {
        let value = try XCTUnwrap(corpusCase["value"] ?? (corpusCase["expand"] as? [String: Any]).map(expandDocument))
        data = try JSONSerialization.data(withJSONObject: value)
      }
      if expected { XCTAssertNoThrow(try validateGeneratedFixture("DisplayDocument", data: data), name) }
      else { XCTAssertThrowsError(try validateGeneratedFixture("DisplayDocument", data: data), name) }
    }
  }

  func testUppercaseMetadataDigestCannotBypassAssetPayloadValidation() throws {
    let png = Data([137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1])
    let digest = DocumentFrameDigest.hex(png)
    var document: [String: Any] = [
      "version": 1,
      "blocks": [["kind": "image", "image": ["asset": 0, "availability": "available", "ordinal": 0]]],
      "assets": [["index": 0, "offset": 0, "length": png.count, "width": 1, "height": 1, "sha256": digest.uppercased()]],
      "first_image": 0,
      "partial": [],
      "styles_partial": false,
    ]
    let model = try JSONSerialization.data(withJSONObject: document)
    XCTAssertNoThrow(try JSONDecoder().decode(ContractDisplayDocument.self, from: model))
    XCTAssertThrowsError(try ApiClient.decodeDocumentPreview(frame(document, assets: png)))
    document["assets"] = [["index": 0, "offset": 0, "length": png.count, "width": 1, "height": 1, "sha256": digest]]
    let preview = try ApiClient.decodeDocumentPreview(frame(document, assets: png))
    XCTAssertEqual(preview.assetBytes(preview.document.assets[0]), png)
  }

  func testSharedFrameMetadataCorpusMatchesTheRealDecoder() throws {
    var root = URL(fileURLWithPath: #filePath)
    for _ in 0..<5 { root.deleteLastPathComponent() }
    let source = try Data(contentsOf: root.appendingPathComponent("shared/schema/fixtures/document-frame-corpus.json"))
    let corpus = try XCTUnwrap(JSONSerialization.jsonObject(with: source) as? [String: Any])
    let model = Data(try XCTUnwrap(corpus["model_json"] as? String).utf8)
    let cases = try XCTUnwrap(corpus["cases"] as? [[String: Any]])
    for item in cases {
      let name = try XCTUnwrap(item["name"] as? String)
      let valid = try XCTUnwrap(item["valid"] as? Bool)
      let metadata: Data
      if let raw = item["json"] as? String { metadata = Data(raw.utf8) }
      else { metadata = try JSONSerialization.data(withJSONObject: XCTUnwrap(item["value"])) }
      var frame = Data("TSDC".utf8)
      frame.append(1)
      let length = UInt32(metadata.count)
      for shift in [0, 8, 16, 24] { frame.append(UInt8((length >> shift) & 255)) }
      frame.append(metadata)
      frame.append(model)
      if valid { XCTAssertNoThrow(try ApiClient.decodeDocumentPreview(frame), name) }
      else { XCTAssertThrowsError(try ApiClient.decodeDocumentPreview(frame), name) }
    }
  }

}
