use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::sync::Arc;
use tailsync_core::{
    cancellation::Cancellation,
    composite::packed_to_png,
    db::{HistoryCollection, HistoryDB, MigrationPolicy},
    protocol::{
        native_archive::{
            DerivedFormat, NativeArchive, CONVERTER_QQ_TO_PORTABLE, QQ_MESSAGE_FORMAT,
        },
        native_clipboard::*,
    },
};
use tailsync_runtime::{
    content_analysis::{analyze_document_cancellable, UnsupportedNativeAnalyzer},
    history::HistoryPageRequestOwned,
    history_display::{image_payload_checked_async, page_display_async},
};
use tokio::sync::Mutex;

#[tokio::test]
async fn qq_history_list_is_a_readable_document_with_both_images_and_provenance() {
    let html = format!(
        "<img src='data:image/png;base64,{}'><p>QQ 中文</p><img src='data:image/png;base64,{}'>",
        STANDARD.encode(packed_to_png(1, 1, &[255, 0, 0, 255]).unwrap()),
        STANDARD.encode(packed_to_png(1, 1, &[0, 0, 255, 255]).unwrap())
    );
    let archive = NativeArchive {
        platform: 1,
        enumeration_complete: true,
        original: NativeSnapshot {
            before: 1,
            after: 1,
            items: 2,
            state: SnapshotState::Stable,
            formats: vec![
                NativeFormat {
                    item: 0,
                    name: QQ_MESSAGE_FORMAT.into(),
                    state: FormatState::Captured,
                    bytes: b"retained source".to_vec(),
                },
                NativeFormat {
                    item: 1,
                    name: "public.file-url".into(),
                    state: FormatState::Absent,
                    bytes: vec![],
                },
            ],
        },
        derived: [
            ("public.html", html),
            ("public.utf8-plain-text", "QQ 中文".into()),
        ]
        .into_iter()
        .map(|(name, bytes)| DerivedFormat {
            item: 0,
            name: name.into(),
            source_name: QQ_MESSAGE_FORMAT.into(),
            converter: CONVERTER_QQ_TO_PORTABLE.into(),
            bytes: bytes.into_bytes(),
        })
        .collect(),
    };
    let database = Arc::new(Mutex::new(
        HistoryDB::new_unavailable_with_policy(MigrationPolicy::CompositeAllowed).unwrap(),
    ));
    let id = database
        .lock()
        .await
        .add_native_archive(&archive, "QQ test")
        .unwrap();
    let request = HistoryPageRequestOwned::new(
        HistoryCollection::All,
        None,
        None,
        None,
        None,
        Some(10),
        Some(0),
        10,
    );
    let page = page_display_async(
        database.clone(),
        request,
        Arc::new(UnsupportedNativeAnalyzer),
    )
    .await
    .unwrap();
    let row = &page.entries[0];
    assert_eq!(row.display.as_ref().unwrap().kind, "document");
    assert!(row.display.as_ref().unwrap().thumbnail_eligible);
    assert!(row.description.contains("QQ 中文"));
    let info = row.composite.as_ref().unwrap();
    assert!(!info.native_only);
    assert_eq!(info.native_item_count, 2);
    assert_eq!(info.image_count, 2);
    let thumbnail = image_payload_checked_async(
        database.clone(),
        id,
        Some(row.display.as_ref().unwrap().cache_key.clone()),
        Arc::new(UnsupportedNativeAnalyzer),
        Cancellation::default(),
    )
    .await
    .expect("QQ document's saved first image must reach the production thumbnail endpoint");
    let packed = tailsync_core::protocol::PackedImage::try_from(thumbnail.data.as_slice()).unwrap();
    assert_eq!(packed.rgba, &[255, 0, 0, 255]);
    let document = analyze_document_cancellable(
        database.clone(),
        id,
        Arc::new(UnsupportedNativeAnalyzer),
        Cancellation::default(),
    )
    .await
    .unwrap();
    let facts = &*document;
    assert_eq!(facts.images.len(), 2);
    assert_eq!(facts.items.len(), 2);
    assert!(!facts.unknown);
    assert!(!facts
        .partial
        .iter()
        .any(|reason| reason.code == "unverified_provenance"));
    assert_eq!(
        database
            .lock()
            .await
            .get_composite_archive(id)
            .unwrap()
            .unwrap(),
        archive
    );
    let mut unproven = facts.clone();
    for representation in &mut unproven.representations {
        representation.source =
            tailsync_core::composite::content_analysis::RepresentationSource::Original;
    }
    assert_eq!(
        tailsync_core::composite::display::classify(&unproven).kind,
        tailsync_core::composite::display::DisplayKind::Unknown
    );
    for source in [
        tailsync_core::composite::content_analysis::RepresentationSource::Unverified,
        tailsync_core::composite::content_analysis::RepresentationSource::Derived {
            converter: "unknown".into(),
            source_name: QQ_MESSAGE_FORMAT.into(),
        },
        tailsync_core::composite::content_analysis::RepresentationSource::Derived {
            converter: CONVERTER_QQ_TO_PORTABLE.into(),
            source_name: "unrelated".into(),
        },
    ] {
        unproven = facts.clone();
        for representation in &mut unproven.representations {
            representation.source = source.clone();
        }
        assert_eq!(
            tailsync_core::composite::display::classify(&unproven).kind,
            tailsync_core::composite::display::DisplayKind::Unknown
        );
    }
}
