#!/bin/bash
set -euo pipefail

# Keep one authoritative macOS build pipeline. The root script builds the
# daemon, SwiftUI shell, clipboard helper, bundle metadata, and icon together.
SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
exec "$SCRIPT_DIR/build-mac.sh" "$@"
