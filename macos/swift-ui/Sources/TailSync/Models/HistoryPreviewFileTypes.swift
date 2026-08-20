import Foundation

enum HistoryPreviewFileTypes {
    static let textExtensions: Set<String> = [
        "txt", "md", "markdown", "svg", "json", "jsonl", "xml", "yaml", "yml",
        "toml", "ini", "cfg", "conf", "log", "csv", "tsv", "html", "htm", "css",
        "js", "jsx", "ts", "tsx", "swift", "rs", "go", "py", "rb", "java", "kt",
        "kts", "c", "h", "cc", "cpp", "cxx", "hpp", "cs", "php", "sh", "bash",
        "zsh", "fish", "ps1", "sql"
    ]

    static let imageExtensions: Set<String> = [
        "png", "jpg", "jpeg", "gif", "webp", "heic", "heif", "tif", "tiff", "bmp"
    ]

    static func fileExtension(for name: String) -> String {
        let component = name.split(whereSeparator: { $0 == "/" || $0 == "\\" }).last
            .map(String.init) ?? name
        guard let dot = component.lastIndex(of: "."),
              dot < component.index(before: component.endIndex) else { return "" }
        return String(component[component.index(after: dot)...]).lowercased()
    }
}
