import Foundation

extension ApiClient {
  struct ImageData {
    let width: Int
    let height: Int
    let rgba: Data
    let cacheKey: String?
  }

  func getImageData(id: Int64, cacheKey: String? = nil) async -> ImageData? {
    var command: [String: Any] = ["cmd": "get_image_data", "id": id]
    if let cacheKey { command["cache_key"] = cacheKey }
    var received: [String: Any]?
    for attempt in 0..<16 {
      guard !Task.isCancelled else { return nil }
      guard let response = try? await request(command, timeoutSeconds: 30) else { return nil }
      let retryable = response["error_code"] as? String == "temporarily_busy"
        || ["content analysis busy", "content analysis is stale", "history image is stale"].contains(response["error"] as? String ?? "")
      if response["ok"] as? Bool != true, retryable, attempt < 15 {
        do { try await Task.sleep(nanoseconds: UInt64(min(400, 50 * (attempt + 1))) * 1_000_000) } catch { return nil }
        continue
      }
      received = response
      break
    }
    guard !Task.isCancelled, let response = received,
      response["ok"] as? Bool == true,
      let data = response["data"] as? [String: Any],
      let widthNumber = data["width"] as? NSNumber,
      let heightNumber = data["height"] as? NSNumber,
      CFGetTypeID(widthNumber) != CFBooleanGetTypeID(),
      CFGetTypeID(heightNumber) != CFBooleanGetTypeID(),
      widthNumber.doubleValue.isFinite, heightNumber.doubleValue.isFinite,
      widthNumber.doubleValue > 0, heightNumber.doubleValue > 0,
      widthNumber.doubleValue <= 256, heightNumber.doubleValue <= 256,
      widthNumber.doubleValue.rounded(.down) == widthNumber.doubleValue,
      heightNumber.doubleValue.rounded(.down) == heightNumber.doubleValue,
      let encoded = data["rgba_b64"] as? String,
      encoded.count <= 350_000,
      let rgba = Data(base64Encoded: encoded)
    else { return nil }
    let width = widthNumber.intValue
    let height = heightNumber.intValue
    let returnedKey = data["cache_key"] as? String
    guard rgba.count == width * height * 4, cacheKey == nil || cacheKey == returnedKey else { return nil }
    return ImageData(width: width, height: height, rgba: rgba, cacheKey: returnedKey)
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

    // The binary path is negotiated explicitly. A malformed, oversized, or
    // unauthorized binary response is surfaced to the caller; it is never
    // treated as a reason to retry through the legacy path.
    if let capabilities = try await getLocalCapabilities(), capabilities.supportsBinaryPreview {
      var binaryCommand = command
      binaryCommand["cmd"] = "get_preview_binary"
      let requestID = UUID().uuidString
      binaryCommand["request_id"] = requestID
      let binary = try await requestBytes(
        binaryCommand,
        timeoutSeconds: 30,
        maxResponseBytes: Int(HistoryPreviewData.maxBytes) + 1024 * 1024 + 9
      )
      if binary.first == 0x7B {
        let line = binary.firstIndex(of: 0x0A).map { Data(binary[..<$0]) } ?? binary
        guard let response = try? JSONSerialization.jsonObject(with: line) as? [String: Any] else {
          throw ApiError.invalidJson
        }
        throw Self.previewError(from: response, fallback: "Could not load history preview")
      }
      return try Self.decodeBinaryPreview(binary, expectedRequestID: requestID)
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

  func getDocumentPreview(
    id: Int64, owner: String, generation: UInt64, requestId: String? = nil
  ) async throws -> HistoryDocumentPreview? {
    guard let capabilities = try await getLocalCapabilities(), capabilities.supportsDocumentPreview
    else { return nil }
    let requestID = requestId ?? UUID().uuidString
    let command: [String: Any] = [
      "cmd": "get_document_preview",
      "id": id,
      "request_id": requestID,
      "document_request": ["entry_id": id, "owner": owner, "generation": generation, "request_id": requestID],
    ]
    let binary = try await requestBytes(
      command,
      timeoutSeconds: 30,
      maxResponseBytes: 16 * 1024 * 1024
    )
    if binary.first == 0x7B {
      let line = binary.firstIndex(of: 0x0A).map { Data(binary[..<$0]) } ?? binary
      guard let response = try? JSONSerialization.jsonObject(with: line) as? [String: Any] else {
        throw ApiError.invalidJson
      }
      throw Self.previewError(from: response, fallback: "Could not load document preview")
    }
    try Task.checkCancellation()
    return try Self.decodeDocumentPreview(binary, expectedRequestID: requestID, expectedEntryId: id, expectedOwner: owner, expectedGeneration: generation)
  }

  func validateDocumentPreview(_ preview: HistoryDocumentPreview) async throws -> Bool {
    try await validateDocumentPreview(id: preview.entryId, owner: preview.owner, generation: preview.generation, requestId: preview.requestId, stamp: preview.stamp)
  }

  func validateDocumentPreview(id: Int64, owner: String, generation: UInt64, requestId: String, stamp: String) async throws -> Bool {
    let response = try await request(["cmd": "validate_document_preview", "document_request": ["entry_id": id, "owner": owner, "generation": generation, "request_id": requestId], "document_stamp": stamp])
    try Task.checkCancellation()
    return response["ok"] as? Bool == true && (response["data"] as? [String: Any])?["valid"] as? Bool == true
  }

  func cancelDocumentPreview(id: Int64, owner: String, generation: UInt64, requestId: String) async {
    _ = try? await request(["cmd": "cancel_document_preview", "document_request": ["entry_id": id, "owner": owner, "generation": generation, "request_id": requestId]])
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
    let capabilities = try await getLocalCapabilities()
    let paged = capabilities?.history_display_version == 1
    var request: [String: Any] = ["cmd": paged ? "get_history_page" : "get_history", "limit": limit, "offset": offset]
    if collection != "all" { request["collection"] = collection }
    if let keyword { request["keyword"] = keyword }
    if let category { request["category"] = category }
    if let startTime { request["start_time"] = startTime }
    if let endTime { request["end_time"] = endTime }
    for _ in 0..<256 {
      try Task.checkCancellation()
      let response = try await self.request(request, timeoutSeconds: 30)
      try Task.checkCancellation()
      guard response["ok"] as? Bool == true, let payload = response["data"] else {
        throw ApiError.serverError(response["error"] as? String ?? "unknown")
      }
      if paged {
        let encoded = try JSONSerialization.data(withJSONObject: payload)
        let page = try JSONDecoder().decode(ContractHistoryQueryPage.self, from: encoded)
        if page.display_index_pending == true {
          try await Task.sleep(nanoseconds: 20_000_000)
          continue
        }
        guard let object = payload as? [String: Any], let rows = object["entries"] else { throw ApiError.invalidJson }
        return try JSONDecoder().decode([HistoryEntry].self, from: JSONSerialization.data(withJSONObject: rows))
      }
      let encoded = try JSONSerialization.data(withJSONObject: payload)
      if capabilities != nil { _ = try JSONDecoder().decode([ContractHistoryEntry].self, from: encoded) }
      return try JSONDecoder().decode([HistoryEntry].self, from: encoded)
    }
    throw ApiError.serverError("History display index is still rebuilding; retry to continue")
  }

  func deleteEntry(id: Int64) async throws {
    let response = try await request(["cmd": "delete_entry", "id": id])
    guard response["ok"] as? Bool == true else {
      throw ApiError.serverError(response["error"] as? String ?? "unknown")
    }
  }

  func restoreEntry(id: Int64, format: String? = nil, originalID: Int64? = nil, identity: ContractHistoryIdentity? = nil) async throws {
    _ = try await restoreEntryWithReport(id: id, format: format, originalID: originalID, identity: identity)
  }

  func restoreEntryWithReport(id: Int64, format: String? = nil, originalID: Int64? = nil, identity: ContractHistoryIdentity? = nil) async throws -> ContractHistoryRestoreReport {
    var command: [String: Any] = ["cmd": "restore_entry", "id": id]
    if let format { command["restore_format"] = format }
    if let identity {
      command["restore_original_id"] = originalID ?? id
      command["restore_identity"] = try JSONSerialization.jsonObject(with: JSONEncoder().encode(identity))
    }
    let response = try await request(command)
    guard response["ok"] as? Bool == true else {
      throw ApiError.serverError(response["error"] as? String ?? "unknown")
    }
    guard let payload = response["data"], !(payload is NSNull) else {
      return try JSONDecoder().decode(ContractHistoryRestoreReport.self, from: Data(#"{"complete":true,"missing_images":0}"#.utf8))
    }
    return try JSONDecoder().decode(ContractHistoryRestoreReport.self, from: JSONSerialization.data(withJSONObject: payload))
  }

  @discardableResult
  func archiveEntryImages(id: Int64) async throws -> ContractArchiveEntryImagesReport {
    let response = try await request(["cmd": "archive_entry_images", "id": id], timeoutSeconds: 60)
    guard response["ok"] as? Bool == true else {
      throw ApiError.serverError(response["error"] as? String ?? "unknown")
    }
    let data = try JSONSerialization.data(withJSONObject: response["data"] ?? [:])
    return try JSONDecoder().decode(ContractArchiveEntryImagesReport.self, from: data)
  }

  @discardableResult
  func sourceContextGrantCreate(id: Int64) async throws -> ContractSourceContextGrantReport {
    let response = try await request(["cmd": "source_context_grant_create", "id": id])
    guard response["ok"] as? Bool == true else {
      throw ApiError.serverError(response["error"] as? String ?? "unknown")
    }
    let data = try JSONSerialization.data(withJSONObject: response["data"] ?? [:])
    return try JSONDecoder().decode(ContractSourceContextGrantReport.self, from: data)
  }

  @discardableResult
  func sourceContextGrantRevoke(grantId: String) async throws -> ContractSourceContextRevokeReport {
    let response = try await request(["cmd": "source_context_grant_revoke", "grant_id": grantId])
    guard response["ok"] as? Bool == true else {
      throw ApiError.serverError(response["error"] as? String ?? "unknown")
    }
    let data = try JSONSerialization.data(withJSONObject: response["data"] ?? [:])
    return try JSONDecoder().decode(ContractSourceContextRevokeReport.self, from: data)
  }

  @discardableResult
  func sourceContextImport(id: Int64, path: String) async throws -> ContractSourceContextImportReport {
    let response = try await request(["cmd": "source_context_import", "id": id, "path": path], timeoutSeconds: 60)
    guard response["ok"] as? Bool == true else {
      throw ApiError.serverError(response["error"] as? String ?? "unknown")
    }
    let data = try JSONSerialization.data(withJSONObject: response["data"] ?? [:])
    return try JSONDecoder().decode(ContractSourceContextImportReport.self, from: data)
  }
}
