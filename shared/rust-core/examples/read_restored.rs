//! Baseline (f719d58) reader for the R1-03 rollback verification.
//!
//! This is the *old binary* the repair plan requires: it opens the restored
//! pre-upgrade store through the production `HistoryDB::new()` entry point and
//! reads every entry back. If the rollback package had omitted the external
//! payloads, `get_data` would fail here — which is exactly the failure the
//! database-only snapshot could not detect.
//!
//! Usage: read_restored <storage-dir>

use tailsync_core::db::HistoryDB;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let storage = std::env::args()
        .nth(1)
        .ok_or("usage: read_restored <storage-dir>")?;
    // Point both roots at the restored copy so the baseline never touches real data.
    std::env::set_var("TAILSYNC_STORAGE_DIR", &storage);
    std::env::set_var("TAILSYNC_DATA_DIR", &storage);

    let db = HistoryDB::new()?;
    let entries = db.get_all(None, None, 50, 0)?;

    let mut read = Vec::new();
    for entry in &entries {
        let data = db.get_data(entry.id)?;
        read.push(serde_json::json!({
            "id": entry.id,
            "type": entry.entry_type,
            "pinned": entry.pinned,
            "size_bytes": entry.size_bytes,
            "data_len": data.len(),
            "data_hash_matches": blake3::hash(&data).to_hex().to_string() == entry.data_hash,
        }));
    }

    println!(
        "{}",
        serde_json::json!({
            "reader": "baseline-f719d58",
            "baseline_schema_version": 11,
            "storage": storage,
            "entries_read": read.len(),
            "all_payloads_readable": read.iter().all(|entry| {
                entry["data_len"].as_u64().unwrap_or(0) > 0
                    && entry["data_hash_matches"].as_bool().unwrap_or(false)
            }),
            "favorites": read.iter().filter(|entry| entry["pinned"] == true).count(),
            "entries": read,
        })
    );
    Ok(())
}
