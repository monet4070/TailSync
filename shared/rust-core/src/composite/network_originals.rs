use super::network_image::{self, NetworkImageFormat, NetworkImageLimits};
use super::resource_archive::{
    ARCHIVE_BYTES_PER_RESOURCE, ARCHIVE_DERIVED_BYTES_PER_RESOURCE, ARCHIVE_PIXELS_PER_RESOURCE,
    ARCHIVE_RESOURCE_LIMIT, ARCHIVE_TOTAL_BYTES, ARCHIVE_TOTAL_PIXELS,
};
use super::{Mime, PortableBundle};
use crate::protocol::ProtocolError;

const MAGIC: [u8; 4] = *b"NIO1";
const HEADER_LEN: usize = 8;
const RECORD_HEADER_LEN: usize = 12;

const FORMAT_PNG: u8 = 1;
const FORMAT_JPEG: u8 = 2;
const FORMAT_WEBP: u8 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkImageOriginal {
    pub image_index: u16,
    pub resource_id: u16,
    pub format: NetworkImageFormat,
    pub bytes: Vec<u8>,
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

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, ProtocolError> {
    let field = bytes.get(offset..offset + 4).ok_or_else(invalid)?;
    Ok(u32::from_be_bytes(field.try_into().map_err(|_| invalid())?))
}

fn checked_total(current: usize, addend: usize) -> Result<usize, ProtocolError> {
    current
        .checked_add(addend)
        .filter(|total| *total <= ARCHIVE_TOTAL_BYTES)
        .ok_or_else(invalid)
}

pub fn encode_originals(originals: &[NetworkImageOriginal]) -> Result<Vec<u8>, ProtocolError> {
    if originals.len() > ARCHIVE_RESOURCE_LIMIT {
        return Err(invalid());
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut total = 0usize;
    for original in originals {
        if original.resource_id == 0
            || original.bytes.is_empty()
            || original.bytes.len() > ARCHIVE_BYTES_PER_RESOURCE
            || !seen.insert(original.image_index)
        {
            return Err(invalid());
        }
        total = checked_total(total, original.bytes.len())?;
    }
    let mut out = Vec::with_capacity(HEADER_LEN + originals.len() * RECORD_HEADER_LEN + total);
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&(originals.len() as u16).to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    for original in originals {
        out.extend_from_slice(&original.image_index.to_be_bytes());
        out.extend_from_slice(&original.resource_id.to_be_bytes());
        out.push(format_tag(original.format));
        out.extend_from_slice(&[0u8; 3]);
        out.extend_from_slice(&(original.bytes.len() as u32).to_be_bytes());
        out.extend_from_slice(&original.bytes);
    }
    Ok(out)
}

pub fn decode_originals(bytes: &[u8]) -> Result<Vec<NetworkImageOriginal>, ProtocolError> {
    if bytes.len() < HEADER_LEN || bytes[..4] != MAGIC {
        return Err(invalid());
    }
    let count = u16_at(bytes, 4)? as usize;
    if u16_at(bytes, 6)? != 0 || count > ARCHIVE_RESOURCE_LIMIT {
        return Err(invalid());
    }
    let mut originals = Vec::with_capacity(count);
    let mut seen = std::collections::BTreeSet::new();
    let mut total = 0usize;
    let mut offset = HEADER_LEN;
    for _ in 0..count {
        let header = bytes
            .get(offset..offset + RECORD_HEADER_LEN)
            .ok_or_else(invalid)?;
        let image_index = u16::from_be_bytes(header[0..2].try_into().map_err(|_| invalid())?);
        let resource_id = u16::from_be_bytes(header[2..4].try_into().map_err(|_| invalid())?);
        let format = format_from_tag(header[4])?;
        if header[5..8].iter().any(|byte| *byte != 0)
            || resource_id == 0
            || !seen.insert(image_index)
        {
            return Err(invalid());
        }
        let length = u32_at(header, 8)? as usize;
        if length == 0 || length > ARCHIVE_BYTES_PER_RESOURCE {
            return Err(invalid());
        }
        offset = offset.checked_add(RECORD_HEADER_LEN).ok_or_else(invalid)?;
        let body = bytes.get(offset..offset + length).ok_or_else(invalid)?;
        offset = offset.checked_add(length).ok_or_else(invalid)?;
        total = checked_total(total, length)?;
        originals.push(NetworkImageOriginal {
            image_index,
            resource_id,
            format,
            bytes: body.to_vec(),
        });
    }
    if offset != bytes.len() {
        return Err(invalid());
    }
    Ok(originals)
}

pub fn validate_originals(
    bundle: &PortableBundle,
    originals: &[NetworkImageOriginal],
) -> Result<(), ProtocolError> {
    bundle.validate()?;
    if originals.is_empty() {
        return Ok(());
    }
    if originals.len() > ARCHIVE_RESOURCE_LIMIT {
        return Err(invalid());
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut total_bytes = 0usize;
    for original in originals {
        if original.resource_id == 0
            || original.bytes.is_empty()
            || original.bytes.len() > ARCHIVE_BYTES_PER_RESOURCE
            || !seen.insert(original.image_index)
        {
            return Err(invalid());
        }
        total_bytes = checked_total(total_bytes, original.bytes.len())?;
    }
    let html_index = bundle
        .representations
        .iter()
        .position(|representation| representation.mime == Mime::Html)
        .ok_or_else(invalid)?;
    let html = &bundle.representations[html_index].bytes;
    let inventory = super::html::reference_inventory(html)?;
    let mut remaining_pixels = ARCHIVE_TOTAL_PIXELS;
    for original in originals {
        let span = *inventory
            .images
            .get(original.image_index as usize)
            .ok_or_else(invalid)?;
        let span_len = span.end.checked_sub(span.start).ok_or_else(invalid)?;
        let binding = bundle
            .bindings
            .iter()
            .find(|binding| {
                binding.representation as usize == html_index
                    && binding.offset as usize == span.start
                    && binding.length as usize == span_len
            })
            .ok_or_else(invalid)?;
        if binding.resource != original.resource_id {
            return Err(invalid());
        }
        let resource = bundle
            .resources
            .iter()
            .find(|resource| resource.id == original.resource_id)
            .ok_or_else(invalid)?;
        if network_image::detect_format(&original.bytes) != Some(original.format) {
            return Err(invalid());
        }
        let max_pixels = ARCHIVE_PIXELS_PER_RESOURCE.min(remaining_pixels);
        let limits = NetworkImageLimits {
            max_pixels,
            max_rgba_bytes: usize::try_from(max_pixels.saturating_mul(4))
                .unwrap_or(usize::MAX)
                .min(super::RGBA_LIMIT),
            max_derived_bytes: ARCHIVE_DERIVED_BYTES_PER_RESOURCE,
        };
        let derived = network_image::decode_static_png(&original.bytes, original.format, &limits)
            .map_err(|_| invalid())?;
        if derived.pixels > remaining_pixels || derived.png != resource.png {
            return Err(invalid());
        }
        remaining_pixels -= derived.pixels;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::packed_to_png;
    use super::*;
    use crate::composite::network_image::fixtures::*;
    use crate::protocol::composite::{Completeness, Representation, Resource, ResourceBinding};

    #[test]
    fn empty_originals_do_not_require_an_html_representation() {
        let bundle = PortableBundle {
            representations: vec![Representation {
                mime: Mime::Text,
                bytes: b"legacy text".to_vec(),
            }],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::Complete,
        };
        validate_originals(&bundle, &[]).unwrap();
    }

    fn assert_refused<T: std::fmt::Debug>(result: Result<T, ProtocolError>) {
        assert!(
            matches!(result, Err(ProtocolError::InvalidComposite)),
            "expected InvalidComposite, got {result:?}"
        );
    }

    fn archived_bundle(
        bodies: &[(NetworkImageFormat, Vec<u8>)],
    ) -> (PortableBundle, Vec<NetworkImageOriginal>) {
        let mut html = String::new();
        let mut resources = Vec::new();
        let mut originals = Vec::new();
        for (index, (format, body)) in bodies.iter().enumerate() {
            let id = (index + 1) as u16;
            html.push_str(&format!("<img src=\"tailsync-resource:{id}\">"));
            let derived =
                network_image::decode_static_png(body, *format, &NetworkImageLimits::default())
                    .unwrap();
            resources.push(Resource {
                id,
                png: derived.png,
            });
            originals.push(NetworkImageOriginal {
                image_index: index as u16,
                resource_id: id,
                format: *format,
                bytes: body.clone(),
            });
        }
        let inventory = super::super::html::reference_inventory(html.as_bytes()).unwrap();
        let bindings = inventory
            .images
            .iter()
            .zip(&originals)
            .map(|(span, original)| ResourceBinding {
                representation: 0,
                resource: original.resource_id,
                offset: span.start as u32,
                length: (span.end - span.start) as u32,
            })
            .collect();
        let bundle = PortableBundle {
            representations: vec![Representation {
                mime: Mime::Html,
                bytes: html.into_bytes(),
            }],
            resources,
            bindings,
            completeness: Completeness::Complete,
        };
        bundle.validate().unwrap();
        (bundle, originals)
    }

    fn sample_bodies() -> Vec<(NetworkImageFormat, Vec<u8>)> {
        vec![
            (NetworkImageFormat::Png, decode(PNG_1X1)),
            (NetworkImageFormat::Jpeg, decode(JPEG_1X1)),
            (NetworkImageFormat::WebP, decode(WEBP_LOSSLESS_1X1)),
        ]
    }

    fn record_offset(originals: &[NetworkImageOriginal], index: usize) -> usize {
        originals[..index]
            .iter()
            .fold(HEADER_LEN, |offset, original| {
                offset + RECORD_HEADER_LEN + original.bytes.len()
            })
    }

    #[test]
    fn round_trips_every_admitted_format() {
        let (_, originals) = archived_bundle(&sample_bodies());
        let encoded = encode_originals(&originals).unwrap();
        assert_eq!(decode_originals(&encoded).unwrap(), originals);
    }

    #[test]
    fn empty_list_is_legal() {
        let encoded = encode_originals(&[]).unwrap();
        assert_eq!(encoded.len(), HEADER_LEN);
        assert!(decode_originals(&encoded).unwrap().is_empty());
    }

    #[test]
    fn every_truncation_boundary_is_refused() {
        let (_, originals) = archived_bundle(&sample_bodies());
        let encoded = encode_originals(&originals).unwrap();
        for cut in 0..encoded.len() {
            assert!(
                matches!(
                    decode_originals(&encoded[..cut]),
                    Err(ProtocolError::InvalidComposite)
                ),
                "cut {cut} was accepted"
            );
        }
        assert!(decode_originals(&encoded).is_ok());
    }

    #[test]
    fn short_header_and_unknown_magic_are_refused() {
        assert_refused(decode_originals(&[]));
        assert_refused(decode_originals(b"NIO"));
        let mut encoded = encode_originals(&[]).unwrap();
        encoded[0] = b'X';
        assert_refused(decode_originals(&encoded));
    }

    #[test]
    fn count_budget_is_enforced() {
        let mut encoded = encode_originals(&[]).unwrap();
        encoded[4..6].copy_from_slice(&((ARCHIVE_RESOURCE_LIMIT + 1) as u16).to_be_bytes());
        assert_refused(decode_originals(&encoded));
    }

    #[test]
    fn nonzero_reserved_is_refused() {
        let (_, originals) = archived_bundle(&sample_bodies());
        let encoded = encode_originals(&originals).unwrap();
        let mut header = encoded.clone();
        header[6] = 1;
        assert_refused(decode_originals(&header));
        let mut record = encoded.clone();
        record[record_offset(&originals, 0) + 5] = 1;
        assert_refused(decode_originals(&record));
    }

    #[test]
    fn unknown_format_tag_is_refused() {
        let (_, originals) = archived_bundle(&sample_bodies());
        let mut encoded = encode_originals(&originals).unwrap();
        encoded[record_offset(&originals, 0) + 4] = 9;
        assert_refused(decode_originals(&encoded));
    }

    #[test]
    fn resource_zero_and_duplicate_ordinals_are_refused() {
        let (_, originals) = archived_bundle(&sample_bodies());
        let encoded = encode_originals(&originals).unwrap();
        let mut zero = encoded.clone();
        let first = record_offset(&originals, 0);
        zero[first + 2..first + 4].copy_from_slice(&0u16.to_be_bytes());
        assert_refused(decode_originals(&zero));
        let mut duplicate = encoded.clone();
        let second = record_offset(&originals, 1);
        duplicate[second..second + 2].copy_from_slice(&0u16.to_be_bytes());
        assert_refused(decode_originals(&duplicate));
    }

    #[test]
    fn zero_and_oversized_lengths_are_refused() {
        let (_, originals) = archived_bundle(&sample_bodies());
        let encoded = encode_originals(&originals).unwrap();
        let mut zero = encoded.clone();
        let first = record_offset(&originals, 0);
        zero[first + 8..first + 12].copy_from_slice(&0u32.to_be_bytes());
        assert_refused(decode_originals(&zero));
        let mut oversized = encoded.clone();
        oversized[first + 8..first + 12]
            .copy_from_slice(&((ARCHIVE_BYTES_PER_RESOURCE as u32) + 1).to_be_bytes());
        assert_refused(decode_originals(&oversized));
    }

    #[test]
    fn trailing_bytes_are_refused() {
        let (_, originals) = archived_bundle(&sample_bodies());
        let mut encoded = encode_originals(&originals).unwrap();
        encoded.push(0);
        assert_refused(decode_originals(&encoded));
    }

    #[test]
    fn encode_refuses_over_budget_inputs() {
        let (_, originals) = archived_bundle(&sample_bodies());
        let too_many = vec![
            NetworkImageOriginal {
                image_index: 0,
                resource_id: 1,
                format: NetworkImageFormat::Png,
                bytes: vec![0u8],
            };
            ARCHIVE_RESOURCE_LIMIT + 1
        ];
        assert_refused(encode_originals(&too_many));
        let mut duplicate = originals.clone();
        duplicate[1].image_index = duplicate[0].image_index;
        assert_refused(encode_originals(&duplicate));
        let mut zero = originals.clone();
        zero[0].resource_id = 0;
        assert_refused(encode_originals(&zero));
        let mut empty = originals.clone();
        empty[0].bytes.clear();
        assert_refused(encode_originals(&empty));
        let mut oversized_single = originals.clone();
        oversized_single[0].bytes = vec![0u8; ARCHIVE_BYTES_PER_RESOURCE + 1];
        assert_refused(encode_originals(&oversized_single));
        let total_overflow = vec![
            NetworkImageOriginal {
                image_index: 0,
                resource_id: 1,
                format: NetworkImageFormat::Png,
                bytes: vec![0u8; 6 * 1024 * 1024],
            },
            NetworkImageOriginal {
                image_index: 1,
                resource_id: 2,
                format: NetworkImageFormat::Png,
                bytes: vec![0u8; 6 * 1024 * 1024],
            },
            NetworkImageOriginal {
                image_index: 2,
                resource_id: 3,
                format: NetworkImageFormat::Png,
                bytes: vec![0u8; 6 * 1024 * 1024],
            },
        ];
        assert_refused(encode_originals(&total_overflow));
    }

    #[test]
    fn codec_preserves_the_raw_body_not_the_derived_png() {
        let (bundle, originals) = archived_bundle(&sample_bodies());
        let decoded = decode_originals(&encode_originals(&originals).unwrap()).unwrap();
        for (original, resource) in decoded.iter().zip(&bundle.resources) {
            assert!(
                original.bytes.starts_with(b"\x89PNG\r\n\x1a\n")
                    || original.bytes.starts_with(b"\xff\xd8\xff")
                    || original.bytes.starts_with(b"RIFF")
            );
            if original.format != NetworkImageFormat::Png {
                assert_ne!(original.bytes, resource.png);
            }
        }
        assert_eq!(decoded, originals);
    }

    #[test]
    fn validate_accepts_bound_and_round_tripped_originals() {
        let (bundle, originals) = archived_bundle(&sample_bodies());
        validate_originals(&bundle, &originals).unwrap();
        let decoded = decode_originals(&encode_originals(&originals).unwrap()).unwrap();
        validate_originals(&bundle, &decoded).unwrap();
    }

    #[test]
    fn validate_refuses_an_ordinal_without_a_binding() {
        let bundle = PortableBundle {
            representations: vec![Representation {
                mime: Mime::Html,
                bytes: b"<img src=\"https://example.com/a.png\">".to_vec(),
            }],
            resources: Vec::new(),
            bindings: Vec::new(),
            completeness: Completeness::ExternalReferences,
        };
        bundle.validate().unwrap();
        let originals = vec![NetworkImageOriginal {
            image_index: 0,
            resource_id: 1,
            format: NetworkImageFormat::Png,
            bytes: decode(PNG_1X1),
        }];
        assert_refused(validate_originals(&bundle, &originals));
    }

    #[test]
    fn validate_refuses_a_resource_mismatch() {
        let (bundle, mut originals) = archived_bundle(&sample_bodies());
        originals[0].resource_id = 7;
        assert_refused(validate_originals(&bundle, &originals));
    }

    #[test]
    fn validate_refuses_format_tag_mismatch_and_unsupported_bodies() {
        let (bundle, mut originals) = archived_bundle(&sample_bodies());
        originals[0].format = NetworkImageFormat::Jpeg;
        assert_refused(validate_originals(&bundle, &originals));
        let (bundle, mut originals) = archived_bundle(&sample_bodies());
        originals[0].bytes = b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>".to_vec();
        assert_refused(validate_originals(&bundle, &originals));
    }

    #[test]
    fn validate_refuses_animated_bodies() {
        let (bundle, mut originals) = archived_bundle(&sample_bodies());
        originals[0].bytes = decode(APNG_TWO_FRAME);
        assert_refused(validate_originals(&bundle, &originals));
        let (bundle, mut originals) = archived_bundle(&sample_bodies());
        originals[2].bytes = decode(WEBP_ANIMATED);
        assert_refused(validate_originals(&bundle, &originals));
    }

    #[test]
    fn validate_refuses_a_derived_png_mismatch() {
        let (bundle, mut originals) = archived_bundle(&sample_bodies());
        originals[0].bytes = decode(PNG_2X2);
        assert_refused(validate_originals(&bundle, &originals));
    }

    #[test]
    fn validate_refuses_duplicate_and_out_of_range_ordinals() {
        let (bundle, mut originals) = archived_bundle(&sample_bodies());
        originals[1].image_index = originals[0].image_index;
        assert_refused(validate_originals(&bundle, &originals));
        let (bundle, mut originals) = archived_bundle(&sample_bodies());
        originals[0].image_index = 9;
        assert_refused(validate_originals(&bundle, &originals));
        let (bundle, mut originals) = archived_bundle(&sample_bodies());
        originals[0].resource_id = 0;
        assert_refused(validate_originals(&bundle, &originals));
    }

    #[test]
    fn validate_enforces_byte_budgets() {
        let (bundle, mut originals) = archived_bundle(&sample_bodies());
        originals[0].bytes = vec![0u8; ARCHIVE_BYTES_PER_RESOURCE + 1];
        assert_refused(validate_originals(&bundle, &originals));
        let (bundle, _) = archived_bundle(&sample_bodies());
        let over_total = (0..3)
            .map(|index| NetworkImageOriginal {
                image_index: index,
                resource_id: index + 1,
                format: NetworkImageFormat::Png,
                bytes: vec![0u8; 6 * 1024 * 1024],
            })
            .collect::<Vec<_>>();
        assert_refused(validate_originals(&bundle, &over_total));
    }

    #[test]
    fn validate_enforces_the_cumulative_pixel_budget() {
        let body = packed_to_png(1900, 1600, &vec![0u8; 1900 * 1600 * 4]).unwrap();
        let derived = network_image::decode_static_png(
            &body,
            NetworkImageFormat::Png,
            &NetworkImageLimits::default(),
        )
        .unwrap();
        assert_eq!(derived.pixels, 1900_u64 * 1600);
        let html = b"<img src=\"tailsync-resource:1\"><img src=\"tailsync-resource:1\"><img src=\"tailsync-resource:1\">".to_vec();
        let inventory = super::super::html::reference_inventory(&html).unwrap();
        let bindings = inventory
            .images
            .iter()
            .map(|span| ResourceBinding {
                representation: 0,
                resource: 1,
                offset: span.start as u32,
                length: (span.end - span.start) as u32,
            })
            .collect();
        let bundle = PortableBundle {
            representations: vec![Representation {
                mime: Mime::Html,
                bytes: html,
            }],
            resources: vec![Resource {
                id: 1,
                png: derived.png,
            }],
            bindings,
            completeness: Completeness::Complete,
        };
        bundle.validate().unwrap();
        let originals = (0..3)
            .map(|index| NetworkImageOriginal {
                image_index: index,
                resource_id: 1,
                format: NetworkImageFormat::Png,
                bytes: body.clone(),
            })
            .collect::<Vec<_>>();
        assert_refused(validate_originals(&bundle, &originals));
    }
}
