use super::*;
use crate::{Command, Frame, MAX_PAYLOAD_SIZE};

fn png_bytes(width: u32, height: u32, tail: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
    bytes.extend_from_slice(&13u32.to_be_bytes());
    bytes.extend_from_slice(b"IHDR");
    bytes.extend_from_slice(&width.to_be_bytes());
    bytes.extend_from_slice(&height.to_be_bytes());
    bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
    bytes.extend_from_slice(&[0, 0, 0, 0]);
    bytes.resize(bytes.len() + tail, 0xAB);
    bytes
}

fn digest(bytes: &[u8]) -> [u8; COMPOSITE_DIGEST_SIZE] {
    *blake3::hash(bytes).as_bytes()
}

#[derive(Default)]
struct RawManifest {
    representations: Vec<(u8, u32, u32, [u8; COMPOSITE_DIGEST_SIZE])>,
    resources: Vec<(u8, u32, u32, [u8; COMPOSITE_DIGEST_SIZE])>,
    bindings: Vec<(u16, u16, Vec<u8>)>,
}

impl RawManifest {
    fn with_representation(mut self, mime: u8, offset: u32, bytes: &[u8]) -> Self {
        self.representations
            .push((mime, offset, bytes.len() as u32, digest(bytes)));
        self
    }

    fn with_resource(mut self, mime: u8, offset: u32, bytes: &[u8]) -> Self {
        self.resources
            .push((mime, offset, bytes.len() as u32, digest(bytes)));
        self
    }

    fn with_binding(mut self, representation: u16, resource: u16, target: &str) -> Self {
        self.bindings
            .push((representation, resource, target.as_bytes().to_vec()));
        self
    }

    fn encode(&self) -> Vec<u8> {
        let mut manifest = Vec::new();
        for (mime, offset, length, entry_digest) in
            self.representations.iter().chain(self.resources.iter())
        {
            manifest.push(*mime);
            manifest.extend_from_slice(&offset.to_be_bytes());
            manifest.extend_from_slice(&length.to_be_bytes());
            manifest.extend_from_slice(entry_digest);
        }
        for (representation, resource, target) in &self.bindings {
            manifest.extend_from_slice(&representation.to_be_bytes());
            manifest.extend_from_slice(&resource.to_be_bytes());
            manifest.extend_from_slice(&(target.len() as u16).to_be_bytes());
            manifest.extend_from_slice(target);
        }
        manifest
    }
}

fn frame_with_counts(counts: (usize, usize, usize), manifest: &[u8], byte_area: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&COMPOSITE_MAGIC);
    out.extend_from_slice(&COMPOSITE_BUNDLE_VERSION.to_be_bytes());
    out.push(Completeness::Complete.as_u8());
    out.push(0);
    out.extend_from_slice(&(counts.0 as u16).to_be_bytes());
    out.extend_from_slice(&(counts.1 as u16).to_be_bytes());
    out.extend_from_slice(&(counts.2 as u16).to_be_bytes());
    out.extend_from_slice(&(manifest.len() as u32).to_be_bytes());
    out.extend_from_slice(&(byte_area.len() as u32).to_be_bytes());
    out.extend_from_slice(manifest);
    out.extend_from_slice(byte_area);
    out
}

fn valid_bundle() -> PortableBundle {
    let png = png_bytes(2, 2, 4);
    PortableBundle::new(
        vec![
            Representation::new(PortableMime::PlainTextUtf8, b"hello".to_vec()),
            Representation::new(PortableMime::HtmlUtf8, b"<p>hi</p>".to_vec()),
            Representation::new(PortableMime::Png, png),
        ],
        Completeness::Complete,
    )
}

#[test]
fn round_trips_a_mixed_bundle() {
    let bundle = valid_bundle();
    let encoded = encode_bundle(&bundle).expect("bundle encodes");
    let decoded = decode_bundle(&encoded).expect("bundle decodes");
    assert_eq!(decoded, bundle);
}

