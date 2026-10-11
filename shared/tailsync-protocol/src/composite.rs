//! Portable, deterministic multi-representation clipboard codec. No OS format IDs,
//! filenames, clipboard revision, or source markers are transmitted here.
use crate::{MessageId, ProtocolError, EVENT_ENVELOPE_HEADER_SIZE, MAX_PAYLOAD_SIZE};
use std::collections::BTreeSet;

pub const BODY_LIMIT: usize = MAX_PAYLOAD_SIZE - EVENT_ENVELOPE_HEADER_SIZE;
pub const MANIFEST_LIMIT: usize = 64 * 1024;
pub const REPRESENTATION_LIMIT: usize = 16;
pub const RESOURCE_LIMIT: usize = 20;
pub const RGBA_LIMIT: usize = 32 * 1024 * 1024;
pub const CHUNK_DATA_LIMIT: usize = 512 * 1024;
pub const CHUNK_HEADER_SIZE: usize = 76;
pub const CHUNK_COUNT_LIMIT: usize = 64;

/// Budget metrics produced by a single validation pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompositeMetrics {
    /// Total decoded RGBA bytes across the PNG representation and every resource.
    pub rgba_bytes: usize,
    /// Encoded manifest size, including the header and every descriptor/binding.
    pub manifest_bytes: usize,
    /// Total encoded size (manifest + representation and resource bytes).
    pub wire_bytes: usize,
}

/// A composite payload that has been validated exactly once.
///
/// There is no public constructor other than [`ValidatedComposite::decode`] (for
/// untrusted wire bytes) and [`ValidatedComposite::from_bundle`] (for a locally
/// built bundle), so a caller cannot obtain an unvalidated instance and the
/// expensive checks — UTF-8 per representation, the cumulative PNG pixel budget,
/// binding ranges and overlap — run once instead of once per consumer.
///
/// It owns the canonical wire bytes, which is what makes the ACK digest cheap and
/// safe: [`ValidatedComposite::digest`] hashes those bytes directly rather than
/// re-running `encode()` (and copying up to 32 MiB) on an async worker. Because
/// `decode` only accepts the canonical encoding, `encode(decode(x)) == x`, so the
/// digest is bit-for-bit the same value the previous `bundle.digest()` produced.
#[derive(Debug, Clone)]
pub struct ValidatedComposite {
    wire: std::sync::Arc<[u8]>,
    bundle: std::sync::Arc<PortableBundle>,
    metrics: CompositeMetrics,
}

impl ValidatedComposite {
    /// Validate untrusted wire bytes once.
    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        Self::decode_owned(bytes.to_vec())
    }
    pub fn decode_owned(bytes: Vec<u8>) -> Result<Self, ProtocolError> {
        let (bundle, metrics) = PortableBundle::decode_with_metrics(&bytes)?;
        Ok(Self {
            wire: bytes.into(),
            bundle: std::sync::Arc::new(bundle),
            metrics,
        })
    }

    /// Validate a locally built bundle once, encoding it a single time.
    pub fn from_bundle(bundle: PortableBundle) -> Result<Self, ProtocolError> {
        let metrics = bundle.validate_with_metrics()?;
        let wire = bundle.encode_validated()?;
        Ok(Self {
            wire: wire.into(),
            bundle: std::sync::Arc::new(bundle),
            metrics,
        })
    }

    pub fn bundle(&self) -> &PortableBundle {
        &self.bundle
    }

    pub fn shared_bundle(&self) -> std::sync::Arc<PortableBundle> {
        self.bundle.clone()
    }
    pub fn wire(&self) -> &[u8] {
        &self.wire
    }

    pub fn metrics(&self) -> CompositeMetrics {
        self.metrics
    }

    /// Wire digest, identical to the previous `PortableBundle::digest()`.
    pub fn digest(&self) -> [u8; 32] {
        *blake3::hash(&self.wire).as_bytes()
    }

    pub fn into_bundle(self) -> PortableBundle {
        std::sync::Arc::try_unwrap(self.bundle).unwrap_or_else(|bundle| (*bundle).clone())
    }
}

