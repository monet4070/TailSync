//! Clipboard representation rules shared by the native adapters and delivery.
pub mod archive_sync;
pub mod available_restore;
pub mod budgets;
pub mod capture;
pub mod capture_retry;
pub mod chunks;
pub mod content_analysis;
pub mod display;
pub mod display_document;
pub mod document_build;
mod document_native_styles;
pub mod fingerprint;
pub mod html;
mod html_lex;
pub mod identity;
pub mod network_image;
pub mod network_originals;
pub mod origin;
pub mod outcome;
pub mod provenance;
mod qq_projection;
pub mod readable_restore;
pub mod resource_archive;
pub mod restore;
pub mod source_context;
pub mod supervisor;
pub use crate::protocol::composite::*;
use crate::protocol::native_clipboard::{FormatState, NativeFormat, NativeSnapshot, SnapshotState};
use crate::protocol::{pack_rgba_image, ProtocolError};
pub use outcome::NativePublish;
pub use provenance::CaptureProvenance;
use std::io::Cursor;

/// Decode under the admitted pixel and decoder allocation budgets. Reject
/// animated PNG instead of silently replacing a document image by frame one.
pub fn png_to_packed(bytes: &[u8]) -> Result<Vec<u8>, ProtocolError> {
    png_rgba_size(bytes)?;
    let mut decoder =
        png::Decoder::new_with_limits(Cursor::new(bytes), png::Limits { bytes: RGBA_LIMIT });
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder
        .read_info()
        .map_err(|_| ProtocolError::InvalidComposite)?;
    if reader.info().animation_control.is_some() {
        return Err(ProtocolError::InvalidComposite);
    }
    let size = reader
        .output_buffer_size()
        .filter(|n| *n <= RGBA_LIMIT)
        .ok_or(ProtocolError::InvalidComposite)?;
    let mut decoded = vec![0; size];
    let info = reader
        .next_frame(&mut decoded)
        .map_err(|_| ProtocolError::InvalidComposite)?;
    reader
        .finish()
        .map_err(|_| ProtocolError::InvalidComposite)?;
    if info.bit_depth != png::BitDepth::Eight {
        return Err(ProtocolError::InvalidComposite);
    }
    let mut rgba = Vec::with_capacity(info.width as usize * info.height as usize * 4);
    let stride = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Grayscale => 1,
        _ => return Err(ProtocolError::InvalidComposite),
    };
    for pixel in decoded[..info.buffer_size()].chunks_exact(stride) {
        match stride {
            4 => rgba.extend_from_slice(pixel),
            3 => rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]),
            2 => rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]]),
            _ => rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], 255]),
        }
    }
    pack_rgba_image(info.width, info.height, &rgba)
}
pub fn packed_to_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, ProtocolError> {
    crate::protocol::pack_rgba_image(width, height, rgba)?;
    let mut bytes = vec![];
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .map_err(|_| ProtocolError::InvalidComposite)?
            .write_image_data(rgba)
            .map_err(|_| ProtocolError::InvalidComposite)?;
    }
    Ok(bytes)
}
pub fn native_digest(snapshot: &NativeSnapshot) -> Result<[u8; 32], ProtocolError> {
    let mut stable = snapshot.clone();
    stable.before = 0;
    stable.after = 0;
    stable
        .formats
        .retain(|f| f.name != "org.tailsync.clipboard-source");
    Ok(*blake3::hash(&stable.encode()?).as_bytes())
}
/// The **structural** gate: wire-legal, resource-complete, budgets respected.
///
/// Storage, chunking and history reads use this. It deliberately excludes native
/// preparation (R2-02): a payload this platform cannot publish is still worth
/// storing, and refusing it here would turn recoverable content into a rejected
/// packet. The two gates are separate on purpose — see `validate_for_publish`.
pub fn validate_storable(bundle: &PortableBundle) -> Result<(), ProtocolError> {
    bundle.validate()
}

