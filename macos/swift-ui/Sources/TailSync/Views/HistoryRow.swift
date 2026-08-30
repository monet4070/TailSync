import AppKit
import Foundation
import SwiftUI

enum HistoryFavoriteFillPolicy {
    static func progress(
        isFavorite: Bool,
        isPressActive: Bool,
        pressProgress: CGFloat
    ) -> CGFloat {
        if isFavorite { return 1 }
        guard isPressActive else { return 0 }
        return min(max(pressProgress, 0), 1)
    }
}

struct HistoryRow: View {
    let entry: HistoryEntry
    let isRestored: Bool
    let isFocused: Bool
    let showsMultipleLabels: Bool
    let favoriteProgress: CGFloat
    let showsTransientFavoriteStamp: Bool
    @State private var thumbnail: NSImage? = nil
    @State private var hovering = false
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.tailSyncSelection) private var selection
    @Environment(\.tailSyncPalette) private var palette

    init(
        entry: HistoryEntry,
        isRestored: Bool,
        isFocused: Bool,
        showsMultipleLabels: Bool,
        favoriteProgress: CGFloat = 0,
        showsTransientFavoriteStamp: Bool = false
    ) {
        self.entry = entry
        self.isRestored = isRestored
        self.isFocused = isFocused
        self.showsMultipleLabels = showsMultipleLabels
        self.favoriteProgress = favoriteProgress
        self.showsTransientFavoriteStamp = showsTransientFavoriteStamp
    }

    private var component: TailSyncThemeComponentTokens? {
        let state = isRestored ? "selected" : (isFocused ? "focus" : (hovering ? "hover" : "default"))
        return selection.component("history", state: state, scheme: colorScheme)
    }

    private var visibleCategoryLabels: [String] {
        showsMultipleLabels ? entry.categoryLabels : [entry.categoryLabel]
    }

    var body: some View {
        HStack(spacing: 10) {
            Group {
                if entry.type == "image", let thumb = thumbnail {
                    thumbnailImage(thumb)
                } else {
                    Image(systemName: entry.icon).frame(width: 24, height: 24)
                        .foregroundColor(component?.iconColor ?? component?.accentColor ?? palette.accentColor)
                        .background((component?.accentColor ?? palette.accentColor).opacity(0.1))
                        .clipShape(RoundedRectangle(cornerRadius: component?.radius ?? selection.metrics.controlRadius, style: .continuous))
                }
            }
            .frame(width: HistoryThumbnailLayout.columnWidth)
            .task {
                guard entry.type == "image" else { return }
                if let cached = HistoryThumbnailCache.image(for: entry.id) {
                    thumbnail = cached
                } else if let img = await ApiClient.shared.getImageData(id: entry.id),
                          let image = rgbaToImage(img) {
                    HistoryThumbnailCache.insert(image, for: entry.id)
                    thumbnail = image
                }
            }

            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(visibleCategoryLabels.map { $0.uppercased() }.joined(separator: "  \u{00B7}  "))
                        .font(selection.readingFont(size: 10, weight: .semibold))
                        .foregroundColor(component?.accentColor ?? palette.accentColor).padding(.horizontal, 4).padding(.vertical, 1)
                        .background((component?.accentColor ?? palette.accentColor).opacity(0.12))
                        .clipShape(RoundedRectangle(cornerRadius: component?.radius ?? selection.metrics.controlRadius, style: .continuous))
                        .fixedSize(horizontal: false, vertical: true)
                        .layoutPriority(1)
                    Text(entry.formattedTime).font(.caption).foregroundColor(palette.tertiaryColor)
                }
                Text(entry.description)
                    .font(selection.builtin == .tailsync
                          ? selection.displayFont(size: selection.typography.historyContentSize)
                          : selection.builtin == .forest
                              ? selection.displayFont(size: selection.typography.historyContentSize, weight: .medium)
                              : selection.readingFont(size: selection.typography.historyContentSize, weight: .medium))
                    .foregroundColor(component?.foregroundColor ?? palette.primaryColor)
                    .lineLimit(1)
                HStack(spacing: 6) {
                    Text(entry.formattedSize).font(.caption2).foregroundColor(component?.secondaryTextColor ?? palette.tertiaryColor).monospacedDigit()
                    Spacer()
                    Text(entry.source_peer).font(.caption2).foregroundColor(component?.secondaryTextColor ?? palette.tertiaryColor).lineLimit(1)
                }
            }
        }
        .padding(.vertical, component?.padding ?? max(2, (selection.metrics.rowPadding - 6) / 2))
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(
            component?.backgroundColor ?? (isRestored
                ? palette.activeColor
                : isFocused ? palette.hoverColor : .clear)
        )
        .clipShape(RoundedRectangle(cornerRadius: component?.radius ?? selection.metrics.controlRadius, style: .continuous))
        .overlay {
            RoundedRectangle(cornerRadius: component?.radius ?? selection.metrics.controlRadius, style: .continuous)
                .stroke(
                    isFocused ? (component?.focusRingColor ?? palette.accentColor).opacity(0.7) : (component?.borderColor ?? .clear),
                    lineWidth: isFocused ? 1 : 0
                )
        }
        .overlay {
            RoundedRectangle(cornerRadius: component?.radius ?? selection.metrics.controlRadius, style: .continuous)
                .fill(palette.accentColor.opacity(0.12))
                .scaleEffect(x: max(0.001, min(favoriteProgress, 1)), y: 1, anchor: .leading)
                .opacity(favoriteProgress > 0 ? 1 : 0)
                .allowsHitTesting(false)
        }
        .overlay(alignment: .topTrailing) {
            if entry.pinned || showsTransientFavoriteStamp {
                Image(systemName: "star.fill")
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundColor(palette.accentColor)
                    .padding(4)
                    .transition(.scale.combined(with: .opacity))
            }
        }
        .accessibilityAddTraits(isFocused ? .isSelected : [])
        .contentShape(Rectangle())
        .onHover { hovering = $0 }
        .animation(Loc.shared.reduceMotion ? nil : .easeOut(duration: 0.4), value: isRestored)
        .animation(Loc.shared.reduceMotion ? nil : .easeOut(duration: 0.18), value: isFocused)
        .animation(Loc.shared.reduceMotion ? nil : .easeOut(duration: 0.12), value: hovering)
        .animation(Loc.shared.reduceMotion ? nil : .linear(duration: 0.42), value: favoriteProgress)
        .animation(Loc.shared.reduceMotion ? nil : .spring(response: 0.28, dampingFraction: 0.68), value: showsTransientFavoriteStamp)
    }

    /// Render an image thumbnail at its natural aspect ratio inside the fixed
    /// column: `.fit` shows the whole image (a long screenshot is never cropped
    /// to an unrecognizable middle band), `.high` interpolation keeps the
    /// downscaled pixels crisp, and the frame comes from the ratio-clamped
    /// layout so extreme strips cannot distort the row.
    private func thumbnailImage(_ thumb: NSImage) -> some View {
        let size = HistoryThumbnailLayout.displaySize(
            pixelWidth: thumb.size.width,
            pixelHeight: thumb.size.height
        )
        return Image(nsImage: thumb)
            .resizable()
            .interpolation(.high)
            .aspectRatio(contentMode: .fit)
            .frame(width: size.width, height: size.height)
            .clipShape(RoundedRectangle(cornerRadius: selection.metrics.controlRadius, style: .continuous))
    }
}

