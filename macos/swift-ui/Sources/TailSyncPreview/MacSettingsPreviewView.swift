import SwiftUI

struct MacSettingsPreviewView: View {
    @State private var isDarkMode = false
    @State private var syncEnabled = true
    @State private var launchAtLogin = true
    @State private var notificationsEnabled = true
    @State private var progressBarEnabled = true
    @State private var syncShortcut = ["⌘", "⇧", "S"]
    @State private var historyShortcut = ["⌘", "⇧", "H"]
    @State private var connectionMode = "auto"
    @State private var isPairingOpen = false
    @State private var isRemoteDrawerOpen = false
    @State private var historyLimit: Double = 100
    @State private var peers = [
        MockMacPeer(
            name: "DESKTOP-5K2P9QM",
            fingerprint: "sha256:1b8d...",
            isTrusted: true,
            isEnabled: true,
            address: "192.168.31.8",
            latency: 8
        ),
        MockMacPeer(
            name: "SURFACE-GO",
            fingerprint: "sha256:4c2a...",
            isTrusted: false,
            isEnabled: false,
            address: "192.168.31.20",
            latency: nil
        )
    ]

    var palette: PreviewThemePalette {
        isDarkMode ? .dark() : .light()
    }

    var body: some View {
        VStack(spacing: 0) {
            // 顶部小横条：试用提示 & 浅/深色切换
            HStack {
                HStack(spacing: 6) {
                    Image(systemName: "sparkles")
                        .foregroundColor(palette.accentColor)
                    Text("TailSync Mac 原生静态原型 (SwiftUI)")
                        .font(.system(size: 11, weight: .semibold))
                }
                Spacer()
                Button(action: { isDarkMode.toggle() }) {
                    Image(systemName: isDarkMode ? "moon.fill" : "sun.max.fill")
                        .font(.system(size: 12))
                        .foregroundColor(palette.secondaryColor)
                        .padding(4)
                }
                .buttonStyle(.plain)
                .help("切换浅色/深色主题")
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 8)
            .background(palette.surfaceColor)
            .overlay(Divider().background(palette.dividerColor), alignment: .bottom)

            // 滚动设置主体
            ScrollView {
                VStack(spacing: 18) {
                    // 1. 连接与设备
                    cardContainer(title: "连接与设备") {
                        MacConnectionsSection(
                            connectionMode: $connectionMode,
                            isPairingOpen: $isPairingOpen,
                            isRemoteDrawerOpen: $isRemoteDrawerOpen,
                            peers: $peers,
                            palette: palette
                        )
                    }

                    // 2. 通用设置（修复原版大面积空白，并凸显实体快捷键）
                    cardContainer(title: "通用") {
                        VStack(spacing: 0) {
                            toggleRow(
                                title: "剪贴板同步",
                                desc: "将本机新复制内容同步到已启用设备",
                                isOn: $syncEnabled
                            )
                            Divider().background(palette.dividerColor)

                            toggleRow(
                                title: "开机时自动启动",
                                desc: "登录 macOS 后自动在后台运行",
                                isOn: $launchAtLogin
                            )
                            Divider().background(palette.dividerColor)

                            // 实体键帽快捷键
                            ShortcutRowView(
                                title: "同步快捷键",
                                description: "全局暂停或恢复剪贴板同步",
                                keys: $syncShortcut,
                                defaultKeys: ["⌘", "⇧", "S"],
                                palette: palette
                            )
                            Divider().background(palette.dividerColor)

                            ShortcutRowView(
                                title: "历史记录快捷键",
                                description: "从任意应用唤起历史记录窗口",
                                keys: $historyShortcut,
                                defaultKeys: ["⌘", "⇧", "H"],
                                palette: palette
                            )
                            Divider().background(palette.dividerColor)

                            toggleRow(
                                title: "通知",
                                desc: "远端内容到达时显示系统通知",
                                isOn: $notificationsEnabled
                            )
                            Divider().background(palette.dividerColor)

                            toggleRow(
                                title: "进度条",
                                desc: "在大文件传输时显示悬浮进度",
                                isOn: $progressBarEnabled
                            )
                        }
                    }

                    // 3. 历史记录与存储
                    cardContainer(title: "历史记录与存储") {
                        VStack(spacing: 0) {
                            HStack {
                                VStack(alignment: .leading, spacing: 2) {
                                    Text("历史记录保留上限")
                                        .font(.system(size: 13, weight: .medium))
                                    Text("最多保留 \(Int(historyLimit)) 条剪贴板历史")
                                        .font(.system(size: 11))
                                        .foregroundColor(palette.tertiaryColor)
                                }
                                Spacer()
                                originalSlider
                            }
                            .padding(.horizontal, 14)
                            .padding(.vertical, 8)
                        }
                    }
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 16)
            }
        }
        .frame(width: 520, height: 640)
        .background(palette.windowBackgroundColor)
    }

