use super::html::ImageSourceSpan;
use super::resource_archive::{self, ResourceReplacement};
use super::{Mime, PortableBundle};
use crate::protocol::ProtocolError;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use url::Url;

pub const SOURCE_TICKET_KIND: &str = "tailsync-source-ticket";
pub const SOURCE_PACKET_KIND: &str = "tailsync-source-packet";
pub const SOURCE_CONTEXT_VERSION: u8 = 1;
pub const SOURCE_NODE_LIMIT: usize = 8;
pub const SOURCE_BYTES_PER_RESOURCE: usize = 8 * 1024 * 1024;
pub const SOURCE_TOTAL_BYTES: usize = 16 * 1024 * 1024;
pub const SOURCE_PACKET_BYTES: usize = 24 * 1024 * 1024;
pub const SOURCE_GRANT_TTL_SECS: u64 = 300;
pub const SOURCE_TICKET_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceError {
    NotHttp,
    UserInfo,
    NoOrigin,
    AmbiguousOrigin,
    TooManyNodes,
    NodeLimit,
    PacketTooLarge,
    PacketMalformed,
    ResourceTooLarge,
    TotalBudget,
    HashMismatch,
    DuplicateNode,
    UnknownNode,
    OriginMismatch,
    Decode,
    Cancelled,
}

impl SourceError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotHttp => "source_not_http",
            Self::UserInfo => "source_user_info",
            Self::NoOrigin => "source_origin_unavailable",
            Self::AmbiguousOrigin => "source_origin_ambiguous",
            Self::TooManyNodes => "source_too_many_nodes",
            Self::NodeLimit => "source_node_limit",
            Self::PacketTooLarge => "source_packet_too_large",
            Self::PacketMalformed => "source_packet_malformed",
            Self::ResourceTooLarge => "source_resource_too_large",
            Self::TotalBudget => "source_total_budget",
            Self::HashMismatch => "source_hash_mismatch",
            Self::DuplicateNode => "source_duplicate_node",
            Self::UnknownNode => "source_unknown_node",
            Self::OriginMismatch => "source_origin_mismatch",
            Self::Decode => "source_decode",
            Self::Cancelled => "source_cancelled",
        }
    }
}

impl std::fmt::Display for SourceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for SourceError {}

pub fn src_sha256(bytes: &[u8]) -> String {
    hex::encode(ring::digest::digest(&ring::digest::SHA256, bytes).as_ref())
}

pub fn new_grant_id() -> String {
    let bytes: [u8; 32] = rand::random();
    hex::encode(bytes)
}

pub fn canonical_origin(value: &str) -> Result<String, SourceError> {
    let url = Url::parse(value).map_err(|_| SourceError::NotHttp)?;
    source_origin(&url)
}

