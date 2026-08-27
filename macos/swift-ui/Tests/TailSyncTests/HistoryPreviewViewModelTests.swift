import AppKit
import Foundation
import XCTest
@testable import TailSync

@MainActor
final class HistoryPreviewViewModelTests: XCTestCase {
    func testRestorePublishesSuccessFeedback() async throws {
        let model = HistoryPreviewViewModel(
            dependencies: HistoryPreviewDependencies(
                load: { id, _ in Self.textPayload(id: id, text: "restorable") },
                restore: { _ in }
            )
        )
        model.present(Self.request(id: 41))
        try await waitUntil { model.currentEntryId == 41 && model.isReady }

        model.restoreCurrent()
        try await waitUntil { model.restoreState == .restored }

        XCTAssertEqual(model.restoreState, .restored)
    }

    func testNewPresentationDiscardsAStaleLoad() async throws {
        let dependencies = HistoryPreviewDependencies(
            load: { id, _ in
                if id == 1 {
                    try? await Task.sleep(nanoseconds: 120_000_000)
                } else {
                    try? await Task.sleep(nanoseconds: 5_000_000)
                }
                return Self.textPayload(id: id, text: "entry-\(id)")
            },
            restore: { _ in }
        )
        let model = HistoryPreviewViewModel(dependencies: dependencies)

        model.present(Self.request(id: 1))
        try await Task.sleep(nanoseconds: 15_000_000)
        model.present(Self.request(id: 2))

        try await waitUntil { model.currentEntryId == 2 && model.isReady }
        try await Task.sleep(nanoseconds: 150_000_000)
        XCTAssertEqual(model.currentEntryId, 2)
        guard case .ready(let payload, _, _) = model.state else {
            return XCTFail("the latest preview should remain installed")
        }
        XCTAssertEqual(String(data: payload.data, encoding: .utf8), "entry-2")
    }

    func testBatchNavigationLoadsIdsOutsideTheHistoryPage() async throws {
        let dependencies = HistoryPreviewDependencies(
            load: { id, batchId in
                if batchId == "batch-a" {
                    return Self.filePayload(
                        id: 10,
                        name: "first.txt",
                        batch: Self.batch(index: 0, previous: nil, next: 11)
                    )
                }
                return Self.filePayload(
                    id: id,
                    name: "item-\(id).txt",
                    batch: Self.batch(
                        index: id == 11 ? 1 : 2,
                        previous: id == 11 ? 10 : 11,
                        next: id == 11 ? 12 : nil
                    )
                )
            },
            restore: { _ in }
        )
        let model = HistoryPreviewViewModel(dependencies: dependencies)
        let collapsedItem = HistoryPreviewItem(
            id: 99,
            batchId: "batch-a",
            batchIndex: 4,
            batchCount: 5,
            category: "file",
            type: "file",
            nameHint: "visible-page-item.txt",
            sizeBytes: 4,
            resolvesBatchFirst: true
        )

        model.present(HistoryPreviewRequest(items: [collapsedItem], selectedIndex: 0))
        try await waitUntil { model.currentEntryId == 10 && model.isReady }
        XCTAssertEqual(model.batchPositionText, "1 / 3")

        model.navigateForward()
        try await waitUntil { model.currentEntryId == 11 && model.isReady }
        XCTAssertTrue(model.items.contains { $0.id == 11 })
        XCTAssertEqual(model.batchPositionText, "2 / 3")
    }

    func testRetryRecoversAfterTypedPayloadFailure() async throws {
        let loader = RetryPreviewLoader()
        let dependencies = HistoryPreviewDependencies(
            load: { id, _ in try await loader.load(id: id) },
            restore: { _ in }
        )
        let model = HistoryPreviewViewModel(dependencies: dependencies)

        model.present(Self.request(id: 7))
        try await waitUntil {
            if case .failed = model.state { return true }
            return false
        }
        guard case .failed(let failure) = model.state else {
            return XCTFail("the first load should fail")
        }
        XCTAssertEqual(failure.kind, .decryption)
        XCTAssertTrue(failure.canRetry)

        model.retry()
        try await waitUntil { model.currentEntryId == 7 && model.isReady }
        let callCount = await loader.callCount
        XCTAssertEqual(callCount, 2)
    }

