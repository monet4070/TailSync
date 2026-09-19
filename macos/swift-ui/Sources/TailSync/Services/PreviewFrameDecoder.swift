import Foundation

// Typed decoding rejects Boolean/fractional/negative values in integer fields.
struct PreviewFrameMetadata: Decodable {
  struct Batch: Decodable {
    let batch_id: String
    let item_index: Int
    let item_count: Int
    let first_entry_id: Int64
    let last_entry_id: Int64
    let previous_entry_id: Int64?
    let next_entry_id: Int64?

    func validated() throws -> HistoryPreviewBatchNavigation {
      guard !batch_id.isEmpty, item_count > 0, item_index >= 0, item_index < item_count,
        first_entry_id > 0, last_entry_id > 0,
        previous_entry_id.map({ $0 > 0 }) ?? true,
        next_entry_id.map({ $0 > 0 }) ?? true,
        (item_index == 0) == (previous_entry_id == nil),
        (item_index == item_count - 1) == (next_entry_id == nil)
      else { throw ApiError.serverError("Invalid binary preview batch navigation") }
      return HistoryPreviewBatchNavigation(
        batchId: batch_id, itemIndex: item_index, itemCount: item_count,
        firstEntryId: first_entry_id, lastEntryId: last_entry_id,
        previousEntryId: previous_entry_id, nextEntryId: next_entry_id
      )
    }
  }

  let entry_id: Int64
  let request_id: String?
  let kind: String
  let name: String
  let size_bytes: UInt64
  let width: UInt32?
  let height: UInt32?
  let batch: Batch?

  func validate() throws {
    guard entry_id > 0, ["text", "image", "file"].contains(kind), !name.isEmpty,
      size_bytes <= UInt64(HistoryPreviewData.maxBytes)
    else { throw ApiError.serverError("Invalid binary preview metadata") }
    if kind == "image" {
      guard let width, let height, width > 0, height > 0,
        UInt64(width) * UInt64(height) <= UInt64(HistoryPreviewData.maxBytes) / 4,
        UInt64(width) * UInt64(height) * 4 == size_bytes
      else { throw ApiError.serverError("Invalid binary preview image dimensions") }
    } else if width != nil || height != nil {
      throw ApiError.serverError("Non-image preview contains image dimensions")
    }
    _ = try batch?.validated()
  }
}

extension ApiClient {
  /// Returns nil until enough bytes arrive to validate metadata. A JSON error
  /// is separately bounded by the transport and never enables legacy fallback.
  static func previewFrameHeader(_ bytes: Data) throws -> (Int, PreviewFrameMetadata)? {
    guard bytes.count >= 9 else { return nil }
    guard bytes.prefix(4) == Data("TSPV".utf8), bytes[4] == 1 else {
      throw ApiError.serverError("Invalid binary preview header or version")
    }
    let length = UInt32(bytes[5]) | UInt32(bytes[6]) << 8
      | UInt32(bytes[7]) << 16 | UInt32(bytes[8]) << 24
    guard length > 0, length <= 1024 * 1024 else {
      throw ApiError.serverError("Invalid binary preview metadata length")
    }
    let end = 9 + Int(length)
    guard bytes.count >= end else { return nil }
    let metadata = try JSONDecoder().decode(PreviewFrameMetadata.self, from: bytes[9..<end])
    try metadata.validate()
    return (end, metadata)
  }

  static func decodeBinaryPreview(_ frame: Data, expectedRequestID: String? = nil) throws -> HistoryPreviewData {
    guard let (metadataEnd, metadata) = try previewFrameHeader(frame),
      frame.count == metadataEnd + Int(metadata.size_bytes),
      expectedRequestID == nil || metadata.request_id == expectedRequestID
    else { throw ApiError.serverError("Invalid binary preview length or request identity") }
    return HistoryPreviewData(
      kind: metadata.kind, name: metadata.name, sizeBytes: Int64(metadata.size_bytes),
      data: Data(frame[metadataEnd...]), entryId: metadata.entry_id,
      batch: try metadata.batch?.validated(),
      imageWidth: metadata.width.map(Int.init), imageHeight: metadata.height.map(Int.init)
    )
  }
}