pub fn source_origin(url: &Url) -> Result<String, SourceError> {
    let effective = if url.scheme() == "blob" {
        Url::parse(url.path()).map_err(|_| SourceError::NotHttp)?
    } else {
        url.clone()
    };
    match effective.scheme() {
        "http" | "https" => {}
        _ => return Err(SourceError::NotHttp),
    }
    if !effective.username().is_empty() || effective.password().is_some() {
        return Err(SourceError::UserInfo);
    }
    let host = effective.host_str().ok_or(SourceError::NoOrigin)?;
    let mut origin = format!("{}://{}", effective.scheme(), host.to_ascii_lowercase());
    if let Some(port) = effective.port() {
        origin.push_str(&format!(":{port}"));
    }
    Ok(origin)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceNode {
    pub index: u16,
    pub src_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceProof {
    pub origin: String,
    pub nodes: Vec<SourceNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceTicket {
    pub kind: String,
    pub version: u8,
    pub grant_id: String,
    pub origin: String,
    pub expires_at: u64,
    pub budget_bytes: u64,
    pub nodes: Vec<SourceNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcePacket {
    pub kind: String,
    pub version: u8,
    pub grant_id: String,
    pub origin: String,
    pub entries: Vec<SourcePacketEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcePacketEntry {
    pub index: u16,
    pub src_sha256: String,
    pub png_b64: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceCandidate {
    pub index: usize,
    pub span: ImageSourceSpan,
    pub url: String,
    pub src_sha256: String,
    pub origin: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedNode {
    pub index: usize,
    pub span: ImageSourceSpan,
    pub src_sha256: String,
    pub png: Vec<u8>,
}

fn html_index(bundle: &PortableBundle) -> Result<usize, ProtocolError> {
    bundle
        .representations
        .iter()
        .position(|representation| representation.mime == Mime::Html)
        .ok_or(ProtocolError::InvalidComposite)
}

pub fn source_candidates(bundle: &PortableBundle) -> Result<Vec<SourceCandidate>, ProtocolError> {
    let images = resource_archive::external_images(bundle)?;
    let index = html_index(bundle)?;
    let html = &bundle.representations[index].bytes;
    Ok(images
        .into_iter()
        .map(|image| {
            let bytes = &html[image.span.start..image.span.end];
            let origin = Url::parse(&image.url)
                .ok()
                .and_then(|url| source_origin(&url).ok());
            SourceCandidate {
                index: image.image_index,
                span: image.span,
                url: image.url,
                src_sha256: src_sha256(bytes),
                origin,
            }
        })
        .collect())
}

pub fn missing_source_candidates(
    original: &PortableBundle,
    layer: Option<&PortableBundle>,
) -> Result<Vec<SourceCandidate>, ProtocolError> {
    let mut candidates = source_candidates(original)?;
    let Some(layer) = layer else {
        return Ok(candidates);
    };
    let original_index = html_index(original)?;
    let layer_index = html_index(layer)?;
    let original_html = &original.representations[original_index].bytes;
    let layer_html = &layer.representations[layer_index].bytes;
    let original_spans = super::html::reference_inventory(original_html)?.images;
    let layer_spans = super::html::reference_inventory(layer_html)?.images;
    if original_spans.len() != layer_spans.len() {
        return Err(ProtocolError::InvalidComposite);
    }
    candidates.retain(|candidate| {
        let position = candidate.index;
        match (original_spans.get(position), layer_spans.get(position)) {
            (Some(source), Some(target)) => {
                original_html[source.start..source.end] == layer_html[target.start..target.end]
            }
            _ => false,
        }
    });
    Ok(candidates)
}

pub fn select_grant(
    candidates: &[SourceCandidate],
) -> Result<(String, Vec<SourceNode>), SourceError> {
    let with_origin = candidates
        .iter()
        .filter_map(|candidate| {
            candidate
                .origin
                .as_deref()
                .map(|origin| (origin, candidate))
        })
        .collect::<Vec<_>>();
    if with_origin.is_empty() {
        return Err(SourceError::NoOrigin);
    }
    let origins = with_origin
        .iter()
        .map(|(origin, _)| *origin)
        .collect::<BTreeSet<_>>();
    if origins.len() > 1 {
        return Err(SourceError::AmbiguousOrigin);
    }
    let origin = origins.into_iter().next().expect("non-empty").to_string();
    let mut nodes = with_origin
        .iter()
        .map(|(_, candidate)| SourceNode {
            index: candidate.index as u16,
            src_sha256: candidate.src_sha256.clone(),
        })
        .collect::<Vec<_>>();
    nodes.sort_by_key(|node| node.index);
    nodes.dedup_by_key(|node| node.index);
    if nodes.is_empty() || nodes.len() > SOURCE_NODE_LIMIT {
        return Err(SourceError::TooManyNodes);
    }
    Ok((origin, nodes))
}

pub fn ticket_json(
    grant_id: &str,
    origin: &str,
    expires_at: u64,
    budget_bytes: u64,
    nodes: &[SourceNode],
) -> Result<String, SourceError> {
    let ticket = SourceTicket {
        kind: SOURCE_TICKET_KIND.to_string(),
        version: SOURCE_CONTEXT_VERSION,
        grant_id: grant_id.to_string(),
        origin: origin.to_string(),
        expires_at,
        budget_bytes,
        nodes: nodes.to_vec(),
    };
    serde_json::to_string_pretty(&ticket).map_err(|_| SourceError::PacketMalformed)
}

pub fn parse_packet(bytes: &[u8], max_bytes: usize) -> Result<SourcePacket, SourceError> {
    parse_packet_decoded(bytes, max_bytes).map(|(packet, _)| packet)
}

pub fn parse_packet_decoded(
    bytes: &[u8],
    max_bytes: usize,
) -> Result<(SourcePacket, Vec<Vec<u8>>), SourceError> {
    if bytes.len() > max_bytes.min(SOURCE_PACKET_BYTES) {
        return Err(SourceError::PacketTooLarge);
    }
    let packet: SourcePacket =
        serde_json::from_slice(bytes).map_err(|_| SourceError::PacketMalformed)?;
    if packet.kind != SOURCE_PACKET_KIND || packet.version != SOURCE_CONTEXT_VERSION {
        return Err(SourceError::PacketMalformed);
    }
    if packet.grant_id.len() != 64
        || !packet
            .grant_id
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err(SourceError::PacketMalformed);
    }
    if canonical_origin(&packet.origin).ok().as_deref() != Some(packet.origin.as_str()) {
        return Err(SourceError::PacketMalformed);
    }
    if packet.entries.is_empty() || packet.entries.len() > SOURCE_NODE_LIMIT {
        return Err(SourceError::NodeLimit);
    }
    let mut seen = BTreeSet::new();
    let mut total = 0usize;
    let mut decoded = Vec::with_capacity(packet.entries.len());
    for entry in &packet.entries {
        if !seen.insert(entry.index) {
            return Err(SourceError::DuplicateNode);
        }
        if entry.src_sha256.len() != 64
            || !entry
                .src_sha256
                .chars()
                .all(|character| character.is_ascii_hexdigit())
        {
            return Err(SourceError::HashMismatch);
        }
        let png = STANDARD
            .decode(entry.png_b64.as_bytes())
            .map_err(|_| SourceError::Decode)?;
        if png.is_empty() || png.len() > SOURCE_BYTES_PER_RESOURCE {
            return Err(SourceError::ResourceTooLarge);
        }
        total = total
            .checked_add(png.len())
            .ok_or(SourceError::TotalBudget)?;
        if total > SOURCE_TOTAL_BYTES {
            return Err(SourceError::TotalBudget);
        }
        decoded.push(png);
    }
    Ok((packet, decoded))
}

pub fn resolve_nodes(
    base: &PortableBundle,
    packet: &SourcePacket,
    origin: &str,
    granted: &[SourceNode],
) -> Result<Vec<ResolvedNode>, SourceError> {
    let decoded = packet
        .entries
        .iter()
        .map(|entry| {
            STANDARD
                .decode(entry.png_b64.as_bytes())
                .map_err(|_| SourceError::Decode)
        })
        .collect::<Result<Vec<_>, _>>()?;
    resolve_decoded_nodes(base, packet, decoded, origin, granted)
}

pub fn resolve_decoded_nodes(
    base: &PortableBundle,
    packet: &SourcePacket,
    decoded: Vec<Vec<u8>>,
    origin: &str,
    granted: &[SourceNode],
) -> Result<Vec<ResolvedNode>, SourceError> {
    if packet.origin != origin {
        return Err(SourceError::OriginMismatch);
    }
    let index = html_index(base).map_err(|_| SourceError::Decode)?;
    let html = &base.representations[index].bytes;
    let spans = super::html::reference_inventory(html)
        .map_err(|_| SourceError::Decode)?
        .images;
    let mut granted_by_index: BTreeMap<u16, &SourceNode> =
        granted.iter().map(|node| (node.index, node)).collect();
    let mut resolved = Vec::with_capacity(packet.entries.len());
    if decoded.len() != packet.entries.len() || packet.entries.len() > SOURCE_NODE_LIMIT {
        return Err(SourceError::NodeLimit);
    }
    let mut total = 0usize;
    for (entry, png) in packet.entries.iter().zip(decoded) {
        let node = granted_by_index
            .remove(&entry.index)
            .ok_or(SourceError::UnknownNode)?;
        if node.src_sha256 != entry.src_sha256 {
            return Err(SourceError::HashMismatch);
        }
        let span = spans
            .get(entry.index as usize)
            .ok_or(SourceError::UnknownNode)?;
        let current = &html[span.start..span.end];
        if src_sha256(current) != entry.src_sha256 {
            return Err(SourceError::HashMismatch);
        }
        if canonical_origin(std::str::from_utf8(current).map_err(|_| SourceError::Decode)?)
            .ok()
            .as_deref()
            != Some(origin)
        {
            return Err(SourceError::OriginMismatch);
        }
        if png.is_empty() || png.len() > SOURCE_BYTES_PER_RESOURCE {
            return Err(SourceError::ResourceTooLarge);
        }
        total = total
            .checked_add(png.len())
            .ok_or(SourceError::TotalBudget)?;
        if total > SOURCE_TOTAL_BYTES {
            return Err(SourceError::TotalBudget);
        }
        if super::png_to_packed(&png).is_err() {
            return Err(SourceError::Decode);
        }
        resolved.push(ResolvedNode {
            index: entry.index as usize,
            span: *span,
            src_sha256: entry.src_sha256.clone(),
            png,
        });
    }
    resolved.sort_by_key(|node| node.index);
    Ok(resolved)
}

pub fn parse_ticket(bytes: &[u8], now: u64) -> Result<SourceTicket, SourceError> {
    if bytes.len() > SOURCE_TICKET_BYTES {
        return Err(SourceError::PacketTooLarge);
    }
    let ticket: SourceTicket =
        serde_json::from_slice(bytes).map_err(|_| SourceError::PacketMalformed)?;
    if ticket.kind != SOURCE_TICKET_KIND
        || ticket.version != SOURCE_CONTEXT_VERSION
        || ticket.grant_id.len() != 64
        || !ticket.grant_id.bytes().all(|byte| byte.is_ascii_hexdigit())
        || canonical_origin(&ticket.origin).ok().as_deref() != Some(ticket.origin.as_str())
        || ticket.expires_at <= now
        || ticket.expires_at > now.saturating_add(SOURCE_GRANT_TTL_SECS)
        || ticket.budget_bytes == 0
        || ticket.budget_bytes > SOURCE_TOTAL_BYTES as u64
        || ticket.nodes.is_empty()
        || ticket.nodes.len() > SOURCE_NODE_LIMIT
    {
        return Err(SourceError::PacketMalformed);
    }
    let mut seen = BTreeSet::new();
    if ticket.nodes.iter().any(|node| {
        !seen.insert(node.index)
            || node.src_sha256.len() != 64
            || !node.src_sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    }) {
        return Err(SourceError::PacketMalformed);
    }
    Ok(ticket)
}

pub fn derive_source_archive(
    base: &PortableBundle,
    resolved: &[ResolvedNode],
    origin: &str,
) -> Result<(PortableBundle, SourceProof, usize), SourceError> {
    if resolved.is_empty() {
        return Err(SourceError::Decode);
    }
    if resolved.len() > SOURCE_NODE_LIMIT {
        return Err(SourceError::TooManyNodes);
    }
    let replacements = resolved
        .iter()
        .map(|node| ResourceReplacement {
            span: node.span,
            png: node.png.clone(),
        })
        .collect::<Vec<_>>();
    let (derived, deduplicated) = resource_archive::apply_resource_bindings(base, &replacements)
        .map_err(|_| SourceError::Decode)?;
    let mut nodes = resolved
        .iter()
        .map(|node| SourceNode {
            index: node.index as u16,
            src_sha256: node.src_sha256.clone(),
        })
        .collect::<Vec<_>>();
    nodes.sort_by_key(|node| node.index);
    Ok((
        derived,
        SourceProof {
            origin: origin.to_string(),
            nodes,
        },
        deduplicated,
    ))
}

pub fn validate_source_derivation(
    base: &PortableBundle,
    derived: &PortableBundle,
    proof: &SourceProof,
) -> Result<(), ProtocolError> {
    let invalid = || ProtocolError::InvalidComposite;
    base.validate()?;
    derived.validate()?;
    if canonical_origin(&proof.origin).ok().as_deref() != Some(proof.origin.as_str())
        || proof.nodes.is_empty()
        || proof.nodes.len() > SOURCE_NODE_LIMIT
    {
        return Err(invalid());
    }
    let mut proof_index = BTreeSet::new();
    for node in &proof.nodes {
        if !proof_index.insert(node.index)
            || node.src_sha256.len() != 64
            || !node
                .src_sha256
                .chars()
                .all(|character| character.is_ascii_hexdigit())
        {
            return Err(invalid());
        }
    }
    if base.representations.len() != derived.representations.len()
        || base
            .resources
            .iter()
            .any(|resource| !derived.resources.contains(resource))
    {
        return Err(invalid());
    }
    let index = html_index(base)?;
    for (position, (original, archived)) in base
        .representations
        .iter()
        .zip(&derived.representations)
        .enumerate()
    {
        if original.mime != archived.mime || (position != index && original != archived) {
            return Err(invalid());
        }
    }
    let original_html = &base.representations[index].bytes;
    let archived_html = &derived.representations[index].bytes;
    let original_spans = super::html::reference_inventory(original_html)?.images;
    let archived_spans = super::html::reference_inventory(archived_html)?.images;
    if original_spans.len() != archived_spans.len() {
        return Err(invalid());
    }
    let mut used = BTreeSet::new();
    let mut replacements = Vec::new();
    let mut expected_bindings = Vec::new();
    for (position, (source, target)) in original_spans.iter().zip(&archived_spans).enumerate() {
        let old = &original_html[source.start..source.end];
        let new = &archived_html[target.start..target.end];
        if old == new {
            continue;
        }
        if base.bindings.iter().any(|binding| {
            binding.representation as usize == index && binding.offset as usize == source.start
        }) {
            return Err(invalid());
        }
        let value = std::str::from_utf8(old).map_err(|_| invalid())?;
        if value.contains('&') || value.contains('\\') {
            return Err(invalid());
        }
        let binding = derived
            .bindings
            .iter()
            .find(|binding| {
                binding.representation as usize == index
                    && binding.offset as usize == target.start
                    && binding.length as usize == target.end - target.start
            })
            .ok_or_else(invalid)?;
        if new != format!("tailsync-resource:{}", binding.resource).as_bytes() {
            return Err(invalid());
        }
        let covered = proof
            .nodes
            .iter()
            .any(|node| node.index as usize == position && node.src_sha256 == src_sha256(old));
        if covered {
            if canonical_origin(value).ok().as_deref() != Some(proof.origin.as_str()) {
                return Err(invalid());
            }
            used.insert(position);
        } else {
            resource_archive::validate_public_url(&Url::parse(value).map_err(|_| invalid())?)
                .map_err(|_| invalid())?;
        }
        expected_bindings.push(binding.clone());
        replacements.push((*source, new));
    }
    if used.len() != proof.nodes.len() {
        return Err(invalid());
    }
    let mut output = Vec::new();
    let mut cursor = 0;
    for (span, replacement) in &replacements {
        output.extend_from_slice(&original_html[cursor..span.start]);
        output.extend_from_slice(replacement);
        cursor = span.end;
    }
    output.extend_from_slice(&original_html[cursor..]);
    if output != *archived_html {
        return Err(invalid());
    }
    for binding in &base.bindings {
        let mut adjusted = binding.clone();
        if binding.representation as usize == index {
            let shift: i64 = replacements
                .iter()
                .filter(|(span, _)| span.end <= binding.offset as usize)
                .map(|(span, value)| value.len() as i64 - (span.end - span.start) as i64)
                .sum();
            adjusted.offset =
                u32::try_from(binding.offset as i64 + shift).map_err(|_| invalid())?;
        }
        expected_bindings.push(adjusted);
    }
    let key = |binding: &super::ResourceBinding| {
        (
            binding.representation,
            binding.offset,
            binding.length,
            binding.resource,
        )
    };
    expected_bindings.sort_by_key(key);
    let mut actual_bindings = derived.bindings.clone();
    actual_bindings.sort_by_key(key);
    if expected_bindings != actual_bindings
        || derived.resources.iter().any(|resource| {
            !base.resources.contains(resource)
                && !derived
                    .bindings
                    .iter()
                    .any(|binding| binding.resource == resource.id)
        })
    {
        return Err(invalid());
    }
    let html_bindings = derived
        .bindings
        .iter()
        .filter(|binding| binding.representation as usize == index)
        .cloned()
        .collect::<Vec<_>>();
    let closed = super::html::restore_reference_inventory_complete(archived_html, &html_bindings)?;
    let completeness = if base.completeness == super::Completeness::MissingFormats {
        super::Completeness::MissingFormats
    } else if closed {
        super::Completeness::Complete
    } else {
        super::Completeness::ExternalReferences
    };
    if derived.completeness != completeness {
        return Err(invalid());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composite::{Completeness, PortableBundle, Representation, Resource};

    fn png(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for _ in 0..(width * height) {
            bytes.extend_from_slice(&rgba);
        }
        crate::composite::packed_to_png(width, height, &bytes).unwrap()
    }

    fn bundle_with(html: &str, resources: Vec<Resource>) -> PortableBundle {
        PortableBundle {
            representations: vec![
                Representation {
                    mime: Mime::Text,
                    bytes: b"hello".to_vec(),
                },
                Representation {
                    mime: Mime::Html,
                    bytes: html.as_bytes().to_vec(),
                },
            ],
            resources,
            bindings: Vec::new(),
            completeness: Completeness::ExternalReferences,
        }
    }

    #[test]
    fn canonical_origin_accepts_http_blob_and_rejects_others() {
        assert_eq!(
            canonical_origin("https://Example.com:443/a?b#c").unwrap(),
            "https://example.com"
        );
        assert_eq!(
            canonical_origin("http://example.com:8080/x").unwrap(),
            "http://example.com:8080"
        );
        assert_eq!(
            canonical_origin("blob:https://cdn.example.com/uuid").unwrap(),
            "https://cdn.example.com"
        );
        assert_eq!(
            canonical_origin("file:///etc/passwd"),
            Err(SourceError::NotHttp)
        );
        assert_eq!(
            canonical_origin("data:image/png;base64,AAAA"),
            Err(SourceError::NotHttp)
        );
        assert_eq!(
            canonical_origin("https://user:pass@example.com/x"),
            Err(SourceError::UserInfo)
        );
    }

    #[test]
    fn grant_requires_a_single_derivable_origin() {
        let candidate = |index: usize, origin: Option<&str>| SourceCandidate {
            index,
            span: ImageSourceSpan { start: 0, end: 4 },
            url: String::new(),
            src_sha256: src_sha256(b"abcd"),
            origin: origin.map(str::to_string),
        };
        assert_eq!(
            select_grant(&[candidate(0, None)]),
            Err(SourceError::NoOrigin)
        );
        assert_eq!(
            select_grant(&[
                candidate(0, Some("https://a")),
                candidate(1, Some("https://b"))
            ]),
            Err(SourceError::AmbiguousOrigin)
        );
        let (origin, nodes) = select_grant(&[
            candidate(1, Some("https://a")),
            candidate(0, None),
            candidate(2, Some("https://a")),
        ])
        .unwrap();
        assert_eq!(origin, "https://a");
        assert_eq!(
            nodes.iter().map(|node| node.index).collect::<Vec<_>>(),
            vec![1, 2]
        );
    }

    #[test]
    fn packet_parse_rejects_unknown_fields_and_budget_overruns() {
        let good = format!(
            r#"{{"kind":"{SOURCE_PACKET_KIND}","version":1,"grant_id":"{}","origin":"https://a","entries":[{{"index":0,"src_sha256":"{}","png_b64":"{}"}}]}}"#,
            "ab".repeat(32),
            "cd".repeat(32),
            STANDARD.encode(png(1, 1, [1, 2, 3, 4]))
        );
        assert!(parse_packet(good.as_bytes(), SOURCE_PACKET_BYTES).is_ok());
        let unknown = good.replace(
            r#""origin":"https://a","#,
            r#""origin":"https://a","consent":true,"#,
        );
        assert_eq!(
            parse_packet(unknown.as_bytes(), SOURCE_PACKET_BYTES),
            Err(SourceError::PacketMalformed)
        );
        let duplicate = format!(
            r#"{{"kind":"{SOURCE_PACKET_KIND}","version":1,"grant_id":"{}","origin":"https://a","entries":[{{"index":0,"src_sha256":"{}","png_b64":"{}"}},{{"index":0,"src_sha256":"{}","png_b64":"{}"}}]}}"#,
            "ab".repeat(32),
            "cd".repeat(32),
            STANDARD.encode(png(1, 1, [1, 2, 3, 4])),
            "cd".repeat(32),
            STANDARD.encode(png(1, 1, [1, 2, 3, 4])),
        );
        assert_eq!(
            parse_packet(duplicate.as_bytes(), SOURCE_PACKET_BYTES),
            Err(SourceError::DuplicateNode)
        );
        assert_eq!(
            parse_packet(good.as_bytes(), 4),
            Err(SourceError::PacketTooLarge)
        );
    }

    #[test]
    fn source_derivation_binds_exact_spans_and_survives_read_proof() {
        let first = png(1, 1, [10, 20, 30, 255]);
        let html = r#"<img src="blob:https://example.com/one"><img src="/two.png">"#;
        let base = bundle_with(html, Vec::new());
        let candidates = source_candidates(&base).unwrap();
        assert_eq!(candidates.len(), 2);
        let (origin, nodes) = select_grant(&candidates).unwrap();
        assert_eq!(origin, "https://example.com");
        assert_eq!(nodes.len(), 1);
        let target = candidates
            .iter()
            .find(|candidate| candidate.origin.is_some())
            .unwrap();
        let packet = SourcePacket {
            kind: SOURCE_PACKET_KIND.to_string(),
            version: 1,
            grant_id: "ab".repeat(32),
            origin: origin.clone(),
            entries: vec![SourcePacketEntry {
                index: target.index as u16,
                src_sha256: target.src_sha256.clone(),
                png_b64: STANDARD.encode(&first),
            }],
        };
        let resolved = resolve_nodes(&base, &packet, &origin, &nodes).unwrap();
        let (derived, proof, _deduplicated) =
            derive_source_archive(&base, &resolved, &origin).unwrap();
        assert_eq!(proof.nodes.len(), 1);
        assert_eq!(derived.resources.len(), 1);
        validate_source_derivation(&base, &derived, &proof).unwrap();
        let mut wrong_origin = proof.clone();
        wrong_origin.origin = "https://other.example".into();
        assert!(validate_source_derivation(&base, &derived, &wrong_origin).is_err());
        assert!(derived.representations[1]
            .bytes
            .windows(b"tailsync-resource:".len())
            .any(|window| window == b"tailsync-resource:"));
        assert_eq!(
            derived.representations[0].bytes,
            base.representations[0].bytes
        );
    }

    #[test]
    fn re_pointed_or_unknown_entries_fail() {
        let first = png(1, 1, [10, 20, 30, 255]);
        let html = r#"<img src="https://example.com/one">"#;
        let base = bundle_with(html, Vec::new());
        let candidates = source_candidates(&base).unwrap();
        let (origin, nodes) = select_grant(&candidates).unwrap();
        let mut packet = SourcePacket {
            kind: SOURCE_PACKET_KIND.to_string(),
            version: 1,
            grant_id: "ab".repeat(32),
            origin: origin.clone(),
            entries: vec![SourcePacketEntry {
                index: 0,
                src_sha256: "00".repeat(32),
                png_b64: STANDARD.encode(&first),
            }],
        };
        assert_eq!(
            resolve_nodes(&base, &packet, &origin, &nodes),
            Err(SourceError::HashMismatch)
        );
        packet.entries[0].src_sha256 = nodes[0].src_sha256.clone();
        packet.entries[0].index = 9;
        assert_eq!(
            resolve_nodes(&base, &packet, &origin, &nodes),
            Err(SourceError::UnknownNode)
        );
        packet.entries[0].index = 0;
        packet.origin = "https://other".to_string();
        assert_eq!(
            resolve_nodes(&base, &packet, &origin, &nodes),
            Err(SourceError::OriginMismatch)
        );
    }

    #[test]
    fn apng_and_bad_png_are_refused() {
        let html = r#"<img src="https://example.com/one">"#;
        let base = bundle_with(html, Vec::new());
        let candidates = source_candidates(&base).unwrap();
        let (origin, nodes) = select_grant(&candidates).unwrap();
        let packet = SourcePacket {
            kind: SOURCE_PACKET_KIND.to_string(),
            version: 1,
            grant_id: "ab".repeat(32),
            origin: origin.clone(),
            entries: vec![SourcePacketEntry {
                index: 0,
                src_sha256: nodes[0].src_sha256.clone(),
                png_b64: STANDARD.encode(b"\x89PNG\r\n\x1a\nnot-a-real-png"),
            }],
        };
        assert_eq!(
            resolve_nodes(&base, &packet, &origin, &nodes),
            Err(SourceError::Decode)
        );
    }
}
