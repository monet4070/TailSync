import SwiftUI

struct HistoryView: View {
    @ObservedObject private var loc = Loc.shared
    @State private var entries: [HistoryEntry] = []
    @State private var keyword = ""
    @State private var page = 0
    @State private var hasNext = false
    @State private var isLoading = false
    @State private var restoredId: Int64? = nil
    @State private var errorMsg: String? = nil
    @State private var lastVersion: UInt64 = 0
    @State private var daemonOnline = false
    @State private var fileProgress: (name: String, sent: UInt64, total: UInt64, active: Bool)? = nil
    @State private var showingClearAlert = false

    private let pageSize = 30

    var body: some View {
        VStack(spacing: 0) {
            // Search + status
            HStack {
                Image(systemName: "magnifyingglass").foregroundColor(.secondary)
                TextField(Loc.t("history.search"), text: $keyword)
                    .textFieldStyle(.plain)
                    .onSubmit { page = 0; load() }
                if !keyword.isEmpty {
                    Button { keyword = ""; page = 0; load() } label: {
                        Image(systemName: "xmark.circle.fill").foregroundColor(.secondary)
                    }.buttonStyle(.plain)
                }
                Circle()
                    .fill(daemonOnline ? Color.green : Color.red)
                    .frame(width: 7, height: 7)
            }
            .padding(8).background(Color.primary.opacity(0.05)).cornerRadius(8)
            .padding(.horizontal, 12).padding(.top, 8)

            // List
            if isLoading && entries.isEmpty {
                Spacer()
                ProgressView().controlSize(.small)
                Spacer()
            } else if entries.isEmpty {
                Spacer()
                VStack(spacing: 8) {
                    Image(systemName: "doc.on.clipboard").font(.system(size: 32)).foregroundColor(.secondary).opacity(0.4)
                    Text(Loc.t("history.empty")).font(.body).foregroundColor(.secondary)
                }
                Spacer()
            } else {
                List {
                    ForEach(entries) { entry in
                        HistoryRow(entry: entry, isRestored: restoredId == entry.id)
                            .contentShape(Rectangle())
                            .onTapGesture(count: 2) { restore(entry.id) }
                            .overlay(RightClickNSView { delete(entry.id) })
                            .transition(.opacity.combined(with: .scale(scale: 0.95)))
                    }
                }
                .listStyle(.plain)
                .animation(.spring(response: 0.35, dampingFraction: 0.9), value: entries.count)
            }

            // Pagination + clear
            HStack(spacing: 12) {
                Button { page -= 1; load() } label: { Image(systemName: "chevron.left") }.disabled(page == 0)
                Text("\(page + 1)").font(.caption).foregroundColor(.secondary).monospacedDigit()
                Button { page += 1; load() } label: { Image(systemName: "chevron.right") }.disabled(!hasNext)
                Spacer()
                if !entries.isEmpty {
                    Button(Loc.t("history.clearAll"), role: .destructive) { showingClearAlert = true }
                        .font(.caption).buttonStyle(.plain)
                }
            }
            .padding(.vertical, 8)
            .padding(.horizontal, 12)
            .alert(Loc.t("history.confirmClear"), isPresented: $showingClearAlert) {
                Button("Cancel", role: .cancel) {}
                Button(Loc.t("history.clearAll"), role: .destructive) { clearAll() }
            }
        }
        .onAppear { load() }
        .onReceive(Timer.publish(every: 0.5, on: .main, in: .common).autoconnect()) { _ in
            Task {
                let v = await ApiClient.shared.getVersion()
                if v != lastVersion { lastVersion = v; page = 0; load() }
                if v > 0 { daemonOnline = true }
                fileProgress = await ApiClient.shared.getFileProgress()
            }
        }
        .overlay(alignment: .bottom) {
            VStack(spacing: 6) {
                if let p = fileProgress, p.active {
                    VStack(spacing: 3) {
                        ProgressView(value: Double(p.sent), total: Double(p.total)).progressViewStyle(.linear)
                        Text("Sending \(p.name) — \(p.sent)/\(p.total)").font(.caption2).foregroundColor(.secondary)
                    }
                    .padding(.horizontal, 14).padding(.vertical, 8)
                    .background(.ultraThinMaterial).cornerRadius(10)
                    .padding(.horizontal, 8)
                }
                if restoredId != nil {
                    Text(Loc.t("history.restored"))
                        .font(.caption).padding(.horizontal, 12).padding(.vertical, 6)
                        .background(.ultraThinMaterial).cornerRadius(20).padding(.bottom, 8)
                }
            }
        }
    }

