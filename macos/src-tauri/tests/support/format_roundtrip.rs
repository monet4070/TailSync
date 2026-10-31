// Opt-in native acceptance: shipped helper, private pasteboard, real capture,
// reopened encrypted history, restore preparation and Swift-consumable frames.

fn matrix_helper(args: &[&str], input: Option<Vec<u8>>) -> Vec<u8> {
    use tailsync_core::composite::supervisor::{run, HelperOutcome, HelperRequest};
    let helper = crate::clipboard_composite::resolve_helper().unwrap();
    // OS process boundary: a hung or failed helper fails this case, with bounded output.
    let result = run(HelperRequest {
        program: &helper,
        args,
        stdin: input,
        deadline: Duration::from_secs(6),
        stdout_limit: 40 * 1024 * 1024,
    });
    match result {
        HelperOutcome::Exited {
            success: true,
            stdout,
            ..
        } => stdout,
        other => panic!("native matrix helper {args:?}: {other:?}"),
    }
}

fn matrix_publish(board: &str, snapshot: &NativeSnapshot) -> i64 {
    use tailsync_core::protocol::native_clipboard::{PublishReceipt, PublishState};
    let token: [u8; 16] = rand::random();
    let mut input = token.to_vec();
    input.extend_from_slice(&(-1_i64).to_be_bytes());
    input.extend(snapshot.encode().unwrap());
    let receipt = PublishReceipt::decode(&matrix_helper(
        &["--pasteboard", board, "--write-snapshot"],
        Some(input),
    ))
    .unwrap();
    assert_eq!(receipt.state, PublishState::Published);
    assert_eq!(receipt.token, token);
    receipt.revision
}

fn matrix_format(item: u16, name: &str, bytes: &[u8]) -> NativeFormat {
    NativeFormat {
        item,
        name: name.into(),
        state: FormatState::Captured,
        bytes: bytes.to_vec(),
    }
}

fn matrix_snapshot(formats: Vec<NativeFormat>) -> NativeSnapshot {
    NativeSnapshot {
        before: 0,
        after: 0,
        state: SnapshotState::Stable,
        items: formats.iter().map(|f| f.item).max().unwrap() + 1,
        formats,
    }
}

fn matrix_output() -> PathBuf {
    PathBuf::from(
        std::env::var_os("TAILSYNC_FORMAT_MATRIX_DIR")
            .expect("set TAILSYNC_FORMAT_MATRIX_DIR to the generated synthetic fixture directory"),
    )
}

