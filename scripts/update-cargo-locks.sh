#!/usr/bin/env bash
set -Eeuo pipefail

usage() {
  cat >&2 <<'EOF'
Usage:
  scripts/update-cargo-locks.sh PACKAGE_SPEC TARGET_VERSION

Examples:
  scripts/update-cargo-locks.sh chacha20@0.10.1 0.10.2
  scripts/update-cargo-locks.sh serde_json 1.0.151

PACKAGE_SPEC may include the current version when multiple versions of the
same crate are present in Cargo.lock.
EOF
}

if [[ $# -ne 2 ]]; then
  usage
  exit 2
fi

package_spec="$1"
target_version="$2"
dependency_name="${package_spec%%@*}"

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repository_root="$(cd -- "$script_dir/.." && pwd)"
cd "$repository_root"

manifests=(
  "Cargo.toml"
  "macos/src-tauri/Cargo.toml"
  "windows/src-tauri/Cargo.toml"
)

lockfiles=(
  "Cargo.lock"
  "macos/src-tauri/Cargo.lock"
  "windows/src-tauri/Cargo.lock"
)

backup_directory="$(mktemp -d)"
updated_count=0

restore_lockfiles() {
  local index

  for index in "${!lockfiles[@]}"; do
    cp -- "$backup_directory/$index.lock" "${lockfiles[$index]}"
  done

  rm -rf -- "$backup_directory"
}

for index in "${!lockfiles[@]}"; do
  cp -- "${lockfiles[$index]}" "$backup_directory/$index.lock"
done

trap restore_lockfiles ERR INT TERM

for index in "${!manifests[@]}"; do
  manifest="${manifests[$index]}"
  lockfile="${lockfiles[$index]}"

  if awk \
    -v needle="name = \"$dependency_name\"" \
    '$0 == needle { found = 1 } END { exit(found ? 0 : 1) }' \
    "$lockfile"
  then
    echo "Updating $dependency_name in $lockfile"
    cargo update \
      --manifest-path "$manifest" \
      -p "$package_spec" \
      --precise "$target_version"

    updated_count=$((updated_count + 1))
  else
    echo "Skipping $lockfile: $dependency_name is not present"
  fi
done

if [[ "$updated_count" -eq 0 ]]; then
  echo "Error: $dependency_name was not found in any Cargo.lock" >&2
  exit 1
fi

echo "Checking that every updated lockfile resolves without modification"

for manifest in "${manifests[@]}"; do
  cargo metadata \
    --manifest-path "$manifest" \
    --locked \
    --no-deps \
    --format-version 1 \
    >/dev/null
done

echo "Running advisory checks"

for manifest in "${manifests[@]}"; do
  cargo deny \
    --manifest-path "$manifest" \
    --locked \
    --all-features \
    check advisories
done

node scripts/check-rustsec-exceptions.mjs .
git diff --check

trap - ERR INT TERM
rm -rf -- "$backup_directory"

echo
echo "Updated lockfiles:"
git diff --name-only -- "${lockfiles[@]}"
echo
echo "Next step: review the diff and run the full CI validation matrix."
