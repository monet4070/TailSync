use super::*;
use crate::cancellation::Cancellation;
use crate::composite::archive_sync::{ArchiveResourceUpdate, ArchiveUpdate};
use crate::composite::network_image::fixtures::{JPEG_1X1, PNG_1X1, WEBP_LOSSLESS_1X1};
use crate::composite::network_image::{decode_static_png, NetworkImageFormat, NetworkImageLimits};
use crate::composite::network_originals::NetworkImageOriginal;
use crate::composite::resource_archive::{
    apply_resource_bindings, external_images, ResourceReplacement,
};
use crate::protocol::MessageId;
use base64::{engine::general_purpose::STANDARD, Engine as _};

fn root() -> PathBuf {
    std::env::temp_dir().join(format!(
        "tailsync-archive-sync-{}",
        hex::encode(rand::random::<[u8; 16]>())
    ))
}

fn body(encoded: &str) -> Vec<u8> {
    STANDARD.decode(encoded).unwrap()
}

fn base(count: usize) -> PortableBundle {
    let mut html = String::from("<p>archive</p>");
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

fn local_archive(
    base: &PortableBundle,
    bodies: &[(NetworkImageFormat, Vec<u8>)],
) -> (PortableBundle, Vec<NetworkImageOriginal>) {
    let images = external_images(base).unwrap();
    let mut originals = Vec::new();
    let mut replacements = Vec::new();
    let mut hashes = Vec::new();
    for (image, (format, bytes)) in images.iter().zip(bodies.iter()) {
        let png = decode_static_png(bytes, *format, &NetworkImageLimits::default())
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
            format: *format,
            bytes: bytes.clone(),
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

fn update(
    base: &PortableBundle,
    event: MessageId,
    revision: u64,
    images: &[(u16, NetworkImageFormat, Vec<u8>)],
) -> ArchiveUpdate {
    ArchiveUpdate {
        base_event: event,
        base_digest: base.digest().unwrap(),
        revision,
        images: images
            .iter()
            .map(|(index, format, bytes)| ArchiveResourceUpdate {
                image_index: *index,
                format: *format,
                bytes: bytes.clone(),
            })
            .collect(),
    }
}

fn sender(root: &Path, base: &PortableBundle, event: MessageId) -> (HistoryDB, i64) {
    let mut database = HistoryDB::open_isolated_for_test_composite(root).unwrap();
    let id = database.add_composite(base, None, "self").unwrap();
    let identity = database.composite_identity(id).unwrap().unwrap();
    let digest = database.get_composite(id).unwrap().digest().unwrap();
    assert!(database
        .bind_composite_outgoing_event(
            id,
            &identity,
            event,
            digest,
            crate::protocol::unix_timestamp_ms()
        )
        .unwrap());
    (database, id)
}

fn receiver(
    root: &Path,
    source: &str,
    event: MessageId,
    base: &PortableBundle,
) -> (HistoryDB, i64) {
    let mut database = HistoryDB::open_isolated_for_test_composite(root).unwrap();
    let id = database
        .add_composite_event(
            base,
            None,
            "peer",
            Some((source, event, crate::protocol::unix_timestamp_ms())),
        )
        .unwrap();
    (database, id)
}

fn tracker_rows(database: &HistoryDB) -> i64 {
    database
        .conn
        .query_row(
            "SELECT COUNT(*) FROM history_composite_events WHERE event_id LIKE 'archive:%'",
            [],
            |r| r.get(0),
        )
        .unwrap()
}

fn binding_rows(database: &HistoryDB) -> i64 {
    database
        .conn
        .query_row(
            "SELECT COUNT(*) FROM history_composite_events WHERE source_device_id=?1",
            params![LOCAL_ARCHIVE_EVENT_SOURCE],
            |r| r.get(0),
        )
        .unwrap()
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
fn local_binding_export_and_remote_apply() {
    let sender_root = root();
    let receiver_root = root();
    let base = base(2);
    let event = MessageId([7; 16]);
    let (mut sender, id) = sender(&sender_root, &base, event);
    let identity = sender.composite_identity(id).unwrap().unwrap();
    let (archived, originals) = local_archive(
        &base,
        &[
            (NetworkImageFormat::Jpeg, body(JPEG_1X1)),
            (NetworkImageFormat::WebP, body(WEBP_LOSSLESS_1X1)),
        ],
    );
    sender
        .commit_composite_archive_with_originals(
            id,
            &identity,
            &archived,
            &originals,
            &Cancellation::default(),
        )
        .unwrap()
        .unwrap();
    let updated = sender.composite_identity(id).unwrap().unwrap();
    let update = sender
        .export_composite_archive_update(id, &updated)
        .unwrap()
        .unwrap();
    assert_eq!(update.base_event, event);
    assert_eq!(update.base_digest, base.digest().unwrap());
    assert_eq!(update.revision, 2);
    assert_eq!(update.images.len(), 2);
    assert_eq!(update.images[0].image_index, 0);
    assert_eq!(update.images[0].bytes, body(JPEG_1X1));
    assert_eq!(update.images[1].image_index, 1);
    assert_eq!(update.images[1].bytes, body(WEBP_LOSSLESS_1X1));

    let (mut receiver, remote_id) = receiver(&receiver_root, "peer-a", event, &base);
    assert_eq!(
        receiver
            .commit_remote_composite_archive_update("peer-a", &update, &Cancellation::default())
            .unwrap(),
        RemoteArchiveOutcome::Applied(remote_id)
    );
    assert_eq!(
        receiver.get_composite_network_originals(remote_id).unwrap(),
        originals
    );
    assert_eq!(
        receiver
            .composite_identity(remote_id)
            .unwrap()
            .unwrap()
            .archive_revision,
        2
    );
    assert_eq!(receiver.get_composite(remote_id).unwrap(), base);
    assert_eq!(
        receiver
            .commit_remote_composite_archive_update("peer-a", &update, &Cancellation::default())
            .unwrap(),
        RemoteArchiveOutcome::Duplicate(remote_id)
    );
    assert_eq!(
        receiver
            .composite_identity(remote_id)
            .unwrap()
            .unwrap()
            .archive_revision,
        2
    );
    drop(receiver);
    drop(sender);
    std::fs::remove_dir_all(sender_root).unwrap();
    std::fs::remove_dir_all(receiver_root).unwrap();
}

#[test]
fn base_missing_for_unknown_source_or_event_and_no_history_is_created() {
    let receiver_root = root();
    let base = base(1);
    let event = MessageId([41; 16]);
    let (mut receiver, _id) = receiver(&receiver_root, "peer-a", event, &base);
    let update = update(
        &base,
        event,
        2,
        &[(0, NetworkImageFormat::Jpeg, body(JPEG_1X1))],
    );
    let before: i64 = receiver
        .conn
        .query_row("SELECT COUNT(*) FROM history", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        receiver
            .commit_remote_composite_archive_update("peer-b", &update, &Cancellation::default())
            .unwrap(),
        RemoteArchiveOutcome::BaseMissing
    );
    let mut unknown = update.clone();
    unknown.base_event = MessageId([42; 16]);
    assert_eq!(
        receiver
            .commit_remote_composite_archive_update("peer-a", &unknown, &Cancellation::default())
            .unwrap(),
        RemoteArchiveOutcome::BaseMissing
    );
    let after: i64 = receiver
        .conn
        .query_row("SELECT COUNT(*) FROM history", [], |r| r.get(0))
        .unwrap();
    assert_eq!(before, after);
    assert_eq!(tracker_rows(&receiver), 0);
    drop(receiver);
    std::fs::remove_dir_all(receiver_root).unwrap();
}

#[test]
fn deleted_base_is_superseded_without_creating_history() {
    let receiver_root = root();
    let base = base(1);
    let event = MessageId([37; 16]);
    let (mut receiver, id) = receiver(&receiver_root, "peer-a", event, &base);
    let update = update(
        &base,
        event,
        2,
        &[(0, NetworkImageFormat::Jpeg, body(JPEG_1X1))],
    );
    receiver.delete(id).unwrap();
    let before: i64 = receiver
        .conn
        .query_row("SELECT COUNT(*) FROM history", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        receiver
            .commit_remote_composite_archive_update("peer-a", &update, &Cancellation::default())
            .unwrap(),
        RemoteArchiveOutcome::Superseded
    );
    let after: i64 = receiver
        .conn
        .query_row("SELECT COUNT(*) FROM history", [], |r| r.get(0))
        .unwrap();
    assert_eq!(before, after);
    assert_eq!(tracker_rows(&receiver), 0);
    assert_eq!(binding_rows(&receiver), 0);
    drop(receiver);
    std::fs::remove_dir_all(receiver_root).unwrap();
}

#[test]
fn same_revision_with_different_payload_is_a_collision() {
    let receiver_root = root();
    let base = base(1);
    let event = MessageId([53; 16]);
    let (mut receiver, id) = receiver(&receiver_root, "peer-a", event, &base);
    let first = update(
        &base,
        event,
        2,
        &[(0, NetworkImageFormat::Jpeg, body(JPEG_1X1))],
    );
    assert_eq!(
        receiver
            .commit_remote_composite_archive_update("peer-a", &first, &Cancellation::default())
            .unwrap(),
        RemoteArchiveOutcome::Applied(id)
    );
    let collision = update(
        &base,
        event,
        2,
        &[(0, NetworkImageFormat::WebP, body(WEBP_LOSSLESS_1X1))],
    );
    assert!(receiver
        .commit_remote_composite_archive_update("peer-a", &collision, &Cancellation::default())
        .is_err());
    assert_eq!(tracker_rows(&receiver), 1);
    assert_eq!(
        receiver
            .composite_identity(id)
            .unwrap()
            .unwrap()
            .archive_revision,
        2
    );
    drop(receiver);
    std::fs::remove_dir_all(receiver_root).unwrap();
}

#[test]
fn lower_revision_is_superseded() {
    let receiver_root = root();
    let base = base(1);
    let event = MessageId([59; 16]);
    let (mut receiver, id) = receiver(&receiver_root, "peer-a", event, &base);
    let newer = update(
        &base,
        event,
        3,
        &[(0, NetworkImageFormat::Jpeg, body(JPEG_1X1))],
    );
    assert_eq!(
        receiver
            .commit_remote_composite_archive_update("peer-a", &newer, &Cancellation::default())
            .unwrap(),
        RemoteArchiveOutcome::Applied(id)
    );
    let older = update(
        &base,
        event,
        2,
        &[(0, NetworkImageFormat::Jpeg, body(JPEG_1X1))],
    );
    assert_eq!(
        receiver
            .commit_remote_composite_archive_update("peer-a", &older, &Cancellation::default())
            .unwrap(),
        RemoteArchiveOutcome::Superseded
    );
    assert_eq!(
        receiver
            .composite_identity(id)
            .unwrap()
            .unwrap()
            .archive_revision,
        2
    );
    assert_eq!(
        receiver.get_composite_network_originals(id).unwrap().len(),
        1
    );
    drop(receiver);
    std::fs::remove_dir_all(receiver_root).unwrap();
}

#[test]
fn partial_completion_preserves_existing_original_and_png() {
    let receiver_root = root();
    let base = base(2);
    let event = MessageId([61; 16]);
    let (mut receiver, id) = receiver(&receiver_root, "peer-a", event, &base);
    let first = update(
        &base,
        event,
        2,
        &[(0, NetworkImageFormat::Jpeg, body(JPEG_1X1))],
    );
    assert_eq!(
        receiver
            .commit_remote_composite_archive_update("peer-a", &first, &Cancellation::default())
            .unwrap(),
        RemoteArchiveOutcome::Applied(id)
    );
    let after_first = receiver.get_composite_network_originals(id).unwrap();
    assert_eq!(after_first.len(), 1);
    let second = update(
        &base,
        event,
        3,
        &[
            (0, NetworkImageFormat::Jpeg, body(JPEG_1X1)),
            (1, NetworkImageFormat::WebP, body(WEBP_LOSSLESS_1X1)),
        ],
    );
    assert_eq!(
        receiver
            .commit_remote_composite_archive_update("peer-a", &second, &Cancellation::default())
            .unwrap(),
        RemoteArchiveOutcome::Applied(id)
    );
    let after_second = receiver.get_composite_network_originals(id).unwrap();
    assert_eq!(after_second.len(), 2);
    assert_eq!(after_second[0], after_first[0]);
    assert_eq!(after_second[1].image_index, 1);
    assert_eq!(after_second[1].bytes, body(WEBP_LOSSLESS_1X1));
    assert_eq!(
        receiver
            .composite_identity(id)
            .unwrap()
            .unwrap()
            .archive_revision,
        3
    );
    drop(receiver);
    std::fs::remove_dir_all(receiver_root).unwrap();
}

#[test]
fn conflicting_bound_position_is_rejected() {
    let receiver_root = root();
    let base = base(1);
    let event = MessageId([67; 16]);
    let (mut receiver, id) = receiver(&receiver_root, "peer-a", event, &base);
    let first = update(
        &base,
        event,
        2,
        &[(0, NetworkImageFormat::Jpeg, body(JPEG_1X1))],
    );
    assert_eq!(
        receiver
            .commit_remote_composite_archive_update("peer-a", &first, &Cancellation::default())
            .unwrap(),
        RemoteArchiveOutcome::Applied(id)
    );
    let conflict = update(
        &base,
        event,
        3,
        &[(0, NetworkImageFormat::WebP, body(WEBP_LOSSLESS_1X1))],
    );
    assert!(receiver
        .commit_remote_composite_archive_update("peer-a", &conflict, &Cancellation::default())
        .is_err());
    assert_eq!(
        receiver
            .composite_identity(id)
            .unwrap()
            .unwrap()
            .archive_revision,
        2
    );
    assert_eq!(
        receiver.get_composite_network_originals(id).unwrap()[0].bytes,
        body(JPEG_1X1)
    );
    drop(receiver);
    std::fs::remove_dir_all(receiver_root).unwrap();
}

#[test]
fn binding_and_tracker_survive_restart() {
    let sender_root = root();
    let receiver_root = root();
    let base = base(1);
    let event = MessageId([43; 16]);
    let (mut sender, id) = sender(&sender_root, &base, event);
    let identity = sender.composite_identity(id).unwrap().unwrap();
    let (archived, originals) = local_archive(&base, &[(NetworkImageFormat::Jpeg, body(JPEG_1X1))]);
    sender
        .commit_composite_archive_with_originals(
            id,
            &identity,
            &archived,
            &originals,
            &Cancellation::default(),
        )
        .unwrap()
        .unwrap();
    let updated = sender.composite_identity(id).unwrap().unwrap();
    let update = sender
        .export_composite_archive_update(id, &updated)
        .unwrap()
        .unwrap();
    drop(sender);
    let sender = HistoryDB::open_at(&sender_root).unwrap();
    let reloaded = sender.composite_identity(id).unwrap().unwrap();
    assert_eq!(reloaded, updated);
    assert_eq!(
        sender
            .export_composite_archive_update(id, &reloaded)
            .unwrap(),
        Some(update.clone())
    );
    drop(sender);

    let (mut receiver, remote_id) = receiver(&receiver_root, "peer-a", event, &base);
    assert_eq!(
        receiver
            .commit_remote_composite_archive_update("peer-a", &update, &Cancellation::default())
            .unwrap(),
        RemoteArchiveOutcome::Applied(remote_id)
    );
    drop(receiver);
    let mut receiver = HistoryDB::open_at(&receiver_root).unwrap();
    assert_eq!(
        receiver
            .commit_remote_composite_archive_update("peer-a", &update, &Cancellation::default())
            .unwrap(),
        RemoteArchiveOutcome::Duplicate(remote_id)
    );
    assert_eq!(
        receiver
            .get_composite_network_originals(remote_id)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(tracker_rows(&receiver), 1);
    drop(receiver);
    std::fs::remove_dir_all(sender_root).unwrap();
    std::fs::remove_dir_all(receiver_root).unwrap();
}

#[test]
fn source_proof_is_preserved_by_remote_completion() {
    let receiver_root = root();
    let mut base = base(1);
    base.representations[0].bytes =
        b"<img src='blob:https://fixture.example/one'><img src='https://fixture.example/two'>"
            .to_vec();
    let event = MessageId([47; 16]);
    let (mut receiver, id) = receiver(&receiver_root, "peer-a", event, &base);
    let identity = receiver.composite_identity(id).unwrap().unwrap();
    let (source_archive, proof) = derive_source(&base);
    receiver
        .commit_composite_source_archive_with_cancellation(
            id,
            &identity,
            &source_archive,
            &proof,
            &Cancellation::default(),
        )
        .unwrap()
        .unwrap();
    let update = update(
        &base,
        event,
        2,
        &[(1, NetworkImageFormat::Jpeg, body(JPEG_1X1))],
    );
    assert_eq!(
        receiver
            .commit_remote_composite_archive_update("peer-a", &update, &Cancellation::default())
            .unwrap(),
        RemoteArchiveOutcome::Applied(id)
    );
    let stored: Vec<u8> = receiver
        .conn
        .query_row("SELECT data FROM history WHERE id=?1", [id], |r| r.get(0))
        .unwrap();
    assert!(manifest(&stored).unwrap().source_info.is_some());
    assert!(receiver.get_composite_archived(id).unwrap().is_some());
    assert_eq!(
        receiver.get_composite_network_originals(id).unwrap().len(),
        1
    );
    drop(receiver);
    std::fs::remove_dir_all(receiver_root).unwrap();
}

#[test]
fn sql_failure_rolls_back_layer_and_tracker() {
    let receiver_root = root();
    let base = base(1);
    let event = MessageId([13; 16]);
    let (mut receiver, id) = receiver(&receiver_root, "peer-a", event, &base);
    let before = receiver.composite_identity(id).unwrap().unwrap();
    receiver
        .conn
        .execute_batch(
            "CREATE TRIGGER reject_archive_part BEFORE INSERT ON history_parts
             BEGIN SELECT RAISE(ABORT,'injected archive failure'); END;",
        )
        .unwrap();
    let update = update(
        &base,
        event,
        2,
        &[(0, NetworkImageFormat::Jpeg, body(JPEG_1X1))],
    );
    assert!(receiver
        .commit_remote_composite_archive_update("peer-a", &update, &Cancellation::default())
        .is_err());
    assert_eq!(receiver.composite_identity(id).unwrap().unwrap(), before);
    assert_eq!(receiver.get_composite_archived(id).unwrap(), None);
    assert_eq!(tracker_rows(&receiver), 0);
    receiver
        .conn
        .execute_batch("DROP TRIGGER reject_archive_part;")
        .unwrap();
    assert_eq!(
        receiver
            .commit_remote_composite_archive_update("peer-a", &update, &Cancellation::default())
            .unwrap(),
        RemoteArchiveOutcome::Applied(id)
    );
    assert_eq!(tracker_rows(&receiver), 1);
    drop(receiver);
    std::fs::remove_dir_all(receiver_root).unwrap();
}

#[test]
fn quota_and_cancellation_change_nothing() {
    let receiver_root = root();
    let base = base(1);
    let event = MessageId([17; 16]);
    let (mut receiver, id) = receiver(&receiver_root, "peer-a", event, &base);
    let before = receiver.composite_identity(id).unwrap().unwrap();
    let update = update(
        &base,
        event,
        2,
        &[(0, NetworkImageFormat::Jpeg, body(JPEG_1X1))],
    );
    receiver.set_storage_quota(1);
    assert!(receiver
        .commit_remote_composite_archive_update("peer-a", &update, &Cancellation::default())
        .is_err());
    assert_eq!(receiver.composite_identity(id).unwrap().unwrap(), before);
    assert_eq!(tracker_rows(&receiver), 0);
    receiver.set_storage_quota(u64::MAX);
    let cancelled = Cancellation::default();
    cancelled.cancel();
    assert!(receiver
        .commit_remote_composite_archive_update("peer-a", &update, &cancelled)
        .is_err());
    assert_eq!(receiver.composite_identity(id).unwrap().unwrap(), before);
    assert_eq!(tracker_rows(&receiver), 0);
    assert_eq!(receiver.get_composite_archived(id).unwrap(), None);
    drop(receiver);
    std::fs::remove_dir_all(receiver_root).unwrap();
}

#[test]
fn live_binding_survives_prune_but_expired_tombstone_is_collected() {
    let sender_root = root();
    let base = base(1);
    let event = MessageId([19; 16]);
    let mut database = HistoryDB::open_isolated_for_test_composite(&sender_root).unwrap();
    let id = database.add_composite(&base, None, "self").unwrap();
    let identity = database.composite_identity(id).unwrap().unwrap();
    let digest = database.get_composite(id).unwrap().digest().unwrap();
    let expired =
        crate::protocol::unix_timestamp_ms() - crate::protocol::EVENT_TIMESTAMP_WINDOW_MS * 3;
    assert!(database
        .bind_composite_outgoing_event(id, &identity, event, digest, expired)
        .unwrap());
    database.prune_composite_tombstones().unwrap();
    assert_eq!(binding_rows(&database), 1);
    assert!(database
        .export_composite_archive_update(id, &identity)
        .unwrap()
        .is_none());
    database.delete(id).unwrap();
    database.prune_composite_tombstones().unwrap();
    assert_eq!(binding_rows(&database), 0);
    drop(database);
    std::fs::remove_dir_all(sender_root).unwrap();
}

#[test]
fn metadata_capacity_is_not_relaxed() {
    let sender_root = root();
    let base = base(1);
    let mut database = HistoryDB::open_isolated_for_test_composite(&sender_root).unwrap();
    let id = database.add_composite(&base, None, "self").unwrap();
    let identity = database.composite_identity(id).unwrap().unwrap();
    let digest = database.get_composite(id).unwrap().digest().unwrap();
    let now = crate::protocol::unix_timestamp_ms();
    {
        let tx = database.conn.transaction().unwrap();
        for index in 0..COMPOSITE_EVENT_METADATA_LIMIT {
            tx.execute(
                "INSERT INTO history_composite_events(source_device_id,event_id,digest,history_id,timestamp_ms) VALUES (?1,?2,?3,NULL,?4)",
                params![
                    format!("source-{index}"),
                    format!("event-{index}"),
                    "digest",
                    now
                ],
            )
            .unwrap();
        }
        tx.commit().unwrap();
    }
    assert!(database
        .bind_composite_outgoing_event(id, &identity, MessageId([23; 16]), digest, now)
        .is_err());
    assert_eq!(
        database
            .conn
            .query_row("SELECT COUNT(*) FROM history_composite_events", [], |r| r
                .get::<_, i64>(
                0
            ))
            .unwrap(),
        COMPOSITE_EVENT_METADATA_LIMIT
    );
    drop(database);
    std::fs::remove_dir_all(sender_root).unwrap();
}

#[test]
fn source_identity_and_event_bounds_are_enforced() {
    let receiver_root = root();
    let base = base(1);
    let event = MessageId([29; 16]);
    let (mut receiver, id) = receiver(&receiver_root, "peer-a", event, &base);
    let update = update(
        &base,
        event,
        2,
        &[(0, NetworkImageFormat::Jpeg, body(JPEG_1X1))],
    );
    let long = "x".repeat(ARCHIVE_SOURCE_IDENTITY_LIMIT + 1);
    for source in ["", long.as_str(), LOCAL_ARCHIVE_EVENT_SOURCE] {
        assert!(receiver
            .commit_remote_composite_archive_update(source, &update, &Cancellation::default())
            .is_err());
    }
    assert_eq!(
        receiver
            .commit_remote_composite_archive_update("peer-b", &update, &Cancellation::default())
            .unwrap(),
        RemoteArchiveOutcome::BaseMissing
    );
    let mut zero = update.clone();
    zero.base_event = MessageId([0; 16]);
    assert!(receiver
        .commit_remote_composite_archive_update("peer-a", &zero, &Cancellation::default())
        .is_err());
    let mut low = update.clone();
    low.revision = 1;
    assert!(receiver
        .commit_remote_composite_archive_update("peer-a", &low, &Cancellation::default())
        .is_err());
    let mut wrong_digest = update.clone();
    wrong_digest.base_digest = [9; 32];
    assert!(receiver
        .commit_remote_composite_archive_update("peer-a", &wrong_digest, &Cancellation::default())
        .is_err());
    assert_eq!(tracker_rows(&receiver), 0);
    assert_eq!(
        receiver
            .composite_identity(id)
            .unwrap()
            .unwrap()
            .archive_revision,
        1
    );
    drop(receiver);
    std::fs::remove_dir_all(receiver_root).unwrap();
}

#[test]
fn export_ignores_remote_received_rows() {
    let receiver_root = root();
    let base = base(1);
    let event = MessageId([31; 16]);
    let (database, id) = receiver(&receiver_root, "peer-a", event, &base);
    let identity = database.composite_identity(id).unwrap().unwrap();
    assert!(database
        .export_composite_archive_update(id, &identity)
        .unwrap()
        .is_none());
    drop(database);
    std::fs::remove_dir_all(receiver_root).unwrap();
}

#[test]
fn incoming_rows_cannot_be_rebound_as_local_archive_origins() {
    let receiver_root = root();
    let base = base(1);
    let (mut database, id) = receiver(&receiver_root, "peer-a", MessageId([61; 16]), &base);
    let identity = database.composite_identity(id).unwrap().unwrap();
    assert!(!database
        .bind_composite_outgoing_event(
            id,
            &identity,
            MessageId([63; 16]),
            base.digest().unwrap(),
            crate::protocol::unix_timestamp_ms()
        )
        .unwrap());
    drop(database);
    std::fs::remove_dir_all(receiver_root).unwrap();
}

#[test]
fn tracker_only_commit_rechecks_history_identity_and_rolls_back() {
    let receiver_root = root();
    let base = base(1);
    let event = MessageId([53; 16]);
    let (mut receiver, id) = receiver(&receiver_root, "peer-a", event, &base);
    let initial = update(
        &base,
        event,
        2,
        &[(0, NetworkImageFormat::Jpeg, body(JPEG_1X1))],
    );
    receiver
        .commit_remote_composite_archive_update("peer-a", &initial, &Cancellation::default())
        .unwrap();
    let before = receiver.composite_identity(id).unwrap().unwrap();
    receiver.conn.execute_batch("CREATE TRIGGER mutate_archive_identity BEFORE UPDATE ON history_composite_events WHEN NEW.event_id LIKE 'archive:%' BEGIN UPDATE history_composite_identity SET archive_revision=archive_revision+1 WHERE history_id=NEW.history_id; END;").unwrap();
    let mut newer = initial;
    newer.revision = 3;
    assert!(receiver
        .commit_remote_composite_archive_update("peer-a", &newer, &Cancellation::default())
        .is_err());
    assert_eq!(receiver.composite_identity(id).unwrap().unwrap(), before);
    receiver
        .conn
        .execute_batch("DROP TRIGGER mutate_archive_identity;")
        .unwrap();
    assert_eq!(
        receiver
            .commit_remote_composite_archive_update("peer-a", &newer, &Cancellation::default())
            .unwrap(),
        RemoteArchiveOutcome::Duplicate(id)
    );
    drop(receiver);
    std::fs::remove_dir_all(receiver_root).unwrap();
}

#[test]
fn duplicate_archive_does_not_acknowledge_corrupt_saved_originals() {
    let receiver_root = root();
    let base = base(1);
    let event = MessageId([59; 16]);
    let (mut receiver, id) = receiver(&receiver_root, "peer-a", event, &base);
    let update = update(
        &base,
        event,
        2,
        &[(0, NetworkImageFormat::Jpeg, body(JPEG_1X1))],
    );
    receiver
        .commit_remote_composite_archive_update("peer-a", &update, &Cancellation::default())
        .unwrap();
    receiver.conn.execute("UPDATE history_parts SET stored=X'00' WHERE history_id=?1 AND kind='network_originals'", [id]).unwrap();
    assert!(receiver
        .commit_remote_composite_archive_update("peer-a", &update, &Cancellation::default())
        .is_err());
    drop(receiver);
    std::fs::remove_dir_all(receiver_root).unwrap();
}
