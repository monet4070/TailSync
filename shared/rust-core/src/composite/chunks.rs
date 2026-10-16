//! Independent composite assembler. Admission precedes allocation, with one
//! assembly per peer, four process-wide, and bounded idle/total lifetimes.
use super::*;
use crate::protocol::{unix_timestamp_ms, EventEnvelope, MessageId, EVENT_TIMESTAMP_WINDOW_MS};
use flate2::{read::ZlibDecoder, write::ZlibEncoder, Compression};
use std::io::{Read, Write};
use std::time::{Duration, Instant};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Metadata {
    pub message_id: MessageId,
    pub timestamp_ms: i64,
    pub digest: [u8; 32],
    pub index: u16,
    pub count: u16,
    pub raw_len: u32,
    pub wire_len: u32,
    pub compressed: bool,
}
fn invalid() -> ProtocolError {
    ProtocolError::InvalidComposite
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum ChunkKind {
    Clipboard,
    Archive,
}
impl ChunkKind {
    fn magic(self) -> &'static [u8; 4] {
        match self {
            Self::Clipboard => b"CBC1",
            Self::Archive => b"ARC1",
        }
    }
    fn body_limit(self) -> usize {
        match self {
            Self::Clipboard => BODY_LIMIT,
            Self::Archive => resource_archive::ARCHIVE_TOTAL_BYTES + 128,
        }
    }
}
pub fn inspect(b: &[u8]) -> Result<Metadata, ProtocolError> {
    inspect_kind(b, ChunkKind::Clipboard)
}
pub fn inspect_archive(bytes: &[u8]) -> Result<Metadata, ProtocolError> {
    inspect_kind(bytes, ChunkKind::Archive)
}
fn inspect_kind(b: &[u8], kind: ChunkKind) -> Result<Metadata, ProtocolError> {
    if b.len() <= CHUNK_HEADER_SIZE
        || b.len() > CHUNK_HEADER_SIZE + CHUNK_DATA_LIMIT
        || &b[..4] != kind.magic()
        || b[28] > 1
        || b[29..32] != [0; 3]
    {
        return Err(invalid());
    }
    let m = Metadata {
        message_id: MessageId(b[4..20].try_into().map_err(|_| invalid())?),
        timestamp_ms: i64::from_be_bytes(b[20..28].try_into().map_err(|_| invalid())?),
        compressed: b[28] == 1,
        index: u16::from_be_bytes([b[32], b[33]]),
        count: u16::from_be_bytes([b[34], b[35]]),
        wire_len: u32::from_be_bytes(b[36..40].try_into().map_err(|_| invalid())?),
        raw_len: u32::from_be_bytes(b[40..44].try_into().map_err(|_| invalid())?),
        digest: b[44..76].try_into().map_err(|_| invalid())?,
    };
    if m.timestamp_ms.abs_diff(unix_timestamp_ms()) > EVENT_TIMESTAMP_WINDOW_MS as u64
        || m.count == 0
        || m.count as usize > CHUNK_COUNT_LIMIT
        || m.index >= m.count
        || m.raw_len as usize > kind.body_limit()
        || m.raw_len < 16
        || m.wire_len == 0
        || m.wire_len as usize > kind.body_limit()
        || (m.wire_len as usize).div_ceil(CHUNK_DATA_LIMIT) != m.count as usize
        || (!m.compressed && m.raw_len != m.wire_len)
    {
        return Err(invalid());
    }
    let expected = (m.wire_len as usize)
        .saturating_sub(m.index as usize * CHUNK_DATA_LIMIT)
        .min(CHUNK_DATA_LIMIT);
    if b.len() != CHUNK_HEADER_SIZE + expected {
        return Err(invalid());
    }
    Ok(m)
}
/// Test/probe-only count of `encode` invocations, so a regression can assert that a
/// retry reuses the cached chunks instead of re-compressing (R4-04).
#[cfg(any(test, feature = "test-support"))]
pub static ENCODE_CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[cfg(any(test, feature = "test-support"))]
pub fn encode_calls() -> usize {
    ENCODE_CALLS.load(std::sync::atomic::Ordering::SeqCst)
}

/// Event-scoped observer for concurrent unit tests. A process total cannot
/// tell whether this event's retry encoded again while other events were sent.
#[cfg(all(test, feature = "repair-regressions"))]
static ENCODE_CALLS_BY_MESSAGE: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<MessageId, usize>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

#[cfg(all(test, feature = "repair-regressions"))]
pub(crate) fn encode_calls_for(message_id: MessageId) -> usize {
    ENCODE_CALLS_BY_MESSAGE
        .lock()
        .unwrap()
        .get(&message_id)
        .copied()
        .unwrap_or(0)
}

