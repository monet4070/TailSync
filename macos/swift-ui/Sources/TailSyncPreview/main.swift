import SwiftUI
import AppKit

final class PreviewAppDelegate: NSObject, NSApplicationDelegate {
    var window: NSWindow?

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.regular)
        let hosting = NSHostingController(rootView: MacSettingsPreviewView())
        let win = NSWindow(contentViewController: hosting)
        win.title = "TailSync 设置 - Mac 原型试用"
        win.styleMask = [.titled, .closable, .miniaturizable, .resizable]
        win.setContentSize(NSSize(width: 520, height: 640))
        win.center()
        win.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        self.window = win
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        true
    }
}

let app = NSApplication.shared
let delegate = PreviewAppDelegate()
app.delegate = delegate
app.run()