    func testCloseDeletesInstalledDocxMaterial() async throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("tailsync-preview-model-test-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: directory) }
        let session = HistoryPreviewSession(store: HistoryPreviewStore(directory: directory))
        let docx = Self.docxFixture()
        let payload = HistoryPreviewData(
            kind: "file",
            name: "private.docx",
            sizeBytes: Int64(docx.count),
            data: docx,
            entryId: 31
        )
        let model = HistoryPreviewViewModel(
            session: session,
            dependencies: HistoryPreviewDependencies(
                load: { _, _ in payload },
                restore: { _ in }
            )
        )

        model.present(Self.request(id: 31, name: "private.docx"))
        try await waitUntil { model.currentEntryId == 31 && model.isReady }
        guard case .ready(_, .quickLook(let url), _) = model.state else {
            return XCTFail("DOCX should use a private Quick Look file")
        }
        XCTAssertTrue(FileManager.default.fileExists(atPath: url.path))

        model.close()
        XCTAssertFalse(FileManager.default.fileExists(atPath: url.path))
        XCTAssertEqual(model.state, .idle)
    }

    func testSVGLoadUpgradesSourceToBrowserEngineImageMaterial() async throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("tailsync-preview-model-test-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let session = HistoryPreviewSession(store: HistoryPreviewStore(directory: directory))
        let payload = Self.svgPayload(id: 51)
        let webRenderer = StubSVGWebRenderer(result: .success(try Self.pngFixture()))
        let model = HistoryPreviewViewModel(
            session: session,
            dependencies: HistoryPreviewDependencies(
                load: { _, _ in payload },
                restore: { _ in }
            ),
            svgWebRenderer: webRenderer
        )

        model.present(Self.request(id: 51, name: "vector.svg"))
        try await waitUntil {
            if case .ready(_, .image, _) = model.state { return true }
            return false
        }
        XCTAssertEqual(webRenderer.renderCount, 1)
        guard case .ready(let installed, .image(let material), let format) = model.state else {
            return XCTFail("the browser engine should rasterize SVG into an image material")
        }
        XCTAssertEqual(format, .svg)
        XCTAssertFalse(material.image.representations.isEmpty)
        XCTAssertEqual(installed.data, payload.data)
        // The snapshot stayed in memory.
        XCTAssertTrue(try FileManager.default.contentsOfDirectory(atPath: directory.path).isEmpty)
    }

    func testSVGLoadFailureKeepsInMemorySourceViewer() async throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("tailsync-preview-model-test-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let session = HistoryPreviewSession(store: HistoryPreviewStore(directory: directory))
        let payload = Self.svgPayload(id: 52)
        let webRenderer = StubSVGWebRenderer(
            result: .failure(HistoryPreviewWebSVGRendererError.timedOut)
        )
        let model = HistoryPreviewViewModel(
            session: session,
            dependencies: HistoryPreviewDependencies(
                load: { _, _ in payload },
                restore: { _ in }
            ),
            svgWebRenderer: webRenderer
        )

        model.present(Self.request(id: 52, name: "vector.svg"))
        try await waitUntil { model.isReady }
        // The web render fails asynchronously after the source viewer is
        // installed; both states end with the escaped source in memory.
        try await Task.sleep(nanoseconds: 150_000_000)
        guard case .ready(_, .text(let source), let format) = model.state else {
            return XCTFail("a failed web render must keep the source viewer")
        }
        XCTAssertEqual(format, .svg)
        XCTAssertEqual(source, String(data: payload.data, encoding: .utf8))
    }

    func testNonSVGEntriesNeverReachTheWebRenderer() async throws {
        let webRenderer = StubSVGWebRenderer(result: .success(try Self.pngFixture()))
        let model = HistoryPreviewViewModel(
            dependencies: HistoryPreviewDependencies(
                load: { id, _ in Self.textPayload(id: id, text: "plain") },
                restore: { _ in }
            ),
            svgWebRenderer: webRenderer
        )
        model.present(Self.request(id: 61))
        try await waitUntil { model.isReady }

        try await Task.sleep(nanoseconds: 50_000_000)
        XCTAssertEqual(webRenderer.renderCount, 0, "text entries must not reach the web renderer")
    }

