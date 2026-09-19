import Foundation

struct PeerLatencyTestTarget: Equatable {
    let id: String
    let address: String
    let interface: String?
    let rttCapable: Bool
}

enum PeerLatencyTestPlan {
    /// The daemon already ranks and de-duplicates candidates in Core. Keep
    /// that order at the UI seam so the platform client does not maintain a
    /// second connection-priority table.
    static func orderedTargets(_ targets: [PeerLatencyTestTarget]) -> [PeerLatencyTestTarget] {
        var seen = Set<String>()
        return targets
            .filter { target in
                !target.address.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                    && (target.interface != "iroh" || target.rttCapable)
            }
            .filter { seen.insert($0.id).inserted }
    }
}
