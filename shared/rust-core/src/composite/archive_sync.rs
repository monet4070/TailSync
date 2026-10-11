use super::html;
use super::network_image::{self, NetworkImageFormat, NetworkImageLimits};
use super::network_originals::{self, NetworkImageOriginal};
use super::resource_archive::{
    self, ResourceReplacement, ARCHIVE_BYTES_PER_RESOURCE, ARCHIVE_DERIVED_BYTES_PER_RESOURCE,
    ARCHIVE_PIXELS_PER_RESOURCE, ARCHIVE_RESOURCE_LIMIT, ARCHIVE_TOTAL_BYTES,
    ARCHIVE_TOTAL_DERIVED_BYTES, ARCHIVE_TOTAL_PIXELS,
};
use super::{validate_storable, Mime, PortableBundle};
use crate::protocol::{MessageId, ProtocolError};
use std::collections::BTreeSet;
use url::Url;

const MAGIC: [u8; 4] = *b"ARU1";
const HEADER_LEN: usize = 64;
const RECORD_HEADER_LEN: usize = 8;
const MAX_REVISION: u64 = 9_007_199_254_740_991;
const WIRE_LIMIT: usize =
    HEADER_LEN + ARCHIVE_RESOURCE_LIMIT * RECORD_HEADER_LEN + ARCHIVE_TOTAL_BYTES;
pub const MAX_WIRE_BYTES: usize = WIRE_LIMIT;

