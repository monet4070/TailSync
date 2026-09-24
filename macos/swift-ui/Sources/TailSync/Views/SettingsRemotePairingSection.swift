import SwiftUI

extension SettingsView {
    /// Collapsed-by-default drawer for the remote Iroh pairing module. The
    /// header is a single row; the invite flow only occupies space once the
    /// user expands it, so the device list stays the visual focus above.
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
                        if !remotePairingExpanded {
                            Text(Loc.t("settings.remotePairingDescription"))
                                .font(.caption2)
                                .foregroundColor(palette.tertiaryColor)
                                .lineLimit(1)
                        }
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
        VStack(alignment: .leading, spacing: 10) {
            VStack(alignment: .leading, spacing: 7) {
                Text(Loc.t("settings.createRemoteInvite"))
                    .font(.caption.weight(.semibold))
                Text(Loc.t("settings.createRemoteInviteDescription"))
                    .font(.caption2)
                    .foregroundColor(palette.secondaryColor)
                    .fixedSize(horizontal: false, vertical: true)
                HStack(spacing: 8) {
                    Button(Loc.t("settings.createRemoteInvite")) {
                        createRemotePairingInvite()
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(remotePairingInProgress)
                    if let invite = remoteInvite {
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
                        .help(invite.link)
                        Button(Loc.t("settings.cancelRemoteInvite")) {
                            cancelRemotePairingInvite()
                        }
                        .buttonStyle(.bordered)
                        .disabled(remotePairingInProgress)
                    }
                }
                if let invite = remoteInvite {
                    Text(invite.link)
                        .font(.caption2.monospaced())
                        .foregroundColor(palette.secondaryColor)
                        .textSelection(.enabled)
                        .lineLimit(3)
                        .fixedSize(horizontal: false, vertical: true)
                    Text(
                        Loc.t("settings.remoteInviteExpires")
                            .replacingOccurrences(of: "{seconds}", with: String(invite.remaining_seconds))
                    )
                    .font(.caption2)
                    .foregroundColor(palette.tertiaryColor)
                }
            }
            .padding(.leading, 34)

            VStack(alignment: .leading, spacing: 7) {
                Text(Loc.t("settings.useRemoteInvite"))
                    .font(.caption.weight(.semibold))
                Text(Loc.t("settings.useRemoteInviteDescription"))
                    .font(.caption2)
                    .foregroundColor(palette.secondaryColor)
                    .fixedSize(horizontal: false, vertical: true)
                TextField(Loc.t("settings.remoteInvitePlaceholder"), text: $remoteInviteLink, axis: .vertical)
                    .textFieldStyle(.roundedBorder)
                    .lineLimit(1...3)
                    .disabled(remotePairingInProgress)
                HStack(spacing: 8) {
                    Button(Loc.t("settings.checkInvite")) {
                        inspectRemotePairingLink()
                    }
                    .buttonStyle(.bordered)
                    .disabled(remotePairingInProgress || remoteInviteLink.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    Button(Loc.t("settings.startRemotePairing")) {
                        startRemotePairing()
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(remotePairingInProgress || remoteInviteLink.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }
                if let preview = remoteInvitePreview {
                    Text(
                        Loc.t("settings.remoteInviteValid")
                            .replacingOccurrences(of: "{seconds}", with: String(preview.remaining_seconds))
                    )
                    .font(.caption2)
                    .foregroundColor(palette.positiveColor)
                }
            }
            .padding(.leading, 34)

            if let remotePairingMessage {
                Text(remotePairingMessage)
                    .font(.caption2)
                    .foregroundColor(palette.warningColor)
                    .fixedSize(horizontal: false, vertical: true)
                    .padding(.leading, 34)
            }
        }
        .padding(.horizontal, 16)
        .padding(.bottom, 10)
    }
}
