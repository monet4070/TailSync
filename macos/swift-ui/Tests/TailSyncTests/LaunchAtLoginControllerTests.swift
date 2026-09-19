import Foundation
import XCTest
@testable import TailSync

@MainActor
final class LaunchAtLoginControllerTests: XCTestCase {
    func testEnablingAndDisablingTracksSystemRegistration() {
        let service = FakeLaunchAtLoginService(status: .notRegistered)
        let controller = LaunchAtLoginController(service: service)

        controller.setEnabled(true)
        XCTAssertEqual(controller.state, .enabled)
        XCTAssertTrue(controller.isRequested)
        XCTAssertEqual(service.registerCount, 1)

        controller.setEnabled(false)
        XCTAssertEqual(controller.state, .notRegistered)
        XCTAssertFalse(controller.isRequested)
        XCTAssertEqual(service.unregisterCount, 1)
    }

    func testApprovalStateStaysRequestedAndCanOpenSystemSettings() {
        let service = FakeLaunchAtLoginService(status: .requiresApproval)
        let controller = LaunchAtLoginController(service: service)

        XCTAssertTrue(controller.isRequested)
        XCTAssertTrue(controller.requiresApproval)
        controller.openSystemSettings()
        XCTAssertEqual(service.openSettingsCount, 1)

        controller.setEnabled(false)
        XCTAssertEqual(controller.state, .notRegistered)
        XCTAssertFalse(controller.isRequested)
    }

    func testNeverSeenServiceCanBeRegisteredFromNotFoundState() {
        let service = FakeLaunchAtLoginService(status: .notFound)
        let controller = LaunchAtLoginController(service: service)

        XCTAssertEqual(controller.state, .notRegistered)
        XCTAssertFalse(controller.isRequested)

        controller.setEnabled(true)

        XCTAssertEqual(service.registerCount, 1)
        XCTAssertEqual(controller.state, .enabled)
        XCTAssertTrue(controller.isRequested)
    }

    func testRegistrationFailureRollsBackToActualSystemState() {
        let service = FakeLaunchAtLoginService(status: .notRegistered)
        service.registerError = NSError(domain: "LaunchAtLoginTests", code: 7)
        let controller = LaunchAtLoginController(service: service)

        controller.setEnabled(true)

        XCTAssertEqual(controller.state, .notRegistered)
        XCTAssertFalse(controller.isRequested)
        XCTAssertNotNil(controller.errorMessage)
    }

    func testRegistrationFailureFromNeverSeenStateShowsTheRealError() {
        let service = FakeLaunchAtLoginService(status: .notFound)
        service.registerError = NSError(domain: "LaunchAtLoginTests", code: 8)
        let controller = LaunchAtLoginController(service: service)

        controller.setEnabled(true)

        XCTAssertEqual(controller.state, .notRegistered)
        XCTAssertFalse(controller.isRequested)
        XCTAssertNotNil(controller.errorMessage)
    }
}

private final class FakeLaunchAtLoginService: LaunchAtLoginServicing {
    var status: LaunchAtLoginRegistrationState
    var registerError: Error?
    var unregisterError: Error?
    private(set) var registerCount = 0
    private(set) var unregisterCount = 0
    private(set) var openSettingsCount = 0

    init(status: LaunchAtLoginRegistrationState) {
        self.status = status
    }

    func register() throws {
        registerCount += 1
        if let registerError { throw registerError }
        status = .enabled
    }

    func unregister() throws {
        unregisterCount += 1
        if let unregisterError { throw unregisterError }
        status = .notRegistered
    }

    func openSystemSettings() {
        openSettingsCount += 1
    }
}
