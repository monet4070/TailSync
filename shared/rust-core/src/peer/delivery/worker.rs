use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::{mpsc, watch};
use tokio::time::{timeout, Duration};
use tracing::Instrument;

use crate::peer::types::{ActiveRoute, ConnectionInterface, ResolvedCandidate};

use super::*;

/// Timing for the per-peer connection worker loop. Defaults match the shared
/// platform constants (30 s heartbeat, 10 s heartbeat ACK window, 5 s
/// reconnect delay).
#[derive(Debug, Clone, Copy)]
pub struct WorkerConfig {
    pub heartbeat_interval: Duration,
    pub heartbeat_ack_timeout: Duration,
    pub reconnect_delay: Duration,
    /// Upper bound on a single candidate-refresh. Refresh acquires the shared
    /// settings lock; if that contends or the settings task wedges, the worker
    /// would park before ever reaching `connect` and the link could never
    /// self-heal. Bounding it guarantees the loop always makes forward progress.
    pub refresh_timeout: Duration,
    /// Maximum time a frame may wait across disconnects. This is aligned with
    /// the protocol's five-minute event timestamp window and also bounds file
    /// frames whose caller has already timed out.
    pub pending_frame_ttl: Duration,
    pub delivery: DeliveryConfig,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            heartbeat_interval: Duration::from_secs(30),
            heartbeat_ack_timeout: Duration::from_secs(10),
            reconnect_delay: Duration::from_secs(5),
            refresh_timeout: Duration::from_secs(5),
            pending_frame_ttl: Duration::from_secs(5 * 60),
            delivery: DeliveryConfig::DEFAULT,
        }
    }
}

/// Platform capabilities the per-peer connection worker needs. The adapter
/// owns candidate resolution, the connection + handshake executor, session
/// registration, protocol-compatibility diagnostics, and remembered-Iroh
/// refresh; the worker owns the lifecycle loop itself (reconnect, heartbeat,
/// keep-frame-across-reconnect, priority queue selection) so both platforms
/// run a single implementation.
#[allow(async_fn_in_trait)] // Concrete adapters; Send bounds are checked at the worker boundary.
pub trait ConnectionAdapter: Send + Sync {
    type Connection: DeliveryConnection;
    type SessionLease: Send;

    /// Connect and authenticate one route. Failures are retried by the
    /// worker after `reconnect_delay`.
    async fn connect(
        &self,
        hostname: &str,
        candidates: &[ResolvedCandidate],
    ) -> Result<(Self::Connection, ResolvedCandidate), String>;

    /// Record an authenticated session for the route (forces `connected`).
    fn register_session(
        &self,
        hostname: &str,
        interface: ConnectionInterface,
        address: &str,
        latency_ms: u64,
    ) -> Self::SessionLease;

    fn record_protocol_error(&self, hostname: &str, error: &str);
    fn clear_protocol_error(&self, hostname: &str);

    /// Refresh remembered Iroh candidates; returns true when a new preferred
    /// route was learned (the worker then reselects the path).
    async fn refresh_candidates(
        &self,
        hostname: &str,
        candidates: &mut Vec<ResolvedCandidate>,
    ) -> bool;
}

async fn wait_for_shutdown(shutdown: &mut watch::Receiver<bool>) {
    if *shutdown.borrow() {
        return;
    }
    while shutdown.changed().await.is_ok() {
        if *shutdown.borrow() {
            return;
        }
    }
}

pub(super) async fn receive_scheduled_frame(
    priority_rx: &mut mpsc::Receiver<QueuedFrame>,
    bulk_rx: &mut mpsc::Receiver<QueuedFrame>,
    priority_streak: &mut usize,
) -> Option<QueuedFrame> {
    loop {
        let priority_open = !priority_rx.is_closed() || !priority_rx.is_empty();
        let bulk_open = !bulk_rx.is_closed() || !bulk_rx.is_empty();
        if !priority_open && !bulk_open {
            return None;
        }

        if *priority_streak >= PRIORITY_BURST_LIMIT && bulk_open {
            match bulk_rx.try_recv() {
                Ok(frame) => {
                    *priority_streak = 0;
                    return Some(frame);
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) if !priority_open => return None,
                Err(TryRecvError::Disconnected) => {}
            }
        }

        tokio::select! {
            biased;
            frame = priority_rx.recv(), if priority_open => {
                if let Some(frame) = frame {
                    *priority_streak = priority_streak.saturating_add(1);
                    return Some(frame);
                }
            }
            frame = bulk_rx.recv(), if bulk_open => {
                if let Some(frame) = frame {
                    *priority_streak = 0;
                    return Some(frame);
                }
            }
        }
    }
}

