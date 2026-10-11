#!/usr/bin/env bash
# R5-07 / H-10,H-11,H-12 — the Windows compatibility-DIB policy.
#
# The acceptance review's H-10 claimed that publishing PNG plus CF_DIBV5 while never
# writing CF_DIB means "consumers that only know CF_DIB cannot paste". Microsoft's
# Clipboard Formats page refutes that as a standalone explanation: CF_DIBV5 -> CF_DIB
# is listed under "Synthesized Clipboard Formats", and the page states that "if the
# system provides an automatic type conversion for a particular clipboard format,
# there is no advantage to placing the conversion format(s) on the clipboard".
# https://learn.microsoft.com/en-us/windows/win32/dataxchg/clipboard-formats
#
# So the compatibility DIB in the Windows adapter must stay what it is: a response to
# an observed primary failure. This guard is structural on purpose — the behaviour
# itself can only be judged on a real Windows target (plan APP-07 / R7), and this
# script must never be presented as that evidence.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ADAPTER="$ROOT/windows/src-tauri/src/sync_adapter.rs"
HELPER="$ROOT/tools/clipboard-helper/src/windows.rs"

fail() { echo "FAIL: $1" >&2; exit 1; }

[[ -f "$ADAPTER" ]] || fail "missing $ADAPTER"
[[ -f "$HELPER" ]] || fail "missing $HELPER"

# 1. The fallback must be reached only from the primary-failure arm.
python3 - "$ADAPTER" <<'PY' || exit 1
import re, sys
source = open(sys.argv[1], encoding="utf-8").read()
calls = [m.start() for m in re.finditer(r"write_clipboard_image\s*\(", source)]
if not calls:
    sys.exit("FAIL: the compatibility-DIB fallback call disappeared; H-10's concern was "
             "never verified, so removing the fallback needs its own evidence")
for at in calls:
    preceding = source[max(0, at - 2000):at]
    # The nearest enclosing failure arm must be the primary write's Err arm.
    if not re.search(r"Err\(\s*primary\s*\)\s*=>\s*\{", preceding):
        sys.exit("FAIL: write_clipboard_image is not reached from the primary-failure arm; a "
                 "speculative DIB write is exactly what R5-07 forbids")
print(f"ok: {len(calls)} compatibility-DIB call site(s), all guarded by an observed failure")
PY

# 2. The rule and its citation must stay documented where the decision is made.
grep -q 'clipboard-formats' "$ADAPTER" \
    || fail "the Windows fallback lost its citation of the verified synthesis behaviour"
grep -q 'Synthesized Clipboard Formats' "$HELPER" \
    || fail "the helper lost the note explaining why CF_DIB is deliberately not written"

# 3. The helper must not start writing a redundant CF_DIB alongside CF_DIBV5.
python3 - "$HELPER" <<'PY' || exit 1
import re, sys
source = open(sys.argv[1], encoding="utf-8").read()
# Only the *publish* direction matters: the read path legitimately accepts CF_DIB.
publish = source[source.index("fn publish("):]
publish = publish[:publish.index("\npub fn run()")]
if re.search(r"prepared\.push\(\s*\(\s*DIB\s*,", publish):
    sys.exit("FAIL: the helper now publishes a redundant CF_DIB next to CF_DIBV5, which the "
             "system already synthesizes on request")
print("ok: the publish path does not write a redundant CF_DIB")
PY

echo "Windows compatibility-DIB policy: ok (structural only — real-app behaviour is APP-07/R7)"
