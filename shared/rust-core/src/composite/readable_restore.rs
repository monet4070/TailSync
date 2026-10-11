use super::available_restore::AvailableContent;
use super::content_analysis::{self, AnalysisIdentity, AnalysisRequest, ContentAnalysis};
use super::display_document::{
    DisplayDocument, DocumentBlockKind, DocumentColor, DocumentImageAvailability, DocumentListKind,
    DocumentParagraph, DocumentPartialCode, DocumentRun,
};
use super::{Completeness, Mime, PortableBundle, Representation, Resource, ResourceBinding};
use crate::cancellation::Cancellation;
use crate::protocol::ProtocolError;
use std::collections::BTreeMap;
use std::fmt::{self, Write};
use std::time::{Duration, Instant};

fn invalid() -> ProtocolError {
    ProtocolError::InvalidComposite
}

struct BoundedText {
    value: String,
    limit: usize,
}

impl BoundedText {
    fn new(mime: Mime) -> Self {
        Self {
            value: String::new(),
            limit: mime.limit(),
        }
    }

    fn append(&mut self, value: &str) -> Result<(), ProtocolError> {
        self.write_str(value).map_err(|_| invalid())
    }
}

impl Write for BoundedText {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        if value.len() > self.limit.saturating_sub(self.value.len()) {
            return Err(fmt::Error);
        }
        self.value.push_str(value);
        Ok(())
    }
}

fn analyze(bundle: &PortableBundle) -> Result<ContentAnalysis, ProtocolError> {
    bundle.validate()?;
    if bundle.completeness == Completeness::MissingFormats
        || bundle.representation(Mime::Html).is_none()
    {
        return Err(invalid());
    }
    let mut request = AnalysisRequest::for_display(bundle.clone(), false, vec![]);
    request.decode_images = true;
    request.document = true;
    let analysis = content_analysis::analyze(
        AnalysisIdentity {
            entry_id: 0,
            authority: String::new(),
            generation: 0,
            owner: "history".into(),
            capture_id: String::new(),
            content_id: String::new(),
            archive_revision: 0,
            archive_digest: String::new(),
        },
        &request,
        &Cancellation::default(),
        Instant::now() + Duration::from_secs(4),
    )
    .map_err(|_| invalid())?;
    let document = analysis.document.as_ref().ok_or_else(invalid)?;
    if analysis.text.truncated
        || analysis.tables.truncated
        || document.partial.iter().any(|partial| {
            matches!(
                partial.code,
                DocumentPartialCode::TextLimit
                    | DocumentPartialCode::BlockLimit
                    | DocumentPartialCode::RunLimit
                    | DocumentPartialCode::CellLimit
                    | DocumentPartialCode::ImageLimit
                    | DocumentPartialCode::PixelBudget
                    | DocumentPartialCode::DepthLimit
                    | DocumentPartialCode::MalformedHtml
                    | DocumentPartialCode::NativePartial
                    | DocumentPartialCode::UnverifiedProvenance
                    | DocumentPartialCode::OtherNativeItems
            )
        })
        || document.blocks.iter().any(|block| {
            block.table.as_ref().is_some_and(|table| table.truncated)
                || block.image.as_ref().is_some_and(|image| {
                    matches!(
                        image.availability,
                        DocumentImageAvailability::Unsupported
                            | DocumentImageAvailability::PixelBudget
                    )
                })
        })
    {
        return Err(invalid());
    }
    document.validate().map_err(|_| invalid())?;
    Ok(analysis)
}

fn html_text(output: &mut BoundedText, value: &str) -> Result<(), ProtocolError> {
    for character in value.chars() {
        match character {
            '&' => output.append("&amp;")?,
            '<' => output.append("&lt;")?,
            '>' => output.append("&gt;")?,
            '"' => output.append("&quot;")?,
            '\'' => output.append("&#39;")?,
            '\n' => output.append("<br>")?,
            _ => output.write_char(character).map_err(|_| invalid())?,
        }
    }
    Ok(())
}

