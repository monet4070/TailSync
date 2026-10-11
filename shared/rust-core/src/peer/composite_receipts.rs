//! Separate versioned write-ahead effect journal. It contains identities and
//! results only; no clipboard bodies, paths, or native revisions are persisted.
use super::effect_journal::{
    ClaimOutcome, EffectJournal, EffectRecord, COMPOSITE_MIRROR_FILE_NAME, LEGACY_MIRROR_FILE_NAME,
};
use crate::protocol::{unix_timestamp_ms, EventEnvelope, EVENT_TIMESTAMP_WINDOW_MS};
use serde::{Deserialize, Serialize};
use std::sync::{LazyLock, Mutex};
pub(super) static LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Receipt {
    source: String,
    event_id: String,
    digest: String,
    timestamp_ms: i64,
    pub result: Option<u8>,
    pub completed: bool,
    #[serde(skip)]
    pub history_ref: Option<i64>,
}
impl Receipt {
    pub fn new(source: String, e: &EventEnvelope) -> Self {
        let mut h = blake3::Hasher::new();
        h.update(&e.timestamp_ms.to_be_bytes());
        h.update(&e.content);
        Self {
            source,
            event_id: e.message_id.as_hex(),
            digest: h.finalize().to_hex().to_string(),
            timestamp_ms: e.timestamp_ms,
            result: None,
            completed: false,
            history_ref: None,
        }
    }
}
/// The legacy event mirror's shape, declared here so the import is strict against
/// the file it actually reads.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyStore {
    version: u8,
    records: Vec<LegacyReceipt>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyReceipt {
    source: String,
    event_id: String,
    digest: String,
    timestamp_ms: i64,
    received: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Store {
    version: u8,
    records: Vec<Receipt>,
}
#[derive(Clone, Copy)]
pub(super) enum Operation {
    Lookup,
    #[allow(dead_code)] // compatibility/import fixtures also claim without history
    Claim,
    ClaimPrepared(i64),
    Report(u8),
    Complete,
    Remove,
}
pub(super) async fn update(key: Receipt, operation: Operation) -> Result<Option<Receipt>, String> {
    tokio::task::spawn_blocking(move || {
        update_at(
            &crate::db::get_data_dir().join("remote-composite-receipts-v1.json"),
            key,
            operation,
        )
    })
    .await
    .map_err(|_| "Composite receipt worker unavailable".to_string())?
}
/// The journal is authoritative; the JSON file is a derived compatibility mirror.
///
/// The mirror exists only for the rollback window (an older binary still reads it),
/// so it is rebuilt from the ledger whenever it is missing or unreadable. A damaged
/// **journal** is a different matter: it is reported, preserved, and blocks
/// automatic native writes, because only the ledger can prove whether an effect
/// already happened.
fn update_at(
    path: &std::path::Path,
    key: Receipt,
    operation: Operation,
) -> Result<Option<Receipt>, String> {
    let _guard = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = path
        .parent()
        .ok_or_else(|| "Composite receipt root missing".to_string())?;
    let journal = initialize_at(root)?;
    if key.timestamp_ms < journal.replay_floor_ms()? {
        return Err("event timestamp is outside the accepted window (journal replay floor)".into());
    }
    if matches!(operation, Operation::Claim | Operation::ClaimPrepared(_))
        && journal.lookup(&key.source, &key.event_id)?.is_none()
        && journal.count_for_kind("composite")? >= 4096
    {
        return Err(format!(
            "{}composite receipt capacity reached",
            crate::protocol::TEMPORARY_PRESSURE_MARKER
        ));
    }
    // All operations, including the completed-event fast path, bind identity to
    // immutable content. Checking only Claim lets Lookup acknowledge a collision.
    if journal
        .lookup(&key.source, &key.event_id)?
        .is_some_and(|record| record.digest != key.digest)
    {
        return Err("Composite event identity collision".into());
    }
    let record = match operation {
        Operation::Lookup => journal.lookup(&key.source, &key.event_id)?,
        Operation::Claim | Operation::ClaimPrepared(_) => {
            match journal.claim_prepared(
                &key.source,
                &key.event_id,
                &key.digest,
                key.timestamp_ms,
                None,
                None,
                match operation {
                    Operation::ClaimPrepared(id) => Some(id),
                    _ => None,
                },
            )? {
                ClaimOutcome::Claimed => {
                    write_mirror(path, &journal)?;
                    return Ok(None);
                }
                ClaimOutcome::Existing(record) => Some(record),
            }
        }
        Operation::Report(result) => {
            journal.report(&key.source, &key.event_id, result)?;
            journal.lookup(&key.source, &key.event_id)?
        }
        Operation::Complete => {
            journal.complete(&key.source, &key.event_id)?;
            journal.lookup(&key.source, &key.event_id)?
        }
        Operation::Remove => {
            journal.remove(&key.source, &key.event_id)?;
            write_mirror(path, &journal)?;
            return Ok(None);
        }
    };
    // Recovery path: a damaged or missing mirror is rebuilt from the ledger rather
    // than failing the lookup.
    write_mirror(path, &journal)?;
    Ok(record.map(receipt_from_record))
}

fn receipt_from_record(record: EffectRecord) -> Receipt {
    Receipt {
        source: record.source,
        event_id: record.event_id,
        digest: record.digest,
        timestamp_ms: record.timestamp_ms,
        result: record.result,
        completed: record.completed,
        history_ref: record.history_ref,
    }
}

pub(super) fn initialize_at(root: &std::path::Path) -> Result<EffectJournal, String> {
    let journal = EffectJournal::open(root)?;
    if journal.health() != super::effect_journal::JournalHealth::Ok {
        return Err(
            "Effect journal integrity check failed; automatic publication is disabled".into(),
        );
    }
    if !journal.legacy_import_done()? {
        import_legacy_mirrors(root, &journal)?;
        journal.mark_legacy_import_done()?;
    }
    let cutoff = crate::protocol::unix_timestamp_ms().saturating_sub(EVENT_TIMESTAMP_WINDOW_MS);
    let floor = journal.advance_replay_floor(cutoff)?;
    journal.prune_below(floor)?;
    Ok(journal)
}

/// Rebuild the compatibility mirror from the authoritative ledger.
pub(super) fn write_mirror(path: &std::path::Path, journal: &EffectJournal) -> Result<(), String> {
    let now = unix_timestamp_ms();
    let records: Vec<Receipt> = journal
        .records_for_kind(Some("composite"))?
        .into_iter()
        .map(receipt_from_record)
        // Keep the mirror bounded exactly as before: an event whose original
        // timestamp can no longer pass receive validation is dropped.
        .filter(|record| {
            record
                .timestamp_ms
                .saturating_add(EVENT_TIMESTAMP_WINDOW_MS)
                >= now
        })
        .collect();
    if records.len() > 4096 {
        return Err("Composite receipt capacity reached".into());
    }
    let store = Store {
        version: 1,
        records,
    };
    let bytes = serde_json::to_vec(&store).map_err(|_| "Composite receipt encoding failed")?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err("Composite receipt journal too large".into());
    }
    crate::private_fs::create_private_dir_all(
        path.parent().ok_or("Composite receipt root missing")?,
    )
    .map_err(|_| "Composite receipt root unavailable")?;
    crate::private_fs::write_private_file(path, &bytes)
        .map_err(|_| "Composite receipt persistence failed")?;
    Ok(())
}

