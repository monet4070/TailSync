#!/bin/bash
# Repair-regression gate.
#
# Expectations are read from the ledger
# (`artifacts/composite-repair/2026-10-04-r0/expected-failures.json`), not hardcoded:
# a regression that is still open is expected RED, one recorded under `closed` is
# expected GREEN. Re-running therefore always compares against the current ledger,
# and a check that flips without the ledger changing is reported as UNEXPECTED —
# which is how a fix (or a broken regression) becomes visible.
#
# Each regression runs under its OWN test filter, so a still-red regression can no
# longer mask a fixed one that happens to share a test binary.
#
# Usage: bash scripts/run-repair-regressions.sh [run-id]

set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RUN_ID="${1:-2026-10-04-r0}"
EV="$ROOT/artifacts/composite-repair/$RUN_ID"
LEDGER="$ROOT/artifacts/composite-repair/2026-10-04-r0/expected-failures.json"
LOGS="$EV/logs"
mkdir -p "$LOGS"

echo "repair-regression gate — run id: $RUN_ID"
echo "ledger: ${LEDGER#"$ROOT"/}"
echo

: > "$EV/.raw"

expect_for() { # expect_for <id>
    python3 - "$LEDGER" "$1" <<'PY'
import json,sys
ledger,ident=sys.argv[1],sys.argv[2]
d=json.load(open(ledger))
for entry in d['entries']:
    if entry['id']==ident:
        print(entry.get('expected','red')); raise SystemExit
for entry in d.get('closed',[]):
    if entry['id']==ident:
        print(entry.get('expected','green')); raise SystemExit
print('red')
PY
}

run() { # run <id> <command...>
    local id="$1"; shift
    local log="$LOGS/$id.log"
    local expected; expected="$(expect_for "$id")"
    printf '%-9s ' "$id"
    ( cd "$ROOT" && "$@" ) > "$log" 2>&1
    local rc=$?
    local observed status
    if [[ "$rc" == "0" ]]; then observed=green; else observed=red; fi
    status=ok
    # RR-NATIVE is a probe *runner*: it exits 0 after successfully observing the
    # defects, and its verdict is derived from the JSON it writes below, not from
    # its exit status.
    if [[ "$id" != "RR-NATIVE" && "$observed" != "$expected" ]]; then status=UNEXPECTED; fi
    printf 'exit=%-3s observed=%-5s expected=%-5s %s\n' "$rc" "$observed" "$expected" "$status"
    printf '%s\t%s\t%s\t%s\n' "$id" "$rc" "$observed" "$expected" >> "$EV/.raw"
}

core() { node scripts/run-isolated-tests.mjs -- cargo test --locked --offline -p tailsync-core \
    --features repair-regressions,test-support --lib "$1" -- --test-threads=1; }
runtime() { node scripts/run-isolated-tests.mjs -- cargo test --locked --offline -p tailsync-runtime \
    --features repair-regressions,test-support --test repair_regressions "$1"; }
effect_failures() {
    core 'an_unproven_clipboard_failure_keeps_the_claim_and_is_not_reapplied' || return "$?"
    core 'a_proven_no_effect_failure_releases_the_claim_and_retries'
}

budget_layers() {
    core 'composite::budgets::tests' || return "$?"
    core 'four_connections_from_one_peer_cannot_hold_four_assembly_slots'
}

run RR-01  bash scripts/check-build-mac-feature-args.sh
run RR-02  core 'repair_regressions::rr02'
run RR-05  node scripts/run-isolated-tests.mjs -- cargo test --locked --offline -p tailsync-core \
    --features repair-regressions,test-support --test repair_regressions rr05 -- --test-threads=1
run RR-06  core 'repair_regressions::rr06'
run RR-17  core 'repair_regressions::rr17'
run RR-18  core 'repair_regressions::rr18'
run RR-14  node scripts/run-isolated-tests.mjs -- cargo test --locked --offline -p tailsync-protocol --lib composite::tests
run RR-15  core 'native_preparation_failure_stores_incomplete_without_a_native_effect'
run RR-16  budget_layers
run RR-19  core 'peer::operation_lock'
run RR-20  effect_failures
run RR-08  runtime rr08
run RR-09  runtime rr09
run RR-21  runtime rr21
run RR-22  runtime rr22
run RR-23  core 'rr23_pending'
run RR-24  runtime rr24
run RR-25  node scripts/run-isolated-tests.mjs -- swift test --package-path macos/swift-ui --filter CompositeRestoreLocalizationTests
run RR-26  node scripts/check-local-contracts.mjs
run RR-27  bash scripts/check-windows-dib-policy.sh
run RR-28  bash scripts/check-capture-failure-visibility.sh
run RR-29  core 'composite::provenance'
run RR-30  runtime rr30
run RR-10  core 'composite_receipts::repair_regressions'
run RR-11  node scripts/run-isolated-tests.mjs -- cargo test --locked --offline -p tailsync-core \
    --features repair-regressions,test-support --test repair_regressions rr11 -- --test-threads=1
