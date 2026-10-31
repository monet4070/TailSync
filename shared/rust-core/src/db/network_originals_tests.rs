use super::*;
use crate::cancellation::Cancellation;
use crate::composite::network_image::fixtures::{JPEG_1X1, PNG_1X1, WEBP_LOSSLESS_1X1};
use crate::composite::network_image::{decode_static_png, NetworkImageFormat, NetworkImageLimits};
use crate::composite::network_originals::NetworkImageOriginal;
use crate::composite::resource_archive::{
    apply_resource_bindings, external_images, ResourceReplacement,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};

fn root() -> PathBuf {
    std::env::temp_dir().join(format!(
        "tailsync-network-originals-{}",
        hex::encode(rand::random::<[u8; 16]>())
    ))
}

fn body(encoded: &str) -> Vec<u8> {
    STANDARD.decode(encoded).unwrap()
}

fn large_jpeg() -> Vec<u8> {
    let mut bytes = body(JPEG_1X1);
    for marker in [0xe2, 0xe3] {
        let mut segment = vec![0xff, marker];
        segment.extend_from_slice(&36002u16.to_be_bytes());
        segment.extend(vec![0x42; 36000]);
        bytes.splice(2..2, segment);
    }
    bytes
}

fn base(count: usize, caption_bytes: usize) -> PortableBundle {
    let mut html = format!("<p>{}</p>", "x".repeat(caption_bytes));
    for index in 0..count {
        html.push_str(&format!("<img src='https://fixture.example/{index}'>"));
    }
    PortableBundle {
        representations: vec![Representation {
            mime: Mime::Html,
            bytes: html.into_bytes(),
        }],
        resources: vec![],
        bindings: vec![],
        completeness: Completeness::ExternalReferences,
    }
}

fn derive(
    base: &PortableBundle,
    bodies: Vec<(NetworkImageFormat, Vec<u8>)>,
) -> (PortableBundle, Vec<NetworkImageOriginal>) {
    let images = external_images(base).unwrap();
    let mut originals = Vec::new();
    let mut replacements = Vec::new();
    let mut hashes = Vec::new();
    for (image, (format, bytes)) in images.iter().zip(bodies) {
        let png = decode_static_png(&bytes, format, &NetworkImageLimits::default())
            .unwrap()
            .png;
        hashes.push(*blake3::hash(&png).as_bytes());
        replacements.push(ResourceReplacement {
            span: image.span,
            png,
        });
        originals.push(NetworkImageOriginal {
            image_index: image.image_index as u16,
            resource_id: 0,
            format,
            bytes,
        });
    }
    let (archived, _) = apply_resource_bindings(base, &replacements).unwrap();
    for (original, hash) in originals.iter_mut().zip(hashes) {
        original.resource_id = archived
            .resources
            .iter()
            .find(|resource| *blake3::hash(&resource.png).as_bytes() == hash)
            .unwrap()
            .id;
    }
    (archived, originals)
}