#[tokio::test]
#[ignore = "native acceptance; requires a packaged helper beside the test harness and generated fixtures"]
async fn native_format_history_restore_preview_matrix() {
    use tailsync_core::composite::{self, restore::RestoreTarget};
    use tailsync_runtime::history::{HistoryOperations, PreparedHistoryRestore};
    let _serial = capture_serial().lock().await;
    let output = matrix_output();
    let images = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../shared/tailsync-protocol/tests/fixtures/images");
    let png = std::fs::read(images.join("transparent.png")).unwrap();
    let rtf = std::fs::read(output.join("fixtures/native.rtf")).unwrap();
    let text = |body: &str| matrix_format(0, "public.utf8-plain-text", body.as_bytes());
    let html = |body: &str| matrix_format(0, "public.html", body.as_bytes());
    let mut fixtures = vec![
        (
            "plain-text",
            "text",
            vec![text("中文、空白与 emoji 🙂\nsecond line\n")],
        ),
        (
            "url",
            "text",
            vec![text("https://example.com/tailsync?case=history")],
        ),
        (
            "code",
            "code",
            vec![text("func acceptance() {\n    let answer = 42\n}")],
        ),
        (
            "command",
            "code",
            vec![text("cargo test --locked --offline")],
        ),
        (
            "json",
            "code",
            vec![text("{\"fixture\":\"history\",\"number\":42}")],
        ),
        (
            "path",
            "text",
            vec![text("/Users/fixture/Documents/history.txt")],
        ),
        (
            "markdown-text",
            "text",
            vec![text("# Heading\n\n**bold**\n\n- first\n- second")],
        ),
        (
            "html",
            "text",
            vec![html("<h1>History</h1><p><b>bold</b> and 中文</p>")],
        ),
        (
            "rtf",
            "document",
            vec![matrix_format(0, "public.rtf", &rtf)],
        ),
        (
            "text-html-rtf",
            "text",
            vec![
                text("History acceptance"),
                html("<p><b>History</b> acceptance</p>"),
                matrix_format(0, "public.rtf", &rtf),
            ],
        ),
        (
            "opaque-native",
            "text",
            vec![matrix_format(
                0,
                "org.tailsync.fixture.opaque",
                &[0, 1, 255, 42],
            )],
        ),
        (
            "multiple-native-items",
            "text",
            vec![
                text("first native item"),
                matrix_format(1, "public.utf8-plain-text", b"second native item"),
            ],
        ),
        (
            "browser-image-missing-html",
            "image",
            vec![
                html("<img src='blob:https://fixture.example/saved-image'>"),
                matrix_format(0, "public.png", &png),
            ],
        ),
        (
            "html-missing-image-with-text",
            "document",
            vec![
                text("saved words"),
                html("<p>saved words</p><img src='blob:https://fixture.example/missing'>"),
            ],
        ),
    ];
    for (name, uti, file) in [
        ("png", "public.png", "transparent.png"),
        ("jpeg", "public.jpeg", "color.jpg"),
        ("tiff", "public.tiff", "color.tiff"),
        ("gif-animation", "com.compuserve.gif", "animated.gif"),
        ("webp-static", "org.webmproject.webp", "static.webp"),
        ("webp-animation", "org.webmproject.webp", "animated.webp"),
        ("apng-animation", "public.png", "animated.png"),
        ("tiff-multiple-pages", "public.tiff", "pages.tiff"),
    ] {
        fixtures.push((
            name,
            "image",
            vec![matrix_format(
                0,
                uti,
                &std::fs::read(images.join(file)).unwrap(),
            )],
        ));
    }
    use base64::Engine as _;
    let inline = base64::engine::general_purpose::STANDARD.encode(&png);
    fixtures.push(("html-two-images", "document", vec![html(&format!(
        "<p>before</p><img src='data:image/png;base64,{inline}'><p>middle</p><img src='data:image/png;base64,{inline}'><p>after</p>"
    ))]));
    let board = format!(
        "tailsync-probe.format-matrix-{:016x}",
        rand::random::<u64>()
    );
    let rtfd = NativeSnapshot::decode(&matrix_helper(
        &["--pasteboard", &board, "--probe-rtfd"],
        None,
    ))
    .unwrap();
    fixtures.push((
        "rtfd-attachment",
        "document",
        rtfd.formats
            .into_iter()
            .filter(|format| format.name == "com.apple.flat-rtfd")
            .collect(),
    ));
    let mut records = Vec::new();
    let mut blocked = Vec::new();
    for (name, expected_format, formats) in fixtures {
        let revision = matrix_publish(&board, &matrix_snapshot(formats));
        let captured = NativeArchive::decode(&matrix_helper(
            &["--pasteboard", &board, "--read-archive"],
            None,
        ))
        .unwrap();
        assert_eq!(captured.original.after, revision, "{name}");
        if matches!(name, "rtf" | "text-html-rtf") {
            assert!(
                captured.originals_complete(),
                "{name}: RTF must remain complete after the publisher exits"
            );
            assert_eq!(
                captured
                    .original
                    .formats
                    .iter()
                    .find(|f| f.name == "public.rtf")
                    .unwrap()
                    .bytes,
                rtf
            );
        }
        std::fs::write(
            output.join(format!("{name}.archive")),
            captured.encode().unwrap(),
        )
        .unwrap();
        println!(
            "native matrix: {name}: capture projection={:?}",
            composite::from_archive(&captured).map(|b| b.completeness)
        );
        let mut fx = fixture(Shape::Html, OriginMode::External, false, false).await;
        fx.revision.store(revision, Ordering::SeqCst);
        let retained = captured.clone();
        fx.env.read_archive = Arc::new(move || Ok(retained.clone()));
        for _ in 0..4 {
            assert_eq!(fx.tick(revision).await, CompositeCapture::Handled, "{name}");
            if fx.history_len().await == 1 {
                break;
            }
            fx.advance_clock_ms(2000);
        }
        assert_eq!(
            fx.history_len().await,
            1,
            "{name}: capture must save exactly one logical history entry; faults={:?}",
            fx.faults()
        );
        assert_eq!(fx.acks(), vec![revision], "{name}");
        let id = fx.history_id().await;
        let reopened = Arc::new(Mutex::new(
            HistoryDB::open_with_policy_for_probe(
                &fx.root,
                tailsync_core::db::MigrationPolicy::CompositeAllowed,
            )
            .unwrap(),
        ));
        let page = tailsync_runtime::history_display::page_display_async(
            reopened.clone(),
            tailsync_runtime::history::HistoryPageRequestOwned {
                collection: tailsync_core::db::HistoryCollection::All,
                category: None,
                keyword: None,
                start_time: None,
                end_time: None,
                limit: 10,
                offset: 0,
            },
            crate::clipboard_composite::native_analyzer(),
        )
        .await
        .unwrap();
        let (entry, payload) = {
            let mut db = reopened.lock().await;
            assert_eq!(
                db.get_composite_archive(id).unwrap(),
                Some(captured.clone()),
                "{name}: original archive survives reopen"
            );
            let entry = page
                .entries
                .into_iter()
                .find(|entry| entry.id == id)
                .unwrap();
            let payload = HistoryOperations::new(&mut db).restore_payload(id).unwrap();
            (entry, payload)
        };
        let prepared = payload
            .prepare_restore_plan(
                payload
                    .native
                    .as_ref()
                    .is_some_and(crate::clipboard_composite::supports_native),
                RestoreTarget::Macos,
                None,
            )
            .unwrap();
        let (default_restore, plan) = match prepared {
            PreparedHistoryRestore::Ready(plan) => ("published".to_string(), Some(plan)),
            PreparedHistoryRestore::Unsupported { code } => {
                blocked.push(format!("{name}: {code}"));
                let fallback_format = match name {
                    "rtf" => Some("rtf"),
                    "text-html-rtf" => Some("html"),
                    _ => None,
                };
                let explicit = if let Some(format) = fallback_format {
                    match payload
                        .prepare_restore_plan(false, RestoreTarget::Macos, Some(format))
                        .unwrap()
                    {
                        PreparedHistoryRestore::Ready(plan) => Some(plan),
                        unsupported => panic!("RTF explicit restore also refused: {unsupported:?}"),
                    }
                } else {
                    None
                };
                (format!("blocked:{code}"), explicit)
            }
        };
        let mut missing_images = 0;
        let mut restored_formats = Vec::new();
        if let Some(plan) = plan {
            let plan = crate::clipboard_composite::prepare_readable_restore(
                *plan,
                &AtomicBool::new(false),
            )
            .unwrap();
            missing_images = plan.missing_images();
            let publication = plan
                .native()
                .cloned()
                .unwrap_or_else(|| composite::to_macos_snapshot(plan.portable().unwrap()).unwrap());
            let restored_revision = matrix_publish(&board, &publication);
            let readback = NativeArchive::decode(&matrix_helper(
                &["--pasteboard", &board, "--read-archive"],
                None,
            ))
            .unwrap()
            .original;
            assert_eq!(readback.after, restored_revision);
            for format in &publication.formats {
                if !tailsync_core::protocol::native_policy::control(&format.name) {
                    assert!(
                        readback.formats.iter().any(|f| f.item == format.item
                            && f.name == format.name
                            && f.bytes == format.bytes),
                        "{name}: restored {} bytes differ",
                        format.name
                    );
                }
            }
            assert_eq!(
                tailsync_core::composite::fingerprint::origin_fingerprint_from_native(&publication).unwrap(),
                tailsync_core::composite::fingerprint::origin_fingerprint_from_capture(&readback).unwrap(),
                "{name}: native publication must still be identifiable as SelfWrite after the helper exits"
            );
            restored_formats = publication.formats.iter().map(|f| f.name.clone()).collect();
        }
        let frame_name = format!("{name}.preview");
        let display_kind = entry.display.as_ref().map(|d| d.kind.as_str());
        let frame = if display_kind == Some("document") {
            let preview = tailsync_runtime::document_preview::document_preview_async(
                reopened.clone(),
                id,
                Some(name.into()),
                crate::clipboard_composite::native_analyzer(),
                Cancellation::default(),
            )
            .await
            .unwrap();
            preview.bytes().to_vec()
        } else {
            let db = reopened.lock().await;
            let metadata = db.get_preview_metadata(id).unwrap();
            let preview = db.get_preview_payload(id).unwrap();
            tailsync_runtime::preview::encode_preview_response_for_request(
                metadata,
                preview,
                Some(name.into()),
            )
            .unwrap()
        };
        std::fs::write(output.join(&frame_name), frame).unwrap();
        records.push(serde_json::json!({"name":name,"frame":frame_name,"expected_format":expected_format,
            "category":entry.category,"display_kind":display_kind,"history_reopened":true,
            "default_restore":default_restore,"restored_formats":restored_formats,
            "missing_images":missing_images,"original_formats":captured.original.formats.iter().map(|f| &f.name).collect::<Vec<_>>() }));
        std::fs::write(
            output.join("native-results.json"),
            serde_json::to_vec_pretty(&records).unwrap(),
        )
        .unwrap();
        println!("native matrix: {name}: saved/reopened/preview-frame; restore={default_restore}; missing_images={missing_images}");
    }
    assert_eq!(records.len(), 24);
    assert!(
        blocked.is_empty(),
        "default restore acceptance failures: {blocked:?}"
    );
}

