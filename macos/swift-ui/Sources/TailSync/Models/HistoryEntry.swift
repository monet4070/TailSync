import Foundation

struct HistoryEntry: Codable, Identifiable {
    let id: Int64
    let timestamp: String
    let type: String   // "text" | "image" | "file"
    let description: String
    let data_hash: String
    let size_bytes: Int64
    let source_peer: String
    let category: String
    let categories: [String]
    let category_confidence: Int64
    let classifier_version: Int64
    let pinned: Bool
    let batch_id: String?
    let batch_index: Int?
    let batch_total: Int?
    let batch_count: Int?
    let batch_status: String

    enum CodingKeys: String, CodingKey {
        case id, timestamp, type, description, data_hash, size_bytes, source_peer
        case category, categories, category_confidence, classifier_version
        case pinned, batch_id, batch_index, batch_total, batch_count, batch_status
    }

    init(from decoder: Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        id = try values.decode(Int64.self, forKey: .id)
        timestamp = try values.decode(String.self, forKey: .timestamp)
        type = try values.decode(String.self, forKey: .type)
        description = try values.decode(String.self, forKey: .description)
        data_hash = try values.decode(String.self, forKey: .data_hash)
        size_bytes = try values.decode(Int64.self, forKey: .size_bytes)
        source_peer = try values.decode(String.self, forKey: .source_peer)
        let decodedCategory = try values.decodeIfPresent(String.self, forKey: .category) ?? type
        switch decodedCategory {
        case "text", "website", "code", "command", "structured_data", "path", "image", "file":
            category = decodedCategory
        default:
            category = type
        }
        let decodedCategories = try values.decodeIfPresent([String].self, forKey: .categories) ?? []
        var resolvedCategories = [category]
        for label in decodedCategories where Self.isKnownCategory(label) && !resolvedCategories.contains(label) {
            resolvedCategories.append(label)
        }
        categories = resolvedCategories
        category_confidence = try values.decodeIfPresent(Int64.self, forKey: .category_confidence) ?? 0
        classifier_version = try values.decodeIfPresent(Int64.self, forKey: .classifier_version) ?? 0
        pinned = try values.decodeIfPresent(Bool.self, forKey: .pinned) ?? false
        batch_id = try values.decodeIfPresent(String.self, forKey: .batch_id)
        batch_index = try values.decodeIfPresent(Int.self, forKey: .batch_index)
        batch_total = try values.decodeIfPresent(Int.self, forKey: .batch_total)
        batch_count = try values.decodeIfPresent(Int.self, forKey: .batch_count)
        batch_status = try values.decodeIfPresent(String.self, forKey: .batch_status) ?? "complete"
    }

    private static func isKnownCategory(_ category: String) -> Bool {
        switch category {
        case "text", "website", "code", "command", "structured_data", "path", "image", "file":
            return true
        default:
            return false
        }
    }

    var formattedTime: String {
        let fmt = ISO8601DateFormatter()
        fmt.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        if let date = fmt.date(from: timestamp) ?? ISO8601DateFormatter().date(from: timestamp) {
            let f = DateFormatter()
            f.dateFormat = "HH:mm:ss"
            return f.string(from: date)
        }
        return String(timestamp.prefix(19))
    }

    var formattedSize: String {
        if size_bytes > 1024 {
            return String(format: "%.1f KB", Double(size_bytes) / 1024)
        }
        return "\(size_bytes) B"
    }

    var icon: String {
        switch category {
        case "website": return "globe"
        case "code": return "chevron.left.forwardslash.chevron.right"
        case "command": return "terminal"
        case "structured_data": return "curlybraces"
        case "path": return "folder"
        case "image": return "photo"
        case "file": return "doc"
        case "text": return "doc.text"
        default: return "doc.on.clipboard"
        }
    }

    var categoryLabel: String {
        Loc.t("history.category.\(category)")
    }

    var categoryLabels: [String] {
        categories.map { Loc.t("history.category.\($0)") }
    }
}
