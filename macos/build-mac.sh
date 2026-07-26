#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")"
APP_NAME="TailSync"
BUNDLE="$APP_NAME.app"
STAGING_BUNDLE="$APP_NAME.app.staging"
SIGN_IDENTITY="${TAILSYNC_CODESIGN_IDENTITY:--}"
SKIP_SWIFT_BUILD=false
if [[ "${1:-}" == "--skip-swift-build" ]]; then
    SKIP_SWIFT_BUILD=true
fi
RUST_TARGET_DIR="$PWD/src-tauri/target-macos"
SWIFT_MODULE_CACHE_DIR="$PWD/swift-ui/.build/module-cache"
export CARGO_TARGET_DIR="$RUST_TARGET_DIR"
export CLANG_MODULE_CACHE_PATH="$SWIFT_MODULE_CACHE_DIR"
export SWIFTPM_MODULECACHE_OVERRIDE="$SWIFT_MODULE_CACHE_DIR"
mkdir -p "$SWIFT_MODULE_CACHE_DIR"

echo "═══ Building $APP_NAME.app ═══"

# ── Build binaries ─────────────────────────────────────────────
echo "[1/6] Building Rust daemon..."
cargo build --release --manifest-path src-tauri/Cargo.toml

echo "[2/6] Building SwiftUI app..."
if $SKIP_SWIFT_BUILD; then
    test -x swift-ui/.build/release/TailSync
    echo "Using existing verified release binary."
else
    swift build -c release --package-path swift-ui
fi

echo "[3/6] Building clipboard helper..."
HELPER_BIN="$RUST_TARGET_DIR/release/clipboard-helper"
mkdir -p "$(dirname "$HELPER_BIN")"
(cd src-tauri && swiftc -O clipboard-helper.swift -o "$HELPER_BIN")
chmod +x "$HELPER_BIN"

# ── Create bundle structure ────────────────────────────────────
echo "[4/6] Creating .app bundle..."
rm -rf "$STAGING_BUNDLE"
mkdir -p "$STAGING_BUNDLE/Contents/MacOS"
mkdir -p "$STAGING_BUNDLE/Contents/Resources"

# Copy binaries
cp swift-ui/.build/release/TailSync "$STAGING_BUNDLE/Contents/MacOS/TailSync"
cp "$RUST_TARGET_DIR/release/tailsync" "$STAGING_BUNDLE/Contents/MacOS/tailsyncd"
cp "$HELPER_BIN" "$STAGING_BUNDLE/Contents/MacOS/clipboard-helper"
chmod +x "$STAGING_BUNDLE/Contents/MacOS/"*

# Copy icon (use the one from src-tauri/icons, or create a placeholder)
if [ -f src-tauri/icons/icon.icns ]; then
    cp src-tauri/icons/icon.icns "$STAGING_BUNDLE/Contents/Resources/icon.icns"
fi

# ── Create Info.plist ──────────────────────────────────────────
cat > "$STAGING_BUNDLE/Contents/Info.plist" << 'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>TailSync</string>
    <key>CFBundleIdentifier</key>
    <string>com.tailsync.app</string>
    <key>CFBundleName</key>
    <string>TailSync</string>
    <key>CFBundleDisplayName</key>
    <string>TailSync</string>
    <key>CFBundleVersion</key>
    <string>2.0.0</string>
    <key>CFBundleShortVersionString</key>
    <string>2.0</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleIconFile</key>
    <string>icon</string>
    <key>LSMinimumSystemVersion</key>
    <string>13.0</string>
    <key>LSUIElement</key>
    <true/>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSLocalNetworkUsageDescription</key>
    <string>TailSync needs local network access to discover devices and synchronize clipboard content.</string>
    <key>NSBonjourServices</key>
    <array>
        <string>_tailsync._tcp</string>
    </array>
</dict>
</plist>
PLIST

# ── Create PkgInfo ─────────────────────────────────────────────
echo "APPL????" > "$STAGING_BUNDLE/Contents/PkgInfo"

echo "[5/6] Signing app bundle..."
SIGN_ARGS=(--sign "$SIGN_IDENTITY" --force --options=runtime)
if [[ "$SIGN_IDENTITY" != "-" ]]; then
    SIGN_ARGS+=(--timestamp)
fi
codesign "${SIGN_ARGS[@]}" "$STAGING_BUNDLE/Contents/MacOS/clipboard-helper"
codesign "${SIGN_ARGS[@]}" "$STAGING_BUNDLE/Contents/MacOS/tailsyncd"
codesign "${SIGN_ARGS[@]}" "$STAGING_BUNDLE/Contents/MacOS/TailSync"
codesign "${SIGN_ARGS[@]}" "$STAGING_BUNDLE"
codesign --verify --deep --strict "$STAGING_BUNDLE"
test -x "$STAGING_BUNDLE/Contents/MacOS/TailSync"
test -x "$STAGING_BUNDLE/Contents/MacOS/tailsyncd"
test -x "$STAGING_BUNDLE/Contents/MacOS/clipboard-helper"
/usr/bin/plutil -lint "$STAGING_BUNDLE/Contents/Info.plist" >/dev/null
/usr/libexec/PlistBuddy -c 'Print :NSLocalNetworkUsageDescription' "$STAGING_BUNDLE/Contents/Info.plist" | grep -q 'local network access'
/usr/libexec/PlistBuddy -c 'Print :NSBonjourServices:0' "$STAGING_BUNDLE/Contents/Info.plist" | grep -qx '_tailsync._tcp'

rm -rf "$BUNDLE"
mv "$STAGING_BUNDLE" "$BUNDLE"

echo "[6/6] Done!"
echo ""
echo "  📦 $BUNDLE ($(du -sh "$BUNDLE" | cut -f1))"
echo "  Signature: $SIGN_IDENTITY"
echo ""
echo "  Double-click to run, or: open $BUNDLE"