/// Shared row surface for both the history and favorites windows. The
/// AppKit responder owns the gesture boundary; this view owns only the
/// declarative progress fill and completion stamp.
struct HistoryInteractiveRow: View {
    let entry: HistoryEntry
    let isRestored: Bool
    let isFocused: Bool
    let showsMultipleLabels: Bool
    let previewRequest: HistoryPreviewRequest?
    let onSelect: () -> Void
    let onRestore: () -> Void
    let onDelete: () -> Void
    let onFavorite: () -> Void
    let onPreview: (HistoryPreviewRequest) -> Void
    let onClosePreview: () -> Void
    let isPreviewVisible: () -> Bool

    @Environment(\.tailSyncSelection) private var selection
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var favoriteProgress: CGFloat = 0
    @State private var favoritePressActive = false
    @State private var showsTransientFavoriteStamp = false

    var body: some View {
        HistoryRow(
            entry: entry,
            isRestored: isRestored,
            isFocused: isFocused,
            showsMultipleLabels: showsMultipleLabels,
            favoriteProgress: HistoryFavoriteFillPolicy.progress(
                isFavorite: entry.pinned,
                isPressActive: favoritePressActive,
                pressProgress: favoriteProgress
            ),
            showsTransientFavoriteStamp: showsTransientFavoriteStamp
        )
        .contentShape(Rectangle())
        .overlay {
            HistoryRowInteraction(
                previewRequest: previewRequest,
                onSelect: onSelect,
                onRestore: onRestore,
                onDelete: onDelete,
                onPreview: onPreview,
                onClosePreview: onClosePreview,
                isPreviewVisible: isPreviewVisible,
                onFavorite: commitFavorite,
                onFavoritePressStarted: {
                    favoritePressActive = true
                    withAnimation(reduceMotion ? nil : .linear(duration: 0.42)) {
                        favoriteProgress = 1
                    }
                },
                onFavoritePressCancelled: {
                    withAnimation(reduceMotion ? nil : .easeOut(duration: 0.12)) {
                        favoriteProgress = 0
                    }
                    favoritePressActive = false
                }
            )
        }
        .accessibilityAction(
            named: Text(Loc.t(entry.pinned ? "history.unpin" : "history.pin"))
        ) {
            commitFavorite()
        }
    }

