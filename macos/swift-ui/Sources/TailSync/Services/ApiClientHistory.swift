import Foundation

extension ApiClient {
  struct ImageData {
    let width: Int
    let height: Int
    let rgba: Data
  }

  func getImageData(id: Int64) async -> ImageData? {
    guard let response = try? await request(["cmd": "get_image_data", "id": id]),
      response["ok"] as? Bool == true,
      let data = response["data"] as? [String: Any],
      let width = (data["width"] as? NSNumber)?.intValue,
      let height = (data["height"] as? NSNumber)?.intValue,
      let encoded = data["rgba_b64"] as? String,
      let rgba = Data(base64Encoded: encoded)
    else { return nil }
    return ImageData(width: width, height: height, rgba: rgba)
  }

  /// Decrypted history bytes for Quick Look/text preview.
  ///
  /// The daemon wraps the bytes in JSON/base64, so the response limit must
  /// account for base64's ~4/3 expansion over the shared 64 MiB payload cap.
  /// This command is deliberately given a longer socket timeout than normal
  /// metadata calls, while every other request retains the 3-second limit.
  func getPreviewData(id: Int64, batchId: String? = nil) async throws -> HistoryPreviewData {
    var command: [String: Any] = ["cmd": "get_preview_data", "id": id]
    if let batchId, !batchId.isEmpty {
      command["batch_id"] = batchId
    }
    let response = try await request(
      command,
      timeoutSeconds: 30,
      maxResponseBytes: 96 * 1024 * 1024
    )
    guard response["ok"] as? Bool == true else {
      throw Self.previewError(
        from: response,
        fallback: "Could not load history preview"
      )
    }
    guard let data = response["data"] as? [String: Any],
      let kind = data["kind"] as? String,
      ["image", "text", "file"].contains(kind),
      let name = data["name"] as? String,
      !name.isEmpty,
      let sizeNumber = data["size_bytes"] as? NSNumber,
      let encoded = data["data_b64"] as? String
    else {
      throw ApiError.serverError(response["error"] as? String ?? "Invalid history preview response")
    }

    let sizeBytes = sizeNumber.int64Value
    guard sizeBytes >= 0, sizeBytes <= HistoryPreviewData.maxBytes else {
      throw HistoryPreviewStoreError.tooLarge
    }
    // Reject an impossible Base64 envelope before allocating its decoded
    // buffer. Four encoded bytes represent at most three payload bytes.
    let maximumEncoded = Int((HistoryPreviewData.maxBytes + 2) / 3 * 4 + 4)
    guard encoded.utf8.count <= maximumEncoded else {
      throw HistoryPreviewStoreError.tooLarge
    }
    guard let bytes = Data(base64Encoded: encoded),
      Int64(bytes.count) <= HistoryPreviewData.maxBytes,
      Int64(bytes.count) == sizeBytes
    else {
      throw ApiError.serverError("Invalid history preview data")
    }
    return HistoryPreviewData(
      kind: kind,
      name: name,
      sizeBytes: sizeBytes,
      data: bytes,
      entryId: (data["entry_id"] as? NSNumber)?.int64Value,
      batch: Self.decodePreviewBatch(data["batch"])
    )
  }

  private static func decodePreviewBatch(_ value: Any?) -> HistoryPreviewBatchNavigation? {
    guard let batch = value as? [String: Any],
      let batchId = batch["batch_id"] as? String,
      let itemIndex = (batch["item_index"] as? NSNumber)?.intValue,
      let itemCount = (batch["item_count"] as? NSNumber)?.intValue,
      let firstEntryId = (batch["first_entry_id"] as? NSNumber)?.int64Value,
      let lastEntryId = (batch["last_entry_id"] as? NSNumber)?.int64Value
    else {
      return nil
    }
    return HistoryPreviewBatchNavigation(
      batchId: batchId,
      itemIndex: itemIndex,
      itemCount: itemCount,
      firstEntryId: firstEntryId,
      lastEntryId: lastEntryId,
      previousEntryId: (batch["previous_entry_id"] as? NSNumber)?.int64Value,
      nextEntryId: (batch["next_entry_id"] as? NSNumber)?.int64Value
    )
  }

  private static func previewError(
    from response: [String: Any],
    fallback: String
  ) -> HistoryPreviewRemoteError {
    let responseData = response["data"] as? [String: Any]
    let rawError = response["error"] as? String
    let encodedFailure: [String: Any]? =
      rawError
      .flatMap { $0.data(using: .utf8) }
      .flatMap { try? JSONSerialization.jsonObject(with: $0) as? [String: Any] }
    let codeValue =
      encodedFailure?["code"] as? String
      ?? responseData?["error_code"] as? String
      ?? response["error_code"] as? String
    let message = encodedFailure?["message"] as? String ?? rawError ?? fallback
    return HistoryPreviewRemoteError(
      code: codeValue.flatMap(HistoryPreviewRemoteErrorCode.init(rawValue:)),
      message: message,
      retryable: encodedFailure?["retryable"] as? Bool
    )
  }

