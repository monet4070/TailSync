#!/usr/bin/env python3
"""Manual TailSync v1 history recovery through the authenticated local API."""

from __future__ import annotations

import argparse
import base64
import json
import os
import socket
import sqlite3
from pathlib import Path
from typing import Callable, Iterable, Sequence

DEFAULT_OLD_DIRECTORY = Path.home() / "TailSync_History"
DEFAULT_OLD_DB = DEFAULT_OLD_DIRECTORY / "history.db"
DEFAULT_KEY_FILE = DEFAULT_OLD_DIRECTORY / ".fernet_key"
DEFAULT_API_PORT = 19889
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


def socket_api_request(payload: dict[str, object], port: int) -> dict[str, object]:
    encoded = json.dumps(payload, separators=(",", ":")).encode()
    with socket.create_connection(("127.0.0.1", port), timeout=5) as connection:
        connection.settimeout(5)
        connection.sendall(encoded + b"\n")
        response = bytearray()
        while b"\n" not in response:
            chunk = connection.recv(65536)
            if not chunk:
                break
            response.extend(chunk)
    if not response:
        raise RuntimeError("TailSync local API closed without a response")
    return json.loads(bytes(response).split(b"\n", 1)[0].decode())


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


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--database", type=Path, default=DEFAULT_OLD_DB)
    parser.add_argument("--key", type=Path, default=DEFAULT_KEY_FILE)
    parser.add_argument("--port", type=int, default=DEFAULT_API_PORT)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if not args.database.is_file():
        print(f"Old DB not found: {args.database}")
        return 1
    if not args.key.is_file():
        print(f"Key file not found: {args.key}")
        return 1

    api_token = os.environ.get("TAILSYNC_API_TOKEN", "")
    if not valid_api_token(api_token):
        print("TAILSYNC_API_TOKEN must contain the daemon's 64-character hexadecimal API token")
        return 1

    from cryptography.fernet import Fernet

    fernet_key = args.key.read_text(encoding="utf-8").strip()
    rows = read_legacy_rows(args.database)
    print(f"Old DB has {len(rows)} entries")
    print("Manual recovery import started; automatic startup migration remains preferred.")
    migrated, skipped = migrate_rows(
        rows,
        Fernet(fernet_key.encode()).decrypt,
        api_token,
        lambda payload: socket_api_request(payload, args.port),
    )
    print(f"\nDone: {migrated} migrated, {skipped} skipped")
    return 0 if skipped == 0 else 2


if __name__ == "__main__":
    raise SystemExit(main())
