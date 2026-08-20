import AppKit
import Foundation

enum HistoryPreviewFormat: String, CaseIterable, Equatable, Sendable {
    case text
    case code
    case markdown
    case image
    case pdf
    case docx
    case presentation
    case unsupported

    var windowKind: HistoryPreviewWindowKind {
        switch self {
        case .text, .code: return .text
        case .markdown, .docx, .presentation: return .document
        case .image: return .image
        case .pdf: return .pdf
        case .unsupported: return .text
        }
    }

    static func detect(
        payload: HistoryPreviewData,
        categoryHint: String? = nil
    ) -> HistoryPreviewFormat {
        let fileExtension = HistoryPreviewFileTypes.fileExtension(for: payload.name)
        if payload.kind == "image" || HistoryPreviewFileTypes.imageExtensions.contains(fileExtension) {
            return .image
        }
        switch fileExtension {
        case "md", "markdown": return .markdown
        case "pdf": return .pdf
        case "docx": return .docx
        case "ppt", "pptx": return .presentation
        default: break
        }
        guard payload.kind == "text"
                || HistoryPreviewFileTypes.textExtensions.contains(fileExtension) else {
            return .unsupported
        }
        if categoryHint == "code"
            || categoryHint == "command"
            || codeFileExtensions.contains(fileExtension)
            || looksLikeCode(payload.data)
        {
            return .code
        }
        return .text
    }

    private static let codeFileExtensions: Set<String> = [
        "svg", "json", "jsonl", "xml", "yaml", "yml", "toml", "ini", "cfg",
        "conf", "html", "htm", "css", "js", "jsx", "ts", "tsx", "swift", "rs",
        "go", "py", "rb", "java", "kt", "kts", "c", "h", "cc", "cpp", "cxx",
        "hpp", "cs", "php", "sh", "bash", "zsh", "fish", "ps1", "sql"
    ]

    /// Deliberately conservative: ordinary prose should never be switched to
    /// code mode because it happens to contain one brace or command-like word.
    private static func looksLikeCode(_ data: Data) -> Bool {
        guard data.count <= 2 * 1024 * 1024,
              let text = String(data: data, encoding: .utf8) else { return false }
        let sample = String(text.prefix(12_000))
        let markers = [
            "func ", "struct ", "class ", "enum ", "import ", "#include",
            "const ", "let ", "var ", "def ", "fn ", "SELECT ", "FROM ",
            "#!/", "=>", "{\n", "</"
        ]
        return markers.reduce(into: 0) { score, marker in
            if sample.contains(marker) { score += 1 }
        } >= 2
    }
}

enum HistoryPreviewWindowKind: String, CaseIterable, Equatable, Sendable {
    case text
    case document
    case image
    case pdf

    var defaultContentSize: NSSize {
        switch self {
        case .text: return NSSize(width: 960, height: 720)
        case .document: return NSSize(width: 1_100, height: 800)
        case .image: return NSSize(width: 1_000, height: 760)
        case .pdf: return NSSize(width: 1_100, height: 820)
        }
    }

    var minimumContentSize: NSSize {
        switch self {
        case .text: return NSSize(width: 640, height: 420)
        case .document: return NSSize(width: 720, height: 520)
        case .image: return NSSize(width: 640, height: 480)
        case .pdf: return NSSize(width: 760, height: 560)
        }
    }
}

struct HistoryPreviewItem: Identifiable, Equatable, Sendable {
    let id: Int64
    let batchId: String?
    let batchIndex: Int?
    let batchCount: Int?
    let category: String
    let type: String
    let nameHint: String
    let sizeBytes: Int64
    let resolvesBatchFirst: Bool

    init(entry: HistoryEntry, resolvesBatchFirst: Bool = false) {
        id = entry.id
        batchId = entry.batch_id
        batchIndex = entry.batch_index
        batchCount = entry.batch_count ?? entry.batch_total
        category = entry.category
        type = entry.type
        nameHint = entry.description
        sizeBytes = entry.size_bytes
        self.resolvesBatchFirst = resolvesBatchFirst
    }

    init(
        id: Int64,
        batchId: String?,
        batchIndex: Int?,
        batchCount: Int?,
        category: String,
        type: String,
        nameHint: String,
        sizeBytes: Int64,
        resolvesBatchFirst: Bool = false
    ) {
        self.id = id
        self.batchId = batchId
        self.batchIndex = batchIndex
        self.batchCount = batchCount
        self.category = category
        self.type = type
        self.nameHint = nameHint
        self.sizeBytes = sizeBytes
        self.resolvesBatchFirst = resolvesBatchFirst
    }

    var estimatedFormat: HistoryPreviewFormat {
        let placeholder = HistoryPreviewData(
            kind: type,
            name: nameHint,
            sizeBytes: max(0, sizeBytes),
            data: Data()
        )
        return HistoryPreviewFormat.detect(payload: placeholder, categoryHint: category)
    }
}

struct HistoryPreviewRequest: Equatable, Sendable {
    let items: [HistoryPreviewItem]
    let selectedIndex: Int

    var selectedItem: HistoryPreviewItem? {
        guard items.indices.contains(selectedIndex) else { return nil }
        return items[selectedIndex]
    }
}

