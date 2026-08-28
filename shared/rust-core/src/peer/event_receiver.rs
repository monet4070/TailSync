//! Inbound reliable event processing for authenticated peer connections.
//!
//! Shared by the macOS and Windows network layers (T106 migration). A
//! reliable text/image event is decoded, timestamp-checked, de-duplicated,
//! applied (clipboard side effect via the sync engine followed by the history
//! write), and acknowledged. `on_applied` lets the caller run platform side
//! effects (clipboard version bump) at the exact position of the original
//! server code — before the acknowledgement is written.

use crate::db::HistoryDB;
use crate::protocol::{unix_timestamp_ms, Command, EventEnvelope, Frame};
use crate::secure::SecureConnection;
use crate::sync::SyncEngine;
use log::{debug, info};
use std::sync::Arc;
use tokio::sync::Mutex;

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
    envelope
        .validate_timestamp(unix_timestamp_ms())
        .map_err(|error| ReliableEventError::permanent(error.to_string()))?;

    let duplicate = sync_engine
        .lock()
        .await
        .has_seen_message(source, envelope.message_id);
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
        process_event_content(
            frame.command,
            &envelope.content,
            source,
            sync_engine,
            database,
        )
        .await?;
        info!("{kind} event from {source} applied");
        sync_engine
            .lock()
            .await
            .record_message(source, envelope.message_id);
        on_applied();
        ReliableEventOutcome::Applied
    } else {
        debug!("Reliable event from {source} was already applied; acknowledging again");
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
    Ok(outcome)
}

