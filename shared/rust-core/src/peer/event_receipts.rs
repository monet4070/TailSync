//! Bounded write-ahead clipboard claims in the private identity/settings root.
//! A claim is durable BEFORE native I/O. After a crash an uncertain claim never
//! reapplies the effect; history can still be retried and durably acknowledged.
//! This deliberately provides at-most-once effect attempts, not atomic OS/SQL
//! exactly-once delivery. No clipboard bytes or paths are stored here.
use crate::protocol::{unix_timestamp_ms, EventEnvelope, EVENT_TIMESTAMP_WINDOW_MS};
use serde::{Deserialize, Serialize};
use std::sync::{LazyLock, Mutex};

const MAX_RECEIPTS: usize = 4096;
const MAX_STORE_BYTES: u64 = 2 * 1024 * 1024;
static STORE_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Receipt {
    source: String,
    event_id: String,
    digest: String,
    timestamp_ms: i64,
    pub received: bool,
}
impl Receipt {
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
    Claim,
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
    let _guard = STORE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut store = match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() && meta.len() <= MAX_STORE_BYTES => {
            crate::private_fs::restrict_private_file(path)
                .map_err(|_| "Remote event receipt store is unavailable")?;
            let bytes =
                std::fs::read(path).map_err(|_| "Remote event receipt store is unavailable")?;
            serde_json::from_slice::<Store>(&bytes)
                .map_err(|_| "Remote event receipt store is corrupt")?
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Store {
            version: 1,
            records: vec![],
        },
        _ => return Err("Remote event receipt store is unavailable".into()),
    };
    if store.version != 1 || store.records.len() > MAX_RECEIPTS {
        return Err("Remote event receipt store format is unsupported".into());
    }
    // Never evict a receipt whose ORIGINAL envelope can still pass timestamp
    // admission. Future-skewed events can therefore occupy a slot for 10 min.
    let now = unix_timestamp_ms();
    store
        .records
        .retain(|r| r.timestamp_ms.saturating_add(EVENT_TIMESTAMP_WINDOW_MS) >= now);
    let index = store
        .records
        .iter()
        .position(|r| r.source == key.source && r.event_id == key.event_id);
    if let Some(index) = index {
        if store.records[index].digest != key.digest {
            return Err("Remote event ID collision".into());
        }
    }
    let result = match operation {
        Operation::Lookup => return Ok(index.map(|i| store.records[i].clone())),
        Operation::Claim => {
            if let Some(index) = index {
                return Ok(Some(store.records[index].clone()));
            }
            if store.records.len() >= MAX_RECEIPTS {
                return Err("Remote event receipt capacity reached; retry after expired events are collected".into());
            }
            store.records.push(key);
            None
        }
        Operation::Complete => {
            let index = index.ok_or("Remote event receipt claim is missing")?;
            store.records[index].received = true;
            Some(store.records[index].clone())
        }
        Operation::Remove => {
            if let Some(index) = index {
                store.records.remove(index);
            }
            None
        }
    };
    crate::private_fs::create_private_dir_all(
        path.parent()
            .ok_or("Remote event receipt directory is missing")?,
    )
    .map_err(|_| "Remote event receipt directory is unavailable")?;
    let bytes = serde_json::to_vec(&store).map_err(|_| "Remote event receipt encoding failed")?;
    crate::private_fs::write_private_file(path, &bytes)
        .map_err(|_| "Remote event receipt persistence failed")?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Command;
    fn path() -> std::path::PathBuf {
        crate::db::get_data_dir().join(format!("receipt-test-{:016x}.json", rand::random::<u64>()))
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
        let mut store: Store = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        for r in &mut store.records {
            r.timestamp_ms = 0;
        }
        std::fs::write(&path, serde_json::to_vec(&store).unwrap()).unwrap();
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
