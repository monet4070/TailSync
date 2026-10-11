//! Independent effect journal (repair plan §4 R4-01).
//!
//! The receipt store used to be a single JSON file per extension, and it was the
//! *sole* authority. One corrupt byte therefore disabled composite receiving for
//! every peer, with no recovery path and no visible fault — and there was nothing
//! to recover *from*, because deleting the file would have destroyed the only
//! evidence of whether a native write had already happened.
//!
//! This module is the authoritative store: a private SQLite metadata database with
//! its own schema, independent of the history database's migration and of user
//! history deletion. It holds identities, digests, states and a recoverable history
//! reference — **never clipboard bodies or absolute paths**.
//!
//! The JSON files are kept as a compatibility mirror for the rollback window. They
//! are *derived*: the ledger answers lookups, and a damaged mirror is rebuilt from
//! it. A damaged **journal** is never rebuilt — it is preserved, automatic native
//! writes stop, and the fault is reported, because only the ledger can prove whether
//! an effect already happened.
//!
//! What is deliberately not claimed: history and this journal are separate
//! databases, so they are not cross-DB atomic. A prepared-but-unclaimed event may
//! safely continue; a claimed event with no result is `Unknown` and is never
//! re-applied.

use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

/// Journal schema version. Independent of the history schema on purpose.
pub const JOURNAL_SCHEMA_VERSION: i64 = 1;
/// Database file name inside the private root.
pub const JOURNAL_FILE_NAME: &str = "effect-journal-v1.db";
/// Composite compatibility mirror (written during the rollback window).
pub const COMPOSITE_MIRROR_FILE_NAME: &str = "remote-composite-receipts-v1.json";
/// Legacy compatibility mirror.
pub const LEGACY_MIRROR_FILE_NAME: &str = "remote-event-receipts-v1.json";

static JOURNAL_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

/// One recorded effect attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectRecord {
    pub source: String,
    pub event_id: String,
    pub digest: String,
    pub timestamp_ms: i64,
    /// Terminal result byte, once reported.
    pub result: Option<u8>,
    pub completed: bool,
    /// Recoverable history reference. An id, not a path.
    pub history_ref: Option<i64>,
}

impl EffectRecord {
    /// Whether this attempt may still be replayed. Only an unclaimed record may.
    pub fn may_apply(&self) -> bool {
        false // an existing record is a claim, never permission to repeat native I/O
    }
}

/// What a claim found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimOutcome {
    /// The event was not known; the caller now owns the attempt.
    Claimed,
    /// The event is already known, with the same digest.
    Existing(EffectRecord),
}

/// Journal health, reported distinctly so a caller can degrade honestly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JournalHealth {
    Ok,
    /// The database exists but cannot be read or fails its integrity check. The file
    /// is preserved; automatic native writes stop until an operator resolves it.
    Corrupt {
        detail: String,
    },
}

/// The journal, opened lazily against the private root.
pub struct EffectJournal {
    conn: Connection,
    root: PathBuf,
}

fn journal_path(root: &Path) -> PathBuf {
    root.join(JOURNAL_FILE_NAME)
}

/// Settle interrupted prepared receives before listeners or automatic capture
/// start. No native I/O is performed: a claim without a result remains Unknown.
pub fn recover_prepared_history(
    root: &Path,
    db: &mut crate::db::HistoryDB,
) -> Result<usize, String> {
    let _guard = super::composite_receipts::LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let journal = super::composite_receipts::initialize_at(root)?;
    let legacy_keys: std::collections::HashSet<_> = journal
        .records_for_kind(Some("legacy"))?
        .into_iter()
        .map(|r| (r.source, r.event_id))
        .collect();
    let mut recovered = 0;
    for record in journal.records()? {
        if record.completed {
            continue;
        }
        let Some(id) = record.history_ref else {
            continue;
        };
        // Explicit deletion is allowed to break the reference; never resurrect it.
        let legacy = legacy_keys.contains(&(record.source.clone(), record.event_id.clone()));
        if legacy && id > 0 && db.get_type(id).is_ok() {
            journal.mark_legacy(&record.source, &record.event_id, true)?;
        }
        if record.result.is_none() {
            journal.report(
                &record.source,
                &record.event_id,
                crate::protocol::composite::AckResult::ApplyUnknown as u8,
            )?;
        }
        journal.complete(&record.source, &record.event_id)?;
        db.release_history(id);
        recovered += 1;
    }
    super::composite_receipts::write_mirror(&root.join(COMPOSITE_MIRROR_FILE_NAME), &journal)?;
    super::event_receipts::write_mirror(&root.join(LEGACY_MIRROR_FILE_NAME), &journal)?;
    Ok(recovered)
}

