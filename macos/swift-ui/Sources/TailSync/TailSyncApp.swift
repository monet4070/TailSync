import SwiftUI
import AppKit
import UserNotifications
import Carbon
import Darwin

private func menuRouteInterfaceLabel(_ interface: String) -> String {
    switch interface {
    case "lan": return "LAN"
    case "iroh": return "Iroh"
    default: return "Tailscale"
    }
}

enum TailSyncWindowPolicy {
    static func configure(_ window: NSWindow) {
        window.isMovableByWindowBackground = false
    }
}

@main
struct TailSyncApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) var delegate

    var body: some Scene {
        Settings { EmptyView() }
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    private var statusItem: NSStatusItem!
    private var menu: NSMenu!
    private var lastNotifiedId: Int64 = 0
    private var notificationPollRunning = false
    private var isFirstNotificationPoll = true
    private var consecutiveWatchdogFailures = 0
    private var terminationInProgress = false
    private var watchdogCheckRunning = false
    private var activeRouteSummary = ""
    private var activeTransfer: ApiClient.FileProgress?
    private var storageUnavailable = false
    private static var historyWC: NSWindowController?
    private static var settingsWC: NSWindowController?
    private static var daemonProcess: Process?

    /// Force the process to be UIElement (no Dock icon) at the Carbon/CGRemote level.
    /// This is lower-level than NSApp.setActivationPolicy and works even when
    /// LSUIElement in Info.plist is ignored on newer macOS versions.
    private static func forceAccessory() {
        var psn = ProcessSerialNumber(highLongOfPSN: 0, lowLongOfPSN: UInt32(kCurrentProcess))
        TransformProcessType(&psn, ProcessApplicationTransformState(kProcessTransformToUIElementApplication))
    }

    override init() {
        super.init()
        Self.forceAccessory()
        NSApp.setActivationPolicy(.accessory)
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        Self.forceAccessory()
        NSApp.setActivationPolicy(.accessory)

        // Set app icon explicitly so notifications show the correct icon
        if let icon = NSImage(contentsOf: Bundle.main.bundleURL
            .appendingPathComponent("Contents/Resources/icon.icns")) {
            NSApp.applicationIconImage = icon
        }

        setupStatusItem()
        launchDaemon()
        requestNotificationPermission()
        startNotificationPoller()
        startDaemonWatchdog()
        registerSleepWakeNotifications()
    }

    /// Request notification permission for UNUserNotificationCenter (needed
    /// for proper app icon in notifications instead of the generic osascript icon).
    private func requestNotificationPermission() {
        // UserNotifications aborts for an unbundled SwiftPM executable on
        // newer macOS releases. The signed product bundle has a stable identity.
        guard Bundle.main.bundleURL.pathExtension == "app" else {
            print("[TailSync] skipping notifications for unbundled development build")
            return
        }
        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound]) { granted, error in
            if let error = error {
                print("[TailSync] Notification permission error: \(error.localizedDescription)")
            }
        }
    }

    /// Register for macOS sleep/wake notifications to reconnect after sleep.
    private func registerSleepWakeNotifications() {
        let nc = NSWorkspace.shared.notificationCenter
        nc.addObserver(forName: NSWorkspace.willSleepNotification, object: nil, queue: .main) { _ in
            print("[TailSync] system sleeping...")
        }
        nc.addObserver(forName: NSWorkspace.didWakeNotification, object: nil, queue: .main) { [weak self] _ in
            print("[TailSync] system woke up — reconnecting...")
            Task { @MainActor [weak self] in
                guard let self else { return }
                // Give Tailscale time to re-establish its tunnel
                try? await Task.sleep(nanoseconds: 2_000_000_000)

                // Cancel stale connection workers and reset clipboard polling
                // before evaluating post-wake health.
                let reconnected = await ApiClient.shared.reconnectPeers()
                try? await Task.sleep(nanoseconds: 300_000_000)
                let status = await ApiClient.shared.getStatus()
                if !reconnected || !status.alive || !status.tcpServerHealthy || !status.clipboardMonitorHealthy {
                    print("[TailSync] daemon unhealthy after wake — restarting")
                    await Self.stopDaemonForRestart()
                    self.launchDaemon()
                } else {
                    print("[TailSync] daemon healthy after wake — peers and clipboard monitor reset")
                }
            }
        }
    }

    /// Background poller: checks for remote clipboard events and shows
    /// notifications even before the History window is opened.
    private func startNotificationPoller() {
        Timer.scheduledTimer(withTimeInterval: 0.5, repeats: true) { [weak self] _ in
            Task { @MainActor in
                guard let self, !self.notificationPollRunning else { return }
                self.notificationPollRunning = true
                defer { self.notificationPollRunning = false }

                // Establish a baseline before notifying so existing history is
                // not reported as newly received after every app launch.
                if self.isFirstNotificationPoll {
                    self.isFirstNotificationPoll = false
                    if let latest = try? await ApiClient.shared.getHistory(limit: 1, offset: 0) {
                        self.lastNotifiedId = latest.first?.id ?? 0
                    }
                    return
                }
                guard Loc.shared.notificationsEnabled else { return }
                guard Bundle.main.bundleURL.pathExtension == "app" else { return }
                guard let latest = try? await ApiClient.shared.getHistory(limit: 1, offset: 0),
                      let newest = latest.first,
                      newest.id > self.lastNotifiedId,
                      newest.source_peer != "self" else { return }
                self.lastNotifiedId = newest.id
                let body: String = switch newest.type {
                    case "image": "📷 Image received"
                    case "file":  "📎 \(newest.description)"
                    default:      newest.description
                }
                // Use UNUserNotificationCenter so notifications show the app icon
                let content = UNMutableNotificationContent()
                content.title = "TailSync"
                content.body = body
                content.sound = nil
                let request = UNNotificationRequest(
                    identifier: "tailsync-\(newest.id)",
                    content: content,
                    trigger: nil
                )
                try? await UNUserNotificationCenter.current().add(request)
                // Keep polling quiet briefly to prevent duplicate notifications.
                try? await Task.sleep(nanoseconds: 1_000_000_000)
            }
        }
    }

    func applicationWillTerminate(_ notification: Notification) {
        // Remove the status item before the process exits to prevent ghost icons.
        if let item = statusItem {
            NSStatusBar.system.removeStatusItem(item)
            statusItem = nil
        }
    }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        guard !terminationInProgress else { return .terminateNow }
        terminationInProgress = true
        Task { @MainActor in
            await Self.stopDaemonForRestart()
            NSApp.reply(toApplicationShouldTerminate: true)
        }
        return .terminateLater
    }

    // ── Status Item ─────────────────────────────────────────────

    private func setupStatusItem() {
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)

        if let button = statusItem.button {
            button.image = NSImage(
                systemSymbolName: "doc.on.clipboard",
                accessibilityDescription: "TailSync"
            )
            button.target = self
            button.action = #selector(handleClick)
            button.sendAction(on: [.leftMouseUp, .rightMouseUp])
        }

        rebuildMenu()

        // Observe language changes to rebuild the menu
        NotificationCenter.default.addObserver(
            forName: .tailSyncLocaleChanged, object: nil, queue: .main) { _ in
            self.rebuildMenu()
        }
    }

    private func rebuildMenu() {
        let isZh = Loc.shared.lang.hasPrefix("zh")
        menu = NSMenu()
        if storageUnavailable {
            let warning = NSMenuItem(
                title: isZh ? "存储不可用，文件传输已暂停" : "Storage unavailable - file transfer paused",
                action: nil,
                keyEquivalent: ""
            )
            warning.isEnabled = false
            menu.addItem(warning)
            menu.addItem(.separator())
        }
        if let transfer = activeTransfer, transfer.active {
            let percent = transfer.sent * 100 / max(transfer.total, 1)
            let summary = NSMenuItem(
                title: "\(transfer.completedFiles)/\(transfer.totalFiles) · \(percent)%",
                action: nil,
                keyEquivalent: ""
            )
            summary.isEnabled = false
            menu.addItem(summary)
            let current = NSMenuItem(title: "\(transfer.device)  \(transfer.name)", action: nil, keyEquivalent: "")
            current.isEnabled = false
            menu.addItem(current)
            let stop = NSMenuItem(
                title: isZh ? "停止传输" : "Stop transfer",
                action: #selector(stopTransfer),
                keyEquivalent: ""
            )
            stop.target = self
            stop.isEnabled = transfer.canStop
            menu.addItem(stop)
            menu.addItem(.separator())
        }
        let hItem = NSMenuItem(title: isZh ? "历史记录" : "History",
                                action: #selector(openHistory), keyEquivalent: "")
        hItem.target = self; menu.addItem(hItem)
        let sItem = NSMenuItem(title: isZh ? "设置" : "Settings",
                                action: #selector(openSettings), keyEquivalent: "")
        sItem.target = self; menu.addItem(sItem)
        if !activeRouteSummary.isEmpty {
            menu.addItem(.separator())
            let routeItem = NSMenuItem(
                title: (isZh ? "当前连接：" : "Current route: ") + activeRouteSummary,
                action: nil,
                keyEquivalent: ""
            )
            routeItem.isEnabled = false
            menu.addItem(routeItem)
        }
        menu.addItem(.separator())
        let qItem = NSMenuItem(title: isZh ? "退出 TailSync" : "Quit TailSync",
                                action: #selector(quitApp), keyEquivalent: "q")
        qItem.target = self; menu.addItem(qItem)
    }

    @objc private func handleClick() {
        guard let event = NSApp.currentEvent else { return }
        if event.type == .rightMouseUp {
            // Right-click → show menu dynamically
            statusItem.menu = menu
            statusItem.button?.performClick(nil)
            statusItem.menu = nil
        } else {
            // Left-click → open History
            Self.showHistory()
        }
    }

    @objc private func openHistory() { Self.showHistory() }
    @objc private func openSettings() { Self.showSettings() }
    @objc private func stopTransfer() {
        guard let transfer = activeTransfer else { return }
        Task { await ApiClient.shared.cancelFileBatch(transfer.batchId) }
    }

    @objc private func quitApp() {
        NSApp.terminate(nil)
    }

    // ── Daemon ──────────────────────────────────────────────────

    private func launchDaemon() {
        if let binPath = resolveDaemonPath() {
            startDaemonProcess(binPath)
        } else {
            print("[TailSync] daemon binary not found, trying cargo build...")
            let task = Process()
            task.launchPath = "/usr/bin/env"
            task.arguments = ["cargo", "build"]
            // Try common locations for Cargo.toml
            let cargoDirs = ["." + "/src-tauri", ".." + "/src-tauri"]
            task.currentDirectoryPath = cargoDirs.first(where: {
                FileManager.default.fileExists(atPath: $0 + "/Cargo.toml")
            }) ?? FileManager.default.currentDirectoryPath
            task.launch()
            task.waitUntilExit()
            if let found = resolveDaemonPath() {
                startDaemonProcess(found)
            } else {
                print("[TailSync] could not find or build daemon")
            }
        }
    }

    private func startDaemonProcess(_ binPath: String) {
        Self.stopDaemon()
        let proc = Process()
        proc.launchPath = URL(fileURLWithPath: binPath).absoluteURL.path
        var environment = ProcessInfo.processInfo.environment
        environment["TAILSYNC_PARENT_PID"] = String(ProcessInfo.processInfo.processIdentifier)
        environment.removeValue(forKey: "TAILSYNC_API_TOKEN")
        environment["TAILSYNC_API_TOKEN_STDIN"] = "1"
        proc.environment = environment
        proc.standardOutput = FileHandle.nullDevice
        proc.standardError = FileHandle.nullDevice
        let tokenPipe = Pipe()
        proc.standardInput = tokenPipe
        do {
            try proc.run()
            let token = Data("\(ApiClient.shared.capabilityToken)\n".utf8)
            tokenPipe.fileHandleForWriting.write(token)
            tokenPipe.fileHandleForWriting.closeFile()
        } catch {
            tokenPipe.fileHandleForWriting.closeFile()
            print("[TailSync] daemon launch failed: \(error)")
            return
        }
        Self.daemonProcess = proc
        print("[TailSync] daemon started (pid=\(proc.processIdentifier))")
    }

    /// Ask the daemon to drain its background tasks, then use bounded signal fallbacks.
    private static func stopDaemon() {
        guard let proc = daemonProcess, proc.isRunning else { return }

        let requestFinished = DispatchSemaphore(value: 0)
        Task.detached {
            _ = await ApiClient.shared.requestShutdown()
            requestFinished.signal()
        }
        _ = requestFinished.wait(timeout: .now() + 1.0)

        // Rust allows up to two seconds for peer disconnect and three seconds
        // for background task drain, with a small allowance for API delivery.
        for _ in 0..<60 {
            if !proc.isRunning { break }
            Thread.sleep(forTimeInterval: 0.1)
        }
        if proc.isRunning {
            print("[TailSync] daemon did not finish coordinated shutdown — sending SIGTERM")
            proc.terminate()
            for _ in 0..<10 {
                if !proc.isRunning { break }
                Thread.sleep(forTimeInterval: 0.1)
            }
        }
        if proc.isRunning {
            print("[TailSync] daemon did not exit after SIGTERM — sending SIGKILL")
            kill(pid_t(proc.processIdentifier), SIGKILL)
            proc.waitUntilExit()
        }
        daemonProcess = nil
    }

    private static func stopDaemonForRestart() async {
        await Task.detached(priority: .userInitiated) {
            Self.stopDaemon()
        }.value
    }

    /// Resolve the daemon binary path (same logic as launchDaemon).
    private func resolveDaemonPath() -> String? {
        let bundledPath = Bundle.main.bundlePath + "/Contents/MacOS/tailsyncd"
        var candidates = [bundledPath]
        if let targetDirectory = ProcessInfo.processInfo.environment["CARGO_TARGET_DIR"] {
            candidates.append(targetDirectory + "/debug/tailsync")
        }
        candidates.append(contentsOf: [
            "../../src-tauri/target-macos/debug/tailsync",
            "../src-tauri/target-macos/debug/tailsync",
            "src-tauri/target-macos/debug/tailsync",
            "../../src-tauri/target/debug/tailsync",
            "../src-tauri/target/debug/tailsync",
            "src-tauri/target/debug/tailsync",
        ])
        return candidates.first(where: { FileManager.default.fileExists(atPath: $0) })
    }

    /// Watchdog: restart daemon if it dies or becomes unresponsive.
    private func startDaemonWatchdog() {
        Timer.scheduledTimer(withTimeInterval: 3.0, repeats: true) { [weak self] _ in
            Task { @MainActor [weak self] in
                guard let self, !self.watchdogCheckRunning else { return }
                self.watchdogCheckRunning = true
                defer { self.watchdogCheckRunning = false }
                let status = await ApiClient.shared.getStatus()
                let transfer = await ApiClient.shared.getFileProgress()
                if transfer != self.activeTransfer {
                    self.activeTransfer = transfer
                    self.rebuildMenu()
                }
                let unavailable = await ApiClient.shared.getStorageStatus()?.available == false
                if unavailable != self.storageUnavailable {
                    self.storageUnavailable = unavailable
                    self.rebuildMenu()
                }
                let routeSummary = status.activeInterfaces
                    .map(menuRouteInterfaceLabel)
                    .sorted()
                    .joined(separator: " / ")
                if routeSummary != self.activeRouteSummary {
                    self.activeRouteSummary = routeSummary
                    self.rebuildMenu()
                }
                // Clipboard polling is part of daemon health; a live API and
                // listener alone do not prove synchronization is working.
                if status.alive && status.tcpServerHealthy && status.clipboardMonitorHealthy {
                    self.consecutiveWatchdogFailures = 0
                } else {
                    let reason: String
                    if !status.alive {
                        reason = "API unresponsive"
                    } else if !status.tcpServerHealthy {
                        reason = "TCP server unhealthy"
                    } else {
                        reason = "clipboard monitor stalled"
                    }
                    self.consecutiveWatchdogFailures += 1
                    // Restart after 2 consecutive failures (~6s of downtime)
                    if self.consecutiveWatchdogFailures >= 2 {
                        print("[TailSync] daemon \(reason) — restarting...")
                        await Self.stopDaemonForRestart()
                        self.launchDaemon()
                        self.consecutiveWatchdogFailures = 0
                    }
                }
            }
        }
    }

    // ── Windows ─────────────────────────────────────────────────

    static func showHistory() {
        if let wc = historyWC {
            wc.window?.makeKeyAndOrderFront(nil)
        } else {
            let wc = makeWindow(title: "History", content: HistoryView(),
                                size: NSSize(width: 400, height: 600),
                                minSize: NSSize(width: 300, height: 360))
            historyWC = wc
        }
        NSApp.activate(ignoringOtherApps: true)
        Self.forceAccessory()
    }

    static func showSettings() {
        if let wc = settingsWC {
            wc.window?.makeKeyAndOrderFront(nil)
        } else {
            let wc = makeWindow(title: "Settings", content: SettingsView(),
                                size: NSSize(width: 540, height: 500),
                                minSize: NSSize(width: 380, height: 400))
            settingsWC = wc
        }
        NSApp.activate(ignoringOtherApps: true)
        Self.forceAccessory()
    }

    private static func makeWindow<V: View>(title: String, content: V,
                                             size: NSSize, minSize: NSSize) -> NSWindowController {
        let hosting = NSHostingController(rootView: content)
        let window = NSWindow(contentViewController: hosting)
        window.title = title
        window.styleMask = [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView]
        window.titlebarAppearsTransparent = true
        TailSyncWindowPolicy.configure(window)
        window.minSize = minSize
        window.isReleasedWhenClosed = false
        window.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        window.setFrameAutosaveName(title + "Window")
        // Set content size AFTER autosave so it overrides any restored tiny frame
        // from a stale UserDefaults entry.
        window.setContentSize(size)
        let wc = NSWindowController(window: window)
        wc.showWindow(nil)
        return wc
    }
}
