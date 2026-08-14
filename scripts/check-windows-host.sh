#!/usr/bin/env bash
# Host-side verification of the Windows crate.
#
# Runs on macOS without a Windows toolchain: `cargo check`/`cargo test
# --no-run` compile the crate for the host target, which still catches type
# errors, unused imports, and missing fields in both production and test
# code. Windows-native compilation, clippy, packaging, and runtime tests
# remain the job of the Windows CI job.
#
# Note: clippy with -D warnings is intentionally NOT used here — host
# compilation drops `#[cfg(target_os = "windows")]` blocks, producing
# false-positive dead-code warnings for symbols used only under that cfg
# (e.g. tray helpers). The Windows CI job runs the authoritative clippy.
set -euo pipefail

cd "$(dirname "$0")/../windows"

cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo check --locked --manifest-path src-tauri/Cargo.toml --all-targets
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib --no-run

echo "Windows host-side verification passed: fmt, check --all-targets, test --no-run."
