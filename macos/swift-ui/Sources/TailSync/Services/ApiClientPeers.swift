import Foundation

extension ApiClient {
  struct DeviceSnapshot: Decodable {
    let hostname: String
    let tailscale_ip: String
    let connection_mode: String
    let public_key: String
    let fingerprint: String
    let iroh_endpoint_id: String?

    init(from decoder: Decoder) throws {
      let values = try decoder.container(keyedBy: CodingKeys.self)
      hostname = try values.decodeIfPresent(String.self, forKey: .hostname) ?? "Unknown"
      tailscale_ip = try values.decodeIfPresent(String.self, forKey: .tailscale_ip) ?? ""
      connection_mode =
        try values.decodeIfPresent(String.self, forKey: .connection_mode) ?? "tailscale"
      public_key = try values.decodeIfPresent(String.self, forKey: .public_key) ?? ""
      fingerprint = try values.decodeIfPresent(String.self, forKey: .fingerprint) ?? ""
      iroh_endpoint_id = try values.decodeIfPresent(String.self, forKey: .iroh_endpoint_id)
    }

    private enum CodingKeys: String, CodingKey {
      case hostname, tailscale_ip, connection_mode, public_key, fingerprint, iroh_endpoint_id
    }
  }

  struct PeerSnapshot: Decodable, Identifiable {
    struct Route: Decodable {
      let interface: String
      let address: String
      let status: String
      let online: Bool
      let connected: Bool
      let latencyMs: Int?
      let pairingEndpoint: Bool
      let rttCapable: Bool

      init(from decoder: Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        interface = try values.decodeIfPresent(String.self, forKey: .interface) ?? "lan"
        address = try values.decodeIfPresent(String.self, forKey: .address) ?? ""
        status = try values.decodeIfPresent(String.self, forKey: .status) ?? "discovered"
        online = try values.decodeIfPresent(Bool.self, forKey: .online) ?? false
        connected = try values.decodeIfPresent(Bool.self, forKey: .connected) ?? false
        latencyMs =
          try values.decodeIfPresent(Int.self, forKey: .latencyMs)
          ?? values.decodeIfPresent(Int.self, forKey: .legacyLatency)
        pairingEndpoint = try values.decodeIfPresent(Bool.self, forKey: .pairingEndpoint) ?? false
        rttCapable =
          try values.decodeIfPresent(Bool.self, forKey: .rttCapable)
          ?? (interface != "iroh")
      }

      private enum CodingKeys: String, CodingKey {
        case interface, address, status, online, connected
        case latencyMs = "latency_ms"
        case legacyLatency = "latency"
        case pairingEndpoint = "pairing_endpoint"
        case rttCapable = "rtt_capable"
      }
    }

    struct Candidate: Decodable {
      let interface: String
      let address: String
      let online: Bool
      let latency: Int?
      let status: String
      let rttCapable: Bool

      init(from decoder: Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        interface = try values.decodeIfPresent(String.self, forKey: .interface) ?? "lan"
        address = try values.decodeIfPresent(String.self, forKey: .address) ?? ""
        online = try values.decodeIfPresent(Bool.self, forKey: .online) ?? false
        latency = try values.decodeIfPresent(Int.self, forKey: .latency)
        status =
          try values.decodeIfPresent(String.self, forKey: .status)
          ?? (online ? "online" : "discovered")
        rttCapable =
          try values.decodeIfPresent(Bool.self, forKey: .rttCapable)
          ?? (interface != "iroh")
      }

      private enum CodingKeys: String, CodingKey {
        case interface, address, online, latency, status
        case rttCapable = "rtt_capable"
      }
    }

    let hostname: String
    let tailscale_ip: String
    let address: String
    let online: Bool
    let enabled: Bool
    let connection_mode: String
    let trusted: Bool
    let fingerprint: String
    let current_interface: String?
    let current_address: String?
    let candidates: [Candidate]
    let routes: [Route]
    let status: String
    let protocolError: String?
    let requiredProtocolVersion: Int?
    var id: String { hostname }

    init(from decoder: Decoder) throws {
      let values = try decoder.container(keyedBy: CodingKeys.self)
      hostname = try values.decodeIfPresent(String.self, forKey: .hostname) ?? "Unknown"
      tailscale_ip = try values.decodeIfPresent(String.self, forKey: .tailscale_ip) ?? ""
      address = try values.decodeIfPresent(String.self, forKey: .address) ?? tailscale_ip
      online = try values.decodeIfPresent(Bool.self, forKey: .online) ?? false
      enabled = try values.decodeIfPresent(Bool.self, forKey: .enabled) ?? true
      connection_mode = try values.decodeIfPresent(String.self, forKey: .connection_mode) ?? ""
      trusted = try values.decodeIfPresent(Bool.self, forKey: .trusted) ?? false
      fingerprint = try values.decodeIfPresent(String.self, forKey: .fingerprint) ?? ""
      current_interface = try values.decodeIfPresent(String.self, forKey: .current_interface)
      current_address = try values.decodeIfPresent(String.self, forKey: .current_address)
      candidates = try values.decodeIfPresent([Candidate].self, forKey: .candidates) ?? []
      routes = try values.decodeIfPresent([Route].self, forKey: .routes) ?? []
      status =
        try values.decodeIfPresent(String.self, forKey: .status)
        ?? (current_address != nil ? "connected" : online ? "online" : "offline")
      protocolError = try values.decodeIfPresent(String.self, forKey: .protocolError)
      requiredProtocolVersion = try values.decodeIfPresent(
        Int.self, forKey: .requiredProtocolVersion)
    }

