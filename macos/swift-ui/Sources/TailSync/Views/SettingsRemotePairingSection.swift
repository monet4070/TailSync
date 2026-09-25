import SwiftUI

extension ConnectionsView {
    /// Collapsed-by-default drawer for the remote Iroh pairing module.
    /// In collapsed state, it presents a single clean row with an earth icon,
    /// title, subtitle, and rotating chevron.
    /// When expanded, it unrolls into a single-column, spacious vertical flow
    /// eliminating any horizontal crowding or clipping.
    @ViewBuilder
    var remotePairingDrawer: some View {
        VStack(alignment: .leading, spacing: 0) {
            Button {
                withAnimation(.easeInOut(duration: 0.2)) {
                    remotePairingExpanded.toggle()
                }
            } label: {
                HStack(spacing: 10) {
                    Image(systemName: "globe")
                        .foregroundColor(palette.accentColor)
                        .frame(width: 24)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(Loc.t("settings.remotePairing"))
                            .font(.body.weight(.medium))
                            .foregroundColor(palette.primaryColor)
                        Text(Loc.t("settings.remotePairingDescription"))
                            .font(.caption2)
                            .foregroundColor(palette.tertiaryColor)
                            .lineLimit(1)
                    }
                    Spacer()
                    Image(systemName: "chevron.right")
                        .font(.caption.weight(.semibold))
                        .foregroundColor(palette.tertiaryColor)
                        .rotationEffect(.degrees(remotePairingExpanded ? 90 : 0))
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 10)
                .frame(minHeight: 36)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)

            if remotePairingExpanded {
                remotePairingBody
                    .transition(.opacity.combined(with: .move(edge: .top)))
            }
        }
    }

    private var remotePairingBody: some View {
        VStack(alignment: .leading, spacing: 14) {
            // ── 「创建远程邀请」模块 ──
            if let invite = remoteInvite {
                // 已生成链接时：全宽横向展示生成的链接文本框、复制按钮、取消按钮与剩余时间
                VStack(alignment: .leading, spacing: 6) {
                    HStack(alignment: .center) {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(Loc.t("settings.createRemoteInvite"))
                                .font(.caption.weight(.semibold))
                                .foregroundColor(palette.primaryColor)
                            Text(Loc.t("settings.createRemoteInviteDescription"))
                                .font(.caption2)
                                .foregroundColor(palette.secondaryColor)
                        }
                        Spacer(minLength: 8)
                        Text(
                            Loc.t("settings.remoteInviteExpires")
                                .replacingOccurrences(of: "{seconds}", with: String(invite.remaining_seconds))
                        )
                        .font(.caption2.monospaced())
                        .foregroundColor(palette.tertiaryColor)
                    }

                    HStack(spacing: 8) {
                        TextField("", text: .constant(invite.link))
                            .textFieldStyle(.roundedBorder)
                            .font(.system(.caption2, design: .monospaced))
                            .frame(maxWidth: .infinity)

                        Button {
                            copyRemotePairingInvite()
                        } label: {
                            Label(
                                remoteInviteCopied
                                    ? Loc.t("settings.copied")
                                    : Loc.t("settings.copyInvite"),
                                systemImage: remoteInviteCopied ? "checkmark" : "doc.on.doc"
                            )
                        }
                        .buttonStyle(.bordered)
                        .controlSize(.small)
                        .help(invite.link)

                        Button(Loc.t("settings.cancelRemoteInvite")) {
                            cancelRemotePairingInvite()
                        }
                        .buttonStyle(.bordered)
                        .controlSize(.small)
                        .disabled(remotePairingInProgress)
                    }
                }
            } else {
                // 未生成链接时：左侧标题与说明，右侧直接水平对齐「创建远程邀请」按钮，单行舒展
                HStack(alignment: .center, spacing: 12) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(Loc.t("settings.createRemoteInvite"))
                            .font(.caption.weight(.semibold))
                            .foregroundColor(palette.primaryColor)
                        Text(Loc.t("settings.createRemoteInviteDescription"))
                            .font(.caption2)
                            .foregroundColor(palette.secondaryColor)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                    Spacer(minLength: 12)
                    Button(Loc.t("settings.createRemoteInvite")) {
                        createRemotePairingInvite()
                    }
                    .buttonStyle(.borderedProminent)
                    .controlSize(.small)
                    .disabled(remotePairingInProgress)
                }
            }

            // ── 「使用远程邀请」模块 ──
            // 标题与描述在上，下方全宽横向放置输入框（提供足够的宽度容纳长链接）；
            // 底部栏左侧动态显示邀请校验反馈，右侧水平排列「检查邀请」与「开始配对」按钮
            VStack(alignment: .leading, spacing: 6) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(Loc.t("settings.useRemoteInvite"))
                        .font(.caption.weight(.semibold))
                        .foregroundColor(palette.primaryColor)
                    Text(Loc.t("settings.useRemoteInviteDescription"))
                        .font(.caption2)
                        .foregroundColor(palette.secondaryColor)
                        .fixedSize(horizontal: false, vertical: true)
                }

                TextField(Loc.t("settings.remoteInvitePlaceholder"), text: $remoteInviteLink)
                    .textFieldStyle(.roundedBorder)
                    .font(.system(.caption2, design: .monospaced))
                    .disabled(remotePairingInProgress)

                HStack(alignment: .center, spacing: 8) {
                    if let preview = remoteInvitePreview {
                        Text(
                            Loc.t("settings.remoteInviteValid")
                                .replacingOccurrences(of: "{seconds}", with: String(preview.remaining_seconds))
                        )
                        .font(.caption2)
                        .foregroundColor(palette.positiveColor)
                    } else if let remotePairingMessage {
                        Text(remotePairingMessage)
                            .font(.caption2)
                            .foregroundColor(palette.warningColor)
                            .lineLimit(1)
                    }
                    Spacer(minLength: 8)
                    Button(Loc.t("settings.checkInvite")) {
                        inspectRemotePairingLink()
                    }
                    .buttonStyle(.bordered)
                    .controlSize(.small)
                    .disabled(remotePairingInProgress || remoteInviteLink.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)

                    Button(Loc.t("settings.startRemotePairing")) {
                        startRemotePairing()
                    }
                    .buttonStyle(.borderedProminent)
                    .controlSize(.small)
                    .disabled(remotePairingInProgress || remoteInviteLink.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }
            }
        }
        .padding(.horizontal, 16)
        .padding(.leading, 34)
        .padding(.bottom, 16)
    }
}
