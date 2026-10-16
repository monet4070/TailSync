import SwiftUI

struct MockMacPeer: Identifiable {
    let id = UUID()
    let name: String
    let fingerprint: String
    let isTrusted: Bool
    var isEnabled: Bool
    let address: String
    let latency: Int?
}

struct MacConnectionsSection: View {
    @Binding var connectionMode: String
    @Binding var isPairingOpen: Bool
    @Binding var isRemoteDrawerOpen: Bool
    @Binding var peers: [MockMacPeer]
    let palette: PreviewThemePalette
    @State private var copied = false
    @State private var inputLink = ""

    var body: some View {
        VStack(spacing: 0) {
            // 1. 顶部模式选择器
            HStack {
                Picker("", selection: $connectionMode) {
                    Text("自动").tag("auto")
                    Text("局域网").tag("lan")
                    Text("Iroh").tag("iroh")
                    Text("Tailscale").tag("tailscale")
                }
                .pickerStyle(.segmented)
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 10)

            Divider().background(palette.dividerColor)

            // 2. 本机设备行（紧凑精简）
            HStack(spacing: 10) {
                Image(systemName: "laptopcomputer")
                    .foregroundColor(palette.accentColor)
                    .frame(width: 20)
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 6) {
                        Text("MONET-MACBOOK")
                            .font(.system(size: 12, weight: .semibold))
                        Text("本机")
                            .font(.system(size: 9, weight: .medium))
                            .foregroundColor(palette.accentColor)
                            .padding(.horizontal, 4)
                            .padding(.vertical, 1)
                            .background(palette.accentSoftColor)
                            .cornerRadius(3)
                    }
                    Text("192.168.31.5 · 在线")
                        .font(.system(size: 10, design: .monospaced))
                        .foregroundColor(palette.tertiaryColor)
                }
                Spacer()
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 8)

            Divider().background(palette.dividerColor)

            // 3. 对端设备列表（核心内容置前）
            ForEach($peers) { $peer in
                HStack(spacing: 10) {
                    Image(systemName: "display")
                        .foregroundColor(peer.isTrusted ? palette.primaryColor : palette.tertiaryColor)
                        .frame(width: 20)
                    VStack(alignment: .leading, spacing: 2) {
                        HStack(spacing: 6) {
                            Text(peer.name)
                                .font(.system(size: 12, weight: .semibold))
                            Text(peer.isTrusted ? "已配对" : "未配对")
                                .font(.system(size: 9, weight: .medium))
                                .foregroundColor(peer.isTrusted ? palette.greenColor : palette.tertiaryColor)
                                .padding(.horizontal, 4)
                                .padding(.vertical, 1)
                                .background(peer.isTrusted ? palette.greenColor.opacity(0.12) : palette.borderColor.opacity(0.4))
                                .cornerRadius(3)
                        }
                        HStack(spacing: 4) {
                            Text(peer.address)
                            if let lat = peer.latency {
                                Text("· \(lat) ms")
                            }
                        }
                        .font(.system(size: 10, design: .monospaced))
                        .foregroundColor(peer.isTrusted ? palette.greenColor : palette.tertiaryColor)
                    }
                    Spacer()

                    if peer.isTrusted {
                        Toggle("", isOn: $peer.isEnabled)
                            .toggleStyle(.switch)
                            .controlSize(.mini)
                            .labelsHidden()
                    } else {
                        Button("配对") {}
                            .buttonStyle(.borderedProminent)
                            .controlSize(.small)
                    }
                }
                .padding(.horizontal, 14)
                .padding(.vertical, 8)
                Divider().background(palette.dividerColor)
            }

            // 4. 配对与远程 Iroh（折叠卡片，避免占满屏幕）
            VStack(spacing: 0) {
                HStack {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("附近设备配对")
                            .font(.system(size: 12, weight: .medium))
                        Text(isPairingOpen ? "正在广播 · 120s" : "允许局域网发现此设备")
                            .font(.system(size: 10))
                            .foregroundColor(palette.tertiaryColor)
                    }
                    Spacer()
                    Button(isPairingOpen ? "关闭" : "允许配对") {
                        isPairingOpen.toggle()
                    }
                    .buttonStyle(.bordered)
                    .controlSize(.small)
                }
                .padding(.horizontal, 14)
                .padding(.vertical, 8)

                // 远程配对折叠栏
                Button(action: { isRemoteDrawerOpen.toggle() }) {
                    HStack {
                        Image(systemName: "globe")
                            .font(.system(size: 11))
                            .foregroundColor(palette.secondaryColor)
                        Text("异地配对 (Iroh)")
                            .font(.system(size: 11, weight: .medium))
                            .foregroundColor(palette.secondaryColor)
                        Spacer()
                        Image(systemName: isRemoteDrawerOpen ? "chevron.up" : "chevron.down")
                            .font(.system(size: 10))
                            .foregroundColor(palette.tertiaryColor)
                    }
                    .padding(.horizontal, 14)
                    .padding(.vertical, 6)
                    .background(palette.windowBackgroundColor.opacity(0.5))
                }
                .buttonStyle(.plain)

                if isRemoteDrawerOpen {
                    VStack(alignment: .leading, spacing: 8) {
                        HStack {
                            Button(copied ? "已复制邀请链接" : "创建并复制邀请链接") {
                                copied = true
                                DispatchQueue.main.asyncAfter(deadline: .now() + 2) { copied = false }
                            }
                            .buttonStyle(.bordered)
                            .controlSize(.small)
                            Spacer()
                        }

                        HStack(spacing: 6) {
                            TextField("粘贴 tailsync://pair/...", text: $inputLink)
                                .textFieldStyle(.roundedBorder)
                                .font(.system(size: 10, design: .monospaced))
                            Button("加入") {}
                                .buttonStyle(.borderedProminent)
                                .controlSize(.small)
                                .disabled(inputLink.isEmpty)
                        }
                    }
                    .padding(12)
                    .background(palette.windowBackgroundColor)
                }
            }
        }
    }
}
