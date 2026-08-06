import AppKit

let pb = NSPasteboard.general
let arguments = Array(CommandLine.arguments.dropFirst())

if arguments.first == "--read-image" {
    guard arguments.count == 1 else {
        fputs("clipboard-helper: --read-image does not accept arguments\n", stderr)
        exit(64)
    }
    guard let image = NSImage(pasteboard: pb) else {
        exit(1)
    }
    var proposedRect = NSRect(origin: .zero, size: image.size)
    guard let cgImage = image.cgImage(
        forProposedRect: &proposedRect,
        context: nil,
        hints: nil
    ) else {
        exit(1)
    }
    let width = cgImage.width
    let height = cgImage.height
    let (pixelCount, pixelOverflow) = width.multipliedReportingOverflow(by: height)
    let (rgbaBytes, byteOverflow) = pixelCount.multipliedReportingOverflow(by: 4)
    let maxPackedImageBytes = 32 * 1024 * 1024
    guard width > 0,
          height > 0,
          width <= Int(UInt32.max),
          height <= Int(UInt32.max),
          !pixelOverflow,
          !byteOverflow,
          rgbaBytes <= maxPackedImageBytes - 8 else {
        fputs("clipboard-helper: image exceeds the TailSync payload limit\n", stderr)
        exit(2)
    }

    let bitmap = NSBitmapImageRep(cgImage: cgImage)
    guard let png = bitmap.representation(using: .png, properties: [:]) else {
        exit(1)
    }
    var encodedWidth = UInt32(width).littleEndian
    var encodedHeight = UInt32(height).littleEndian
    var output = Data(capacity: 8 + png.count)
    withUnsafeBytes(of: &encodedWidth) { output.append(contentsOf: $0) }
    withUnsafeBytes(of: &encodedHeight) { output.append(contentsOf: $0) }
    output.append(png)
    FileHandle.standardOutput.write(output)
    exit(0)
}

if arguments.first == "--write-files" {
    let paths = arguments.dropFirst()
    guard !paths.isEmpty else {
        fputs("clipboard-helper: --write-files requires at least one path\n", stderr)
        exit(64)
    }
    let urls = paths.map { NSURL(fileURLWithPath: $0) }
    pb.clearContents()
    let wrote = pb.writeObjects(urls)
    if wrote {
        // NSPasteboard may ask the short-lived helper to finish serializing the
        // file URL after writeObjects returns. Give that request a brief run
        // loop window before the process exits.
        RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.2))
    }
    exit(wrote ? 0 : 1)
}

guard arguments.isEmpty else {
    fputs("usage: clipboard-helper [--read-image | --write-files path ...]\n", stderr)
    exit(64)
}

if let urls = pb.readObjects(
    forClasses: [NSURL.self],
    options: [.urlReadingFileURLsOnly: true]
) as? [URL] {
    for url in urls { print(url.path) }
}
