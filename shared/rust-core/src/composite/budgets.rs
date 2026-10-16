//! Multi-layer admission budgets (repair plan §4 R2-04).
//!
//! The verification found two accounting holes: the assembly limit was a single
//! global counter with no per-peer dimension (one authenticated peer could hold all
//! four slots with four connections), and nothing bounded the *CPU* a peer could buy
//! — rate limiting charged wire bytes, while a compressed chunk can inflate to the
//! full raw body.
//!
//! Five budgets, all keyed by the **authenticated public-key fingerprint** rather
//! than a connection, hostname or user-editable device name:
//!
//! | budget | rule |
//! |---|---|
//! | assembly | 1 per identity, 4 globally |
//! | raw CPU | 64 MiB burst, 64 MiB refilled per 60 s |
//! | decode | 1 per identity, 2 globally |
//! | pipeline memory | 256 MiB explicitly reserved |
//! | wire | unchanged; the existing wire rate budget still applies |
//!
//! Two deliberate properties:
//!
//! - **Non-blocking.** Every reservation either succeeds immediately or is rejected.
//!   Nothing ever waits while holding another budget, so the layers cannot deadlock
//!   against each other.
//! - **Raw CPU is not refunded on release.** The work was already spent, so the
//!   window survives a disconnect: a peer cannot reset its CPU budget by
//!   reconnecting, which is exactly the "换连接绕过" case.

use crate::protocol::ProtocolError;
use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

/// An authenticated peer, as a public-key fingerprint.
pub type Identity = [u8; 32];

/// Concurrent assemblies one identity may hold.
pub const ASSEMBLY_PER_IDENTITY: usize = 1;
/// Concurrent assemblies across all identities.
pub const ASSEMBLY_GLOBAL: usize = 4;
/// Concurrent decodes one identity may hold.
pub const DECODE_PER_IDENTITY: usize = 1;
/// Concurrent decodes across all identities.
pub const DECODE_GLOBAL: usize = 2;
/// Raw CPU bytes an identity may spend in one burst.
pub const RAW_BURST_BYTES: u64 = 64 * 1024 * 1024;
/// Raw CPU bytes refilled per window.
pub const RAW_REFILL_BYTES: u64 = 64 * 1024 * 1024;
/// Raw CPU refill window.
pub const RAW_REFILL_WINDOW: Duration = Duration::from_secs(60);
/// Total pipeline memory (wire + raw + pixels + native preparation + retry caches).
pub const PIPELINE_BUDGET_BYTES: u64 = 256 * 1024 * 1024;

fn rejected(reason: &str) -> ProtocolError {
    // The marker tells the sender this is temporary pressure it may retry, not a
    // rejection of the content (R4-03).
    ProtocolError::AdmissionRejected(format!(
        "{}{reason}",
        crate::protocol::TEMPORARY_PRESSURE_MARKER
    ))
}

#[derive(Debug, Clone, Copy)]
struct RawWindow {
    remaining: u64,
    last_refill: Instant,
}

impl RawWindow {
    fn refill(&mut self, now: Instant) {
        let elapsed = now.saturating_duration_since(self.last_refill);
        if elapsed.is_zero() {
            return;
        }
        let gained = (RAW_REFILL_BYTES as u128)
            .saturating_mul(elapsed.as_millis())
            .checked_div(RAW_REFILL_WINDOW.as_millis().max(1))
            .unwrap_or(0) as u64;
        if gained > 0 {
            self.remaining = (self.remaining.saturating_add(gained)).min(RAW_BURST_BYTES);
            self.last_refill = now;
        }
    }
}

#[derive(Default)]
struct Inner {
    assemblies: BTreeMap<Identity, usize>,
    assembly_total: usize,
    decodes: BTreeMap<Identity, usize>,
    decode_total: usize,
    raw: BTreeMap<Identity, RawWindow>,
    pipeline_reserved: u64,
}

/// A point-in-time view of the ledger, for diagnostics and tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BudgetSnapshot {
    pub assembly_total: usize,
    pub decode_total: usize,
    pub pipeline_reserved: u64,
}

/// The admission ledger.
///
/// Kept as an instance rather than a bare global so tests can use a private ledger
/// and stay isolated from each other; production uses [`global`].
#[derive(Default)]
pub struct BudgetLedger {
    inner: Mutex<Inner>,
}

impl std::fmt::Debug for BudgetLedger {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BudgetLedger")
            .field("snapshot", &self.snapshot())
            .finish()
    }
}

