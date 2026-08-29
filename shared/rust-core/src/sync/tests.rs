use super::{
    normalize_transferred_file_name, prepare_file_batch, validate_incoming_file_meta,
    verify_and_commit_received_file, FileBatchEntry, FileBatchManifest, FileBatchProgress,
    FileBatchRef, FileMeta, PendingReceivedFile, ReceiveSuspendGuard, ReceivedFile, SyncEngine,
    SyncPlatform, CANCELLED_BATCH_MAX_ENTRIES, CANCELLED_BATCH_RETENTION_SECONDS,
    MAX_ACTIVE_BATCHES_GLOBAL, MAX_ACTIVE_BATCHES_PER_PEER, MAX_FILE_BATCH_BYTES,
    MAX_FILE_BATCH_COUNT, MAX_FILE_SIZE, SEEN_MESSAGE_MAX_ENTRIES,
};
use crate::protocol::{FileChunkPayload, MessageId, TransferId, FILE_CHUNK_SIZE};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

#[derive(Debug)]
struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(label: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("tailsync-{label}-{:016x}", rand::random::<u64>()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn cancelled_batch_cache_expires_and_caps_entries() {
    let mut engine = SyncEngine::new();
    let now = chrono::Utc::now().timestamp();
    let expired_key = ("expired-peer".to_string(), TransferId([0xFE; 16]));
    engine.cancelled_batches.insert(
        expired_key.clone(),
        now - CANCELLED_BATCH_RETENTION_SECONDS - 1,
    );
    for index in 0..(CANCELLED_BATCH_MAX_ENTRIES + 32) {
        let mut id = [0_u8; 16];
        id[..8].copy_from_slice(&(index as u64).to_le_bytes());
        engine
            .cancelled_batches
            .insert(("peer".to_string(), TransferId(id)), now);
    }

    engine.prune_cancelled_batches();

    assert_eq!(engine.cancelled_batches.len(), CANCELLED_BATCH_MAX_ENTRIES);
    assert!(!engine.cancelled_batches.contains_key(&expired_key));
}

#[tokio::test]
async fn cancelled_batch_survives_connection_suspend() {
    let directory = TestDirectory::new("cancelled-batch-suspend");
    let manifest = manifest_with_sizes(&[0]);
    let batch_id = manifest.batch_id;
    let mut engine = SyncEngine::new();
    engine
        .begin_file_batch(manifest.clone(), "peer".to_string(), directory.path())
        .unwrap();
    engine.cancel_file_batch("peer", batch_id).await;
    engine.suspend_receive("peer");

    let error = engine
        .begin_file_batch(manifest, "peer".to_string(), directory.path())
        .unwrap_err();
    assert!(error.contains("cancelled"));
}

#[test]
fn stale_connection_suspend_keeps_state_reassigned_to_new_epoch() {
    let directory = TestDirectory::new("receive-session-epoch");
    let manifest = manifest_with_sizes(&[0]);
    let mut engine = SyncEngine::new();
    let old_epoch = engine.start_receive_session("peer");
    engine
        .begin_file_batch_at_epoch(
            manifest.clone(),
            "peer".to_string(),
            directory.path(),
            old_epoch,
        )
        .unwrap();

    let new_epoch = engine.start_receive_session("peer");
    assert_ne!(old_epoch, new_epoch);
    engine
        .begin_file_batch_at_epoch(
            manifest.clone(),
            "peer".to_string(),
            directory.path(),
            new_epoch,
        )
        .unwrap();
    engine
        .begin_file_batch_at_epoch(
            manifest.clone(),
            "peer".to_string(),
            directory.path(),
            old_epoch,
        )
        .unwrap();
    engine.suspend_receive_epoch("peer", old_epoch);

    assert!(engine.has_file_batch("peer", manifest.batch_id));
}

#[tokio::test]
async fn stale_receive_metadata_cannot_downgrade_active_epoch() {
    let directory = TestDirectory::new("receive-metadata-epoch");
    let transfer_id = TransferId([87; 16]);
    let meta = FileMeta {
        transfer_id: Some(transfer_id),
        name: "received.bin".into(),
        size: 4,
        hash: blake3::hash(b"data").to_hex().to_string(),
        chunk_size: FILE_CHUNK_SIZE as u32,
        batch: None,
    };
    let mut engine = SyncEngine::new();
    let old_epoch = engine.start_receive_session("peer");
    engine
        .begin_file_receive_at_epoch(
            meta.clone(),
            &directory.path().join("received.bin"),
            "peer".into(),
            old_epoch,
        )
        .await
        .unwrap();
    let new_epoch = engine.start_receive_session("peer");
    engine
        .begin_file_receive_at_epoch(
            meta.clone(),
            &directory.path().join("received.bin"),
            "peer".into(),
            new_epoch,
        )
        .await
        .unwrap();
    engine
        .begin_file_receive_at_epoch(
            meta,
            &directory.path().join("received.bin"),
            "peer".into(),
            old_epoch,
        )
        .await
        .unwrap();
    engine.suspend_receive_epoch("peer", old_epoch);

    let progress = engine
        .handle_resumable_file_chunk(
            &FileChunkPayload {
                transfer_id,
                offset: 0,
                data: b"data".to_vec(),
            },
            "peer".into(),
        )
        .await
        .unwrap();
    assert_eq!(progress.next_offset, 4);
}

#[derive(Debug, Clone)]
struct ReceivedBatchEvent {
    batch_id: Option<TransferId>,
    files: Vec<ReceivedFile>,
    batch_total: usize,
    batch_complete: bool,
    activate_clipboard: bool,
    device: String,
}

#[derive(Default)]
struct TestPlatform {
    received: Mutex<Vec<ReceivedBatchEvent>>,
}

impl TestPlatform {
    fn received(&self) -> Vec<ReceivedBatchEvent> {
        self.received.lock().unwrap().clone()
    }
}

impl SyncPlatform for TestPlatform {
    fn write_text(&self, _text: &str) -> Result<(), String> {
        Ok(())
    }

    fn write_image(&self, _width: u32, _height: u32, _rgba: &[u8]) -> Result<(), String> {
        Ok(())
    }

    fn set_file_progress(&self, _name: &str, _received: u64, _total: u64) {}

    fn clear_file_progress(&self, _batch_id: Option<TransferId>, _device: Option<&str>) {}

    fn set_file_batch_progress(&self, _progress: FileBatchProgress) {}

    fn files_received(
        &self,
        batch_id: Option<TransferId>,
        files: Vec<ReceivedFile>,
        batch_total: usize,
        batch_complete: bool,
        activate_clipboard: bool,
        device: String,
    ) {
        self.received.lock().unwrap().push(ReceivedBatchEvent {
            batch_id,
            files,
            batch_total,
            batch_complete,
            activate_clipboard,
            device,
        });
    }

    fn file_batch_failed(&self, _batch_id: Option<TransferId>, _message: &str) {}
}

fn manifest_with_sizes(sizes: &[u64]) -> FileBatchManifest {
    FileBatchManifest {
        batch_id: TransferId([200; 16]),
        generation: 1,
        total_bytes: sizes.iter().sum(),
        files: sizes
            .iter()
            .enumerate()
            .map(|(index, size)| FileBatchEntry {
                transfer_id: TransferId([u8::try_from(index + 1).unwrap(); 16]),
                index: u16::try_from(index).unwrap(),
                name: format!("file-{index}.bin"),
                source_parent: "Source".to_string(),
                size: *size,
                hash: blake3::hash(&[]).to_hex().to_string(),
                chunk_size: FILE_CHUNK_SIZE as u32,
            })
            .collect(),
    }
}

async fn receive_empty_file(
    sync: &mut SyncEngine,
    manifest: &FileBatchManifest,
    index: usize,
    incoming: &Path,
    source: &str,
) {
    let entry = manifest.files[index].clone();
    let meta = FileMeta {
        transfer_id: Some(entry.transfer_id),
        name: entry.name.clone(),
        size: entry.size,
        hash: entry.hash,
        chunk_size: entry.chunk_size,
        batch: Some(FileBatchRef {
            batch_id: manifest.batch_id,
            index: entry.index,
        }),
    };
    sync.begin_file_receive(meta, &incoming.join(&entry.name), source.to_string())
        .await
        .unwrap();
}

#[test]
fn file_batch_count_and_size_boundaries_are_exact() {
    assert!(manifest_with_sizes(&[0; MAX_FILE_BATCH_COUNT])
        .validate()
        .is_ok());
    assert!(matches!(
        manifest_with_sizes(&[0; MAX_FILE_BATCH_COUNT + 1])
            .validate()
            .unwrap_err(),
        crate::sync::prepare::PrepareError::BadBatchSize(_)
    ));
    assert!(manifest_with_sizes(&[MAX_FILE_BATCH_BYTES])
        .validate()
        .is_ok());
    assert!(matches!(
        manifest_with_sizes(&[MAX_FILE_BATCH_BYTES + 1])
            .validate()
            .unwrap_err(),
        crate::sync::prepare::PrepareError::BatchTooLarge
    ));
}

#[test]
fn active_file_batch_limits_are_enforced_without_affecting_other_peers() {
    let directory = TestDirectory::new("batch-limits");
    let mut sync = SyncEngine::new();

    for batch_index in 0..MAX_ACTIVE_BATCHES_PER_PEER {
        let mut manifest = manifest_with_sizes(&[0]);
        manifest.batch_id = TransferId([batch_index as u8; 16]);
        sync.begin_file_batch(manifest, "peer".to_string(), directory.path())
            .unwrap();
    }

    let mut rejected = manifest_with_sizes(&[0]);
    rejected.batch_id = TransferId([99; 16]);
    assert!(sync
        .begin_file_batch(rejected, "peer".to_string(), directory.path())
        .unwrap_err()
        .contains("active file batches"));

    let mut other_peer = manifest_with_sizes(&[0]);
    other_peer.batch_id = TransferId([100; 16]);
    sync.begin_file_batch(other_peer, "other-peer".to_string(), directory.path())
        .unwrap();
}

#[test]
fn pending_file_batch_bytes_exclude_partial_data_already_on_disk() {
    let directory = TestDirectory::new("pending-batch-bytes");
    let manifest = manifest_with_sizes(&[10, 20]);
    let first_transfer = manifest.files[0].transfer_id;
    let mut sync = SyncEngine::new();
    sync.begin_file_batch(manifest, "peer-a".into(), directory.path())
        .unwrap();
    std::fs::write(
        directory
            .path()
            .join(format!("{}.part", first_transfer.as_hex())),
        [0_u8; 4],
    )
    .unwrap();

    assert_eq!(sync.pending_file_batch_bytes(), 26);
}

#[test]
fn global_active_file_batch_limit_is_enforced() {
    let directory = TestDirectory::new("batch-global-limit");
    let mut sync = SyncEngine::new();

    for batch_index in 0..MAX_ACTIVE_BATCHES_GLOBAL {
        let mut manifest = manifest_with_sizes(&[0]);
        manifest.batch_id = TransferId([batch_index as u8; 16]);
        sync.begin_file_batch(manifest, format!("peer-{batch_index}"), directory.path())
            .unwrap();
    }

    let mut rejected = manifest_with_sizes(&[0]);
    rejected.batch_id = TransferId([99; 16]);
    assert!(sync
        .begin_file_batch(rejected, "peer-final".to_string(), directory.path())
        .unwrap_err()
        .contains("global active file batch limit"));
}

#[test]
fn folder_rejects_the_entire_selection() {
    let directory = TestDirectory::new("folder-batch");
    let file = directory.path().join("ordinary.txt");
    let folder = directory.path().join("folder");
    std::fs::write(&file, b"ordinary").unwrap();
    std::fs::create_dir(&folder).unwrap();

    let error = prepare_file_batch(vec![file, folder], 1).unwrap_err();
    assert!(matches!(
        error,
        crate::sync::prepare::PrepareError::NonFileSelected(_)
    ));
    assert!(error.to_string().contains("folder or non-file"));
}

#[test]
fn symbolic_link_rejects_the_entire_selection_when_supported() {
    let directory = TestDirectory::new("symlink-batch");
    let target = directory.path().join("target.txt");
    let link = directory.path().join("link.txt");
    std::fs::write(&target, b"target").unwrap();

    #[cfg(windows)]
    if std::os::windows::fs::symlink_file(&target, &link).is_err() {
        return;
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let error = prepare_file_batch(vec![target, link], 1).unwrap_err();
    assert!(matches!(
        error,
        crate::sync::prepare::PrepareError::SymlinkSelected(_)
    ));
    assert!(error.to_string().contains("symbolic link"));
}

#[test]
fn duplicate_names_get_a_short_source_directory_label() {
    let directory = TestDirectory::new("duplicate-names");
    let finance = directory.path().join("Finance");
    let legal = directory.path().join("Legal");
    std::fs::create_dir_all(&finance).unwrap();
    std::fs::create_dir_all(&legal).unwrap();
    let first = finance.join("report.pdf");
    let second = legal.join("report.pdf");
    std::fs::write(&first, b"finance").unwrap();
    std::fs::write(&second, b"legal").unwrap();

    let batch = prepare_file_batch(vec![first, second], 1).unwrap();
    assert_eq!(batch.manifest.files[0].name, "report.pdf");
    assert_eq!(batch.manifest.files[1].name, "report (Legal).pdf");
    assert_eq!(batch.manifest.files[1].source_parent, "Legal");
}

#[tokio::test]
async fn batch_updates_clipboard_only_after_all_files_and_completion() {
    let directory = TestDirectory::new("batch-complete");
    let manifest = manifest_with_sizes(&[0, 0]);
    let platform = Arc::new(TestPlatform::default());
    let mut sync = SyncEngine::new();
    sync.set_platform(platform.clone());
    sync.begin_file_batch(manifest.clone(), "peer".to_string(), directory.path())
        .unwrap();

    receive_empty_file(&mut sync, &manifest, 0, directory.path(), "peer").await;
    receive_empty_file(&mut sync, &manifest, 1, directory.path(), "peer").await;
    assert!(platform.received().is_empty());

    sync.finish_file_batch("peer", manifest.batch_id).unwrap();
    let events = platform.received();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].batch_id, Some(manifest.batch_id));
    assert_eq!(events[0].files.len(), 2);
    assert_eq!(events[0].batch_total, 2);
    assert!(events[0].batch_complete);
    assert!(events[0].activate_clipboard);
    assert_eq!(events[0].device, "peer");
}

#[tokio::test]
async fn completed_file_batch_is_idempotent() {
    let directory = TestDirectory::new("batch-idempotent");
    let manifest = manifest_with_sizes(&[0]);
    let platform = Arc::new(TestPlatform::default());
    let mut sync = SyncEngine::new();
    sync.set_platform(platform.clone());
    sync.begin_file_batch(manifest.clone(), "peer".to_string(), directory.path())
        .unwrap();
    receive_empty_file(&mut sync, &manifest, 0, directory.path(), "peer").await;

    sync.finish_file_batch("peer", manifest.batch_id).unwrap();
    sync.finish_file_batch("peer", manifest.batch_id).unwrap();
    assert_eq!(platform.received().len(), 1);
}

#[tokio::test]
async fn newer_clipboard_generation_prevents_old_batch_activation() {
    let directory = TestDirectory::new("superseded-batch");
    let manifest = manifest_with_sizes(&[0]);
    let platform = Arc::new(TestPlatform::default());
    let mut sync = SyncEngine::new();
    sync.set_platform(platform.clone());
    sync.begin_file_batch(manifest.clone(), "peer".to_string(), directory.path())
        .unwrap();
    receive_empty_file(&mut sync, &manifest, 0, directory.path(), "peer").await;

    sync.supersede_file_clipboard();
    sync.finish_file_batch("peer", manifest.batch_id).unwrap();
    let events = platform.received();
    assert_eq!(events.len(), 1);
    assert!(events[0].batch_complete);
    assert!(!events[0].activate_clipboard);
}

#[tokio::test]
async fn cancellation_exposes_only_completed_files_as_incomplete_history() {
    let directory = TestDirectory::new("cancelled-batch");
    let manifest = manifest_with_sizes(&[0, 0]);
    let platform = Arc::new(TestPlatform::default());
    let mut sync = SyncEngine::new();
    sync.set_platform(platform.clone());
    sync.begin_file_batch(manifest.clone(), "peer".to_string(), directory.path())
        .unwrap();
    receive_empty_file(&mut sync, &manifest, 0, directory.path(), "peer").await;

    sync.cancel_file_batch("peer", manifest.batch_id).await;
    let events = platform.received();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].files.len(), 1);
    assert_eq!(events[0].batch_total, 2);
    assert!(!events[0].batch_complete);
    assert!(!events[0].activate_clipboard);
}

