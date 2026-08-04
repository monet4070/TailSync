import AppKit
import Foundation

// ═══════════════════════════════════════════════════════════════════
// TailSync macOS Tray Helper
//
// Apple's NSStatusItem handles left/right click natively:
//   • Left  click → button.action fires   → TCP to main app
//   • Right click → NSMenu shows           → native Cocoa menu
//
// Communication with the Tauri app is via a plain TCP connection
// to 127.0.0.1:<port>.  One short-lived connection per command.
// ═══════════════════════════════════════════════════════════════════

// ── Configuration ─────────────────────────────────────────────────
let CMD_PORT: UInt16 = 19889
let ICON_SIZE = NSSize(width: 18, height: 18)
let API_TOKEN = ProcessInfo.processInfo.environment["TAILSYNC_API_TOKEN"] ?? ""

/// Parse `--lang zh-CN` from command-line args.
func parseLanguage() -> String {
    let args = CommandLine.arguments
    if let idx = args.firstIndex(of: "--lang"), idx + 1 < args.count {
        return args[idx + 1]
    }
    return "en"
}
let LANG = parseLanguage()

// ── Helpers ───────────────────────────────────────────────────────

/// Resolve the icon path relative to the executable.
/// In dev:   target/debug/tray-helper  →  icons are at  src-tauri/icons/
/// In prod:  TrayHelper inside .app bundle → icons in Resources/
func resolveIconPath(_ name: String) -> String? {
    let candidates = [
        // Production: inside .app bundle
        Bundle.main.path(forResource: name, ofType: nil),
        Bundle.main.path(forResource: name, ofType: nil, inDirectory: "Resources"),
        // Development: relative to CWD
        "src-tauri/icons/\(name)",
        "../src-tauri/icons/\(name)",
        // Development: relative to executable
        URL(fileURLWithPath: CommandLine.arguments[0])
            .deletingLastPathComponent()
            .appendingPathComponent("../../../src-tauri/icons/\(name)")
            .path,
    ]
    for c in candidates {
        if let c, FileManager.default.fileExists(atPath: c) {
            return c
        }
    }
    return nil
}

/// Resize with rounded corners (menu bar style), keeping original colors.
func trayIcon(_ image: NSImage) -> NSImage {
    let size = ICON_SIZE
    let result = NSImage(size: size)
    result.lockFocus()
    let rect = NSRect(origin: .zero, size: size)
    let path = NSBezierPath(roundedRect: rect, xRadius: 3, yRadius: 3)
    path.addClip()
    image.draw(in: rect, from: .zero, operation: .sourceOver, fraction: 1.0)
    result.unlockFocus()
    return result
}

/// Send a JSON command to the Tauri app via TCP.
func sendCommand(_ cmd: String, id: Int64? = nil) {
    DispatchQueue.global().async {
        let sock = socket(AF_INET, SOCK_STREAM, 0)
        guard sock >= 0 else { return }
        defer { close(sock) }

        var addr = sockaddr_in()
        addr.sin_family = sa_family_t(AF_INET)
        addr.sin_port = CFSwapInt16HostToBig(CMD_PORT)
        addr.sin_addr.s_addr = inet_addr("127.0.0.1")

        var timeout = timeval(tv_sec: 2, tv_usec: 0)
        setsockopt(sock, SOL_SOCKET, SO_SNDTIMEO, &timeout, socklen_t(MemoryLayout<timeval>.size))

        let connected = withUnsafePointer(to: &addr) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                connect(sock, $0, socklen_t(MemoryLayout<sockaddr_in>.size))
            }
        }
        guard connected == 0 else { return }

        // Build JSON payload
        var payload = "{\"cmd\":\"\(cmd)\",\"token\":\"\(API_TOKEN)\""
        if let id = id { payload += ",\"id\":\(id)" }
        payload += "}\n"
        _ = payload.withCString { send(sock, $0, strlen($0), 0) }
    }
}

// ── NSApplication Delegate ────────────────────────────────────────

class AppDelegate: NSObject, NSApplicationDelegate {
    private let statusItem: NSStatusItem
    private let menu: NSMenu

    override init() {
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        menu = NSMenu()

        super.init()
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        setupStatusItem()
    }

    private func setupStatusItem() {
        // --- Icon ---
        if let iconPath = resolveIconPath("icon.png"),
           let image = NSImage(contentsOfFile: iconPath) {
            statusItem.button?.image = trayIcon(image)
        } else {
            statusItem.button?.image = NSImage(
                systemSymbolName: "doc.on.clipboard",
                accessibilityDescription: "TailSync"
            )
        }

        // --- Build the menu (used on right-click only) ---
        let isZh = LANG.hasPrefix("zh")
        let historyItem = NSMenuItem(
            title: isZh ? "历史记录" : "History",
            action: #selector(openHistory),
            keyEquivalent: ""
        )
        historyItem.target = self
        menu.addItem(historyItem)

        let settingsItem = NSMenuItem(
            title: isZh ? "设置" : "Settings",
            action: #selector(openSettings),
            keyEquivalent: ""
        )
        settingsItem.target = self
        menu.addItem(settingsItem)

        menu.addItem(.separator())

        let quitItem = NSMenuItem(
            title: isZh ? "退出 TailSync" : "Quit TailSync",
            action: #selector(quitApp),
            keyEquivalent: "q"
        )
        quitItem.target = self
        menu.addItem(quitItem)

        // --- Unified click handler — we distinguish left vs right ---
        // Do NOT set statusItem.menu permanently — that would force the
        // menu to appear on *any* click.  Instead we set it on-the-fly
        // inside the handler for right-click only.
        statusItem.button?.target = self
        statusItem.button?.action = #selector(handleClick)
        statusItem.button?.sendAction(on: [.leftMouseUp, .rightMouseUp])
    }

    // ── Actions ──────────────────────────────────────────────────

    /// Called on every mouse-up (left or right) because we set
    /// `sendAction(on: [.leftMouseUp, .rightMouseUp])`.
    @objc private func handleClick() {
        guard let event = NSApp.currentEvent else { return }

        if event.type == .rightMouseUp {
            // Right-click → show the menu.  We temporarily set the menu
            // so that performClick shows it, then clear it so left-clicks
            // continue to fire handleClick instead.
            statusItem.menu = menu
            statusItem.button?.performClick(nil)
            statusItem.menu = nil
        } else {
            // Left-click → send command to Tauri
            sendCommand("HISTORY")
        }
    }

    @objc private func openHistory() {
        sendCommand("HISTORY")
    }

    @objc private func openSettings() {
        sendCommand("SETTINGS")
    }

    @objc private func quitApp() {
        sendCommand("QUIT")
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) {
            NSApp.terminate(nil)
        }
    }
}

// ── Entry Point ───────────────────────────────────────────────────

let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.setActivationPolicy(.accessory)  // no Dock icon
app.run()
