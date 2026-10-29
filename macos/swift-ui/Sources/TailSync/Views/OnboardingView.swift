import SwiftUI
import AppKit

final class GuidedTourController: ObservableObject {
    static let shared = GuidedTourController()
    @Published private(set) var active = false
    @Published private(set) var index = 0
    @Published private(set) var error = false
    @Published private(set) var suppressed: Bool
    private let progress: OnboardingProgress
    private let navigate: (String) -> Void
    var step: OnboardingGuide.Step { OnboardingGuide.shared.steps[index] }

    init(progress: OnboardingProgress = OnboardingProgress(), navigate: @escaping (String) -> Void = GuidedTourController.openWindow) {
        self.progress = progress
        self.navigate = navigate
        suppressed = progress.suppressed
    }

    func start(resume: Bool = false) {
        guard !suppressed else { return }
        // Automatic presentation happens once. The walkthrough can still be
        // started from Settings until the user explicitly disables it forever.
        do { try progress.finish() } catch { self.error = true; return }
        active = true
        index = resume ? progress.step : 0
        progress.select(index)
        error = false
        showWindow()
    }
    func move(_ index: Int) {
        guard OnboardingGuide.shared.steps.indices.contains(index) else { return }
        self.index = index
        error = false
        progress.select(index)
        showWindow()
    }
    func finish(permanently: Bool = false) {
        do {
            try progress.finish(permanently: permanently)
            suppressed = progress.suppressed
            active = false
            error = false
            Task { @MainActor in HistoryPreviewWindowController.shared.close() }
        }
        catch { self.error = true }
    }
    func preserveAfterUpdate() throws {
        try progress.preserveAfterUpdate()
    }
    private func showWindow() {
        navigate(step.window)
    }
    private static func openWindow(_ window: String) {
        switch window {
        case "connections": AppDelegate.showConnections()
        case "history": AppDelegate.showHistory()
        default: AppDelegate.showSettings()
        }
    }
}

struct TourAnchors: PreferenceKey {
    static var defaultValue: [String: Anchor<CGRect>] = [:]
    static func reduce(value: inout [String: Anchor<CGRect>], nextValue: () -> [String: Anchor<CGRect>]) {
        value.merge(nextValue(), uniquingKeysWith: { _, next in next })
    }
}

extension View {
    func tourTarget(_ id: String) -> some View {
        // Preserve targets inside this view (for example the Favorites button
        // inside the search bar) when adding the enclosing target.
        self.id(id).transformAnchorPreference(key: TourAnchors.self, value: .bounds) { anchors, anchor in
            anchors[id] = anchor
        }
    }
    func guidedTour(window: String, controller: GuidedTourController = .shared) -> some View {
        modifier(GuidedTourModifier(window: window, tour: controller))
    }
    func tourScrollable() -> some View { modifier(TourScrollModifier()) }
}

private struct TourScrollModifier: ViewModifier {
    @ObservedObject private var tour = GuidedTourController.shared
    func body(content: Content) -> some View {
        ScrollViewReader { proxy in
            content.onReceive(tour.$index) { _ in
                guard tour.active else { return }
                DispatchQueue.main.async { proxy.scrollTo(tour.step.target, anchor: .top) }
            }
            .onAppear {
                if tour.active { proxy.scrollTo(tour.step.target, anchor: .top) }
            }
            .background(GeometryReader { geometry in
                Color.clear.onChange(of: geometry.size) { _ in
                    guard tour.active else { return }
                    DispatchQueue.main.async { proxy.scrollTo(tour.step.target, anchor: .top) }
                }
            })
            .clipped()
            .tourTarget("tour-viewport")
        }
    }
}

private struct GuidedTourModifier: ViewModifier {
    let window: String
    @ObservedObject var tour: GuidedTourController
    @State private var panelHeight: CGFloat = 260
    private var active: Bool { tour.active && tour.step.window == window }
    func body(content: Content) -> some View {
        // Reserve real layout space for the card, so a large highlighted list
        // or section remains operable without inventing a smaller target rect.
        content.padding(.bottom, active ? panelHeight : 0)
        .overlayPreferenceValue(TourAnchors.self) { anchors in
            if active {
                GeometryReader { geometry in
                    TourSpotlight(rect: anchors[tour.step.target].map { geometry[$0] },
                                  viewport: anchors["tour-viewport"].map { geometry[$0] },
                                  size: geometry.size, tour: tour, panelHeight: $panelHeight)
                }
            }
        }
    }
}

enum TourGeometry {
    static func highlight(target: CGRect?, viewport: CGRect) -> CGRect? {
        guard let target, target.width > 0, target.height > 0 else { return nil }
        let clipped = target.insetBy(dx: -4, dy: -4).intersection(viewport)
        return clipped.isNull || clipped.isEmpty ? nil : clipped
    }
}

