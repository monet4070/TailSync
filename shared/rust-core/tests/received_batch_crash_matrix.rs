#![cfg(feature = "acceptance-injection")]

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use tailsync_core::protocol::{TransferId, FILE_CHUNK_SIZE};
use tailsync_core::sync::{
    verify_and_commit_received_file, FileBatchEntry, FileBatchManifest, FileBatchProgress,
    FileBatchRef, FileMeta, FileReceiveCommit, PlatformResultFuture, SyncEngine, SyncPlatform,
};

const WORKER_ENV: &str = "TAILSYNC_CRASH_MATRIX_WORKER";
const ROOT_ENV: &str = "TAILSYNC_CRASH_MATRIX_ROOT";
const ABORT_STAGE_ENV: &str = "TAILSYNC_RECEIVED_BATCH_ABORT_STAGE";
const SOURCE: &str = "acceptance-peer";

struct DurableTestPlatform {
    root: PathBuf,
}

impl DurableTestPlatform {
    fn create_once(&self, name: &str) -> Result<(), String> {
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.root.join(name))
        {
            Ok(mut file) => file.write_all(b"1").map_err(|error| error.to_string()),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }
}

impl SyncPlatform for DurableTestPlatform {
    fn write_text(&self, _text: &str) -> Result<(), String> {
        Ok(())
    }

    fn write_image(&self, _width: u32, _height: u32, _rgba: &[u8]) -> Result<(), String> {
        Ok(())
    }

    fn set_file_progress(&self, _name: &str, _received: u64, _total: u64) {}

    fn clear_file_progress(&self, _batch_id: Option<TransferId>, _device: Option<&str>) {}

    fn set_file_batch_progress(&self, _progress: FileBatchProgress) {}

    fn files_received(&self, _commit: FileReceiveCommit) -> PlatformResultFuture {
        let root = self.root.clone();
        Box::pin(async move {
            let platform = DurableTestPlatform { root };
            platform.create_once("history.persisted")?;
            platform.create_once("receipt.persisted")?;
            Ok(())
        })
    }

    fn file_batch_failed(&self, _batch_id: Option<TransferId>, _message: &str) {}
}

fn manifest_for(root: &Path) -> Result<FileBatchManifest, String> {
    let manifest_path = root.join("manifest.json");
    if manifest_path.is_file() {
        return serde_json::from_slice(&fs::read(manifest_path).map_err(|e| e.to_string())?)
            .map_err(|error| error.to_string());
    }
    let batch_id = TransferId([0xA5; 16]);
    let transfer_id = TransferId([0x5A; 16]);
    let manifest = FileBatchManifest {
        batch_id,
        generation: 1,
        total_bytes: 0,
        files: vec![FileBatchEntry {
            transfer_id,
            index: 0,
            name: "payload.bin".to_string(),
            source_parent: String::new(),
            size: 0,
            hash: blake3::hash(&[]).to_hex().to_string(),
            chunk_size: FILE_CHUNK_SIZE as u32,
        }],
    };
    fs::write(
        manifest_path,
        serde_json::to_vec_pretty(&manifest).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    Ok(manifest)
}

fn record_ack_attempt(root: &Path) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("ack.attempts"))
        .map_err(|error| error.to_string())?;
    file.write_all(b"1\n").map_err(|error| error.to_string())
}

