use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tailsync_core::{
    composite::{Completeness, Mime, NativePublish, PortableBundle, Representation},
    protocol::{
        native_clipboard::{PublishReceipt, PublishState},
        TransferId,
    },
    sync::{FileBatchProgress, FileReceiveCommit, PlatformResultFuture, SyncEngine, SyncPlatform},
};
use tailsync_runtime::history::{HistoryRestorePayload, RestoreOutcome};

#[derive(Default)]
struct Recording {
    writes: AtomicUsize,
    preparations: AtomicUsize,
    preparation_fails: bool,
    target: Option<tailsync_core::composite::restore::RestoreTarget>,
    native: std::sync::Mutex<Vec<tailsync_core::protocol::native_clipboard::NativeSnapshot>>,
    outcome: std::sync::Mutex<Option<NativePublish>>,
    revision: std::sync::atomic::AtomicI64,
    track_revision: bool,
}
impl SyncPlatform for Recording {
    fn prepare_restore_plan(
        &self,
        plan: tailsync_core::composite::restore::RestorePlan,
        cancelled: Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<tailsync_core::composite::restore::RestorePlan, String> {
        self.preparations.fetch_add(1, Ordering::SeqCst);
        if self.preparation_fails || cancelled.load(Ordering::Acquire) {
            return Err("synthetic native preparation failure".into());
        }
        Ok(plan)
    }
    fn restore_target(&self) -> tailsync_core::composite::restore::RestoreTarget {
        self.target
            .unwrap_or(tailsync_core::composite::restore::RestoreTarget::Adapter)
    }
    fn clipboard_revision(&self) -> Option<i64> {
        self.track_revision
            .then(|| self.revision.load(Ordering::SeqCst))
    }
    fn write_native_snapshot(
        &self,
        s: &tailsync_core::protocol::native_clipboard::NativeSnapshot,
    ) -> Result<NativePublish, String> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        self.native.lock().unwrap().push(s.clone());
        Ok(self
            .outcome
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(NativePublish::Published(PublishReceipt {
                state: PublishState::Published,
                revision: 2,
                token: [1; 16],
            })))
    }
    fn write_text(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn write_image(&self, _: u32, _: u32, _: &[u8]) -> Result<(), String> {
        Ok(())
    }
    fn write_composite(&self, _: &PortableBundle, _: Option<i64>) -> Result<NativePublish, String> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        Ok(NativePublish::Published(PublishReceipt {
            state: PublishState::Published,
            revision: 2,
            token: [1; 16],
        }))
    }
    fn set_file_progress(&self, _: &str, _: u64, _: u64) {}
    fn clear_file_progress(&self, _: Option<TransferId>, _: Option<&str>) {}
    fn set_file_batch_progress(&self, _: FileBatchProgress) {}
    fn file_batch_failed(&self, _: Option<TransferId>, _: &str) {}
    fn files_received(&self, _: FileReceiveCommit) -> PlatformResultFuture {
        Box::pin(async { Ok(()) })
    }
}
fn payload(completeness: Completeness, html: &str) -> HistoryRestorePayload {
    let b = PortableBundle {
        representations: vec![
            Representation {
                mime: Mime::Text,
                bytes: b"visible text".to_vec(),
            },
            Representation {
                mime: Mime::Html,
                bytes: html.as_bytes().to_vec(),
            },
        ],
        resources: vec![],
        bindings: vec![],
        completeness,
    };
    HistoryRestorePayload {
        entry_type: "composite".into(),
        file_path: None,
        file_name: None,
        data: Some(b.encode().unwrap()),
        identity: None,
        native_only: false,
        native_archive_complete: None,
        native_archive_platform: None,
        native: None,
    }
}
#[test]
fn o06_missing_or_external_resources_never_report_whole_group_success() {
    let external = "<p>text</p><img src='https://fixture.invalid/deleted.png'>";
    for (completeness, html) in [
        (Completeness::ExternalReferences, external),
        (Completeness::MissingFormats, external),
        (Completeness::Complete, external),
        (
            Completeness::Complete,
            "<img src='tailsync-resource:1'><img>",
        ),
    ] {
        let recording = Arc::new(Recording::default());
        let mut engine = SyncEngine::new();
        engine.set_platform(recording.clone());
        let mut p = payload(completeness, html);
        if let Some(offset) = html.find("tailsync-resource:1") {
            use tailsync_core::composite::{Resource, ResourceBinding};
            let mut bundle = PortableBundle::decode(p.data.as_deref().unwrap()).unwrap();
            bundle.resources.push(Resource {
                id: 1,
                png: tailsync_core::composite::packed_to_png(1, 1, &[255, 0, 0, 128]).unwrap(),
            });
            bundle.bindings.push(ResourceBinding {
                representation: 1,
                resource: 1,
                offset: offset as u32,
                length: "tailsync-resource:1".len() as u32,
            });
            p.data = Some(bundle.encode().unwrap());
        }
        let outcome = p.restore_composite(&mut engine, None).unwrap();
        assert_eq!(
            outcome,
            RestoreOutcome::PublishedAvailable { missing_images: 1 }
        );
        assert!(!outcome.report().unwrap().complete);
        assert_eq!(recording.writes.load(Ordering::SeqCst), 1);
        let explicit = p.restore_composite(&mut engine, Some("html")).unwrap();
        assert!(!explicit.is_published());
        assert_eq!(recording.writes.load(Ordering::SeqCst), 1);
        let capabilities = p.restore_capabilities().unwrap();
        assert!(!capabilities.whole_group);
        assert!(!capabilities.offers("html"));
        assert!(capabilities.offers("text"));
        assert!(p
            .restore_composite(&mut engine, Some("text"))
            .unwrap()
            .is_published());
        assert_eq!(recording.writes.load(Ordering::SeqCst), 2);
    }
}