#[tokio::test]
async fn completed_batch_files_resume_after_engine_restart() {
    let directory = TestDirectory::new("batch-restart");
    let manifest = manifest_with_sizes(&[0, 0]);
    let mut initial = SyncEngine::new();
    initial
        .begin_file_batch(manifest.clone(), "peer".to_string(), directory.path())
        .unwrap();
    receive_empty_file(&mut initial, &manifest, 0, directory.path(), "peer").await;
    drop(initial);

    let platform = Arc::new(TestPlatform::default());
    let mut restored = SyncEngine::new();
    restored.set_platform(platform.clone());
    restored
        .begin_file_batch(manifest.clone(), "peer".to_string(), directory.path())
        .unwrap();
    receive_empty_file(&mut restored, &manifest, 1, directory.path(), "peer").await;
    restored
        .finish_file_batch("peer", manifest.batch_id)
        .unwrap();

    let events = platform.received();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].files.len(), 2);
    assert!(events[0].batch_complete);
}

#[tokio::test]
async fn connection_suspend_releases_handles_and_preserves_partial_offset() {
    let directory = TestDirectory::new("suspend-receive");
    let transfer_id = TransferId([44; 16]);
    let data = b"first-second";
    let meta = FileMeta {
        transfer_id: Some(transfer_id),
        name: "received.bin".to_string(),
        size: data.len() as u64,
        hash: blake3::hash(data).to_hex().to_string(),
        chunk_size: FILE_CHUNK_SIZE as u32,
        batch: None,
    };
    let mut initial = SyncEngine::new();
    initial
        .begin_file_receive(
            meta.clone(),
            &directory.path().join("received.bin"),
            "peer".to_string(),
        )
        .await
        .unwrap();
    initial
        .handle_resumable_file_chunk(
            &FileChunkPayload {
                transfer_id,
                offset: 0,
                data: b"first-".to_vec(),
            },
            "peer".to_string(),
        )
        .await
        .unwrap();

    initial.suspend_receive("peer");
    assert!(directory
        .path()
        .join(format!("{}.part", transfer_id.as_hex()))
        .is_file());
    assert!(directory
        .path()
        .join(format!("{}.resume.json", transfer_id.as_hex()))
        .is_file());

    let mut restored = SyncEngine::new();
    let offset = restored
        .begin_file_receive(
            meta,
            &directory.path().join("different-name.bin"),
            "peer".to_string(),
        )
        .await
        .unwrap()
        .next_offset;
    assert_eq!(offset, 6);
}

