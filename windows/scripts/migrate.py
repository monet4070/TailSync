#!/usr/bin/env python3
"""Compatibility entry point for the shared TailSync v1 recovery tool."""

import sys
from pathlib import Path

SHARED_SCRIPTS = Path(__file__).resolve().parents[2] / "shared" / "scripts"
sys.path.insert(0, str(SHARED_SCRIPTS))

from migrate_v1 import main  # noqa: E402


if __name__ == "__main__":
    raise SystemExit(main())
