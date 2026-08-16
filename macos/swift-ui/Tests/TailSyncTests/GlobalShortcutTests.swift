import Carbon
import XCTest
@testable import TailSync

final class GlobalShortcutTests: XCTestCase {
    private func parsed(_ shortcut: String) throws -> ShortcutParser.Parsed {
        let result = ShortcutParser.parse(shortcut)
        guard case .success(let value) = result else {
            throw result.failure ?? ShortcutParser.Error.empty
        }
        return value
    }

    func testParsesTheDefaultCombination() throws {
        let sync = try parsed("CommandOrControl+Shift+S")
        XCTAssertEqual(sync.keyCode, UInt32(kVK_ANSI_S))
        XCTAssertEqual(sync.carbonModifiers, UInt32(cmdKey) | UInt32(shiftKey))

        let history = try parsed("CommandOrControl+Shift+H")
        XCTAssertEqual(history.keyCode, UInt32(kVK_ANSI_H))
        XCTAssertEqual(history.carbonModifiers, UInt32(cmdKey) | UInt32(shiftKey))
    }

    func testParsesDigitAndFunctionKeys() throws {
        let digit = try parsed("Control+Digit1")
        XCTAssertEqual(digit.keyCode, UInt32(kVK_ANSI_1))
        XCTAssertEqual(digit.carbonModifiers, UInt32(controlKey))

        let function = try parsed("Control+Shift+F5")
        XCTAssertEqual(function.keyCode, UInt32(kVK_F5))
        XCTAssertEqual(function.carbonModifiers, UInt32(controlKey) | UInt32(shiftKey))
    }

    func testRequiresAtLeastOneModifier() {
        guard case .failure(let error) = ShortcutParser.parse("KeyS") else {
            return XCTFail("Expected a modifier error")
        }
        XCTAssertEqual(error, .missingModifier)
    }

    func testRejectsMultipleMainKeys() {
        guard case .failure(let error) = ShortcutParser.parse("KeyA+KeyB") else {
            return XCTFail("Expected a multi-key error")
        }
        XCTAssertEqual(error, .multipleMainKeys)
    }

    func testRejectsUnsupportedKeysAndModifiers() {
        XCTAssertEqual(ShortcutParser.parse("Control+Insert"), .failure(.unsupportedKey("Insert")))
        XCTAssertEqual(ShortcutParser.parse("Control+Super+KeyQ"), .failure(.unsupportedModifier("Super")))
    }

    func testCapturedKeysRoundTripThroughTheParser() throws {
        XCTAssertEqual(ShortcutParser.keyCodeName(for: UInt32(kVK_ANSI_S)), "KeyS")
        let name = ShortcutParser.keyCodeName(for: UInt32(kVK_ANSI_S))!
        let captured = "CommandOrControl+Shift+\(name)"
        let value = try parsed(captured)
        XCTAssertEqual(value.keyCode, UInt32(kVK_ANSI_S))
    }

    func testNumpadEightAndNineUseTheirCarbonKeyCodes() throws {
        XCTAssertEqual(try parsed("Control+Numpad8").keyCode, UInt32(kVK_ANSI_Keypad8))
        XCTAssertEqual(try parsed("Control+Numpad9").keyCode, UInt32(kVK_ANSI_Keypad9))
        XCTAssertEqual(ShortcutParser.keyCodeName(for: UInt32(kVK_ANSI_Keypad8)), "Numpad8")
        XCTAssertEqual(ShortcutParser.keyCodeName(for: UInt32(kVK_ANSI_Keypad9)), "Numpad9")
    }

    func testFormatsStoredShortcutsUsingMacKeyboardSymbols() {
        XCTAssertEqual(ShortcutDisplayFormatter.string(for: "Alt+Slash"), "⌥/")
        XCTAssertEqual(
            ShortcutDisplayFormatter.string(for: "CommandOrControl+Shift+KeyS"),
            "⇧⌘S"
        )
        XCTAssertEqual(
            ShortcutDisplayFormatter.string(for: "Shift+Option+Control+ArrowLeft"),
            "⌃⌥⇧←"
        )
    }

    func testApplyRestoresThePreviousShortcutWhenRegistrationConflicts() async {
        var calls: [String] = []
        let register: (String) -> Result<Void, ShortcutError> = { shortcut in
            calls.append(shortcut)
            if shortcut == "CommandOrControl+Shift+K" {
                return .failure(ShortcutError(message: "already in use"))
            }
            return .success(())
        }
        let error = await GlobalShortcutController.apply(
            previous: "CommandOrControl+Shift+S",
            next: "CommandOrControl+Shift+K",
            register: register,
            persist: { _ in true }
        )
        XCTAssertEqual(error, "already in use")
        XCTAssertEqual(calls, ["CommandOrControl+Shift+K", "CommandOrControl+Shift+S"])
    }

    func testApplyRestoresThePreviousShortcutWhenPersistenceFails() async {
        var calls: [String] = []
        let register: (String) -> Result<Void, ShortcutError> = { shortcut in
            calls.append(shortcut)
            return .success(())
        }
        let error = await GlobalShortcutController.apply(
            previous: "CommandOrControl+Shift+S",
            next: "CommandOrControl+Shift+K",
            register: register,
            persist: { _ in false }
        )
        XCTAssertEqual(error, "Could not save the shortcut setting")
        XCTAssertEqual(calls, ["CommandOrControl+Shift+K", "CommandOrControl+Shift+S"])
    }

    func testApplyMentionsRestoreFailures() async {
        let register: (String) -> Result<Void, ShortcutError> = { shortcut in
            if shortcut == "CommandOrControl+Shift+K" { return .success(()) }
            return .failure(ShortcutError(message: "previous no longer available"))
        }
        let error = await GlobalShortcutController.apply(
            previous: "CommandOrControl+Shift+S",
            next: "CommandOrControl+Shift+K",
            register: register,
            persist: { _ in false }
        )
        XCTAssertNotNil(error)
        XCTAssertTrue(error?.contains("Could not save the shortcut setting") == true)
        XCTAssertTrue(error?.contains("previous no longer available") == true)
    }

    func testApplySucceedsAndRegistersOnlyTheNewShortcut() async {
        var calls: [String] = []
        let register: (String) -> Result<Void, ShortcutError> = { shortcut in
            calls.append(shortcut)
            return .success(())
        }
        let error = await GlobalShortcutController.apply(
            previous: "CommandOrControl+Shift+S",
            next: "CommandOrControl+Shift+K",
            register: register,
            persist: { _ in true }
        )
        XCTAssertNil(error)
        XCTAssertEqual(calls, ["CommandOrControl+Shift+K"])
    }

    func testApplyReregistersAnUnchangedShortcutWithoutPersisting() async {
        var calls: [String] = []
        let register: (String) -> Result<Void, ShortcutError> = { shortcut in
            calls.append(shortcut)
            return .success(())
        }
        let error = await GlobalShortcutController.apply(
            previous: "CommandOrControl+Shift+S",
            next: "CommandOrControl+Shift+S",
            register: register,
            persist: { _ in
                XCTFail("persist must not run for an unchanged shortcut")
                return true
            }
        )
        XCTAssertNil(error)
        XCTAssertEqual(calls, ["CommandOrControl+Shift+S"])
    }
}

private extension Result {
    var failure: Failure? {
        if case .failure(let failure) = self { return failure }
        return nil
    }
}
