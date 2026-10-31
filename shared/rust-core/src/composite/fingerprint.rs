//! Origin fingerprint: a projection of *what the target platform will actually
//! publish*, used only for local self-write detection.
//!
//! This is deliberately **not** the wire digest (ADR-004 K1). The wire digest keeps
//! the exact encoding semantics — representation order included — because it is the
//! ACK contract. The origin fingerprint answers a different question: "is the
//! clipboard content I am looking at the content I just published?" The operating
//! system normalises what it hands back, so the answer must not depend on:
//!
//! - the order representations happen to be enumerated in (the defect that let a
//!   device re-broadcast its own write — H-20);
//! - PNG encoding metadata, compression or ancillary chunks: identity is the pixels;
//! - the temporary resource ids used inside HTML bindings: identity is the resource
//!   *content*, so two equal images are interchangeable;
//! - resources no binding refers to, which the read-back legitimately will not have.
//!
//! It must still preserve everything that carries user meaning: text whitespace,
//! RTF control words, and the boundary between native items.
//!
//! When a native shape cannot be converted to a portable bundle, callers get an
//! error and must treat the origin as `UnknownMismatch` rather than fabricating a
//! match from a second, divergent model (ADR-004 K1/R2-03).

use crate::composite::{from_native, png_to_packed, Mime, PortableBundle};
use crate::protocol::native_clipboard::NativeSnapshot;
use crate::protocol::ProtocolError;
use blake3::Hasher;

/// Stable tag for a representation's MIME, so the sort order is explicit and does
/// not depend on enum declaration order.
const fn mime_tag(mime: Mime) -> u8 {
    match mime {
        Mime::Text => 1,
        Mime::Html => 2,
        Mime::Rtf => 3,
        Mime::Png => 4,
    }
}

/// Digest of one representation's *content identity*, after projection.
fn normalized_representation(
    bundle: &PortableBundle,
    index: usize,
) -> Result<[u8; 32], ProtocolError> {
    let representation = &bundle.representations[index];
    match representation.mime {
        Mime::Png => {
            // Pixel identity: decode to the canonical packed form so compression
            // level, filters and ancillary chunks cannot change the answer.
            Ok(*blake3::hash(&png_to_packed(&representation.bytes)?).as_bytes())
        }
        Mime::Html => {
            // Replace every bound resource reference with the digest of the
            // resource *content* it points at. Unbound resources contribute
            // nothing, which is what makes an unused resource harmless.
            let mut bindings: Vec<_> = bundle
                .bindings
                .iter()
                .filter(|binding| binding.representation as usize == index)
                .collect();
            bindings.sort_by_key(|binding| binding.offset);
            let mut hasher = Hasher::new();
            let mut cursor = 0usize;
            for binding in bindings {
                let start = binding.offset as usize;
                let end = start.saturating_add(binding.length as usize);
                if start < cursor || end > representation.bytes.len() {
                    return Err(ProtocolError::InvalidComposite);
                }
                hasher.update(&representation.bytes[cursor..start]);
                let resource = bundle
                    .resources
                    .iter()
                    .find(|resource| resource.id == binding.resource)
                    .ok_or(ProtocolError::InvalidComposite)?;
                hasher.update(b"resource\0");
                hasher.update(&png_to_packed(&resource.png)?);
                cursor = end;
            }
            hasher.update(&representation.bytes[cursor..]);
            Ok(*hasher.finalize().as_bytes())
        }
        // Text and RTF keep their bytes verbatim: whitespace and control words are
        // user-visible content, so no normalisation is applied.
        Mime::Text | Mime::Rtf => Ok(*blake3::hash(&representation.bytes).as_bytes()),
    }
}

/// The order-independent origin fingerprint of a portable bundle.
pub fn origin_fingerprint(bundle: &PortableBundle) -> Result<[u8; 32], ProtocolError> {
    let mut entries = Vec::with_capacity(bundle.representations.len());
    for index in 0..bundle.representations.len() {
        let mime = bundle.representations[index].mime;
        entries.push((mime_tag(mime), normalized_representation(bundle, index)?));
    }
    // Sort by (mime, content) so enumeration order cannot change the result.
    entries.sort_unstable();

    let mut hasher = Hasher::new();
    hasher.update(b"tailsync-origin-fingerprint-v2\0");
    for (tag, digest) in &entries {
        hasher.update(&[*tag]);
        hasher.update(digest);
    }
    Ok(*hasher.finalize().as_bytes())
}

