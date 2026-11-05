import AppKit
import SwiftUI

struct HistoryPreviewDocumentView: View {
    let material: HistoryPreviewDocumentMaterial

    @Environment(\.tailSyncPalette) private var palette
    @AppStorage(HistoryPreviewDocumentReadingOptions.fontSizeKey)
    private var fontSize = HistoryPreviewDocumentReadingOptions.defaultFontSize
    @AppStorage(HistoryPreviewDocumentReadingOptions.lineSpacingKey)
    private var lineSpacing: HistoryPreviewDocumentReadingOptions.LineSpacing = .standard
    @AppStorage(HistoryPreviewDocumentReadingOptions.comfortableWidthKey)
    private var comfortableWidth = false
    @State private var query = ""
    @State private var searchRevision = 0
    @State private var searchDirection = 1
    @State private var searchSummary = HistoryPreviewDocumentSearchSummary()

    var body: some View {
        VStack(spacing: 0) {
            HistoryPreviewDocumentToolbar(
                query: $query,
                fontSize: Binding(
                    get: { HistoryPreviewPreferences.clampedTextFontSize(fontSize) },
                    set: { fontSize = HistoryPreviewPreferences.clampedTextFontSize($0) }
                ),
                lineSpacing: $lineSpacing,
                comfortableWidth: $comfortableWidth,
                searchSummary: searchSummary,
                find: { forward in
                    searchDirection = forward ? 1 : -1
                    searchRevision &+= 1
                },
                resetReading: {
                    fontSize = HistoryPreviewDocumentReadingOptions.defaultFontSize
                    lineSpacing = .standard
                    comfortableWidth = false
                },
                copyText: {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(
                        HistoryPreviewDocumentReadingStyle.plainText(material.attributed),
                        forType: .string
                    )
                }
            )
            noticeSection
            HistoryPreviewDocumentTextView(
                material: material,
                readingOptions: HistoryPreviewDocumentReadingOptions(
                    fontSize: fontSize, lineSpacing: lineSpacing, comfortableWidth: comfortableWidth
                ),
                searchQuery: query,
                searchRevision: searchRevision,
                searchDirection: searchDirection,
                onSearchSummary: { searchSummary = $0 }
            )
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .clipped()
                .background(HistoryPreviewModifierScrollMonitor { delta in
                    fontSize = HistoryPreviewPreferences.textFontSize(afterModifierScroll: delta, current: fontSize)
                })
        }
        .background(palette.surfaceColor)
    }

    @ViewBuilder
    private var noticeSection: some View {
        if !material.warningNotices.isEmpty {
            warningBanner
        } else if !material.diagnosticNotices.isEmpty {
            diagnosticEntry
        }
    }

    private var warningBanner: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(summaryLine)
                .font(.caption.weight(.semibold))
                .foregroundColor(palette.warningColor)
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityLabel(summaryLine)
            noticeDisclosure(notices: material.notices, labelKey: "history.preview.noticeDetails")
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
        .background(palette.warningColor.opacity(0.08))
        .overlay(alignment: .bottom) {
            Rectangle().fill(palette.dividerColor).frame(height: 1)
        }
    }

    private var diagnosticEntry: some View {
        noticeDisclosure(notices: material.diagnosticNotices, labelKey: "history.preview.noticeDiagnostics")
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 16)
            .padding(.vertical, 6)
            .background(palette.surfaceColor)
    }

    private var summaryLine: String {
        let separator = Loc.t("history.preview.noticeSeparator")
        let reasons = material.warningSummaryKeys.map { Loc.t($0) }.joined(separator: separator)
        return Loc.t("history.preview.noticeSummary").replacingOccurrences(of: "{reasons}", with: reasons)
    }

    private func noticeDisclosure(notices: [HistoryPreviewDocumentMaterial.Notice], labelKey: String) -> some View {
        DisclosureGroup {
            VStack(alignment: .leading, spacing: 6) {
                ForEach(Array(notices.enumerated()), id: \.offset) { _, notice in
                    VStack(alignment: .leading, spacing: 1) {
                        Text(Loc.t(notice.labelKey))
                            .font(.caption2)
                            .foregroundColor(palette.secondaryColor)
                        Text(notice.code)
                            .font(.caption2.monospaced())
                            .foregroundColor(palette.tertiaryColor)
                            .textSelection(.enabled)
                        if let detail = notice.detail {
                            Text(detail)
                                .font(.caption2)
                                .foregroundColor(palette.tertiaryColor)
                                .textSelection(.enabled)
                        }
                    }
                    .accessibilityElement(children: .combine)
                    .accessibilityLabel(notice.detail.map { "\(Loc.t(notice.labelKey)) — \($0)" } ?? Loc.t(notice.labelKey))
                }
            }
            .padding(.top, 4)
        } label: {
            Text(Loc.t(labelKey).replacingOccurrences(of: "{count}", with: "\(notices.count)"))
                .font(.caption)
                .foregroundColor(palette.secondaryColor)
        }
    }
}

