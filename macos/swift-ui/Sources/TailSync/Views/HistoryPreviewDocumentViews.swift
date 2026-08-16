import AppKit
import PDFKit
import QuickLookUI
import SwiftUI

@MainActor
final class HistoryPDFPreviewController: ObservableObject {
    let document: PDFDocument
    @Published private(set) var pageIndex = 0
    @Published private(set) var pageCount = 0
    @Published private(set) var matchIndex = 0
    @Published private(set) var matchCount = 0
    @Published private(set) var zoomPercent = 100

    weak var pdfView: PDFView?
    private var matches: [PDFSelection] = []

    init(data: Data) {
        document = PDFDocument(data: data) ?? PDFDocument()
        pageCount = document.pageCount
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
        if index != NSNotFound { pageIndex = index }
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
        matches = query.isEmpty
            ? []
            : document.findString(query, withOptions: [.caseInsensitive])
        matchCount = matches.count
        matchIndex = 0
        showCurrentMatch()
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
        pdfView.setCurrentSelection(selection, animate: true)
        pdfView.go(to: selection)
    }
}

@MainActor
struct HistoryPDFPreviewView: View {
    @StateObject private var controller: HistoryPDFPreviewController
    @State private var query = ""
    @State private var showsThumbnails = true

    init(data: Data) {
        _controller = StateObject(wrappedValue: HistoryPDFPreviewController(data: data))
    }

    var body: some View {
        VStack(spacing: 0) {
            pdfToolbar
            Divider()
            HistoryPDFContainer(controller: controller, showsThumbnails: showsThumbnails)
        }
        .background(HistoryPreviewModifierScrollMonitor { delta in
            controller.adjustZoom(wheelDelta: delta)
        })
    }

    private var pdfToolbar: some View {
        HStack(spacing: 8) {
            Button { showsThumbnails.toggle() } label: {
                Image(systemName: "sidebar.left")
            }
            .help(Loc.t("history.preview.thumbnails"))
            Divider().frame(height: 18)
            Button(action: controller.previousPage) {
                Image(systemName: "chevron.up")
            }
            .disabled(controller.pageIndex <= 0)
            Button(action: controller.nextPage) {
                Image(systemName: "chevron.down")
            }
            .disabled(controller.pageIndex + 1 >= controller.pageCount)
            Text("\(min(controller.pageIndex + 1, max(controller.pageCount, 1))) / \(controller.pageCount)")
                .font(.caption.monospacedDigit())
                .frame(minWidth: 62)
            Divider().frame(height: 18)
            Button(action: controller.zoomOut) {
                Image(systemName: "minus.magnifyingglass")
            }
            Button(action: controller.fit) {
                Image(systemName: "arrow.up.left.and.arrow.down.right")
            }
            .help(Loc.t("history.preview.fit"))
            Button(action: controller.zoomIn) {
                Image(systemName: "plus.magnifyingglass")
            }
            Text("\(controller.zoomPercent)%")
                .font(.caption.monospacedDigit())
                .frame(minWidth: 46)
            Spacer(minLength: 8)
            HStack(spacing: 5) {
                Image(systemName: "magnifyingglass")
                    .foregroundStyle(.secondary)
                TextField(Loc.t("history.preview.search"), text: $query)
                    .textFieldStyle(.plain)
                    .onSubmit { controller.nextMatch() }
                    .onChange(of: query) { controller.updateSearch($0) }
                if controller.matchCount > 0 {
                    Text("\(controller.matchIndex + 1)/\(controller.matchCount)")
                        .font(.caption2.monospacedDigit())
                        .foregroundStyle(.secondary)
                }
                Button(action: controller.previousMatch) {
                    Image(systemName: "chevron.up")
                }
                .disabled(controller.matchCount == 0)
                Button(action: controller.nextMatch) {
                    Image(systemName: "chevron.down")
                }
                .disabled(controller.matchCount == 0)
            }
            .padding(.horizontal, 8)
            .frame(width: 250, height: 28)
            .background(.quaternary.opacity(0.45))
            .clipShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
        }
        .buttonStyle(.borderless)
        .controlSize(.small)
        .padding(.horizontal, 12)
        .frame(height: 42)
    }
}

@MainActor
private struct HistoryPDFContainer: NSViewRepresentable {
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
        container.thumbnailView.pdfView = container.pdfView
        controller.attach(container.pdfView)
        context.coordinator.observe(container.pdfView)
        return container
    }

    func updateNSView(_ container: PDFContainerView, context: Context) {
        container.setThumbnailsVisible(showsThumbnails)
        controller.attach(container.pdfView)
    }

    static func dismantleNSView(_ nsView: PDFContainerView, coordinator: Coordinator) {
        coordinator.stopObserving()
        nsView.pdfView.document = nil
        nsView.thumbnailView.pdfView = nil
    }

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

        deinit { stopObserving() }
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
        thumbnailView.layoutMode = .vertical
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
    }
}

struct HistoryQuickLookPreviewView: NSViewRepresentable {
    let url: URL

    func makeNSView(context: Context) -> QLPreviewView {
        let preview = QLPreviewView(frame: .zero, style: .normal)!
        preview.autostarts = true
        preview.previewItem = url as NSURL
        return preview
    }

    func updateNSView(_ preview: QLPreviewView, context: Context) {
        let currentURL = (preview.previewItem as? NSURL).map { $0 as URL }
        if currentURL?.standardizedFileURL != url.standardizedFileURL {
            preview.previewItem = url as NSURL
            preview.refreshPreviewItem()
        }
    }

    static func dismantleNSView(_ nsView: QLPreviewView, coordinator: ()) {
        nsView.previewItem = nil
    }
}