#[test]
fn enriched_capture_is_atomic_and_rejects_body_or_image_substitution() {
    use crate::composite::resource_archive::ArchiveOutcome;
    use crate::protocol::native_archive::NativeArchive;
    use crate::protocol::native_clipboard::{
        FormatState, NativeFormat, NativeSnapshot, SnapshotState,
    };
    let root = root();
    let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
    let source = base(2, 8);
    let archive = NativeArchive {
        platform: 1,
        enumeration_complete: true,
        original: NativeSnapshot {
            before: 7,
            after: 7,
            items: 1,
            state: SnapshotState::Stable,
            formats: vec![NativeFormat {
                item: 0,
                name: "public.html".into(),
                state: FormatState::Captured,
                bytes: source.representation(Mime::Html).unwrap().to_vec(),
            }],
        },
        derived: vec![],
    };
    let (bundle, originals) = derive(
        &source,
        vec![
            (NetworkImageFormat::Png, body(PNG_1X1)),
            (NetworkImageFormat::Jpeg, body(JPEG_1X1)),
        ],
    );
    let mut outcome = ArchiveOutcome {
        bundle,
        originals,
        archived: 2,
        deduplicated: 0,
        failures: vec![],
    };
    let saved_html = outcome.bundle.representations[0].bytes.clone();
    outcome.bundle.representations[0]
        .bytes
        .extend_from_slice(b"<p>substituted body</p>");
    assert!(db
        .add_native_archive_with_images(&archive, &outcome, "self")
        .is_err());
    outcome.bundle.representations[0].bytes = saved_html;
    let saved_image = outcome.bundle.resources[0].png.clone();
    outcome.bundle.resources[0].png =
        crate::composite::packed_to_png(1, 1, &[0, 255, 0, 255]).unwrap();
    assert!(db
        .add_native_archive_with_images(&archive, &outcome, "self")
        .is_err());
    outcome.bundle.resources[0].png = saved_image;
    db.conn.execute_batch("CREATE TRIGGER reject_capture_originals BEFORE INSERT ON history_parts WHEN NEW.kind='network_originals' BEGIN SELECT RAISE(ABORT, 'injected capture failure'); END;").unwrap();
    assert!(db
        .add_native_archive_with_images(&archive, &outcome, "self")
        .is_err());
    assert!(db.get_all(None, None, 10, 0).unwrap().is_empty());
    db.conn
        .execute_batch("DROP TRIGGER reject_capture_originals")
        .unwrap();
    let id = db
        .add_native_archive_with_images(&archive, &outcome, "self")
        .unwrap();
    assert_eq!(db.get_composite(id).unwrap(), outcome.bundle);
    assert_eq!(db.get_composite_archive(id).unwrap().unwrap(), archive);
    assert_eq!(
        db.get_composite_network_originals(id).unwrap(),
        outcome.originals
    );
    let legacy = db
        .add_native_capture_with_images(&archive.original, &outcome, "self")
        .unwrap();
    assert_eq!(db.get_composite(legacy).unwrap(), outcome.bundle);
    assert_eq!(
        db.get_composite_native(legacy).unwrap().unwrap().formats,
        archive.original.formats
    );
    drop(db);
    let reopened =
        HistoryDB::open_with_policy_for_probe(&root, MigrationPolicy::CompositeAllowed).unwrap();
    assert_eq!(reopened.get_composite(id).unwrap(), outcome.bundle);
    assert_eq!(
        reopened.get_composite_network_originals(id).unwrap(),
        outcome.originals
    );
    drop(reopened);
    std::fs::remove_dir_all(root).unwrap();
}

fn commit(
    database: &mut HistoryDB,
    id: i64,
    archived: &PortableBundle,
    originals: &[NetworkImageOriginal],
) -> HistoryIdentity {
    let identity = database.composite_identity(id).unwrap().unwrap();
    database
        .commit_composite_archive_with_originals(
            id,
            &identity,
            archived,
            originals,
            &Cancellation::default(),
        )
        .unwrap()
        .unwrap()
}