run RR-12  core 'repair_regressions::rr12'
run RR-13  core 'repair_regressions::rr13'
run RR-NATIVE python3 tools/repair-probes/native_fault_probe.py --json

python3 - "$EV" "$LEDGER" <<'PY'
import json, os, re, sys
ev, ledger_path = sys.argv[1], sys.argv[2]
raw = {}
for line in open(os.path.join(ev, '.raw')):
    parts = line.rstrip('\n').split('\t')
    if len(parts) == 4:
        raw[parts[0]] = {"exit": int(parts[1]), "observed": parts[2], "expected": parts[3]}

try:
    native = json.load(open(os.path.join(ev, 'logs', 'RR-NATIVE.log')))
except Exception:
    native = {}

checks = []
for ident, entry in sorted(raw.items()):
    if ident == "RR-NATIVE":
        continue
    checks.append({"id": ident, "kind": "shell-gate" if ident == "RR-01" else "rust-test",
                   "exit": entry["exit"], "observed": entry["observed"],
                   "expected": entry["expected"]})
    if ident == "RR-25":
        log = open(os.path.join(ev, "logs", ident + ".log")).read() if os.path.exists(os.path.join(ev, "logs", ident + ".log")) else ""
        cases = re.findall(r"Test Case .* passed \(", log)
        checks[-1]["kind"] = "swift-test"
        checks[-1]["executed_tests"] = len(cases)
        if not cases:
            checks[-1]["validity_error"] = "required Swift regression cases did not execute"
    elif ident not in {"RR-01", "RR-26", "RR-27", "RR-28"}:
        log_path = os.path.join(ev, "logs", ident + ".log")
        log = open(log_path).read() if os.path.exists(log_path) else ""
        summaries = re.findall(r"test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed;", log)
        executed = sum(int(passed) + int(failed) for passed, failed in summaries)
        checks[-1]["executed_tests"] = executed
        if executed < (2 if ident == "RR-20" else 1):
            checks[-1]["validity_error"] = "required Rust regression cases did not execute"
# Native cases: the probe key that carries the verdict, and the observation the
# ledger says to expect. The expectation is explicit because the polarity differs:
# a fixed contract is expected to hold, an unfixed defect is expected to still be
# observable.
native_cases = {
    "RR-03": ("RR-03-contract", lambda e: e.get("satisfied")),
    "RR-04": ("RR-04-contract", lambda e: e.get("satisfied")),
    "RR-07": ("RR-07", lambda e: e.get("classified_as_file_selection")),
}
ledger = json.load(open(ledger_path))
expectation = {}
for entry in ledger["entries"] + ledger.get("closed", []):
    if "native_expectation" in entry:
        expectation[entry["id"]] = entry["native_expectation"]
for ident, (source, predicate) in native_cases.items():
    entry = native.get(source)
    satisfied = bool(entry and predicate(entry))
    expected = expectation.get(ident, "defect-observed")
    positive = expected
    negative = "not-satisfied" if expected == "contract-satisfied" else "not-observed"
    checks.append({
        "id": ident,
        "kind": "native-probe",
        "observed": positive if satisfied else negative,
        "expected": expected,
    })

for entry in ledger["entries"] + ledger.get("closed", []):
    if entry["id"] not in {check["id"] for check in checks}:
        checks.append({"id": entry["id"], "kind": "missing-gate", "observed": "not-run", "expected": entry.get("expected", "green"), "validity_error": "ledger regression did not execute"})

unexpected = [c for c in checks if c["observed"] != c["expected"] or c.get("validity_error")]
json.dump({
    "run_id": os.path.basename(ev),
    "note": ("Expectations come from the ledger: open regressions are expected red, closed "
             "ones green. A native probe is expected to keep observing its defect until its "
             "stage lands."),
    "checks": checks,
    "unexpected": unexpected,
}, open(os.path.join(ev, "results.json"), "w"), indent=2, ensure_ascii=False)
os.remove(os.path.join(ev, ".raw"))

print()
print(f"checks: {len(checks)}  unexpected: {len(unexpected)}")
for c in checks:
    print(f"  {c['id']:9s} {c['observed']:16s} expected {c['expected']}")
print()
print(f"wrote {os.path.join(ev, 'results.json')}")
if unexpected:
    print("UNEXPECTED:")
    for c in unexpected:
        print(f"  {c['id']}: observed={c['observed']} expected={c['expected']}")
sys.exit(1 if unexpected else 0)
PY