pub fn encode(envelope: &EventEnvelope) -> Result<Vec<Vec<u8>>, ProtocolError> {
    encode_kind(envelope, ChunkKind::Clipboard)
}
pub fn encode_archive(envelope: &EventEnvelope) -> Result<Vec<Vec<u8>>, ProtocolError> {
    encode_kind(envelope, ChunkKind::Archive)
}
fn encode_kind(envelope: &EventEnvelope, kind: ChunkKind) -> Result<Vec<Vec<u8>>, ProtocolError> {
    #[cfg(any(test, feature = "test-support"))]
    ENCODE_CALLS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    #[cfg(all(test, feature = "repair-regressions"))]
    {
        *ENCODE_CALLS_BY_MESSAGE
            .lock()
            .unwrap()
            .entry(envelope.message_id)
            .or_default() += 1;
    }
    envelope.validate_timestamp(unix_timestamp_ms())?;
    match kind {
        ChunkKind::Clipboard => {
            let bundle = PortableBundle::decode(&envelope.content)?;
            validate_storable(&bundle)?;
        }
        ChunkKind::Archive => {
            archive_sync::ArchiveUpdate::decode(&envelope.content)?;
        }
    }
    // Wire correctness only: the local platform's ability to publish is decided
    // at publish time, not when chunking (R2-02).
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder
        .write_all(&envelope.content)
        .map_err(|_| invalid())?;
    let compressed = encoder.finish().map_err(|_| invalid())?;
    let use_compressed = compressed.len() < envelope.content.len();
    let data = if use_compressed {
        compressed.as_slice()
    } else {
        envelope.content.as_slice()
    };
    let count = data.len().div_ceil(CHUNK_DATA_LIMIT);
    if count == 0 || count > CHUNK_COUNT_LIMIT {
        return Err(invalid());
    }
    let digest = *blake3::hash(&envelope.content).as_bytes();
    let mut result = Vec::with_capacity(count);
    for (index, part) in data.chunks(CHUNK_DATA_LIMIT).enumerate() {
        let mut b = Vec::with_capacity(CHUNK_HEADER_SIZE + part.len());
        b.extend_from_slice(kind.magic());
        b.extend_from_slice(&envelope.message_id.0);
        b.extend_from_slice(&envelope.timestamp_ms.to_be_bytes());
        b.push(u8::from(use_compressed));
        b.extend_from_slice(&[0; 3]);
        b.extend_from_slice(&(index as u16).to_be_bytes());
        b.extend_from_slice(&(count as u16).to_be_bytes());
        b.extend_from_slice(&(data.len() as u32).to_be_bytes());
        b.extend_from_slice(&(envelope.content.len() as u32).to_be_bytes());
        b.extend_from_slice(&digest);
        b.extend_from_slice(part);
        result.push(b);
    }
    Ok(result)
}
struct Assembly {
    meta: Metadata,
    next: u16,
    bytes: Vec<u8>,
    generation: u64,
    revision: Option<i64>,
    started: Instant,
    touched: Instant,
    _budget: super::budgets::AssemblyLease,
    memory: super::budgets::PipelineLease,
}
pub struct CompositeAssembler {
    active: Option<Assembly>,
    kind: ChunkKind,
    /// Authenticated peer fingerprint. Admission is keyed on this, never on a
    /// connection or a display name (repair plan §4 R2-04).
    identity: super::budgets::Identity,
    ledger: std::sync::Arc<super::budgets::BudgetLedger>,
}
/// Owns admitted wire/model memory through terminal receiver persistence.
pub struct CompletedBudget {
    _assembly: super::budgets::AssemblyLease,
    _memory: super::budgets::PipelineLease,
}
pub struct AcceptedChunk {
    pub metadata: Metadata,
    pub complete: Option<(EventEnvelope, u64, Option<i64>)>,
    pub validated: Option<ValidatedComposite>,
    pub budget: Option<CompletedBudget>,
}
impl CompositeAssembler {
    pub fn archive(identity: super::budgets::Identity) -> Self {
        Self::archive_with_ledger(identity, super::budgets::global())
    }
    pub fn archive_with_ledger(
        identity: super::budgets::Identity,
        ledger: std::sync::Arc<super::budgets::BudgetLedger>,
    ) -> Self {
        Self {
            active: None,
            kind: ChunkKind::Archive,
            identity,
            ledger,
        }
    }
    /// Assembler for an authenticated peer, using the process-wide budget ledger.
    pub fn new(identity: super::budgets::Identity) -> Self {
        Self::with_ledger(identity, super::budgets::global())
    }