#[test]
fn reliable_message_dedup_is_scoped_to_the_peer() {
    let mut sync = SyncEngine::new();
    let message_id = MessageId([9; 16]);

    assert!(!sync.has_seen_message("peer-a", message_id));
    sync.record_message("peer-a", message_id);
    assert!(sync.has_seen_message("peer-a", message_id));
    assert!(!sync.has_seen_message("peer-b", message_id));
}

#[test]
fn reliable_message_dedup_is_bounded_by_insertion_order() {
    let mut sync = SyncEngine::new();
    let mut first_id = [0_u8; 16];
    first_id[..8].copy_from_slice(&0_u64.to_le_bytes());
    let first_id = MessageId(first_id);

    sync.record_message("peer", first_id);
    for index in 1..=SEEN_MESSAGE_MAX_ENTRIES {
        let mut id = [0_u8; 16];
        id[..8].copy_from_slice(&(index as u64).to_le_bytes());
        sync.record_message("peer", MessageId(id));
    }

    assert!(!sync.has_seen_message("peer", first_id));
    assert_eq!(sync.seen_message_order.len(), SEEN_MESSAGE_MAX_ENTRIES);
    let mut latest_id = [0_u8; 16];
    latest_id[..8].copy_from_slice(&(SEEN_MESSAGE_MAX_ENTRIES as u64).to_le_bytes());
    assert!(sync.has_seen_message("peer", MessageId(latest_id)));
}

