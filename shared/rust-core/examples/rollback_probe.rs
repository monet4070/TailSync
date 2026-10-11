//! Produce a restored pre-upgrade store for cross-version verification.
//!
//! Repair plan §4 R1-03: the rollback must be proven by having the **old binary**
//! open the restored store and read text, image, file and favorite entries. This
//! probe builds that store with the current code (a v11 fixture carrying real
//! external payloads, upgraded so the rollback package is written, then restored
//! into a fresh directory). The companion reader is built from the baseline commit
//! and consumes `<work>/restored`.
//!
//! Usage:
//!   cargo run -p tailsync-core --features test-support --example rollback_probe -- <work-dir>

use std::path::PathBuf;
use tailsync_core::db::{HistoryDB, MigrationPolicy};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let work: PathBuf = std::env::args()
        .nth(1)
        .ok_or("usage: rollback_probe <work-dir>")?
        .into();
    let store = work.join("store");
    let restored = work.join("restored");
    for path in [&store, &restored] {
        if path.exists() {
            std::fs::remove_dir_all(path)?;
        }
    }
    std::fs::create_dir_all(&work)?;

    let image_bytes = packed_image(6, 6, 0x2a);
    let file_bytes = b"cross-version file payload".to_vec();

    // 1) A v12 store with real external payloads, stripped back to a v11 shape.
    let (text_id, image_id, file_id) = {
        let mut db = HistoryDB::open_isolated_for_test_composite(&store)?;
        db.add_text("cross-version text fixture", "self")?;
        let text_id = db.conn_last_insert_rowid();
        db.add_image(&image_bytes, "self")?;
        let image_id = db.conn_last_insert_rowid();
        db.add_file("cross-version.bin", &file_bytes, "self")?;
        let file_id = db.conn_last_insert_rowid();
        db.set_favorite(text_id, true)?;
        db.strip_composite_schema_for_probe()?;
        (text_id, image_id, file_id)
    };

    // 2) Upgrade: this must write the complete rollback package first.
    let mut db = HistoryDB::open_with_policy_for_probe(&store, MigrationPolicy::CompositeAllowed)?;
    let upgraded_version = db.schema_version_for_probe()?;
    let post_upgrade_id = db.add_composite(
        &tailsync_core::composite::PortableBundle {
            representations: vec![tailsync_core::composite::Representation {
                mime: tailsync_core::composite::Mime::Text,
                bytes: b"post-upgrade composite archive fixture".to_vec(),
            }],
            resources: vec![],
            bindings: vec![],
            completeness: tailsync_core::composite::Completeness::Complete,
        },
        None,
        "fixture",
    )?;
    let post_upgrade_readable = db
        .get_composite(post_upgrade_id)?
        .representation(tailsync_core::composite::Mime::Text)
        == Some(b"post-upgrade composite archive fixture".as_slice());
    drop(db);

    let manifest = tailsync_core::db::verify_rollback_package(&store)?;

    // 3) Restore into a fresh directory for the baseline reader.
    tailsync_core::db::restore_rollback_package(&store, &restored)?;

    println!(
        "{}",
        serde_json::json!({
            "probe": "rollback-cross-version",
            "store": store.display().to_string(),
            "restored": restored.display().to_string(),
            "upgraded_schema_version": upgraded_version,
            "post_upgrade_id": post_upgrade_id,
            "post_upgrade_readable": post_upgrade_readable,
            "manifest_source_schema_version": manifest.source_schema_version,
            "manifest_payloads": manifest.payloads.len(),
            "manifest_total_bytes": manifest.total_bytes,
            "manifest_excludes": manifest.excludes,
            "fixture": {
                "text_id": text_id,
                "image_id": image_id,
                "file_id": file_id,
                "text_bytes": "cross-version text fixture".len(),
                "image_bytes": image_bytes.len(),
                "file_bytes": file_bytes.len(),
            }
        })
    );
    Ok(())
}

fn packed_image(width: u32, height: u32, channel: u8) -> Vec<u8> {
    let mut data = Vec::with_capacity(8 + (width * height * 4) as usize);
    data.extend_from_slice(&width.to_le_bytes());
    data.extend_from_slice(&height.to_le_bytes());
    data.extend(std::iter::repeat_n(channel, (width * height * 4) as usize));
    data
}
