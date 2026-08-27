import AppKit
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

/// Why an SVG shows the source viewer instead of a rendered snapshot.
/// Only `renderFailed` is retryable: active markup and oversized payloads
/// fail deterministically, so retrying them is noise.
enum HistoryPreviewSVGVisualFallback: Equatable {
    case blockedContent
    case tooLarge
    case renderFailed
}

@MainActor
final class HistoryPreviewViewModel: ObservableObject {
    @Published private(set) var state: HistoryPreviewLoadState = .idle
    @Published private(set) var items: [HistoryPreviewItem] = []
    @Published private(set) var currentIndex = 0
    @Published private(set) var restoreState: RestoreState = .idle
    /// True while the browser engine is rasterizing the current SVG; the
    /// preview shows a placeholder instead of the intermediate source text.
    @Published private(set) var isRenderingSVG = false
    /// Per-entry user choice to let the SVG preview load external images and
    /// fonts.  Resets on navigation and starts disabled.  Enabling is
    /// transactional: the flag turns true only after a trusted re-render
    /// successfully installs a new snapshot, so a failed render never leaves
    /// the UI claiming trust it did not deliver.
    @Published private(set) var svgExternalResourcesTrusted = false
    /// Set when the current SVG must stay in the source viewer; nil while a
    /// visual render is still possible or a snapshot is installed.  Only
    /// `renderFailed` is retryable — active markup and oversized payloads
    /// fail deterministically.
    @Published private(set) var svgVisualFallback: HistoryPreviewSVGVisualFallback?

    /// Host disclosure for the trust confirmation: which hosts enabling
    /// trust would contact, and which references can never be trusted
    /// (non-HTTPS or non-public literal IP targets).
    var svgExternalReferenceSummary: HistoryPreviewWebViewSVGRenderer.ExternalReferenceSummary {
        guard case .ready(let payload, _, let format) = state,
              format == .svg,
              let source = String(data: payload.data, encoding: .utf8) else {
            return .init()
        }
        return HistoryPreviewWebViewSVGRenderer.externalReferenceSummary(for: source)
    }

    enum RestoreState: Equatable {
        case idle
        case restoring
        case restored
        case failed
    }

    var onFormatChange: ((HistoryPreviewWindowKind) -> Void)?

    private let session: HistoryPreviewSession
    private let dependencies: HistoryPreviewDependencies
    private let svgWebRenderer: any HistoryPreviewWebSVGRendering
    private var generation = 0
    private var loadTask: Task<Void, Never>?
    private var restoreFeedbackTask: Task<Void, Never>?
    private var svgRenderTask: Task<Void, Never>?

