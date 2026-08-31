import Foundation

extension ApiClient {
  struct RemotePairingInvite: Decodable {
    let link: String
    let expires_at: UInt64
    let remaining_seconds: UInt64
  }

  struct RemotePairingInvitePreview: Decodable {
    let endpoint_id: String
    let expires_at: UInt64
    let remaining_seconds: UInt64
  }

  enum RemoteInviteState: String, Decodable {
    case ready
    case claimed
  }

  struct RemoteInviteStatus: Decodable {
    let active: Bool
    let state: RemoteInviteState?
    let expires_at: UInt64?
    let remaining_seconds: UInt64
  }

  func createRemotePairingInvite() async throws -> RemotePairingInvite {
    try await remotePairingRequest(["cmd": "create_remote_pairing_invite"])
  }

  func inspectRemotePairingLink(_ link: String) async throws -> RemotePairingInvitePreview {
    try await remotePairingRequest([
      "cmd": "inspect_remote_pairing_link",
      "invite_link": link,
    ])
  }

  func startRemotePairing(inviteLink: String) async throws -> PairingStatus {
    try await remotePairingRequest([
      "cmd": "start_remote_pairing",
      "invite_link": inviteLink,
    ])
  }

  func getRemotePairingInviteStatus() async throws -> RemoteInviteStatus {
    try await remotePairingRequest(["cmd": "get_remote_pairing_invite_status"])
  }

  func cancelRemotePairingInvite() async throws -> PairingStatus {
    try await remotePairingRequest(["cmd": "cancel_remote_pairing_invite"])
  }

  private func remotePairingRequest<Value: Decodable>(_ payload: [String: Any]) async throws -> Value {
    let response = try await request(payload)
    guard response["ok"] as? Bool == true,
          let value = response["data"] as? [String: Any]
    else {
      throw ApiError.serverError(response["error"] as? String ?? "Remote pairing failed")
    }
    do {
      return try JSONDecoder().decode(
        Value.self,
        from: JSONSerialization.data(withJSONObject: value)
      )
    } catch {
      throw ApiError.invalidJson
    }
  }
}
