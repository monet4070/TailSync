#!/usr/bin/env python3
"""Capture metadata only, or test a private named macOS pasteboard.

Never prints clipboard bodies or touches the user's general pasteboard in
--self-test. Requires the clipboard-helper built from this same checkout.
"""
import argparse
import base64
import hashlib
import itertools
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

def parse_archive_original(data):
    if len(data) < 32 or len(data) > LIMIT:
        raise ValueError("archive size")
    magic, version, state, platform, complete, before, after, items, count, derived, reserved = struct.unpack(
        ">4sBBBBqqHHH2s", data[:32]
    )
    if magic != b"CNA1" or version != 1 or platform != 1 or complete != 1 or reserved != bytes(2) or count + derived > 64:
        raise ValueError("archive header")
    offset = 32
    for _ in range(count):
        if offset + 12 > len(data):
            raise ValueError("archive record")
        name_length = int.from_bytes(data[offset + 4:offset + 6], "big")
        size = int.from_bytes(data[offset + 8:offset + 12], "big")
        offset += 12 + name_length + size
        if offset > len(data):
            raise ValueError("archive record size")
    header = struct.pack(">4sBBHqqH6s", b"CNS1", 1, state, items, before, after, count, bytes(6))
    return parse_snapshot(header + data[32:offset])

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
    rtf = b"{\\rtf1\\ansi{\\fonttbl{\\f0 Helvetica;}}\\f0\\fs28\\b History\\b0  acceptance\\par second paragraph}"
    fixtures = [
        [("public.utf8-plain-text", "中文 🙂\n".encode()), ("public.html", "<b>中文 🙂</b>".encode())],
        [("public.utf8-plain-text", b"text and image"), ("public.html", b"<b>text and image</b>"), ("public.png", png_pixel([255, 0, 0, 128]))],
        [("public.rtf", rtf)],
        [("public.html", ("<p>embedded two images</p>" + "".join('<img src="data:image/png;base64,' + base64.b64encode(png_pixel(p)).decode() + '">' for p in [[255,0,0,128],[0,255,0,0]])).encode())],
        [(0,"public.utf8-plain-text",b"first item"),(1,"public.utf8-plain-text",b"second item")],
        [(0,"public.rtf",rtf),(1,"public.rtf",rtf)],
    ]
    fixtures.extend(list(order) for order in itertools.permutations([
        ("public.utf8-plain-text", b"intentionally independent plain text"),
        ("public.html", b"<b>independent HTML</b>"),
        ("public.rtf", rtf),
    ]))
    fixtures.append([("public.rtf", rtf), ("public.utf16-external-plain-text", "explicit UTF-16".encode("utf-16"))])
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
            if n != "public.utf16-external-plain-text":  # UTF-16 is archive-only, outside the legacy CNS1 projection.
                assert captured[(i,n)] == body, n
        assert captured[(0,"org.tailsync.clipboard-source")] == token
        # Process boundary: re-read the archive after the publisher exits; every advertised original must still exist.
        archive = parse_archive_original(run(helper, args + ["--read-archive"]))
        assert archive[:4] == (0, items, revision, revision)
        originals = {(i,n): b for i,n,s,b in archive[4] if n != "org.tailsync.clipboard-source" and s == 0}
        assert all(s == 0 for _,_,s,_ in archive[4]), "publisher left an unreadable promised format"
        expected = {(i,n):body for i,n,body in fixture}
        for i,n,body in fixture:
            if n == "public.utf16-external-plain-text":
                expected[(i,"public.utf8-plain-text")] = body.decode("utf-16").encode()
        assert originals == expected, "published original bytes or readable UTF-16 compatibility text changed"
        for i,n,body in fixture:
            if n == "public.rtf":
                analysis = run(helper, ["--analyze-rtf"], originals[(i,n)])
                assert analysis[:6] == b"RTA1\x01\x01" and analysis[7] & 1, "restored RTF lost its bold styling"
        revisions.append(revision)
    # Expected old revision must refuse before mutating the clipboard.
    output = run(helper, args + ["--write-snapshot"], uuid.uuid4().bytes + struct.pack(">q", revisions[0]) + snapshot(fixtures[0]))
    assert output[4] == 3
    assert parse_snapshot(run(helper, args + ["--read-snapshot"]))[3] == revisions[-1]
    # Malformed input must not clear the current contents.
    result = subprocess.run([helper, *args, "--write-snapshot"], input=bytes(57), capture_output=True, timeout=6)
    assert result.returncode != 0
    assert parse_snapshot(run(helper, args + ["--read-snapshot"]))[3] == revisions[-1]
    # WebKit's cosmetic attributes must not reject an otherwise complete RTFD.
    run(helper, args + ["--probe-rtfd"])
    rich_archive = run(helper, args + ["--read-archive"])
    assert int.from_bytes(rich_archive[28:30], "big") == 2, "RTFD lost its HTML/text conversion pair"
    return {"result": "passed", "fixtures": len(fixtures), "pasteboard": name,
            "verified": ["text+HTML", "text+HTML+PNG alpha", "RTF after publisher exit with bold styling", "all six rich-text format orders", "explicit UTF-16 unchanged", "embedded two images", "multiple native items including RTF", "exact archive format set", "operation revision", "superseded before write", "malformed before clear"],
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