#[test]
fn round_trips_resources_and_bindings() {
    let bundle = valid_bundle().with_resources(
        vec![
            Resource::new(PortableMime::Png, png_bytes(4, 4, 2)),
            Resource::new(PortableMime::Png, png_bytes(8, 8, 0)),
        ],
        vec![
            ResourceBinding {
                representation: 1,
                resource: 0,
                target: "img-a".into(),
            },
            ResourceBinding {
                representation: 1,
                resource: 1,
                target: "img-b".into(),
            },
        ],
    );
    let encoded = encode_bundle(&bundle).expect("bundle encodes");
    assert_eq!(decode_bundle(&encoded).expect("bundle decodes"), bundle);
}

#[test]
fn encoded_body_stays_inside_the_event_budget() {
    let encoded = encode_bundle(&valid_bundle()).unwrap();
    assert!(encoded.len() <= MAX_COMPOSITE_PAYLOAD_SIZE);
    assert!(encoded.len() + crate::EVENT_ENVELOPE_HEADER_SIZE <= MAX_COMPOSITE_EVENT_SIZE);
}

#[test]
fn rejects_invalid_magic() {
    let mut encoded = encode_bundle(&valid_bundle()).unwrap();
    encoded[0] = b'X';
    assert!(matches!(
        decode_bundle(&encoded),
        Err(ProtocolError::CompositeInvalidMagic)
    ));
}

#[test]
fn rejects_unsupported_version() {
    let mut encoded = encode_bundle(&valid_bundle()).unwrap();
    encoded[4..6].copy_from_slice(&2u16.to_be_bytes());
    assert!(matches!(
        decode_bundle(&encoded),
        Err(ProtocolError::CompositeUnsupportedVersion(2))
    ));
}

#[test]
fn rejects_reserved_flags() {
    let mut encoded = encode_bundle(&valid_bundle()).unwrap();
    encoded[7] = 1;
    assert!(matches!(
        decode_bundle(&encoded),
        Err(ProtocolError::CompositeReservedFlags)
    ));
}

#[test]
fn rejects_unknown_completeness() {
    let mut encoded = encode_bundle(&valid_bundle()).unwrap();
    encoded[6] = 0x7f;
    assert!(matches!(
        decode_bundle(&encoded),
        Err(ProtocolError::CompositeUnknownCompleteness(0x7f))
    ));
}

#[test]
fn rejects_unknown_portable_mime() {
    let manifest = RawManifest::default().with_representation(0x09, 0, &[1]);
    let frame = frame_with_counts((1, 0, 0), &manifest.encode(), &[1]);
    assert!(matches!(
        decode_bundle(&frame),
        Err(ProtocolError::CompositeUnknownMime(0x09))
    ));
}

#[test]
fn rejects_truncated_body() {
    let mut encoded = encode_bundle(&valid_bundle()).unwrap();
    encoded.pop();
    assert!(matches!(
        decode_bundle(&encoded),
        Err(ProtocolError::CompositeTruncated)
    ));
}

#[test]
fn rejects_trailing_bytes() {
    let mut encoded = encode_bundle(&valid_bundle()).unwrap();
    encoded.push(0);
    assert!(matches!(
        decode_bundle(&encoded),
        Err(ProtocolError::CompositeTrailingBytes)
    ));
}

#[test]
fn rejects_manifest_trailing_bytes() {
    let manifest =
        RawManifest::default().with_representation(PortableMime::PlainTextUtf8.as_u8(), 0, &[1]);
    let mut raw_manifest = manifest.encode();
    raw_manifest.extend_from_slice(&[0, 0]);
    let frame = frame_with_counts((1, 0, 0), &raw_manifest, &[1]);
    assert!(matches!(
        decode_bundle(&frame),
        Err(ProtocolError::CompositeManifestMismatch)
    ));
}

#[test]
fn rejects_too_many_representations() {
    let frame = frame_with_counts((MAX_COMPOSITE_REPRESENTATIONS + 1, 0, 0), &[], &[]);
    assert!(matches!(
        decode_bundle(&frame),
        Err(ProtocolError::CompositeTooManyEntries {
            kind: "representations",
            ..
        })
    ));
}

