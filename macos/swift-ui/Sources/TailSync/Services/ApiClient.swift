import Foundation

enum ApiError: LocalizedError {
  case connectionFailed
  case sendFailed
  case noResponse
  case invalidJson
  case serverError(String)
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
      return message
    case .themeError(let diagnostic):
      let pointer = diagnostic.jsonPointer.isEmpty ? "" : " (\(diagnostic.jsonPointer))"
      return "\(diagnostic.code): \(diagnostic.message)\(pointer)"
    }
  }

  var pairingErrorDescription: String {
    guard case .serverError(let message) = self else { return localizedDescription }
    if message.contains("Pairing window is closed") {
      return Loc.t("error.pairingWindowClosed")
    }
    if message.contains("Pairing handshake timed out") {
      return Loc.t("error.pairingHandshakeTimedOut")
    }
    if message.contains("Connection reset by peer") || message.contains("early eof") {
      return Loc.t("error.pairingConnectionClosed")
    }
    return message
  }
}
