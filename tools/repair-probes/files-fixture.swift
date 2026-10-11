
import AppKit
let name=CommandLine.arguments[1]
guard name.hasPrefix("tailsync-probe.") else {exit(64)}
let pb=NSPasteboard(name:NSPasteboard.Name(name))
let items=(0..<17).map {i -> NSPasteboardItem in let item=NSPasteboardItem();item.setString("file:///tmp/tailsync-review-file-\(i).txt",forType:.fileURL);return item}
pb.clearContents();guard pb.writeObjects(items) else {exit(65)}
print("synthetic_native_file_items=\(pb.pasteboardItems?.count ?? -1)")
