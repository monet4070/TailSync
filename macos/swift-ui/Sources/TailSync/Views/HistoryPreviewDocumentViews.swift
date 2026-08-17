import AppKit
import PDFKit
import QuickLookUI
import SwiftUI

@MainActor
final class HistoryPDFPreviewController: ObservableObject {
    static let maximumSearchMatches = 2_000

    let document: PDFDocument
    @Published private(set) var pageIndex = 0
    @Published private(set) var pageCount = 0
    @Published private(set) var matchIndex = 0
    @Published private(set) var matchCount = 0
    @Published private(set) var zoomPercent = 100
    @Published private(set) var isSearching = false
    @Published private(set) var isSearchTruncated = false

    weak var pdfView: PDFView?
    private var matches: [PDFSelection] = []
    private var pendingSearchTask: Task<Void, Never>?
    private var searchObservers: [NSObjectProtocol] = []
    private var acceptsSearchMatches = false

    init(document: PDFDocument) {
        self.document = document
        pageCount = document.pageCount
        observeSearch()
    }

    func attach(_ pdfView: PDFView) {
        self.pdfView = pdfView
        if pdfView.document !== document { pdfView.document = document }
        updateCurrentPage()
        updateZoom()
    }

    func updateCurrentPage() {
        guard let page = pdfView?.currentPage else { return }
        let index = document.index(for: page)
        if index != NSNotFound, pageIndex != index { pageIndex = index }
    }

    func previousPage() {
        pdfView?.goToPreviousPage(nil)
        updateCurrentPage()
    }

    func nextPage() {
        pdfView?.goToNextPage(nil)
        updateCurrentPage()
    }

    func zoomOut() { pdfView?.zoomOut(nil) }
    func zoomIn() { pdfView?.zoomIn(nil) }
    func fit() {
        pdfView?.autoScales = true
        updateZoom()
    }

    func adjustZoom(wheelDelta: CGFloat) {
        guard wheelDelta != 0, let pdfView else { return }
        pdfView.autoScales = false
        let factor: CGFloat = wheelDelta > 0 ? 1.1 : 1 / 1.1
        pdfView.scaleFactor = min(
            pdfView.maxScaleFactor,
            max(pdfView.minScaleFactor, pdfView.scaleFactor * factor)
        )
        updateZoom()
    }

    func updateZoom() {
        guard let pdfView else { return }
        let updatedPercent = Int((pdfView.scaleFactor * 100).rounded())
        if zoomPercent != updatedPercent { zoomPercent = updatedPercent }
    }

    func updateSearch(_ query: String) {
        pendingSearchTask?.cancel()
        pendingSearchTask = nil
        acceptsSearchMatches = false
        document.cancelFindString()
        matches.removeAll(keepingCapacity: true)
        matchCount = 0
        matchIndex = 0
        showCurrentMatch()
        isSearchTruncated = false
        guard !query.isEmpty else {
            isSearching = false
            return
        }

        // PDFDocument.findString is synchronous and can block the main actor
        // for seconds on large documents. The notification-based PDFKit API
        // performs the search incrementally; debounce typing so stale queries
        // never start expensive work.
        isSearching = true
        pendingSearchTask = Task { [weak self] in
            try? await Task.sleep(for: .milliseconds(180))
            guard let self, !Task.isCancelled else { return }
            acceptsSearchMatches = true
            document.beginFindString(query, withOptions: [.caseInsensitive])
        }
    }

    func searchDidBegin() {
        if !isSearching { isSearching = true }
    }

    func searchDidFind(_ selection: PDFSelection) {
        guard acceptsSearchMatches,
              matches.count < Self.maximumSearchMatches else { return }
        matches.append(selection)
        matchCount = matches.count
        if matches.count == 1 {
            matchIndex = 0
            showCurrentMatch()
        }
        if matches.count == Self.maximumSearchMatches {
            isSearchTruncated = true
            document.cancelFindString()
        }
    }

    func searchDidEnd() {
        pendingSearchTask = nil
        guard acceptsSearchMatches else { return }
        acceptsSearchMatches = false
        if isSearching { isSearching = false }
    }

