//! DB/runtime preparation -> production supervisor -> real packaged helper.
//! Synthetic private boards only; no installed App or target-app acceptance.
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tailsync_core::{
    composite::{
        self,
        origin::{Origin, OriginTracker},
        restore::{RestorePlan, RestoreSelection, RestoreTarget},
        supervisor::{HelperOutcome, HelperRequest},
        Completeness, Mime, PortableBundle, Representation, Resource, ResourceBinding,
    },
    db::HistoryDB,
    protocol::{native_archive::NativeArchive, native_clipboard::*},
};
use tailsync_runtime::history::{HistoryOperations, PreparedHistoryRestore};

fn exchange(helper: &Path, board: &str, action: &str, input: Option<Vec<u8>>) -> Vec<u8> {
    assert!(board.starts_with("tailsync-probe.o06."));
    let out = composite::supervisor::run(HelperRequest {
        program: helper,
        args: &["--pasteboard", board, action],
        stdin: input,
        deadline: Duration::from_secs(5),
        stdout_limit: NATIVE_LIMIT,
    });
    let HelperOutcome::Exited {
        success: true,
        stdout,
        ..
    } = out
    else {
        panic!("{out:?}")
    };
    stdout
}
fn read(helper: &Path, board: &str) -> NativeArchive {
    NativeArchive::decode(&exchange(helper, board, "--read-archive", None)).unwrap()
}
fn write(
    helper: &Path,
    board: &str,
    snapshot: &NativeSnapshot,
    expected: i64,
    token: [u8; 16],
) -> PublishReceipt {
    let mut body = token.to_vec();
    body.extend_from_slice(&expected.to_be_bytes());
    body.extend_from_slice(&snapshot.encode().unwrap());
    PublishReceipt::decode(&exchange(helper, board, "--write-snapshot", Some(body))).unwrap()
}
fn ready(prepared: PreparedHistoryRestore) -> RestorePlan {
    let PreparedHistoryRestore::Ready(plan) = prepared else {
        panic!("{prepared:?}")
    };
    *plan
}
fn publish(helper: &Path, board: &str, plan: &RestorePlan, token: [u8; 16]) -> NativeArchive {
    let snapshot = plan.native().unwrap();
    let digest = composite::fingerprint::origin_fingerprint_from_native(snapshot).unwrap();
    let mut origin = OriginTracker::default();
    assert!(origin.record_intent(token, digest));
    let receipt = write(helper, board, snapshot, -1, token);
    assert_eq!(receipt.state, PublishState::Published);
    origin.record(receipt.revision, digest, receipt.token);
    let back = read(helper, board);
    assert_eq!(back.publication().unwrap().formats, snapshot.formats);
    assert_eq!(back.original.items, snapshot.items);
    assert_eq!(back.original.after, receipt.revision);
    let actual = composite::fingerprint::origin_fingerprint_from_native(&back.original).unwrap();
    assert_eq!(
        origin.classify(Some(back.original.after), actual, Some(token)),
        Origin::SelfWrite
    );
    back
}
fn rich() -> PortableBundle {
    let html = "<p>archived two images: 中文</p><img src='tailsync-resource:1'><img src='tailsync-resource:2'>".as_bytes().to_vec();
    let text = std::str::from_utf8(&html).unwrap();
    let bindings = (1..=2)
        .map(|id| {
            let uri = format!("tailsync-resource:{id}");
            ResourceBinding {
                representation: 1,
                resource: id,
                offset: text.find(&uri).unwrap() as u32,
                length: uri.len() as u32,
            }
        })
        .collect();
    PortableBundle {
        representations: vec![
            Representation {
                mime: Mime::Text,
                bytes: b"archived two images".to_vec(),
            },
            Representation {
                mime: Mime::Html,
                bytes: html,
            },
        ],
        resources: vec![
            Resource {
                id: 1,
                png: composite::packed_to_png(1, 1, &[255, 0, 0, 128]).unwrap(),
            },
            Resource {
                id: 2,
                png: composite::packed_to_png(1, 1, &[0, 0, 255, 255]).unwrap(),
            },
        ],
        bindings,
        completeness: Completeness::Complete,
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let run = PathBuf::from(&args[1]);
    let helper = Path::new(&args[2]);
    let originals = Path::new(&args[3]);
    std::fs::create_dir(&run)?;
    tailsync_core::db::configure_storage_dir(Some(&run.join("probe-db")))?;
    tailsync_core::secure::set_helper_health(true);
    let board = format!("tailsync-probe.o06.{}", std::process::id());
    let b = rich();
    let raw=NativeSnapshot {before:0,after:0,items:1,state:SnapshotState::Stable,formats:vec![
        NativeFormat{item:0,name:"public.html".into(),state:FormatState::Captured,bytes:b"<p>original</p><img src='https://fixture.invalid/red.png'><img src='https://fixture.invalid/blue.png'>".to_vec()},
        NativeFormat{item:0,name:"public.utf8-plain-text".into(),state:FormatState::Captured,bytes:b"original text".to_vec()},
    ]};
    let mut db = HistoryDB::new()?;
    let id = db.add_composite(&b, Some(&raw), "synthetic O06")?;
    let mut native_rows = vec![];
    for (index, name) in ["rtfd-a-0", "tiff"].iter().enumerate() {
        let a = NativeArchive::decode(&std::fs::read(originals.join(format!("{name}.cna")))?)?;
        native_rows.push((
            db.add_native_archive(&a, "synthetic originals")?,
            (*name).to_owned(),
            a,
            index as u8,
        ));
    }
    let opaque = NativeArchive {
        platform: 1,
        enumeration_complete: true,
        derived: vec![],
        original: NativeSnapshot {
            before: 0,
            after: 0,
            items: 2,
            state: SnapshotState::Stable,
            formats: vec![
                NativeFormat {
                    item: 0,
                    name: "org.tailsync.fixture.opaque".into(),
                    state: FormatState::Captured,
                    bytes: vec![0, 1, 255, 0],
                },
                NativeFormat {
                    item: 1,
                    name: "public.utf8-plain-text".into(),
                    state: FormatState::Captured,
                    bytes: b"second item".to_vec(),
                },
            ],
        },
    };
    native_rows.push((
        db.add_native_archive(&opaque, "synthetic opaque")?,
        "opaque".into(),
        opaque,
        2,
    ));
    drop(db);
    let mut db = HistoryDB::new()?;
    assert_eq!(db.get_composite(id)?, b);
    assert_eq!(db.get_composite_native(id)?.unwrap(), raw);
    let payload = HistoryOperations::new(&mut db).restore_payload(id)?;
    let plan = ready(payload.prepare_restore_plan(true, RestoreTarget::Macos, None)?);
    assert_eq!(plan.version(), 1);
    assert_eq!(plan.selection(), RestoreSelection::PortableCompatibility);
    assert_eq!(plan.outputs().len(), 2);
    let html = &plan
        .native()
        .unwrap()
        .formats
        .iter()
        .find(|f| f.name == "public.html")
        .unwrap()
        .bytes;
    assert_eq!(*html, composite::html::materialize(&b, 1)?);
    assert_eq!(
        std::str::from_utf8(html)?
            .matches("data:image/png;base64,")
            .count(),
        2
    );
    assert!(!std::str::from_utf8(html)?.contains("fixture.invalid"));
    let text = ready(payload.prepare_restore_plan(true, RestoreTarget::Macos, Some("text"))?);
    assert_eq!(text.selection(), RestoreSelection::ExplicitFormat);
    assert_eq!(text.outputs().len(), 1);
    assert_eq!(db.get_composite(id)?, b);
    assert_eq!(db.get_composite_native(id)?.unwrap(), raw);
    // Preparation owns all output bytes. Delete the source history through normal
    // product deletion, close the DB, then publish the already prepared plan.
    db.delete(id)?;
    drop(db);
    drop(payload);
    let back = publish(helper, &board, &plan, [1; 16]);
    std::fs::write(run.join("materialized-readback.cna"), back.encode()?)?;
    let seeded = read(helper, &board);
    let mut broken = rich();
    broken.bindings.pop();
    let mut db = HistoryDB::new()?;
    let bad = db.add_composite(&broken, None, "missing resource")?;
    let payload = HistoryOperations::new(&mut db).restore_payload(bad)?;
    assert!(matches!(
        payload.prepare_restore_plan(false, RestoreTarget::Macos, None)?,
        PreparedHistoryRestore::Unsupported {
            code: "resources_incomplete"
        }
    ));
    assert_eq!(
        read(helper, &board),
        seeded,
        "preparation refusal cannot mutate native bytes or revision"
    );
    let stale = write(
        helper,
        &board,
        text.native().unwrap(),
        seeded.original.after - 1,
        [2; 16],
    );
    assert_eq!(stale.state, PublishState::Superseded);
    assert_eq!(
        read(helper, &board),
        seeded,
        "expected revision refused before clear"
    );
    let mut results = vec![];
    for (id, name, a, index) in native_rows {
        assert_eq!(db.get_composite_archive(id)?.unwrap(), a);
        let payload = HistoryOperations::new(&mut db).restore_payload(id)?;
        let default = ready(payload.prepare_restore_plan(true, RestoreTarget::Macos, None)?);
        assert_eq!(default.selection(), RestoreSelection::Originals);
        let original_formats = default.native().unwrap().formats.clone();
        let _explicit = payload.prepare_restore_plan(true, RestoreTarget::Macos, Some("text"))?;
        let unchanged = ready(payload.prepare_restore_plan(true, RestoreTarget::Macos, None)?);
        assert_eq!(unchanged.native().unwrap().formats, original_formats);
        assert_eq!(db.get_composite_archive(id)?.unwrap(), a);
        db.delete(id)?;
        let readback = publish(helper, &board, &unchanged, [10 + index; 16]);
        std::fs::write(run.join(format!("{name}-readback.cna")), readback.encode()?)?;
        results.push(serde_json::json!({"name":name,"archive_after_reopen":true,"originals_exact":true,"explicit_independent":true,"published_after_source_deletion":true,"self_write":true,"items":unchanged.native().unwrap().items}));
    }
    let result = serde_json::json!({"status":"PASS","board":board,"plan_version":1,"checks":{
        "db_reopen":true,"two_bound_images_in_order":true,"originals_unchanged":true,
        "explicit_format_independent":true,"publication_after_source_db_close_and_delete":true,
        "refusal_keeps_seeded_board_bytes_and_revision":true,"stale_revision_refused_before_clear":true,
        "materialized_self_write":true},"originals":results,
        "scope":"production DB/runtime selector/supervisor and real packaged helper; no full App, platform adapter journal, external downloads, target apps or device acceptance"});
    std::fs::write(
        run.join("results.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    println!("{result}");
    Ok(())
}
