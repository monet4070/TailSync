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
                invalidateOperation()
                preview = nil
                message = nil
            }
        }
    }
    var preview: ApiClient.RemotePairingInvitePreview?
    var message: String?
    private(set) var operationGeneration: UInt64 = 0
    private(set) var operationInProgress = false

    var feedback: Feedback? {
        if let message { return .error(message) }
        if let preview { return .valid(preview.remaining_seconds) }
        return nil
    }

    mutating func receive(_ value: String) {
        invalidateOperation()
        expanded = true
        link = value.trimmingCharacters(in: .whitespacesAndNewlines)
        preview = nil
        message = nil
    }

    @discardableResult
    mutating func beginOperation() -> UInt64 {
        operationGeneration &+= 1
        operationInProgress = true
        preview = nil
        message = nil
        return operationGeneration
    }

    func isCurrentOperation(_ generation: UInt64) -> Bool {
        operationGeneration == generation
    }

    mutating func finishOperation(_ generation: UInt64) {
        guard isCurrentOperation(generation) else { return }
        operationInProgress = false
    }

    private mutating func invalidateOperation() {
        operationGeneration &+= 1
        operationInProgress = false
    }

    mutating func fail(_ error: String) {
        preview = nil
        message = error
    }

    /// Typed-aware failure: a finite stable code wins over the legacy text so
    /// the remote panel matches the pairing sheet's presentation.
    mutating func fail(_ error: Error) {
        fail(PairingFailurePresentation.message(for: error))
    }

    mutating func fail(_ error: Error, for generation: UInt64) {
        guard isCurrentOperation(generation) else { return }
        fail(error)
    }
}
