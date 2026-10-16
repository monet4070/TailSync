//! Verify one existing image in a private SQLite backup, never the live store.
use rusqlite::{Connection, OpenFlags};
use std::path::PathBuf;
use tailsync_core::{composite, crypto, db::HistoryDB};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(
        std::env::args()
            .nth(1)
            .ok_or("snapshot directory required")?,
    )
    .canonicalize()?;
    if root.file_name().and_then(|name| name.to_str()) != Some("snapshot") {
        return Err("this probe only opens a directory named snapshot".into());
    }
    let id: i64 = std::env::args()
        .nth(2)
        .ok_or("entry id required")?
        .parse()?;
    let before =
        Connection::open_with_flags(root.join("history-v2.db"), OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let stored: Vec<u8> =
        before.query_row("SELECT data FROM history WHERE id=?1", [id], |row| {
            row.get(0)
        })?;
    let manifest: serde_json::Value = serde_json::from_slice(&crypto::decrypt(&stored)?)?;
    drop(before);
    tailsync_core::db::configure_storage_dir(Some(&root))?;
    let db = HistoryDB::new()?;
    let identity = db.composite_identity(id)?;
    let bundle = db.get_composite(id)?;
    let png = bundle
        .representation(composite::Mime::Png)
        .ok_or("PNG missing")?;
    let metadata = tailsync_core::protocol::image::metadata("public.png", png)
        .ok_or("invalid image metadata")?;
    let expected = format!(
        "image/png · {}×{} · 1 frame",
        metadata.width, metadata.height
    );
    let mut entry = db
        .get_all(None, None, 30, 0)?
        .into_iter()
        .find(|entry| entry.id == id)
        .ok_or("entry missing from page")?;
    db.attach_display_info(&mut entry)?;
    assert_eq!(entry.description, expected);
    assert_eq!(entry.display.as_ref().unwrap().kind, "image");
    let preview = db.get_preview_payload(id)?;
    assert_eq!(preview.kind, "image");
    assert_eq!(preview.data, composite::png_to_packed(png)?);
    assert_eq!(db.composite_identity(id)?, identity);
    drop(db);
    let after =
        Connection::open_with_flags(root.join("history-v2.db"), OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let unchanged: Vec<u8> =
        after.query_row("SELECT data FROM history WHERE id=?1", [id], |row| {
            row.get(0)
        })?;
    assert_eq!(stored, unchanged);
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "entry_id": id, "stored_summary": manifest["summary"], "displayed_summary": entry.description,
            "display_kind": entry.display.unwrap().kind, "width": metadata.width, "height": metadata.height,
            "png_blake3": blake3::hash(png).to_hex().to_string(),
            "encrypted_manifest_unchanged": true, "history_identity_unchanged": true, "pixel_preview_matches": true
        }))?
    );
    Ok(())
}