struct HistoryPreviewDocumentTextView: NSViewRepresentable {
    let material: HistoryPreviewDocumentMaterial
    var readingOptions = HistoryPreviewDocumentReadingOptions()
    var searchQuery = ""
    var searchRevision = 0
    var searchDirection = 1
    var onSearchSummary: ((HistoryPreviewDocumentSearchSummary) -> Void)?
    var opener: HistoryPreviewDocumentLinkOpening = SystemHistoryPreviewDocumentLinkOpener()

    func makeCoordinator() -> Coordinator { Coordinator(opener: opener) }

    func makeNSView(context: Context) -> NSScrollView {
        let scrollView = NSScrollView()
        scrollView.hasVerticalScroller = true
        scrollView.autohidesScrollers = true
        scrollView.borderType = .noBorder
        scrollView.drawsBackground = true
        scrollView.backgroundColor = NSColor.textBackgroundColor

        let textView = HistoryPreviewDocumentTextFactory.makeTextView()
        textView.delegate = context.coordinator
        scrollView.documentView = textView

        context.coordinator.textView = textView
        configure(context.coordinator, textView: textView, scrollView: scrollView)
        return scrollView
    }

    func updateNSView(_ scrollView: NSScrollView, context: Context) {
        guard let textView = context.coordinator.textView else { return }
        configure(context.coordinator, textView: textView, scrollView: scrollView)
    }

    private func configure(_ coordinator: Coordinator, textView: NSTextView, scrollView: NSScrollView) {
        coordinator.onSearchSummary = onSearchSummary
        coordinator.install(material, options: readingOptions, in: textView, scrollView: scrollView)
        coordinator.updateSearch(searchQuery, revision: searchRevision, direction: searchDirection)
    }

    final class Coordinator: NSObject, NSTextViewDelegate {
        weak var textView: NSTextView?
        private var installed: HistoryPreviewDocumentMaterial?
        private var readingOptions: HistoryPreviewDocumentReadingOptions?
        private var query: String?
        private var searchRevision = 0
        private var matches: [NSRange] = []
        private var summary = HistoryPreviewDocumentSearchSummary()
        var onSearchSummary: ((HistoryPreviewDocumentSearchSummary) -> Void)?
        let linkDelegate: HistoryPreviewDocumentLinkDelegate

        init(opener: HistoryPreviewDocumentLinkOpening) {
            self.linkDelegate = HistoryPreviewDocumentLinkDelegate(opener: opener)
        }

        func textView(_ textView: NSTextView, clickedOnLink link: Any, at charIndex: Int) -> Bool {
            linkDelegate.textView(textView, clickedOnLink: link, at: charIndex)
        }

        func install(
            _ material: HistoryPreviewDocumentMaterial,
            options: HistoryPreviewDocumentReadingOptions = HistoryPreviewDocumentReadingOptions(),
            in textView: NSTextView,
            scrollView: NSScrollView
        ) {
            let changed = installed !== material
            guard changed || readingOptions != options else { return }
            let selection = textView.selectedRange()
            let anchor = textView.characterIndexForInsertion(at: NSPoint(
                x: textView.textContainerInset.width + 5,
                y: scrollView.contentView.bounds.minY + textView.textContainerInset.height
            ))
            installed = material
            readingOptions = options
            let viewport = scrollView.contentSize
            textView.minSize = NSSize(width: 0, height: viewport.height)
            textView.setFrameSize(NSSize(width: max(1, viewport.width), height: max(viewport.height, textView.frame.height)))
            (textView as? HistoryPreviewDocumentReadingTextView)?.usesComfortableWidth = options.comfortableWidth
            textView.textContainer?.containerSize = NSSize(
                width: max(1, viewport.width - textView.textContainerInset.width * 2),
                height: CGFloat.greatestFiniteMagnitude
            )
            textView.textStorage?.setAttributedString(HistoryPreviewDocumentReadingStyle.attributed(material.attributed, options: options))
            textView.setAccessibilityLabel(Loc.t("history.preview.documentTitle"))
            textView.setAccessibilityHelp(
                material.accessibilitySummary(
                    template: Loc.t("history.preview.documentAccessibilitySummary")
                )
            )
            if changed {
                query = nil
                textView.setSelectedRange(NSRange(location: 0, length: 0))
                textView.scroll(.zero)
            } else {
                textView.setSelectedRange(selection)
                textView.scrollRangeToVisible(NSRange(location: min(anchor, textView.string.utf16.count), length: 0))
                highlightMatches()
            }
        }