    func testTrustingExternalResourcesReRendersAndResetsPerEntry() async throws {
        let session = HistoryPreviewSession()
        let firstPayload = Self.svgPayload(id: 71)
        let secondPayload = Self.svgPayload(id: 72)
        let payloads: [Int64: HistoryPreviewData] = [71: firstPayload, 72: secondPayload]
        let webRenderer = StubSVGWebRenderer(result: .success(try Self.pngFixture()))
        let model = HistoryPreviewViewModel(
            session: session,
            dependencies: HistoryPreviewDependencies(
                load: { id, _ in payloads[id] ?? firstPayload },
                restore: { _ in }
            ),
            svgWebRenderer: webRenderer
        )

        let firstItem = HistoryPreviewItem(
            id: 71,
            batchId: nil,
            batchIndex: nil,
            batchCount: nil,
            category: "file",
            type: "file",
            nameHint: "vector.svg",
            sizeBytes: 4
        )
        let secondItem = HistoryPreviewItem(
            id: 72,
            batchId: nil,
            batchIndex: nil,
            batchCount: nil,
            category: "file",
            type: "file",
            nameHint: "vector.svg",
            sizeBytes: 4
        )
        model.present(HistoryPreviewRequest(items: [firstItem, secondItem], selectedIndex: 0))
        try await waitUntil {
            if case .ready(_, .image, _) = model.state { return true }
            return false
        }
        XCTAssertEqual(webRenderer.trustedFlags, [false])

        model.setSVGExternalResourcesTrusted(true)
        try await waitUntil { webRenderer.trustedFlags.count == 2 }
        XCTAssertEqual(webRenderer.trustedFlags.last, true)
        XCTAssertTrue(model.svgExternalResourcesTrusted)
        try await waitUntil {
            if case .ready(_, .image, _) = model.state { return true }
            return false
        }

        // Navigating to the next entry resets the trust choice.
        model.navigateForward()
        try await waitUntil {
            if case .ready(let payload, .image, _) = model.state,
               payload.entryId == 72 {
                return true
            }
            return false
        }
        XCTAssertFalse(model.svgExternalResourcesTrusted)
        try await waitUntil { webRenderer.trustedFlags.count == 3 }
        XCTAssertEqual(webRenderer.trustedFlags.last, false)
    }

    func testSVGRenderKeepsPreviousMaterialOnTrustRerenderFailure() async throws {
        let session = HistoryPreviewSession()
        let payload = Self.svgPayload(id: 81)
        let webRenderer = StubSVGWebRenderer(result: .success(try Self.pngFixture()))
        let model = HistoryPreviewViewModel(
            session: session,
            dependencies: HistoryPreviewDependencies(
                load: { _, _ in payload },
                restore: { _ in }
            ),
            svgWebRenderer: webRenderer
        )
        model.present(Self.request(id: 81, name: "vector.svg"))
        try await waitUntil {
            if case .ready(_, .image, _) = model.state { return true }
            return false
        }

        webRenderer.result = .failure(HistoryPreviewWebSVGRendererError.timedOut)
        model.setSVGExternalResourcesTrusted(true)
        try await waitUntil { webRenderer.renderCount == 2 }
        try await Task.sleep(nanoseconds: 50_000_000)
        guard case .ready(_, .image, _) = model.state else {
            return XCTFail("a failed trusted re-render must keep the previous snapshot")
        }
        XCTAssertFalse(model.isRenderingSVG)
        // Transactional trust: the flag must not claim trust the failed
        // re-render did not deliver, and no retry fallback is published
        // because the previous snapshot is still the visible state.
        XCTAssertFalse(model.svgExternalResourcesTrusted)
        XCTAssertNil(model.svgVisualFallback)
    }

