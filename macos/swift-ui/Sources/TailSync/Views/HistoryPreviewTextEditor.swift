import AppKit
import SwiftUI

struct HistoryPreviewTextEditor: NSViewRepresentable {
    @Environment(\.tailSyncPalette) private var palette

    let text: String
    let isCode: Bool
    let wrapsLines: Bool
    let fontSize: CGFloat
    let searchQuery: String
    let searchRevision: Int
    let searchDirection: Int

    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeNSView(context: Context) -> NSScrollView {
        let scrollView = NSScrollView()
        scrollView.hasVerticalScroller = true
        scrollView.autohidesScrollers = true
        scrollView.borderType = .noBorder
        scrollView.drawsBackground = true
        scrollView.backgroundColor = editorBackgroundColor
        scrollView.contentView.postsBoundsChangedNotifications = true

        let textView = NSTextView(frame: .zero)
        textView.isEditable = false
        textView.isSelectable = true
        textView.isRichText = true
        textView.importsGraphics = false
        textView.drawsBackground = true
        textView.backgroundColor = editorBackgroundColor
        textView.isVerticallyResizable = true
        textView.minSize = .zero
        scrollView.documentView = textView

        let ruler = HistoryPreviewLineNumberRulerView(textView: textView)
        scrollView.verticalRulerView = ruler
        context.coordinator.textView = textView
        context.coordinator.ruler = ruler
        context.coordinator.boundsObserver = NotificationCenter.default.addObserver(
            forName: NSView.boundsDidChangeNotification,
            object: scrollView.contentView,
            queue: .main
        ) { [weak ruler] _ in
            ruler?.needsDisplay = true
        }
        configure(scrollView, textView: textView, coordinator: context.coordinator)
        return scrollView
    }

    func updateNSView(_ scrollView: NSScrollView, context: Context) {
        guard let textView = context.coordinator.textView else { return }
        configure(scrollView, textView: textView, coordinator: context.coordinator)
    }

    static func dismantleNSView(_ nsView: NSScrollView, coordinator: Coordinator) {
        if let observer = coordinator.boundsObserver {
            NotificationCenter.default.removeObserver(observer)
        }
    }

    private func configure(
        _ scrollView: NSScrollView,
        textView: NSTextView,
        coordinator: Coordinator
    ) {
        scrollView.backgroundColor = editorBackgroundColor
        textView.backgroundColor = editorBackgroundColor
        coordinator.ruler?.backgroundColor = editorBackgroundColor
        let signature = Coordinator.Signature(
            text: text,
            isCode: isCode,
            wrapsLines: wrapsLines,
            fontSize: fontSize,
            query: searchQuery
        )
        if coordinator.signature != signature {
            coordinator.signature = signature
            configureLayout(scrollView, textView: textView)
            textView.textStorage?.setAttributedString(
                HistoryPreviewTextStyler.attributedString(
                    text,
                    isCode: isCode,
                    fontSize: fontSize,
                    query: searchQuery
                )
            )
            if coordinator.indexedText != text {
                coordinator.indexedText = text
                coordinator.ruler?.lineIndex = HistoryPreviewLogicalLineIndex(text: text)
            }
            coordinator.ruler?.needsDisplay = true
        }
        if coordinator.searchRevision != searchRevision {
            coordinator.searchRevision = searchRevision
            coordinator.find(searchQuery, direction: searchDirection)
        }
    }

    private var editorBackgroundColor: NSColor {
        NSColor(palette.surfaceColor)
    }

    private func configureLayout(_ scrollView: NSScrollView, textView: NSTextView) {
        let viewportSize = scrollView.contentSize
        scrollView.hasHorizontalScroller = !wrapsLines
        scrollView.hasVerticalRuler = isCode
        scrollView.rulersVisible = isCode
        textView.textContainerInset = NSSize(width: isCode ? 12 : 20, height: 16)
        textView.isHorizontallyResizable = !wrapsLines
        textView.autoresizingMask = wrapsLines ? [.width] : []
        textView.textContainer?.widthTracksTextView = wrapsLines
        if wrapsLines {
            // A text view that was previously laid out horizontally keeps its
            // expanded document width. Reset it to the viewport before asking
            // TextKit to track the view again, otherwise enabling wrapping has
            // no visible effect.
            textView.setFrameSize(NSSize(
                width: max(1, viewportSize.width),
                height: max(viewportSize.height, textView.frame.height)
            ))
        }
        textView.minSize = NSSize(width: 0, height: viewportSize.height)
        textView.textContainer?.containerSize = NSSize(
            width: wrapsLines
                ? max(1, viewportSize.width)
                : CGFloat.greatestFiniteMagnitude,
            height: CGFloat.greatestFiniteMagnitude
        )
        textView.maxSize = NSSize(
            width: CGFloat.greatestFiniteMagnitude,
            height: CGFloat.greatestFiniteMagnitude
        )
    }

    final class Coordinator {
        struct Signature: Equatable {
            let text: String
            let isCode: Bool
            let wrapsLines: Bool
            let fontSize: CGFloat
            let query: String
        }

        weak var textView: NSTextView?
        weak var ruler: HistoryPreviewLineNumberRulerView?
        var signature: Signature?
        var indexedText: String?
        var searchRevision = 0
        var boundsObserver: NSObjectProtocol?

        func find(_ query: String, direction: Int) {
            guard !query.isEmpty, let textView else { return }
            let source = textView.string as NSString
            let current = textView.selectedRange()
            let options: NSString.CompareOptions = direction > 0
                ? [.caseInsensitive]
                : [.caseInsensitive, .backwards]
            let initialLocation = direction > 0 ? NSMaxRange(current) : 0
            let initialLength = direction > 0
                ? max(0, source.length - initialLocation)
                : max(0, current.location)
            var match = source.range(
                of: query,
                options: options,
                range: NSRange(location: initialLocation, length: initialLength)
            )
            if match.location == NSNotFound {
                match = source.range(
                    of: query,
                    options: options,
                    range: NSRange(location: 0, length: source.length)
                )
            }
            guard match.location != NSNotFound else { return }
            textView.setSelectedRange(match)
            textView.scrollRangeToVisible(match)
        }
    }
}
