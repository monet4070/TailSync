//! Bounded, versioned composite clipboard bundle codec.
//!
//! A composite body is the content of a reliable [`crate::EventEnvelope`]
//! carried by [`crate::Command::CompositePayload`]. The body is a small,
//! versioned manifest followed by one raw byte area:
//!
//! ```text
//! ┌────────┬────────┬────────┬───────┬──────┬─────┬──────┬─────────────┬─────────────┐
//! │ Magic  │ Ver(2) │ Comp(1)│Flags(1)│Reps(2)│Res(2)│Bind(2)│ManifestLen(4)│ByteAreaLen(4)│
//! │ "TCB1" │  BE    │        │       │      │     │      │   BE        │   BE        │
//! └────────┴────────┴────────┴───────┴──────┴─────┴──────┴─────────────┴─────────────┘
//! │ manifest (representations, then resources, then bindings) │ byte area │
//! ```
//!
//! Manifest entry layout (fixed, 41 bytes) for a representation or a resource:
//!
//! ```text
//! │ mime(1) │ offset(4 BE) │ length(4 BE) │ digest(32) │
//! ```
//!
//! Binding entry layout:
//!
//! ```text
//! │ representation(2 BE) │ resource(2 BE) │ target_len(2 BE) │ target bytes (UTF-8) │
//! ```
//!
//! The codec is deliberately strict: it rejects integer overflow, out-of-range
//! or overlapping byte ranges, duplicate representation formats, duplicate
//! bindings, unknown formats, unknown completeness, reserved flags, non-image
//! resources, declared image decompression bombs, per-format over-limit bodies,
//! digest mismatches, manifest/body length mismatches, truncation, and trailing
//! bytes. Every limit in the plan's table is enforced independently and the
//! overall body cap must also hold, so a body can never be "valid per field but
//! oversized in total".
use std::sync::Arc;

use crate::{ProtocolError, EVENT_ENVELOPE_HEADER_SIZE, MAX_PAYLOAD_SIZE};

/// Magic prefix for a versioned composite bundle body.
pub const COMPOSITE_MAGIC: [u8; 4] = *b"TCB1";
/// Frozen wire version of the composite bundle body.
pub const COMPOSITE_BUNDLE_VERSION: u16 = 1;
/// Fixed composite header size in bytes.
pub const COMPOSITE_HEADER_SIZE: usize = 22;
/// Fixed manifest entry size for a representation or a resource.
pub const COMPOSITE_ENTRY_SIZE: usize = 41;
/// Fixed manifest header size for a resource binding.
pub const COMPOSITE_BINDING_HEADER_SIZE: usize = 6;
/// Per-entry digest length (BLAKE3-256).
pub const COMPOSITE_DIGEST_SIZE: usize = 32;

