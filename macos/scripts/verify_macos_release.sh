#!/bin/bash
set -euo pipefail

cd "$(dirname "$0")/.."
WIN_ROOT="${1:-$(cd .. && pwd)/windows}"

bash scripts/check_macos_sources.sh "$WIN_ROOT"
bash ./build-mac.sh --skip-swift-build
bash scripts/verify_macos_bundle.sh
