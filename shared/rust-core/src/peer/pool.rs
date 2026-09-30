//! Shared connection-pool mechanics.
//!
//! The platform crates still own candidate resolution, trust checks, and the
//! concrete connection worker adapter.  This module owns the state that was
//! previously duplicated in both network crates: sender channels, worker
//! replacement, per-peer batch serialization, and bounded queue handoff.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{mpsc, oneshot, watch, Mutex, Notify};
use tokio::time::{timeout, Duration};

use crate::peer::delivery::QueuedFrame;
use crate::peer::types::DeliveryReceipt;
use crate::peer::types::{ResolvedCandidate, ResolvedTarget};
use crate::protocol::Command;

pub(crate) const CHANNEL_SIZE: usize = 64;
const SEND_TIMEOUT: Duration = Duration::from_secs(5);

/// The two bounded queues used by a pooled peer worker.
#[derive(Clone)]
pub struct PoolSender {
    priority: mpsc::Sender<QueuedFrame>,
    bulk: mpsc::Sender<QueuedFrame>,
    shutdown: watch::Sender<bool>,
    retry_wakeup: Arc<Notify>,
    candidates: watch::Sender<Vec<ResolvedCandidate>>,
}

impl PoolSender {
    /// Construct a sender from its worker channels. This is public so platform
    /// integration tests can exercise the shared pool without reaching into
    /// private channel fields.
    pub fn new(
        priority: mpsc::Sender<QueuedFrame>,
        bulk: mpsc::Sender<QueuedFrame>,
        shutdown: watch::Sender<bool>,
    ) -> Self {
        let (candidates, _) = watch::channel(Vec::new());
        Self::with_retry_wakeup(
            priority,
            bulk,
            shutdown,
            Arc::new(Notify::new()),
            candidates,
        )
    }

    fn with_retry_wakeup(
        priority: mpsc::Sender<QueuedFrame>,
        bulk: mpsc::Sender<QueuedFrame>,
        shutdown: watch::Sender<bool>,
        retry_wakeup: Arc<Notify>,
        candidates: watch::Sender<Vec<ResolvedCandidate>>,
    ) -> Self {
        Self {
            priority,
            bulk,
            shutdown,
            retry_wakeup,
            candidates,
        }
    }

    pub fn channel_for(&self, command: Command) -> &mpsc::Sender<QueuedFrame> {
        if command == Command::FileChunk {
            &self.bulk
        } else {
            &self.priority
        }
    }

    pub fn request_shutdown(&self) {
        let _ = self.shutdown.send(true);
    }

    pub fn same_channel(&self, other: &Self) -> bool {
        self.priority.same_channel(&other.priority) && self.bulk.same_channel(&other.bulk)
    }

    pub fn priority_is_closed(&self) -> bool {
        self.priority.is_closed()
    }

    pub fn bulk_is_closed(&self) -> bool {
        self.bulk.is_closed()
    }
}

/// Platform-independent mutable state for a connection pool.
pub struct ConnectionPoolState {
    senders: HashMap<(ResolvedTarget, String), PoolSender>,
    batch_serializers: HashMap<String, Arc<Mutex<()>>>,
}

impl Default for ConnectionPoolState {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectionPoolState {
    pub fn new() -> Self {
        Self {
            senders: HashMap::new(),
            batch_serializers: HashMap::new(),
        }
    }