        func updateSearch(_ query: String, revision: Int, direction: Int) {
            guard let textView else { return }
            if self.query != query {
                self.query = query
                searchRevision = revision
                let result = HistoryPreviewDocumentSearch.matches(in: textView.string, query: query)
                matches = result.ranges
                summary = HistoryPreviewDocumentSearchSummary(current: matches.isEmpty ? 0 : 1, count: matches.count, truncated: result.truncated)
                highlightMatches()
                selectCurrentMatch()
                publishSummary()
            } else if searchRevision != revision {
                searchRevision = revision
                guard !matches.isEmpty else { return }
                summary.current = (summary.current - 1 + (direction > 0 ? 1 : -1) + matches.count) % matches.count + 1
                selectCurrentMatch()
                publishSummary()
            }
        }

        private func highlightMatches() {
            guard let textView, let layout = textView.layoutManager else { return }
            layout.removeTemporaryAttribute(.backgroundColor, forCharacterRange: NSRange(location: 0, length: textView.string.utf16.count))
            for range in matches {
                layout.addTemporaryAttribute(.backgroundColor, value: NSColor.findHighlightColor.withAlphaComponent(0.35), forCharacterRange: range)
            }
        }

        private func selectCurrentMatch() {
            guard let textView, matches.indices.contains(summary.current - 1) else { return }
            let range = matches[summary.current - 1]
            textView.setSelectedRange(range)
            textView.scrollRangeToVisible(range)
        }

        private func publishSummary() {
            let updated = summary
            DispatchQueue.main.async { [weak self] in
                guard let self, summary == updated else { return }
                onSearchSummary?(updated)
            }
        }
    }
}

protocol HistoryPreviewDocumentLinkOpening {
    func open(_ url: URL) -> Bool
}

struct SystemHistoryPreviewDocumentLinkOpener: HistoryPreviewDocumentLinkOpening {
    func open(_ url: URL) -> Bool {
        NSWorkspace.shared.open(url)
    }
}

final class HistoryPreviewDocumentLinkDelegate: NSObject, NSTextViewDelegate {
    private let opener: HistoryPreviewDocumentLinkOpening

    init(opener: HistoryPreviewDocumentLinkOpening = SystemHistoryPreviewDocumentLinkOpener()) {
        self.opener = opener
    }

    func textView(_ textView: NSTextView, clickedOnLink link: Any, at charIndex: Int) -> Bool {
        if let url = HistoryPreviewDocumentLink.safeURL(from: link) {
            _ = opener.open(url)
        }
        return true
    }
}

enum HistoryPreviewDocumentTextFactory {
    static func makeTextView() -> NSTextView {
        let textView = HistoryPreviewDocumentReadingTextView(frame: .zero)
        textView.isEditable = false
        textView.isSelectable = true
        textView.isRichText = true
        textView.importsGraphics = false
        textView.isAutomaticLinkDetectionEnabled = false
        textView.drawsBackground = true
        textView.backgroundColor = NSColor.textBackgroundColor
        textView.isVerticallyResizable = true
        textView.isHorizontallyResizable = false
        textView.autoresizingMask = [.width]
        textView.minSize = .zero
        textView.textContainerInset = NSSize(width: 20, height: 16)
        textView.textContainer?.widthTracksTextView = true
        textView.usesFindBar = true
        textView.isIncrementalSearchingEnabled = true
        textView.allowsUndo = false
        return textView
    }
}
