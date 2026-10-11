//! Bounded write-ahead clipboard claims in the private identity/settings root.
//! A claim is durable BEFORE native I/O. After a crash an uncertain claim never
//! reapplies the effect; history can still be retried and durably acknowledged.
//! This deliberately provides at-most-once effect attempts, not atomic OS/SQL
//! exactly-once delivery. No clipboard bytes or paths are stored here.
use super::effect_journal::{ClaimOutcome, EffectJournal};
use crate::protocol::{unix_timestamp_ms, EventEnvelope, EVENT_TIMESTAMP_WINDOW_MS};
use serde::{Deserialize, Serialize};

const MAX_RECEIPTS: usize = 4096;
const MAX_STORE_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Receipt {
    source: String,
    event_id: String,
    digest: String,
    timestamp_ms: i64,
    pub received: bool,
    #[serde(skip)]
    pub history_ref: Option<i64>,
}
impl Receipt {
    pub fn operation_key(&self) -> super::operation_lock::OperationKey {
        super::operation_lock::OperationKey::new(self.source.clone(), self.event_id.clone())
    }

    pub fn new(
        source: String,
        command: crate::protocol::Command,
        envelope: &EventEnvelope,
    ) -> Self {
        let mut hash = blake3::Hasher::new();
        hash.update(&[command as u8]);
        hash.update(&envelope.timestamp_ms.to_le_bytes());
        hash.update(&envelope.content);
        Self {
            source,
            event_id: envelope.message_id.as_hex(),
            digest: hash.finalize().to_hex().to_string(),
            timestamp_ms: envelope.timestamp_ms,
            received: false,
            history_ref: None,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Store {
    version: u32,
    records: Vec<Receipt>,
}
#[derive(Clone, Copy)]
pub(super) enum Operation {
    Lookup,
    #[allow(dead_code)]
    Claim,
    ClaimPrepared(i64),
    Complete,
    Remove,
}

/// All read/modify/write operations are serialized. Production's existing
/// single-instance lease protects the data directory across processes.
pub(super) async fn update(key: Receipt, operation: Operation) -> Result<Option<Receipt>, String> {
    tokio::task::spawn_blocking(move || {
        update_at(
            &crate::db::get_data_dir().join("remote-event-receipts-v1.json"),
            key,
            operation,
        )
    })
    .await
    .map_err(|_| "Remote event receipt worker failed".to_string())?
}
fn update_at(
    path: &std::path::Path,
    key: Receipt,
    operation: Operation,
) -> Result<Option<Receipt>, String> {
    let _guard = super::composite_receipts::LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = path.parent().ok_or("Remote receipt directory missing")?;
    let journal = super::composite_receipts::initialize_at(root)?;
    if key.timestamp_ms < journal.replay_floor_ms()? {
        return Err("event timestamp is outside the accepted window (journal replay floor)".into());
    }
    if matches!(operation, Operation::Claim | Operation::ClaimPrepared(_))
        && journal.lookup(&key.source, &key.event_id)?.is_none()
        && journal.count_for_kind("legacy")? >= MAX_RECEIPTS
    {
        return Err(format!(
            "{}remote receipt capacity reached",
            crate::protocol::TEMPORARY_PRESSURE_MARKER
        ));
    }
    let existing = journal.lookup(&key.source, &key.event_id)?;
    if existing
        .as_ref()
        .is_some_and(|record| record.digest != key.digest)
    {
        return Err("Remote event ID collision".into());
    }
    let result = match operation {
        Operation::Lookup => existing,
        Operation::Claim | Operation::ClaimPrepared(_) => match journal.claim_prepared(
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
                journal.mark_legacy(&key.source, &key.event_id, false)?;
                None
            }
            ClaimOutcome::Existing(record) => Some(record),
        },
        Operation::Complete => {
            journal.mark_legacy(&key.source, &key.event_id, true)?;
            if journal
                .lookup(&key.source, &key.event_id)?
                .is_some_and(|r| r.result.is_none())
            {
                journal.report(
                    &key.source,
                    &key.event_id,
                    crate::protocol::composite::AckResult::ApplyUnknown as u8,
                )?;
            }
            journal.complete(&key.source, &key.event_id)?;
            journal.lookup(&key.source, &key.event_id)?
        }
        Operation::Remove => {
            journal.remove(&key.source, &key.event_id)?;
            None
        }
    };
    write_mirror(path, &journal)?;
    result
        .map(|record| {
            Ok(Receipt {
                history_ref: record.history_ref,
                received: journal.legacy_received(&record.source, &record.event_id)?,
                source: record.source,
                event_id: record.event_id,
                digest: record.digest,
                timestamp_ms: record.timestamp_ms,
            })
        })
        .transpose()
}

pub(super) fn write_mirror(path: &std::path::Path, journal: &EffectJournal) -> Result<(), String> {
    let now = unix_timestamp_ms();
    let records: Vec<Receipt> = journal
        .records_for_kind(Some("legacy"))?
        .into_iter()
        .filter(|record| {
            record
                .timestamp_ms
                .saturating_add(EVENT_TIMESTAMP_WINDOW_MS)
                >= now
        })
        .map(|record| {
            Ok(Receipt {
                history_ref: record.history_ref,
                received: journal.legacy_received(&record.source, &record.event_id)?,
                source: record.source,
                event_id: record.event_id,
                digest: record.digest,
                timestamp_ms: record.timestamp_ms,
            })
        })
        .collect::<Result<_, String>>()?;
    if records.len() > MAX_RECEIPTS {
        return Err("Remote event receipt capacity reached".into());
    }
    let bytes = serde_json::to_vec(&Store {
        version: 1,
        records,
    })
    .map_err(|_| "Remote receipt encoding failed")?;
    if bytes.len() as u64 > MAX_STORE_BYTES {
        return Err("Remote receipt store too large".into());
    }
    crate::private_fs::create_private_dir_all(
        path.parent().ok_or("Remote receipt directory missing")?,
    )
    .map_err(|_| "Remote receipt directory unavailable")?;
    crate::private_fs::write_private_file(path, &bytes)
        .map_err(|_| "Remote receipt persistence failed")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Command;
    fn path() -> std::path::PathBuf {
        let root =
            crate::db::get_data_dir().join(format!("receipt-test-{:016x}", rand::random::<u64>()));
        std::fs::create_dir_all(&root).unwrap();
        root.join(super::super::effect_journal::LEGACY_MIRROR_FILE_NAME)
    }
    fn key() -> Receipt {
        Receipt::new(
            "noise-key".into(),
            Command::TextPayload,
            &EventEnvelope::new(b"hello".to_vec()),
        )
    }
    #[test]
    fn receipt_collision_identity_and_crash_claim() {
        let path = path();
        let key = key();
        assert!(update_at(&path, key.clone(), Operation::Claim)
            .unwrap()
            .is_none());
        assert!(
            !update_at(&path, key.clone(), Operation::Lookup)
                .unwrap()
                .unwrap()
                .received
        );
        let mut collision = key.clone();
        collision.digest = "different".into();
        assert!(update_at(&path, collision, Operation::Claim)
            .unwrap_err()
            .contains("collision"));
        let mut other = key.clone();
        other.source = "different-noise-key".into();
        assert!(update_at(&path, other, Operation::Claim).unwrap().is_none());
        update_at(&path, key.clone(), Operation::Complete).unwrap();
        assert!(
            update_at(&path, key, Operation::Lookup)
                .unwrap()
                .unwrap()
                .received
        );
    }
    #[test]
    fn capacity_never_evicts_live_claims_and_expiry_gc_is_bounded() {
        let path = path();
        let key = key();
        let mut records = vec![key.clone(); MAX_RECEIPTS];
        for (i, r) in records.iter_mut().enumerate() {
            r.event_id = i.to_string();
        }
        std::fs::write(
            &path,
            serde_json::to_vec(&Store {
                version: 1,
                records,
            })
            .unwrap(),
        )
        .unwrap();
        assert!(update_at(&path, key.clone(), Operation::Claim)
            .unwrap_err()
            .contains("capacity"));
        // Expiry is driven by the authoritative journal, never by editing a mirror.
        let conn = rusqlite::Connection::open(
            path.parent()
                .unwrap()
                .join(super::super::effect_journal::JOURNAL_FILE_NAME),
        )
        .unwrap();
        conn.execute("UPDATE effects SET timestamp_ms = 0", [])
            .unwrap();
        drop(conn);
        assert!(update_at(&path, key.clone(), Operation::Claim)
            .unwrap()
            .is_none());
        let store: Store = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(store.records.len(), 1);
    }
    #[test]
    fn corrupt_future_and_write_failure_fail_closed() {
        let path = path();
        let key = key();
        for bytes in [
            b"broken".as_slice(),
            b"{\"version\":2,\"records\":[]}".as_slice(),
        ] {
            std::fs::write(&path, bytes).unwrap();
            assert!(update_at(&path, key.clone(), Operation::Claim).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(update_at(&path, key, Operation::Claim).is_err());
    }
}