/// Persist content-free capture failure metadata before acknowledging a revision.
pub fn record_capture_fault(root: &Path, revision: i64, kind: &str) -> Result<(), String> {
    if !matches!(
        kind,
        "capture_not_saved" | "clipboard_unavailable" | "capture_failed" | "capture_incomplete"
    ) {
        return Err("Invalid capture fault kind".into());
    }
    let journal = EffectJournal::open(root)?;
    let _guard = JOURNAL_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    journal
        .conn
        .execute(
            "INSERT INTO capture_faults(revision,kind,occurred_at_ms) VALUES (?1,?2,?3)",
            params![revision, kind, crate::protocol::unix_timestamp_ms()],
        )
        .map_err(|e| e.to_string())?;
    journal.conn.execute("DELETE FROM capture_faults WHERE id NOT IN (SELECT id FROM capture_faults ORDER BY id DESC LIMIT 512)", []).map_err(|e| e.to_string())?;
    Ok(())
}

fn open_at(root: &Path) -> Result<Connection, String> {
    crate::private_fs::create_private_dir_all(root).map_err(|e| e.to_string())?;
    let path = journal_path(root);
    let conn = Connection::open(&path).map_err(|e| e.to_string())?;
    // Durable by construction: WAL plus a full synchronous commit, and the commit
    // result is checked rather than assumed.
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = FULL;
         PRAGMA foreign_keys = ON;",
    )
    .map_err(|e| e.to_string())?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS effects (
            source       TEXT NOT NULL,
            event_id     TEXT NOT NULL,
            digest       TEXT NOT NULL,
            timestamp_ms INTEGER NOT NULL,
            result       INTEGER,
            completed    INTEGER NOT NULL DEFAULT 0,
            history_ref  INTEGER,
            kind         TEXT NOT NULL DEFAULT 'composite',
            legacy_received INTEGER NOT NULL DEFAULT 0,
            token        BLOB,
            epoch        INTEGER,
            PRIMARY KEY (source, event_id)
        );
        CREATE INDEX IF NOT EXISTS effects_replay_floor ON effects(timestamp_ms);
        CREATE TABLE IF NOT EXISTS journal_state (
            key   TEXT PRIMARY KEY,
            value INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS capture_faults (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            revision INTEGER NOT NULL,
            kind TEXT NOT NULL,
            occurred_at_ms INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS journal_meta (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );",
    )
    .map_err(|e| e.to_string())?;
    // Additive metadata migration inside journal schema 1; old readers ignore it.
    let columns: Vec<String> = conn
        .prepare("PRAGMA table_info(effects)")
        .map_err(|e| e.to_string())?
        .query_map([], |row| row.get(1))
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    if !columns.iter().any(|name| name == "kind") {
        conn.execute_batch(
            "ALTER TABLE effects ADD COLUMN kind TEXT NOT NULL DEFAULT 'composite';",
        )
        .map_err(|e| e.to_string())?;
    }
    if !columns.iter().any(|name| name == "legacy_received") {
        conn.execute_batch(
            "ALTER TABLE effects ADD COLUMN legacy_received INTEGER NOT NULL DEFAULT 0;",
        )
        .map_err(|e| e.to_string())?;
    }
    let version: Option<i64> = conn
        .query_row(
            "SELECT value FROM journal_state WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    match version {
        None => {
            conn.execute(
                "INSERT INTO journal_state(key, value) VALUES ('schema_version', ?1)",
                params![JOURNAL_SCHEMA_VERSION],
            )
            .map_err(|e| e.to_string())?;
        }
        Some(found) if found == JOURNAL_SCHEMA_VERSION => {}
        Some(found) => {
            return Err(format!(
                "effect journal schema {found} is not supported (expected {JOURNAL_SCHEMA_VERSION})"
            ))
        }
    }
    crate::private_fs::enforce_private_database(&path).map_err(|e| e.to_string())?;
    Ok(conn)
}

