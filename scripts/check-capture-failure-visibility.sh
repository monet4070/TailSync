#!/usr/bin/env bash
# R5-01 — a capture whose history write fails must be a visible outcome.
#
# The acceptance review's D-2/G-6 line: a body that could not be stored must never be
# reported as saved. Before this, both platform capture paths did exactly that — the
# clipboard item had already been read (and, for a composite item, already broadcast),
# the history write failed (typically ENOSPC), and the only trace was a `warn!` in the
# log. The user saw nothing and the copy was gone.
#
# This is a structural routing guard. A synthetic SQLite write rejection is
# exercised by the Core O10 regression; physical ENOSPC remains a device check.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fail() { echo "FAIL: $1" >&2; exit 1; }

python3 - "$ROOT" <<'PY' || exit 1
import re, sys
from pathlib import Path

root = Path(sys.argv[1])
targets = {
    # macOS keeps one transaction implementation; F3-A1 moved the composite
    # capture seam into a private module inside this source, so the guard
    # tracks the same file at the module's indentation. F3-B routes both
    # platforms' failed terminals through the shared canonical include.
    "macOS": root / "macos/src-tauri/src/clipboard.rs",
    "Windows": root / "windows/src-tauri/src/clipboard.rs",
}
shared_terminal = root / "shared/platform-clipboard-capture-terminal.rs"
shared_publish = root / "shared/platform-clipboard-capture-publish.rs"


def code_only(text):
    # Mask Rust comments and literals while preserving offsets, so a name that
    # only appears in a comment or a string can never satisfy the guard.
    masked = list(text)
    i, n = 0, len(text)
    raw_pattern = re.compile(r'(?:br|cr|r)(#{0,255})"')
    character_pattern = re.compile(r"'(?:\\(?:u\{[0-9a-fA-F_]+\}|x[0-9a-fA-F]{2}|.)|[^'\\\r\n])'")
    while i < n:
        two = text[i:i + 2]
        if two == "//":
            end = text.find("\n", i)
            end = n if end < 0 else end
            for j in range(i, end):
                if masked[j] != "\n":
                    masked[j] = " "
            i = end
        elif two == "/*":
            depth, j = 1, i + 2
            while j < n and depth > 0:
                if text[j:j + 2] == "/*":
                    depth += 1
                    j += 2
                elif text[j:j + 2] == "*/":
                    depth -= 1
                    j += 2
                else:
                    j += 1
            for k in range(i, j):
                if masked[k] != "\n":
                    masked[k] = " "
            i = j
        elif (i == 0 or not (text[i - 1].isalnum() or text[i - 1] == "_")) and (raw := raw_pattern.match(text, i)):
            closing = '"' + raw.group(1)
            end = text.find(closing, raw.end())
            j = n if end < 0 else end + len(closing)
            for k in range(i, j):
                if masked[k] != "\n":
                    masked[k] = " "
            i = j
        elif text[i] == "'" and (character := character_pattern.match(text, i)):
            j = character.end()
            for k in range(i, j):
                if masked[k] != "\n":
                    masked[k] = " "
            i = j
        elif text[i] == '"':
            j = i + 1
            while j < n:
                if text[j] == "\\":
                    j += 2
                elif text[j] == '"':
                    j += 1
                    break
                else:
                    j += 1
            for k in range(i, j):
                if masked[k] != "\n":
                    masked[k] = " "
            i = j
        else:
            i += 1
    return "".join(masked)


def call_count(source, name):
    return len(re.findall(r"\b" + re.escape(name) + r"\s*\(", source))


# macOS persists both outcomes through CNA1; Windows uses CNS1/composite.
# F3-C moved the portable publish tail into the shared include: a portable save
# now routes to `publish_saved_portable_capture`, while the native-only save
# still records its own warning directly. Both shapes keep their storage-failure
# route to the shared terminal.
for platform, path in targets.items():
    if not path.exists():
        sys.exit(f"FAIL: missing {path}")
    source = code_only(path.read_text(encoding="utf-8"))
    pattern = r"\.add_native_archive\(" if platform == "macOS" else r"\.add_(?:composite|native_snapshot)\("
    calls = list(re.finditer(pattern, source))
    if len(calls) != 2:
        sys.exit(f"FAIL: {platform}: expected both portable and native capture writes")
    portable_routes = 0
    native_routes = 0
    for call in calls:
        # The next capture arm ends this branch. The arm sits one level under
        # the transaction's `match`, so the guard keys on that indent exactly:
        # a nested `Ok(Ok((id, ..))` projection must not terminate the branch.
        arms = list(re.finditer(r"(?m)^(?P<indent>[ \t]*)Ok\(Ok\(\(", source[:call.start()]))
        if not arms:
            sys.exit(f"FAIL: {platform}: capture write has no enclosing outcome arm")
        indent = arms[-1].group("indent")
        end = source.find("\n" + indent + "Ok(Ok((", call.end())
        branch = source[call.start():end if end >= 0 else len(source)]
        if call_count(branch, "publish_saved_portable_capture"):
            portable_routes += 1
        elif call_count(branch, "record_saved_capture"):
            native_routes += 1
        else:
            sys.exit(f"FAIL: {platform}: successful capture bypasses shared outcome facts")
        if not call_count(branch, "finish_failed_capture") or not re.search(r"\bCaptureFailure::NotSaved\b", branch):
            sys.exit(f"FAIL: {platform}: capture storage failure does not route to the shared terminal")
    if portable_routes != 1 or native_routes != 1:
        sys.exit(
            f"FAIL: {platform}: expected one portable publish route and one native-only saved warning"
        )

