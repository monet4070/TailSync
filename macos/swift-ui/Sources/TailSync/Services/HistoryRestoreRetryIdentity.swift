import Foundation

/// A deliberate retry follows content identity when history deduplication
/// replaces a row. It never recreates a deleted history entry.
struct HistoryRestoreRetryIdentity {
  let originalID: Int64
  let type: String
  let dataHash: String
  let identity: ContractHistoryIdentity?

  init(_ entry: HistoryEntry) {
    originalID = entry.id
    type = entry.type
    dataHash = entry.data_hash
    identity = entry.composite?.identity
  }

  func resolve(in entries: [HistoryEntry]) -> Int64? {
    if let original = entries.first(where: { $0.id == originalID }) {
      guard original.type == type else { return nil }
      if let identity {
        guard let current = original.composite?.identity,
              identity.version == 1, current.version == 1,
              current.capture_id == identity.capture_id, current.content_id == identity.content_id else { return nil }
        return original.id
      }
      return original.data_hash == dataHash && original.composite?.identity == nil ? original.id : nil
    }
    guard let identity, identity.version == 1, let key = identity.dedupe_key, !key.isEmpty else { return nil }
    return entries.first(where: { entry in
      guard entry.type == type, let current = entry.composite?.identity else { return false }
      return current.version == 1 && current.content_id == identity.content_id && current.dedupe_key == key
    })?.id
  }
}
