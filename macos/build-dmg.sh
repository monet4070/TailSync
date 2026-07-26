#!/bin/bash
set -euo pipefail

cd "$(dirname "$0")"

APP_NAME="TailSync"
APP_BUNDLE="$APP_NAME.app"
OUTPUT_DIR="release"
SKIP_APP_BUILD=false
NOTARY_PROFILE="${TAILSYNC_NOTARY_PROFILE:-}"
SIGN_IDENTITY="${TAILSYNC_CODESIGN_IDENTITY:--}"

if [[ "${1:-}" == "--skip-app-build" ]]; then
    SKIP_APP_BUILD=true
elif [[ -n "${1:-}" ]]; then
    echo "Usage: $0 [--skip-app-build]" >&2
    exit 2
fi

if ! $SKIP_APP_BUILD; then
    ./build-mac.sh
fi

if [[ ! -d "$APP_BUNDLE" ]]; then
    echo "Missing $APP_BUNDLE; run ./build-mac.sh first." >&2
    exit 1
fi

codesign --verify --deep --strict "$APP_BUNDLE"
/usr/bin/plutil -lint "$APP_BUNDLE/Contents/Info.plist" >/dev/null

VERSION=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleVersion' "$APP_BUNDLE/Contents/Info.plist")
ARCHITECTURES=$(lipo -archs "$APP_BUNDLE/Contents/MacOS/TailSync")
ARCH_LABEL=${ARCHITECTURES// /-}
DMG_NAME="$APP_NAME-$VERSION-macOS-$ARCH_LABEL.dmg"
DMG_PATH="$OUTPUT_DIR/$DMG_NAME"
CHECKSUM_PATH="$DMG_PATH.sha256"

WORK_DIR=$(mktemp -d "${TMPDIR:-/tmp}/tailsync-dmg.XXXXXX")
STAGING_DIR="$WORK_DIR/TailSync"

cleanup() {
    rm -rf "$WORK_DIR"
}
trap cleanup EXIT

mkdir -p "$STAGING_DIR" "$OUTPUT_DIR"
/usr/bin/ditto "$APP_BUNDLE" "$STAGING_DIR/$APP_BUNDLE"
ln -s /Applications "$STAGING_DIR/Applications"

rm -f "$DMG_PATH" "$CHECKSUM_PATH"

echo "═══ Building $DMG_NAME ═══"
hdiutil create \
    -volname "$APP_NAME $VERSION" \
    -srcfolder "$STAGING_DIR" \
    -format UDZO \
    -imagekey zlib-level=9 \
    -ov \
    "$DMG_PATH"

if [[ "$SIGN_IDENTITY" != "-" ]]; then
    codesign --force --sign "$SIGN_IDENTITY" --timestamp "$DMG_PATH"
    codesign --verify --verbose=2 "$DMG_PATH"
fi

if [[ -n "$NOTARY_PROFILE" ]]; then
    if [[ "$SIGN_IDENTITY" == "-" ]]; then
        echo "TAILSYNC_NOTARY_PROFILE requires TAILSYNC_CODESIGN_IDENTITY." >&2
        exit 1
    fi
    xcrun notarytool submit "$DMG_PATH" \
        --keychain-profile "$NOTARY_PROFILE" \
        --wait
    xcrun stapler staple "$DMG_PATH"
    xcrun stapler validate "$DMG_PATH"
fi

hdiutil verify "$DMG_PATH"
shasum -a 256 "$DMG_PATH" > "$CHECKSUM_PATH"

echo ""
echo "  📀 $DMG_PATH ($(du -sh "$DMG_PATH" | cut -f1))"
echo "  🔎 $CHECKSUM_PATH"
if [[ "$SIGN_IDENTITY" == "-" ]]; then
    echo "  Signature: ad-hoc app; DMG not notarized"
else
    echo "  Signature: $SIGN_IDENTITY"
fi
