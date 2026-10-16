use super::*;
use proptest::prelude::*;

fn png_bytes(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
    bytes.extend_from_slice(&13u32.to_be_bytes());
    bytes.extend_from_slice(b"IHDR");
    bytes.extend_from_slice(&width.to_be_bytes());
    bytes.extend_from_slice(&height.to_be_bytes());
    bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
    bytes.extend_from_slice(&[0, 0, 0, 0]);
    bytes
}

fn bundle() -> PortableBundle {
    PortableBundle::new(
        vec![
            Representation::new(PortableMime::PlainTextUtf8, b"hello world".to_vec()),
            Representation::new(PortableMime::HtmlUtf8, b"<b>hello</b>".to_vec()),
        ],
        Completeness::Complete,
    )
    .with_resources(
        vec![Resource::new(PortableMime::Png, png_bytes(2, 2))],
        vec![ResourceBinding {
            representation: 1,
            resource: 0,
            target: "inline-0".into(),
        }],
    )
}

#[test]
fn content_hash_is_deterministic_and_bundle_scoped() {
    let first = bundle();
    let second = bundle();
    assert_eq!(content_hash(&first), content_hash(&second));

    let mut different_text = bundle();
    different_text.representations[0].bytes = b"hello worlD".to_vec().into();
    assert_ne!(content_hash(&first), content_hash(&different_text));
}

#[test]
fn content_hash_ignores_completeness() {
    let complete = bundle();
    let mut external = bundle();
    external.completeness = Completeness::ExternalReferences;
    let mut missing = bundle();
    missing.completeness = Completeness::MissingFormats;

    assert_eq!(content_hash(&complete), content_hash(&external));
    assert_eq!(content_hash(&complete), content_hash(&missing));
}

#[test]
fn content_hash_is_order_sensitive() {
    let ordered = bundle();
    let mut swapped = bundle();
    swapped.representations.swap(0, 1);
    assert_ne!(content_hash(&ordered), content_hash(&swapped));
}

#[test]
fn content_hash_covers_resources_and_bindings() {
    let baseline = bundle();

    let mut different_resource = bundle();
    different_resource.resources[0].bytes = png_bytes(3, 3).into();
    assert_ne!(content_hash(&baseline), content_hash(&different_resource));

    let mut different_binding_target = bundle();
    different_binding_target.bindings[0].target = "inline-1".into();
    assert_ne!(
        content_hash(&baseline),
        content_hash(&different_binding_target)
    );

    let mut different_binding_index = bundle();
    different_binding_index.bindings[0].resource = 0;
    different_binding_index.bindings[0].representation = 0;
    assert_ne!(
        content_hash(&baseline),
        content_hash(&different_binding_index)
    );
}

#[test]
fn support_level_matches_the_frozen_scope() {
    assert_eq!(
        support_level(PortableMime::PlainTextUtf8),
        SupportLevel::Portable
    );
    assert_eq!(
        support_level(PortableMime::HtmlUtf8),
        SupportLevel::Portable
    );
    assert_eq!(support_level(PortableMime::Png), SupportLevel::Portable);
    assert_eq!(support_level(PortableMime::Rtf), SupportLevel::LocalOnly);
}

#[test]
fn native_format_mapping_rejects_unknown_formats() {
    assert_eq!(
        portable_mime_for_native("public.utf8-plain-text"),
        Some(PortableMime::PlainTextUtf8)
    );
    assert_eq!(
        portable_mime_for_native("CF_UNICODETEXT"),
        Some(PortableMime::PlainTextUtf8)
    );
    assert_eq!(
        portable_mime_for_native("HTML Format"),
        Some(PortableMime::HtmlUtf8)
    );
    assert_eq!(
        portable_mime_for_native("public.png"),
        Some(PortableMime::Png)
    );
    assert_eq!(portable_mime_for_native("com.example.private"), None);
    assert_eq!(portable_mime_for_native(""), None);
}

#[test]
fn codec_round_trip_feeds_the_fingerprint() {
    use tailsync_protocol::composite::{decode_bundle, encode_bundle};

    let bundle = bundle();
    let encoded = encode_bundle(&bundle).expect("bundle encodes");
    let decoded = decode_bundle(&encoded).expect("bundle decodes");
    assert_eq!(decoded, bundle);
    assert_eq!(content_hash(&decoded), content_hash(&bundle));
}

#[test]
fn conversion_result_accessors_track_completeness() {
    let complete = ConversionResult::Complete(bundle());
    assert!(complete.is_complete());
    assert!(complete.bundle().is_some());
    assert!(complete.dropped().is_empty());

    let partial = ConversionResult::Partial {
        bundle: bundle(),
        dropped: vec!["com.example.private".into()],
    };
    assert!(!partial.is_complete());
    assert!(partial.bundle().is_some());
    assert_eq!(partial.dropped(), ["com.example.private"]);

    let unsupported = ConversionResult::Unsupported {
        reason: "no supported formats".into(),
    };
    assert!(!unsupported.is_complete());
    assert!(unsupported.bundle().is_none());
    assert!(unsupported.dropped().is_empty());
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
        prop::collection::vec(any::<u8>(), 0..32),
        0u8..3,
    )
        .prop_map(|(mimes, seed, completeness)| {
            let representations = mimes
                .into_iter()
                .enumerate()
                .map(|(index, mime)| {
                    let bytes = match mime {
                        PortableMime::Png => png_bytes(2 + index as u32, 2),
                        _ => seed
                            .iter()
                            .copied()
                            .chain(std::iter::once(index as u8))
                            .collect::<Vec<u8>>(),
                    };
                    Representation::new(mime, bytes)
                })
                .collect();
            PortableBundle {
                version: COMPOSITE_BUNDLE_VERSION,
                representations,
                resources: Vec::new(),
                bindings: Vec::new(),
                completeness: Completeness::from_u8(completeness).expect("0..3 is valid"),
            }
        })
}

proptest! {
    #[test]
    fn content_hash_is_deterministic(bundle in bundle_strategy()) {
        prop_assert_eq!(content_hash(&bundle), content_hash(&bundle.clone()));
    }

    #[test]
    fn content_hash_is_insensitive_to_completeness(bundle in bundle_strategy(), completeness in 0u8..3) {
        let mut other = bundle.clone();
        other.completeness = Completeness::from_u8(completeness).expect("0..3 is valid");
        prop_assert_eq!(content_hash(&bundle), content_hash(&other));
    }

    #[test]
    fn content_hash_changes_when_a_representation_byte_changes(
        bundle in bundle_strategy(),
        index in any::<usize>(),
        mask in 1u8..=255,
    ) {
        let target = index % bundle.representations.len();
        let mut bytes = bundle.representations[target].bytes.to_vec();
        prop_assume!(!bytes.is_empty());
        let position = index % bytes.len();
        bytes[position] ^= mask;
        let mut mutated = bundle.clone();
        mutated.representations[target].bytes = bytes.into();
        prop_assert_ne!(content_hash(&bundle), content_hash(&mutated));
    }
}
