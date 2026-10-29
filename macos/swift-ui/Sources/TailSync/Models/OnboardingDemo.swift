import Foundation

enum OnboardingDemo {
    struct Record {
        let id: Int64
        let name: String
        let category: String
        let kind: String
        let description: String
        let data: Data
        let batchId: String?
        let batchIndex: Int?
        let batchTotal: Int?
        let document: [String: Any]?
    }

    static let records: [Record] = {
        guard let root = (try? JSONSerialization.jsonObject(with: Data(OnboardingDemoContent.json.utf8))) as? [String: Any],
              let values = root["entries"] as? [[String: Any]] else { return [] }
        return values.compactMap { value in
            guard let id = value["id"] as? NSNumber,
                  let name = value["name"] as? String,
                  let category = value["category"] as? String,
                  let kind = value["kind"] as? String,
                  let description = value["description"] as? String,
                  let encoded = value["data_b64"] as? String,
                  let data = Data(base64Encoded: encoded) else { return nil }
            return Record(
                id: id.int64Value,
                name: name,
                category: category,
                kind: kind,
                description: description,
                data: data,
                batchId: value["batch_id"] as? String,
                batchIndex: (value["batch_index"] as? NSNumber)?.intValue,
                batchTotal: (value["batch_total"] as? NSNumber)?.intValue,
                document: value["document"] as? [String: Any]
            )
        }
    }()

    static func isDemoEntry(_ id: Int64) -> Bool {
        records.contains { $0.id == id }
    }

    static var genericDocument: [String: Any] {
        [
            "version": 1,
            "blocks": [
                ["kind": "paragraph", "paragraph": ["runs": [["text": "TAILSYNC DEMO DOCUMENT", "style": style, "unsupported": false]]]],
                ["kind": "paragraph", "paragraph": ["runs": [["text": "This sample uses the real document preview. Copy on one device, then paste on another.", "style": style, "unsupported": false]]]],
                ["kind": "paragraph", "paragraph": ["runs": [["text": "Double-click restores. Right-click deletes. Long-press favorites.", "style": style, "unsupported": false]]]]
            ],
            "assets": [],
            "partial": [],
            "styles_partial": false
        ]
    }

    private static var style: [String: Any] {
        ["bold": false, "italic": false, "underline": false, "strikethrough": false, "subscript": false, "superscript": false]
    }

    static func entries(query: HistoryQuery) -> [HistoryEntry] {
        let now = Date()
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        let values = records.compactMap { record -> [String: Any]? in
            if let category = query.category, category != "all", record.category != category { return nil }
            if let keyword = query.keyword, !keyword.isEmpty,
               !record.description.localizedCaseInsensitiveContains(keyword),
               !record.name.localizedCaseInsensitiveContains(keyword) { return nil }
            if query.collection == "favorites" && record.category != "code" { return nil }
            let kind = displayKind(record)
            var entry: [String: Any] = [
                "id": record.id,
                "timestamp": formatter.string(from: now.addingTimeInterval(-Double(records.firstIndex(where: { $0.id == record.id }) ?? 0) * 37)),
                "type": record.batchId == nil ? record.kind : "file",
                "description": record.description,
                "data_hash": "demo-\(record.id)",
                "size_bytes": record.data.count,
                "source_peer": record.id == 8000000001 ? "DEMO-LAPTOP" : "self",
                "category": record.category,
                "categories": [record.category],
                "category_confidence": 100,
                "classifier_version": 1,
                "pinned": record.id == 8000000003,
                "batch_status": "complete",
                "display": ["cache_key": "demo-\(record.id)", "kind": kind, "thumbnail_eligible": kind == "image", "version": 1]
            ]
            if let batchId = record.batchId { entry["batch_id"] = batchId }
            if let batchIndex = record.batchIndex { entry["batch_index"] = batchIndex }
            if let batchTotal = record.batchTotal {
                entry["batch_total"] = batchTotal
                entry["batch_count"] = batchTotal
            }
            return entry
        }
        let entries = values.compactMap { value -> HistoryEntry? in
            guard let data = try? JSONSerialization.data(withJSONObject: value) else { return nil }
            return try? JSONDecoder().decode(HistoryEntry.self, from: data)
        }
        return entries
    }

    static func displayKind(_ record: Record) -> String {
        if record.document != nil { return "document" }
        if ["docx", "pptx"].contains(fileExtension(record.name)) { return "document" }
        if record.category == "image" || record.category == "file" { return record.name.lowercased().hasSuffix(".svg") ? "image" : (["docx", "pptx"].contains(fileExtension(record.name)) || record.document != nil ? "document" : (record.category == "image" ? "image" : "file")) }
        return "text"
    }

    private static func fileExtension(_ name: String) -> String {
        (name as NSString).pathExtension.lowercased()
    }

    static func previewData(id: Int64, batchId: String?) -> HistoryPreviewData? {
        guard let record = records.first(where: { $0.id == id }) else { return nil }
        let siblings = records.filter { $0.batchId != nil && $0.batchId == (batchId ?? record.batchId) }.sorted { ($0.batchIndex ?? 0) < ($1.batchIndex ?? 0) }
        let batch: HistoryPreviewBatchNavigation? = siblings.count > 1 ? {
            let index = siblings.firstIndex(where: { $0.id == id }) ?? 0
            return HistoryPreviewBatchNavigation(
                batchId: record.batchId ?? "", itemIndex: index, itemCount: siblings.count,
                firstEntryId: siblings.first!.id, lastEntryId: siblings.last!.id,
                previousEntryId: index > 0 ? siblings[index - 1].id : nil,
                nextEntryId: index + 1 < siblings.count ? siblings[index + 1].id : nil
            )
        }() : nil
        return HistoryPreviewData(kind: record.kind, name: record.name, sizeBytes: Int64(record.data.count), data: record.data, entryId: record.id, batch: batch)
    }

    static func documentPreview(id: Int64, owner: String, generation: UInt64, requestId: String) -> ApiClient.HistoryDocumentPreview? {
        guard let record = records.first(where: { $0.id == id }), displayKind(record) == "document" else { return nil }
        let object = record.document ?? genericDocument
        guard let data = try? JSONSerialization.data(withJSONObject: object),
              let document = try? JSONDecoder().decode(ContractDisplayDocument.self, from: data) else { return nil }
        return ApiClient.HistoryDocumentPreview(entryId: id, stamp: "demo-\(id)", owner: owner, generation: generation, requestId: requestId, document: document, assets: Data())
    }

    static func peerSnapshot() -> ApiClient.PeerSnapshot? {
        let value: [String: Any] = [
            "hostname": "DEMO-LAPTOP · 演示设备",
            "tailscale_ip": "", "address": "192.0.2.42:19890", "online": true, "enabled": true,
            "connection_mode": "auto", "trusted": false, "fingerprint": "演示设备 · 不会连接真实网络",
            "current_interface": NSNull(), "current_address": NSNull(),
            "candidates": [], "routes": [["interface": "lan", "address": "192.0.2.42:19890", "status": "online", "online": true, "connected": false, "latency_ms": 12, "pairing_endpoint": true, "rtt_capable": true]],
            "status": "online"
        ]
        guard let data = try? JSONSerialization.data(withJSONObject: value) else { return nil }
        return try? JSONDecoder().decode(ApiClient.PeerSnapshot.self, from: data)
    }

    static let verificationCode = "246 810"
}
