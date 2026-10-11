import AppKit
import SwiftUI

struct HistoryPreviewDocumentView: View {
    let material: HistoryPreviewDocumentMaterial

    @Environment(\.tailSyncPalette) private var palette

    var body: some View {
        VStack(spacing: 0) {
            noticeSection
            HistoryPreviewDocumentTextView(material: material)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
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
        context.coordinator.install(material, in: textView, scrollView: scrollView)
        return scrollView
    }

    func updateNSView(_ scrollView: NSScrollView, context: Context) {
        guard let textView = context.coordinator.textView else { return }
        context.coordinator.install(material, in: textView, scrollView: scrollView)
    }

    final class Coordinator: NSObject, NSTextViewDelegate {
        weak var textView: NSTextView?
        private var installed: HistoryPreviewDocumentMaterial?
        let linkDelegate: HistoryPreviewDocumentLinkDelegate

        init(opener: HistoryPreviewDocumentLinkOpening) {
            self.linkDelegate = HistoryPreviewDocumentLinkDelegate(opener: opener)
        }

        func textView(_ textView: NSTextView, clickedOnLink link: Any, at charIndex: Int) -> Bool {
            linkDelegate.textView(textView, clickedOnLink: link, at: charIndex)
        }

        func install(
            _ material: HistoryPreviewDocumentMaterial,
            in textView: NSTextView,
            scrollView: NSScrollView
        ) {
            guard installed !== material else { return }
            installed = material
            let viewport = scrollView.contentSize
            textView.minSize = NSSize(width: 0, height: viewport.height)
            textView.textContainer?.containerSize = NSSize(
                width: max(1, viewport.width),
                height: CGFloat.greatestFiniteMagnitude
            )
            textView.textStorage?.setAttributedString(material.attributed)
            textView.setAccessibilityLabel(Loc.t("history.preview.documentTitle"))
            textView.setAccessibilityHelp(
                material.accessibilitySummary(
                    template: Loc.t("history.preview.documentAccessibilitySummary")
                )
            )
            textView.scroll(.zero)
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
        let textView = NSTextView(frame: .zero)
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
