use std::sync::{Arc, Mutex, OnceLock};
use tailsync_core::composite::content_analysis::NativeAnalysisFacts;
use tailsync_core::composite::{
    self,
    origin::{Origin, OriginTracker},
    PortableBundle,
};
use tailsync_core::protocol::content_analysis::{
    SavedAnalysisKind, SavedTextAnalysis, SAVED_FRAME_LIMIT,
};
use tailsync_core::protocol::native_clipboard::{NativeSnapshot, PublishReceipt, PublishState};
use tailsync_runtime::content_analysis::{NativeAnalysisTask, NativeAnalyzer};
static ORIGIN: OnceLock<Mutex<OriginTracker>> = OnceLock::new();
static PUBLISH: Mutex<()> = Mutex::new(());
fn tracker() -> &'static Mutex<OriginTracker> {
    ORIGIN.get_or_init(|| {
        let path = tailsync_core::db::get_data_dir().join("clipboard-origin-v1.json");
        let restored = (|| -> Result<OriginTracker, String> {
            use std::io::Read;
            let mut file = match std::fs::File::open(&path) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(OriginTracker::default())
                }
                Err(_) => return Err("Origin metadata unavailable".into()),
            };
            let mut bytes = Vec::new();
            file.by_ref()
                .take(65537)
                .read_to_end(&mut bytes)
                .map_err(|_| "Origin metadata read failed")?;
            if bytes.len() > 65536 {
                return Err("Origin metadata exceeds limit".into());
            }
            OriginTracker::from_durable(
                serde_json::from_slice(&bytes).map_err(|_| "Origin metadata damaged")?,
            )
        })();
        Mutex::new(restored.unwrap_or_else(|error| {
            tracing::warn!("{error}; automatic clipboard publication disabled");
            tailsync_core::secure::set_origin_health(false);
            tailsync_core::sync_warning::record_clipboard_unavailable();
            // Keep a permanent unresolved operation rather than starting empty.
            let mut tracker = OriginTracker::default();
            tracker.quarantine();
            tracker
        }))
    })
}
fn persist_origin(tracker: &OriginTracker) -> Result<(), String> {
    if tracker.is_quarantined() {
        return Err("Origin metadata quarantined".into());
    }
    let root = tailsync_core::db::get_data_dir();
    tailsync_core::private_fs::create_private_dir_all(&root)
        .map_err(|_| "Origin metadata directory unavailable")?;
    let bytes = serde_json::to_vec(&tracker.durable_state())
        .map_err(|_| "Origin metadata encoding failed")?;
    tailsync_core::private_fs::write_private_file(&root.join("clipboard-origin-v1.json"), &bytes)
        .map_err(|_| "Origin metadata persistence failed".into())
}
pub fn origin_health() -> bool {
    let tracker = tracker()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let healthy = !tracker.is_quarantined() && !tracker.has_unresolved_intent();
    if !healthy {
        tailsync_core::sync_warning::record_clipboard_unavailable();
    }
    healthy
}
pub fn capture_enabled() -> bool {
    tailsync_core::secure::composite_capture_serviceable()
        && std::env::var_os("TAILSYNC_DISABLE_COMPOSITE_CLIPBOARD").is_none()
}
pub fn revision() -> Option<i64> {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::NSPasteboard;
        Some(NSPasteboard::generalPasteboard().changeCount() as i64)
    }
    #[cfg(target_os = "windows")]
    {
        let sequence =
            unsafe { windows_sys::Win32::System::DataExchange::GetClipboardSequenceNumber() };
        (sequence != 0).then_some(sequence as i64)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        None
    }
}
/// Classify a *publish-side* bundle against the local operation log.
/// Both sides go through `origin_fingerprint`, so the publish projection and the
/// read-back projection are the same model (ADR-004 K1/R2-03). A conversion or
/// fingerprint failure yields `UnknownMismatch` rather than a fabricated match.
pub fn classify(snapshot: &NativeSnapshot, bundle: &PortableBundle) -> Origin {
    if tailsync_core::composite::fingerprint::requires_original_domain(snapshot) {
        return classify_native(snapshot);
    }
    classify_digest(
        snapshot,
        composite::fingerprint::origin_fingerprint(bundle).ok(),
    )
}
pub fn classify_native(snapshot: &NativeSnapshot) -> Origin {
    classify_digest(
        snapshot,
        composite::fingerprint::origin_fingerprint_from_capture(snapshot).ok(),
    )
}
fn process_has_exited(pid: u32) -> bool {
    #[cfg(unix)]
    {
        unsafe {
            libc::kill(pid as i32, 0) != 0
                && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
        }
    }
    #[cfg(target_os = "windows")]
    {
        unsafe {
            use windows_sys::Win32::{Foundation::*, System::Threading::*};
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if process.is_null() {
                return GetLastError() == ERROR_INVALID_PARAMETER;
            }
            let mut code = 259;
            let queried = GetExitCodeProcess(process, &mut code);
            CloseHandle(process);
            queried != 0 && code != 259
        }
    }
    #[cfg(not(any(unix, target_os = "windows")))]
    {
        let _ = pid;
        false
    }
}