# The shared publish stage must itself record the saved warning and run the real
# broadcast/archive tail. A name that only survives in a wrapper arm, a comment
# or a string is not a routed outcome.
if not shared_publish.exists():
    sys.exit(f"FAIL: missing {shared_publish}")
publish = code_only(shared_publish.read_text(encoding="utf-8"))
# Only the live publication function body may satisfy this routing guard.
function = re.search(r"\basync\s+fn\s+publish_saved_portable_capture\s*\(", publish)
if not function:
    sys.exit("FAIL: shared publish stage has no publication function")
start = publish.find("{", function.end())
if start < 0:
    sys.exit("FAIL: shared publish stage has no publication body")
depth, i = 1, start + 1
while i < len(publish) and depth > 0:
    depth += (publish[i] == "{") - (publish[i] == "}")
    i += 1
publish = publish[start + 1:i - 1]
# Both archive paths (the superseded early return and the completed broadcast)
# must stay real calls, so the stage cannot keep the name in one arm only.
for needle, minimum in (
    ("record_saved_capture", 1),
    ("prepare_broadcast", 1),
    ("broadcast_composite_to_peers", 1),
    ("enqueue_automatic_archive", 2),
):
    if call_count(publish, needle) < minimum:
        sys.exit(
            f"FAIL: shared publish stage does not run {needle} ({call_count(publish, needle)} < {minimum})"
        )

# The shared terminal must itself record the visible not-saved warning inside
# its NotSaved arm; a name that only survives in another arm or a comment is
# not a routed failure.
if not shared_terminal.exists():
    sys.exit(f"FAIL: missing {shared_terminal}")
shared = code_only(shared_terminal.read_text(encoding="utf-8"))
arm = re.search(r"CaptureFailure::NotSaved\s*=>\s*\{", shared)
if not arm:
    sys.exit("FAIL: shared capture terminal has no CaptureFailure::NotSaved arm")
depth, i = 1, arm.end()
while i < len(shared) and depth > 0:
    if shared[i] == "{":
        depth += 1
    elif shared[i] == "}":
        depth -= 1
    i += 1
if not call_count(shared[arm.end():i - 1], "record_capture_not_saved"):
    sys.exit("FAIL: shared capture terminal does not record the not-saved warning in its NotSaved arm")
print("ok: both platforms route failed captures to the shared terminal, record the not-saved warning, and publish a stored portable capture through the shared stage")
PY

# The warning kind has to reach both UIs, in both languages.
python3 - "$ROOT" <<'PY' || exit 1
import json, sys
from pathlib import Path

root = Path(sys.argv[1])
checks = {
    "macOS UI": (root / "macos/swift-ui/Sources/TailSync/Views/HistoryView.swift", '"capture_not_saved"'),
    "macOS Loc": (root / "macos/swift-ui/Sources/TailSync/Services/Loc.swift", "history.captureNotSaved"),
    "Windows UI": (root / "windows/src/pages/History.tsx", "capture_not_saved"),
}
for label, (path, needle) in checks.items():
    if needle not in path.read_text(encoding="utf-8"):
        sys.exit(f"FAIL: {label} does not surface the capture_not_saved warning")

for locale in ("en", "zh-CN"):
    path = root / f"windows/src/i18n/{locale}.json"
    if "history.captureNotSaved" not in json.loads(path.read_text(encoding="utf-8")):
        sys.exit(f"FAIL: windows/src/i18n/{locale}.json is missing history.captureNotSaved")

# The Swift side keeps both languages in one file; the key must appear twice.
loc = (root / "macos/swift-ui/Sources/TailSync/Services/Loc.swift").read_text(encoding="utf-8")
if loc.count("history.captureNotSaved") < 2:
    sys.exit("FAIL: the macOS localisation is missing one of the two languages")
print("ok: the warning reaches both UIs in both languages")
PY

echo "Capture-failure visibility: ok (routing guard; SQLite rejection has a separate behavioral regression)"
