#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")"
APP_NAME="TailSync"
BUNDLE="$APP_NAME.app"
STAGING_BUNDLE="$APP_NAME.app.staging"
APP_VERSION="$(cargo metadata --no-deps --format-version 1 --manifest-path src-tauri/Cargo.toml | /usr/bin/plutil -extract packages.0.version raw -o - -)"
SIGN_IDENTITY="${TAILSYNC_CODESIGN_IDENTITY:--}"
FORMAL_RELEASE="${TAILSYNC_RELEASE:-0}"
RELEASE_TIER="${TAILSYNC_RELEASE_TIER:-community}"
MACOS_TARGET="${TAILSYNC_MACOS_TARGET:-native}"
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

if [[ "$RELEASE_TIER" != "community" && "$RELEASE_TIER" != "trusted" ]]; then
    echo "TAILSYNC_RELEASE_TIER must be community or trusted." >&2
    exit 2
fi

if [[ "$FORMAL_RELEASE" == "1" ]]; then
    if [[ "$RELEASE_TIER" == "trusted" && "$SIGN_IDENTITY" == "-" ]]; then
        echo "TAILSYNC_CODESIGN_IDENTITY is required for a trusted release." >&2
        exit 1
    fi
fi

if [[ "$FORMAL_RELEASE" == "1" && "$RELEASE_TIER" == "community" ]]; then
    SIGN_IDENTITY="-"
fi

echo "═══ Building $APP_NAME.app ═══"

# ── Build binaries ─────────────────────────────────────────────
echo "[1/6] Building Rust daemon..."
if [[ "$MACOS_TARGET" == "universal-apple-darwin" ]]; then
    rustup target add aarch64-apple-darwin x86_64-apple-darwin
    cargo build --locked --release --target aarch64-apple-darwin --manifest-path src-tauri/Cargo.toml
    cargo build --locked --release --target x86_64-apple-darwin --manifest-path src-tauri/Cargo.toml
    mkdir -p "$RUST_TARGET_DIR/release"
    lipo -create \
        "$RUST_TARGET_DIR/aarch64-apple-darwin/release/tailsync" \
        "$RUST_TARGET_DIR/x86_64-apple-darwin/release/tailsync" \
        -output "$RUST_TARGET_DIR/release/tailsync"
elif [[ "$MACOS_TARGET" == "native" ]]; then
    cargo build --locked --release --manifest-path src-tauri/Cargo.toml
else
    echo "Unsupported TAILSYNC_MACOS_TARGET: $MACOS_TARGET" >&2
    exit 2
fi

echo "[2/6] Building SwiftUI app..."
if $SKIP_SWIFT_BUILD; then
    SWIFT_BIN_DIR="$(swift build -c release --show-bin-path --package-path swift-ui)"
    test -x "$SWIFT_BIN_DIR/TailSync"
    echo "Using existing verified release binary."
else
    if [[ "$MACOS_TARGET" == "universal-apple-darwin" ]]; then
        swift build -c release --arch arm64 --arch x86_64 --package-path swift-ui
        SWIFT_BIN_DIR="$(swift build -c release --arch arm64 --arch x86_64 --show-bin-path --package-path swift-ui)"
    else
        swift build -c release --package-path swift-ui
        SWIFT_BIN_DIR="$(swift build -c release --show-bin-path --package-path swift-ui)"
    fi
fi

echo "[3/6] Building clipboard helper..."
HELPER_BIN="$RUST_TARGET_DIR/release/clipboard-helper"
mkdir -p "$(dirname "$HELPER_BIN")"
if [[ "$MACOS_TARGET" == "universal-apple-darwin" ]]; then
    HELPER_ARM64="$RUST_TARGET_DIR/release/clipboard-helper-arm64"
    HELPER_X86_64="$RUST_TARGET_DIR/release/clipboard-helper-x86_64"
    (cd src-tauri && swiftc -O -target arm64-apple-macosx13.0 clipboard-helper.swift -o "$HELPER_ARM64")
    (cd src-tauri && swiftc -O -target x86_64-apple-macosx13.0 clipboard-helper.swift -o "$HELPER_X86_64")
    lipo -create "$HELPER_ARM64" "$HELPER_X86_64" -output "$HELPER_BIN"
    rm -f "$HELPER_ARM64" "$HELPER_X86_64"
else
    (cd src-tauri && swiftc -O clipboard-helper.swift -o "$HELPER_BIN")
fi
chmod +x "$HELPER_BIN"

# ── Create bundle structure ────────────────────────────────────
echo "[4/6] Creating .app bundle..."
rm -rf "$STAGING_BUNDLE"
mkdir -p "$STAGING_BUNDLE/Contents/MacOS"
mkdir -p "$STAGING_BUNDLE/Contents/Resources"

# Copy binaries
cp "$SWIFT_BIN_DIR/TailSync" "$STAGING_BUNDLE/Contents/MacOS/TailSync"
cp "$RUST_TARGET_DIR/release/tailsync" "$STAGING_BUNDLE/Contents/MacOS/tailsyncd"
cp "$HELPER_BIN" "$STAGING_BUNDLE/Contents/MacOS/clipboard-helper"
chmod +x "$STAGING_BUNDLE/Contents/MacOS/"*

# Copy icon (use the one from src-tauri/icons, or create a placeholder)
if [ -f src-tauri/icons/icon.icns ]; then
    cp src-tauri/icons/icon.icns "$STAGING_BUNDLE/Contents/Resources/icon.icns"
fi

# The client compares this signed metadata with latest.json before installing.
printf '{"schema":1,"product":"TailSync","version":"%s"}\n' "$APP_VERSION" \
    > "$STAGING_BUNDLE/Contents/Resources/tailsync-update.json"

# ── Create Info.plist ──────────────────────────────────────────
cat > "$STAGING_BUNDLE/Contents/Info.plist" << PLIST
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
    <string>$APP_VERSION</string>
    <key>CFBundleShortVersionString</key>
    <string>$APP_VERSION</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleIconFile</key>
    <string>icon</string>
    <key>LSMinimumSystemVersion</key>
    <string>13.0</string>
    <key>LSUIElement</key>
    <true/>
    <key>CFBundleURLTypes</key>
    <array>
        <dict>
            <key>CFBundleURLName</key>
            <string>com.tailsync.app</string>
            <key>CFBundleURLSchemes</key>
            <array>
                <string>tailsync</string>
            </array>
        </dict>
    </array>
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
/usr/libexec/PlistBuddy -c 'Print :CFBundleURLTypes:0:CFBundleURLSchemes:0' "$STAGING_BUNDLE/Contents/Info.plist" | grep -qx 'tailsync'
grep -Fq "\"version\":\"$APP_VERSION\"" "$STAGING_BUNDLE/Contents/Resources/tailsync-update.json"

rm -rf "$BUNDLE"
mv "$STAGING_BUNDLE" "$BUNDLE"

echo "[6/6] Done!"
echo ""
echo "  📦 $BUNDLE ($(du -sh "$BUNDLE" | cut -f1))"
echo "  Signature: $SIGN_IDENTITY"
echo ""
echo "  Double-click to run, or: open $BUNDLE"