/// Byte formats outside the existing portable model require the whole original
/// set in the independent native domain, even when plain text is also present.
pub fn requires_original_fingerprint(name: &str) -> bool {
    !(crate::protocol::native_policy::legacy_storable(name)
        || crate::protocol::native_policy::control(name))
}
/// Whether a native set must be matched in the independent original domain
/// rather than its portable projection.
///
/// Name-only admission is not enough for images: `public.png` is normally the
/// portable pixel identity, but an APNG carries animation the portable model
/// cannot represent. Classifying such a set through the static cover would let
/// a cover be mistaken for the original and turn the app's own animated write
/// into an external event.
pub fn requires_original_domain(snapshot: &NativeSnapshot) -> bool {
    snapshot.formats.iter().any(|f| {
        requires_original_fingerprint(&f.name)
            || (f.state == crate::protocol::native_clipboard::FormatState::Captured
                && crate::protocol::image::is_image_format_name(&f.name)
                && !crate::protocol::image::probe(&f.bytes)
                    .is_some_and(|m| m.is_static_png() && m.within_budget()))
    })
}
/// Identify recognized QQ messages and self-contained Qt images with unread
/// file descriptors. Bind every captured byte and descriptor in
/// an observation-only domain; this never makes the set eligible for publication.
/// Ordinary captures keep the exact fingerprint used by native publication.
pub fn origin_fingerprint_from_capture(
    snapshot: &NativeSnapshot,
) -> Result<[u8; 32], ProtocolError> {
    if let Ok(digest) = origin_fingerprint_from_native(snapshot) {
        return Ok(digest);
    }
    snapshot.validate()?;
    if super::qt_image::matches(snapshot) {
        let mut original = snapshot.clone();
        original.before = 0;
        original.after = 0;
        original
            .formats
            .retain(|f| !crate::protocol::native_policy::control(&f.name));
        original
            .formats
            .sort_by(|a, b| (a.item, &a.name).cmp(&(b.item, &b.name)));
        let mut h = Hasher::new();
        h.update(b"tailsync-origin-qt-image-observation-v1\0");
        h.update(&original.encode()?);
        return Ok(*h.finalize().as_bytes());
    }
    if !super::qq_projection::message_shape(snapshot)
        || !snapshot
            .formats
            .iter()
            .any(super::qq_projection::deferred_image_descriptor)
    {
        return Err(ProtocolError::InvalidComposite);
    }
    let mut original = snapshot.clone();
    original.before = 0;
    original.after = 0;
    original
        .formats
        .retain(|f| !crate::protocol::native_policy::control(&f.name));
    if original.formats.iter().any(|f| {
        !super::qq_projection::deferred_image_descriptor(f)
            && (f.state != crate::protocol::native_clipboard::FormatState::Captured
                || !crate::protocol::native_policy::macos_publishable(&f.name, f.bytes.len()))
    }) {
        return Err(ProtocolError::InvalidComposite);
    }
    original
        .formats
        .sort_by(|a, b| (a.item, &a.name).cmp(&(b.item, &b.name)));
    let mut h = Hasher::new();
    h.update(b"tailsync-origin-qq-observation-v1\0");
    h.update(&original.encode()?);
    Ok(*h.finalize().as_bytes())
}