    private func commitFavorite() {
        favoritePressActive = false
        favoriteProgress = 0
        showsTransientFavoriteStamp = true
        onFavorite()
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.55) {
            showsTransientFavoriteStamp = false
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────

private func rgbaToImage(_ data: ApiClient.ImageData) -> NSImage? {
    let w = data.width, h = data.height
    let (pixelCount, pixelOverflow) = w.multipliedReportingOverflow(by: h)
    let (byteCount, byteOverflow) = pixelCount.multipliedReportingOverflow(by: 4)
    guard w > 0, h > 0, !pixelOverflow, !byteOverflow, data.rgba.count == byteCount else {
        return nil
    }
    guard let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: w, pixelsHigh: h,
                                      bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                                      colorSpaceName: .deviceRGB, bytesPerRow: w * 4, bitsPerPixel: 32) else { return nil }
    data.rgba.copyBytes(to: rep.bitmapData!, count: byteCount)
    let img = NSImage(size: NSSize(width: w, height: h))
    img.addRepresentation(rep)
    return img
}

/// Layout math for history-row image thumbnails.
///
/// Thumbnails render at their true aspect ratio inside a fixed-width column so
/// the text beside them stays left-aligned across rows. The ratio is clamped to
/// `maxAspect`:1 in both directions: without the clamp an extreme long-strip
/// (a full-page screenshot, a wide banner) would either blow the row height up
/// or shrink to an unreadable sliver.
enum HistoryThumbnailLayout {
    /// Longest displayed edge, in points.
    static let maxSide: CGFloat = 48
    /// Steepest displayed width:height (or height:width) ratio.
    static let maxAspect: CGFloat = 2.5
    /// Fixed column width so rows align regardless of thumbnail shape.
    static let columnWidth: CGFloat = maxSide

    /// Display size for a thumbnail of the given pixel dimensions.
    static func displaySize(pixelWidth: CGFloat, pixelHeight: CGFloat) -> CGSize {
        let w = max(pixelWidth, 1)
        let h = max(pixelHeight, 1)
        let clamped = min(max(w / h, 1 / maxAspect), maxAspect)
        if clamped >= 1 {
            // Wider than tall (or square): width drives the longest edge.
            return CGSize(width: maxSide, height: maxSide / clamped)
        } else {
            // Taller than wide: height drives the longest edge.
            return CGSize(width: maxSide * clamped, height: maxSide)
        }
    }
}

enum HistoryThumbnailCache {
    private static let cache: NSCache<NSNumber, NSImage> = {
        let cache = NSCache<NSNumber, NSImage>()
        cache.countLimit = 30
        // 160px thumbnails cost ~100 KB each; 8 MB leaves headroom over the
        // 30-item count limit so the cache is bounded by count, not evictions.
        cache.totalCostLimit = 8 * 1024 * 1024
        return cache
    }()

    static func image(for id: Int64) -> NSImage? {
        cache.object(forKey: NSNumber(value: id))
    }

    static func insert(_ image: NSImage, for id: Int64) {
        let pixels = max(1, Int(image.size.width * image.size.height))
        cache.setObject(image, forKey: NSNumber(value: id), cost: pixels * 4)
    }

    static func removeAll() {
        cache.removeAllObjects()
    }
}