impl EffectJournal {
    /// Open the journal in the private root, if it is healthy.
    pub fn open(root: &Path) -> Result<Self, String> {
        let conn = open_at(root)?;
        Ok(Self {
            conn,
            root: root.to_path_buf(),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Integrity check. A failure is reported, never repaired in place.
    pub fn health(&self) -> JournalHealth {
        match self
            .conn
            .query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
        {
            Ok(status) if status == "ok" => JournalHealth::Ok,
            Ok(status) => JournalHealth::Corrupt { detail: status },
            Err(error) => JournalHealth::Corrupt {
                detail: error.to_string(),
            },
        }
    }

    fn record(
        conn: &Connection,
        source: &str,
        event_id: &str,
    ) -> Result<Option<EffectRecord>, String> {
        conn.query_row(
            "SELECT source, event_id, digest, timestamp_ms, result, completed, history_ref
             FROM effects WHERE source = ?1 AND event_id = ?2",
            params![source, event_id],
            |row| {
                Ok(EffectRecord {
                    source: row.get(0)?,
                    event_id: row.get(1)?,
                    digest: row.get(2)?,
                    timestamp_ms: row.get(3)?,
                    result: row.get(4)?,
                    completed: row.get::<_, i64>(5)? != 0,
                    history_ref: row.get(6)?,
                })
            },
        )
        .optional()
        .map_err(|e| e.to_string())
    }

    /// Claim an effect attempt.
    ///
    /// The key is the authenticated source plus the message id. The same key with a
    /// *different* digest is a collision and is rejected rather than silently
    /// overwriting the original attempt.
    pub fn claim(
        &self,
        source: &str,
        event_id: &str,
        digest: &str,
        timestamp_ms: i64,
        token: Option<&[u8]>,
        epoch: Option<i64>,
    ) -> Result<ClaimOutcome, String> {
        self.claim_prepared(source, event_id, digest, timestamp_ms, token, epoch, None)
    }

    /// Atomically bind the claim to history that was committed before native I/O.
    #[allow(clippy::too_many_arguments)]
    pub fn claim_prepared(
        &self,
        source: &str,
        event_id: &str,
        digest: &str,
        timestamp_ms: i64,
        token: Option<&[u8]>,
        epoch: Option<i64>,
        history_ref: Option<i64>,
    ) -> Result<ClaimOutcome, String> {
        let _guard = JOURNAL_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(existing) = Self::record(&self.conn, source, event_id)? {
            if existing.digest != digest {
                return Err("Effect identity collision".into());
            }
            return Ok(ClaimOutcome::Existing(existing));
        }
        self.conn
            .execute(
                "INSERT INTO effects(source, event_id, digest, timestamp_ms, token, epoch, history_ref)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![source, event_id, digest, timestamp_ms, token, epoch, history_ref],
            )
            .map_err(|e| e.to_string())?;
        Ok(ClaimOutcome::Claimed)
    }

    pub fn mark_legacy(&self, source: &str, event_id: &str, received: bool) -> Result<(), String> {
        let _guard = JOURNAL_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        self.conn.execute("UPDATE effects SET kind = 'legacy', legacy_received = MAX(legacy_received, ?3) WHERE source = ?1 AND event_id = ?2", params![source, event_id, received]).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn legacy_received(&self, source: &str, event_id: &str) -> Result<bool, String> {
        let _guard = JOURNAL_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        self.conn
            .query_row(
                "SELECT legacy_received FROM effects WHERE source = ?1 AND event_id = ?2",
                params![source, event_id],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())
    }

    /// Record a terminal result. A conflicting second result is rejected.
    pub fn report(&self, source: &str, event_id: &str, result: u8) -> Result<(), String> {
        let _guard = JOURNAL_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let existing = Self::record(&self.conn, source, event_id)?
            .ok_or_else(|| "Missing effect claim".to_string())?;
        if existing.result.is_some_and(|old| old != result) {
            return Err("Effect result collision".into());
        }
        self.conn
            .execute(
                "UPDATE effects SET result = ?3 WHERE source = ?1 AND event_id = ?2",
                params![source, event_id, result],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Mark the effect complete. Completion requires a terminal result, so an
    /// `Unknown` attempt can never be silently promoted to a success.
    pub fn complete(&self, source: &str, event_id: &str) -> Result<(), String> {
        let _guard = JOURNAL_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let existing = Self::record(&self.conn, source, event_id)?
            .ok_or_else(|| "Missing effect claim".to_string())?;
        if existing.result.is_none() {
            return Err("Cannot complete an effect with no result".into());
        }
        self.conn
            .execute(
                "UPDATE effects SET completed = 1 WHERE source = ?1 AND event_id = ?2",
                params![source, event_id],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Drop a claim that provably had no effect, so a retry may proceed.
    pub fn remove(&self, source: &str, event_id: &str) -> Result<(), String> {
        let _guard = JOURNAL_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.conn
            .execute(
                "DELETE FROM effects WHERE source = ?1 AND event_id = ?2",
                params![source, event_id],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn lookup(&self, source: &str, event_id: &str) -> Result<Option<EffectRecord>, String> {
        let _guard = JOURNAL_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Self::record(&self.conn, source, event_id)
    }

    /// Every record, oldest first. Used to rebuild the compatibility mirror.
    pub fn count_for_kind(&self, kind: &str) -> Result<usize, String> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM effects WHERE kind = ?1",
                params![kind],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| count as usize)
            .map_err(|e| e.to_string())
    }

    pub fn records(&self) -> Result<Vec<EffectRecord>, String> {
        self.records_for_kind(None)
    }

    pub fn records_for_kind(&self, kind: Option<&str>) -> Result<Vec<EffectRecord>, String> {
        let _guard = JOURNAL_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut statement = self
            .conn
            .prepare(
                "SELECT source, event_id, digest, timestamp_ms, result, completed, history_ref
                 FROM effects WHERE (?1 IS NULL OR kind = ?1) ORDER BY timestamp_ms ASC, event_id ASC",
            )
            .map_err(|e| e.to_string())?;
        let rows = statement
            .query_map(params![kind], |row| {
                Ok(EffectRecord {
                    source: row.get(0)?,
                    event_id: row.get(1)?,
                    digest: row.get(2)?,
                    timestamp_ms: row.get(3)?,
                    result: row.get(4)?,
                    completed: row.get::<_, i64>(5)? != 0,
                    history_ref: row.get(6)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    /// The oldest original timestamp that must still be retained.
    ///
    /// Tombstones and terminals are kept until an event bearing that timestamp can
    /// no longer pass receive validation, so a replay after a clock step cannot be
    /// re-admitted.
    pub fn replay_floor_ms(&self) -> Result<i64, String> {
        let _guard = JOURNAL_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.conn
            .query_row(
                "SELECT value FROM journal_state WHERE key = 'replay_floor_ms'",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())
            .map(|value| value.unwrap_or(0))
    }

    /// Advance the replay floor. It never moves backwards.
    pub fn advance_replay_floor(&self, ms: i64) -> Result<i64, String> {
        let _guard = JOURNAL_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let current: i64 = self
            .conn
            .query_row(
                "SELECT value FROM journal_state WHERE key = 'replay_floor_ms'",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?
            .unwrap_or(0);
        let next = current.max(ms);
        self.conn
            .execute(
                "INSERT INTO journal_state(key, value) VALUES ('replay_floor_ms', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![next],
            )
            .map_err(|e| e.to_string())?;
        Ok(next)
    }

    /// Remove records whose original timestamp is below the floor.
    ///
    /// Only called with a floor that a receiver would already reject, so the
    /// anti-replay guarantee is preserved by construction.
    pub fn prune_below(&self, floor_ms: i64) -> Result<usize, String> {
        let _guard = JOURNAL_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.conn
            .execute(
                "DELETE FROM effects WHERE timestamp_ms < ?1",
                params![floor_ms],
            )
            .map_err(|e| e.to_string())
    }

    /// Whether the one-time import of the legacy JSON mirrors already ran.
    pub fn legacy_import_done(&self) -> Result<bool, String> {
        let _guard = JOURNAL_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.conn
            .query_row(
                "SELECT value FROM journal_meta WHERE key = 'legacy_import_v2'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|e| e.to_string())
            .map(|value| value.as_deref() == Some("done"))
    }

    pub fn mark_legacy_import_done(&self) -> Result<(), String> {
        let _guard = JOURNAL_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.conn
            .execute(
                "INSERT INTO journal_meta(key, value) VALUES ('legacy_import_v2', 'done')
                 ON CONFLICT(key) DO UPDATE SET value = 'done'",
                [],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Import one legacy record, preserving its original digest.
    ///
    /// A legacy claim with no result becomes an *unresolved* record, i.e. `Unknown`:
    /// it is never re-applied. `received` is deliberately **not** stored as a result
    /// or as completion — the old flag only proves that the old history write
    /// completed, which says nothing about whether a native effect happened. The
    /// record therefore stays unresolved, which is the conservative reading.
    pub fn import_legacy(
        &self,
        source: &str,
        event_id: &str,
        digest: &str,
        timestamp_ms: i64,
        received: bool,
    ) -> Result<(), String> {
        let _guard = JOURNAL_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _legacy_received = received;
        if let Some(existing) = Self::record(&self.conn, source, event_id)? {
            if existing.digest != digest {
                return Err("Effect identity collision".into());
            }
            return Ok(());
        }
        self.conn
            .execute(
                "INSERT INTO effects(source, event_id, digest, timestamp_ms, result, completed)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    source,
                    event_id,
                    digest,
                    timestamp_ms,
                    // Not a native result, and not complete: see the doc comment.
                    Option::<u8>::None,
                    0_i64
                ],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("effect-journal-{:016x}", rand::random::<u64>()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn claim_is_idempotent_and_rejects_a_different_digest() {
        let root = root();
        let journal = EffectJournal::open(&root).unwrap();
        assert_eq!(
            journal
                .claim("peer", "evt", "digest-a", 10, None, None)
                .unwrap(),
            ClaimOutcome::Claimed
        );
        // Same key, same digest: the caller sees the existing record.
        match journal
            .claim("peer", "evt", "digest-a", 10, None, None)
            .unwrap()
        {
            ClaimOutcome::Existing(record) => assert!(!record.may_apply()),
            other => panic!("expected the existing record, got {other:?}"),
        }
        // Same key, different digest: a collision, not an overwrite.
        assert!(journal
            .claim("peer", "evt", "digest-b", 10, None, None)
            .is_err());
        // A different source is a different key.
        assert_eq!(
            journal
                .claim("other", "evt", "digest-b", 10, None, None)
                .unwrap(),
            ClaimOutcome::Claimed
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_terminal_result_is_immutable_and_completion_requires_one() {
        let root = root();
        let journal = EffectJournal::open(&root).unwrap();
        journal.claim("peer", "evt", "d", 10, None, None).unwrap();
        assert!(
            journal.complete("peer", "evt").is_err(),
            "an attempt with no result must not be completable"
        );
        journal.report("peer", "evt", 1).unwrap();
        assert!(
            journal.report("peer", "evt", 2).is_err(),
            "a conflicting result is rejected"
        );
        journal.report("peer", "evt", 1).unwrap();
        journal.complete("peer", "evt").unwrap();
        let record = journal.lookup("peer", "evt").unwrap().unwrap();
        assert!(record.completed);
        assert!(!record.may_apply(), "a completed effect is never replayed");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn the_replay_floor_never_moves_backwards() {
        let root = root();
        let journal = EffectJournal::open(&root).unwrap();
        assert_eq!(journal.replay_floor_ms().unwrap(), 0);
        assert_eq!(journal.advance_replay_floor(100).unwrap(), 100);
        assert_eq!(
            journal.advance_replay_floor(50).unwrap(),
            100,
            "a clock step must not lower the floor"
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn pruning_only_removes_records_below_the_floor() {
        let root = root();
        let journal = EffectJournal::open(&root).unwrap();
        journal.claim("peer", "old", "d", 10, None, None).unwrap();
        journal.claim("peer", "new", "d", 20, None, None).unwrap();
        assert_eq!(journal.prune_below(20).unwrap(), 1);
        assert!(journal.lookup("peer", "old").unwrap().is_none());
        assert!(journal.lookup("peer", "new").unwrap().is_some());
        std::fs::remove_dir_all(root).ok();
    }

    /// A legacy claim with no result must import as unresolved, never as a success.
    #[test]
    fn legacy_import_preserves_status_without_claiming_an_effect() {
        let root = root();
        let journal = EffectJournal::open(&root).unwrap();
        journal.import_legacy("peer", "evt", "d", 10, true).unwrap();
        let record = journal.lookup("peer", "evt").unwrap().unwrap();
        assert!(
            record.result.is_none(),
            "received=true is not a native result"
        );
        assert!(!record.completed);
        assert!(!record.may_apply());
        // Importing the same key with a different digest is still a collision.
        assert!(journal
            .import_legacy("peer", "evt", "other", 10, true)
            .is_err());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_healthy_journal_reports_ok() {
        let root = root();
        let journal = EffectJournal::open(&root).unwrap();
        assert_eq!(journal.health(), JournalHealth::Ok);
        std::fs::remove_dir_all(root).ok();
    }

    /// A journal whose file is not a database must be reported as corrupt, and its
    /// bytes must be preserved rather than replaced.
    #[test]
    fn a_damaged_journal_is_reported_and_preserved() {
        let root = root();
        {
            let _ = EffectJournal::open(&root).unwrap();
        }
        let path = journal_path(&root);
        let damaged = b"this is not a sqlite database".to_vec();
        std::fs::write(&path, &damaged).unwrap();
        // Opening may fail outright; either way the bytes stay on disk.
        let _ = EffectJournal::open(&root);
        assert_eq!(
            std::fs::read(&path).unwrap(),
            damaged,
            "a damaged journal must be preserved for diagnosis, never rebuilt empty"
        );
        std::fs::remove_dir_all(root).ok();
    }
}
