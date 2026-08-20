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

    nonisolated private static func docxFixture() -> Data {
        Data([0x50, 0x4B, 0x03, 0x04])
            + Data("[Content_Types].xml word/document.xml".utf8)
            + Data([0x50, 0x4B, 0x05, 0x06])
    }
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