    func cancelSearch() {
        pendingSearchTask?.cancel()
        pendingSearchTask = nil
        acceptsSearchMatches = false
        document.cancelFindString()
        if isSearching { isSearching = false }
    }

    func previousMatch() {
        guard !matches.isEmpty else { return }
        matchIndex = (matchIndex - 1 + matches.count) % matches.count
        showCurrentMatch()
    }

    func nextMatch() {
        guard !matches.isEmpty else { return }
        matchIndex = (matchIndex + 1) % matches.count
        showCurrentMatch()
    }

    private func showCurrentMatch() {
        guard matches.indices.contains(matchIndex), let pdfView else {
            pdfView?.setCurrentSelection(nil, animate: false)
            return
        }
        let selection = matches[matchIndex]
        pdfView.setCurrentSelection(selection, animate: false)
        pdfView.go(to: selection)
    }

    private func observeSearch() {
        let center = NotificationCenter.default
        searchObservers = [
            center.addObserver(
                forName: .PDFDocumentDidBeginFind,
                object: document,
                queue: .main
            ) { [weak self] _ in
                Task { @MainActor in self?.searchDidBegin() }
            },
            center.addObserver(
                forName: .PDFDocumentDidFindMatch,
                object: document,
                queue: .main
            ) { [weak self] notification in
                guard let selection = notification.userInfo?[PDFDocumentFoundSelectionKey]
                    as? PDFSelection else { return }
                Task { @MainActor in self?.searchDidFind(selection) }
            },
            center.addObserver(
                forName: .PDFDocumentDidEndFind,
                object: document,
                queue: .main
            ) { [weak self] _ in
                Task { @MainActor in self?.searchDidEnd() }
            }
        ]
    }

    deinit {
        pendingSearchTask?.cancel()
        let center = NotificationCenter.default
        searchObservers.forEach(center.removeObserver)
    }
}

@MainActor
struct HistoryPDFPreviewView: View {
    @StateObject private var controller: HistoryPDFPreviewController
    @State private var query = ""
    @State private var showsThumbnails = false
    @Environment(\.tailSyncPalette) private var palette

    init(material: HistoryPreviewPDFMaterial) {
        _controller = StateObject(
            wrappedValue: HistoryPDFPreviewController(document: material.document)
        )
    }

    var body: some View {
        VStack(spacing: 0) {
            pdfToolbar
            HistoryPDFContainer(controller: controller, showsThumbnails: showsThumbnails)
                .background(HistoryPreviewModifierScrollMonitor { delta in
                    controller.adjustZoom(wheelDelta: delta)
                })
        }
        .background(palette.surfaceColor)
    }

    private var pdfToolbar: some View {
        HStack(spacing: 8) {
            HistoryPreviewToolbarIconButton(
                systemName: "sidebar.left",
                selected: showsThumbnails,
                action: { showsThumbnails.toggle() }
            )
            .help(Loc.t("history.preview.thumbnails"))
            Divider().frame(height: 18)
            HistoryPreviewToolbarIconButton(
                systemName: "chevron.up",
                action: controller.previousPage
            )
            .disabled(controller.pageIndex <= 0)
            HistoryPreviewToolbarIconButton(
                systemName: "chevron.down",
                action: controller.nextPage
            )
            .disabled(controller.pageIndex + 1 >= controller.pageCount)
            Text("\(min(controller.pageIndex + 1, max(controller.pageCount, 1))) / \(controller.pageCount)")
                .font(.caption.monospacedDigit())
                .frame(minWidth: 62)
            Divider().frame(height: 18)
            HistoryPreviewToolbarIconButton(
                systemName: "minus.magnifyingglass",
                action: controller.zoomOut
            )
            HistoryPreviewToolbarIconButton(
                systemName: "arrow.up.left.and.arrow.down.right",
                action: controller.fit
            )
            .help(Loc.t("history.preview.fit"))
            HistoryPreviewToolbarIconButton(
                systemName: "plus.magnifyingglass",
                action: controller.zoomIn
            )
            Text("\(controller.zoomPercent)%")
                .font(.caption.monospacedDigit())
                .frame(minWidth: 46)
            Spacer(minLength: 8)
            HStack(spacing: 5) {
                Image(systemName: "magnifyingglass")
                    .foregroundStyle(.secondary)
                TextField(Loc.t("history.preview.search"), text: $query)
                    .textFieldStyle(.plain)
                    .font(.system(size: 13))
                    .onSubmit { controller.nextMatch() }
                    .onChange(of: query) { controller.updateSearch($0) }
                if controller.isSearching {
                    ProgressView().controlSize(.mini)
                }
                if controller.matchCount > 0 {
                    Text("\(controller.matchIndex + 1)/\(controller.matchCount)\(controller.isSearchTruncated ? "+" : "")")
                        .font(.caption2.monospacedDigit())
                        .foregroundColor(palette.tertiaryColor)
                }
                HistoryPreviewToolbarIconButton(
                    systemName: "chevron.up",
                    compact: true,
                    action: controller.previousMatch
                )
                .disabled(controller.matchCount == 0)
                HistoryPreviewToolbarIconButton(
                    systemName: "chevron.down",
                    compact: true,
                    action: controller.nextMatch
                )
                .disabled(controller.matchCount == 0)
            }
            .padding(.leading, 10)
            .padding(.trailing, 4)
            .frame(width: 270, height: HistoryPreviewLayoutMetrics.regularControlSize)
            .background(palette.softSurfaceColor)
            .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
            .overlay {
                RoundedRectangle(cornerRadius: 8, style: .continuous)
                    .stroke(palette.borderColor, lineWidth: 1)
            }
        }
        .historyPreviewToolbarStyle()
    }
}

