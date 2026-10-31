//! Inbound reliable event processing for authenticated peer connections.
//!
//! Shared by the macOS and Windows network layers (T106 migration). A
//! reliable text/image event is decoded, timestamp-checked, de-duplicated,
//! prepared in durable history, claimed, then applied through an unlocked
//! clipboard port and acknowledged. `on_applied` lets the caller run platform side
//! effects (clipboard version bump) at the exact position of the original
//! server code — before the acknowledgement is written.

use super::event_receipts::{self, Operation, Receipt};
use crate::db::HistoryDB;
use crate::protocol::{unix_timestamp_ms, Command, EventEnvelope, Frame};
use crate::secure::SecureConnection;
use crate::sync::SyncEngine;
use log::{debug, info};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::Instrument;

/// How the reliable event was handled.
#[derive(Debug, PartialEq, Eq)]
pub enum ReliableEventOutcome {
    Applied,
    Duplicate,
}

/// Failure while applying an event.  Permanent protocol/data failures are
/// sent back as a PeerError; transient clipboard or storage failures leave
/// the frame unacknowledged so the sender's normal timeout/retry path can
/// make progress without permanently dropping a fresh clipboard event.
#[derive(Debug, PartialEq, Eq)]
pub enum ReliableEventError {
    Permanent(String),
    Retryable(String),
}

impl ReliableEventError {
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Retryable(_))
    }

    fn permanent(message: impl Into<String>) -> Self {
        Self::Permanent(message.into())
    }

    fn retryable(message: impl Into<String>) -> Self {
        Self::Retryable(message.into())
    }
}

impl std::fmt::Display for ReliableEventError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Permanent(message) | Self::Retryable(message) => f.write_str(message),
        }
    }
}

/// Processes one inbound reliable event frame and writes its acknowledgement.
///
/// Error strings are part of the observable wire contract (the caller
/// forwards them to the peer); keep them stable.
pub async fn process_reliable_event(
    stream: &mut SecureConnection,
    frame: &Frame,
    source: &str,
    sync_engine: &Arc<Mutex<SyncEngine>>,
    database: &Arc<Mutex<HistoryDB>>,
    last_sequence: &mut Option<u32>,
    on_applied: impl FnOnce(),
) -> Result<ReliableEventOutcome, ReliableEventError> {
    let envelope = EventEnvelope::decode(&frame.payload)
        .map_err(|error| ReliableEventError::permanent(error.to_string()))?;
    let session_id = stream.session_id().to_string();
    let event_span = tracing::info_span!(
        "receive.event",
        session_id = %session_id,
        peer = %source,
        message_id = %envelope.message_id.as_hex(),
        sequence = frame.sequence,
        command = ?frame.command,
    );
    envelope
        .validate_timestamp(unix_timestamp_ms())
        .map_err(|error| ReliableEventError::permanent(error.to_string()))?;

    let receipt_source = crate::secure::fingerprint(stream.remote_public_key());
    let receipt = Receipt::new(receipt_source.clone(), frame.command, &envelope);
    let existing = event_receipts::update(receipt.clone(), Operation::Lookup)
        .await
        .map_err(receipt_error)?;
    let duplicate = existing.as_ref().is_some_and(|r| r.received)
        || sync_engine
            .lock()
            .await
            .has_seen_message(&receipt_source, envelope.message_id);
    if last_sequence.is_some_and(|last| frame.sequence <= last) && !duplicate {
        return Err(ReliableEventError::permanent(format!(
            "replayed or out-of-order event sequence {}",
            frame.sequence
        )));
    }

    let outcome = if !duplicate {
        let kind = match frame.command {
            Command::TextPayload => "text",
            Command::ImagePayload => "image",
            _ => {
                return Err(ReliableEventError::permanent(format!(
                    "{:?} is not a reliable event command",
                    frame.command
                )))
            }
        };
        let history_ref = process_event_content(
            frame.command,
            &envelope.content,
            source,
            envelope.message_id,
            receipt.clone(),
            sync_engine,
            database,
        )
        .instrument(event_span.clone())
        .await?;
        info!(
            "event_applied kind={kind} source={source} session_id={session_id} message_id={} sequence={}",
            envelope.message_id.as_hex(),
            frame.sequence
        );
        {
            let mut engine = sync_engine.lock().await;
            engine.record_message(source, envelope.message_id);
            engine.record_message(&receipt_source, envelope.message_id);
            // The history write committed, so the intermediate "applied" state
            // is no longer needed; the event is now fully seen.
            engine.clear_applied_event(source, envelope.message_id);
            if let Some(history_ref) = history_ref {
                if let Some(platform) = engine.platform_handle() {
                    platform.received_content(crate::notifications::ClipboardArrival {
                        source_id: receipt_source.clone(),
                        source_name: source.to_string(),
                        event_id: envelope.message_id.as_hex(),
                        kind: kind.into(),
                        count: 1,
                        entry_id: Some(history_ref),
                    });
                }
            }
        }
        on_applied();
        ReliableEventOutcome::Applied
    } else {
        debug!(
            "event_duplicate source={source} session_id={session_id} message_id={} sequence={}",
            envelope.message_id.as_hex(),
            frame.sequence
        );
        ReliableEventOutcome::Duplicate
    };

    if last_sequence.is_none_or(|last| frame.sequence > last) {
        *last_sequence = Some(frame.sequence);
    }
    let ack = Frame::try_new(
        Command::EventAck,
        0,
        frame.sequence,
        envelope.message_id.ack_payload(),
    )
    .map_err(|error| ReliableEventError::permanent(error.to_string()))?;
    stream
        .write_frame(&ack)
        .await
        .map_err(|error| ReliableEventError::retryable(error.to_string()))?;
    tracing::debug!(
        session_id = %session_id,
        peer = %source,
        message_id = %envelope.message_id.as_hex(),
        sequence = frame.sequence,
        "event acknowledgement sent"
    );
    Ok(outcome)
}

