#!/usr/bin/env python3
"""Migrate old TailSync v1 history.db → TailSync v2 history-v2.db.

Old format:
  - SQLite with Fernet-encrypted `data` column
  - Key stored in .fernet_key (same directory)
  - Schema: id, time, type, desc, data

New format:
  - SQLite with AES-256-GCM encryption (Rust `crypto::encrypt`)
  - Schema: id, timestamp, type, description, data, size_bytes, source_peer, data_hash

We decrypt with Fernet, re-encrypt with the Rust backend's encrypt(), and insert.
"""

import os
import sqlite3
import sys
import subprocess
import json
from pathlib import Path

OLD_DB = Path.home() / "TailSync_History" / "history.db"
KEY_FILE = Path.home() / "TailSync_History" / ".fernet_key"
NEW_DB = Path.home() / "Library" / "Application Support" / "com.tailsync.TailSync" / "history-v2.db"

def main():
    if not OLD_DB.exists():
        print(f"Old DB not found: {OLD_DB}")
        sys.exit(1)
    if not KEY_FILE.exists():
        print(f"Key file not found: {KEY_FILE}")
        sys.exit(1)
    if not NEW_DB.exists():
        print(f"New DB not found: {NEW_DB}")
        sys.exit(1)

    # Read Fernet key
    fernet_key = KEY_FILE.read_text().strip()
    print(f"Fernet key loaded ({len(fernet_key)} chars)")

    # Connect to both DBs
    old = sqlite3.connect(str(OLD_DB))
    new = sqlite3.connect(str(NEW_DB))

    # Count existing entries in new DB to avoid duplicates
    existing = new.execute("SELECT COUNT(*) FROM history").fetchone()[0]
    print(f"New DB has {existing} entries")

    rows = old.execute("SELECT id, time, type, desc, data FROM history ORDER BY id").fetchall()
    print(f"Old DB has {len(rows)} entries")

    # Use the Rust daemon's API to encrypt and insert
    # We send decrypted data to the API which handles encryption + insertion
    port = 19889

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

    # But the API doesn't have an "add_entry" endpoint — it only reads.
    # So we need to insert directly via the Rust crypto functions.
    # Alternative: write a temporary Rust migration binary.
    #
    # Actually, simpler approach: since these are mostly file entries
    # and the data is already in the old DB, we can do the migration
    # directly in Rust by adding a migration command to the API.

    print("\nMigration needs a Rust-side endpoint. Options:")
    print("1. Add 'migrate_entry' API endpoint that takes decrypted data + metadata")
    print("2. Write standalone Rust migration binary")
    print("3. Use Python cryptography to re-encrypt with AES-256-GCM (need key)")

    # Let's use Fernet to decrypt, then call the Rust API with the data.
    # We need a new API endpoint: migrate_insert
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

        # Call Rust API: migrate_entry { time, type, desc, data_b64 }
        import base64
        payload = json.dumps({
            "cmd": "migrate_entry",
            "time": time,
            "type": etype,
            "desc": (desc or "")[:100],
            "data_b64": base64.b64encode(plain).decode(),
        })
        try:
            resp = api_request(payload.encode())
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
