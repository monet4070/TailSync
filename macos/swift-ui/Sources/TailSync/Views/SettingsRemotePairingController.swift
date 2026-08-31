import AppKit
import SwiftUI

extension SettingsView {
    func handleRemotePairingLink(_ link: String) {
        remoteInviteLink = link.trimmingCharacters(in: .whitespacesAndNewlines)
        remoteInvitePreview = nil
        remotePairingMessage = nil
        inspectRemotePairingLink()
    }

    func createRemotePairingInvite() {
        remotePairingInProgress = true
        remotePairingMessage = nil
        Task { @MainActor in
            do {
                remoteInvite = try await ApiClient.shared.createRemotePairingInvite()
                remoteInviteCopied = false
                pairingStatus = try? await ApiClient.shared.getPairingStatus()
            } catch {
                remotePairingMessage = error.localizedDescription
            }
            remotePairingInProgress = false
        }
    }

    func inspectRemotePairingLink() {
        let link = remoteInviteLink.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !link.isEmpty else {
            remoteInvitePreview = nil
            return
        }
        remotePairingInProgress = true
        remotePairingMessage = nil
        Task { @MainActor in
            do {
                remoteInvitePreview = try await ApiClient.shared.inspectRemotePairingLink(link)
            } catch {
                remoteInvitePreview = nil
                remotePairingMessage = error.localizedDescription
            }
            remotePairingInProgress = false
        }
    }

    func startRemotePairing() {
        let link = remoteInviteLink.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !link.isEmpty else { return }
        remotePairingInProgress = true
        remotePairingMessage = nil
        Task { @MainActor in
            do {
                pairingStatus = try await ApiClient.shared.startRemotePairing(inviteLink: link)
                showPairingSheet = true
            } catch {
                remotePairingMessage = pairingErrorDescription(error)
                pairingStatus = try? await ApiClient.shared.getPairingStatus()
            }
            remotePairingInProgress = false
        }
    }

    func cancelRemotePairingInvite() {
        remotePairingInProgress = true
        remotePairingMessage = nil
        Task { @MainActor in
            do {
                _ = try await ApiClient.shared.cancelRemotePairingInvite()
                remoteInvite = nil
            } catch {
                remotePairingMessage = error.localizedDescription
            }
            remotePairingInProgress = false
        }
    }

    func copyRemotePairingInvite() {
        guard let invite = remoteInvite else { return }
        let pasteboard = NSPasteboard.general
        pasteboard.clearContents()
        guard pasteboard.setString(invite.link, forType: .string) else {
            remotePairingMessage = Loc.t("settings.remotePairingCopyFailed")
            return
        }
        remoteInviteCopied = true
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) {
            remoteInviteCopied = false
        }
    }
}