    private var originalSlider: some View {
        HStack(spacing: 12) {
            GeometryReader { geometry in
                let thumbSize: CGFloat = 18
                let travelWidth = max(1, geometry.size.width - thumbSize)
                let progress = CGFloat(historyLimit - 10) / 490

                ZStack(alignment: .leading) {
                    Capsule()
                        .fill(palette.borderColor.opacity(0.65))
                        .frame(height: 5)
                        .padding(.horizontal, thumbSize / 2)

                    Capsule()
                        .fill(palette.accentColor)
                        .frame(width: max(1, travelWidth * progress), height: 5)
                        .offset(x: thumbSize / 2)

                    Circle()
                        .fill(palette.surfaceColor)
                        .overlay {
                            Circle()
                                .stroke(palette.accentColor.opacity(0.75), lineWidth: 1)
                        }
                        .shadow(color: .black.opacity(0.18), radius: 2.5, y: 1)
                        .frame(width: thumbSize, height: thumbSize)
                        .offset(x: travelWidth * progress)
                }
                .frame(maxHeight: .infinity)
                .contentShape(Rectangle())
                .gesture(
                    DragGesture(minimumDistance: 0)
                        .onChanged { value in
                            let position = min(travelWidth, max(0, value.location.x - thumbSize / 2))
                            let step = Int((position / travelWidth * 49).rounded())
                            historyLimit = Double(10 + step * 10)
                        }
                )
            }
            .frame(width: 170, height: 28)

            Text("\(Int(historyLimit))")
                .font(.system(.caption, design: .monospaced).weight(.medium))
                .foregroundColor(palette.accentColor)
                .frame(width: 44, height: 24)
                .background(palette.accentSoftColor)
                .clipShape(RoundedRectangle(cornerRadius: 5, style: .continuous))
        }
    }

    private func cardContainer<Content: View>(title: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(title)
                .font(.system(size: 11, weight: .semibold))
                .foregroundColor(palette.tertiaryColor)
                .padding(.horizontal, 4)

            VStack(spacing: 0) {
                content()
            }
            .background(palette.surfaceColor)
            .cornerRadius(8)
            .overlay(
                RoundedRectangle(cornerRadius: 8)
                    .stroke(palette.borderColor, lineWidth: 1)
            )
            .shadow(color: Color.black.opacity(0.03), radius: 2, y: 1)
        }
    }

    private func toggleRow(title: String, desc: String, isOn: Binding<Bool>) -> some View {
        HStack {
            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                    .font(.system(size: 13, weight: .medium))
                    .foregroundColor(palette.primaryColor)
                Text(desc)
                    .font(.system(size: 11))
                    .foregroundColor(palette.tertiaryColor)
            }
            Spacer()
            Toggle("", isOn: isOn)
                .toggleStyle(.switch)
                .controlSize(.small)
                .labelsHidden()
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 8)
    }
}