    /// Return a live sender for a resolved candidate set, replacing a worker
    /// whose receiver has already gone away. The platform supplies the worker
    /// spawn closure so the shared state never depends on a concrete socket or
    /// handshake implementation.
    pub fn sender_for_candidates<F>(
        &mut self,
        hostname: String,
        candidates: Vec<ResolvedCandidate>,
        spawn_worker: F,
    ) -> Result<PoolSender, String>
    where
        F: FnOnce(
                Vec<ResolvedCandidate>,
                String,
                mpsc::Receiver<QueuedFrame>,
                mpsc::Receiver<QueuedFrame>,
                watch::Receiver<bool>,
                Arc<Notify>,
                watch::Receiver<Vec<ResolvedCandidate>>,
            ) + Send
            + 'static,
    {
        let target = candidates
            .first()
            .ok_or_else(|| format!("Peer {hostname} has no usable connection candidates"))?
            .target
            .clone();
        // Reuse the live worker for this hostname regardless of a route
        // change. The preferred target is dynamic — background network
        // introspection can re-rank candidates mid-transfer — and keying on
        // the target alone would shut the worker down and abort an in-flight
        // transfer. The candidate watch supplies the latest routes on its
        // next reconnect; an explicit disconnect or unpair
        // still closes it through `disconnect_hostname`.
        if let Some((_, sender)) = self
            .senders
            .iter()
            .find(|((_, known_hostname), _)| known_hostname == &hostname)
        {
            if !sender.priority_is_closed() && !sender.bulk_is_closed() {
                // Keep the active connection, but give its next reconnect the
                // latest LAN/Tailscale routes as well as the Iroh refresh.
                sender.candidates.send_replace(candidates);
                return Ok(sender.clone());
            }
        }

        // Only a dead worker for this hostname reaches here; drop it before
        // spawning its replacement.
        self.senders.retain(|(_, peer_hostname), sender| {
            let keep = peer_hostname != &hostname;
            if !keep {
                sender.request_shutdown();
            }
            keep
        });

        let (priority, priority_rx) = mpsc::channel::<QueuedFrame>(CHANNEL_SIZE);
        let (bulk, bulk_rx) = mpsc::channel::<QueuedFrame>(CHANNEL_SIZE);
        let (shutdown, shutdown_rx) = watch::channel(false);
        let retry_wakeup = Arc::new(Notify::new());
        let (candidate_updates, candidate_rx) = watch::channel(candidates.clone());
        let sender = PoolSender::with_retry_wakeup(
            priority,
            bulk,
            shutdown,
            retry_wakeup.clone(),
            candidate_updates,
        );
        self.senders
            .insert((target, hostname.clone()), sender.clone());
        spawn_worker(
            candidates,
            hostname,
            priority_rx,
            bulk_rx,
            shutdown_rx,
            retry_wakeup,
            candidate_rx,
        );
        Ok(sender)
    }

