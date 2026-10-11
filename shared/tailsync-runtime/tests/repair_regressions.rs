//! R0 production-seam regressions for the composite-clipboard repair (runtime side).
//!
//! Plan: `docs/COMPOSITE-CLIPBOARD-REPAIR-EXECUTION-PLAN-2026-10-04.md` §4 R0.2.
//! Ledger: `artifacts/composite-repair/2026-10-04-r0/expected-failures.json`.
//!
//! These assert the **desired** behaviour of the restore defects, so they are
//! expected to FAIL until R5-03 lands. Gated behind `repair-regressions` rather
//! than weakened; the gate joins the standard entry at R6.
//!
//! Run with (plan §6):
//! `node scripts/run-isolated-tests.mjs -- cargo test --locked --offline \
//!    -p tailsync-runtime --features repair-regressions,test-support --test repair_regressions`
#![cfg(feature = "repair-regressions")]

use std::sync::{Arc, Mutex};

use tailsync_core::composite::{
    packed_to_png, Completeness, Mime, PortableBundle, Representation, Resource, ResourceBinding,
};
use tailsync_core::protocol::native_clipboard::{PublishReceipt, PublishState};
use tailsync_core::protocol::TransferId;
use tailsync_core::sync::{
    FileBatchProgress, FileReceiveCommit, PlatformResultFuture, SyncEngine, SyncPlatform,
};
use tailsync_runtime::history::{HistoryRestorePayload, RestoreOutcome};

/// Records what the engine actually handed to the platform, so the regression can
/// assert on the *published* set rather than on a return value.
#[derive(Default)]
struct Recording {
    published: Mutex<Vec<PortableBundle>>,
    native: Mutex<Vec<tailsync_core::protocol::native_clipboard::NativeSnapshot>>,
    delay: std::sync::atomic::AtomicBool,
    entered: tokio::sync::Notify,
}

#[tokio::test]
async fn maximum_legal_rgba_image_can_be_restored_asynchronously() {
    let rgba = vec![0x7f; tailsync_core::composite::RGBA_LIMIT];
    let bundle = PortableBundle {
        representations: vec![Representation {
            mime: Mime::Png,
            bytes: packed_to_png(2048, 4096, &rgba).unwrap(),
        }],
        resources: vec![],
        bindings: vec![],
        completeness: Completeness::Complete,
    };
    let recording = Arc::new(Recording::default());
    let mut engine = SyncEngine::new();
    engine.set_platform(recording.clone());
    let result = payload_for(&bundle, false)
        .restore_composite_async(Arc::new(tokio::sync::Mutex::new(engine)), None)
        .await;
    assert!(
        result.is_ok(),
        "a legal 32 MiB RGBA image must be restorable: {result:?}"
    );
    let published = recording.published.lock().unwrap();
    assert_eq!(published.len(), 1);
    assert_eq!(published[0], bundle);
    assert_eq!(
        tailsync_core::composite::budgets::global()
            .snapshot()
            .pipeline_reserved,
        0
    );
}

