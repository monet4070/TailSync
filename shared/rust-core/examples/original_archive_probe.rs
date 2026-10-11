//! Synthetic production storage/restart/publication preparation; no general board.
use tailsync_core::{
    composite::fingerprint::origin_fingerprint_from_native,
    db::HistoryDB,
    protocol::{native_archive::NativeArchive, native_clipboard::NativeSnapshot},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let run = std::path::PathBuf::from(std::env::args().nth(1).ok_or("run directory required")?);
    let inputs: serde_json::Value =
        serde_json::from_slice(&std::fs::read(run.join("capture-index.json"))?)?;
    let helper = std::env::args().nth(2).ok_or("fresh helper required")?;
    let root = run.join("probe-db");
    tailsync_core::db::configure_storage_dir(Some(&root))?;
    tailsync_core::secure::set_helper_health(true);
    let mut db = HistoryDB::new()?;
    let mut ids = vec![];
    for case in inputs.as_array().ok_or("case array required")? {
        let name = case["name"].as_str().ok_or("name")?;
        let a = NativeArchive::decode(&std::fs::read(run.join(format!("{name}.cna")))?)?;
        assert!(!a.derived.is_empty());
        let id = db.add_native_archive(&a, "synthetic O01")?;
        ids.push((name.to_string(), id, a));
    }
    drop(db);
    let db = HistoryDB::new()?;
    let mut results = vec![];
    for (name, id, a) in ids {
        let source =
            NativeSnapshot::decode(&std::fs::read(run.join(format!("{name}.source.cns")))?)?;
        let stored = db.get_composite_archive(id)?.ok_or("archive missing")?;
        assert_eq!(stored, a);
        let native = db.get_composite_native(id)?.ok_or("native missing")?;
        for f in &source.formats {
            assert!(native
                .formats
                .iter()
                .any(|g| g.item == f.item && g.name == f.name && g.bytes == f.bytes));
        }
        assert!(!native
            .formats
            .iter()
            .any(|f| f.name == "org.tailsync.rtfd-converted"));
        std::fs::write(run.join(format!("{name}.publish.cns")), native.encode()?)?;
        use tailsync_core::composite::{
            origin::{Origin, OriginTracker},
            supervisor::{self, HelperOutcome, HelperRequest},
        };
        use tailsync_core::protocol::native_clipboard::{
            PublishReceipt, PublishState, NATIVE_LIMIT,
        };
        let board = format!(
            "tailsync-probe.o01.restore-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        );
        let target = origin_fingerprint_from_native(&native)?;
        let token = rand::random::<[u8; 16]>();
        let mut tracker = OriginTracker::default();
        tracker.record_intent(token, target);
        let mut body = token.to_vec();
        body.extend_from_slice(&(-1i64).to_be_bytes());
        body.extend_from_slice(&native.encode()?);
        let HelperOutcome::Exited {
            success: true,
            stdout,
            ..
        } = supervisor::run(HelperRequest {
            program: std::path::Path::new(&helper),
            args: &["--pasteboard", &board, "--write-snapshot"],
            stdin: Some(body),
            deadline: std::time::Duration::from_secs(5),
            stdout_limit: NATIVE_LIMIT,
        })
        else {
            return Err("private publication failed".into());
        };
        let receipt = PublishReceipt::decode(&stdout)?;
        assert_eq!(receipt.state, PublishState::Published);
        tracker.record(receipt.revision, target, receipt.token);
        let HelperOutcome::Exited {
            success: true,
            stdout,
            ..
        } = supervisor::run(HelperRequest {
            program: std::path::Path::new(&helper),
            args: &["--pasteboard", &board, "--read-archive"],
            stdin: None,
            deadline: std::time::Duration::from_secs(5),
            stdout_limit: NATIVE_LIMIT,
        })
        else {
            return Err("private archive readback failed".into());
        };
        std::fs::write(run.join(format!("{name}.restored.cna")), &stdout)?;
        let restored = NativeArchive::decode(&stdout)?;
        for f in &source.formats {
            assert!(restored
                .original
                .formats
                .iter()
                .any(|g| g.item == f.item && g.name == f.name && g.bytes == f.bytes));
        }
        let matched = origin_fingerprint_from_native(&restored.original)?;
        assert_eq!(target, matched);
        assert_eq!(
            tracker.classify(Some(restored.original.after), matched, Some(token)),
            Origin::SelfWrite
        );
        results.push(serde_json::json!({"name":name,"id":id,"original_formats":native.formats.len(),"derived_formats":stored.derived.len(),"content_id":db.composite_identity(id)?.unwrap().content_id,"native_fingerprint":hex::encode(origin_fingerprint_from_native(&native)?)}));
    }
    std::fs::write(
        run.join("storage-results.json"),
        serde_json::to_vec_pretty(&results)?,
    )?;
    println!(
        "{} captures: originals/derived/restart/native preparation PASS",
        results.len()
    );
    Ok(())
}
