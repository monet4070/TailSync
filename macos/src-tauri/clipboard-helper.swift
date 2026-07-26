import AppKit

let pb = NSPasteboard.general
let arguments = Array(CommandLine.arguments.dropFirst())

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
    fputs("usage: clipboard-helper [--write-files path ...]\n", stderr)
    exit(64)
}

if let urls = pb.readObjects(
    forClasses: [NSURL.self],
    options: [.urlReadingFileURLsOnly: true]
) as? [URL] {
    for url in urls { print(url.path) }
}