struct TourSpotlight: View {
    let rect: CGRect?
    let viewport: CGRect?
    let size: CGSize
    @ObservedObject var tour: GuidedTourController
    @Binding var panelHeight: CGFloat
    @ObservedObject private var loc = Loc.shared
    @Environment(\.tailSyncPalette) private var palette
    @State private var cardHeight: CGFloat = 260
    @State private var copyHeight: CGFloat = 120
    private var guide: OnboardingGuide { .shared }
    private func text(_ value: [String: String]) -> String { guide.text(value, language: loc.lang) }
    private func label(_ key: String) -> String { guide.label(key, language: loc.lang) }
    private var highlight: CGRect? {
        let contentBounds = CGRect(x: 2, y: 2, width: max(0, size.width - 4), height: max(0, size.height - panelHeight - 4))
        return TourGeometry.highlight(target: rect, viewport: viewport.map { $0.intersection(contentBounds) } ?? contentBounds)
    }
    var body: some View {
        let hole = highlight
        let width = min(440, size.width - 24)
        let visibleCardHeight = min(cardHeight, size.height * 0.46)
        let left = (size.width - width) / 2
        let top = size.height - visibleCardHeight - 12
        ZStack(alignment: .topLeading) {
            // Four panels leave the actual controls inside the hole interactive.
            if let hole {
                shade(CGRect(x: 0, y: 0, width: size.width, height: hole.minY))
                shade(CGRect(x: 0, y: hole.maxY, width: size.width, height: max(0, size.height - hole.maxY)))
                shade(CGRect(x: 0, y: hole.minY, width: hole.minX, height: hole.height))
                shade(CGRect(x: hole.maxX, y: hole.minY, width: max(0, size.width - hole.maxX), height: hole.height))
                RoundedRectangle(cornerRadius: 6).stroke(palette.accentColor, lineWidth: 2)
                    .frame(width: hole.width, height: hole.height).offset(x: hole.minX, y: hole.minY)
                    .allowsHitTesting(false)
            } else {
                // Loading/retry controls must remain reachable when no target exists.
                shade(CGRect(origin: .zero, size: size)).allowsHitTesting(false)
            }
            card(hasTarget: hole != nil)
                    .background(GeometryReader { proxy in
                        Color.clear.onAppear { cardHeight = proxy.size.height }
                            .onChange(of: proxy.size.height) { cardHeight = $0 }
                    })
                .frame(width: width, height: visibleCardHeight)
                .clipShape(RoundedRectangle(cornerRadius: 12))
                .offset(x: left, y: top)
        }
        .frame(width: size.width, height: size.height, alignment: .topLeading)
        .onAppear { panelHeight = visibleCardHeight + 24 }
        .onChange(of: visibleCardHeight) { panelHeight = $0 + 24 }
    }
    private func shade(_ rect: CGRect) -> some View {
        Color.black.opacity(0.66).frame(width: rect.width, height: rect.height)
            .contentShape(Rectangle()).onTapGesture {}
            .offset(x: rect.minX, y: rect.minY)
    }
    private func card(hasTarget: Bool) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text("TAILSYNC · \(label("title"))").font(.system(size: 10, weight: .semibold)).foregroundColor(palette.accentColor)
                Spacer()
                Button { tour.finish() } label: { Image(systemName: "xmark") }
                    .buttonStyle(.plain).accessibilityLabel(label("skip"))
            }
            ScrollView {
                VStack(alignment: .leading, spacing: 10) {
                    Text(text(tour.step.title)).font(.system(size: 17, weight: .semibold)).accessibilityAddTraits(.isHeader)
                    Text(hasTarget ? text(tour.step.body) : label("waiting")).font(.system(size: 12)).foregroundColor(palette.secondaryColor).lineSpacing(3)
                    Text(text(tour.step.tip)).font(.system(size: 11)).foregroundColor(palette.secondaryColor)
                    if hasTarget {
                        Label(label("hint"), systemImage: "cursorarrow").font(.system(size: 10)).foregroundColor(palette.accentColor)
                    }
                    if tour.error { Text(label("error")).font(.caption).foregroundColor(palette.warningColor) }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .fixedSize(horizontal: false, vertical: true)
                .background(GeometryReader { geometry in
                    Color.clear.onAppear { copyHeight = geometry.size.height }
                        .onChange(of: geometry.size.height) { copyHeight = $0 }
                })
            }
            .frame(height: min(copyHeight, max(40, size.height * 0.46 - 96)))
            HStack {
                Text("\(tour.index + 1) / \(guide.steps.count)").font(.system(size: 11, design: .monospaced)).foregroundColor(palette.tertiaryColor)
                Spacer()
                Button { tour.move(tour.index - 1) } label: { Image(systemName: "arrow.left") }
                    .disabled(tour.index == 0).accessibilityLabel(label("back"))
                Button(label(tour.index == guide.steps.count - 1 ? "finish" : "next")) {
                    if tour.index == guide.steps.count - 1 { tour.finish() }
                    else { tour.move(tour.index + 1) }
                }.buttonStyle(.borderedProminent)
            }.padding(.top, 6)
            Button(label("never")) { tour.finish(permanently: true) }
                .buttonStyle(.plain).font(.system(size: 10)).foregroundColor(palette.secondaryColor)
                .help(label("neverHint"))
        }
        .fixedSize(horizontal: false, vertical: true)
        .padding(18).foregroundColor(palette.primaryColor)
        .background(palette.raisedColor).clipShape(RoundedRectangle(cornerRadius: 12))
        .overlay(RoundedRectangle(cornerRadius: 12).stroke(palette.borderColor))
        .shadow(color: .black.opacity(0.3), radius: 20, y: 6)
        .tint(palette.accentColor)
    }
}
