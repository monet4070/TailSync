import Foundation

enum ThemeResolvedPairDecoder {
  static func decode(
    themeId: String,
    highContrast: Bool,
    lightData: [String: Any],
    darkData: [String: Any]
  ) -> TailSyncThemeDefinition? {
    guard lightData["id"] as? String == themeId,
      darkData["id"] as? String == themeId,
      lightData["mode"] as? String == "light",
      darkData["mode"] as? String == "dark",
      lightData["highContrast"] as? Bool == highContrast,
      darkData["highContrast"] as? Bool == highContrast,
      let lightDigest = lightData["digest"] as? String,
      darkData["digest"] as? String == lightDigest,
      let lightTokens = lightData["tokens"] as? [String: Any],
      let darkTokens = darkData["tokens"] as? [String: Any],
      let lightSlots = assetSlots(lightData),
      let darkSlots = assetSlots(darkData),
      lightSlots == darkSlots
    else { return nil }
    return TailSyncThemeDefinition.resolvedV2(
      id: themeId,
      packageDigest: lightDigest,
      light: lightTokens,
      dark: darkTokens,
      assetSlots: lightSlots
    )
  }

  private static func assetSlots(
    _ data: [String: Any]
  ) -> [String: TailSyncThemeAssetDescriptor]? {
    let rawSlots = data["assetSlots"] as? [String: Any] ?? [:]
    var slots: [String: TailSyncThemeAssetDescriptor] = [:]
    for (key, raw) in rawSlots {
      guard let value = raw as? [String: Any],
        let slot = value["slot"] as? String,
        slot == key,
        let assetKey = value["key"] as? String,
        let digest = value["digest"] as? String,
        let mime = value["mimeType"] as? String,
        let bytes = (value["bytes"] as? NSNumber)?.intValue,
        let width = (value["width"] as? NSNumber)?.intValue,
        let height = (value["height"] as? NSNumber)?.intValue
      else { return nil }
      slots[key] = TailSyncThemeAssetDescriptor(
        slot: slot,
        key: assetKey,
        digest: digest,
        mimeType: mime,
        bytes: bytes,
        width: width,
        height: height
      )
    }
    return slots
  }
}

extension ApiClient {
  struct LocalThemeSettings: Codable, Equatable, Sendable {
    var activeThemeId: String
    var appearance: String
    var highContrast: Bool
  }
  struct ThemeV2Descriptor: Equatable, Identifiable {
    let id: String
    let storageHandle: String
    let source: String
    let version: String
    let digest: String
    let name: [String: String]
    let status: String
    let diagnostics: [ThemeDiagnostic]
  }
  struct ThemeDiagnostic: Equatable {
    let code: String
    let message: String
    let jsonPointer: String
    let severity: String
    let platforms: [String]
    let recoverable: Bool
    let fallbackApplied: Bool
  }
  struct ThemeValidation {
    let valid: Bool
    let digest: String?
    let candidateVersion: String?
    let diagnostics: [ThemeDiagnostic]
    let previewId: String?
    let previewTokens: [String: Any]?
    let previewAssetSlots: [String: TailSyncThemeAssetDescriptor]
  }