fn classify_digest(snapshot: &NativeSnapshot, digest: Option<[u8; 32]>) -> Origin {
    let token = snapshot
        .formats
        .iter()
        .find(|f| f.name == "org.tailsync.clipboard-source")
        .and_then(|f| f.bytes.as_slice().try_into().ok());
    let mut tracker = tracker()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if snapshot.before == snapshot.after {
        for (token, pid, guarded) in tracker.recovery_processes() {
            if (pid.is_none() && guarded) || pid.is_some_and(process_has_exited) {
                tracker.resolve_intent(token, snapshot.after);
            }
        }
        tailsync_core::secure::set_origin_health(
            !tracker.has_unresolved_intent() && !tracker.is_quarantined(),
        );
    }
    let Some(digest) = digest else {
        if persist_origin(&tracker).is_err() {
            tailsync_core::secure::set_origin_health(false);
            tailsync_core::sync_warning::record_clipboard_unavailable();
        }
        return Origin::UnknownMismatch;
    };
    let origin = tracker.classify(
        (snapshot.before == snapshot.after).then_some(snapshot.after),
        digest,
        token,
    );
    if persist_origin(&tracker).is_err() {
        tailsync_core::sync_warning::record_clipboard_unavailable();
    }
    origin
}
pub async fn capture_fault(revision: i64, kind: &'static str) -> bool {
    match kind {
        "clipboard_unavailable" => tailsync_core::sync_warning::record_clipboard_unavailable(),
        "capture_not_saved" => tailsync_core::sync_warning::record_capture_not_saved(),
        _ => tailsync_core::sync_warning::record_capture_failed(),
    }
    matches!(
        tokio::task::spawn_blocking(move || {
            tailsync_core::peer::effect_journal::record_capture_fault(
                &tailsync_core::db::get_data_dir(),
                revision,
                kind,
            )
        })
        .await,
        Ok(Ok(()))
    )
}

#[cfg(target_os = "macos")]
pub fn read_archive() -> Result<tailsync_core::protocol::native_archive::NativeArchive, String> {
    let bytes = run_helper("--read-archive", None)?;
    let archive = tailsync_core::protocol::native_archive::NativeArchive::decode(&bytes)
        .map_err(|e| e.to_string())?;
    if archive.platform != 1 {
        return Err("Clipboard archive source platform mismatch".into());
    }
    Ok(archive)
}
pub fn read() -> Result<NativeSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        read_archive().map(|a| a.original)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let bytes = run_helper("--read-snapshot", None)?;
        NativeSnapshot::decode(&bytes).map_err(|e| e.to_string())
    }
}
pub fn write(bundle: &PortableBundle, expected: Option<i64>) -> Result<NativePublish, String> {
    write_cancellable(bundle, expected, None)
}
pub fn write_cancellable(
    bundle: &PortableBundle,
    expected: Option<i64>,
    cancelled: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
) -> Result<NativePublish, String> {
    #[cfg(target_os = "macos")]
    let snapshot = composite::to_macos_snapshot(bundle).map_err(|e| e.to_string())?;
    #[cfg(target_os = "windows")]
    let snapshot = composite::to_windows_snapshot(bundle).map_err(|e| e.to_string())?;
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let snapshot = composite::to_macos_snapshot(bundle).map_err(|e| e.to_string())?;
    write_native_cancellable(&snapshot, expected, cancelled)
}
pub fn supports_native(snapshot: &NativeSnapshot) -> bool {
    snapshot.validate().is_ok()
        && snapshot.state == tailsync_core::protocol::native_clipboard::SnapshotState::Stable
        && (0..snapshot.items).all(|item| {
            snapshot.formats.iter().any(|f| {
                f.item == item
                    && f.state == tailsync_core::protocol::native_clipboard::FormatState::Captured
                    && !tailsync_core::protocol::native_policy::control(&f.name)
            })
        })
        && snapshot.formats.iter().all(|f| {
            if tailsync_core::protocol::native_policy::control(&f.name) {
                return true;
            }
            if f.state != tailsync_core::protocol::native_clipboard::FormatState::Captured {
                return false;
            }
            if cfg!(target_os = "macos") {
                tailsync_core::protocol::native_policy::macos_publishable(&f.name, f.bytes.len())
            } else {
                tailsync_core::protocol::native_policy::windows_publishable(&f.name)
            }
        })
        && (cfg!(target_os = "macos") || snapshot.items == 1)
}

