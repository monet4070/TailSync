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

enum DaemonShutdownPolicy {
    static let requestWait: TimeInterval = 0.25
    static let gracefulExitWait: TimeInterval = 1.0
    static let terminateWait: TimeInterval = 0.25
    static let pollInterval: TimeInterval = 0.025
    static var maximumWait: TimeInterval { requestWait + gracefulExitWait + terminateWait }
}

enum DaemonLifecyclePolicy {
    static func allowsDaemonActivity(terminationInProgress: Bool) -> Bool {
        !terminationInProgress
    }
}

enum RuntimeNotificationPolicy {
    static func shouldRefreshHistory(
        previousHistoryVersion: UInt64?,
        currentHistoryVersion: UInt64,
        isFirstPoll: Bool
    ) -> Bool {
        isFirstPoll || previousHistoryVersion != currentHistoryVersion
    }
}

enum StatusItemImagePolicy {
    static func image(from cachedImage: NSImage) -> NSImage? {
        guard let image = cachedImage.copy() as? NSImage else { return nil }
        image.size = NSSize(width: 18, height: 18)
        image.isTemplate = false
        return image
    }
}

enum TailSyncAppVersion {
    private static let developmentFallback = "2.2.2"

    static var current: String {
        let info = Bundle.main.infoDictionary
        return (info?["CFBundleShortVersionString"] as? String)
            ?? (info?["CFBundleVersion"] as? String)
            ?? developmentFallback
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
    private let singleInstanceLock = SingleInstanceLock()
    private var ownsSingleInstanceLock = false
    private var statusItem: NSStatusItem!
    private var menu: NSMenu!
    private var lastNotifiedId: Int64 = 0
    private var isFirstNotificationPoll = true
    private var notificationRuntimeRevision: UInt64 = 0
    private var notificationHistoryVersion: UInt64?
    private var notificationEventId: UInt64 = 0
    private var consecutiveWatchdogFailures = 0
    private var watchdogBackoffSeconds: TimeInterval = 0
    private var watchdogRetryAfter: Date?
    private var daemonBuildInProgress = false
    private var terminationInProgress = false
    private var watchdogCheckRunning = false
    private var activeRouteSummary = ""
    private var activeTransfer: ApiClient.FileProgress?
    private var storageUnavailable = false
    private var syncEnabled = true
    private var shortcutRegistered = false
    private var updateCheckRunning = false
    private var notificationTask: Task<Void, Never>?
    private var watchdogTimer: Timer?
    private static var historyWC: NSWindowController?
    private static var settingsWC: NSWindowController?
    private static var daemonProcess: Process?
    private static let daemonStopLock = NSLock()

    private var daemonActivityAllowed: Bool {
        DaemonLifecyclePolicy.allowsDaemonActivity(
            terminationInProgress: terminationInProgress
        )
    }

    private func scheduleWatchdogRetry() {
        watchdogBackoffSeconds = min(
            watchdogBackoffSeconds > 0 ? watchdogBackoffSeconds * 2 : 3,
            60
        )
        watchdogRetryAfter = Date().addingTimeInterval(watchdogBackoffSeconds)
        print("[TailSync] daemon watchdog backing off for \(Int(watchdogBackoffSeconds))s")
    }

    private func resetWatchdogBackoff() {
        watchdogBackoffSeconds = 0
        watchdogRetryAfter = nil
    }

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

    func applicationWillFinishLaunching(_ notification: Notification) {
        do {
            guard try singleInstanceLock.acquire() else {
                Self.activateExistingInstance()
                NSApp.terminate(nil)
                return
            }
            ownsSingleInstanceLock = true
        } catch {
            print("[TailSync] could not acquire the single-instance lock: \(error)")
            NSApp.terminate(nil)
        }
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        guard ownsSingleInstanceLock else { return }
        Self.forceAccessory()
        NSApp.setActivationPolicy(.accessory)
        // Remove plaintext Quick Look files left by a previous crash before
        // any history window can create a new preview session.
        _ = HistoryPreviewSession.cleanupAtStartup()

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
        scheduleUpdateCheck()

        GlobalShortcutController.shared.onSyncActivate = { [weak self] in
            Task { @MainActor in
                guard let self, self.daemonActivityAllowed else { return }
                guard let enabled = await ApiClient.shared.toggleSync(), self.daemonActivityAllowed else { return }
                self.syncEnabled = enabled
                self.rebuildMenu()
                NotificationCenter.default.post(
                    name: GlobalShortcutController.syncStateChanged,
                    object: nil,
                    userInfo: ["enabled": enabled]
                )
            }
        }
        GlobalShortcutController.shared.onHistoryActivate = {
            Self.showHistory()
        }
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
                guard let self, self.daemonActivityAllowed else { return }
                // Give Tailscale time to re-establish its tunnel
                try? await Task.sleep(nanoseconds: 2_000_000_000)
                guard self.daemonActivityAllowed else { return }

                // Cancel stale connection workers and reset clipboard polling
                // before evaluating post-wake health.
                let reconnected = await ApiClient.shared.reconnectPeers()
                guard self.daemonActivityAllowed else { return }
                try? await Task.sleep(nanoseconds: 300_000_000)
                guard self.daemonActivityAllowed else { return }
                let status = await ApiClient.shared.getStatus()
                guard self.daemonActivityAllowed else { return }
                if !reconnected || !status.alive || !status.tcpServerHealthy || !status.clipboardMonitorHealthy {
                    print("[TailSync] daemon unhealthy after wake — restarting")
                    await Self.stopDaemonForRestart()
                    guard self.daemonActivityAllowed else { return }
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
        notificationTask?.cancel()
        notificationTask = Task { @MainActor [weak self] in
            guard let self else { return }
            while !Task.isCancelled {
                guard self.daemonActivityAllowed else { return }
                guard let snapshot = await ApiClient.shared.waitForRuntimeSnapshot(
                    since: self.notificationRuntimeRevision,
                    sinceNotificationId: self.notificationEventId
                ) else {
                    try? await Task.sleep(for: .milliseconds(750))
                    continue
                }
                self.notificationRuntimeRevision = snapshot.revision
                let shouldRefreshHistory = RuntimeNotificationPolicy.shouldRefreshHistory(
                    previousHistoryVersion: self.notificationHistoryVersion,
                    currentHistoryVersion: snapshot.historyVersion,
                    isFirstPoll: self.isFirstNotificationPoll
                )
                self.notificationHistoryVersion = snapshot.historyVersion

                for event in snapshot.notifications {
                    self.notificationEventId = max(self.notificationEventId, event.id)
                    guard Loc.shared.notificationsEnabled,
                          Bundle.main.bundleURL.pathExtension == "app",
                          self.daemonActivityAllowed else { continue }
                    let content = UNMutableNotificationContent()
                    content.title = "TailSync"
                    content.body = event.message
                    content.sound = nil
                    let request = UNNotificationRequest(
                        identifier: "tailsync-runtime-\(event.id)",
                        content: content,
                        trigger: nil
                    )
                    try? await UNUserNotificationCenter.current().add(request)
                }

                // Establish a baseline before notifying so existing history is
                // not reported as newly received after every app launch.
                if self.isFirstNotificationPoll {
                    self.isFirstNotificationPoll = false
                    if let latest = try? await ApiClient.shared.getHistory(limit: 1, offset: 0) {
                        self.lastNotifiedId = latest.first?.id ?? 0
                    }
                    continue
                }
                guard shouldRefreshHistory,
                      Loc.shared.notificationsEnabled,
                      Bundle.main.bundleURL.pathExtension == "app",
                      snapshot.historyVersion > 0,
                      self.daemonActivityAllowed else { continue }
                guard let latest = try? await ApiClient.shared.getHistory(limit: 1, offset: 0),
                      let newest = latest.first,
                      newest.id > self.lastNotifiedId,
                      self.daemonActivityAllowed else { continue }
                self.lastNotifiedId = newest.id
                guard newest.source_peer != "self" else { continue }
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
            }
        }
    }

    func applicationWillTerminate(_ notification: Notification) {
        guard ownsSingleInstanceLock else { return }
        stopBackgroundActivity()
        HistoryPreviewWindowController.shared.shutdown()
        // Remove the status item before the process exits to prevent ghost icons.
        if let item = statusItem {
            NSStatusBar.system.removeStatusItem(item)
            statusItem = nil
        }
        GlobalShortcutController.shared.unregister()
        singleInstanceLock.release()
        ownsSingleInstanceLock = false
    }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        guard ownsSingleInstanceLock else { return .terminateNow }
        guard !terminationInProgress else { return .terminateLater }
        terminationInProgress = true
        stopBackgroundActivity()
        Task { @MainActor in
            await Self.stopDaemonForRestart()
            NSApp.reply(toApplicationShouldTerminate: true)
        }
        return .terminateLater
    }

    private func stopBackgroundActivity() {
        notificationTask?.cancel()
        notificationTask = nil
        watchdogTimer?.invalidate()
        watchdogTimer = nil
        GlobalShortcutController.shared.onSyncActivate = nil
        GlobalShortcutController.shared.onHistoryActivate = nil
        GlobalShortcutController.shared.unregister()
    }

    private static func activateExistingInstance() {
        let currentPID = ProcessInfo.processInfo.processIdentifier
        let bundleIdentifier = Bundle.main.bundleIdentifier ?? "com.tailsync.app"
        let executableURL = Bundle.main.executableURL?.resolvingSymlinksInPath()
        let existing = NSWorkspace.shared.runningApplications.first { application in
            guard application.processIdentifier != currentPID else { return false }
            if application.bundleIdentifier == bundleIdentifier {
                return true
            }
            return application.executableURL?.resolvingSymlinksInPath() == executableURL
        }
        existing?.activate(options: [.activateIgnoringOtherApps])
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
        NotificationCenter.default.addObserver(
            forName: .tailSyncThemeAssetsChanged, object: nil, queue: .main) { [weak self] _ in
            self?.applyThemeLogo()
        }
        applyThemeLogo()
    }

    private func applyThemeLogo() {
        guard let button = statusItem?.button else { return }
        if let cachedImage = Loc.shared.themeAssetImages["logo"],
           let image = StatusItemImagePolicy.image(from: cachedImage) {
            button.image = image
        } else {
            button.image = NSImage(systemSymbolName: "doc.on.clipboard", accessibilityDescription: "TailSync")
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
        let syncItem = NSMenuItem(
            title: isZh
                ? (syncEnabled ? "暂停同步" : "开启同步")
                : (syncEnabled ? "Pause sync" : "Enable sync"),
            action: #selector(toggleSyncAction),
            keyEquivalent: ""
        )
        syncItem.target = self
        menu.addItem(syncItem)
        menu.addItem(.separator())
        let hItem = NSMenuItem(title: isZh ? "历史记录" : "History",
                                action: #selector(openHistory), keyEquivalent: "")
        hItem.target = self; menu.addItem(hItem)
        let sItem = NSMenuItem(title: isZh ? "设置" : "Settings",
                                action: #selector(openSettings), keyEquivalent: "")
        sItem.target = self; menu.addItem(sItem)
        let updateItem = NSMenuItem(
            title: Loc.t("menu.checkForUpdates"),
            action: #selector(checkForUpdatesAction),
            keyEquivalent: ""
        )
        updateItem.target = self
        menu.addItem(updateItem)
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
    @objc private func checkForUpdatesAction() { scheduleUpdateCheck(showWhenCurrent: true) }
    @objc private func toggleSyncAction() {
        Task { @MainActor [weak self] in
            guard let self, let enabled = await ApiClient.shared.toggleSync() else { return }
            self.syncEnabled = enabled
            self.rebuildMenu()
        }
    }
    @objc private func stopTransfer() {
        guard let transfer = activeTransfer else { return }
        Task { await ApiClient.shared.cancelFileBatch(transfer.batchId) }
    }

    @objc private func quitApp() {
        NSApp.terminate(nil)
    }

    private func scheduleUpdateCheck(showWhenCurrent: Bool = false) {
        guard !updateCheckRunning else { return }
        updateCheckRunning = true
        Task { @MainActor [weak self] in
            defer { self?.updateCheckRunning = false }
            do {
                guard let update = try await ApiClient.shared.checkForUpdate() else {
                    if showWhenCurrent { self?.showUpdateMessage(Loc.t("update.current")) }
                    return
                }
                self?.presentUpdate(update)
            } catch {
                // Surface the updater error so a missing feed or invalid release is diagnosable.
                if showWhenCurrent {
                    self?.showUpdateMessage(error.localizedDescription, error: true)
                }
            }
        }
    }

    private func presentUpdate(_ update: ApiClient.UpdateInfo) {
        let alert = NSAlert()
        alert.alertStyle = .informational
        alert.messageText = Loc.t("update.available")
            .replacingOccurrences(of: "{version}", with: update.version)
        let notes = update.notes?.trimmingCharacters(in: .whitespacesAndNewlines)
        alert.informativeText = notes.flatMap { $0.isEmpty ? nil : $0 }
            ?? Loc.t("update.installPrompt")
        alert.addButton(withTitle: Loc.t("update.install"))
        alert.addButton(withTitle: Loc.t("common.cancel"))

        let visibleWindow = [NSApp.keyWindow, Self.settingsWC?.window, Self.historyWC?.window]
            .compactMap { $0 }
            .first(where: { $0.isVisible })
        if let visibleWindow {
            beginUpdateAlert(alert, for: visibleWindow)
        } else {
            Self.showHistory { [weak self] window in
                self?.beginUpdateAlert(alert, for: window)
            }
        }
    }

    private func beginUpdateAlert(_ alert: NSAlert, for parentWindow: NSWindow) {
        alert.beginSheetModal(for: parentWindow) { [weak self] response in
            guard response == .alertFirstButtonReturn else { return }
            Task { @MainActor [weak self] in
                do {
                    _ = try await Self.installAvailableUpdateAndRelaunch()
                } catch {
                    self?.showUpdateMessage(error.localizedDescription, error: true)
                }
            }
        }
    }

    @MainActor
    static func installAvailableUpdateAndRelaunch() async throws -> Bool {
        guard try await ApiClient.shared.installUpdate() else { return false }
        await stopDaemonForRestart()
        try scheduleUpdatedAppRelaunch()
        NSApp.terminate(nil)
        return true
    }

    private func showUpdateMessage(_ message: String, error: Bool = false) {
        let alert = NSAlert()
        alert.alertStyle = error ? .warning : .informational
        alert.messageText = error ? Loc.t("update.failed") : Loc.t("update.title")
        alert.informativeText = message
        alert.addButton(withTitle: "OK")
        alert.runModal()
    }

    // ── Daemon ──────────────────────────────────────────────────

    private func launchDaemon() {
        guard daemonActivityAllowed else { return }
        if let binPath = resolveDaemonPath() {
            startDaemonProcess(binPath)
        } else {
            guard !daemonBuildInProgress else { return }
            print("[TailSync] daemon binary not found, trying cargo build...")
            // Try common locations for Cargo.toml
            let cargoDirs = ["." + "/src-tauri", ".." + "/src-tauri"]
            guard let cargoDirectory = cargoDirs.first(where: {
                FileManager.default.fileExists(atPath: $0 + "/Cargo.toml")
            }) else {
                print("[TailSync] could not find daemon Cargo.toml")
                return
            }
            daemonBuildInProgress = true
            DispatchQueue.global(qos: .utility).async { [weak self] in
                let task = Process()
                task.executableURL = URL(fileURLWithPath: "/usr/bin/env")
                task.arguments = ["cargo", "build"]
                task.currentDirectoryURL = URL(fileURLWithPath: cargoDirectory)
                do {
                    try task.run()
                    task.waitUntilExit()
                    if task.terminationStatus != 0 {
                        print("[TailSync] cargo build exited with status \(task.terminationStatus)")
                    }
                } catch {
                    print("[TailSync] cargo build failed: \(error)")
                }
                DispatchQueue.main.async { [weak self] in
                    guard let self else { return }
                    self.daemonBuildInProgress = false
                    guard self.daemonActivityAllowed else { return }
                    if let found = self.resolveDaemonPath() {
                        self.startDaemonProcess(found)
                    } else {
                        print("[TailSync] could not find or build daemon")
                    }
                }
            }
        }
    }

    private func startDaemonProcess(_ binPath: String) {
        Self.stopDaemon()
        let proc = Process()
        proc.executableURL = URL(fileURLWithPath: binPath).absoluteURL
        var environment = ProcessInfo.processInfo.environment
        environment["TAILSYNC_PARENT_PID"] = String(ProcessInfo.processInfo.processIdentifier)
        environment["TAILSYNC_API_SOCKET"] = ApiClient.apiSocketPathForDaemon()
        environment.removeValue(forKey: "TAILSYNC_API_TOKEN")
        environment["TAILSYNC_API_TOKEN_STDIN"] = "1"
        proc.environment = environment
        if let logHandle = Self.daemonLogHandle() {
            // Share one handle across stdout+stderr so their writes interleave
            // into a single file, exactly like `tailsyncd >log 2>&1`. Previously
            // both went to /dev/null, so a wedged link left no trace to inspect.
            proc.standardOutput = logHandle
            proc.standardError = logHandle
        } else {
            proc.standardOutput = FileHandle.nullDevice
            proc.standardError = FileHandle.nullDevice
        }
        let tokenPipe = Pipe()
        proc.standardInput = tokenPipe
        do {
            try proc.run()
            Self.daemonProcess = proc
            ApiClient.shared.setExpectedDaemonPID(pid_t(proc.processIdentifier))
            let token = Data("\(ApiClient.shared.capabilityToken)\n".utf8)
            tokenPipe.fileHandleForWriting.write(token)
            tokenPipe.fileHandleForWriting.closeFile()
        } catch {
            tokenPipe.fileHandleForWriting.closeFile()
            print("[TailSync] daemon launch failed: \(error)")
            return
        }
        print("[TailSync] daemon started (pid=\(proc.processIdentifier))")
    }

    /// Open (creating if needed) the daemon log at
    /// `~/Library/Logs/TailSync/tailsyncd.log`, positioned to append. The
    /// daemon's `env_logger` already writes to stderr at `info` by default, so
    /// pointing stderr here captures diagnostics for a wedged link. Returns
    /// `nil` on any filesystem error so the caller falls back to `/dev/null`.
    private static func daemonLogHandle() -> FileHandle? {
        let fileManager = FileManager.default
        guard
            let logsDirectory = fileManager
                .urls(for: .libraryDirectory, in: .userDomainMask)
                .first?
                .appendingPathComponent("Logs/TailSync", isDirectory: true)
        else { return nil }

        do {
            try fileManager.createDirectory(at: logsDirectory, withIntermediateDirectories: true)
        } catch {
            print("[TailSync] could not create daemon log directory: \(error)")
            return nil
        }

        let logURL = logsDirectory.appendingPathComponent("tailsyncd.log")

        // Bound growth: if the log has passed the cap, truncate before reopening
        // so it can't grow without limit across restarts.
        let maxLogBytes: UInt64 = 5 * 1024 * 1024
        if let size = try? fileManager.attributesOfItem(atPath: logURL.path)[.size] as? UInt64,
           size > maxLogBytes {
            try? Data().write(to: logURL)
        }
        if !fileManager.fileExists(atPath: logURL.path) {
            fileManager.createFile(atPath: logURL.path, contents: nil)
        }

        guard let handle = try? FileHandle(forWritingTo: logURL) else {
            print("[TailSync] could not open daemon log at \(logURL.path)")
            return nil
        }
        _ = try? handle.seekToEnd()
        return handle
    }

    /// Ask the daemon to drain its background tasks, then use bounded signal fallbacks.
    private static func stopDaemon() {
        daemonStopLock.lock()
        defer { daemonStopLock.unlock() }
        guard let proc = daemonProcess, proc.isRunning else {
            daemonProcess = nil
            return
        }

        let requestFinished = DispatchSemaphore(value: 0)
        Task.detached {
            _ = await ApiClient.shared.requestShutdown()
            requestFinished.signal()
        }
        _ = requestFinished.wait(timeout: .now() + DaemonShutdownPolicy.requestWait)

        waitForDaemonExit(proc, timeout: DaemonShutdownPolicy.gracefulExitWait)
        if proc.isRunning {
            print("[TailSync] daemon did not finish coordinated shutdown — sending SIGTERM")
            proc.terminate()
            waitForDaemonExit(proc, timeout: DaemonShutdownPolicy.terminateWait)
        }
        if proc.isRunning {
            print("[TailSync] daemon did not exit after SIGTERM — sending SIGKILL")
            kill(pid_t(proc.processIdentifier), SIGKILL)
            proc.waitUntilExit()
        }
        daemonProcess = nil
    }

    private static func waitForDaemonExit(_ process: Process, timeout: TimeInterval) {
        let deadline = Date().addingTimeInterval(timeout)
        while process.isRunning && Date() < deadline {
            Thread.sleep(forTimeInterval: DaemonShutdownPolicy.pollInterval)
        }
    }

    private static func stopDaemonForRestart() async {
        await Task.detached(priority: .userInitiated) {
            Self.stopDaemon()
        }.value
    }

    private static func scheduleUpdatedAppRelaunch() throws {
        let relaunch = Process()
        relaunch.executableURL = URL(fileURLWithPath: "/bin/sh")
        relaunch.arguments = [
            "-c",
            "sleep 1; exec /usr/bin/open \"$1\"",
            "tailsync-relaunch",
            Bundle.main.bundleURL.path,
        ]
        relaunch.standardOutput = FileHandle.nullDevice
        relaunch.standardError = FileHandle.nullDevice
        try relaunch.run()
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
        watchdogTimer = Timer.scheduledTimer(withTimeInterval: 3.0, repeats: true) { [weak self] _ in
            Task { @MainActor [weak self] in
                guard let self, self.daemonActivityAllowed, !self.watchdogCheckRunning else { return }
                if let retryAfter = self.watchdogRetryAfter, Date() < retryAfter { return }
                self.watchdogCheckRunning = true
                defer { self.watchdogCheckRunning = false }
                // A zero revision requests an immediate consolidated snapshot;
                // this keeps the watchdog independent while avoiding four
                // separate status/progress/storage/settings round trips.
                guard let snapshot = await ApiClient.shared.waitForRuntimeSnapshot(since: 0) else {
                    self.consecutiveWatchdogFailures += 1
                    if self.consecutiveWatchdogFailures >= 2 {
                        print("[TailSync] daemon API unresponsive — restarting...")
                        await Self.stopDaemonForRestart()
                        guard self.daemonActivityAllowed else { return }
                        self.launchDaemon()
                        self.consecutiveWatchdogFailures = 0
                        self.scheduleWatchdogRetry()
                    }
                    return
                }
                let status = snapshot.status
                guard self.daemonActivityAllowed else { return }
                let transfer = snapshot.progress
                if transfer != self.activeTransfer {
                    self.activeTransfer = transfer
                    self.rebuildMenu()
                }
                let unavailable = snapshot.storage?.available == false
                if unavailable != self.storageUnavailable {
                    self.storageUnavailable = unavailable
                    self.rebuildMenu()
                }
                if snapshot.syncEnabled != self.syncEnabled {
                    guard self.daemonActivityAllowed else { return }
                    self.syncEnabled = snapshot.syncEnabled
                    self.rebuildMenu()
                }
                if !self.shortcutRegistered,
                   let settings = try? await ApiClient.shared.getSettings() {
                    guard self.daemonActivityAllowed else { return }
                    if !self.shortcutRegistered {
                        self.shortcutRegistered = true
                        if case .failure(let error) =
                            GlobalShortcutController.shared.register(
                                syncShortcut: settings.sync_shortcut,
                                historyShortcut: settings.history_shortcut
                            )
                        {
                            print("[TailSync] could not register global shortcuts: \(error)")
                        }
                    }
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
                    self.resetWatchdogBackoff()
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
                        guard self.daemonActivityAllowed else { return }
                        self.launchDaemon()
                        self.consecutiveWatchdogFailures = 0
                        self.scheduleWatchdogRetry()
                    }
                }
            }
        }
    }

    // ── Windows ─────────────────────────────────────────────────

    static func showHistory(completion: ((NSWindow) -> Void)? = nil) {
        DispatchQueue.main.async {
            let historyWindowController = HistoryWindowController.shared
            Self.forceAccessory()
            if let wc = historyWC, let window = wc.window {
                historyWindowController.attach(window)
                historyWindowController.present()
                completion?(window)
            } else {
                let wc = makeWindow(title: "History", content: HistoryView(),
                                    size: NSSize(width: 400, height: 600),
                                    minSize: NSSize(width: 300, height: 360),
                                    onVisibilityChange: { isVisible in
                                        NotificationCenter.default.post(
                                            name: .tailSyncHistoryWindowVisibilityChanged,
                                            object: nil,
                                            userInfo: ["visible": isVisible]
                                        )
                                    }) {
                    historyWindowController.detach()
                    historyWC = nil
                    Task { @MainActor in
                        HistoryPreviewWindowController.shared.attachHistoryWindow(nil)
                    }
                }
                historyWC = wc
                guard let window = wc.window else { return }
                historyWindowController.attach(window)
                historyWindowController.present()
                completion?(window)
            }
            Task { @MainActor in
                HistoryPreviewWindowController.shared.attachHistoryWindow(historyWC?.window)
            }
        }
    }

    static func showSettings() {
        if let wc = settingsWC {
            wc.window?.makeKeyAndOrderFront(nil)
        } else {
            let wc = makeWindow(title: "Settings", content: SettingsView(),
                                size: NSSize(width: 540, height: 500),
                                minSize: NSSize(width: 380, height: 400)) {
                settingsWC = nil
            }
            settingsWC = wc
        }
        NSApp.activate(ignoringOtherApps: true)
        Self.forceAccessory()
    }

    private static func makeWindow<V: View>(
        title: String,
        content: V,
        size: NSSize,
        minSize: NSSize,
        onVisibilityChange: ((Bool) -> Void)? = nil,
        onClose: @escaping () -> Void
    ) -> NSWindowController {
        let hosting = NSHostingController(rootView: content)
        let window = NSWindow(contentViewController: hosting)
        window.title = title
        window.styleMask = [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView]
        window.titlebarAppearsTransparent = true
        TailSyncWindowPolicy.configure(window)
        window.minSize = minSize
        window.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        window.setFrameAutosaveName(title + "Window")
        // Set content size AFTER autosave so it overrides any restored tiny frame
        // from a stale UserDefaults entry.
        window.setContentSize(size)
        let wc = TailSyncTransientWindowController(
            window: window,
            onVisibilityChange: onVisibilityChange,
            onClose: onClose
        )
        wc.showWindow(nil)
        return wc
    }
}
