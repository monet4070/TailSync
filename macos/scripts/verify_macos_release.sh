#!/bin/bash
set -euo pipefail

cd "$(dirname "$0")/.."
APP_PATH="$PWD/TailSync.app"
APP_ID="com.tailsync.app"
API_PORT=19889
PEER_PORT=19890
APP_STARTED=0
APP_PROCESS_PID=''
API_TOKEN=''
WIN_ROOT="${1:-$(cd .. && pwd)/tailsync-v2-win}"

cleanup() {
    if [ "$APP_STARTED" -eq 1 ]; then
        /usr/bin/osascript -e "tell application id \"$APP_ID\" to quit" >/dev/null 2>&1 || true
        if [[ -n "$APP_PROCESS_PID" ]] && kill -0 "$APP_PROCESS_PID" >/dev/null 2>&1; then
            kill "$APP_PROCESS_PID" >/dev/null 2>&1 || true
        fi
        for _ in {1..50}; do
            if ! /usr/sbin/lsof -nP -iTCP:"$API_PORT" -sTCP:LISTEN >/dev/null 2>&1; then
                break
            fi
            sleep 0.1
        done
    fi
}
trap cleanup EXIT

for port in "$API_PORT" "$PEER_PORT"; do
    if /usr/sbin/lsof -nP -iTCP:"$port" -sTCP:LISTEN >/dev/null 2>&1; then
        echo "Port $port is already occupied; quit TailSync before verification." >&2
        exit 1
    fi
done

echo '[1/6] Checking the Rust daemon...'
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo fmt --manifest-path ../shared/rust-core/Cargo.toml --all -- --check
cargo clippy --locked --manifest-path ../shared/rust-core/Cargo.toml --all-targets -- -D warnings
cargo test --locked --manifest-path ../shared/rust-core/Cargo.toml
cargo clippy --locked --manifest-path src-tauri/Cargo.toml --lib -- -D warnings
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib

echo '[2/6] Checking the SwiftUI frontend...'
swift build -c release --package-path swift-ui

echo '[3/6] Checking the cross-project contract...'
node scripts/check_cross_platform_sync.mjs --win-root "$WIN_ROOT" --mac-root "$PWD"

echo '[4/6] Building and inspecting TailSync.app...'
bash ./build-mac.sh
test -d "$APP_PATH"
test -x "$APP_PATH/Contents/MacOS/TailSync"
test -x "$APP_PATH/Contents/MacOS/tailsyncd"
test -x "$APP_PATH/Contents/MacOS/clipboard-helper"
/usr/bin/plutil -lint "$APP_PATH/Contents/Info.plist" >/dev/null
/usr/libexec/PlistBuddy -c 'Print :NSLocalNetworkUsageDescription' "$APP_PATH/Contents/Info.plist" | grep -q 'local network access'
/usr/libexec/PlistBuddy -c 'Print :NSBonjourServices:0' "$APP_PATH/Contents/Info.plist" | grep -qx '_tailsync._tcp'
codesign --verify --deep --strict "$APP_PATH"
helper_probe="$(mktemp -t tailsync-clipboard-helper.XXXXXX)"
trap 'rm -f "$helper_probe"; cleanup' EXIT
"$APP_PATH/Contents/MacOS/clipboard-helper" --write-files "$helper_probe"
helper_output="$("$APP_PATH/Contents/MacOS/clipboard-helper")"
if ! printf '%s\n' "$helper_output" | grep -Fxq "$helper_probe"; then
    echo "Packaged clipboard helper round-trip failed: $helper_output" >&2
    exit 1
fi
rm -f "$helper_probe"

echo '[5/6] Launching the packaged application...'
API_TOKEN="$(/usr/bin/openssl rand -hex 32)"
TAILSYNC_API_TOKEN="$API_TOKEN" "$APP_PATH/Contents/MacOS/TailSync" >/dev/null 2>&1 &
APP_PROCESS_PID=$!
APP_STARTED=1
api_ready=0
for _ in {1..100}; do
    if /usr/sbin/lsof -nP -iTCP:"$API_PORT" -sTCP:LISTEN >/dev/null 2>&1 &&
        /usr/sbin/lsof -nP -iTCP:"$PEER_PORT" -sTCP:LISTEN >/dev/null 2>&1; then
        api_ready=1
        break
    fi
    sleep 0.1
done
if [ "$api_ready" -ne 1 ]; then
    echo 'Packaged app did not listen on both API 19889 and peer 19890 ports.' >&2
    exit 1
fi

echo '[6/6] Verifying the JSON-lines API...'
response="$(printf '{\"cmd\":\"get_version\",\"token\":\"%s\"}\n' "$API_TOKEN" | /usr/bin/nc -w 3 127.0.0.1 "$API_PORT")"
if ! printf '%s\n' "$response" | grep -Eq '^\{"data":[0-9]+,"ok":true\}$'; then
    echo "Unexpected API response: $response" >&2
    exit 1
fi

echo 'macOS release verification passed: signed bundle, SwiftUI, daemon listeners, and local API are healthy.'