#[test]
fn rejects_too_many_resources() {
    let frame = frame_with_counts((0, MAX_COMPOSITE_RESOURCES + 1, 0), &[], &[]);
    assert!(matches!(
        decode_bundle(&frame),
        Err(ProtocolError::CompositeTooManyEntries {
            kind: "resources",
            ..
        })
    ));
}

#[test]
fn rejects_too_many_bindings() {
    let frame = frame_with_counts((0, 0, MAX_COMPOSITE_BINDINGS + 1), &[], &[]);
    assert!(matches!(
        decode_bundle(&frame),
        Err(ProtocolError::CompositeTooManyEntries {
            kind: "bindings",
            ..
        })
    ));
}

#[test]
fn rejects_oversized_manifest() {
    let mut frame = frame_with_counts((0, 0, 0), &[], &[]);
    frame[14..18].copy_from_slice(&((MAX_COMPOSITE_MANIFEST_SIZE + 1) as u32).to_be_bytes());
    assert!(matches!(
        decode_bundle(&frame),
        Err(ProtocolError::CompositeManifestTooLarge)
    ));
}

#[test]
fn rejects_body_over_the_total_budget() {
    let mut frame = frame_with_counts((0, 0, 0), &[], &[]);
    frame[18..22].copy_from_slice(&((MAX_COMPOSITE_PAYLOAD_SIZE as u32) + 1).to_be_bytes());
    assert!(matches!(
        decode_bundle(&frame),
        Err(ProtocolError::CompositeTooLarge { .. })
    ));
}

#[test]
fn rejects_overflowing_byte_range() {
    let manifest = RawManifest {
        representations: vec![(
            PortableMime::PlainTextUtf8.as_u8(),
            u32::MAX,
            2,
            [0; COMPOSITE_DIGEST_SIZE],
        )],
        ..RawManifest::default()
    };
    let frame = frame_with_counts((1, 0, 0), &manifest.encode(), &[1, 2]);
    assert!(matches!(
        decode_bundle(&frame),
        Err(ProtocolError::CompositeInvalidRange)
    ));
}

#[test]
fn rejects_out_of_range_byte_range() {
    let manifest = RawManifest::default().with_representation(
        PortableMime::PlainTextUtf8.as_u8(),
        0,
        &[1, 2, 3, 4, 5],
    );
    let frame = frame_with_counts((1, 0, 0), &manifest.encode(), &[1, 2, 3, 4]);
    assert!(matches!(
        decode_bundle(&frame),
        Err(ProtocolError::CompositeInvalidRange)
    ));
}

#[test]
fn rejects_overlapping_byte_ranges() {
    let byte_area = [1u8, 2, 3, 4, 5, 6];
    let manifest = RawManifest::default()
        .with_representation(PortableMime::PlainTextUtf8.as_u8(), 0, &byte_area[0..4])
        .with_representation(PortableMime::HtmlUtf8.as_u8(), 2, &byte_area[2..6]);
    let frame = frame_with_counts((2, 0, 0), &manifest.encode(), &byte_area);
    assert!(matches!(
        decode_bundle(&frame),
        Err(ProtocolError::CompositeOverlappingRanges)
    ));
}

#[test]
fn rejects_duplicate_representation_format() {
    let byte_area = [1u8, 2];
    let manifest = RawManifest::default()
        .with_representation(PortableMime::PlainTextUtf8.as_u8(), 0, &byte_area[0..1])
        .with_representation(PortableMime::PlainTextUtf8.as_u8(), 1, &byte_area[1..2]);
    let frame = frame_with_counts((2, 0, 0), &manifest.encode(), &byte_area);
    assert!(matches!(
        decode_bundle(&frame),
        Err(ProtocolError::CompositeDuplicateRepresentation(
            PortableMime::PlainTextUtf8
        ))
    ));
}

#[test]
fn rejects_non_image_resource() {
    let byte_area = [1u8, 2];
    let manifest = RawManifest::default()
        .with_representation(PortableMime::PlainTextUtf8.as_u8(), 0, &byte_area[0..1])
        .with_resource(PortableMime::PlainTextUtf8.as_u8(), 1, &byte_area[1..2]);
    let frame = frame_with_counts((1, 1, 0), &manifest.encode(), &byte_area);
    assert!(matches!(
        decode_bundle(&frame),
        Err(ProtocolError::CompositeNonImageResource)
    ));
}

