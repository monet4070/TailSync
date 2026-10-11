import Foundation

/// The bundled helper both creates and resolves its own security-scoped bookmark.
/// Capture never invokes the grant command: only the settings button opens a panel.
enum QQImageCacheAccessController {
    enum Result: String {
        case authorized, notAuthorized = "not_authorized"
        case granted, cancelled, invalidDirectory = "invalid_directory", unavailable
    }

    static func status() async -> Result {
        await run(arguments: ["--qq-image-access-status"])
    }

    static func chooseDirectory(language: String) async -> Result {
        await run(arguments: ["--grant-qq-image-access", language == "zh-CN" ? "zh-CN" : "en"])
    }

    private static func run(arguments: [String]) async -> Result {
        let helper = Bundle.main.bundleURL.appendingPathComponent("Contents/MacOS/clipboard-helper")
        return await Task.detached(priority: .userInitiated) {
            let process = Process()
            let output = Pipe()
            process.executableURL = helper
            process.arguments = arguments
            process.standardOutput = output
            process.standardError = FileHandle.nullDevice
            process.standardInput = FileHandle.nullDevice
            do {
                try process.run()
                let data = output.fileHandleForReading.readDataToEndOfFile()
                process.waitUntilExit()
                guard process.terminationStatus == 0, data.count <= 256,
                      let raw = String(data: data, encoding: .utf8), let result = Result(rawValue: raw)
                else { return .unavailable }
                return result
            } catch {
                return .unavailable
            }
        }.value
    }
}