    private func load() {
        Task {
            isLoading = true
            do {
                let result = try await ApiClient.shared.getHistory(
                    keyword: keyword.isEmpty ? nil : keyword, limit: pageSize, offset: page * pageSize)
                entries = result
                hasNext = result.count >= pageSize
            } catch { errorMsg = error.localizedDescription }
            isLoading = false
        }
    }

    private func restore(_ id: Int64) {
        Task {
            try? await ApiClient.shared.restoreEntry(id: id)
            withAnimation(.easeOut(duration: 0.4)) { restoredId = id }
            try? await Task.sleep(nanoseconds: 1_500_000_000)
            withAnimation(.easeOut(duration: 0.4)) { restoredId = nil }
        }
    }

    private func delete(_ id: Int64) {
        Task { try? await ApiClient.shared.deleteEntry(id: id); entries.removeAll { $0.id == id } }
    }

    private func clearAll() {
        Task {
            let ok = await ApiClient.shared.clearAllHistory()
            if ok { entries = []; page = 0; hasNext = false }
        }
    }
}

// ── Row ──────────────────────────────────────────────────────────

struct HistoryRow: View {
    let entry: HistoryEntry; let isRestored: Bool
    @State private var thumbnail: NSImage? = nil

    var body: some View {
        HStack(spacing: 10) {
            Group {
                if entry.type == "image", let thumb = thumbnail {
                    Image(nsImage: thumb).resizable().aspectRatio(contentMode: .fill)
                        .frame(width: 32, height: 32).cornerRadius(5)
                } else {
                    Image(systemName: entry.icon).frame(width: 24, height: 24)
                        .foregroundColor(.accentColor).background(Color.accentColor.opacity(0.1)).cornerRadius(5)
                }
            }
            .frame(width: 32, height: 32)
            .task { if entry.type == "image", let img = await ApiClient.shared.getImageData(id: entry.id) { thumbnail = rgbaToImage(img) } }

            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(entry.type.uppercased()).font(.caption2).fontWeight(.semibold)
                        .foregroundColor(.accentColor).padding(.horizontal, 4).padding(.vertical, 1)
                        .background(Color.accentColor.opacity(0.12)).cornerRadius(3)
                    Text(entry.formattedTime).font(.caption).foregroundColor(.secondary)
                    Spacer()
                    Text(entry.source_peer).font(.caption2).foregroundColor(.secondary).lineLimit(1)
                }
                Text(entry.description).font(.body).lineLimit(1)
                Text(entry.formattedSize).font(.caption2).foregroundColor(.secondary).monospacedDigit()
            }
        }
        .padding(.vertical, 2).frame(maxWidth: .infinity, alignment: .leading)
        .background(isRestored ? Color.accentColor.opacity(0.12) : .clear)
        .cornerRadius(6).contentShape(Rectangle())
        .animation(.easeOut(duration: 0.4), value: isRestored)
    }
}

// ── Helpers ──────────────────────────────────────────────────────

private func rgbaToImage(_ data: ApiClient.ImageData) -> NSImage? {
    let w = data.width, h = data.height
    guard w > 0, h > 0, data.rgba.count >= w * h * 4 else { return nil }
    guard let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: w, pixelsHigh: h,
                                      bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                                      colorSpaceName: .deviceRGB, bytesPerRow: w * 4, bitsPerPixel: 32) else { return nil }
    data.rgba.copyBytes(to: rep.bitmapData!, count: data.rgba.count)
    let img = NSImage(size: NSSize(width: w, height: h))
    img.addRepresentation(rep)
    return img
}

// ── Right-click capture ─────────────────────────────────────────

/// Overlays each row to intercept right-clicks directly via `rightMouseDown`.
/// Much safer than `NSEvent.addLocalMonitorForEvents` which is global and
/// crashes when the monitored view is deallocated during event propagation.
struct RightClickNSView: NSViewRepresentable {
    let action: () -> Void

    func makeNSView(context: Context) -> NSView {
        let view = RightClickView(frame: .zero)
        view.action = action
        return view
    }

    func updateNSView(_ nsView: NSView, context: Context) {
        if let v = nsView as? RightClickView {
            v.action = action
        }
    }

    class RightClickView: NSView {
        var action: (() -> Void)?

        override func hitTest(_ point: NSPoint) -> NSView? {
            // Only claim right-clicks; pass everything else through
            // to the SwiftUI gesture recognizers (double-tap, etc.).
            guard let event = NSApp.currentEvent else { return nil }
            if event.type == .rightMouseDown || event.type == .rightMouseUp {
                return self
            }
            return nil
        }

        override func rightMouseDown(with event: NSEvent) {
            // Defer to next run-loop so the view is still alive when the
            // action removes the row from the List.
            let act = action
            DispatchQueue.main.async { act?() }
        }
    }
}
