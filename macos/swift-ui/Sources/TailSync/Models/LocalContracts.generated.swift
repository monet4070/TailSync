// Generated from production Rust DTOs by shared/schema/generate-local-contracts.mjs. Do not edit.
import Foundation


enum ContractDocumentPolicies {
  static let maxMetadataBytes: UInt64 = 1048576
  static let maxModelBytes: UInt64 = 4194304
  static let maxBytes: UInt64 = 16777216
}

private func documentContractFailure() -> DecodingError { .dataCorrupted(.init(codingPath: [], debugDescription: "Invalid document contract")) }
private func validContractDocumentLink(_ value: String) -> Bool {
  guard !value.isEmpty, value.utf8.count <= 2048,
        !value.unicodeScalars.contains(where: { $0.properties.isWhitespace || $0.value <= 31 || (127...159).contains($0.value) || $0.value == 92 }),
        let components = URLComponents(string: value), let scheme = components.scheme?.lowercased(),
        ["http", "https"].contains(scheme), let host = components.host, !host.isEmpty,
        components.port.map({ (0...65535).contains($0) }) ?? true,
        !host.unicodeScalars.contains(where: { $0.properties.isWhitespace || $0.value == 64 || $0.value <= 31 || (127...159).contains($0.value) }),
        components.user == nil, components.password == nil,
        value.range(of: "^https?://[^/?#]+", options: [.regularExpression, .caseInsensitive]) != nil else { return false }
  return true
}
private func validateContractDocument(_ value: ContractDisplayDocument) throws {
  var textBytes = 0, runs = 0, cells = 0, images = 0
  var offset: UInt64 = 0, pixels: UInt64 = 0
  var first: UInt64?
  var used = Set<UInt64>()
  func validateRuns(_ items: [ContractDocumentRun]) throws {
    runs += items.count
    for run in items {
      guard !run.text.contains("\0") else { throw documentContractFailure() }; textBytes += run.text.utf8.count
      if let link = run.link { guard validContractDocumentLink(link) else { throw documentContractFailure() }; textBytes += link.utf8.count }
    }
    guard runs <= 8192, textBytes <= 1048576 else { throw documentContractFailure() }
  }
  guard value.blocks.count <= 8192, value.assets.count <= 64, value.partial.count <= 8192 else { throw documentContractFailure() }
  for block in value.blocks {
    switch block.kind {
    case .paragraph:
      guard let paragraph = block.paragraph, block.table == nil, block.image == nil, paragraph.list != nil || (paragraph.list_depth ?? 0) == 0 else { throw documentContractFailure() }; try validateRuns(paragraph.runs)
    case .table:
      guard let table = block.table, block.paragraph == nil, block.image == nil, table.rows.count <= 2048 else { throw documentContractFailure() }
      for row in table.rows { guard !row.cells.isEmpty, row.cells.count <= 2048 else { throw documentContractFailure() }; for cell in row.cells { cells += 1; guard cells <= 2048 else { throw documentContractFailure() }; try validateRuns(cell.runs) } }
    case .image:
      guard let image = block.image, block.paragraph == nil, block.table == nil, image.ordinal == UInt64(images), (image.availability == .available) == (image.asset != nil) else { throw documentContractFailure() }
      if images == 0 { first = image.asset }; images += 1
      if let asset = image.asset { guard asset < UInt64(value.assets.count) else { throw documentContractFailure() }; used.insert(asset) }
    }
  }
  guard images <= 64, value.first_image == first, used.count == value.assets.count else { throw documentContractFailure() }
  for (index, asset) in value.assets.enumerated() {
    guard asset.index == UInt64(index), asset.offset == offset, asset.length > 0, asset.length <= 16777216, asset.width > 0, asset.height > 0, asset.width <= 16000000, asset.height <= 16000000, asset.sha256.count == 64, asset.sha256.allSatisfy({ "0123456789abcdefABCDEF".contains($0) }) else { throw documentContractFailure() }
    offset += asset.length; pixels += UInt64(asset.width) * UInt64(asset.height)
    guard offset <= 16777216, pixels <= 16000000 else { throw documentContractFailure() }
  }
  for reason in value.partial { guard !reason.detail.contains("\0"), reason.detail.utf8.count <= 512 else { throw documentContractFailure() } }
}
private func validateContractDocumentFrame(_ value: ContractDocumentFrameMetadata) throws {
  guard let request = value.request_id, !request.isEmpty, request.utf8.count <= 128, !value.name.isEmpty, value.name.utf8.count <= 256, value.stamp.hasPrefix("dc1:"), value.stamp.count == 68, value.stamp.dropFirst(4).allSatisfy({ "0123456789abcdefABCDEF".contains($0) }), value.model_bytes > 0, value.model_bytes <= value.size_bytes, value.batch == nil else { throw documentContractFailure() }
}
struct ContractActiveRoute: Codable, Sendable {
  let `address`: String
  let `interface`: ContractConnectionInterface
  let `latency`: UInt64
  private enum CodingKeys: String, CodingKey { case `address`, `interface`, `latency` }
  init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`address` = try container.decode(String.self, forKey: .`address`)
    self.`interface` = try container.decode(ContractConnectionInterface.self, forKey: .`interface`)
    self.`latency` = try container.decode(UInt64.self, forKey: .`latency`)
  }
}

struct ContractArchiveEntryImagesReport: Codable, Sendable {
  let `archived`: UInt64
  let `complete`: Bool
  let `deduplicated`: UInt64
  let `failures`: [ContractArchiveResourceFailure]
  private enum CodingKeys: String, CodingKey { case `archived`, `complete`, `deduplicated`, `failures` }
  init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`archived` = try container.decode(UInt64.self, forKey: .`archived`)
    guard self.`archived` <= 65535 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
    self.`complete` = try container.decode(Bool.self, forKey: .`complete`)
    self.`deduplicated` = try container.decode(UInt64.self, forKey: .`deduplicated`)
    guard self.`deduplicated` <= 65535 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
    self.`failures` = try container.decode([ContractArchiveResourceFailure].self, forKey: .`failures`)
  }
}

struct ContractArchiveResourceFailure: Codable, Sendable {
  let `code`: String
  let `image_index`: UInt64
  private enum CodingKeys: String, CodingKey { case `code`, `image_index` }
  init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`code` = try container.decode(String.self, forKey: .`code`)
    self.`image_index` = try container.decode(UInt64.self, forKey: .`image_index`)
    guard self.`image_index` <= 65535 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
  }
}

