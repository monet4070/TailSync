import Foundation
import Combine

/// The bundled helper both creates and resolves its own security-scoped bookmark.
/// Capture never invokes the grant command: only the settings switch opens a panel.
enum QQImageCacheAccessController {
    enum Result: String, Sendable {
        case authorized, notAuthorized = "not_authorized"
        case granted, cancelled, invalidDirectory = "invalid_directory", unavailable
        case revoked, copyMessageFirst = "copy_message_first"
    }

    fileprivate static func run(arguments: [String]) async -> Result {
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

/// The bookmark is the enabled state. A cancelled/failed system authorization
/// never turns the switch on, and a failed revocation never pretends it is off.
@MainActor
final class ThirdPartyImageAccessSettings: ObservableObject {
    @Published private(set) var isEnabled = false
    @Published private(set) var isBusy = false
    @Published private(set) var messageKey: String?
    private let request: @Sendable ([String]) async -> QQImageCacheAccessController.Result

    nonisolated init(request: @escaping @Sendable ([String]) async -> QQImageCacheAccessController.Result = {
        await QQImageCacheAccessController.run(arguments: $0)
    }) {
        self.request = request
    }

    func load() async {
        guard !isBusy else { return }
        isBusy = true
        defer { isBusy = false }
        switch await request(["--qq-image-access-status"]) {
        case .authorized:
            isEnabled = true
            messageKey = "settings.qqImageAccessReady"
        case .notAuthorized:
            isEnabled = false
            messageKey = nil
        default:
            messageKey = "settings.qqImageAccessFailed"
        }
    }

    func setEnabled(_ enabled: Bool, language: String) async {
        guard !isBusy, enabled != isEnabled else { return }
        isBusy = true
        defer { isBusy = false }
        let arguments = enabled
            ? ["--grant-qq-image-access", language == "zh-CN" ? "zh-CN" : "en"]
            : ["--revoke-qq-image-access"]
        switch await request(arguments) {
        case .granted where enabled:
            isEnabled = true
            messageKey = "settings.qqImageAccessReady"
        case .revoked where !enabled:
            isEnabled = false
            messageKey = "settings.qqImageAccessDisabled"
        case .cancelled:
            break
        case .copyMessageFirst:
            messageKey = "settings.qqImageAccessCopyFirst"
        default:
            messageKey = "settings.qqImageAccessFailed"
        }
    }
}