pub(super) fn should_reselect_route_after_receipt(
    command: Command,
    receipt: &DeliveryReceipt,
    route: &ResolvedCandidate,
    candidates: &[ResolvedCandidate],
) -> bool {
    command == Command::FileChunk
        && receipt.resume_required
        && candidates
            .iter()
            .any(|candidate| candidate.candidate.priority < route.candidate.priority)
}

/// Pull one frame while the worker is offline, dropping stale frames and
/// retaining the first live frame for the next connection attempt. This is a
/// non-blocking maintenance pass: fresh frames remain ordered within each
/// queue, while a full queue can no longer be occupied forever by an old
/// frame that has no chance of being accepted by the peer.
fn maintain_offline_queue(
    priority_rx: &mut mpsc::Receiver<QueuedFrame>,
    bulk_rx: &mut mpsc::Receiver<QueuedFrame>,
    pending: &mut Option<PendingFrame>,
    next_sequence: &mut u32,
    hostname: &str,
    ttl: Duration,
) {
    if pending.is_some() {
        return;
    }
    loop {
        let queued = match priority_rx.try_recv() {
            Ok(frame) => Some(frame),
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => {
                match bulk_rx.try_recv() {
                    Ok(frame) => Some(frame),
                    Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => None,
                }
            }
        };
        let Some(queued) = queued else { return };
        let frame = PendingFrame::new_for_peer(queued, *next_sequence, hostname);
        *next_sequence = next_sequence.wrapping_add(1).max(1);
        if frame.is_expired(ttl) {
            complete_expired_frame(frame, hostname);
        } else {
            *pending = Some(frame);
            return;
        }
    }
}

