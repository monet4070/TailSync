import Foundation
import AppKit
import SwiftUI
import XCTest
@testable import TailSync

private struct ResolvedTourRects: PreferenceKey {
    static var defaultValue: [String: CGRect] = [:]
    static func reduce(value: inout [String: CGRect], nextValue: () -> [String: CGRect]) {
        value.merge(nextValue(), uniquingKeysWith: { _, next in next })
    }
}

final class OnboardingTests: XCTestCase {
    private func temporaryURL() throws -> URL {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("tailsync-tour-\(UUID().uuidString)")
        addTeardownBlock { try? FileManager.default.removeItem(at: directory) }
        return directory.appendingPathComponent("onboarding-v2.json")
    }

    func testProgressResumesWithoutAcknowledgingAndFinishPersists() throws {
        let url = try temporaryURL()
        let progress = OnboardingProgress(url: url)
        XCTAssertFalse(progress.dismissed)
        progress.select(3)
        let resumed = OnboardingProgress(url: url)
        XCTAssertEqual(resumed.step, 3)
        XCTAssertFalse(resumed.dismissed)
        try resumed.finish()
        let finished = OnboardingProgress(url: url)
        XCTAssertTrue(finished.dismissed)
        XCTAssertEqual(finished.step, 3)
        finished.select(0)
        XCTAssertTrue(OnboardingProgress(url: url).dismissed)
    }

    func testCorruptOrOutOfRangeProgressFallsBackToWelcome() throws {
        let url = try temporaryURL()
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        for bytes in ["broken", "{\"step\":999,\"dismissed\":false}", "{\"step\":-1,\"dismissed\":false}"] {
            try Data(bytes.utf8).write(to: url)
            let progress = OnboardingProgress(url: url)
            XCTAssertEqual(progress.step, 0)
            XCTAssertFalse(progress.dismissed)
        }
    }

    func testFailedPersistenceDoesNotAcknowledgeTheTour() throws {
        let blocker = try temporaryURL()
        try FileManager.default.createDirectory(at: blocker.deletingLastPathComponent(), withIntermediateDirectories: true)
        try Data().write(to: blocker)
        let progress = OnboardingProgress(url: blocker.appendingPathComponent("child.json"))
        XCTAssertThrowsError(try progress.finish())
        XCTAssertFalse(progress.dismissed)
    }

    func testPermanentDismissalSurvivesRelaunchAndResetsAfterReinstall() throws {
        let url = try temporaryURL()
        let progress = OnboardingProgress(url: url, installationID: "installation-A")
        progress.select(8)
        try progress.finish(permanently: true)
        let relaunched = OnboardingProgress(url: url, installationID: "installation-A")
        XCTAssertTrue(relaunched.suppressed)
        XCTAssertTrue(relaunched.dismissed)
        let reinstalled = OnboardingProgress(url: url, installationID: "installation-B")
        XCTAssertFalse(reinstalled.suppressed)
        XCTAssertFalse(reinstalled.dismissed)
        XCTAssertEqual(reinstalled.step, 0)
        try reinstalled.finish()
        XCTAssertFalse(OnboardingProgress(url: url, installationID: "installation-B").suppressed)
    }

    func testBuiltInUpdatePreservesPermanentDismissal() throws {
        let url = try temporaryURL()
        let progress = OnboardingProgress(url: url, installationID: "old-bundle")
        try progress.finish(permanently: true)
        try progress.preserveAfterUpdate(installationID: "updated-bundle")
        XCTAssertTrue(OnboardingProgress(url: url, installationID: "updated-bundle").suppressed)
        XCTAssertFalse(OnboardingProgress(url: url, installationID: "reinstalled-bundle").suppressed)
    }

