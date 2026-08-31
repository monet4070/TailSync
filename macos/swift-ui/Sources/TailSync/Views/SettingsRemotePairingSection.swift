import SwiftUI

extension SettingsView {
    var remotePairingSection: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .top, spacing: 10) {
                Image(systemName: "globe")
                    .foregroundColor(palette.accentColor)
                    .frame(width: 24)
                VStack(alignment: .leading, spacing: 2) {
                    Text(Loc.t("settings.remotePairing"))
                        .font(.body.weight(.medium))
                    Text(Loc.t("settings.remotePairingDescription"))
                        .font(.caption2)
                        .foregroundColor(palette.tertiaryColor)
                        .fixedSize(horizontal: false, vertical: true)
                }
                Spacer()
            }

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
        .padding(.vertical, 10)
    }
}