  func getLocalThemeSettings() async throws -> LocalThemeSettings {
    let response = try await request(["cmd": "get_local_theme_settings"])
    guard response["ok"] as? Bool == true, let data = response["data"] else {
      throw themeApiError(response, fallback: "unknown")
    }
    return try JSONDecoder().decode(
      LocalThemeSettings.self, from: JSONSerialization.data(withJSONObject: data))
  }
  func setLocalThemeSettings(_ settings: LocalThemeSettings) async throws {
    let data = try jsonDictionary(settings)
    let response = try await request(["cmd": "set_local_theme_settings", "settings": data])
    guard response["ok"] as? Bool == true else {
      throw themeApiError(response, fallback: "unknown")
    }
  }
  func listThemesV2() async throws -> [ThemeV2Descriptor] {
    let response = try await request(["cmd": "list_themes_v2"])
    guard response["ok"] as? Bool == true, let data = response["data"] as? [[String: Any]] else {
      throw themeApiError(response, fallback: "unknown")
    }
    return data.compactMap(themeV2Descriptor)
  }
  func validateThemeV2(path: String, mode: String = "light", highContrast: Bool = false)
    async throws -> ThemeValidation
  {
    let response = try await request([
      "cmd": "validate_theme", "path": path, "mode": mode, "high_contrast": highContrast,
    ])
    guard response["ok"] as? Bool == true, let data = response["data"] as? [String: Any] else {
      throw themeApiError(response, fallback: "unknown")
    }
    let previewData = data["preview"] as? [String: Any]
    let preview = previewData?["tokens"] as? [String: Any]
    let assets = (previewData?["assetSlots"] as? [String: Any] ?? [:]).compactMapValues {
      raw -> TailSyncThemeAssetDescriptor? in
      guard let value = raw as? [String: Any], let slot = value["slot"] as? String,
        let key = value["key"] as? String, let digest = value["digest"] as? String,
        let mime = value["mimeType"] as? String,
        let bytes = (value["bytes"] as? NSNumber)?.intValue,
        let width = (value["width"] as? NSNumber)?.intValue,
        let height = (value["height"] as? NSNumber)?.intValue
      else { return nil }
      return TailSyncThemeAssetDescriptor(
        slot: slot, key: key, digest: digest, mimeType: mime, bytes: bytes, width: width,
        height: height)
    }
    return ThemeValidation(
      valid: data["valid"] as? Bool ?? false, digest: data["digest"] as? String,
      candidateVersion: data["candidateVersion"] as? String,
      diagnostics: themeDiagnostics(data["diagnostics"]), previewId: previewData?["id"] as? String,
      previewTokens: preview, previewAssetSlots: assets)
  }
  func installThemeV2(path: String, digest: String) async throws -> ThemeV2Descriptor {
    let response = try await request([
      "cmd": "install_theme", "path": path, "expected_digest": digest,
    ])
    guard response["ok"] as? Bool == true,
      let data = response["data"] as? [String: Any],
      let descriptor = themeV2Descriptor(data)
    else {
      throw themeApiError(response, fallback: "invalid theme install response")
    }
    return descriptor
  }
  func updateThemeV2(
    path: String,
    digest: String,
    allowSameVersion: Bool = false,
    allowDowngrade: Bool = false
  ) async throws -> ThemeV2Descriptor {
    let response = try await request([
      "cmd": "update_theme",
      "path": path,
      "expected_digest": digest,
      "options": [
        "allowSameVersion": allowSameVersion,
        "allowDowngrade": allowDowngrade,
      ],
    ])
    guard response["ok"] as? Bool == true,
      let data = response["data"] as? [String: Any],
      let descriptor = themeV2Descriptor(data)
    else {
      throw themeApiError(response, fallback: "invalid theme update response")
    }
    return descriptor
  }
  func rollbackThemeV2(id: String) async throws -> ThemeV2Descriptor {
    let response = try await request(["cmd": "rollback_theme", "theme_id": id])
    guard response["ok"] as? Bool == true,
      let data = response["data"] as? [String: Any],
      let descriptor = themeV2Descriptor(data)
    else {
      throw themeApiError(response, fallback: "invalid theme rollback response")
    }
    return descriptor
  }
  func deleteThemeV2(id: String, storageHandle: String) async throws {
    let response = try await request([
      "cmd": "delete_theme_v2", "theme_id": id, "storage_handle": storageHandle,
    ])
    guard response["ok"] as? Bool == true else {
      throw themeApiError(response, fallback: "unknown")
    }
  }