#[test]
fn rich_text_missing_image_keeps_saved_format_without_mutating_history() {
    let html = "<p style='font-size:18px'><strong>saved body</strong></p><img src='https://example.com/missing.png'>";
    let payload = payload(Completeness::ExternalReferences, html);
    let original = payload.data.clone();
    let prepared = payload
        .prepare_restore_plan(
            true,
            tailsync_core::composite::restore::RestoreTarget::Macos,
            None,
        )
        .unwrap();
    let tailsync_runtime::history::PreparedHistoryRestore::Ready(plan) = prepared else {
        panic!("saved text and formatting should remain restorable with a missing image");
    };
    let snapshot = plan.native().unwrap();
    assert_eq!(plan.missing_images(), 1);
    assert_eq!(
        plan.selection(),
        tailsync_core::composite::restore::RestoreSelection::AvailableContent
    );
    let restored = snapshot
        .formats
        .iter()
        .find(|format| format.name == "public.html")
        .unwrap();
    let restored = std::str::from_utf8(&restored.bytes).unwrap();
    assert!(restored.contains("<strong>saved body</strong>"));
    assert!(restored.contains("font-size:18px"));
    assert!(!restored.contains("https://example.com/missing.png"));
    assert_eq!(payload.data, original);
}

#[tokio::test]
async fn rich_text_available_content_reports_only_a_complete_selected_publication() {
    use tailsync_core::composite::restore::RestoreTarget;
    let recording = Arc::new(Recording {
        target: Some(RestoreTarget::Macos),
        ..Default::default()
    });
    let mut engine = SyncEngine::new();
    engine.set_platform(recording.clone());
    let engine = Arc::new(tokio::sync::Mutex::new(engine));
    let html = "<p><strong>saved body</strong></p><img src='missing'>";
    let outcome = payload(Completeness::ExternalReferences, html)
        .restore_composite_async(engine.clone(), None)
        .await
        .unwrap();
    assert_eq!(
        outcome,
        RestoreOutcome::PublishedAvailable { missing_images: 1 }
    );
    assert_eq!(outcome.report().unwrap().missing_images, 1);
    assert_eq!(recording.writes.load(Ordering::SeqCst), 1);
    *recording.outcome.lock().unwrap() = Some(NativePublish::Partial(PublishReceipt {
        state: PublishState::Partial,
        revision: 3,
        token: [2; 16],
    }));
    let outcome = payload(Completeness::ExternalReferences, html)
        .restore_composite_async(engine, None)
        .await
        .unwrap();
    assert!(matches!(outcome, RestoreOutcome::Partial { .. }));
    assert!(outcome.report().is_none());
    assert_eq!(recording.writes.load(Ordering::SeqCst), 2);
}