static GLOBAL: LazyLock<Arc<BudgetLedger>> = LazyLock::new(|| Arc::new(BudgetLedger::default()));

/// The process-wide ledger.
pub fn global() -> Arc<BudgetLedger> {
    GLOBAL.clone()
}

impl BudgetLedger {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn snapshot(&self) -> BudgetSnapshot {
        let inner = self.lock();
        BudgetSnapshot {
            assembly_total: inner.assembly_total,
            decode_total: inner.decode_total,
            pipeline_reserved: inner.pipeline_reserved,
        }
    }

    /// Admit the first chunk of a new assembly.
    ///
    /// `declared_raw` is the raw length the chunk header declares, charged against
    /// the raw CPU budget and reserved as pipeline memory. Rejected outright if any
    /// layer is full — never queued.
    pub fn admit_assembly(
        self: &Arc<Self>,
        identity: &Identity,
        declared_raw: u64,
        now: Instant,
    ) -> Result<AssemblyLease, ProtocolError> {
        let mut inner = self.lock();
        // Check every layer before committing any of them, so a refusal never leaves
        // a partial charge behind.
        let held = inner.assemblies.get(identity).copied().unwrap_or(0);
        if held >= ASSEMBLY_PER_IDENTITY {
            return Err(rejected(
                "composite assembly already in progress for this peer",
            ));
        }
        if inner.assembly_total >= ASSEMBLY_GLOBAL {
            return Err(rejected("composite assembly budget exhausted"));
        }
        // Raw CPU: a rate budget, so once committed it is *not* refunded on release.
        {
            let window = inner.raw.entry(*identity).or_insert(RawWindow {
                remaining: RAW_BURST_BYTES,
                last_refill: now,
            });
            window.refill(now);
            if declared_raw > window.remaining {
                return Err(rejected("composite raw CPU budget exhausted"));
            }
        }
        // Pipeline memory: reserved for the duration of the assembly.
        if inner.pipeline_reserved.saturating_add(declared_raw) > PIPELINE_BUDGET_BYTES {
            return Err(rejected("composite pipeline memory budget exhausted"));
        }
        if let Some(window) = inner.raw.get_mut(identity) {
            window.remaining -= declared_raw;
        }
        inner.pipeline_reserved += declared_raw;
        *inner.assemblies.entry(*identity).or_insert(0) += 1;
        inner.assembly_total += 1;
        Ok(AssemblyLease {
            ledger: self.clone(),
            identity: *identity,
            pipeline_bytes: declared_raw,
        })
    }

    /// Admit one decode (PNG decode / pixel conversion work).
    pub fn admit_decode(
        self: &Arc<Self>,
        identity: &Identity,
    ) -> Result<DecodeLease, ProtocolError> {
        let mut inner = self.lock();
        let held = inner.decodes.get(identity).copied().unwrap_or(0);
        if held >= DECODE_PER_IDENTITY {
            return Err(rejected(
                "composite decode already in progress for this peer",
            ));
        }
        if inner.decode_total >= DECODE_GLOBAL {
            return Err(rejected("composite decode budget exhausted"));
        }
        *inner.decodes.entry(*identity).or_insert(0) += 1;
        inner.decode_total += 1;
        Ok(DecodeLease {
            ledger: self.clone(),
            identity: *identity,
        })
    }

    /// Reserve pipeline memory directly (native preparation, retry caches).
    pub fn reserve_pipeline(self: &Arc<Self>, bytes: u64) -> Result<PipelineLease, ProtocolError> {
        let mut inner = self.lock();
        if inner.pipeline_reserved.saturating_add(bytes) > PIPELINE_BUDGET_BYTES {
            return Err(rejected("composite pipeline memory budget exhausted"));
        }
        inner.pipeline_reserved += bytes;
        Ok(PipelineLease {
            ledger: self.clone(),
            bytes,
        })
    }

    /// Raw CPU bytes still available to an identity, refilling with time.
    pub fn raw_remaining(&self, identity: &Identity, now: Instant) -> u64 {
        let mut inner = self.lock();
        match inner.raw.get_mut(identity) {
            Some(window) => {
                window.refill(now);
                window.remaining
            }
            None => RAW_BURST_BYTES,
        }
    }
}

/// The pipeline budget is global to the process, exactly as the product
/// behaves, so two tests that reserve from it at the same time reject each
/// other's legitimate reservations. Tests that consume this budget take this
/// permit first; each test still runs its own real concurrency, and only the
/// budget *users* are serialized against each other.
#[cfg(test)]
static PIPELINE_TEST_PERMIT: std::sync::OnceLock<Arc<tokio::sync::Semaphore>> =
    std::sync::OnceLock::new();

