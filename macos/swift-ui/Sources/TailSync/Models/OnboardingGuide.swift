import Foundation
import Combine

struct OnboardingGuide: Decodable {
    struct Step: Decodable, Identifiable {
        let id: String
        let window: String
        let target: String
        let title: [String: String]
        let body: [String: String]
        let tip: [String: String]
    }
    let version: Int
    let ui: [String: [String: String]]
    let steps: [Step]

    // Bundled, generated content; decoding and locale coverage are tested together.
    static let shared = try! JSONDecoder().decode(Self.self, from: Data(OnboardingContent.json.utf8))

    func text(_ value: [String: String], language: String) -> String {
        value[language.hasPrefix("zh") ? "zh-CN" : "en"] ?? value["en"] ?? ""
    }

    func label(_ key: String, language: String) -> String {
        text(ui[key] ?? [:], language: language)
    }
}

/// Device-local presentation state, outside shared settings and clipboard storage.
final class OnboardingProgress: ObservableObject {
    private struct Saved: Codable {
        var step: Int
        var dismissed: Bool
        var suppressed: Bool?
        var installationID: String?
    }
    @Published private(set) var step: Int
    private(set) var dismissed: Bool
    private(set) var suppressed: Bool
    private let url: URL
    private var installationID: String

    /// The bundle directory survives an in-place UI rebuild, but a freshly
    /// installed bundle is a new filesystem object even at the same path.
    static func installationIdentity(at bundle: URL = Bundle.main.bundleURL) -> String {
        let attributes = try? FileManager.default.attributesOfItem(atPath: bundle.path)
        if let volume = attributes?[.systemNumber] as? NSNumber,
           let file = attributes?[.systemFileNumber] as? NSNumber {
            return "\(volume):\(file)"
        }
        return bundle.standardizedFileURL.path
    }

    static var defaultURL: URL {
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
        return Loc.configurationURL(environment: ProcessInfo.processInfo.environment, applicationSupport: support)
            .deletingLastPathComponent().appendingPathComponent("onboarding-v2.json")
    }

    init(url: URL = OnboardingProgress.defaultURL,
         installationID: String = OnboardingProgress.installationIdentity()) {
        self.url = url
        self.installationID = installationID
        var saved = (try? Data(contentsOf: url)).flatMap { try? JSONDecoder().decode(Saved.self, from: $0) }
        if let previous = saved?.installationID, previous != installationID { saved = nil }
        step = Self.validStep(saved?.step ?? 0)
        dismissed = saved?.dismissed ?? false
        suppressed = saved?.suppressed ?? false
        // Adopt legacy preferences once, so upgrading does not reopen a tour
        // that the user has already closed.
        if saved != nil && saved?.installationID == nil {
            try? persist(dismissed: dismissed, suppressed: suppressed)
        }
    }

    static func validStep(_ value: Int) -> Int {
        OnboardingGuide.shared.steps.indices.contains(value) ? value : 0
    }

    func select(_ value: Int) {
        step = Self.validStep(value)
        try? persist(dismissed: dismissed, suppressed: suppressed)
    }

    func finish(permanently: Bool = false) throws {
        try persist(dismissed: true, suppressed: suppressed || permanently)
        dismissed = true
        suppressed = suppressed || permanently
    }

    /// The built-in updater replaces the entire bundle. Carry the preference
    /// to that new bundle before relaunching; manual reinstall starts fresh.
    func preserveAfterUpdate(installationID: String = OnboardingProgress.installationIdentity()) throws {
        try persist(dismissed: dismissed, suppressed: suppressed, installationID: installationID)
        self.installationID = installationID
    }

    private func persist(dismissed: Bool, suppressed: Bool, installationID: String? = nil) throws {
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        let data = try JSONEncoder().encode(Saved(step: step, dismissed: dismissed, suppressed: suppressed,
                                                installationID: installationID ?? self.installationID))
        try data.write(to: url, options: .atomic)
    }
}
