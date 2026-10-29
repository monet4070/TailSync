use super::composite_receipts::{self, Operation, Receipt};
use crate::{
    composite::{self, Completeness},
    db::HistoryDB,
    protocol::{
        composite::{AckResult, CompositeAck, ValidatedComposite},
        unix_timestamp_ms, EventEnvelope,
    },
    sync::SyncEngine,
};
use std::sync::Arc;
use tokio::sync::Mutex;
// Serializes duplicate finish frames across sessions; assemblies remain
// independent and native effects are already serialized by the engine.
pub type AuthorizationCheck = Arc<dyn Fn() -> bool + Send + Sync>;
struct CancelPublication {
    flag: Arc<std::sync::atomic::AtomicBool>,
    watcher: tokio::task::JoinHandle<()>,
}
impl Drop for CancelPublication {
    fn drop(&mut self) {
        self.flag.store(true, std::sync::atomic::Ordering::Release);
        self.watcher.abort();
    }
}
fn watch_authorization(
    check: AuthorizationCheck,
    engine: Arc<Mutex<SyncEngine>>,
) -> CancelPublication {
    let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let cancelled = flag.clone();
    let watcher = tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            let check = check.clone();
            if !matches!(tokio::task::spawn_blocking(move || check()).await, Ok(true)) {
                cancelled.store(true, std::sync::atomic::Ordering::Release);
                engine.lock().await.supersede_file_clipboard();
                break;
            }
        }
    });
    CancelPublication { flag, watcher }
}
#[allow(clippy::too_many_arguments)]
pub async fn receive(
    envelope: EventEnvelope,
    source_identity: String,
    source_name: String,
    // Authenticated public-key fingerprint of the sender. Admission budgets are
    // keyed on this, never on a connection or a display name (R2-04).
    budget_identity: [u8; 32],
    generation: u64,
    revision: Option<i64>,
    chunk_index: u16,
    engine: Arc<Mutex<SyncEngine>>,
    database: Arc<Mutex<HistoryDB>>,
    authorized: AuthorizationCheck,
) -> Result<CompositeAck, String> {
    receive_assembled(
        envelope,
        source_identity,
        source_name,
        budget_identity,
        generation,
        revision,
        chunk_index,
        engine,
        database,
        authorized,
        None,
        None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn receive_assembled(
    mut envelope: EventEnvelope,
    source_identity: String,
    source_name: String,
    // Authenticated public-key fingerprint of the sender. Admission budgets are
    // keyed on this, never on a connection or a display name (R2-04).
    budget_identity: [u8; 32],
    generation: u64,
    revision: Option<i64>,
    chunk_index: u16,
    engine: Arc<Mutex<SyncEngine>>,
    database: Arc<Mutex<HistoryDB>>,
    authorized: AuthorizationCheck,
    prevalidated: Option<ValidatedComposite>,
    assembly_budget: Option<composite::chunks::CompletedBudget>,
) -> Result<CompositeAck, String> {
    envelope
        .validate_timestamp(unix_timestamp_ms())
        .map_err(|e| e.to_string())?;
    // Account for wire/model/history clones, the PNG decoder + pixel workspace,
    // native HTML and helper buffers for the entire receive, before allocation.
    let base_memory = if assembly_budget.is_some() {
        0
    } else {
        (envelope.content.len() as u64).saturating_mul(3)
    };
    let assembly_budget = assembly_budget.map(Arc::new);
    let pipeline_lease = Arc::new(
        composite::budgets::global()
            .reserve_pipeline(base_memory.saturating_add(4 * 1024 * 1024))
            .map_err(|error| error.to_string())?,
    );
    let mut admission: Vec<crate::sync::AdmissionHold> = vec![pipeline_lease.clone()];
    if let Some(budget) = assembly_budget {
        admission.push(budget);
    }
    let content = std::mem::take(&mut envelope.content);
    let id = envelope.message_id;
    let timestamp = envelope.timestamp_ms;
    let identity = source_identity.clone();
    // Validate exactly once, off the async worker, and keep the canonical wire bytes.
    // R2-01: the ACK digest is then a hash of the *received* bytes instead of a fresh
    // `encode()` — previously that re-encode (and a copy of up to 32 MiB) ran on a
    // Tokio worker thread, and every later consumer re-ran the same validation.
    // Gate 1 — structure and resources. A failure here is a malformed packet: the
    // event is rejected and never enters history.
    let held = admission.clone();
    let (validated, key, source_inline_png) = tokio::task::spawn_blocking(move || {
        let _held = held;
        let e = EventEnvelope {
            message_id: id,
            timestamp_ms: timestamp,
            content,
        };
        let key = Receipt::new(identity, &e);
        let validated = match prevalidated {
            Some(validated) if validated.wire() == e.content => Ok(validated),
            Some(_) => Err("Composite validation identity mismatch".to_string()),
            None => ValidatedComposite::decode_owned(e.content).map_err(|e| e.to_string()),
        }?;
        let source_inline_png = validated.bundle().representations.iter().any(|r| {
            r.mime == composite::Mime::Html
                && r.bytes
                    .windows(22)
                    .any(|s| s.eq_ignore_ascii_case(b"data:image/png;base64,"))
        });
        Ok::<_, String>((validated, key, source_inline_png))
    })
    .await
    .map_err(|_| "Composite validation worker failed")??;
    let digest = validated.digest();
    let mut arrival = crate::notifications::ClipboardArrival {
        source_id: source_identity.clone(),
        source_name: source_name.clone(),
        event_id: envelope.message_id.as_hex(),
        kind: "content".into(),
        count: 1,
        entry_id: None,
    };
    // R4-02 fast path: an event that already reached a terminal result is answered
    // from the journal without queueing behind anything. This is what makes the lock
    // a *dedup* lock rather than a serialisation point.
    if let Some(existing) = composite_receipts::update(key.clone(), Operation::Lookup).await? {
        if existing.completed {
            let result = existing
                .result
                .ok_or("Completed composite event has no result")?;
            return Ok(CompositeAck {
                message_id: envelope.message_id,
                digest,
                chunk_index,
                result: result_from_byte(result)?,
            });
        }
    }
    // R4-02: bounded per-(authenticated source, event id) lock instead of one global
    // lock. The same event arriving over two sessions serialises; different events do
    // not block each other.
    let rgba_bytes = if source_inline_png {
        composite::RGBA_LIMIT as u64
    } else {
        validated.metrics().rgba_bytes as u64
    };
    let native_memory = Arc::new(
        composite::budgets::global()
            // The metric already includes four bytes per pixel. Reserve three
            // decoded workspaces, rather than multiplying the channel count again.
            .reserve_pipeline(
                rgba_bytes
                    .saturating_mul(3)
                    .saturating_add(if rgba_bytes > 0 { 32 * 1024 * 1024 } else { 0 }),
            )
            .map_err(|e| e.to_string())?,
    );
    admission.push(native_memory);
    let bundle = validated.shared_bundle();
    arrival.kind = if bundle.representations.len() == 1 {
        match bundle.representations[0].mime {
            composite::Mime::Text => "text",
            composite::Mime::Png => "image",
            _ => "content",
        }
    } else {
        "content"
    }
    .into();
    drop(validated);
    let permission = authorized.clone();
    if !tokio::task::spawn_blocking(move || permission())
        .await
        .map_err(|_| "Composite authorization worker failed")?
    {
        return Err("Composite authorization revoked".into());
    }
    let (target, preparation_superseded) = {
        let engine = engine.lock().await;
        (
            engine.restore_target(),
            engine.clipboard_generation() != generation
                || revision.is_none()
                || engine.clipboard_revision() != revision,
        )
    };
    // Gate 2 — native preparation, kept separate (R2-02). A payload can be
    // structurally valid and still be unpublishable here (a PNG that will not
    // decode, or HTML whose bound resources materialise past the native HTML
    // limit). That is not a bad packet: the content is still worth storing, so the
    // event is recorded as incomplete and the native effect is skipped — which also
    // means the platform never clears the clipboard for something it cannot write.
    let native_preparation = if preparation_superseded {
        Ok(None)
    } else {
        let decode_lease = Arc::new(
            composite::budgets::global()
                .admit_decode(&budget_identity)
                .map_err(|error| error.to_string())?,
        );
        let candidate = bundle.clone();
        let held = (admission.clone(), decode_lease.clone());
        // R2-04: PNG decoding is the expensive step here, so it runs under the decode
        // budget (1 per identity, 2 globally) and its pipeline footprint is reserved.
        let outcome = tokio::task::spawn_blocking(move || {
            let _held = held;
            let input = composite::restore::RestoreInput {
                source_platform: None,
                requested_format: None,
            };
            if candidate.completeness == Completeness::MissingFormats {
                // A missing source format can contain uncaptured body content.
                // Automatic publication needs a trustworthy captured document.
                return Err(crate::protocol::ProtocolError::InvalidComposite);
            }
            // Use the same default selection as history restoration: keep saved
            // images bound to their own nodes and missing images in body order.
            composite::restore::RestorePlan::portable_default(&candidate, input, target)
        })
        .await
        .map_err(|_| "Composite native-preparation worker failed")?;
        drop(decode_lease);
        outcome.map(Some)
    };
    let mut native_prepared = match native_preparation {
        Ok(plan) => plan,
        Err(error) => {
            tracing::warn!("Composite stored as incomplete: native preparation failed ({error})");
            None
        }
    };
    let available_content_selected = native_prepared.as_ref().is_some_and(|plan| {
        plan.selection() == composite::restore::RestoreSelection::AvailableContent
    });
    let _operation = crate::peer::operation_lock::global()
        .acquire(crate::peer::operation_lock::OperationKey::new(
            source_identity.clone(),
            envelope.message_id.as_hex(),
        ))
        .await?;
    let permission = authorized.clone();
    if !tokio::task::spawn_blocking(move || permission())
        .await
        .map_err(|_| "Composite authorization worker failed")?
    {
        return Err("Composite authorization revoked".into());
    }
    if !database.lock().await.is_storage_available() {
        return Err("Composite history storage is unavailable".into());
    }
    // Re-check under the lock: another session may have finished this event while we
    // waited for it.
    let existing = composite_receipts::update(key.clone(), Operation::Lookup).await?;
    let mut result = existing
        .as_ref()
        .and_then(|r| r.result)
        .map(result_from_byte)
        .transpose()?;
    if !existing.as_ref().is_some_and(|r| r.completed) {
        if existing.is_none() {
            let check = authorized.clone();
            if !tokio::task::spawn_blocking(move || check())
                .await
                .map_err(|_| "Composite authorization worker failed")?
            {
                return Err("Composite authorization revoked".into());
            }
            // K5: persist the recoverable encrypted content BEFORE the effect
            // claim. A disk-full error cannot change the user's clipboard.
            let stored_bundle = bundle.clone();
            let db = database.clone();
            let identity = source_identity.clone();
            let name = source_name.clone();
            let message_id = envelope.message_id;
            let timestamp = envelope.timestamp_ms;
            let held = admission.clone();
            let (history_ref, protection) = tokio::task::spawn_blocking(move || {
                let _held = held;
                let id = db
                    .blocking_lock()
                    .prepare_composite_event(
                        &stored_bundle,
                        &name,
                        (&identity, message_id, timestamp),
                    )
                    .map_err(|e| e.to_string())?;
                Ok::<_, String>((id, crate::db::PendingHistoryProtection::new(db, id)))
            })
            .await
            .map_err(|_| "Composite history worker failed")??;
            admission.push(protection);
            composite_receipts::update(key.clone(), Operation::ClaimPrepared(history_ref)).await?;
            result = Some(if history_ref <= 0 {
                AckResult::Superseded // a durable deletion tombstone must not resurrect content
            } else if bundle.completeness != Completeness::Complete && !available_content_selected {
                AckResult::StoredIncomplete
            } else if preparation_superseded {
                AckResult::Superseded
            } else if native_prepared.is_none() {
                AckResult::StoredIncomplete
            } else {
                // R3-01: prepare under the lock, publish with NO lock held, then
                // apply the outcome under the lock again. The previous shape held the
                // engine lock on a blocking thread for the whole native call, which
                // stalled the local monitor just as badly as holding it on an async
                // worker did.
                'publish: {
                    let publication: crate::sync::AdmissionHold =
                        Arc::new(crate::sync::publication_permit().await);
                    admission.push(publication);
                    // The authorization closure may itself take a blocking lock, so it
                    // is evaluated on the blocking pool — never on an async worker.
                    // Its answer is then carried into the gate, which is decided under
                    // the engine lock together with the generation check.
                    let authorized_now = tokio::task::spawn_blocking({
                        let check = authorized.clone();
                        move || check()
                    })
                    .await
                    .map_err(|_| "Composite authorization worker failed")?;
                    let gate = {
                        let mut engine = engine.lock().await;
                        match engine.prepare_composite_context(
                            bundle.clone(),
                            generation,
                            revision,
                            authorized_now,
                        ) {
                            Ok(gate) => gate,
                            Err(error) => return Err(error),
                        }
                    };
                    let prepared = match gate {
                        crate::sync::CompositePublishGate::Ready(prepared) => prepared,
                        // A lost race is a definite no-write, so it becomes a durable
                        // `Superseded` result rather than an error.
                        crate::sync::CompositePublishGate::Superseded => {
                            break 'publish AckResult::Superseded
                        }
                        crate::sync::CompositePublishGate::Unauthorized => {
                            composite_receipts::update(key.clone(), Operation::Remove).await?;
                            database.lock().await.release_history(history_ref);
                            return Err("Composite authorization revoked".into());
                        }
                    };
                    let port = {
                        let engine = engine.lock().await;
                        engine.clipboard_port()
                    };
                    let (_, write_revision) = prepared.into_parts();
                    let cancellation = watch_authorization(authorized.clone(), engine.clone());
                    let outcome = match port {
                        Some(port) => {
                            port.publish_restore_plan_admitted(
                                native_prepared
                                    .take()
                                    .ok_or("Prepared restore output unavailable")?,
                                write_revision,
                                cancellation.flag.clone(),
                                admission.clone(),
                            )
                            .await
                        }
                        None => Err(crate::sync::WriteFailure::may_have_effect(
                            "clipboard platform is unavailable",
                        )),
                    };
                    let settled = match outcome {
                        Ok(outcome) => {
                            // Phase 3 under a short lock.
                            let mut engine = engine.lock().await;
                            engine.finish_composite_publish(outcome)
                        }
                        Err(error) => {
                            tracing::warn!("Composite publish failed: {}", error.message());
                            break 'publish AckResult::ApplyUnknown;
                        }
                    };
                    // The typed outcome is what decides the ACK. Only a definite
                    // no-effect is retryable; everything else becomes a durable
                    // result, because the platform may already have changed.
                    match settled {
                        crate::composite::NativePublish::Published(_)
                            if available_content_selected =>
                        {
                            AckResult::StoredIncomplete
                        }
                        crate::composite::NativePublish::Published(_) => AckResult::Applied,
                        crate::composite::NativePublish::Partial(_) => AckResult::FailedPartial,
                        crate::composite::NativePublish::Superseded(_) => AckResult::Superseded,
                        crate::composite::NativePublish::NoEffect { .. } => {
                            composite_receipts::update(key.clone(), Operation::Remove).await?;
                            database.lock().await.release_history(history_ref);
                            return Err("Composite clipboard is busy; no effect attempted".into());
                        }
                        crate::composite::NativePublish::Unknown { detail } => {
                            tracing::warn!(
                                "Composite publish outcome unknown ({detail}); recording ApplyUnknown"
                            );
                            AckResult::ApplyUnknown
                        }
                    }
                }
            });
        }
        // A durable claim without a reported outcome after restart never
        // re-enters native I/O. Only history persistence is retried.
        let outcome = result.unwrap_or(AckResult::ApplyUnknown);
        composite_receipts::update(key.clone(), Operation::Report(outcome as u8)).await?;
        if existing
            .as_ref()
            .is_some_and(|record| record.history_ref.is_none())
        {
            // Upgrade an old unresolved claim without repeating its native effect.
            let message_id = envelope.message_id;
            let timestamp = envelope.timestamp_ms;
            let stored_bundle = bundle;
            let db = database.clone();
            let held = admission.clone();
            tokio::task::spawn_blocking(move || {
                let _held = held;
                db.blocking_lock()
                    .add_composite_event(
                        &stored_bundle,
                        None,
                        &source_name,
                        Some((&source_identity, message_id, timestamp)),
                    )
                    .map_err(|e| e.to_string())
            })
            .await
            .map_err(|_| "Composite history worker failed")??;
        }
        let completed = composite_receipts::update(key, Operation::Complete).await?;
        if let Some(id) = completed.and_then(|record| record.history_ref) {
            database.lock().await.release_history(id);
            if id > 0 {
                arrival.entry_id = Some(id);
                if let Some(platform) = engine.lock().await.platform_handle() {
                    platform.received_content(arrival);
                }
            }
        }
        result = Some(outcome);
    }
    Ok(CompositeAck {
        message_id: envelope.message_id,
        digest,
        chunk_index,
        result: result.ok_or("Missing composite terminal result")?,
    })
}
fn result_from_byte(v: u8) -> Result<AckResult, String> {
    match v {
        1 => Ok(AckResult::Applied),
        2 => Ok(AckResult::Superseded),
        3 => Ok(AckResult::ApplyUnknown),
        4 => Ok(AckResult::FailedPartial),
        5 => Ok(AckResult::StoredIncomplete),
        _ => Err("Invalid composite result".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composite::{
        packed_to_png, Completeness, Mime, PortableBundle, Representation, Resource,
        ResourceBinding,
    };
    use crate::protocol::{
        native_clipboard::{PublishReceipt, PublishState},
        TransferId,
    };
    use crate::sync::{FileBatchProgress, FileReceiveCommit, PlatformResultFuture, SyncPlatform};
    use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
    struct Platform {
        arrivals: std::sync::Mutex<Vec<crate::notifications::ClipboardArrival>>,
        writes: AtomicUsize,
        published: std::sync::Mutex<Vec<PortableBundle>>,
        revision: AtomicI64,
        result: std::sync::Mutex<PublishState>,
    }
    impl SyncPlatform for Platform {
        fn received_content(&self, arrival: crate::notifications::ClipboardArrival) {
            self.arrivals.lock().unwrap().push(arrival);
        }
        fn write_text(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn write_image(&self, _: u32, _: u32, _: &[u8]) -> Result<(), String> {
            Ok(())
        }
        fn clipboard_revision(&self) -> Option<i64> {
            Some(self.revision.load(Ordering::SeqCst))
        }
        fn write_composite(
            &self,
            bundle: &PortableBundle,
            expected: Option<i64>,
        ) -> Result<crate::composite::NativePublish, String> {
            use crate::composite::NativePublish;
            if expected != self.clipboard_revision() {
                return Ok(NativePublish::Superseded(PublishReceipt {
                    state: PublishState::Superseded,
                    revision: self.revision.load(Ordering::SeqCst),
                    token: [0; 16],
                }));
            }
            let state = *self.result.lock().unwrap();
            if matches!(state, PublishState::Published | PublishState::Partial) {
                self.writes.fetch_add(1, Ordering::SeqCst);
                self.published.lock().unwrap().push(bundle.clone());
                self.revision.fetch_add(1, Ordering::SeqCst);
            }
            let receipt = PublishReceipt {
                state,
                revision: self.revision.load(Ordering::SeqCst),
                token: [1; 16],
            };
            Ok(match state {
                PublishState::Published => NativePublish::Published(receipt),
                PublishState::Partial => NativePublish::Partial(receipt),
                PublishState::Superseded => NativePublish::Superseded(receipt),
                PublishState::Failed => NativePublish::Unknown {
                    detail: "test double reported a failed publish".into(),
                },
            })
        }
        fn set_file_progress(&self, _: &str, _: u64, _: u64) {}
        fn clear_file_progress(&self, _: Option<TransferId>, _: Option<&str>) {}
        fn set_file_batch_progress(&self, _: FileBatchProgress) {}
        fn files_received(&self, _: FileReceiveCommit) -> PlatformResultFuture {
            Box::pin(async { Ok(()) })
        }
        fn file_batch_failed(&self, _: Option<TransferId>, _: &str) {}
    }
    fn event() -> EventEnvelope {
        EventEnvelope::new(
            PortableBundle {
                representations: vec![
                    composite::Representation {
                        mime: composite::Mime::Text,
                        bytes: b"receiver fixture".to_vec(),
                    },
                    composite::Representation {
                        mime: composite::Mime::Html,
                        bytes: b"<b>receiver fixture</b>".to_vec(),
                    },
                ],
                resources: vec![],
                bindings: vec![],
                completeness: Completeness::Complete,
            }
            .encode()
            .unwrap(),
        )
    }
    async fn setup() -> (
        std::path::PathBuf,
        Arc<Mutex<HistoryDB>>,
        Arc<Mutex<SyncEngine>>,
        Arc<Platform>,
        String,
    ) {
        let root = std::env::temp_dir().join(format!(
            "tailsync-composite-receiver-{:016x}",
            rand::random::<u64>()
        ));
        let db = Arc::new(Mutex::new(
            HistoryDB::open_isolated_for_test_composite(&root).unwrap(),
        ));
        let platform = Arc::new(Platform {
            arrivals: std::sync::Mutex::new(Vec::new()),
            writes: AtomicUsize::new(0),
            published: std::sync::Mutex::new(Vec::new()),
            revision: AtomicI64::new(7),
            result: std::sync::Mutex::new(PublishState::Published),
        });
        let mut engine = SyncEngine::new();
        engine.set_platform(platform.clone());
        (
            root,
            db,
            Arc::new(Mutex::new(engine)),
            platform,
            format!("{:064x}", rand::random::<u64>()),
        )
    }
    async fn run(
        e: EventEnvelope,
        source: &str,
        engine: &Arc<Mutex<SyncEngine>>,
        db: &Arc<Mutex<HistoryDB>>,
    ) -> Result<CompositeAck, String> {
        receive(
            e,
            source.into(),
            "fixture".into(),
            // A distinct fingerprint per source keeps the budget ledger from
            // coupling otherwise independent tests.
            *blake3::hash(source.as_bytes()).as_bytes(),
            0,
            Some(7),
            0,
            engine.clone(),
            db.clone(),
            Arc::new(|| true),
        )
        .await
    }
    #[tokio::test]
    async fn revocation_after_validation_prevents_both_native_effect_and_history() {
        let _pipeline = crate::composite::budgets::pipeline_budget_test_lock_async().await;
        let _warnings = crate::sync_warning::test_lock_async().await;
        while crate::sync_warning::take().is_some() {}
        let (root, db, engine, platform, source) = setup().await;
        let checks = Arc::new(AtomicUsize::new(0));
        let check = checks.clone();
        let result = receive(
            event(),
            source.clone(),
            "fixture".into(),
            *blake3::hash(source.as_bytes()).as_bytes(),
            0,
            Some(7),
            0,
            engine,
            db.clone(),
            Arc::new(move || check.fetch_add(1, Ordering::SeqCst) == 0),
        )
        .await;
        assert!(result.unwrap_err().contains("authorization revoked"));
        assert_eq!(checks.load(Ordering::SeqCst), 2);
        assert_eq!(platform.writes.load(Ordering::SeqCst), 0);
        assert!(db
            .lock()
            .await
            .get_all(None, None, 10, 0)
            .unwrap()
            .is_empty());
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn o10_revoked_or_superseded_events_skip_busy_native_preparation() {
        let _pipeline = crate::composite::budgets::pipeline_budget_test_lock_async().await;
        let _warnings = crate::sync_warning::test_lock_async().await;
        while crate::sync_warning::take().is_some() {}
        for authorized in [false, true] {
            let (root, db, engine, platform, source) = setup().await;
            let identity = *blake3::hash(source.as_bytes()).as_bytes();
            // Exhaust this peer's decode slot. Admission must still reject a
            // revoked event, or store a stale event, before native conversion.
            let lease = composite::budgets::global()
                .admit_decode(&identity)
                .unwrap();
            if authorized {
                engine.lock().await.supersede_file_clipboard();
            }
            let result = receive(
                event(),
                source,
                "fixture".into(),
                identity,
                0,
                Some(7),
                0,
                engine,
                db.clone(),
                Arc::new(move || authorized),
            )
            .await;
            if authorized {
                assert_eq!(result.unwrap().result, AckResult::Superseded);
            } else {
                assert!(result.unwrap_err().contains("authorization revoked"));
            }
            assert_eq!(platform.writes.load(Ordering::SeqCst), 0);
            assert_eq!(
                db.lock().await.get_all(None, None, 10, 0).unwrap().len(),
                usize::from(authorized)
            );
            drop(lease);
            drop(db);
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[tokio::test]
    async fn failed_history_prevents_effect_and_deleted_history_stays_deleted() {
        let _pipeline = crate::composite::budgets::pipeline_budget_test_lock_async().await;
        let _warnings = crate::sync_warning::test_lock_async().await;
        while crate::sync_warning::take().is_some() {}
        let (root, db, engine, platform, source) = setup().await;
        let e = event();
        db.lock()
            .await
            .reject_composite_inserts_for_test(true)
            .unwrap();
        assert!(run(e.clone(), &source, &engine, &db).await.is_err());
        assert_eq!(platform.writes.load(Ordering::SeqCst), 0);
        db.lock()
            .await
            .reject_composite_inserts_for_test(false)
            .unwrap();
        let ack = run(e.clone(), &source, &engine, &db).await.unwrap();
        assert_eq!(ack.result, AckResult::Applied);
        let rows = db.lock().await.get_all(None, None, 10, 0).unwrap();
        assert_eq!(rows.len(), 1);
        db.lock().await.delete(rows[0].id).unwrap();
        assert_eq!(
            run(e, &source, &engine, &db).await.unwrap().result,
            AckResult::Applied
        );
        assert!(db
            .lock()
            .await
            .get_all(None, None, 10, 0)
            .unwrap()
            .is_empty());
        assert_eq!(platform.writes.load(Ordering::SeqCst), 1);
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn crash_after_claim_saves_history_as_unknown_without_a_native_effect() {
        let _pipeline = crate::composite::budgets::pipeline_budget_test_lock_async().await;
        let _warnings = crate::sync_warning::test_lock_async().await;
        while crate::sync_warning::take().is_some() {}
        let (root, db, engine, platform, source) = setup().await;
        let e = event();
        composite_receipts::update(Receipt::new(source.clone(), &e), Operation::Claim)
            .await
            .unwrap();
        assert_eq!(
            run(e.clone(), &source, &engine, &db).await.unwrap().result,
            AckResult::ApplyUnknown
        );
        assert_eq!(
            run(e, &source, &engine, &db).await.unwrap().result,
            AckResult::ApplyUnknown
        );
        assert_eq!(platform.writes.load(Ordering::SeqCst), 0);
        assert_eq!(db.lock().await.get_all(None, None, 10, 0).unwrap().len(), 1);
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn newer_native_revision_and_partial_effect_have_distinct_durable_results() {
        let _pipeline = crate::composite::budgets::pipeline_budget_test_lock_async().await;
        let _warnings = crate::sync_warning::test_lock_async().await;
        while crate::sync_warning::take().is_some() {}
        let (root, db, engine, platform, source) = setup().await;
        platform.revision.store(8, Ordering::SeqCst);
        assert_eq!(
            run(event(), &source, &engine, &db).await.unwrap().result,
            AckResult::Superseded
        );
        assert_eq!(platform.writes.load(Ordering::SeqCst), 0);
        platform.revision.store(7, Ordering::SeqCst);
        *platform.result.lock().unwrap() = PublishState::Partial;
        let e = event();
        assert_eq!(
            run(e.clone(), &source, &engine, &db).await.unwrap().result,
            AckResult::FailedPartial
        );
        assert_eq!(
            run(e, &source, &engine, &db).await.unwrap().result,
            AckResult::FailedPartial
        );
        assert_eq!(platform.writes.load(Ordering::SeqCst), 1);
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
    /// R2-02 / T04 — the structural gate and the native-preparation gate are
    /// separate, and only the first one decides whether content is worth keeping.
    ///
    /// A payload can be perfectly legal on the wire and still be unpublishable: here
    /// the HTML plus its base64-inlined resource materialises past the native HTML
    /// limit. Before R2-02 that was only discovered *during* materialisation, i.e.
    /// after the platform had already cleared the clipboard. It must instead be
    /// stored as incomplete with no native effect at all.
    #[tokio::test]
    async fn native_preparation_failure_stores_incomplete_without_a_native_effect() {
        let _pipeline = crate::composite::budgets::pipeline_budget_test_lock_async().await;
        let _warnings = crate::sync_warning::test_lock_async().await;
        while crate::sync_warning::take().is_some() {}
        let (root, db, engine, platform, source) = setup().await;

        // 1.9 MiB of HTML with one binding, plus a 200x200 incompressible PNG whose
        // base64 form pushes the materialised HTML past the 2 MiB limit.
        let needle = b"tailsync-resource:1";
        let mut html = vec![b'a'; 1_900_000];
        html[..needle.len()].copy_from_slice(needle);
        let mut rgba = vec![0_u8; 200 * 200 * 4];
        let mut state = 0x1234_5678_u32;
        for byte in rgba.iter_mut() {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *byte = (state >> 24) as u8;
        }
        let bundle = PortableBundle {
            representations: vec![Representation {
                mime: Mime::Html,
                bytes: html,
            }],
            resources: vec![Resource {
                id: 1,
                png: packed_to_png(200, 200, &rgba).unwrap(),
            }],
            bindings: vec![ResourceBinding {
                representation: 0,
                resource: 1,
                offset: 0,
                length: needle.len() as u32,
            }],
            completeness: Completeness::Complete,
        };

        // Structural gate: legal. Native-preparation gate: refused.
        assert!(
            bundle.validate().is_ok(),
            "the sample must be a structurally legal composite"
        );
        assert!(
            crate::composite::validate_for_publish(&bundle).is_err(),
            "the sample must fail native preparation, which is the point of the test"
        );
        // The failure is raised before any allocation of the materialised form.
        assert!(
            crate::composite::html::materialized_len(&bundle, 0).is_err(),
            "the overrun must be detectable without materialising"
        );

        let mut e = event();
        e.content = bundle.encode().unwrap();
        assert_eq!(
            run(e, &source, &engine, &db).await.unwrap().result,
            AckResult::StoredIncomplete,
            "an unpublishable but recoverable payload is stored, not rejected"
        );
        assert_eq!(
            platform.writes.load(Ordering::SeqCst),
            0,
            "the platform must never be asked to publish something it cannot write"
        );
        assert_eq!(
            db.lock().await.get_all(None, None, 10, 0).unwrap().len(),
            1,
            "the content is still recoverable from history"
        );

        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn unbound_html_publishes_available_body_without_trusting_wire_completeness() {
        let _pipeline = crate::composite::budgets::pipeline_budget_test_lock_async().await;
        let (root, db, engine, platform, source) = setup().await;
        let mut e = event();
        let mut b = PortableBundle::decode(&e.content).unwrap();
        b.representations[1].bytes =
            b"<p>recoverable</p><img src='https://fixture.invalid/image.png'>".to_vec();
        assert_eq!(b.completeness, Completeness::Complete);
        assert!(b.validate().is_ok());
        e.content = b.encode().unwrap();
        let ack = run(e.clone(), &source, &engine, &db).await.unwrap();
        assert_eq!(ack.result, AckResult::StoredIncomplete);
        assert_eq!(
            run(e, &source, &engine, &db).await.unwrap().result,
            AckResult::StoredIncomplete
        );
        assert_eq!(platform.writes.load(Ordering::SeqCst), 1);
        let published = platform.published.lock().unwrap().clone();
        let html = std::str::from_utf8(published[0].representation(Mime::Html).unwrap()).unwrap();
        assert!(html.contains("recoverable"));
        assert!(html.contains("[🖼]"));
        assert!(!html.contains("fixture.invalid"));
        let database = db.lock().await;
        let rows = database.get_all(None, None, 10, 0).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(database.get_composite(rows[0].id).unwrap(), b);
        drop(database);
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn browser_image_is_applied_once_without_replaying_unresolved_html() {
        let _pipeline = crate::composite::budgets::pipeline_budget_test_lock_async().await;
        let (root, db, engine, platform, source) = setup().await;
        let png = composite::packed_to_png(1, 1, &[1, 2, 3, 255]).unwrap();
        let bundle = PortableBundle {
            representations: vec![
                composite::Representation {
                    mime: composite::Mime::Html,
                    bytes: b"<img src='file:///C:/browser/image.png'>".to_vec(),
                },
                composite::Representation {
                    mime: composite::Mime::Png,
                    bytes: png.clone(),
                },
            ],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::ExternalReferences,
        };
        let event = EventEnvelope::new(bundle.encode().unwrap());
        assert_eq!(
            run(event.clone(), &source, &engine, &db)
                .await
                .unwrap()
                .result,
            AckResult::StoredIncomplete
        );
        assert_eq!(
            run(event, &source, &engine, &db).await.unwrap().result,
            AckResult::StoredIncomplete
        );
        let arrivals = platform.arrivals.lock().unwrap().clone();
        assert_eq!(arrivals.len(), 1);
        assert_eq!(arrivals[0].source_id, source);
        assert!(arrivals[0].entry_id.unwrap() > 0);
        assert_eq!(platform.writes.load(Ordering::SeqCst), 1);
        {
            let published = platform.published.lock().unwrap();
            assert_eq!(published.len(), 1);
            assert_eq!(published[0].representations.len(), 1);
            assert_eq!(
                published[0].representation(composite::Mime::Png).unwrap(),
                png
            );
            assert_eq!(published[0].completeness, Completeness::Complete);
        }
        let database = db.lock().await;
        let rows = database.get_all(None, None, 10, 0).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(database.get_composite(rows[0].id).unwrap(), bundle);
        drop(database);
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }

    fn web_article(stylesheet: bool, missing_image: bool) -> PortableBundle {
        let style = if stylesheet {
            "<style>@import 'https://fixture.invalid/page.css';</style>"
        } else {
            ""
        };
        let missing = if missing_image {
            "<img src='https://fixture.invalid/missing.png'>"
        } else {
            ""
        };
        let html = format!("{style}<p>Alpha<img src='tailsync-resource:7'>Beta{missing}Gamma<img src='tailsync-resource:3'>Delta<img src='tailsync-resource:7'>Omega</p>");
        let bindings = [7, 3]
            .into_iter()
            .flat_map(|id| {
                let reference = format!("tailsync-resource:{id}");
                html.match_indices(&reference)
                    .map(|(offset, _)| ResourceBinding {
                        representation: 1,
                        resource: id,
                        offset: offset as u32,
                        length: reference.len() as u32,
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        PortableBundle {
            representations: vec![
                Representation {
                    mime: Mime::Text,
                    bytes: b"AlphaBetaGammaDeltaOmega".to_vec(),
                },
                Representation {
                    mime: Mime::Html,
                    bytes: html.into_bytes(),
                },
            ],
            resources: vec![
                Resource {
                    id: 7,
                    png: packed_to_png(1, 1, &[255, 0, 0, 255]).unwrap(),
                },
                Resource {
                    id: 3,
                    png: packed_to_png(1, 1, &[0, 255, 0, 255]).unwrap(),
                },
            ],
            bindings,
            completeness: if stylesheet || missing_image {
                Completeness::ExternalReferences
            } else {
                Completeness::Complete
            },
        }
    }

    #[tokio::test]
    async fn web_article_sync_keeps_text_and_repeated_images_in_place_and_publishes_once() {
        use base64::Engine as _;
        let _pipeline = crate::composite::budgets::pipeline_budget_test_lock_async().await;
        for (stylesheet, missing_image) in
            [(false, false), (false, true), (true, false), (true, true)]
        {
            let (root, db, engine, platform, source) = setup().await;
            let original = web_article(stylesheet, missing_image);
            let expected = if stylesheet || missing_image {
                AckResult::StoredIncomplete
            } else {
                AckResult::Applied
            };
            let event = EventEnvelope::new(original.encode().unwrap());
            let digest = original.digest().unwrap();
            let ack = run(event.clone(), &source, &engine, &db).await.unwrap();
            assert_eq!(ack.result, expected);
            assert_eq!(ack.digest, digest);
            assert_eq!(
                run(event, &source, &engine, &db).await.unwrap().result,
                expected
            );
            assert_eq!(platform.writes.load(Ordering::SeqCst), 1);
            let published = platform.published.lock().unwrap().clone();
            let bundle = &published[0];
            assert!(composite::restore::dependencies_complete(bundle).unwrap());
            let index = bundle
                .representations
                .iter()
                .position(|r| r.mime == Mime::Html)
                .unwrap();
            let html =
                String::from_utf8(composite::html::materialize(bundle, index).unwrap()).unwrap();
            let red = base64::engine::general_purpose::STANDARD.encode(&original.resources[0].png);
            let green =
                base64::engine::general_purpose::STANDARD.encode(&original.resources[1].png);
            let mut tail = html.as_str();
            let mut expected_order = vec!["Alpha", red.as_str(), "Beta"];
            if missing_image {
                expected_order.push("[🖼]");
            }
            expected_order.extend(["Gamma", green.as_str(), "Delta", red.as_str(), "Omega"]);
            for token in expected_order {
                let position = tail
                    .find(token)
                    .unwrap_or_else(|| panic!("missing or reordered {token} in {html}"));
                tail = &tail[position + token.len()..];
            }
            assert_eq!(html.matches("data:image/png;base64,").count(), 3);
            assert!(!html.contains("fixture.invalid"));
            let database = db.lock().await;
            let rows = database.get_all(None, None, 10, 0).unwrap();
            assert_eq!(rows.len(), 1);
            assert_eq!(database.get_composite(rows[0].id).unwrap(), original);
            drop(database);
            drop(db);
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[tokio::test]
    async fn web_article_fallback_preserves_partial_unknown_and_superseded_results() {
        let _pipeline = crate::composite::budgets::pipeline_budget_test_lock_async().await;
        for (state, expected) in [
            (PublishState::Partial, AckResult::FailedPartial),
            (PublishState::Failed, AckResult::ApplyUnknown),
            (PublishState::Superseded, AckResult::Superseded),
        ] {
            let (root, db, engine, platform, source) = setup().await;
            *platform.result.lock().unwrap() = state;
            let event = EventEnvelope::new(web_article(false, true).encode().unwrap());
            assert_eq!(
                run(event.clone(), &source, &engine, &db)
                    .await
                    .unwrap()
                    .result,
                expected
            );
            assert_eq!(
                run(event, &source, &engine, &db).await.unwrap().result,
                expected
            );
            assert_eq!(
                platform.writes.load(Ordering::SeqCst),
                usize::from(state == PublishState::Partial)
            );
            drop(db);
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[tokio::test]
    async fn web_article_missing_formats_or_newer_copy_never_automatically_publish_a_fallback() {
        let _pipeline = crate::composite::budgets::pipeline_budget_test_lock_async().await;
        for missing_formats in [false, true] {
            let (root, db, engine, platform, source) = setup().await;
            let mut bundle = web_article(false, true);
            if missing_formats {
                bundle.completeness = Completeness::MissingFormats;
            } else {
                engine.lock().await.supersede_file_clipboard();
            }
            let event = EventEnvelope::new(bundle.encode().unwrap());
            assert_eq!(
                run(event, &source, &engine, &db).await.unwrap().result,
                AckResult::StoredIncomplete
            );
            assert_eq!(platform.writes.load(Ordering::SeqCst), 0);
            drop(db);
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[tokio::test]
    async fn incomplete_and_superseded_generation_only_store_history() {
        let _pipeline = crate::composite::budgets::pipeline_budget_test_lock_async().await;
        let _warnings = crate::sync_warning::test_lock_async().await;
        while crate::sync_warning::take().is_some() {}
        let (root, db, engine, platform, source) = setup().await;
        engine.lock().await.supersede_file_clipboard();
        assert_eq!(
            run(event(), &source, &engine, &db).await.unwrap().result,
            AckResult::Superseded
        );
        let mut e = event();
        let mut b = PortableBundle::decode(&e.content).unwrap();
        b.completeness = Completeness::ExternalReferences;
        e.content = b.encode().unwrap();
        assert_eq!(
            run(e, &source, &engine, &db).await.unwrap().result,
            AckResult::StoredIncomplete
        );
        assert_eq!(platform.writes.load(Ordering::SeqCst), 0);
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
}
