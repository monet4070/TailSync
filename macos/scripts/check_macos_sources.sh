#!/bin/bash
set -euo pipefail

cd "$(dirname "$0")/.."
WIN_ROOT="${1:-$(cd .. && pwd)/windows}"

echo '[1/3] Checking Rust sources...'
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo fmt --manifest-path ../shared/rust-core/Cargo.toml --all -- --check
cargo clippy --locked --manifest-path ../shared/rust-core/Cargo.toml --all-targets -- -D warnings
cargo test --locked --manifest-path ../shared/rust-core/Cargo.toml
cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --locked --manifest-path src-tauri/Cargo.toml --all-targets

echo '[2/3] Checking the SwiftUI frontend...'
swift test --package-path swift-ui
swift build -c release --package-path swift-ui

echo '[3/3] Checking cross-platform contracts...'
node scripts/check_cross_platform_sync.mjs --win-root "$WIN_ROOT" --mac-root "$PWD"

echo 'macOS source checks passed.'
