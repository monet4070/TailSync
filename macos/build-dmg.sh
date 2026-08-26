#!/bin/bash
set -euo pipefail

cd "$(dirname "$0")"

APP_NAME="TailSync"
APP_BUNDLE="$APP_NAME.app"
OUTPUT_DIR="release"
SKIP_APP_BUILD=false
NOTARY_PROFILE="${TAILSYNC_NOTARY_PROFILE:-}"
NOTARY_KEYCHAIN="${TAILSYNC_NOTARY_KEYCHAIN:-}"
SIGN_IDENTITY="${TAILSYNC_CODESIGN_IDENTITY:--}"
FORMAL_RELEASE="${TAILSYNC_RELEASE:-0}"
RELEASE_TIER="${TAILSYNC_RELEASE_TIER:-community}"
TAURI_CLI="${TAILSYNC_TAURI_CLI:-../windows/node_modules/.bin/tauri}"

if [[ "${1:-}" == "--skip-app-build" ]]; then
    SKIP_APP_BUILD=true
elif [[ -n "${1:-}" ]]; then
    echo "Usage: $0 [--skip-app-build]" >&2
    exit 2
fi

if [[ "$RELEASE_TIER" != "community" && "$RELEASE_TIER" != "trusted" ]]; then
    echo "TAILSYNC_RELEASE_TIER must be community or trusted." >&2
    exit 2
fi

if [[ "$FORMAL_RELEASE" == "1" ]]; then
    if [[ "$RELEASE_TIER" == "trusted" && ( "$SIGN_IDENTITY" == "-" || -z "$NOTARY_PROFILE" ) ]]; then
        echo "Trusted releases require TAILSYNC_CODESIGN_IDENTITY and TAILSYNC_NOTARY_PROFILE." >&2
        exit 1
    fi
    for required_name in TAURI_SIGNING_PRIVATE_KEY; do
        if [[ -z "${!required_name:-}" ]]; then
            echo "$required_name is required for every published release." >&2
            exit 1
        fi
    done
    if [[ ! -x "$TAURI_CLI" ]]; then
        echo "Tauri CLI is required to sign the updater archive: $TAURI_CLI" >&2
        exit 1
    fi
fi


if [[ "$FORMAL_RELEASE" == "1" && "$RELEASE_TIER" == "community" ]]; then
    SIGN_IDENTITY="-"
    NOTARY_PROFILE=""
    NOTARY_KEYCHAIN=""
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
CHECKSUM_PATH="$OUTPUT_DIR/$APP_NAME-$VERSION-macOS-$ARCH_LABEL.sha256"
UPDATER_NAME="$APP_NAME-$VERSION-macOS-$ARCH_LABEL.app.tar.gz"
UPDATER_PATH="$OUTPUT_DIR/$UPDATER_NAME"
UPDATER_SIGNATURE_PATH="$UPDATER_PATH.sig"

WORK_DIR=$(mktemp -d "${TMPDIR:-/tmp}/tailsync-dmg.XXXXXX")
STAGING_DIR="$WORK_DIR/TailSync"

cleanup() {
    rm -rf "$WORK_DIR"
}
trap cleanup EXIT

if [[ -n "$NOTARY_PROFILE" ]]; then
    if [[ "$SIGN_IDENTITY" == "-" ]]; then
        echo "TAILSYNC_NOTARY_PROFILE requires TAILSYNC_CODESIGN_IDENTITY." >&2
        exit 1
    fi
    NOTARY_ARGS=(--keychain-profile "$NOTARY_PROFILE")
    if [[ -n "$NOTARY_KEYCHAIN" ]]; then
        NOTARY_ARGS+=(--keychain "$NOTARY_KEYCHAIN")
    fi

    # Notarize and staple the app before copying it into the DMG. Stapling only
    # the source app later would leave the shipped copy without its offline ticket.
    APP_NOTARY_ZIP="$WORK_DIR/$APP_NAME-notary.zip"
    /usr/bin/ditto -c -k --keepParent "$APP_BUNDLE" "$APP_NOTARY_ZIP"
    xcrun notarytool submit "$APP_NOTARY_ZIP" "${NOTARY_ARGS[@]}" --wait
    xcrun stapler staple "$APP_BUNDLE"
    xcrun stapler validate "$APP_BUNDLE"
    spctl --assess --type execute --verbose=2 "$APP_BUNDLE"
fi

mkdir -p "$STAGING_DIR" "$OUTPUT_DIR"
/usr/bin/ditto "$APP_BUNDLE" "$STAGING_DIR/$APP_BUNDLE"
if [[ -n "$NOTARY_PROFILE" ]]; then
    xcrun stapler validate "$STAGING_DIR/$APP_BUNDLE"
    spctl --assess --type execute --verbose=2 "$STAGING_DIR/$APP_BUNDLE"
fi
ln -s /Applications "$STAGING_DIR/Applications"

rm -f "$DMG_PATH" "$CHECKSUM_PATH" "$UPDATER_PATH" "$UPDATER_SIGNATURE_PATH"

echo "Building $DMG_NAME"
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
    xcrun notarytool submit "$DMG_PATH" "${NOTARY_ARGS[@]}" --wait
    xcrun stapler staple "$DMG_PATH"
    xcrun stapler validate "$DMG_PATH"
fi

hdiutil verify "$DMG_PATH"

if [[ "$FORMAL_RELEASE" == "1" ]]; then
    echo "Building and signing $UPDATER_NAME"
    COPYFILE_DISABLE=1 /usr/bin/tar -czf "$UPDATER_PATH" "$APP_BUNDLE"
    "$TAURI_CLI" signer sign "$UPDATER_PATH"
    test -s "$UPDATER_SIGNATURE_PATH"
    emitted_platform=0
    for updater_arch in $ARCHITECTURES; do
        case "$updater_arch" in
            arm64) UPDATER_PLATFORM="darwin-aarch64" ;;
            x86_64) UPDATER_PLATFORM="darwin-x86_64" ;;
            *) continue ;;
        esac
        RELEASE_FRAGMENT="$OUTPUT_DIR/release-$UPDATER_PLATFORM.json"
        printf '{"schema":1,"product":"TailSync","version":"%s","platform":"%s","artifact":"%s","signatureFile":"%s"}\n' \
            "$VERSION" "$UPDATER_PLATFORM" "$UPDATER_NAME" "$(basename "$UPDATER_SIGNATURE_PATH")" \
            > "$RELEASE_FRAGMENT"
        emitted_platform=1
    done
    if [[ "$emitted_platform" -ne 1 ]]; then
        echo "Unsupported updater architecture set: $ARCHITECTURES" >&2
        exit 1
    fi
fi

checksum_files=("$DMG_NAME")
if [[ "$FORMAL_RELEASE" == "1" ]]; then
    checksum_files+=("$UPDATER_NAME" "$(basename "$UPDATER_SIGNATURE_PATH")")
fi
(
    cd "$OUTPUT_DIR"
    shasum -a 256 "${checksum_files[@]}"
) > "$CHECKSUM_PATH"

echo ""
echo "  📀 $DMG_PATH ($(du -sh "$DMG_PATH" | cut -f1))"
echo "  🔎 $CHECKSUM_PATH"
if [[ "$SIGN_IDENTITY" == "-" ]]; then
    echo "  Signature: Community build (ad-hoc app; DMG not notarized)"
else
    echo "  Signature: $SIGN_IDENTITY"
fi