#[test]
fn rejects_digest_mismatch() {
    let byte_area = [1u8, 2];
    let mut manifest = RawManifest::default().with_representation(
        PortableMime::PlainTextUtf8.as_u8(),
        0,
        &byte_area[0..1],
    );
    manifest.representations[0].3[0] ^= 0xff;
    let frame = frame_with_counts((1, 0, 0), &manifest.encode(), &byte_area);
    assert!(matches!(
        decode_bundle(&frame),
        Err(ProtocolError::CompositeDigestMismatch)
    ));
}

#[test]
fn rejects_empty_bundle() {
    let frame = frame_with_counts((0, 0, 0), &[], &[]);
    assert!(matches!(
        decode_bundle(&frame),
        Err(ProtocolError::CompositeEmpty)
    ));
}

#[test]
fn rejects_out_of_range_binding_index() {
    let png = png_bytes(2, 2, 0);
    let byte_area = [png.as_slice(), b"text".as_slice()].concat();
    let manifest = RawManifest::default()
        .with_representation(PortableMime::PlainTextUtf8.as_u8(), 33, &byte_area[33..37])
        .with_resource(PortableMime::Png.as_u8(), 0, &byte_area[0..33])
        .with_binding(4, 0, "img");
    let frame = frame_with_counts((1, 1, 1), &manifest.encode(), &byte_area);
    assert!(matches!(
        decode_bundle(&frame),
        Err(ProtocolError::CompositeInvalidBinding)
    ));
}

#[test]
fn rejects_invalid_binding_target() {
    let bundle = valid_bundle().with_resources(
        vec![Resource::new(PortableMime::Png, png_bytes(2, 2, 0))],
        vec![ResourceBinding {
            representation: 0,
            resource: 0,
            target: String::new(),
        }],
    );
    assert!(matches!(
        encode_bundle(&bundle),
        Err(ProtocolError::CompositeInvalidBinding)
    ));

    let manifest = RawManifest::default()
        .with_representation(PortableMime::PlainTextUtf8.as_u8(), 0, &[1])
        .with_resource(PortableMime::Png.as_u8(), 1, &png_bytes(2, 2, 0));
    let mut raw = manifest.encode();
    raw.extend_from_slice(&0u16.to_be_bytes());
    raw.extend_from_slice(&0u16.to_be_bytes());
    raw.extend_from_slice(&(MAX_COMPOSITE_BINDING_TARGET_BYTES as u16 + 1).to_be_bytes());
    raw.resize(raw.len() + MAX_COMPOSITE_BINDING_TARGET_BYTES + 1, b'a');
    let mut byte_area = vec![1u8];
    byte_area.extend_from_slice(&png_bytes(2, 2, 0));
    let frame = frame_with_counts((1, 1, 1), &raw, &byte_area);
    assert!(matches!(
        decode_bundle(&frame),
        Err(ProtocolError::CompositeInvalidBinding)
    ));
}

#[test]
fn rejects_duplicate_binding() {
    let binding = ResourceBinding {
        representation: 0,
        resource: 0,
        target: "img".into(),
    };
    let bundle = valid_bundle().with_resources(
        vec![Resource::new(PortableMime::Png, png_bytes(2, 2, 0))],
        vec![binding.clone(), binding],
    );
    assert!(matches!(
        encode_bundle(&bundle),
        Err(ProtocolError::CompositeDuplicateBinding)
    ));
}

#[test]
fn rejects_oversized_text_representation() {
    let bundle = PortableBundle::new(
        vec![Representation::new(
            PortableMime::PlainTextUtf8,
            vec![b'a'; MAX_COMPOSITE_TEXT_BYTES + 1],
        )],
        Completeness::Complete,
    );
    assert!(matches!(
        encode_bundle(&bundle),
        Err(ProtocolError::CompositeRepresentationTooLarge {
            mime: PortableMime::PlainTextUtf8,
            ..
        })
    ));
}