    /// Transactional trust commit: the flag only turns true once the
    /// trusted snapshot is installed.
    func testTrustEnableCommitsOnlyAfterSuccessfulRender() async throws {
        let payload = Self.svgPayload(id: 83)
        let webRenderer = StubSVGWebRenderer(result: .success(try Self.pngFixture()))
        let model = HistoryPreviewViewModel(
            dependencies: HistoryPreviewDependencies(
                load: { _, _ in payload },
                restore: { _ in }
            ),
            svgWebRenderer: webRenderer
        )
        model.present(Self.request(id: 83, name: "vector.svg"))
        try await waitUntil {
            if case .ready(_, .image, _) = model.state { return true }
            return false
        }
        XCTAssertFalse(model.svgExternalResourcesTrusted)

        model.setSVGExternalResourcesTrusted(true)
        try await waitUntil { webRenderer.renderCount == 2 }
        try await waitUntil {
            if case .ready(_, .image, _) = model.state { return true }
            return false
        }
        XCTAssertEqual(webRenderer.trustedFlags.last, true)
        XCTAssertTrue(model.svgExternalResourcesTrusted)
    }

    /// Active markup is classified before any web view exists: the renderer
    /// is never called and the source viewer explains why.
    func testActiveMarkupSVGNeverReachesTheWebRenderer() async throws {
        let source = ##"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="2"><script>alert(1)</script><rect width="2" height="2"/></svg>"##
        let data = Data(source.utf8)
        let payload = HistoryPreviewData(
            kind: "file",
            name: "active.svg",
            sizeBytes: Int64(data.count),
            data: data,
            entryId: 84
        )
        let webRenderer = StubSVGWebRenderer(result: .success(try Self.pngFixture()))
        let model = HistoryPreviewViewModel(
            dependencies: HistoryPreviewDependencies(
                load: { _, _ in payload },
                restore: { _ in }
            ),
            svgWebRenderer: webRenderer
        )
        model.present(Self.request(id: 84, name: "active.svg"))
        try await waitUntil { model.isReady }
        try await Task.sleep(nanoseconds: 50_000_000)

        XCTAssertEqual(webRenderer.renderCount, 0, "active markup must not reach a web view")
        XCTAssertEqual(model.svgVisualFallback, .blockedContent)
        guard case .ready(_, .text, _) = model.state else {
            return XCTFail("active markup stays in the source viewer")
        }
        // Trust cannot be enabled for a document with no visual render.
        model.setSVGExternalResourcesTrusted(true)
        try await Task.sleep(nanoseconds: 30_000_000)
        XCTAssertEqual(webRenderer.renderCount, 0)
        XCTAssertFalse(model.svgExternalResourcesTrusted)
    }

    func testOversizedSVGClassifiesToTooLargeWithoutRendering() async throws {
        let header = Data("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"2\" height=\"2\"><rect width=\"2\" height=\"2\"/>".utf8)
        let oversized = header
            + Data(repeating: 0x20, count: HistoryPreviewWebSVGLimits.maximumInputBytes)
            + Data("</svg>".utf8)
        let payload = HistoryPreviewData(
            kind: "file",
            name: "huge.svg",
            sizeBytes: Int64(oversized.count),
            data: oversized,
            entryId: 85
        )
        let webRenderer = StubSVGWebRenderer(result: .success(try Self.pngFixture()))
        let model = HistoryPreviewViewModel(
            dependencies: HistoryPreviewDependencies(
                load: { _, _ in payload },
                restore: { _ in }
            ),
            svgWebRenderer: webRenderer
        )
        model.present(Self.request(id: 85, name: "huge.svg"))
        try await waitUntil { model.isReady }
        try await Task.sleep(nanoseconds: 50_000_000)

        XCTAssertEqual(webRenderer.renderCount, 0)
        XCTAssertEqual(model.svgVisualFallback, .tooLarge)
        guard case .ready(_, .text, _) = model.state else {
            return XCTFail("oversized SVG stays in the source viewer")
        }
    }