#[cfg(test)]
fn pipeline_test_permit() -> Arc<tokio::sync::Semaphore> {
    PIPELINE_TEST_PERMIT
        .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(1)))
        .clone()
}

/// Blocking form for `#[test]` cases, mirroring `sync_warning::test_lock`.
/// Take it before any other test lock so lock ordering stays consistent.
#[cfg(test)]
pub(crate) fn pipeline_budget_test_lock() -> tokio::sync::OwnedSemaphorePermit {
    loop {
        if let Ok(permit) = pipeline_test_permit().try_acquire_owned() {
            return permit;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

/// Async form for `#[tokio::test]` cases.
#[cfg(test)]
pub(crate) async fn pipeline_budget_test_lock_async() -> tokio::sync::OwnedSemaphorePermit {
    pipeline_test_permit().acquire_owned().await.unwrap()
}

/// Holds an assembly slot, its raw CPU charge and its pipeline reservation.
#[derive(Debug)]
pub struct AssemblyLease {
    ledger: Arc<BudgetLedger>,
    identity: Identity,
    pipeline_bytes: u64,
}

impl AssemblyLease {
    pub fn identity(&self) -> &Identity {
        &self.identity
    }
}

impl Drop for AssemblyLease {
    fn drop(&mut self) {
        let mut inner = self.ledger.lock();
        if let Some(held) = inner.assemblies.get_mut(&self.identity) {
            *held = held.saturating_sub(1);
            if *held == 0 {
                inner.assemblies.remove(&self.identity);
            }
        }
        inner.assembly_total = inner.assembly_total.saturating_sub(1);
        inner.pipeline_reserved = inner.pipeline_reserved.saturating_sub(self.pipeline_bytes);
        // The raw CPU charge is deliberately NOT refunded: the work was spent, and
        // refunding it would let a peer reset its budget by reconnecting.
    }
}

/// Holds one decode slot.
#[derive(Debug)]
pub struct DecodeLease {
    ledger: Arc<BudgetLedger>,
    identity: Identity,
}

impl Drop for DecodeLease {
    fn drop(&mut self) {
        let mut inner = self.ledger.lock();
        if let Some(held) = inner.decodes.get_mut(&self.identity) {
            *held = held.saturating_sub(1);
            if *held == 0 {
                inner.decodes.remove(&self.identity);
            }
        }
        inner.decode_total = inner.decode_total.saturating_sub(1);
    }
}

/// Holds a direct pipeline-memory reservation.
#[derive(Debug)]
pub struct PipelineLease {
    ledger: Arc<BudgetLedger>,
    bytes: u64,
}

impl PipelineLease {
    #[cfg(test)]
    pub(crate) fn reserved_bytes(&self) -> u64 {
        self.bytes
    }

    /// Release scratch capacity after assembly; capacity can never grow unchecked.
    pub fn shrink_to(&mut self, bytes: u64) {
        if bytes >= self.bytes {
            return;
        }
        let released = self.bytes - bytes;
        self.ledger.lock().pipeline_reserved -= released;
        self.bytes = bytes;
    }
}
impl Drop for PipelineLease {
    fn drop(&mut self) {
        let mut inner = self.ledger.lock();
        inner.pipeline_reserved = inner.pipeline_reserved.saturating_sub(self.bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(byte: u8) -> Identity {
        [byte; 32]
    }

    /// Gate R2: one authenticated peer must not be able to hold four slots with four
    /// connections. The second assembly from the same identity is refused, while a
    /// different identity is still admitted.
    #[test]
    fn one_identity_cannot_hold_more_than_one_assembly() {
        let ledger = Arc::new(BudgetLedger::default());
        let now = Instant::now();
        let first = ledger.admit_assembly(&identity(1), 1024, now).unwrap();
        assert!(
            ledger.admit_assembly(&identity(1), 1024, now).is_err(),
            "a second assembly from the same authenticated peer must be refused"
        );
        let other = ledger
            .admit_assembly(&identity(2), 1024, now)
            .expect("a different peer is unaffected");
        assert_eq!(ledger.snapshot().assembly_total, 2);
        drop(first);
        assert_eq!(ledger.snapshot().assembly_total, 1);
        assert!(
            ledger.admit_assembly(&identity(1), 1024, now).is_ok(),
            "releasing the slot lets the peer start again"
        );
        drop(other);
    }

    /// The global cap still applies across identities.
    #[test]
    fn the_global_assembly_cap_still_applies() {
        let ledger = Arc::new(BudgetLedger::default());
        let now = Instant::now();
        let mut leases = Vec::new();
        for index in 0..ASSEMBLY_GLOBAL as u8 {
            leases.push(ledger.admit_assembly(&identity(index), 1024, now).unwrap());
        }
        assert!(
            ledger.admit_assembly(&identity(99), 1024, now).is_err(),
            "the global budget must still bound the number of concurrent assemblies"
        );
        leases.clear();
        assert!(ledger.admit_assembly(&identity(99), 1024, now).is_ok());
    }

    /// Raw CPU is a *rate* budget: spending it is not refunded by releasing the
    /// assembly, so reconnecting cannot reset it. Time refills it.
    #[test]
    fn raw_cpu_is_charged_once_and_refills_with_time() {
        let ledger = Arc::new(BudgetLedger::default());
        let now = Instant::now();
        let big = RAW_BURST_BYTES;
        let lease = ledger.admit_assembly(&identity(1), big, now).unwrap();
        assert_eq!(ledger.raw_remaining(&identity(1), now), 0);
        drop(lease);
        assert_eq!(
            ledger.raw_remaining(&identity(1), now),
            0,
            "releasing the assembly must not refund spent CPU"
        );
        assert!(
            ledger.admit_assembly(&identity(1), 1, now).is_err(),
            "an exhausted raw budget refuses the next assembly"
        );
        let later = now + RAW_REFILL_WINDOW;
        assert!(
            ledger.raw_remaining(&identity(1), later) >= RAW_REFILL_BYTES,
            "the window refills over time"
        );
        assert!(ledger.admit_assembly(&identity(1), 1024, later).is_ok());
    }

    /// Decode is a concurrency budget: one per identity, two globally, released on
    /// drop.
    #[test]
    fn decode_budget_is_per_identity_and_global() {
        let ledger = Arc::new(BudgetLedger::default());
        let first = ledger.admit_decode(&identity(1)).unwrap();
        assert!(ledger.admit_decode(&identity(1)).is_err());
        let second = ledger.admit_decode(&identity(2)).unwrap();
        assert!(
            ledger.admit_decode(&identity(3)).is_err(),
            "the global decode budget is two"
        );
        drop(first);
        let third = ledger
            .admit_decode(&identity(3))
            .expect("a released slot is reusable");
        assert_eq!(ledger.snapshot().decode_total, 2);
        drop(second);
        assert_eq!(ledger.snapshot().decode_total, 1);
        drop(third);
        assert_eq!(ledger.snapshot().decode_total, 0);
    }

    /// Pipeline memory is an explicit reservation that is released on drop.
    #[test]
    fn pipeline_memory_is_reserved_and_released() {
        let ledger = Arc::new(BudgetLedger::default());
        let first = ledger.reserve_pipeline(PIPELINE_BUDGET_BYTES / 2).unwrap();
        let second = ledger.reserve_pipeline(PIPELINE_BUDGET_BYTES / 2).unwrap();
        assert!(
            ledger.reserve_pipeline(1).is_err(),
            "the pipeline budget is a hard ceiling"
        );
        assert_eq!(ledger.snapshot().pipeline_reserved, PIPELINE_BUDGET_BYTES);
        drop(first);
        assert_eq!(
            ledger.snapshot().pipeline_reserved,
            PIPELINE_BUDGET_BYTES / 2
        );
        drop(second);
        assert_eq!(ledger.snapshot().pipeline_reserved, 0);
    }

    /// An assembly also reserves pipeline memory for its raw length.
    #[test]
    fn assembly_reserves_pipeline_memory_for_its_raw_length() {
        let ledger = Arc::new(BudgetLedger::default());
        let lease = ledger
            .admit_assembly(&identity(1), 4096, Instant::now())
            .unwrap();
        assert_eq!(ledger.snapshot().pipeline_reserved, 4096);
        drop(lease);
        assert_eq!(ledger.snapshot().pipeline_reserved, 0);
    }

    /// Every reservation is non-blocking: a full layer is refused, never queued, so
    /// one layer can never be held while waiting on another.
    #[test]
    fn reservations_never_block() {
        let ledger = Arc::new(BudgetLedger::default());
        let now = Instant::now();
        let _held = ledger.admit_assembly(&identity(1), 4096, now).unwrap();
        let started = Instant::now();
        for _ in 0..1000 {
            let _ = ledger.admit_assembly(&identity(1), 4096, now);
        }
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "a refused reservation must return immediately"
        );
    }
}