impl SyncPlatform for Recording {
    fn supports_native_snapshot(
        &self,
        _: &tailsync_core::protocol::native_clipboard::NativeSnapshot,
    ) -> bool {
        true
    }
    fn write_native_snapshot(
        &self,
        snapshot: &tailsync_core::protocol::native_clipboard::NativeSnapshot,
    ) -> Result<tailsync_core::composite::NativePublish, String> {
        self.native.lock().unwrap().push(snapshot.clone());
        Ok(tailsync_core::composite::NativePublish::Published(
            PublishReceipt {
                state: PublishState::Published,
                revision: 1,
                token: [1; 16],
            },
        ))
    }
    fn write_text(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn write_image(&self, _: u32, _: u32, _: &[u8]) -> Result<(), String> {
        Ok(())
    }
    fn write_composite(
        &self,
        bundle: &PortableBundle,
        _: Option<i64>,
    ) -> Result<tailsync_core::composite::NativePublish, String> {
        self.entered.notify_one();
        if self.delay.load(std::sync::atomic::Ordering::Acquire) {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        self.published.lock().unwrap().push(bundle.clone());
        Ok(tailsync_core::composite::NativePublish::Published(
            PublishReceipt {
                state: PublishState::Published,
                revision: 1,
                token: [1; 16],
            },
        ))
    }
    fn set_file_progress(&self, _: &str, _: u64, _: u64) {}
    fn clear_file_progress(&self, _: Option<TransferId>, _: Option<&str>) {}
    fn set_file_batch_progress(&self, _: FileBatchProgress) {}
    fn file_batch_failed(&self, _: Option<TransferId>, _: &str) {}
    fn files_received(&self, _: FileReceiveCommit) -> PlatformResultFuture {
        Box::pin(async { Ok(()) })
    }
}

/// Independent images carried by a bundle: standalone PNG representations plus
/// bound image resources. This is what the user means by "the pictures".
fn image_count(bundle: &PortableBundle) -> usize {
    bundle
        .representations
        .iter()
        .filter(|r| r.mime == Mime::Png)
        .count()
        + bundle.resources.len()
}

/// A legal HTML entry with two bound images, as produced by a Chrome/Word copy.
fn two_image_html_bundle() -> PortableBundle {
    let html =
        b"<p>rich</p><img src=\"tailsync-resource:1\"><img src=\"tailsync-resource:2\">".to_vec();
    let text = std::str::from_utf8(&html).expect("ascii fixture");
    let bindings = (1..=2_u16)
        .map(|id| {
            let needle = format!("tailsync-resource:{id}");
            ResourceBinding {
                representation: 0,
                resource: id,
                offset: text.find(&needle).expect("binding present") as u32,
                length: needle.len() as u32,
            }
        })
        .collect();
    PortableBundle {
        representations: vec![Representation {
            mime: Mime::Html,
            bytes: html,
        }],
        resources: vec![
            Resource {
                id: 1,
                png: packed_to_png(1, 1, &[255, 0, 0, 255]).expect("1x1 png"),
            },
            Resource {
                id: 2,
                png: packed_to_png(1, 1, &[0, 0, 255, 255]).expect("1x1 png"),
            },
        ],
        bindings,
        completeness: Completeness::Complete,
    }
}

fn payload_for(bundle: &PortableBundle, native_only: bool) -> HistoryRestorePayload {
    HistoryRestorePayload {
        native_archive_complete: None,
        native_archive_platform: None,
        identity: None,
        entry_type: "composite".to_string(),
        file_path: None,
        file_name: None,
        data: Some(bundle.encode().expect("fixture bundle encodes")),
        native_only,
        native: None,
    }
}

#[tokio::test]
async fn native_only_explicit_format_is_refused_even_on_its_original_platform() {
    use tailsync_core::protocol::native_clipboard::{
        FormatState, NativeFormat, NativeSnapshot, SnapshotState,
    };
    let bundle = PortableBundle {
        representations: vec![Representation {
            mime: Mime::Text,
            bytes: b"summary".to_vec(),
        }],
        resources: vec![],
        bindings: vec![],
        completeness: Completeness::MissingFormats,
    };
    let mut payload = payload_for(&bundle, true);
    payload.native = Some(NativeSnapshot {
        before: 7,
        after: 7,
        items: 2,
        state: SnapshotState::Stable,
        formats: (0..2)
            .map(|item| NativeFormat {
                item,
                name: "public.utf8-plain-text".into(),
                state: FormatState::Captured,
                bytes: vec![b'a' + item as u8],
            })
            .collect(),
    });
    let platform = Arc::new(Recording::default());
    let mut engine = SyncEngine::new();
    engine.set_platform(platform.clone());
    assert_eq!(
        payload
            .restore_composite(&mut engine, Some("text"))
            .unwrap(),
        RestoreOutcome::Unsupported {
            code: "native_only"
        }
    );
    assert!(platform.native.lock().unwrap().is_empty());
    assert_eq!(
        payload.restore_composite(&mut engine, None).unwrap(),
        RestoreOutcome::Published
    );
    assert_eq!(platform.native.lock().unwrap().len(), 1);
}

// ---------------------------------------------------------------------------
// RR-08 / G-2 / T24 — explicit PNG restore must not silently drop images
// ---------------------------------------------------------------------------

/// The verification reproduced this against the production runtime: for a bundle
/// with **two** image resources and no standalone PNG representation, an explicit
/// `restore_format = "png"` published exactly **one** image and still returned
/// `Ok`. The UI badge meanwhile reports the image count, so the user is told the
/// entry has two pictures and is then silently given the first one.
///
/// Desired (R5-03): either publish every image the user was promised, or refuse
/// with an explicit `NoSingleImage`-class result. Silent partial success is the
/// defect.
#[tokio::test]
async fn rr08_explicit_png_restore_must_not_silently_publish_only_the_first_image() {
    let platform = Arc::new(Recording::default());
    let mut engine = SyncEngine::new();
    engine.set_platform(platform.clone() as Arc<dyn SyncPlatform>);

    let bundle = two_image_html_bundle();
    let source_images = image_count(&bundle);
    assert_eq!(source_images, 2, "fixture must carry two images");

    let result = payload_for(&bundle, false).restore_composite(&mut engine, Some("png"));

    let published_images: usize = {
        let published = platform.published.lock().unwrap();
        published.iter().map(image_count).sum()
    };

    match result {
        // The implemented guard: with several images and no standalone PNG, refuse
        // with a stable code rather than silently taking the first.
        Ok(RestoreOutcome::Unsupported { code }) => {
            assert_eq!(
                code, "no_single_image",
                "the refusal must name the reason the UI localises (G-2/G-6)"
            );
            assert_eq!(
                published_images, 0,
                "a refused restore must not reach the system clipboard"
            );
        }
        // Acceptable alternative: publish every image the row promised.
        Ok(RestoreOutcome::Published) => assert_eq!(
            published_images, source_images,
            "an explicit PNG restore reported success but published {published_images} of \
             {source_images} images: partial content must not be reported as success (G-2)"
        ),
        other => panic!(
            "an explicit PNG restore over {source_images} images must either publish all of \
             them or refuse with a stable code, got {other:?} (published {published_images})"
        ),
    }
}

// ---------------------------------------------------------------------------
// RR-09 / G-4 / T24 — native_only must be enforced by the server, not the menu
// ---------------------------------------------------------------------------

/// The verification reproduced this: for a `native_only` entry, the whole-group
/// restore is correctly refused, but `restore_format = "text"` **succeeded** and
/// published a truncated representation with `completeness = MissingFormats`.
/// The only thing hiding this today is menu-visibility logic in the two UIs, so
/// every non-UI entry point (HTTP route, Tauri command, shortcut) can bypass the
/// documented "this shape can only be restored on its original platform" promise.
///
/// Desired (R5-03): `native_only` is validated before any format selection.
#[tokio::test]
async fn rr09_native_only_entry_must_reject_explicit_format_restore() {
    let platform = Arc::new(Recording::default());
    let mut engine = SyncEngine::new();
    engine.set_platform(platform.clone() as Arc<dyn SyncPlatform>);

    let native_shape = PortableBundle {
        representations: vec![Representation {
            mime: Mime::Text,
            bytes: b"native-only shape summary".to_vec(),
        }],
        resources: Vec::new(),
        bindings: Vec::new(),
        completeness: Completeness::Complete,
    };

    let result = payload_for(&native_shape, true).restore_composite(&mut engine, Some("text"));

    assert_eq!(
        result.unwrap(),
        RestoreOutcome::Unsupported {
            code: "native_only"
        },
        "a native_only entry must be refused server-side even when an explicit format is \
         requested; hiding the menu is not a guard (G-4)"
    );
    assert_eq!(
        platform.published.lock().unwrap().len(),
        0,
        "a refused restore must not reach the system clipboard at all (G-4)"
    );
}

// ---------------------------------------------------------------------------
// RR-30 / G-1,G-6 — the listing capability must match what the server accepts
// ---------------------------------------------------------------------------

/// R5-04: the UIs build their menus from the entry listing, so the capability recorded
/// in `CompositeInfo` has to be the same answer `restore_capabilities()` gives. If the
/// two ever disagree, the menu offers an action the server refuses (or hides one it
/// would accept) — which is the defect R5-04 exists to close.
#[tokio::test]
async fn rr30_the_listing_capability_matches_the_restore_capability() {
    use tailsync_core::db::HistoryDB;

    let root = std::env::temp_dir().join(format!(
        "rr30-{:016x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0)
    ));
    let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();

    let cases: Vec<(&str, PortableBundle, bool)> = vec![
        (
            "two images, no standalone png",
            two_image_html_bundle(),
            false,
        ),
        (
            "a lone image resource",
            {
                let mut bundle = two_image_html_bundle();
                bundle.resources.truncate(1);
                bundle.bindings.truncate(1);
                bundle
            },
            false,
        ),
    ];

    for (label, bundle, native_only) in cases {
        // `native_only` is recorded from the native shape, so an entry without one is
        // never native-only; the capability rule still has to agree for the portable case.
        let _ = native_only;
        let id = db.add_composite(&bundle, None, "fixture").unwrap();
        let entry = db
            .get_all(None, None, 50, 0)
            .unwrap()
            .into_iter()
            .find(|entry| entry.id == id)
            .unwrap_or_else(|| panic!("{label}: the entry must appear in the listing"));
        let info = entry
            .composite
            .unwrap_or_else(|| panic!("{label}: a composite entry must carry its metadata"));

        let payload = payload_for(&bundle, false);
        let capabilities = payload.restore_capabilities().unwrap();

        let mut listed = info.restorable_formats.clone();
        listed.sort();
        let mut offered = capabilities
            .formats
            .iter()
            .map(|format| format.to_string())
            .collect::<Vec<_>>();
        offered.sort();
        assert_eq!(
            listed, offered,
            "{label}: the listing offers {listed:?} but the server accepts {offered:?}"
        );

        let mut withheld = info
            .withheld_formats
            .iter()
            .map(|withheld| (withheld.format.clone(), withheld.code.clone()))
            .collect::<Vec<_>>();
        withheld.sort();
        let mut expected = capabilities
            .withheld
            .iter()
            .map(|(format, code)| (format.to_string(), code.to_string()))
            .collect::<Vec<_>>();
        expected.sort();
        assert_eq!(
            withheld, expected,
            "{label}: the listing withholds {withheld:?} but the server withholds {expected:?}"
        );
    }
    std::fs::remove_dir_all(root).ok();
}

// ---------------------------------------------------------------------------
// RR-21 / G-2 — a lone image resource may stand in for a standalone PNG
// ---------------------------------------------------------------------------

/// R5-03: "prefer a real standalone PNG; when it is the *only* image resource, that
/// one may be used". Without this, the multi-image guard would also refuse the
/// single-image case and the entry would become unrestorable.
#[tokio::test]
async fn rr21_a_lone_image_resource_may_serve_an_explicit_png_restore() {
    let platform = Arc::new(Recording::default());
    let mut engine = SyncEngine::new();
    engine.set_platform(platform.clone() as Arc<dyn SyncPlatform>);

    let mut bundle = two_image_html_bundle();
    bundle.resources.truncate(1);
    bundle.bindings.truncate(1);
    assert_eq!(image_count(&bundle), 1, "fixture must carry one image");

    let result = payload_for(&bundle, false).restore_composite(&mut engine, Some("png"));
    assert_eq!(
        result.unwrap(),
        RestoreOutcome::Published,
        "the only image resource must be usable for an explicit PNG restore"
    );
    let published = platform.published.lock().unwrap();
    assert_eq!(published.len(), 1, "exactly one publish");
    assert_eq!(
        image_count(&published[0]),
        1,
        "the one image is what got published"
    );
}

// ---------------------------------------------------------------------------
// RR-22 / G-3 — a whole-group HTML restore keeps its bound resources
// ---------------------------------------------------------------------------

/// R5-03: "a whole-group HTML restore keeps every bound resource; a single-format
/// HTML restore keeps the bindings it needs". Dropping the resources would publish
/// HTML whose `tailsync-resource:` references resolve to nothing.
#[tokio::test]
async fn rr22_html_restore_keeps_its_bound_resources() {
    let platform = Arc::new(Recording::default());
    let mut engine = SyncEngine::new();
    engine.set_platform(platform.clone() as Arc<dyn SyncPlatform>);

    let bundle = two_image_html_bundle();

    // Whole-group restore.
    let whole = payload_for(&bundle, false).restore_composite(&mut engine, None);
    assert_eq!(whole.unwrap(), RestoreOutcome::Published);
    // Single-format HTML restore.
    let single = payload_for(&bundle, false).restore_composite(&mut engine, Some("html"));
    assert_eq!(single.unwrap(), RestoreOutcome::Published);

    let published = platform.published.lock().unwrap();
    assert_eq!(published.len(), 2, "one publish per restore");
    for (index, published_bundle) in published.iter().enumerate() {
        assert_eq!(
            published_bundle.resources.len(),
            2,
            "publish {index} dropped the bound resources, leaving dangling \
             tailsync-resource: references"
        );
        assert_eq!(
            published_bundle.bindings.len(),
            2,
            "publish {index} dropped the bindings that make the resources reachable"
        );
    }
}

// ---------------------------------------------------------------------------
// RR-24 / G-1,G-6 / T29 — the menu must come from the server's real capability
// ---------------------------------------------------------------------------

/// R5-04: every offered action must be accepted, and every withheld format must be
/// refused. This is the property that lets both UIs build their menus from the server
/// instead of guessing — an option the user can click can never come back as a
/// refusal, and a refusal is never hidden.
#[tokio::test]
async fn rr24_offered_restore_actions_are_exactly_the_accepted_ones() {
    let cases: Vec<(&str, PortableBundle, bool)> = vec![
        (
            "two images, no standalone png",
            two_image_html_bundle(),
            false,
        ),
        (
            "native only",
            {
                let mut bundle = two_image_html_bundle();
                bundle.resources.truncate(1);
                bundle.bindings.truncate(1);
                bundle
            },
            true,
        ),
    ];

    for (label, bundle, native_only) in cases {
        let payload = payload_for(&bundle, native_only);
        let capabilities = payload
            .restore_capabilities()
            .unwrap_or_else(|error| panic!("{label}: capabilities must resolve: {error}"));

        // Every offered format is accepted.
        for format in &capabilities.formats {
            let platform = Arc::new(Recording::default());
            let mut engine = SyncEngine::new();
            engine.set_platform(platform.clone() as Arc<dyn SyncPlatform>);
            let outcome = payload_for(&bundle, native_only)
                .restore_composite(&mut engine, Some(format))
                .unwrap_or_else(|error| panic!("{label}/{format}: must not error: {error}"));
            assert_ne!(
                outcome,
                RestoreOutcome::Unsupported {
                    code: capabilities
                        .withheld
                        .iter()
                        .find(|(withheld, _)| withheld == format)
                        .map(|(_, code)| *code)
                        .unwrap_or("format_unavailable")
                },
                "{label}: the menu offers {format} but the server refused it"
            );
            assert!(
                outcome.is_published(),
                "{label}: the menu offers {format} but the server answered {outcome:?}"
            );
        }

        // Every withheld format is refused, with the code the menu would show.
        for (format, code) in &capabilities.withheld {
            let platform = Arc::new(Recording::default());
            let mut engine = SyncEngine::new();
            engine.set_platform(platform.clone() as Arc<dyn SyncPlatform>);
            let outcome = payload_for(&bundle, native_only)
                .restore_composite(&mut engine, Some(format))
                .unwrap_or_else(|error| panic!("{label}/{format}: must not error: {error}"));
            assert_eq!(
                outcome,
                RestoreOutcome::Unsupported { code },
                "{label}: {format} is withheld as {code} but the server answered {outcome:?}"
            );
            assert_eq!(
                platform.published.lock().unwrap().len(),
                0,
                "{label}: a withheld action must not reach the clipboard"
            );
        }

        // A native-only entry offers no explicit format at all.
        if native_only {
            assert!(
                capabilities.formats.is_empty(),
                "{label}: a native-only entry must not offer explicit formats"
            );
        }
    }
}

/// The descriptor must not hide a format the server would accept, either: a portable
/// entry offers every format it carries.
#[tokio::test]
async fn rr24b_portable_entry_offers_every_format_it_carries() {
    let bundle = two_image_html_bundle();
    let capabilities = payload_for(&bundle, false).restore_capabilities().unwrap();
    assert!(capabilities.whole_group);
    assert!(capabilities.offers("html"));
    assert!(
        capabilities.offers("text")
            || capabilities.offers("rtf")
            || !capabilities.formats.is_empty()
    );
    // Two images and no standalone PNG, so the PNG action is withheld with its reason.
    assert!(!capabilities.offers("png"));
    assert_eq!(
        capabilities
            .withheld
            .iter()
            .find(|(format, _)| *format == "png")
            .map(|(_, code)| *code),
        Some("no_single_image"),
        "the withheld PNG action must carry the code the UI localises"
    );
}

/// Counterpart guard: a normal (non-native-only) entry with an explicit format
/// that genuinely exists must still work, so the R5-03 guard cannot be satisfied
/// by refusing everything.
#[tokio::test]
async fn rr09b_available_explicit_format_still_restores() {
    let platform = Arc::new(Recording::default());
    let mut engine = SyncEngine::new();
    engine.set_platform(platform.clone() as Arc<dyn SyncPlatform>);

    let bundle = two_image_html_bundle();
    let result = payload_for(&bundle, false).restore_composite(&mut engine, Some("html"));

    assert_eq!(
        result.unwrap(),
        RestoreOutcome::Published,
        "a portable entry that does carry the requested format must still restore"
    );
    assert_eq!(
        platform.published.lock().unwrap().len(),
        1,
        "exactly one publish per restore"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn async_restore_releases_engine_during_native_work_and_supersedes_old_arrivals() {
    let platform = Arc::new(Recording::default());
    platform
        .delay
        .store(true, std::sync::atomic::Ordering::Release);
    let mut state = SyncEngine::new();
    state.set_platform(platform.clone());
    let engine = Arc::new(tokio::sync::Mutex::new(state));
    let before = engine.lock().await.clipboard_generation();
    let restore = tokio::spawn(
        payload_for(&two_image_html_bundle(), false)
            .restore_composite_async(engine.clone(), Some("html".into())),
    );
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        platform.entered.notified(),
    )
    .await
    .unwrap();
    let guard = tokio::time::timeout(std::time::Duration::from_millis(50), engine.lock())
        .await
        .expect("native I/O must not hold engine");
    assert!(guard.clipboard_generation() > before);
    drop(guard);
    assert_eq!(restore.await.unwrap().unwrap(), RestoreOutcome::Published);
}
