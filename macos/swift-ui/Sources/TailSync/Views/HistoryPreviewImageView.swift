import AppKit
import SwiftUI

enum HistoryImageViewport {
    struct Layout: Equatable {
        let fittedScale: CGFloat
        let imageSize: CGSize
        let center: CGPoint
    }

    static func fittedScale(
        imageSize: CGSize,
        containerSize: CGSize,
        rotation: Angle
    ) -> CGFloat {
        guard imageSize.width > 0,
              imageSize.height > 0,
              containerSize.width > 0,
              containerSize.height > 0 else { return 1 }
        let normalizedTurns = Int((rotation.degrees / 90).rounded()).magnitude % 2
        let displayedSize = normalizedTurns == 1
            ? CGSize(width: imageSize.height, height: imageSize.width)
            : imageSize
        return min(
            containerSize.width / displayedSize.width,
            containerSize.height / displayedSize.height,
            1
        )
    }

    static func adding(_ translation: CGSize, to offset: CGSize) -> CGSize {
        CGSize(
            width: offset.width + translation.width,
            height: offset.height + translation.height
        )
    }

    static func layout(
        imageSize: CGSize,
        containerSize: CGSize,
        rotation: Angle
    ) -> Layout {
        let scale = fittedScale(
            imageSize: imageSize,
            containerSize: containerSize,
            rotation: rotation
        )
        return Layout(
            fittedScale: scale,
            imageSize: CGSize(
                width: max(1, imageSize.width * scale),
                height: max(1, imageSize.height * scale)
            ),
            center: CGPoint(x: containerSize.width / 2, y: containerSize.height / 2)
        )
    }
}

struct HistoryImagePreviewView: View {
    let material: HistoryPreviewImageMaterial

    @Environment(\.tailSyncPalette) private var palette

    @State private var zoom: CGFloat = 1
    @State private var rotation = Angle.zero
    @State private var offset = CGSize.zero
    @State private var showsTransparency = true
    @GestureState private var dragTranslation = CGSize.zero

    private var image: NSImage { material.image }

    init(
        material: HistoryPreviewImageMaterial,
        showsTransparencyInitially: Bool = true
    ) {
        self.material = material
        _showsTransparency = State(initialValue: showsTransparencyInitially)
    }

    var body: some View {
        VStack(spacing: 0) {
            imageToolbar
            GeometryReader { proxy in
                let layout = HistoryImageViewport.layout(
                    imageSize: image.size,
                    containerSize: proxy.size,
                    rotation: rotation
                )
                let translation = HistoryImageViewport.adding(dragTranslation, to: offset)
                ZStack(alignment: .topLeading) {
                    if showsTransparency {
                        HistoryPreviewCheckerboard()
                            .allowsHitTesting(false)
                    }
                    palette.surfaceColor
                        .opacity(showsTransparency ? 0 : 1)
                        .allowsHitTesting(false)
                    Image(nsImage: image)
                        .resizable()
                        .interpolation(.high)
                        .frame(
                            width: layout.imageSize.width * zoom,
                            height: layout.imageSize.height * zoom
                        )
                        .rotationEffect(rotation)
                        .position(
                            x: layout.center.x + translation.width,
                            y: layout.center.y + translation.height
                        )
                        .allowsHitTesting(false)
                }
                .frame(width: proxy.size.width, height: proxy.size.height, alignment: .center)
                .clipped()
                .contentShape(Rectangle())
                .gesture(dragGesture)
                .background(HistoryPreviewModifierScrollMonitor { delta in
                    setZoom(zoom * (delta > 0 ? 1.1 : 1 / 1.1))
                })
            }
            imageMetadata(image)
        }
        .background(palette.surfaceColor)
    }

    private var dragGesture: some Gesture {
        DragGesture()
            .updating($dragTranslation) { value, translation, _ in
                translation = value.translation
            }
            .onEnded { value in
                offset = HistoryImageViewport.adding(value.translation, to: offset)
            }
    }

    private var imageToolbar: some View {
        HStack(spacing: 8) {
            HistoryPreviewToolbarIconButton(
                systemName: "arrow.up.left.and.arrow.down.right",
                action: fitCurrentImage
            )
            .help(Loc.t("history.preview.fit"))
            Divider().frame(height: 18)
            HistoryPreviewToolbarIconButton(
                systemName: "minus.magnifyingglass",
                action: { setZoom(zoom / 1.2) }
            )
            Text("\(Int((zoom * 100).rounded()))%")
                .font(.caption.monospacedDigit())
                .frame(width: 52)
            HistoryPreviewToolbarIconButton(
                systemName: "plus.magnifyingglass",
                action: { setZoom(zoom * 1.2) }
            )
            Spacer()
            HistoryPreviewToolbarIconButton(
                systemName: "rotate.right",
                action: rotateClockwise
            )
            .help(Loc.t("history.preview.rotate"))
            HistoryPreviewToolbarIconButton(
                systemName: showsTransparency
                    ? "checkerboard.rectangle"
                    : "rectangle.fill",
                selected: showsTransparency,
                action: { showsTransparency.toggle() }
            )
            .help(Loc.t("history.preview.transparency"))
        }
        .historyPreviewToolbarStyle(height: HistoryPreviewLayoutMetrics.imageToolbarHeight)
    }

    private func imageMetadata(_ image: NSImage) -> some View {
        let representation = image.representations.max { left, right in
            left.pixelsWide * left.pixelsHigh < right.pixelsWide * right.pixelsHigh
        }
        let width = representation?.pixelsWide ?? Int(image.size.width)
        let height = representation?.pixelsHigh ?? Int(image.size.height)
        return HStack {
            Text("\(width) × \(height)")
            Spacer()
            Text(ByteCountFormatter.string(
                fromByteCount: Int64(material.data.count),
                countStyle: .file
            ))
        }
        .font(.caption2)
        .foregroundColor(palette.tertiaryColor)
        .padding(.horizontal, 16)
        .frame(height: 30)
        .background(palette.raisedColor)
        .overlay(alignment: .top) {
            Rectangle().fill(palette.dividerColor).frame(height: 1)
        }
    }

    private func fitCurrentImage() {
        zoom = 1
        offset = .zero
    }

    private func rotateClockwise() {
        let nextRotation = rotation + .degrees(90)
        rotation = nextRotation
        offset = .zero
    }

    private func setZoom(_ newZoom: CGFloat) {
        zoom = min(max(newZoom, 0.1), 8)
    }
}

private struct HistoryPreviewCheckerboard: View {
    private let tile: CGFloat = 14

    var body: some View {
        Canvas { context, size in
            context.fill(
                Path(CGRect(origin: .zero, size: size)),
                with: .color(Color(nsColor: .white))
            )
            let columns = Int(ceil(size.width / tile))
            let rows = Int(ceil(size.height / tile))
            for row in 0..<rows {
                for column in 0..<columns where (row + column).isMultiple(of: 2) {
                    context.fill(
                        Path(CGRect(
                            x: CGFloat(column) * tile,
                            y: CGFloat(row) * tile,
                            width: tile,
                            height: tile
                        )),
                        with: .color(Color(nsColor: .lightGray).opacity(0.28))
                    )
                }
            }
        }
        .accessibilityHidden(true)
    }
}