async fn process_event_content(
    command: Command,
    content: &[u8],
    source: &str,
    message_id: crate::protocol::MessageId,
    receipt: Receipt,
    sync_engine: &Arc<Mutex<SyncEngine>>,
    database: &Arc<Mutex<HistoryDB>>,
) -> Result<Option<i64>, ReliableEventError> {
    let text = match command {
        Command::TextPayload => Some(
            String::from_utf8(content.to_vec())
                .map_err(|_| ReliableEventError::permanent("text event is not valid UTF-8"))?,
        ),
        Command::ImagePayload => {
            crate::protocol::PackedImage::try_from(content)
                .map_err(|error| ReliableEventError::permanent(error.to_string()))?;
            None
        }
        _ => {
            return Err(ReliableEventError::permanent(format!(
                "{command:?} is not a reliable event command"
            )))
        }
    };
    let (arrival_generation, expected_revision) = sync_engine
        .lock()
        .await
        .legacy_arrival(source, message_id)
        .map_err(ReliableEventError::retryable)?;
    if !database.lock().await.is_storage_available() {
        crate::sync_warning::record_receive_not_saved(source);
        return Err(ReliableEventError::retryable(
            "History storage is unavailable; native effect was not attempted",
        ));
    }
    let _operation = super::operation_lock::global()
        .acquire(receipt.operation_key())
        .await
        .map_err(receipt_error)?;
    let existing = event_receipts::update(receipt.clone(), Operation::Lookup)
        .await
        .map_err(receipt_error)?;
    if existing.as_ref().is_some_and(|record| record.received) {
        // Another session completed this event while we waited; there is no new arrival to announce.
        return Ok(None);
    }
    let (history_ref, protection) =
        if let Some(id) = existing.as_ref().and_then(|record| record.history_ref) {
            (
                id,
                crate::db::PendingHistoryProtection::new(database.clone(), id),
            ) // deletion breaks the reference; never recreate acknowledged content
        } else {
            let db = database.clone();
            let bytes = content.to_vec();
            let stored_source = source.to_string();
            let saved = tokio::task::spawn_blocking(move || {
                let id = {
                    let mut guard = db.blocking_lock();
                    if command == Command::TextPayload {
                        guard.prepare_legacy_text(
                            std::str::from_utf8(&bytes).expect("validated UTF-8"),
                            &stored_source,
                        )
                    } else {
                        guard.prepare_legacy_image(&bytes, &stored_source)
                    }
                    .map_err(|error| error.to_string())?
                };
                Ok::<_, String>((id, crate::db::PendingHistoryProtection::new(db, id)))
            })
            .await
            .map_err(|error| ReliableEventError::retryable(error.to_string()))?;
            match saved {
                Ok(saved) => saved,
                Err(error) => {
                    // Persist a conservative suppression marker: after restart a retry
                    // may recover history, but cannot acquire freshness and overwrite
                    // a newer copy. No native effect has been started.
                    event_receipts::update(receipt.clone(), Operation::Claim)
                        .await
                        .map_err(receipt_error)?;
                    crate::sync_warning::record_receive_not_saved(source);
                    return Err(ReliableEventError::retryable(error));
                }
            }
        };
    let claimed = match event_receipts::update(
        receipt.clone(),
        Operation::ClaimPrepared(history_ref),
    )
    .await
    {
        Ok(record) => record.is_some(),
        Err(error) => {
            database.lock().await.release_history(history_ref);
            return Err(receipt_error(error));
        }
    };
    let publication: crate::sync::AdmissionHold = Arc::new(crate::sync::publication_permit().await);
    if !claimed
        && expected_revision.is_some()
        && sync_engine.lock().await.clipboard_generation() == arrival_generation
    {
        // Metadata commits and helper I/O never hold the engine mutex.
        let port = sync_engine.lock().await.clipboard_port();
        let error = if let Some(text) = text {
            let prepared = sync_engine
                .lock()
                .await
                .prepare_incoming_text(&text)
                .map_err(ReliableEventError::retryable)?;
            let result = match port {
                Some(port) => {
                    port.write_text_admitted(
                        text,
                        expected_revision,
                        vec![publication.clone(), protection.clone()],
                    )
                    .await
                }
                None => Err(crate::sync::WriteFailure::no_effect(
                    "clipboard unavailable",
                )),
            };
            sync_engine
                .lock()
                .await
                .finish_incoming_text(prepared, result)
                .err()
        } else {
            let prepared = sync_engine
                .lock()
                .await
                .prepare_incoming_image(content)
                .map_err(ReliableEventError::retryable)?;
            let result = match port {
                Some(port) => {
                    port.write_image_admitted(
                        prepared.width,
                        prepared.height,
                        prepared.rgba.clone(),
                        expected_revision,
                        vec![publication.clone(), protection.clone()],
                    )
                    .await
                }
                None => Err(crate::sync::WriteFailure::no_effect(
                    "clipboard unavailable",
                )),
            };
            sync_engine
                .lock()
                .await
                .finish_incoming_image(prepared, result)
                .err()
        };
        if let Some(failure) = error.filter(|failure| !failure.is_superseded()) {
            if failure.is_proven_no_effect() {
                event_receipts::update(receipt.clone(), Operation::Remove)
                    .await
                    .map_err(receipt_error)?;
            } else {
                crate::sync_warning::record_composite_result(
                    source,
                    crate::protocol::composite::AckResult::ApplyUnknown,
                );
            }
            database.lock().await.release_history(history_ref);
            // The peer-visible text is the legacy façade: only a provable no-effect
            // adds the marker, and nothing on this path reads it back.
            return Err(ReliableEventError::retryable(failure.message()));
        }
        sync_engine
            .lock()
            .await
            .mark_event_applied(source, message_id);
    }
    if database.lock().await.is_storage_available() {
        event_receipts::update(receipt, Operation::Complete)
            .await
            .map_err(receipt_error)?;
    }
    database.lock().await.release_history(history_ref);
    sync_engine
        .lock()
        .await
        .settle_legacy_arrival(source, message_id);
    Ok((history_ref > 0).then_some(history_ref))
}

