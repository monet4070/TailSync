#!/usr/bin/env python3
"""Test the helper's actual image functions with checked-in real containers.

Compiles only the bounded image function section. No pasteboard, app or user
library is opened. Fixtures require no generator/decoder dependency at runtime.
"""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
helper = (ROOT / 'macos/src-tauri/clipboard-helper.swift').read_text()
POLICY_BEGIN = '// BEGIN GENERATED NATIVE POLICY'
POLICY_END = '// END GENERATED NATIVE POLICY'
IMAGE_START = 'func imageWithinBudget('
IMAGE_END = 'func rtfdToPortable('
for marker in (POLICY_BEGIN, POLICY_END, IMAGE_START, IMAGE_END):
    if marker not in helper:
        raise SystemExit(f'clipboard-helper.swift is missing required marker: {marker}')
policy = helper[helper.index(POLICY_BEGIN):helper.index(POLICY_END) + len(POLICY_END)]
functions = helper[helper.index(IMAGE_START):helper.index(IMAGE_END)]
for signature in (
    'func imageWithinBudget(_ bytes: Data) -> Bool',
    'func staticCover(_ bytes: Data) -> Data?',
    'func isStaticPng(_ bytes: Data) -> Bool',
):
    if signature not in functions:
        raise SystemExit(f'clipboard-helper.swift is missing image function: {signature}')
for generated in ('let policyImageFormats', 'let policyCoverFrameLimit', 'let policyCoverPixelLimit'):
    if generated not in policy:
        raise SystemExit(f'generated policy block is missing: {generated}')
harness = r'''
let fixtures = URL(fileURLWithPath: CommandLine.arguments[1])
var checks = 0
func check(_ ok: @autoclosure () -> Bool, _ message: String) {
    guard ok() else { fatalError(message) }; checks += 1
}
func load(_ name: String) -> Data { try! Data(contentsOf: fixtures.appendingPathComponent(name)) }
check(isStaticPng(load("transparent.png")), "static PNG passthrough")
check(!isStaticPng(load("animated.png")), "APNG is an original")
check(!isStaticPng(load("single-frame-apng.png")), "one-frame APNG also needs a cover")
check(!isStaticPng(load("color.jpg")), "a one-frame JPEG is not PNG")
check(!imageWithinBudget(load("over-budget.png")), "original retention has a separate pixel budget")
check(staticCover(load("over-budget.png")) == nil, "no over-budget pixel decoding")
for name in ["animated.gif", "animated.png", "single-frame-apng.png", "color.jpg", "color.tiff", "pages.tiff", "animated.webp", "static.webp"] {
    let bytes = load(name)
    guard let source = CGImageSourceCreateWithData(bytes as CFData, nil),
          let frame = CGImageSourceCreateImageAtIndex(source, 0, nil) else {
        check(name.hasSuffix(".webp"), "required decoder unavailable: \(name)")
        check(staticCover(bytes) == nil, "unsupported input has no fabricated cover")
        print("ImageIO unavailable: \(name); original retention remains independent")
        continue
    }
    let cover = staticCover(bytes)
    check(cover != nil, "supported source must derive a bounded cover: \(name)")
    let decoded = CGImageSourceCreateWithData(cover! as CFData, nil)!
    check(CGImageSourceGetCount(decoded) == 1 && isStaticPng(cover!), "cover is static PNG: \(name)")
    let image = CGImageSourceCreateImageAtIndex(decoded, 0, nil)!
    check(image.width == frame.width && image.height == frame.height, "cover dimensions: \(name)")
    let original = NSBitmapImageRep(cgImage: frame)
    let derived = NSBitmapImageRep(cgImage: image)
    for (x,y) in [(1,1),(0,0)] {
        let a = original.colorAt(x:x,y:y)!.usingColorSpace(.deviceRGB)!
        let b = derived.colorAt(x:x,y:y)!.usingColorSpace(.deviceRGB)!
        check(abs(a.alphaComponent-b.alphaComponent) <= 1/255.0, "cover alpha: \(name)")
        if a.alphaComponent > 0 {
            check(abs(a.redComponent-b.redComponent) <= 1/255.0 && abs(a.greenComponent-b.greenComponent) <= 1/255.0 && abs(a.blueComponent-b.blueComponent) <= 1/255.0, "decoded cover color: \(name)")
        }
    }
}
// Bounded image work remains 20 frames. Build a valid 21-frame GIF without
// decoding/reencoding pixels: repeat its first descriptor/compressed stream.
let gif = [UInt8](load("animated.gif"))
let descriptor = gif.firstIndex(of: 0x2c)!
var end = descriptor+10
if gif[descriptor+9] & 0x80 != 0 { end += 3 << (Int(gif[descriptor+9] & 7)+1) }
end += 1
repeat { let n=Int(gif[end]); end += 1+n; if n == 0 { break } } while end < gif.count
var many = Data(gif[..<descriptor])
for _ in 0..<21 { many.append(contentsOf: gif[descriptor..<end]) }
many.append(0x3b)
check(!imageWithinBudget(many), "21 frames cannot relax cover admission")
check(staticCover(many) == nil, "no over-budget frame decoding")
print("Image cover checks: \(checks) passed")
'''
with tempfile.TemporaryDirectory(prefix='tailsync-o03-cover-') as folder:
    source = Path(folder) / 'main.swift'
    source.write_text('import AppKit\nimport ImageIO\n' + policy + '\n' + functions + harness)
    binary = Path(folder) / 'image-covers'
    subprocess.run(['swiftc', str(source), '-o', str(binary)], cwd=ROOT, check=True)
    subprocess.run([str(binary), str(ROOT / 'shared/tailsync-protocol/tests/fixtures/images')], cwd=ROOT, check=True)
