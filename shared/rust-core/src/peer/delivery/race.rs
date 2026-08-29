use std::future::Future;

use tokio::sync::mpsc;
use tokio::time::{timeout, Duration};

use crate::peer::types::{ConnectionInterface, ResolvedCandidate, ResolvedTarget};

/// Delay before a candidate attempt starts, biased toward preferred
/// interfaces so LAN wins when available without serializing the race:
/// LAN starts immediately; Iroh and Tailscale give the faster route a
/// short head start.
pub fn candidate_delay(interface: ConnectionInterface, has_lan: bool, has_iroh: bool) -> Duration {
    match interface {
        ConnectionInterface::Lan => Duration::ZERO,
        ConnectionInterface::Iroh if has_lan => Duration::from_millis(150),
        ConnectionInterface::Iroh => Duration::ZERO,
        ConnectionInterface::Tailscale if has_lan => Duration::from_millis(300),
        ConnectionInterface::Tailscale if has_iroh => Duration::from_millis(150),
        ConnectionInterface::Tailscale => Duration::ZERO,
    }
}

/// Race connect attempts across all candidates in parallel, applying the
/// per-interface delay bias so preferred routes win without blocking
/// fallbacks. The first successful attempt wins and every remaining attempt
/// is cancelled; if all attempts fail, the collected errors are joined.
/// `connect` performs the actual connection and handshake for one route;
/// it receives owned route values so its future can be `'static`.
pub async fn race_connections<T, F, Fut>(
    candidates: &[ResolvedCandidate],
    handshake_timeout: Duration,
    connect: F,
) -> Result<(T, ResolvedCandidate), String>
where
    T: Send + 'static,
    F: Fn(ResolvedTarget, ResolvedCandidate) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<T, String>> + Send + 'static,
{
    if candidates.is_empty() {
        return Err("no connection candidates to race".to_string());
    }
    let has_lan = candidates
        .iter()
        .any(|candidate| candidate.candidate.interface == ConnectionInterface::Lan);
    let has_iroh = candidates
        .iter()
        .any(|candidate| candidate.candidate.interface == ConnectionInterface::Iroh);
    let (tx, mut rx) = mpsc::channel(candidates.len().max(1));
    // A JoinSet owns its tasks: dropping it (including early returns and
    // cancellation of the race future itself) aborts every outstanding
    // connect attempt, so no handshake outlives the race.
    let mut tasks = tokio::task::JoinSet::new();
    let connect = std::sync::Arc::new(connect);

    for candidate in candidates.iter().cloned() {
        let tx = tx.clone();
        let connect = connect.clone();
        let delay = candidate_delay(candidate.candidate.interface, has_lan, has_iroh);
        tasks.spawn(async move {
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
            let started = tokio::time::Instant::now();
            let result = timeout(
                handshake_timeout,
                connect(candidate.target.clone(), candidate.clone()),
            )
            .await
            .map_err(|_| "handshake timed out".to_string())
            .and_then(|result| result);
            let mut candidate = candidate;
            candidate.candidate.latency = Some(started.elapsed().as_millis() as u64);
            let _ = tx.send((candidate, result)).await;
        });
    }
    drop(tx);

    let mut errors = Vec::new();
    while let Some((candidate, result)) = rx.recv().await {
        match result {
            Ok(stream) => {
                tasks.abort_all();
                return Ok((stream, candidate));
            }
            Err(error) => errors.push(format!(
                "{} {}: {error}",
                candidate.candidate.interface.as_str(),
                candidate.target
            )),
        }
    }
    Err(errors.join("; "))
}