#[tokio::test]
#[ignore = "native acceptance; requires a packaged helper beside the test harness and generated fixtures"]
async fn file_format_history_restore_preview_matrix() {
    let _serial = capture_serial().lock().await;
    let output = matrix_output();
    let files: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(output.join("fixtures/manifest.json")).unwrap())
            .unwrap();
    let board = format!("tailsync-probe.file-matrix-{:016x}", rand::random::<u64>());
    let mut records = Vec::new();
    for (batch_index, group) in files.chunks(20).enumerate() {
        let paths = group
            .iter()
            .map(|v| output.join("fixtures").join(v["name"].as_str().unwrap()))
            .collect::<Vec<_>>();
        let mut args = vec!["--pasteboard", &board, "--write-files"];
        args.extend(paths.iter().map(|p| p.to_str().unwrap()));
        matrix_helper(&args, None);
        let archive = NativeArchive::decode(&matrix_helper(
            &["--pasteboard", &board, "--read-archive"],
            None,
        ))
        .unwrap();
        assert_eq!(
            tailsync_core::composite::from_archive(&archive).unwrap_err(),
            tailsync_core::composite::CaptureError::Files
        );
        let readback = String::from_utf8(matrix_helper(&["--pasteboard", &board], None)).unwrap();
        assert_eq!(
            readback.lines().map(PathBuf::from).collect::<Vec<_>>(),
            paths
        );
        let fx = fixture(Shape::Files, OriginMode::External, false, false).await;
        let prepared = crate::sync::prepare_file_batch(paths.clone(), 1).unwrap();
        let batch = format!("format-matrix-{batch_index}");
        let inputs = prepared
            .files
            .iter()
            .map(|file| tailsync_core::db::HistoryFileInput {
                name: file.entry.name.clone(),
                path: file.path.clone(),
                data_hash: file.entry.hash.clone(),
                size: file.entry.size,
            })
            .collect::<Vec<_>>();
        fx.database
            .lock()
            .await
            .add_file_batch(&batch, &inputs, "self", false)
            .unwrap();
        let mut reopened = HistoryDB::open_with_policy_for_probe(
            &fx.root,
            tailsync_core::db::MigrationPolicy::CompositeAllowed,
        )
        .unwrap();
        let entries = reopened.get_all(None, None, 100, 0).unwrap();
        assert_eq!(entries.len(), group.len());
        let restored = reopened.materialize_file_batch(&batch).unwrap();
        let mut args = vec!["--pasteboard", &board, "--write-files"];
        args.extend(restored.iter().map(|p| p.to_str().unwrap()));
        matrix_helper(&args, None);
        let readback = String::from_utf8(matrix_helper(&["--pasteboard", &board], None)).unwrap();
        assert_eq!(
            readback.lines().map(PathBuf::from).collect::<Vec<_>>(),
            restored
        );
        for spec in group {
            let name = spec["name"].as_str().unwrap();
            let entry = entries.iter().find(|e| e.description == name).unwrap();
            let original = std::fs::read(output.join("fixtures").join(name)).unwrap();
            let payload = tailsync_runtime::history::HistoryOperations::new(&mut reopened)
                .restore_payload(entry.id)
                .unwrap();
            assert_eq!(
                std::fs::read(payload.file_path.unwrap()).unwrap(),
                original,
                "{name}: individual restore bytes"
            );
            assert_eq!(
                std::fs::read(
                    restored
                        .iter()
                        .find(|p| p.file_name().unwrap() == name)
                        .unwrap()
                )
                .unwrap(),
                original,
                "{name}: batch restore bytes"
            );
            let metadata = reopened.get_preview_metadata(entry.id).unwrap();
            let navigation = metadata.batch.as_ref().unwrap();
            assert_eq!(navigation.item_count, group.len());
            let preview = reopened.get_preview_payload(entry.id).unwrap();
            assert_eq!(preview.data, original, "{name}: preview bytes");
            let frame_name = format!("{name}.preview");
            let frame = tailsync_runtime::preview::encode_preview_response_for_request(
                metadata,
                preview,
                Some(name.into()),
            )
            .unwrap();
            std::fs::write(output.join(&frame_name), frame).unwrap();
            records.push(serde_json::json!({"name":name,"frame":frame_name,"expected_format":spec["format"],
                "category":"file","display_kind":null,"batch_size":group.len(),"history_reopened":true}));
            println!("file matrix: {name}: captured/saved/reopened/restored/preview-frame");
        }
        std::fs::write(
            output.join("file-results.json"),
            serde_json::to_vec_pretty(&records).unwrap(),
        )
        .unwrap();
    }
    assert_eq!(records.len(), files.len());
}
