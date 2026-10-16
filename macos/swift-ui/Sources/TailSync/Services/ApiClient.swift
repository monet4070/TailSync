import Foundation

enum ApiError: LocalizedError {
  case connectionFailed
  case sendFailed
  case noResponse
  case invalidJson
  case serverError(String)
  /// A finite, local stable error code carried by the normalized error
  /// envelope. Unlike `.serverError` the display text is derived from the code
  /// policy, never from untrusted remote message_key/detail fields.
  case stableServerError(ContractStableErrorCode)
  case themeError(ApiClient.ThemeDiagnostic)

  var errorDescription: String? {
    switch self {
    case .connectionFailed:
      return Loc.t("error.localServiceUnavailable")
    case .sendFailed:
      return Loc.t("error.localServiceSendFailed")
    case .noResponse:
      return Loc.t("error.localServiceNoResponse")
    case .invalidJson:
      return Loc.t("error.localServiceInvalidResponse")
    case .serverError(let message):
      if Self.restoreCodes.contains(message) {
        return Loc.t("history.restore.code.\(message)")
      }
      return message
    case .stableServerError(let code):
      return Self.stableErrorDescription(code)
    case .themeError(let diagnostic):
      let pointer = diagnostic.jsonPointer.isEmpty ? "" : " (\(diagnostic.jsonPointer))"
      return "\(diagnostic.code): \(diagnostic.message)\(pointer)"
    }
  }

  static let restoreCodes: Set<String> = [
    "publish_partial", "publish_unknown", "publish_superseded", "native_only",
    "originals_incomplete", "resources_incomplete", "no_single_image", "format_unavailable", "unsupported_format",
  ]

  var pairingErrorDescription: String {
    switch self {
    case .stableServerError(let code):
      return Self.stableErrorDescription(code)
    case .serverError(let message):
      return Self.pairingErrorDescription(message)
    default:
      return localizedDescription
    }
  }

  static func pairingErrorDescription(_ message: String) -> String {
    if message.contains("Pairing window is closed") {
      return Loc.t("error.pairingWindowClosed")
    }
    if message.contains("Pairing handshake timed out") {
      return Loc.t("error.pairingHandshakeTimedOut")
    }
    if message.contains("Connection reset by peer") || message.contains("early eof") {
      return Loc.t("error.pairingConnectionClosed")
    }
    if message.range(of: "Peer identity mismatch", options: .caseInsensitive) != nil {
      return Loc.t("error.identityMismatch")
    }
    return message
  }

  /// Fixed display text for a finite stable error code.
  static func stableErrorDescription(_ code: ContractStableErrorCode) -> String {
    Loc.t(stableMessageKey(code))
  }

  /// The code→message-key policy is owned by the generated contract envelope.
  /// Decode a code-only probe so the mapping keeps a single source of truth
  /// instead of a hand-maintained duplicate.
  static func stableMessageKey(_ code: ContractStableErrorCode) -> String {
    let probe: [String: Any] = [
      "schema_version": 1,
      "code": code.rawValue,
      "retryable": false,
      "message_key": "",
      "detail_class": "internal",
    ]
    guard let data = try? JSONSerialization.data(withJSONObject: probe),
      let envelope = try? JSONDecoder().decode(ContractStableErrorEnvelope.self, from: data)
    else { return "error.internal" }
    return envelope.message_key
  }

  /// Narrow extraction for the pairing request paths: keep the finite stable
  /// code the local normalizer derived from the fixed envelope, otherwise fall
  /// back to the legacy string error. Other services keep throwing plain
  /// `.serverError`.
  static func responseError(_ response: [String: Any], fallback: String) -> ApiError {
    if let raw = response["error_code"] as? String,
      let code = ContractStableErrorCode(rawValue: raw)
    {
      return .stableServerError(code)
    }
    return .serverError(response["error"] as? String ?? fallback)
  }
}

/// Narrow pairing presentation seam. The Connections view and the remote
/// pairing input state both call this so the fixed precedence lives in one
/// place and can be exercised without a GUI.
enum PairingFailurePresentation {
  /// Message for a direct request failure (typed stable code preferred).
  static func message(for error: Error) -> String {
    if let apiError = error as? ApiError {
      return apiError.pairingErrorDescription
    }
    return error.localizedDescription
  }

  /// The pairing panel/sheet shows a direct request error ahead of the polled
  /// status so a later poll can never replace it.
  static func message(directError: String?, status: ApiClient.PairingStatus?) -> String? {
    if let directError { return directError }
    return statusMessage(status)
  }

  /// Polled status text: a known typed kind wins over the legacy text, which
  /// stays the fallback for a missing/unknown kind.
  static func statusMessage(_ status: ApiClient.PairingStatus?) -> String? {
    guard let status else { return nil }
    if let code = stableCode(for: status.failure_kind) {
      return ApiError.stableErrorDescription(code)
    }
    return status.error.map(ApiError.pairingErrorDescription)
  }

  /// Finite kind→code mapping mirroring the runtime classification. Unknown
  /// kinds return nil so the legacy text fallback still applies; Transport and
  /// InvalidAdvertisement stay conservative and never claim a network failure.
  static func stableCode(for kind: ContractPairingFailureKind?) -> ContractStableErrorCode? {
    guard let kind else { return nil }
    switch kind {
    case .protocol_incompatible: return .protocol_incompatible
    case .invalid_argument: return .invalid_argument
    case .identity_mismatch: return .pairing_identity_mismatch
    case .window_closed: return .pairing_window_closed
    case .timed_out: return .pairing_timed_out
    case .connection_closed: return .pairing_connection_closed
    case .busy: return .pairing_busy
    case .cancelled: return .pairing_cancelled
    case .transport, .invalid_advertisement: return .internal_error
    case .unknown: return nil
    }
  }
}
