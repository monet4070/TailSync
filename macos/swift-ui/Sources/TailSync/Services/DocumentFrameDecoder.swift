import CryptoKit
import Foundation

extension ApiClient {
  struct HistoryDocumentPreview {
    let entryId: Int64
    let stamp: String
    let owner: String
    let generation: UInt64
    let requestId: String
    let document: ContractDisplayDocument
    let assets: Data

    func assetBytes(_ asset: ContractDocumentAsset) -> Data? {
      guard asset.index < UInt64(document.assets.count) else { return nil }
      let declared = document.assets[Int(asset.index)]
      guard declared.offset == asset.offset, declared.length == asset.length, declared.sha256 == asset.sha256, declared.width == asset.width, declared.height == asset.height else { return nil }
      guard asset.offset <= UInt64(Int.max), asset.length <= UInt64(Int.max) else { return nil }
      let start = Int(asset.offset)
      let length = Int(asset.length)
      guard length > 0, start >= 0, start <= assets.count, assets.count - start >= length else { return nil }
      let bytes = assets.subdata(in: start..<(start + length))
      guard bytes.count == length, DocumentFrameDigest.hex(bytes) == asset.sha256 else { return nil }
      guard bytes.count >= 24, bytes.prefix(16) == Data([137,80,78,71,13,10,26,10,0,0,0,13,73,72,68,82]),
        bytes[16..<20].reduce(UInt32(0), { ($0 << 8) | UInt32($1) }) == asset.width,
        bytes[20..<24].reduce(UInt32(0), { ($0 << 8) | UInt32($1) }) == asset.height else { return nil }
      return bytes
    }
  }

  static func documentFrameHeader(_ bytes: Data) throws -> (Int, ContractDocumentFrameMetadata)? {
    guard bytes.count >= 9 else { return nil }
    guard bytes.prefix(4) == Data("TSDC".utf8), bytes[4] == 1 else {
      throw ApiError.serverError("Invalid document frame header or version")
    }
    let length = UInt32(bytes[5]) | UInt32(bytes[6]) << 8
      | UInt32(bytes[7]) << 16 | UInt32(bytes[8]) << 24
    guard length > 0, UInt64(length) <= ContractDocumentPolicies.maxMetadataBytes else {
      throw ApiError.serverError("Invalid document frame metadata length")
    }
    let end = 9 + Int(length)
    guard bytes.count >= end else { return nil }
    let metadata = try JSONDecoder().decode(
      ContractDocumentFrameMetadata.self, from: bytes[9..<end])
    guard metadata.size_bytes <= ContractDocumentPolicies.maxBytes - UInt64(end) else {
      throw ApiError.serverError("Invalid document frame payload length")
    }
    return (end, metadata)
  }

  static func decodeDocumentPreview(
    _ frame: Data, expectedRequestID: String? = nil, expectedEntryId: Int64? = nil, expectedOwner: String? = nil, expectedGeneration: UInt64? = nil
  ) throws -> HistoryDocumentPreview {
    guard let (metadataEnd, metadata) = try documentFrameHeader(frame),
      frame.count - metadataEnd == Int(metadata.size_bytes),
      frame.count <= Int(ContractDocumentPolicies.maxBytes),
      expectedRequestID == nil || metadata.request_id == expectedRequestID,
      expectedEntryId == nil || metadata.entry_id == expectedEntryId,
      expectedOwner == nil || metadata.owner == expectedOwner,
      expectedGeneration == nil || metadata.generation == expectedGeneration
    else { throw ApiError.serverError("Invalid document frame length or request identity") }
    let modelEnd = metadataEnd + Int(metadata.model_bytes)
    let document = try JSONDecoder().decode(ContractDisplayDocument.self, from: frame[metadataEnd..<modelEnd])
    let preview = HistoryDocumentPreview(
      entryId: metadata.entry_id,
      stamp: metadata.stamp,
      owner: metadata.owner, generation: metadata.generation, requestId: metadata.request_id!,
      document: document,
      assets: Data(frame[modelEnd...])
    )
    var offset: UInt64 = 0
    for asset in document.assets { guard asset.offset == offset, preview.assetBytes(asset) != nil else { throw ApiError.serverError("Invalid document assets") }; offset += asset.length }
    guard offset == UInt64(preview.assets.count) else { throw ApiError.serverError("Unclaimed document assets") }
    return preview
  }
}

enum DocumentFrameDigest {
  static func hex(_ data: Data) -> String {
    SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
  }
}
