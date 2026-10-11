#!/bin/bash
# RR-01 regression (repair plan §4 R0.2 / §7 item A, T01).
#
# The default macOS build must actually reach `cargo build`, and the feature
# argument must be exactly right in both states. Before the repair it did not:
# `macos/build-mac.sh` expanded an empty `CARGO_FEATURE_ARGS[@]` under `set -u`,
# and macOS system bash 3.2 aborts with "unbound variable", so the
# README-documented build, the DMG build, the release verification script and the
# CI packaging gate all failed in the default (feature-off) configuration.
#
# This uses a `cargo` stub, so it asserts the *argument shape* without a real
# build. A real build in both states is required by gate R1 and is recorded
# separately; this regression only proves the script reaches cargo and passes the
# right flags. It must run under the system bash (`/bin/bash`).
#
# Usage: bash scripts/check-build-mac-feature-args.sh
# Exit 0 = the script behaves; non-zero = the defect is present or the shape drifted.

set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

mkdir -p "$TMP/bin"

# `cargo metadata` must answer the version probe; `cargo build` must record argv
# and stop the script after RR01_STOP_AFTER builds (so the universal branch can
# record both of its target builds before the script is stopped).
cat > "$TMP/bin/cargo" <<'STUB'
#!/bin/bash
case "${1:-}" in
  metadata)
    printf '{"packages":[{"version":"0.0.0-rr01"}]}\n'
    exit 0
    ;;
  build)
    printf '%s\n' "$*" >> "${RR01_ARGS_LOG:?}"
    recorded="$(wc -l < "${RR01_ARGS_LOG}" | tr -d ' ')"
    if [[ "$recorded" -ge "${RR01_STOP_AFTER:-1}" ]]; then
      exit 88
    fi
    exit 0
    ;;
  *)
    exit 0
    ;;
esac
STUB
chmod +x "$TMP/bin/cargo"

# Keep the universal branch from needing network or real binaries.
printf '#!/bin/bash\nexit 0\n' > "$TMP/bin/rustup"
printf '#!/bin/bash\nexit 0\n' > "$TMP/bin/lipo"
chmod +x "$TMP/bin/rustup" "$TMP/bin/lipo"

failures=0
check() { # check <description> <condition-result>
    if [[ "$2" == "0" ]]; then
        printf 'PASS  %s\n' "$1"
    else
        printf 'FAIL  %s\n' "$1"
        failures=$((failures + 1))
    fi
}

run_case() { # run_case <label> <composite-env|unset> <target> <stop-after>
    local label="$1" composite="$2" target="$3" stop_after="$4"
    : > "$TMP/$label.args"
    local -a env_args=(
        "RR01_ARGS_LOG=$TMP/$label.args"
        "RR01_STOP_AFTER=$stop_after"
        "PATH=$TMP/bin:$PATH"
        "TAILSYNC_APP_BUNDLE=$TMP/$label.app"
        "TAILSYNC_MACOS_TARGET=$target"
    )
    if [[ "$composite" != "unset" ]]; then
        env_args+=("TAILSYNC_COMPOSITE_CLIPBOARD=$composite")
    fi
    ( cd "$ROOT/macos" && env "${env_args[@]}" /bin/bash ./build-mac.sh --skip-swift-build ) \
        > "$TMP/$label.log" 2>&1
    return $?
}

printf 'bash: %s\n\n' "$(/bin/bash --version | head -1)"

# --- default (feature off), native -----------------------------------------
run_case default-native unset native 1
default_rc=$?
default_args="$(cat "$TMP/default-native.args")"
printf 'default/native: exit=%s args=%s\n' "$default_rc" "${default_args:-<none>}"
check "default build reaches cargo build (feature off must not abort the script)" \
    "$([[ "$default_rc" == "88" ]] && echo 0 || echo 1)"
check "default build passes no --features flag" \
    "$([[ -n "$default_args" && "$default_args" != *"--features"* ]] && echo 0 || echo 1)"
check "default build keeps the original cargo arguments" \
    "$([[ "$default_args" == "build --locked --release --manifest-path src-tauri/Cargo.toml" ]] && echo 0 || echo 1)"

# --- enabled, native -------------------------------------------------------
run_case enabled-native 1 native 1
enabled_rc=$?
enabled_args="$(cat "$TMP/enabled-native.args")"
printf 'enabled/native: exit=%s args=%s\n' "$enabled_rc" "${enabled_args:-<none>}"
check "enabled build reaches cargo build" \
    "$([[ "$enabled_rc" == "88" ]] && echo 0 || echo 1)"
check "enabled build passes exactly the composite feature" \
    "$([[ "$enabled_args" == *"--features protocol-composite-clipboard"* ]] && echo 0 || echo 1)"

# --- default, universal ----------------------------------------------------
# The universal branch issues two cargo builds (arm64 then x86_64); stop after the
# second so both argv lines are recorded.
run_case default-universal unset universal-apple-darwin 2
universal_rc=$?
universal_builds="$(wc -l < "$TMP/default-universal.args" | tr -d ' ')"
printf 'default/universal: exit=%s builds=%s\n' "$universal_rc" "$universal_builds"
check "universal default build reaches both target builds" \
    "$([[ "$universal_builds" == "2" ]] && echo 0 || echo 1)"
check "universal default build passes no --features flag" \
    "$(! grep -q -- '--features' "$TMP/default-universal.args" && echo 0 || echo 1)"
check "universal default build keeps both target arguments" \
    "$(grep -q -- '--target aarch64-apple-darwin' "$TMP/default-universal.args" \
        && grep -q -- '--target x86_64-apple-darwin' "$TMP/default-universal.args" && echo 0 || echo 1)"

# --- enabled, universal ----------------------------------------------------
run_case enabled-universal 1 universal-apple-darwin 2
enabled_universal_builds="$(wc -l < "$TMP/enabled-universal.args" | tr -d ' ')"
printf 'enabled/universal: builds=%s\n' "$enabled_universal_builds"
check "universal enabled build passes the feature to both targets" \
    "$([[ "$(grep -c -- '--features protocol-composite-clipboard' "$TMP/enabled-universal.args")" == "2" ]] && echo 0 || echo 1)"

printf '\n'
if [[ "$failures" -ne 0 ]]; then
    printf 'RR-01: %d check(s) failed — the defect is present or the argument shape drifted.\n' "$failures"
    exit 1
fi
printf 'RR-01: all checks passed.\n'