    /// A transient render failure publishes the retryable fallback, and the
    /// retry recovers the visual render.
    func testRenderFailureFallsBackAndRetryRecovers() async throws {
        let payload = Self.svgPayload(id: 86)
        let webRenderer = StubSVGWebRenderer(
            result: .failure(HistoryPreviewWebSVGRendererError.timedOut)
        )
        let model = HistoryPreviewViewModel(
            dependencies: HistoryPreviewDependencies(
                load: { _, _ in payload },
                restore: { _ in }
            ),
            svgWebRenderer: webRenderer
        )
        model.present(Self.request(id: 86, name: "vector.svg"))
        try await waitUntil { model.isReady }
        try await waitUntil { model.svgVisualFallback == .renderFailed }
        guard case .ready(_, .text, _) = model.state else {
            return XCTFail("a failed initial render keeps the source viewer")
        }

        webRenderer.result = .success(try Self.pngFixture())
        model.retrySVGVisualRender()
        try await waitUntil {
            if case .ready(_, .image, _) = model.state { return true }
            return false
        }
        XCTAssertNil(model.svgVisualFallback)
    }

    func testRejectedExternalTargetCannotEnableTrustOrLeaveRendererBusy() async throws {
        let source = ##"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="2"><image href="https://127.0.0.1/private.png"/></svg>"##
        let data = Data(source.utf8)
        let payload = HistoryPreviewData(
            kind: "file",
            name: "private.svg",
            sizeBytes: Int64(data.count),
            data: data,
            entryId: 82
        )
        let webRenderer = StubSVGWebRenderer(result: .success(try Self.pngFixture()))
        let model = HistoryPreviewViewModel(
            dependencies: HistoryPreviewDependencies(
                load: { _, _ in payload },
                restore: { _ in }
            ),
            svgWebRenderer: webRenderer
        )
        model.present(Self.request(id: 82, name: "private.svg"))
        try await waitUntil {
            if case .ready(_, .image, _) = model.state { return true }
            return false
        }

        model.setSVGExternalResourcesTrusted(true)
        try await Task.sleep(nanoseconds: 50_000_000)

        XCTAssertEqual(webRenderer.renderCount, 1)
        XCTAssertFalse(model.svgExternalResourcesTrusted)
        XCTAssertFalse(model.isRenderingSVG)
    }