const FORMAT_PNG: u8 = 1;
const FORMAT_JPEG: u8 = 2;
const FORMAT_WEBP: u8 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveResourceUpdate {
    pub image_index: u16,
    pub format: NetworkImageFormat,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveUpdate {
    pub base_event: MessageId,
    pub base_digest: [u8; 32],
    pub revision: u64,
    pub images: Vec<ArchiveResourceUpdate>,
}

fn invalid() -> ProtocolError {
    ProtocolError::InvalidComposite
}

fn format_tag(format: NetworkImageFormat) -> u8 {
    match format {
        NetworkImageFormat::Png => FORMAT_PNG,
        NetworkImageFormat::Jpeg => FORMAT_JPEG,
        NetworkImageFormat::WebP => FORMAT_WEBP,
    }
}

fn format_from_tag(tag: u8) -> Result<NetworkImageFormat, ProtocolError> {
    match tag {
        FORMAT_PNG => Ok(NetworkImageFormat::Png),
        FORMAT_JPEG => Ok(NetworkImageFormat::Jpeg),
        FORMAT_WEBP => Ok(NetworkImageFormat::WebP),
        _ => Err(invalid()),
    }
}

fn u16_at(bytes: &[u8], offset: usize) -> Result<u16, ProtocolError> {
    let field = bytes.get(offset..offset + 2).ok_or_else(invalid)?;
    Ok(u16::from_be_bytes(field.try_into().map_err(|_| invalid())?))
}

fn u64_at(bytes: &[u8], offset: usize) -> Result<u64, ProtocolError> {
    let field = bytes.get(offset..offset + 8).ok_or_else(invalid)?;
    Ok(u64::from_be_bytes(field.try_into().map_err(|_| invalid())?))
}

fn checked_total(current: usize, addend: usize) -> Result<usize, ProtocolError> {
    current
        .checked_add(addend)
        .filter(|total| *total <= ARCHIVE_TOTAL_BYTES)
        .ok_or_else(invalid)
}

fn decode_limits(remaining_pixels: u64, remaining_derived: usize) -> NetworkImageLimits {
    let max_pixels = ARCHIVE_PIXELS_PER_RESOURCE.min(remaining_pixels);
    NetworkImageLimits {
        max_pixels,
        max_rgba_bytes: usize::try_from(max_pixels.saturating_mul(4))
            .unwrap_or(usize::MAX)
            .min(super::RGBA_LIMIT),
        max_derived_bytes: ARCHIVE_DERIVED_BYTES_PER_RESOURCE.min(remaining_derived),
    }
}

impl ArchiveUpdate {
    fn validate(&self) -> Result<(), ProtocolError> {
        if self.base_event == MessageId([0u8; 16])
            || !(2..=MAX_REVISION).contains(&self.revision)
            || self.images.is_empty()
            || self.images.len() > ARCHIVE_RESOURCE_LIMIT
        {
            return Err(invalid());
        }
        let mut seen = BTreeSet::new();
        let mut total = 0usize;
        for image in &self.images {
            if image.bytes.is_empty()
                || image.bytes.len() > ARCHIVE_BYTES_PER_RESOURCE
                || !seen.insert(image.image_index)
            {
                return Err(invalid());
            }
            total = checked_total(total, image.bytes.len())?;
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>, ProtocolError> {
        self.validate()?;
        let body_bytes = self
            .images
            .iter()
            .map(|image| image.bytes.len())
            .sum::<usize>();
        let mut out =
            Vec::with_capacity(HEADER_LEN + self.images.len() * RECORD_HEADER_LEN + body_bytes);
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&self.base_event.0);
        out.extend_from_slice(&self.base_digest);
        out.extend_from_slice(&self.revision.to_be_bytes());
        out.extend_from_slice(&(self.images.len() as u16).to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        for image in &self.images {
            out.extend_from_slice(&image.image_index.to_be_bytes());
            out.push(format_tag(image.format));
            out.push(0);
            out.extend_from_slice(&(image.bytes.len() as u32).to_be_bytes());
            out.extend_from_slice(&image.bytes);
        }
        Ok(out)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        if bytes.len() < HEADER_LEN || bytes.len() > WIRE_LIMIT || bytes[..4] != MAGIC {
            return Err(invalid());
        }
        let base_event = MessageId(bytes[4..20].try_into().map_err(|_| invalid())?);
        let base_digest: [u8; 32] = bytes[20..52].try_into().map_err(|_| invalid())?;
        let revision = u64_at(bytes, 52)?;
        let count = u16_at(bytes, 60)? as usize;
        if u16_at(bytes, 62)? != 0 || count == 0 || count > ARCHIVE_RESOURCE_LIMIT {
            return Err(invalid());
        }
        let mut images = Vec::with_capacity(count);
        let mut seen = BTreeSet::new();
        let mut total = 0usize;
        let mut offset = HEADER_LEN;
        for _ in 0..count {
            let header = bytes
                .get(offset..offset + RECORD_HEADER_LEN)
                .ok_or_else(invalid)?;
            let image_index = u16::from_be_bytes(header[0..2].try_into().map_err(|_| invalid())?);
            let format = format_from_tag(header[2])?;
            if header[3] != 0 || !seen.insert(image_index) {
                return Err(invalid());
            }
            let length =
                u32::from_be_bytes(header[4..8].try_into().map_err(|_| invalid())?) as usize;
            if length == 0 || length > ARCHIVE_BYTES_PER_RESOURCE {
                return Err(invalid());
            }
            offset = offset.checked_add(RECORD_HEADER_LEN).ok_or_else(invalid)?;
            let body = bytes.get(offset..offset + length).ok_or_else(invalid)?;
            offset = offset.checked_add(length).ok_or_else(invalid)?;
            total = checked_total(total, length)?;
            images.push(ArchiveResourceUpdate {
                image_index,
                format,
                bytes: body.to_vec(),
            });
        }
        if offset != bytes.len() {
            return Err(invalid());
        }
        let update = Self {
            base_event,
            base_digest,
            revision,
            images,
        };
        update.validate()?;
        Ok(update)
    }

    pub fn derive(
        &self,
        base: &PortableBundle,
    ) -> Result<(PortableBundle, Vec<NetworkImageOriginal>), ProtocolError> {
        self.validate()?;
        validate_storable(base)?;
        if base.digest()? != self.base_digest {
            return Err(invalid());
        }
        let external = resource_archive::external_images(base)?;
        let mut replacements: Vec<ResourceReplacement> = Vec::with_capacity(self.images.len());
        let mut remaining_pixels = ARCHIVE_TOTAL_PIXELS;
        let mut remaining_derived = ARCHIVE_TOTAL_DERIVED_BYTES;
        for image in &self.images {
            let source = external
                .iter()
                .find(|candidate| candidate.image_index == image.image_index as usize)
                .ok_or_else(invalid)?;
            let url = Url::parse(&source.url).map_err(|_| invalid())?;
            resource_archive::validate_public_url(&url).map_err(|_| invalid())?;
            let limits = decode_limits(remaining_pixels, remaining_derived);
            let derived = network_image::decode_static_png(&image.bytes, image.format, &limits)
                .map_err(|_| invalid())?;
            if derived.pixels > remaining_pixels || derived.png.len() > remaining_derived {
                return Err(invalid());
            }
            remaining_pixels -= derived.pixels;
            remaining_derived -= derived.png.len();
            replacements.push(ResourceReplacement {
                span: source.span,
                png: derived.png,
            });
        }
        let (derived_bundle, _deduplicated) =
            resource_archive::apply_resource_bindings(base, &replacements)
                .map_err(|_| invalid())?;
        let html_index = derived_bundle
            .representations
            .iter()
            .position(|representation| representation.mime == Mime::Html)
            .ok_or_else(invalid)?;
        let inventory =
            html::reference_inventory(&derived_bundle.representations[html_index].bytes)?;
        let mut originals = Vec::with_capacity(self.images.len());
        for image in &self.images {
            let span = *inventory
                .images
                .get(image.image_index as usize)
                .ok_or_else(invalid)?;
            let span_len = span.end.checked_sub(span.start).ok_or_else(invalid)?;
            let binding = derived_bundle
                .bindings
                .iter()
                .find(|binding| {
                    binding.representation as usize == html_index
                        && binding.offset as usize == span.start
                        && binding.length as usize == span_len
                })
                .ok_or_else(invalid)?;
            originals.push(NetworkImageOriginal {
                image_index: image.image_index,
                resource_id: binding.resource,
                format: image.format,
                bytes: image.bytes.clone(),
            });
        }
        network_originals::validate_originals(&derived_bundle, &originals)?;
        Ok((derived_bundle, originals))
    }
}

#[cfg(test)]
mod tests {
    use super::super::network_image::fixtures::{
        APNG_TWO_FRAME, JPEG_1X1, PNG_1X1, WEBP_ANIMATED, WEBP_LOSSLESS_1X1,
    };
    use super::super::network_image::{self, NetworkImageFormat, NetworkImageLimits};
    use super::super::{
        packed_to_png, Completeness, Mime, PortableBundle, Representation, Resource,
        ResourceBinding,
    };
    use super::{
        ArchiveResourceUpdate, ArchiveUpdate, ARCHIVE_BYTES_PER_RESOURCE, ARCHIVE_RESOURCE_LIMIT,
        ARCHIVE_TOTAL_DERIVED_BYTES, HEADER_LEN, MAX_REVISION, RECORD_HEADER_LEN,
    };
    use crate::protocol::{MessageId, ProtocolError};

    fn assert_refused<T: std::fmt::Debug>(result: Result<T, ProtocolError>) {
        assert!(
            matches!(result, Err(ProtocolError::InvalidComposite)),
            "expected InvalidComposite, got {result:?}"
        );
    }

    fn body(encoded: &str) -> Vec<u8> {
        super::super::network_image::fixtures::decode(encoded)
    }

    fn sample_update() -> ArchiveUpdate {
        ArchiveUpdate {
            base_event: MessageId([9u8; 16]),
            base_digest: [3u8; 32],
            revision: 2,
            images: vec![
                ArchiveResourceUpdate {
                    image_index: 0,
                    format: NetworkImageFormat::Png,
                    bytes: body(PNG_1X1),
                },
                ArchiveResourceUpdate {
                    image_index: 1,
                    format: NetworkImageFormat::Jpeg,
                    bytes: body(JPEG_1X1),
                },
                ArchiveResourceUpdate {
                    image_index: 2,
                    format: NetworkImageFormat::WebP,
                    bytes: body(WEBP_LOSSLESS_1X1),
                },
            ],
        }
    }

    fn record_offset(update: &ArchiveUpdate, index: usize) -> usize {
        update.images[..index]
            .iter()
            .fold(HEADER_LEN, |offset, image| {
                offset + RECORD_HEADER_LEN + image.bytes.len()
            })
    }

    #[test]
    fn round_trips_every_admitted_format_and_preserves_order() {
        let update = sample_update();
        let encoded = update.encode().unwrap();
        assert_eq!(
            encoded.len(),
            HEADER_LEN
                + 3 * RECORD_HEADER_LEN
                + update
                    .images
                    .iter()
                    .map(|image| image.bytes.len())
                    .sum::<usize>()
        );
        assert_eq!(ArchiveUpdate::decode(&encoded).unwrap(), update);
    }

    #[test]
    fn empty_images_are_refused() {
        let mut update = sample_update();
        update.images.clear();
        assert_refused(update.encode());
        let mut encoded = sample_update().encode().unwrap();
        encoded[60..62].copy_from_slice(&0u16.to_be_bytes());
        assert_refused(ArchiveUpdate::decode(&encoded));
    }

    #[test]
    fn count_budget_is_enforced() {
        let mut update = sample_update();
        update.images = (0..=ARCHIVE_RESOURCE_LIMIT as u16)
            .map(|image_index| ArchiveResourceUpdate {
                image_index,
                format: NetworkImageFormat::Png,
                bytes: body(PNG_1X1),
            })
            .collect();
        assert_refused(update.encode());
        let mut encoded = sample_update().encode().unwrap();
        encoded[60..62].copy_from_slice(&((ARCHIVE_RESOURCE_LIMIT + 1) as u16).to_be_bytes());
        assert_refused(ArchiveUpdate::decode(&encoded));
    }

    #[test]
    fn every_truncation_boundary_is_refused() {
        let encoded = sample_update().encode().unwrap();
        for cut in 0..encoded.len() {
            assert!(
                matches!(
                    ArchiveUpdate::decode(&encoded[..cut]),
                    Err(ProtocolError::InvalidComposite)
                ),
                "cut {cut} was accepted"
            );
        }
        assert!(ArchiveUpdate::decode(&encoded).is_ok());
    }

    #[test]
    fn short_header_and_unknown_magic_are_refused() {
        assert_refused(ArchiveUpdate::decode(&[]));
        assert_refused(ArchiveUpdate::decode(b"ARU"));
        let mut encoded = sample_update().encode().unwrap();
        encoded[0] = b'X';
        assert_refused(ArchiveUpdate::decode(&encoded));
    }

    #[test]
    fn nonzero_reserved_is_refused() {
        let mut encoded = sample_update().encode().unwrap();
        encoded[62] = 1;
        assert_refused(ArchiveUpdate::decode(&encoded));
        let update = sample_update();
        let mut encoded = update.encode().unwrap();
        encoded[record_offset(&update, 1) + 3] = 1;
        assert_refused(ArchiveUpdate::decode(&encoded));
    }

    #[test]
    fn unknown_format_tag_is_refused() {
        let update = sample_update();
        let mut encoded = update.encode().unwrap();
        encoded[record_offset(&update, 0) + 2] = 9;
        assert_refused(ArchiveUpdate::decode(&encoded));
    }

    #[test]
    fn duplicate_ordinals_are_refused() {
        let mut update = sample_update();
        update.images[1].image_index = update.images[0].image_index;
        assert_refused(update.encode());
        let update = sample_update();
        let mut encoded = update.encode().unwrap();
        let second = record_offset(&update, 1);
        encoded[second..second + 2].copy_from_slice(&update.images[0].image_index.to_be_bytes());
        assert_refused(ArchiveUpdate::decode(&encoded));
    }

    #[test]
    fn zero_and_oversized_lengths_are_refused() {
        let update = sample_update();
        let mut encoded = update.encode().unwrap();
        let first = record_offset(&update, 0);
        encoded[first + 4..first + 8].copy_from_slice(&0u32.to_be_bytes());
        assert_refused(ArchiveUpdate::decode(&encoded));
        let mut encoded = update.encode().unwrap();
        let first = record_offset(&update, 0);
        encoded[first + 4..first + 8]
            .copy_from_slice(&((ARCHIVE_BYTES_PER_RESOURCE + 1) as u32).to_be_bytes());
        assert_refused(ArchiveUpdate::decode(&encoded));
    }

    #[test]
    fn trailing_bytes_are_refused() {
        let mut encoded = sample_update().encode().unwrap();
        encoded.push(0);
        assert_refused(ArchiveUpdate::decode(&encoded));
    }

    #[test]
    fn revision_and_base_event_bounds_are_enforced() {
        for revision in [0u64, 1, MAX_REVISION + 1, u64::MAX] {
            let mut update = sample_update();
            update.revision = revision;
            assert_refused(update.encode());
        }
        let mut update = sample_update();
        update.base_event = MessageId([0u8; 16]);
        assert_refused(update.encode());
        let mut update = sample_update();
        update.revision = MAX_REVISION;
        assert!(update.encode().is_ok());
    }

    #[test]
    fn encode_refuses_over_budget_inputs() {
        let mut update = sample_update();
        update.images[0].bytes = vec![0u8; ARCHIVE_BYTES_PER_RESOURCE + 1];
        assert_refused(update.encode());
        let mut update = sample_update();
        update.images[0].bytes.clear();
        assert_refused(update.encode());
        let mut update = sample_update();
        update.images = (0..3)
            .map(|image_index| ArchiveResourceUpdate {
                image_index,
                format: NetworkImageFormat::Png,
                bytes: vec![0u8; 6 * 1024 * 1024],
            })
            .collect();
        assert_refused(update.encode());
    }

    fn external_base(sources: &[&str]) -> PortableBundle {
        let mut html = String::new();
        for source in sources {
            html.push_str("<img src=\"");
            html.push_str(source);
            html.push_str("\">");
        }
        PortableBundle {
            representations: vec![Representation {
                mime: Mime::Html,
                bytes: html.into_bytes(),
            }],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::ExternalReferences,
        }
    }

    fn update_for(
        base: &PortableBundle,
        images: Vec<(u16, NetworkImageFormat, Vec<u8>)>,
    ) -> ArchiveUpdate {
        ArchiveUpdate {
            base_event: MessageId([7u8; 16]),
            base_digest: base.digest().unwrap(),
            revision: 2,
            images: images
                .into_iter()
                .map(|(image_index, format, bytes)| ArchiveResourceUpdate {
                    image_index,
                    format,
                    bytes,
                })
                .collect(),
        }
    }

    fn incompressible(len: usize) -> Vec<u8> {
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        let mut out = Vec::with_capacity(len);
        while out.len() < len {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            out.extend_from_slice(&state.to_le_bytes());
        }
        out.truncate(len);
        out
    }

    fn large_png(width: u32, height: u32) -> Vec<u8> {
        packed_to_png(
            width,
            height,
            &incompressible((width as usize) * (height as usize) * 4),
        )
        .unwrap()
    }

    #[test]
    fn derive_archives_three_formats_and_preserves_original_bytes() {
        let base = external_base(&[
            "https://a.example/1.png",
            "https://b.example/2.jpg",
            "https://c.example/3.webp",
        ]);
        let bodies = [
            (NetworkImageFormat::Png, body(PNG_1X1)),
            (NetworkImageFormat::Jpeg, body(JPEG_1X1)),
            (NetworkImageFormat::WebP, body(WEBP_LOSSLESS_1X1)),
        ];
        let update = update_for(
            &base,
            bodies
                .iter()
                .enumerate()
                .map(|(index, (format, bytes))| (index as u16, *format, bytes.clone()))
                .collect(),
        );
        let (derived, originals) = update.derive(&base).unwrap();
        assert_eq!(derived.resources.len(), 3);
        assert_eq!(originals.len(), 3);
        for (index, (format, bytes)) in bodies.iter().enumerate() {
            assert_eq!(originals[index].image_index, index as u16);
            assert_eq!(originals[index].resource_id, (index + 1) as u16);
            assert_eq!(originals[index].format, *format);
            assert_eq!(&originals[index].bytes, bytes);
        }
        let html = derived.representation(Mime::Html).unwrap();
        let html = std::str::from_utf8(html).unwrap();
        assert!(html.contains("tailsync-resource:1"));
        assert!(html.contains("tailsync-resource:2"));
        assert!(html.contains("tailsync-resource:3"));
        assert!(!html.contains("https://a.example"));
    }

    #[test]
    fn derive_deduplicates_identical_pngs_and_keeps_both_originals() {
        let base = external_base(&["https://a.example/1.png", "https://a.example/2.png"]);
        let update = update_for(
            &base,
            vec![
                (0, NetworkImageFormat::Png, body(PNG_1X1)),
                (1, NetworkImageFormat::Png, body(PNG_1X1)),
            ],
        );
        let (derived, originals) = update.derive(&base).unwrap();
        assert_eq!(derived.resources.len(), 1);
        assert_eq!(originals.len(), 2);
        assert_eq!(originals[0].resource_id, originals[1].resource_id);
        assert_eq!(originals[0].bytes, originals[1].bytes);
    }

    #[test]
    fn derive_refuses_a_base_digest_mismatch() {
        let base = external_base(&["https://a.example/1.png"]);
        let mut update = update_for(&base, vec![(0, NetworkImageFormat::Png, body(PNG_1X1))]);
        update.base_digest = [0u8; 32];
        assert_refused(update.derive(&base));
    }

    #[test]
    fn derive_refuses_an_unknown_ordinal() {
        let base = external_base(&["https://a.example/1.png"]);
        let update = update_for(&base, vec![(4, NetworkImageFormat::Png, body(PNG_1X1))]);
        assert_refused(update.derive(&base));
    }

    #[test]
    fn derive_refuses_an_already_bound_source() {
        let html = b"<img src=\"tailsync-resource:1\">".to_vec();
        let span = super::super::html::reference_inventory(&html)
            .unwrap()
            .images[0];
        let base = PortableBundle {
            representations: vec![Representation {
                mime: Mime::Html,
                bytes: html,
            }],
            resources: vec![Resource {
                id: 1,
                png: body(PNG_1X1),
            }],
            bindings: vec![ResourceBinding {
                representation: 0,
                resource: 1,
                offset: span.start as u32,
                length: (span.end - span.start) as u32,
            }],
            completeness: Completeness::Complete,
        };
        base.validate().unwrap();
        let update = update_for(&base, vec![(0, NetworkImageFormat::Png, body(PNG_1X1))]);
        assert_refused(update.derive(&base));
    }

    #[test]
    fn derive_refuses_non_public_or_unsafe_sources() {
        for source in [
            "http://127.0.0.1/a.png",
            "https://user:pass@example.com/a.png",
            "blob:https://example.com/00000000-0000-0000-0000-000000000000",
            "file:///tmp/a.png",
            "data:image/png;base64,iVBORw0KGgo=",
        ] {
            let base = external_base(&[source]);
            let update = update_for(&base, vec![(0, NetworkImageFormat::Png, body(PNG_1X1))]);
            assert_refused(update.derive(&base));
        }
    }

    #[test]
    fn derive_refuses_animated_bodies() {
        for (format, bytes) in [
            (NetworkImageFormat::Png, body(APNG_TWO_FRAME)),
            (NetworkImageFormat::WebP, body(WEBP_ANIMATED)),
        ] {
            let base = external_base(&["https://a.example/1.png"]);
            let update = update_for(&base, vec![(0, format, bytes)]);
            assert_refused(update.derive(&base));
        }
    }

    #[test]
    fn derive_refuses_a_format_tag_mismatch() {
        let base = external_base(&["https://a.example/1.png"]);
        let update = update_for(&base, vec![(0, NetworkImageFormat::Jpeg, body(PNG_1X1))]);
        assert_refused(update.derive(&base));
    }

    #[test]
    fn derive_enforces_the_cumulative_pixel_budget() {
        let base = external_base(&[
            "https://a.example/1.png",
            "https://a.example/2.png",
            "https://a.example/3.png",
        ]);
        let png = packed_to_png(1900, 1600, &vec![0u8; 1900 * 1600 * 4]).unwrap();
        let update = update_for(
            &base,
            (0..3)
                .map(|index| (index, NetworkImageFormat::Png, png.clone()))
                .collect(),
        );
        assert_refused(update.derive(&base));
    }

    #[test]
    fn derive_enforces_the_cumulative_derived_png_budget() {
        let base = external_base(&["https://a.example/1.png", "https://a.example/2.png"]);
        let png = large_png(1800, 1800);
        let derived_len = network_image::decode_static_png(
            &png,
            NetworkImageFormat::Png,
            &NetworkImageLimits::default(),
        )
        .unwrap()
        .png
        .len();
        assert!(derived_len > ARCHIVE_TOTAL_DERIVED_BYTES / 2);
        let update = update_for(
            &base,
            vec![
                (0, NetworkImageFormat::Png, png.clone()),
                (1, NetworkImageFormat::Png, png),
            ],
        );
        assert_refused(update.derive(&base));
    }

    #[test]
    fn derive_refuses_a_base_without_html() {
        let base = PortableBundle {
            representations: vec![Representation {
                mime: Mime::Text,
                bytes: b"legacy text".to_vec(),
            }],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::Complete,
        };
        let update = update_for(&base, vec![(0, NetworkImageFormat::Png, body(PNG_1X1))]);
        assert_refused(update.derive(&base));
    }

    #[test]
    fn derive_accepts_a_round_tripped_update() {
        let base = external_base(&["https://a.example/1.png", "https://a.example/2.jpg"]);
        let update = update_for(
            &base,
            vec![
                (0, NetworkImageFormat::Png, body(PNG_1X1)),
                (1, NetworkImageFormat::Jpeg, body(JPEG_1X1)),
            ],
        );
        let decoded = ArchiveUpdate::decode(&update.encode().unwrap()).unwrap();
        assert_eq!(decoded, update);
        let (derived, originals) = decoded.derive(&base).unwrap();
        assert_eq!(derived.resources.len(), 2);
        assert_eq!(originals.len(), 2);
    }
}