fn sidecar_has_completed_file(incoming: &Path, batch_id: TransferId) -> Result<bool, String> {
    let path = incoming.join(format!("{}.batch.json", batch_id.as_hex()));
    let json: serde_json::Value =
        serde_json::from_slice(&fs::read(path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    Ok(json["files"]
        .as_array()
        .and_then(|files| files.first())
        .is_some_and(|file| !file.is_null()))
}

async fn run_worker(root: &Path) -> Result<(), String> {
    fs::create_dir_all(root).map_err(|error| error.to_string())?;
    let incoming = root.join("incoming");
    fs::create_dir_all(&incoming).map_err(|error| error.to_string())?;
    let manifest = manifest_for(root)?;
    let batch_id = manifest.batch_id;
    let entry = manifest.files[0].clone();
    let engine = Arc::new(tokio::sync::Mutex::new(SyncEngine::new()));
    engine
        .lock()
        .await
        .set_platform(Arc::new(DurableTestPlatform {
            root: root.to_path_buf(),
        }));

    SyncEngine::begin_file_batch_shared(
        &engine,
        manifest,
        SOURCE.to_string(),
        "acceptance-device".to_string(),
        incoming.clone(),
        1,
    )
    .await?;

    if !sidecar_has_completed_file(&incoming, batch_id)? {
        let progress = SyncEngine::begin_file_receive_shared(
            &engine,
            FileMeta {
                transfer_id: Some(entry.transfer_id),
                name: entry.name,
                size: entry.size,
                hash: entry.hash,
                chunk_size: entry.chunk_size,
                batch: Some(FileBatchRef {
                    batch_id,
                    index: entry.index,
                }),
            },
            &incoming.join("payload.bin"),
            SOURCE.to_string(),
            1,
        )
        .await?;
        let pending = progress
            .completed
            .ok_or_else(|| "empty receive did not complete".to_string())?;
        verify_and_commit_received_file(&engine, SOURCE, pending).await?;
    }

    SyncEngine::finish_file_batch_shared(&engine, SOURCE, batch_id).await?;
    record_ack_attempt(root)?;
    SyncEngine::acknowledge_file_batch_shared(&engine, SOURCE, batch_id, &incoming).await?;
    Ok(())
}

fn sidecar_state(root: &Path) -> String {
    let incoming = root.join("incoming");
    let sidecar = fs::read_dir(incoming)
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.to_string_lossy().ends_with(".batch.json"))
        .expect("crash must retain the batch sidecar");
    let json: serde_json::Value = serde_json::from_slice(&fs::read(sidecar).unwrap()).unwrap();
    json["commit_state"].as_str().unwrap().to_string()
}

fn ack_attempts(root: &Path) -> usize {
    fs::read_to_string(root.join("ack.attempts"))
        .map(|value| value.lines().count())
        .unwrap_or(0)
}

#[tokio::test]
#[ignore]
async fn crash_worker() {
    if std::env::var_os(WORKER_ENV).is_none() {
        return;
    }
    let root = PathBuf::from(std::env::var_os(ROOT_ENV).expect("worker root"));
    run_worker(&root).await.unwrap();
}

#[test]
fn received_batch_commit_recovers_after_every_persisted_stage() {
    let stages = [
        "hash_verified",
        "clipboard_staged",
        "history_persisted",
        "receipt_persisted",
        "acked",
        "cleaned",
    ];
    let executable = std::env::current_exe().unwrap();
    for stage in stages {
        let root = std::env::temp_dir().join(format!(
            "tailsync-received-batch-crash-{stage}-{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        fs::create_dir_all(&root).unwrap();
        let crashed = Command::new(&executable)
            .args(["crash_worker", "--exact", "--ignored", "--nocapture"])
            .env(WORKER_ENV, "1")
            .env(ROOT_ENV, &root)
            .env(ABORT_STAGE_ENV, stage)
            .status()
            .unwrap();
        assert_eq!(crashed.code(), Some(70), "stage {stage} did not abort");
        assert_eq!(sidecar_state(&root), stage);

        let resumed = Command::new(&executable)
            .args(["crash_worker", "--exact", "--ignored", "--nocapture"])
            .env(WORKER_ENV, "1")
            .env(ROOT_ENV, &root)
            .env_remove(ABORT_STAGE_ENV)
            .status()
            .unwrap();
        assert!(resumed.success(), "stage {stage} did not recover");
        assert!(root.join("history.persisted").is_file());
        assert!(root.join("receipt.persisted").is_file());
        assert!(!root
            .join("incoming")
            .join(format!("{}.batch.json", "a5".repeat(16)))
            .exists());
        let expected_acks = if matches!(stage, "acked" | "cleaned") {
            2
        } else {
            1
        };
        assert_eq!(ack_attempts(&root), expected_acks, "stage {stage}");
        fs::remove_dir_all(root).unwrap();
    }
}