struct ContractClipboardArrival: Codable, Sendable {
  let `count`: UInt64
  let `entry_id`: Int64?
  let `event_id`: String
  let `kind`: String
  let `source_id`: String
  let `source_name`: String
  private enum CodingKeys: String, CodingKey { case `count`, `entry_id`, `event_id`, `kind`, `source_id`, `source_name` }
  init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`count` = try container.decode(UInt64.self, forKey: .`count`)
    guard container.contains(.`entry_id`) else { throw DecodingError.keyNotFound(CodingKeys.`entry_id`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`entry_id` = try container.decodeIfPresent(Int64.self, forKey: .`entry_id`)
    self.`event_id` = try container.decode(String.self, forKey: .`event_id`)
    self.`kind` = try container.decode(String.self, forKey: .`kind`)
    self.`source_id` = try container.decode(String.self, forKey: .`source_id`)
    self.`source_name` = try container.decode(String.self, forKey: .`source_name`)
  }
}

struct ContractCompositeInfo: Codable, Sendable {
  let `completeness`: String
  let `formats`: [String]
  let `identity`: ContractHistoryIdentity?
  let `image_count`: UInt64
  let `native_item_count`: UInt64
  let `native_only`: Bool
  let `representation_count`: UInt64
  let `restorable_formats`: [String]
  let `withheld_formats`: [ContractWithheldRestoreFormat]
  private enum CodingKeys: String, CodingKey { case `completeness`, `formats`, `identity`, `image_count`, `native_item_count`, `native_only`, `representation_count`, `restorable_formats`, `withheld_formats` }
  init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`completeness` = try container.decode(String.self, forKey: .`completeness`)
    guard ["complete","external_references","missing_formats"].contains(self.`completeness`) else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
    self.`formats` = try container.decode([String].self, forKey: .`formats`)
    self.`identity` = try container.decodeIfPresent(ContractHistoryIdentity.self, forKey: .`identity`)
    self.`image_count` = try container.decode(UInt64.self, forKey: .`image_count`)
    guard self.`image_count` <= 65535 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
    self.`native_item_count` = try container.decode(UInt64.self, forKey: .`native_item_count`)
    guard self.`native_item_count` <= 65535 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
    self.`native_only` = try container.decode(Bool.self, forKey: .`native_only`)
    self.`representation_count` = try container.decode(UInt64.self, forKey: .`representation_count`)
    guard self.`representation_count` <= 65535 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
    self.`restorable_formats` = try container.decode([String].self, forKey: .`restorable_formats`)
    self.`withheld_formats` = try container.decode([ContractWithheldRestoreFormat].self, forKey: .`withheld_formats`)
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
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`active_routes` = try container.decode([String: ContractActiveRoute].self, forKey: .`active_routes`)
    self.`clipboard_monitor_failures` = try container.decode(UInt64.self, forKey: .`clipboard_monitor_failures`)
    self.`clipboard_monitor_healthy` = try container.decode(Bool.self, forKey: .`clipboard_monitor_healthy`)
    self.`tcp_server_healthy` = try container.decode(Bool.self, forKey: .`tcp_server_healthy`)
  }
}

struct ContractDisplayDocument: Codable, Sendable {
  let `assets`: [ContractDocumentAsset]
  let `blocks`: [ContractDocumentBlock]
  let `first_image`: UInt64?
  let `partial`: [ContractDocumentPartial]
  let `styles_partial`: Bool
  let `version`: UInt64
  private enum CodingKeys: String, CodingKey { case `assets`, `blocks`, `first_image`, `partial`, `styles_partial`, `version` }
  init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`assets` = try container.decode([ContractDocumentAsset].self, forKey: .`assets`)
    self.`blocks` = try container.decode([ContractDocumentBlock].self, forKey: .`blocks`)
    self.`first_image` = try container.decodeIfPresent(UInt64.self, forKey: .`first_image`)
    if let value = self.`first_image` { guard value <= 65535 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) } }
    self.`partial` = try container.decode([ContractDocumentPartial].self, forKey: .`partial`)
    self.`styles_partial` = try container.decode(Bool.self, forKey: .`styles_partial`)
    self.`version` = try container.decode(UInt64.self, forKey: .`version`)
    guard self.`version` >= 1, self.`version` <= 1 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
    try validateContractDocument(self)
  }
}

struct ContractDocumentAsset: Codable, Sendable {
  let `height`: UInt32
  let `index`: UInt64
  let `length`: UInt64
  let `offset`: UInt64
  let `sha256`: String
  let `width`: UInt32
  private enum CodingKeys: String, CodingKey { case `height`, `index`, `length`, `offset`, `sha256`, `width` }
  init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`height` = try container.decode(UInt32.self, forKey: .`height`)
    self.`index` = try container.decode(UInt64.self, forKey: .`index`)
    guard self.`index` <= 65535 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
    self.`length` = try container.decode(UInt64.self, forKey: .`length`)
    self.`offset` = try container.decode(UInt64.self, forKey: .`offset`)
    self.`sha256` = try container.decode(String.self, forKey: .`sha256`)
    self.`width` = try container.decode(UInt32.self, forKey: .`width`)
  }
}

struct ContractDocumentBlock: Codable, Sendable {
  let `image`: ContractDocumentImage?
  let `kind`: ContractDocumentBlockKind
  let `paragraph`: ContractDocumentParagraph?
  let `table`: ContractDocumentTable?
  private enum CodingKeys: String, CodingKey { case `image`, `kind`, `paragraph`, `table` }
  init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`image` = try container.decodeIfPresent(ContractDocumentImage.self, forKey: .`image`)
    self.`kind` = try container.decode(ContractDocumentBlockKind.self, forKey: .`kind`)
    self.`paragraph` = try container.decodeIfPresent(ContractDocumentParagraph.self, forKey: .`paragraph`)
    self.`table` = try container.decodeIfPresent(ContractDocumentTable.self, forKey: .`table`)
  }
}

enum ContractDocumentBlockKind: String, Codable, Sendable {
  case `paragraph` = "paragraph"
  case `table` = "table"
  case `image` = "image"
}

struct ContractDocumentColor: Codable, Sendable {
  let `blue`: UInt8
  let `green`: UInt8
  let `red`: UInt8
  private enum CodingKeys: String, CodingKey { case `blue`, `green`, `red` }
  init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`blue` = try container.decode(UInt8.self, forKey: .`blue`)
    self.`green` = try container.decode(UInt8.self, forKey: .`green`)
    self.`red` = try container.decode(UInt8.self, forKey: .`red`)
  }
}

