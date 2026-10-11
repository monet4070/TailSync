import AppKit
import Foundation

guard CommandLine.arguments.count == 2 else { fatalError("RTF path required") }
let input = URL(fileURLWithPath: CommandLine.arguments[1])
let data = try Data(contentsOf: input)
let type: NSAttributedString.DocumentType = input.pathExtension == "html" ? .html : input.pathExtension == "rtfd" ? .rtfd : .rtf
let document = try NSAttributedString(
    data: data,
    options: [.documentType: type, .characterEncoding: String.Encoding.utf8.rawValue],
    documentAttributes: nil
)
guard document.string.contains("中文 🙂"), document.string.contains("after picture") else {
    fatalError("Unicode or body order was lost")
}
var attachmentCount = 0
var attachmentOffset = -1
var hasBold = false
var hasItalic = false
document.enumerateAttributes(in: NSRange(location: 0, length: document.length)) { attributes, range, _ in
    if attributes[.attachment] is NSTextAttachment {
        attachmentCount += 1
        attachmentOffset = range.location
    }
    if let font = attributes[.font] as? NSFont {
        let traits = NSFontManager.shared.traits(of: font)
        hasBold = hasBold || traits.contains(.boldFontMask)
        hasItalic = hasItalic || traits.contains(.italicFontMask)
    }
}
let body = document.string as NSString
let afterOffset = body.range(of: "after picture").location
guard attachmentCount == 1, attachmentOffset < afterOffset, hasBold, hasItalic else {
    print("RTF FAIL: attachments=\(attachmentCount), bold=\(hasBold), italic=\(hasItalic), imageOffset=\(attachmentOffset), afterOffset=\(afterOffset)")
    exit(1)
}
print("AppKit \(input.pathExtension) PASS: Unicode, bold, italic, inline picture, body order; no clipboard access")
