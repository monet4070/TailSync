import Combine
import Foundation

/// Owns one preview's memory and any short-lived Quick Look file.
final class HistoryPreviewSession: ObservableObject {
    @Published private(set) var text: String?
    @Published private(set) var fileURL: URL?
    @Published private(set) var payload: HistoryPreviewData?
    @Published private(set) var material: HistoryPreviewMaterial?

    private let store: HistoryPreviewStore

    init(store: HistoryPreviewStore = HistoryPreviewStore()) {
        self.store = store
    }

    /// Clean files left by a crashed process. Log rather than silently hiding
    /// permission/path failures so a security cleanup issue is diagnosable.
    @discardableResult
    static func cleanupAtStartup(store: HistoryPreviewStore = HistoryPreviewStore()) -> Int {
        do {
            return try store.cleanupStaleFiles()
        } catch {
            NSLog("[TailSync] history preview cleanup failed: %@", error.localizedDescription)
            return 0
        }
    }

    func open(_ preview: HistoryPreviewData) throws {
        close()
        let material = try store.materialize(preview)
        install(preview, material: material)
    }

    func prepare(_ preview: HistoryPreviewData) async throws -> HistoryPreviewMaterial {
        let store = self.store
        return try await Task.detached(priority: .userInitiated) {
            try store.materialize(preview)
        }.value
    }

    func install(_ preview: HistoryPreviewData, material: HistoryPreviewMaterial) {
        close()
        payload = preview
        self.material = material
        switch material {
        case .text(let value):
            text = value
            fileURL = nil
        case .quickLook(let url):
            text = nil
            fileURL = url
        case .image, .pdf, .unsupported:
            text = nil
            fileURL = nil
        }
    }

    /// Dispose a prepared Quick Look file if its request became stale before
    /// it could be installed in the live session.
    func discard(_ material: HistoryPreviewMaterial) {
        if case .quickLook(let url) = material {
            try? store.remove(url)
        }
    }

    func close() {
        if let url = fileURL {
            try? store.remove(url)
        }
        text = nil
        fileURL = nil
        payload = nil
        material = nil
    }

    deinit {
        if let url = fileURL {
            try? store.remove(url)
        }
    }
}