/// Whole plaintext event budget: envelope header plus composite body. This
/// matches the current maximum content frame so a composite event never needs a
/// larger frame than an image event.
pub const MAX_COMPOSITE_EVENT_SIZE: usize = MAX_PAYLOAD_SIZE;
/// Maximum composite body, leaving room for the reliable event envelope header.
pub const MAX_COMPOSITE_PAYLOAD_SIZE: usize = MAX_PAYLOAD_SIZE - EVENT_ENVELOPE_HEADER_SIZE;
/// Maximum serialized manifest size.
pub const MAX_COMPOSITE_MANIFEST_SIZE: usize = 64 * 1024;
/// Maximum number of standard representations.
pub const MAX_COMPOSITE_REPRESENTATIONS: usize = 16;
/// Maximum number of image resources.
pub const MAX_COMPOSITE_RESOURCES: usize = 20;
/// Maximum number of resource bindings.
pub const MAX_COMPOSITE_BINDINGS: usize = 64;
/// Maximum plain-text representation size.
pub const MAX_COMPOSITE_TEXT_BYTES: usize = 1024 * 1024;
/// Maximum HTML representation size.
pub const MAX_COMPOSITE_HTML_BYTES: usize = 2 * 1024 * 1024;
/// Maximum RTF representation size.
pub const MAX_COMPOSITE_RTF_BYTES: usize = 4 * 1024 * 1024;
/// Maximum encoded size of a single image.
pub const MAX_COMPOSITE_IMAGE_BYTES: usize = 32 * 1024 * 1024;
/// Maximum decoded RGBA size of a single image.
pub const MAX_COMPOSITE_IMAGE_RGBA_BYTES: usize = 32 * 1024 * 1024;
/// Maximum decoded RGBA size of all images in one bundle.
pub const MAX_COMPOSITE_TOTAL_RGBA_BYTES: usize = 32 * 1024 * 1024;
/// Maximum binding target size.
pub const MAX_COMPOSITE_BINDING_TARGET_BYTES: usize = 4096;
/// Logical chunk size for a chunked composite transfer.
pub const COMPOSITE_CHUNK_SIZE: usize = 512 * 1024;
/// Maximum number of chunks in one composite transfer.
pub const MAX_COMPOSITE_CHUNKS: usize = 64;
/// Maximum concurrent composite assemblies per authenticated peer.
pub const MAX_COMPOSITE_ASSEMBLIES_PER_PEER: usize = 1;
/// Maximum concurrent composite assemblies across all authenticated peers.
pub const MAX_COMPOSITE_ASSEMBLIES_GLOBAL: usize = 4;
/// Idle timeout for an incomplete composite assembly.
pub const COMPOSITE_ASSEMBLY_IDLE_TIMEOUT_MS: u64 = 30_000;
/// Total deadline for an incomplete composite assembly.
pub const COMPOSITE_ASSEMBLY_DEADLINE_MS: u64 = 60_000;
/// Maximum wire payload of one composite chunk frame. The 512-byte allowance
/// covers the chunk header frozen with the M4 assembler.
pub const MAX_COMPOSITE_CHUNK_PAYLOAD_SIZE: usize = COMPOSITE_CHUNK_SIZE + 512;

/// Standard, cross-platform representation format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum PortableMime {
    PlainTextUtf8 = 0x01,
    HtmlUtf8 = 0x02,
    Rtf = 0x03,
    Png = 0x04,
}

impl PortableMime {
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x01 => Some(Self::PlainTextUtf8),
            0x02 => Some(Self::HtmlUtf8),
            0x03 => Some(Self::Rtf),
            0x04 => Some(Self::Png),
            _ => None,
        }
    }

    /// Maximum encoded byte length for this representation format.
    pub const fn byte_limit(self) -> usize {
        match self {
            Self::PlainTextUtf8 => MAX_COMPOSITE_TEXT_BYTES,
            Self::HtmlUtf8 => MAX_COMPOSITE_HTML_BYTES,
            Self::Rtf => MAX_COMPOSITE_RTF_BYTES,
            Self::Png => MAX_COMPOSITE_IMAGE_BYTES,
        }
    }

    pub const fn is_image(self) -> bool {
        matches!(self, Self::Png)
    }
}

/// How complete a portable bundle is relative to the captured native snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Completeness {
    Complete = 0x00,
    ExternalReferences = 0x01,
    MissingFormats = 0x02,
}

impl Completeness {
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x00 => Some(Self::Complete),
            0x01 => Some(Self::ExternalReferences),
            0x02 => Some(Self::MissingFormats),
            _ => None,
        }
    }
}

/// One standard representation of the semantic clipboard content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Representation {
    pub mime: PortableMime,
    pub bytes: Arc<[u8]>,
}

impl Representation {
    pub fn new(mime: PortableMime, bytes: impl Into<Arc<[u8]>>) -> Self {
        Self {
            mime,
            bytes: bytes.into(),
        }
    }
}

/// One image resource referenced by a representation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resource {
    pub mime: PortableMime,
    pub bytes: Arc<[u8]>,
}

impl Resource {
    pub fn new(mime: PortableMime, bytes: impl Into<Arc<[u8]>>) -> Self {
        Self {
            mime,
            bytes: bytes.into(),
        }
    }
}

/// Association between a representation and an image resource it embeds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceBinding {
    pub representation: u16,
    pub resource: u16,
    pub target: String,
}

/// A complete portable clipboard content: representations, image resources, and
/// the bindings between them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortableBundle {
    pub version: u16,
    pub representations: Vec<Representation>,
    pub resources: Vec<Resource>,
    pub bindings: Vec<ResourceBinding>,
    pub completeness: Completeness,
}

