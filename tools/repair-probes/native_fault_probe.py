#!/usr/bin/env python3
"""R0 native seam probes for the composite-clipboard repair (plan §4 R0.2/R0.3).

Covers the three native seams that cannot be reached from a pure Rust test:

  RR-03  write-after stall  -> the side effect happened, the receipt was lost
  RR-04  clear-after stall  -> the clipboard was emptied, nothing was written
  RR-07  17 fileURL items   -> classified as a file selection

Everything runs on a **named** pasteboard under the `tailsync-probe.` prefix, so
the user's general clipboard is never touched. The fault variants are produced by
compiling `macos/src-tauri/clipboard-helper.swift` with `-D TAILSYNC_PROBE_FAULTS`
plus one compile-time selector; a product build contains no such code path and no
environment variable can enable it (plan §4 R0.3).

The deadline mirrors `shared/platform-clipboard-composite.rs` `run_helper`:
stdin write, stdout read, `try_wait` poll every 10 ms, kill+wait on expiry. A
client timeout is *not* evidence that nothing happened — that is exactly the
point of these probes.

Usage:
  python3 tools/repair-probes/native_fault_probe.py [--keep] [--json]
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import struct
import subprocess
import sys
import tempfile
import time
import uuid
import zlib

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
HELPER_SWIFT = os.path.join(REPO, "macos", "src-tauri", "clipboard-helper.swift")
FILES_FIXTURE = os.path.join(os.path.dirname(__file__), "files-fixture.swift")

# Production helper deadline in shared/platform-clipboard-composite.rs.
HELPER_DEADLINE_S = 5.0
PROBE_FAULT_STALL_MS = 8000
NATIVE_LIMIT = 40 * 1024 * 1024

TEXT_TYPE = "public.utf8-plain-text"
PNG_TYPE = "public.png"


# --- CNS1 encoding (mirrors shared/tailsync-protocol/src/native_clipboard.rs) ---


def snapshot(formats: list[tuple[int, str, bytes]]) -> bytes:
    items = max(f[0] for f in formats) + 1
    out = bytearray(
        struct.pack(">4sBBHqqH6s", b"CNS1", 1, 0, items, 0, 0, len(formats), bytes(6))
    )
    for item, name, body in formats:
        encoded = name.encode()
        out.extend(struct.pack(">HBBHHI", item, 0, 0, len(encoded), 0, len(body)))
        out.extend(encoded)
        out.extend(body)
    return bytes(out)


def parse_snapshot(data: bytes) -> dict:
    if len(data) < 32 or len(data) > NATIVE_LIMIT:
        raise ValueError("snapshot size")
    magic, version, state, items, before, after, count, reserved = struct.unpack(
        ">4sBBHqqH6s", data[:32]
    )
    if magic != b"CNS1" or version != 1 or reserved != bytes(6) or count > 64:
        raise ValueError("snapshot header")
    offset = 32
    formats = []
    for _ in range(count):
        item, status, pad, name_length, pad2, size = struct.unpack(
            ">HBBHHI", data[offset : offset + 12]
        )
        offset += 12
        if pad or pad2 or size > NATIVE_LIMIT or offset + name_length + size > len(data):
            raise ValueError("snapshot record")
        name = data[offset : offset + name_length].decode("utf-8")
        offset += name_length
        body = data[offset : offset + size]
        offset += size
        formats.append({"item": item, "name": name, "status": status, "bytes": len(body)})
    if offset != len(data):
        raise ValueError("trailing data")
    return {"state": state, "items": items, "before": before, "after": after, "formats": formats}


def png_pixel(rgba: bytes) -> bytes:
    def chunk(kind: bytes, body: bytes) -> bytes:
        return (
            struct.pack(">I", len(body))
            + kind
            + body
            + struct.pack(">I", zlib.crc32(kind + body))
        )

    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", 1, 1, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(b"\0" + rgba))
        + chunk(b"IEND", b"")
    )


# --- helper invocation ---------------------------------------------------------


def read_snapshot(helper: str, board: str) -> dict:
    completed = subprocess.run(
        [helper, "--pasteboard", board, "--read-snapshot"],
        capture_output=True,
        timeout=HELPER_DEADLINE_S + 5,
    )
    if completed.returncode != 0:
        return {"error": f"helper exit {completed.returncode}", "stdout_bytes": len(completed.stdout)}
    try:
        return parse_snapshot(completed.stdout)
    except ValueError as error:
        return {"error": f"decode: {error}", "stdout_bytes": len(completed.stdout)}


def write_with_deadline(helper: str, board: str, body: bytes, deadline: float) -> dict:
    """Mirror run_helper: send stdin, wait for stdout, kill on deadline."""
    token = uuid.uuid4().bytes
    payload = token + struct.pack(">q", -1) + body
    started = time.monotonic()
    child = subprocess.Popen(
        [helper, "--pasteboard", board, "--write-snapshot"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
    )
    try:
        child.stdin.write(payload)
        child.stdin.close()
    except BrokenPipeError:
        pass
    timed_out = False
    stdout = b""
    while True:
        if child.poll() is not None:
            stdout = child.stdout.read() if child.stdout else b""
            break
        if time.monotonic() - started >= deadline:
            timed_out = True
            child.kill()
            child.wait()
            break
        time.sleep(0.01)
    elapsed_ms = int((time.monotonic() - started) * 1000)
    return {
        "client_timed_out": timed_out,
        "client_elapsed_ms": elapsed_ms,
        "helper_exit": child.returncode,
        "receipt_bytes": len(stdout),
        "receipt_received": len(stdout) >= 28,
    }


# --- build ---------------------------------------------------------------------


def build(source: str, out: str, defines: list[str]) -> None:
    cmd = ["swiftc", "-O", "-target", f"{os.uname().machine}-apple-macosx13.0"]
    for define in defines:
        cmd += ["-D", define]
    cmd += [source, "-o", out]
    completed = subprocess.run(cmd, capture_output=True, text=True)
    if completed.returncode != 0:
        raise SystemExit(f"swiftc failed for {defines}: {completed.stderr.strip()}")


def contracts_satisfied(results: dict) -> bool:
    return all(results.get(key, {}).get("satisfied") is True for key in
               ("RR-03-contract", "RR-04-contract", "R6-before-spawn", "R6-output-exceeded")) \
        and results.get("RR-07", {}).get("classified_as_file_selection") is True


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--keep", action="store_true", help="keep the built helpers")
    parser.add_argument("--json", action="store_true", help="emit machine-readable results")
    parser.add_argument("--out-dir", default=None, help="where to build the helper variants")
    parser.add_argument("--product-helper", help="read snapshots using this freshly packaged helper")
    args = parser.parse_args()

    out_dir = args.out_dir or tempfile.mkdtemp(prefix="tailsync-repair-probes-")
    os.makedirs(out_dir, exist_ok=True)

    product = args.product_helper or os.path.join(out_dir, "helper-product")
    after_write = os.path.join(out_dir, "helper-after-write")
    after_clear = os.path.join(out_dir, "helper-after-clear")
    files_fixture = os.path.join(out_dir, "files-fixture")

    if not args.product_helper:
        build(HELPER_SWIFT, product, [])
    elif not os.path.isfile(product):
        raise SystemExit("packaged helper does not exist")
    # The contract probe exercises the production sequence (intent -> supervisor ->
    # resolution boundary -> classify) against a fault helper.
    contract_probe = os.path.join(REPO, "target", "debug", "examples", "native_outcome_probe")
    # A preexisting executable does not certify the current dirty source. Cargo
    # must check freshness on every probe run.
    subprocess.run(
        ["cargo", "build", "--locked", "--offline", "-q", "-p", "tailsync-core",
         "--example", "native_outcome_probe"],
        cwd=REPO, check=True,
    )
    build(
        HELPER_SWIFT,
        after_write,
        ["TAILSYNC_PROBE_FAULTS", "TAILSYNC_PROBE_FAULT_AFTER_WRITE"],
    )
    build(
        HELPER_SWIFT,
        after_clear,
        ["TAILSYNC_PROBE_FAULTS", "TAILSYNC_PROBE_FAULT_AFTER_CLEAR"],
    )
    build(FILES_FIXTURE, files_fixture, [])

    results: dict[str, dict] = {}

    def board() -> str:
        return f"tailsync-probe.repair-{uuid.uuid4()}"

    body = snapshot(
        [
            (0, TEXT_TYPE, b"repair probe text"),
            (0, PNG_TYPE, png_pixel(bytes([9, 8, 7, 255]))),
        ]
    )

    # RR-03: side effect happened, receipt lost.
    name = board()
    write = write_with_deadline(after_write, name, body, HELPER_DEADLINE_S)
    observed = read_snapshot(product, name)
    results["RR-03"] = {
        "seam": "write-after stall: publish done, receipt lost",
        "write": write,
        "clipboard_after": observed,
        "content_present": bool(observed.get("formats")),
        "expected_defect": (
            "client times out, yet the pasteboard already holds the content, so the device "
            "changed its own clipboard without recording a receipt -> next poll classifies "
            "it External and re-broadcasts it (E)"
        ),
    }

    # RR-04: cleared, never written.
    name = board()
    write = write_with_deadline(after_clear, name, body, HELPER_DEADLINE_S)
    observed = read_snapshot(product, name)
    results["RR-04"] = {
        "seam": "clear-after stall: clipboard emptied, nothing written",
        "write": write,
        "clipboard_after": observed,
        "clipboard_empty": observed.get("state") == 2 or observed.get("items") == 0,
        "expected_defect": (
            "client times out while the helper holds the clipboard cleared, so the user's "
            "previous clipboard content is destroyed and no receipt exists (E)"
        ),
    }

    # RR-03 / RR-04 contract: the production sequence must report a typed timeout and
    # must not let the device re-broadcast its own write.
    def contract(helper: str) -> dict:
        completed = subprocess.run(
            [contract_probe, helper, f"tailsync-probe.contract-{uuid.uuid4()}"],
            capture_output=True, text=True, timeout=120,
        )
        if completed.returncode != 0:
            return {"error": completed.stderr.strip() or f"exit {completed.returncode}"}
        return json.loads(completed.stdout)

    after_write_contract = contract(after_write)
    after_clear_contract = contract(after_clear)
    results["RR-03-contract"] = {
        "seam": "post-write stall: typed outcome and no echo",
        "probe": after_write_contract,
        "satisfied": bool(
            after_write_contract.get("timed_out")
            and after_write_contract.get("may_have_had_an_effect")
            and after_write_contract.get("classification") == "unknown_mismatch"
        ),
        "expected": (
            "the deadline must be reported as a timeout (not a no-effect), and the "
            "read-back must classify as unknown_mismatch so the device does not re-broadcast "
            "its own write (K2/K4)"
        ),
    }
    results["RR-04-contract"] = {
        "seam": "post-clear stall: typed outcome, never a no-effect",
        "probe": after_clear_contract,
        "satisfied": bool(
            after_clear_contract.get("timed_out")
            and not after_clear_contract.get("spawn_failed")
            and after_clear_contract.get("clipboard_empty")
        ),
        "expected": (
            "the clipboard was cleared and the write never happened, so the result must "
            "be a timeout that may have had an effect, never a clean no-effect"
        ),
    }

    missing_contract = contract(os.path.join(out_dir, "helper-does-not-exist"))
    results["R6-before-spawn"] = {
        "seam": "missing executable before spawn", "probe": missing_contract,
        "satisfied": bool(missing_contract.get("spawn_failed")
            and missing_contract.get("native_attempts") == 0
            and missing_contract.get("budget_released")),
    }
    flooding = os.path.join(out_dir, "helper-output-flood")
    with open(flooding, "w") as stream:
        stream.write("#!/usr/bin/env python3\nimport os,time\nos.write(1,b'x'*65536)\ntime.sleep(60)\n")
    os.chmod(flooding, 0o700)
    flood_contract = contract(flooding)
    results["R6-output-exceeded"] = {
        "seam": "adversarial helper stdout exceeds cap", "probe": flood_contract,
        "satisfied": bool(flood_contract.get("outcome") == "output_exceeded"
            and flood_contract.get("child_reaped") and flood_contract.get("budget_released")
            and flood_contract.get("native_attempts") == 1),
        "scope": "controlled output-only helper; never writes an OS clipboard",
    }

    # RR-07: 17 fileURL items must not silently take down the file pipeline.
    name = board()
    fixture = subprocess.run(
        [files_fixture, name], capture_output=True, text=True, timeout=30
    )
    observed = read_snapshot(product, name)
    results["RR-07"] = {
        "seam": "17 fileURL native items",
        "fixture_exit": fixture.returncode,
        "fixture_stdout": fixture.stdout.strip(),
        "clipboard_after": observed,
        "helper_read_failed": "error" in observed,
        # R5-01: the helper must classify the shape *before* applying the composite
        # item limit, so a genuine file selection is reported as such (state 5,
        # metadata only) instead of failing the read. The old behaviour was exit 65
        # with empty stdout, which made a 17-file copy invisible to both sync and
        # history.
        "classified_as_file_selection": (
            observed.get("state") == 5
            and observed.get("items") == 17
            and not observed.get("formats")
        ),
        "expected": (
            "a pure file selection is classified before the composite item limit and "
            "reported as SnapshotState::FileSelection with no payload, so the caller "
            "hands it to the file pipeline (H-9/R5-01)"
        ),
    }

    if args.json:
        print(json.dumps(results, indent=2, ensure_ascii=False))
    else:
        for key in sorted(results):
            entry = results[key]
            print(f"--- {key}: {entry['seam']}")
            if key in ("RR-03", "RR-04"):
                print(
                    f"    client_timed_out={entry['write']['client_timed_out']} "
                    f"elapsed={entry['write']['client_elapsed_ms']}ms "
                    f"receipt_received={entry['write']['receipt_received']}"
                )
                print(
                    f"    clipboard_after: state={entry['clipboard_after'].get('state')} "
                    f"items={entry['clipboard_after'].get('items')} "
                    f"formats={len(entry['clipboard_after'].get('formats') or [])}"
                    f"{' error=' + entry['clipboard_after']['error'] if 'error' in entry['clipboard_after'] else ''}"
                )
            elif key.startswith("R6-") or key.endswith("-contract"):
                probe = entry.get("probe", {})
                print(f"    outcome={probe.get('outcome')} attempts={probe.get('native_attempts')} "
                      f"reaped={probe.get('child_reaped')} budget_released={probe.get('budget_released')} "
                      f"satisfied={entry.get('satisfied')}")
            else:
                print(
                    f"    fixture_exit={entry['fixture_exit']} "
                    f"stdout={entry['fixture_stdout']!r} helper_read_failed={entry['helper_read_failed']}"
                )

    if args.keep:
        print(f"\nhelpers kept in {out_dir}", file=sys.stderr)
    else:
        shutil.rmtree(out_dir, ignore_errors=True)
    return 0 if contracts_satisfied(results) else 1


if __name__ == "__main__":
    sys.exit(main())