/// One-time strict import of both legacy JSON mirrors.
///
/// Strict on purpose: a file that does not parse exactly as the old schema is an
/// error rather than a partial import, because a half-imported ledger would be
/// indistinguishable from a fresh one.
fn import_legacy_mirrors(root: &std::path::Path, journal: &EffectJournal) -> Result<usize, String> {
    let mut imported = 0usize;
    let composite = root.join(COMPOSITE_MIRROR_FILE_NAME);
    if let Some(bytes) = read_legacy_mirror(&composite)? {
        let store: Store = serde_json::from_slice(&bytes)
            .map_err(|_| "legacy composite mirror is unreadable; refusing a partial import")?;
        if store.records.len() > 4096 {
            return Err("legacy mirror capacity exceeded".into());
        }
        if store.version != 1 {
            return Err("legacy composite mirror version is unsupported".into());
        }
        for record in store.records {
            journal.import_legacy(
                &record.source,
                &record.event_id,
                &record.digest,
                record.timestamp_ms,
                record.completed,
            )?;
            // A legacy terminal result is preserved; a legacy claim with no result
            // stays unresolved and is never re-applied.
            if let Some(result) = record.result {
                journal.report(&record.source, &record.event_id, result)?;
            }
            if record.completed {
                if record.result.is_none() {
                    journal.report(&record.source, &record.event_id, 3)?;
                }
                journal.complete(&record.source, &record.event_id)?;
            }
            imported += 1;
        }
    }
    let legacy = root.join(LEGACY_MIRROR_FILE_NAME);
    if let Some(bytes) = read_legacy_mirror(&legacy)? {
        let store: LegacyStore = serde_json::from_slice(&bytes)
            .map_err(|_| "legacy event mirror is unreadable; refusing a partial import")?;
        if store.records.len() > 4096 {
            return Err("legacy mirror capacity exceeded".into());
        }
        if store.version != 1 {
            return Err("legacy event mirror version is unsupported".into());
        }
        for record in store.records {
            journal.import_legacy(
                &record.source,
                &record.event_id,
                &record.digest,
                record.timestamp_ms,
                record.received,
            )?;
            journal.mark_legacy(&record.source, &record.event_id, record.received)?;
            imported += 1;
        }
    }
    Ok(imported)
}