struct ContractDocumentFrameMetadata: Codable, Sendable {
  let `batch`: ContractPreviewBatchNavigation?
  let `entry_id`: Int64
  let `generation`: UInt64
  let `kind`: String
  let `model_bytes`: UInt64
  let `name`: String
  let `owner`: String
  let `request_id`: String?
  let `size_bytes`: UInt64
  let `stamp`: String
  private enum CodingKeys: String, CodingKey { case `batch`, `entry_id`, `generation`, `kind`, `model_bytes`, `name`, `owner`, `request_id`, `size_bytes`, `stamp` }
  init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`batch` = try container.decodeIfPresent(ContractPreviewBatchNavigation.self, forKey: .`batch`)
    self.`entry_id` = try container.decode(Int64.self, forKey: .`entry_id`)
    guard self.`entry_id` >= 1 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
    self.`generation` = try container.decode(UInt64.self, forKey: .`generation`)
    guard self.`generation` <= 9007199254740991 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
    self.`kind` = try container.decode(String.self, forKey: .`kind`)
    guard ["document"].contains(self.`kind`) else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
    self.`model_bytes` = try container.decode(UInt64.self, forKey: .`model_bytes`)
    guard self.`model_bytes` <= 4194304 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
    self.`name` = try container.decode(String.self, forKey: .`name`)
    guard self.`name`.count >= 1 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
    self.`owner` = try container.decode(String.self, forKey: .`owner`)
    guard ["history","favorites"].contains(self.`owner`) else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
    self.`request_id` = try container.decodeIfPresent(String.self, forKey: .`request_id`)
    self.`size_bytes` = try container.decode(UInt64.self, forKey: .`size_bytes`)
    guard self.`size_bytes` <= 16777216 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
    self.`stamp` = try container.decode(String.self, forKey: .`stamp`)
    guard self.`stamp`.count >= 1 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
    try validateContractDocumentFrame(self)
  }
}

struct ContractDocumentImage: Codable, Sendable {
  let `asset`: UInt64?
  let `availability`: ContractDocumentImageAvailability
  let `ordinal`: UInt64
  private enum CodingKeys: String, CodingKey { case `asset`, `availability`, `ordinal` }
  init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`asset` = try container.decodeIfPresent(UInt64.self, forKey: .`asset`)
    if let value = self.`asset` { guard value <= 65535 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) } }
    self.`availability` = try container.decode(ContractDocumentImageAvailability.self, forKey: .`availability`)
    self.`ordinal` = try container.decode(UInt64.self, forKey: .`ordinal`)
    guard self.`ordinal` <= 65535 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
  }
}

enum ContractDocumentImageAvailability: String, Codable, Sendable {
  case `available` = "available"
  case `missing_resource` = "missing_resource"
  case `missing_source` = "missing_source"
  case `external_reference` = "external_reference"
  case `unsupported` = "unsupported"
  case `pixel_budget` = "pixel_budget"
}

enum ContractDocumentListKind: String, Codable, Sendable {
  case `bullet` = "bullet"
  case `ordered` = "ordered"
}

struct ContractDocumentParagraph: Codable, Sendable {
  let `code_block`: Bool?
  let `heading`: UInt8?
  let `list`: ContractDocumentListKind?
  let `list_depth`: UInt8?
  let `quote_depth`: UInt8?
  let `runs`: [ContractDocumentRun]
  private enum CodingKeys: String, CodingKey { case `code_block`, `heading`, `list`, `list_depth`, `quote_depth`, `runs` }
  init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`code_block` = container.contains(.`code_block`) ? try container.decode(Bool.self, forKey: .`code_block`) : nil
    self.`heading` = try container.decodeIfPresent(UInt8.self, forKey: .`heading`)
    if let value = self.`heading` { guard value >= 1, value <= 3 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) } }
    self.`list` = try container.decodeIfPresent(ContractDocumentListKind.self, forKey: .`list`)
    self.`list_depth` = container.contains(.`list_depth`) ? try container.decode(UInt8.self, forKey: .`list_depth`) : nil
    if let value = self.`list_depth` { guard value <= 8 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) } }
    self.`quote_depth` = container.contains(.`quote_depth`) ? try container.decode(UInt8.self, forKey: .`quote_depth`) : nil
    if let value = self.`quote_depth` { guard value <= 8 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) } }
    self.`runs` = try container.decode([ContractDocumentRun].self, forKey: .`runs`)
  }
}

struct ContractDocumentPartial: Codable, Sendable {
  let `code`: ContractDocumentPartialCode
  let `detail`: String
  private enum CodingKeys: String, CodingKey { case `code`, `detail` }
  init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`code` = try container.decode(ContractDocumentPartialCode.self, forKey: .`code`)
    self.`detail` = try container.decode(String.self, forKey: .`detail`)
  }
}

enum ContractDocumentPartialCode: String, Codable, Sendable {
  case `text_limit` = "text_limit"
  case `block_limit` = "block_limit"
  case `run_limit` = "run_limit"
  case `cell_limit` = "cell_limit"
  case `image_limit` = "image_limit"
  case `pixel_budget` = "pixel_budget"
  case `depth_limit` = "depth_limit"
  case `malformed_html` = "malformed_html"
  case `unsupported_markup` = "unsupported_markup"
  case `unsupported_dependency` = "unsupported_dependency"
  case `missing_image_source` = "missing_image_source"
  case `external_reference` = "external_reference"
  case `native_analysis_unavailable` = "native_analysis_unavailable"
  case `native_styles_unavailable` = "native_styles_unavailable"
  case `native_table_structure_unavailable` = "native_table_structure_unavailable"
  case `native_partial` = "native_partial"
  case `unverified_provenance` = "unverified_provenance"
  case `other_native_items` = "other_native_items"
}

struct ContractDocumentRequest: Codable, Sendable {
  let `entry_id`: Int64
  let `generation`: UInt64
  let `owner`: String
  let `request_id`: String
  private enum CodingKeys: String, CodingKey { case `entry_id`, `generation`, `owner`, `request_id` }
  init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`entry_id` = try container.decode(Int64.self, forKey: .`entry_id`)
    guard self.`entry_id` >= 1 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
    self.`generation` = try container.decode(UInt64.self, forKey: .`generation`)
    guard self.`generation` <= 9007199254740991 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
    self.`owner` = try container.decode(String.self, forKey: .`owner`)
    guard ["history","favorites"].contains(self.`owner`) else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
    self.`request_id` = try container.decode(String.self, forKey: .`request_id`)
    guard self.`request_id`.count >= 1, self.`request_id`.count <= 128 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
  }
}

struct ContractDocumentRun: Codable, Sendable {
  let `link`: String?
  let `style`: ContractDocumentRunStyle
  let `text`: String
  let `unsupported`: Bool
  private enum CodingKeys: String, CodingKey { case `link`, `style`, `text`, `unsupported` }
  init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`link` = try container.decodeIfPresent(String.self, forKey: .`link`)
    self.`style` = try container.decode(ContractDocumentRunStyle.self, forKey: .`style`)
    self.`text` = try container.decode(String.self, forKey: .`text`)
    self.`unsupported` = try container.decode(Bool.self, forKey: .`unsupported`)
  }
}

