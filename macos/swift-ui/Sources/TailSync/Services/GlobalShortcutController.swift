import AppKit
import Carbon

/// A human-readable shortcut registration failure.
struct ShortcutError: Error, Equatable {
    let message: String
}

/// Parses TailSync shortcut strings (e.g. "CommandOrControl+Shift+S") into
/// Carbon hot key values. Kept as a pure type so it can be unit tested
/// without touching Carbon's event machinery.
enum ShortcutParser {
    struct Parsed: Equatable {
        let keyCode: UInt32
        let carbonModifiers: UInt32
    }

    enum Error: Swift.Error, Equatable {
        case empty
        case missingModifier
        case multipleMainKeys
        case unsupportedKey(String)
        case unsupportedModifier(String)
    }

    static let modifiers: [String: UInt32] = [
        "Command": UInt32(cmdKey),
        "CommandOrControl": UInt32(cmdKey),
        "Control": UInt32(controlKey),
        "Shift": UInt32(shiftKey),
        "Alt": UInt32(optionKey),
        "Option": UInt32(optionKey),
    ]

    /// Modifier names the Windows recorder can produce that have no macOS
    /// equivalent (there is no Windows/Super key), reported explicitly.
    static let unsupportedModifiers: Set<String> = ["Super", "Win"]

