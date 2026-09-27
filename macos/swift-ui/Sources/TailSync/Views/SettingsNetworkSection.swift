import AppKit
import SwiftUI

extension SettingsView {
    /// Lightweight navigation card row linking to the standalone Connections & Devices window.
    var networkSection: some View {
        settingsCard(title: Loc.t("settings.manageConnections")) {
            settingRow {
                VStack(alignment: .leading, spacing: 2) {
                    Text(Loc.t("settings.manageConnections"))
                        .foregroundColor(palette.primaryColor)
                    Text(Loc.t("settings.manageConnectionsDescription"))
                        .font(.caption2)
                        .foregroundColor(palette.tertiaryColor)
                        .fixedSize(horizontal: false, vertical: true)
                }
                Spacer(minLength: 16)
                Button(Loc.t("settings.openConnections")) {
                    AppDelegate.showConnections()
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
                .frame(minWidth: 54)
            }
        }
    }
}
