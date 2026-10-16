import AppKit

// Synthetic Finder-shaped items on a private board only. No filesystem reads.
let args = Array(CommandLine.arguments.dropFirst())
guard args.count == 3, args[0].hasPrefix("tailsync-probe."),
      let count = Int(args[1]), (1...64).contains(count) else { exit(64) }
let pb = NSPasteboard(name: NSPasteboard.Name(args[0]))
let mode = args[2]
if mode == "clear" { pb.releaseGlobally(); exit(0) }
guard ["finder", "url-only", "source", "html", "qq", "opaque", "suppressed", "text-only"].contains(mode) else { exit(64) }
let items = (0..<count).map { index -> NSPasteboardItem in
    let item = NSPasteboardItem()
    let name = "文件 \(index).txt"
    if mode != "text-only" {
        item.setString("file:///tmp/tailsync-finder-fixture-\(index).txt", forType: .fileURL)
    }
    if mode != "url-only" {
        item.setString(name, forType: .string)
        item.setData(name.data(using: .utf16)!, forType: NSPasteboard.PasteboardType("public.utf16-external-plain-text"))
        item.setData(Data("icns\0\0\0\u{8}".utf8), forType: NSPasteboard.PasteboardType("com.apple.icns"))
    }
    if index == 0 {
        switch mode {
        case "source": item.setData(Data(repeating: 1, count: 16), forType: NSPasteboard.PasteboardType("org.tailsync.clipboard-source"))
        case "html": item.setString("<p>document with a file reference</p>", forType: .html)
        case "qq": item.setData(Data([1]), forType: NSPasteboard.PasteboardType("dyn.ah62d4rv4gu8zk3psqrw067cbsv4he4pcsz4gny5ysmy0634ytf2gn"))
        case "opaque": item.setData(Data([1]), forType: NSPasteboard.PasteboardType("com.example.document"))
        case "suppressed": item.setData(Data([1]), forType: NSPasteboard.PasteboardType("org.nspasteboard.ConcealedType"))
        default: break
        }
    }
    return item
}
pb.clearContents()
guard pb.writeObjects(items) else { exit(65) }
let actual = pb.pasteboardItems ?? []
let metadata = actual.map { $0.types.map(\.rawValue) }
FileHandle.standardOutput.write(try JSONSerialization.data(withJSONObject: metadata))
