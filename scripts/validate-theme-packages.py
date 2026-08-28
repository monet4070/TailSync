#!/usr/bin/env python3
"""Check that published theme archives contain the checked-in source manifest."""

from __future__ import annotations

import argparse
import json
import sys
import zipfile
from pathlib import Path


def fail(message: str) -> None:
    raise SystemExit(message)


def read_json(path: Path) -> object:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        fail(f"could not read JSON {path}: {error}")


def validate(root: Path) -> int:
    source_root = root / "themes"
    package_root = source_root / "packages"
    checked = 0
    expected_packages: set[str] = set()
    for source in sorted(source_root.glob("*/theme.json")):
        manifest = read_json(source)
        theme_id = manifest.get("id") if isinstance(manifest, dict) else None
        version = manifest.get("version") if isinstance(manifest, dict) else None
        if not isinstance(theme_id, str) or not isinstance(version, str):
            fail(f"{source} must define string id and version")
        slug = source.parent.name
        package = package_root / f"{slug}-{version}.tailsync-theme"
        expected_packages.add(package.name)
        if not package.is_file():
            fail(f"missing versioned theme package for {slug}: {package.name}")
        try:
            with zipfile.ZipFile(package) as archive:
                names = set(archive.namelist())
                if "theme.json" not in names:
                    fail(f"{package} does not contain theme.json")
                packaged = json.loads(archive.read("theme.json"))
        except (OSError, zipfile.BadZipFile, KeyError, json.JSONDecodeError) as error:
            fail(f"invalid theme package {package}: {error}")
        if packaged != manifest:
            fail(f"{package} theme.json differs from {source}")
        if packaged["id"] != theme_id or packaged["version"] != version:
            fail(f"{package} has inconsistent id/version")
        checked += 1
    if checked == 0:
        fail(f"no source themes found under {source_root}")
    stale_packages = sorted(
        package.name for package in package_root.glob("*.tailsync-theme") if package.name not in expected_packages
    )
    if stale_packages:
        fail(f"unversioned or stale theme packages are not publishable: {', '.join(stale_packages)}")
    return checked


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path("."))
    args = parser.parse_args()
    checked = validate(args.root.resolve())
    print(f"Validated {checked} versioned theme package(s).")


if __name__ == "__main__":
    main()
