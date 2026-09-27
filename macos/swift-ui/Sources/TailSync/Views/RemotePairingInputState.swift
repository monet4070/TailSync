import Foundation

struct RemotePairingInputState {
    enum Feedback: Equatable {
        case valid(UInt64)
        case error(String)
    }

    var expanded = false
    var link = "" {
        didSet {
            if link != oldValue {
                preview = nil
                message = nil
            }
        }
    }
    var preview: ApiClient.RemotePairingInvitePreview?
    var message: String?

    var feedback: Feedback? {
        if let message { return .error(message) }
        if let preview { return .valid(preview.remaining_seconds) }
        return nil
    }

    mutating func receive(_ value: String) {
        expanded = true
        link = value.trimmingCharacters(in: .whitespacesAndNewlines)
        preview = nil
        message = nil
    }

    mutating func beginOperation() {
        preview = nil
        message = nil
    }

    mutating func fail(_ error: String) {
        preview = nil
        message = error
    }
}
