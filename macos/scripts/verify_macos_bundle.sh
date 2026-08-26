#!/bin/bash
set -euo pipefail

cd "$(dirname "$0")/.."
APP_PATH="$PWD/TailSync.app"
API_PORT=19889
PEER_PORT=19890
DAEMON_STARTED=0
DAEMON_PROCESS_PID=''
DAEMON_LOG=''
API_TOKEN=''
RELEASE_TIER="${TAILSYNC_RELEASE_TIER:-community}"

cleanup() {
    if [ "$DAEMON_STARTED" -eq 1 ]; then
        if [[ -n "$DAEMON_PROCESS_PID" ]] && kill -0 "$DAEMON_PROCESS_PID" >/dev/null 2>&1; then
            kill "$DAEMON_PROCESS_PID" >/dev/null 2>&1 || true
            wait "$DAEMON_PROCESS_PID" >/dev/null 2>&1 || true
        fi
        for _ in {1..50}; do
            if ! /usr/sbin/lsof -nP -iTCP:"$API_PORT" -sTCP:LISTEN >/dev/null 2>&1 &&
                ! /usr/sbin/lsof -nP -iTCP:"$PEER_PORT" -sTCP:LISTEN >/dev/null 2>&1; then
                break
            fi
            sleep 0.1
        done
    fi
    if [[ -n "$DAEMON_LOG" ]]; then
        rm -f "$DAEMON_LOG"
    fi
}
trap cleanup EXIT

report_daemon_failure() {
    if [[ -n "$DAEMON_LOG" && -s "$DAEMON_LOG" ]]; then
        echo 'Packaged daemon output:' >&2
        tail -n 80 "$DAEMON_LOG" >&2 || true
    fi
}

for port in "$API_PORT" "$PEER_PORT"; do
    if /usr/sbin/lsof -nP -iTCP:"$port" -sTCP:LISTEN >/dev/null 2>&1; then
        echo "Port $port is already occupied; quit TailSync before verification." >&2
        exit 1
    fi
done

echo '[1/3] Inspecting the packaged app...'
test -d "$APP_PATH"
test -x "$APP_PATH/Contents/MacOS/TailSync"
test -x "$APP_PATH/Contents/MacOS/tailsyncd"
test -x "$APP_PATH/Contents/MacOS/clipboard-helper"
/usr/bin/plutil -lint "$APP_PATH/Contents/Info.plist" >/dev/null
/usr/libexec/PlistBuddy -c 'Print :NSLocalNetworkUsageDescription' "$APP_PATH/Contents/Info.plist" | grep -q 'local network access'
/usr/libexec/PlistBuddy -c 'Print :NSBonjourServices:0' "$APP_PATH/Contents/Info.plist" | grep -qx '_tailsync._tcp'
codesign --verify --deep --strict "$APP_PATH"
if [[ "$RELEASE_TIER" == "trusted" ]]; then
    xcrun stapler validate "$APP_PATH"
    spctl --assess --type execute --verbose=2 "$APP_PATH"
fi

helper_probe="$(mktemp -t tailsync-clipboard-helper.XXXXXX)"
trap 'rm -f "$helper_probe"; cleanup' EXIT
"$APP_PATH/Contents/MacOS/clipboard-helper" --write-files "$helper_probe"
helper_output="$("$APP_PATH/Contents/MacOS/clipboard-helper")"
if ! printf '%s\n' "$helper_output" | grep -Fxq "$helper_probe"; then
    echo "Packaged clipboard helper round-trip failed: $helper_output" >&2
    exit 1
fi
rm -f "$helper_probe"

# GitHub-hosted macOS jobs may not have a WindowServer session. Launching the
# SwiftUI menu-bar executable directly in that environment can terminate with
# SIGTRAP before it gets a chance to start its child daemon. The daemon is the
# network/API payload used by the UI, so smoke-test that exact packaged binary
# here and keep the AppKit UI checks above structural and code-signature based.
echo '[2/3] Launching the packaged daemon from the app bundle...'
API_TOKEN="$(/usr/bin/openssl rand -hex 32)"
DAEMON_LOG="$(mktemp -t tailsync-daemon.XXXXXX)"
TAILSYNC_API_TOKEN="$API_TOKEN" "$APP_PATH/Contents/MacOS/tailsyncd" >"$DAEMON_LOG" 2>&1 &
DAEMON_PROCESS_PID=$!
DAEMON_STARTED=1
api_ready=0
for _ in {1..100}; do
    if /usr/sbin/lsof -nP -iTCP:"$API_PORT" -sTCP:LISTEN >/dev/null 2>&1 &&
        /usr/sbin/lsof -nP -iTCP:"$PEER_PORT" -sTCP:LISTEN >/dev/null 2>&1; then
        api_ready=1
        break
    fi
    if ! kill -0 "$DAEMON_PROCESS_PID" >/dev/null 2>&1; then
        break
    fi
    sleep 0.1
done
if [ "$api_ready" -ne 1 ]; then
    echo 'Packaged daemon did not listen on both API 19889 and peer 19890 ports.' >&2
    report_daemon_failure
    exit 1
fi

echo '[3/3] Verifying the JSON-lines API...'
response="$(printf '{\"cmd\":\"get_version\",\"token\":\"%s\"}\n' "$API_TOKEN" | /usr/bin/nc -w 3 127.0.0.1 "$API_PORT")"
response_ok="$(printf '%s\n' "$response" | /usr/bin/plutil -extract ok raw -o - - 2>/dev/null || true)"
response_data="$(printf '%s\n' "$response" | /usr/bin/plutil -extract data raw -o - - 2>/dev/null || true)"
if [[ "$response_ok" != "true" || ! "$response_data" =~ ^[0-9]+$ ]]; then
    echo "Unexpected API response: $response" >&2
    exit 1
fi

unauthorized_response="$(printf '{\"cmd\":\"get_version\"}\n' | /usr/bin/nc -w 3 127.0.0.1 "$API_PORT")"
unauthorized_ok="$(printf '%s\n' "$unauthorized_response" | /usr/bin/plutil -extract ok raw -o - - 2>/dev/null || true)"
unauthorized_error="$(printf '%s\n' "$unauthorized_response" | /usr/bin/plutil -extract error raw -o - - 2>/dev/null || true)"
if [[ "$unauthorized_ok" != "false" || "$unauthorized_error" != "unauthorized" ]]; then
    echo "Local API accepted a request without its capability token: $unauthorized_response" >&2
    exit 1
fi

echo "macOS $RELEASE_TIER bundle verification passed."