#[test]
fn o06_native_only_menu_does_not_offer_a_default_restore_with_known_missing_resources() {
    use tailsync_core::protocol::native_clipboard::*;
    let mut p = payload(Completeness::Complete, "<p>summary only</p>");
    p.native_only = true;
    p.native_archive_complete = Some(true);
    p.native_archive_platform = Some(1);
    p.native = Some(NativeSnapshot {
        before: 0,
        after: 0,
        items: 1,
        state: SnapshotState::Stable,
        formats: vec![NativeFormat {
            item: 0,
            name: "public.html".into(),
            state: FormatState::Captured,
            bytes: b"<img src='https://fixture.invalid/missing.png'>".to_vec(),
        }],
    });
    let caps = p.restore_capabilities().unwrap();
    assert!(!caps.whole_group);
    assert_eq!(caps.whole_group_code, Some("resources_incomplete"));
    assert!(caps.formats.is_empty());
}

#[tokio::test]
async fn rich_text_windows_source_context_restores_available_content_through_native_publication() {
    use tailsync_core::composite::restore::RestoreTarget;
    let recording = Arc::new(Recording {
        target: Some(RestoreTarget::Windows),
        ..Default::default()
    });
    let mut engine = SyncEngine::new();
    engine.set_platform(recording.clone());
    let engine = Arc::new(tokio::sync::Mutex::new(engine));
    let html = "<base href='https://example.com/posts/'><p><a href='../other'><strong>saved body</strong></a></p><img src='missing'>";
    let saved = payload(Completeness::ExternalReferences, html);
    let before = saved.data.clone();
    saved
        .prepare_restore_plan(true, RestoreTarget::Windows, None)
        .unwrap();
    assert_eq!(saved.data, before);
    let outcome = saved.restore_composite_async(engine, None).await.unwrap();
    assert_eq!(
        outcome,
        RestoreOutcome::PublishedAvailable { missing_images: 1 }
    );
    assert!(!outcome.report().unwrap().complete);
    assert_eq!(recording.writes.load(Ordering::SeqCst), 1);
    let snapshots = recording.native.lock().unwrap();
    let native = &snapshots[0];
    let format = native
        .formats
        .iter()
        .find(|format| format.name == "HTML Format")
        .unwrap();
    let restored = tailsync_core::composite::html::decode_cf_html(&format.bytes).unwrap();
    assert!(!restored.contains("<base"));
    assert!(restored.contains("https://example.com/other"));
    assert!(restored.contains("<strong>saved body</strong>"));
}

