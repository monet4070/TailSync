use rusqlite::Connection;

/// Base schema, valid at every supported version.
///
/// Composite tables are deliberately **not** created here: they belong to the v12
/// migration so that a `MigrationPolicy::LegacyOnly` open leaves a v11 database at
/// v11 with no composite DDL (`docs/adr/ADR-004-composite-clipboard-contracts.md`, K8).
pub(super) fn initialize(connection: &Connection) -> Result<(), rusqlite::Error> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS history (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp   TEXT NOT NULL,
            type        TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '',
            data        BLOB NOT NULL,
            size_bytes  INTEGER NOT NULL DEFAULT 0,
            source_peer TEXT NOT NULL DEFAULT '',
            data_hash   TEXT NOT NULL DEFAULT '',
            category    TEXT NOT NULL DEFAULT 'text',
            categories  TEXT NOT NULL DEFAULT '[]',
            category_confidence INTEGER NOT NULL DEFAULT 0,
            classifier_version INTEGER NOT NULL DEFAULT 0,
            pinned      INTEGER NOT NULL DEFAULT 0,
            batch_id    TEXT,
            batch_index INTEGER,
            batch_total INTEGER,
            batch_status TEXT NOT NULL DEFAULT 'complete',
            created_at  TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE INDEX IF NOT EXISTS idx_history_timestamp
            ON history(timestamp DESC);
        CREATE INDEX IF NOT EXISTS idx_history_type
            ON history(type);
        CREATE INDEX IF NOT EXISTS idx_history_description_nontext
            ON history(description) WHERE type <> 'text';
        CREATE INDEX IF NOT EXISTS idx_history_hash
            ON history(data_hash);
        CREATE TABLE IF NOT EXISTS received_file_batches (
            source_device_id TEXT NOT NULL,
            batch_id TEXT NOT NULL,
            manifest_hash TEXT NOT NULL,
            status TEXT NOT NULL,
            completed_at TEXT,
            UNIQUE(source_device_id, batch_id)
        );
        CREATE TABLE IF NOT EXISTS migration_issues (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            history_id INTEGER NOT NULL,
            migration_version INTEGER NOT NULL,
            issue_type TEXT NOT NULL,
            details TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            resolved_at TEXT,
            UNIQUE(history_id, migration_version, issue_type)
        );
        CREATE INDEX IF NOT EXISTS idx_migration_issues_unresolved
            ON migration_issues(resolved_at, history_id);",
    )?;
    Ok(())
}

/// Composite structures, valid only at schema v12 and above.
///
/// Called from inside the v12 migration transaction so the tables and the version
/// marker commit together. It is idempotent, so it can also be re-asserted for a
/// database that already reports v12.
pub(super) fn initialize_composite(connection: &Connection) -> Result<(), rusqlite::Error> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS history_parts (
            history_id INTEGER NOT NULL REFERENCES history(id) ON DELETE CASCADE,
            part_id INTEGER NOT NULL, kind TEXT NOT NULL, mime TEXT NOT NULL,
            data_hash TEXT NOT NULL, size_bytes INTEGER NOT NULL, stored BLOB NOT NULL,
            PRIMARY KEY(history_id, part_id)
        );
        CREATE INDEX IF NOT EXISTS history_parts_hash ON history_parts(data_hash);
        CREATE TABLE IF NOT EXISTS history_composite_events (
            source_device_id TEXT NOT NULL, event_id TEXT NOT NULL, digest TEXT NOT NULL,
            history_id INTEGER REFERENCES history(id) ON DELETE SET NULL, timestamp_ms INTEGER NOT NULL,
            PRIMARY KEY(source_device_id,event_id)
        );",
    )?;
    Ok(())
}

/// Whether the composite structures exist in this connection.
pub(super) fn composite_schema_present(connection: &Connection) -> Result<bool, rusqlite::Error> {
    connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='history_parts')
            AND EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='history_composite_events')",
        [],
        |row| row.get(0),
    )
}

/// Highest applied schema version, or 0 for a database that has no version row yet.
///
/// A genuinely absent `schema_version` table means "brand new database", which is
/// legitimately version 0 — that is checked explicitly against `sqlite_master`
/// rather than by swallowing the query error. A *real* read error still propagates:
/// flattening it into "version 0" used to skip the pre-upgrade snapshot while the
/// migration ran anyway, losing the only rollback artefact.
pub(super) fn schema_version(connection: &Connection) -> Result<i64, rusqlite::Error> {
    let present: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='schema_version')",
        [],
        |row| row.get(0),
    )?;
    if !present {
        return Ok(0);
    }
    connection.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_version",
        [],
        |row| row.get(0),
    )
}

/// v13 metadata is incremental: v12 encrypted manifests and hashes stay intact.
pub(super) fn initialize_composite_identity(
    connection: &Connection,
) -> Result<(), rusqlite::Error> {
    connection.execute_batch("CREATE TABLE IF NOT EXISTS history_composite_identity (
        history_id INTEGER PRIMARY KEY REFERENCES history(id) ON DELETE CASCADE,
        capture_id TEXT NOT NULL UNIQUE CHECK(length(capture_id)=32),
        version INTEGER NOT NULL CHECK(version=1),
        content_id TEXT NOT NULL,
        dedupe_key TEXT,
        archive_revision INTEGER NOT NULL CHECK(archive_revision BETWEEN 1 AND 9007199254740991),
        archive_digest TEXT NOT NULL CHECK(length(archive_digest)=64)
    ); CREATE INDEX IF NOT EXISTS history_composite_dedupe ON history_composite_identity(dedupe_key) WHERE dedupe_key IS NOT NULL;")
}
pub(super) fn identity_schema_present(connection: &Connection) -> Result<bool, rusqlite::Error> {
    connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='history_composite_identity')", [], |row| row.get(0))
}
