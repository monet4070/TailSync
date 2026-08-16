import AppKit
import SwiftUI

enum HistoryImageViewport {
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
}

struct HistoryImagePreviewView: View {
    let data: Data

    @State private var fittedScale: CGFloat = 1
    @State private var zoom: CGFloat = 1
    @State private var rotation = Angle.zero
    @State private var offset = CGSize.zero
    @State private var containerSize = CGSize.zero
    @State private var showsTransparency = true
    @GestureState private var dragTranslation = CGSize.zero

    private var image: NSImage? { NSImage(data: data) }

    var body: some View {
        VStack(spacing: 0) {
            imageToolbar
            Divider()
            if let image {
                GeometryReader { proxy in
                    ZStack {
                        if showsTransparency { HistoryPreviewCheckerboard() }
                        Color(nsColor: .textBackgroundColor)
                            .opacity(showsTransparency ? 0 : 1)
                        Image(nsImage: image)
                            .resizable()
                            .frame(width: image.size.width, height: image.size.height)
                            .scaleEffect(fittedScale * zoom)
                            .rotationEffect(rotation)
                            .offset(HistoryImageViewport.adding(
                                dragTranslation,
                                to: offset
                            ))
                            .gesture(dragGesture)
                    }
                    .clipped()
                    .onAppear { updateContainer(proxy.size, image: image) }
                    .onChange(of: proxy.size) { size in
                        updateContainer(size, image: image)
                    }
                    .background(HistoryPreviewModifierScrollMonitor { delta in
                        setZoom(zoom * (delta > 0 ? 1.1 : 1 / 1.1))
                    })
                }
                imageMetadata(image)
            }
        }
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
            Button(action: fitCurrentImage) {
                Image(systemName: "arrow.up.left.and.arrow.down.right")
            }
            .help(Loc.t("history.preview.fit"))
            Divider().frame(height: 18)
            Button { setZoom(zoom / 1.2) } label: {
                Image(systemName: "minus.magnifyingglass")
            }
            Text("\(Int((zoom * 100).rounded()))%")
                .font(.caption.monospacedDigit())
                .frame(width: 52)
            Button { setZoom(zoom * 1.2) } label: {
                Image(systemName: "plus.magnifyingglass")
            }
            Spacer()
            Button(action: rotateClockwise) {
                Image(systemName: "rotate.right")
            }
            .help(Loc.t("history.preview.rotate"))
            Button {
                showsTransparency.toggle()
            } label: {
                Image(systemName: showsTransparency
                    ? "checkerboard.rectangle"
                    : "rectangle.fill")
            }
            .help(Loc.t("history.preview.transparency"))
        }
        .buttonStyle(.borderless)
        .controlSize(.small)
        .padding(.horizontal, 12)
        .frame(height: 42)
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
                fromByteCount: Int64(data.count),
                countStyle: .file
            ))
        }
        .font(.caption2)
        .foregroundStyle(.secondary)
        .padding(.horizontal, 12)
        .frame(height: 26)
    }

    private func updateContainer(_ size: CGSize, image: NSImage) {
        containerSize = size
        updateFittedScale(image, rotation: rotation)
    }

    private func fitCurrentImage() {
        guard let image else { return }
        zoom = 1
        offset = .zero
        updateFittedScale(image, rotation: rotation)
    }

    private func updateFittedScale(_ image: NSImage, rotation: Angle) {
        fittedScale = HistoryImageViewport.fittedScale(
            imageSize: image.size,
            containerSize: containerSize,
            rotation: rotation
        )
    }

    private func rotateClockwise() {
        let nextRotation = rotation + .degrees(90)
        rotation = nextRotation
        if let image {
            updateFittedScale(image, rotation: nextRotation)
        }
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
