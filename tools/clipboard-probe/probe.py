#!/usr/bin/env python3
"""Capture metadata only, or test a private named macOS pasteboard.

Never prints clipboard bodies or touches the user's general pasteboard in
--self-test. Requires the clipboard-helper built from this same checkout.
"""
import argparse
import base64
import hashlib
import json
import struct
import subprocess
import uuid
import zlib

LIMIT = 40 * 1024 * 1024

def parse_snapshot(data):
    if len(data) < 32 or len(data) > LIMIT:
        raise ValueError("snapshot size")
    magic, version, state, items, before, after, count, reserved = struct.unpack(
        ">4sBBHqqH6s", data[:32]
    )
    if magic != b"CNS1" or version != 1 or reserved != bytes(6) or count > 64:
        raise ValueError("snapshot header")
    offset, formats = 32, []
    for _ in range(count):
        item, status, pad, name_length, pad2, size = struct.unpack(
            ">HBBHHI", data[offset:offset + 12]
        )
        offset += 12
        if pad or pad2 or size > LIMIT or offset + name_length + size > len(data):
            raise ValueError("snapshot record")
        name = data[offset:offset + name_length].decode("utf-8")
        offset += name_length
        body = data[offset:offset + size]
        offset += size
        formats.append((item, name, status, body))
    if offset != len(data):
        raise ValueError("trailing data")
    return state, items, before, after, formats

def snapshot(formats):
    formats = [(0, *f) if len(f) == 2 else f for f in formats]
    data = bytearray(struct.pack(">4sBBHqqH6s", b"CNS1", 1, 0, max(f[0] for f in formats)+1, 0, 0, len(formats), bytes(6)))
    for item, name, body in formats:
        name = name.encode()
        data.extend(struct.pack(">HBBHHI", item, 0, 0, len(name), 0, len(body)))
        data.extend(name)
        data.extend(body)
    return bytes(data)

def png_pixel(rgba):
    # Synthetic codec fixture: a single pixel, not user image processing.
    def chunk(kind, body):
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body))
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 1, 1, 8, 6, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(b"\0" + bytes(rgba))) + chunk(b"IEND", b"")

def run(helper, args, body=None):
    completed = subprocess.run([helper, *args], input=body, capture_output=True, timeout=6)
    if completed.returncode:
        raise RuntimeError(f"helper exit {completed.returncode}")
    if len(completed.stdout) > LIMIT + 32:
        raise RuntimeError("helper output exceeds budget")
    return completed.stdout

def self_test(helper):
    name = "tailsync-probe." + str(uuid.uuid4())
    args = ["--pasteboard", name]
    fixtures = [
        [("public.utf8-plain-text", "中文 🙂\n".encode()), ("public.html", "<b>中文 🙂</b>".encode())],
        [("public.utf8-plain-text", b"text and image"), ("public.html", b"<b>text and image</b>"), ("public.png", png_pixel([255, 0, 0, 128]))],
        [("public.rtf", b"{\\rtf1\\ansi hello}")],
        [("public.html", ("<p>embedded two images</p>" + "".join('<img src="data:image/png;base64,' + base64.b64encode(png_pixel(p)).decode() + '">' for p in [[255,0,0,128],[0,255,0,0]])).encode())],
        [(0,"public.utf8-plain-text",b"first item"),(1,"public.utf8-plain-text",b"second item")],
    ]
    revisions = []
    for fixture in fixtures:
        token = uuid.uuid4().bytes
        receipt = run(helper, args + ["--write-snapshot"], token + struct.pack(">q", -1) + snapshot(fixture))
        magic, result, reserved, revision, returned_token = struct.unpack(">4sB3sq16s", receipt)
        assert (magic, result, reserved, returned_token) == (b"CWR1", 0, bytes(3), token)
        state, items, before, after, formats = parse_snapshot(run(helper, args + ["--read-snapshot"]))
        fixture = [(0,*f) if len(f)==2 else f for f in fixture]
        assert state == 0 and items == max(f[0] for f in fixture)+1 and before == after == revision
        captured = {(i,n): b for i, n, s, b in formats if s == 0}
        for i,n,body in fixture:
            assert captured[(i,n)] == body, n
        assert captured[(0,"org.tailsync.clipboard-source")] == token
        revisions.append(revision)
    # Expected old revision must refuse before mutating the clipboard.
    output = run(helper, args + ["--write-snapshot"], uuid.uuid4().bytes + struct.pack(">q", revisions[0]) + snapshot(fixtures[0]))
    assert output[4] == 3
    assert parse_snapshot(run(helper, args + ["--read-snapshot"]))[3] == revisions[-1]
    # Malformed input must not clear the current contents.
    result = subprocess.run([helper, *args, "--write-snapshot"], input=bytes(57), capture_output=True, timeout=6)
    assert result.returncode != 0
    assert parse_snapshot(run(helper, args + ["--read-snapshot"]))[3] == revisions[-1]
    return {"result": "passed", "fixtures": len(fixtures), "pasteboard": name,
            "verified": ["text+HTML", "text+HTML+PNG alpha", "RTF", "embedded two images", "multiple native items", "operation revision", "superseded before write", "malformed before clear"],
            "general_clipboard_modified": False, "target_app_acceptance": "UNKNOWN"}

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--helper", required=True)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--source-app", default="unknown")
    parser.add_argument("--source-version", default="unknown")
    args = parser.parse_args()
    if args.self_test:
        result = self_test(args.helper)
    else:
        state, items, before, after, formats = parse_snapshot(run(args.helper, ["--read-snapshot"]))
        result = {"source_app": args.source_app, "source_version": args.source_version,
                  "state": state, "items": items, "stable": before == after,
                  "formats": [{"item": i, "name": n, "state": s, "bytes": len(b),
                               "sha256": hashlib.sha256(b).hexdigest()} for i, n, s, b in formats]}
    print(json.dumps(result, ensure_ascii=False, indent=2))

if __name__ == "__main__":
    main()
