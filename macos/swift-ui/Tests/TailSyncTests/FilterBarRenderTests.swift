import AppKit
import SwiftUI
import XCTest
@testable import TailSync

/// Renders the reworked filter bar and the filter menu bodies to PNG files so
/// the layout can be inspected directly (AC8). Files land in
/// /tmp/tailsync-filter-render/ — this is a visual self-check, not a pixel
/// assertion suite. A second renderer (NSHostingView) is used in addition to
/// ImageRenderer because AppKit-bridged controls (TextField, DatePicker) do
/// not draw faithfully inside ImageRenderer.
final class FilterBarRenderTests: XCTestCase {
    private var outputDirectory: URL {
        URL(fileURLWithPath: "/tmp/tailsync-filter-render", isDirectory: true)
    }

    private func writePNG(_ image: NSImage, name: String) throws {
        try FileManager.default.createDirectory(
            at: outputDirectory,
            withIntermediateDirectories: true
        )
        guard
            let tiff = image.tiffRepresentation,
            let rep = NSBitmapImageRep(data: tiff),
            let png = rep.representation(using: .png, properties: [:])
        else {
            XCTFail("could not encode PNG for \(name)")
            return
        }
        try png.write(to: outputDirectory.appendingPathComponent("\(name).png"))
    }

    @MainActor
    private func renderImageRenderer(_ view: some View, name: String, width: CGFloat) throws {
        let renderer = ImageRenderer(
            content: view
                .frame(width: width)
                .padding(1)
        )
        renderer.scale = 2
        try writePNG(try XCTUnwrap(renderer.nsImage, "renderer produced no image"), name: name)
    }

    @MainActor
    private func renderHosting(_ view: some View, name: String, width: CGFloat) throws {
        let hosting = NSHostingView(rootView: view.frame(width: width))
        hosting.frame = NSRect(x: 0, y: 0, width: width * 2, height: 600)
        hosting.layoutSubtreeIfNeeded()
        let height = hosting.fittingSize.height
        hosting.frame = NSRect(x: 0, y: 0, width: width * 2, height: max(height * 2, 200))
        hosting.layoutSubtreeIfNeeded()
        guard let rep = hosting.bitmapImageRepForCachingDisplay(in: hosting.bounds) else {
            XCTFail("hosting renderer produced no bitmap for \(name)")
            return
        }
        hosting.cacheDisplay(in: hosting.bounds, to: rep)
        let image = NSImage(size: rep.size)
        image.addRepresentation(rep)
        try writePNG(image, name: name)
    }

    @MainActor
    func testRenderFilterBarAtSupportedWindowWidths() throws {
        _ = NSApplication.shared

        for width in [300.0, 400.0] {
            for scheme in [ColorScheme.light, ColorScheme.dark] {
                let bar = HistoryFilterBar(
                    keyword: .constant(""),
                    selectedCategory: .constant("all"),
                    selectedDateFilter: .constant(.last7),
                    customStartDate: .constant(Date()),
                    customEndDate: .constant(Date()),
                    categories: ["all", "text", "website", "code", "command",
                                 "structured_data", "path", "image", "file"],
                    categoryFilteringSupported: true,
                    dateRangeFilteringSupported: true,
                    daemonOnline: true,
                    onSubmit: {},
                    onFilterChanged: {}
                )
                .environment(\.colorScheme, scheme)

                let name = "bar-\(Int(width))-\(scheme == .light ? "light" : "dark")"
                try renderImageRenderer(bar, name: name, width: width)
                try renderHosting(bar, name: "\(name)-hosting", width: width)
            }
        }
    }

    @MainActor
    func testRenderCustomDateRangeRow() throws {
        _ = NSApplication.shared
        let bar = HistoryFilterBar(
            keyword: .constant("clipboard"),
            selectedCategory: .constant("text"),
            selectedDateFilter: .constant(.custom),
            customStartDate: .constant(Date()),
            customEndDate: .constant(Date()),
            categories: ["all", "text", "image", "file"],
            categoryFilteringSupported: true,
            dateRangeFilteringSupported: true,
            daemonOnline: true,
            onSubmit: {},
            onFilterChanged: {}
        )
        .environment(\.colorScheme, .light)

        try renderImageRenderer(bar, name: "bar-custom-range-400-light", width: 400)
        try renderHosting(bar, name: "bar-custom-range-400-light-hosting", width: 400)
    }

    @MainActor
    func testRenderFilterMenuBodies() throws {
        _ = NSApplication.shared
        for scheme in [ColorScheme.light, ColorScheme.dark] {
            let dateMenu = FilterMenuList(
                options: HistoryDateFilter.allCases.map { (id: $0.rawValue, label: $0.label) },
                selectedID: HistoryDateFilter.last7.rawValue,
                onSelect: { _ in }
            )
            .environment(\.colorScheme, scheme)

            try renderImageRenderer(
                dateMenu,
                name: "date-menu-\(scheme == .light ? "light" : "dark")",
                width: 220
            )

            let categoryMenu = FilterMenuList(
                options: ["all", "text", "website", "code", "command",
                          "structured_data", "path", "image", "file"]
                    .map { (id: $0, label: Loc.t("history.category.\($0)")) },
                selectedID: "text",
                onSelect: { _ in }
            )
            .environment(\.colorScheme, scheme)

            try renderImageRenderer(
                categoryMenu,
                name: "category-menu-\(scheme == .light ? "light" : "dark")",
                width: 220
            )
        }
    }
}