@MainActor
private struct HistoryPDFContainer: NSViewRepresentable {
    @Environment(\.tailSyncPalette) private var palette

    @ObservedObject var controller: HistoryPDFPreviewController
    let showsThumbnails: Bool

    func makeCoordinator() -> Coordinator { Coordinator(controller: controller) }

    func makeNSView(context: Context) -> PDFContainerView {
        let container = PDFContainerView()
        container.pdfView.document = controller.document
        container.pdfView.autoScales = true
        container.pdfView.displayMode = .singlePageContinuous
        container.pdfView.displayDirection = .vertical
        container.pdfView.displaysPageBreaks = true
        container.pdfView.backgroundColor = NSColor(palette.softSurfaceColor)
        container.setThumbnailsVisible(showsThumbnails)
        controller.attach(container.pdfView)
        context.coordinator.observe(container.pdfView)
        return container
    }

    func updateNSView(_ container: PDFContainerView, context: Context) {
        container.pdfView.backgroundColor = NSColor(palette.softSurfaceColor)
        container.setThumbnailsVisible(showsThumbnails)
        controller.attach(container.pdfView)
    }

    static func dismantleNSView(_ nsView: PDFContainerView, coordinator: Coordinator) {
        coordinator.stopObserving()
        coordinator.controller?.cancelSearch()
        nsView.pdfView.document = nil
        nsView.thumbnailView.pdfView = nil
    }

    @MainActor
    final class Coordinator {
        weak var controller: HistoryPDFPreviewController?
        var observers: [NSObjectProtocol] = []

        init(controller: HistoryPDFPreviewController) {
            self.controller = controller
        }

        func observe(_ pdfView: PDFView) {
            stopObserving()
            let center = NotificationCenter.default
            observers = [
                center.addObserver(
                    forName: .PDFViewPageChanged,
                    object: pdfView,
                    queue: .main
                ) { [weak controller] _ in
                    Task { @MainActor in controller?.updateCurrentPage() }
                },
                center.addObserver(
                    forName: .PDFViewScaleChanged,
                    object: pdfView,
                    queue: .main
                ) { [weak controller] _ in
                    Task { @MainActor in controller?.updateZoom() }
                }
            ]
        }

        func stopObserving() {
            let center = NotificationCenter.default
            observers.forEach(center.removeObserver)
            observers.removeAll()
        }

        deinit {
            let center = NotificationCenter.default
            observers.forEach(center.removeObserver)
        }
    }
}