async fn process_event_content(
    command: Command,
    content: &[u8],
    source: &str,
    sync_engine: &Arc<Mutex<SyncEngine>>,
    database: &Arc<Mutex<HistoryDB>>,
) -> Result<(), ReliableEventError> {
    match command {
        Command::TextPayload => {
            let text = String::from_utf8(content.to_vec())
                .map_err(|_| ReliableEventError::permanent("text event is not valid UTF-8"))?;
            sync_engine
                .lock()
                .await
                .handle_incoming_text(&text, source.to_string())
                .await
                .map_err(ReliableEventError::retryable)?;

            let db = database.clone();
            let db_text = text.clone();
            let db_source = source.to_string();
            let result = tokio::task::spawn_blocking(move || {
                db.blocking_lock()
                    .add_text(&db_text, &db_source)
                    .map_err(|error| error.to_string())
            })
            .await
            .map_err(|error| ReliableEventError::retryable(error.to_string()))?;
            result.map_err(ReliableEventError::retryable)?;

            info!("Received text from {}: {} chars", source, text.len());
        }
        Command::ImagePayload => {
            crate::protocol::PackedImage::try_from(content)
                .map(|_| ())
                .map_err(|error| ReliableEventError::permanent(error.to_string()))?;
            sync_engine
                .lock()
                .await
                .handle_incoming_image(content, source.to_string())
                .await
                .map_err(ReliableEventError::retryable)?;

            let db = database.clone();
            let image = content.to_vec();
            let db_source = source.to_string();
            let result = tokio::task::spawn_blocking(move || {
                db.blocking_lock()
                    .add_image(&image, &db_source)
                    .map_err(|error| error.to_string())
            })
            .await
            .map_err(|error| ReliableEventError::retryable(error.to_string()))?;
            result.map_err(ReliableEventError::retryable)?;

            info!("Received image from {}: {} bytes", source, content.len());
        }
        _ => {
            return Err(ReliableEventError::permanent(format!(
                "{:?} is not a reliable event command",
                command
            )))
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::DeviceIdentity;
    use crate::protocol::TransferId;
    use crate::secure::{self, PeerIdentity};
    use crate::sync::{FileBatchProgress, ReceivedFile, SyncPlatform};
    use std::sync::atomic::{AtomicBool, Ordering};

    struct TestPlatform {
        writes: std::sync::Mutex<Vec<String>>,
        fail_text: AtomicBool,
    }

    impl Default for TestPlatform {
        fn default() -> Self {
            Self {
                writes: std::sync::Mutex::new(Vec::new()),
                fail_text: AtomicBool::new(false),
            }
        }
    }

    impl SyncPlatform for TestPlatform {
        fn write_text(&self, text: &str) -> Result<(), String> {
            if self.fail_text.load(Ordering::Relaxed) {
                return Err("clipboard unavailable".to_string());
            }
            self.writes.lock().unwrap().push(text.to_string());
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
            _batch_id: Option<TransferId>,
            _files: Vec<ReceivedFile>,
            _batch_total: usize,
            _batch_complete: bool,
            _activate_clipboard: bool,
            _device: String,
        ) {
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

    async fn test_engine() -> (Arc<Mutex<SyncEngine>>, Arc<TestPlatform>) {
        let platform = Arc::new(TestPlatform::default());
        let mut engine = SyncEngine::new();
        engine.set_platform(platform.clone());
        (Arc::new(Mutex::new(engine)), platform)
    }

    fn text_frame(sequence: u32, envelope: &EventEnvelope) -> Frame {
        Frame::try_new(Command::TextPayload, 0, sequence, envelope.encode()).unwrap()
    }

    #[tokio::test]
    async fn text_event_is_applied_and_acknowledged() {
        let server_identity = Arc::new(DeviceIdentity::generate_for_test());
        let client_identity = DeviceIdentity::generate_for_test();
        let (mut client, mut server) = establish_pair(&server_identity, &client_identity).await;
        let db = Arc::new(Mutex::new(HistoryDB::new_unavailable().unwrap()));
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

    #[tokio::test]
    async fn transient_clipboard_failure_does_not_persist_or_deduplicate_event() {
        let server_identity = Arc::new(DeviceIdentity::generate_for_test());
        let client_identity = DeviceIdentity::generate_for_test();
        let (mut client, mut server) = establish_pair(&server_identity, &client_identity).await;
        let db = Arc::new(Mutex::new(HistoryDB::new_unavailable().unwrap()));
        let (engine, platform) = test_engine().await;
        platform.fail_text.store(true, Ordering::Relaxed);

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
            0
        );
        assert!(!engine
            .lock()
            .await
            .has_seen_message("client", envelope.message_id));

        platform.fail_text.store(false, Ordering::Relaxed);
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
            1
        );
        assert_eq!(platform.writes.lock().unwrap().as_slice(), ["retry me"]);
    }

    #[tokio::test]
    async fn duplicate_event_is_acknowledged_without_reapply() {
        let server_identity = Arc::new(DeviceIdentity::generate_for_test());
        let client_identity = DeviceIdentity::generate_for_test();
        let (mut client, mut server) = establish_pair(&server_identity, &client_identity).await;
        let db = Arc::new(Mutex::new(HistoryDB::new_unavailable().unwrap()));
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
        let _ = client.read_frame().await.unwrap();

        assert_eq!(platform.writes.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn replay_with_unknown_message_id_is_rejected() {
        let server_identity = Arc::new(DeviceIdentity::generate_for_test());
        let client_identity = DeviceIdentity::generate_for_test();
        let (mut client, mut server) = establish_pair(&server_identity, &client_identity).await;
        let db = Arc::new(Mutex::new(HistoryDB::new_unavailable().unwrap()));
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
        let server_identity = Arc::new(DeviceIdentity::generate_for_test());
        let client_identity = DeviceIdentity::generate_for_test();
        let (mut client, mut server) = establish_pair(&server_identity, &client_identity).await;
        let db = Arc::new(Mutex::new(HistoryDB::new_unavailable().unwrap()));
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
        let server_identity = Arc::new(DeviceIdentity::generate_for_test());
        let client_identity = DeviceIdentity::generate_for_test();
        let (mut client, mut server) = establish_pair(&server_identity, &client_identity).await;
        let db = Arc::new(Mutex::new(HistoryDB::new_unavailable().unwrap()));
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
}
