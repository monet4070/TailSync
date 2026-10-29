// Native clipboard consumer for opt-in acceptance. Never touches general pasteboard.
import AppKit
import Foundation

guard CommandLine.arguments.count == 2,
      CommandLine.arguments[1].hasPrefix("tailsync-probe.") else { exit(64) }
let board = NSPasteboard(name: NSPasteboard.Name(CommandLine.arguments[1]))
let app = NSApplication.shared
app.setActivationPolicy(.prohibited)
let view = NSTextView(frame: NSRect(x: 0, y: 0, width: 720, height: 1200))
view.isRichText = true
view.importsGraphics = true
view.isEditable = true
let pasted = view.readSelection(from: board)
let text = view.textStorage ?? NSTextStorage()
var positions: [Int] = []
var colors: [[Int]] = []
text.enumerateAttribute(.attachment, in: NSRange(location: 0, length: text.length)) { value, range, _ in
    if let attachment = value as? NSTextAttachment {
        positions.append(range.location)
        if let bytes = attachment.fileWrapper?.regularFileContents ?? attachment.image?.tiffRepresentation,
           let bitmap = NSBitmapImageRep(data: bytes),
           let color = bitmap.colorAt(x: bitmap.pixelsWide / 2, y: bitmap.pixelsHigh / 2)?.usingColorSpace(.deviceRGB) {
            colors.append([color.redComponent, color.greenComponent, color.blueComponent].map { Int(($0 * 255).rounded()) })
        }
    }
}
let result: [String: Any] = [
    "pasted": pasted,
    "text": text.string,
    "image_count": positions.count,
    "image_positions_utf16": positions,
    "image_colors_rgb": colors,
    "formats": (board.types ?? []).map(\.rawValue),
    "general_clipboard_modified": false
]
let data = try JSONSerialization.data(withJSONObject: result, options: [.prettyPrinted, .sortedKeys])
FileHandle.standardOutput.write(data)
