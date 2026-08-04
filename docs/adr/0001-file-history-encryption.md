# ADR 0001: Encrypted file-history containers

Status: Accepted

## Context

TailSync encrypts text and image history with the data encryption key (DEK), but
historical file bytes were stored as plaintext in `file-history/`. Files may be
as large as 5 GiB, so the existing whole-buffer AES-GCM helper cannot be used
without excessive memory use. The storage format also needs deterministic
validation, crash-safe replacement, and a migration path for existing files.

## Decision

File history uses the `TSFENC1` container format. A database file reference with
version 2 identifies the encrypted format; version 1 remains the legacy
plaintext reference during migration.

Each container has:

- an authenticated fixed header containing the format magic, 1 MiB chunk size,
  plaintext length, a random 128-bit salt, and the expected BLAKE3 hash;
- an authentication tag for the header, including for empty files;
- independently authenticated chunks, each containing at most 1 MiB of
  plaintext plus a 16-byte AES-GCM tag.

HKDF-SHA256 derives a separate 256-bit AES-GCM key from the process DEK and the
per-file salt. Chunk nonces are the chunk index encoded into a 96-bit nonce.
The header, chunk index, and plaintext chunk length are authenticated as AAD.
The reserved all-ones chunk index authenticates the header.

Encryption streams from source to destination and verifies the plaintext size
and BLAKE3 hash before installation. The completed temporary file is flushed,
synced, and atomically moved over the destination. A failed write or failed
authentication never replaces the last readable destination.

At startup, database migration v7 enables the encrypted reference format
without scanning file contents on the startup thread. A tracked background
worker then uses a separate SQLite connection to scan version 1 references in
bounded batches. Each plaintext file is encrypted in place and only then is its
database reference updated to version 2. The worker checks the shutdown signal
between batches. If a process stops between the file replacement and database
update, the next run recognizes the encrypted container and finishes the
reference update. Migration errors are recorded in `migration_issues`; source
bytes are not deleted.

Clipboard restoration decrypts into the controlled `clipboard-files/`
directory. Encrypted files are never handed directly to the operating-system
clipboard. Legacy plaintext files remain readable if migration is temporarily
blocked, but migration is retried on every startup.

## Consequences

- File-history bytes are protected at rest by the same OS-protected DEK as
  other history data.
- Random access requires authenticating chunks up to the requested offset in
  the current implementation; clipboard restoration streams the whole file.
- Container storage adds 68 bytes of header, one 16-byte header tag, and 16
  bytes per 1 MiB chunk.
- Losing the DEK makes encrypted history unrecoverable, so key-store errors must
  never generate or overwrite a replacement key.
