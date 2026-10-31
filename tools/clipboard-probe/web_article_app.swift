// Opt-in WebKit -> private pasteboard -> native text consumer acceptance app.
import AppKit
import WebKit

final class Probe: NSObject, NSApplicationDelegate {
    let output: URL
    let helper: String
    let targetBoard: String
    let web = WKWebView()
    let consumer = NSTextView()
    let status = NSTextField(labelWithString: "请选择网页正文，然后导出选区")
    var window: NSWindow!
    var sourceBoard: NSPasteboard?

    init(output: URL, helper: String, targetBoard: String) {
        self.output = output; self.helper = helper; self.targetBoard = targetBoard
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1000, height: 720),
            styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
        window.title = "TailSync 网页图文验收"
        let container = NSStackView()
        container.orientation = .vertical; container.spacing = 10
        let buttons = NSStackView()
        buttons.addArrangedSubview(NSButton(title: "导出网页选区", target: self, action: #selector(exportSelection)))
        buttons.addArrangedSubview(NSButton(title: "粘贴接收内容", target: self, action: #selector(pasteReceived)))
        container.addArrangedSubview(buttons)
        container.addArrangedSubview(status)
        let split = NSSplitView(); split.isVertical = true
        let scroll = NSScrollView(); scroll.hasVerticalScroller = true
        consumer.isRichText = true; consumer.importsGraphics = true
        consumer.isVerticallyResizable = true; consumer.autoresizingMask = [.width]
        consumer.frame = NSRect(x: 0, y: 0, width: 470, height: 660)
        consumer.textContainer?.widthTracksTextView = true
        scroll.documentView = consumer
        split.addArrangedSubview(web); split.addArrangedSubview(scroll)
        container.addArrangedSubview(split)
        split.heightAnchor.constraint(greaterThanOrEqualToConstant: 580).isActive = true
        web.widthAnchor.constraint(greaterThanOrEqualToConstant: 470).isActive = true
        scroll.widthAnchor.constraint(greaterThanOrEqualToConstant: 470).isActive = true
        window.contentView = container
        window.center(); window.makeKeyAndOrderFront(nil)
        let html = (try? String(contentsOf: output.appendingPathComponent("article.html"), encoding: .utf8)) ?? "<p>Fixture missing</p>"
        web.loadHTMLString(html, baseURL: nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    @objc func exportSelection() {
        let board = NSPasteboard(name: NSPasteboard.Name("tailsync-probe.browser." + UUID().uuidString))
        sourceBoard = board
        // The documented macOS Services selector takes legacy type names.
        // Modern UTI aliases are accepted by validRequestor but can yield nil
        // data in WebKit's Services encoder, hiding a valid earlier result.
        let types: [NSPasteboard.PasteboardType] = [
            .init("NeXT RTFD pasteboard type"), .init("NSStringPboardType")
        ]
        var exported = false
        var diagnostics: [[String: Any]] = []
        let selector = NSSelectorFromString("writeSelectionToPasteboard:types:")
        for responder in [web, window.firstResponder] {
            guard let responder else { continue }
            let supported = types.filter { responder.validRequestor(forSendType: $0, returnType: nil) != nil }
            guard let pasteboardType = supported.first,
                  let requestor = responder.validRequestor(forSendType: pasteboardType, returnType: nil) as? NSObject,
                  requestor.responds(to: selector) else { continue }
            // WebKit's Services requestor implements the documented Objective-C
            // selector without always declaring formal protocol conformance.
            typealias WriteSelection = @convention(c) (AnyObject, Selector, NSPasteboard, NSArray) -> Bool
            let write = unsafeBitCast(requestor.method(for: selector), to: WriteSelection.self)
            diagnostics.append(["responder": String(describing: type(of: responder)), "requestor": String(describing: type(of: requestor)), "type": pasteboardType.rawValue])
            exported = write(requestor, selector, board, supported as NSArray)
            if exported { break }
        }
        if let data = try? JSONSerialization.data(withJSONObject: diagnostics, options: [.prettyPrinted]) { try? data.write(to: output.appendingPathComponent("source-diagnostics.json")) }
        guard exported else {
            status.stringValue = "选区导出失败（\(String(describing: window.firstResponder)))"; return
        }
        let process = Process(); process.executableURL = URL(fileURLWithPath: helper)
        process.arguments = ["--pasteboard", board.name.rawValue, "--read-archive"]
        let pipe = Pipe(); process.standardOutput = pipe
        do {
            try process.run()
            let destination = output.appendingPathComponent("browser-source.cna")
            status.stringValue = "正在读取网页原生选区…"
            // WebKit may provide formats lazily. Keep its main run loop alive
            // while the helper requests the promised pasteboard bytes.
            DispatchQueue.global().async {
                let data = pipe.fileHandleForReading.readDataToEndOfFile()
                process.waitUntilExit()
                let succeeded = process.terminationStatus == 0 && (try? data.write(to: destination)) != nil
                DispatchQueue.main.async {
                    self.status.stringValue = succeeded ? "网页原生选区已导出到隔离剪贴板" : "网页原生选区导出失败"
                }
            }
        } catch { status.stringValue = "导出失败：\(error.localizedDescription)" }
    }

    @objc func pasteReceived() {
        consumer.selectAll(nil)
        let board = NSPasteboard(name: NSPasteboard.Name(targetBoard))
        let pasted = consumer.readSelection(from: board)
        let rich = consumer.textStorage ?? NSTextStorage()
        var positions: [Int] = []
        rich.enumerateAttribute(.attachment, in: NSRange(location: 0, length: rich.length)) { value, range, _ in
            if value is NSTextAttachment { positions.append(range.location) }
        }
        let result: [String: Any] = ["pasted": pasted, "text": rich.string, "image_count": positions.count,
            "image_positions_utf16": positions, "general_clipboard_modified": false]
        if let data = try? JSONSerialization.data(withJSONObject: result, options: [.prettyPrinted, .sortedKeys]) {
            try? data.write(to: output.appendingPathComponent("browser-consumer.json"))
        }
        status.stringValue = "接收文稿：\(positions.count) 张图片（隔离剪贴板）"
    }
}

let configuration = Bundle.main.object(forInfoDictionaryKey: "TailSyncAcceptance") as? [String: String]
let arguments = CommandLine.arguments.count == 4 ? Array(CommandLine.arguments.dropFirst()) :
    [configuration?["output"] ?? "", configuration?["helper"] ?? "", configuration?["targetBoard"] ?? ""]
guard arguments.count == 3, !arguments[0].isEmpty,
      arguments[2].hasPrefix("tailsync-probe.") else { exit(64) }
let delegate = Probe(output: URL(fileURLWithPath: arguments[0]), helper: arguments[1], targetBoard: arguments[2])
let app = NSApplication.shared
let menu = NSMenu()
let editItem = NSMenuItem(); let edit = NSMenu(title: "Edit")
edit.addItem(withTitle: "Select All", action: #selector(NSText.selectAll(_:)), keyEquivalent: "a")
editItem.submenu = edit; menu.addItem(editItem); app.mainMenu = menu
app.delegate = delegate; app.setActivationPolicy(.regular); app.run()