@MainActor
private final class PDFContainerView: NSView {
    let thumbnailView = PDFThumbnailView()
    let pdfView = PDFView()
    private var thumbnailWidth: NSLayoutConstraint!

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        thumbnailView.translatesAutoresizingMaskIntoConstraints = false
        pdfView.translatesAutoresizingMaskIntoConstraints = false
        // Vertical is the documented default layout mode for a thumbnail
        // sidebar; PDFThumbnailView.layoutMode is not importable in this
        // SDK's Swift overlay, so the default is left in place.
        thumbnailView.thumbnailSize = NSSize(width: 92, height: 124)
        addSubview(thumbnailView)
        addSubview(pdfView)
        thumbnailWidth = thumbnailView.widthAnchor.constraint(equalToConstant: 150)
        NSLayoutConstraint.activate([
            thumbnailView.leadingAnchor.constraint(equalTo: leadingAnchor),
            thumbnailView.topAnchor.constraint(equalTo: topAnchor),
            thumbnailView.bottomAnchor.constraint(equalTo: bottomAnchor),
            thumbnailWidth,
            pdfView.leadingAnchor.constraint(equalTo: thumbnailView.trailingAnchor),
            pdfView.trailingAnchor.constraint(equalTo: trailingAnchor),
            pdfView.topAnchor.constraint(equalTo: topAnchor),
            pdfView.bottomAnchor.constraint(equalTo: bottomAnchor)
        ])
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    func setThumbnailsVisible(_ visible: Bool) {
        thumbnailView.isHidden = !visible
        thumbnailWidth.constant = visible ? 150 : 0
        thumbnailView.pdfView = visible ? pdfView : nil
    }
}

@MainActor
struct HistoryQuickLookPreviewView: NSViewRepresentable {
    @Environment(\.tailSyncPalette) private var palette

    let url: URL

    func makeNSView(context: Context) -> HistoryQuickLookContainerView {
        HistoryQuickLookContainerView(
            url: url,
            backgroundColor: NSColor(palette.surfaceColor)
        )
    }

    func updateNSView(_ container: HistoryQuickLookContainerView, context: Context) {
        container.backgroundColor = NSColor(palette.surfaceColor)
        container.setURL(url)
    }

    static func dismantleNSView(_ nsView: HistoryQuickLookContainerView, coordinator: ()) {
        nsView.clear()
    }
}

@MainActor
final class HistoryQuickLookContainerView: NSView {
    private(set) var previewView: QLPreviewView?
    private let fallbackLabel = NSTextField(labelWithString: Loc.t("history.preview.unavailableTitle"))

    var backgroundColor: NSColor {
        didSet { layer?.backgroundColor = backgroundColor.cgColor }
    }

    init(url: URL, backgroundColor: NSColor) {
        self.backgroundColor = backgroundColor
        super.init(frame: .zero)
        wantsLayer = true
        layer?.backgroundColor = backgroundColor.cgColor

        if let preview = QLPreviewView(frame: .zero, style: .normal) {
            preview.translatesAutoresizingMaskIntoConstraints = false
            preview.autostarts = true
            addSubview(preview)
            NSLayoutConstraint.activate([
                preview.leadingAnchor.constraint(equalTo: leadingAnchor),
                preview.trailingAnchor.constraint(equalTo: trailingAnchor),
                preview.topAnchor.constraint(equalTo: topAnchor),
                preview.bottomAnchor.constraint(equalTo: bottomAnchor)
            ])
            previewView = preview
            setURL(url)
        } else {
            fallbackLabel.translatesAutoresizingMaskIntoConstraints = false
            fallbackLabel.textColor = .secondaryLabelColor
            addSubview(fallbackLabel)
            NSLayoutConstraint.activate([
                fallbackLabel.centerXAnchor.constraint(equalTo: centerXAnchor),
                fallbackLabel.centerYAnchor.constraint(equalTo: centerYAnchor)
            ])
        }
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    func setURL(_ url: URL) {
        guard let previewView else { return }
        let currentURL = (previewView.previewItem as? NSURL).map { $0 as URL }
        guard currentURL?.standardizedFileURL != url.standardizedFileURL else { return }
        previewView.previewItem = url as NSURL
        previewView.refreshPreviewItem()
    }

    func clear() {
        previewView?.previewItem = nil
    }
}