    init(
        session: HistoryPreviewSession = HistoryPreviewSession(),
        dependencies: HistoryPreviewDependencies = .live,
        svgWebRenderer: any HistoryPreviewWebSVGRendering = HistoryPreviewWebViewSVGRenderer()
    ) {
        self.session = session
        self.dependencies = dependencies
        self.svgWebRenderer = svgWebRenderer
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

    /// Explicit user choice to (stop) letting the current SVG preview load
    /// external resources.  Only affects the current entry and re-renders it;
    /// scripts stay disabled either way.  Enabling trust is transactional:
    /// the flag flips only after the trusted re-render succeeds.
    func setSVGExternalResourcesTrusted(_ trusted: Bool) {
        guard trusted != svgExternalResourcesTrusted,
              case .ready(let payload, _, let format) = state,
              format == .svg,
              svgVisualFallback == nil else { return }
        if trusted {
            guard let source = String(data: payload.data, encoding: .utf8) else { return }
            let summary = HistoryPreviewWebViewSVGRenderer.externalReferenceSummary(for: source)
            guard summary.rejectedHosts.isEmpty else { return }
        } else {
            svgExternalResourcesTrusted = false
        }
        startSVGRender(payload: payload, trusting: trusted)
    }

    /// Retry the visual render after a `renderFailed` fallback.  Documents
    /// blocked for active markup or size are not retryable.
    func retrySVGVisualRender() {
        guard case .ready(let payload, _, let format) = state,
              format == .svg,
              svgVisualFallback == .renderFailed,
              let source = String(data: payload.data, encoding: .utf8),
              payload.data.count <= HistoryPreviewWebSVGLimits.maximumInputBytes,
              HistoryPreviewWebViewSVGRenderer.isSVGVisualEligible(source) else { return }
        svgVisualFallback = nil
        startSVGRender(payload: payload)
    }

    /// Rasterize (or re-rasterize) an SVG with the browser engine.  The
    /// result upgrades the base source-text material; a failure keeps
    /// whatever material is currently installed (escaped source, or the
    /// previous snapshot when re-rendering after a trust change).
    private func startSVGRender(payload: HistoryPreviewData, trusting: Bool? = nil) {
        svgRenderTask?.cancel()
        svgWebRenderer.cancel()
        // Set before publishing anything the placeholder must cover: the
        // render task only starts on the next main-actor hop, so a frame
        // could otherwise show the intermediate source or stale snapshot.
        isRenderingSVG = true
        let requestGeneration = generation
        let trustingExternalResources = trusting ?? svgExternalResourcesTrusted
        svgRenderTask = Task { [weak self] in
            await self?.renderSVG(
                payload: payload,
                trustingExternalResources: trustingExternalResources,
                requestGeneration: requestGeneration
            )
        }
    }

    private func renderSVG(
        payload: HistoryPreviewData,
        trustingExternalResources: Bool,
        requestGeneration: Int
    ) async {
        guard requestGeneration == generation else { return }
        isRenderingSVG = true
        defer {
            if requestGeneration == generation {
                isRenderingSVG = false
            }
        }
        guard let source = String(data: payload.data, encoding: .utf8) else { return }
        if trustingExternalResources {
            // Second enforcement point behind the UI gate: even if the
            // confirmation was bypassed, refuse to render a trusted document
            // whose references include ineligible targets.
            let summary = HistoryPreviewWebViewSVGRenderer.externalReferenceSummary(for: source)
            guard summary.rejectedHosts.isEmpty else {
                svgExternalResourcesTrusted = false
                return
            }
        }
        do {
            let png = try await svgWebRenderer.renderPNG(
                fromSVG: source,
                trustingExternalResources: trustingExternalResources
            )
            try Task.checkCancellation()
            guard requestGeneration == generation else { return }
            guard let image = NSImage(data: png), !image.representations.isEmpty else {
                markSVGRenderFailure()
                return
            }
            let material = HistoryPreviewImageMaterial(data: png, image: image)
            session.install(payload, material: .image(material))
            state = .ready(payload: payload, material: .image(material), format: .svg)
            svgVisualFallback = nil
            if trustingExternalResources {
                // Transactional commit: trust is only claimed once the
                // trusted snapshot is actually installed.
                svgExternalResourcesTrusted = true
            }
        } catch {
            // Cancellation, timeout, or a render failure: the currently
            // installed material remains the visible state.
            markSVGRenderFailure()
        }
    }

    /// Record a render failure for the fallback notice.  A failed trust
    /// re-render keeps the previous (untrusted) snapshot visible and must
    /// not leave the trusted flag set; a failed initial render leaves the
    /// escaped source as the only material.
    private func markSVGRenderFailure() {
        if !svgExternalResourcesTrusted,
           case .ready(_, .text, let format) = state,
           format == .svg {
            svgVisualFallback = .renderFailed
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
        svgRenderTask?.cancel()
        svgRenderTask = nil
        svgWebRenderer.cancel()
        session.close()
        state = .idle
        restoreState = .idle
        isRenderingSVG = false
        svgExternalResourcesTrusted = false
        svgVisualFallback = nil
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
        svgRenderTask?.cancel()
        svgRenderTask = nil
        svgWebRenderer.cancel()
        isRenderingSVG = false
        svgExternalResourcesTrusted = false
        svgVisualFallback = nil
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
                // SVG materializes as its escaped source; the locked-down
                // browser-engine snapshot upgrades it to a rasterized image
                // while the load task (and its generation) still govern it.
                // Documents with active markup or oversized payloads never
                // reach a web view: they classify straight to the source
                // viewer with an explicit reason, matching Windows.
                if format == .svg {
                    if let source = String(data: payload.data, encoding: .utf8) {
                        if payload.data.count > HistoryPreviewWebSVGLimits.maximumInputBytes {
                            svgVisualFallback = .tooLarge
                        } else if !HistoryPreviewWebViewSVGRenderer.isSVGVisualEligible(source) {
                            svgVisualFallback = .blockedContent
                        } else {
                            startSVGRender(payload: payload)
                        }
                    } else {
                        // Invalid UTF-8 SVG never reaches the materializer's
                        // text path either; the load fails with invalidText.
                        svgVisualFallback = .blockedContent
                    }
                }
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