fn read_legacy_mirror(path: &std::path::Path) -> Result<Option<Vec<u8>>, String> {
    use std::io::Read;
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("legacy mirror could not be read; refusing an empty import".into()),
    };
    let mut bytes = Vec::new();
    file.by_ref()
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "legacy mirror read failed")?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err("legacy mirror exceeds limit".into());
    }
    Ok(Some(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn crash_claim_terminal_result_and_collision_are_durable() {
        let path = std::env::temp_dir().join(format!(
            "composite-receipts-{:016x}.json",
            rand::random::<u64>()
        ));
        let e = EventEnvelope::new(b"fixture".to_vec());
        let r = Receipt::new("identity".into(), &e);
        assert!(update_at(&path, r.clone(), Operation::Claim)
            .unwrap()
            .is_none());
        assert!(update_at(&path, r.clone(), Operation::Lookup)
            .unwrap()
            .unwrap()
            .result
            .is_none());
        update_at(&path, r.clone(), Operation::Report(3)).unwrap();
        update_at(&path, r.clone(), Operation::Complete).unwrap();
        assert!(
            update_at(&path, r.clone(), Operation::Lookup)
                .unwrap()
                .unwrap()
                .completed
        );
        let mut other = r;
        other.digest = "other".into();
        assert!(update_at(&path, other, Operation::Claim).is_err());
        std::fs::remove_file(path).unwrap();
    }
}

/// R4-01 seam regressions for the effect journal (plan §4 R4-01, ledger RR-10).
#[cfg(all(test, feature = "repair-regressions"))]
mod repair_regressions {
    use super::*;

    fn mirror_path(root: &std::path::Path) -> std::path::PathBuf {
        root.join(COMPOSITE_MIRROR_FILE_NAME)
    }

    fn key(event: u8) -> Receipt {
        let mut envelope = EventEnvelope::new(b"fixture".to_vec());
        envelope.message_id = crate::protocol::MessageId([event; 16]);
        static TIMESTAMP: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
        envelope.timestamp_ms = *TIMESTAMP.get_or_init(crate::protocol::unix_timestamp_ms);
        Receipt::new("peer".into(), &envelope)
    }

    /// R4-01 — **guard**, must pass before *and* after the repair.
    ///
    /// A damaged *journal* is the one case that must never be papered over: it is the
    /// only record of whether a native effect already happened. The file is kept, the
    /// failure is reported, and no operation silently succeeds.
    #[test]
    fn rr10_a_damaged_journal_is_preserved_and_never_succeeds_silently() {
        let root =
            std::env::temp_dir().join(format!("rr10-journal-{:016x}", rand::random::<u64>()));
        crate::private_fs::create_private_dir_all(&root).unwrap();
        let journal = root.join(crate::peer::effect_journal::JOURNAL_FILE_NAME);
        let damaged = b"this is not a sqlite database".to_vec();
        std::fs::write(&journal, &damaged).unwrap();

        for _ in 0..3 {
            assert!(
                update_at(&mirror_path(&root), key(1), Operation::Lookup).is_err(),
                "a damaged journal must not report a known state"
            );
        }
        assert_eq!(
            std::fs::read(&journal).unwrap(),
            damaged,
            "the damaged journal must be preserved for diagnosis; rebuilding it empty \
             would erase the evidence of whether an effect already happened"
        );
        std::fs::remove_dir_all(root).ok();
    }

    /// R4-01 / G-10 — **defect**, expected to FAIL until the journal lands.
    ///
    /// The verification reproduced the old behaviour: three consecutive `Lookup`s on a
    /// corrupt JSON store all returned the same opaque hard error, the file was
    /// preserved, and **no recovery path and no visible fault existed**. Because that
    /// JSON file was the sole authority, one corrupt byte permanently disabled
    /// composite receiving for every peer.
    ///
    /// With the journal authoritative the mirror is *derived*: a damaged mirror is
    /// rebuilt from the ledger and lookups keep working.
    #[test]
    fn rr10b_a_damaged_mirror_is_rebuilt_from_the_ledger() {
        let _guard = crate::sync_warning::test_lock();
        while crate::sync_warning::take().is_some() {}
        let root = std::env::temp_dir().join(format!("rr10-mirror-{:016x}", rand::random::<u64>()));
        let mirror = mirror_path(&root);

        // Establish a real record, then damage the derived mirror.
        assert!(update_at(&mirror, key(1), Operation::Claim)
            .unwrap()
            .is_none());
        std::fs::write(&mirror, b"{ this is not json").unwrap();

        let recovered = update_at(&mirror, key(1), Operation::Lookup);
        let visible = crate::sync_warning::peek().is_some();
        let recovered_ok = recovered.is_ok();
        assert!(
            recovered_ok,
            "a damaged mirror must be rebuilt from the authoritative ledger, got {}",
            recovered.as_ref().err().map(String::as_str).unwrap_or("ok")
        );
        let record = recovered.unwrap().expect("the claim is still known");
        assert_eq!(record.event_id, key(1).event_id);
        let rebuilt: Store = serde_json::from_slice(&std::fs::read(&mirror).unwrap())
            .expect("the mirror is valid JSON again");
        assert_eq!(
            rebuilt.records.len(),
            1,
            "the mirror was rebuilt from the ledger"
        );

        // The visible-fault channel stays available for the cases that need it.
        assert!(
            recovered_ok || visible,
            "a damaged receipt store must end in a state the system can act on"
        );
        std::fs::remove_dir_all(root).ok();
    }
}
