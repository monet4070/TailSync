use rusqlite::Connection;

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
        CREATE INDEX IF NOT EXISTS idx_history_description
            ON history(description);
        CREATE INDEX IF NOT EXISTS idx_history_hash
            ON history(data_hash);

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
