// macOS CNA1 archive-provider regressions. Included only by the macOS capture fixture.

#[tokio::test]
async fn qq_multi_image_capture_saves_and_broadcasts_once_without_origin_warning() {
    qq_image_capture_saves_and_broadcasts(2, 1).await;
}

#[tokio::test]
async fn qq_message_item_file_descriptor_saves_and_broadcasts_without_origin_warning() {
    qq_image_capture_saves_and_broadcasts(1, 0).await;
}

async fn qq_image_capture_saves_and_broadcasts(items: u16, descriptor_item: u16) {
    use base64::Engine as _;
    use image::ImageEncoder as _;
    use tailsync_core::protocol::native_archive::{
        DerivedFormat, CONVERTER_QQ_TO_PORTABLE, QQ_MESSAGE_FORMAT,
    };
    let _serial = capture_serial().lock().await;
    let _ = drain_warnings();
    let mut fx = fixture(Shape::Html, OriginMode::External, false, true).await;
    let mut queue = install_capture_sender(&fx.pool).await;
    let mut images = Vec::new();
    for color in [[255_u8, 0, 0, 255], [0, 0, 255, 255]] {
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(&color, 1, 1, image::ExtendedColorType::Rgba8)
            .unwrap();
        images.push(base64::engine::general_purpose::STANDARD.encode(png));
    }
    let html = format!("<p>QQ before</p><img src='data:image/png;base64,{}'><p>middle</p><img src='data:image/png;base64,{}'><p>after</p>", images[0], images[1]);
    let archive = NativeArchive {
        platform: 1,
        enumeration_complete: true,
        original: NativeSnapshot {
            before: 100,
            after: 100,
            items,
            state: SnapshotState::Stable,
            formats: vec![
                NativeFormat {
                    item: 0,
                    name: QQ_MESSAGE_FORMAT.into(),
                    state: FormatState::Captured,
                    bytes: b"retained QQ private message".to_vec(),
                },
                NativeFormat {
                    item: descriptor_item,
                    name: "public.file-url".into(),
                    state: FormatState::Absent,
                    bytes: vec![],
                },
            ],
        },
        derived: [
            ("public.html", html),
            ("public.utf8-plain-text", "QQ before middle after".into()),
        ]
        .into_iter()
        .map(|(name, value)| DerivedFormat {
            item: 0,
            name: name.into(),
            source_name: QQ_MESSAGE_FORMAT.into(),
            converter: CONVERTER_QQ_TO_PORTABLE.into(),
            bytes: value.into_bytes(),
        })
        .collect(),
    };
    archive.validate().unwrap();
    let expected = tailsync_core::composite::from_archive(&archive).unwrap();
    assert_eq!(expected.resources.len(), 2);
    assert!(!crate::clipboard_composite::supports_native(
        &archive.original
    ));
    fx.env.read_archive = Arc::new(move || Ok(archive.clone()));
    assert_eq!(fx.tick(100).await, CompositeCapture::Handled);
    assert_eq!(fx.history_len().await, 1);
    assert_eq!(fx.acks(), vec![100]);
    assert!(!drain_warnings().contains(&"capture_origin_unknown"));
    let frame = queue
        .priority
        .try_recv()
        .expect("the QQ message must enter the real peer queue");
    assert_eq!(frame.command(), Command::CompositePayload);
    assert!(
        queue.priority.try_recv().is_err(),
        "one capture must queue only one event"
    );
    let wire = tailsync_core::composite::PortableBundle::decode(
        &frame.payload()[tailsync_core::protocol::EVENT_ENVELOPE_HEADER_SIZE..],
    )
    .unwrap();
    assert_eq!(
        wire, expected,
        "both images and text must reach the peer payload"
    );
    let id = fx.history_id().await;
    let db = fx.database.lock().await;
    assert_eq!(db.get_composite(id).unwrap(), expected);
}

#[tokio::test]
async fn wechat_qt_image_is_saved_and_queued_once_without_following_file_url() {
    use tailsync_core::protocol::native_archive::{DerivedFormat, CONVERTER_TIFF_TO_PNG};
    let _serial = capture_serial().lock().await;
    let _ = drain_warnings();
    let mut fx = fixture(Shape::Html, OriginMode::External, false, true).await;
    let mut queue = install_capture_sender(&fx.pool).await;
    let png = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../shared/tailsync-protocol/tests/fixtures/images/transparent.png"
    ));
    let tiff = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../shared/tailsync-protocol/tests/fixtures/images/color.tiff"
    ));
    let archive = NativeArchive {
        platform: 1,
        enumeration_complete: true,
        original: NativeSnapshot {
            before: 100,
            after: 100,
            items: 1,
            state: SnapshotState::Stable,
            formats: vec![
                NativeFormat {
                    item: 0,
                    name: "public.file-url".into(),
                    state: FormatState::Absent,
                    bytes: vec![],
                },
                NativeFormat {
                    item: 0,
                    name: "com.trolltech.anymime.text--uri-list".into(),
                    state: FormatState::Captured,
                    bytes: vec![],
                },
                NativeFormat {
                    item: 0,
                    name: "public.tiff".into(),
                    state: FormatState::Captured,
                    bytes: tiff.to_vec(),
                },
                NativeFormat {
                    item: 0,
                    name: "com.trolltech.anymime.application--x-qt-image".into(),
                    state: FormatState::Captured,
                    bytes: vec![],
                },
            ],
        },
        derived: vec![DerivedFormat {
            item: 0,
            name: "public.png".into(),
            source_name: "public.tiff".into(),
            converter: CONVERTER_TIFF_TO_PNG.into(),
            bytes: png.to_vec(),
        }],
    };
    let expected = tailsync_core::composite::from_archive(&archive).unwrap();
    assert_eq!(
        expected.representation(tailsync_core::composite::Mime::Png),
        Some(png.as_slice())
    );
    assert!(!crate::clipboard_composite::supports_native(
        &archive.original
    ));
    let retained = archive.clone();
    fx.env.read_archive = Arc::new(move || Ok(archive.clone()));
    assert_eq!(fx.tick(100).await, CompositeCapture::Handled);
    assert_eq!(fx.history_len().await, 1);
    assert_eq!(fx.acks(), vec![100]);
    assert!(!drain_warnings().contains(&"capture_origin_unknown"));
    let frame = queue
        .priority
        .try_recv()
        .expect("WeChat image must enter the peer queue");
    assert_eq!(frame.command(), Command::CompositePayload);
    let sent = tailsync_core::composite::PortableBundle::decode(
        &frame.payload()[tailsync_core::protocol::EVENT_ENVELOPE_HEADER_SIZE..],
    )
    .unwrap();
    assert_eq!(sent, expected);
    assert!(queue.priority.try_recv().is_err());
    let id = fx.history_id().await;
    let db = fx.database.lock().await;
    assert_eq!(db.get_composite(id).unwrap(), expected);
    assert_eq!(db.get_composite_archive(id).unwrap(), Some(retained));
    let entries = db.get_all(None, None, 10, 0).unwrap();
    let entry = entries.iter().find(|entry| entry.id == id).unwrap();
    let metadata = tailsync_core::protocol::image::metadata("public.png", png).unwrap();
    assert_eq!(
        entry.description,
        format!(
            "image/png · {}×{} · 1 frame",
            metadata.width, metadata.height
        )
    );
    assert_eq!(entry.category, "image");
}
