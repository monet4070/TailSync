use super::composite_receiver::AuthorizationCheck;
use crate::cancellation::Cancellation;
use crate::composite::{archive_sync::ArchiveUpdate, budgets};
use crate::db::{HistoryDB, RemoteArchiveOutcome};
use crate::protocol::composite::{AckResult, CompositeAck};
use crate::protocol::{unix_timestamp_ms, EventEnvelope, TEMPORARY_PRESSURE_MARKER};
use std::sync::Arc;
use tokio::sync::Mutex;

struct CancelReceive {
    cancellation: Cancellation,
    watcher: tokio::task::JoinHandle<()>,
}

impl Drop for CancelReceive {
    fn drop(&mut self) {
        self.cancellation.cancel();
        self.watcher.abort();
    }
}

pub async fn receive(
    envelope: EventEnvelope,
    source_identity: String,
    budget_identity: [u8; 32],
    chunk_index: u16,
    database: Arc<Mutex<HistoryDB>>,
    authorized: AuthorizationCheck,
    on_commit: Arc<dyn Fn() + Send + Sync>,
) -> Result<CompositeAck, String> {
    envelope
        .validate_timestamp(unix_timestamp_ms())
        .map_err(|error| error.to_string())?;
    let permission = authorized.clone();
    if !tokio::task::spawn_blocking(move || permission())
        .await
        .map_err(|_| "Archive authorization worker unavailable")?
    {
        return Err("Archive authorization revoked".into());
    }
    let pipeline = budgets::global()
        .reserve_pipeline(
            (envelope.content.len() as u64)
                .saturating_mul(4)
                .saturating_add(96 * 1024 * 1024),
        )
        .map_err(|error| error.to_string())?;
    let decode = budgets::global()
        .admit_decode(&budget_identity)
        .map_err(|error| error.to_string())?;
    let cancellation = Cancellation::default();
    let watcher_cancellation = cancellation.clone();
    let watcher_permission = authorized.clone();
    let watcher = tokio::spawn(async move {
        loop {
            let check = watcher_permission.clone();
            if !matches!(tokio::task::spawn_blocking(move || check()).await, Ok(true)) {
                watcher_cancellation.cancel();
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    });
    let _scope = CancelReceive {
        cancellation: cancellation.clone(),
        watcher,
    };
    let message_id = envelope.message_id;
    let digest = *blake3::hash(&envelope.content).as_bytes();
    let outcome = tokio::task::spawn_blocking(move || {
        let _held = (pipeline, decode);
        cancellation.check().map_err(|error| error.to_string())?;
        let update = ArchiveUpdate::decode(&envelope.content).map_err(|error| error.to_string())?;
        if !authorized() {
            return Err("Archive authorization revoked".to_string());
        }
        let mut database = database.blocking_lock();
        if !database.is_storage_available() {
            return Err("Archive history storage unavailable".to_string());
        }
        let result = database
            .commit_remote_composite_archive_update(&source_identity, &update, &cancellation)
            .map_err(|error| error.to_string())?;
        if matches!(result, RemoteArchiveOutcome::Applied(_)) {
            on_commit();
        }
        Ok::<_, String>(result)
    })
    .await
    .map_err(|_| "Archive persistence worker unavailable")??;
    let result = match outcome {
        RemoteArchiveOutcome::Applied(_) | RemoteArchiveOutcome::Duplicate(_) => AckResult::Applied,
        RemoteArchiveOutcome::Superseded => AckResult::Superseded,
        RemoteArchiveOutcome::BaseMissing => {
            return Err(format!(
                "{TEMPORARY_PRESSURE_MARKER}Archive base event has not arrived"
            ));
        }
    };
    Ok(CompositeAck {
        message_id,
        digest,
        chunk_index,
        result,
    })
}