struct ContractDocumentRunStyle: Codable, Sendable {
  let `bold`: Bool
  let `color`: ContractDocumentColor?
  let `highlight`: ContractDocumentColor?
  let `italic`: Bool
  let `monospace`: Bool?
  let `strikethrough`: Bool
  let `subscript`: Bool
  let `superscript`: Bool
  let `underline`: Bool
  private enum CodingKeys: String, CodingKey { case `bold`, `color`, `highlight`, `italic`, `monospace`, `strikethrough`, `subscript`, `superscript`, `underline` }
  init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`bold` = try container.decode(Bool.self, forKey: .`bold`)
    self.`color` = try container.decodeIfPresent(ContractDocumentColor.self, forKey: .`color`)
    self.`highlight` = try container.decodeIfPresent(ContractDocumentColor.self, forKey: .`highlight`)
    self.`italic` = try container.decode(Bool.self, forKey: .`italic`)
    self.`monospace` = container.contains(.`monospace`) ? try container.decode(Bool.self, forKey: .`monospace`) : nil
    self.`strikethrough` = try container.decode(Bool.self, forKey: .`strikethrough`)
    self.`subscript` = try container.decode(Bool.self, forKey: .`subscript`)
    self.`superscript` = try container.decode(Bool.self, forKey: .`superscript`)
    self.`underline` = try container.decode(Bool.self, forKey: .`underline`)
  }
}

struct ContractDocumentTable: Codable, Sendable {
  let `rows`: [ContractDocumentTableRow]
  let `truncated`: Bool
  private enum CodingKeys: String, CodingKey { case `rows`, `truncated` }
  init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`rows` = try container.decode([ContractDocumentTableRow].self, forKey: .`rows`)
    self.`truncated` = try container.decode(Bool.self, forKey: .`truncated`)
  }
}

struct ContractDocumentTableCell: Codable, Sendable {
  let `header`: Bool
  let `runs`: [ContractDocumentRun]
  private enum CodingKeys: String, CodingKey { case `header`, `runs` }
  init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`header` = try container.decode(Bool.self, forKey: .`header`)
    self.`runs` = try container.decode([ContractDocumentRun].self, forKey: .`runs`)
  }
}

struct ContractDocumentTableRow: Codable, Sendable {
  let `cells`: [ContractDocumentTableCell]
  private enum CodingKeys: String, CodingKey { case `cells` }
  init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`cells` = try container.decode([ContractDocumentTableCell].self, forKey: .`cells`)
  }
}

struct ContractFavoriteMutation: Codable, Sendable {
  let `affected_ids`: [Int64]
  let `favorite`: Bool
  private enum CodingKeys: String, CodingKey { case `affected_ids`, `favorite` }
  init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`affected_ids` = try container.decode([Int64].self, forKey: .`affected_ids`)
    self.`favorite` = try container.decode(Bool.self, forKey: .`favorite`)
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
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`active` = try container.decode(Bool.self, forKey: .`active`)
    self.`batch_id` = try container.decode(String.self, forKey: .`batch_id`)
    self.`can_stop` = try container.decode(Bool.self, forKey: .`can_stop`)
    self.`completed_files` = try container.decode(UInt64.self, forKey: .`completed_files`)
    self.`device` = try container.decode(String.self, forKey: .`device`)
    self.`direction` = try container.decode(String.self, forKey: .`direction`)
    guard ["sending","receiving"].contains(self.`direction`) else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
    self.`name` = try container.decode(String.self, forKey: .`name`)
    self.`sent` = try container.decode(UInt64.self, forKey: .`sent`)
    self.`speed_bytes_per_second` = try container.decode(UInt64.self, forKey: .`speed_bytes_per_second`)
    self.`status` = try container.decode(String.self, forKey: .`status`)
    self.`total` = try container.decode(UInt64.self, forKey: .`total`)
    self.`total_files` = try container.decode(UInt64.self, forKey: .`total_files`)
  }
}