    private enum CodingKeys: String, CodingKey {
      case hostname, tailscale_ip, address, online, enabled, connection_mode, trusted,
        fingerprint, current_interface, current_address, candidates, routes, status
      case protocolError = "protocol_error"
      case requiredProtocolVersion = "required_protocol_version"
    }
  }

  typealias PeersResult = (
    local: DeviceSnapshot?,
    peers: [PeerSnapshot],
    pairedEndpoints: [String: String],
    error: String?,
    requestSucceeded: Bool
  )

  private func decodePeersResponse(_ response: [String: Any]) -> PeersResult? {
    guard response["ok"] as? Bool == true,
      let data = response["data"] as? [String: Any]
    else { return nil }
    let local: DeviceSnapshot?
    if let value = data["self"], let json = try? JSONSerialization.data(withJSONObject: value) {
      local = try? JSONDecoder().decode(DeviceSnapshot.self, from: json)
    } else {
      local = nil
    }
    let peers: [PeerSnapshot]
    if let value = data["peers"], let json = try? JSONSerialization.data(withJSONObject: value) {
      peers = (try? JSONDecoder().decode([PeerSnapshot].self, from: json)) ?? []
    } else {
      peers = []
    }
    return (
      local,
      peers,
      data["paired_peer_endpoints"] as? [String: String] ?? [:],
      data["discovery_error"] as? String,
      true
    )
  }

  func getPeers() async -> PeersResult {
    guard let response = try? await request(["cmd": "get_peers"]),
      let result = decodePeersResponse(response)
    else {
      return (nil, [], [:], responseError(), false)
    }
    return result
  }

  func refreshPeers() async -> PeersResult {
    guard let response = try? await request(["cmd": "refresh_peers"]),
      let result = decodePeersResponse(response)
    else {
      return (nil, [], [:], responseError(), false)
    }
    return result
  }

  func togglePeer(hostname: String, enabled: Bool) async -> Bool {
    guard
      let response = try? await request([
        "cmd": "toggle_peer", "hostname": hostname, "enabled": enabled,
      ])
    else { return false }
    return response["ok"] as? Bool == true
  }

  func trustPeer(hostname: String, publicKey: String, address: String? = nil) async throws -> String
  {
    var payload: [String: Any] = [
      "cmd": "trust_peer", "hostname": hostname, "public_key": publicKey,
    ]
    if let address, !address.isEmpty { payload["address"] = address }
    let response = try await request(payload)
    guard response["ok"] as? Bool == true,
      let data = response["data"] as? [String: Any],
      let fingerprint = data["fingerprint"] as? String
    else {
      throw ApiError.serverError(response["error"] as? String ?? "Pairing failed")
    }
    return fingerprint
  }

  func forgetPeer(hostname: String) async throws {
    let response = try await request(["cmd": "forget_peer", "hostname": hostname])
    guard response["ok"] as? Bool == true else {
      throw ApiError.serverError(response["error"] as? String ?? "Unpair failed")
    }
  }

  struct PairingPeerStatus: Decodable {
    let hostname: String
    let address: String
    let fingerprint: String
    let verification_code: String
    let local_confirmed: Bool
    let remote_confirmed: Bool
  }

  struct PairingStatus: Decodable {
    let pairing_enabled: Bool
    let phase: String
    let expires_at: UInt64?
    let remaining_seconds: UInt64
    let failed_attempts: Int
    let max_failures: Int
    let peer: PairingPeerStatus?
    let error: String?
  }

  func getPairingStatus() async throws -> PairingStatus {
    try await pairingRequest(["cmd": "get_pairing_status"])
  }

  func enablePairing() async throws -> PairingStatus {
    try await pairingRequest(["cmd": "enable_pairing"])
  }

  func startPairing(address: String) async throws -> PairingStatus {
    try await pairingRequest(["cmd": "start_pairing", "address": address])
  }

  func confirmPairing() async throws -> PairingStatus {
    try await pairingRequest(["cmd": "confirm_pairing"])
  }

  func cancelPairing() async throws -> PairingStatus {
    try await pairingRequest(["cmd": "cancel_pairing"])
  }

  private func pairingRequest(_ payload: [String: Any]) async throws -> PairingStatus {
    let response = try await request(payload)
    guard response["ok"] as? Bool == true,
      let value = response["data"] as? [String: Any]
    else {
      throw ApiError.serverError(response["error"] as? String ?? "Pairing failed")
    }
    return try JSONDecoder().decode(
      PairingStatus.self,
      from: JSONSerialization.data(withJSONObject: value)
    )
  }

  func testConnection(address: String) async -> (latencyMs: Int, path: String, error: String)? {
    guard let response = try? await request(["cmd": "test_connection", "hostname": address]) else {
      return nil
    }
    if response["ok"] as? Bool == true,
      let data = response["data"] as? [String: Any],
      let latency = (data["latency_ms"] as? NSNumber)?.intValue
    {
      return (latency, data["path"] as? String ?? "", "")
    }
    return (0, "", response["error"] as? String ?? "Connection failed")
  }

  private func responseError() -> String {
    "Connection failed"
  }
}