#[cfg(all(test, target_os = "macos"))]
#[test]
fn o02_opaque_native_publish_admission_does_not_admit_file_dependencies() {
    use tailsync_core::protocol::native_clipboard::*;
    let mut s = NativeSnapshot {
        before: 0,
        after: 0,
        items: 1,
        state: SnapshotState::Stable,
        formats: vec![NativeFormat {
            item: 0,
            name: "org.tailsync.fixture.opaque".into(),
            state: FormatState::Captured,
            bytes: vec![0, 255],
        }],
    };
    assert!(supports_native(&s));
    s.formats[0].name = "public.file-url".into();
    assert!(!supports_native(&s));
    s.formats[0].name = "org.nspasteboard.ConcealedType".into();
    assert!(!supports_native(&s));
}
pub fn write_native(
    snapshot: &NativeSnapshot,
    expected: Option<i64>,
) -> Result<NativePublish, String> {
    write_native_cancellable(snapshot, expected, None)
}
#[cfg(target_os = "macos")]
pub fn prepare_readable_restore(
    plan: composite::restore::RestorePlan,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<composite::restore::RestorePlan, String> {
    if cancelled.load(std::sync::atomic::Ordering::Acquire) {
        return Err("Restore preparation cancelled".into());
    }
    if plan.target() != composite::restore::RestoreTarget::Macos
        || !matches!(
            plan.selection(),
            composite::restore::RestoreSelection::AvailableContent
                | composite::restore::RestoreSelection::DocumentCompatibility
        )
    {
        return Ok(plan);
    }
    let Some(html) = plan.native().and_then(|snapshot| {
        snapshot
            .formats
            .iter()
            .find(|format| format.name == "public.html")
    }) else {
        // A saved PNG is already publishable; only HTML needs rich text attachment conversion.
        return Ok(plan);
    };
    if !composite::html::restore_reference_inventory_complete(&html.bytes, &[])
        .map_err(|error| error.to_string())?
    {
        return Err("Readable restore still depends on external resources".into());
    }
    let image_count = composite::html::available_image_inventory(&html.bytes)
        .map_err(|error| error.to_string())?
        .len();
    let mut input = u16::try_from(image_count)
        .map_err(|_| "Readable restore image inventory exceeded".to_string())?
        .to_be_bytes()
        .to_vec();
    input.extend_from_slice(&html.bytes);
    let outcome = run_helper_outcome_cancellable_limited(
        "--rtfd-from-html",
        Some(input),
        HELPER_PUBLISH_DEADLINE,
        32 * 1024 * 1024,
        Some(cancelled),
    )?;
    match outcome {
        composite::supervisor::HelperOutcome::Exited {
            success: true,
            stdout,
            ..
        } => plan
            .with_macos_rtfd(stdout)
            .map_err(|error| error.to_string()),
        composite::supervisor::HelperOutcome::Exited { success: false, .. } => {
            Err("Clipboard helper could not prepare rich text attachments".into())
        }
        composite::supervisor::HelperOutcome::TimedOut { .. } => {
            Err("Rich text attachment preparation timed out".into())
        }
        composite::supervisor::HelperOutcome::OutputExceeded { .. } => {
            Err("Rich text attachment preparation exceeded the limit".into())
        }
        composite::supervisor::HelperOutcome::SpawnFailed { detail, .. } => Err(detail),
    }
}
#[cfg(all(test, target_os = "macos"))]
mod restore_preparation_tests {
    use super::*;
    use composite::{restore::RestoreTarget, Completeness, Mime, Representation};
    use tailsync_runtime::history::{HistoryRestorePayload, PreparedHistoryRestore};

    #[test]
    #[ignore = "native consumer acceptance; requires helper and exported article fixtures"]
    fn export_prepared_web_article_native_fixtures() {
        let output = std::path::PathBuf::from(
            std::env::var_os("TAILSYNC_WEB_ARTICLE_NATIVE_DIR")
                .expect("set native article fixture output"),
        );
        for name in ["html-only", "with-text-rtf"] {
            let bundle =
                PortableBundle::decode(&std::fs::read(output.join(format!("{name}.cbp"))).unwrap())
                    .unwrap();
            let plan = composite::restore::RestorePlan::portable_default(
                &bundle,
                composite::restore::RestoreInput {
                    source_platform: None,
                    requested_format: None,
                },
                RestoreTarget::Macos,
            )
            .unwrap();
            let plan = prepare_readable_restore(plan, &false.into()).unwrap();
            assert!(plan
                .native()
                .unwrap()
                .formats
                .iter()
                .any(|f| f.name == "com.apple.flat-rtfd"));
            std::fs::write(
                output.join(format!("{name}-prepared.cns")),
                plan.native().unwrap().encode().unwrap(),
            )
            .unwrap();
        }
    }

    #[test]
    #[ignore = "WebKit selection acceptance; requires exported browser source and helper"]
    fn prepare_browser_selection_for_native_consumer() {
        use tailsync_core::composite::supervisor::{run, HelperOutcome, HelperRequest};
        use tailsync_core::protocol::{
            native_archive::NativeArchive,
            native_clipboard::{PublishReceipt, PublishState},
        };
        let output = std::path::PathBuf::from(
            std::env::var_os("TAILSYNC_WEB_ARTICLE_NATIVE_DIR").expect("set native article output"),
        );
        let archive =
            NativeArchive::decode(&std::fs::read(output.join("browser-source.cna")).unwrap())
                .unwrap();
        let bundle = composite::from_archive(&archive).unwrap();
        let isolated = std::env::temp_dir().join(format!(
            "tailsync-web-article-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        let mut database = tailsync_core::db::HistoryDB::open_isolated_for_test_composite(
            &isolated,
        )
        .unwrap();
        let id = database
            .add_native_archive(&archive, "browser-acceptance")
            .unwrap();
        let payload = tailsync_runtime::history::HistoryOperations::new(&mut database)
            .restore_payload(id)
            .unwrap();
        let prepared = payload
            .prepare_restore_plan(true, RestoreTarget::Macos, None)
            .unwrap();
        let PreparedHistoryRestore::Ready(plan) = prepared
        else {
            panic!("browser document unavailable: {prepared:?}")
        };
        let plan = prepare_readable_restore(*plan, &false.into()).unwrap();
        let snapshot = plan.native().unwrap();
        let board = std::fs::read_to_string(output.join("target-board.txt")).unwrap();
        assert!(board.starts_with("tailsync-probe."));
        let mut input = [17; 16].to_vec();
        input.extend_from_slice(&(-1_i64).to_be_bytes());
        input.extend(snapshot.encode().unwrap());
        let result = run(HelperRequest {
            program: &resolve_helper().unwrap(),
            args: &["--pasteboard", board.trim(), "--write-snapshot"],
            stdin: Some(input),
            deadline: std::time::Duration::from_secs(6),
            stdout_limit: 4096,
        });
        let HelperOutcome::Exited {
            success: true,
            stdout,
            ..
        } = result
        else {
            panic!("private publication failed: {result:?}")
        };
        assert_eq!(
            PublishReceipt::decode(&stdout).unwrap().state,
            PublishState::Published
        );
        assert_eq!(
            database.get_composite_archive(id).unwrap().unwrap(),
            archive
        );
        std::fs::write(
            output.join("browser-portable.cbp"),
            bundle.encode().unwrap(),
        )
        .unwrap();
        drop(database);
        std::fs::remove_dir_all(isolated).unwrap();
    }

    #[test]
    fn browser_png_available_restore_survives_macos_preparation() {
        let png = composite::packed_to_png(1, 1, &[1, 2, 3, 255]).unwrap();
        let bundle = PortableBundle {
            representations: vec![
                Representation {
                    mime: Mime::Html,
                    bytes: b"<img src='blob:https://fixture.example/image'>".to_vec(),
                },
                Representation {
                    mime: Mime::Png,
                    bytes: png.clone(),
                },
            ],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::ExternalReferences,
        };
        for native in [
            None,
            Some(NativeSnapshot {
                before: 1,
                after: 1,
                items: 1,
                state: tailsync_core::protocol::native_clipboard::SnapshotState::Stable,
                formats: bundle
                    .representations
                    .iter()
                    .map(
                        |representation| tailsync_core::protocol::native_clipboard::NativeFormat {
                            item: 0,
                            name: if representation.mime == Mime::Png {
                                "public.png"
                            } else {
                                "public.html"
                            }
                            .into(),
                            state: tailsync_core::protocol::native_clipboard::FormatState::Captured,
                            bytes: representation.bytes.clone(),
                        },
                    )
                    .collect(),
            }),
        ] {
            let payload = HistoryRestorePayload {
                entry_type: "composite".into(),
                file_path: None,
                file_name: None,
                data: Some(bundle.encode().unwrap()),
                identity: None,
                native_only: false,
                native_archive_complete: native.as_ref().map(|_| true),
                native_archive_platform: native.as_ref().map(|_| 1),
                native,
            };
            let PreparedHistoryRestore::Ready(plan) = payload
                .prepare_restore_plan(true, RestoreTarget::Macos, None)
                .unwrap()
            else {
                panic!("saved browser PNG must be available for copying")
            };
            let outputs = plan.outputs();
            let plan = prepare_readable_restore(*plan, &false.into()).unwrap();
            assert_eq!(plan.outputs(), outputs);
            assert_eq!(plan.missing_images(), 1);
            let formats = &plan.native().unwrap().formats;
            assert_eq!(formats.len(), 1);
            assert_eq!(formats[0].name, "public.png");
            assert_eq!(formats[0].bytes, png);
        }
    }
}

pub fn write_native_cancellable(
    snapshot: &NativeSnapshot,
    expected: Option<i64>,
    cancelled: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
) -> Result<NativePublish, String> {
    if !supports_native(snapshot) {
        return Err("Native history belongs to another platform".into());
    }
    // Same projection as the read-back side: one model, not two (R2-03).
    let target = composite::fingerprint::origin_fingerprint_from_native(snapshot)
        .map_err(|_| "Invalid native publish set")?;
    // Source/conversion evidence belongs to history capture, not the native
    // publish set. The helper installs a fresh operation token itself.
    let mut publish = snapshot.clone();
    publish
        .formats
        .retain(|f| !tailsync_core::protocol::native_policy::control(&f.name));
    let encoded = publish.encode().map_err(|e| e.to_string())?;
    // Also covers synchronous compatibility callers until all transports use the
    // async port. Preparation above cannot hold up another helper operation.
    let _publish = PUBLISH
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if tracker()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .has_unresolved_intent()
    {
        return Ok(NativePublish::NoEffect {
            detail: "A previous clipboard operation remains unresolved".into(),
        });
    }
    let token: [u8; 16] = rand::random();
    // R3-03 / K4: register the intent **before** the helper can touch the clipboard.
    // Without it a lost receipt is unrecoverable: the device would observe its own
    // write with no matching record and re-broadcast it as a fresh user copy.
    if !tracker()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .record_intent(token, target)
    {
        // The intent budget is a hard limit. Publishing without an intent is exactly
        // the case that produces an echo, so refuse instead.
        return Err("Too many unresolved clipboard operations; refusing to publish".into());
    }
    if let Err(error) = persist_origin(
        &tracker()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    ) {
        discard_intent(token);
        return Ok(NativePublish::NoEffect { detail: error });
    }
    let mut input = token.to_vec();
    input.extend_from_slice(&expected.unwrap_or(-1).to_be_bytes());
    input.extend(encoded);

    let path = match resolve_helper() {
        Ok(path) => path,
        Err(health) => {
            discard_intent(token);
            return Ok(NativePublish::NoEffect {
                detail: health.reason(),
            });
        }
    };
    let register = |pid| {
        let mut tracker = tracker()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        tracker.mark_spawned(token, pid);
        persist_origin(&tracker)
    };
    let outcome = match Ok::<_, String>(composite::supervisor::run_registered(
        composite::supervisor::HelperRequest {
            program: &path,
            args: &["--write-snapshot"],
            stdin: Some(input),
            deadline: HELPER_PUBLISH_DEADLINE,
            stdout_limit: tailsync_core::protocol::native_clipboard::NATIVE_LIMIT,
        },
        cancelled.as_deref(),
        Some(&register),
    )) {
        Ok(outcome) => outcome,
        Err(error) => {
            discard_intent(token);
            return Ok(NativePublish::NoEffect { detail: error });
        }
    };
    match outcome {
        composite::supervisor::HelperOutcome::SpawnFailed { detail, .. } => {
            // Nothing was attempted, so this is a definite no-effect and a caller may
            // safely retry.
            discard_intent(token);
            Ok(NativePublish::NoEffect { detail })
        }
        composite::supervisor::HelperOutcome::TimedOut { .. }
        | composite::supervisor::HelperOutcome::OutputExceeded { .. } => {
            // The helper was killed, reaped and both pipes joined by the supervisor.
            // Establish the resolution boundary: bind the intent to the revision
            // observed *after* termination, so the next observation of our own write
            // is recognised instead of re-broadcast. If no revision is available the
            // intent stays unresolved, which keeps automatic publishing restricted
            // rather than letting a TTL turn it back into `External`.
            resolve_after_termination(token);
            Ok(NativePublish::Unknown {
                detail: "clipboard helper deadline expired; publish outcome unknown".into(),
            })
        }
        composite::supervisor::HelperOutcome::Exited { stdout, .. } => {
            match PublishReceipt::decode(&stdout) {
                Ok(receipt) if receipt.token == token => settle(token, target, receipt),
                // A missing, corrupt or foreign receipt after the helper ran is
                // precisely "a side effect may have happened": never a no-effect.
                _ => {
                    resolve_after_termination(token);
                    Ok(NativePublish::Unknown {
                        detail: "clipboard helper returned no usable receipt".into(),
                    })
                }
            }
        }
    }
}

/// Bind an unresolved intent to the revision observed after the helper was
/// terminated and reaped (K4).
fn resolve_after_termination(token: [u8; 16]) {
    let Some(revision) = read()
        .ok()
        .filter(|snapshot| snapshot.before == snapshot.after)
        .map(|snapshot| snapshot.after)
    else {
        // No revision means no resolution boundary. The intent stays unresolved on
        // purpose: the caller keeps a visible fault instead of a silent echo.
        return;
    };
    tracker()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .resolve_intent(token, revision);
    let _ = persist_origin(
        &tracker()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    );
}

fn discard_intent(token: [u8; 16]) {
    tracker()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .discard_intent(token);
    let _ = persist_origin(
        &tracker()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    );
}

/// Turn a receipt into a typed outcome and settle the intent.
fn settle(
    token: [u8; 16],
    target: [u8; 32],
    receipt: PublishReceipt,
) -> Result<NativePublish, String> {
    match receipt.state {
        PublishState::Published => {
            tracker()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .record(receipt.revision, target, receipt.token);
            persist_origin(
                &tracker()
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            )?;
            Ok(NativePublish::Published(receipt))
        }
        PublishState::Partial => {
            // K2: the *actual* published set must be confirmed by a stable read-back
            // at the receipt's revision. Falling back to the intended whole-group
            // fingerprint when that read fails would claim a self-write we cannot
            // prove, so the outcome stays Unknown instead.
            let confirmed = read()
                .ok()
                .filter(|s| s.before == receipt.revision && s.after == receipt.revision)
                .and_then(|s| composite::fingerprint::origin_fingerprint_from_native(&s).ok());
            match confirmed {
                Some(digest) => {
                    tracker()
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .record(receipt.revision, digest, receipt.token);
                    persist_origin(
                        &tracker()
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner),
                    )?;
                    Ok(NativePublish::Partial(receipt))
                }
                None => {
                    resolve_after_termination(token);
                    Ok(NativePublish::Unknown {
                        detail: "partial publish could not be confirmed by a stable read-back"
                            .into(),
                    })
                }
            }
        }
        PublishState::Superseded => {
            // The platform declined before writing, so the intent can go.
            discard_intent(token);
            Ok(NativePublish::Superseded(receipt))
        }
        PublishState::Failed => {
            // The helper reported a failure after possibly clearing. It did run, so
            // this is not a no-effect.
            resolve_after_termination(token);
            Ok(NativePublish::Unknown {
                detail: "clipboard helper reported a failed publish".into(),
            })
        }
    }
}
/// Protocol revision this build expects from the helper.
/// Bumped whenever the helper's wire contract changes, so a stale bundled helper
/// is reported as a version mismatch instead of being used and misparsed.
pub const HELPER_PROTOCOL_VERSION: u32 = if cfg!(target_os = "macos") { 4 } else { 2 };

/// Why the helper is or is not serviceable.
/// Reported distinctly (plan §4 R1-04) so startup can say "helper missing" versus
/// "helper present but the wrong version" instead of one generic failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HelperHealth {
    Healthy { protocol: u32 },
    Missing,
    NotExecutable,
    VersionMismatch { found: Option<u32>, expected: u32 },
    ProbeFailed { detail: String },
}