fn receipt_error(error: String) -> ReliableEventError {
    if error == "Remote event ID collision" {
        ReliableEventError::permanent(error)
    } else {
        ReliableEventError::retryable(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::DeviceIdentity;
    use crate::protocol::TransferId;
    use crate::secure::{self, PeerIdentity};
    use crate::sync::{FileBatchProgress, FileReceiveCommit, SyncPlatform};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn history_fixture() -> HistoryDB {
        let root = crate::db::get_data_dir()
            .join(format!("event-receiver-{:016x}", rand::random::<u64>()));
        HistoryDB::open_isolated_for_test(&root).unwrap()
    }

    struct TestPlatform {
        arrivals: std::sync::Mutex<Vec<crate::notifications::ClipboardArrival>>,
        writes: std::sync::Mutex<Vec<String>>,
        revision: std::sync::atomic::AtomicI64,
        supersede_next_write: std::sync::atomic::AtomicBool,
        /// The typed failure the next text write reports, if any.
        text_failure: std::sync::Mutex<Option<crate::sync::WriteFailure>>,
        /// The typed failure the next image write reports, if any.
        image_failure: std::sync::Mutex<Option<crate::sync::WriteFailure>>,
    }

    impl Default for TestPlatform {
        fn default() -> Self {
            Self {
                arrivals: std::sync::Mutex::new(Vec::new()),
                writes: std::sync::Mutex::new(Vec::new()),
                revision: std::sync::atomic::AtomicI64::new(1),
                supersede_next_write: std::sync::atomic::AtomicBool::new(false),
                text_failure: std::sync::Mutex::new(None),
                image_failure: std::sync::Mutex::new(None),
            }
        }
    }

    impl TestPlatform {
        fn check_revision(&self, expected: Option<i64>) -> Result<(), crate::sync::WriteFailure> {
            if self.supersede_next_write.swap(false, Ordering::AcqRel) {
                self.classified_text_write("new local copy")?;
            }
            if expected.is_some_and(|revision| revision != self.revision.load(Ordering::Acquire)) {
                return Err(crate::sync::WriteFailure::superseded("newer local copy"));
            }
            Ok(())
        }

        fn fail_text_with(&self, failure: crate::sync::WriteFailure) {
            *self.text_failure.lock().unwrap() = Some(failure);
        }

        fn clear_text_failure(&self) {
            *self.text_failure.lock().unwrap() = None;
        }

        fn fail_image_with(&self, failure: crate::sync::WriteFailure) {
            *self.image_failure.lock().unwrap() = Some(failure);
        }

        fn clear_image_failure(&self) {
            *self.image_failure.lock().unwrap() = None;
        }

        /// The typed projection a platform owns: only a platform that can prove the
        /// write never reached the pasteboard reports a no-effect.
        fn classified_text_write(&self, text: &str) -> Result<(), crate::sync::WriteFailure> {
            if let Some(failure) = self.text_failure.lock().unwrap().clone() {
                return Err(failure);
            }
            self.writes.lock().unwrap().push(text.to_string());
            self.revision.fetch_add(1, Ordering::AcqRel);
            Ok(())
        }

        fn classified_image_write(
            &self,
            width: u32,
            height: u32,
        ) -> Result<(), crate::sync::WriteFailure> {
            if let Some(failure) = self.image_failure.lock().unwrap().clone() {
                return Err(failure);
            }
            self.writes
                .lock()
                .unwrap()
                .push(format!("image:{width}x{height}"));
            self.revision.fetch_add(1, Ordering::AcqRel);
            Ok(())
        }
    }

    impl SyncPlatform for TestPlatform {
        fn clipboard_revision(&self) -> Option<i64> {
            let revision = self.revision.load(Ordering::Acquire);
            (revision >= 0).then_some(revision)
        }

        fn received_content(&self, arrival: crate::notifications::ClipboardArrival) {
            self.arrivals.lock().unwrap().push(arrival);
        }
        fn write_text(&self, text: &str) -> Result<(), String> {
            self.classified_text_write(text).map_err(String::from)
        }

        fn write_text_typed(&self, text: &str) -> Result<(), crate::sync::WriteFailure> {
            self.classified_text_write(text)
        }

        fn write_text_cancellable_typed(
            &self,
            text: &str,
            expected_revision: Option<i64>,
            cancelled: Arc<std::sync::atomic::AtomicBool>,
        ) -> Result<(), crate::sync::WriteFailure> {
            if cancelled.load(Ordering::Acquire) {
                return Err(crate::sync::WriteFailure::no_effect(
                    "cancelled before write",
                ));
            }
            self.check_revision(expected_revision)?;
            self.classified_text_write(text)
        }

        fn write_image(&self, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
            self.write_image_typed(width, height, rgba)
                .map_err(String::from)
        }

        fn write_image_typed(
            &self,
            width: u32,
            height: u32,
            _rgba: &[u8],
        ) -> Result<(), crate::sync::WriteFailure> {
            self.classified_image_write(width, height)
        }

        fn write_image_cancellable_typed(
            &self,
            width: u32,
            height: u32,
            _rgba: &[u8],
            expected_revision: Option<i64>,
            cancelled: Arc<std::sync::atomic::AtomicBool>,
        ) -> Result<(), crate::sync::WriteFailure> {
            if cancelled.load(Ordering::Acquire) {
                return Err(crate::sync::WriteFailure::no_effect(
                    "cancelled before write",
                ));
            }
            self.check_revision(expected_revision)?;
            self.classified_image_write(width, height)
        }

        fn set_file_progress(&self, _name: &str, _received: u64, _total: u64) {}

        fn clear_file_progress(&self, _batch_id: Option<TransferId>, _device: Option<&str>) {}

        fn set_file_batch_progress(&self, _progress: FileBatchProgress) {}

        fn files_received(&self, _commit: FileReceiveCommit) -> crate::sync::PlatformResultFuture {
            Box::pin(async { Ok(()) })
        }

        fn file_batch_failed(&self, _batch_id: Option<TransferId>, _message: &str) {}
    }

    fn server_peer_identity() -> PeerIdentity {
        PeerIdentity {
            hostname: "server".into(),
            tailscale_ip: String::new(),
            iroh_endpoint_id: None,
        }
    }

    fn client_peer_identity() -> PeerIdentity {
        PeerIdentity {
            hostname: "client".into(),
            tailscale_ip: String::new(),
            iroh_endpoint_id: None,
        }
    }

    /// Establish a Noise-authenticated pair over an in-memory duplex; the
    /// returned client is ready to send, the server to receive.
    async fn establish_pair(
        server_identity: &Arc<DeviceIdentity>,
        client_identity: &DeviceIdentity,
    ) -> (SecureConnection, SecureConnection) {
        let expected_key = server_identity.public_key().to_vec();
        let (client_io, server_io) = tokio::io::duplex(256 * 1024);
        let server_identity = server_identity.clone();
        let server = tokio::spawn(async move {
            let accepted = secure::accept(server_io, &server_identity, server_peer_identity())
                .await
                .unwrap();
            let mut connection = accepted.connection;
            secure::write_ready(&mut connection).await.unwrap();
            connection
        });
        let client = secure::connect(
            client_io,
            client_identity,
            client_peer_identity(),
            "server",
            &expected_key,
        )
        .await
        .unwrap();
        let server = server.await.unwrap();
        (client, server)
    }

    /// A platform whose text write blocks for 250 ms, standing in for the helper
    /// subprocess the composite path now spawns.
    #[derive(Default)]
    struct SlowTextPlatform {
        writes: AtomicUsize,
        delay: std::time::Duration,
    }

    impl SyncPlatform for SlowTextPlatform {
        fn clipboard_revision(&self) -> Option<i64> {
            Some(1)
        }
        fn write_text_cancellable_typed(
            &self,
            text: &str,
            expected_revision: Option<i64>,
            _cancelled: Arc<std::sync::atomic::AtomicBool>,
        ) -> Result<(), crate::sync::WriteFailure> {
            assert_eq!(expected_revision, Some(1));
            self.write_text(text)
                .map_err(crate::sync::WriteFailure::from)
        }
        fn write_text(&self, _: &str) -> Result<(), String> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(self.delay);
            Ok(())
        }
        fn write_image(&self, _: u32, _: u32, _: &[u8]) -> Result<(), String> {
            Ok(())
        }
        fn set_file_progress(&self, _: &str, _: u64, _: u64) {}
        fn clear_file_progress(&self, _: Option<TransferId>, _: Option<&str>) {}
        fn set_file_batch_progress(&self, _: FileBatchProgress) {}
        fn files_received(&self, _: FileReceiveCommit) -> crate::sync::PlatformResultFuture {
            Box::pin(async { Ok(()) })
        }
        fn file_batch_failed(&self, _: Option<TransferId>, _: &str) {}
    }

    /// R3-01 / Gate R3 — the **production** inbound text path must not hold the
    /// engine lock, or an async worker, across the native write.
    ///
    /// The verification measured the old path with the production
    /// `handle_incoming_text`: a 250 ms platform call dragged a 10 ms timer out to
    /// 255 ms, because `sync_engine.lock()` was held across the await. The fix is
    /// two short lock phases with the port call in between, so the runtime keeps
    /// making progress.
    #[tokio::test(flavor = "current_thread")]
    async fn inbound_text_does_not_stall_the_runtime_on_a_slow_platform() {
        let _warnings = crate::sync_warning::test_lock_async().await;
        while crate::sync_warning::take().is_some() {}
        const NATIVE_DELAY_MS: u64 = 250;
        const TIMER_MS: u64 = 10;
        const CEILING_MS: u128 = 150;

        let root =
            crate::db::get_data_dir().join(format!("r3-slow-text-{:016x}", rand::random::<u64>()));
        let server_identity = Arc::new(DeviceIdentity::generate_for_test());
        let client_identity = DeviceIdentity::generate_for_test();
        let (_client, mut server) = establish_pair(&server_identity, &client_identity).await;
        let db = Arc::new(Mutex::new(
            HistoryDB::open_isolated_for_test(&root).unwrap(),
        ));
        let platform = Arc::new(SlowTextPlatform {
            writes: AtomicUsize::new(0),
            delay: std::time::Duration::from_millis(NATIVE_DELAY_MS),
        });
        let mut engine = SyncEngine::new();
        engine.set_platform(platform.clone());
        let engine = Arc::new(Mutex::new(engine));

        let envelope = EventEnvelope::new(b"slow fixture".to_vec());
        let frame = text_frame(1, &envelope);
        let mut sequence = None;

        let started = std::time::Instant::now();
        let pulse = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(TIMER_MS)).await;
            started.elapsed().as_millis()
        });
        tokio::task::yield_now().await;

        process_reliable_event(
            &mut server,
            &frame,
            "client",
            &engine,
            &db,
            &mut sequence,
            || (),
        )
        .await
        .expect("the text event must be applied");

        let observed = pulse.await.expect("the timer task must run");
        assert_eq!(platform.writes.load(Ordering::SeqCst), 1);
        assert!(
            observed < CEILING_MS,
            "a {TIMER_MS}ms timer was observed at {observed}ms while a {NATIVE_DELAY_MS}ms native \
             write was in flight: the engine lock (or an async worker) is still held across the \
             native call"
        );
        std::fs::remove_dir_all(root).ok();
    }

    async fn test_engine() -> (Arc<Mutex<SyncEngine>>, Arc<TestPlatform>) {
        let platform = Arc::new(TestPlatform::default());
        let mut engine = SyncEngine::new();
        engine.set_platform(platform.clone());
        (Arc::new(Mutex::new(engine)), platform)
    }

    fn text_frame(sequence: u32, envelope: &EventEnvelope) -> Frame {
        Frame::try_new(Command::TextPayload, 0, sequence, envelope.encode()).unwrap()
    }

    fn image_frame(sequence: u32, envelope: &EventEnvelope) -> Frame {
        Frame::try_new(Command::ImagePayload, 0, sequence, envelope.encode()).unwrap()
    }

    #[tokio::test]
    async fn legacy_text_and_image_cas_preserve_a_copy_made_before_the_native_write() {
        let _warnings = crate::sync_warning::test_lock_async().await;
        while crate::sync_warning::take().is_some() {}
        for (command, content) in [
            (Command::TextPayload, b"remote old copy".to_vec()),
            (
                Command::ImagePayload,
                crate::protocol::pack_rgba_image(1, 1, &[9; 4]).unwrap(),
            ),
        ] {
            let server_identity = Arc::new(DeviceIdentity::generate_for_test());
            let client_identity = DeviceIdentity::generate_for_test();
            let (mut client, mut server) = establish_pair(&server_identity, &client_identity).await;
            let db = Arc::new(Mutex::new(history_fixture()));
            let (engine, platform) = test_engine().await;
            platform.supersede_next_write.store(true, Ordering::Release);
            let generation = engine.lock().await.clipboard_generation();
            let envelope = EventEnvelope::new(content.clone());
            let frame = Frame::try_new(command, 0, 1, envelope.encode()).unwrap();
            let mut last_sequence = None;
            let outcome = process_reliable_event(
                &mut server,
                &frame,
                "client",
                &engine,
                &db,
                &mut last_sequence,
                || (),
            )
            .await
            .unwrap();
            assert_eq!(outcome, ReliableEventOutcome::Applied);
            assert_eq!(
                client.read_frame().await.unwrap().command,
                Command::EventAck
            );
            assert_eq!(*platform.writes.lock().unwrap(), vec!["new local copy"]);
            assert_eq!(engine.lock().await.clipboard_generation(), generation);
            let entries = db.lock().await.get_all(None, None, 10, 0).unwrap();
            assert_eq!(entries.len(), 1);
            assert_eq!(db.lock().await.get_data(entries[0].id).unwrap(), content);
            let receipt = Receipt::new(
                crate::secure::fingerprint(client_identity.public_key()),
                command,
                &envelope,
            );
            assert!(
                event_receipts::update(receipt, Operation::Lookup)
                    .await
                    .unwrap()
                    .unwrap()
                    .received
            );
            let duplicate = process_reliable_event(
                &mut server,
                &frame,
                "client",
                &engine,
                &db,
                &mut last_sequence,
                || (),
            )
            .await
            .unwrap();
            assert_eq!(duplicate, ReliableEventOutcome::Duplicate);
            assert_eq!(
                client.read_frame().await.unwrap().command,
                Command::EventAck
            );
            assert_eq!(*platform.writes.lock().unwrap(), vec!["new local copy"]);
            assert!(crate::sync_warning::take().is_none());
        }
    }

    #[tokio::test]
    async fn legacy_retry_keeps_its_original_revision_after_a_new_local_copy() {
        let _warnings = crate::sync_warning::test_lock_async().await;
        while crate::sync_warning::take().is_some() {}
        for (command, content) in [
            (Command::TextPayload, b"remote retry".to_vec()),
            (
                Command::ImagePayload,
                crate::protocol::pack_rgba_image(1, 1, &[8; 4]).unwrap(),
            ),
        ] {
            let server_identity = Arc::new(DeviceIdentity::generate_for_test());
            let client_identity = DeviceIdentity::generate_for_test();
            let (mut client, mut server) = establish_pair(&server_identity, &client_identity).await;
            let db = Arc::new(Mutex::new(history_fixture()));
            let (engine, platform) = test_engine().await;
            let failure = crate::sync::WriteFailure::no_effect("temporarily unavailable");
            if command == Command::TextPayload {
                platform.fail_text_with(failure);
            } else {
                platform.fail_image_with(failure);
            }
            let frame =
                Frame::try_new(command, 0, 1, EventEnvelope::new(content).encode()).unwrap();
            let mut last_sequence = None;
            assert!(process_reliable_event(
                &mut server,
                &frame,
                "client",
                &engine,
                &db,
                &mut last_sequence,
                || ()
            )
            .await
            .unwrap_err()
            .is_retryable());
            platform.clear_text_failure();
            platform.clear_image_failure();
            platform.write_text("new local copy").unwrap();
            assert_eq!(
                process_reliable_event(
                    &mut server,
                    &frame,
                    "client",
                    &engine,
                    &db,
                    &mut last_sequence,
                    || ()
                )
                .await
                .unwrap(),
                ReliableEventOutcome::Applied
            );
            assert_eq!(
                client.read_frame().await.unwrap().command,
                Command::EventAck
            );
            assert_eq!(*platform.writes.lock().unwrap(), vec!["new local copy"]);
            assert_eq!(
                db.lock()
                    .await
                    .count_all_filtered(None, None, None, None)
                    .unwrap(),
                1
            );
        }
    }

    #[tokio::test]
    async fn legacy_receive_without_a_clipboard_revision_saves_history_without_publishing() {
        let _warnings = crate::sync_warning::test_lock_async().await;
        while crate::sync_warning::take().is_some() {}
        let server_identity = Arc::new(DeviceIdentity::generate_for_test());
        let client_identity = DeviceIdentity::generate_for_test();
        let (mut client, mut server) = establish_pair(&server_identity, &client_identity).await;
        let db = Arc::new(Mutex::new(history_fixture()));
        let (engine, platform) = test_engine().await;
        platform.revision.store(-1, Ordering::Release);
        let frame = text_frame(1, &EventEnvelope::new(b"remote history".to_vec()));
        let mut last_sequence = None;
        process_reliable_event(
            &mut server,
            &frame,
            "client",
            &engine,
            &db,
            &mut last_sequence,
            || (),
        )
        .await
        .unwrap();
        assert_eq!(
            client.read_frame().await.unwrap().command,
            Command::EventAck
        );
        assert!(platform.writes.lock().unwrap().is_empty());
        assert_eq!(
            db.lock()
                .await
                .count_all_filtered(None, None, None, None)
                .unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn text_event_is_applied_and_acknowledged() {
        let _warnings = crate::sync_warning::test_lock_async().await;
        while crate::sync_warning::take().is_some() {}
        let server_identity = Arc::new(DeviceIdentity::generate_for_test());
        let client_identity = DeviceIdentity::generate_for_test();
        let (mut client, mut server) = establish_pair(&server_identity, &client_identity).await;
        let db = Arc::new(Mutex::new(history_fixture()));
        let (engine, platform) = test_engine().await;

        let envelope = EventEnvelope::new(b"hello from peer".to_vec());
        let frame = text_frame(1, &envelope);
        client.write_frame(&frame).await.unwrap();

        let mut last_sequence = None;
        let outcome = process_reliable_event(
            &mut server,
            &frame,
            "client",
            &engine,
            &db,
            &mut last_sequence,
            || (),
        )
        .await
        .unwrap();
        assert_eq!(outcome, ReliableEventOutcome::Applied);
        let arrivals = platform.arrivals.lock().unwrap().clone();
        assert_eq!(arrivals.len(), 1);
        assert_eq!(
            arrivals[0].source_id,
            crate::secure::fingerprint(client_identity.public_key())
        );
        assert_eq!(arrivals[0].event_id, envelope.message_id.as_hex());
        assert!(arrivals[0].entry_id.unwrap() > 0);
        assert_eq!(last_sequence, Some(1));

        // The acknowledgement is written back with the same sequence.
        let ack = client.read_frame().await.unwrap();
        assert_eq!(ack.command, Command::EventAck);
        assert_eq!(ack.sequence, 1);

        // Clipboard side effect ran exactly once and the message is recorded.
        assert_eq!(platform.writes.lock().unwrap().len(), 1);
        assert!(engine
            .lock()
            .await
            .has_seen_message("client", envelope.message_id));
    }

    /// R4-02 — an **unproven** write failure keeps the claim.
    ///
    /// This replaces the old `transient_clipboard_failure_does_not_persist_or_deduplicate_event`,
    /// which asserted the opposite. The plan is explicit: only a *proven* `NoEffect`
    /// may release an at-most-once claim, because re-applying a system write that may
    /// already have happened is worse than skipping it. The retry therefore records
    /// the history and acknowledges the event, but does **not** write the clipboard
    /// again.
    #[tokio::test]
    async fn an_unproven_clipboard_failure_keeps_the_claim_and_is_not_reapplied() {
        let _warnings = crate::sync_warning::test_lock_async().await;
        while crate::sync_warning::take().is_some() {}
        let server_identity = Arc::new(DeviceIdentity::generate_for_test());
        let client_identity = DeviceIdentity::generate_for_test();
        let (mut client, mut server) = establish_pair(&server_identity, &client_identity).await;
        let db = Arc::new(Mutex::new(history_fixture()));
        let (engine, platform) = test_engine().await;
        platform.fail_text_with(crate::sync::WriteFailure::may_have_effect(
            "clipboard unavailable",
        ));

        let envelope = EventEnvelope::new(b"retry me".to_vec());
        let frame = text_frame(1, &envelope);
        client.write_frame(&frame).await.unwrap();

        let mut last_sequence = None;
        let error = process_reliable_event(
            &mut server,
            &frame,
            "client",
            &engine,
            &db,
            &mut last_sequence,
            || (),
        )
        .await
        .unwrap_err();
        assert!(error.is_retryable());
        assert_eq!(
            db.lock()
                .await
                .count_all_filtered(None, None, None, None)
                .unwrap(),
            1
        );

        // The platform recovers, but the first failure proved nothing, so the native
        // effect is not applied a second time.
        platform.clear_text_failure();
        let outcome = process_reliable_event(
            &mut server,
            &frame,
            "client",
            &engine,
            &db,
            &mut last_sequence,
            || (),
        )
        .await
        .unwrap();
        assert_eq!(outcome, ReliableEventOutcome::Applied);
        let _ = client.read_frame().await.unwrap();
        assert_eq!(
            db.lock()
                .await
                .count_all_filtered(None, None, None, None)
                .unwrap(),
            1,
            "the content is still recorded in history"
        );
        assert!(
            platform.writes.lock().unwrap().is_empty(),
            "an unproven failure must not be re-applied"
        );
    }

    /// R4-02 — a **proven** no-effect failure does release the claim, so the effect is
    /// retried rather than lost.
    #[tokio::test]
    async fn a_proven_no_effect_failure_releases_the_claim_and_retries() {
        let _warnings = crate::sync_warning::test_lock_async().await;
        while crate::sync_warning::take().is_some() {}
        let server_identity = Arc::new(DeviceIdentity::generate_for_test());
        let client_identity = DeviceIdentity::generate_for_test();
        let (mut client, mut server) = establish_pair(&server_identity, &client_identity).await;
        let db = Arc::new(Mutex::new(history_fixture()));
        let (engine, platform) = test_engine().await;
        platform.fail_text_with(crate::sync::WriteFailure::no_effect(
            "clipboard unavailable",
        ));

        let envelope = EventEnvelope::new(b"retry me".to_vec());
        let frame = text_frame(1, &envelope);
        client.write_frame(&frame).await.unwrap();

        let mut last_sequence = None;
        let error = process_reliable_event(
            &mut server,
            &frame,
            "client",
            &engine,
            &db,
            &mut last_sequence,
            || (),
        )
        .await
        .unwrap_err();
        assert!(error.is_retryable());

        platform.clear_text_failure();
        let outcome = process_reliable_event(
            &mut server,
            &frame,
            "client",
            &engine,
            &db,
            &mut last_sequence,
            || (),
        )
        .await
        .unwrap();
        assert_eq!(outcome, ReliableEventOutcome::Applied);
        let _ = client.read_frame().await.unwrap();
        assert_eq!(
            platform.writes.lock().unwrap().as_slice(),
            ["retry me"],
            "a proven no-effect releases the claim, so the effect is applied on retry"
        );
    }

    /// F2 — the same release rule on the image seam: only a platform that proves it
    /// did not touch the pasteboard may release the claim.
    #[tokio::test]
    async fn an_image_no_effect_failure_releases_the_claim_and_retries() {
        let _warnings = crate::sync_warning::test_lock_async().await;
        while crate::sync_warning::take().is_some() {}
        let server_identity = Arc::new(DeviceIdentity::generate_for_test());
        let client_identity = DeviceIdentity::generate_for_test();
        let (mut client, mut server) = establish_pair(&server_identity, &client_identity).await;
        let db = Arc::new(Mutex::new(history_fixture()));
        let (engine, platform) = test_engine().await;
        platform.fail_image_with(crate::sync::WriteFailure::no_effect(
            "clipboard unavailable",
        ));

        let envelope = EventEnvelope::new(crate::protocol::pack_rgba_image(1, 1, &[2; 4]).unwrap());
        let frame = image_frame(1, &envelope);
        client.write_frame(&frame).await.unwrap();

        let mut last_sequence = None;
        let error = process_reliable_event(
            &mut server,
            &frame,
            "client",
            &engine,
            &db,
            &mut last_sequence,
            || (),
        )
        .await
        .unwrap_err();
        assert!(error.is_retryable());

        platform.clear_image_failure();
        let outcome = process_reliable_event(
            &mut server,
            &frame,
            "client",
            &engine,
            &db,
            &mut last_sequence,
            || (),
        )
        .await
        .unwrap();
        assert_eq!(outcome, ReliableEventOutcome::Applied);
        let _ = client.read_frame().await.unwrap();
        assert_eq!(
            platform.writes.lock().unwrap().as_slice(),
            ["image:1x1"],
            "a proven no-effect releases the image claim, so the effect is applied on retry"
        );
    }

    /// F2 — an image failure that proves nothing keeps the claim.
    #[tokio::test]
    async fn an_unproven_image_failure_keeps_the_claim() {
        let _warnings = crate::sync_warning::test_lock_async().await;
        while crate::sync_warning::take().is_some() {}
        let server_identity = Arc::new(DeviceIdentity::generate_for_test());
        let client_identity = DeviceIdentity::generate_for_test();
        let (mut client, mut server) = establish_pair(&server_identity, &client_identity).await;
        let db = Arc::new(Mutex::new(history_fixture()));
        let (engine, platform) = test_engine().await;
        platform.fail_image_with(crate::sync::WriteFailure::may_have_effect(
            "clipboard helper returned no usable receipt",
        ));

        let envelope = EventEnvelope::new(crate::protocol::pack_rgba_image(1, 1, &[2; 4]).unwrap());
        let frame = image_frame(1, &envelope);
        client.write_frame(&frame).await.unwrap();

        let mut last_sequence = None;
        let error = process_reliable_event(
            &mut server,
            &frame,
            "client",
            &engine,
            &db,
            &mut last_sequence,
            || (),
        )
        .await
        .unwrap_err();
        assert!(error.is_retryable());

        platform.clear_image_failure();
        let outcome = process_reliable_event(
            &mut server,
            &frame,
            "client",
            &engine,
            &db,
            &mut last_sequence,
            || (),
        )
        .await
        .unwrap();
        assert_eq!(outcome, ReliableEventOutcome::Applied);
        let _ = client.read_frame().await.unwrap();
        assert!(
            platform.writes.lock().unwrap().is_empty(),
            "an unproven image failure must not be re-applied"
        );
    }

    /// F2 — the decision reads the typed kind, never the message. A `MayHaveEffect`
    /// detail that happens to contain the legacy marker must still keep the claim.
    #[tokio::test]
    async fn a_no_effect_detail_that_mentions_the_marker_still_keeps_the_claim() {
        let _warnings = crate::sync_warning::test_lock_async().await;
        while crate::sync_warning::take().is_some() {}
        let server_identity = Arc::new(DeviceIdentity::generate_for_test());
        let client_identity = DeviceIdentity::generate_for_test();
        let (mut client, mut server) = establish_pair(&server_identity, &client_identity).await;
        let db = Arc::new(Mutex::new(history_fixture()));
        let (engine, platform) = test_engine().await;
        platform.fail_text_with(crate::sync::WriteFailure::may_have_effect(format!(
            "{}worker died after writing",
            crate::protocol::NO_EFFECT_MARKER
        )));

        let envelope = EventEnvelope::new(b"marker text".to_vec());
        let frame = text_frame(1, &envelope);
        client.write_frame(&frame).await.unwrap();

        let mut last_sequence = None;
        let error = process_reliable_event(
            &mut server,
            &frame,
            "client",
            &engine,
            &db,
            &mut last_sequence,
            || (),
        )
        .await
        .unwrap_err();
        assert!(error.is_retryable());

        platform.clear_text_failure();
        let outcome = process_reliable_event(
            &mut server,
            &frame,
            "client",
            &engine,
            &db,
            &mut last_sequence,
            || (),
        )
        .await
        .unwrap();
        assert_eq!(outcome, ReliableEventOutcome::Applied);
        let _ = client.read_frame().await.unwrap();
        assert!(
            platform.writes.lock().unwrap().is_empty(),
            "the message text must not be able to release the claim"
        );
    }

    #[tokio::test]
    async fn receive_arrivals_are_independent_of_newer_local_history() {
        let _warnings = crate::sync_warning::test_lock_async().await;
        let server_identity = Arc::new(DeviceIdentity::generate_for_test());
        let client_identity = DeviceIdentity::generate_for_test();
        let (mut client, mut server) = establish_pair(&server_identity, &client_identity).await;
        let db = Arc::new(Mutex::new(history_fixture()));
        let (engine, platform) = test_engine().await;
        let mut sequence = None;
        for index in 1..=3 {
            let envelope = EventEnvelope::new(format!("remote {index}").into_bytes());
            process_reliable_event(
                &mut server,
                &text_frame(index, &envelope),
                "client",
                &engine,
                &db,
                &mut sequence,
                || (),
            )
            .await
            .unwrap();
            client.read_frame().await.unwrap();
            db.lock()
                .await
                .add_text(&format!("local {index}"), "self")
                .unwrap();
        }
        assert_eq!(
            db.lock().await.get_all(None, None, 1, 0).unwrap()[0].source_peer,
            "self"
        );
        let arrivals = platform.arrivals.lock().unwrap();
        assert_eq!(arrivals.len(), 3);
        assert!(arrivals
            .iter()
            .all(|arrival| arrival.source_name == "client"));
    }

    #[tokio::test]
    async fn duplicate_event_is_acknowledged_without_reapply() {
        let _warnings = crate::sync_warning::test_lock_async().await;
        while crate::sync_warning::take().is_some() {}
        let server_identity = Arc::new(DeviceIdentity::generate_for_test());
        let client_identity = DeviceIdentity::generate_for_test();
        let (mut client, mut server) = establish_pair(&server_identity, &client_identity).await;
        let db = Arc::new(Mutex::new(history_fixture()));
        let (engine, platform) = test_engine().await;

        let envelope = EventEnvelope::new(b"hello from peer".to_vec());
        let frame = text_frame(1, &envelope);
        client.write_frame(&frame).await.unwrap();

        let mut last_sequence = None;
        assert_eq!(
            process_reliable_event(
                &mut server,
                &frame,
                "client",
                &engine,
                &db,
                &mut last_sequence,
                || (),
            )
            .await
            .unwrap(),
            ReliableEventOutcome::Applied
        );
        let _ = client.read_frame().await.unwrap();

        // Same frame again: acknowledged as a duplicate, not reapplied.
        let outcome = process_reliable_event(
            &mut server,
            &frame,
            "client",
            &engine,
            &db,
            &mut last_sequence,
            || (),
        )
        .await
        .unwrap();
        assert_eq!(outcome, ReliableEventOutcome::Duplicate);
        assert_eq!(platform.arrivals.lock().unwrap().len(), 1);
        let _ = client.read_frame().await.unwrap();

        assert_eq!(platform.writes.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn replay_with_unknown_message_id_is_rejected() {
        let _warnings = crate::sync_warning::test_lock_async().await;
        while crate::sync_warning::take().is_some() {}
        let server_identity = Arc::new(DeviceIdentity::generate_for_test());
        let client_identity = DeviceIdentity::generate_for_test();
        let (mut client, mut server) = establish_pair(&server_identity, &client_identity).await;
        let db = Arc::new(Mutex::new(history_fixture()));
        let (engine, _platform) = test_engine().await;

        let first = EventEnvelope::new(b"one".to_vec());
        client.write_frame(&text_frame(1, &first)).await.unwrap();
        let mut last_sequence = None;
        process_reliable_event(
            &mut server,
            &text_frame(1, &first),
            "client",
            &engine,
            &db,
            &mut last_sequence,
            || (),
        )
        .await
        .unwrap();
        let _ = client.read_frame().await.unwrap();

        // A fresh message id reusing sequence 1 is a replay.
        let replay = EventEnvelope::new(b"two".to_vec());
        let error = process_reliable_event(
            &mut server,
            &text_frame(1, &replay),
            "client",
            &engine,
            &db,
            &mut last_sequence,
            || (),
        )
        .await
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("replayed or out-of-order event sequence 1"));
    }

    #[tokio::test]
    async fn stale_timestamp_is_rejected() {
        let _warnings = crate::sync_warning::test_lock_async().await;
        while crate::sync_warning::take().is_some() {}
        let server_identity = Arc::new(DeviceIdentity::generate_for_test());
        let client_identity = DeviceIdentity::generate_for_test();
        let (mut client, mut server) = establish_pair(&server_identity, &client_identity).await;
        let db = Arc::new(Mutex::new(history_fixture()));
        let (engine, _platform) = test_engine().await;

        let mut envelope = EventEnvelope::new(b"ancient".to_vec());
        envelope.timestamp_ms = 0;
        let frame = text_frame(1, &envelope);
        client.write_frame(&frame).await.unwrap();

        let mut last_sequence = None;
        let error = process_reliable_event(
            &mut server,
            &frame,
            "client",
            &engine,
            &db,
            &mut last_sequence,
            || (),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("timestamp"));
    }

    #[tokio::test]
    async fn non_reliable_command_is_rejected() {
        let _warnings = crate::sync_warning::test_lock_async().await;
        while crate::sync_warning::take().is_some() {}
        let server_identity = Arc::new(DeviceIdentity::generate_for_test());
        let client_identity = DeviceIdentity::generate_for_test();
        let (mut client, mut server) = establish_pair(&server_identity, &client_identity).await;
        let db = Arc::new(Mutex::new(history_fixture()));
        let (engine, _platform) = test_engine().await;

        let envelope = EventEnvelope::new(b"payload".to_vec());
        let frame = Frame::try_new(Command::Heartbeat, 0, 1, envelope.encode()).unwrap();
        client.write_frame(&frame).await.unwrap();

        let mut last_sequence = None;
        let error = process_reliable_event(
            &mut server,
            &frame,
            "client",
            &engine,
            &db,
            &mut last_sequence,
            || (),
        )
        .await
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("Heartbeat is not a reliable event command"));
    }

    /// A clipboard effect that already happened must not be re-applied by a
    /// retry after the history write failed; the retry must still persist the
    /// history row and acknowledge the event.
    #[tokio::test]
    async fn db_failure_prevents_clipboard_apply_until_history_recovers() {
        let _warnings = crate::sync_warning::test_lock_async().await;
        while crate::sync_warning::take().is_some() {}
        let server_identity = Arc::new(DeviceIdentity::generate_for_test());
        let client_identity = DeviceIdentity::generate_for_test();
        let (mut client, mut server) = establish_pair(&server_identity, &client_identity).await;
        let db = Arc::new(Mutex::new(history_fixture()));
        db.lock().await.reject_text_inserts_for_test(true).unwrap();
        let (engine, platform) = test_engine().await;

        let envelope = EventEnvelope::new(b"event A".to_vec());
        let frame = text_frame(1, &envelope);
        client.write_frame(&frame).await.unwrap();

        let mut last_sequence = None;
        let error = process_reliable_event(
            &mut server,
            &frame,
            "client",
            &engine,
            &db,
            &mut last_sequence,
            || (),
        )
        .await
        .unwrap_err();
        assert!(error.is_retryable());
        let warning = crate::sync_warning::peek().unwrap();
        assert_eq!(warning.kind, "receive_not_saved");
        assert_eq!(warning.peer, "client");
        assert!(crate::sync_warning::ack(warning.id));
        // Failed history preparation must prevent all native effects.
        assert!(platform.writes.lock().unwrap().is_empty());
        assert!(!engine
            .lock()
            .await
            .has_seen_message("client", envelope.message_id));
        assert_eq!(
            db.lock()
                .await
                .count_all_filtered(None, None, None, None)
                .unwrap(),
            0
        );

        // The storage failure clears and the sender retries the same event.
        db.lock().await.reject_text_inserts_for_test(false).unwrap();
        let outcome = process_reliable_event(
            &mut server,
            &frame,
            "client",
            &engine,
            &db,
            &mut last_sequence,
            || (),
        )
        .await
        .unwrap();
        assert_eq!(outcome, ReliableEventOutcome::Applied);
        let _ = client.read_frame().await.unwrap();

        // The clipboard effect was not written a second time, the history row
        // is durable, the event is marked seen, and the applied state cleared.
        assert!(platform.writes.lock().unwrap().is_empty());
        assert_eq!(
            db.lock()
                .await
                .count_all_filtered(None, None, None, None)
                .unwrap(),
            1
        );
        assert!(engine
            .lock()
            .await
            .has_seen_message("client", envelope.message_id));
        assert!(!engine
            .lock()
            .await
            .has_applied_event("client", envelope.message_id));
    }

    /// A retried event whose clipboard effect already applied must not clobber
    /// newer local clipboard intent that appeared before the retry.
    #[tokio::test]
    async fn retry_of_an_applied_event_does_not_overwrite_newer_local_clipboard() {
        let _warnings = crate::sync_warning::test_lock_async().await;
        while crate::sync_warning::take().is_some() {}
        let server_identity = Arc::new(DeviceIdentity::generate_for_test());
        let client_identity = DeviceIdentity::generate_for_test();
        let (mut client, mut server) = establish_pair(&server_identity, &client_identity).await;
        let db = Arc::new(Mutex::new(history_fixture()));
        db.lock().await.reject_text_inserts_for_test(true).unwrap();
        let (engine, platform) = test_engine().await;

        let envelope = EventEnvelope::new(b"remote A".to_vec());
        let frame = text_frame(1, &envelope);
        client.write_frame(&frame).await.unwrap();

        let mut last_sequence = None;
        let error = process_reliable_event(
            &mut server,
            &frame,
            "client",
            &engine,
            &db,
            &mut last_sequence,
            || (),
        )
        .await
        .unwrap_err();
        assert!(error.is_retryable());
        assert!(platform.writes.lock().unwrap().is_empty());

        // The user copies newer content locally before the remote retry lands.
        platform
            .writes
            .lock()
            .unwrap()
            .push("newer local B".to_string());
        engine.lock().await.supersede_file_clipboard();

        db.lock().await.reject_text_inserts_for_test(false).unwrap();
        let outcome = process_reliable_event(
            &mut server,
            &frame,
            "client",
            &engine,
            &db,
            &mut last_sequence,
            || (),
        )
        .await
        .unwrap();
        assert_eq!(outcome, ReliableEventOutcome::Applied);
        let _ = client.read_frame().await.unwrap();

        assert_eq!(
            platform.writes.lock().unwrap().as_slice(),
            ["newer local B"],
            "the stale remote retry must not overwrite newer local clipboard intent"
        );
        assert_eq!(
            db.lock()
                .await
                .count_all_filtered(None, None, None, None)
                .unwrap(),
            1
        );
    }
    #[tokio::test]
    async fn receiver_restart_preserves_newer_intent_and_acks_old_text_and_image() {
        let _warnings = crate::sync_warning::test_lock_async().await;
        while crate::sync_warning::take().is_some() {}
        for failed_history in [false, true] {
            for (command, content) in [
                (Command::TextPayload, b"remote old A".to_vec()),
                (
                    Command::ImagePayload,
                    crate::protocol::pack_rgba_image(1, 1, &[2; 4]).unwrap(),
                ),
            ] {
                let root = crate::db::get_data_dir()
                    .join(format!("p35-restart-{:016x}", rand::random::<u64>()));
                let server_identity = Arc::new(DeviceIdentity::generate_for_test());
                let client_identity = DeviceIdentity::generate_for_test();
                let (mut client, mut server) =
                    establish_pair(&server_identity, &client_identity).await;
                let db = Arc::new(Mutex::new(
                    HistoryDB::open_isolated_for_test(&root).unwrap(),
                ));
                let (engine, platform) = test_engine().await;
                let sql = rusqlite::Connection::open(root.join("history-v2.db")).unwrap();
                if failed_history {
                    sql.execute_batch("CREATE TRIGGER p35_failure BEFORE INSERT ON history BEGIN SELECT RAISE(ABORT, 'transient history failure'); END;").unwrap();
                }
                let envelope = EventEnvelope::new(content.clone());
                let frame = Frame::try_new(command, 0, 1, envelope.encode()).unwrap();
                let mut sequence = None;
                let result = process_reliable_event(
                    &mut server,
                    &frame,
                    "client",
                    &engine,
                    &db,
                    &mut sequence,
                    || (),
                )
                .await;
                if failed_history {
                    assert!(result.unwrap_err().is_retryable());
                    sql.execute_batch("DROP TRIGGER p35_failure;").unwrap();
                } else {
                    result.unwrap();
                    assert_eq!(
                        client.read_frame().await.unwrap().command,
                        Command::EventAck
                    );
                }
                drop(sql);
                drop(engine);
                drop(db);
                drop(server);
                drop(client);
                // A fresh engine, SQLite connection and authenticated transport.
                crate::db::configure_storage_dir(Some(&root)).unwrap();
                let db = Arc::new(Mutex::new(HistoryDB::new().unwrap()));
                let mut fresh_engine = SyncEngine::new();
                fresh_engine.set_platform(platform.clone());
                let engine = Arc::new(Mutex::new(fresh_engine));
                platform.write_text("newer local B").unwrap();
                let writes_before = platform.writes.lock().unwrap().len();
                let (mut client, mut server) =
                    establish_pair(&server_identity, &client_identity).await;
                let mut sequence = None;
                process_reliable_event(
                    &mut server,
                    &frame,
                    "client",
                    &engine,
                    &db,
                    &mut sequence,
                    || (),
                )
                .await
                .unwrap();
                assert_eq!(
                    client.read_frame().await.unwrap().command,
                    Command::EventAck
                );
                assert_eq!(
                    platform.writes.lock().unwrap().len(),
                    writes_before,
                    "old event acquired freshness after restart"
                );
                assert_eq!(
                    platform.writes.lock().unwrap().last().unwrap(),
                    "newer local B"
                );
                let fresh = EventEnvelope::new(content);
                let fresh_frame = Frame::try_new(command, 0, 2, fresh.encode()).unwrap();
                process_reliable_event(
                    &mut server,
                    &fresh_frame,
                    "client",
                    &engine,
                    &db,
                    &mut sequence,
                    || (),
                )
                .await
                .unwrap();
                assert_eq!(
                    client.read_frame().await.unwrap().command,
                    Command::EventAck
                );
                assert_eq!(
                    platform.writes.lock().unwrap().len(),
                    writes_before + 1,
                    "same content with a new event ID must apply"
                );
            }
        }
    }
}
