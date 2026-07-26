#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")"
RUST_TARGET_DIR="$PWD/src-tauri/target-macos"
export CARGO_TARGET_DIR="$RUST_TARGET_DIR"

echo "═══ TailSync v2 Dev Launcher ═══"

# ── Build Rust daemon ──────────────────────────────────────────
echo ""
echo "[1/4] Building Rust daemon..."
(cd src-tauri && cargo build 2>&1) | tail -1

# ── Build SwiftUI app ──────────────────────────────────────────
echo "[2/4] Building SwiftUI app..."
(cd swift-ui && swift build 2>&1) | tail -1

# ── Build native clipboard helper ──────────────────────────────
echo "[3/4] Building clipboard helper..."
(cd src-tauri && swiftc clipboard-helper.swift -o clipboard-helper)
chmod +x src-tauri/clipboard-helper

# ── Kill old processes ─────────────────────────────────────────
echo "[4/4] Starting..."
pkill -9 -f "TailSync$" 2>/dev/null || true
pkill -9 -f "/TailSync.app/Contents/MacOS/tailsyncd" 2>/dev/null || true
pkill -9 -f "target-macos/debug/tailsync" 2>/dev/null || true
pkill -9 -f "target/debug/tailsync" 2>/dev/null || true
sleep 1

# ── Launch SwiftUI (which auto-launches the Rust daemon) ───────
nohup swift-ui/.build/debug/TailSync > /dev/null 2>&1 &
sleep 2

echo ""
echo "✅ TailSync is running"
echo "   SwiftUI: $(pgrep -f 'TailSync$')"
echo "   Daemon:  $(pgrep -f 'target-macos/debug/tailsync')"
echo ""
echo "Look for the clipboard icon in your menu bar 📋"
