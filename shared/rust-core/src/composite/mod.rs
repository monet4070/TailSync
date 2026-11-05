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
mod qt_image;
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
/// Whether the Windows snapshot already carries captured raster image bytes.
fn has_windows_raster_image(snapshot: &NativeSnapshot) -> bool {
    snapshot.formats.iter().any(|f| {
        f.state == FormatState::Captured && matches!(f.name.as_str(), "CF_DIB" | "CF_DIBV5" | "PNG")
    })
}

/// Windows applications that place a raster image on the clipboard also expose
/// a `CF_HDROP` entry for the temporary file backing it: QQ image messages,
/// browsers and screenshot tools all do. That is an image event with an
/// implementation detail attached, not a file selection. Routing it to the file
/// pipeline or to native-only dropped a picture whose pixels were already in the
/// clipboard, which is why such a copy never reached the peer.
pub(crate) fn has_backing_file_image(snapshot: &NativeSnapshot) -> bool {
    let has_file = snapshot
        .formats
        .iter()
        .any(|f| matches!(f.name.as_str(), "public.file-url" | "CF_HDROP"));
    has_file && has_windows_raster_image(snapshot)
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
    let image_with_backing_file = has_backing_file_image(snapshot);
    if !image_with_backing_file
        && snapshot
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
            let html =
                html::decode_cf_html(&format.bytes).map_err(|_| CaptureError::InvalidFormat)?;
            // The backing file's machine-local path must never become part of a
            // synced payload: the peer can neither resolve it nor read it.
            let local_image_wrapper = html::images_are_local_files(html.as_bytes())
                && (image_with_backing_file
                    || (has_windows_raster_image(snapshot)
                        && html::text_summary(html.as_bytes()).trim().is_empty()));
            if local_image_wrapper {
                continue;
            }
            html.into_bytes()
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
        CONVERTER_WINDOWS_DIB_TO_PNG,
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
            // Windows: the DIB/DIBV5 bytes remain the original; this static PNG
            // is a compatibility representation only. The converter name alone
            // is not admission: platform, source and a genuine captured original
            // must all agree.
            ("CF_DIB" | "CF_DIBV5", CONVERTER_WINDOWS_DIB_TO_PNG, "PNG") => {
                archive.platform == 2 && image::is_static_png(&d.bytes)
            }
            _ => false,
        };
        // Never replace an already advertised original PNG with a derived
        // cover. A corrupt or animated original PNG keeps its failure status;
        // the first round does not substitute a different image.
        let original_png =
            match d.name.as_str() {
                // A genuine static PNG wins. An animated macOS PNG may still use a
                // bounded static cover; that existing projection stays unchanged.
                "public.png" => archive.original.formats.iter().any(|f| {
                    f.item == d.item
                        && f.name == d.name
                        && f.state == FormatState::Captured
                        && image::is_static_png(&f.bytes)
                }),
                // Windows does not substitute another image for an advertised PNG,
                // even if that original is corrupt or animated.
                "PNG" => archive.original.formats.iter().any(|f| {
                    f.item == d.item && f.name == d.name && f.state == FormatState::Captured
                }),
                _ => false,
            };
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
    // WeChat/Qt advertises a deferred file URL alongside complete image bytes.
    // The validated single-image shape needs no file access. Retain the original
    // descriptor in history, but omit it from the portable image projection.
    if archive.platform == 1 && archive.enumeration_complete && qt_image::matches(&archive.original)
    {
        projection.formats.retain(|f| f.name != "public.file-url");
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
    use crate::protocol::native_archive::{
        DerivedFormat, NativeArchive, CONVERTER_WINDOWS_DIB_TO_PNG,
    };
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

    fn one_pixel_png() -> Vec<u8> {
        packed_to_png(1, 1, &[255, 0, 0, 255]).unwrap()
    }

    /// The shape QQ puts on the Windows clipboard for one image message: the
    /// bitmap, an HTML wrapper that points at the local original, and a
    /// `CF_HDROP` backing entry.
    fn qq_image_snapshot(html: &str) -> NativeSnapshot {
        NativeSnapshot {
            before: 7,
            after: 7,
            items: 1,
            state: SnapshotState::Stable,
            formats: vec![
                NativeFormat {
                    item: 0,
                    name: "CF_DIBV5".into(),
                    state: FormatState::Captured,
                    bytes: vec![0x42; 64],
                },
                NativeFormat {
                    item: 0,
                    name: "CF_HDROP".into(),
                    state: FormatState::Captured,
                    bytes: vec![0u8; 20],
                },
                NativeFormat {
                    item: 0,
                    name: "HTML Format".into(),
                    state: FormatState::Captured,
                    bytes: html::encode_cf_html(html).unwrap(),
                },
                NativeFormat {
                    item: 0,
                    name: "PNG".into(),
                    state: FormatState::Captured,
                    bytes: one_pixel_png(),
                },
            ],
        }
    }

    #[test]
    fn image_with_backing_file_projects_the_bitmap_instead_of_native_only() {
        let snapshot = qq_image_snapshot(
            r#"<html><body><img src="file:///C:\Users\me\Documents\Tencent Files\pic.png"></body></html>"#,
        );
        let bundle = from_native(&snapshot).unwrap();
        assert_eq!(bundle.completeness, Completeness::Complete);
        assert!(bundle.representation(Mime::Png).is_some());
        assert!(bundle.representation(Mime::Html).is_none());
        assert!(!bundle.text().unwrap_or_default().contains("Tencent"));
    }

    #[test]
    fn image_with_backing_file_keeps_portable_html() {
        let snapshot = qq_image_snapshot(
            r#"<html><body><img src="https://example.invalid/a.png"></body></html>"#,
        );
        let bundle = from_native(&snapshot).unwrap();
        assert!(bundle.representation(Mime::Png).is_some());
        assert!(bundle.representation(Mime::Html).is_some());
        assert_eq!(bundle.completeness, Completeness::ExternalReferences);
    }

    #[test]
    fn plain_file_selection_still_uses_the_file_pipeline() {
        let snapshot = NativeSnapshot {
            before: 3,
            after: 3,
            items: 1,
            state: SnapshotState::Stable,
            formats: vec![NativeFormat {
                item: 0,
                name: "CF_HDROP".into(),
                state: FormatState::Captured,
                bytes: vec![0u8; 20],
            }],
        };
        assert_eq!(from_native(&snapshot), Err(CaptureError::Files));
    }
    fn windows_archive(formats: Vec<NativeFormat>, derived: Vec<DerivedFormat>) -> NativeArchive {
        NativeArchive {
            platform: 2,
            enumeration_complete: false,
            original: NativeSnapshot {
                before: 11,
                after: 11,
                items: 1,
                state: SnapshotState::Stable,
                formats,
            },
            derived,
        }
    }

    fn windows_dib_archive() -> NativeArchive {
        windows_archive(
            vec![NativeFormat {
                item: 0,
                name: "CF_DIBV5".into(),
                state: FormatState::Captured,
                bytes: b"retained DIBV5 original".to_vec(),
            }],
            vec![DerivedFormat {
                item: 0,
                name: "PNG".into(),
                source_name: "CF_DIBV5".into(),
                converter: CONVERTER_WINDOWS_DIB_TO_PNG.into(),
                bytes: one_pixel_png(),
            }],
        )
    }

    #[test]
    fn wcap_dib_only_archive_keeps_original_and_admits_only_derived_png() {
        let archive = windows_dib_archive();
        let bundle =
            from_archive(&archive).expect("the explicit Windows DIB conversion is admitted");
        assert_eq!(
            bundle.representation(Mime::Png),
            Some(one_pixel_png().as_slice())
        );
        assert_eq!(archive.original.formats.len(), 1);
        assert_eq!(archive.original.formats[0].name, "CF_DIBV5");
        assert_eq!(
            archive.original.formats[0].bytes,
            b"retained DIBV5 original"
        );
        assert!(archive.derived[0].source_name == "CF_DIBV5");
    }

    #[test]
    fn wcap_dib_derived_rejects_forged_source_converter_platform_and_name() {
        let mut platform = windows_dib_archive();
        platform.platform = 1;
        assert!(from_archive(&platform).is_err());

        let mut converter = windows_dib_archive();
        converter.derived[0].converter = "example.forged-converter.v1".into();
        assert!(from_archive(&converter).is_err());

        let mut name = windows_dib_archive();
        name.derived[0].name = "public.png".into();
        assert!(from_archive(&name).is_err());

        let mut source = windows_dib_archive();
        source.derived[0].source_name = "CF_DIB".into();
        assert!(from_archive(&source).is_err());

        let mut bytes = windows_dib_archive();
        bytes.derived[0].bytes = b"not a PNG".to_vec();
        assert!(from_archive(&bytes).is_err());
    }

    #[test]
    fn wcap_archive_prefers_a_real_png_original_over_a_dib_derivative() {
        let blue = packed_to_png(1, 1, &[0, 0, 255, 255]).unwrap();
        let archive = windows_archive(
            vec![
                NativeFormat {
                    item: 0,
                    name: "CF_DIBV5".into(),
                    state: FormatState::Captured,
                    bytes: b"DIBV5 original".to_vec(),
                },
                NativeFormat {
                    item: 0,
                    name: "PNG".into(),
                    state: FormatState::Captured,
                    bytes: blue.clone(),
                },
            ],
            vec![DerivedFormat {
                item: 0,
                name: "PNG".into(),
                source_name: "CF_DIBV5".into(),
                converter: CONVERTER_WINDOWS_DIB_TO_PNG.into(),
                bytes: one_pixel_png(),
            }],
        );
        let bundle = from_archive(&archive).unwrap();
        assert_eq!(bundle.representation(Mime::Png), Some(blue.as_slice()));
    }

    #[test]
    fn wcap_corrupt_png_original_is_not_replaced_by_a_derivative() {
        let archive = windows_archive(
            vec![
                NativeFormat {
                    item: 0,
                    name: "PNG".into(),
                    state: FormatState::Captured,
                    bytes: b"broken original PNG".to_vec(),
                },
                NativeFormat {
                    item: 0,
                    name: "CF_DIBV5".into(),
                    state: FormatState::Captured,
                    bytes: b"DIBV5 original".to_vec(),
                },
            ],
            vec![DerivedFormat {
                item: 0,
                name: "PNG".into(),
                source_name: "CF_DIBV5".into(),
                converter: CONVERTER_WINDOWS_DIB_TO_PNG.into(),
                bytes: one_pixel_png(),
            }],
        );
        assert_eq!(from_archive(&archive), Err(CaptureError::NativeOnly));
    }

    #[test]
    fn wcap_dib_with_backing_file_is_an_image_not_a_file_selection() {
        let archive = windows_archive(
            vec![
                NativeFormat {
                    item: 0,
                    name: "CF_DIBV5".into(),
                    state: FormatState::Captured,
                    bytes: b"DIBV5 original".to_vec(),
                },
                NativeFormat {
                    item: 0,
                    name: "CF_HDROP".into(),
                    state: FormatState::Captured,
                    bytes: vec![0u8; 20],
                },
                NativeFormat {
                    item: 0,
                    name: "HTML Format".into(),
                    state: FormatState::Captured,
                    bytes: html::encode_cf_html(
                        r#"<html><body><!--StartFragment--><img src="file:///C:/Users/me/Tencent%20Files/nt_qq/pic.jpg"><!--EndFragment--></body></html>"#,
                    )
                    .unwrap(),
                },
            ],
            vec![DerivedFormat {
                item: 0,
                name: "PNG".into(),
                source_name: "CF_DIBV5".into(),
                converter: CONVERTER_WINDOWS_DIB_TO_PNG.into(),
                bytes: one_pixel_png(),
            }],
        );
        let bundle = from_archive(&archive).expect("the bitmap remains an image event");
        assert!(bundle.representation(Mime::Png).is_some());
        assert!(bundle.representation(Mime::Html).is_none());
        assert_eq!(bundle.completeness, Completeness::Complete);
        assert_eq!(archive.original.formats[1].name, "CF_HDROP");
        assert!(
            !fingerprint::requires_original_domain(&archive.original),
            "QQ backing-file image must classify in the portable domain so peer sync is not blocked"
        );
    }

    #[test]
    fn wcap_png_with_backing_file_is_an_image_not_a_file_selection() {
        let archive = windows_archive(
            vec![
                NativeFormat {
                    item: 0,
                    name: "PNG".into(),
                    state: FormatState::Captured,
                    bytes: one_pixel_png(),
                },
                NativeFormat {
                    item: 0,
                    name: "CF_HDROP".into(),
                    state: FormatState::Captured,
                    bytes: vec![0u8; 20],
                },
            ],
            vec![],
        );
        let bundle = from_archive(&archive).expect("the captured PNG is the image event");
        assert_eq!(
            bundle.representation(Mime::Png),
            Some(one_pixel_png().as_slice())
        );
    }

    #[test]
    fn wcap_pure_hdrop_archive_still_uses_the_file_pipeline() {
        let archive = windows_archive(
            vec![NativeFormat {
                item: 0,
                name: "CF_HDROP".into(),
                state: FormatState::Captured,
                bytes: vec![0u8; 20],
            }],
            vec![],
        );
        assert_eq!(from_archive(&archive), Err(CaptureError::Files));
    }

    #[test]
    fn wcap_dib_derived_write_does_not_echo_and_a_new_copy_is_external() {
        use crate::composite::fingerprint::{origin_fingerprint, origin_fingerprint_from_native};
        use crate::composite::origin::{Origin, OriginTracker};

        let bundle = from_archive(&windows_dib_archive()).unwrap();
        let digest = origin_fingerprint(&bundle).unwrap();
        assert_eq!(
            OriginTracker::default().classify(Some(11), digest, None),
            Origin::External,
            "a DIB-only external copy is not a local write"
        );

        let published = to_windows_snapshot(&bundle).unwrap();
        let written = origin_fingerprint_from_native(&published).unwrap();
        let mut tracker = OriginTracker::default();
        tracker.record(12, written, [9; 16]);
        assert_eq!(tracker.classify(Some(12), written, None), Origin::SelfWrite);
        assert_eq!(
            tracker.classify(Some(13), written, None),
            Origin::External,
            "a later user copy of the same pixels is still a new event"
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
    fn qq_capture_origin_accepts_deferred_descriptors_without_admitting_native_publish() {
        use super::origin::{Origin, OriginTracker};
        let a = archive();
        let original = a.encode().unwrap();
        let bundle = from_archive(&a).unwrap();
        let digest = fingerprint::origin_fingerprint_from_capture(&a.original)
            .expect("QQ descriptor-only extra items must not prevent external-copy detection");
        assert_eq!(
            OriginTracker::default().classify(Some(a.original.after), digest, None),
            Origin::External
        );
        assert!(fingerprint::origin_fingerprint_from_native(&a.original).is_err());
        assert_eq!(a.encode().unwrap(), original);

        // Receiving or restoring the converted message publishes portable formats,
        // which must still match their actual read-back and never echo to a peer.
        let published = to_macos_snapshot(&bundle).unwrap();
        let written = fingerprint::origin_fingerprint_from_native(&published).unwrap();
        let mut tracker = OriginTracker::default();
        tracker.record(42, written, [7; 16]);
        assert_eq!(
            tracker.classify(
                Some(42),
                fingerprint::origin_fingerprint_from_capture(&published).unwrap(),
                Some([7; 16])
            ),
            Origin::SelfWrite
        );
    }

    #[test]
    fn qq_capture_origin_accepts_descriptor_on_message_item() {
        use super::origin::{Origin, OriginTracker};
        let mut a = archive();
        a.original.items = 1;
        a.original.formats[1].item = 0;
        a.original.formats.push(NativeFormat {
            item: 0,
            name: "dyn.ah62d4rv4gu8y6y4grf0gn5xbrzw1gydcr7u1e3cytf2gn".into(),
            state: FormatState::Captured,
            bytes: b"QQ companion metadata".to_vec(),
        });
        a.validate().unwrap();
        assert!(from_archive(&a).is_ok());
        let digest = fingerprint::origin_fingerprint_from_capture(&a.original)
            .expect("QQ advertises its unread file URL on the message item too");
        assert_eq!(
            OriginTracker::default().classify(Some(a.original.after), digest, None),
            Origin::External
        );
        assert!(fingerprint::origin_fingerprint_from_native(&a.original).is_err());
        for state in [
            FormatState::Failed,
            FormatState::TooLarge,
            FormatState::Captured,
        ] {
            let mut invalid = a.original.clone();
            invalid.formats[1].state = state;
            assert!(fingerprint::origin_fingerprint_from_capture(&invalid).is_err());
        }
        let mut invalid = a.original.clone();
        invalid.formats[1].name = "public.png".into();
        assert!(fingerprint::origin_fingerprint_from_capture(&invalid).is_err());
        let mut invalid = a.original.clone();
        invalid.formats[0].name = "com.example.not-qq".into();
        assert!(fingerprint::origin_fingerprint_from_capture(&invalid).is_err());
    }

    #[test]
    fn qq_capture_origin_preserves_mismatches_and_unresolved_writes() {
        use super::origin::{Origin, OriginTracker};
        let a = archive();
        let digest = fingerprint::origin_fingerprint_from_capture(&a.original).unwrap();
        let mut tracker = OriginTracker::default();
        tracker.record(1, [0; 32], [7; 16]);
        assert_eq!(
            tracker.classify(Some(1), digest, None),
            Origin::UnknownMismatch
        );
        let mut tracker = OriginTracker::default();
        assert!(tracker.record_intent([8; 16], digest));
        assert_eq!(
            tracker.classify(Some(1), digest, None),
            Origin::UnknownMismatch
        );
    }

    #[test]
    fn qq_capture_origin_binds_original_bytes_and_descriptors() {
        let a = archive();
        let digest = fingerprint::origin_fingerprint_from_capture(&a.original).unwrap();
        let mut reordered = a.original.clone();
        reordered.before = 100;
        reordered.after = 100;
        reordered.formats.reverse();
        assert_eq!(
            fingerprint::origin_fingerprint_from_capture(&reordered).unwrap(),
            digest
        );
        let mut changed = a.original.clone();
        changed.formats[0].bytes.push(1);
        assert_ne!(
            fingerprint::origin_fingerprint_from_capture(&changed).unwrap(),
            digest
        );
        let mut changed = a.original.clone();
        changed.items += 1;
        changed.formats.push(NativeFormat {
            item: 2,
            name: "public.file-url".into(),
            state: FormatState::Absent,
            bytes: vec![],
        });
        assert_ne!(
            fingerprint::origin_fingerprint_from_capture(&changed).unwrap(),
            digest
        );
    }

    #[test]
    fn qq_capture_origin_does_not_admit_other_missing_or_failed_formats() {
        for state in [
            FormatState::Failed,
            FormatState::TooLarge,
            FormatState::Captured,
        ] {
            let mut a = archive();
            a.original.formats[1].state = state;
            assert!(fingerprint::origin_fingerprint_from_capture(&a.original).is_err());
        }
        for name in [
            "public.utf8-plain-text",
            "public.png",
            "com.example.private",
        ] {
            let mut a = archive();
            a.original.formats[1].name = name.into();
            assert!(fingerprint::origin_fingerprint_from_capture(&a.original).is_err());
        }
        let mut a = archive();
        a.original.formats[0].name = "com.example.not-qq".into();
        assert!(fingerprint::origin_fingerprint_from_capture(&a.original).is_err());
        let mut a = archive();
        a.original.formats[0].state = FormatState::Failed;
        a.original.formats[0].bytes.clear();
        assert!(fingerprint::origin_fingerprint_from_capture(&a.original).is_err());
        let mut a = archive();
        a.original.after += 1;
        assert!(fingerprint::origin_fingerprint_from_capture(&a.original).is_err());
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
