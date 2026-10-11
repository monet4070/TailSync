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
    "macOS": root / "macos/src-tauri/src/clipboard.rs",
    "Windows": root / "windows/src-tauri/src/clipboard.rs",
}

# macOS persists both outcomes through CNA1; Windows uses CNS1/composite.
for platform, path in targets.items():
    if not path.exists():
        sys.exit(f"FAIL: missing {path}")
    source = path.read_text(encoding="utf-8")
    pattern = r"\.add_native_archive\(" if platform == "macOS" else r"\.add_(?:composite|native_snapshot)\("
    calls = list(re.finditer(pattern, source))
    if len(calls) != 2:
        sys.exit(f"FAIL: {platform}: expected both portable and native capture writes")
    for call in calls:
        end = source.find("\n                    Ok(Ok(", call.end())
        branch = source[call.start():end if end >= 0 else len(source)]
        if "record_capture_not_saved" not in branch:
            sys.exit(f"FAIL: {platform}: capture storage failure is not visible")
        if "record_saved_capture" not in branch:
            sys.exit(f"FAIL: {platform}: successful capture bypasses shared outcome facts")
print("ok: both platforms distinguish successful capture from failed persistence")
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