#[test]
fn rejects_oversized_html_representation() {
    let bundle = PortableBundle::new(
        vec![Representation::new(
            PortableMime::HtmlUtf8,
            vec![b'a'; MAX_COMPOSITE_HTML_BYTES + 1],
        )],
        Completeness::Complete,
    );
    assert!(matches!(
        encode_bundle(&bundle),
        Err(ProtocolError::CompositeRepresentationTooLarge {
            mime: PortableMime::HtmlUtf8,
            ..
        })
    ));
}

#[test]
fn rejects_oversized_rtf_representation() {
    let bundle = PortableBundle::new(
        vec![Representation::new(
            PortableMime::Rtf,
            vec![b'a'; MAX_COMPOSITE_RTF_BYTES + 1],
        )],
        Completeness::Complete,
    );
    assert!(matches!(
        encode_bundle(&bundle),
        Err(ProtocolError::CompositeRepresentationTooLarge {
            mime: PortableMime::Rtf,
            ..
        })
    ));
}

#[test]
fn accepts_representation_at_the_exact_limit() {
    let bundle = PortableBundle::new(
        vec![Representation::new(
            PortableMime::PlainTextUtf8,
            vec![b'a'; MAX_COMPOSITE_TEXT_BYTES],
        )],
        Completeness::Complete,
    );
    let encoded = encode_bundle(&bundle).unwrap();
    assert_eq!(decode_bundle(&encoded).unwrap(), bundle);
}

#[test]
fn rejects_non_png_image_payloads() {
    let bundle = PortableBundle::new(
        vec![Representation::new(
            PortableMime::Png,
            b"not a png".to_vec(),
        )],
        Completeness::Complete,
    );
    assert!(matches!(
        encode_bundle(&bundle),
        Err(ProtocolError::CompositeInvalidImage)
    ));
}

#[test]
fn rejects_declared_image_decompression_bombs() {
    let bundle = PortableBundle::new(
        vec![Representation::new(
            PortableMime::Png,
            png_bytes(100_000, 100_000, 0),
        )],
        Completeness::Complete,
    );
    assert!(matches!(
        encode_bundle(&bundle),
        Err(ProtocolError::CompositeImageTooLarge)
    ));
}

#[test]
fn rejects_total_over_budget_even_when_every_entry_is_legal() {
    let bundle = PortableBundle::new(
        vec![
            Representation::new(
                PortableMime::PlainTextUtf8,
                vec![b'a'; MAX_COMPOSITE_TEXT_BYTES],
            ),
            Representation::new(PortableMime::HtmlUtf8, vec![b'b'; MAX_COMPOSITE_HTML_BYTES]),
            Representation::new(PortableMime::Png, png_bytes(2, 2, 30 * 1024 * 1024)),
        ],
        Completeness::Complete,
    );
    assert!(matches!(
        encode_bundle(&bundle),
        Err(ProtocolError::CompositeTooLarge { .. })
    ));
}

#[test]
fn composite_commands_are_registered_and_round_trip() {
    for (command, number) in [
        (Command::CompositePayload, 0x010e),
        (Command::CompositeChunk, 0x010f),
        (Command::CompositeAck, 0x0110),
    ] {
        assert_eq!(Command::from_u16(number), Some(command));
        assert_eq!(command as u16, number);
        let frame = Frame::try_new(command, 0, 3, vec![1, 2, 3]).unwrap();
        let encoded = frame.encode();
        let (decoded, consumed) = Frame::decode(&encoded).unwrap();
        assert_eq!(decoded.command, command);
        assert_eq!(consumed, encoded.len());
    }
    assert_eq!(Command::CompositePayload.payload_limit(), MAX_PAYLOAD_SIZE);
    assert_eq!(
        Command::CompositePayload.payload_limit(),
        MAX_COMPOSITE_EVENT_SIZE
    );
    assert_eq!(
        Command::CompositeAck.payload_limit(),
        crate::MAX_CONTROL_PAYLOAD_SIZE
    );
}
