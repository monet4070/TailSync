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

struct HistoryRowFocusStyle {
    let color: Color
    let cornerRadius: CGFloat
    let lineWidth: CGFloat

    init(selection: TailSyncThemeSelection, scheme: ColorScheme) {
        let focus = selection.component("history", state: "focus", scheme: scheme)
        color = focus?.focusRingColor ?? selection.palette(for: scheme).accentColor
        cornerRadius = focus?.radius ?? selection.metrics(for: scheme).controlRadius
        lineWidth = selection.builtin == .highContrast ? 3 : 2
    }
}

struct HistoryRow: View {
    let entry: HistoryEntry
    let owner: String
    let isRestored: Bool
    let isFocused: Bool
    let showsMultipleLabels: Bool
    let favoriteProgress: CGFloat
    let showsTransientFavoriteStamp: Bool
    let trailingContentWidth: CGFloat
    let shortcutNumber: Int?
    let shortcutEnabled: Bool
    @State private var thumbnail: NSImage? = nil
    @State private var installedThumbnailKey: HistoryThumbnailKey? = nil
    @State private var hovering = false
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.tailSyncSelection) private var selection
    @Environment(\.tailSyncPalette) private var palette

    init(
        entry: HistoryEntry,
        owner: String = "all",
        isRestored: Bool,
        isFocused: Bool,
        showsMultipleLabels: Bool,
        favoriteProgress: CGFloat = 0,
        showsTransientFavoriteStamp: Bool = false,
        trailingContentWidth: CGFloat = 0,
        shortcutNumber: Int? = nil,
        shortcutEnabled: Bool = false
    ) {
        self.entry = entry
        self.owner = owner
        self.isRestored = isRestored
        self.isFocused = isFocused
        self.showsMultipleLabels = showsMultipleLabels
        self.favoriteProgress = favoriteProgress
        self.showsTransientFavoriteStamp = showsTransientFavoriteStamp
        self.trailingContentWidth = trailingContentWidth
        self.shortcutNumber = shortcutNumber
        self.shortcutEnabled = shortcutEnabled
    }

    private var component: TailSyncThemeComponentTokens? {
        let state = isRestored ? "selected" : (isFocused ? "focus" : (hovering ? "hover" : "default"))
        return selection.component("history", state: state, scheme: colorScheme)
    }

    private var focusStyle: HistoryRowFocusStyle {
        HistoryRowFocusStyle(selection: selection, scheme: colorScheme)
    }

    private var rowCornerRadius: CGFloat {
        isFocused ? focusStyle.cornerRadius : (component?.radius ?? selection.metrics.controlRadius)
    }

    private var visibleCategoryLabels: [String] {
        showsMultipleLabels ? entry.categoryLabels : [entry.categoryLabel]
    }

    private var thumbnailKey: HistoryThumbnailKey {
        HistoryThumbnailKey(entry: entry, owner: owner)
    }

    var body: some View {
        HStack(spacing: 10) {
            Group {
                if entry.hasThumbnail, installedThumbnailKey == thumbnailKey, let thumb = thumbnail {
                    thumbnailImage(thumb)
                } else {
                    Image(systemName: entry.icon).frame(width: 24, height: 24)
                        .foregroundColor(component?.iconColor ?? component?.accentColor ?? palette.accentColor)
                        .background((component?.accentColor ?? palette.accentColor).opacity(0.1))
                        .clipShape(RoundedRectangle(cornerRadius: component?.radius ?? selection.metrics.controlRadius, style: .continuous))
                }
            }
            .padding(.top, 8)
            .frame(width: HistoryThumbnailLayout.columnWidth)
            .task(id: thumbnailKey) {
                thumbnail = nil
                installedThumbnailKey = nil
                guard entry.hasThumbnail else {
                    thumbnail = nil
                    return
                }
                if let cached = HistoryThumbnailCache.image(for: thumbnailKey) {
                    guard !Task.isCancelled else { return }
                    thumbnail = cached
                    installedThumbnailKey = thumbnailKey
                } else if let img = await ApiClient.shared.getImageData(id: entry.id, cacheKey: entry.display?.cache_key),
                          let image = rgbaToImage(img) {
                    guard !Task.isCancelled else { return }
                    HistoryThumbnailCache.insert(image, for: thumbnailKey, encodedBytes: img.rgba.count)
                    thumbnail = image
                    installedThumbnailKey = thumbnailKey
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
                    Spacer(minLength: 8)
                    Text(entry.sourceDisplayName)
                        .font(.caption2)
                        .foregroundColor(component?.secondaryTextColor ?? palette.tertiaryColor)
                        .lineLimit(1)
                        .truncationMode(.middle)
                        .frame(maxWidth: 90, alignment: .trailing)
                        .layoutPriority(1)
                    if entry.pinned || showsTransientFavoriteStamp {
                        Image(systemName: "star.fill")
                            .font(.system(size: 12, weight: .semibold))
                            .foregroundColor(palette.accentColor)
                            .transition(.scale.combined(with: .opacity))
                    }
                }
                .padding(.trailing, 6)
                Text(entry.listDescription)
                    .font(selection.builtin == .tailsync
                          ? selection.displayFont(size: selection.typography.historyContentSize)
                          : selection.builtin == .forest
                              ? selection.displayFont(size: selection.typography.historyContentSize, weight: .medium)
                              : selection.readingFont(size: selection.typography.historyContentSize, weight: .medium))
                    .foregroundColor(component?.foregroundColor ?? palette.primaryColor)
                    .lineLimit(1)
                HStack(alignment: .lastTextBaseline, spacing: 6) {
                    if let status = HistoryRowStatusLabel.text(for: entry.composite) {
                        Text(status)
                            .font(.caption2).foregroundColor(palette.secondaryColor)
                    }
                    Text(entry.formattedSize).font(.caption2).foregroundColor(component?.secondaryTextColor ?? palette.tertiaryColor).monospacedDigit()
                    Spacer()
                }
                .padding(.trailing, entry.composite == nil ? 0 : trailingContentWidth + 8)
            }
        }
        .padding(.vertical, component?.padding ?? max(2, (selection.metrics.rowPadding - 6) / 2))
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(
            component?.backgroundColor ?? (isRestored
                ? palette.activeColor
                : isFocused ? palette.accentSoftColor : .clear)
        )
        .clipShape(RoundedRectangle(cornerRadius: rowCornerRadius, style: .continuous))
        .overlay {
            RoundedRectangle(cornerRadius: rowCornerRadius, style: .continuous)
                .strokeBorder(
                    isFocused ? focusStyle.color : (component?.borderColor ?? .clear),
                    lineWidth: isFocused ? focusStyle.lineWidth : 0
                )
                .allowsHitTesting(false)
        }
        .overlay {
            RoundedRectangle(cornerRadius: component?.radius ?? selection.metrics.controlRadius, style: .continuous)
                .fill(palette.accentColor.opacity(0.12))
                .scaleEffect(x: max(0.001, min(favoriteProgress, 1)), y: 1, anchor: .leading)
                .opacity(favoriteProgress > 0 ? 1 : 0)
                .allowsHitTesting(false)
        }
        .overlay(alignment: .topLeading) {
            if let shortcutNumber {
                Text(String(shortcutNumber))
                    .font(.system(size: 10, weight: .semibold, design: .rounded).monospacedDigit())
                    .foregroundColor(shortcutEnabled ? palette.accentContrastColor : palette.secondaryColor)
                    .frame(width: 14, height: 14)
                    .background(shortcutEnabled ? palette.accentColor : palette.surfaceColor,
                                in: RoundedRectangle(cornerRadius: 4, style: .continuous))
                    .overlay(RoundedRectangle(cornerRadius: 4, style: .continuous)
                        .strokeBorder(palette.surfaceColor, lineWidth: 1))
                    .offset(x: -7, y: -7)
                    .allowsHitTesting(false)
                    .accessibilityLabel(Loc.t("history.keyboard.number").replacingOccurrences(of: "{number}", with: String(shortcutNumber)))
            }
        }
        // Reserve the outside half of the corner badge, including for the first row.
        .padding(.top, 8)
        .padding(.leading, 8)
        .accessibilityAddTraits(isFocused ? .isSelected : [])
        .contentShape(Rectangle())
        .onHover { hovering = $0 }
        .animation(Loc.shared.reduceMotion ? nil : .easeOut(duration: 0.4), value: isRestored)
        // The focus ring must switch instantly, never cross-fade: during fast
        // navigation an animated ring leaves the previous row's box fading out
        // while the next one fades in, so two selection boxes are visible at
        // once. Exactly one row may ever show the ring.
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
    var owner: String = "all"
    let isRestored: Bool
    let isFocused: Bool
    let showsMultipleLabels: Bool
    var shortcutNumber: Int? = nil
    var shortcutEnabled = false
    let previewRequest: HistoryPreviewRequest?
    let onSelect: () -> Void
    let onRestore: () -> Void
    var onRestoreFormat: ((String) -> Void)? = nil
    var onArchiveImages: (() -> Void)? = nil
    var onAuthorizeSource: (() -> Void)? = nil
    var onImportSourcePacket: (() -> Void)? = nil
    var onRevokeSource: (() -> Void)? = nil
    let onDelete: () -> Void
    let onFavorite: () -> Void
    let onPreview: (HistoryPreviewRequest) -> Void
    let onClosePreview: () -> Void
    let isPreviewVisible: () -> Bool

    @Environment(\.tailSyncSelection) private var selection
    @Environment(\.tailSyncPalette) private var palette
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var favoriteProgress: CGFloat = 0
    @State private var favoritePressActive = false
    @State private var showsTransientFavoriteStamp = false
    @State private var trailingContentWidth: CGFloat = 0

    var body: some View {
        HistoryRow(
            entry: entry,
            owner: owner,
            isRestored: isRestored,
            isFocused: isFocused,
            showsMultipleLabels: showsMultipleLabels,
            favoriteProgress: HistoryFavoriteFillPolicy.progress(
                isFavorite: entry.pinned,
                isPressActive: favoritePressActive,
                pressProgress: favoriteProgress
            ),
            showsTransientFavoriteStamp: showsTransientFavoriteStamp,
            trailingContentWidth: trailingContentWidth,
            shortcutNumber: shortcutNumber,
            shortcutEnabled: shortcutEnabled
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
        .overlay(alignment: .bottomTrailing) {
            if let info = entry.composite {
                HistoryRowTrailingActions(
                    info: info,
                    canRestoreFormat: onRestoreFormat != nil,
                    onRestoreFormat: onRestoreFormat,
                    onArchiveImages: onArchiveImages,
                    onAuthorizeSource: onAuthorizeSource,
                    onImportSourcePacket: onImportSourcePacket,
                    onRevokeSource: onRevokeSource
                )
                .fixedSize(horizontal: true, vertical: false)
                .background {
                    GeometryReader { geometry in
                        Color.clear.preference(key: HistoryRowTrailingWidthKey.self, value: geometry.size.width)
                    }
                }
            }
        }
        .onPreferenceChange(HistoryRowTrailingWidthKey.self) { trailingContentWidth = $0 }
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

private struct HistoryRowTrailingWidthKey: PreferenceKey {
    static var defaultValue: CGFloat = 0
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) { value = max(value, nextValue()) }
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

struct HistoryThumbnailKey: Hashable {
    let authorityKey: String
    let owner: String
    let id: Int64
    let dataHash: String
    let captureId: String
    let contentId: String
    let archiveRevision: Int64
    let archiveDigest: String
    let classifierVersion: Int64
    let spec: Int

    init(entry: HistoryEntry, owner: String = "all") {
        authorityKey = entry.display?.cache_key ?? ""
        self.owner = owner
        let identity = entry.composite?.identity
        id = entry.id
        dataHash = entry.data_hash
        captureId = identity?.capture_id ?? ""
        contentId = identity?.content_id ?? ""
        archiveRevision = identity?.archive_revision ?? 0
        archiveDigest = identity?.archive_digest ?? ""
        classifierVersion = entry.classifier_version
        spec = Int(HistoryThumbnailLayout.maxSide)
    }

    var storageKey: NSString {
        "\(authorityKey)|\(owner)|\(id)|\(dataHash)|\(captureId)|\(contentId)|\(archiveRevision)|\(archiveDigest)|\(classifierVersion)|\(spec)" as NSString
    }
}

enum HistoryThumbnailCache {
    private struct Cached {
        let image: NSImage
        let cost: Int
    }
    private static var cache: [HistoryThumbnailKey: Cached] = [:]
    private static var order: [HistoryThumbnailKey] = []
    private(set) static var memoryCost = 0
    static let byteLimit = 4 * 1024 * 1024

    static func image(for key: HistoryThumbnailKey) -> NSImage? {
        guard let cached = cache[key] else { return nil }
        order.removeAll { $0 == key }
        order.append(key)
        return cached.image
    }

    static func insert(_ image: NSImage, for key: HistoryThumbnailKey, encodedBytes: Int = 0) {
        guard !key.authorityKey.isEmpty, encodedBytes >= 0, !image.representations.isEmpty else { return }
        var cost = encodedBytes
        for representation in image.representations {
            guard let bitmap = representation as? NSBitmapImageRep, bitmap.pixelsWide > 0, bitmap.pixelsHigh > 0 else { return }
            let (bytes, overflow) = bitmap.bytesPerRow.multipliedReportingOverflow(by: bitmap.pixelsHigh)
            let (nextCost, costOverflow) = cost.addingReportingOverflow(bytes)
            guard !overflow, !costOverflow, bytes > 0, nextCost <= byteLimit else { return }
            cost = nextCost
        }
        if let old = cache.removeValue(forKey: key) { memoryCost -= old.cost }
        order.removeAll { $0 == key }
        while (memoryCost + cost > byteLimit || cache.count >= 30), let oldest = order.first {
            order.removeFirst()
            if let removed = cache.removeValue(forKey: oldest) { memoryCost -= removed.cost }
        }
        guard memoryCost + cost <= byteLimit else { return }
        cache[key] = Cached(image: image, cost: cost)
        order.append(key)
        memoryCost += cost
    }

    static func removeAll() {
        cache.removeAll()
        order.removeAll()
        memoryCost = 0
    }
}
