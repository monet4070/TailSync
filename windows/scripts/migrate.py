#!/usr/bin/env python3
"""Deprecated compatibility entry point for the shared TailSync v1 recovery tool.

It now only reports that manual recovery is unsupported; legacy history is imported
automatically on first launch. See shared/scripts/migrate_v1.py.
"""

import sys
from pathlib import Path

SHARED_SCRIPTS = Path(__file__).resolve().parents[2] / "shared" / "scripts"
sys.path.insert(0, str(SHARED_SCRIPTS))

from migrate_v1 import main  # noqa: E402


if __name__ == "__main__":
    raise SystemExit(main())
