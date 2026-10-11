use super::*;
use crate::composite::{
    Completeness, Mime, PortableBundle, Representation, Resource, ResourceBinding, BODY_LIMIT,
};
use proptest::prelude::*;

fn command_strategy() -> impl Strategy<Value = Command> {
    let variants = (0x0001..=0x0113)
        .filter_map(Command::from_u16)
        .collect::<Vec<_>>();
    assert!(!variants.is_empty());
    proptest::sample::select(variants)
}

fn small_bytes(max: usize) -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 0..max)
}

fn bundle_strategy() -> impl Strategy<Value = PortableBundle> {
    (
        proptest::sample::subsequence(vec![Mime::Text, Mime::Html, Mime::Rtf, Mime::Png], 1..=4),
        "[a-zA-Z0-9 中]{1,48}",
        0usize..3,
        proptest::sample::select(vec![
            Completeness::Complete,
            Completeness::ExternalReferences,
            Completeness::MissingFormats,
        ]),
    )
        .prop_map(|(mimes, seed, resource_count, completeness)| {
            let png = include_bytes!("../tests/fixtures/images/transparent.png").to_vec();
            let mut resources = Vec::new();
            let mut bindings = Vec::new();
            let representations = mimes
                .into_iter()
                .enumerate()
                .map(|(index, mime)| {
                    let bytes = match mime {
                        Mime::Png => png.clone(),
                        Mime::Html => {
                            let mut html = format!("<p>{seed}</p>");
                            for id in 1..=resource_count {
                                html.push_str("<img src=\"");
                                let offset = html.len();
                                let reference = format!("tailsync-resource:{id}");
                                html.push_str(&reference);
                                html.push_str("\">");
                                resources.push(Resource {
                                    id: id as u16,
                                    png: png.clone(),
                                });
                                bindings.push(ResourceBinding {
                                    representation: index as u16,
                                    resource: id as u16,
                                    offset: offset as u32,
                                    length: reference.len() as u32,
                                });
                            }
                            html.into_bytes()
                        }
                        Mime::Text => seed.as_bytes().to_vec(),
                        Mime::Rtf => format!("{{\\rtf1 {seed}}}").into_bytes(),
                    };
                    Representation { mime, bytes }
                })
                .collect();
            PortableBundle {
                representations,
                resources,
                bindings,
                completeness,
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
        let payload = payload[..payload.len().min(command.payload_limit())].to_vec();
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
        let encoded = bundle.encode().expect("generated bundle encodes");
        prop_assert!(encoded.len() <= BODY_LIMIT);
        prop_assert_eq!(PortableBundle::decode(&encoded).unwrap(), bundle);
    }

    #[test]
    fn composite_decode_never_panics_after_a_byte_flip(
        bundle in bundle_strategy(),
        offset in any::<usize>(),
        mask in 1u8..=255,
    ) {
        let mut encoded = bundle.encode().expect("generated bundle encodes");
        let index = offset % encoded.len();
        encoded[index] ^= mask;
        let _ = PortableBundle::decode(&encoded);
    }
}