/// Prefix of an HTML resource reference binding.
pub const RESOURCE_REFERENCE_PREFIX: &[u8] = b"tailsync-resource:";
/// Maximum decimal digits in a `u16` resource id.
pub const RESOURCE_REFERENCE_DIGITS: usize = 5;

/// Write `value` as canonical decimal into `out`, returning the digit count.
///
/// Replaces a per-binding `format!` so validation stays allocation-free in the
/// binding loop (repair plan §4 R2-01).
pub fn decimal_digits(mut value: u32, out: &mut [u8; RESOURCE_REFERENCE_DIGITS]) -> usize {
    let mut buffer = [0_u8; RESOURCE_REFERENCE_DIGITS];
    let mut length = 0;
    loop {
        buffer[length] = b'0' + (value % 10) as u8;
        length += 1;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    for index in 0..length {
        out[index] = buffer[length - 1 - index];
    }
    length
}

/// Whether `index` is a UTF-8 character boundary in `bytes`.
///
/// This is exactly the test `str::is_char_boundary` performs (a continuation byte
/// is `0b10xxxxxx`), without re-validating the whole buffer per binding. Callers
/// must already have established that `bytes` is valid UTF-8.
fn is_utf8_boundary(bytes: &[u8], index: usize) -> bool {
    if index == 0 {
        return true;
    }
    if index >= bytes.len() {
        return index == bytes.len();
    }
    bytes[index] & 0xC0 != 0x80
}

pub const ACK_SIZE: usize = 56;
const HEADER: usize = 16;
const DESCRIPTOR: usize = 44;
const BINDING: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Mime {
    Text = 1,
    Html = 2,
    Rtf = 3,
    Png = 4,
}
impl Mime {
    pub fn from_byte(v: u8) -> Result<Self, ProtocolError> {
        match v {
            1 => Ok(Self::Text),
            2 => Ok(Self::Html),
            3 => Ok(Self::Rtf),
            4 => Ok(Self::Png),
            _ => Err(invalid()),
        }
    }
    pub const fn limit(self) -> usize {
        match self {
            Self::Text => 1024 * 1024,
            Self::Html => 2 * 1024 * 1024,
            Self::Rtf => 4 * 1024 * 1024,
            Self::Png => BODY_LIMIT,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Completeness {
    Complete = 0,
    ExternalReferences = 1,
    MissingFormats = 2,
}
impl Completeness {
    fn from_byte(v: u8) -> Result<Self, ProtocolError> {
        match v {
            0 => Ok(Self::Complete),
            1 => Ok(Self::ExternalReferences),
            2 => Ok(Self::MissingFormats),
            _ => Err(invalid()),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Representation {
    pub mime: Mime,
    pub bytes: Vec<u8>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resource {
    pub id: u16,
    pub png: Vec<u8>,
}
/// Byte range of the source HTML image reference. Binding is accepted only
/// on UTF-8 boundaries and never inferred from an unrelated bitmap flavor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceBinding {
    pub representation: u16,
    pub resource: u16,
    pub offset: u32,
    pub length: u32,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortableBundle {
    pub representations: Vec<Representation>,
    pub resources: Vec<Resource>,
    pub bindings: Vec<ResourceBinding>,
    pub completeness: Completeness,
}
fn invalid() -> ProtocolError {
    ProtocolError::InvalidComposite
}
fn u16_at(b: &[u8], n: usize) -> Result<u16, ProtocolError> {
    Ok(u16::from_be_bytes(
        b.get(n..n + 2)
            .ok_or_else(invalid)?
            .try_into()
            .map_err(|_| invalid())?,
    ))
}
fn u32_at(b: &[u8], n: usize) -> Result<u32, ProtocolError> {
    Ok(u32::from_be_bytes(
        b.get(n..n + 4)
            .ok_or_else(invalid)?
            .try_into()
            .map_err(|_| invalid())?,
    ))
}

/// Early dimension admission, before a platform decoder allocates pixels.
/// The Core decoder must additionally validate the full PNG stream.
pub fn png_rgba_size(bytes: &[u8]) -> Result<usize, ProtocolError> {
    if bytes.len() < 33 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" || &bytes[8..16] != b"\0\0\0\rIHDR" {
        return Err(invalid());
    }
    let w = u32_at(bytes, 16)? as usize;
    let h = u32_at(bytes, 20)? as usize;
    if w == 0 || h == 0 {
        return Err(invalid());
    }
    w.checked_mul(h)
        .and_then(|n| n.checked_mul(4))
        .filter(|n| *n <= RGBA_LIMIT)
        .ok_or_else(invalid)
}
impl PortableBundle {
    pub fn representation(&self, mime: Mime) -> Option<&[u8]> {
        self.representations
            .iter()
            .find(|r| r.mime == mime)
            .map(|r| r.bytes.as_slice())
    }
    pub fn text(&self) -> Option<&str> {
        self.representation(Mime::Text)
            .and_then(|b| std::str::from_utf8(b).ok())
    }
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.validate_with_metrics().map(|_| ())
    }

    /// Validate once and return the budget metrics the same pass computed.
    ///
    /// [`ValidatedComposite`] is built on this so a consumer never re-scans the
    /// payload for the pixel budget, the manifest size or the binding count.
    pub fn validate_with_metrics(&self) -> Result<CompositeMetrics, ProtocolError> {
        if self.representations.is_empty()
            || self.representations.len() > REPRESENTATION_LIMIT
            || self.resources.len() > RESOURCE_LIMIT
        {
            return Err(invalid());
        }
        let mut mimes = BTreeSet::new();
        let mut ids = BTreeSet::new();
        let mut pixels = 0usize;
        for r in &self.representations {
            if r.bytes.is_empty() || r.bytes.len() > r.mime.limit() || !mimes.insert(r.mime) {
                return Err(invalid());
            }
            match r.mime {
                Mime::Text | Mime::Html => {
                    std::str::from_utf8(&r.bytes).map_err(|_| invalid())?;
                }
                Mime::Rtf => {
                    if !r.bytes.starts_with(b"{\\rtf") {
                        return Err(invalid());
                    }
                }
                Mime::Png => {
                    pixels = pixels
                        .checked_add(png_rgba_size(&r.bytes)?)
                        .ok_or_else(invalid)?;
                }
            }
        }
        for r in &self.resources {
            if r.id == 0 || !ids.insert(r.id) {
                return Err(invalid());
            }
            pixels = pixels
                .checked_add(png_rgba_size(&r.png)?)
                .ok_or_else(invalid)?;
        }
        if pixels > RGBA_LIMIT {
            return Err(invalid());
        }
        let mut ranges = BTreeSet::new();
        // Cost discipline (repair plan §4 R2-01): every check in this loop must be
        // O(1) in the size of the HTML representation. An earlier revision ran
        // `from_utf8` over the whole HTML once per binding and allocated a
        // `format!` per binding, making validation O(bindings × html_len) — with
        // ~5400 bindings that is tens of GB of scanning for a ~2 MiB payload.
        //
        // UTF-8 validity was already established for every Text/Html representation
        // in the loop above, so the boundary check below is the plain byte test that
        // `str::is_char_boundary` performs, and the resource reference is compared
        // against a stack-formatted decimal instead of an allocated string.
        for b in &self.bindings {
            let r = self
                .representations
                .get(b.representation as usize)
                .filter(|r| r.mime == Mime::Html)
                .ok_or_else(invalid)?;
            let start = b.offset as usize;
            let end = start
                .checked_add(b.length as usize)
                .filter(|n| *n <= r.bytes.len())
                .ok_or_else(invalid)?;
            let reference = &r.bytes[start..end];
            let mut expected = [0_u8; RESOURCE_REFERENCE_DIGITS];
            let expected_len = decimal_digits(b.resource as u32, &mut expected);
            let expected_reference_len = RESOURCE_REFERENCE_PREFIX.len() + expected_len;
            if b.length == 0
                || !ids.contains(&b.resource)
                || !is_utf8_boundary(&r.bytes, start)
                || !is_utf8_boundary(&r.bytes, end)
                || reference.len() != expected_reference_len
                || !reference.starts_with(RESOURCE_REFERENCE_PREFIX)
                || reference[RESOURCE_REFERENCE_PREFIX.len()..] != expected[..expected_len]
                || !ranges.insert((b.representation, b.offset, end))
            {
                return Err(invalid());
            }
        }
        let ordered: Vec<_> = ranges.into_iter().collect();
        if ordered
            .windows(2)
            .any(|w| w[0].0 == w[1].0 && w[0].2 > w[1].1 as usize)
        {
            return Err(invalid());
        }
        let manifest = HEADER
            .checked_add((self.representations.len() + self.resources.len()) * DESCRIPTOR)
            .and_then(|n| n.checked_add(self.bindings.len().checked_mul(BINDING)?))
            .filter(|n| *n <= MANIFEST_LIMIT)
            .ok_or_else(invalid)?;
        let size = self
            .representations
            .iter()
            .map(|r| r.bytes.len())
            .chain(self.resources.iter().map(|r| r.png.len()))
            .try_fold(manifest, |n, len| n.checked_add(len))
            .ok_or_else(invalid)?;
        if size > BODY_LIMIT {
            return Err(invalid());
        }
        Ok(CompositeMetrics {
            rgba_bytes: pixels,
            manifest_bytes: manifest,
            wire_bytes: size,
        })
    }
    pub fn encode(&self) -> Result<Vec<u8>, ProtocolError> {
        self.validate()?;
        self.encode_validated()
    }
    fn encode_validated(&self) -> Result<Vec<u8>, ProtocolError> {
        let manifest_len = HEADER
            + (self.representations.len() + self.resources.len()) * DESCRIPTOR
            + self.bindings.len() * BINDING;
        let mut manifest = Vec::with_capacity(manifest_len);
        let mut data = Vec::new();
        manifest.extend_from_slice(b"CBP1");
        manifest.push(1);
        manifest.push(self.completeness as u8);
        manifest.extend_from_slice(&[0, 0]);
        manifest.extend_from_slice(&(self.representations.len() as u16).to_be_bytes());
        manifest.extend_from_slice(&(self.resources.len() as u16).to_be_bytes());
        manifest.extend_from_slice(&(self.bindings.len() as u16).to_be_bytes());
        manifest.extend_from_slice(&[0, 0]);
        for (kind, id, mime, bytes) in self
            .representations
            .iter()
            .enumerate()
            .map(|(i, r)| (0u8, i as u16, r.mime, r.bytes.as_slice()))
            .chain(
                self.resources
                    .iter()
                    .map(|r| (1u8, r.id, Mime::Png, r.png.as_slice())),
            )
        {
            manifest.push(kind);
            manifest.push(mime as u8);
            manifest.extend_from_slice(&id.to_be_bytes());
            manifest.extend_from_slice(&(data.len() as u32).to_be_bytes());
            manifest.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
            manifest.extend_from_slice(blake3::hash(bytes).as_bytes());
            data.extend_from_slice(bytes);
        }
        for b in &self.bindings {
            manifest.extend_from_slice(&b.representation.to_be_bytes());
            manifest.extend_from_slice(&b.resource.to_be_bytes());
            manifest.extend_from_slice(&b.offset.to_be_bytes());
            manifest.extend_from_slice(&b.length.to_be_bytes());
        }
        manifest.extend(data);
        Ok(manifest)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        Self::decode_with_metrics(bytes).map(|(bundle, _)| bundle)
    }
    fn decode_with_metrics(bytes: &[u8]) -> Result<(Self, CompositeMetrics), ProtocolError> {
        if bytes.len() < HEADER
            || bytes.len() > BODY_LIMIT
            || &bytes[..5] != b"CBP1\x01"
            || bytes[6..8] != [0, 0]
            || bytes[14..16] != [0, 0]
        {
            return Err(invalid());
        }
        let reps = u16_at(bytes, 8)? as usize;
        let resources = u16_at(bytes, 10)? as usize;
        let bindings = u16_at(bytes, 12)? as usize;
        if reps == 0 || reps > REPRESENTATION_LIMIT || resources > RESOURCE_LIMIT {
            return Err(invalid());
        }
        let manifest_len = HEADER + (reps + resources) * DESCRIPTOR + bindings * BINDING;
        if manifest_len > MANIFEST_LIMIT || manifest_len > bytes.len() {
            return Err(invalid());
        }
        let data = &bytes[manifest_len..];
        let mut next = 0usize;
        let mut result = Self {
            representations: Vec::with_capacity(reps),
            resources: Vec::with_capacity(resources),
            bindings: Vec::with_capacity(bindings),
            completeness: Completeness::from_byte(bytes[5])?,
        };
        for i in 0..reps + resources {
            let d = &bytes[HEADER + i * DESCRIPTOR..HEADER + (i + 1) * DESCRIPTOR];
            let mime = Mime::from_byte(d[1])?;
            let id = u16_at(d, 2)?;
            let offset = u32_at(d, 4)? as usize;
            let len = u32_at(d, 8)? as usize;
            let end = offset
                .checked_add(len)
                .filter(|n| *n <= data.len())
                .ok_or_else(invalid)?;
            if offset != next
                || len == 0
                || len > mime.limit()
                || (i < reps && (d[0] != 0 || id != i as u16))
                || (i >= reps && (d[0] != 1 || mime != Mime::Png))
            {
                return Err(invalid());
            }
            let content = &data[offset..end];
            if blake3::hash(content).as_bytes() != &d[12..44] {
                return Err(invalid());
            }
            if i < reps {
                result.representations.push(Representation {
                    mime,
                    bytes: content.to_vec(),
                });
            } else {
                result.resources.push(Resource {
                    id,
                    png: content.to_vec(),
                });
            }
            next = end;
        }
        if next != data.len() {
            return Err(invalid());
        }
        for i in 0..bindings {
            let n = HEADER + (reps + resources) * DESCRIPTOR + i * BINDING;
            result.bindings.push(ResourceBinding {
                representation: u16_at(bytes, n)?,
                resource: u16_at(bytes, n + 2)?,
                offset: u32_at(bytes, n + 4)?,
                length: u32_at(bytes, n + 8)?,
            });
        }
        let metrics = result.validate_with_metrics()?;
        Ok((result, metrics))
    }
    pub fn digest(&self) -> Result<[u8; 32], ProtocolError> {
        Ok(*blake3::hash(&self.encode()?).as_bytes())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AckResult {
    Chunk = 0,
    Applied = 1,
    Superseded = 2,
    ApplyUnknown = 3,
    FailedPartial = 4,
    StoredIncomplete = 5,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompositeAck {
    pub message_id: MessageId,
    pub digest: [u8; 32],
    pub chunk_index: u16,
    pub result: AckResult,
}
impl CompositeAck {
    pub fn encode(self) -> Vec<u8> {
        let mut b = Vec::with_capacity(ACK_SIZE);
        b.extend_from_slice(b"CAK1");
        b.extend_from_slice(&self.message_id.0);
        b.extend_from_slice(&self.digest);
        b.extend_from_slice(&self.chunk_index.to_be_bytes());
        b.push(self.result as u8);
        b.push(0);
        b
    }
    pub fn decode(b: &[u8]) -> Result<Self, ProtocolError> {
        if b.len() != ACK_SIZE || &b[..4] != b"CAK1" || b[55] != 0 {
            return Err(invalid());
        }
        let result = match b[54] {
            0 => AckResult::Chunk,
            1 => AckResult::Applied,
            2 => AckResult::Superseded,
            3 => AckResult::ApplyUnknown,
            4 => AckResult::FailedPartial,
            5 => AckResult::StoredIncomplete,
            _ => return Err(invalid()),
        };
        Ok(Self {
            message_id: MessageId(b[4..20].try_into().map_err(|_| invalid())?),
            digest: b[20..52].try_into().map_err(|_| invalid())?,
            chunk_index: u16_at(b, 52)?,
            result,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bundle() -> PortableBundle {
        PortableBundle {
            representations: vec![
                Representation {
                    mime: Mime::Text,
                    bytes: "中文 🙂\n".as_bytes().to_vec(),
                },
                Representation {
                    mime: Mime::Html,
                    bytes: b"<b>hello</b>".to_vec(),
                },
            ],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::Complete,
        }
    }
    #[test]
    fn round_trip_preserves_whitespace_and_order() {
        let b = bundle();
        let wire = b.encode().unwrap();
        assert_eq!(PortableBundle::decode(&wire).unwrap(), b);
        assert_eq!(b.digest().unwrap(), *blake3::hash(&wire).as_bytes());
    }
    #[test]
    fn rejects_every_truncation_trailing_bytes_and_digest_tampering() {
        let mut bytes = bundle().encode().unwrap();
        for n in 0..bytes.len() {
            assert!(PortableBundle::decode(&bytes[..n]).is_err());
        }
        bytes.push(0);
        assert!(PortableBundle::decode(&bytes).is_err());
        bytes.pop();
        *bytes.last_mut().unwrap() ^= 1;
        assert!(PortableBundle::decode(&bytes).is_err());
    }
    #[test]
    fn rejects_overlap_duplicate_mime_and_unknown_binding() {
        let mut b = bundle();
        b.representations.push(b.representations[0].clone());
        assert!(b.encode().is_err());
        b.representations.pop();
        b.bindings.push(ResourceBinding {
            representation: 1,
            resource: 1,
            offset: 0,
            length: 1,
        });
        assert!(b.encode().is_err());
        let mut wire = bundle().encode().unwrap();
        wire[HEADER + DESCRIPTOR + 4..HEADER + DESCRIPTOR + 8].copy_from_slice(&0u32.to_be_bytes());
        assert!(PortableBundle::decode(&wire).is_err());
    }
    #[test]
    fn rejects_oversized_dimensions_before_pixel_allocation() {
        let mut b = vec![0; 33];
        b[..16].copy_from_slice(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR");
        b[16..20].copy_from_slice(&u32::MAX.to_be_bytes());
        b[20..24].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(png_rgba_size(&b).is_err());
    }
    #[test]
    fn acknowledgement_carries_result_and_identity() {
        let ack = CompositeAck {
            message_id: MessageId([7; 16]),
            digest: [9; 32],
            chunk_index: 4,
            result: AckResult::Superseded,
        };
        assert_eq!(CompositeAck::decode(&ack.encode()).unwrap(), ack);
    }

    /// R2-01 — `ValidatedComposite::digest` must equal the previous
    /// `PortableBundle::digest()` so the ACK contract is unchanged (K1: the wire
    /// digest keeps its semantics; only the *origin* fingerprint becomes a
    /// projection).
    #[test]
    fn validated_composite_digest_matches_the_bundle_digest() {
        let wire = bundle().encode().unwrap();
        let validated = ValidatedComposite::decode(&wire).unwrap();
        assert_eq!(
            validated.digest(),
            *blake3::hash(&wire).as_bytes(),
            "the digest is the hash of the canonical wire bytes"
        );
        let reencoded = validated.bundle().encode().unwrap();
        assert_eq!(
            reencoded, wire,
            "decode/encode is the identity on canonical input"
        );
        assert_eq!(
            validated.digest(),
            *blake3::hash(&reencoded).as_bytes(),
            "the new digest agrees with the previous bundle digest"
        );
    }

    /// R2-01 / T04 — validation cost must not grow with bindings × html_len.
    ///
    /// The verification measured the old loop at ~184 ms for 2 MiB HTML with 5383
    /// bindings versus ~0.1 ms with none, because `from_utf8` re-scanned the whole
    /// HTML once per binding. The primary assertion is therefore about *scaling*:
    /// with a fixed binding count, growing the HTML eight-fold must not grow the
    /// cost eight-fold. Pre-fix that ratio was ~8x; post-fix the only extra work is
    /// one more UTF-8 pass over the larger buffer. A generous absolute ceiling backs
    /// it up without making the gate a machine-speed contest.
    #[test]
    fn validation_cost_does_not_scale_with_the_binding_count() {
        const SMALL_HTML: usize = 256 * 1024;
        const LARGE_HTML: usize = 2 * 1024 * 1024;
        const BINDINGS: usize = 4000;

        let sample = |html_bytes: usize, bindings: usize| {
            let mut bytes = vec![b'a'; html_bytes];
            let mut list = Vec::with_capacity(bindings);
            // Distinct, non-overlapping offsets each holding a literal reference.
            let stride = html_bytes / (bindings + 1);
            for index in 0..bindings {
                let needle = b"tailsync-resource:1";
                let offset = index * stride;
                bytes[offset..offset + needle.len()].copy_from_slice(needle);
                list.push(ResourceBinding {
                    representation: 0,
                    resource: 1,
                    offset: offset as u32,
                    length: needle.len() as u32,
                });
            }
            PortableBundle {
                representations: vec![Representation {
                    mime: Mime::Html,
                    bytes,
                }],
                resources: vec![Resource {
                    id: 1,
                    png: one_pixel_png(),
                }],
                bindings: list,
                completeness: Completeness::Complete,
            }
        };

        let measure = |html_bytes: usize, bindings: usize| {
            let payload = sample(html_bytes, bindings);
            let _ = payload.validate(); // warm up
            let mut best = std::time::Duration::from_secs(3600);
            for _ in 0..3 {
                let started = std::time::Instant::now();
                payload.validate().expect("the sample must be valid");
                best = best.min(started.elapsed());
            }
            best
        };

        let none = measure(LARGE_HTML, 0);
        let small = measure(SMALL_HTML, BINDINGS);
        let large = measure(LARGE_HTML, BINDINGS);
        eprintln!(
            "validate: 2MiB/0 bindings {none:?}; {BINDINGS} bindings at 256KiB {small:?} \
             and 2MiB {large:?} (growth {:.1}x for 8x more HTML)",
            large.as_secs_f64() / small.as_secs_f64().max(f64::EPSILON)
        );

        // The complexity gate: 8x the HTML must not cost anything like 8x.
        let ceiling = small.mul_f64(4.0).max(std::time::Duration::from_millis(20));
        assert!(
            large < ceiling,
            "validation grew with the HTML size at a fixed binding count: {small:?} -> {large:?}, \
             which means the per-binding work is still proportional to the HTML length"
        );
        // Sanity ceiling: the pre-fix implementation needed seconds for this input.
        assert!(
            large < std::time::Duration::from_millis(300),
            "validating {BINDINGS} bindings over 2 MiB took {large:?}; the pre-fix magnitude was seconds"
        );
        // Zero bindings must stay cheap: one UTF-8 pass over the HTML.
        assert!(
            none < std::time::Duration::from_millis(50),
            "a binding-free 2 MiB payload took {none:?}"
        );

        // Gate R2 names this exact case: 1 MiB of HTML with 5383 bindings must still
        // be accepted, and cheaply. The pre-fix implementation needed ~96 ms here.
        let named = measure(1024 * 1024, 5383);
        eprintln!("validate: 1MiB/5383 bindings {named:?}");
        assert!(
            named < std::time::Duration::from_millis(300),
            "the plan's 1MiB/5383 binding sample took {named:?}"
        );
    }

    /// R2-01 — the binding count stays bounded by the manifest limit, and a payload
    /// that exceeds it is rejected by `decode` rather than accepted and scanned.
    #[test]
    fn binding_count_is_bounded_by_the_manifest_limit() {
        let max_bindings = (MANIFEST_LIMIT - HEADER - DESCRIPTOR) / BINDING;
        let mut oversized = bundle();
        oversized.representations[1].bytes = b"<b>hello</b>".to_vec();
        oversized.resources = vec![Resource {
            id: 1,
            png: one_pixel_png(),
        }];
        oversized.bindings = (0..=max_bindings)
            .map(|index| ResourceBinding {
                representation: 1,
                resource: 1,
                offset: index as u32,
                length: 1,
            })
            .collect();
        assert!(
            oversized.encode().is_err(),
            "more bindings than the manifest can address must be rejected"
        );
    }

    fn one_pixel_png() -> Vec<u8> {
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        let mut chunk = |kind: &[u8; 4], body: &[u8]| {
            bytes.extend_from_slice(&(body.len() as u32).to_be_bytes());
            bytes.extend_from_slice(kind);
            bytes.extend_from_slice(body);
            let mut crc_input = kind.to_vec();
            crc_input.extend_from_slice(body);
            bytes.extend_from_slice(&crc32(&crc_input).to_be_bytes());
        };
        chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0]);
        let mut raw = vec![0_u8];
        raw.extend_from_slice(&[1, 2, 3, 255]);
        chunk(b"IDAT", &deflate_stored(&raw));
        chunk(b"IEND", &[]);
        bytes
    }

    /// Minimal stored (uncompressed) deflate stream: no compression dependency here.
    fn deflate_stored(data: &[u8]) -> Vec<u8> {
        let mut out = vec![0x78, 0x01];
        let mut remaining = data;
        loop {
            let block = remaining.len().min(0xffff);
            let last = block == remaining.len();
            out.push(if last { 1 } else { 0 });
            out.extend_from_slice(&(block as u16).to_le_bytes());
            out.extend_from_slice(&(!(block as u16)).to_le_bytes());
            out.extend_from_slice(&remaining[..block]);
            remaining = &remaining[block..];
            if last {
                break;
            }
        }
        out.extend_from_slice(&adler32(data).to_be_bytes());
        out
    }

    fn adler32(data: &[u8]) -> u32 {
        let (mut a, mut b) = (1_u32, 0_u32);
        for byte in data {
            a = (a + *byte as u32) % 65521;
            b = (b + a) % 65521;
        }
        (b << 16) | a
    }

    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xffff_ffff_u32;
        for byte in data {
            crc ^= *byte as u32;
            for _ in 0..8 {
                let mask = (crc & 1).wrapping_neg();
                crc = (crc >> 1) ^ (0xedb8_8320 & mask);
            }
        }
        !crc
    }
}

#[cfg(test)]
mod fuzz_tests {
    use super::*;
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn arbitrary_frames_do_not_panic(bytes in proptest::collection::vec(any::<u8>(),0..8192)) {
            let _=PortableBundle::decode(&bytes);
            let _=CompositeAck::decode(&bytes);
            let _=crate::native_clipboard::NativeSnapshot::decode(&bytes);
            let _=crate::bitmap::decode_dib(&bytes);
        }
        #[test]
        fn text_roundtrip_preserves_exact_whitespace(text in "[a-zA-Z0-9 \n\t中🙂]{1,500}") {
            let bundle=PortableBundle{representations:vec![Representation{mime:Mime::Text,bytes:text.into_bytes()}],resources:vec![],bindings:vec![],completeness:Completeness::Complete};
            let bytes=bundle.encode().unwrap();
            prop_assert_eq!(PortableBundle::decode(&bytes).unwrap(),bundle);
            for size in 0..bytes.len(){prop_assert!(PortableBundle::decode(&bytes[..size]).is_err());}
        }
    }
    #[test]
    fn composite_commands_are_v5_only_and_registry_values_are_frozen() {
        for (command, code) in [
            (crate::Command::CompositePayload, 0x010e),
            (crate::Command::CompositeChunk, 0x010f),
            (crate::Command::CompositeAck, 0x0110),
        ] {
            assert_eq!(command as u16, code);
            assert_eq!(crate::Command::from_u16(code), Some(command));
            let frame = crate::Frame::try_new(command, 0, 1, vec![0; 16]).unwrap();
            assert!(
                crate::Frame::decode(&frame.encode_with_version(crate::LEGACY_VERSION)).is_err()
            );
        }
    }
}
