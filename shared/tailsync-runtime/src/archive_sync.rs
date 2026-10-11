use std::collections::VecDeque;
use std::sync::{Arc, Mutex as StdMutex};

use tailsync_core::composite::{archive_sync::MAX_WIRE_BYTES, budgets, PortableBundle};
use tailsync_core::db::HistoryDB;
use tailsync_core::peer::delivery::SharedEvent;
use tailsync_core::protocol::Command;
use tokio::sync::{Mutex, Notify, OwnedMutexGuard};

use crate::execution::{run_db_named, DbExecutionError};
use crate::history::ArchiveCommit;

const MAX_PENDING: usize = 8;

#[derive(Debug, PartialEq, Eq)]
pub enum EnqueueResult {
    Queued,
    Coalesced,
    Full,
}

pub struct ArchiveDispatch {
    pub event: SharedEvent,
    pub commit: ArchiveCommit,
}

pub struct ArchiveSyncQueue {
    database: Arc<Mutex<HistoryDB>>,
    pending: StdMutex<VecDeque<ArchiveCommit>>,
    changed: Notify,
}

impl ArchiveSyncQueue {
    pub fn new(database: Arc<Mutex<HistoryDB>>) -> Self {
        Self {
            database,
            pending: StdMutex::new(VecDeque::new()),
            changed: Notify::new(),
        }
    }

    pub fn enqueue(&self, commit: ArchiveCommit) -> EnqueueResult {
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(existing) = pending.iter_mut().find(|existing| existing.id == commit.id) {
            *existing = commit;
            self.changed.notify_one();
            return EnqueueResult::Coalesced;
        }
        if pending.len() >= MAX_PENDING {
            return EnqueueResult::Full;
        }
        pending.push_back(commit);
        self.changed.notify_one();
        EnqueueResult::Queued
    }

    pub async fn next(&self) -> Result<Option<ArchiveDispatch>, String> {
        let commit = loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let next = self
                .pending
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .pop_front();
            if let Some(commit) = next {
                break commit;
            }
            notified.await;
        };
        let expected = commit.clone();
        let event = run_db_named(
            "archive_sync.export",
            self.database.clone(),
            move |database| {
                database
                    .validate_storage_authority(&expected.guard.authority)
                    .map_err(|_| "archive_superseded".to_string())?;
                let _memory = budgets::global()
                    .reserve_pipeline(4 * MAX_WIRE_BYTES as u64 + 96 * 1024 * 1024)
                    .map_err(|error| error.to_string())?;
                let _decode = budgets::global()
                    .admit_decode(&[0; 32])
                    .map_err(|error| error.to_string())?;
                let update = database
                    .export_composite_archive_update(expected.id, &expected.guard.identity)
                    .map_err(|error| error.to_string())?;
                update
                    .map(|update| {
                        SharedEvent::encode(
                            Command::ArchiveUpdatePayload,
                            update.encode().map_err(|error| error.to_string())?,
                        )
                    })
                    .transpose()
            },
        )
        .await
        .map_err(|error: DbExecutionError<String>| error.to_string())?;
        Ok(event.map(|event| ArchiveDispatch { event, commit }))
    }

    pub async fn current(&self, commit: ArchiveCommit) -> Option<OwnedMutexGuard<HistoryDB>> {
        let database = self.database.clone().lock_owned().await;
        tokio::task::spawn_blocking(move || {
            if database
                .validate_storage_authority(&commit.guard.authority)
                .is_err()
                || database
                    .composite_identity(commit.id)
                    .ok()
                    .flatten()
                    .as_ref()
                    != Some(&commit.guard.identity)
            {
                return None;
            }
            Some(database)
        })
        .await
        .ok()
        .flatten()
    }
}

pub async fn bind_local_event(
    database: Arc<Mutex<HistoryDB>>,
    commit: ArchiveCommit,
    event: &SharedEvent,
    payload: Arc<[u8]>,
) -> Result<Option<ArchiveCommit>, String> {
    if event.queued().command() != Command::CompositePayload
        || event
            .queued()
            .payload()
            .get(tailsync_core::protocol::EVENT_ENVELOPE_HEADER_SIZE..)
            != Some(payload.as_ref())
    {
        return Err("Archive binding requires the exact original composite event".into());
    }
    let (event_id, timestamp_ms) = event.envelope_identity();
    run_db_named("archive_sync.bind", database, move |database| {
        let _memory = budgets::global()
            .reserve_pipeline(3 * payload.len() as u64)
            .map_err(|error| error.to_string())?;
        let _decode = budgets::global()
            .admit_decode(&[0; 32])
            .map_err(|error| error.to_string())?;
        database
            .validate_storage_authority(&commit.guard.authority)
            .map_err(|_| "archive_superseded".to_string())?;
        let Some(identity) = database
            .composite_identity(commit.id)
            .map_err(|error| error.to_string())?
        else {
            return Ok(None);
        };
        if identity.capture_id != commit.guard.identity.capture_id
            || identity.content_id != commit.guard.identity.content_id
            || identity.version != commit.guard.identity.version
            || identity.dedupe_key != commit.guard.identity.dedupe_key
        {
            return Ok(None);
        }
        let base = PortableBundle::decode(&payload).map_err(|error| error.to_string())?;
        if !database
            .bind_composite_outgoing_event(
                commit.id,
                &identity,
                event_id,
                base.digest().map_err(|error| error.to_string())?,
                timestamp_ms,
            )
            .map_err(|error| error.to_string())?
        {
            return Ok(None);
        }
        Ok(Some(ArchiveCommit {
            id: commit.id,
            guard: crate::history::ArchiveEntryGuard::capture(database, commit.id)?,
        }))
    })
    .await
    .map_err(|error: DbExecutionError<String>| error.to_string())
}
