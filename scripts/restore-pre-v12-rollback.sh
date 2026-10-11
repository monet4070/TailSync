#!/bin/bash
# Restore a pre-v12 rollback package into a new directory (repair plan §4 R1-03).
#
# The package is written automatically before a v11 -> v12 upgrade:
#
#   <storage>/pre-v12-rollback/
#     history-before-v12.db        consistent SQLite snapshot (VACUUM INTO)
#     payloads/image-history/...   independent copies of every live payload
#     payloads/file-history/...
#     manifest.json                sizes, digests, permissions, and what is NOT rolled back
#
# The documented rollback order is:
#   1. stop the candidate and confirm it is quiescent;
#   2. archive the post-upgrade database and payloads separately — an older binary
#      cannot read composite history, so those entries are not part of this restore;
#   3. verify the pre-upgrade package (this script does it first);
#   4. restore into a NEW directory (this script refuses an existing target);
#   5. open the restored directory with the previous release binary and read text,
#      images, files and favorites back;
#   6. only then switch the storage path.
#
# identity, trust and effect receipts are deliberately NOT in the package: rolling
# them back would make an old binary re-run system writes whose receipts it lost.
#
# Usage: bash scripts/restore-pre-v12-rollback.sh <storage-dir> <target-dir>

set -euo pipefail

if [[ $# -ne 2 ]]; then
    echo "usage: $0 <storage-dir> <target-dir>" >&2
    exit 64
fi

STORAGE="$1"
TARGET="$2"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PACKAGE="$STORAGE/pre-v12-rollback"

if [[ ! -f "$PACKAGE/manifest.json" ]]; then
    echo "No rollback package at $PACKAGE" >&2
    echo "Nothing to restore: the store was never upgraded past v11." >&2
    exit 66
fi

if [[ -e "$TARGET" ]]; then
    echo "Refusing to restore: $TARGET already exists." >&2
    echo "Restore into a new directory so a live store can never be overwritten." >&2
    exit 65
fi

echo "Package : $PACKAGE"
echo "Target  : $TARGET"
echo
echo "Verifying every file against the manifest before touching anything..."
( cd "$ROOT" && cargo run --locked --offline -q -p tailsync-core \
    --example restore_rollback -- "$STORAGE" "$TARGET" )

cat <<EOF

Restore complete. Remaining steps (see the script header):
  1. open $TARGET with the PREVIOUS release binary and read text, images, files and
     favorites back — a package that missed payload bytes would fail here;
  2. archive the post-upgrade composite history separately;
  3. keep identity, trust and effect receipts as they are;
  4. release the package only when the rollback is no longer needed:
       rm -rf "$PACKAGE"
EOF
