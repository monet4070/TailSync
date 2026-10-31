"""QQ cache bookmark and anchored filesystem reader, on task-owned fixtures only."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from test_qq_clipboard_conversion import png


class QQCacheAccessTests(unittest.TestCase):
    def test_production_cache_access_reader(self):
        source = Path('macos/src-tauri/clipboard-helper.swift').read_text()
        self.assertTrue('// BEGIN QQ CACHE ACCESS' in source, 'QQ cache authorization reader is missing')
        functions = source.split('// BEGIN QQ CACHE ACCESS\n', 1)[1].split('// END QQ CACHE ACCESS', 1)[0]
        harness = r'''
import AppKit
import Darwin
func require(_ condition: Bool, _ message: String) {
    if !condition { fputs("FAIL: \(message)\n", stderr); exit(1) }
}
let temp = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
let home = temp.appendingPathComponent("home", isDirectory: true).path
let root = home + "/Library/Containers/com.tencent.qq/Data/Library/Application Support/QQ/nt_qq_123abc/nt_data/Pic"
let directory = URL(fileURLWithPath: root, isDirectory: true)
let image = root + "/2026-10/Ori/" + String(repeating: "a", count: 32) + ".png"
let bookmark = temp.appendingPathComponent("access/qq-image-cache.bookmark")
let reader = QQImageCacheAccess(home: home, bookmarkURL: bookmark)
if CommandLine.arguments.count == 3 {
    require(reader.status() == "authorized", "bookmark survives helper restart")
    require(reader.authorizedDirectory(for: root) != nil, "saved scope remains bound to the correct account")
    require(reader.read(image) == Data("copied image".utf8), "restarted reader uses saved bookmark")
    exit(0)
}
try FileManager.default.createDirectory(atPath: (image as NSString).deletingLastPathComponent, withIntermediateDirectories: true)
try Data("copied image".utf8).write(to: URL(fileURLWithPath: image))
require(reader.read(image) == nil, "disabled access must not read even an unprotected image")
require(reader.status() == "not_authorized", "missing bookmark never claims access")
require(reader.suggestedDirectory() == nil, "no Documents-folder fallback when the QQ account is unknown")
let suggestion = try PropertyListSerialization.data(fromPropertyList: [
    ["TencentElementType": 1, "TencentElementValue": image],
    ["TencentElementType": 11, "TencentElementValue": "copied message"]
], format: .binary, options: 0)
require(reader.rememberSuggestion(from: suggestion), "copied QQ message identifies its exact Pic folder")
require(reader.suggestedDirectory()?.path == root, "folder hint survives a later non-QQ clipboard copy")
require(reader.status() == "not_authorized" && reader.read(image) == nil, "a folder hint never grants image access")
let suggestionMode = try FileManager.default.attributesOfItem(atPath: bookmark.appendingPathExtension("suggestion").path)[.posixPermissions] as! NSNumber
require(suggestionMode.intValue == 0o600, "folder hint is private")
let invalidSuggestion = try PropertyListSerialization.data(fromPropertyList: [
    ["TencentElementType": 1, "TencentElementValue": home + "/other.png"]
], format: .binary, options: 0)
require(!reader.rememberSuggestion(from: invalidSuggestion), "folder hint cannot authorize an arbitrary directory")
require(!reader.rememberSuggestion(from: Data("bad plist".utf8)), "malformed hints are rejected")
let mixedAccounts = try PropertyListSerialization.data(fromPropertyList: [
    ["TencentElementType": 1, "TencentElementValue": image],
    ["TencentElementType": 1, "TencentElementValue": image.replacingOccurrences(of: "nt_qq_123abc", with: "nt_qq_456def")]
], format: .binary, options: 0)
require(!reader.rememberSuggestion(from: mixedAccounts), "ambiguous account hints are rejected")
try reader.remember(directory)
require(reader.status() == "authorized", "directory bookmark resolves")
require(reader.read(image) == Data("copied image".utf8), "resolved bookmark reads the exact referenced image")
try reader.revoke()
require(reader.status() == "not_authorized" && reader.read(image) == nil, "turning off revokes access even on readable fixture paths")
try reader.revoke()
require(reader.suggestedDirectory()?.path == root, "re-enabling can locate the same folder without browsing")
try reader.remember(directory)
let permissions = try FileManager.default.attributesOfItem(atPath: bookmark.path)[.posixPermissions] as! NSNumber
require(permissions.intValue == 0o600, "bookmark is private")
for suffix in ["/../2026-10/Ori/" + String(repeating: "a", count: 32) + ".png", "/2026-10/Ori/other.png", "/2026-10/Ori/" + String(repeating: "a", count: 32) + ".png\n", "/2026-10/Ori/" + String(repeating: "a", count: 32) + ".png\0"] {
    require(reader.read(root + suffix) == nil, "refuse malformed image path")
}
require(QQImageCachePath(image, home: home)?.root == root, "image yields its account Pic directory")
require(QQImageCachePath.cacheRoot(root, home: home), "only exact Pic directory can be selected")
for wrong in [home, (root as NSString).deletingLastPathComponent, root + "/2026-10", root + "/", root + "\n", root.replacingOccurrences(of: "nt_qq_123abc", with: "nt_qq_bad-name")] {
    require(!QQImageCachePath.cacheRoot(wrong, home: home), "refuse broad or malformed selected directory")
    if wrong != root + "/" { // URL canonicalizes a trailing separator to the same exact directory.
        do { try reader.remember(URL(fileURLWithPath: wrong)); require(false, "invalid selection must throw") } catch {}
    }
}
require(reader.status() == "authorized", "invalid selection preserves previous bookmark")
// A valid bookmark for a different account must not be used for this message.
let otherRoot = root.replacingOccurrences(of: "nt_qq_123abc", with: "nt_qq_456def")
try FileManager.default.createDirectory(atPath: otherRoot, withIntermediateDirectories: true)
try reader.remember(URL(fileURLWithPath: otherRoot))
require(reader.authorizedDirectory(for: root) == nil, "account scope does not bleed into another account")
try reader.remember(directory)
// Never follow file, month, cache-root, or bookmark symlinks.
try FileManager.default.removeItem(atPath: image)
let outside = temp.appendingPathComponent("outside.png")
try Data("outside".utf8).write(to: outside)
try FileManager.default.createSymbolicLink(atPath: image, withDestinationPath: outside.path)
require(reader.read(image) == nil, "file symlink")
try FileManager.default.removeItem(atPath: image)
let month = directory.appendingPathComponent("2026-10")
let moved = temp.appendingPathComponent("moved-month")
try FileManager.default.moveItem(at: month, to: moved)
try FileManager.default.createSymbolicLink(at: month, withDestinationURL: moved)
require(reader.read(image) == nil, "directory symlink")
try FileManager.default.removeItem(at: month)
try FileManager.default.moveItem(at: moved, to: month)
let large = Data(repeating: 0, count: 4 * 1024 * 1024 + 1)
try large.write(to: URL(fileURLWithPath: image))
require(reader.read(image) == nil, "oversized image")
try Data().write(to: URL(fileURLWithPath: image))
require(reader.read(image) == nil, "empty image")
try FileManager.default.removeItem(at: bookmark)
let evilBookmark = temp.appendingPathComponent("evil.bookmark")
try Data(repeating: 0, count: 64 * 1024 + 1).write(to: evilBookmark)
try FileManager.default.createSymbolicLink(at: bookmark, withDestinationURL: evilBookmark)
require(reader.status() == "not_authorized", "bookmark symlink")
try FileManager.default.removeItem(at: bookmark)
try Data(repeating: 0, count: 64 * 1024 + 1).write(to: bookmark)
require(reader.status() == "not_authorized", "oversized bookmark")
try Data("corrupt".utf8).write(to: bookmark)
require(reader.status() == "not_authorized", "corrupt bookmark")
try reader.remember(directory)
// Four images read by the production QQ converter through the bookmark reader.
let colors = [@@COLORS@@]
var rows: [[String: Any]] = []
for (index, bytes) in colors.enumerated() {
    let file = root + "/2026-10/Ori/" + String(repeating: String(index + 1), count: 32) + ".png"
    try Data(base64Encoded: bytes)!.write(to: URL(fileURLWithPath: file))
    rows.append(["TencentElementType": 1, "TencentElementValue": file])
    rows.append(["TencentElementType": 11, "TencentElementValue": "after image \(index + 1)\n"])
}
let message = try PropertyListSerialization.data(fromPropertyList: rows, format: .binary, options: 0)
let result = qqToPortable(message, itemCount: 4, loadImage: reader.read)!
let html = String(data: result.html, encoding: .utf8)!
require(result.missing == 0, "all four cache images are captured")
require(html.components(separatedBy: "data:image/png;base64,").count == 5, "all four images are embedded")
require(!html.contains(root), "cache paths never leak into portable content")
var offset = html.startIndex
for index in 1...4 {
    let imageRange = html.range(of: "<img", range: offset..<html.endIndex)!
    let textRange = html.range(of: "after image \(index)", range: imageRange.upperBound..<html.endIndex)!
    offset = textRange.upperBound
}
try Data("copied image".utf8).write(to: URL(fileURLWithPath: image))
let config = QQImageCacheAccess.configurationURL(environment: ["TAILSYNC_DATA_DIR": temp.path], applicationSupport: temp.appendingPathComponent("support"))
require(config.path == temp.appendingPathComponent("qq-image-cache.bookmark").path, "isolation root is respected")
print("PASS: QQ bookmark persistence, exact account scope, bounded reads, symlinks, invalid selection, corrupted state, isolation")
'''
        with tempfile.TemporaryDirectory(prefix='tailsync-qq-cache-') as tmp:
            path = Path(tmp).resolve()
            bitmap = source.split('func imageWithinBudget', 1)[1].split('// QQ advertises', 1)[0]
            converter = source.split('func qqToPortable', 1)[1].split('/// RTFD attachments', 1)[0]
            endian = source.split('func readBE', 1)[1].split('func validNativeName', 1)[0]
            harness = harness.replace('@@COLORS@@', ','.join('"' + __import__('base64').b64encode(png(color)).decode() + '"' for color in [[255,0,0,255],[0,255,0,255],[0,0,255,255],[255,255,0,255]]))
            prelude = 'import AppKit\nimport ImageIO\nimport Darwin\nlet policyCoverFrameLimit = 20\nlet policyCoverPixelLimit = 8388608\n'
            dependencies = 'func readBE' + endian + 'func imageWithinBudget' + bitmap
            fallback = 'func readQQAttachment(_ path: String) -> Data? { nil }\n'
            (path / 'probe.swift').write_text(prelude + dependencies + functions + fallback + 'func qqToPortable' + converter + harness.replace('import AppKit\nimport Darwin\n', ''))
            subprocess.run(['swiftc', str(path / 'probe.swift'), '-o', str(path / 'probe')], check=True, timeout=60)
            subprocess.run([str(path / 'probe'), str(path)], check=True, timeout=15)
            subprocess.run([str(path / 'probe'), str(path), '--reopen'], check=True, timeout=15)


if __name__ == '__main__':
    unittest.main()