#[tokio::test]
async fn file_receive_limit_is_enforced_per_peer() {
    let directory = std::env::temp_dir().join(format!(
        "tailsync-receive-limit-{:016x}",
        rand::random::<u64>()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let mut sync = SyncEngine::new();
    for index in 0..2_u8 {
        let meta = FileMeta {
            transfer_id: Some(TransferId([index + 1; 16])),
            name: format!("{index}.bin"),
            size: 1,
            hash: "unused".into(),
            chunk_size: FILE_CHUNK_SIZE as u32,
            batch: None,
        };
        sync.begin_file_receive(meta, &directory.join(format!("{index}.bin")), "peer".into())
            .await
            .unwrap();
    }
    let third = FileMeta {
        transfer_id: Some(TransferId([3; 16])),
        name: "third.bin".into(),
        size: 1,
        hash: "unused".into(),
        chunk_size: FILE_CHUNK_SIZE as u32,
        batch: None,
    };
    assert!(sync
        .begin_file_receive(third, &directory.join("third.bin"), "peer".into())
        .await
        .unwrap_err()
        .contains("active file receives"));
    sync.cancel_receive("peer").await;
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn transferred_file_name_removes_current_and_legacy_storage_prefixes() {
    let hash = "0b3a82ef490260f43fc7bf96477ca20ca44bd60eabd9ad94ae3114e7d8a974b1";
    assert_eq!(
        normalize_transferred_file_name(
            "0b3a82ef490260f43fc7bf96477ca20ca44bd60eabd9ad94ae3114e7d8a974b1-report.pdf",
            hash,
        ),
        "report.pdf"
    );
    assert_eq!(
        normalize_transferred_file_name("0b3a82ef4902_0b3a82ef4902_report.pdf", hash),
        "report.pdf"
    );
    assert_eq!(
        normalize_transferred_file_name("quarterly-report.pdf", hash),
        "quarterly-report.pdf"
    );
}

#[tokio::test]
async fn file_receive_restores_the_confirmed_offset_from_disk() {
    let directory = std::env::temp_dir().join(format!(
        "tailsync-resume-test-{:016x}",
        rand::random::<u64>()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let final_path = directory.join("received.bin");
    let transfer_id = TransferId([6; 16]);
    let full_data = b"first-second";
    let meta = FileMeta {
        transfer_id: Some(transfer_id),
        name: "received.bin".into(),
        size: full_data.len() as u64,
        hash: blake3::hash(full_data).to_hex().to_string(),
        chunk_size: FILE_CHUNK_SIZE as u32,
        batch: None,
    };
    let first = FileChunkPayload {
        transfer_id,
        offset: 0,
        data: b"first-".to_vec(),
    };

    let mut initial = SyncEngine::new();
    assert_eq!(
        initial
            .begin_file_receive(meta.clone(), &final_path, "peer".into())
            .await
            .unwrap()
            .next_offset,
        0
    );
    assert_eq!(
        initial
            .handle_resumable_file_chunk(&first, "peer".into())
            .await
            .unwrap()
            .next_offset,
        6
    );
    assert_eq!(
        initial
            .handle_resumable_file_chunk(&first, "peer".into())
            .await
            .unwrap()
            .next_offset,
        6
    );
    drop(initial);

    let mut restored = SyncEngine::new();
    assert_eq!(
        restored
            .begin_file_receive(meta, &directory.join("different-name.bin"), "peer".into())
            .await
            .unwrap()
            .next_offset,
        6
    );
    drop(restored);
    assert_eq!(
        std::fs::metadata(directory.join(format!("{}.part", transfer_id.as_hex())))
            .unwrap()
            .len(),
        6
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn completed_transfer_shortcut_requires_matching_hash() {
    let directory = TestDirectory::new("completed-transfer-hash");
    let transfer_id = TransferId([71; 16]);
    let original = b"original";
    let original_meta = FileMeta {
        transfer_id: Some(transfer_id),
        name: "original.bin".into(),
        size: original.len() as u64,
        hash: blake3::hash(original).to_hex().to_string(),
        chunk_size: FILE_CHUNK_SIZE as u32,
        batch: None,
    };
    let mut sync = SyncEngine::new();
    sync.begin_file_receive(
        original_meta,
        &directory.path().join("original.bin"),
        "peer".into(),
    )
    .await
    .unwrap();
    sync.handle_resumable_file_chunk(
        &FileChunkPayload {
            transfer_id,
            offset: 0,
            data: original.to_vec(),
        },
        "peer".into(),
    )
    .await
    .unwrap();

    let replacement = b"replaced";
    let progress = sync
        .begin_file_receive(
            FileMeta {
                transfer_id: Some(transfer_id),
                name: "replacement.bin".into(),
                size: replacement.len() as u64,
                hash: blake3::hash(replacement).to_hex().to_string(),
                chunk_size: FILE_CHUNK_SIZE as u32,
                batch: None,
            },
            &directory.path().join("replacement.bin"),
            "peer".into(),
        )
        .await
        .unwrap();
    assert_eq!(progress.next_offset, 0);
}

#[tokio::test]
async fn resumed_file_is_verified_before_commit() {
    let directory = TestDirectory::new("resumed-file-verification");
    let transfer_id = TransferId([72; 16]);
    let data = b"first-second";
    let meta = FileMeta {
        transfer_id: Some(transfer_id),
        name: "received.bin".into(),
        size: data.len() as u64,
        hash: blake3::hash(data).to_hex().to_string(),
        chunk_size: FILE_CHUNK_SIZE as u32,
        batch: None,
    };
    let mut initial = SyncEngine::new();
    initial
        .begin_file_receive(
            meta.clone(),
            &directory.path().join("received.bin"),
            "peer".into(),
        )
        .await
        .unwrap();
    initial
        .handle_resumable_file_chunk(
            &FileChunkPayload {
                transfer_id,
                offset: 0,
                data: b"first-".to_vec(),
            },
            "peer".into(),
        )
        .await
        .unwrap();
    initial.suspend_receive("peer");

    let platform = Arc::new(TestPlatform::default());
    let mut restored = SyncEngine::new();
    restored.set_platform(platform.clone());
    let progress = restored
        .begin_file_receive(meta, &directory.path().join("different.bin"), "peer".into())
        .await
        .unwrap();
    assert_eq!(progress.next_offset, 6);
    let progress = restored
        .handle_resumable_file_chunk(
            &FileChunkPayload {
                transfer_id,
                offset: 6,
                data: b"second".to_vec(),
            },
            "peer".into(),
        )
        .await
        .unwrap();
    assert!(platform.received().is_empty());
    let verified = progress.completed.unwrap().verify_hash().unwrap();
    restored.commit_received_file("peer", verified).unwrap();
    assert_eq!(platform.received().len(), 1);
}

#[tokio::test]
async fn corrupt_resumed_file_fails_deferred_verification() {
    let directory = TestDirectory::new("corrupt-resumed-file");
    let transfer_id = TransferId([73; 16]);
    let expected = b"first-second";
    let meta = FileMeta {
        transfer_id: Some(transfer_id),
        name: "received.bin".into(),
        size: expected.len() as u64,
        hash: blake3::hash(expected).to_hex().to_string(),
        chunk_size: FILE_CHUNK_SIZE as u32,
        batch: None,
    };
    let mut initial = SyncEngine::new();
    initial
        .begin_file_receive(
            meta.clone(),
            &directory.path().join("received.bin"),
            "peer".into(),
        )
        .await
        .unwrap();
    initial
        .handle_resumable_file_chunk(
            &FileChunkPayload {
                transfer_id,
                offset: 0,
                data: b"wrong-".to_vec(),
            },
            "peer".into(),
        )
        .await
        .unwrap();
    initial.suspend_receive("peer");

    let mut restored = SyncEngine::new();
    restored
        .begin_file_receive(meta, &directory.path().join("different.bin"), "peer".into())
        .await
        .unwrap();
    let progress = restored
        .handle_resumable_file_chunk(
            &FileChunkPayload {
                transfer_id,
                offset: 6,
                data: b"second".to_vec(),
            },
            "peer".into(),
        )
        .await
        .unwrap();
    let pending = progress.completed.unwrap();
    let path = pending.path().to_path_buf();
    assert!(pending
        .verify_hash()
        .unwrap_err()
        .contains("checksum mismatch"));
    assert!(path.is_file());
}

#[test]
fn expired_transfer_cleanup_removes_orphans_but_preserves_recent_and_nested_files() {
    let directory = TestDirectory::new("expired-transfer-cleanup");
    let old_plaintext = directory.path().join("hash-old.txt");
    let recent_plaintext = directory.path().join("hash-recent.txt");
    let old_partial = directory.path().join("transfer.part");
    let nested_directory = directory.path().join("clipboard-files");
    let nested_file = nested_directory.join("managed.txt");
    std::fs::create_dir(&nested_directory).unwrap();
    for path in [
        &old_plaintext,
        &recent_plaintext,
        &old_partial,
        &nested_file,
    ] {
        std::fs::write(path, b"data").unwrap();
    }
    let old = SystemTime::now() - Duration::from_secs(2 * 60 * 60);
    for path in [&old_plaintext, &old_partial, &nested_file] {
        let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_times(std::fs::FileTimes::new().set_modified(old))
            .unwrap();
    }

    super::resume::cleanup_expired_transfers_in(directory.path(), Duration::from_secs(60 * 60));

    assert!(!old_plaintext.exists());
    assert!(!old_partial.exists());
    assert!(recent_plaintext.exists());
    assert!(nested_file.exists());
}

// ------------------------------------------------------------------
// verify_and_commit_received_file (T108): deferred verification and
// commit orchestration for completed inbound files.
// ------------------------------------------------------------------

async fn completed_pending_receive(
    directory: &TestDirectory,
    data: &[u8],
    corrupt_first_chunk: bool,
    transfer_id: TransferId,
) -> (
    Arc<tokio::sync::Mutex<SyncEngine>>,
    Arc<TestPlatform>,
    PendingReceivedFile,
) {
    let meta = FileMeta {
        transfer_id: Some(transfer_id),
        name: "received.bin".into(),
        size: data.len() as u64,
        hash: blake3::hash(data).to_hex().to_string(),
        chunk_size: FILE_CHUNK_SIZE as u32,
        batch: None,
    };
    let mut initial = SyncEngine::new();
    initial
        .begin_file_receive(
            meta.clone(),
            &directory.path().join("received.bin"),
            "peer".into(),
        )
        .await
        .unwrap();
    let first_chunk = if corrupt_first_chunk {
        b"wrong-".to_vec()
    } else {
        data[..6].to_vec()
    };
    initial
        .handle_resumable_file_chunk(
            &FileChunkPayload {
                transfer_id,
                offset: 0,
                data: first_chunk,
            },
            "peer".into(),
        )
        .await
        .unwrap();
    initial.suspend_receive("peer");

    let platform = Arc::new(TestPlatform::default());
    let mut restored = SyncEngine::new();
    restored.set_platform(platform.clone());
    restored
        .begin_file_receive(meta, &directory.path().join("different.bin"), "peer".into())
        .await
        .unwrap();
    let progress = restored
        .handle_resumable_file_chunk(
            &FileChunkPayload {
                transfer_id,
                offset: 6,
                data: data[6..].to_vec(),
            },
            "peer".into(),
        )
        .await
        .unwrap();
    let pending = progress.completed.unwrap();
    (
        Arc::new(tokio::sync::Mutex::new(restored)),
        platform,
        pending,
    )
}

#[tokio::test]
async fn verify_and_commit_commits_verified_files() {
    let directory = TestDirectory::new("verify-commit-ok");
    let (engine, platform, pending) =
        completed_pending_receive(&directory, b"first-second", false, TransferId([74; 16])).await;

    verify_and_commit_received_file(&engine, "peer", pending)
        .await
        .unwrap();
    assert_eq!(platform.received().len(), 1);
    // The temp file's fate on success is the platform files_received
    // responsibility (the test platform is a no-op), mirroring the
    // original server behavior of removing it only on failure.
}

#[tokio::test]
async fn verify_and_commit_removes_corrupt_files_without_committing() {
    let directory = TestDirectory::new("verify-commit-corrupt");
    let (engine, platform, pending) =
        completed_pending_receive(&directory, b"first-second", true, TransferId([75; 16])).await;
    let path = pending.path().to_path_buf();
    assert!(path.is_file());

    let error = verify_and_commit_received_file(&engine, "peer", pending)
        .await
        .unwrap_err();
    assert!(error.contains("checksum mismatch"));
    assert!(!path.exists(), "corrupt temp file must be removed");
    assert!(platform.received().is_empty());
}

// ------------------------------------------------------------------
// ReceiveSuspendGuard (T109): pairing-time receive suspension.
// ------------------------------------------------------------------

#[tokio::test]
async fn receive_suspend_guard_suspends_active_receives_on_drop() {
    let directory = TestDirectory::new("suspend-guard");
    let transfer_id = TransferId([76; 16]);
    // A deliberately partial receive: 6 of 12 bytes arrive before
    // the guard drops, so the transfer stays active.
    let data = b"first-second";
    let meta = FileMeta {
        transfer_id: Some(transfer_id),
        name: "received.bin".into(),
        size: data.len() as u64,
        hash: blake3::hash(data).to_hex().to_string(),
        chunk_size: FILE_CHUNK_SIZE as u32,
        batch: None,
    };
    let mut engine = SyncEngine::new();
    let epoch = engine.start_receive_session("peer");
    engine
        .begin_file_receive(meta, &directory.path().join("received.bin"), "peer".into())
        .await
        .unwrap();
    engine
        .handle_resumable_file_chunk(
            &FileChunkPayload {
                transfer_id,
                offset: 0,
                data: data[..6].to_vec(),
            },
            "peer".into(),
        )
        .await
        .unwrap();
    let engine = Arc::new(tokio::sync::Mutex::new(engine));

    {
        let _guard = ReceiveSuspendGuard::new(engine.clone(), "peer".to_string(), epoch);
    }
    tokio::task::yield_now().await;

    // The active receive is gone; another chunk for it is unknown.
    let error = engine
        .lock()
        .await
        .handle_resumable_file_chunk(
            &FileChunkPayload {
                transfer_id,
                offset: 4,
                data: b"rest".to_vec(),
            },
            "peer".into(),
        )
        .await
        .unwrap_err();
    assert!(error.contains("file transfer metadata is not available"));
}

#[test]
fn receive_suspend_guard_drop_without_runtime_does_not_panic() {
    let engine = Arc::new(tokio::sync::Mutex::new(SyncEngine::new()));
    let guard = ReceiveSuspendGuard::new(engine, "peer".to_string(), 0);
    drop(guard);
}

// ------------------------------------------------------------------
// Inbound FileMeta validation (T107): untrusted peer input handling.
// ------------------------------------------------------------------
fn meta_with_name(name: &str) -> FileMeta {
    FileMeta {
        transfer_id: None,
        name: name.to_string(),
        size: 10,
        hash: "abcd".repeat(16),
        chunk_size: 0,
        batch: Some(FileBatchRef {
            batch_id: TransferId([7; 16]),
            index: 0,
        }),
    }
}

#[test]
fn file_meta_accepts_valid_input_and_normalizes_the_name() {
    let mut meta = meta_with_name("../nested/notes.txt");
    validate_incoming_file_meta(&mut meta).unwrap();
    assert_eq!(meta.name, "notes.txt");

    // Hash-prefixed names are stripped by the normalizer.
    let mut meta = meta_with_name(&format!("{}-notes.txt", "abcd".repeat(16)));
    validate_incoming_file_meta(&mut meta).unwrap();
    assert_eq!(meta.name, "notes.txt");
}

#[test]
fn file_meta_requires_the_batch_protocol() {
    let mut meta = meta_with_name("a.txt");
    meta.batch = None;
    assert!(matches!(
        validate_incoming_file_meta(&mut meta),
        Err(crate::sync::prepare::PrepareError::ProtocolRequired)
    ));
}

#[test]
fn file_meta_rejects_oversized_files() {
    let mut meta = meta_with_name("big.bin");
    meta.size = MAX_FILE_SIZE + 1;
    assert!(matches!(
        validate_incoming_file_meta(&mut meta),
        Err(crate::sync::prepare::PrepareError::FileTooLarge)
    ));
}

#[test]
fn file_meta_rejects_missing_or_invalid_names() {
    let mut meta = meta_with_name("/");
    assert!(matches!(
        validate_incoming_file_meta(&mut meta),
        Err(crate::sync::prepare::PrepareError::InvalidFileName)
    ));

    // A name that normalizes back to itself stays accepted.
    let mut meta = meta_with_name(&format!("{}-", "abcd".repeat(16)));
    assert!(validate_incoming_file_meta(&mut meta).is_ok());
    assert_eq!(meta.name, format!("{}-", "abcd".repeat(16)));

    for suffix in [".", ".."] {
        let mut meta = meta_with_name(&format!("{}-{suffix}", "abcd".repeat(16)));
        assert!(matches!(
            validate_incoming_file_meta(&mut meta),
            Err(crate::sync::prepare::PrepareError::InvalidFileName)
        ));
    }
}

#[test]
fn file_meta_rejects_invalid_chunk_sizes_for_resumable_transfers() {
    let mut meta = meta_with_name("resume.bin");
    meta.transfer_id = Some(TransferId([1; 16]));
    meta.chunk_size = 0;
    assert!(matches!(
        validate_incoming_file_meta(&mut meta),
        Err(crate::sync::prepare::PrepareError::InvalidChunkSize)
    ));

    let mut meta = meta_with_name("resume.bin");
    meta.transfer_id = Some(TransferId([1; 16]));
    meta.chunk_size = (crate::protocol::FILE_CHUNK_SIZE + 1) as u32;
    assert!(matches!(
        validate_incoming_file_meta(&mut meta),
        Err(crate::sync::prepare::PrepareError::InvalidChunkSize)
    ));

    // Non-resumable transfers do not carry a chunk size requirement.
    let mut meta = meta_with_name("plain.bin");
    meta.chunk_size = 0;
    assert!(validate_incoming_file_meta(&mut meta).is_ok());
}
