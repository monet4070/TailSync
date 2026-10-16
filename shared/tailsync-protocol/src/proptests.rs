use super::*;
use crate::composite::{
    decode_bundle, encode_bundle, Completeness, PortableBundle, PortableMime, Representation,
    Resource, ResourceBinding, COMPOSITE_BUNDLE_VERSION,
};
use proptest::prelude::*;

fn command_strategy() -> impl Strategy<Value = Command> {
    let variants = (0x0001..=0x0110)
        .filter_map(Command::from_u16)
        .collect::<Vec<_>>();
    assert!(!variants.is_empty());
    proptest::sample::select(variants)
}

fn small_bytes(max: usize) -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 0..max)
}

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

fn bundle_strategy() -> impl Strategy<Value = PortableBundle> {
    (
        proptest::sample::subsequence(
            vec![
                PortableMime::PlainTextUtf8,
                PortableMime::HtmlUtf8,
                PortableMime::Rtf,
                PortableMime::Png,
            ],
            1..=4,
        ),
        small_bytes(48),
        prop::collection::vec((1u32..32, 1u32..32, 0usize..64), 0..3),
        0u8..3,
    )
        .prop_map(|(mimes, seed, resources, completeness)| {
            let representations = mimes
                .into_iter()
                .enumerate()
                .map(|(index, mime)| {
                    let bytes = match mime {
                        PortableMime::Png => png_bytes(2 + index as u32, 2, index),
                        _ => seed
                            .iter()
                            .copied()
                            .chain(std::iter::once(index as u8))
                            .collect::<Vec<u8>>(),
                    };
                    Representation::new(mime, bytes)
                })
                .collect::<Vec<_>>();
            let resources = resources
                .into_iter()
                .map(|(width, height, tail)| {
                    Resource::new(PortableMime::Png, png_bytes(width, height, tail))
                })
                .collect::<Vec<_>>();
            let bindings = resources
                .iter()
                .enumerate()
                .map(|(index, _)| ResourceBinding {
                    representation: 0,
                    resource: index as u16,
                    target: format!("img-{index}"),
                })
                .collect::<Vec<_>>();
            PortableBundle {
                version: COMPOSITE_BUNDLE_VERSION,
                representations,
                resources,
                bindings,
                completeness: Completeness::from_u8(completeness).expect("0..3 is valid"),
            }
        })
}

proptest! {
    #[test]
    fn frame_round_trips_through_encode_and_decode(
        command in command_strategy(),
        flags in any::<u8>(),
        sequence in any::<u32>(),
        payload in small_bytes(256),
    ) {
        let frame = Frame::try_new(command, flags, sequence, payload).unwrap();
        let encoded = frame.encode();
        let (decoded, consumed) = Frame::decode(&encoded).unwrap();
        prop_assert_eq!(consumed, encoded.len());
        prop_assert_eq!(decoded.command, frame.command);
        prop_assert_eq!(decoded.flags.0, frame.flags.0);
        prop_assert_eq!(decoded.sequence, frame.sequence);
        prop_assert_eq!(decoded.payload, frame.payload);
    }

    #[test]
    fn frame_decode_never_panics_on_arbitrary_bytes(data in small_bytes(2048)) {
        let _ = Frame::decode(&data);
    }

    #[test]
    fn frame_decode_consumes_at_most_the_input_length(data in small_bytes(1024)) {
        if let Ok((_, consumed)) = Frame::decode(&data) {
            prop_assert!(consumed <= data.len());
        }
    }

    #[test]
    fn file_chunk_payload_decode_never_panics_on_arbitrary_bytes(data in small_bytes(1024)) {
        let _ = FileChunkPayload::decode(&data);
    }

    #[test]
    fn file_offset_decode_never_panics_on_arbitrary_bytes(data in small_bytes(1024)) {
        let _ = FileOffset::decode(&data);
    }

    #[test]
    fn event_envelope_decode_never_panics_on_arbitrary_bytes(data in small_bytes(1024)) {
        let _ = EventEnvelope::decode(&data);
    }

    #[test]
    fn composite_round_trips_through_encode_and_decode(bundle in bundle_strategy()) {
        let encoded = encode_bundle(&bundle).expect("generated bundle encodes");
        prop_assert!(encoded.len() <= crate::composite::MAX_COMPOSITE_PAYLOAD_SIZE);
        let decoded = decode_bundle(&encoded).expect("generated bundle decodes");
        prop_assert_eq!(decoded, bundle);
    }

    #[test]
    fn composite_decode_never_panics_on_arbitrary_bytes(data in small_bytes(4096)) {
        let _ = decode_bundle(&data);
    }

    #[test]
    fn composite_decode_never_panics_after_a_byte_flip(
        bundle in bundle_strategy(),
        offset in any::<usize>(),
        mask in 1u8..=255,
    ) {
        let mut encoded = encode_bundle(&bundle).expect("generated bundle encodes");
        let index = offset % encoded.len();
        encoded[index] ^= mask;
        let _ = decode_bundle(&encoded);
    }
}
