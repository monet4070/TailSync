import Foundation

struct HistoryPreviewDependencies: @unchecked Sendable {
    var load: @Sendable (Int64, String?) async throws -> HistoryPreviewData
    var restore: @Sendable (Int64) async throws -> Void

    static let live = HistoryPreviewDependencies(
        load: { id, batchId in
            try await ApiClient.shared.getPreviewData(id: id, batchId: batchId)
        },
        restore: { id in
            try await ApiClient.shared.restoreEntry(id: id)
        }
    )
}

enum HistoryPreviewLoadState: Equatable {
    case idle
    case loading
    case ready(
        payload: HistoryPreviewData,
        material: HistoryPreviewMaterial,
        format: HistoryPreviewFormat
    )
    case failed(HistoryPreviewFailure)
}

@MainActor
final class HistoryPreviewViewModel: ObservableObject {
    @Published private(set) var state: HistoryPreviewLoadState = .idle
    @Published private(set) var items: [HistoryPreviewItem] = []
    @Published private(set) var currentIndex = 0
    @Published private(set) var restoreState: RestoreState = .idle

    enum RestoreState: Equatable {
        case idle
        case restoring
        case restored
        case failed
    }

    var onFormatChange: ((HistoryPreviewWindowKind) -> Void)?

    private let session: HistoryPreviewSession
    private let dependencies: HistoryPreviewDependencies
    private var generation = 0
    private var loadTask: Task<Void, Never>?
    private var restoreFeedbackTask: Task<Void, Never>?

    init(
        session: HistoryPreviewSession = HistoryPreviewSession(),
        dependencies: HistoryPreviewDependencies = .live
    ) {
        self.session = session
        self.dependencies = dependencies
    }

    var currentItem: HistoryPreviewItem? {
        guard items.indices.contains(currentIndex) else { return nil }
        return items[currentIndex]
    }

    var currentEntryId: Int64? {
        if case .ready(let payload, _, _) = state, let entryId = payload.entryId {
            return entryId
        }
        return currentItem?.id
    }

    var currentName: String {
        if case .ready(let payload, _, _) = state { return payload.name }
        return currentItem?.nameHint ?? Loc.t("history.preview.title")
    }

    var currentSize: Int64 {
        if case .ready(let payload, _, _) = state { return payload.sizeBytes }
        return currentItem?.sizeBytes ?? 0
    }

    var batchPositionText: String? {
        if case .ready(let payload, _, _) = state, let batch = payload.batch {
            return "\(batch.itemIndex + 1) / \(batch.itemCount)"
        }
        guard items.count > 1 else { return nil }
        return "\(currentIndex + 1) / \(items.count)"
    }

    var canNavigateBackward: Bool {
        if case .ready(let payload, _, _) = state, let batch = payload.batch {
            return batch.previousEntryId != nil
        }
        return currentIndex > 0
    }

    var canNavigateForward: Bool {
        if case .ready(let payload, _, _) = state, let batch = payload.batch {
            return batch.nextEntryId != nil
        }
        return currentIndex + 1 < items.count
    }

    func present(_ request: HistoryPreviewRequest) {
        guard !request.items.isEmpty,
              request.items.indices.contains(request.selectedIndex) else {
            close()
            return
        }
        items = request.items
        currentIndex = request.selectedIndex
        restoreState = .idle
        onFormatChange?(request.items[request.selectedIndex].estimatedFormat.windowKind)
        loadCurrent()
    }

    func navigateBackward() {
        navigate(to: adjacentEntryId(forward: false), fallbackOffset: -1)
    }

    func navigateForward() {
        navigate(to: adjacentEntryId(forward: true), fallbackOffset: 1)
    }

    func retry() {
        guard currentItem != nil else { return }
        loadCurrent()
    }

    func restoreCurrent() {
        guard let id = currentEntryId, restoreState != .restoring else { return }
        restoreFeedbackTask?.cancel()
        restoreState = .restoring
        restoreFeedbackTask = Task { [weak self] in
            guard let self else { return }
            do {
                try await dependencies.restore(id)
                guard !Task.isCancelled else { return }
                restoreState = .restored
                try? await Task.sleep(for: .seconds(1.5))
                guard !Task.isCancelled else { return }
                restoreState = .idle
            } catch {
                guard !Task.isCancelled else { return }
                restoreState = .failed
            }
        }
    }

    func contains(entryId: Int64) -> Bool {
        items.contains { $0.id == entryId } || currentEntryId == entryId
    }

    func close() {
        generation += 1
        loadTask?.cancel()
        loadTask = nil
        restoreFeedbackTask?.cancel()
        restoreFeedbackTask = nil
        session.close()
        state = .idle
        restoreState = .idle
        items = []
        currentIndex = 0
    }

    private func adjacentEntryId(forward: Bool) -> Int64? {
        guard case .ready(let payload, _, _) = state,
              let batch = payload.batch else { return nil }
        return forward ? batch.nextEntryId : batch.previousEntryId
    }

    private func navigate(to adjacentId: Int64?, fallbackOffset: Int) {
        let targetIndex: Int?
        if let adjacentId {
            if let existing = items.firstIndex(where: { $0.id == adjacentId }) {
                targetIndex = existing
            } else if let currentItem {
                let directionIndex = max(0, (currentItem.batchIndex ?? currentIndex) + fallbackOffset)
                let synthetic = HistoryPreviewItem(
                    id: adjacentId,
                    batchId: currentItem.batchId,
                    batchIndex: directionIndex,
                    batchCount: currentItem.batchCount,
                    category: "file",
                    type: "file",
                    nameHint: Loc.t("history.preview.title"),
                    sizeBytes: 0
                )
                let insertionIndex = fallbackOffset > 0 ? currentIndex + 1 : currentIndex
                items.insert(synthetic, at: insertionIndex)
                targetIndex = insertionIndex
            } else {
                targetIndex = nil
            }
        } else {
            let fallback = currentIndex + fallbackOffset
            targetIndex = items.indices.contains(fallback) ? fallback : nil
        }
        guard let targetIndex else { return }
        currentIndex = targetIndex
        restoreState = .idle
        loadCurrent()
    }

    private func loadCurrent() {
        guard let item = currentItem else { return }
        generation += 1
        let requestGeneration = generation
        loadTask?.cancel()
        session.close()
        state = .loading
        let batchLookup = item.resolvesBatchFirst ? item.batchId : nil

        loadTask = Task { [weak self] in
            guard let self else { return }
            var preparedMaterial: HistoryPreviewMaterial?
            do {
                let payload = try await dependencies.load(item.id, batchLookup)
                try Task.checkCancellation()
                let material = try await session.prepare(payload)
                preparedMaterial = material
                try Task.checkCancellation()
                guard requestGeneration == generation else {
                    session.discard(material)
                    preparedMaterial = nil
                    return
                }
                session.install(payload, material: material)
                preparedMaterial = nil
                let format = HistoryPreviewFormat.detect(
                    payload: payload,
                    categoryHint: item.category
                )
                state = .ready(payload: payload, material: material, format: format)
                onFormatChange?(format.windowKind)
            } catch is CancellationError {
                if let preparedMaterial {
                    session.discard(preparedMaterial)
                }
                return
            } catch {
                if let preparedMaterial {
                    session.discard(preparedMaterial)
                }
                guard requestGeneration == generation else { return }
                session.close()
                state = .failed(HistoryPreviewFailure.classify(error))
            }
        }
    }

    deinit {
        loadTask?.cancel()
        restoreFeedbackTask?.cancel()
    }
}
