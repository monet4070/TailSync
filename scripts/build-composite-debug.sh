#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
# Isolated build output, same as the packaged daemon's production source.
export CARGO_TARGET_DIR="$PWD/macos/src-tauri/target-macos"
export MACOSX_DEPLOYMENT_TARGET=13.0
cargo build --locked --offline --manifest-path macos/src-tauri/Cargo.toml --features protocol-composite-clipboard
swiftc -Onone -target "$(uname -m)-apple-macosx13.0" macos/src-tauri/clipboard-helper.swift -o "$CARGO_TARGET_DIR/debug/clipboard-helper"
"$CARGO_TARGET_DIR/debug/tailsync" --tailsync-clipboard-helper-check
