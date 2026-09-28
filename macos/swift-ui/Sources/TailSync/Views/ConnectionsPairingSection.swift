import SwiftUI

/// Displays the local pairing window and verification sheet from parent-owned state.
struct ConnectionsPairingSection: SettingsChrome {
    @Environment(\.colorScheme) var colorScheme
    let activeTheme: TailSyncThemeSelection
    let pairingStatus: ApiClient.PairingStatus?
    let pairingMessage: String?
    let pairingInProgress: Bool
    let onToggle: () -> Void
    let onCancel: () -> Void
    let onConfirm: () -> Void

    var body: some View { pairingPanel }

    var pairingPanel: some View {
        settingRow {
            Image(systemName: "link.badge.plus")
                .foregroundColor(palette.accentColor)
                .frame(width: 24)
            VStack(alignment: .leading, spacing: 2) {
                Text(Loc.t("settings.pairDevice"))
                    .font(.body.weight(.medium))
                    .foregroundColor(palette.primaryColor)
                Text(pairingWindowSummary)
                    .font(.caption2)
                    .foregroundColor(palette.tertiaryColor)
            }
            Spacer()
            Button(pairingStatus?.pairing_enabled == true
                   ? Loc.t("settings.closePairing")
                   : Loc.t("settings.allowPairing")) {
                onToggle()
            }
            .buttonStyle(.bordered)
            .controlSize(.small)
            .disabled(pairingInProgress)
            if let pairingMessage {
                Text(pairingMessage)
                    .font(.caption2)
                    .foregroundColor(.red)
                    .textSelection(.enabled)
            }
        }
    }

    var pairingWindowSummary: String {
        guard let status = pairingStatus, status.pairing_enabled else {
            return Loc.t("settings.pairingClosed")
        }
        return "\(Loc.t("settings.waitingPairing")) · \(ConnectionsText.pairingWindow(remaining: status.remaining_seconds, failed: status.failed_attempts, maximum: status.max_failures))"
    }

    var pairingSheet: some View {
        VStack(spacing: 14) {
            Image(systemName: "lock.shield")
                .font(.system(size: 28))
                .foregroundColor(palette.accentColor)
            Text(pairingStatus?.peer?.hostname ?? Loc.t("settings.pairDevice"))
                .font(activeTheme.displayFont(size: 17, weight: .semibold))

            if let peer = pairingStatus?.peer {
                Text(peer.verification_code)
                    .font(.system(size: 34, weight: .bold, design: .monospaced))
                    .textSelection(.enabled)
                Text(Loc.t("settings.compareCode"))
                    .font(.caption)
                    .foregroundColor(palette.secondaryColor)
                    .multilineTextAlignment(.center)
                Text(peer.fingerprint)
                    .font(.caption2.monospaced())
                    .foregroundColor(palette.tertiaryColor)
                    .textSelection(.enabled)
                if pairingStatus?.phase == "finalizing" {
                    ProgressView(Loc.t("settings.pairingFinalizing"))
                        .controlSize(.small)
                } else if peer.local_confirmed {
                    Text(Loc.t("settings.waitingPeerConfirm"))
                        .font(.caption)
                        .foregroundColor(palette.accentColor)
                }
            } else if pairingInProgress || pairingStatus?.phase == "handshaking" {
                ProgressView(Loc.t("settings.secureHandshake"))
                    .controlSize(.small)
            } else {
                VStack(spacing: 7) {
                    Text(Loc.t("settings.waitingPairing"))
                        .font(.caption.weight(.medium))
                    Text(Loc.t("settings.pairingInstruction"))
                        .font(.caption2)
                        .foregroundColor(palette.secondaryColor)
                        .multilineTextAlignment(.center)
                }
            }

            if let message = pairingMessage
                ?? pairingStatus?.error.map(ApiError.pairingErrorDescription)
            {
                Text(message)
                    .font(.caption2)
                    .foregroundColor(.red)
                    .multilineTextAlignment(.center)
            }

            HStack(spacing: 10) {
                Button(Loc.t("settings.cancel")) { onCancel() }
                    .keyboardShortcut(.cancelAction)
                Button(
                    pairingStatus?.peer?.local_confirmed == true
                    ? Loc.t("settings.confirmed")
                    : Loc.t("settings.codesMatch")
                ) { onConfirm() }
                    .buttonStyle(.borderedProminent)
                    .disabled(
                        pairingInProgress
                        || pairingStatus?.phase == "finalizing"
                        || pairingStatus?.peer == nil
                        || pairingStatus?.peer?.local_confirmed == true
                    )
            }
        }
        .padding(24)
        .frame(width: 340)
        .frame(minHeight: 300)
        .background(palette.windowColor)
        .interactiveDismissDisabled(pairingStatus?.pairing_enabled == true)
    }
}
