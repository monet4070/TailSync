//! Content-free capture provenance (repair plan §4 R5-06).
//!
//! R5-06 asks for the *relation* between the formats a real application puts on the
//! clipboard — which HTML nodes carry images, whether a standalone screenshot
//! coexists, whether RTF/RTFD attachments are present — so that resource binding can be
//! judged from evidence instead of assumption, and so that "a standalone screenshot must
//! not be attached to an arbitrary `<img>`" is a property someone can check rather than
//! a claim someone made.
//!
//! The plan is equally explicit that this evidence **must not store private content**.
//! This type therefore carries only shapes and counts: format *names*, node counts,
//! booleans, and the resulting completeness. No bytes, no offsets, no text.

use super::{Completeness, Mime, PortableBundle};
use crate::protocol::native_clipboard::{FormatState, NativeSnapshot};
use serde::{Deserialize, Serialize};

/// The shape of one capture, with no content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureProvenance {
    /// Format names present on the pasteboard, sorted and deduplicated.
    pub formats: Vec<String>,
    /// Format names that were present but not captured.
    pub uncaptured_formats: Vec<String>,
    /// How many `<img src="data:image/png;base64,…">` nodes the HTML carried. This is
    /// the only thing that may create a resource binding.
    pub inline_html_images: u16,
    /// Whether the HTML referenced images the capture could not inline.
    pub html_external_references: bool,
    /// Whether a standalone image representation coexisted with the HTML. Such an image
    /// is a representation, never a binding.
    pub standalone_image: bool,
    /// How many byte resources were bound to HTML nodes.
    pub bound_resources: u16,
    /// Whether an RTFD attachment was present (the native-only shape).
    pub rtfd_attachment: bool,
    /// Resulting completeness, as a stable name.
    pub completeness: &'static str,
}

fn completeness_name(completeness: Completeness) -> &'static str {
    match completeness {
        Completeness::Complete => "complete",
        Completeness::ExternalReferences => "external_references",
        Completeness::MissingFormats => "missing_formats",
    }
}

impl CaptureProvenance {
    /// Describe a raw pasteboard read, before assembly.
    /// Executable specification for future preview work; capture diagnostics use from_bundle.
    pub fn from_snapshot(snapshot: &NativeSnapshot) -> Self {
        let mut formats: Vec<String> = snapshot
            .formats
            .iter()
            .map(|format| format.name.clone())
            .collect();
        formats.sort();
        formats.dedup();
        let mut uncaptured: Vec<String> = snapshot
            .formats
            .iter()
            .filter(|format| format.state != FormatState::Captured)
            .map(|format| format.name.clone())
            .collect();
        uncaptured.sort();
        uncaptured.dedup();
        let has = |name: &str| formats.iter().any(|format| format == name);
        let rtfd_attachment = has("com.apple.flat-rtfd");
        let standalone_image = has("public.png") || has("PNG");
        Self {
            rtfd_attachment,
            formats,
            uncaptured_formats: uncaptured,
            // The raw read says nothing about HTML internals; that is the bundle's job.
            inline_html_images: 0,
            html_external_references: false,
            standalone_image,
            bound_resources: 0,
            completeness: "unknown",
        }
    }

    /// Describe an assembled bundle, after resource extraction.
    pub fn from_bundle(bundle: &PortableBundle) -> Self {
        let formats: Vec<String> = [Mime::Text, Mime::Html, Mime::Rtf, Mime::Png]
            .into_iter()
            .filter(|mime| bundle.representation(*mime).is_some())
            .map(|mime| {
                match mime {
                    Mime::Text => "text",
                    Mime::Html => "html",
                    Mime::Rtf => "rtf",
                    Mime::Png => "png",
                }
                .to_string()
            })
            .collect();
        let inline_html_images = bundle
            .representations
            .iter()
            .position(|representation| representation.mime == Mime::Html)
            .map(|index| {
                bundle
                    .bindings
                    .iter()
                    .filter(|binding| binding.representation as usize == index)
                    .count() as u16
            })
            .unwrap_or(0);
        Self {
            formats,
            uncaptured_formats: Vec::new(),
            inline_html_images,
            html_external_references: bundle.completeness == Completeness::ExternalReferences,
            standalone_image: bundle.representation(Mime::Png).is_some(),
            bound_resources: bundle.resources.len() as u16,
            rtfd_attachment: false,
            completeness: completeness_name(bundle.completeness),
        }
    }