impl PortableBundle {
    /// A version-current bundle with no resources or bindings.
    pub fn new(representations: Vec<Representation>, completeness: Completeness) -> Self {
        Self {
            version: COMPOSITE_BUNDLE_VERSION,
            representations,
            resources: Vec::new(),
            bindings: Vec::new(),
            completeness,
        }
    }

    pub fn with_resources(
        mut self,
        resources: Vec<Resource>,
        bindings: Vec<ResourceBinding>,
    ) -> Self {
        self.resources = resources;
        self.bindings = bindings;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RawEntry {
    mime: PortableMime,
    offset: usize,
    length: usize,
    digest: [u8; COMPOSITE_DIGEST_SIZE],
}

fn invalid_range() -> ProtocolError {
    ProtocolError::CompositeInvalidRange
}

fn resolve_entry(byte_area: &[u8], entry: RawEntry) -> Result<(usize, usize), ProtocolError> {
    let end = entry
        .offset
        .checked_add(entry.length)
        .ok_or_else(invalid_range)?;
    if end > byte_area.len() {
        return Err(invalid_range());
    }
    Ok((entry.offset, end))
}

fn append_entry(
    manifest: &mut Vec<u8>,
    byte_area: &mut Vec<u8>,
    mime: PortableMime,
    bytes: &[u8],
) -> Result<(), ProtocolError> {
    let offset = u32::try_from(byte_area.len()).map_err(|_| ProtocolError::CompositeTooLarge {
        actual: byte_area.len(),
        limit: MAX_COMPOSITE_PAYLOAD_SIZE,
    })?;
    let length = u32::try_from(bytes.len()).map_err(|_| ProtocolError::CompositeTooLarge {
        actual: bytes.len(),
        limit: MAX_COMPOSITE_PAYLOAD_SIZE,
    })?;
    byte_area.extend_from_slice(bytes);
    manifest.push(mime.as_u8());
    manifest.extend_from_slice(&offset.to_be_bytes());
    manifest.extend_from_slice(&length.to_be_bytes());
    manifest.extend_from_slice(blake3::hash(bytes).as_bytes());
    Ok(())
}

fn parse_entry(manifest: &[u8], cursor: &mut usize) -> Result<RawEntry, ProtocolError> {
    let slice = manifest
        .get(*cursor..*cursor + COMPOSITE_ENTRY_SIZE)
        .ok_or(ProtocolError::CompositeTruncated)?;
    *cursor += COMPOSITE_ENTRY_SIZE;
    let mime =
        PortableMime::from_u8(slice[0]).ok_or(ProtocolError::CompositeUnknownMime(slice[0]))?;
    let offset = u32::from_be_bytes(slice[1..5].try_into().expect("fixed slice")) as usize;
    let length = u32::from_be_bytes(slice[5..9].try_into().expect("fixed slice")) as usize;
    let mut digest = [0u8; COMPOSITE_DIGEST_SIZE];
    digest.copy_from_slice(&slice[9..COMPOSITE_ENTRY_SIZE]);
    Ok(RawEntry {
        mime,
        offset,
        length,
        digest,
    })
}

/// Verify a decoded image resource is a PNG whose declared dimensions cannot
/// exceed the decoded RGBA budget. This is the protocol-level defence against a
/// small payload that would expand into a huge bitmap.
fn png_rgba_len(bytes: &[u8]) -> Result<usize, ProtocolError> {
    const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if bytes.len() < 24 || bytes[..8] != PNG_SIGNATURE || bytes[12..16] != *b"IHDR" {
        return Err(ProtocolError::CompositeInvalidImage);
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().expect("fixed slice"));
    let height = u32::from_be_bytes(bytes[20..24].try_into().expect("fixed slice"));
    if width == 0 || height == 0 {
        return Err(ProtocolError::CompositeInvalidImage);
    }
    let pixels = (width as usize)
        .checked_mul(height as usize)
        .ok_or(ProtocolError::CompositeImageTooLarge)?;
    pixels
        .checked_mul(4)
        .ok_or(ProtocolError::CompositeImageTooLarge)
}

/// Enforce every per-field limit and structural rule that does not depend on
/// the wire offsets. Used by both encode and decode so a locally produced
/// bundle cannot be more permissive than a received one.
fn validate_bundle(bundle: &PortableBundle) -> Result<(), ProtocolError> {
    if bundle.version != COMPOSITE_BUNDLE_VERSION {
        return Err(ProtocolError::CompositeUnsupportedVersion(bundle.version));
    }
    if bundle.representations.is_empty() {
        return Err(ProtocolError::CompositeEmpty);
    }
    if bundle.representations.len() > MAX_COMPOSITE_REPRESENTATIONS {
        return Err(ProtocolError::CompositeTooManyEntries {
            kind: "representations",
            actual: bundle.representations.len(),
            limit: MAX_COMPOSITE_REPRESENTATIONS,
        });
    }
    if bundle.resources.len() > MAX_COMPOSITE_RESOURCES {
        return Err(ProtocolError::CompositeTooManyEntries {
            kind: "resources",
            actual: bundle.resources.len(),
            limit: MAX_COMPOSITE_RESOURCES,
        });
    }
    if bundle.bindings.len() > MAX_COMPOSITE_BINDINGS {
        return Err(ProtocolError::CompositeTooManyEntries {
            kind: "bindings",
            actual: bundle.bindings.len(),
            limit: MAX_COMPOSITE_BINDINGS,
        });
    }

    let mut seen_mimes = [false; 5];
    for representation in &bundle.representations {
        if seen_mimes[representation.mime.as_u8() as usize] {
            return Err(ProtocolError::CompositeDuplicateRepresentation(
                representation.mime,
            ));
        }
        seen_mimes[representation.mime.as_u8() as usize] = true;
        if representation.bytes.len() > representation.mime.byte_limit() {
            return Err(ProtocolError::CompositeRepresentationTooLarge {
                mime: representation.mime,
                actual: representation.bytes.len(),
                limit: representation.mime.byte_limit(),
            });
        }
    }

    let mut total_rgba = 0usize;
    for (index, entry) in bundle
        .representations
        .iter()
        .map(|representation| (representation.mime, representation.bytes.as_ref()))
        .chain(
            bundle
                .resources
                .iter()
                .map(|resource| (resource.mime, resource.bytes.as_ref())),
        )
        .enumerate()
    {
        let (mime, bytes) = entry;
        if mime.is_image() {
            if bytes.len() > MAX_COMPOSITE_IMAGE_BYTES {
                return Err(ProtocolError::CompositeImageTooLarge);
            }
            let rgba = png_rgba_len(bytes)?;
            if rgba > MAX_COMPOSITE_IMAGE_RGBA_BYTES {
                return Err(ProtocolError::CompositeImageTooLarge);
            }
            total_rgba = total_rgba
                .checked_add(rgba)
                .ok_or(ProtocolError::CompositeImageTooLarge)?;
            if total_rgba > MAX_COMPOSITE_TOTAL_RGBA_BYTES {
                return Err(ProtocolError::CompositeImageTooLarge);
            }
        } else if index >= bundle.representations.len() {
            // Only images are allowed in the resource list.
            return Err(ProtocolError::CompositeNonImageResource);
        }
    }

    for binding in &bundle.bindings {
        if usize::from(binding.representation) >= bundle.representations.len()
            || usize::from(binding.resource) >= bundle.resources.len()
        {
            return Err(ProtocolError::CompositeInvalidBinding);
        }
        if binding.target.is_empty() || binding.target.len() > MAX_COMPOSITE_BINDING_TARGET_BYTES {
            return Err(ProtocolError::CompositeInvalidBinding);
        }
    }
    for (index, binding) in bundle.bindings.iter().enumerate() {
        if bundle.bindings[..index].iter().any(|earlier| {
            earlier.representation == binding.representation
                && earlier.resource == binding.resource
                && earlier.target == binding.target
        }) {
            return Err(ProtocolError::CompositeDuplicateBinding);
        }
    }

    Ok(())
}

/// Encode a bundle into the versioned binary framing.
pub fn encode_bundle(bundle: &PortableBundle) -> Result<Vec<u8>, ProtocolError> {
    validate_bundle(bundle)?;

    let mut byte_area = Vec::new();
    let mut manifest = Vec::new();
    for representation in &bundle.representations {
        append_entry(
            &mut manifest,
            &mut byte_area,
            representation.mime,
            &representation.bytes,
        )?;
    }
    for resource in &bundle.resources {
        append_entry(
            &mut manifest,
            &mut byte_area,
            resource.mime,
            &resource.bytes,
        )?;
    }
    for binding in &bundle.bindings {
        let target = binding.target.as_bytes();
        let target_len =
            u16::try_from(target.len()).map_err(|_| ProtocolError::CompositeInvalidBinding)?;
        manifest.extend_from_slice(&binding.representation.to_be_bytes());
        manifest.extend_from_slice(&binding.resource.to_be_bytes());
        manifest.extend_from_slice(&target_len.to_be_bytes());
        manifest.extend_from_slice(target);
    }

    if manifest.len() > MAX_COMPOSITE_MANIFEST_SIZE {
        return Err(ProtocolError::CompositeManifestTooLarge);
    }
    let total = COMPOSITE_HEADER_SIZE
        .checked_add(manifest.len())
        .and_then(|value| value.checked_add(byte_area.len()))
        .ok_or(ProtocolError::CompositeTooLarge {
            actual: usize::MAX,
            limit: MAX_COMPOSITE_PAYLOAD_SIZE,
        })?;
    if total > MAX_COMPOSITE_PAYLOAD_SIZE {
        return Err(ProtocolError::CompositeTooLarge {
            actual: total,
            limit: MAX_COMPOSITE_PAYLOAD_SIZE,
        });
    }

    let mut encoded = Vec::with_capacity(total);
    encoded.extend_from_slice(&COMPOSITE_MAGIC);
    encoded.extend_from_slice(&COMPOSITE_BUNDLE_VERSION.to_be_bytes());
    encoded.push(bundle.completeness.as_u8());
    encoded.push(0);
    encoded.extend_from_slice(&(bundle.representations.len() as u16).to_be_bytes());
    encoded.extend_from_slice(&(bundle.resources.len() as u16).to_be_bytes());
    encoded.extend_from_slice(&(bundle.bindings.len() as u16).to_be_bytes());
    encoded.extend_from_slice(&(manifest.len() as u32).to_be_bytes());
    encoded.extend_from_slice(&(byte_area.len() as u32).to_be_bytes());
    encoded.extend_from_slice(&manifest);
    encoded.extend_from_slice(&byte_area);
    debug_assert_eq!(encoded.len(), total);
    Ok(encoded)
}

/// Decode and fully validate a composite bundle body.
pub fn decode_bundle(data: &[u8]) -> Result<PortableBundle, ProtocolError> {
    if data.len() < COMPOSITE_HEADER_SIZE {
        return Err(ProtocolError::CompositeTruncated);
    }
    if data[..4] != COMPOSITE_MAGIC {
        return Err(ProtocolError::CompositeInvalidMagic);
    }
    let version = u16::from_be_bytes(data[4..6].try_into().expect("fixed slice"));
    if version != COMPOSITE_BUNDLE_VERSION {
        return Err(ProtocolError::CompositeUnsupportedVersion(version));
    }
    let completeness = Completeness::from_u8(data[6])
        .ok_or(ProtocolError::CompositeUnknownCompleteness(data[6]))?;
    if data[7] != 0 {
        return Err(ProtocolError::CompositeReservedFlags);
    }
    let representation_count =
        u16::from_be_bytes(data[8..10].try_into().expect("fixed slice")) as usize;
    let resource_count = u16::from_be_bytes(data[10..12].try_into().expect("fixed slice")) as usize;
    let binding_count = u16::from_be_bytes(data[12..14].try_into().expect("fixed slice")) as usize;
    let manifest_len = u32::from_be_bytes(data[14..18].try_into().expect("fixed slice")) as usize;
    let byte_area_len = u32::from_be_bytes(data[18..22].try_into().expect("fixed slice")) as usize;

    if representation_count > MAX_COMPOSITE_REPRESENTATIONS {
        return Err(ProtocolError::CompositeTooManyEntries {
            kind: "representations",
            actual: representation_count,
            limit: MAX_COMPOSITE_REPRESENTATIONS,
        });
    }
    if resource_count > MAX_COMPOSITE_RESOURCES {
        return Err(ProtocolError::CompositeTooManyEntries {
            kind: "resources",
            actual: resource_count,
            limit: MAX_COMPOSITE_RESOURCES,
        });
    }
    if binding_count > MAX_COMPOSITE_BINDINGS {
        return Err(ProtocolError::CompositeTooManyEntries {
            kind: "bindings",
            actual: binding_count,
            limit: MAX_COMPOSITE_BINDINGS,
        });
    }
    if manifest_len > MAX_COMPOSITE_MANIFEST_SIZE {
        return Err(ProtocolError::CompositeManifestTooLarge);
    }

    let total = COMPOSITE_HEADER_SIZE
        .checked_add(manifest_len)
        .and_then(|value| value.checked_add(byte_area_len))
        .ok_or(ProtocolError::CompositeTooLarge {
            actual: usize::MAX,
            limit: MAX_COMPOSITE_PAYLOAD_SIZE,
        })?;
    if total > MAX_COMPOSITE_PAYLOAD_SIZE {
        return Err(ProtocolError::CompositeTooLarge {
            actual: total,
            limit: MAX_COMPOSITE_PAYLOAD_SIZE,
        });
    }
    if data.len() < total {
        return Err(ProtocolError::CompositeTruncated);
    }
    if data.len() > total {
        return Err(ProtocolError::CompositeTrailingBytes);
    }

    let manifest = &data[COMPOSITE_HEADER_SIZE..COMPOSITE_HEADER_SIZE + manifest_len];
    let byte_area = &data[COMPOSITE_HEADER_SIZE + manifest_len..total];

    let mut cursor = 0usize;
    let mut ranges: Vec<(usize, usize)> = Vec::with_capacity(representation_count + resource_count);

    let mut representations = Vec::with_capacity(representation_count);
    for _ in 0..representation_count {
        let entry = parse_entry(manifest, &mut cursor)?;
        let (start, end) = resolve_entry(byte_area, entry)?;
        if blake3::hash(&byte_area[start..end]).as_bytes() != &entry.digest {
            return Err(ProtocolError::CompositeDigestMismatch);
        }
        ranges.push((start, end));
        representations.push(Representation::new(
            entry.mime,
            Arc::<[u8]>::from(&byte_area[start..end]),
        ));
    }

    let mut resources = Vec::with_capacity(resource_count);
    for _ in 0..resource_count {
        let entry = parse_entry(manifest, &mut cursor)?;
        let (start, end) = resolve_entry(byte_area, entry)?;
        if blake3::hash(&byte_area[start..end]).as_bytes() != &entry.digest {
            return Err(ProtocolError::CompositeDigestMismatch);
        }
        ranges.push((start, end));
        resources.push(Resource::new(
            entry.mime,
            Arc::<[u8]>::from(&byte_area[start..end]),
        ));
    }

    let mut bindings = Vec::with_capacity(binding_count);
    for _ in 0..binding_count {
        let slice = manifest
            .get(cursor..cursor + COMPOSITE_BINDING_HEADER_SIZE)
            .ok_or(ProtocolError::CompositeTruncated)?;
        cursor += COMPOSITE_BINDING_HEADER_SIZE;
        let representation = u16::from_be_bytes(slice[0..2].try_into().expect("fixed slice"));
        let resource = u16::from_be_bytes(slice[2..4].try_into().expect("fixed slice"));
        let target_len = u16::from_be_bytes(slice[4..6].try_into().expect("fixed slice")) as usize;
        if target_len == 0 || target_len > MAX_COMPOSITE_BINDING_TARGET_BYTES {
            return Err(ProtocolError::CompositeInvalidBinding);
        }
        let target_bytes = manifest
            .get(cursor..cursor + target_len)
            .ok_or(ProtocolError::CompositeTruncated)?;
        cursor += target_len;
        let target = std::str::from_utf8(target_bytes)
            .map_err(|_| ProtocolError::CompositeInvalidBinding)?
            .to_string();
        bindings.push(ResourceBinding {
            representation,
            resource,
            target,
        });
    }

    if cursor != manifest.len() {
        return Err(ProtocolError::CompositeManifestMismatch);
    }

    ranges.sort_unstable();
    for window in ranges.windows(2) {
        if window[0].1 > window[1].0 {
            return Err(ProtocolError::CompositeOverlappingRanges);
        }
    }

    let bundle = PortableBundle {
        version,
        representations,
        resources,
        bindings,
        completeness,
    };
    validate_bundle(&bundle)?;
    Ok(bundle)
}

#[cfg(test)]
mod tests;