/// The **native-preparation** gate, separate from the structural gate
/// `PortableBundle::validate`.
///
/// Structure can be perfectly legal on the wire and still be unpublishable on this
/// platform: a PNG that will not decode, or HTML whose bound resources materialise
/// past the native HTML limit. R2-02 requires the second gate to be *distinct* from
/// the first, so a payload that only fails here is still safe to store in history —
/// it must not be discarded as a malformed packet, and it must be refused **before**
/// the platform clears the clipboard.
pub fn validate_for_publish(bundle: &PortableBundle) -> Result<(), ProtocolError> {
    bundle.validate()?;
    for bytes in bundle
        .representation(Mime::Png)
        .into_iter()
        .chain(bundle.resources.iter().map(|r| r.png.as_slice()))
    {
        png_to_packed(bytes)?;
    }
    // R2-02: size every HTML representation up front. Discovering the overrun during
    // materialisation would happen after `clearContents()`.
    for index in 0..bundle.representations.len() {
        if bundle.representations[index].mime == Mime::Html {
            html::materialized_len(bundle, index)?;
        }
    }
    Ok(())
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureError {
    Unstable,
    Empty,
    Busy,
    Failed,
    UnsupportedItems,
    NativeOnly,
    Files,
    InvalidFormat,
    Suppressed,
}
/// Capture retains valid source bytes even when their portable projection is
/// unsupported or a provider only partially succeeded. With no captured source
/// bytes, a failed provider remains a read error rather than an empty clipboard.
pub fn from_native_capture(snapshot: &NativeSnapshot) -> Result<PortableBundle, CaptureError> {
    snapshot
        .validate()
        .map_err(|_| CaptureError::InvalidFormat)?;
    from_native(snapshot).map_err(|error| preserve_native_conversion(snapshot, error))
}
fn preserve_native_conversion(snapshot: &NativeSnapshot, error: CaptureError) -> CaptureError {
    match error {
        CaptureError::InvalidFormat | CaptureError::Empty | CaptureError::Failed
            if snapshot.state == SnapshotState::Stable
                && snapshot.formats.iter().any(|f| {
                    f.state == FormatState::Captured
                        && !crate::protocol::native_policy::control(&f.name)
                }) =>
        {
            CaptureError::NativeOnly
        }
        other => other,
    }
}
/// Only a single native semantic item is portable. Genuine multiple items
/// remain distinguishable; no implicit flattening or first-item truncation.
pub fn from_native(snapshot: &NativeSnapshot) -> Result<PortableBundle, CaptureError> {
    if snapshot
        .formats
        .iter()
        .any(|f| crate::protocol::native_policy::suppressed(&f.name))
    {
        return Err(CaptureError::Suppressed);
    }
    snapshot
        .validate()
        .map_err(|_| CaptureError::InvalidFormat)?;
    match snapshot.state {
        SnapshotState::Stable => {}
        SnapshotState::Empty => return Err(CaptureError::Empty),
        SnapshotState::Unstable => return Err(CaptureError::Unstable),
        SnapshotState::Busy => return Err(CaptureError::Busy),
        SnapshotState::Failed => return Err(CaptureError::Failed),
        // A genuine file selection belongs to the file pipeline, not to the composite
        // model. Reporting it as `Files` is what routes a large file copy to sync and
        // history instead of dropping it (R5-01).
        SnapshotState::FileSelection => return Err(CaptureError::Files),
        SnapshotState::Suppressed => return Err(CaptureError::Suppressed),
    }
    if snapshot
        .formats
        .iter()
        .any(|f| matches!(f.name.as_str(), "public.file-url" | "CF_HDROP"))
    {
        let pure_files = snapshot.formats.iter().all(|f| {
            matches!(
                f.name.as_str(),
                "public.file-url" | "CF_HDROP" | "org.tailsync.clipboard-source"
            )
        });
        return Err(if pure_files {
            CaptureError::Files
        } else {
            CaptureError::NativeOnly
        });
    }
    if snapshot.items != 1 {
        return Err(CaptureError::UnsupportedItems);
    }
    if snapshot
        .formats
        .iter()
        .any(|f| f.name == "com.apple.flat-rtfd")
        && !snapshot
            .formats
            .iter()
            .any(|f| f.name == "org.tailsync.rtfd-converted" && f.state == FormatState::Captured)
    {
        // Keep attachments in native history until bounded helper conversion succeeds.
        return Err(CaptureError::NativeOnly);
    }
    let mut bundle = PortableBundle {
        representations: vec![],
        resources: vec![],
        bindings: vec![],
        completeness: Completeness::Complete,
    };
    for format in &snapshot.formats {
        let mime = match format.name.as_str() {
            "public.utf8-plain-text" | "CF_UNICODETEXT" => Mime::Text,
            "public.html" | "HTML Format" => Mime::Html,
            "public.rtf" | "Rich Text Format" => Mime::Rtf,
            "public.png" | "PNG" => Mime::Png,
            _ => continue,
        };
        if format.state != FormatState::Captured {
            bundle.completeness = Completeness::MissingFormats;
            continue;
        }
        let bytes = if format.name == "CF_UNICODETEXT" {
            if format.bytes.len() % 2 != 0 {
                return Err(CaptureError::InvalidFormat);
            }
            let units: Vec<_> = format
                .bytes
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .take_while(|v| *v != 0)
                .collect();
            String::from_utf16(&units)
                .map_err(|_| CaptureError::InvalidFormat)?
                .into_bytes()
        } else if format.name == "HTML Format" {
            html::decode_cf_html(&format.bytes)
                .map_err(|_| CaptureError::InvalidFormat)?
                .into_bytes()
        } else {
            let mut bytes = format.bytes.clone();
            if format.name == "Rich Text Format" {
                while bytes.last() == Some(&0) {
                    bytes.pop();
                }
            }
            bytes
        };
        if !bytes.is_empty() {
            bundle.representations.push(Representation { mime, bytes });
        }
    }
    if bundle.representations.is_empty() {
        if snapshot
            .formats
            .iter()
            .any(|f| matches!(f.state, FormatState::Failed | FormatState::TooLarge))
        {
            return Err(CaptureError::Failed);
        }
        return Err(
            if snapshot
                .formats
                .iter()
                .any(|f| f.state == FormatState::Captured && f.name == "com.apple.flat-rtfd")
            {
                CaptureError::NativeOnly
            } else {
                CaptureError::Empty
            },
        );
    }
    html::extract_resources(&mut bundle).map_err(|_| CaptureError::InvalidFormat)?;
    validate_for_publish(&bundle).map_err(|_| CaptureError::InvalidFormat)?;
    Ok(bundle)
}
/// Select a compatibility projection without modifying the captured originals.
///
/// O03 separates *retention* from *projection*. A retained original encoding
/// (APNG/GIF/JPEG/WebP/multi-page TIFF) is never dropped by the archive, and a
/// static decoder refusing it can never turn a captured item into
/// `InvalidFormat`. Only a static PNG original or an explicitly derived static
/// cover enters the portable model; everything else stays native-only.
pub fn from_archive(
    archive: &crate::protocol::native_archive::NativeArchive,
) -> Result<PortableBundle, CaptureError> {
    use crate::protocol::image;
    use crate::protocol::native_archive::{
        CONVERTER_RTFD_TO_PORTABLE, CONVERTER_STATIC_COVER, CONVERTER_TIFF_TO_PNG,
    };
    archive
        .validate()
        .map_err(|_| CaptureError::InvalidFormat)?;
    if let Some(projection) = qq_projection::project(archive) {
        return from_native(&projection)
            .map_err(|error| preserve_native_conversion(&archive.original, error));
    }
    let mut projection = archive.original.clone();
    projection
        .formats
        .retain(|f| !crate::protocol::native_policy::control(&f.name));
    // A captured image the portable model cannot carry is retained in the
    // archive but withheld from this projection. Keeping it here would let the
    // static PNG gate reject the whole capture (APNG) or publish a frame as if
    // it were the original (GIF/WebP).
    projection.formats.retain(|f| {
        !(f.state == FormatState::Captured
            && image::is_image_format_name(&f.name)
            && !image::is_static_png(&f.bytes))
    });
    let mut rtfd_html = false;
    let mut rtfd_text = false;
    for d in &archive.derived {
        let admitted = match (
            d.source_name.as_str(),
            d.converter.as_str(),
            d.name.as_str(),
        ) {
            ("com.apple.flat-rtfd", CONVERTER_RTFD_TO_PORTABLE, "public.html") => {
                rtfd_html = true;
                true
            }
            ("com.apple.flat-rtfd", CONVERTER_RTFD_TO_PORTABLE, "public.utf8-plain-text") => {
                rtfd_text = true;
                true
            }
            ("public.tiff", CONVERTER_TIFF_TO_PNG, "public.png") => true,
            // Explicit bounded static cover for an encoding the portable model
            // cannot carry. The original stays in `archive.original`.
            (source, CONVERTER_STATIC_COVER, "public.png") => {
                image::is_image_format_name(source) && image::is_static_png(&d.bytes)
            }
            _ => false,
        };
        // Never replace an already captured static PNG with a derived cover.
        let original_png = d.name == "public.png"
            && projection.formats.iter().any(|f| {
                f.item == d.item
                    && f.name == d.name
                    && f.state == FormatState::Captured
                    && image::is_static_png(&f.bytes)
            });
        if admitted && !original_png {
            projection
                .formats
                .retain(|f| f.item != d.item || f.name != d.name);
            projection.formats.push(NativeFormat {
                item: d.item,
                name: d.name.clone(),
                state: FormatState::Captured,
                bytes: d.bytes.clone(),
            });
        }
    }
    if rtfd_html && rtfd_text && projection.items == 1 {
        projection.formats.push(NativeFormat {
            item: 0,
            name: "org.tailsync.rtfd-converted".into(),
            state: FormatState::Captured,
            bytes: vec![1],
        });
    }
    // An unsuccessful portable conversion must not discard a valid archive.
    from_native(&projection).map_err(|error| preserve_native_conversion(&archive.original, error))
}
pub fn to_macos_snapshot(bundle: &PortableBundle) -> Result<NativeSnapshot, ProtocolError> {
    validate_for_publish(bundle)?;
    Ok(NativeSnapshot {
        before: 0,
        after: 0,
        items: 1,
        state: SnapshotState::Stable,
        formats: bundle
            .representations
            .iter()
            .enumerate()
            .map(|(index, r)| {
                Ok(NativeFormat {
                    item: 0,
                    name: match r.mime {
                        Mime::Text => "public.utf8-plain-text",
                        Mime::Html => "public.html",
                        Mime::Rtf => "public.rtf",
                        Mime::Png => "public.png",
                    }
                    .into(),
                    state: FormatState::Captured,
                    bytes: if r.mime == Mime::Html {
                        html::materialize(bundle, index)?
                    } else {
                        r.bytes.clone()
                    },
                })
            })
            .collect::<Result<_, ProtocolError>>()?,
    })
}
/// Exactly one legacy event is selected. Capability changes must never
/// reuse the identity of an already attempted different representation.
pub fn legacy_content(
    bundle: &PortableBundle,
) -> Result<(crate::protocol::Command, Vec<u8>), ProtocolError> {
    validate_storable(bundle)?;
    if let Some(png) = bundle
        .representation(Mime::Png)
        .or_else(|| (bundle.resources.len() == 1).then(|| bundle.resources[0].png.as_slice()))
    {
        // The legacy image body adds dimensions and the event adds its header.
        // A wire-legal composite PNG can exceed that older frame's limit.
        if png_rgba_size(png)? + 8 + crate::protocol::EVENT_ENVELOPE_HEADER_SIZE
            <= crate::protocol::Command::ImagePayload.payload_limit()
        {
            return Ok((crate::protocol::Command::ImagePayload, png_to_packed(png)?));
        }
    }
    if let Some(text) = bundle.representation(Mime::Text) {
        if text.len() + crate::protocol::EVENT_ENVELOPE_HEADER_SIZE
            <= crate::protocol::Command::TextPayload.payload_limit()
        {
            return Ok((crate::protocol::Command::TextPayload, text.to_vec()));
        }
    }
    Err(ProtocolError::UnsupportedCompositeFallback)
}

pub fn to_windows_snapshot(bundle: &PortableBundle) -> Result<NativeSnapshot, ProtocolError> {
    validate_for_publish(bundle)?;
    let mut formats = vec![];
    for (index, r) in bundle.representations.iter().enumerate() {
        let (name, bytes) = match r.mime {
            Mime::Text => {
                let text =
                    std::str::from_utf8(&r.bytes).map_err(|_| ProtocolError::InvalidComposite)?;
                (
                    "CF_UNICODETEXT",
                    text.encode_utf16()
                        .chain(Some(0))
                        .flat_map(u16::to_le_bytes)
                        .collect(),
                )
            }
            Mime::Html => (
                "HTML Format",
                html::encode_cf_html(
                    std::str::from_utf8(&html::materialize(bundle, index)?)
                        .map_err(|_| ProtocolError::InvalidComposite)?,
                )?,
            ),
            Mime::Rtf => {
                let mut bytes = r.bytes.clone();
                bytes.push(0);
                ("Rich Text Format", bytes)
            }
            Mime::Png => ("PNG", r.bytes.clone()),
        };
        formats.push(NativeFormat {
            item: 0,
            name: name.into(),
            state: FormatState::Captured,
            bytes,
        });
    }
    Ok(NativeSnapshot {
        before: 0,
        after: 0,
        items: 1,
        state: SnapshotState::Stable,
        formats,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn o10_conversion_limits_preserve_source_but_provider_failure_is_not_empty() {
        let mut snapshot = NativeSnapshot {
            before: 1,
            after: 1,
            items: 1,
            state: SnapshotState::Stable,
            formats: vec![NativeFormat {
                item: 0,
                name: "CF_UNICODETEXT".into(),
                state: FormatState::Captured,
                bytes: vec![1], // A valid byte snapshot, invalid portable UTF-16.
            }],
        };
        assert_eq!(from_native(&snapshot), Err(CaptureError::InvalidFormat));
        assert_eq!(
            from_native_capture(&snapshot),
            Err(CaptureError::NativeOnly)
        );
        snapshot.formats[0].name = "fixture.registered.format".into();
        assert_eq!(
            from_native_capture(&snapshot),
            Err(CaptureError::NativeOnly)
        );
        for state in [FormatState::Failed, FormatState::TooLarge] {
            snapshot.formats[0].name = "PNG".into();
            snapshot.formats[0].state = state;
            snapshot.formats[0].bytes.clear();
            assert_eq!(from_native_capture(&snapshot), Err(CaptureError::Failed));
        }
        snapshot.after = 2;
        assert_eq!(
            from_native_capture(&snapshot),
            Err(CaptureError::InvalidFormat)
        );
    }
    #[test]
    fn text_does_not_exclude_html() {
        let s = NativeSnapshot {
            before: 1,
            after: 1,
            items: 1,
            state: SnapshotState::Stable,
            formats: vec![
                NativeFormat {
                    item: 0,
                    name: "public.utf8-plain-text".into(),
                    state: FormatState::Captured,
                    bytes: b"text".to_vec(),
                },
                NativeFormat {
                    item: 0,
                    name: "public.html".into(),
                    state: FormatState::Captured,
                    bytes: b"<b>text</b>".to_vec(),
                },
            ],
        };
        assert_eq!(from_native(&s).unwrap().representations.len(), 2);
    }
    #[test]
    fn failed_image_is_explicit_missing() {
        let s = NativeSnapshot {
            before: 1,
            after: 1,
            items: 1,
            state: SnapshotState::Stable,
            formats: vec![
                NativeFormat {
                    item: 0,
                    name: "public.utf8-plain-text".into(),
                    state: FormatState::Captured,
                    bytes: b"text".to_vec(),
                },
                NativeFormat {
                    item: 0,
                    name: "public.png".into(),
                    state: FormatState::Failed,
                    bytes: vec![],
                },
            ],
        };
        assert_eq!(
            from_native(&s).unwrap().completeness,
            Completeness::MissingFormats
        );
    }
}

#[cfg(test)]
mod qq_projection_tests {
    use super::*;
    use crate::protocol::native_archive::{DerivedFormat, NativeArchive};
    const QQ: &str = "dyn.ah62d4rv4gu8zk3psqrw067cbsv4he4pcsz4gny5ysmy0634ytf2gn";
    const CONVERTER: &str = "tailsync.qq-to-portable.v1";
    fn archive() -> NativeArchive {
        NativeArchive {
            platform: 1,
            enumeration_complete: true,
            original: NativeSnapshot {
                before: 1,
                after: 1,
                items: 2,
                state: SnapshotState::Stable,
                formats: vec![
                    NativeFormat {
                        item: 0,
                        name: QQ.into(),
                        state: FormatState::Captured,
                        bytes: b"retained QQ source".to_vec(),
                    },
                    NativeFormat {
                        item: 1,
                        name: "public.file-url".into(),
                        state: FormatState::Absent,
                        bytes: vec![],
                    },
                ],
            },
            derived: [
                (
                    "public.html",
                    "<p>QQ 中文</p><img src='qq-missing-image:0'><p>after</p>",
                ),
                ("public.utf8-plain-text", "QQ 中文\nafter"),
            ]
            .into_iter()
            .map(|(name, bytes)| DerivedFormat {
                item: 0,
                name: name.into(),
                source_name: QQ.into(),
                converter: CONVERTER.into(),
                bytes: bytes.as_bytes().to_vec(),
            })
            .collect(),
        }
    }
    #[test]
    fn qq_semantic_projection_keeps_message_order_and_original_items() {
        let archive = archive();
        let before = archive.encode().unwrap();
        let portable = from_archive(&archive)
            .expect("a bounded QQ semantic conversion is readable across native items");
        assert_eq!(
            portable.representation(Mime::Text).unwrap(),
            "QQ 中文\nafter".as_bytes()
        );
        assert_eq!(portable.completeness, Completeness::ExternalReferences);
        assert_eq!(archive.encode().unwrap(), before);
        assert_eq!(archive.original.items, 2);
    }
    #[test]
    fn partial_untrusted_or_ambiguous_qq_conversion_never_flattens_native_items() {
        let mut a = archive();
        a.derived.pop();
        assert!(from_archive(&a).is_err());
        let mut a = archive();
        a.derived[0].converter = "unknown".into();
        assert!(from_archive(&a).is_err());
        let mut a = archive();
        a.enumeration_complete = false;
        assert!(from_archive(&a).is_err());
        let mut a = archive();
        a.original.formats[1].state = FormatState::Captured;
        a.original.formats[1].bytes = b"file:///unrelated".to_vec();
        assert!(from_archive(&a).is_err());
        let mut a = archive();
        a.original.formats.push(NativeFormat {
            item: 1,
            name: "public.utf8-plain-text".into(),
            state: FormatState::Captured,
            bytes: b"unrelated item".to_vec(),
        });
        assert!(from_archive(&a).is_err());
    }
}