fn payload_files(root: &Path) -> std::collections::BTreeSet<PathBuf> {
    std::fs::read_dir(root.join("image-history"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect()
}

fn derive_source(
    bundle: &PortableBundle,
) -> (
    PortableBundle,
    crate::composite::source_context::SourceProof,
) {
    use crate::composite::source_context;
    let candidates = source_context::source_candidates(bundle)
        .unwrap()
        .into_iter()
        .filter(|candidate| candidate.url.starts_with("blob:"))
        .collect::<Vec<_>>();
    let (origin, nodes) = source_context::select_grant(&candidates).unwrap();
    let target = &candidates[0];
    let packet = source_context::SourcePacket {
        kind: source_context::SOURCE_PACKET_KIND.into(),
        version: 1,
        grant_id: "ab".repeat(32),
        origin: origin.clone(),
        entries: vec![source_context::SourcePacketEntry {
            index: target.index as u16,
            src_sha256: target.src_sha256.clone(),
            png_b64: STANDARD.encode(body(PNG_1X1)),
        }],
    };
    let resolved = source_context::resolve_nodes(bundle, &packet, &origin, &nodes).unwrap();
    let (archived, proof, _) =
        source_context::derive_source_archive(bundle, &resolved, &origin).unwrap();
    (archived, proof)
}

#[test]
fn network_originals_preserve_native_archive_and_its_capture_domain() {
    let root = root();
    let mut database = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
    let input = base(1, 0);
    let native = NativeArchive {
        platform: 1,
        enumeration_complete: true,
        original: NativeSnapshot {
            before: 7,
            after: 7,
            items: 1,
            state: SnapshotState::Stable,
            formats: vec![NativeFormat {
                item: 0,
                name: "public.html".into(),
                state: FormatState::Captured,
                bytes: input.representations[0].bytes.clone(),
            }],
        },
        derived: vec![],
    };
    let id = database.add_native_archive(&native, "local").unwrap();
    let original = database.get_composite(id).unwrap();
    let before = database.composite_identity(id).unwrap().unwrap();
    let (archived, originals) = derive(&original, vec![(NetworkImageFormat::Jpeg, body(JPEG_1X1))]);
    let after = commit(&mut database, id, &archived, &originals);
    assert_eq!(before.content_id, after.content_id);
    assert_eq!(before.dedupe_key, after.dedupe_key);
    assert_eq!(
        database
            .get_composite_archive(id)
            .unwrap()
            .unwrap()
            .encode()
            .unwrap(),
        native.encode().unwrap()
    );
    drop(database);
    let database = HistoryDB::open_at(&root).unwrap();
    assert_eq!(
        database
            .get_composite_archive(id)
            .unwrap()
            .unwrap()
            .encode()
            .unwrap(),
        native.encode().unwrap()
    );
    assert_eq!(
        database.get_composite_network_originals(id).unwrap(),
        originals
    );
    drop(database);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn network_originals_and_source_context_preserve_each_other_in_either_order() {
    for source_first in [false, true] {
        let root = root();
        let mut database = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let mut base = base(0, 0);
        base.representations[0].bytes =
            b"<img src='https://example.com/a'><img src='blob:https://example.com/one'>".to_vec();
        let id = database.add_composite(&base, None, "local").unwrap();
        let original_identity = database.composite_identity(id).unwrap().unwrap();
        let mut current = base.clone();
        if source_first {
            let (archived, proof) = derive_source(&current);
            database
                .commit_composite_source_archive_with_cancellation(
                    id,
                    &original_identity,
                    &archived,
                    &proof,
                    &Cancellation::default(),
                )
                .unwrap()
                .unwrap();
            current = archived;
        }
        let (archived, originals) =
            derive(&current, vec![(NetworkImageFormat::Jpeg, body(JPEG_1X1))]);
        let updated = commit(&mut database, id, &archived, &originals);
        current = archived;
        if !source_first {
            let (archived, proof) = derive_source(&current);
            database
                .commit_composite_source_archive_with_cancellation(
                    id,
                    &updated,
                    &archived,
                    &proof,
                    &Cancellation::default(),
                )
                .unwrap()
                .unwrap();
            current = archived;
        }
        let stored: Vec<u8> = database
            .conn
            .query_row("SELECT data FROM history WHERE id=?1", [id], |row| {
                row.get(0)
            })
            .unwrap();
        let manifest = manifest(&stored).unwrap();
        assert_eq!(manifest.version, 5);
        assert!(manifest.source_info.is_some());
        assert_eq!(
            database.get_composite_archived(id).unwrap().unwrap(),
            current
        );
        assert_eq!(
            database.get_composite_network_originals(id).unwrap(),
            originals
        );
        let input = database
            .prepare_content_analysis(id)
            .unwrap()
            .unwrap()
            .read_input(&Cancellation::default())
            .unwrap();
        assert_eq!(input.bundle, current);
        assert_eq!(database.get_composite(id).unwrap(), base);
        drop(database);
        let database = HistoryDB::open_at(&root).unwrap();
        assert_eq!(
            database.get_composite_archived(id).unwrap().unwrap(),
            current
        );
        assert_eq!(
            database.get_composite_network_originals(id).unwrap(),
            originals
        );
        drop(database);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn network_originals_three_formats_survive_reopen_without_changing_capture() {
    let root = root();
    let mut database = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
    let base = base(3, 0);
    let id = database.add_composite(&base, None, "local").unwrap();
    let identity = database.composite_identity(id).unwrap().unwrap();
    let data_hash: String = database
        .conn
        .query_row("SELECT data_hash FROM history WHERE id=?1", [id], |row| {
            row.get(0)
        })
        .unwrap();
    let (archived, originals) = derive(
        &base,
        vec![
            (NetworkImageFormat::Png, body(PNG_1X1)),
            (NetworkImageFormat::Jpeg, body(JPEG_1X1)),
            (NetworkImageFormat::WebP, body(WEBP_LOSSLESS_1X1)),
        ],
    );
    let updated = commit(&mut database, id, &archived, &originals);
    assert_eq!(updated.capture_id, identity.capture_id);
    assert_eq!(updated.content_id, identity.content_id);
    assert_eq!(updated.dedupe_key, identity.dedupe_key);
    assert_eq!(updated.archive_revision, identity.archive_revision + 1);
    assert_ne!(updated.archive_digest, identity.archive_digest);
    assert_eq!(database.get_composite(id).unwrap(), base);
    assert_eq!(
        database.get_composite_network_originals(id).unwrap(),
        originals
    );
    let stored: Vec<u8> = database
        .conn
        .query_row("SELECT data FROM history WHERE id=?1", [id], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(manifest(&stored).unwrap().version, 5);
    let after_hash: String = database
        .conn
        .query_row("SELECT data_hash FROM history WHERE id=?1", [id], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(data_hash, after_hash);
    drop(database);
    let database = HistoryDB::open_at(&root).unwrap();
    assert_eq!(
        database.get_composite_network_originals(id).unwrap(),
        originals
    );
    assert_eq!(
        database.get_composite_archived(id).unwrap().unwrap(),
        archived
    );
    assert_eq!(database.get_composite(id).unwrap(), base);
    drop(database);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn network_originals_corrupt_shared_file_cannot_be_reused_as_a_successful_save() {
    let root = root();
    let mut database = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
    let first_base = base(1, 1);
    let first_id = database.add_composite(&first_base, None, "local").unwrap();
    let (first_archive, originals) =
        derive(&first_base, vec![(NetworkImageFormat::Jpeg, large_jpeg())]);
    commit(&mut database, first_id, &first_archive, &originals);
    let files = payload_files(&root);
    assert_eq!(files.len(), 1);
    let path = files.iter().next().unwrap();
    let corrupted = crypto::encrypt(b"invalid shared original").unwrap();
    crate::private_fs::write_private_file(path, &corrupted).unwrap();
    let second_base = base(1, 2);
    let second_id = database.add_composite(&second_base, None, "local").unwrap();
    let expected = database.composite_identity(second_id).unwrap().unwrap();
    let (second_archive, second_originals) =
        derive(&second_base, vec![(NetworkImageFormat::Jpeg, large_jpeg())]);
    let result = database.commit_composite_archive_with_originals(
        second_id,
        &expected,
        &second_archive,
        &second_originals,
        &Cancellation::default(),
    );
    assert!(
        result.is_err(),
        "a corrupt retained original was reported as successfully saved"
    );
    assert_eq!(
        database.composite_identity(second_id).unwrap().unwrap(),
        expected
    );
    assert!(database
        .get_composite_archived(second_id)
        .unwrap()
        .is_none());
    assert_eq!(std::fs::read(path).unwrap(), corrupted);
    assert_eq!(payload_files(&root), files);
    drop(database);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn network_originals_partial_completion_and_legacy_commit_preserve_prior_records() {
    let root = root();
    let mut database = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
    let base = base(2, 0);
    let id = database.add_composite(&base, None, "local").unwrap();
    let (partial, first) = derive(&base, vec![(NetworkImageFormat::Png, body(PNG_1X1))]);
    commit(&mut database, id, &partial, &first);
    let (complete, second) = derive(&partial, vec![(NetworkImageFormat::Jpeg, body(JPEG_1X1))]);
    let identity = commit(&mut database, id, &complete, &second);
    let expected = [first, second].concat();
    assert_eq!(
        database.get_composite_network_originals(id).unwrap(),
        expected
    );
    database
        .commit_composite_archive(id, &identity, &complete)
        .unwrap()
        .unwrap();
    assert_eq!(
        database.get_composite_network_originals(id).unwrap(),
        expected
    );
    assert_eq!(database.get_composite(id).unwrap(), base);
    drop(database);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn network_originals_sql_failure_rolls_back_both_parts_and_cleans_payloads() {
    let root = root();
    let mut database = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
    let base = base(1, 70000);
    let id = database.add_composite(&base, None, "local").unwrap();
    let identity = database.composite_identity(id).unwrap().unwrap();
    let (archived, originals) = derive(&base, vec![(NetworkImageFormat::Jpeg, large_jpeg())]);
    let before = payload_files(&root);
    database.conn.execute_batch("CREATE TRIGGER network_original_failure BEFORE INSERT ON history_parts WHEN NEW.kind='network_originals' BEGIN SELECT RAISE(ABORT, 'original write refused'); END;").unwrap();
    assert!(database
        .commit_composite_archive_with_originals(
            id,
            &identity,
            &archived,
            &originals,
            &Cancellation::default()
        )
        .is_err());
    assert_eq!(database.composite_identity(id).unwrap().unwrap(), identity);
    assert_eq!(database.get_composite(id).unwrap(), base);
    assert!(database.get_composite_archived(id).unwrap().is_none());
    assert!(database
        .get_composite_network_originals(id)
        .unwrap()
        .is_empty());
    assert_eq!(payload_files(&root), before);
    drop(database);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn network_originals_quota_includes_original_bytes_before_any_write() {
    let root = root();
    let mut database = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
    let base = base(1, 0);
    let id = database.add_composite(&base, None, "local").unwrap();
    let identity = database.composite_identity(id).unwrap().unwrap();
    let (archived, originals) = derive(&base, vec![(NetworkImageFormat::Jpeg, large_jpeg())]);
    let used = database.storage_status().used_bytes;
    database.storage_quota_bytes = used + archived.encode().unwrap().len() as u64 + 28 + 4096;
    let before = payload_files(&root);
    let error = database
        .commit_composite_archive_with_originals(
            id,
            &identity,
            &archived,
            &originals,
            &Cancellation::default(),
        )
        .unwrap_err();
    assert!(error.to_string().contains("Storage quota"), "{error}");
    assert_eq!(database.composite_identity(id).unwrap().unwrap(), identity);
    assert_eq!(payload_files(&root), before);
    assert!(database.get_composite_archived(id).unwrap().is_none());
    drop(database);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn network_originals_corruption_is_explicit_and_not_loaded_for_preview() {
    let root = root();
    let mut database = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
    let base = base(1, 0);
    let id = database.add_composite(&base, None, "local").unwrap();
    let (archived, originals) = derive(&base, vec![(NetworkImageFormat::Png, body(PNG_1X1))]);
    commit(&mut database, id, &archived, &originals);
    let stored = crypto::encrypt(b"corrupt original container").unwrap();
    database
        .conn
        .execute(
            "UPDATE history_parts SET stored=?1 WHERE history_id=?2 AND kind='network_originals'",
            params![stored, id],
        )
        .unwrap();
    assert!(database.get_composite_network_originals(id).is_err());
    assert_eq!(
        database.get_composite_archived(id).unwrap().unwrap(),
        archived
    );
    let input = database
        .prepare_content_analysis(id)
        .unwrap()
        .unwrap()
        .read_input(&Cancellation::default())
        .unwrap();
    assert_eq!(input.bundle, archived);
    drop(database);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn network_originals_cancellation_stale_identity_and_duplicate_input_never_commit() {
    let root = root();
    let mut database = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
    let base = base(1, 0);
    let id = database.add_composite(&base, None, "local").unwrap();
    let identity = database.composite_identity(id).unwrap().unwrap();
    let (archived, originals) = derive(&base, vec![(NetworkImageFormat::Png, body(PNG_1X1))]);
    let cancelled = Cancellation::default();
    cancelled.cancel();
    assert!(database
        .commit_composite_archive_with_originals(id, &identity, &archived, &originals, &cancelled)
        .is_err());
    let duplicated = [originals.clone(), originals.clone()].concat();
    assert!(database
        .commit_composite_archive_with_originals(
            id,
            &identity,
            &archived,
            &duplicated,
            &Cancellation::default()
        )
        .is_err());
    let updated = commit(&mut database, id, &archived, &originals);
    assert!(database
        .commit_composite_archive_with_originals(
            id,
            &identity,
            &archived,
            &originals,
            &Cancellation::default()
        )
        .unwrap()
        .is_none());
    assert_eq!(database.composite_identity(id).unwrap().unwrap(), updated);
    database.delete(id).unwrap();
    assert!(database
        .commit_composite_archive_with_originals(
            id,
            &updated,
            &archived,
            &originals,
            &Cancellation::default()
        )
        .unwrap()
        .is_none());
    drop(database);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn network_originals_encrypted_external_payload_is_reclaimed_with_history() {
    let root = root();
    let mut database = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
    let base = base(1, 0);
    let id = database.add_composite(&base, None, "local").unwrap();
    let (archived, originals) = derive(&base, vec![(NetworkImageFormat::Jpeg, large_jpeg())]);
    commit(&mut database, id, &archived, &originals);
    let files = payload_files(&root);
    assert_eq!(files.len(), 1);
    for path in &files {
        let encrypted = std::fs::read(path).unwrap();
        assert!(!encrypted
            .windows(originals[0].bytes.len())
            .any(|window| window == originals[0].bytes));
    }
    assert_eq!(
        database.get_composite_network_originals(id).unwrap(),
        originals
    );
    database.set_favorite(id, true).unwrap();
    database.clear_all().unwrap();
    assert_eq!(payload_files(&root), files);
    assert_eq!(
        database.get_composite_network_originals(id).unwrap(),
        originals
    );
    database.delete_favorite(id).unwrap();
    assert!(payload_files(&root).is_empty());
    drop(database);
    std::fs::remove_dir_all(root).unwrap();
}