/// Fingerprint of an actual publishable set. No deferred/missing format is
/// admitted here, including QQ file descriptors; original replay stays strict.
pub fn origin_fingerprint_from_native(
    snapshot: &NativeSnapshot,
) -> Result<[u8; 32], ProtocolError> {
    let projected = if requires_original_domain(snapshot) {
        Err(crate::composite::CaptureError::NativeOnly)
    } else {
        from_native(snapshot)
    };
    match projected {
        Ok(bundle) => origin_fingerprint(&bundle),
        Err(
            crate::composite::CaptureError::NativeOnly
            | crate::composite::CaptureError::UnsupportedItems
            | crate::composite::CaptureError::Empty,
        ) => {
            snapshot.validate()?;
            let mut original = snapshot.clone();
            original.before = 0;
            original.after = 0;
            original
                .formats
                .retain(|f| !crate::protocol::native_policy::control(&f.name));
            if original.formats.is_empty()
                || original.state != crate::protocol::native_clipboard::SnapshotState::Stable
                || original.formats.iter().any(|f| {
                    f.state != crate::protocol::native_clipboard::FormatState::Captured
                        || !crate::protocol::native_policy::macos_publishable(
                            &f.name,
                            f.bytes.len(),
                        )
                })
            {
                return Err(ProtocolError::InvalidComposite);
            }
            // An independent native-only domain. Existing portable v2 hashes are
            // untouched; original bytes are compared with the published set.
            original
                .formats
                .sort_by(|a, b| (a.item, &a.name).cmp(&(b.item, &b.name)));
            let mut h = Hasher::new();
            h.update(b"tailsync-origin-native-original-v1\0");
            h.update(&original.encode()?);
            Ok(*h.finalize().as_bytes())
        }
        Err(_) => Err(ProtocolError::InvalidComposite),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composite::{Completeness, Representation, Resource, ResourceBinding};

    #[test]
    fn original_fingerprint_name_admission_keeps_the_legacy_domain() {
        for name in [
            "public.utf8-plain-text",
            "public.html",
            "public.rtf",
            "com.apple.flat-rtfd",
            "public.png",
            "public.tiff",
            "CF_UNICODETEXT",
            "HTML Format",
            "Rich Text Format",
            "PNG",
            "CF_DIB",
            "CF_DIBV5",
            "org.tailsync.clipboard-source",
            "org.tailsync.rtfd-converted",
        ] {
            assert!(!requires_original_fingerprint(name), "{name}");
        }
        for name in [
            "public.jpeg",
            "com.compuserve.gif",
            "org.webmproject.webp",
            "TIFF",
            "com.example.vendor-format",
        ] {
            assert!(requires_original_fingerprint(name), "{name}");
        }
    }

    #[test]
    fn o02_opaque_bytes_participate_in_native_origin_matching() {
        use crate::protocol::native_clipboard::*;
        let mut s = NativeSnapshot {
            before: 1,
            after: 1,
            items: 1,
            state: SnapshotState::Stable,
            formats: vec![NativeFormat {
                item: 0,
                name: "org.tailsync.fixture.opaque".into(),
                state: FormatState::Captured,
                bytes: vec![0, 255, 3],
            }],
        };
        let first = origin_fingerprint_from_native(&s).unwrap();
        s.formats.push(NativeFormat {
            item: 0,
            name: "public.utf8-plain-text".into(),
            state: FormatState::Captured,
            bytes: b"text".to_vec(),
        });
        let both = origin_fingerprint_from_native(&s).unwrap();
        assert_ne!(first, both);
        s.formats[0].bytes.push(5);
        assert_ne!(both, origin_fingerprint_from_native(&s).unwrap());
        s.formats[0].state = FormatState::Failed;
        s.formats[0].bytes.clear();
        assert!(origin_fingerprint_from_native(&s).is_err());
    }

    #[test]
    fn o01_raw_rtfd_publication_has_a_matching_original_fingerprint() {
        use crate::protocol::native_clipboard::*;
        let mut snapshot = NativeSnapshot {
            before: 0,
            after: 0,
            items: 1,
            state: SnapshotState::Stable,
            formats: vec![NativeFormat {
                item: 0,
                name: "com.apple.flat-rtfd".into(),
                state: FormatState::Captured,
                bytes: b"RTFD original".to_vec(),
            }],
        };
        let digest = origin_fingerprint_from_native(&snapshot).unwrap();
        snapshot.before = 7;
        snapshot.after = 7;
        snapshot.formats.push(NativeFormat {
            item: 0,
            name: "org.tailsync.clipboard-source".into(),
            state: FormatState::Captured,
            bytes: vec![3; 16],
        });
        assert_eq!(digest, origin_fingerprint_from_native(&snapshot).unwrap());
        snapshot.formats[0].bytes.push(0);
        assert_ne!(digest, origin_fingerprint_from_native(&snapshot).unwrap());
    }
    const RED: [u8; 4] = [255, 0, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];

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

    fn adler32(data: &[u8]) -> u32 {
        let (mut a, mut b) = (1_u32, 0_u32);
        for byte in data {
            a = (a + *byte as u32) % 65521;
            b = (b + a) % 65521;
        }
        (b << 16) | a
    }

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

    /// Hand-rolled 1x1 RGBA PNG so the test can vary the *encoding* while keeping
    /// the pixels identical: stored deflate, and an optional ancillary text chunk.
    fn png(rgba: [u8; 4], with_text_chunk: bool) -> Vec<u8> {
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
        if with_text_chunk {
            // Irrelevant metadata: must not change the pixel identity.
            chunk(b"tEXt", b"Comment\0not part of the image");
        }
        let mut raw = vec![0_u8];
        raw.extend_from_slice(&rgba);
        chunk(b"IDAT", &deflate_stored(&raw));
        chunk(b"IEND", &[]);
        bytes
    }

    fn html_bundle(
        order: [Mime; 2],
        first_resource_id: u16,
        include_unused: bool,
    ) -> PortableBundle {
        let html =
            b"<p>x</p><img src=\"tailsync-resource:1\"><img src=\"tailsync-resource:2\">".to_vec();
        let text = std::str::from_utf8(&html).unwrap();
        let binding = |id: u16| {
            let needle = format!("tailsync-resource:{id}");
            ResourceBinding {
                representation: 1,
                resource: id,
                offset: text.find(&needle).unwrap() as u32,
                length: needle.len() as u32,
            }
        };
        let mut resources = vec![
            Resource {
                id: first_resource_id,
                png: png(RED, false),
            },
            Resource {
                id: first_resource_id + 1,
                png: png(BLUE, false),
            },
        ];
        if include_unused {
            resources.push(Resource {
                id: first_resource_id + 2,
                png: png([1, 2, 3, 4], false),
            });
        }
        let bindings = vec![
            ResourceBinding {
                resource: first_resource_id,
                ..binding(1)
            },
            ResourceBinding {
                resource: first_resource_id + 1,
                ..binding(2)
            },
        ];
        let mut representations = vec![
            Representation {
                mime: Mime::Text,
                bytes: b"plain".to_vec(),
            },
            Representation {
                mime: Mime::Html,
                bytes: html,
            },
        ];
        if order[0] == Mime::Html {
            representations.swap(0, 1);
            // bindings point at whichever index the HTML now occupies
            let html_index = representations
                .iter()
                .position(|r| r.mime == Mime::Html)
                .unwrap() as u16;
            for binding in bindings.iter() {
                let _ = binding;
            }
            let bindings = bindings
                .into_iter()
                .map(|b| ResourceBinding {
                    representation: html_index,
                    ..b
                })
                .collect();
            return PortableBundle {
                representations,
                resources,
                bindings,
                completeness: Completeness::Complete,
            };
        }
        PortableBundle {
            representations,
            resources,
            bindings,
            completeness: Completeness::Complete,
        }
    }

    /// T06 — representation enumeration order must not change the fingerprint.
    #[test]
    fn fingerprint_is_independent_of_representation_order() {
        let a = html_bundle([Mime::Text, Mime::Html], 1, false);
        let b = html_bundle([Mime::Html, Mime::Text], 1, false);
        assert_eq!(
            origin_fingerprint(&a).unwrap(),
            origin_fingerprint(&b).unwrap()
        );
    }

    /// T06 — the temporary resource *id* is not identity; the bytes are.
    #[test]
    fn fingerprint_is_independent_of_resource_ids() {
        let a = html_bundle([Mime::Text, Mime::Html], 1, false);
        let b = html_bundle([Mime::Text, Mime::Html], 7, false);
        assert_eq!(
            origin_fingerprint(&a).unwrap(),
            origin_fingerprint(&b).unwrap()
        );
    }

    /// T07 — a resource no binding refers to must not change the answer: the OS
    /// read-back legitimately will not carry it.
    #[test]
    fn unused_resources_do_not_change_the_fingerprint() {
        let without = html_bundle([Mime::Text, Mime::Html], 1, false);
        let with = html_bundle([Mime::Text, Mime::Html], 1, true);
        assert_eq!(
            origin_fingerprint(&without).unwrap(),
            origin_fingerprint(&with).unwrap()
        );
    }

    /// T07 — PNG encoding metadata is irrelevant; the pixels are the identity.
    #[test]
    fn png_encoding_metadata_does_not_change_the_fingerprint() {
        let plain = PortableBundle {
            representations: vec![Representation {
                mime: Mime::Png,
                bytes: png(RED, false),
            }],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::Complete,
        };
        let annotated = PortableBundle {
            representations: vec![Representation {
                mime: Mime::Png,
                bytes: png(RED, true),
            }],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::Complete,
        };
        assert_eq!(
            origin_fingerprint(&plain).unwrap(),
            origin_fingerprint(&annotated).unwrap(),
            "an ancillary chunk must not change pixel identity"
        );
        let different = PortableBundle {
            representations: vec![Representation {
                mime: Mime::Png,
                bytes: png(BLUE, false),
            }],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::Complete,
        };
        assert_ne!(
            origin_fingerprint(&plain).unwrap(),
            origin_fingerprint(&different).unwrap(),
            "different pixels must differ"
        );
    }

    /// T06 — text whitespace is user-visible content and must be preserved.
    #[test]
    fn text_whitespace_is_preserved() {
        let bundle = |text: &[u8]| PortableBundle {
            representations: vec![Representation {
                mime: Mime::Text,
                bytes: text.to_vec(),
            }],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::Complete,
        };
        assert_ne!(
            origin_fingerprint(&bundle(b"a b")).unwrap(),
            origin_fingerprint(&bundle(b"a  b")).unwrap(),
            "collapsing whitespace would swallow a real user copy"
        );
    }

    /// T06 — RTF control words are content; no reordering or stripping.
    #[test]
    fn rtf_control_words_are_preserved() {
        let bundle = |rtf: &[u8]| PortableBundle {
            representations: vec![Representation {
                mime: Mime::Rtf,
                bytes: rtf.to_vec(),
            }],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::Complete,
        };
        assert_ne!(
            origin_fingerprint(&bundle(br"{\rtf1 a}")).unwrap(),
            origin_fingerprint(&bundle(br"{\rtf1 b}")).unwrap()
        );
    }

    /// O03 — an APNG is matched against its original bytes, not against the
    /// static cover the portable projection would carry. Otherwise the app's own
    /// animated write reads back as an external event.
    #[test]
    fn o03_animated_png_uses_the_original_domain_not_a_cover() {
        use crate::protocol::native_clipboard::*;
        use base64::Engine;
        let apng = base64::engine::general_purpose::STANDARD
            .decode("iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAYAAABytg0kAAAACGFjVEwAAAACAAAAAPONk3AAAAAaZmNUTAAAAAAAAAACAAAAAgAAAAAAAAAAAAEACgAA6FTcAAAAABFJREFUeJxj+M/A8B+EGWAMAEfKB/lnWW63AAAAGmZjVEwAAAABAAAAAgAAAAIAAAAAAAAAAAABAAoAAHMnNtQAAAAUZmRBVAAAAAJ4nGNgYPj/H4KhDAA/0gf51gDBTQAAAABJRU5ErkJggg==")
            .unwrap();
        let snapshot = NativeSnapshot {
            before: 5,
            after: 5,
            items: 1,
            state: SnapshotState::Stable,
            formats: vec![NativeFormat {
                item: 0,
                name: "public.png".into(),
                state: FormatState::Captured,
                bytes: apng,
            }],
        };
        assert!(requires_original_domain(&snapshot));
        let digest =
            origin_fingerprint_from_native(&snapshot).expect("an APNG is a valid original");
        // A different animated frame set must not collide with it.
        let mut other = snapshot.clone();
        other.formats[0].bytes = b"different animated bytes".to_vec();
        assert_ne!(digest, origin_fingerprint_from_native(&other).unwrap());

        // A static PNG keeps the portable pixel domain and never uses the
        // original-only branch.
        let static_png = crate::composite::packed_to_png(1, 1, &[1, 2, 3, 255]).unwrap();
        let static_snapshot = NativeSnapshot {
            before: 5,
            after: 5,
            items: 1,
            state: SnapshotState::Stable,
            formats: vec![NativeFormat {
                item: 0,
                name: "public.png".into(),
                state: FormatState::Captured,
                bytes: static_png.clone(),
            }],
        };
        assert!(!requires_original_domain(&static_snapshot));
        assert!(origin_fingerprint_from_native(&static_snapshot).is_ok());
        assert_ne!(
            digest,
            origin_fingerprint_from_native(&static_snapshot).unwrap()
        );
        // Native original replay does not borrow the portable decoder budget.
        let mut large = static_snapshot.clone();
        large.formats[0].bytes =
            include_bytes!("../../../tailsync-protocol/tests/fixtures/images/over-budget.png")
                .to_vec();
        assert!(requires_original_domain(&large));
        assert!(origin_fingerprint_from_native(&large).is_ok());
    }
}