/// Converts current history rows into one preview request without changing the
/// row-selection policy. Batch items are sorted by their persisted index so
/// toolbar navigation remains stable even if the list is refreshed.
func historyPreviewRequest(
    focusedId: Int64,
    entries: [HistoryEntry],
    expandedBatchIds: Set<String>
) -> HistoryPreviewRequest? {
    guard let focused = entries.first(where: { $0.id == focusedId }) else { return nil }
    guard let batchId = focused.batch_id else {
        return HistoryPreviewRequest(items: [HistoryPreviewItem(entry: focused)], selectedIndex: 0)
    }

    let sorted = entries
        .filter { $0.batch_id == batchId }
        .sorted {
            let left = $0.batch_index ?? Int.max
            let right = $1.batch_index ?? Int.max
            return left == right ? $0.id < $1.id : left < right
        }
    guard !sorted.isEmpty else { return nil }

    let selectedIndex: Int
    let resolvesBatchFirst: Bool
    if expandedBatchIds.contains(batchId) {
        selectedIndex = sorted.firstIndex(where: { $0.id == focusedId }) ?? 0
        resolvesBatchFirst = false
    } else {
        selectedIndex = 0
        resolvesBatchFirst = true
    }
    let items = sorted.enumerated().map { index, entry in
        HistoryPreviewItem(
            entry: entry,
            resolvesBatchFirst: resolvesBatchFirst && index == selectedIndex
        )
    }
    return HistoryPreviewRequest(items: items, selectedIndex: selectedIndex)
}

func historyPreviewTargetId(
    focusedId: Int64,
    entries: [HistoryEntry],
    expandedBatchIds: Set<String>
) -> Int64? {
    historyPreviewRequest(
        focusedId: focusedId,
        entries: entries,
        expandedBatchIds: expandedBatchIds
    )?.selectedItem?.id
}

enum HistoryPreviewFailureKind: String, Equatable, Sendable {
    case tooLarge
    case unsupported
    case corrupt
    case decryption
    case unavailable
}

enum HistoryPreviewRemoteErrorCode: String, Equatable, Sendable {
    case entryNotFound = "entry_not_found"
    case batchNotFound = "batch_not_found"
    case entryNotInBatch = "entry_not_in_batch"
    case metadataUnavailable = "metadata_unavailable"
    case payloadUnavailable = "payload_unavailable"
    case previewTooLarge = "preview_too_large"
    case unsupportedType = "unsupported_type"
    case invalidSize = "invalid_size"
}

struct HistoryPreviewRemoteError: LocalizedError, Equatable, Sendable {
    let code: HistoryPreviewRemoteErrorCode?
    let message: String
    let retryable: Bool?

    init(
        code: HistoryPreviewRemoteErrorCode?,
        message: String,
        retryable: Bool? = nil
    ) {
        self.code = code
        self.message = message
        self.retryable = retryable
    }

    var errorDescription: String? { message }
}

struct HistoryPreviewFailure: Equatable, Sendable {
    let kind: HistoryPreviewFailureKind
    let canRetry: Bool

    static func classify(_ error: Error) -> HistoryPreviewFailure {
        if let remoteError = error as? HistoryPreviewRemoteError,
           let code = remoteError.code
        {
            switch code {
            case .previewTooLarge:
                return HistoryPreviewFailure(kind: .tooLarge, canRetry: false)
            case .unsupportedType:
                return HistoryPreviewFailure(kind: .unsupported, canRetry: false)
            case .invalidSize:
                return HistoryPreviewFailure(kind: .corrupt, canRetry: false)
            case .payloadUnavailable:
                return HistoryPreviewFailure(
                    kind: .decryption,
                    canRetry: remoteError.retryable ?? true
                )
            case .entryNotFound, .batchNotFound, .entryNotInBatch:
                return HistoryPreviewFailure(kind: .unavailable, canRetry: false)
            case .metadataUnavailable:
                return HistoryPreviewFailure(
                    kind: .unavailable,
                    canRetry: remoteError.retryable ?? true
                )
            }
        }
        if let storeError = error as? HistoryPreviewStoreError {
            switch storeError {
            case .tooLarge:
                return HistoryPreviewFailure(kind: .tooLarge, canRetry: false)
            case .invalidText, .invalidImage, .invalidDocument:
                // These errors are deterministic validation failures for bytes
                // already loaded successfully; retrying the same payload cannot
                // repair it.
                return HistoryPreviewFailure(kind: .corrupt, canRetry: false)
            case .invalidPath, .writeFailed:
                return HistoryPreviewFailure(kind: .unavailable, canRetry: true)
            }
        }
        // Compatibility fallback for older daemons that predate stable
        // preview error codes. New daemons are classified exclusively above.
        let message = error.localizedDescription.lowercased()
        if message.contains("too large") || message.contains("size limit") {
            return HistoryPreviewFailure(kind: .tooLarge, canRetry: false)
        }
        if message.contains("unsupported") {
            return HistoryPreviewFailure(kind: .unsupported, canRetry: false)
        }
        if message.contains("decrypt") || message.contains("cipher") || message.contains("authentication tag") {
            return HistoryPreviewFailure(kind: .decryption, canRetry: true)
        }
        if message.contains("corrupt") || message.contains("invalid preview data") {
            return HistoryPreviewFailure(kind: .corrupt, canRetry: true)
        }
        return HistoryPreviewFailure(kind: .unavailable, canRetry: true)
    }
}
