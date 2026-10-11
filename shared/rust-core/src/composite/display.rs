use super::content_analysis::{
    analyze, AnalysisIdentity, AnalysisRequest, ContentAnalysis, ImageAvailability,
    ImageCoordinate, ImagePlacement, NativeItemFact,
};
use super::PortableBundle;
use crate::cancellation::Cancellation;
use std::time::{Duration, Instant};

pub const DISPLAY_CLASSIFIER_VERSION: i64 = 1003;

const CLASSIFY_DEADLINE: Duration = Duration::from_secs(4);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayKind {
    Image,
    Document,
    Text,
    File,
    Unknown,
}

impl DisplayKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Document => "document",
            Self::Text => "text",
            Self::File => "file",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayClassification {
    pub kind: DisplayKind,
    pub category: &'static str,
    pub categories: Vec<&'static str>,
    pub confidence: u8,
    pub version: i64,
}

impl DisplayClassification {
    pub fn categories_json(&self) -> String {
        serde_json::to_string(&self.categories).unwrap_or_else(|_| "[]".into())
    }
}

fn image_classification() -> DisplayClassification {
    DisplayClassification {
        kind: DisplayKind::Image,
        category: "image",
        categories: vec!["image"],
        confidence: 95,
        version: DISPLAY_CLASSIFIER_VERSION,
    }
}

fn document_classification(with_images: bool) -> DisplayClassification {
    DisplayClassification {
        kind: DisplayKind::Document,
        category: "text",
        categories: if with_images {
            vec!["text", "image"]
        } else {
            vec!["text"]
        },
        confidence: 90,
        version: DISPLAY_CLASSIFIER_VERSION,
    }
}

fn text_classification(text: &str) -> DisplayClassification {
    let classification = crate::history_classifier::classify_text(text);
    DisplayClassification {
        kind: DisplayKind::Text,
        category: classification.category,
        categories: classification.categories(),
        confidence: 85,
        version: DISPLAY_CLASSIFIER_VERSION,
    }
}

pub fn unknown_classification(has_image: bool) -> DisplayClassification {
    DisplayClassification {
        kind: DisplayKind::Unknown,
        category: if has_image { "image" } else { "text" },
        categories: if has_image {
            vec!["text", "image"]
        } else {
            vec!["text"]
        },
        confidence: 0,
        version: DISPLAY_CLASSIFIER_VERSION,
    }
}

fn is_representation_image(image: &ImagePlacement) -> bool {
    matches!(image.coordinate, ImageCoordinate::Representation)
}

pub fn classify(analysis: &ContentAnalysis) -> DisplayClassification {
    let has_body = !analysis.text.text.trim().is_empty();
    let positioned = analysis
        .images
        .iter()
        .filter(|image| !is_representation_image(image))
        .count();
    let standalone = analysis
        .images
        .iter()
        .filter(|image| is_representation_image(image))
        .count();
    let total_images = analysis.images.len();
    // QQ's private element sequence is one message spanning image file items.
    // Only its proven selected HTML can represent those items as one body.
    let qq_message = !analysis.native_only
        && analysis.representations.iter().any(|representation| {
            representation.selected
                && representation.mime == super::Mime::Html
                && matches!(
                    &representation.source,
                    super::content_analysis::RepresentationSource::Derived { converter, source_name }
                        if converter == crate::protocol::native_archive::CONVERTER_QQ_TO_PORTABLE
                            && source_name == crate::protocol::native_archive::QQ_MESSAGE_FORMAT
                )
        });

    if analysis.unknown
        || analysis.partial.iter().any(|reason| {
            matches!(
                reason.code,
                "native_analysis_unavailable" | "depth_limit" | "run_limit" | "malformed_html"
            )
        })
        || (analysis.items.len() > 1 && !qq_message)
    {
        return unknown_classification(total_images > 0 || analysis.has_png_representation());
    }

    if has_body {
        // Text-only copies often carry editor font/emphasis styles. Keep them in
        // the text viewer; images and tables need the document layout. Originals
        // retain all formatting for restore regardless of this display choice.
        if positioned > 0
            || analysis.tables.tables > 0
            || analysis.native_only
            || !analysis.representations.iter().any(|representation| {
                matches!(representation.mime, super::Mime::Text | super::Mime::Html)
            })
        {
            return document_classification(positioned > 0);
        }
        if standalone > 0 || analysis.has_png_representation() {
            return unknown_classification(true);
        }
        return text_classification(&analysis.text.text);
    }

    if total_images > 0 {
        return image_classification();
    }
    if analysis
        .representations
        .iter()
        .any(|representation| representation.mime == super::Mime::Text)
    {
        return text_classification("");
    }
    unknown_classification(false)
}

