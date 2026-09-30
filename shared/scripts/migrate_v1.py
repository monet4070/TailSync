#!/usr/bin/env python3
"""Deprecated manual TailSync v1 recovery tool.

TailSync imports legacy v1 history automatically on first launch: the migration is
idempotent by content hash, failures are written to `v1-migration-report.json` in the
application data directory, and the original `history.db` / `.fernet_key` are retained
and never deleted automatically. Running this script is therefore no longer supported
and it deliberately does not connect to anything.

Why the old entry point cannot work: it dialled the legacy JSON TCP API on
`127.0.0.1:19889`, which Windows no longer starts, while macOS serves its local API
over a Unix socket that requires peer-PID validation and a capability token. A future
manual path, if one is ever needed, must use that authenticated local IPC rather than
reopening a production TCP listener.

The row-level helpers below (`read_legacy_rows`, `migrate_rows`) still describe the
legacy format and the import protocol, and remain unit-tested, so a supported channel
can reuse them.
"""

from __future__ import annotations

import base64
import sqlite3
from pathlib import Path
from typing import Callable, Iterable, Sequence

DEFAULT_OLD_DIRECTORY = Path.home() / "TailSync_History"
DEFAULT_OLD_DB = DEFAULT_OLD_DIRECTORY / "history.db"
DEFAULT_KEY_FILE = DEFAULT_OLD_DIRECTORY / ".fernet_key"

#: Exit code for "this entry point is deprecated", distinct from the old success (0)
#: and partial-failure (2) codes so existing callers cannot mistake it for a result.
DEPRECATION_EXIT_CODE = 3
IMPORT_CHUNK_SIZE = 512 * 1024
TEXT_DESCRIPTION_PLACEHOLDER = "Encrypted text"

LegacyRow = Sequence[object]
ApiRequest = Callable[[dict[str, object]], dict[str, object]]


def valid_api_token(token: str) -> bool:
    return len(token) == 64 and all(character in "0123456789abcdefABCDEF" for character in token)


def read_legacy_rows(database_path: Path) -> list[LegacyRow]:
    connection = sqlite3.connect(str(database_path))
    try:
        return connection.execute(
            "SELECT id, time, type, desc, data FROM history ORDER BY id"
        ).fetchall()
    finally:
        connection.close()


def migrate_rows(
    rows: Iterable[LegacyRow],
    decrypt: Callable[[bytes], bytes],
    api_token: str,
    api_request: ApiRequest,
    report: Callable[[str], None] = print,
) -> tuple[int, int]:
    migrated = 0
    skipped = 0
    rows = list(rows)

    for row in rows:
        old_id, timestamp, entry_type, description, encrypted_data = row
        if not encrypted_data:
            skipped += 1
            continue

        try:
            token = encrypted_data.encode() if isinstance(encrypted_data, str) else bytes(encrypted_data)
            plaintext = decrypt(token)
        except Exception as error:
            report(f"  [skip id={old_id}] decrypt failed: {error}")
            skipped += 1
            continue

        safe_description = (
            TEXT_DESCRIPTION_PLACEHOLDER
            if entry_type == "text"
            else str(description or "")[:100]
        )
        begin = {
            "cmd": "begin_import",
            "token": api_token,
            "time": timestamp,
            "type": entry_type,
            "desc": safe_description,
            "total_size": len(plaintext),
        }
        try:
            response = api_request(begin)
            if not response.get("ok"):
                raise RuntimeError(response.get("error") or "begin_import failed")
            import_id = response["data"]["import_id"]  # type: ignore[index]
            for offset in range(0, len(plaintext), IMPORT_CHUNK_SIZE):
                chunk_response = api_request(
                    {
                        "cmd": "import_chunk",
                        "token": api_token,
                        "import_id": import_id,
                        "import_offset": offset,
                        "chunk_b64": base64.b64encode(
                            plaintext[offset : offset + IMPORT_CHUNK_SIZE]
                        ).decode(),
                    }
                )
                if not chunk_response.get("ok"):
                    raise RuntimeError(chunk_response.get("error") or "import_chunk failed")
            response = api_request(
                {
                    "cmd": "finish_import",
                    "token": api_token,
                    "import_id": import_id,
                }
            )
            if not response.get("ok"):
                raise RuntimeError(response.get("error") or "finish_import failed")
            migrated += 1
            if migrated % 5 == 0:
                report(f"  migrated {migrated}/{len(rows)}...")
        except Exception as error:
            report(f"  [skip id={old_id}] API error: {error}")
            skipped += 1

    return migrated, skipped


def main() -> int:
    """Report that this entry point is deprecated; never connect to anything."""
    print("The manual TailSync v1 recovery tool is no longer supported and will not connect.")
    print("Legacy history is imported automatically on the next launch; the original")
    print("history.db and .fernet_key are retained and never deleted automatically.")
    print("If that import reported failures, read v1-migration-report.json from the")
    print("application data directory instead.")
    return DEPRECATION_EXIT_CODE


if __name__ == "__main__":
    raise SystemExit(main())
