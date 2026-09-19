import Foundation
import ServiceManagement

enum LaunchAtLoginRegistrationState: Equatable {
    case notRegistered
    case enabled
    case requiresApproval
    case notFound
}

protocol LaunchAtLoginServicing: AnyObject {
    var status: LaunchAtLoginRegistrationState { get }
    func register() throws
    func unregister() throws
    func openSystemSettings()
}

final class SystemLaunchAtLoginService: LaunchAtLoginServicing {
    var status: LaunchAtLoginRegistrationState {
        switch SMAppService.mainApp.status {
        case .notRegistered: return .notRegistered
        case .enabled: return .enabled
        case .requiresApproval: return .requiresApproval
        case .notFound: return .notFound
        @unknown default: return .notFound
        }
    }

    func register() throws {
        try SMAppService.mainApp.register()
    }

    func unregister() throws {
        try SMAppService.mainApp.unregister()
    }

    func openSystemSettings() {
        SMAppService.openSystemSettingsLoginItems()
    }
}

/// Uses the system login-item registry as the single source of truth. No
/// duplicate preference is persisted, so the toggle also reflects changes
/// made in System Settings while TailSync is running.
final class LaunchAtLoginController: ObservableObject {
    @Published private(set) var state: LaunchAtLoginRegistrationState
    @Published private(set) var errorMessage: String?

    private let service: any LaunchAtLoginServicing

    init(service: any LaunchAtLoginServicing = SystemLaunchAtLoginService()) {
        self.service = service
        state = Self.userFacingState(for: service.status)
    }

    var isRequested: Bool {
        state == .enabled || state == .requiresApproval
    }

    var requiresApproval: Bool {
        state == .requiresApproval
    }

    func refresh() {
        state = Self.userFacingState(for: service.status)
    }

    func setEnabled(_ enabled: Bool) {
        errorMessage = nil
        do {
            if enabled {
                try service.register()
            } else {
                try service.unregister()
            }
        } catch {
            errorMessage = error.localizedDescription
        }
        refresh()
    }

    func openSystemSettings() {
        service.openSystemSettings()
    }

    /// For the main-app service, `.notFound` is the normal first-run state
    /// before Service Management has ever seen a registration. It must remain
    /// actionable so `register()` gets the chance to create that registration.
    private static func userFacingState(
        for state: LaunchAtLoginRegistrationState
    ) -> LaunchAtLoginRegistrationState {
        state == .notFound ? .notRegistered : state
    }
}
