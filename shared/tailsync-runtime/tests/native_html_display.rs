use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::sync::Arc;
use tailsync_core::{
    cancellation::Cancellation,
    composite::packed_to_png,
    db::{HistoryDB, MigrationPolicy},
    protocol::{native_archive::NativeArchive, native_clipboard::*},
};
use tailsync_runtime::content_analysis::{analyze_document_cancellable, UnsupportedNativeAnalyzer};
use tokio::sync::Mutex;

#[tokio::test]
async fn saved_native_html_document_keeps_image_text_and_verified_provenance() {
    let archive = NativeArchive {
        platform: 1,
        enumeration_complete: true,
        original: NativeSnapshot {
            before: 1,
            after: 1,
            items: 1,
            state: SnapshotState::Stable,
            formats: vec![NativeFormat {
                item: 0,
                name: "public.html".into(),
                state: FormatState::Captured,
                bytes: format!(
                    "<img src='data:image/png;base64,{}'><p>截图和文字 🙂</p>",
                    STANDARD.encode(packed_to_png(1, 1, &[255, 0, 0, 255]).unwrap())
                )
                .into_bytes(),
            }],
        },
        derived: vec![],
    };
    let database = Arc::new(Mutex::new(
        HistoryDB::new_unavailable_with_policy(MigrationPolicy::CompositeAllowed).unwrap(),
    ));
    let id = database
        .lock()
        .await
        .add_native_archive(&archive, "native HTML test")
        .unwrap();
    let identity = database.lock().await.composite_identity(id).unwrap();
    let analysis = analyze_document_cancellable(
        database.clone(),
        id,
        Arc::new(UnsupportedNativeAnalyzer),
        Cancellation::default(),
    )
    .await
    .unwrap();
    assert!(analysis.text.text.contains("截图和文字 🙂"));
    assert_eq!(analysis.images.len(), 1);
    assert!(!analysis
        .partial
        .iter()
        .any(|reason| reason.code == "unverified_provenance"));
    let document = analysis.document().unwrap();
    assert_eq!(document.assets.len(), 1);
    assert!(document.partial.is_empty());
    assert_eq!(
        database
            .lock()
            .await
            .get_composite_archive(id)
            .unwrap()
            .unwrap(),
        archive
    );
    assert_eq!(
        database.lock().await.composite_identity(id).unwrap(),
        identity
    );
}
