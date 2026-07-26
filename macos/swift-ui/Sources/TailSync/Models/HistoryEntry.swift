import Foundation

struct HistoryEntry: Codable, Identifiable {
    let id: Int64
    let timestamp: String
    let type: String   // "text" | "image" | "file"
    let description: String
    let data_hash: String
    let size_bytes: Int64
    let source_peer: String

    enum CodingKeys: String, CodingKey {
        case id, timestamp, type, description, data_hash, size_bytes, source_peer
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
        switch type {
        case "text": return "doc.text"
        case "image": return "photo"
        case "file": return "doc"
        default: return "doc.on.clipboard"
        }
    }
}