impl HelperHealth {
    pub fn is_healthy(&self) -> bool {
        matches!(self, Self::Healthy { .. })
    }
    /// Stable, content-free description for logs and the capability surface.
    pub fn reason(&self) -> String {
        match self {
            Self::Healthy { protocol } => format!("helper protocol {protocol}"),
            Self::Missing => "clipboard helper is not present".into(),
            Self::NotExecutable => "clipboard helper is not executable".into(),
            Self::VersionMismatch { found, expected } => match found {
                Some(found) => format!("clipboard helper protocol {found}, expected {expected}"),
                None => format!("clipboard helper reported no protocol, expected {expected}"),
            },
            Self::ProbeFailed { detail } => format!("clipboard helper self-check failed: {detail}"),
        }
    }
}

fn helper_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "tailsync-clipboard-helper.exe"
    } else {
        "clipboard-helper"
    }
}

/// The single canonical helper resolver for both platforms.
/// Release builds only ever use the helper that ships next to the executable; the
/// search never consults `PATH`, so a binary dropped in a writable directory cannot
/// be executed by accident. Debug builds additionally accept the build-script
/// output locations, including the triple-suffixed Windows sidecar
/// (`binaries/tailsync-clipboard-helper-<triple>.exe`) that `build-composite-helper.ps1`
/// produces — previously unreachable, so `tauri dev` could never find it (H-8).
pub fn resolve_helper() -> Result<std::path::PathBuf, HelperHealth> {
    let name = helper_name();
    if let Ok(executable) = std::env::current_exe() {
        if let Some(directory) = executable.parent() {
            let candidate = directory.join(name);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    #[cfg(debug_assertions)]
    {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let candidate = manifest.join(name);
        if candidate.is_file() {
            return Ok(candidate);
        }
        // Windows dev sidecar: a triple-suffixed binary under `binaries/`.
        let binaries = manifest.join("binaries");
        if let Ok(entries) = std::fs::read_dir(&binaries) {
            let prefix = "tailsync-clipboard-helper-";
            for entry in entries.flatten() {
                let file_name = entry.file_name();
                let Some(file_name) = file_name.to_str() else {
                    continue;
                };
                if file_name.starts_with(prefix) && file_name.ends_with(".exe") {
                    let candidate = entry.path();
                    if candidate.is_file() {
                        return Ok(candidate);
                    }
                }
            }
        }
    }
    Err(HelperHealth::Missing)
}

/// Whether the resolved helper is present, executable and the expected revision.
/// Runs `--self-check`, which touches no pasteboard, under a short deadline. This
/// is the "no-body helper self-check" the startup order requires *before* the
/// migration policy is frozen (plan §4 R1-04).
pub fn helper_health() -> HelperHealth {
    let path = match resolve_helper() {
        Ok(path) => path,
        Err(health) => return health,
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let executable = std::fs::metadata(&path)
            .map(|meta| meta.permissions().mode() & 0o111 != 0)
            .unwrap_or(false);
        if !executable {
            return HelperHealth::NotExecutable;
        }
    }
    let output = match composite::supervisor::run(composite::supervisor::HelperRequest {
        program: &path,
        args: &["--self-check"],
        stdin: None,
        deadline: HELPER_SELF_CHECK_DEADLINE,
        stdout_limit: 4096,
    }) {
        composite::supervisor::HelperOutcome::Exited {
            success: true,
            stdout,
            ..
        } => stdout,
        composite::supervisor::HelperOutcome::Exited { .. } => {
            return HelperHealth::ProbeFailed {
                detail: "helper self-check reported failure".into(),
            }
        }
        composite::supervisor::HelperOutcome::TimedOut { .. } => {
            return HelperHealth::ProbeFailed {
                detail: "helper self-check timed out".into(),
            }
        }
        composite::supervisor::HelperOutcome::OutputExceeded { .. } => {
            return HelperHealth::ProbeFailed {
                detail: "helper self-check output exceeded the limit".into(),
            }
        }
        composite::supervisor::HelperOutcome::SpawnFailed { detail, .. } => {
            return HelperHealth::ProbeFailed { detail }
        }
    };
    let text = String::from_utf8_lossy(&output);
    let protocol = text
        .split_whitespace()
        .find_map(|token| token.strip_prefix("protocol="))
        .and_then(|value| value.parse::<u32>().ok());
    match protocol {
        Some(protocol) if protocol == HELPER_PROTOCOL_VERSION => HelperHealth::Healthy { protocol },
        found => HelperHealth::VersionMismatch {
            found,
            expected: HELPER_PROTOCOL_VERSION,
        },
    }
}

/// Short deadline for the health probe: it does no clipboard work, so it must not
/// inherit the multi-second publish budget.
const HELPER_SELF_CHECK_DEADLINE: std::time::Duration = std::time::Duration::from_secs(2);

pub const HELPER_ANALYZE_CAPABILITY: u32 = 1;

fn helper_self_check() -> Option<String> {
    helper_self_check_cancellable(None)
}

fn helper_self_check_cancellable(
    cancelled: Option<&std::sync::atomic::AtomicBool>,
) -> Option<String> {
    let path = resolve_helper().ok()?;
    match composite::supervisor::run_cancellable(
        composite::supervisor::HelperRequest {
            program: &path,
            args: &["--self-check"],
            stdin: None,
            deadline: HELPER_SELF_CHECK_DEADLINE,
            stdout_limit: 4096,
        },
        cancelled,
    ) {
        composite::supervisor::HelperOutcome::Exited {
            success: true,
            stdout,
            ..
        } => Some(String::from_utf8_lossy(&stdout).into_owned()),
        _ => None,
    }
}

pub fn helper_analyze_supported() -> bool {
    helper_analysis_capable(helper_self_check())
}

fn helper_analysis_capable(text: Option<String>) -> bool {
    let Some(text) = text else {
        return false;
    };
    let field = |name: &str| {
        text.split_whitespace()
            .find_map(|token| token.strip_prefix(name))
            .and_then(|value| value.parse::<u32>().ok())
    };
    field("protocol=") == Some(HELPER_PROTOCOL_VERSION)
        && field("analyze=").is_some_and(|value| value >= HELPER_ANALYZE_CAPABILITY)
}

pub fn analyze_saved_bytes(
    task: &NativeAnalysisTask,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<NativeAnalysisFacts, String> {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        if !helper_analysis_capable(helper_self_check_cancellable(Some(cancelled))) {
            return Err("clipboard helper does not advertise saved-bytes analysis".into());
        }
        let command = match task.kind {
            SavedAnalysisKind::Rtf => "--analyze-rtf",
            SavedAnalysisKind::Rtfd => "--analyze-rtfd",
        };
        let outcome = run_helper_outcome_cancellable_limited(
            command,
            Some(task.bytes.clone()),
            tailsync_runtime::content_analysis::NATIVE_ANALYSIS_DEADLINE,
            SAVED_FRAME_LIMIT,
            Some(cancelled),
        )?;
        match outcome {
            composite::supervisor::HelperOutcome::Exited {
                success: true,
                stdout,
                ..
            } => {
                let analysis =
                    SavedTextAnalysis::decode(&stdout).map_err(|error| error.to_string())?;
                if analysis.kind != task.kind {
                    return Err("saved-bytes analysis kind mismatch".into());
                }
                Ok(NativeAnalysisFacts::from_saved(analysis, task.item))
            }
            composite::supervisor::HelperOutcome::Exited { success: false, .. } => {
                Err("clipboard helper rejected the saved bytes".into())
            }
            composite::supervisor::HelperOutcome::TimedOut { .. } => {
                Err("saved-bytes analysis timed out".into())
            }
            composite::supervisor::HelperOutcome::OutputExceeded { .. } => {
                Err("saved-bytes analysis output exceeded the limit".into())
            }
            composite::supervisor::HelperOutcome::SpawnFailed { detail, .. } => Err(detail),
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = (task, cancelled);
        Err("Native clipboard unavailable".into())
    }
}

pub struct ClipboardNativeAnalyzer;

impl NativeAnalyzer for ClipboardNativeAnalyzer {
    fn analyze(
        &self,
        task: &NativeAnalysisTask,
        cancelled: &std::sync::atomic::AtomicBool,
    ) -> Result<NativeAnalysisFacts, String> {
        analyze_saved_bytes(task, cancelled)
    }
}

pub fn native_analyzer() -> Arc<dyn NativeAnalyzer> {
    Arc::new(ClipboardNativeAnalyzer)
}

/// Typed result of a native publish attempt (ADR-004 K2).
/// The type lives in Core so the `SyncPlatform` trait can carry it; the platform
/// layer only produces it.
pub use tailsync_core::composite::NativePublish;

/// One helper exchange under a single deadline, via the shared supervisor.
/// All process handling — stdin/stdout/stderr, the deadline, cancellation, kill,
/// reap and thread joins — lives in `composite::supervisor`, so the platform layer
/// and the probes exercise exactly the same implementation (R3-02).
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn run_helper_outcome(
    command: &str,
    input: Option<Vec<u8>>,
    deadline: std::time::Duration,
) -> Result<composite::supervisor::HelperOutcome, String> {
    run_helper_outcome_cancellable(command, input, deadline, None)
}
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn run_helper_outcome_cancellable(
    command: &str,
    input: Option<Vec<u8>>,
    deadline: std::time::Duration,
    cancelled: Option<&std::sync::atomic::AtomicBool>,
) -> Result<composite::supervisor::HelperOutcome, String> {
    run_helper_outcome_cancellable_limited(
        command,
        input,
        deadline,
        tailsync_core::protocol::native_clipboard::NATIVE_LIMIT,
        cancelled,
    )
}
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn run_helper_outcome_cancellable_limited(
    command: &str,
    input: Option<Vec<u8>>,
    deadline: std::time::Duration,
    stdout_limit: usize,
    cancelled: Option<&std::sync::atomic::AtomicBool>,
) -> Result<composite::supervisor::HelperOutcome, String> {
    let path = resolve_helper().map_err(|health| health.reason())?;
    Ok(composite::supervisor::run_cancellable(
        composite::supervisor::HelperRequest {
            program: &path,
            args: &[command],
            stdin: input,
            deadline,
            stdout_limit,
        },
        cancelled,
    ))
}

/// Read-side helper call: a timeout here is a capture failure, not a publish result.
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn run_helper(command: &str, input: Option<Vec<u8>>) -> Result<Vec<u8>, String> {
    match run_helper_outcome(command, input, HELPER_PUBLISH_DEADLINE)? {
        composite::supervisor::HelperOutcome::Exited {
            success: true,
            stdout,
            ..
        } => Ok(stdout),
        composite::supervisor::HelperOutcome::Exited { success: false, .. } => {
            Err("Clipboard helper reported failure".into())
        }
        composite::supervisor::HelperOutcome::TimedOut { .. } => {
            Err("Clipboard helper timed out".into())
        }
        composite::supervisor::HelperOutcome::OutputExceeded { .. } => {
            Err("Clipboard helper output exceeded the limit".into())
        }
        composite::supervisor::HelperOutcome::SpawnFailed { detail, .. } => Err(detail),
    }
}

/// Deadline for a publish exchange. Covers spawn, stdin, run and stdout together.
const HELPER_PUBLISH_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn run_helper(_: &str, _: Option<Vec<u8>>) -> Result<Vec<u8>, String> {
    Err("Native clipboard unavailable".into())
}

/// Read-only packaging diagnostic. It uses the exact production resolver and
/// health probe before logging, identity, storage, network or clipboard startup.
pub fn helper_check_cli() -> Option<i32> {
    if !std::env::args().any(|arg| arg == "--tailsync-clipboard-helper-check") {
        return None;
    }
    let health = helper_health();
    println!(
        "{}",
        serde_json::json!({
            "schema": 1,
            "helper_path": resolve_helper().ok(),
            "expected_protocol": HELPER_PROTOCOL_VERSION,
            "healthy": health.is_healthy(),
            "detail": health.reason(),
            "analyze_capable": helper_analyze_supported(),
            "analyzer": std::any::type_name_of_val(&*native_analyzer()),
            "composite_compiled": cfg!(feature = "protocol-composite-clipboard"),
            "debug_resolver": cfg!(debug_assertions),
        })
    );
    Some(if health.is_healthy() { 0 } else { 1 })
}