    static let keyCodes: [String: UInt32] = {
        var map: [String: UInt32] = [:]
        let letters: [(String, UInt32)] = [
            ("KeyA", UInt32(kVK_ANSI_A)), ("KeyS", UInt32(kVK_ANSI_S)),
            ("KeyD", UInt32(kVK_ANSI_D)), ("KeyF", UInt32(kVK_ANSI_F)),
            ("KeyH", UInt32(kVK_ANSI_H)), ("KeyG", UInt32(kVK_ANSI_G)),
            ("KeyZ", UInt32(kVK_ANSI_Z)), ("KeyX", UInt32(kVK_ANSI_X)),
            ("KeyC", UInt32(kVK_ANSI_C)), ("KeyV", UInt32(kVK_ANSI_V)),
            ("KeyB", UInt32(kVK_ANSI_B)), ("KeyQ", UInt32(kVK_ANSI_Q)),
            ("KeyW", UInt32(kVK_ANSI_W)), ("KeyE", UInt32(kVK_ANSI_E)),
            ("KeyR", UInt32(kVK_ANSI_R)), ("KeyY", UInt32(kVK_ANSI_Y)),
            ("KeyT", UInt32(kVK_ANSI_T)), ("KeyO", UInt32(kVK_ANSI_O)),
            ("KeyU", UInt32(kVK_ANSI_U)), ("KeyI", UInt32(kVK_ANSI_I)),
            ("KeyP", UInt32(kVK_ANSI_P)), ("KeyL", UInt32(kVK_ANSI_L)),
            ("KeyJ", UInt32(kVK_ANSI_J)), ("KeyK", UInt32(kVK_ANSI_K)),
            ("KeyN", UInt32(kVK_ANSI_N)), ("KeyM", UInt32(kVK_ANSI_M)),
        ]
        for (name, code) in letters { map[name] = code }
        let digits: [(String, UInt32)] = [
            ("Digit1", UInt32(kVK_ANSI_1)), ("Digit2", UInt32(kVK_ANSI_2)),
            ("Digit3", UInt32(kVK_ANSI_3)), ("Digit4", UInt32(kVK_ANSI_4)),
            ("Digit5", UInt32(kVK_ANSI_5)), ("Digit6", UInt32(kVK_ANSI_6)),
            ("Digit7", UInt32(kVK_ANSI_7)), ("Digit8", UInt32(kVK_ANSI_8)),
            ("Digit9", UInt32(kVK_ANSI_9)), ("Digit0", UInt32(kVK_ANSI_0)),
        ]
        for (name, code) in digits { map[name] = code }
        let functions: [(String, UInt32)] = [
            ("F1", UInt32(kVK_F1)), ("F2", UInt32(kVK_F2)), ("F3", UInt32(kVK_F3)),
            ("F4", UInt32(kVK_F4)), ("F5", UInt32(kVK_F5)), ("F6", UInt32(kVK_F6)),
            ("F7", UInt32(kVK_F7)), ("F8", UInt32(kVK_F8)), ("F9", UInt32(kVK_F9)),
            ("F10", UInt32(kVK_F10)), ("F11", UInt32(kVK_F11)), ("F12", UInt32(kVK_F12)),
            ("F13", UInt32(kVK_F13)), ("F14", UInt32(kVK_F14)), ("F15", UInt32(kVK_F15)),
            ("F16", UInt32(kVK_F16)), ("F17", UInt32(kVK_F17)), ("F18", UInt32(kVK_F18)),
            ("F19", UInt32(kVK_F19)), ("F20", UInt32(kVK_F20)),
        ]
        for (name, code) in functions { map[name] = code }
        let keys: [(String, UInt32)] = [
            ("Backquote", UInt32(kVK_ANSI_Grave)), ("Backslash", UInt32(kVK_ANSI_Backslash)),
            ("BracketLeft", UInt32(kVK_ANSI_LeftBracket)),
            ("BracketRight", UInt32(kVK_ANSI_RightBracket)),
            ("Comma", UInt32(kVK_ANSI_Comma)), ("Equal", UInt32(kVK_ANSI_Equal)),
            ("Minus", UInt32(kVK_ANSI_Minus)), ("Period", UInt32(kVK_ANSI_Period)),
            ("Quote", UInt32(kVK_ANSI_Quote)), ("Semicolon", UInt32(kVK_ANSI_Semicolon)),
            ("Slash", UInt32(kVK_ANSI_Slash)),
            ("Space", UInt32(kVK_Space)), ("Enter", UInt32(kVK_Return)),
            ("Tab", UInt32(kVK_Tab)), ("Backspace", UInt32(kVK_Delete)),
            ("Delete", UInt32(kVK_ForwardDelete)), ("Escape", UInt32(kVK_Escape)),
            ("CapsLock", UInt32(kVK_CapsLock)),
            ("ArrowLeft", UInt32(kVK_LeftArrow)), ("ArrowRight", UInt32(kVK_RightArrow)),
            ("ArrowUp", UInt32(kVK_UpArrow)), ("ArrowDown", UInt32(kVK_DownArrow)),
            ("Home", UInt32(kVK_Home)), ("End", UInt32(kVK_End)),
            ("PageUp", UInt32(kVK_PageUp)), ("PageDown", UInt32(kVK_PageDown)),
            ("NumpadAdd", UInt32(kVK_ANSI_KeypadPlus)),
            ("NumpadSubtract", UInt32(kVK_ANSI_KeypadMinus)),
            ("NumpadMultiply", UInt32(kVK_ANSI_KeypadMultiply)),
            ("NumpadDivide", UInt32(kVK_ANSI_KeypadDivide)),
            ("NumpadDecimal", UInt32(kVK_ANSI_KeypadDecimal)),
            ("NumpadEnter", UInt32(kVK_ANSI_KeypadEnter)),
            ("NumpadEqual", UInt32(kVK_ANSI_KeypadEquals)),
        ]
        for (name, code) in keys { map[name] = code }
        let numpadDigits: [(String, UInt32)] = [
            ("Numpad0", UInt32(kVK_ANSI_Keypad0)),
            ("Numpad1", UInt32(kVK_ANSI_Keypad1)),
            ("Numpad2", UInt32(kVK_ANSI_Keypad2)),
            ("Numpad3", UInt32(kVK_ANSI_Keypad3)),
            ("Numpad4", UInt32(kVK_ANSI_Keypad4)),
            ("Numpad5", UInt32(kVK_ANSI_Keypad5)),
            ("Numpad6", UInt32(kVK_ANSI_Keypad6)),
            ("Numpad7", UInt32(kVK_ANSI_Keypad7)),
            ("Numpad8", UInt32(kVK_ANSI_Keypad8)),
            ("Numpad9", UInt32(kVK_ANSI_Keypad9)),
        ]
        for (name, code) in numpadDigits { map[name] = code }
        return map
    }()

    /// Canonical code name for a physical key code, used when capturing a
    /// shortcut so the stored string round-trips through `keyCodes`.
    static func keyCodeName(for keyCode: UInt32) -> String? {
        for (name, code) in keyCodes where code == keyCode {
            return name
        }
        return nil
    }