fn html_run(output: &mut BoundedText, run: &DocumentRun) -> Result<(), ProtocolError> {
    if let Some(link) = &run.link {
        output.append("<a href=\"")?;
        html_text(output, link)?;
        output.append("\">")?;
    }
    let style = run.style;
    output.append("<span style=\"")?;
    if style.bold {
        output.append("font-weight:bold;")?;
    }
    if style.italic {
        output.append("font-style:italic;")?;
    }
    if style.underline || style.strikethrough {
        output.append("text-decoration:")?;
        if style.underline {
            output.append("underline ")?;
        }
        if style.strikethrough {
            output.append("line-through")?;
        }
        output.append(";")?;
    }
    if style.monospace {
        output.append("font-family:monospace;")?;
    }
    for (property, color) in [
        ("color", style.color),
        ("background-color", style.highlight),
    ] {
        if let Some(color) = color {
            write!(
                output,
                "{property}:#{:02x}{:02x}{:02x};",
                color.red, color.green, color.blue
            )
            .map_err(|_| invalid())?;
        }
    }
    output.append("\">")?;
    if style.subscript {
        output.append("<sub>")?;
    } else if style.superscript {
        output.append("<sup>")?;
    }
    html_text(output, &run.text)?;
    if style.subscript {
        output.append("</sub>")?;
    } else if style.superscript {
        output.append("</sup>")?;
    }
    output.append("</span>")?;
    if run.link.is_some() {
        output.append("</a>")?;
    }
    Ok(())
}

fn list_marker(paragraph: &DocumentParagraph, counters: &mut [usize; 9]) -> String {
    match paragraph.list {
        Some(DocumentListKind::Bullet) => "• ".into(),
        Some(DocumentListKind::Ordered) => {
            let depth = usize::from(paragraph.list_depth);
            counters[depth] += 1;
            counters[depth + 1..].fill(0);
            format!("{}. ", counters[depth])
        }
        None => {
            counters.fill(0);
            String::new()
        }
    }
}