    /// The property R5-06 exists to protect: a standalone image is a representation, and
    /// is never attached to an HTML node.
    ///
    /// It holds when every bound resource was created by an inline HTML image — i.e.
    /// the bundle has no binding that a standalone screenshot could have produced.
    /// Diagnostic/test predicate; it does not currently select the UI thumbnail.
    pub fn standalone_image_is_unbound(&self) -> bool {
        !self.standalone_image || self.bound_resources <= self.inline_html_images
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composite::{Completeness, Mime, PortableBundle, Representation};

    fn html_bundle(html: &str) -> PortableBundle {
        PortableBundle {
            representations: vec![Representation {
                mime: Mime::Html,
                bytes: html.as_bytes().to_vec(),
            }],
            resources: Vec::new(),
            bindings: Vec::new(),
            completeness: Completeness::Complete,
        }
    }

    /// A standalone image next to HTML becomes a representation, never a binding.
    #[test]
    fn a_standalone_image_is_never_bound_to_an_html_node() {
        let mut bundle = html_bundle("<p>no images here</p>");
        bundle.representations.push(Representation {
            mime: Mime::Png,
            bytes: crate::composite::packed_to_png(1, 1, &[1, 2, 3, 255]).unwrap(),
        });
        crate::composite::html::extract_resources(&mut bundle).unwrap();

        assert!(
            bundle.bindings.is_empty(),
            "HTML with no inline image must gain no binding from a standalone screenshot"
        );
        let provenance = CaptureProvenance::from_bundle(&bundle);
        assert!(provenance.standalone_image);
        assert_eq!(provenance.inline_html_images, 0);
        assert_eq!(provenance.bound_resources, 0);
        assert!(provenance.standalone_image_is_unbound());
    }

    /// A credible binding — an inline data-URI image at an exact node — is the only
    /// thing that creates a resource.
    #[test]
    fn only_an_inline_html_image_creates_a_binding() {
        let png = crate::composite::packed_to_png(1, 1, &[9, 8, 7, 255]).unwrap();
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        let html = format!(
            "<p>x</p><img src=\"data:image/png;base64,{}\">",
            STANDARD.encode(&png)
        );
        let mut bundle = html_bundle(&html);
        crate::composite::html::extract_resources(&mut bundle).unwrap();

        assert_eq!(bundle.bindings.len(), 1, "one inline image, one binding");
        assert_eq!(bundle.resources.len(), 1);
        let provenance = CaptureProvenance::from_bundle(&bundle);
        assert_eq!(provenance.inline_html_images, 1);
        assert_eq!(provenance.bound_resources, 1);
        assert!(!provenance.standalone_image);
        assert!(provenance.standalone_image_is_unbound());
    }

    /// The provenance record must not carry private content — that is the whole point of
    /// keeping only shapes.
    #[test]
    fn provenance_never_carries_captured_content() {
        const SECRET: &str = "tailsync-private-marker-9f3a";
        let html = format!("<p>{SECRET}</p>");
        let bundle = html_bundle(&html);
        let provenance = CaptureProvenance::from_bundle(&bundle);
        let serialized = serde_json::to_string(&provenance).unwrap();
        assert!(
            !serialized.contains(SECRET),
            "the provenance record leaked captured content: {serialized}"
        );
        // Stronger than "the marker is absent": *every* string in the record must be a
        // known shape name, so no captured text can hide in a field this test forgot.
        let value: serde_json::Value = serde_json::from_str(&serialized).unwrap();
        let allowed = [
            "html",
            "text",
            "rtf",
            "png",
            "complete",
            "external_references",
            "missing_formats",
            "unknown",
        ];
        let mut seen = Vec::new();
        fn collect(value: &serde_json::Value, out: &mut Vec<String>) {
            match value {
                serde_json::Value::String(text) => out.push(text.clone()),
                serde_json::Value::Array(items) => items.iter().for_each(|i| collect(i, out)),
                serde_json::Value::Object(map) => map.values().for_each(|v| collect(v, out)),
                _ => {}
            }
        }
        collect(&value, &mut seen);
        for text in &seen {
            assert!(
                allowed.contains(&text.as_str()),
                "the provenance record contains a string that is not a known shape name: {text}"
            );
        }
        assert!(
            seen.contains(&"html".to_string()),
            "the format name is still recorded"
        );
    }

    /// A raw read records the shapes it can see without claiming to know the HTML
    /// internals it has not parsed yet.
    #[test]
    fn a_raw_read_records_only_what_it_can_see() {
        use crate::protocol::native_clipboard::{FormatState, NativeFormat, SnapshotState};
        let snapshot = NativeSnapshot {
            before: 1,
            after: 1,
            items: 1,
            state: SnapshotState::Stable,
            formats: vec![
                NativeFormat {
                    item: 0,
                    name: "public.html".into(),
                    state: FormatState::Captured,
                    bytes: vec![1],
                },
                NativeFormat {
                    item: 0,
                    name: "public.png".into(),
                    state: FormatState::TooLarge,
                    bytes: Vec::new(),
                },
                NativeFormat {
                    item: 0,
                    name: "com.apple.flat-rtfd".into(),
                    state: FormatState::Captured,
                    bytes: vec![2],
                },
            ],
        };
        let provenance = CaptureProvenance::from_snapshot(&snapshot);
        assert_eq!(
            provenance.formats,
            ["com.apple.flat-rtfd", "public.html", "public.png"]
        );
        assert_eq!(provenance.uncaptured_formats, ["public.png"]);
        assert!(provenance.standalone_image);
        assert!(provenance.rtfd_attachment);
        // It does not pretend to know the HTML internals before parsing.
        assert_eq!(provenance.inline_html_images, 0);
        assert_eq!(provenance.bound_resources, 0);
    }
}