/// Background worker for one pooled connection: connects and handshakes,
/// serves queued frames with heartbeat keepalive, reconnects transparently
/// on transient failures, and keeps the in-flight frame across reconnects so
/// clipboard content is never lost silently. Exits when all senders drop or
/// shutdown is signalled.
pub async fn run_connection_worker<A: ConnectionAdapter>(
    adapter: &A,
    config: &WorkerConfig,
    mut candidates: Vec<ResolvedCandidate>,
    hostname: String,
    mut priority_rx: mpsc::Receiver<QueuedFrame>,
    mut bulk_rx: mpsc::Receiver<QueuedFrame>,
    mut shutdown: watch::Receiver<bool>,
) {
    let Some(preferred_target) = candidates.first().map(|candidate| candidate.target.clone())
    else {
        log::warn!("Connection task for {hostname} started without a route");
        return;
    };
    let mut pending: Option<PendingFrame> = None;
    let mut next_sequence = 1u32;
    let mut priority_streak = 0usize;
    let mut connection_attempt = 0usize;
    'connection: loop {
        connection_attempt = connection_attempt.saturating_add(1);
        maintain_offline_queue(
            &mut priority_rx,
            &mut bulk_rx,
            &mut pending,
            &mut next_sequence,
            &hostname,
            config.pending_frame_ttl,
        );
        if timeout(
            config.refresh_timeout,
            adapter.refresh_candidates(&hostname, &mut candidates),
        )
        .await
        .is_err()
        {
            log::warn!(
                "Candidate refresh for {hostname} exceeded {:?}; proceeding with known routes",
                config.refresh_timeout
            );
        }
        let (connection_id, connection_result) = {
            let connection_id = crate::observability::connection_id();
            let span = tracing::info_span!(
                "connection.attempt",
                connection_id = %connection_id,
                peer = %hostname,
                attempt = connection_attempt,
            );
            let connection = adapter.connect(&hostname, &candidates).instrument(span);
            tokio::pin!(connection);
            let result = tokio::select! {
                biased;
                _ = wait_for_shutdown(&mut shutdown) => {
                    crate::sync_warning::record_delivery_shutdown(&hostname);
                    return;
                },
                result = &mut connection => result,
            };
            (connection_id, result)
        };
        let (mut stream, route) = match connection_result {
            Ok(result) => {
                adapter.clear_protocol_error(&hostname);
                result
            }
            Err(error) => {
                if error.contains("Incompatible TailSync protocol:") {
                    adapter.record_protocol_error(&hostname, &error);
                }
                log::warn!(
                    "Pool connect to {} ({}) failed connection_id={}: {} — retrying in {:?}",
                    preferred_target,
                    hostname,
                    connection_id,
                    error,
                    config.reconnect_delay
                );
                tokio::select! {
                    biased;
                    _ = wait_for_shutdown(&mut shutdown) => {
                        crate::sync_warning::record_delivery_shutdown(&hostname);
                        return;
                    },
                    _ = tokio::time::sleep(config.reconnect_delay) => {}
                }
                continue;
            }
        };
        tracing::info!(
            connection_id = %connection_id,
            peer = %hostname,
            interface = %route.candidate.interface.as_str(),
            session_id = ?stream.session_id(),
            "delivery session ready"
        );
        let learned_iroh = timeout(
            config.refresh_timeout,
            adapter.refresh_candidates(&hostname, &mut candidates),
        )
        .await
        .unwrap_or(false);
        if learned_iroh
            && candidates
                .iter()
                .any(|candidate| candidate.candidate.priority < route.candidate.priority)
        {
            log::debug!("Learned a preferred Iroh route for {hostname}; reselecting path");
            continue;
        }
        let target = route.target.clone();
        let active = ActiveRoute {
            interface: route.candidate.interface,
            address: route.candidate.address.clone(),
            latency: route.candidate.latency.unwrap_or_default(),
        };
        log::debug!(
            "Pool connected to {} via {} in {} ms",
            target,
            active.interface.as_str(),
            active.latency
        );
        let _session_lease = adapter.register_session(
            &hostname,
            active.interface,
            &route.candidate.address,
            active.latency,
        );

        let mut last_heartbeat = tokio::time::Instant::now();

        // A write can fail after the frame has been removed from the queue.
        // Keep that frame across reconnects so transient breaks do not lose
        // clipboard content silently.
        if let Some(frame) = pending.take() {
            if frame.is_expired(config.pending_frame_ttl) {
                complete_expired_frame(frame, &hostname);
                continue;
            }
            let session_id = stream.session_id().map(str::to_owned);
            let delivery = tokio::select! {
                biased;
                _ = wait_for_shutdown(&mut shutdown) => {
                    crate::sync_warning::record_delivery_shutdown(&hostname);
                    return;
                },
                result = deliver_pending_frame(&mut stream, &frame, &config.delivery).instrument(
                    tracing::info_span!(
                        "delivery.frame",
                        connection_id = %connection_id,
                        session_id = ?session_id,
                        peer = %hostname,
                        sequence = frame.sequence,
                    ),
                ) => result,
            };
            match delivery {
                Ok(receipt) => {
                    tracing::info!(
                        connection_id = %connection_id,
                        session_id = ?stream.session_id(),
                        peer = %hostname,
                        sequence = frame.sequence,
                        command = ?frame.queued.command(),
                        "delivery completed"
                    );
                    let reselect_route = should_reselect_route_after_receipt(
                        frame.queued.command(),
                        &receipt,
                        &route,
                        &candidates,
                    );
                    frame.complete(Ok(receipt));
                    if reselect_route {
                        log::debug!(
                            "Receiver requested file batch replay over fallback route {target}; reselecting preferred path"
                        );
                        continue 'connection;
                    }
                }
                Err(error) if !error.is_retryable() => {
                    record_permanent_delivery_warning(&hostname, &error);
                    log::warn!("Dropping event rejected by remote peer: {error}");
                    log::debug!("Rejected event route: {target}");
                    frame.complete(Err(error));
                }
                Err(error) => {
                    log::debug!(
                        "Pool delivery to {} failed: {error} — reselecting path",
                        target
                    );
                    pending = Some(frame);
                    continue;
                }
            }
        }

        // Inner loop: read from channel, write to wire
        loop {
            if last_heartbeat.elapsed() >= config.heartbeat_interval {
                let Ok(hb) = Frame::try_new(Command::Heartbeat, 0, next_sequence, vec![]) else {
                    log::error!("Could not construct a heartbeat frame");
                    return;
                };
                next_sequence = next_sequence.wrapping_add(1).max(1);
                let heartbeat_ok = tokio::select! {
                    biased;
                    _ = wait_for_shutdown(&mut shutdown) => {
                        crate::sync_warning::record_delivery_shutdown(&hostname);
                        return;
                    },
                    result = async {
                        if stream.write_frame(&hb).await.is_err() {
                            return false;
                        }
                        matches!(
                            timeout(config.heartbeat_ack_timeout, stream.read_frame()).await,
                            Ok(Ok(Frame { command: Command::HeartbeatAck, .. }))
                        )
                    } => result,
                };
                if !heartbeat_ok {
                    log::debug!("Pool heartbeat to {} failed — reconnecting", target);
                    break;
                }
                last_heartbeat = tokio::time::Instant::now();
            }

            // Wait for next frame or heartbeat deadline
            let deadline = config
                .heartbeat_interval
                .saturating_sub(last_heartbeat.elapsed());
            let next_frame =
                receive_scheduled_frame(&mut priority_rx, &mut bulk_rx, &mut priority_streak);
            let next = tokio::select! {
                biased;
                    _ = wait_for_shutdown(&mut shutdown) => {
                        crate::sync_warning::record_delivery_shutdown(&hostname);
                        return;
                    },
                result = tokio::time::timeout(deadline, next_frame) => result,
            };
            match next {
                Ok(Some(queued)) => {
                    let frame = PendingFrame::new_for_peer(queued, next_sequence, &hostname);
                    next_sequence = next_sequence.wrapping_add(1).max(1);
                    if frame.is_expired(config.pending_frame_ttl) {
                        complete_expired_frame(frame, &hostname);
                        continue;
                    }
                    let session_id = stream.session_id().map(str::to_owned);
                    let delivery = tokio::select! {
                        biased;
                        _ = wait_for_shutdown(&mut shutdown) => {
                            crate::sync_warning::record_delivery_shutdown(&hostname);
                            return;
                        },
                        result = deliver_pending_frame(&mut stream, &frame, &config.delivery).instrument(
                            tracing::info_span!(
                                "delivery.frame",
                                connection_id = %connection_id,
                                session_id = ?session_id,
                                peer = %hostname,
                                sequence = frame.sequence,
                            ),
                        ) => result,
                    };
                    match delivery {
                        Ok(receipt) => {
                            tracing::info!(
                                connection_id = %connection_id,
                                session_id = ?stream.session_id(),
                                peer = %hostname,
                                sequence = frame.sequence,
                                command = ?frame.queued.command(),
                                "delivery completed"
                            );
                            let reselect_route = should_reselect_route_after_receipt(
                                frame.queued.command(),
                                &receipt,
                                &route,
                                &candidates,
                            );
                            frame.complete(Ok(receipt));
                            if reselect_route {
                                log::debug!(
                                    "Receiver requested file batch replay over fallback route {target}; reselecting preferred path"
                                );
                                continue 'connection;
                            }
                        }
                        Err(error) if !error.is_retryable() => {
                            record_permanent_delivery_warning(&hostname, &error);
                            log::warn!("Dropping event rejected by remote peer: {error}");
                            log::debug!("Rejected event route: {target}");
                            frame.complete(Err(error));
                        }
                        Err(error) => {
                            pending = Some(frame);
                            log::debug!(
                                "Pool delivery to {} failed: {error} — reselecting path",
                                target
                            );
                            break;
                        }
                    }
                }
                Ok(None) => {
                    // All senders dropped — exit this connection for good
                    log::debug!("Pool channel for {} closed — shutting down", target);
                    return;
                }
                Err(_) => {
                    // Timeout — loop back to send heartbeat
                }
            }
        }
        // Outer loop: reconnect and try again
    }
}
