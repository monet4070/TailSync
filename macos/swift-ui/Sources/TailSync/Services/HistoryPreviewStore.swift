import Foundation

/// Owns the private directory used only for Quick Look source files.
///
/// Native text, image and PDF previews never use this store. The directory is
/// mode 0700 and each file is mode 0600; every deletion validates that the
/// target remains inside this root.
final class HistoryPreviewStore: @unchecked Sendable {
    static let directoryName = "tailsync-preview"
    static let maximumSanitizedNameBytes = 255 - 36 - 1

    let directory: URL
    private let fileManager: FileManager

    init(
        directory: URL = FileManager.default.temporaryDirectory
            .appendingPathComponent(HistoryPreviewStore.directoryName, isDirectory: true),
        fileManager: FileManager = .default
    ) {
        self.directory = directory.standardizedFileURL
        self.fileManager = fileManager
    }

    @discardableResult
    func cleanupStaleFiles() throws -> Int {
        guard !isSymbolicLink(directory) else {
            throw HistoryPreviewStoreError.invalidPath
        }
        guard fileManager.fileExists(atPath: directory.path) else { return 0 }
        try setDirectoryPermissions()
        let contents = try fileManager.contentsOfDirectory(
            at: directory,
            includingPropertiesForKeys: [.isDirectoryKey, .isSymbolicLinkKey],
            options: []
        )
        var removed = 0
        for item in contents {
            guard isInsideDirectory(item) else { continue }
            try fileManager.removeItem(at: item)
            removed += 1
        }
        return removed
    }

    func write(_ data: Data, named name: String) throws -> URL {
        guard Int64(data.count) <= HistoryPreviewData.maxBytes else {
            throw HistoryPreviewStoreError.tooLarge
        }
        try ensureDirectory()
        let safeName = Self.sanitizedFileName(name, fallback: "preview")
        let target = directory.appendingPathComponent(
            "\(UUID().uuidString)-\(safeName)",
            isDirectory: false
        )
        guard isInsideDirectory(target) else {
            throw HistoryPreviewStoreError.invalidPath
        }
        guard fileManager.createFile(
            atPath: target.path,
            contents: data,
            attributes: [.posixPermissions: NSNumber(value: 0o600)]
        ) else {
            throw HistoryPreviewStoreError.writeFailed
        }
        do {
            try fileManager.setAttributes(
                [.posixPermissions: NSNumber(value: 0o600)],
                ofItemAtPath: target.path
            )
        } catch {
            try? fileManager.removeItem(at: target)
            throw error
        }
        return target
    }

    func remove(_ url: URL) throws {
        let standardized = url.standardizedFileURL
        guard isInsideDirectory(standardized) else {
            throw HistoryPreviewStoreError.invalidPath
        }
        guard fileManager.fileExists(atPath: standardized.path) else { return }
        var isDirectory: ObjCBool = false
        guard fileManager.fileExists(atPath: standardized.path, isDirectory: &isDirectory),
              !isDirectory.boolValue else {
            throw HistoryPreviewStoreError.invalidPath
        }
        try fileManager.removeItem(at: standardized)
    }

    /// Normalize an untrusted filename to one safe path component.
    static func sanitizedFileName(_ name: String, fallback: String = "preview") -> String {
        let scalars = name.unicodeScalars.map { scalar -> Character in
            let value = scalar.value
            if CharacterSet.controlCharacters.contains(scalar)
                || value == 0x7F
                || scalar == "/"
                || scalar == "\\"
            {
                return "_"
            }
            return Character(String(scalar))
        }
        var result = String(scalars).trimmingCharacters(in: .whitespacesAndNewlines)
        while result.hasPrefix(".") {
            result.removeFirst()
        }
        if result.isEmpty || result == "." || result == ".." {
            result = fallback
        }
        result = boundedFileName(result, maxBytes: maximumSanitizedNameBytes)
        return result.isEmpty ? "preview" : result
    }

    private static func boundedFileName(_ value: String, maxBytes: Int) -> String {
        guard storageByteCount(value) > maxBytes else { return value }
        if let dot = value.lastIndex(of: "."), dot != value.startIndex {
            let suffix = String(value[dot...])
            let suffixBytes = storageByteCount(suffix)
            if suffixBytes < maxBytes {
                let stem = String(value[..<dot])
                let boundedStem = prefix(stem, fittingStorageBytes: maxBytes - suffixBytes)
                if !boundedStem.isEmpty { return boundedStem + suffix }
            }
        }
        return prefix(value, fittingStorageBytes: maxBytes)
    }

    private static func prefix(_ value: String, fittingStorageBytes maxBytes: Int) -> String {
        guard maxBytes > 0 else { return "" }
        var result = ""
        var usedBytes = 0
        for character in value {
            let part = String(character)
            let partBytes = storageByteCount(part)
            guard usedBytes + partBytes <= maxBytes else { break }
            result.append(character)
            usedBytes += partBytes
        }
        return result
    }

    private static func storageByteCount(_ value: String) -> Int {
        value.decomposedStringWithCanonicalMapping.utf8.count
    }

    private func ensureDirectory() throws {
        guard !isSymbolicLink(directory) else {
            throw HistoryPreviewStoreError.invalidPath
        }
        try fileManager.createDirectory(
            at: directory,
            withIntermediateDirectories: true,
            attributes: [.posixPermissions: NSNumber(value: 0o700)]
        )
        try setDirectoryPermissions()
    }

    private func setDirectoryPermissions() throws {
        guard !isSymbolicLink(directory) else {
            throw HistoryPreviewStoreError.invalidPath
        }
        try fileManager.setAttributes(
            [.posixPermissions: NSNumber(value: 0o700)],
            ofItemAtPath: directory.path
        )
    }

    private func isInsideDirectory(_ url: URL) -> Bool {
        guard !isSymbolicLink(directory) else { return false }
        let root = directory.standardizedFileURL.path
        let candidate = url.standardizedFileURL.path
        guard candidate != root else { return false }
        return candidate.hasPrefix(root + "/")
    }

    private func isSymbolicLink(_ url: URL) -> Bool {
        do {
            _ = try fileManager.destinationOfSymbolicLink(atPath: url.path)
            return true
        } catch {
            return false
        }
    }
}

enum HistoryPreviewStoreError: LocalizedError, Equatable {
    case tooLarge
    case invalidText
    case invalidImage
    case invalidDocument
    case invalidPath
    case writeFailed

    var errorDescription: String? {
        switch self {
        case .tooLarge: return "History preview exceeds the size limit"
        case .invalidText: return "History preview is not valid UTF-8 text"
        case .invalidImage: return "History preview image is invalid"
        case .invalidDocument: return "History preview document is invalid"
        case .invalidPath: return "History preview path is invalid"
        case .writeFailed: return "Could not create history preview file"
        }
    }
}
