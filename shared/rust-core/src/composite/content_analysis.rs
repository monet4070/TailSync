use super::display_document::DisplayDocument;
use super::html_lex::{attribute_span, decode_entity, find_tag_end};
use crate::cancellation::Cancellation;
use crate::protocol::composite::{Mime, PortableBundle};
use crate::protocol::content_analysis::{
    AttachmentState, SavedAnalysisKind, SavedTextAnalysis, STYLE_BOLD, STYLE_ITALIC,
    STYLE_STRIKETHROUGH, STYLE_SUBSCRIPT, STYLE_SUPERSCRIPT, STYLE_UNDERLINE, STYLE_UNSUPPORTED,
};
use std::time::Instant;

pub const ANALYSIS_VERSION: u16 = 1;

pub const TEXT_LIMIT: usize = 1024 * 1024;
pub const DISPLAY_OUTPUT_LIMIT: usize = 16 * 1024 * 1024;
pub const BLOCK_LIMIT: usize = 8192;
pub const RUN_LIMIT: usize = 8192;
pub const CELL_LIMIT: usize = 2048;
pub const DEPTH_LIMIT: usize = 64;
pub const IMAGE_LIMIT: usize = crate::protocol::composite::RESOURCE_LIMIT;
pub const DECODED_PIXEL_LIMIT: u64 = crate::protocol::native_policy::COVER_PIXEL_LIMIT;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnalysisError {
    Cancelled,
    Deadline,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisIdentity {
    pub entry_id: i64,
    pub authority: String,
    pub generation: u64,
    pub owner: String,
    pub capture_id: String,
    pub content_id: String,
    pub archive_revision: i64,
    pub archive_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepresentationSource {
    Original,
    Unverified,
    Derived {
        converter: String,
        source_name: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepresentationRole {
    Primary,
    Alternative,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepresentationFact {
    pub index: u16,
    pub mime: Mime,
    pub source: RepresentationSource,
    pub role: RepresentationRole,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TextFacts {
    pub text: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StyleFacts {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub subscript: bool,
    pub superscript: bool,
    pub unsupported: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TableFacts {
    pub tables: usize,
    pub rows: usize,
    pub cells: usize,
    pub max_columns: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageCoordinate {
    Representation,
    HtmlByteRange { offset: u32, length: u32 },
    Utf16Character { position: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageAvailability {
    Available,
    MissingResource,
    MissingSource,
    ExternalReference,
    Unsupported,
    PixelBudget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImagePlacement {
    pub ordinal: u16,
    pub representation: u16,
    pub item: u16,
    pub coordinate: ImageCoordinate,
    pub resource_id: Option<u16>,
    pub availability: ImageAvailability,
    pub png: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeSelection {
    pub item: u16,
    pub kind: SavedAnalysisKind,
    pub source_format: String,
    pub converter: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeItemFact {
    pub item: u16,
    pub formats: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeAttachment {
    pub utf16_position: u32,
    pub state: AttachmentState,
    pub png: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeAnalysisFacts {
    pub kind: SavedAnalysisKind,
    pub item: u16,
    pub source_format: String,
    pub converter: String,
    pub body: String,
    pub truncated: bool,
    pub partial: bool,
    pub style_bits: u8,
    pub table_rows: u16,
    pub table_cells: u16,
    pub attachments: Vec<NativeAttachment>,
}

impl NativeAnalysisFacts {
    pub fn from_saved(analysis: SavedTextAnalysis, item: u16) -> Self {
        Self {
            kind: analysis.kind,
            item,
            source_format: analysis.source_format,
            converter: analysis.converter,
            body: analysis.body,
            truncated: analysis.truncated,
            partial: analysis.partial,
            style_bits: analysis.style_bits,
            table_rows: analysis.table_rows,
            table_cells: analysis.table_cells,
            attachments: analysis
                .attachments
                .into_iter()
                .map(|attachment| NativeAttachment {
                    utf16_position: attachment.utf16_position,
                    state: attachment.state,
                    png: attachment.png,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartialReason {
    pub code: &'static str,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentAnalysis {
    pub version: u16,
    pub identity: AnalysisIdentity,
    pub native_only: bool,
    pub representations: Vec<RepresentationFact>,
    pub selected: Option<u16>,
    pub selected_native: Option<NativeSelection>,
    pub items: Vec<NativeItemFact>,
    pub text: TextFacts,
    pub styles: StyleFacts,
    pub tables: TableFacts,
    pub images: Vec<ImagePlacement>,
    pub first_image: Option<u16>,
    pub partial: Vec<PartialReason>,
    pub unknown: bool,
    pub document: Option<DisplayDocument>,
}

pub struct AnalysisRequest {
    pub bundle: PortableBundle,
    pub provenance: Vec<RepresentationSource>,
    pub native_only: bool,
    pub native: Vec<NativeAnalysisFacts>,
    pub native_items: Vec<NativeItemFact>,
    pub decode_images: bool,
    pub document: bool,
}

impl AnalysisRequest {
    pub fn for_display(
        bundle: PortableBundle,
        native_only: bool,
        native_items: Vec<NativeItemFact>,
    ) -> Self {
        let provenance = vec![RepresentationSource::Original; bundle.representations.len()];
        Self {
            bundle,
            provenance,
            native_only,
            native: vec![],
            native_items,
            decode_images: false,
            document: false,
        }
    }
}

fn partial(reason: &'static str, detail: impl Into<String>) -> PartialReason {
    PartialReason {
        code: reason,
        detail: detail.into(),
    }
}

fn select_representation(bundle: &PortableBundle) -> Result<Option<usize>, AnalysisError> {
    if let Some(png) = super::available_restore::image_only_png_alternative(bundle)
        .map_err(|_| AnalysisError::Invalid)?
    {
        return Ok(Some(png));
    }
    Ok([Mime::Html, Mime::Rtf, Mime::Text, Mime::Png]
        .into_iter()
        .find_map(|mime| {
            bundle
                .representations
                .iter()
                .position(|representation| representation.mime == mime)
        }))
}

fn source_for(provenance: &[RepresentationSource], index: usize) -> RepresentationSource {
    provenance
        .get(index)
        .cloned()
        .unwrap_or(RepresentationSource::Original)
}

pub fn analyze(
    identity: AnalysisIdentity,
    request: &AnalysisRequest,
    cancellation: &Cancellation,
    deadline: Instant,
) -> Result<ContentAnalysis, AnalysisError> {
    check(cancellation, deadline)?;
    if request.bundle.representations.len() > 4
        || request.bundle.resources.len() > IMAGE_LIMIT
        || request.bundle.bindings.len() > IMAGE_LIMIT
        || request.native.len() > 64
        || request.native_items.len() > 16
        || request.bundle.representations.iter().any(|representation| {
            representation.bytes.len() > DISPLAY_OUTPUT_LIMIT.min(representation.mime.limit())
        })
        || request.native_items.iter().any(|item| {
            item.formats.len() > 64 || item.formats.iter().any(|format| format.len() > 256)
        })
    {
        return Err(AnalysisError::Invalid);
    }
    let mut partial_reasons = Vec::new();
    let mut images = Vec::new();
    let mut text = TextFacts::default();
    let mut styles = StyleFacts::default();
    let mut tables = TableFacts::default();
    let mut unknown = false;

    let selected = if request.native_only {
        None
    } else {
        select_representation(&request.bundle)?
    };
    let selected_is_rtf = selected
        .map(|index| request.bundle.representations[index].mime == Mime::Rtf)
        .unwrap_or(false);
    let use_native = !request.native.is_empty() && (request.native_only || selected_is_rtf);
    if use_native {
        let facts = primary_native(&request.native).ok_or(AnalysisError::Invalid)?;
        if facts.body.len() > TEXT_LIMIT
            || facts.attachments.len() > IMAGE_LIMIT
            || facts.source_format.len() > 256
            || facts.converter.len() > 128
            || facts.table_cells as usize > CELL_LIMIT
        {
            return Err(AnalysisError::Invalid);
        }
        let units: Vec<u16> = facts.body.encode_utf16().collect();
        let mut previous = None;
        let mut total = 0usize;
        for attachment in &facts.attachments {
            if units.get(attachment.utf16_position as usize) != Some(&0xfffc)
                || previous.is_some_and(|position| position >= attachment.utf16_position)
                || attachment.png.len()
                    > crate::protocol::content_analysis::SAVED_ATTACHMENT_BYTES_LIMIT
                || (attachment.state == AttachmentState::Available) == attachment.png.is_empty()
            {
                return Err(AnalysisError::Invalid);
            }
            previous = Some(attachment.utf16_position);
            total = total.saturating_add(attachment.png.len());
            if total > DISPLAY_OUTPUT_LIMIT - TEXT_LIMIT {
                return Err(AnalysisError::Invalid);
            }
        }
        if request.native_items.len() > 1 {
            partial_reasons.push(partial(
                "other_native_items",
                "other items remain separate from the selected body",
            ));
        }
    }
    let representations = request
        .bundle
        .representations
        .iter()
        .enumerate()
        .map(|(index, representation)| RepresentationFact {
            index: index as u16,
            mime: representation.mime,
            source: source_for(&request.provenance, index),
            role: if Some(index) == selected && !use_native {
                RepresentationRole::Primary
            } else {
                RepresentationRole::Alternative
            },
            selected: Some(index) == selected && !use_native,
        })
        .collect();

    if use_native {
        apply_native(
            &request.native,
            &mut text,
            &mut styles,
            &mut tables,
            &mut images,
            &mut partial_reasons,
        );
    } else if let Some(index) = selected {
        let representation = &request.bundle.representations[index];
        check(cancellation, deadline)?;
        match representation.mime {
            Mime::Html => {
                let scan = scan_html(
                    &representation.bytes,
                    &request.bundle,
                    index as u16,
                    request.decode_images,
                    cancellation,
                    deadline,
                )?;
                text = scan.text;
                styles = scan.styles;
                tables = scan.tables;
                images = scan.images;
                partial_reasons.extend(scan.partial);
            }
            Mime::Rtf => partial_reasons.push(partial(
                "native_analysis_unavailable",
                "RTF body facts require the supervised helper",
            )),
            Mime::Text => match std::str::from_utf8(&representation.bytes) {
                Ok(value) => {
                    let (bounded, truncated) = bounded_text(value, TEXT_LIMIT);
                    text = TextFacts {
                        text: bounded,
                        truncated,
                    };
                }
                Err(_) => unknown = true,
            },
            Mime::Png => {
                let mut pixels = 0u64;
                let mut asset_bytes = 0usize;
                let availability = if request.decode_images {
                    admit_png(&representation.bytes, &mut pixels, &mut asset_bytes)
                } else {
                    admit_png_cheap(&representation.bytes, &mut pixels, &mut asset_bytes)
                };
                images.push(ImagePlacement {
                    ordinal: 0,
                    representation: index as u16,
                    item: 0,
                    coordinate: ImageCoordinate::Representation,
                    resource_id: None,
                    availability,
                    png: if availability == ImageAvailability::Available && request.decode_images {
                        representation.bytes.clone()
                    } else {
                        vec![]
                    },
                });
            }
        }
    } else {
        apply_native(
            &request.native,
            &mut text,
            &mut styles,
            &mut tables,
            &mut images,
            &mut partial_reasons,
        );
        if request.native.is_empty() {
            unknown = true;
        }
    }

    if request.native_only && !use_native {
        partial_reasons.push(partial(
            "native_summary",
            "showing a bounded native summary, not parsed content",
        ));
        unknown = true;
    }
    if !use_native
        && selected.is_some_and(|index| {
            source_for(&request.provenance, index) == RepresentationSource::Unverified
        })
    {
        partial_reasons.push(partial(
            "unverified_provenance",
            "representation derivation could not be proven",
        ));
    }
    if request.bundle.representations.is_empty()
        && request.native.is_empty()
        && !request.native_only
    {
        unknown = true;
    }

    let mut rtf_preview = None;
    if request.document && selected_is_rtf {
        let index = selected.unwrap();
        if source_for(&request.provenance, index) == RepresentationSource::Original {
            let (saved, spans) = crate::protocol::content_analysis::scan_rtf_with_styles(
                &request.bundle.representations[index].bytes,
            );
            check(cancellation, deadline)?;
            if let Some(spans) = spans {
                let matches_native = !use_native
                    || primary_native(&request.native).is_some_and(|facts| {
                        facts.kind == SavedAnalysisKind::Rtf
                            && facts.source_format == "public.rtf"
                            && !facts.partial
                            && !facts.truncated
                            && facts.body == saved.body
                    });
                if matches_native {
                    let facts = NativeAnalysisFacts::from_saved(saved, 0);
                    if !use_native {
                        partial_reasons
                            .retain(|reason| reason.code != "native_analysis_unavailable");
                        apply_native(
                            std::slice::from_ref(&facts),
                            &mut text,
                            &mut styles,
                            &mut tables,
                            &mut images,
                            &mut partial_reasons,
                        );
                    }
                    rtf_preview = Some((facts, spans));
                }
            }
        }
    }
    let document = if request.document {
        let mut built = if use_native {
            primary_native(&request.native).map(|facts| {
                super::document_build::document_from_native(facts, &images, &partial_reasons)
            })
        } else if let Some(index) = selected {
            match request.bundle.representations[index].mime {
                Mime::Html => Some(super::document_build::document_from_html_checked(
                    &request.bundle.representations[index].bytes,
                    &images,
                    &partial_reasons,
                    cancellation,
                    deadline,
                )?),
                Mime::Rtf if rtf_preview.is_some() => {
                    Some(super::document_build::document_from_native(
                        &rtf_preview.as_ref().unwrap().0,
                        &images,
                        &partial_reasons,
                    ))
                }
                _ => Some(super::document_build::document_from_text_with_reasons(
                    &text,
                    &partial_reasons,
                )),
            }
        } else {
            Some(super::document_build::document_from_text_with_reasons(
                &text,
                &partial_reasons,
            ))
        };
        if let (Some(document), Some((facts, spans))) = (built.as_mut(), &rtf_preview) {
            super::document_native_styles::apply_rtf_styles(document, &facts.body, spans);
        }
        if let Some(document) = built.as_ref() {
            document.validate().map_err(|_| AnalysisError::Invalid)?;
        }
        built
    } else {
        None
    };
    let first_image = images.iter().map(|image| image.ordinal).min();
    let mut analysis = ContentAnalysis {
        version: ANALYSIS_VERSION,
        identity,
        native_only: request.native_only,
        representations,
        selected: if use_native {
            None
        } else {
            selected.map(|index| index as u16)
        },
        selected_native: if use_native {
            primary_native(&request.native).map(|facts| NativeSelection {
                item: facts.item,
                kind: facts.kind,
                source_format: facts.source_format.clone(),
                converter: facts.converter.clone(),
            })
        } else {
            None
        },
        items: request.native_items.clone(),
        text,
        styles,
        tables,
        images,
        first_image,
        partial: partial_reasons,
        unknown,
        document,
    };
    if analysis.text.text.len() > TEXT_LIMIT {
        let (bounded, _) = bounded_text(&analysis.text.text, TEXT_LIMIT);
        analysis.text.text = bounded;
        analysis.text.truncated = true;
    }
    Ok(analysis)
}

fn primary_native(native: &[NativeAnalysisFacts]) -> Option<&NativeAnalysisFacts> {
    native.iter().min_by_key(|facts| {
        (
            facts.item,
            if facts.kind == SavedAnalysisKind::Rtfd {
                0
            } else {
                1
            },
        )
    })
}

fn apply_native(
    native: &[NativeAnalysisFacts],
    text: &mut TextFacts,
    styles: &mut StyleFacts,
    tables: &mut TableFacts,
    images: &mut Vec<ImagePlacement>,
    partial_reasons: &mut Vec<PartialReason>,
) {
    let Some(facts) = primary_native(native) else {
        return;
    };
    let mut pixels = 0u64;
    let mut asset_bytes = 0usize;
    {
        let (bounded, truncated) = bounded_text(&facts.body, TEXT_LIMIT);
        *text = TextFacts {
            text: bounded,
            truncated: truncated || facts.truncated,
        };
        if native.iter().any(|other| other.item != facts.item) {
            partial_reasons.push(partial(
                "other_native_items",
                "other items remain separate from the selected body",
            ));
        }
        if facts.style_bits & STYLE_BOLD != 0 {
            styles.bold = true;
        }
        if facts.style_bits & STYLE_ITALIC != 0 {
            styles.italic = true;
        }
        if facts.style_bits & STYLE_UNDERLINE != 0 {
            styles.underline = true;
        }
        if facts.style_bits & STYLE_STRIKETHROUGH != 0 {
            styles.strikethrough = true;
        }
        if facts.style_bits & STYLE_SUBSCRIPT != 0 {
            styles.subscript = true;
        }
        if facts.style_bits & STYLE_SUPERSCRIPT != 0 {
            styles.superscript = true;
        }
        if facts.style_bits & STYLE_UNSUPPORTED != 0 || facts.partial {
            styles.unsupported = true;
            partial_reasons.push(partial(
                "native_partial",
                format!("{} attributes were not representable", facts.source_format),
            ));
        }
        if facts.table_rows > 0 || facts.table_cells > 0 {
            tables.tables = tables.tables.max(1);
            tables.rows = tables.rows.max(facts.table_rows as usize);
            tables.cells = tables.cells.max(facts.table_cells as usize);
        }
        for attachment in &facts.attachments {
            if images.len() >= IMAGE_LIMIT {
                partial_reasons.push(partial("image_limit", "image budget reached"));
                break;
            }
            let availability = match attachment.state {
                AttachmentState::Available => ImageAvailability::Available,
                AttachmentState::Missing => ImageAvailability::MissingSource,
                AttachmentState::Unsupported => ImageAvailability::Unsupported,
                AttachmentState::PixelBudget => ImageAvailability::PixelBudget,
            };
            let availability = if attachment.state == AttachmentState::Available {
                admit_png(&attachment.png, &mut pixels, &mut asset_bytes)
            } else {
                availability
            };
            images.push(ImagePlacement {
                ordinal: images.len() as u16,
                representation: 0,
                item: facts.item,
                coordinate: ImageCoordinate::Utf16Character {
                    position: attachment.utf16_position,
                },
                resource_id: None,
                availability,
                png: if availability == ImageAvailability::Available {
                    attachment.png.clone()
                } else {
                    vec![]
                },
            });
        }
    }
}

fn check(cancellation: &Cancellation, deadline: Instant) -> Result<(), AnalysisError> {
    if cancellation.is_cancelled() {
        return Err(AnalysisError::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(AnalysisError::Deadline);
    }
    Ok(())
}

fn bounded_text(value: &str, limit: usize) -> (String, bool) {
    if value.len() <= limit {
        return (value.to_string(), false);
    }
    let mut end = limit;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    (value[..end].to_string(), true)
}

fn admit_png(png: &[u8], pixels: &mut u64, asset_bytes: &mut usize) -> ImageAvailability {
    if png.len() > crate::protocol::content_analysis::SAVED_ATTACHMENT_BYTES_LIMIT {
        return ImageAvailability::PixelBudget;
    }
    let Some(decoded) = crate::protocol::composite::png_rgba_size(png).ok() else {
        return ImageAvailability::Unsupported;
    };
    let next_pixels = pixels.saturating_add(decoded as u64 / 4);
    let next_bytes = asset_bytes.saturating_add(png.len());
    if next_pixels > DECODED_PIXEL_LIMIT || next_bytes > DISPLAY_OUTPUT_LIMIT - TEXT_LIMIT - 65536 {
        return ImageAvailability::PixelBudget;
    }
    if !crate::protocol::image::is_static_png(png) || crate::composite::png_to_packed(png).is_err()
    {
        return ImageAvailability::Unsupported;
    }
    *pixels = next_pixels;
    *asset_bytes = next_bytes;
    ImageAvailability::Available
}

fn admit_png_cheap(png: &[u8], pixels: &mut u64, asset_bytes: &mut usize) -> ImageAvailability {
    if png.len() > crate::protocol::content_analysis::SAVED_ATTACHMENT_BYTES_LIMIT {
        return ImageAvailability::PixelBudget;
    }
    let Some(decoded) = crate::protocol::composite::png_rgba_size(png).ok() else {
        return ImageAvailability::Unsupported;
    };
    let next_pixels = pixels.saturating_add(decoded as u64 / 4);
    let next_bytes = asset_bytes.saturating_add(png.len());
    if next_pixels > DECODED_PIXEL_LIMIT || next_bytes > DISPLAY_OUTPUT_LIMIT - TEXT_LIMIT - 65536 {
        return ImageAvailability::PixelBudget;
    }
    if !crate::protocol::image::is_static_png(png) {
        return ImageAvailability::Unsupported;
    }
    *pixels = next_pixels;
    *asset_bytes = next_bytes;
    ImageAvailability::Available
}

struct HtmlScan {
    text: TextFacts,
    styles: StyleFacts,
    tables: TableFacts,
    images: Vec<ImagePlacement>,
    partial: Vec<PartialReason>,
}

fn scan_html(
    bytes: &[u8],
    bundle: &PortableBundle,
    representation: u16,
    decode_images: bool,
    cancellation: &Cancellation,
    deadline: Instant,
) -> Result<HtmlScan, AnalysisError> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let html = std::str::from_utf8(bytes).map_err(|_| AnalysisError::Invalid)?;
    let mut scan = HtmlScan {
        text: TextFacts::default(),
        styles: StyleFacts::default(),
        tables: TableFacts::default(),
        images: Vec::new(),
        partial: Vec::new(),
    };
    let mut index = 0usize;
    let mut stack = Vec::<String>::new();
    let mut row_cells = 0usize;
    let mut nodes = 0usize;
    let mut blocks = 0usize;
    let mut pixels = 0u64;
    let mut asset_bytes = 0usize;
    let mut pending_space = false;
    let mut raw_tag: Option<String> = None;
    while index < bytes.len() {
        check(cancellation, deadline)?;
        if bytes[index] != b'<' {
            let end = html[index..]
                .find('<')
                .map(|offset| index + offset)
                .unwrap_or(bytes.len());
            if raw_tag.is_none() {
                if append_html_text(&html[index..end], &mut scan.text, &mut pending_space) {
                    scan.styles.unsupported = true;
                }
                if scan.text.truncated {
                    scan.partial
                        .push(partial("text_limit", "body byte budget reached"));
                    break;
                }
            }
            index = end;
            continue;
        }
        if bytes[index..].starts_with(b"<!--") {
            index = html[index + 4..]
                .find("-->")
                .map(|offset| index + 7 + offset)
                .unwrap_or(bytes.len());
            continue;
        }
        let Some(end) = find_tag_end(bytes, index + 1) else {
            scan.partial
                .push(partial("malformed_html", "unterminated element"));
            break;
        };
        let mut cursor = index + 1;
        let closing = bytes.get(cursor) == Some(&b'/');
        if closing {
            cursor += 1;
        }
        let start = cursor;
        while cursor < end && bytes[cursor].is_ascii_alphanumeric() {
            cursor += 1;
        }
        if cursor == start {
            index = end + 1;
            continue;
        }
        let tag = html[start..cursor].to_ascii_lowercase();
        if let Some(raw) = &raw_tag {
            if closing && &tag == raw {
                raw_tag = None;
                stack.pop();
            }
            index = end + 1;
            continue;
        }
        nodes += 1;
        if attribute_span(bytes, cursor, end, b"style").is_some() {
            scan.styles.unsupported = true;
        }
        if nodes > RUN_LIMIT {
            scan.partial
                .push(partial("run_limit", "element budget reached"));
            break;
        }
        if closing {
            if let Some(position) = stack.iter().rposition(|open| open == &tag) {
                stack.truncate(position);
            }
            if matches!(
                tag.as_str(),
                "p" | "div" | "li" | "tr" | "blockquote" | "pre"
            ) {
                append_break(&mut scan.text, &mut pending_space);
            }
            index = end + 1;
            continue;
        }
        let void = matches!(
            tag.as_str(),
            "area"
                | "base"
                | "br"
                | "col"
                | "embed"
                | "hr"
                | "img"
                | "input"
                | "link"
                | "meta"
                | "param"
                | "source"
                | "track"
                | "wbr"
        );
        if !void && bytes.get(end.saturating_sub(1)) != Some(&b'/') {
            if stack.len() >= DEPTH_LIMIT {
                scan.partial
                    .push(partial("depth_limit", "element nesting budget reached"));
                break;
            }
            stack.push(tag.clone());
        }
        match tag.as_str() {
            "b" | "strong" => scan.styles.bold = true,
            "i" | "em" => scan.styles.italic = true,
            "u" => scan.styles.underline = true,
            "s" | "strike" | "del" => scan.styles.strikethrough = true,
            "sub" => scan.styles.subscript = true,
            "sup" => scan.styles.superscript = true,
            "script" | "style" => {
                scan.styles.unsupported = true;
                raw_tag = Some(tag.clone());
            }
            "object" | "iframe" | "link" | "embed" => scan.styles.unsupported = true,
            "table" => scan.tables.tables += 1,
            "tr" => {
                scan.tables.rows += 1;
                row_cells = 0;
            }
            "td" | "th" => {
                if scan.tables.cells >= CELL_LIMIT {
                    scan.tables.truncated = true;
                    scan.partial
                        .push(partial("cell_limit", "table cell budget reached"));
                    break;
                }
                scan.tables.cells += 1;
                row_cells += 1;
                scan.tables.max_columns = scan.tables.max_columns.max(row_cells);
            }
            "img" => {
                if scan.images.len() >= IMAGE_LIMIT {
                    scan.partial
                        .push(partial("image_limit", "image budget reached"));
                    break;
                }
                let span = attribute_span(bytes, cursor, end, b"src");
                let mut resource_id = None;
                let mut png = vec![];
                let availability_for =
                    |candidate: &[u8], pixels: &mut u64, asset_bytes: &mut usize| {
                        if decode_images {
                            admit_png(candidate, pixels, asset_bytes)
                        } else {
                            admit_png_cheap(candidate, pixels, asset_bytes)
                        }
                    };
                let availability = if let Some((start, finish)) = span {
                    let value = &html[start..finish];
                    if let Some(binding) = bundle.bindings.iter().find(|binding| {
                        binding.representation == representation
                            && binding.offset as usize == start
                            && binding.length as usize == finish - start
                    }) {
                        resource_id = Some(binding.resource);
                        if let Some(resource) = bundle
                            .resources
                            .iter()
                            .find(|resource| resource.id == binding.resource)
                        {
                            let availability =
                                availability_for(&resource.png, &mut pixels, &mut asset_bytes);
                            if availability == ImageAvailability::Available && decode_images {
                                png = resource.png.clone();
                            }
                            availability
                        } else {
                            ImageAvailability::MissingResource
                        }
                    } else if value
                        .get(..22)
                        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("data:image/png;base64,"))
                    {
                        match STANDARD.decode(&value[22..]) {
                            Ok(decoded) => {
                                let availability =
                                    availability_for(&decoded, &mut pixels, &mut asset_bytes);
                                if availability == ImageAvailability::Available && decode_images {
                                    png = decoded;
                                }
                                availability
                            }
                            Err(_) => ImageAvailability::MissingSource,
                        }
                    } else if value.starts_with("tailsync-resource:") {
                        ImageAvailability::MissingResource
                    } else if value.is_empty() {
                        ImageAvailability::MissingSource
                    } else {
                        ImageAvailability::ExternalReference
                    }
                } else {
                    scan.partial
                        .push(partial("missing_image_source", "an img element has no src"));
                    ImageAvailability::MissingSource
                };
                let (offset, finish) = span.unwrap_or((index, end + 1));
                scan.images.push(ImagePlacement {
                    ordinal: scan.images.len() as u16,
                    representation,
                    item: 0,
                    coordinate: ImageCoordinate::HtmlByteRange {
                        offset: offset as u32,
                        length: (finish - offset) as u32,
                    },
                    resource_id,
                    availability,
                    png,
                });
            }
            _ => {}
        }
        if matches!(
            tag.as_str(),
            "p" | "div" | "br" | "li" | "tr" | "blockquote" | "pre"
        ) {
            blocks += 1;
            if blocks > BLOCK_LIMIT {
                scan.partial
                    .push(partial("block_limit", "block budget reached"));
                break;
            }
            append_break(&mut scan.text, &mut pending_space);
        }
        index = end + 1;
    }
    if scan.styles.unsupported {
        scan.partial.push(partial(
            "unsupported_dependency",
            "active or unsupported markup is not rendered",
        ));
    }
    Ok(scan)
}

pub(super) fn append_break(text: &mut TextFacts, pending_space: &mut bool) {
    *pending_space = false;
    if !text.text.is_empty() && !text.text.ends_with('\n') && text.text.len() < TEXT_LIMIT {
        text.text.push('\n');
    }
}

pub(super) fn append_html_text(
    value: &str,
    text: &mut TextFacts,
    pending_space: &mut bool,
) -> bool {
    let mut unsupported = false;
    let mut cursor = 0usize;
    while cursor < value.len() {
        let mut character = value[cursor..].chars().next().unwrap_or('\u{fffd}');
        let mut consumed = character.len_utf8();
        if character == '&' {
            let tail = &value[cursor + 1..];
            if let Some(end) = tail
                .as_bytes()
                .iter()
                .take(33)
                .position(|byte| *byte == b';')
            {
                let entity = &tail[..end];
                let decoded = decode_entity(entity);
                if let Some(decoded) = decoded {
                    character = decoded;
                    consumed = end + 2;
                } else {
                    unsupported = true;
                }
            }
        }
        cursor += consumed;
        if character.is_whitespace() {
            *pending_space = !text.text.is_empty() && !text.text.ends_with('\n');
            continue;
        }
        let needed = character.len_utf8() + usize::from(*pending_space);
        if text.text.len() + needed > TEXT_LIMIT {
            text.truncated = true;
            return unsupported;
        }
        if *pending_space {
            text.text.push(' ');
            *pending_space = false;
        }
        text.text.push(character);
    }
    unsupported
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composite::{Completeness, Representation, Resource, ResourceBinding};

    fn identity() -> AnalysisIdentity {
        AnalysisIdentity {
            entry_id: 1,
            authority: "authority".into(),
            generation: 3,
            owner: "history".into(),
            capture_id: "cap".into(),
            content_id: "content".into(),
            archive_revision: 1,
            archive_digest: "digest".into(),
        }
    }

    fn deadline() -> Instant {
        Instant::now() + std::time::Duration::from_secs(4)
    }

    fn bundle(
        html: &str,
        bindings: Vec<ResourceBinding>,
        resources: Vec<Resource>,
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

    fn request(bundle: PortableBundle) -> AnalysisRequest {
        AnalysisRequest {
            bundle,
            provenance: vec![RepresentationSource::Original],
            native_only: false,
            native: vec![],
            native_items: vec![],
            decode_images: true,
            document: false,
        }
    }

    #[test]
    fn rtf_document_maps_only_proven_positional_emphasis() {
        let bytes = br"{\rtf1 A{\b B{\i C}D}E\ul F\ul0 G}";
        let mut input = request(PortableBundle {
            representations: vec![Representation {
                mime: Mime::Rtf,
                bytes: bytes.to_vec(),
            }],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::Complete,
        });
        input.document = true;
        let analysis = analyze(identity(), &input, &Cancellation::default(), deadline()).unwrap();
        assert_eq!(analysis.text.text, "ABCDEFG");
        let document = analysis.document.unwrap();
        let runs = &document.blocks[0].paragraph.as_ref().unwrap().runs;
        assert_eq!(
            runs.iter().map(|run| run.text.as_str()).collect::<Vec<_>>(),
            vec!["A", "B", "C", "D", "E", "F", "G"]
        );
        assert!(!runs[0].style.bold);
        assert!(runs[1].style.bold);
        assert!(runs[2].style.bold && runs[2].style.italic);
        assert!(runs[3].style.bold && !runs[3].style.italic);
        assert!(!runs[4].style.bold);
        assert!(runs[5].style.underline);
        assert!(!runs[6].style.underline);
    }

    #[test]
    fn rtf_document_does_not_borrow_styles_when_native_body_disagrees() {
        let mut input = request(PortableBundle {
            representations: vec![Representation {
                mime: Mime::Rtf,
                bytes: br"{\rtf1\b original}".to_vec(),
            }],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::Complete,
        });
        let saved = crate::protocol::content_analysis::scan_rtf(br"{\rtf1 different}");
        input.native.push(NativeAnalysisFacts::from_saved(saved, 0));
        input.document = true;
        let analysis = analyze(identity(), &input, &Cancellation::default(), deadline()).unwrap();
        let document = analysis.document.unwrap();
        let paragraph = document.blocks[0].paragraph.as_ref().unwrap();
        assert_eq!(paragraph.runs[0].text, "different");
        assert!(!paragraph.runs[0].style.bold);
        assert!(document.partial.iter().any(|reason| reason.code
            == super::super::display_document::DocumentPartialCode::NativeStylesUnavailable));
    }

    #[test]
    fn rtf_document_maps_matching_native_unicode_without_applying_global_style_bits() {
        let bytes = br"{\rtf1\uc1 \u20013?{\b \u-10179?\u-8638?}\u25991?}";
        let mut input = request(PortableBundle {
            representations: vec![Representation {
                mime: Mime::Rtf,
                bytes: bytes.to_vec(),
            }],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::Complete,
        });
        input.native.push(NativeAnalysisFacts::from_saved(
            crate::protocol::content_analysis::scan_rtf(bytes),
            0,
        ));
        input.document = true;
        let analysis = analyze(identity(), &input, &Cancellation::default(), deadline()).unwrap();
        let document = analysis.document.unwrap();
        let runs = &document.blocks[0].paragraph.as_ref().unwrap().runs;
        assert_eq!(runs.len(), 3);
        assert_eq!(runs[1].text, "🙂");
        assert!(!runs[0].style.bold && runs[1].style.bold && !runs[2].style.bold);
        assert!(!document.partial.iter().any(|reason| reason.code
            == super::super::display_document::DocumentPartialCode::NativeStylesUnavailable));
    }

    #[test]
    fn rtf_without_native_body_preserves_its_warning_in_the_document() {
        let mut input = request(PortableBundle {
            representations: vec![crate::protocol::composite::Representation {
                mime: Mime::Rtf,
                bytes: b"{\\rtf1\\ansi\\unknown saved}".to_vec(),
            }],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::Complete,
        });
        input.document = true;
        let analysis = analyze(
            identity(),
            &input,
            &Cancellation::default(),
            Instant::now() + std::time::Duration::from_secs(1),
        )
        .unwrap();
        assert!(analysis
            .partial
            .iter()
            .any(|reason| reason.code == "native_analysis_unavailable"));
        let document = analysis.document.unwrap();
        assert!(document.blocks.is_empty());
        assert!(document.partial.iter().any(|reason| reason.code
            == super::super::display_document::DocumentPartialCode::NativeAnalysisUnavailable));
    }

    #[test]
    fn image_order_follows_node_position_not_resource_storage_order() {
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
        let analysis = analyze(
            identity(),
            &request(bundle(html, bindings, resources)),
            &Cancellation::default(),
            deadline(),
        )
        .unwrap();
        let order: Vec<u16> = analysis
            .images
            .iter()
            .map(|image| image.resource_id.unwrap())
            .collect();
        assert_eq!(order, vec![2, 1]);
        assert_eq!(analysis.first_image, Some(0));
    }

    #[test]
    fn duplicate_reference_keeps_every_position() {
        let html = "<img src=\"tailsync-resource:1\"><p>x</p><img src=\"tailsync-resource:1\">";
        let bindings = [0usize, 1]
            .iter()
            .map(|occurrence| {
                let offset = html
                    .match_indices("tailsync-resource:1")
                    .nth(*occurrence)
                    .unwrap()
                    .0 as u32;
                ResourceBinding {
                    representation: 0,
                    resource: 1,
                    offset,
                    length: "tailsync-resource:1".len() as u32,
                }
            })
            .collect();
        let resources = vec![Resource {
            id: 1,
            png: png_1x1(),
        }];
        let analysis = analyze(
            identity(),
            &request(bundle(html, bindings, resources)),
            &Cancellation::default(),
            deadline(),
        )
        .unwrap();
        assert_eq!(analysis.images.len(), 2);
        assert_eq!(analysis.images[0].resource_id, Some(1));
        assert_eq!(analysis.images[1].resource_id, Some(1));
        assert_ne!(analysis.images[0].coordinate, analysis.images[1].coordinate);
    }

    #[test]
    fn first_image_missing_resource_is_still_located_and_not_replaced_by_the_second() {
        let html = "<img src=\"tailsync-resource:1\"><img src=\"tailsync-resource:2\">";
        let bindings = vec![
            ResourceBinding {
                representation: 0,
                resource: 1,
                offset: html.find("tailsync-resource:1").unwrap() as u32,
                length: "tailsync-resource:1".len() as u32,
            },
            ResourceBinding {
                representation: 0,
                resource: 2,
                offset: html.find("tailsync-resource:2").unwrap() as u32,
                length: "tailsync-resource:2".len() as u32,
            },
        ];
        let resources = vec![Resource {
            id: 2,
            png: png_1x1(),
        }];
        let analysis = analyze(
            identity(),
            &request(bundle(html, bindings, resources)),
            &Cancellation::default(),
            deadline(),
        )
        .unwrap();
        assert_eq!(analysis.first_image, Some(0));
        assert_eq!(
            analysis.images[0].availability,
            ImageAvailability::MissingResource
        );
        assert_eq!(
            analysis.images[1].availability,
            ImageAvailability::Available
        );
    }

    #[test]
    fn external_reference_is_not_fetched_and_not_body_text() {
        let html =
            "<img src=\"https://example.com/secret.png\" alt=\"a filename.png\"><p>real 正文</p>";
        let analysis = analyze(
            identity(),
            &request(bundle(html, vec![], vec![])),
            &Cancellation::default(),
            deadline(),
        )
        .unwrap();
        assert_eq!(analysis.images.len(), 1);
        assert_eq!(
            analysis.images[0].availability,
            ImageAvailability::ExternalReference
        );
        assert!(
            analysis.text.text.contains("real"),
            "{}",
            analysis.text.text
        );
        assert!(
            !analysis.text.text.contains("filename"),
            "{}",
            analysis.text.text
        );
        assert!(
            !analysis.text.text.contains("example.com"),
            "{}",
            analysis.text.text
        );
    }

    #[test]
    fn basic_styles_and_tables_are_reported() {
        let html = "<p><b>bold</b> <i>it</i></p><table><tr><td>a</td><td>b</td></tr><tr><td>c</td></tr></table>";
        let analysis = analyze(
            identity(),
            &request(bundle(html, vec![], vec![])),
            &Cancellation::default(),
            deadline(),
        )
        .unwrap();
        assert!(analysis.styles.bold);
        assert!(analysis.styles.italic);
        assert_eq!(analysis.tables.tables, 1);
        assert_eq!(analysis.tables.rows, 2);
        assert_eq!(analysis.tables.cells, 3);
        assert_eq!(analysis.tables.max_columns, 2);
    }

    #[test]
    fn deep_nesting_stops_at_the_bound_and_reports_partial() {
        let mut html = String::new();
        for _ in 0..(DEPTH_LIMIT + 8) {
            html.push_str("<div>");
        }
        html.push_str("deep");
        let analysis = analyze(
            identity(),
            &request(bundle(&html, vec![], vec![])),
            &Cancellation::default(),
            deadline(),
        )
        .unwrap();
        assert!(analysis
            .partial
            .iter()
            .any(|reason| reason.code == "depth_limit"));
    }

    #[test]
    fn native_only_without_facts_is_unknown_and_fabricates_nothing() {
        let request = AnalysisRequest {
            bundle: PortableBundle {
                representations: vec![],
                resources: vec![],
                bindings: vec![],
                completeness: Completeness::Complete,
            },
            provenance: vec![],
            native_only: true,
            native: vec![],
            native_items: vec![],
            decode_images: true,
            document: false,
        };
        let analysis = analyze(identity(), &request, &Cancellation::default(), deadline()).unwrap();
        assert!(analysis.unknown);
        assert!(analysis.text.text.is_empty());
        assert!(analysis.images.is_empty());
        assert_eq!(analysis.selected, None);
    }

    #[test]
    fn native_facts_supply_body_styles_tables_and_utf16_attachment_position() {
        let request = AnalysisRequest {
            bundle: PortableBundle {
                representations: vec![],
                resources: vec![],
                bindings: vec![],
                completeness: Completeness::Complete,
            },
            provenance: vec![],
            native_only: true,
            native: vec![NativeAnalysisFacts {
                kind: SavedAnalysisKind::Rtfd,
                item: 0,
                source_format: "com.apple.flat-rtfd".into(),
                converter: "tailsync.rtfd-to-facts.v1".into(),
                body: "RTFD 中文\u{fffc}".into(),
                truncated: false,
                partial: false,
                style_bits: STYLE_BOLD | STYLE_UNDERLINE,
                table_rows: 2,
                table_cells: 3,
                attachments: vec![NativeAttachment {
                    utf16_position: 7,
                    state: AttachmentState::Available,
                    png: png_1x1(),
                }],
            }],
            native_items: vec![NativeItemFact {
                item: 0,
                formats: vec!["com.apple.flat-rtfd".into()],
            }],
            decode_images: true,
            document: false,
        };
        let analysis = analyze(identity(), &request, &Cancellation::default(), deadline()).unwrap();
        assert!(!analysis.unknown);
        assert_eq!(analysis.text.text, "RTFD 中文\u{fffc}");
        assert!(analysis.styles.bold);
        assert!(analysis.styles.underline);
        assert_eq!(analysis.tables.tables, 1);
        assert_eq!(analysis.tables.rows, 2);
        assert_eq!(analysis.tables.cells, 3);
        assert_eq!(analysis.first_image, Some(0));
        assert_eq!(
            analysis.images[0].coordinate,
            ImageCoordinate::Utf16Character { position: 7 }
        );
        assert_eq!(
            analysis.images[0].availability,
            ImageAvailability::Available
        );
    }

    #[test]
    fn cancelled_analysis_returns_cancelled_before_scanning() {
        let cancellation = Cancellation::default();
        cancellation.cancel();
        let error = analyze(
            identity(),
            &request(bundle("<p>x</p>", vec![], vec![])),
            &cancellation,
            deadline(),
        )
        .unwrap_err();
        assert_eq!(error, AnalysisError::Cancelled);
    }

    fn html_facts(html: &str) -> ContentAnalysis {
        analyze(
            identity(),
            &request(bundle(html, vec![], vec![])),
            &Cancellation::default(),
            deadline(),
        )
        .unwrap()
    }

    #[test]
    fn native_summary_is_not_parsed_body_and_a_portable_png_is_a_real_asset() {
        let mut input = request(bundle("<p>native format summary</p>", vec![], vec![]));
        input.native_only = true;
        let analysis = analyze(identity(), &input, &Cancellation::default(), deadline()).unwrap();
        assert!(analysis.unknown);
        assert!(analysis.text.text.is_empty());
        assert_eq!(analysis.selected, None);
        let mut input = request(bundle("", vec![], vec![]));
        input.bundle.representations = vec![Representation {
            mime: Mime::Png,
            bytes: png_1x1(),
        }];
        let analysis = analyze(identity(), &input, &Cancellation::default(), deadline()).unwrap();
        assert_eq!(analysis.first_image, Some(0));
        assert_eq!(
            analysis.images[0].coordinate,
            ImageCoordinate::Representation
        );
        assert_eq!(analysis.images[0].png, png_1x1());
    }

    #[test]
    fn html_entities_and_inline_whitespace_are_actual_body_text() {
        assert_eq!(
            html_facts("<p>&lt;&amp;&#x1F642;&#20013;&#25991;</p>")
                .text
                .text
                .trim(),
            "<&🙂中文"
        );
        assert_eq!(
            html_facts("<b>Hello</b> <i>world</i>").text.text.trim(),
            "Hello world"
        );
    }

    #[test]
    fn void_elements_and_raw_text_do_not_fabricate_structure() {
        let html = format!("<p>{}</p>", "line<br>".repeat(100));
        let analysis = html_facts(&html);
        assert_eq!(analysis.text.text.matches("line").count(), 100);
        assert!(!analysis
            .partial
            .iter()
            .any(|reason| reason.code == "depth_limit"));
        let analysis = html_facts("<script>const hidden='<b><table><tr><td>';</script><p>safe</p>");
        assert_eq!(analysis.text.text.trim(), "safe");
        assert!(!analysis.styles.bold);
        assert_eq!(analysis.tables.tables, 0);
    }

    #[test]
    fn first_image_without_src_is_not_replaced_by_the_second() {
        let analysis = html_facts("<img><img src='https://tailsync.invalid/blue'>");
        assert_eq!(analysis.images.len(), 2);
        assert_eq!(
            analysis.images[0].availability,
            ImageAvailability::MissingSource
        );
        assert_eq!(analysis.first_image, Some(0));
    }

    #[test]
    fn independent_png_cannot_replace_a_document_image() {
        let mut input = request(bundle(
            "<p>正文</p><img src='https://tailsync.invalid/missing'>",
            vec![],
            vec![],
        ));
        input.bundle.representations.push(Representation {
            mime: Mime::Png,
            bytes: png_1x1(),
        });
        let analysis = analyze(identity(), &input, &Cancellation::default(), deadline()).unwrap();
        assert_eq!(analysis.selected, Some(0));
        assert_eq!(analysis.text.text.trim(), "正文");
        assert_eq!(analysis.images.len(), 1);
        assert_eq!(
            analysis.images[0].availability,
            ImageAvailability::ExternalReference
        );
        assert!(analysis.images[0].png.is_empty());
    }

    #[test]
    fn native_alternative_item_does_not_contaminate_selected_body() {
        let mut input = request(bundle("<p>summary</p>", vec![], vec![]));
        input.native_only = true;
        let primary = NativeAnalysisFacts::from_saved(
            crate::protocol::content_analysis::scan_rtf(b"{\\rtf1 first}"),
            0,
        );
        let mut other = NativeAnalysisFacts::from_saved(
            crate::protocol::content_analysis::scan_rtf(b"{\\rtf1 second}"),
            1,
        );
        other.style_bits = STYLE_BOLD;
        other.attachments.push(NativeAttachment {
            utf16_position: 999,
            state: AttachmentState::Unsupported,
            png: vec![],
        });
        input.native = vec![other, primary];
        let analysis = analyze(identity(), &input, &Cancellation::default(), deadline()).unwrap();
        assert_eq!(analysis.text.text, "first");
        assert!(!analysis.styles.bold);
        assert!(analysis.images.is_empty());
        assert_eq!(analysis.selected_native.unwrap().item, 0);
    }

    #[test]
    fn width_and_text_limits_stop_during_scanning() {
        let analysis = html_facts(&"<br>".repeat(RUN_LIMIT + 1));
        assert!(analysis
            .partial
            .iter()
            .any(|reason| reason.code == "run_limit"));
        let analysis = html_facts(&format!(
            "<p>{}</p><img src='https://tailsync.invalid/late'>",
            "🙂".repeat(TEXT_LIMIT / 4 + 1)
        ));
        assert!(analysis.text.truncated);
        assert!(analysis.text.text.len() <= TEXT_LIMIT);
        assert!(analysis.images.is_empty());
    }

    #[test]
    fn unused_alternative_provenance_does_not_warn_about_the_displayed_body() {
        let mut input = request(bundle("<p>saved HTML body</p>", vec![], vec![]));
        input.bundle.representations.push(Representation {
            mime: Mime::Text,
            bytes: b"alternate text".to_vec(),
        });
        input.provenance = vec![
            RepresentationSource::Original,
            RepresentationSource::Unverified,
        ];
        let analysis = analyze(identity(), &input, &Cancellation::default(), deadline()).unwrap();
        assert_eq!(analysis.selected, Some(0));
        assert!(!analysis
            .partial
            .iter()
            .any(|reason| reason.code == "unverified_provenance"));
        input.provenance[0] = RepresentationSource::Unverified;
        let analysis = analyze(identity(), &input, &Cancellation::default(), deadline()).unwrap();
        assert!(analysis
            .partial
            .iter()
            .any(|reason| reason.code == "unverified_provenance"));
    }

    #[test]
    fn png_admission_is_cumulative_and_rejects_truncated_streams() {
        let valid = png_1x1();
        let mut pixels = DECODED_PIXEL_LIMIT;
        let mut asset_bytes = 0;
        assert_eq!(
            admit_png(&valid, &mut pixels, &mut asset_bytes),
            ImageAvailability::PixelBudget
        );
        pixels = 0;
        assert_eq!(
            admit_png(&valid[..33], &mut pixels, &mut asset_bytes),
            ImageAvailability::Unsupported
        );
        assert_eq!(
            admit_png(&valid, &mut pixels, &mut asset_bytes),
            ImageAvailability::Available
        );
        assert_eq!(pixels, 1);
        assert_eq!(asset_bytes, valid.len());
    }
}