  /// Core resolves V2 inheritance/expressions; Swift maps only resolved
  /// semantic values into its native palette model.
  func resolveThemeV2(themeId: String, highContrast: Bool = false) async -> TailSyncThemeDefinition?
  {
    async let light = request([
      "cmd": "resolve_theme", "theme_id": themeId, "mode": "light", "platform": "macos",
      "high_contrast": highContrast,
    ])
    async let dark = request([
      "cmd": "resolve_theme", "theme_id": themeId, "mode": "dark", "platform": "macos",
      "high_contrast": highContrast,
    ])
    guard let l = try? await light, let d = try? await dark,
      l["ok"] as? Bool == true, d["ok"] as? Bool == true,
      let lightData = l["data"] as? [String: Any],
      let darkData = d["data"] as? [String: Any]
    else { return nil }
    return ThemeResolvedPairDecoder.decode(
      themeId: themeId,
      highContrast: highContrast,
      lightData: lightData,
      darkData: darkData
    )
  }

  func getThemeAssetSlot(themeId: String, digest: String, slot: String) async throws -> Data {
    let response = try await request(
      [
        "cmd": "get_theme_asset_slot", "theme_id": themeId, "expected_digest": digest,
        "asset_slot": slot,
      ], maxResponseBytes: 16 * 1024 * 1024)
    guard response["ok"] as? Bool == true, let value = response["data"] as? [String: Any],
      let encoded = value["data_b64"] as? String, let bytes = Data(base64Encoded: encoded)
    else {
      throw themeApiError(response, fallback: "invalid theme asset response")
    }
    return bytes
  }

  func previewThemeAssetSlot(path: String, digest: String, slot: String) async throws -> Data {
    let response = try await request(
      [
        "cmd": "preview_theme_asset_slot", "path": path, "expected_digest": digest,
        "asset_slot": slot,
      ], maxResponseBytes: 16 * 1024 * 1024)
    guard response["ok"] as? Bool == true, let value = response["data"] as? [String: Any],
      let encoded = value["data_b64"] as? String, let bytes = Data(base64Encoded: encoded)
    else {
      throw themeApiError(response, fallback: "invalid theme preview asset response")
    }
    return bytes
  }

  private func themeErrorMessage(_ response: [String: Any], fallback: String) -> String {
    if let data = response["data"] as? [String: Any], let code = data["code"] as? String,
      let message = data["message"] as? String
    {
      return "\(code): \(message)"
    }
    return response["error"] as? String ?? fallback
  }

  private func themeApiError(_ response: [String: Any], fallback: String) -> ApiError {
    if let data = response["data"] as? [String: Any],
      let diagnostic = themeDiagnostic(data)
    {
      return .themeError(diagnostic)
    }
    return .serverError(themeErrorMessage(response, fallback: fallback))
  }

  private func themeDiagnostic(_ value: [String: Any]) -> ThemeDiagnostic? {
    guard let code = value["code"] as? String,
      let message = value["message"] as? String
    else { return nil }
    return ThemeDiagnostic(
      code: code,
      message: message,
      jsonPointer: value["jsonPointer"] as? String ?? "",
      severity: value["severity"] as? String ?? "error",
      platforms: value["platforms"] as? [String] ?? [],
      recoverable: value["recoverable"] as? Bool ?? false,
      fallbackApplied: value["fallbackApplied"] as? Bool ?? false
    )
  }

  private func themeV2Descriptor(_ item: [String: Any]) -> ThemeV2Descriptor? {
    guard let id = item["id"] as? String,
      let handle = item["storageHandle"] as? String
    else { return nil }
    return ThemeV2Descriptor(
      id: id,
      storageHandle: handle,
      source: item["source"] as? String ?? "custom",
      version: item["version"] as? String ?? "",
      digest: item["digest"] as? String ?? "",
      name: item["name"] as? [String: String] ?? [:],
      status: item["status"] as? String ?? "invalid",
      diagnostics: themeDiagnostics(item["diagnostics"])
    )
  }

  private func themeDiagnostics(_ raw: Any?) -> [ThemeDiagnostic] {
    guard let values = raw as? [[String: Any]] else { return [] }
    return values.compactMap { value in
      themeDiagnostic(value)
    }
  }
}
