#!/bin/bash
set -euo pipefail

cd "$(dirname "$0")/.."
APP_PATH="$PWD/TailSync.app"
APP_ID="com.tailsync.app"
API_PORT=19889
PEER_PORT=19890
APP_STARTED=0
WIN_ROOT="${1:-$(cd .. && pwd)/tailsync-v2-win}"

cleanup() {
    if [ "$APP_STARTED" -eq 1 ]; then
        /usr/bin/osascript -e "tell application id \"$APP_ID\" to quit" >/dev/null 2>&1 || true
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

echo '[1/8] Installing locked frontend dependencies...'
npm ci

echo '[2/8] Checking the shared frontend...'
npm run lint
npm run build

echo '[3/8] Checking the Rust daemon...'
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --lib -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --lib

echo '[4/8] Checking the SwiftUI frontend...'
swift build -c release --package-path swift-ui

echo '[5/8] Checking the cross-project contract...'
node scripts/check_cross_platform_sync.mjs --win-root "$WIN_ROOT" --mac-root "$PWD"

echo '[6/8] Building and inspecting TailSync.app...'
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

echo '[7/8] Launching the packaged application...'
open -n "$APP_PATH"
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

echo '[8/8] Verifying the JSON-lines API...'
response="$(printf '{\"cmd\":\"get_version\"}\n' | /usr/bin/nc -w 3 127.0.0.1 "$API_PORT")"
if ! printf '%s\n' "$response" | grep -Eq '^\{"data":[0-9]+,"ok":true\}$'; then
    echo "Unexpected API response: $response" >&2
    exit 1
fi

echo 'macOS release verification passed: signed bundle, SwiftUI, daemon listeners, and local API are healthy.'