    /// Resolve a main-key name to a key code, accepting both the physical
    /// names ("KeyS", "Digit1") and the layout characters the default and
    /// hand-edited shortcuts use ("S", "1").
    static func keyCodeForName(_ name: String) -> UInt32? {
        if let code = keyCodes[name] { return code }
        if name.count == 1, let scalar = name.unicodeScalars.first?.value {
            if (48...57).contains(scalar) {
                return keyCodes["Digit\(name)"]
            }
            if (65...90).contains(scalar) {
                return keyCodes["Key\(name)"]
            }
        }
        return nil
    }

    static func parse(_ shortcut: String) -> Result<Parsed, Error> {
        let trimmed = shortcut.trimmingCharacters(in: .whitespaces)
        guard !trimmed.isEmpty else { return .failure(.empty) }
        let parts = trimmed.split(separator: "+").map(String.init)
        var carbonModifiers: UInt32 = 0
        var mainKey: String?
        for part in parts {
            if let modifier = modifiers[part] {
                carbonModifiers |= modifier
            } else if unsupportedModifiers.contains(part) {
                return .failure(.unsupportedModifier(part))
            } else if mainKey == nil {
                mainKey = part
            } else {
                return .failure(.multipleMainKeys)
            }
        }
        guard let key = mainKey else { return .failure(.empty) }
        guard carbonModifiers != 0 else { return .failure(.missingModifier) }
        guard let keyCode = keyCodeForName(key) else {
            return .failure(.unsupportedKey(key))
        }
        return .success(Parsed(keyCode: keyCode, carbonModifiers: carbonModifiers))
    }
}

/// Converts the cross-platform stored shortcut into macOS keyboard notation.
/// Storage remains unchanged so settings continue to round-trip with Windows.
enum ShortcutDisplayFormatter {
    private static let modifierNames: Set<String> = [
        "Command", "CommandOrControl", "Control", "Shift", "Alt", "Option",
    ]

    private static let keyLabels: [String: String] = [
        "Backquote": "`", "Backslash": "\\", "BracketLeft": "[", "BracketRight": "]",
        "Comma": ",", "Equal": "=", "Minus": "-", "Period": ".", "Quote": "'",
        "Semicolon": ";", "Slash": "/", "Space": "Space", "Enter": "↩",
        "Tab": "⇥", "Backspace": "⌫", "Delete": "⌦", "Escape": "⎋",
        "CapsLock": "⇪", "ArrowLeft": "←", "ArrowRight": "→", "ArrowUp": "↑",
        "ArrowDown": "↓", "Home": "↖", "End": "↘", "PageUp": "⇞", "PageDown": "⇟",
        "NumpadAdd": "Num +", "NumpadSubtract": "Num -", "NumpadMultiply": "Num ×",
        "NumpadDivide": "Num /", "NumpadDecimal": "Num .", "NumpadEnter": "Num ↩",
        "NumpadEqual": "Num =",
    ]

    static func string(for shortcut: String) -> String {
        let parts = shortcut
            .trimmingCharacters(in: .whitespaces)
            .split(separator: "+")
            .map(String.init)
        guard !parts.isEmpty else { return "" }

        let tokens = Set(parts)
        var result = ""
        if tokens.contains("Control") { result += "⌃" }
        if tokens.contains("Alt") || tokens.contains("Option") { result += "⌥" }
        if tokens.contains("Shift") { result += "⇧" }
        if tokens.contains("Command") || tokens.contains("CommandOrControl") { result += "⌘" }

        let keys = parts.filter { !modifierNames.contains($0) }.map(keyLabel)
        result += keys.joined(separator: "+")
        return result
    }

    private static func keyLabel(_ name: String) -> String {
        if let label = keyLabels[name] { return label }
        if name.hasPrefix("Key"), name.count == 4 { return String(name.suffix(1)) }
        if name.hasPrefix("Digit"), name.count == 6 { return String(name.suffix(1)) }
        if name.hasPrefix("Numpad"), name.count == 7 { return "Num \(name.suffix(1))" }
        return name
    }
}