struct ContractHistoryDisplayInfo: Codable, Sendable {
  let `cache_key`: String
  let `kind`: String
  let `thumbnail_eligible`: Bool
  let `version`: UInt64
  private enum CodingKeys: String, CodingKey { case `cache_key`, `kind`, `thumbnail_eligible`, `version` }
  init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`cache_key` = try container.decode(String.self, forKey: .`cache_key`)
    self.`kind` = try container.decode(String.self, forKey: .`kind`)
    guard ["image","document","text","file","unknown"].contains(self.`kind`) else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
    self.`thumbnail_eligible` = try container.decode(Bool.self, forKey: .`thumbnail_eligible`)
    self.`version` = try container.decode(UInt64.self, forKey: .`version`)
    guard self.`version` >= 1, self.`version` <= 1 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
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
  let `composite`: ContractCompositeInfo?
  let `data_hash`: String
  let `description`: String
  let `display`: ContractHistoryDisplayInfo?
  let `id`: Int64
  let `pinned`: Bool
  let `size_bytes`: Int64
  let `source_peer`: String
  let `timestamp`: String
  let `type`: String
  private enum CodingKeys: String, CodingKey { case `batch_count`, `batch_id`, `batch_index`, `batch_status`, `batch_total`, `categories`, `category`, `category_confidence`, `classifier_version`, `composite`, `data_hash`, `description`, `display`, `id`, `pinned`, `size_bytes`, `source_peer`, `timestamp`, `type` }
  init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    guard container.contains(.`batch_count`) else { throw DecodingError.keyNotFound(CodingKeys.`batch_count`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`batch_count` = try container.decodeIfPresent(Int64.self, forKey: .`batch_count`)
    guard container.contains(.`batch_id`) else { throw DecodingError.keyNotFound(CodingKeys.`batch_id`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`batch_id` = try container.decodeIfPresent(String.self, forKey: .`batch_id`)
    guard container.contains(.`batch_index`) else { throw DecodingError.keyNotFound(CodingKeys.`batch_index`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`batch_index` = try container.decodeIfPresent(Int64.self, forKey: .`batch_index`)
    self.`batch_status` = try container.decode(String.self, forKey: .`batch_status`)
    guard ["complete","incomplete"].contains(self.`batch_status`) else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
    guard container.contains(.`batch_total`) else { throw DecodingError.keyNotFound(CodingKeys.`batch_total`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`batch_total` = try container.decodeIfPresent(Int64.self, forKey: .`batch_total`)
    self.`categories` = try container.decode([String].self, forKey: .`categories`)
    self.`category` = try container.decode(String.self, forKey: .`category`)
    guard ["text","website","code","command","structured_data","path","image","file"].contains(self.`category`) else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
    self.`category_confidence` = try container.decode(Int64.self, forKey: .`category_confidence`)
    self.`classifier_version` = try container.decode(Int64.self, forKey: .`classifier_version`)
    self.`composite` = try container.decodeIfPresent(ContractCompositeInfo.self, forKey: .`composite`)
    self.`data_hash` = try container.decode(String.self, forKey: .`data_hash`)
    self.`description` = try container.decode(String.self, forKey: .`description`)
    self.`display` = try container.decodeIfPresent(ContractHistoryDisplayInfo.self, forKey: .`display`)
    self.`id` = try container.decode(Int64.self, forKey: .`id`)
    self.`pinned` = try container.decode(Bool.self, forKey: .`pinned`)
    self.`size_bytes` = try container.decode(Int64.self, forKey: .`size_bytes`)
    self.`source_peer` = try container.decode(String.self, forKey: .`source_peer`)
    self.`timestamp` = try container.decode(String.self, forKey: .`timestamp`)
    self.`type` = try container.decode(String.self, forKey: .`type`)
    guard ["text","image","file","composite"].contains(self.`type`) else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
  }
}

struct ContractHistoryIdentity: Codable, Sendable {
  let `archive_digest`: String
  let `archive_revision`: Int64
  let `capture_id`: String
  let `content_id`: String
  let `dedupe_key`: String?
  let `version`: UInt8
  private enum CodingKeys: String, CodingKey { case `archive_digest`, `archive_revision`, `capture_id`, `content_id`, `dedupe_key`, `version` }
  init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`archive_digest` = try container.decode(String.self, forKey: .`archive_digest`)
    self.`archive_revision` = try container.decode(Int64.self, forKey: .`archive_revision`)
    guard self.`archive_revision` >= 1, self.`archive_revision` <= 9007199254740991 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
    self.`capture_id` = try container.decode(String.self, forKey: .`capture_id`)
    self.`content_id` = try container.decode(String.self, forKey: .`content_id`)
    guard container.contains(.`dedupe_key`) else { throw DecodingError.keyNotFound(CodingKeys.`dedupe_key`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`dedupe_key` = try container.decodeIfPresent(String.self, forKey: .`dedupe_key`)
    self.`version` = try container.decode(UInt8.self, forKey: .`version`)
    guard self.`version` >= 1, self.`version` <= 1 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
  }
}

struct ContractHistoryQueryPage: Codable, Sendable {
  let `display_index_pending`: Bool?
  let `entries`: [ContractHistoryEntry]
  let `has_more`: Bool
  let `total`: UInt64?
  private enum CodingKeys: String, CodingKey { case `display_index_pending`, `entries`, `has_more`, `total` }
  init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`display_index_pending` = try container.decodeIfPresent(Bool.self, forKey: .`display_index_pending`)
    self.`entries` = try container.decode([ContractHistoryEntry].self, forKey: .`entries`)
    self.`has_more` = try container.decode(Bool.self, forKey: .`has_more`)
    guard container.contains(.`total`) else { throw DecodingError.keyNotFound(CodingKeys.`total`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`total` = try container.decodeIfPresent(UInt64.self, forKey: .`total`)
  }
}

struct ContractHistoryRestoreReport: Codable, Sendable {
  let `complete`: Bool
  let `missing_images`: UInt64
  private enum CodingKeys: String, CodingKey { case `complete`, `missing_images` }
  init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`complete` = try container.decode(Bool.self, forKey: .`complete`)
    self.`missing_images` = try container.decode(UInt64.self, forKey: .`missing_images`)
    guard self.`missing_images` <= 4096 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
  }
}

struct ContractLocalCapabilities: Codable, Sendable {
  let `document_preview_version`: UInt64?
  let `history_display_version`: UInt64?
  let `max_preview_bytes`: UInt64
  let `platform`: String
  let `schema_version`: UInt32
  let `supports_binary_preview`: Bool
  let `supports_runtime_snapshot`: Bool
  let `supports_stable_errors`: Bool
  let `wire_version`: UInt32
  private enum CodingKeys: String, CodingKey { case `document_preview_version`, `history_display_version`, `max_preview_bytes`, `platform`, `schema_version`, `supports_binary_preview`, `supports_runtime_snapshot`, `supports_stable_errors`, `wire_version` }
  init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`document_preview_version` = try container.decodeIfPresent(UInt64.self, forKey: .`document_preview_version`)
    if let value = self.`document_preview_version` { guard value >= 1, value <= 1 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) } }
    self.`history_display_version` = try container.decodeIfPresent(UInt64.self, forKey: .`history_display_version`)
    if let value = self.`history_display_version` { guard value >= 1, value <= 1 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) } }
    self.`max_preview_bytes` = try container.decode(UInt64.self, forKey: .`max_preview_bytes`)
    guard self.`max_preview_bytes` >= 67108864, self.`max_preview_bytes` <= 67108864 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
    self.`platform` = try container.decode(String.self, forKey: .`platform`)
    self.`schema_version` = try container.decode(UInt32.self, forKey: .`schema_version`)
    guard self.`schema_version` >= 2, self.`schema_version` <= 2 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
    self.`supports_binary_preview` = try container.decode(Bool.self, forKey: .`supports_binary_preview`)
    self.`supports_runtime_snapshot` = try container.decode(Bool.self, forKey: .`supports_runtime_snapshot`)
    self.`supports_stable_errors` = try container.decode(Bool.self, forKey: .`supports_stable_errors`)
    self.`wire_version` = try container.decode(UInt32.self, forKey: .`wire_version`)
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
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`connection_mode` = try container.decode(String.self, forKey: .`connection_mode`)
    self.`fingerprint` = try container.decode(String.self, forKey: .`fingerprint`)
    self.`hostname` = try container.decode(String.self, forKey: .`hostname`)
    guard container.contains(.`iroh_endpoint_id`) else { throw DecodingError.keyNotFound(CodingKeys.`iroh_endpoint_id`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`iroh_endpoint_id` = try container.decodeIfPresent(String.self, forKey: .`iroh_endpoint_id`)
    self.`public_key` = try container.decode(String.self, forKey: .`public_key`)
    self.`routes` = try container.decode([ContractPeerRouteSnapshot].self, forKey: .`routes`)
    self.`tailscale_ip` = try container.decode(String.self, forKey: .`tailscale_ip`)
  }
}

struct ContractMacRuntimeSnapshot: Codable, Sendable {
  let `history_version`: UInt64
  let `notification_dropped_total`: UInt64
  let `notification_earliest_available_id`: UInt64
  let `notifications`: [ContractRuntimeNotification]
  let `progress`: ContractFileProgress?
  let `revision`: UInt64
  let `service_instance`: UInt64
  let `status`: ContractDaemonStatus
  let `storage`: ContractStorageStatus
  let `sync_enabled`: Bool
  private enum CodingKeys: String, CodingKey { case `history_version`, `notification_dropped_total`, `notification_earliest_available_id`, `notifications`, `progress`, `revision`, `service_instance`, `status`, `storage`, `sync_enabled` }
  init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`history_version` = try container.decode(UInt64.self, forKey: .`history_version`)
    self.`notification_dropped_total` = try container.decode(UInt64.self, forKey: .`notification_dropped_total`)
    self.`notification_earliest_available_id` = try container.decode(UInt64.self, forKey: .`notification_earliest_available_id`)
    self.`notifications` = try container.decode([ContractRuntimeNotification].self, forKey: .`notifications`)
    guard container.contains(.`progress`) else { throw DecodingError.keyNotFound(CodingKeys.`progress`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`progress` = try container.decodeIfPresent(ContractFileProgress.self, forKey: .`progress`)
    self.`revision` = try container.decode(UInt64.self, forKey: .`revision`)
    self.`service_instance` = try container.decode(UInt64.self, forKey: .`service_instance`)
    self.`status` = try container.decode(ContractDaemonStatus.self, forKey: .`status`)
    self.`storage` = try container.decode(ContractStorageStatus.self, forKey: .`storage`)
    self.`sync_enabled` = try container.decode(Bool.self, forKey: .`sync_enabled`)
  }
}

enum ContractPairingFailureKind: String, Codable, Sendable {
  case `protocol_incompatible` = "protocol_incompatible"
  case `identity_mismatch` = "identity_mismatch"
  case `window_closed` = "window_closed"
  case `timed_out` = "timed_out"
  case `connection_closed` = "connection_closed"
  case `invalid_advertisement` = "invalid_advertisement"
  case `invalid_argument` = "invalid_argument"
  case `busy` = "busy"
  case `cancelled` = "cancelled"
  case `transport` = "transport"
  case `unknown` = "unknown"
  init(from decoder: Decoder) throws {
    let value = try decoder.singleValueContainer().decode(String.self)
    self = Self(rawValue: value) ?? .unknown
  }
  func encode(to encoder: Encoder) throws {
    var container = encoder.singleValueContainer()
    try container.encode(rawValue)
  }
}

struct ContractPairingPeerStatus: Codable, Sendable {
  let `address`: String
  let `fingerprint`: String
  let `hostname`: String
  let `local_confirmed`: Bool
  let `remote_confirmed`: Bool
  let `verification_code`: String
  private enum CodingKeys: String, CodingKey { case `address`, `fingerprint`, `hostname`, `local_confirmed`, `remote_confirmed`, `verification_code` }
  init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`address` = try container.decode(String.self, forKey: .`address`)
    self.`fingerprint` = try container.decode(String.self, forKey: .`fingerprint`)
    self.`hostname` = try container.decode(String.self, forKey: .`hostname`)
    self.`local_confirmed` = try container.decode(Bool.self, forKey: .`local_confirmed`)
    self.`remote_confirmed` = try container.decode(Bool.self, forKey: .`remote_confirmed`)
    self.`verification_code` = try container.decode(String.self, forKey: .`verification_code`)
  }
}

enum ContractPairingPhase: String, Codable, Sendable {
  case `disabled` = "disabled"
  case `waiting` = "waiting"
  case `handshaking` = "handshaking"
  case `verification` = "verification"
  case `waiting_for_peer` = "waiting_for_peer"
  case `finalizing` = "finalizing"
  case `paired` = "paired"
  case `cancelled` = "cancelled"
  case `timed_out` = "timed_out"
  case `locked` = "locked"
}

struct ContractPairingStatus: Codable, Sendable {
  let `error`: String?
  let `expires_at`: UInt64?
  let `failed_attempts`: UInt8
  let `failure_kind`: ContractPairingFailureKind?
  let `max_failures`: UInt8
  let `pairing_enabled`: Bool
  let `peer`: ContractPairingPeerStatus?
  let `pending`: [ContractPendingPairingSummary]
  let `pending_store_unavailable`: Bool
  let `phase`: ContractPairingPhase
  let `remaining_seconds`: UInt64
  private enum CodingKeys: String, CodingKey { case `error`, `expires_at`, `failed_attempts`, `failure_kind`, `max_failures`, `pairing_enabled`, `peer`, `pending`, `pending_store_unavailable`, `phase`, `remaining_seconds` }
  init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    guard container.contains(.`error`) else { throw DecodingError.keyNotFound(CodingKeys.`error`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`error` = try container.decodeIfPresent(String.self, forKey: .`error`)
    guard container.contains(.`expires_at`) else { throw DecodingError.keyNotFound(CodingKeys.`expires_at`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`expires_at` = try container.decodeIfPresent(UInt64.self, forKey: .`expires_at`)
    self.`failed_attempts` = try container.decode(UInt8.self, forKey: .`failed_attempts`)
    self.`failure_kind` = try container.decodeIfPresent(ContractPairingFailureKind.self, forKey: .`failure_kind`)
    self.`max_failures` = try container.decode(UInt8.self, forKey: .`max_failures`)
    self.`pairing_enabled` = try container.decode(Bool.self, forKey: .`pairing_enabled`)
    guard container.contains(.`peer`) else { throw DecodingError.keyNotFound(CodingKeys.`peer`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`peer` = try container.decodeIfPresent(ContractPairingPeerStatus.self, forKey: .`peer`)
    self.`pending` = try container.decode([ContractPendingPairingSummary].self, forKey: .`pending`)
    self.`pending_store_unavailable` = try container.decode(Bool.self, forKey: .`pending_store_unavailable`)
    self.`phase` = try container.decode(ContractPairingPhase.self, forKey: .`phase`)
    self.`remaining_seconds` = try container.decode(UInt64.self, forKey: .`remaining_seconds`)
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
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`address` = try container.decode(String.self, forKey: .`address`)
    self.`interface` = try container.decode(ContractConnectionInterface.self, forKey: .`interface`)
    self.`latency` = try container.decodeIfPresent(UInt64.self, forKey: .`latency`)
    self.`online` = try container.decode(Bool.self, forKey: .`online`)
    self.`priority` = try container.decode(UInt8.self, forKey: .`priority`)
    self.`rtt_capable` = try container.decode(Bool.self, forKey: .`rtt_capable`)
    self.`status` = try container.decode(ContractPeerStatus.self, forKey: .`status`)
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
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`address` = try container.decode(String.self, forKey: .`address`)
    self.`connected` = try container.decode(Bool.self, forKey: .`connected`)
    self.`interface` = try container.decode(ContractConnectionInterface.self, forKey: .`interface`)
    guard container.contains(.`latency_ms`) else { throw DecodingError.keyNotFound(CodingKeys.`latency_ms`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`latency_ms` = try container.decodeIfPresent(UInt64.self, forKey: .`latency_ms`)
    self.`online` = try container.decode(Bool.self, forKey: .`online`)
    self.`pairing_endpoint` = try container.decode(Bool.self, forKey: .`pairing_endpoint`)
    self.`rtt_capable` = try container.decode(Bool.self, forKey: .`rtt_capable`)
    self.`status` = try container.decode(ContractPeerStatus.self, forKey: .`status`)
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
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`address` = try container.decode(String.self, forKey: .`address`)
    self.`candidates` = try container.decode([ContractPeerCandidate].self, forKey: .`candidates`)
    self.`connection_mode` = try container.decode(String.self, forKey: .`connection_mode`)
    self.`current_address` = try container.decodeIfPresent(String.self, forKey: .`current_address`)
    self.`current_interface` = try container.decodeIfPresent(ContractConnectionInterface.self, forKey: .`current_interface`)
    self.`enabled` = try container.decode(Bool.self, forKey: .`enabled`)
    self.`fingerprint` = try container.decode(String.self, forKey: .`fingerprint`)
    self.`hostname` = try container.decode(String.self, forKey: .`hostname`)
    self.`online` = try container.decode(Bool.self, forKey: .`online`)
    guard container.contains(.`protocol_error`) else { throw DecodingError.keyNotFound(CodingKeys.`protocol_error`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`protocol_error` = try container.decodeIfPresent(String.self, forKey: .`protocol_error`)
    guard container.contains(.`required_protocol_version`) else { throw DecodingError.keyNotFound(CodingKeys.`required_protocol_version`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`required_protocol_version` = try container.decodeIfPresent(UInt8.self, forKey: .`required_protocol_version`)
    self.`routes` = try container.decode([ContractPeerRouteSnapshot].self, forKey: .`routes`)
    self.`status` = try container.decode(ContractPeerStatus.self, forKey: .`status`)
    self.`tailscale_ip` = try container.decode(String.self, forKey: .`tailscale_ip`)
    self.`trusted` = try container.decode(Bool.self, forKey: .`trusted`)
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
    let container = try decoder.container(keyedBy: CodingKeys.self)
    guard container.contains(.`discovery_error`) else { throw DecodingError.keyNotFound(CodingKeys.`discovery_error`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`discovery_error` = try container.decodeIfPresent(String.self, forKey: .`discovery_error`)
    self.`paired_peer_endpoints` = try container.decode([String: String].self, forKey: .`paired_peer_endpoints`)
    self.`peers` = try container.decode([ContractPeerSnapshot].self, forKey: .`peers`)
    self.`self` = try container.decode(ContractLocalDeviceSnapshot.self, forKey: .`self`)
  }
}

struct ContractPendingPairingSummary: Codable, Sendable {
  let `address`: String
  let `fingerprint`: String
  let `hostname`: String
  let `interface`: String
  let `locally_trusted`: Bool
  private enum CodingKeys: String, CodingKey { case `address`, `fingerprint`, `hostname`, `interface`, `locally_trusted` }
  init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`address` = try container.decode(String.self, forKey: .`address`)
    self.`fingerprint` = try container.decode(String.self, forKey: .`fingerprint`)
    self.`hostname` = try container.decode(String.self, forKey: .`hostname`)
    self.`interface` = try container.decode(String.self, forKey: .`interface`)
    self.`locally_trusted` = try container.decode(Bool.self, forKey: .`locally_trusted`)
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
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`batch_id` = try container.decode(String.self, forKey: .`batch_id`)
    self.`first_entry_id` = try container.decode(Int64.self, forKey: .`first_entry_id`)
    self.`item_count` = try container.decode(UInt64.self, forKey: .`item_count`)
    self.`item_index` = try container.decode(UInt64.self, forKey: .`item_index`)
    self.`last_entry_id` = try container.decode(Int64.self, forKey: .`last_entry_id`)
    guard container.contains(.`next_entry_id`) else { throw DecodingError.keyNotFound(CodingKeys.`next_entry_id`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`next_entry_id` = try container.decodeIfPresent(Int64.self, forKey: .`next_entry_id`)
    guard container.contains(.`previous_entry_id`) else { throw DecodingError.keyNotFound(CodingKeys.`previous_entry_id`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`previous_entry_id` = try container.decodeIfPresent(Int64.self, forKey: .`previous_entry_id`)
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
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`code` = try container.decode(ContractPreviewErrorCode.self, forKey: .`code`)
    guard container.contains(.`entry_id`) else { throw DecodingError.keyNotFound(CodingKeys.`entry_id`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`entry_id` = try container.decodeIfPresent(Int64.self, forKey: .`entry_id`)
    guard container.contains(.`limit_bytes`) else { throw DecodingError.keyNotFound(CodingKeys.`limit_bytes`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`limit_bytes` = try container.decodeIfPresent(UInt64.self, forKey: .`limit_bytes`)
    self.`message` = try container.decode(String.self, forKey: .`message`)
    self.`retryable` = try container.decode(Bool.self, forKey: .`retryable`)
    guard container.contains(.`size_bytes`) else { throw DecodingError.keyNotFound(CodingKeys.`size_bytes`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`size_bytes` = try container.decodeIfPresent(UInt64.self, forKey: .`size_bytes`)
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
    let container = try decoder.container(keyedBy: CodingKeys.self)
    guard container.contains(.`batch`) else { throw DecodingError.keyNotFound(CodingKeys.`batch`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`batch` = try container.decodeIfPresent(ContractPreviewBatchNavigation.self, forKey: .`batch`)
    self.`entry_id` = try container.decode(Int64.self, forKey: .`entry_id`)
    guard self.`entry_id` >= 1 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
    guard container.contains(.`height`) else { throw DecodingError.keyNotFound(CodingKeys.`height`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`height` = try container.decodeIfPresent(UInt32.self, forKey: .`height`)
    self.`kind` = try container.decode(String.self, forKey: .`kind`)
    guard ["text","image","file"].contains(self.`kind`) else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
    self.`name` = try container.decode(String.self, forKey: .`name`)
    guard self.`name`.count >= 1 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
    self.`request_id` = try container.decodeIfPresent(String.self, forKey: .`request_id`)
    self.`size_bytes` = try container.decode(UInt64.self, forKey: .`size_bytes`)
    guard self.`size_bytes` <= 67108864 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
    guard container.contains(.`width`) else { throw DecodingError.keyNotFound(CodingKeys.`width`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`width` = try container.decodeIfPresent(UInt32.self, forKey: .`width`)
  }
}

struct ContractRuntimeNotification: Codable, Sendable {
  let `arrival`: ContractClipboardArrival?
  let `code`: String?
  let `id`: UInt64
  let `level`: String
  let `message`: String
  private enum CodingKeys: String, CodingKey { case `arrival`, `code`, `id`, `level`, `message` }
  init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`arrival` = try container.decodeIfPresent(ContractClipboardArrival.self, forKey: .`arrival`)
    self.`code` = try container.decodeIfPresent(String.self, forKey: .`code`)
    self.`id` = try container.decode(UInt64.self, forKey: .`id`)
    self.`level` = try container.decode(String.self, forKey: .`level`)
    self.`message` = try container.decode(String.self, forKey: .`message`)
  }
}

struct ContractSourceContextGrantReport: Codable, Sendable {
  let `budget_bytes`: UInt64
  let `expires_at`: UInt64
  let `grant_id`: String
  let `node_count`: UInt64
  let `origin`: String
  let `ticket`: String
  private enum CodingKeys: String, CodingKey { case `budget_bytes`, `expires_at`, `grant_id`, `node_count`, `origin`, `ticket` }
  init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`budget_bytes` = try container.decode(UInt64.self, forKey: .`budget_bytes`)
    self.`expires_at` = try container.decode(UInt64.self, forKey: .`expires_at`)
    self.`grant_id` = try container.decode(String.self, forKey: .`grant_id`)
    self.`node_count` = try container.decode(UInt64.self, forKey: .`node_count`)
    guard self.`node_count` <= 65535 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
    self.`origin` = try container.decode(String.self, forKey: .`origin`)
    self.`ticket` = try container.decode(String.self, forKey: .`ticket`)
  }
}

struct ContractSourceContextImportReport: Codable, Sendable {
  let `complete`: Bool
  let `deduplicated`: UInt64
  let `failures`: [ContractArchiveResourceFailure]
  let `imported`: UInt64
  private enum CodingKeys: String, CodingKey { case `complete`, `deduplicated`, `failures`, `imported` }
  init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`complete` = try container.decode(Bool.self, forKey: .`complete`)
    self.`deduplicated` = try container.decode(UInt64.self, forKey: .`deduplicated`)
    guard self.`deduplicated` <= 65535 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
    self.`failures` = try container.decode([ContractArchiveResourceFailure].self, forKey: .`failures`)
    self.`imported` = try container.decode(UInt64.self, forKey: .`imported`)
    guard self.`imported` <= 65535 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }
  }
}

struct ContractSourceContextRevokeReport: Codable, Sendable {
  let `revoked`: Bool
  private enum CodingKeys: String, CodingKey { case `revoked` }
  init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`revoked` = try container.decode(Bool.self, forKey: .`revoked`)
  }
}

enum ContractStableErrorCode: String, Codable, Sendable {
  case `invalid_argument` = "invalid_argument"
  case `not_found` = "not_found"
  case `temporarily_busy` = "temporarily_busy"
  case `storage_unavailable` = "storage_unavailable"
  case `unauthorized` = "unauthorized"
  case `protocol_incompatible` = "protocol_incompatible"
  case `publish_partial` = "publish_partial"
  case `publish_unknown` = "publish_unknown"
  case `publish_superseded` = "publish_superseded"
  case `native_only` = "native_only"
  case `originals_incomplete` = "originals_incomplete"
  case `resources_incomplete` = "resources_incomplete"
  case `no_single_image` = "no_single_image"
  case `format_unavailable` = "format_unavailable"
  case `unsupported_format` = "unsupported_format"
  case `pairing_identity_mismatch` = "pairing_identity_mismatch"
  case `pairing_window_closed` = "pairing_window_closed"
  case `pairing_timed_out` = "pairing_timed_out"
  case `pairing_connection_closed` = "pairing_connection_closed"
  case `pairing_busy` = "pairing_busy"
  case `pairing_cancelled` = "pairing_cancelled"
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
    let container = try decoder.container(keyedBy: CodingKeys.self)
    let version = try container.decode(UInt32.self, forKey: .schema_version)
    guard version == 1 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Unsupported stable error schema version")) }
    self.schema_version = version
    let rawCode = try container.decode(String.self, forKey: .code)
    _ = try container.decode(Bool.self, forKey: .retryable)
    _ = try container.decode(String.self, forKey: .message_key)
    _ = try container.decode(String.self, forKey: .detail_class)
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
    case .publish_partial:
      self.retryable = true
      self.message_key = "history.restore.code.publish_partial"
      self.detail_class = .resource
    case .publish_unknown:
      self.retryable = true
      self.message_key = "history.restore.code.publish_unknown"
      self.detail_class = .resource
    case .publish_superseded:
      self.retryable = false
      self.message_key = "history.restore.code.publish_superseded"
      self.detail_class = .resource
    case .native_only:
      self.retryable = false
      self.message_key = "history.restore.code.native_only"
      self.detail_class = .resource
    case .originals_incomplete:
      self.retryable = false
      self.message_key = "history.restore.code.originals_incomplete"
      self.detail_class = .resource
    case .resources_incomplete:
      self.retryable = false
      self.message_key = "history.restore.code.resources_incomplete"
      self.detail_class = .resource
    case .no_single_image:
      self.retryable = false
      self.message_key = "history.restore.code.no_single_image"
      self.detail_class = .resource
    case .format_unavailable:
      self.retryable = false
      self.message_key = "history.restore.code.format_unavailable"
      self.detail_class = .resource
    case .unsupported_format:
      self.retryable = false
      self.message_key = "history.restore.code.unsupported_format"
      self.detail_class = .resource
    case .pairing_identity_mismatch:
      self.retryable = false
      self.message_key = "error.pairing_identity_mismatch"
      self.detail_class = .authorization
    case .pairing_window_closed:
      self.retryable = false
      self.message_key = "error.pairing_window_closed"
      self.detail_class = .resource
    case .pairing_timed_out:
      self.retryable = false
      self.message_key = "error.pairing_timed_out"
      self.detail_class = .contention
    case .pairing_connection_closed:
      self.retryable = false
      self.message_key = "error.pairing_connection_closed"
      self.detail_class = .protocol
    case .pairing_busy:
      self.retryable = false
      self.message_key = "error.pairing_busy"
      self.detail_class = .contention
    case .pairing_cancelled:
      self.retryable = false
      self.message_key = "error.pairing_cancelled"
      self.detail_class = .request
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
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`available` = try container.decode(Bool.self, forKey: .`available`)
    guard container.contains(.`error`) else { throw DecodingError.keyNotFound(CodingKeys.`error`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`error` = try container.decodeIfPresent(String.self, forKey: .`error`)
    self.`quota_bytes` = try container.decode(UInt64.self, forKey: .`quota_bytes`)
    self.`root` = try container.decode(String.self, forKey: .`root`)
    self.`used_bytes` = try container.decode(UInt64.self, forKey: .`used_bytes`)
  }
}

struct ContractSyncWarning: Codable, Sendable {
  let `id`: UInt64
  let `kind`: String
  let `occurred_at_ms`: Int64
  let `peer`: String
  private enum CodingKeys: String, CodingKey { case `id`, `kind`, `occurred_at_ms`, `peer` }
  init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`id` = try container.decode(UInt64.self, forKey: .`id`)
    self.`kind` = try container.decode(String.self, forKey: .`kind`)
    self.`occurred_at_ms` = try container.decode(Int64.self, forKey: .`occurred_at_ms`)
    self.`peer` = try container.decode(String.self, forKey: .`peer`)
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
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`history_version` = try container.decode(UInt64.self, forKey: .`history_version`)
    self.`notifications` = try container.decode([ContractRuntimeNotification].self, forKey: .`notifications`)
    guard container.contains(.`progress`) else { throw DecodingError.keyNotFound(CodingKeys.`progress`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`progress` = try container.decodeIfPresent(ContractFileProgress.self, forKey: .`progress`)
    self.`revision` = try container.decode(UInt64.self, forKey: .`revision`)
    guard container.contains(.`sync_warning`) else { throw DecodingError.keyNotFound(CodingKeys.`sync_warning`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }
    self.`sync_warning` = try container.decodeIfPresent(ContractSyncWarning.self, forKey: .`sync_warning`)
  }
}

struct ContractWithheldRestoreFormat: Codable, Sendable {
  let `code`: String
  let `format`: String
  private enum CodingKeys: String, CodingKey { case `code`, `format` }
  init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.`code` = try container.decode(String.self, forKey: .`code`)
    self.`format` = try container.decode(String.self, forKey: .`format`)
  }
}
