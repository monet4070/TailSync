import AppKit
import SwiftUI

extension ConnectionsView {
    func handleRemotePairingLink(_ link: String) {
        remotePairing.receive(link)
        inspectRemotePairingLink()
    }

    func createRemotePairingInvite() {
        remotePairingInProgress = true
        remotePairing.beginOperation()
        Task { @MainActor in
            do {
                remoteInvite = try await ApiClient.shared.createRemotePairingInvite()
                remoteInviteCopied = false
                pairingStatus = try? await ApiClient.shared.getPairingStatus()
            } catch {
                remotePairing.fail(error.localizedDescription)
            }
            remotePairingInProgress = false
        }
    }

    func inspectRemotePairingLink() {
        let link = remotePairing.link.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !link.isEmpty else {
            remotePairing.preview = nil
            return
        }
        remotePairingInProgress = true
        remotePairing.beginOperation()
        Task { @MainActor in
            do {
                remotePairing.preview = try await ApiClient.shared.inspectRemotePairingLink(link)
            } catch {
                remotePairing.preview = nil
                remotePairing.fail(error.localizedDescription)
            }
            remotePairingInProgress = false
        }
    }

    func startRemotePairing() {
        let link = remotePairing.link.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !link.isEmpty else { return }
        remotePairingInProgress = true
        remotePairing.beginOperation()
        Task { @MainActor in
            do {
                pairingStatus = try await ApiClient.shared.startRemotePairing(inviteLink: link)
                showPairingSheet = true
            } catch {
                remotePairing.fail(pairingErrorDescription(error))
                pairingStatus = try? await ApiClient.shared.getPairingStatus()
            }
            remotePairingInProgress = false
        }
    }

    func cancelRemotePairingInvite() {
        remotePairingInProgress = true
        remotePairing.beginOperation()
        Task { @MainActor in
            do {
                _ = try await ApiClient.shared.cancelRemotePairingInvite()
                remoteInvite = nil
            } catch {
                remotePairing.fail(error.localizedDescription)
            }
            remotePairingInProgress = false
        }
    }

    func copyRemotePairingInvite() {
        guard let invite = remoteInvite else { return }
        let pasteboard = NSPasteboard.general
        pasteboard.clearContents()
        guard pasteboard.setString(invite.link, forType: .string) else {
            remotePairing.fail(Loc.t("settings.remotePairingCopyFailed"))
            return
        }
        remoteInviteCopied = true
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) {
            remoteInviteCopied = false
        }
    }
}