impl ContentAnalysis {
    pub(crate) fn has_png_representation(&self) -> bool {
        self.representations
            .iter()
            .any(|representation| representation.mime == super::Mime::Png)
    }

    pub fn first_image_placement(&self) -> Option<&ImagePlacement> {
        let ordinal = self.first_image?;
        self.images.iter().find(|image| image.ordinal == ordinal)
    }

    pub fn first_image_if_available(&self) -> Option<&ImagePlacement> {
        let first = self.first_image_placement()?;
        (first.availability == ImageAvailability::Available).then_some(first)
    }
}

pub fn classify_bundle(
    bundle: &PortableBundle,
    native_only: bool,
    native_items: Vec<NativeItemFact>,
) -> DisplayClassification {
    let request = AnalysisRequest::for_display(bundle.clone(), native_only, native_items);
    let identity = AnalysisIdentity {
        entry_id: 0,
        authority: String::new(),
        generation: 0,
        owner: String::new(),
        capture_id: String::new(),
        content_id: String::new(),
        archive_revision: 0,
        archive_digest: String::new(),
    };
    match analyze(
        identity,
        &request,
        &Cancellation::default(),
        Instant::now() + CLASSIFY_DEADLINE,
    ) {
        Ok(analysis) => classify(&analysis),
        Err(_) => unknown_classification(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composite::content_analysis::{analyze, AnalysisRequest, RepresentationSource};
    use crate::composite::{Completeness, Mime, Representation, Resource, ResourceBinding};

    fn identity() -> AnalysisIdentity {
        AnalysisIdentity {
            entry_id: 1,
            authority: "test".into(),
            generation: 1,
            owner: "history".into(),
            capture_id: "c".into(),
            content_id: "i".into(),
            archive_revision: 1,
            archive_digest: "d".into(),
        }
    }

    fn deadline() -> Instant {
        Instant::now() + Duration::from_secs(4)
    }

    fn png_1x1() -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&[0, 0, 0, 0])
                .unwrap();
        }
        bytes
    }

    fn analyze_bundle(bundle: PortableBundle) -> ContentAnalysis {
        let request = AnalysisRequest {
            bundle,
            provenance: vec![RepresentationSource::Original],
            native_only: false,
            native: vec![],
            native_items: vec![],
            decode_images: true,
            document: false,
        };
        analyze(identity(), &request, &Cancellation::default(), deadline()).unwrap()
    }

    fn html_bundle(
        html: &str,
        resources: Vec<Resource>,
        bindings: Vec<ResourceBinding>,
    ) -> PortableBundle {
        PortableBundle {
            representations: vec![Representation {
                mime: Mime::Html,
                bytes: html.as_bytes().to_vec(),
            }],
            resources,
            bindings,
            completeness: Completeness::Complete,
        }
    }

    #[test]
    fn a_single_image_with_three_formats_is_an_image_not_a_document() {
        let bundle = PortableBundle {
            representations: vec![
                Representation {
                    mime: Mime::Png,
                    bytes: png_1x1(),
                },
                Representation {
                    mime: Mime::Text,
                    bytes: b"https://example.com/a.png".to_vec(),
                },
                Representation {
                    mime: Mime::Html,
                    bytes: {
                        use base64::Engine as _;
                        format!(
                            "<img src='data:image/png;base64,{}'>",
                            base64::engine::general_purpose::STANDARD.encode(png_1x1())
                        )
                        .into_bytes()
                    },
                },
            ],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::Complete,
        };
        let classification = classify_bundle(&bundle, false, vec![]);
        assert_eq!(classification.kind, DisplayKind::Image);
        assert_eq!(classification.categories, vec!["image"]);
    }

    #[test]
    fn html_with_only_an_image_is_an_image() {
        let html = "<img src=\"data:image/png;base64,iVBORw0KGgo=\">";
        let bundle = html_bundle(html, vec![], vec![]);
        let classification = classify_bundle(&bundle, false, vec![]);
        assert_eq!(classification.kind, DisplayKind::Image);
    }

    #[test]
    fn real_body_with_a_positioned_image_is_a_document() {
        let html = "<p>hello world</p><img src=\"https://example.com/a.png\">";
        let bundle = html_bundle(html, vec![], vec![]);
        let classification = classify_bundle(&bundle, false, vec![]);
        assert_eq!(classification.kind, DisplayKind::Document);
        assert_eq!(classification.categories, vec!["text", "image"]);
    }

    #[test]
    fn image_with_an_unproven_url_relationship_stays_unknown() {
        let bundle = PortableBundle {
            representations: vec![
                Representation {
                    mime: Mime::Text,
                    bytes: b"https://example.com/photo.png".to_vec(),
                },
                Representation {
                    mime: Mime::Png,
                    bytes: png_1x1(),
                },
            ],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::Complete,
        };
        let classification = classify_bundle(&bundle, false, vec![]);
        assert_eq!(classification.kind, DisplayKind::Unknown);
    }

    #[test]
    fn real_prose_without_images_is_text() {
        let bundle = PortableBundle {
            representations: vec![Representation {
                mime: Mime::Text,
                bytes: b"just a normal note".to_vec(),
            }],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::Complete,
        };
        let classification = classify_bundle(&bundle, false, vec![]);
        assert_eq!(classification.kind, DisplayKind::Text);
        assert_eq!(classification.categories, vec!["text"]);
    }

    #[test]
    fn text_styles_without_images_keep_text_preview_tools() {
        let html = "<p><b>bold</b> and <i>italic</i></p>";
        let bundle = html_bundle(html, vec![], vec![]);
        let classification = classify_bundle(&bundle, false, vec![]);
        assert_eq!(classification.kind, DisplayKind::Text);
        assert_eq!(classification.categories, vec!["text"]);
    }

    #[test]
    fn styled_commands_and_json_use_the_code_category() {
        for html in [
            "<pre><b>git status --short</b></pre>",
            "<p><i>{&quot;enabled&quot;:true}</i></p>",
        ] {
            let classification = classify_bundle(&html_bundle(html, vec![], vec![]), false, vec![]);
            assert_eq!(classification.kind, DisplayKind::Text);
            assert_eq!(classification.categories, vec!["code"]);
        }
    }

    #[test]
    fn text_tables_keep_the_document_renderer() {
        let classification = classify_bundle(
            &html_bundle(
                "<table><tr><td>one</td><td>two</td></tr></table>",
                vec![],
                vec![],
            ),
            false,
            vec![],
        );
        assert_eq!(classification.kind, DisplayKind::Document);
    }

    #[test]
    fn native_only_without_helper_facts_is_unknown_not_text() {
        let bundle = PortableBundle {
            representations: vec![],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::Complete,
        };
        let classification = classify_bundle(&bundle, true, vec![]);
        assert_eq!(classification.kind, DisplayKind::Unknown);
        assert_eq!(classification.confidence, 0);
    }

    #[test]
    fn first_image_follows_body_order_not_resource_storage_order() {
        let html = "<p><img src=\"tailsync-resource:2\"><img src=\"tailsync-resource:1\"></p>";
        let bindings = vec![
            ResourceBinding {
                representation: 0,
                resource: 2,
                offset: html.find("tailsync-resource:2").unwrap() as u32,
                length: "tailsync-resource:2".len() as u32,
            },
            ResourceBinding {
                representation: 0,
                resource: 1,
                offset: html.find("tailsync-resource:1").unwrap() as u32,
                length: "tailsync-resource:1".len() as u32,
            },
        ];
        let resources = vec![
            Resource {
                id: 1,
                png: png_1x1(),
            },
            Resource {
                id: 2,
                png: png_1x1(),
            },
        ];
        let analysis = analyze_bundle(html_bundle(html, resources, bindings));
        let first = analysis.first_image_placement().unwrap();
        assert_eq!(first.resource_id, Some(2));
        assert!(analysis.first_image_if_available().is_some());
    }

    #[test]
    fn missing_first_image_does_not_substitute_the_second() {
        let html = "<p><img src=\"tailsync-resource:9\"><img src=\"tailsync-resource:1\"></p>";
        let bindings = vec![
            ResourceBinding {
                representation: 0,
                resource: 9,
                offset: html.find("tailsync-resource:9").unwrap() as u32,
                length: "tailsync-resource:9".len() as u32,
            },
            ResourceBinding {
                representation: 0,
                resource: 1,
                offset: html.find("tailsync-resource:1").unwrap() as u32,
                length: "tailsync-resource:1".len() as u32,
            },
        ];
        let resources = vec![Resource {
            id: 1,
            png: png_1x1(),
        }];
        let analysis = analyze_bundle(html_bundle(html, resources, bindings));
        assert_eq!(analysis.first_image, Some(0));
        assert_eq!(
            analysis.first_image_placement().unwrap().availability,
            ImageAvailability::MissingResource
        );
        assert!(analysis.first_image_if_available().is_none());
    }

    #[test]
    fn classify_bundle_never_decodes_images_but_still_reports_availability() {
        let html = "<img src=\"tailsync-resource:1\">";
        let bindings = vec![ResourceBinding {
            representation: 0,
            resource: 1,
            offset: html.find("tailsync-resource:1").unwrap() as u32,
            length: "tailsync-resource:1".len() as u32,
        }];
        let resources = vec![Resource {
            id: 1,
            png: png_1x1(),
        }];
        let bundle = html_bundle(html, resources, bindings);
        let classification = classify_bundle(&bundle, false, vec![]);
        assert_eq!(classification.kind, DisplayKind::Image);
    }
}
