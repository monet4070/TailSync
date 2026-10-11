//! Isolated native byte admission proof; only named synthetic pasteboards.
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tailsync_core::{
    composite::{
        self,
        fingerprint::origin_fingerprint_from_native,
        origin::{Origin, OriginTracker},
        supervisor::{self, HelperOutcome, HelperRequest},
    },
    db::HistoryDB,
    protocol::{native_archive::NativeArchive, native_clipboard::*},
};
fn exchange(helper: &Path, args: &[&str], body: Option<Vec<u8>>) -> HelperOutcome {
    supervisor::run(HelperRequest {
        program: helper,
        args,
        stdin: body,
        deadline: Duration::from_secs(5),
        stdout_limit: NATIVE_LIMIT,
    })
}
fn read(helper: &Path, board: &str) -> Result<NativeArchive, Box<dyn std::error::Error>> {
    let HelperOutcome::Exited {
        success: true,
        stdout,
        ..
    } = exchange(helper, &["--pasteboard", board, "--read-archive"], None)
    else {
        return Err("archive read failed".into());
    };
    Ok(NativeArchive::decode(&stdout)?)
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args[1] == "timeout" {
        let outcome = exchange(
            Path::new(&args[2]),
            &["--pasteboard", &args[3], "--read-archive"],
            None,
        );
        assert!(
            matches!(outcome, HelperOutcome::TimedOut { .. }),
            "{outcome:?}"
        );
        println!(
            "{}",
            serde_json::json!({"status":"PASS","outcome":"TimedOut","scope":"production supervisor returns only after kill/reap and pipe joins"})
        );
        return Ok(());
    }
    let run = PathBuf::from(&args[1]);
    let helper = Path::new(&args[2]);
    let root = run.join("probe-db");
    tailsync_core::db::configure_storage_dir(Some(&root))?;
    tailsync_core::secure::set_helper_health(true);
    let index: Vec<String> =
        serde_json::from_slice(&std::fs::read(run.join("capture-index.json"))?)?;
    let mut db = HistoryDB::new()?;
    let mut saved = vec![];
    let mut results = vec![];
    for name in index {
        let a = NativeArchive::decode(&std::fs::read(run.join(format!("{name}.cna")))?)?;
        if a.original.state != SnapshotState::Stable {
            let before = db.get_all(None, None, 50, 0)?.len();
            assert!(db.add_native_archive(&a, "synthetic").is_err());
            assert_eq!(db.get_all(None, None, 50, 0)?.len(), before);
            assert!(composite::from_archive(&a).is_err());
            results.push(serde_json::json!({"name":name,"rejected_before_history":true,"state":a.original.state as u8}));
            continue;
        }
        let id = db.add_native_archive(&a, "synthetic O02")?;
        assert!(db.composite_identity(id)?.unwrap().dedupe_key.is_none());
        saved.push((name, id, a));
    }
    drop(db);
    let db = HistoryDB::new()?;
    for (name, id, a) in saved {
        assert_eq!(db.get_composite_archive(id)?.unwrap(), a);
        let native = db.get_composite_native(id)?.unwrap();
        let admitted = a.originals_complete()
            && native.formats.iter().all(|f| {
                tailsync_core::protocol::native_policy::macos_publishable(&f.name, f.bytes.len())
            });
        let mut published = false;
        if admitted {
            let target = origin_fingerprint_from_native(&native)?;
            let token = rand::random::<[u8; 16]>();
            let mut tracker = OriginTracker::default();
            tracker.record_intent(token, target);
            let board = format!("tailsync-probe.o02.restore-{}", rand::random::<u64>());
            let mut body = token.to_vec();
            body.extend_from_slice(&(-1i64).to_be_bytes());
            body.extend_from_slice(&native.encode()?);
            let HelperOutcome::Exited {
                success: true,
                stdout,
                ..
            } = exchange(
                helper,
                &["--pasteboard", &board, "--write-snapshot"],
                Some(body),
            )
            else {
                return Err("native restore failed".into());
            };
            let receipt = PublishReceipt::decode(&stdout)?;
            assert_eq!(receipt.state, PublishState::Published);
            tracker.record(receipt.revision, target, receipt.token);
            let back = read(helper, &board)?;
            assert_eq!(back.publication()?.formats, native.formats);
            assert_eq!(back.original.items, native.items);
            let matched = origin_fingerprint_from_native(&back.original)?;
            assert_eq!(target, matched);
            assert_eq!(
                tracker.classify(Some(back.original.after), matched, Some(token)),
                Origin::SelfWrite
            );
            std::fs::write(run.join(format!("{name}.restored.cna")), back.encode()?)?;
            published = true;
        }
        results.push(serde_json::json!({"name":name,"id":id,"archive_exact_after_restart":true,"items":native.items,"formats":native.formats.len(),"native_admitted":admitted,"published_and_selfwrite":published}));
    }
    std::fs::write(
        run.join("storage-results.json"),
        serde_json::to_vec_pretty(&results)?,
    )?;
    println!(
        "{}",
        serde_json::json!({"status":"PASS","checks":results.len()})
    );
    Ok(())
}
