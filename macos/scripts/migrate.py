#!/usr/bin/env python3
"""Manually retry or recover a TailSync v1 history import.

TailSync v2 normally detects and imports this database during startup. The
automatic importer is idempotent, preserves the old database and key, and
writes a migration report. Use this script only for an explicit recovery run
against a running daemon, for example when importing from a custom backup.

Old format:
  - SQLite with Fernet-encrypted `data` column
  - Key stored in .fernet_key (same directory)
  - Schema: id, time, type, desc, data

New format:
  - SQLite with AES-256-GCM encryption (Rust `crypto::encrypt`)
  - Schema: id, timestamp, type, description, data, size_bytes, source_peer, data_hash

The script decrypts Fernet rows and sends bounded chunks to the authenticated
Rust API, which validates and stores each entry in the current format.

The API token is intentionally not discoverable from a running app. For a
manual recovery, start the daemon or app with an explicit 64-character token
and run this script from the same shell, for example:

  TAILSYNC_API_TOKEN=<64 hex chars> /path/to/TailSync.app/Contents/MacOS/TailSync &
  TAILSYNC_API_TOKEN=<same value> python3 macos/scripts/migrate.py
"""

import os
import sqlite3
import sys
import json
from pathlib import Path

OLD_DB = Path.home() / "TailSync_History" / "history.db"
KEY_FILE = Path.home() / "TailSync_History" / ".fernet_key"

def main():
    if not OLD_DB.exists():
        print(f"Old DB not found: {OLD_DB}")
        sys.exit(1)
    if not KEY_FILE.exists():
        print(f"Key file not found: {KEY_FILE}")
        sys.exit(1)
    # Read Fernet key
    fernet_key = KEY_FILE.read_text().strip()
    print(f"Fernet key loaded ({len(fernet_key)} chars)")

    # The destination is owned exclusively by the running Rust daemon.
    old = sqlite3.connect(str(OLD_DB))

    rows = old.execute("SELECT id, time, type, desc, data FROM history ORDER BY id").fetchall()
    print(f"Old DB has {len(rows)} entries")

    # Use the Rust daemon's API to encrypt and insert
    # We send decrypted data to the API which handles encryption + insertion
    port = 19889
    api_token = os.environ.get("TAILSYNC_API_TOKEN", "")
    if len(api_token) != 64 or any(c not in "0123456789abcdefABCDEF" for c in api_token):
        print("TAILSYNC_API_TOKEN must contain the daemon's 64-character hexadecimal API token")
        sys.exit(1)

    def api_request(cmd_payload: bytes) -> dict:
        import socket
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.settimeout(5)
        sock.connect(("127.0.0.1", port))
        sock.sendall(cmd_payload + b"\n")
        buf = b""
        while True:
            chunk = sock.recv(65536)
            if not chunk:
                break
            buf += chunk
            if b"\n" in buf:
                break
        sock.close()
        return json.loads(buf.decode())

    print("Manual recovery import started; automatic startup migration remains preferred.")
    from cryptography.fernet import Fernet
    f = Fernet(fernet_key.encode())

    migrated = 0
    skipped = 0
    for row in rows:
        old_id, time, etype, desc, data = row
        if not data:
            skipped += 1
            continue

        try:
            plain = f.decrypt(data.encode() if isinstance(data, str) else data)
        except Exception as e:
            print(f"  [skip id={old_id}] decrypt failed: {e}")
            skipped += 1
            continue

        # Stream bounded chunks so large files stay below the 1 MiB request cap.
        import base64
        begin_payload = json.dumps({
            "cmd": "begin_import",
            "token": api_token,
            "time": time,
            "type": etype,
            "desc": (desc or "")[:100],
            "total_size": len(plain),
        })
        try:
            resp = api_request(begin_payload.encode())
            if not resp.get("ok"):
                raise RuntimeError(resp.get("error") or "begin_import failed")
            import_id = resp["data"]["import_id"]
            chunk_size = 512 * 1024
            for offset in range(0, len(plain), chunk_size):
                chunk_payload = json.dumps({
                    "cmd": "import_chunk",
                    "token": api_token,
                    "import_id": import_id,
                    "import_offset": offset,
                    "chunk_b64": base64.b64encode(plain[offset:offset + chunk_size]).decode(),
                })
                chunk_response = api_request(chunk_payload.encode())
                if not chunk_response.get("ok"):
                    raise RuntimeError(chunk_response.get("error") or "import_chunk failed")
            resp = api_request(json.dumps({
                "cmd": "finish_import",
                "token": api_token,
                "import_id": import_id,
            }).encode())
            if resp.get("ok"):
                migrated += 1
                if migrated % 5 == 0:
                    print(f"  migrated {migrated}/{len(rows)}...")
            else:
                print(f"  [skip id={old_id}] API: {resp.get('error')}")
                skipped += 1
        except Exception as e:
            print(f"  [skip id={old_id}] API error: {e}")
            skipped += 1

    print(f"\nDone: {migrated} migrated, {skipped} skipped")

if __name__ == "__main__":
    main()
