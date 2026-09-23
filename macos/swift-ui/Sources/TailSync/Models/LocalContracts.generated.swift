// Generated from production Rust DTOs by shared/schema/generate-local-contracts.mjs. Do not edit.
import Foundation

struct ContractActiveRoute: Codable, Sendable {
  let `address`: String
  let `interface`: ContractConnectionInterface
  let `latency`: UInt64
  private enum CodingKeys: String, CodingKey { case `address`, `interface`, `latency` }
  init(from decoder: Decoder) throws {
    let c = try decoder.container(keyedBy: CodingKeys.self)
    self.`address` = try c.decode(String.self, forKey: .`address`)
    self.`interface` = try c.decode(ContractConnectionInterface.self, forKey: .`interface`)
    self.`latency` = try c.decode(UInt64.self, forKey: .`latency`)
  }
}

enum ContractConnectionInterface: String, Codable, Sendable {
  case `lan` = "lan"
  case `iroh` = "iroh"
  case `tailscale` = "tailscale"
}

struct ContractDaemonStatus: Codable, Sendable {
  let `active_routes`: [String: ContractActiveRoute]
  let `clipboard_monitor_failures`: UInt64
  let `clipboard_monitor_healthy`: Bool
  let `tcp_server_healthy`: Bool
  private enum CodingKeys: String, CodingKey { case `active_routes`, `clipboard_monitor_failures`, `clipboard_monitor_healthy`, `tcp_server_healthy` }
  init(from decoder: Decoder) throws {
    let c = try decoder.container(keyedBy: CodingKeys.self)
    self.`active_routes` = try c.decode([String: ContractActiveRoute].self, forKey: .`active_routes`)
    self.`clipboard_monitor_failures` = try c.decode(UInt64.self, forKey: .`clipboard_monitor_failures`)
    self.`clipboard_monitor_healthy` = try c.decode(Bool.self, forKey: .`clipboard_monitor_healthy`)
    self.`tcp_server_healthy` = try c.decode(Bool.self, forKey: .`tcp_server_healthy`)
  }
}

struct ContractFavoriteMutation: Codable, Sendable {
  let `affected_ids`: [Int64]
  let `favorite`: Bool
  private enum CodingKeys: String, CodingKey { case `affected_ids`, `favorite` }
  init(from decoder: Decoder) throws {
    let c = try decoder.container(keyedBy: CodingKeys.self)
    self.`affected_ids` = try c.decode([Int64].self, forKey: .`affected_ids`)
    self.`favorite` = try c.decode(Bool.self, forKey: .`favorite`)
  }
}

struct ContractFileProgress: Codable, Sendable {
  let `active`: Bool
  let `batch_id`: String
  let `can_stop`: Bool
  let `completed_files`: UInt64
  let `device`: String
  let `direction`: String
  let `name`: String
  let `sent`: UInt64
  let `speed_bytes_per_second`: UInt64
  let `status`: String
  let `total`: UInt64
  let `total_files`: UInt64
  private enum CodingKeys: String, CodingKey { case `active`, `batch_id`, `can_stop`, `completed_files`, `device`, `direction`, `name`, `sent`, `speed_bytes_per_second`, `status`, `total`, `total_files` }
  init(from decoder: Decoder) throws {
    let c = try decoder.container(keyedBy: CodingKeys.self)
    self.`active` = try c.decode(Bool.self, forKey: .`active`)
    self.`batch_id` = try c.decode(String.self, forKey: .`batch_id`)
    self.`can_stop` = try c.decode(Bool.self, forKey: .`can_stop`)
    self.`completed_files` = try c.decode(UInt64.self, forKey: .`completed_files`)
    self.`device` = try c.decode(String.self, forKey: .`device`)
    self.`direction` = try c.decode(String.self, forKey: .`direction`)
    guard ["sending","receiving"].contains(self.`direction`) else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
    self.`name` = try c.decode(String.self, forKey: .`name`)
    self.`sent` = try c.decode(UInt64.self, forKey: .`sent`)
    self.`speed_bytes_per_second` = try c.decode(UInt64.self, forKey: .`speed_bytes_per_second`)
    self.`status` = try c.decode(String.self, forKey: .`status`)
    self.`total` = try c.decode(UInt64.self, forKey: .`total`)
    self.`total_files` = try c.decode(UInt64.self, forKey: .`total_files`)
  }
}