pub fn prepare(bundle: &PortableBundle) -> Result<AvailableContent, ProtocolError> {
    let analysis = analyze(bundle)?;
    let document = analysis.document.as_ref().ok_or_else(invalid)?;
    let payload = super::document_build::document_asset_bytes(document, &analysis.images)
        .map_err(|_| invalid())?;
    let resources = document
        .assets
        .iter()
        .map(|asset| {
            let start = usize::try_from(asset.offset).map_err(|_| invalid())?;
            let length = usize::try_from(asset.length).map_err(|_| invalid())?;
            let end = start.checked_add(length).ok_or_else(invalid)?;
            Ok(Resource {
                id: asset.index.checked_add(1).ok_or_else(invalid)?,
                png: payload.get(start..end).ok_or_else(invalid)?.to_vec(),
            })
        })
        .collect::<Result<Vec<_>, ProtocolError>>()?;
    let mut html = BoundedText::new(Mime::Html);
    let mut text = BoundedText::new(Mime::Text);
    let mut bindings = Vec::new();
    let mut missing_images = 0_u16;
    let mut counters = [0_usize; 9];
    for block in &document.blocks {
        match block.kind {
            DocumentBlockKind::Paragraph => {
                let paragraph = block.paragraph.as_ref().ok_or_else(invalid)?;
                let marker = list_marker(paragraph, &mut counters);
                for _ in 0..paragraph.quote_depth {
                    html.append("<blockquote>")?;
                }
                let tag = if paragraph.code_block { "pre" } else { "p" };
                if let Some(level) = paragraph.heading {
                    write!(html, "<h{level}>").map_err(|_| invalid())?;
                } else {
                    write!(html, "<{tag}>").map_err(|_| invalid())?;
                }
                html_text(&mut html, &marker)?;
                text.append(&marker)?;
                for run in &paragraph.runs {
                    html_run(&mut html, run)?;
                    text.append(&run.text)?;
                }
                if let Some(level) = paragraph.heading {
                    write!(html, "</h{level}>").map_err(|_| invalid())?;
                } else {
                    write!(html, "</{tag}>").map_err(|_| invalid())?;
                }
                for _ in 0..paragraph.quote_depth {
                    html.append("</blockquote>")?;
                }
                text.append("\n")?;
            }
            DocumentBlockKind::Table => {
                html.append("<table>")?;
                for row in &block.table.as_ref().ok_or_else(invalid)?.rows {
                    html.append("<tr>")?;
                    for (index, cell) in row.cells.iter().enumerate() {
                        if index > 0 {
                            text.append("\t")?;
                        }
                        let tag = if cell.header { "th" } else { "td" };
                        write!(html, "<{tag}>").map_err(|_| invalid())?;
                        for run in &cell.runs {
                            html_run(&mut html, run)?;
                            text.append(&run.text)?;
                        }
                        write!(html, "</{tag}>").map_err(|_| invalid())?;
                    }
                    html.append("</tr>")?;
                    text.append("\n")?;
                }
                html.append("</table>")?;
            }
            DocumentBlockKind::Image => {
                let image = block.image.as_ref().ok_or_else(invalid)?;
                if let Some(asset) = image.asset {
                    let resource = asset.checked_add(1).ok_or_else(invalid)?;
                    html.append("<img src=\"")?;
                    let offset = html.value.len();
                    let reference = format!("tailsync-resource:{resource}");
                    html.append(&reference)?;
                    html.append("\">")?;
                    bindings.push(ResourceBinding {
                        representation: 1,
                        resource,
                        offset: u32::try_from(offset).map_err(|_| invalid())?,
                        length: reference.len() as u32,
                    });
                } else {
                    missing_images = missing_images.checked_add(1).ok_or_else(invalid)?;
                    html.append("<p>[🖼]</p>")?;
                    text.append("[🖼]\n")?;
                }
            }
        }
    }
    if analysis.text.text.trim().is_empty() && resources.is_empty() {
        return Err(invalid());
    }
    let projected = PortableBundle {
        representations: vec![
            Representation {
                mime: Mime::Text,
                bytes: text.value.into_bytes(),
            },
            Representation {
                mime: Mime::Html,
                bytes: html.value.into_bytes(),
            },
        ],
        resources,
        bindings,
        completeness: Completeness::Complete,
    };
    super::validate_for_publish(&projected)?;
    if !super::restore::dependencies_complete(&projected)? {
        return Err(invalid());
    }
    Ok(AvailableContent {
        bundle: projected,
        missing_images,
    })
}

type Palette = BTreeMap<(u8, u8, u8), usize>;

fn color_key(color: DocumentColor) -> (u8, u8, u8) {
    (color.red, color.green, color.blue)
}

fn palette(document: &DisplayDocument) -> Palette {
    let mut colors = Palette::new();
    for block in &document.blocks {
        let runs = block
            .paragraph
            .iter()
            .flat_map(|paragraph| paragraph.runs.iter())
            .chain(
                block
                    .table
                    .iter()
                    .flat_map(|table| table.rows.iter())
                    .flat_map(|row| row.cells.iter())
                    .flat_map(|cell| cell.runs.iter()),
            );
        for run in runs {
            for color in [run.style.color, run.style.highlight].into_iter().flatten() {
                colors.entry(color_key(color)).or_insert(0);
            }
        }
    }
    for (index, value) in colors.values_mut().enumerate() {
        *value = index + 1;
    }
    colors
}

fn rtf_text(output: &mut BoundedText, value: &str) -> Result<(), ProtocolError> {
    for unit in value.encode_utf16() {
        match unit {
            92 => output.append("\\\\")?,
            123 => output.append("\\{")?,
            125 => output.append("\\}")?,
            10 => output.append("\\line ")?,
            13 => {}
            9 => output.append("\\tab ")?,
            32..=126 => output
                .write_char(char::from_u32(u32::from(unit)).ok_or_else(invalid)?)
                .map_err(|_| invalid())?,
            _ => write!(output, "\\u{}?", unit as i16).map_err(|_| invalid())?,
        }
    }
    Ok(())
}