    pub fn batch_serializer(&mut self, hostname: &str) -> Arc<Mutex<()>> {
        self.batch_serializers
            .entry(hostname.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    pub fn disconnect_hostname(&mut self, hostname: &str) {
        self.senders.retain(|(_, peer_hostname), sender| {
            let keep = peer_hostname != hostname;
            if !keep {
                sender.request_shutdown();
            }
            keep
        });
    }

    pub fn disconnect_all(&mut self) {
        for sender in self.senders.values() {
            sender.request_shutdown();
        }
        self.senders.clear();
    }

    pub fn sender_count(&self) -> usize {
        self.senders.len()
    }

    pub fn insert_sender(&mut self, target: ResolvedTarget, hostname: String, sender: PoolSender) {
        self.senders.insert((target, hostname), sender);
    }
}

pub async fn acquire_batch_serializer(
    state: &Arc<Mutex<ConnectionPoolState>>,
    hostname: &str,
) -> tokio::sync::OwnedMutexGuard<()> {
    let serializer = state.lock().await.batch_serializer(hostname);
    serializer.lock_owned().await
}

pub async fn enqueue_queued_frame(
    sender: PoolSender,
    target: ResolvedTarget,
    queued: QueuedFrame,
) -> Result<(), String> {
    let command = queued.command();
    timeout(SEND_TIMEOUT, sender.channel_for(command).send(queued))
        .await
        .map_err(|_| format!("Timed out queueing frame for {target}"))?
        .map_err(|_| format!("Connection to {target} closed"))?;
    sender.retry_wakeup.notify_one();
    Ok(())
}

pub async fn await_delivery(
    completion_rx: oneshot::Receiver<Result<DeliveryReceipt, crate::peer::delivery::DeliveryError>>,
    command: Command,
    hostname: &str,
    timeout_duration: Duration,
) -> Result<DeliveryReceipt, String> {
    timeout(timeout_duration, completion_rx)
        .await
        .map_err(|_| format!("Timed out waiting for {command:?} confirmation"))?
        .map_err(|_| format!("Connection task for {hostname} closed"))?
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate() -> ResolvedCandidate {
        ResolvedCandidate {
            candidate: crate::peer::types::PeerCandidate::new(
                crate::peer::types::ConnectionInterface::Lan,
                "127.0.0.1",
            ),
            target: ResolvedTarget::Tcp("127.0.0.1:19890".parse().unwrap()),
        }
    }

    #[tokio::test]
    async fn batch_serializers_are_per_peer() {
        let state = Arc::new(Mutex::new(ConnectionPoolState::new()));
        let first = acquire_batch_serializer(&state, "peer-a").await;
        assert!(timeout(
            Duration::from_millis(20),
            acquire_batch_serializer(&state, "peer-a")
        )
        .await
        .is_err());
        assert!(timeout(
            Duration::from_millis(20),
            acquire_batch_serializer(&state, "peer-b")
        )
        .await
        .is_ok());
        drop(first);
    }

    #[tokio::test]
    async fn sender_for_candidates_replaces_dead_workers() {
        let mut state = ConnectionPoolState::new();
        let (priority, priority_rx) = mpsc::channel(CHANNEL_SIZE);
        let (bulk, bulk_rx) = mpsc::channel(CHANNEL_SIZE);
        let (shutdown, _shutdown_rx) = watch::channel(false);
        drop(priority_rx);
        drop(bulk_rx);
        state.insert_sender(
            candidate().target.clone(),
            "peer".to_string(),
            PoolSender::new(priority, bulk, shutdown),
        );

        let rebuilt = state
            .sender_for_candidates(
                "peer".to_string(),
                vec![candidate()],
                |_, _, priority_rx, bulk_rx, shutdown_rx, _retry_wakeup, _candidate_rx| {
                    tokio::spawn(async move {
                        let _receivers = (priority_rx, bulk_rx, shutdown_rx);
                        std::future::pending::<()>().await;
                    });
                },
            )
            .unwrap();
        assert_eq!(state.sender_count(), 1);
        assert!(!rebuilt.priority_is_closed());
    }

    #[tokio::test]
    async fn sender_for_candidates_keeps_a_live_worker_across_route_changes() {
        let mut state = ConnectionPoolState::new();
        let (priority, priority_rx) = mpsc::channel(CHANNEL_SIZE);
        let (bulk, bulk_rx) = mpsc::channel(CHANNEL_SIZE);
        let (shutdown, shutdown_rx) = watch::channel(false);
        // Keep the receivers alive so the cached worker is not "dead".
        let _keep_alive = (priority_rx, bulk_rx);
        state.insert_sender(
            ResolvedTarget::Tcp("10.0.0.1:19890".parse().unwrap()),
            "peer".to_string(),
            PoolSender::new(priority, bulk, shutdown),
        );

        let mut rerouted = candidate();
        rerouted.target = ResolvedTarget::Tcp("10.0.0.2:19890".parse().unwrap());
        let reused = state
            .sender_for_candidates(
                "peer".to_string(),
                vec![rerouted.clone()],
                |_, _, _, _, _, _, _| panic!("a live worker must survive a preferred-route change"),
            )
            .unwrap();

        assert_eq!(state.sender_count(), 1);
        assert!(!reused.priority_is_closed());
        assert_eq!(
            reused.candidates.subscribe().borrow()[0].target,
            rerouted.target,
            "the next reconnect must use the new candidate"
        );
        assert!(
            !*shutdown_rx.borrow(),
            "a re-ranked route must not shut the worker down"
        );
    }
}