/// Registers the sync global shortcut with Carbon's hot key API, avoiding the
/// Accessibility permission that NSEvent global monitors would require. Only
/// one hot key is registered at a time.
final class GlobalShortcutController {
    static let shared = GlobalShortcutController()
    static let syncStateChanged = Notification.Name("TailSyncSyncStateChanged")

    /// Fired on the main thread when the registered hot key is pressed.
    var onActivate: (() -> Void)?

    private var hotKeyRef: EventHotKeyRef?
    private var eventHandlerRef: EventHandlerRef?
    private var eventHandlerError: ShortcutError?
    private let hotKeySignature: UInt32 = 0x54415359 // 'TASY'

    private init() {
        installEventHandler()
    }

    func register(shortcut: String) -> Result<Void, ShortcutError> {
        unregister()
        guard !shortcut.isEmpty else { return .success(()) }
        if let eventHandlerError { return .failure(eventHandlerError) }
        let parsed: ShortcutParser.Parsed
        switch ShortcutParser.parse(shortcut) {
        case .success(let value): parsed = value
        case .failure(let error):
            return .failure(ShortcutError(message: "Invalid shortcut: \(error)"))
        }
        let id = EventHotKeyID(signature: hotKeySignature, id: 1)
        var ref: EventHotKeyRef?
        let status = RegisterEventHotKey(
            parsed.keyCode,
            parsed.carbonModifiers,
            id,
            GetApplicationEventTarget(),
            0,
            &ref
        )
        guard status == noErr, let ref else {
            return .failure(
                ShortcutError(
                    message: "Could not register shortcut (it may already be in use): OSStatus \(status)"
                )
            )
        }
        hotKeyRef = ref
        return .success(())
    }

    func unregister() {
        guard let hotKeyRef else { return }
        UnregisterEventHotKey(hotKeyRef)
        self.hotKeyRef = nil
    }

    /// Apply a shortcut change as a transaction: register the next shortcut,
    /// then persist it, restoring the previous shortcut if either step fails.
    /// Returns the original failure, with any restore failure appended.
    static func apply(
        previous: String,
        next: String,
        register: (String) -> Result<Void, ShortcutError>,
        persist: (String) async -> Bool
    ) async -> String? {
        if next == previous {
            if case .failure(let error) = register(next) { return error.message }
            return nil
        }
        if case .failure(let error) = register(next) {
            return rollback(previous: previous, register: register, originalError: error.message)
        }
        if !(await persist(next)) {
            return rollback(
                previous: previous,
                register: register,
                originalError: "Could not save the shortcut setting"
            )
        }
        return nil
    }

    private static func rollback(
        previous: String,
        register: (String) -> Result<Void, ShortcutError>,
        originalError: String
    ) -> String {
        switch register(previous) {
        case .success: return originalError
        case .failure(let restoreError):
            return "\(originalError); additionally could not restore the previous shortcut: \(restoreError.message)"
        }
    }

    private func installEventHandler() {
        var eventType = EventTypeSpec(
            eventClass: OSType(kEventClassKeyboard),
            eventKind: UInt32(kEventHotKeyPressed)
        )
        let status = InstallEventHandler(
            GetApplicationEventTarget(),
            { _, event, userData -> OSStatus in
                guard let userData else { return noErr }
                let controller = Unmanaged<GlobalShortcutController>
                    .fromOpaque(userData).takeUnretainedValue()
                controller.handleHotKeyPressed(event)
                return noErr
            },
            1,
            &eventType,
            Unmanaged.passUnretained(self).toOpaque(),
            &eventHandlerRef
        )
        guard status == noErr, eventHandlerRef != nil else {
            eventHandlerError = ShortcutError(
                message: "Could not install the global shortcut event handler: OSStatus \(status)"
            )
            return
        }
        eventHandlerError = nil
    }

    private func handleHotKeyPressed(_ event: EventRef?) {
        var hotKeyID = EventHotKeyID()
        let status = GetEventParameter(
            event,
            EventParamName(kEventParamDirectObject),
            EventParamType(typeEventHotKeyID),
            nil,
            MemoryLayout<EventHotKeyID>.size,
            nil,
            &hotKeyID
        )
        guard status == noErr,
              hotKeyID.signature == hotKeySignature,
              hotKeyID.id == 1 else { return }
        DispatchQueue.main.async { [weak self] in
            self?.onActivate?()
        }
    }
}