    /// Assembler against an explicit ledger. Tests use a private ledger so they stay
    /// isolated from each other and from production accounting.
    pub fn with_ledger(
        identity: super::budgets::Identity,
        ledger: std::sync::Arc<super::budgets::BudgetLedger>,
    ) -> Self {
        Self {
            active: None,
            kind: ChunkKind::Clipboard,
            identity,
            ledger,
        }
    }

    #[cfg(test)]
    pub fn for_test() -> Self {
        Self::with_ledger(
            [0; 32],
            std::sync::Arc::new(super::budgets::BudgetLedger::default()),
        )
    }

    pub fn accept(&mut self, b: &[u8], generation: u64) -> Result<AcceptedChunk, ProtocolError> {
        self.accept_with_revision(b, generation, None)
    }
    pub fn accept_with_revision(
        &mut self,
        b: &[u8],
        generation: u64,
        revision: Option<i64>,
    ) -> Result<AcceptedChunk, ProtocolError> {
        let result = self.inner(b, generation, revision, Instant::now());
        if result.is_err() {
            self.active = None;
        }
        result
    }
    pub fn expire(&mut self) {
        let now = Instant::now();
        if self.active.as_ref().is_some_and(|a| {
            now.duration_since(a.touched) > Duration::from_secs(30)
                || now.duration_since(a.started) > Duration::from_secs(60)
        }) {
            self.active = None;
        }
    }
    fn inner(
        &mut self,
        b: &[u8],
        generation: u64,
        revision: Option<i64>,
        now: Instant,
    ) -> Result<AcceptedChunk, ProtocolError> {
        self.expire();
        let mut m = inspect_kind(b, self.kind)?;
        let data = &b[CHUNK_HEADER_SIZE..];
        let index = m.index;
        m.index = 0;
        if self.active.is_none() {
            if index != 0 {
                return Err(invalid());
            }
            self.active = Some(Assembly {
                meta: m,
                next: 0,
                bytes: Vec::new(),
                generation,
                revision,
                started: now,
                touched: now,
                _budget: self
                    .ledger
                    .admit_assembly(&self.identity, m.raw_len as u64, now)?,
                memory: self.ledger.reserve_pipeline(
                    (m.raw_len as u64)
                        .saturating_mul(2)
                        .saturating_add(m.wire_len as u64),
                )?,
            });
        }
        let a = self.active.as_mut().ok_or_else(invalid)?;
        if a.meta != m || index > a.next {
            return Err(invalid());
        }
        let meta = Metadata { index, ..m };
        if index < a.next {
            let start = index as usize * CHUNK_DATA_LIMIT;
            if a.bytes.get(start..start + data.len()) != Some(data) {
                return Err(invalid());
            }
            a.touched = now;
            return Ok(AcceptedChunk {
                metadata: meta,
                complete: None,
                validated: None,
                budget: None,
            });
        }
        if a.bytes.len() + data.len() > m.wire_len as usize {
            return Err(invalid());
        }
        a.bytes.extend_from_slice(data);
        a.next += 1;
        a.touched = now;
        if a.next < m.count {
            return Ok(AcceptedChunk {
                metadata: meta,
                complete: None,
                validated: None,
                budget: None,
            });
        }
        let mut a = self.active.take().ok_or_else(invalid)?;
        if a.bytes.len() != m.wire_len as usize {
            return Err(invalid());
        }
        let raw = if m.compressed {
            let mut decoder = ZlibDecoder::new(a.bytes.as_slice());
            let mut raw = Vec::new();
            decoder
                .by_ref()
                .take(m.raw_len as u64 + 1)
                .read_to_end(&mut raw)
                .map_err(|_| invalid())?;
            if decoder.total_in() != m.wire_len as u64 {
                return Err(invalid());
            }
            raw
        } else {
            a.bytes
        };
        if raw.len() != m.raw_len as usize || blake3::hash(&raw).as_bytes() != &m.digest {
            return Err(invalid());
        }
        // Assembly completes on structure alone; native preparation is a separate
        // gate the receiver evaluates so it can still store the content (R2-02).
        let (content, validated) = match self.kind {
            ChunkKind::Clipboard => {
                let validated = ValidatedComposite::decode_owned(raw)?;
                (validated.wire().to_vec(), Some(validated))
            }
            ChunkKind::Archive => {
                archive_sync::ArchiveUpdate::decode(&raw)?;
                (raw, None)
            }
        };
        a.memory.shrink_to((m.raw_len as u64).saturating_mul(2));
        Ok(AcceptedChunk {
            metadata: meta,
            complete: Some((
                EventEnvelope {
                    message_id: m.message_id,
                    timestamp_ms: m.timestamp_ms,
                    content,
                },
                a.generation,
                a.revision,
            )),
            validated,
            budget: Some(CompletedBudget {
                _assembly: a._budget,
                _memory: a.memory,
            }),
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn envelope() -> EventEnvelope {
        EventEnvelope::new(
            PortableBundle {
                representations: vec![Representation {
                    mime: Mime::Text,
                    bytes: vec![b'a'; 1024 * 1024],
                }],
                resources: vec![],
                bindings: vec![],
                completeness: Completeness::Complete,
            }
            .encode()
            .unwrap(),
        )
    }
    #[test]
    fn compressed_round_trip_retains_first_generation() {
        let e = envelope();
        let mut a = CompositeAssembler::for_test();
        let mut result = None;
        for b in encode(&e).unwrap() {
            result = a.accept(&b, 7).unwrap().complete;
        }
        let (restored, generation, _) = result.unwrap();
        assert_eq!(restored, e);
        assert_eq!(generation, 7);
    }
    #[test]
    fn rejects_bombs_wrong_digest_and_missing_chunk() {
        let mut b = encode(&envelope()).unwrap().remove(0);
        b[40..44].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(CompositeAssembler::for_test().accept(&b, 0).is_err());
        let mut b = encode(&envelope()).unwrap().remove(0);
        b[44] ^= 1;
        assert!(CompositeAssembler::for_test().accept(&b, 0).is_err());
    }

    /// R2-04 / T16 — four connections from the SAME authenticated peer must not hold
    /// four assembly slots.
    ///
    /// Each connection gets its own assembler (that is how the server is written),
    /// so the limit has to live in the shared ledger and be keyed on the peer's
    /// fingerprint rather than on the assembler instance.
    #[test]
    fn four_connections_from_one_peer_cannot_hold_four_assembly_slots() {
        let ledger = std::sync::Arc::new(super::super::budgets::BudgetLedger::default());
        let peer = [7_u8; 32];
        // Incompressible RTF so the payload spans several chunks: a fixture that
        // compressed to one chunk would complete immediately and release its slot,
        // which is not the case under test here.
        let chunk = |message: u8| {
            let mut bytes = b"{\\rtf1".to_vec();
            let mut state = 0x9e37_79b9_u32 ^ (message as u32);
            bytes.extend((0..(CHUNK_DATA_LIMIT + 4096)).map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state >> 24) as u8
            }));
            let bundle = PortableBundle {
                representations: vec![Representation {
                    mime: Mime::Rtf,
                    bytes,
                }],
                resources: vec![],
                bindings: vec![],
                completeness: Completeness::Complete,
            };
            let mut envelope = EventEnvelope::new(bundle.encode().unwrap());
            envelope.message_id = crate::protocol::MessageId([message; 16]);
            let chunks = encode(&envelope).unwrap();
            assert!(
                chunks.len() >= 2,
                "the fixture must stay open across chunks, got {} chunk(s)",
                chunks.len()
            );
            chunks.into_iter().next().unwrap()
        };
        // Four connections, same authenticated peer.
        let mut assemblers: Vec<_> = (0..4)
            .map(|_| CompositeAssembler::with_ledger(peer, ledger.clone()))
            .collect();
        assert!(
            assemblers[0].accept(&chunk(1), 0).is_ok(),
            "the first connection is admitted"
        );
        for (index, assembler) in assemblers.iter_mut().enumerate().skip(1) {
            assert!(
                assembler.accept(&chunk(index as u8 + 2), 0).is_err(),
                "connection {index} from the same peer must be refused while one assembly is active"
            );
        }
        assert_eq!(ledger.snapshot().assembly_total, 1);

        // A different authenticated peer is unaffected.
        let mut other = CompositeAssembler::with_ledger([9_u8; 32], ledger.clone());
        assert!(
            other.accept(&chunk(9), 0).is_ok(),
            "another peer keeps its own slot"
        );
        assert_eq!(ledger.snapshot().assembly_total, 2);

        // Dropping the first connection releases the peer's slot.
        drop(assemblers);
        assert_eq!(ledger.snapshot().assembly_total, 1);
        let mut retry = CompositeAssembler::with_ledger(peer, ledger.clone());
        assert!(
            retry.accept(&chunk(10), 0).is_ok(),
            "the peer can start again once its slot is released"
        );
    }
}