  struct FileProgress: Equatable {
    let batchId: String
    let name: String
    let sent: UInt64
    let total: UInt64
    let active: Bool
    let direction: String
    let device: String
    let completedFiles: Int
    let totalFiles: Int
    let speedBytesPerSecond: UInt64
    let canStop: Bool
  }

  func getFileProgress() async -> FileProgress? {
    guard let response = try? await request(["cmd": "get_file_progress"]),
      response["ok"] as? Bool == true,
      let data = response["data"] as? [String: Any]
    else { return nil }
    return Self.decodeFileProgress(data)
  }

  static func decodeFileProgress(_ value: Any?) -> FileProgress? {
    guard let data = value as? [String: Any],
      let name = data["name"] as? String,
      let sent = (data["sent"] as? NSNumber)?.uint64Value,
      let total = (data["total"] as? NSNumber)?.uint64Value,
      let active = data["active"] as? Bool
    else { return nil }
    return FileProgress(
      batchId: data["batch_id"] as? String ?? "",
      name: name,
      sent: sent,
      total: total,
      active: active,
      direction: data["direction"] as? String ?? "receiving",
      device: data["device"] as? String ?? "",
      completedFiles: (data["completed_files"] as? NSNumber)?.intValue ?? 0,
      totalFiles: (data["total_files"] as? NSNumber)?.intValue ?? 1,
      speedBytesPerSecond: (data["speed_bytes_per_second"] as? NSNumber)?.uint64Value ?? 0,
      canStop: data["can_stop"] as? Bool ?? false
    )
  }

  func cancelFileBatch(_ batchId: String) async {
    _ = try? await request(["cmd": "cancel_file_batch", "batch_id": batchId])
  }

  func restoreFileBatch(_ batchId: String) async throws {
    let response = try await request(["cmd": "restore_file_batch", "batch_id": batchId])
    guard response["ok"] as? Bool == true else {
      throw ApiError.serverError(response["error"] as? String ?? "unknown")
    }
  }

  func setHistoryPinned(id: Int64, pinned: Bool) async throws {
    let response = try await request(["cmd": "set_history_pinned", "id": id, "pinned": pinned])
    guard response["ok"] as? Bool == true else {
      throw ApiError.serverError(response["error"] as? String ?? "unknown")
    }
  }

  func setHistoryFavorite(id: Int64, favorite: Bool) async throws {
    let response = try await request(["cmd": "set_history_favorite", "id": id, "favorite": favorite])
    guard response["ok"] as? Bool == true else {
      throw ApiError.serverError(response["error"] as? String ?? "unknown")
    }
  }

  func deleteFavoriteEntry(id: Int64) async throws {
    let response = try await request(["cmd": "delete_favorite_entry", "id": id])
    guard response["ok"] as? Bool == true else {
      throw ApiError.serverError(response["error"] as? String ?? "unknown")
    }
  }

  func getHistory(
    keyword: String? = nil,
    category: String? = nil,
    startTime: String? = nil,
    endTime: String? = nil,
    limit: Int = 30,
    offset: Int = 0,
    collection: String = "all"
  ) async throws -> [HistoryEntry] {
    var request: [String: Any] = ["cmd": "get_history", "limit": limit, "offset": offset]
    if collection != "all" { request["collection"] = collection }
    if let keyword { request["keyword"] = keyword }
    if let category { request["category"] = category }
    if let startTime { request["start_time"] = startTime }
    if let endTime { request["end_time"] = endTime }
    let response = try await self.request(request)
    guard response["ok"] as? Bool == true,
      let data = response["data"] as? [[String: Any]]
    else {
      throw ApiError.serverError(response["error"] as? String ?? "unknown")
    }
    return data.compactMap { item in
      guard let data = try? JSONSerialization.data(withJSONObject: item) else { return nil }
      return try? JSONDecoder().decode(HistoryEntry.self, from: data)
    }
  }

  func deleteEntry(id: Int64) async throws {
    let response = try await request(["cmd": "delete_entry", "id": id])
    guard response["ok"] as? Bool == true else {
      throw ApiError.serverError(response["error"] as? String ?? "unknown")
    }
  }

  func restoreEntry(id: Int64) async throws {
    let response = try await request(["cmd": "restore_entry", "id": id])
    guard response["ok"] as? Bool == true else {
      throw ApiError.serverError(response["error"] as? String ?? "unknown")
    }
  }
}