fn rtf_run(
    output: &mut BoundedText,
    run: &DocumentRun,
    colors: &Palette,
) -> Result<(), ProtocolError> {
    output.append("{")?;
    for (enabled, control) in [
        (run.style.bold, "\\b "),
        (run.style.italic, "\\i "),
        (run.style.underline, "\\ul "),
        (run.style.strikethrough, "\\strike "),
        (run.style.subscript, "\\sub "),
        (run.style.superscript, "\\super "),
        (run.style.monospace, "\\f1 "),
    ] {
        if enabled {
            output.append(control)?;
        }
    }
    for (control, color) in [("cf", run.style.color), ("highlight", run.style.highlight)] {
        if let Some(color) = color {
            let index = colors.get(&color_key(color)).ok_or_else(invalid)?;
            write!(output, "\\{control}{index} ").map_err(|_| invalid())?;
        }
    }
    rtf_text(output, &run.text)?;
    output.append("}")
}

pub fn rtf(bundle: &PortableBundle) -> Result<Vec<u8>, ProtocolError> {
    let analysis = analyze(bundle)?;
    let document = analysis.document.as_ref().ok_or_else(invalid)?;
    let payload = super::document_build::document_asset_bytes(document, &analysis.images)
        .map_err(|_| invalid())?;
    let colors = palette(document);
    let mut output = BoundedText::new(Mime::Rtf);
    output.append("{\\rtf1\\ansi\\ansicpg1252\\deff0\\uc1{\\fonttbl{\\f0\\fswiss Helvetica;}{\\f1\\fmodern Menlo;}}{\\colortbl;")?;
    for (red, green, blue) in colors.keys() {
        write!(output, "\\red{red}\\green{green}\\blue{blue};").map_err(|_| invalid())?;
    }
    output.append("}\\f0\\fs24 ")?;
    let mut counters = [0_usize; 9];
    for block in &document.blocks {
        match block.kind {
            DocumentBlockKind::Paragraph => {
                let paragraph = block.paragraph.as_ref().ok_or_else(invalid)?;
                output.append("\\pard ")?;
                if let Some(level) = paragraph.heading {
                    write!(output, "\\b\\fs{} ", 44 - u32::from(level) * 4)
                        .map_err(|_| invalid())?;
                } else {
                    output.append("\\fs24 ")?;
                }
                if paragraph.code_block {
                    output.append("\\f1 ")?;
                }
                let indent = u32::from(paragraph.quote_depth + paragraph.list_depth) * 360;
                write!(output, "\\li{indent} ").map_err(|_| invalid())?;
                rtf_text(&mut output, &list_marker(paragraph, &mut counters))?;
                for run in &paragraph.runs {
                    rtf_run(&mut output, run, &colors)?;
                }
                output.append("\\par\\plain\\f0\\fs24 ")?;
            }
            DocumentBlockKind::Table => {
                for row in &block.table.as_ref().ok_or_else(invalid)?.rows {
                    output.append("\\pard ")?;
                    for (index, cell) in row.cells.iter().enumerate() {
                        if index > 0 {
                            output.append("\\tab ")?;
                        }
                        for run in &cell.runs {
                            rtf_run(&mut output, run, &colors)?;
                        }
                    }
                    output.append("\\par ")?;
                }
            }
            DocumentBlockKind::Image => {
                let image = block.image.as_ref().ok_or_else(invalid)?;
                if let Some(index) = image.asset {
                    let asset = document
                        .assets
                        .get(usize::from(index))
                        .ok_or_else(invalid)?;
                    let start = usize::try_from(asset.offset).map_err(|_| invalid())?;
                    let length = usize::try_from(asset.length).map_err(|_| invalid())?;
                    let end = start.checked_add(length).ok_or_else(invalid)?;
                    let png = payload.get(start..end).ok_or_else(invalid)?;
                    output.append("{\\pict\\pngblip")?;
                    write!(
                        output,
                        "\\picw{}\\pich{}\\picwgoal{}\\pichgoal{} ",
                        asset.width,
                        asset.height,
                        asset.width * 15,
                        asset.height * 15
                    )
                    .map_err(|_| invalid())?;
                    for byte in png {
                        write!(output, "{byte:02x}").map_err(|_| invalid())?;
                    }
                    output.append("}\\par ")?;
                } else {
                    rtf_text(&mut output, "[🖼]")?;
                    output.append("\\par ")?;
                }
            }
        }
    }
    output.append("}")?;
    Ok(output.value.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundle(html: &str) -> PortableBundle {
        PortableBundle {
            representations: vec![Representation {
                mime: Mime::Html,
                bytes: html.as_bytes().to_vec(),
            }],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::ExternalReferences,
        }
    }

    #[test]
    fn styles_and_colors_survive_without_loading_page_stylesheets() {
        let source = bundle("<style>@import 'https://fixture.invalid/site.css';</style><h2>Title</h2><p style='color:#123456;background-color:#abcdef'><b>bold</b> <i>italic</i> <u>underlined</u></p>");
        let before = source.encode().unwrap();
        let restored = prepare(&source).unwrap();
        let html =
            std::str::from_utf8(restored.bundle.representation(Mime::Html).unwrap()).unwrap();
        assert!(html.contains("<h2>"));
        assert!(html.contains("font-weight:bold"));
        assert!(html.contains("font-style:italic"));
        assert!(html.contains("underline"));
        assert!(html.contains("color:#123456"));
        assert!(!html.contains("fixture.invalid"));
        assert_eq!(restored.missing_images, 0);
        assert!(super::super::restore::dependencies_complete(&restored.bundle).unwrap());
        let native = String::from_utf8(rtf(&restored.bundle).unwrap()).unwrap();
        assert!(native.contains("\\red18\\green52\\blue86"));
        assert!(native.contains("\\highlight"));
        assert_eq!(source.encode().unwrap(), before);
    }

    #[test]
    fn missing_images_remain_in_body_order_and_do_not_invent_saved_images() {
        let restored = prepare(&bundle(
            "<p>before</p><img src='https://fixture.invalid/missing'><p>after</p>",
        ))
        .unwrap();
        assert_eq!(restored.missing_images, 1);
        assert!(restored.bundle.resources.is_empty());
        assert_eq!(restored.bundle.text().unwrap(), "before\n[🖼]\nafter\n");
    }

    #[test]
    fn truncation_or_empty_missing_image_is_not_a_complete_compatible_document() {
        let oversized =
            "<p>body</p>".repeat(super::super::display_document::DOCUMENT_BLOCK_LIMIT + 1);
        assert!(prepare(&bundle(&oversized)).is_err());
        assert!(prepare(&bundle("<img src='https://fixture.invalid/missing'>")).is_err());
        let mut incomplete = bundle("<p>body</p>");
        incomplete.completeness = Completeness::MissingFormats;
        assert!(prepare(&incomplete).is_err());
    }

    #[test]
    fn rtf_escapes_unicode_and_literal_control_sequences() {
        let result = rtf(&bundle("<p>\\pict { braces } 中文 🙂</p>")).unwrap();
        let result = String::from_utf8(result).unwrap();
        assert!(result.contains("\\\\pict \\{ braces \\}"));
        assert!(result.contains("\\u20013?"));
        assert!(result.contains("\\u-10179?\\u-8638?"));
        assert!(!result.contains("{\\pict"));
    }

    #[test]
    fn active_content_is_not_exported_with_readable_body() {
        let source = bundle("<script>secret_code()</script><p>safe &lt;img src=bad&gt;</p><iframe src='https://fixture.invalid/private'></iframe>");
        let restored = prepare(&source).unwrap();
        let html =
            std::str::from_utf8(restored.bundle.representation(Mime::Html).unwrap()).unwrap();
        assert!(html.contains("safe &lt;img src=bad&gt;"));
        assert!(!html.contains("secret_code"));
        assert!(!html.contains("iframe"));
        assert!(!html.contains("fixture.invalid"));
    }
}
