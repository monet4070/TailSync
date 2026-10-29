//! Versioned, owned preparation for one local history publication. No OS,
//! filesystem or network I/O occurs here. Every output is inline: publication
//! never depends on a temporary file that history GC could remove.
use super::{Completeness, Mime, PortableBundle};
use crate::protocol::{native_clipboard::*, ProtocolError};

pub const RESTORE_PLAN_VERSION: u8 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreTarget {
    /// Compatibility seam for adapters without a native format contract.
    Adapter,
    Macos,
    Windows,
}
impl RestoreTarget {
    pub fn archive_platform(self) -> Option<u8> {
        match self {
            Self::Adapter => None,
            Self::Macos => Some(1),
            Self::Windows => Some(2),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreSelection {
    Originals,
    PortableCompatibility,
    DocumentCompatibility,
    ExplicitFormat,
    AvailableContent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreInput {
    pub source_platform: Option<u8>,
    pub requested_format: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreOutput {
    pub item: u16,
    pub name: String,
    pub bytes: usize,
    pub digest: [u8; 32],
}

/// Cheap facts shared by manifest menus and full payload preparation. HTML
/// reference closure and image decoding remain separate, stricter gates.
pub struct RestoreFormatInventory {
    pub native_only: bool,
    pub completeness: Completeness,
    pub standalone_png: bool,
    pub resource_count: usize,
}
impl RestoreFormatInventory {
    pub fn withheld_reason(&self, mime: Mime) -> Option<&'static str> {
        if self.native_only {
            Some("native_only")
        } else if mime == Mime::Html && self.completeness != Completeness::Complete {
            Some("resources_incomplete")
        } else if mime == Mime::Png && !self.standalone_png && self.resource_count > 1 {
            Some("no_single_image")
        } else {
            None
        }
    }
}
#[derive(Debug)]
enum PreparedOutput {
    Native(NativeSnapshot),
    Portable(PortableBundle),
}

/// Fields and bytes are immutable once preparation succeeds. The same owned
/// value crosses the FIFO/worker boundary; callers cannot alter the output list
/// between inspection and the single native commit.
#[derive(Debug)]
pub struct RestorePlan {
    input: RestoreInput,
    target: RestoreTarget,
    selection: RestoreSelection,
    prepared: PreparedOutput,
    missing_images: u16,
}
impl RestorePlan {
    pub fn version(&self) -> u8 {
        RESTORE_PLAN_VERSION
    }
    pub fn input(&self) -> &RestoreInput {
        &self.input
    }
    pub fn target(&self) -> RestoreTarget {
        self.target
    }
    pub fn selection(&self) -> RestoreSelection {
        self.selection
    }
    pub fn missing_images(&self) -> u16 {
        self.missing_images
    }
    pub fn with_macos_rtfd(self, bytes: Vec<u8>) -> Result<Self, ProtocolError> {
        if self.target != RestoreTarget::Macos
            || !matches!(
                self.selection,
                RestoreSelection::AvailableContent | RestoreSelection::DocumentCompatibility
            )
            || bytes.is_empty()
        {
            return Err(ProtocolError::InvalidNativeSnapshot);
        }
        let PreparedOutput::Native(mut snapshot) = self.prepared else {
            return Err(ProtocolError::InvalidNativeSnapshot);
        };
        snapshot
            .formats
            .retain(|format| format.name != "public.rtf");
        snapshot.formats.push(NativeFormat {
            item: 0,
            name: "com.apple.flat-rtfd".into(),
            state: FormatState::Captured,
            bytes,
        });
        let mut prepared = Self::native_plan(snapshot, self.input, self.target, self.selection)?;
        prepared.missing_images = self.missing_images;
        Ok(prepared)
    }
    pub fn available_content(
        bundle: &PortableBundle,
        input: RestoreInput,
        target: RestoreTarget,
    ) -> Result<Self, ProtocolError> {
        let mut available = super::available_restore::prepare(bundle)
            .or_else(|_| super::readable_restore::prepare(bundle))?;
        if let Ok(bytes) = super::readable_restore::rtf(&available.bundle) {
            available
                .bundle
                .representations
                .push(super::Representation {
                    mime: Mime::Rtf,
                    bytes,
                });
        }
        let mut plan = Self::compatibility(available.bundle, input, target)?;
        plan.selection = RestoreSelection::AvailableContent;
        plan.missing_images = available.missing_images;
        Ok(plan)
    }
    /// Select a self-contained portable output for default publication. Original
    /// bytes remain in history; unavailable images stay at their document positions
    /// in the selected copy. An explicit format request never silently degrades.
    pub fn portable_default(
        bundle: &PortableBundle,
        input: RestoreInput,
        target: RestoreTarget,
    ) -> Result<Self, ProtocolError> {
        if input.requested_format.is_none() && !dependencies_complete(bundle)? {
            return Self::available_content(bundle, input, target);
        }
        if input.requested_format.is_none() && prefers_document_compatibility(bundle)? {
            // An original RTF representation can contain only the text while HTML
            // carries the images. Native consumers often prefer RTF, so publish
            // one coherent document instead of letting that alternative hide images.
            let mut document = bundle.clone();
            if let Some(index) = document
                .representations
                .iter()
                .position(|r| r.mime == Mime::Rtf)
            {
                document.representations.remove(index);
                for binding in &mut document.bindings {
                    if binding.representation as usize > index {
                        binding.representation -= 1;
                    }
                }
            }
            if target != RestoreTarget::Macos {
                if let Ok(bytes) = super::readable_restore::rtf(&document) {
                    document.representations.push(super::Representation {
                        mime: Mime::Rtf,
                        bytes,
                    });
                }
            }
            // Keep the captured HTML and its image positions. macOS adds a
            // native RTFD representation from this same HTML in its adapter.
            let mut plan = Self::compatibility(document, input, target)?;
            plan.selection = RestoreSelection::DocumentCompatibility;
            return Ok(plan);
        }
        Self::compatibility(bundle.clone(), input, target)
    }
    /// Diagnostic inventory, computed only when inspected. Publication uses the
    /// owned prepared bytes directly and does not allocate or hash this inventory.
    pub fn outputs(&self) -> Vec<RestoreOutput> {
        match &self.prepared {
            PreparedOutput::Native(s) => s
                .formats
                .iter()
                .map(|f| RestoreOutput {
                    item: f.item,
                    name: f.name.clone(),
                    bytes: f.bytes.len(),
                    digest: *blake3::hash(&f.bytes).as_bytes(),
                })
                .collect(),
            PreparedOutput::Portable(b) => b
                .representations
                .iter()
                .map(|r| RestoreOutput {
                    item: 0,
                    name: format!("{:?}", r.mime),
                    bytes: r.bytes.len(),
                    digest: *blake3::hash(&r.bytes).as_bytes(),
                })
                .collect(),
        }
    }
    pub fn native(&self) -> Option<&NativeSnapshot> {
        match &self.prepared {
            PreparedOutput::Native(s) => Some(s),
            _ => None,
        }
    }
    pub fn portable(&self) -> Option<&PortableBundle> {
        match &self.prepared {
            PreparedOutput::Portable(b) => Some(b),
            _ => None,
        }
    }

    pub fn originals(
        mut snapshot: NativeSnapshot,
        input: RestoreInput,
        target: RestoreTarget,
    ) -> Result<Self, ProtocolError> {
        snapshot
            .formats
            .retain(|f| !crate::protocol::native_policy::control(&f.name));
        snapshot.validate()?;
        if snapshot.state != SnapshotState::Stable
            || (0..snapshot.items).any(|item| !snapshot.formats.iter().any(|f| f.item == item))
            || snapshot
                .formats
                .iter()
                .any(|f| f.state != FormatState::Captured)
        {
            return Err(ProtocolError::InvalidNativeSnapshot);
        }
        if native_needs_materialization(&snapshot)? {
            return Err(ProtocolError::InvalidComposite);
        }
        // Target byte admission is also enforced by the adapter/helper. An
        // unrelated platform can never replay an archive merely because names fit.
        if input
            .source_platform
            .zip(target.archive_platform())
            .is_some_and(|(a, b)| a != b)
        {
            return Err(ProtocolError::InvalidNativeSnapshot);
        }
        Self::native_plan(snapshot, input, target, RestoreSelection::Originals)
    }
    pub fn compatibility(
        bundle: PortableBundle,
        input: RestoreInput,
        target: RestoreTarget,
    ) -> Result<Self, ProtocolError> {
        super::validate_for_publish(&bundle)?;
        if !dependencies_complete(&bundle)? {
            return Err(ProtocolError::InvalidComposite);
        }
        let selection = if input.requested_format.is_some() {
            RestoreSelection::ExplicitFormat
        } else {
            RestoreSelection::PortableCompatibility
        };
        match target {
            RestoreTarget::Macos => {
                Self::native_plan(super::to_macos_snapshot(&bundle)?, input, target, selection)
            }
            RestoreTarget::Windows => Self::native_plan(
                super::to_windows_snapshot(&bundle)?,
                input,
                target,
                selection,
            ),
            RestoreTarget::Adapter => Ok(Self {
                input,
                target,
                selection,
                prepared: PreparedOutput::Portable(bundle),
                missing_images: 0,
            }),
        }
    }
    fn native_plan(
        mut snapshot: NativeSnapshot,
        input: RestoreInput,
        target: RestoreTarget,
        selection: RestoreSelection,
    ) -> Result<Self, ProtocolError> {
        snapshot.validate()?;
        if snapshot.state != SnapshotState::Stable
            || snapshot.formats.iter().any(|f| {
                f.state != FormatState::Captured
                    || crate::protocol::native_policy::suppressed(&f.name)
            })
        {
            return Err(ProtocolError::InvalidNativeSnapshot);
        }
        if target == RestoreTarget::Windows {
            for f in &mut snapshot.formats {
                if matches!(f.name.as_str(), "CF_DIB" | "CF_DIBV5") {
                    crate::protocol::bitmap::decode_dib(&f.bytes)?;
                }
                if f.name == "CF_UNICODETEXT"
                    && (f.bytes.len() % 2 != 0
                        || !f.bytes.ends_with(&[0, 0])
                        || f.bytes.len() > 2 * 1024 * 1024 + 2)
                {
                    return Err(ProtocolError::InvalidNativeSnapshot);
                }
                if matches!(f.name.as_str(), "HTML Format" | "Rich Text Format")
                    && f.bytes.last() != Some(&0)
                {
                    f.bytes.push(0);
                }
            }
            // Prepare the helper's existing PNG -> DIBV5 combination for both
            // original and portable selections. No helper-only format or byte
            // normalization may be absent from the inspected output inventory.
            if !snapshot.formats.iter().any(|f| f.name == "CF_DIBV5") {
                if let Some(png) = snapshot.formats.iter().find(|f| f.name == "PNG") {
                    let packed = super::png_to_packed(&png.bytes)?;
                    let dib = crate::protocol::bitmap::encode_dib_v5(
                        crate::protocol::PackedImage::try_from(packed.as_slice())?,
                    )?;
                    snapshot.formats.push(NativeFormat {
                        item: 0,
                        name: "CF_DIBV5".into(),
                        state: FormatState::Captured,
                        bytes: dib,
                    });
                }
            }
            snapshot.validate()?;
        }
        if target == RestoreTarget::Windows
            && (snapshot.items != 1
                || snapshot
                    .formats
                    .iter()
                    .any(|f| !crate::protocol::native_policy::windows_publishable(&f.name)))
        {
            return Err(ProtocolError::InvalidNativeSnapshot);
        }
        if target == RestoreTarget::Macos
            && snapshot
                .formats
                .iter()
                .any(|f| !crate::protocol::native_policy::macos_publishable(&f.name, f.bytes.len()))
        {
            return Err(ProtocolError::InvalidNativeSnapshot);
        }
        Ok(Self {
            input,
            target,
            selection,
            prepared: PreparedOutput::Native(snapshot),
            missing_images: 0,
        })
    }
}

pub fn prefers_document_compatibility(bundle: &PortableBundle) -> Result<bool, ProtocolError> {
    let Some(html) = bundle.representation(Mime::Html) else {
        return Ok(false);
    };
    let images = super::html::available_image_inventory(html)?;
    Ok(!images.is_empty()
        && (images.len() > 1 || !super::html::text_summary(html).trim().is_empty()))
}

/// A wire completeness label alone is not evidence of closed HTML dependencies.
/// Resource bindings and actual reference spans are checked without loading any
/// resource. Image decoding/size checks belong to validate_for_publish.
pub fn dependencies_complete(bundle: &PortableBundle) -> Result<bool, ProtocolError> {
    bundle.validate()?;
    if bundle.completeness != Completeness::Complete {
        return Ok(false);
    }
    for (index, r) in bundle.representations.iter().enumerate() {
        if r.mime == Mime::Html && !super::html::restore_dependencies_complete(bundle, index)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Known external dependencies must be materialized before restoring a native
/// HTML set. Opaque byte support does not establish semantic closure for unknown
/// application types; those retain the explicit byte-replay boundary of O02.
pub fn native_needs_materialization(snapshot: &NativeSnapshot) -> Result<bool, ProtocolError> {
    for f in &snapshot.formats {
        let html = match f.name.as_str() {
            "public.html" => Some(f.bytes.clone()),
            "HTML Format" => Some(super::html::decode_cf_html(&f.bytes)?.into_bytes()),
            _ => None,
        };
        if let Some(bytes) = html {
            let b = PortableBundle {
                representations: vec![super::Representation {
                    mime: Mime::Html,
                    bytes,
                }],
                resources: vec![],
                bindings: vec![],
                completeness: Completeness::Complete,
            };
            if !dependencies_complete(&b)? {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composite::{packed_to_png, Representation, Resource, ResourceBinding};
    fn input() -> RestoreInput {
        RestoreInput {
            source_platform: None,
            requested_format: None,
        }
    }
    fn rich() -> PortableBundle {
        let bytes = "<p>中文 🙂</p><img src='tailsync-resource:1'><img src='tailsync-resource:2'>"
            .as_bytes()
            .to_vec();
        let html = std::str::from_utf8(&bytes).unwrap();
        let bindings = (1..=2)
            .map(|id| {
                let uri = format!("tailsync-resource:{id}");
                ResourceBinding {
                    representation: 1,
                    resource: id,
                    offset: html.find(&uri).unwrap() as u32,
                    length: uri.len() as u32,
                }
            })
            .collect();
        PortableBundle {
            representations: vec![
                Representation {
                    mime: Mime::Text,
                    bytes: "中文 🙂".as_bytes().to_vec(),
                },
                Representation {
                    mime: Mime::Html,
                    bytes,
                },
                Representation {
                    mime: Mime::Png,
                    bytes: packed_to_png(1, 1, &[255, 0, 0, 128]).unwrap(),
                },
            ],
            resources: vec![
                Resource {
                    id: 1,
                    png: packed_to_png(1, 1, &[255, 0, 0, 128]).unwrap(),
                },
                Resource {
                    id: 2,
                    png: packed_to_png(1, 1, &[0, 255, 0, 255]).unwrap(),
                },
            ],
            bindings,
            completeness: Completeness::Complete,
        }
    }
    #[test]
    fn o06_windows_plan_prepares_cf_html_utf16_and_dibv5_before_any_publication() {
        let b = rich();
        let plan = RestorePlan::compatibility(b.clone(), input(), RestoreTarget::Windows).unwrap();
        assert_eq!(plan.version(), 1);
        assert_eq!(plan.selection(), RestoreSelection::PortableCompatibility);
        assert_eq!(
            plan.outputs()
                .iter()
                .map(|o| o.name.as_str())
                .collect::<Vec<_>>(),
            vec!["CF_UNICODETEXT", "HTML Format", "PNG", "CF_DIBV5"]
        );
        let s = plan.native().unwrap();
        let html = &s.formats[1].bytes;
        let decoded = super::super::html::decode_cf_html(html).unwrap();
        assert!(decoded.contains("中文 🙂"));
        assert_eq!(decoded.matches("data:image/png;base64,").count(), 2);
        assert!(!decoded.contains("tailsync-resource:"));
        let dib = crate::protocol::bitmap::decode_dib(&s.formats[3].bytes).unwrap();
        assert_eq!(&dib[8..], &[255, 0, 0, 128]);
        assert!(s.formats[0].bytes.ends_with(&[0, 0]));
        for (output, format) in plan.outputs().iter().zip(&s.formats) {
            assert_eq!(output.bytes, format.bytes.len());
            assert_eq!(output.digest, *blake3::hash(&format.bytes).as_bytes());
        }
        assert_eq!(
            b,
            rich(),
            "preparation cannot mutate source bindings or bytes"
        );
    }
    #[test]
    fn article_default_keeps_html_positions_when_text_only_rtf_competes() {
        let mut bundle = rich();
        bundle.representations.insert(
            0,
            Representation {
                mime: Mime::Rtf,
                bytes: br"{\rtf1 text without attachments}".to_vec(),
            },
        );
        for binding in &mut bundle.bindings {
            binding.representation += 1;
        }
        let original = bundle.clone();
        let mut expected = bundle.clone();
        expected.representations.retain(|r| r.mime != Mime::Rtf);
        for binding in &mut expected.bindings {
            binding.representation -= 1;
        }
        let expected = RestorePlan::compatibility(expected, input(), RestoreTarget::Macos).unwrap();
        let plan = RestorePlan::portable_default(&bundle, input(), RestoreTarget::Macos).unwrap();
        assert_eq!(plan.selection(), RestoreSelection::DocumentCompatibility);
        assert_eq!(plan.missing_images(), 0);
        assert_eq!(
            plan.native(),
            expected.native(),
            "HTML and every inline binding must stay in place"
        );
        assert!(!plan.outputs().iter().any(|o| o.name == "public.rtf"));
        let windows =
            RestorePlan::portable_default(&bundle, input(), RestoreTarget::Windows).unwrap();
        let rtf = windows
            .native()
            .unwrap()
            .formats
            .iter()
            .find(|f| f.name == "Rich Text Format")
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&rtf.bytes)
                .matches("\\pict")
                .count(),
            2
        );
        assert_eq!(
            bundle, original,
            "the stored alternatives must remain intact"
        );
    }
    #[test]
    fn o06_bound_images_do_not_prove_neighboring_external_dependencies_closed() {
        for extra in [
            " srcset='https://fixture.invalid/a.png 2x'",
            " style='background:url(file:///fixture)'",
            " poster='blob:fixture'",
        ] {
            let mut b = rich();
            let text = String::from_utf8(b.representations[1].bytes.clone()).unwrap();
            let at = text.rfind("><img").unwrap();
            b.representations[1].bytes =
                format!("{}{}{}", &text[..at], extra, &text[at..]).into_bytes();
            b.bindings[1].offset += extra.len() as u32;
            assert!(b.validate().is_ok());
            assert!(!dependencies_complete(&b).unwrap());
            assert!(RestorePlan::compatibility(b, input(), RestoreTarget::Macos).is_err());
        }
    }
    #[test]
    fn o06_original_plan_keeps_boundaries_and_never_accepts_foreign_originals() {
        let snapshot = NativeSnapshot {
            before: 9,
            after: 9,
            items: 2,
            state: SnapshotState::Stable,
            formats: vec![
                NativeFormat {
                    item: 0,
                    name: "org.tailsync.fixture.opaque".into(),
                    bytes: vec![0, 255],
                    state: FormatState::Captured,
                },
                NativeFormat {
                    item: 1,
                    name: "public.utf8-plain-text".into(),
                    bytes: b"item 2".to_vec(),
                    state: FormatState::Captured,
                },
                NativeFormat {
                    item: 0,
                    name: "org.tailsync.clipboard-source".into(),
                    bytes: vec![1; 16],
                    state: FormatState::Captured,
                },
            ],
        };
        let mut i = input();
        i.source_platform = Some(1);
        let p = RestorePlan::originals(snapshot.clone(), i.clone(), RestoreTarget::Macos).unwrap();
        assert_eq!(p.selection(), RestoreSelection::Originals);
        assert_eq!(p.native().unwrap().items, 2);
        assert_eq!(p.outputs().len(), 2);
        assert_eq!(p.outputs()[1].item, 1);
        assert!(RestorePlan::originals(snapshot, i, RestoreTarget::Windows).is_err());
    }
    #[test]
    fn o06_invalid_image_or_unbound_resource_is_a_preparation_failure() {
        let mut b = rich();
        b.resources[0].png = vec![0, 1, 2];
        assert!(RestorePlan::compatibility(b, input(), RestoreTarget::Macos).is_err());
        let mut b = rich();
        b.bindings.pop();
        assert!(RestorePlan::compatibility(b, input(), RestoreTarget::Macos).is_err());
        let mut b = rich();
        b.representations[1].bytes = b"<img src='data:image/png;base64,AAAA'>".to_vec();
        b.bindings.clear();
        b.resources.clear();
        assert!(!dependencies_complete(&b).unwrap());
    }

    #[test]
    fn o06_windows_original_inventory_includes_helper_framing_and_bitmap() {
        let s = NativeSnapshot {
            before: 0,
            after: 0,
            items: 1,
            state: SnapshotState::Stable,
            formats: vec![
                NativeFormat {
                    item: 0,
                    name: "PNG".into(),
                    state: FormatState::Captured,
                    bytes: packed_to_png(1, 1, &[1, 2, 3, 128]).unwrap(),
                },
                NativeFormat {
                    item: 0,
                    name: "Rich Text Format".into(),
                    state: FormatState::Captured,
                    bytes: b"{\\rtf1 original}".to_vec(),
                },
            ],
        };
        let mut i = input();
        i.source_platform = Some(2);
        let p = RestorePlan::originals(s.clone(), i.clone(), RestoreTarget::Windows).unwrap();
        assert_eq!(p.outputs().len(), 3);
        assert!(p.native().unwrap().formats[1].bytes.ends_with(&[0]));
        assert_eq!(p.outputs()[2].name, "CF_DIBV5");
        assert!(
            !s.formats[1].bytes.ends_with(&[0]),
            "input archive remains exact"
        );
        let mut invalid = s;
        invalid.formats[0].name = "CF_UNICODETEXT".into();
        invalid.formats[0].bytes = vec![1, 2, 3];
        assert!(RestorePlan::originals(invalid, i, RestoreTarget::Windows).is_err());
    }

    #[test]
    fn o06_original_constructor_refuses_known_external_dependencies() {
        let s = NativeSnapshot {
            before: 0,
            after: 0,
            items: 1,
            state: SnapshotState::Stable,
            formats: vec![NativeFormat {
                item: 0,
                name: "public.html".into(),
                state: FormatState::Captured,
                bytes: b"<img src='https://fixture.invalid/missing.png'>".to_vec(),
            }],
        };
        assert!(RestorePlan::originals(s, input(), RestoreTarget::Macos).is_err());
    }

    #[test]
    fn readable_restore_removes_page_dependencies_and_embeds_saved_picture_in_rtf() {
        let html = "<link rel='stylesheet' href='https://example.com/page.css'><div style='background-image:url(https://example.com/background);color:red'><strong>中文 🙂</strong></div><img src='tailsync-resource:7'><img src='https://example.com/missing'>";
        let bundle = PortableBundle {
            representations: vec![super::super::Representation {
                mime: Mime::Html,
                bytes: html.as_bytes().to_vec(),
            }],
            resources: vec![super::super::Resource {
                id: 7,
                png: super::super::packed_to_png(1, 1, &[255, 0, 0, 255]).unwrap(),
            }],
            bindings: vec![super::super::ResourceBinding {
                representation: 0,
                resource: 7,
                offset: html.find("tailsync-resource:7").unwrap() as u32,
                length: "tailsync-resource:7".len() as u32,
            }],
            completeness: Completeness::ExternalReferences,
        };
        let before = bundle.encode().unwrap();
        for target in [RestoreTarget::Macos, RestoreTarget::Windows] {
            let plan = RestorePlan::available_content(&bundle, input(), target).unwrap();
            assert_eq!(plan.missing_images(), 1);
            let snapshot = plan.native().unwrap();
            let html_format = snapshot
                .formats
                .iter()
                .find(|format| matches!(format.name.as_str(), "public.html" | "HTML Format"))
                .unwrap();
            let output = if target == RestoreTarget::Windows {
                super::super::html::decode_cf_html(&html_format.bytes).unwrap()
            } else {
                String::from_utf8(html_format.bytes.clone()).unwrap()
            };
            assert!(output.contains("中文 🙂"));
            assert!(output.contains("data:image/png;base64,"));
            assert!(!output.contains("example.com"));
            assert!(!output.contains("background-image"));
            let rtf = snapshot
                .formats
                .iter()
                .find(|format| matches!(format.name.as_str(), "public.rtf" | "Rich Text Format"))
                .unwrap();
            let rtf = std::str::from_utf8(&rtf.bytes).unwrap();
            assert!(rtf.contains("\\pict\\pngblip"));
            assert!(rtf.contains("\\b "));
        }
        assert_eq!(bundle.encode().unwrap(), before);
    }

    #[test]
    fn review_external_css_and_svg_cannot_claim_restorable_html() {
        for html in [
            "<style>@import 'https://fixture.invalid/style.css';</style><p>text</p>",
            r"<p style='background:u\72l(https://fixture.invalid/image.png)'>text</p>",
            "<p style='background:&#117;rl(https://fixture.invalid/image.png)'>text</p>",
            "<svg><image href='https://fixture.invalid/image.png'/></svg>",
            "<svg><use xlink:href='https://fixture.invalid/sprites.svg#icon'/></svg>",
        ] {
            let snapshot = NativeSnapshot {
                before: 1,
                after: 1,
                items: 1,
                state: SnapshotState::Stable,
                formats: vec![NativeFormat {
                    item: 0,
                    name: "public.html".into(),
                    state: FormatState::Captured,
                    bytes: html.as_bytes().to_vec(),
                }],
            };
            let bundle = super::super::from_native_capture(&snapshot).unwrap();
            assert_ne!(bundle.completeness, Completeness::Complete, "{html}");
            assert!(!super::super::html::restore_reference_inventory_complete(
                html.as_bytes(),
                &[],
            )
            .unwrap());
            assert!(RestorePlan::compatibility(bundle, input(), RestoreTarget::Macos).is_err());
            assert!(RestorePlan::originals(snapshot, input(), RestoreTarget::Macos).is_err());
        }
        for html in [
            "<p style='font-weight:bold;color:red'>text</p>",
            "<style>p { font-size:12px }</style><p>text</p>",
            "<a href='https://fixture.invalid/'>link</a>",
        ] {
            let bundle = PortableBundle {
                representations: vec![Representation {
                    mime: Mime::Html,
                    bytes: html.as_bytes().to_vec(),
                }],
                resources: vec![],
                bindings: vec![],
                completeness: Completeness::Complete,
            };
            assert!(RestorePlan::compatibility(bundle, input(), RestoreTarget::Macos).is_ok());
        }
    }

    #[test]
    fn o06_mixed_inline_and_bound_resources_cannot_exceed_the_shared_inventory_limit() {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        let mut b = rich();
        // Twenty distinct bound images plus one source-inline image would fail
        // the production readback projection's twenty-resource budget.
        b.resources.clear();
        b.bindings.clear();
        let mut html = String::new();
        for id in 1..=crate::composite::RESOURCE_LIMIT as u16 {
            let png = packed_to_png(1, 1, &[id as u8, 0, 0, 255]).unwrap();
            let reference = format!("tailsync-resource:{id}");
            html.push_str("<img src='");
            b.bindings.push(ResourceBinding {
                representation: 1,
                resource: id,
                offset: html.len() as u32,
                length: reference.len() as u32,
            });
            html.push_str(&reference);
            html.push_str("'>");
            b.resources.push(Resource { id, png });
        }
        html.push_str(&format!(
            "<img src='data:image/png;base64,{}'>",
            STANDARD.encode(packed_to_png(1, 1, &[0, 255, 0, 255]).unwrap())
        ));
        b.representations[1].bytes = html.into_bytes();
        assert!(b.validate().is_ok());
        assert!(RestorePlan::compatibility(b, input(), RestoreTarget::Macos).is_err());
    }
}