    func testSVGExternalReferenceSummaryClassifiesHosts() async throws {
        let source = ##"""
<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><image href="https://cdn.example.com/a.png"/><image href="http://plain.example.org/b.png"/><image href="https://192.168.5.5/c.png"/></svg>
"""##
        let payload = HistoryPreviewData(
            kind: "file",
            name: "vector.svg",
            sizeBytes: Int64(source.utf8.count),
            data: Data(source.utf8),
            entryId: 91
        )
        let model = HistoryPreviewViewModel(
            dependencies: HistoryPreviewDependencies(
                load: { _, _ in payload },
                restore: { _ in }
            ),
            svgWebRenderer: StubSVGWebRenderer(
                result: .failure(HistoryPreviewWebSVGRendererError.timedOut)
            )
        )
        model.present(Self.request(id: 91, name: "vector.svg"))
        try await waitUntil { model.isReady }

        let summary = model.svgExternalReferenceSummary
        XCTAssertEqual(summary.allowedHosts, ["cdn.example.com"])
        XCTAssertEqual(
            Set(summary.rejectedHosts),
            ["plain.example.org", "192.168.5.5"]
        )

        // Non-SVG entries never disclose trust targets.
        let textModel = HistoryPreviewViewModel(
            dependencies: HistoryPreviewDependencies(
                load: { id, _ in Self.textPayload(id: id, text: "plain") },
                restore: { _ in }
            ),
            svgWebRenderer: StubSVGWebRenderer(result: .success(try Self.pngFixture()))
        )
        textModel.present(Self.request(id: 92))
        try await waitUntil { textModel.isReady }
        XCTAssertTrue(textModel.svgExternalReferenceSummary.allowedHosts.isEmpty)
        XCTAssertTrue(textModel.svgExternalReferenceSummary.rejectedHosts.isEmpty)
    }

    private func waitUntil(
        attempts: Int = 200,
        condition: @MainActor () -> Bool
    ) async throws {
        for _ in 0..<attempts {
            if condition() { return }
            try await Task.sleep(nanoseconds: 10_000_000)
        }
        XCTFail("Timed out waiting for preview state")
    }

    nonisolated private static func request(
        id: Int64,
        name: String = "text.txt"
    ) -> HistoryPreviewRequest {
        HistoryPreviewRequest(
            items: [HistoryPreviewItem(
                id: id,
                batchId: nil,
                batchIndex: nil,
                batchCount: nil,
                category: "text",
                type: "text",
                nameHint: name,
                sizeBytes: 4
            )],
            selectedIndex: 0
        )
    }

    nonisolated private static func textPayload(
        id: Int64,
        text: String
    ) -> HistoryPreviewData {
        let data = Data(text.utf8)
        return HistoryPreviewData(
            kind: "text",
            name: "text.txt",
            sizeBytes: Int64(data.count),
            data: data,
            entryId: id
        )
    }

    nonisolated private static func filePayload(
        id: Int64,
        name: String,
        batch: HistoryPreviewBatchNavigation
    ) -> HistoryPreviewData {
        let data = Data("preview".utf8)
        return HistoryPreviewData(
            kind: "file",
            name: name,
            sizeBytes: Int64(data.count),
            data: data,
            entryId: id,
            batch: batch
        )
    }

    nonisolated private static func batch(
        index: Int,
        previous: Int64?,
        next: Int64?
    ) -> HistoryPreviewBatchNavigation {
        HistoryPreviewBatchNavigation(
            batchId: "batch-a",
            itemIndex: index,
            itemCount: 3,
            firstEntryId: 10,
            lastEntryId: 12,
            previousEntryId: previous,
            nextEntryId: next
        )
    }

    nonisolated private static func svgPayload(id: Int64) -> HistoryPreviewData {
        let source = #"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="2"><rect width="2" height="2"/></svg>"#
        let data = Data(source.utf8)
        return HistoryPreviewData(
            kind: "file",
            name: "vector.svg",
            sizeBytes: Int64(data.count),
            data: data,
            entryId: id
        )
    }

    nonisolated private static func pngFixture() throws -> Data {
        let image = NSImage(size: NSSize(width: 2, height: 2))
        image.lockFocus()
        NSColor.systemBlue.setFill()
        NSRect(x: 0, y: 0, width: 2, height: 2).fill()
        image.unlockFocus()
        return try XCTUnwrap(
            NSBitmapImageRep(data: try XCTUnwrap(image.tiffRepresentation))?
                .representation(using: .png, properties: [:])
        )
    }

    nonisolated private static func docxFixture() -> Data {
        Data([0x50, 0x4B, 0x03, 0x04])
            + Data("[Content_Types].xml word/document.xml".utf8)
            + Data([0x50, 0x4B, 0x05, 0x06])
    }
}

@MainActor
private final class StubSVGWebRenderer: HistoryPreviewWebSVGRendering {
    var result: Result<Data, Error>
    private(set) var renderCount = 0
    private(set) var trustedFlags: [Bool] = []

    init(result: Result<Data, Error>) {
        self.result = result
    }

    func renderPNG(
        fromSVG source: String,
        trustingExternalResources: Bool
    ) async throws -> Data {
        renderCount += 1
        trustedFlags.append(trustingExternalResources)
        switch result {
        case .success(let png): return png
        case .failure(let error): throw error
        }
    }

    func cancel() {}
}

private actor RetryPreviewLoader {
    private(set) var callCount = 0

    func load(id: Int64) throws -> HistoryPreviewData {
        callCount += 1
        if callCount == 1 {
            throw HistoryPreviewRemoteError(
                code: .payloadUnavailable,
                message: "wording intentionally irrelevant"
            )
        }
        let data = Data("recovered".utf8)
        return HistoryPreviewData(
            kind: "text",
            name: "text.txt",
            sizeBytes: Int64(data.count),
            data: data,
            entryId: id
        )
    }
}

private extension HistoryPreviewViewModel {
    var isReady: Bool {
        if case .ready = state { return true }
        return false
    }
}