    func testLegacyDismissalIsAdoptedWithoutReopening() throws {
        let url = try temporaryURL()
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try Data(#"{"step":4,"dismissed":true,"suppressed":true}"#.utf8).write(to: url)
        XCTAssertTrue(OnboardingProgress(url: url, installationID: "legacy-install").suppressed)
        XCTAssertFalse(OnboardingProgress(url: url, installationID: "new-install").suppressed)
    }

    func testInstallationIdentityTracksBundleReplacementAtTheSamePath() throws {
        let root = try temporaryURL().deletingLastPathComponent()
        let bundle = root.appendingPathComponent("TailSync.app")
        try FileManager.default.createDirectory(at: bundle, withIntermediateDirectories: true)
        let original = OnboardingProgress.installationIdentity(at: bundle)
        XCTAssertEqual(original, OnboardingProgress.installationIdentity(at: bundle))
        try FileManager.default.moveItem(at: bundle, to: root.appendingPathComponent("backup.app"))
        try FileManager.default.createDirectory(at: bundle, withIntermediateDirectories: true)
        XCTAssertNotEqual(original, OnboardingProgress.installationIdentity(at: bundle))
    }

    func testEveryBundledStepHasBothLocalesAndActualWindowTargets() {
        let guide = OnboardingGuide.shared
        XCTAssertEqual(guide.steps.count, 15)
        XCTAssertEqual(Set(guide.steps.map(\.id)).count, guide.steps.count)
        for step in guide.steps {
            XCTAssertTrue(["history", "connections", "settings"].contains(step.window))
            XCTAssertFalse(step.target.isEmpty)
            for locale in ["en", "zh-CN"] {
                for copy in [step.title, step.body, step.tip] {
                    XCTAssertFalse(copy[locale, default: ""].isEmpty, "Missing \(locale) in \(step.id)")
                }
            }
        }
        for copy in guide.ui.values {
            XCTAssertFalse(copy["en", default: ""].isEmpty)
            XCTAssertFalse(copy["zh-CN", default: ""].isEmpty)
        }
    }

    func testDemoHistoryCoversEveryCategoryAndPreviewFamily() throws {
        let query = HistoryQuery(keyword: nil, category: nil, startTime: nil, endTime: nil, limit: 30, offset: 0, collection: "all")
        let entries = OnboardingDemo.entries(query: query)
        XCTAssertEqual(entries.count, OnboardingDemo.records.count)
        XCTAssertEqual(Set(entries.map(\.category)), Set(["text", "website", "code", "path", "image", "file"]))
        let names = Set(OnboardingDemo.records.map { $0.name.lowercased() })
        for expected in ["demo.txt", "demo.md", "demo.png", "demo.svg", "demo.pdf", "demo.docx", "demo.pptx"] {
            XCTAssertTrue(names.contains(expected), "Missing sample \(expected)")
        }
        XCTAssertTrue(OnboardingDemo.records.contains { $0.batchId == "tailsync-demo-batch" })
        XCTAssertNotNil(OnboardingDemo.records.first { $0.document != nil }.flatMap {
            OnboardingDemo.documentPreview(id: $0.id, owner: "history", generation: 1, requestId: "test")
        })
        for first in entries where first.batch_id == "tailsync-demo-batch" {
            XCTAssertNotNil(OnboardingDemo.previewData(id: first.id, batchId: first.batch_id))
        }
        XCTAssertNotNil(OnboardingDemo.peerSnapshot())
    }

    @MainActor
    func testPermanentDismissalCannotBeRestartedFromSettings() throws {
        let url = try temporaryURL()
        var destination = ""
        let controller = GuidedTourController(progress: OnboardingProgress(url: url), navigate: { destination = $0 })
        controller.start()
        controller.finish(permanently: true)
        XCTAssertTrue(controller.suppressed)
        controller.start()
        XCTAssertFalse(controller.active)
        XCTAssertEqual(destination, "connections")
        XCTAssertTrue(OnboardingProgress(url: url).suppressed)
    }

    @MainActor
    func testDemoEntriesLoadThroughTheRealPreviewPipeline() async throws {
        let query = HistoryQuery(keyword: nil, category: nil, startTime: nil, endTime: nil, limit: 30, offset: 0, collection: "all")
        let entries = OnboardingDemo.entries(query: query)
        let text = try XCTUnwrap(entries.first { $0.id == 8_000_000_001 })

        let textPreview = HistoryPreviewViewModel()
        textPreview.present(HistoryPreviewRequest(items: [HistoryPreviewItem(entry: text)], selectedIndex: 0))
        var textReady = false
        for _ in 0..<80 {
            if case .ready(let payload, _, let format) = textPreview.state {
                XCTAssertEqual(payload.kind, "text")
                XCTAssertEqual(format, .text)
                textReady = true
                textPreview.close()
                break
            }
            try await Task.sleep(for: .milliseconds(25))
        }
        XCTAssertTrue(textReady, "Text demo did not reach a preview")
        guard textReady else { return }

        for (id, expectedFormat) in [(8_000_000_008 as Int64, HistoryPreviewFormat.image), (8_000_000_010, .pdf), (8_000_000_009, .svg)] {
            let entry = try XCTUnwrap(entries.first { $0.id == id })
            let model = HistoryPreviewViewModel()
            model.present(HistoryPreviewRequest(items: [HistoryPreviewItem(entry: entry)], selectedIndex: 0))
            var rendered = false
            for _ in 0..<80 {
                if case .ready(_, _, let format) = model.state {
                    XCTAssertEqual(format, expectedFormat)
                    rendered = true
                    model.close()
                    break
                }
                if case .failed(let failure) = model.state {
                    XCTFail("Preview \(id) failed: \(failure)")
                    break
                }
                try await Task.sleep(for: .milliseconds(25))
            }
            XCTAssertTrue(rendered, "Preview \(id) did not render")
        }

        for id in [8_000_000_011 as Int64, 8_000_000_012, 8_000_000_013] {
            let document = try XCTUnwrap(entries.first { $0.id == id })
            let documentPreview = HistoryPreviewViewModel()
            documentPreview.present(HistoryPreviewRequest(items: [HistoryPreviewItem(entry: document)], selectedIndex: 0))
            var rendered = false
            for _ in 0..<80 {
                if case .document = documentPreview.state {
                    rendered = true
                    documentPreview.close()
                    break
                }
                if case .failed(let failure) = documentPreview.state {
                    XCTFail("Document demo \(id) failed: \(failure)")
                    break
                }
                try await Task.sleep(for: .milliseconds(25))
            }
            XCTAssertTrue(rendered, "Document demo \(id) did not render")
        }
    }

    @MainActor
    func testRoutesToRealWindowsAndResumesThenCanRestart() throws {
        let url = try temporaryURL()
        var windows: [String] = []
        let controller = GuidedTourController(progress: OnboardingProgress(url: url), navigate: { windows.append($0) })
        controller.start()
        controller.move(4)
        XCTAssertEqual(windows, ["connections", "history"])
        let resumed = GuidedTourController(progress: OnboardingProgress(url: url), navigate: { windows.append($0) })
        resumed.start(resume: true)
        XCTAssertEqual(resumed.index, 4)
        XCTAssertEqual(windows.last, "history")
        resumed.start()
        XCTAssertEqual(resumed.index, 0)
        resumed.move(10)
        XCTAssertEqual(windows.last, "settings")
        resumed.finish()
        XCTAssertFalse(resumed.active)
        XCTAssertTrue(OnboardingProgress(url: url).dismissed)
    }

    @MainActor
    func testNestedFavoritesTargetSurvivesSearchBarAndViewportAnchors() throws {
        _ = NSApplication.shared
        var frames: [String: CGRect] = [:]
        let bar = HistoryFilterBar(
            keyword: .constant(""), selectedCategory: .constant("all"), selectedDateFilter: .constant(.all),
            customStartDate: .constant(Date()), customEndDate: .constant(Date()), categories: [],
            categoryFilteringSupported: true, dateRangeFilteringSupported: true, daemonOnline: true,
            onSubmit: {}, onFilterChanged: {}
        )
        for width in [360.0, 540.0] {
            frames = [:]
            let hosting = NSHostingView(rootView: VStack {
                bar.tourTarget("history-search")
                Spacer()
            }.frame(width: width, height: 400).tourTarget("tour-viewport")
                .overlayPreferenceValue(TourAnchors.self) { anchors in
                    GeometryReader { geometry in
                        Color.clear.preference(key: ResolvedTourRects.self, value: anchors.mapValues { geometry[$0] })
                    }
                }.onPreferenceChange(ResolvedTourRects.self) { frames = $0 })
            hosting.frame = NSRect(x: 0, y: 0, width: width, height: 400)
            hosting.layoutSubtreeIfNeeded()
            let rep = try XCTUnwrap(hosting.bitmapImageRepForCachingDisplay(in: hosting.bounds))
            hosting.cacheDisplay(in: hosting.bounds, to: rep)
            let star = try XCTUnwrap(frames["favorites"], "The nested Favorites anchor must not be replaced")
            let search = try XCTUnwrap(frames["history-search"])
            XCTAssertTrue(search.contains(star))
            XCTAssertGreaterThan(star.width, 10)
            XCTAssertLessThan(star.width, 50, "Highlight only the actual star button")
            XCTAssertLessThan(star.height, 50)
            XCTAssertEqual(frames["tour-viewport"]?.width, CGFloat(width))
        }
    }

    func testHighlightUsesFullVisibleTargetAndClipsOnlyAtViewport() throws {
        let viewport = CGRect(x: 0, y: 0, width: 540, height: 400)
        let target = CGRect(x: 20, y: 12, width: 500, height: 370)
        XCTAssertEqual(TourGeometry.highlight(target: target, viewport: viewport), target.insetBy(dx: -4, dy: -4))
        let scrolled = CGRect(x: 20, y: -100, width: 500, height: 700)
        XCTAssertEqual(TourGeometry.highlight(target: scrolled, viewport: viewport), CGRect(x: 16, y: 0, width: 508, height: 400))
        XCTAssertNil(TourGeometry.highlight(target: CGRect(x: 20, y: 500, width: 100, height: 40), viewport: viewport))
        XCTAssertNil(TourGeometry.highlight(target: .zero, viewport: viewport))
        XCTAssertNil(TourGeometry.highlight(target: nil, viewport: viewport))
    }

    @MainActor
    func testRenderSpotlightOnActualConnectionControls() throws {
        _ = NSApplication.shared
        let previousLanguage = Loc.shared.lang
        defer { Loc.shared.lang = previousLanguage }
        let output = FileManager.default.temporaryDirectory.appendingPathComponent("tailsync-onboarding-render")
        try FileManager.default.createDirectory(at: output, withIntermediateDirectories: true)
        let controller = GuidedTourController(progress: OnboardingProgress(url: try temporaryURL()), navigate: { _ in })
        controller.start()
        for (language, width, height) in [("zh-CN", 540.0, 720.0), ("en", 480.0, 600.0)] {
            Loc.shared.lang = language
            let hosting = NSHostingView(rootView: VStack {
                ConnectionsView().connectionsCard
                Spacer()
            }
                .guidedTour(window: "connections", controller: controller)
                .environment(\.colorScheme, .light)
                .frame(width: width, height: height))
            hosting.frame = NSRect(x: 0, y: 0, width: width, height: height)
            hosting.layoutSubtreeIfNeeded()
            RunLoop.main.run(until: Date().addingTimeInterval(0.05))
            hosting.layoutSubtreeIfNeeded()
            let rep = try XCTUnwrap(hosting.bitmapImageRepForCachingDisplay(in: hosting.bounds))
            hosting.cacheDisplay(in: hosting.bounds, to: rep)
            let png = try XCTUnwrap(rep.representation(using: .png, properties: [:]))
            XCTAssertGreaterThan(png.count, 1000)
            try png.write(to: output.appendingPathComponent("onboarding-\(language).png"))
        }
    }

}
