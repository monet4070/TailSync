import AppKit
import SwiftUI

extension ConnectionsView {
    func handleRemotePairingLink(_ link: String) {
        remotePairing.receive(link)
        inspectRemotePairingLink()
    }

    func createRemotePairingInvite() {
        let operation = remotePairing.beginOperation()
        Task { @MainActor in
            defer { remotePairing.finishOperation(operation) }
            do {
                let invite = try await ApiClient.shared.createRemotePairingInvite()
                guard remotePairing.isCurrentOperation(operation) else { return }
                remoteInvite = invite
                remoteInviteCopied = false
                let status = try? await ApiClient.shared.getPairingStatus()
                guard remotePairing.isCurrentOperation(operation) else { return }
                pairingStatus = status
            } catch {
                guard remotePairing.isCurrentOperation(operation) else { return }
                remotePairing.fail(error, for: operation)
            }
        }
    }

    func inspectRemotePairingLink() {
        let link = remotePairing.link.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !link.isEmpty else {
            remotePairing.preview = nil
            return
        }
        let operation = remotePairing.beginOperation()
        Task { @MainActor in
            defer { remotePairing.finishOperation(operation) }
            do {
                let preview = try await ApiClient.shared.inspectRemotePairingLink(link)
                guard remotePairing.isCurrentOperation(operation) else { return }
                remotePairing.preview = preview
            } catch {
                guard remotePairing.isCurrentOperation(operation) else { return }
                remotePairing.preview = nil
                remotePairing.fail(error, for: operation)
            }
        }
    }

    func startRemotePairing() {
        let link = remotePairing.link.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !link.isEmpty else { return }
        let operation = remotePairing.beginOperation()
        Task { @MainActor in
            defer { remotePairing.finishOperation(operation) }
            do {
                let status = try await ApiClient.shared.startRemotePairing(inviteLink: link)
                guard remotePairing.isCurrentOperation(operation) else { return }
                pairingStatus = status
                showPairingSheet = true
            } catch {
                guard remotePairing.isCurrentOperation(operation) else { return }
                remotePairing.fail(error, for: operation)
                let status = try? await ApiClient.shared.getPairingStatus()
                guard remotePairing.isCurrentOperation(operation) else { return }
                pairingStatus = status
            }
        }
    }

    func cancelRemotePairingInvite() {
        let operation = remotePairing.beginOperation()
        Task { @MainActor in
            defer { remotePairing.finishOperation(operation) }
            do {
                _ = try await ApiClient.shared.cancelRemotePairingInvite()
                guard remotePairing.isCurrentOperation(operation) else { return }
                remoteInvite = nil
            } catch {
                guard remotePairing.isCurrentOperation(operation) else { return }
                remotePairing.fail(error, for: operation)
            }
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