#[tokio::test]
async fn readable_restore_publishes_once_despite_unused_page_dependencies() {
    use tailsync_core::composite::restore::RestoreTarget;
    for target in [RestoreTarget::Macos, RestoreTarget::Windows] {
        let recording = Arc::new(Recording {
            target: Some(target),
            ..Default::default()
        });
        let mut engine = SyncEngine::new();
        engine.set_platform(recording.clone());
        let engine = Arc::new(tokio::sync::Mutex::new(engine));
        let html = "<style>@import 'https://fixture.invalid/page.css';</style><p><b>saved body</b></p><img src='https://fixture.invalid/missing'>";
        let saved = payload(Completeness::ExternalReferences, html);
        let before = saved.data.clone();
        saved.prepare_restore_plan(true, target, None).unwrap();
        assert_eq!(saved.data, before);
        let outcome = saved.restore_composite_async(engine, None).await.unwrap();
        assert_eq!(
            outcome,
            RestoreOutcome::PublishedAvailable { missing_images: 1 }
        );
        assert_eq!(recording.writes.load(Ordering::SeqCst), 1);
        let native = recording.native.lock().unwrap();
        assert_eq!(native.len(), 1);
        assert!(native[0].formats.iter().any(|format| {
            matches!(format.name.as_str(), "public.rtf" | "Rich Text Format")
                && std::str::from_utf8(&format.bytes).unwrap().contains("\\b ")
        }));
        assert_eq!(recording.preparations.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn native_attachment_preparation_failure_never_writes_the_clipboard() {
    let recording = Arc::new(Recording {
        target: Some(tailsync_core::composite::restore::RestoreTarget::Macos),
        preparation_fails: true,
        ..Default::default()
    });
    let mut engine = SyncEngine::new();
    engine.set_platform(recording.clone());
    let saved = payload(
        Completeness::ExternalReferences,
        "<p><b>saved</b></p><img src='https://fixture.invalid/missing'>",
    );
    let result = saved
        .restore_composite_async(Arc::new(tokio::sync::Mutex::new(engine)), None)
        .await;
    assert!(result.is_err());
    assert_eq!(recording.preparations.load(Ordering::SeqCst), 1);
    assert_eq!(recording.writes.load(Ordering::SeqCst), 0);
}

#[test]
fn rich_text_complete_source_context_keeps_original_html_and_formatting() {
    use tailsync_core::{
        composite::restore::{RestoreSelection, RestoreTarget},
        protocol::native_clipboard::*,
    };
    let html = "<base href='https://example.com/posts/'><p style='font-family:serif;font-size:18px'><a href='../other'><strong>saved body</strong></a></p>";
    let mut saved = payload(Completeness::Complete, html);
    saved.native_archive_complete = Some(true);
    saved.native_archive_platform = Some(1);
    saved.native = Some(NativeSnapshot {
        before: 1,
        after: 1,
        items: 1,
        state: SnapshotState::Stable,
        formats: vec![NativeFormat {
            item: 0,
            name: "public.html".into(),
            state: FormatState::Captured,
            bytes: html.as_bytes().to_vec(),
        }],
    });
    let tailsync_runtime::history::PreparedHistoryRestore::Ready(plan) = saved
        .prepare_restore_plan(true, RestoreTarget::Macos, None)
        .unwrap()
    else {
        panic!("source context is metadata, not a missing loading dependency");
    };
    assert_eq!(plan.selection(), RestoreSelection::Originals);
    assert_eq!(plan.missing_images(), 0);
    assert_eq!(plan.native().unwrap().formats[0].bytes, html.as_bytes());
}
#[tokio::test]
async fn o06_newer_restore_cancels_an_older_plan_waiting_for_publication() {
    let blocker = tailsync_core::sync::publication_permit().await;
    let recording = Arc::new(Recording::default());
    let mut state = SyncEngine::new();
    state.set_platform(recording.clone());
    let engine = Arc::new(tokio::sync::Mutex::new(state));
    let first = tokio::spawn(
        payload(Completeness::Complete, "<p>first</p>")
            .restore_composite_async(engine.clone(), None),
    );
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while engine.lock().await.clipboard_generation() < 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let second = tokio::spawn(
        payload(Completeness::Complete, "<p>second</p>")
            .restore_composite_async(engine.clone(), None),
    );
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while engine.lock().await.clipboard_generation() < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    drop(blocker);
    assert_eq!(first.await.unwrap().unwrap(), RestoreOutcome::Superseded);
    assert_eq!(second.await.unwrap().unwrap(), RestoreOutcome::Published);
    assert_eq!(recording.writes.load(Ordering::SeqCst), 1);
}

#[test]
fn o06_foreign_archive_uses_complete_compatibility_without_publishing_private_types() {
    use tailsync_core::{
        composite::restore::{RestoreSelection, RestoreTarget},
        protocol::native_clipboard::*,
    };
    let mut p = payload(Completeness::Complete, "<p>text</p>");
    p.native_archive_complete = Some(true);
    p.native_archive_platform = Some(1);
    p.native = Some(NativeSnapshot {
        before: 1,
        after: 1,
        items: 1,
        state: SnapshotState::Stable,
        formats: vec![NativeFormat {
            item: 0,
            name: "org.tailsync.fixture.vendor".into(),
            bytes: vec![255, 0],
            state: FormatState::Captured,
        }],
    });
    let recording = Arc::new(Recording {
        target: Some(RestoreTarget::Windows),
        ..Default::default()
    });
    let mut engine = SyncEngine::new();
    engine.set_platform(recording.clone());
    let tailsync_runtime::history::PreparedHistoryRestore::Ready(plan) = p
        .prepare_restore_plan(true, RestoreTarget::Windows, None)
        .unwrap()
    else {
        panic!("complete foreign compatibility must prepare")
    };
    assert_eq!(plan.selection(), RestoreSelection::PortableCompatibility);
    assert_eq!(plan.input().source_platform, Some(1));
    assert!(plan
        .outputs()
        .iter()
        .all(|o| !o.name.starts_with("org.tailsync")));
    assert_eq!(
        p.restore_composite(&mut engine, None).unwrap(),
        RestoreOutcome::Published
    );
    assert_eq!(recording.writes.load(Ordering::SeqCst), 1);
    assert_eq!(
        recording.native.lock().unwrap()[0].formats[0].name,
        "CF_UNICODETEXT"
    );
    p.native_only = true;
    assert_eq!(
        p.restore_composite(&mut engine, None).unwrap(),
        RestoreOutcome::Unsupported {
            code: "native_only"
        }
    );
    assert_eq!(recording.writes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn o06_changed_os_revision_declines_before_any_write() {
    let blocker = tailsync_core::sync::publication_permit().await;
    let recording = Arc::new(Recording {
        track_revision: true,
        revision: 1.into(),
        ..Default::default()
    });
    let mut state = SyncEngine::new();
    state.set_platform(recording.clone());
    let engine = Arc::new(tokio::sync::Mutex::new(state));
    let task = tokio::spawn(
        payload(Completeness::Complete, "<p>held</p>")
            .restore_composite_async(engine.clone(), None),
    );
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while engine.lock().await.clipboard_generation() < 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    recording.revision.store(2, Ordering::SeqCst);
    drop(blocker);
    assert_eq!(task.await.unwrap().unwrap(), RestoreOutcome::Superseded);
    assert_eq!(recording.writes.load(Ordering::SeqCst), 0);
}

#[test]
fn o06_archived_bindings_materialize_as_separate_output_without_mutating_originals() {
    use tailsync_core::{
        composite::{
            packed_to_png,
            restore::{RestoreSelection, RestoreTarget},
            Resource, ResourceBinding,
        },
        protocol::native_clipboard::*,
    };
    let html = b"<p>offline</p><img src='tailsync-resource:1'>";
    let offset = std::str::from_utf8(html)
        .unwrap()
        .find("tailsync-resource:1")
        .unwrap();
    let b = PortableBundle {
        representations: vec![
            Representation {
                mime: Mime::Text,
                bytes: b"offline".to_vec(),
            },
            Representation {
                mime: Mime::Html,
                bytes: html.to_vec(),
            },
        ],
        resources: vec![Resource {
            id: 1,
            png: packed_to_png(1, 1, &[3, 4, 5, 255]).unwrap(),
        }],
        bindings: vec![ResourceBinding {
            representation: 1,
            resource: 1,
            offset: offset as u32,
            length: 19,
        }],
        completeness: Completeness::Complete,
    };
    let mut p = payload(Completeness::Complete, "<p>unused</p>");
    p.data = Some(b.encode().unwrap());
    p.native_archive_complete = Some(true);
    p.native_archive_platform = Some(1);
    p.native = Some(NativeSnapshot {
        before: 1,
        after: 1,
        items: 1,
        state: SnapshotState::Stable,
        formats: vec![NativeFormat {
            item: 0,
            name: "public.html".into(),
            bytes: b"<p>offline</p><img src='https://fixture.invalid/deleted.png'>".to_vec(),
            state: FormatState::Captured,
        }],
    });
    let original = p.native.clone();
    let data = p.data.clone();
    let tailsync_runtime::history::PreparedHistoryRestore::Ready(plan) = p
        .prepare_restore_plan(true, RestoreTarget::Macos, None)
        .unwrap()
    else {
        panic!("offline resources must prepare")
    };
    assert_eq!(plan.selection(), RestoreSelection::PortableCompatibility);
    let output = std::str::from_utf8(&plan.native().unwrap().formats[1].bytes).unwrap();
    assert!(output.contains("data:image/png;base64,"));
    assert!(!output.contains("https://"));
    assert!(!output.contains("tailsync-resource:"));
    let tailsync_runtime::history::PreparedHistoryRestore::Ready(text) = p
        .prepare_restore_plan(true, RestoreTarget::Macos, Some("text"))
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(text.selection(), RestoreSelection::ExplicitFormat);
    assert_eq!(text.outputs().len(), 1);
    assert_eq!(p.native, original);
    assert_eq!(p.data, data);
}

#[test]
fn o06_partial_unknown_and_no_effect_never_become_completed_restore_or_retry() {
    use tailsync_core::composite::restore::RestoreTarget;
    for outcome in [
        NativePublish::Partial(PublishReceipt {
            state: PublishState::Partial,
            revision: 2,
            token: [1; 16],
        }),
        NativePublish::Unknown {
            detail: "missing receipt".into(),
        },
        NativePublish::NoEffect {
            detail: "not started".into(),
        },
    ] {
        let recording = Arc::new(Recording {
            target: Some(RestoreTarget::Macos),
            outcome: std::sync::Mutex::new(Some(outcome)),
            ..Default::default()
        });
        let mut engine = SyncEngine::new();
        engine.set_platform(recording.clone());
        let result = payload(Completeness::Complete, "<p>ready</p>")
            .restore_composite(&mut engine, None)
            .unwrap();
        assert!(!result.is_published());
        assert_eq!(recording.writes.load(Ordering::SeqCst), 1);
        let e = result.error_envelope();
        assert_ne!(
            e.code,
            tailsync_runtime::contracts::StableErrorCode::InternalError
        );
    }
}