struct ContractHistoryEntry: Codable, Sendable {
  let `batch_count`: Int64?
  let `batch_id`: String?
  let `batch_index`: Int64?
  let `batch_status`: String
  let `batch_total`: Int64?
  let `categories`: [String]
  let `category`: String
  let `category_confidence`: Int64
  let `classifier_version`: Int64
  let `data_hash`: String
  let `description`: String
  let `id`: Int64
  let `pinned`: Bool
  let `size_bytes`: Int64
  let `source_peer`: String
  let `timestamp`: String
  let `type`: ContractPreviewKind
  private enum CodingKeys: String, CodingKey { case `batch_count`, `batch_id`, `batch_index`, `batch_status`, `batch_total`, `categories`, `category`, `category_confidence`, `classifier_version`, `data_hash`, `description`, `id`, `pinned`, `size_bytes`, `source_peer`, `timestamp`, `type` }
  init(from decoder: Decoder) throws {
    let c = try decoder.container(keyedBy: CodingKeys.self)
    guard c.contains(.`batch_count`) else { throw DecodingError.keyNotFound(CodingKeys.`batch_count`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`batch_count` = try c.decodeIfPresent(Int64.self, forKey: .`batch_count`)
    guard c.contains(.`batch_id`) else { throw DecodingError.keyNotFound(CodingKeys.`batch_id`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`batch_id` = try c.decodeIfPresent(String.self, forKey: .`batch_id`)
    guard c.contains(.`batch_index`) else { throw DecodingError.keyNotFound(CodingKeys.`batch_index`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`batch_index` = try c.decodeIfPresent(Int64.self, forKey: .`batch_index`)
    self.`batch_status` = try c.decode(String.self, forKey: .`batch_status`)
    guard ["complete","incomplete"].contains(self.`batch_status`) else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
    guard c.contains(.`batch_total`) else { throw DecodingError.keyNotFound(CodingKeys.`batch_total`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`batch_total` = try c.decodeIfPresent(Int64.self, forKey: .`batch_total`)
    self.`categories` = try c.decode([String].self, forKey: .`categories`)
    self.`category` = try c.decode(String.self, forKey: .`category`)
    guard ["text","website","code","command","structured_data","path","image","file"].contains(self.`category`) else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
    self.`category_confidence` = try c.decode(Int64.self, forKey: .`category_confidence`)
    self.`classifier_version` = try c.decode(Int64.self, forKey: .`classifier_version`)
    self.`data_hash` = try c.decode(String.self, forKey: .`data_hash`)
    self.`description` = try c.decode(String.self, forKey: .`description`)
    self.`id` = try c.decode(Int64.self, forKey: .`id`)
    self.`pinned` = try c.decode(Bool.self, forKey: .`pinned`)
    self.`size_bytes` = try c.decode(Int64.self, forKey: .`size_bytes`)
    self.`source_peer` = try c.decode(String.self, forKey: .`source_peer`)
    self.`timestamp` = try c.decode(String.self, forKey: .`timestamp`)
    self.`type` = try c.decode(ContractPreviewKind.self, forKey: .`type`)
  }
}

struct ContractHistoryQueryPage: Codable, Sendable {
  let `entries`: [ContractHistoryEntry]
  let `has_more`: Bool
  let `total`: UInt64?
  private enum CodingKeys: String, CodingKey { case `entries`, `has_more`, `total` }
  init(from decoder: Decoder) throws {
    let c = try decoder.container(keyedBy: CodingKeys.self)
    self.`entries` = try c.decode([ContractHistoryEntry].self, forKey: .`entries`)
    self.`has_more` = try c.decode(Bool.self, forKey: .`has_more`)
    guard c.contains(.`total`) else { throw DecodingError.keyNotFound(CodingKeys.`total`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`total` = try c.decodeIfPresent(UInt64.self, forKey: .`total`)
  }
}

struct ContractLocalCapabilities: Codable, Sendable {
  let `max_preview_bytes`: UInt64
  let `platform`: String
  let `schema_version`: UInt32
  let `supports_binary_preview`: Bool
  let `supports_runtime_snapshot`: Bool
  let `supports_stable_errors`: Bool
  let `wire_version`: UInt32
  private enum CodingKeys: String, CodingKey { case `max_preview_bytes`, `platform`, `schema_version`, `supports_binary_preview`, `supports_runtime_snapshot`, `supports_stable_errors`, `wire_version` }
  init(from decoder: Decoder) throws {
    let c = try decoder.container(keyedBy: CodingKeys.self)
    self.`max_preview_bytes` = try c.decode(UInt64.self, forKey: .`max_preview_bytes`)
    guard self.`max_preview_bytes` >= 67108864, self.`max_preview_bytes` <= 67108864 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
    self.`platform` = try c.decode(String.self, forKey: .`platform`)
    self.`schema_version` = try c.decode(UInt32.self, forKey: .`schema_version`)
    guard self.`schema_version` >= 1, self.`schema_version` <= 1 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
    self.`supports_binary_preview` = try c.decode(Bool.self, forKey: .`supports_binary_preview`)
    self.`supports_runtime_snapshot` = try c.decode(Bool.self, forKey: .`supports_runtime_snapshot`)
    self.`supports_stable_errors` = try c.decode(Bool.self, forKey: .`supports_stable_errors`)
    self.`wire_version` = try c.decode(UInt32.self, forKey: .`wire_version`)
    guard self.`wire_version` >= 4, self.`wire_version` <= 5 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
  }
}

struct ContractLocalDeviceSnapshot: Codable, Sendable {
  let `connection_mode`: String
  let `fingerprint`: String
  let `hostname`: String
  let `iroh_endpoint_id`: String?
  let `public_key`: String
  let `routes`: [ContractPeerRouteSnapshot]
  let `tailscale_ip`: String
  private enum CodingKeys: String, CodingKey { case `connection_mode`, `fingerprint`, `hostname`, `iroh_endpoint_id`, `public_key`, `routes`, `tailscale_ip` }
  init(from decoder: Decoder) throws {
    let c = try decoder.container(keyedBy: CodingKeys.self)
    self.`connection_mode` = try c.decode(String.self, forKey: .`connection_mode`)
    self.`fingerprint` = try c.decode(String.self, forKey: .`fingerprint`)
    self.`hostname` = try c.decode(String.self, forKey: .`hostname`)
    guard c.contains(.`iroh_endpoint_id`) else { throw DecodingError.keyNotFound(CodingKeys.`iroh_endpoint_id`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`iroh_endpoint_id` = try c.decodeIfPresent(String.self, forKey: .`iroh_endpoint_id`)
    self.`public_key` = try c.decode(String.self, forKey: .`public_key`)
    self.`routes` = try c.decode([ContractPeerRouteSnapshot].self, forKey: .`routes`)
    self.`tailscale_ip` = try c.decode(String.self, forKey: .`tailscale_ip`)
  }
}

struct ContractMacRuntimeSnapshot: Codable, Sendable {
  let `history_version`: UInt64
  let `notifications`: [ContractRuntimeNotification]
  let `progress`: ContractFileProgress?
  let `revision`: UInt64
  let `status`: ContractDaemonStatus
  let `storage`: ContractStorageStatus
  let `sync_enabled`: Bool
  private enum CodingKeys: String, CodingKey { case `history_version`, `notifications`, `progress`, `revision`, `status`, `storage`, `sync_enabled` }
  init(from decoder: Decoder) throws {
    let c = try decoder.container(keyedBy: CodingKeys.self)
    self.`history_version` = try c.decode(UInt64.self, forKey: .`history_version`)
    self.`notifications` = try c.decode([ContractRuntimeNotification].self, forKey: .`notifications`)
    guard c.contains(.`progress`) else { throw DecodingError.keyNotFound(CodingKeys.`progress`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`progress` = try c.decodeIfPresent(ContractFileProgress.self, forKey: .`progress`)
    self.`revision` = try c.decode(UInt64.self, forKey: .`revision`)
    self.`status` = try c.decode(ContractDaemonStatus.self, forKey: .`status`)
    self.`storage` = try c.decode(ContractStorageStatus.self, forKey: .`storage`)
    self.`sync_enabled` = try c.decode(Bool.self, forKey: .`sync_enabled`)
  }
}

struct ContractPeerCandidate: Codable, Sendable {
  let `address`: String
  let `interface`: ContractConnectionInterface
  let `latency`: UInt64?
  let `online`: Bool
  let `priority`: UInt8
  let `rtt_capable`: Bool
  let `status`: ContractPeerStatus
  private enum CodingKeys: String, CodingKey { case `address`, `interface`, `latency`, `online`, `priority`, `rtt_capable`, `status` }
  init(from decoder: Decoder) throws {
    let c = try decoder.container(keyedBy: CodingKeys.self)
    self.`address` = try c.decode(String.self, forKey: .`address`)
    self.`interface` = try c.decode(ContractConnectionInterface.self, forKey: .`interface`)
    self.`latency` = try c.decodeIfPresent(UInt64.self, forKey: .`latency`)
    self.`online` = try c.decode(Bool.self, forKey: .`online`)
    self.`priority` = try c.decode(UInt8.self, forKey: .`priority`)
    self.`rtt_capable` = try c.decode(Bool.self, forKey: .`rtt_capable`)
    self.`status` = try c.decode(ContractPeerStatus.self, forKey: .`status`)
  }
}

struct ContractPeerRouteSnapshot: Codable, Sendable {
  let `address`: String
  let `connected`: Bool
  let `interface`: ContractConnectionInterface
  let `latency_ms`: UInt64?
  let `online`: Bool
  let `pairing_endpoint`: Bool
  let `rtt_capable`: Bool
  let `status`: ContractPeerStatus
  private enum CodingKeys: String, CodingKey { case `address`, `connected`, `interface`, `latency_ms`, `online`, `pairing_endpoint`, `rtt_capable`, `status` }
  init(from decoder: Decoder) throws {
    let c = try decoder.container(keyedBy: CodingKeys.self)
    self.`address` = try c.decode(String.self, forKey: .`address`)
    self.`connected` = try c.decode(Bool.self, forKey: .`connected`)
    self.`interface` = try c.decode(ContractConnectionInterface.self, forKey: .`interface`)
    guard c.contains(.`latency_ms`) else { throw DecodingError.keyNotFound(CodingKeys.`latency_ms`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`latency_ms` = try c.decodeIfPresent(UInt64.self, forKey: .`latency_ms`)
    self.`online` = try c.decode(Bool.self, forKey: .`online`)
    self.`pairing_endpoint` = try c.decode(Bool.self, forKey: .`pairing_endpoint`)
    self.`rtt_capable` = try c.decode(Bool.self, forKey: .`rtt_capable`)
    self.`status` = try c.decode(ContractPeerStatus.self, forKey: .`status`)
  }
}

struct ContractPeerSnapshot: Codable, Sendable {
  let `address`: String
  let `candidates`: [ContractPeerCandidate]
  let `connection_mode`: String
  let `current_address`: String?
  let `current_interface`: ContractConnectionInterface?
  let `enabled`: Bool
  let `fingerprint`: String
  let `hostname`: String
  let `online`: Bool
  let `protocol_error`: String?
  let `required_protocol_version`: UInt8?
  let `routes`: [ContractPeerRouteSnapshot]
  let `status`: ContractPeerStatus
  let `tailscale_ip`: String
  let `trusted`: Bool
  private enum CodingKeys: String, CodingKey { case `address`, `candidates`, `connection_mode`, `current_address`, `current_interface`, `enabled`, `fingerprint`, `hostname`, `online`, `protocol_error`, `required_protocol_version`, `routes`, `status`, `tailscale_ip`, `trusted` }
  init(from decoder: Decoder) throws {
    let c = try decoder.container(keyedBy: CodingKeys.self)
    self.`address` = try c.decode(String.self, forKey: .`address`)
    self.`candidates` = try c.decode([ContractPeerCandidate].self, forKey: .`candidates`)
    self.`connection_mode` = try c.decode(String.self, forKey: .`connection_mode`)
    self.`current_address` = try c.decodeIfPresent(String.self, forKey: .`current_address`)
    self.`current_interface` = try c.decodeIfPresent(ContractConnectionInterface.self, forKey: .`current_interface`)
    self.`enabled` = try c.decode(Bool.self, forKey: .`enabled`)
    self.`fingerprint` = try c.decode(String.self, forKey: .`fingerprint`)
    self.`hostname` = try c.decode(String.self, forKey: .`hostname`)
    self.`online` = try c.decode(Bool.self, forKey: .`online`)
    guard c.contains(.`protocol_error`) else { throw DecodingError.keyNotFound(CodingKeys.`protocol_error`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`protocol_error` = try c.decodeIfPresent(String.self, forKey: .`protocol_error`)
    guard c.contains(.`required_protocol_version`) else { throw DecodingError.keyNotFound(CodingKeys.`required_protocol_version`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`required_protocol_version` = try c.decodeIfPresent(UInt8.self, forKey: .`required_protocol_version`)
    self.`routes` = try c.decode([ContractPeerRouteSnapshot].self, forKey: .`routes`)
    self.`status` = try c.decode(ContractPeerStatus.self, forKey: .`status`)
    self.`tailscale_ip` = try c.decode(String.self, forKey: .`tailscale_ip`)
    self.`trusted` = try c.decode(Bool.self, forKey: .`trusted`)
  }
}

enum ContractPeerStatus: String, Codable, Sendable {
  case `discovered` = "discovered"
  case `confirming` = "confirming"
  case `online` = "online"
  case `connected` = "connected"
  case `offline` = "offline"
}

struct ContractPeersResponse: Codable, Sendable {
  let `discovery_error`: String?
  let `paired_peer_endpoints`: [String: String]
  let `peers`: [ContractPeerSnapshot]
  let `self`: ContractLocalDeviceSnapshot
  private enum CodingKeys: String, CodingKey { case `discovery_error`, `paired_peer_endpoints`, `peers`, `self` }
  init(from decoder: Decoder) throws {
    let c = try decoder.container(keyedBy: CodingKeys.self)
    guard c.contains(.`discovery_error`) else { throw DecodingError.keyNotFound(CodingKeys.`discovery_error`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`discovery_error` = try c.decodeIfPresent(String.self, forKey: .`discovery_error`)
    self.`paired_peer_endpoints` = try c.decode([String: String].self, forKey: .`paired_peer_endpoints`)
    self.`peers` = try c.decode([ContractPeerSnapshot].self, forKey: .`peers`)
    self.`self` = try c.decode(ContractLocalDeviceSnapshot.self, forKey: .`self`)
  }
}

struct ContractPreviewBatchNavigation: Codable, Sendable {
  let `batch_id`: String
  let `first_entry_id`: Int64
  let `item_count`: UInt64
  let `item_index`: UInt64
  let `last_entry_id`: Int64
  let `next_entry_id`: Int64?
  let `previous_entry_id`: Int64?
  private enum CodingKeys: String, CodingKey { case `batch_id`, `first_entry_id`, `item_count`, `item_index`, `last_entry_id`, `next_entry_id`, `previous_entry_id` }
  init(from decoder: Decoder) throws {
    let c = try decoder.container(keyedBy: CodingKeys.self)
    self.`batch_id` = try c.decode(String.self, forKey: .`batch_id`)
    self.`first_entry_id` = try c.decode(Int64.self, forKey: .`first_entry_id`)
    self.`item_count` = try c.decode(UInt64.self, forKey: .`item_count`)
    self.`item_index` = try c.decode(UInt64.self, forKey: .`item_index`)
    self.`last_entry_id` = try c.decode(Int64.self, forKey: .`last_entry_id`)
    guard c.contains(.`next_entry_id`) else { throw DecodingError.keyNotFound(CodingKeys.`next_entry_id`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`next_entry_id` = try c.decodeIfPresent(Int64.self, forKey: .`next_entry_id`)
    guard c.contains(.`previous_entry_id`) else { throw DecodingError.keyNotFound(CodingKeys.`previous_entry_id`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`previous_entry_id` = try c.decodeIfPresent(Int64.self, forKey: .`previous_entry_id`)
  }
}

enum ContractPreviewErrorCode: String, Codable, Sendable {
  case `entry_not_found` = "entry_not_found"
  case `batch_not_found` = "batch_not_found"
  case `entry_not_in_batch` = "entry_not_in_batch"
  case `metadata_unavailable` = "metadata_unavailable"
  case `payload_unavailable` = "payload_unavailable"
  case `preview_too_large` = "preview_too_large"
  case `unsupported_type` = "unsupported_type"
  case `invalid_size` = "invalid_size"
}

struct ContractPreviewErrorInfo: Codable, Sendable {
  let `code`: ContractPreviewErrorCode
  let `entry_id`: Int64?
  let `limit_bytes`: UInt64?
  let `message`: String
  let `retryable`: Bool
  let `size_bytes`: UInt64?
  private enum CodingKeys: String, CodingKey { case `code`, `entry_id`, `limit_bytes`, `message`, `retryable`, `size_bytes` }
  init(from decoder: Decoder) throws {
    let c = try decoder.container(keyedBy: CodingKeys.self)
    self.`code` = try c.decode(ContractPreviewErrorCode.self, forKey: .`code`)
    guard c.contains(.`entry_id`) else { throw DecodingError.keyNotFound(CodingKeys.`entry_id`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`entry_id` = try c.decodeIfPresent(Int64.self, forKey: .`entry_id`)
    guard c.contains(.`limit_bytes`) else { throw DecodingError.keyNotFound(CodingKeys.`limit_bytes`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`limit_bytes` = try c.decodeIfPresent(UInt64.self, forKey: .`limit_bytes`)
    self.`message` = try c.decode(String.self, forKey: .`message`)
    self.`retryable` = try c.decode(Bool.self, forKey: .`retryable`)
    guard c.contains(.`size_bytes`) else { throw DecodingError.keyNotFound(CodingKeys.`size_bytes`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`size_bytes` = try c.decodeIfPresent(UInt64.self, forKey: .`size_bytes`)
  }
}

struct ContractPreviewFrameMetadata: Codable, Sendable {
  let `batch`: ContractPreviewBatchNavigation?
  let `entry_id`: Int64
  let `height`: UInt32?
  let `kind`: String
  let `name`: String
  let `request_id`: String?
  let `size_bytes`: UInt64
  let `width`: UInt32?
  private enum CodingKeys: String, CodingKey { case `batch`, `entry_id`, `height`, `kind`, `name`, `request_id`, `size_bytes`, `width` }
  init(from decoder: Decoder) throws {
    let c = try decoder.container(keyedBy: CodingKeys.self)
    guard c.contains(.`batch`) else { throw DecodingError.keyNotFound(CodingKeys.`batch`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`batch` = try c.decodeIfPresent(ContractPreviewBatchNavigation.self, forKey: .`batch`)
    self.`entry_id` = try c.decode(Int64.self, forKey: .`entry_id`)
    guard self.`entry_id` >= 1 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
    guard c.contains(.`height`) else { throw DecodingError.keyNotFound(CodingKeys.`height`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`height` = try c.decodeIfPresent(UInt32.self, forKey: .`height`)
    self.`kind` = try c.decode(String.self, forKey: .`kind`)
    guard ["text","image","file"].contains(self.`kind`) else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
    self.`name` = try c.decode(String.self, forKey: .`name`)
    guard self.`name`.count >= 1 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
    self.`request_id` = try c.decodeIfPresent(String.self, forKey: .`request_id`)
    self.`size_bytes` = try c.decode(UInt64.self, forKey: .`size_bytes`)
    guard self.`size_bytes` <= 67108864 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
    guard c.contains(.`width`) else { throw DecodingError.keyNotFound(CodingKeys.`width`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`width` = try c.decodeIfPresent(UInt32.self, forKey: .`width`)
  }
}

enum ContractPreviewKind: String, Codable, Sendable {
  case `text` = "text"
  case `image` = "image"
  case `file` = "file"
}

struct ContractRuntimeNotification: Codable, Sendable {
  let `id`: UInt64
  let `level`: String
  let `message`: String
  private enum CodingKeys: String, CodingKey { case `id`, `level`, `message` }
  init(from decoder: Decoder) throws {
    let c = try decoder.container(keyedBy: CodingKeys.self)
    self.`id` = try c.decode(UInt64.self, forKey: .`id`)
    self.`level` = try c.decode(String.self, forKey: .`level`)
    self.`message` = try c.decode(String.self, forKey: .`message`)
  }
}

enum ContractStableErrorCode: String, Codable, Sendable {
  case `invalid_argument` = "invalid_argument"
  case `not_found` = "not_found"
  case `temporarily_busy` = "temporarily_busy"
  case `storage_unavailable` = "storage_unavailable"
  case `unauthorized` = "unauthorized"
  case `protocol_incompatible` = "protocol_incompatible"
  case `internal_error` = "internal_error"
  init(from decoder: Decoder) throws {
    let value = try decoder.singleValueContainer().decode(String.self)
    self = Self(rawValue: value) ?? .internal_error
  }
  func encode(to encoder: Encoder) throws {
    var container = encoder.singleValueContainer()
    try container.encode(rawValue)
  }
}

enum ContractStableErrorDetailClass: String, Codable, Sendable {
  case `request` = "request"
  case `resource` = "resource"
  case `contention` = "contention"
  case `storage` = "storage"
  case `authorization` = "authorization"
  case `protocol` = "protocol"
  case `internal` = "internal"
}

struct ContractStableErrorEnvelope: Codable, Sendable {
  let schema_version: UInt32
  let code: ContractStableErrorCode
  let retryable: Bool
  let message_key: String
  let detail_class: ContractStableErrorDetailClass
  private enum CodingKeys: String, CodingKey { case schema_version, code, retryable, message_key, detail_class }
  init(from decoder: Decoder) throws {
    let c = try decoder.container(keyedBy: CodingKeys.self)
    let version = try c.decode(UInt32.self, forKey: .schema_version)
    guard version == 1 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Unsupported stable error schema version")) }
    self.schema_version = version
    let rawCode = try c.decode(String.self, forKey: .code)
    _ = try c.decode(Bool.self, forKey: .retryable)
    _ = try c.decode(String.self, forKey: .message_key)
    _ = try c.decode(String.self, forKey: .detail_class)
    self.code = ContractStableErrorCode(rawValue: rawCode) ?? .internal_error
    switch self.code {
    case .invalid_argument:
      self.retryable = false
      self.message_key = "error.invalid_argument"
      self.detail_class = .request
    case .not_found:
      self.retryable = false
      self.message_key = "error.not_found"
      self.detail_class = .resource
    case .temporarily_busy:
      self.retryable = true
      self.message_key = "error.temporarily_busy"
      self.detail_class = .contention
    case .storage_unavailable:
      self.retryable = true
      self.message_key = "error.storage_unavailable"
      self.detail_class = .storage
    case .unauthorized:
      self.retryable = false
      self.message_key = "error.unauthorized"
      self.detail_class = .authorization
    case .protocol_incompatible:
      self.retryable = false
      self.message_key = "error.protocol_incompatible"
      self.detail_class = .protocol
    case .internal_error:
      self.retryable = false
      self.message_key = "error.internal"
      self.detail_class = .internal
    }
  }
}

struct ContractStorageStatus: Codable, Sendable {
  let `available`: Bool
  let `error`: String?
  let `quota_bytes`: UInt64
  let `root`: String
  let `used_bytes`: UInt64
  private enum CodingKeys: String, CodingKey { case `available`, `error`, `quota_bytes`, `root`, `used_bytes` }
  init(from decoder: Decoder) throws {
    let c = try decoder.container(keyedBy: CodingKeys.self)
    self.`available` = try c.decode(Bool.self, forKey: .`available`)
    guard c.contains(.`error`) else { throw DecodingError.keyNotFound(CodingKeys.`error`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`error` = try c.decodeIfPresent(String.self, forKey: .`error`)
    self.`quota_bytes` = try c.decode(UInt64.self, forKey: .`quota_bytes`)
    self.`root` = try c.decode(String.self, forKey: .`root`)
    self.`used_bytes` = try c.decode(UInt64.self, forKey: .`used_bytes`)
  }
}

struct ContractSyncWarning: Codable, Sendable {
  let `kind`: String
  let `occurred_at_ms`: Int64
  let `peer`: String
  private enum CodingKeys: String, CodingKey { case `kind`, `occurred_at_ms`, `peer` }
  init(from decoder: Decoder) throws {
    let c = try decoder.container(keyedBy: CodingKeys.self)
    self.`kind` = try c.decode(String.self, forKey: .`kind`)
    self.`occurred_at_ms` = try c.decode(Int64.self, forKey: .`occurred_at_ms`)
    self.`peer` = try c.decode(String.self, forKey: .`peer`)
  }
}

struct ContractWindowsRuntimeSnapshot: Codable, Sendable {
  let `history_version`: UInt64
  let `notifications`: [ContractRuntimeNotification]
  let `progress`: ContractFileProgress?
  let `revision`: UInt64
  let `sync_warning`: ContractSyncWarning?
  private enum CodingKeys: String, CodingKey { case `history_version`, `notifications`, `progress`, `revision`, `sync_warning` }
  init(from decoder: Decoder) throws {
    let c = try decoder.container(keyedBy: CodingKeys.self)
    self.`history_version` = try c.decode(UInt64.self, forKey: .`history_version`)
    self.`notifications` = try c.decode([ContractRuntimeNotification].self, forKey: .`notifications`)
    guard c.contains(.`progress`) else { throw DecodingError.keyNotFound(CodingKeys.`progress`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`progress` = try c.decodeIfPresent(ContractFileProgress.self, forKey: .`progress`)
    self.`revision` = try c.decode(UInt64.self, forKey: .`revision`)
    guard c.contains(.`sync_warning`) else { throw DecodingError.keyNotFound(CodingKeys.`sync_warning`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`sync_warning` = try c.decodeIfPresent(ContractSyncWarning.self, forKey: .`sync_warning`)
  }
}
