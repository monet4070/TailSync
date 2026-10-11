#!/bin/bash
# Restore only history into a new root; keep the complete post-v13 source tree.
set -euo pipefail
if [[ $# -ne 2 ]]; then
    echo "usage: $0 <storage-dir> <new-target-dir>" >&2
    exit 64
fi
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
cargo run --locked --offline -q -p tailsync-core --example restore_rollback -- "$1" "$2" --v13
