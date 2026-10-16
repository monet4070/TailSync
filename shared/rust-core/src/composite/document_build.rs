use super::content_analysis::{
    AnalysisError, ImageAvailability, ImagePlacement, NativeAnalysisFacts, PartialReason, TextFacts,
};
use super::display_document::{
    DisplayDocument, DocumentAsset, DocumentBlock, DocumentBlockKind, DocumentColor, DocumentImage,
    DocumentImageAvailability, DocumentListKind, DocumentParagraph, DocumentPartial,
    DocumentPartialCode, DocumentRun, DocumentRunStyle, DocumentTable, DocumentTableCell,
    DocumentTableRow, DOCUMENT_BLOCK_LIMIT, DOCUMENT_CELL_LIMIT, DOCUMENT_DETAIL_LIMIT,
    DOCUMENT_LINK_LIMIT, DOCUMENT_RUN_LIMIT, DOCUMENT_TEXT_LIMIT, DOCUMENT_VERSION,
};
use super::html_lex::{attribute_span, decode_entity, find_tag_end};
use crate::cancellation::Cancellation;
use crate::protocol::content_analysis::AttachmentState;
use std::time::{Duration, Instant};

const TAG_DEPTH_LIMIT: usize = 64;

fn partial_code(reason: &str) -> DocumentPartialCode {
    match reason {
        "text_limit" => DocumentPartialCode::TextLimit,
        "block_limit" => DocumentPartialCode::BlockLimit,
        "run_limit" => DocumentPartialCode::RunLimit,
        "cell_limit" => DocumentPartialCode::CellLimit,
        "image_limit" => DocumentPartialCode::ImageLimit,
        "pixel_budget" => DocumentPartialCode::PixelBudget,
        "depth_limit" => DocumentPartialCode::DepthLimit,
        "malformed_html" => DocumentPartialCode::MalformedHtml,
        "unsupported_markup" => DocumentPartialCode::UnsupportedMarkup,
        "unsupported_dependency" => DocumentPartialCode::UnsupportedDependency,
        "missing_image_source" => DocumentPartialCode::MissingImageSource,
        "external_reference" => DocumentPartialCode::ExternalReference,
        "native_analysis_unavailable" => DocumentPartialCode::NativeAnalysisUnavailable,
        "native_partial" => DocumentPartialCode::NativePartial,
        "unverified_provenance" => DocumentPartialCode::UnverifiedProvenance,
        "other_native_items" => DocumentPartialCode::OtherNativeItems,
        _ => DocumentPartialCode::UnsupportedMarkup,
    }
}

fn availability(value: ImageAvailability) -> DocumentImageAvailability {
    match value {
        ImageAvailability::Available => DocumentImageAvailability::Available,
        ImageAvailability::MissingResource => DocumentImageAvailability::MissingResource,
        ImageAvailability::MissingSource => DocumentImageAvailability::MissingSource,
        ImageAvailability::ExternalReference => DocumentImageAvailability::ExternalReference,
        ImageAvailability::Unsupported => DocumentImageAvailability::Unsupported,
        ImageAvailability::PixelBudget => DocumentImageAvailability::PixelBudget,
    }
}

fn png_dimensions(png: &[u8]) -> Option<(u32, u32)> {
    if png.len() < 24 || &png[..8] != b"\x89PNG\r\n\x1a\n" || &png[8..16] != b"\0\0\0\rIHDR" {
        return None;
    }
    let width = u32::from_be_bytes(png[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(png[20..24].try_into().ok()?);
    (width > 0 && height > 0).then_some((width, height))
}

pub fn asset_digest(png: &[u8]) -> String {
    hex::encode(ring::digest::digest(&ring::digest::SHA256, png).as_ref())
}

fn assign_assets(
    blocks: &mut [DocumentBlock],
    images: &[ImagePlacement],
) -> (Vec<DocumentAsset>, Option<u16>) {
    let mut assets: Vec<DocumentAsset> = Vec::new();
    let mut first_image = None;
    let mut first_seen = false;
    let mut pixels = 0u64;
    for block in blocks.iter_mut() {
        if block.kind != DocumentBlockKind::Image {
            continue;
        }
        let Some(image) = block.image.as_mut() else {
            continue;
        };
        let is_first = !first_seen;
        first_seen = true;
        let png = images
            .get(image.ordinal as usize)
            .filter(|placement| placement.availability == ImageAvailability::Available)
            .map(|placement| placement.png.as_slice())
            .filter(|png| !png.is_empty());
        let Some(png) = png else {
            if image.availability == DocumentImageAvailability::Available {
                image.availability = DocumentImageAvailability::MissingSource;
            }
            if is_first {
                first_image = None;
            }
            continue;
        };
        let Some((width, height)) = png_dimensions(png) else {
            image.availability = DocumentImageAvailability::Unsupported;
            if is_first {
                first_image = None;
            }
            continue;
        };
        let sha256 = asset_digest(png);
        let index = match assets.iter().position(|asset| asset.sha256 == sha256) {
            Some(index) => index as u16,
            None => {
                let cost = u64::from(width) * u64::from(height);
                if cost > super::display_document::DOCUMENT_PIXEL_LIMIT.saturating_sub(pixels) {
                    image.availability = DocumentImageAvailability::PixelBudget;
                    continue;
                }
                pixels += cost;
                let offset = assets
                    .last()
                    .map(|asset| asset.offset + asset.length)
                    .unwrap_or(0);
                assets.push(DocumentAsset {
                    index: assets.len() as u16,
                    offset,
                    length: png.len() as u64,
                    width,
                    height,
                    sha256,
                });
                (assets.len() - 1) as u16
            }
        };
        image.asset = Some(index);
        if is_first {
            first_image = Some(index);
        }
    }
    (assets, first_image)
}

fn push_partial(partial: &mut Vec<DocumentPartial>, code: DocumentPartialCode, detail: &str) {
    let mut detail = detail.to_string();
    if detail.len() > DOCUMENT_DETAIL_LIMIT {
        let mut end = DOCUMENT_DETAIL_LIMIT;
        while !detail.is_char_boundary(end) {
            end -= 1;
        }
        detail.truncate(end);
    }
    if partial
        .iter()
        .any(|reason| reason.code == code && reason.detail == detail)
    {
        return;
    }
    partial.push(DocumentPartial { code, detail });
}

fn map_reasons(partial: &mut Vec<DocumentPartial>, reasons: &[PartialReason]) {
    for reason in reasons {
        push_partial(partial, partial_code(reason.code), &reason.detail);
    }
}

struct PendingRuns {
    runs: Vec<DocumentRun>,
    text: String,
    style: DocumentRunStyle,
    link: Option<String>,
    verbatim: bool,
    unsupported: bool,
    overflow: bool,
    run_overflow: bool,
    retained_bytes: usize,
    pending_space: bool,
}

impl PendingRuns {
    fn new() -> Self {
        Self {
            runs: Vec::new(),
            text: String::new(),
            style: DocumentRunStyle::default(),
            link: None,
            verbatim: false,
            unsupported: false,
            overflow: false,
            run_overflow: false,
            retained_bytes: 0,
            pending_space: false,
        }
    }

    fn apply(&mut self, style: DocumentRunStyle, link: Option<String>, verbatim: bool) {
        if style != self.style || link != self.link || verbatim != self.verbatim {
            if self.pending_space && !self.text.is_empty() {
                self.append_char(' ');
                self.pending_space = false;
            }
            self.flush();
            self.style = style;
            self.link = link;
            self.verbatim = verbatim;
        }
    }

    fn flush(&mut self) {
        if self.text.is_empty() {
            return;
        }
        if self.runs.len() >= DOCUMENT_RUN_LIMIT {
            self.run_overflow = true;
            self.text.clear();
            return;
        }
        let bytes = self.text.len() + self.link.as_ref().map_or(0, String::len);
        if bytes > DOCUMENT_TEXT_LIMIT.saturating_sub(self.retained_bytes) {
            self.overflow = true;
            self.text.clear();
            return;
        }
        self.retained_bytes += bytes;
        self.runs.push(DocumentRun {
            text: std::mem::take(&mut self.text),
            style: self.style,
            link: self.link.clone(),
            unsupported: self.unsupported,
        });
        self.unsupported = false;
    }

    fn append_char(&mut self, character: char) {
        if self.overflow || self.run_overflow {
            return;
        }
        if self.runs.len() >= DOCUMENT_RUN_LIMIT {
            self.run_overflow = true;
            return;
        }
        let bytes =
            self.text.len() + character.len_utf8() + self.link.as_ref().map_or(0, String::len);
        if bytes > DOCUMENT_TEXT_LIMIT.saturating_sub(self.retained_bytes) {
            self.overflow = true;
            return;
        }
        self.text.push(character);
    }

    fn push_char(&mut self, character: char) {
        if self.verbatim {
            self.append_char(character);
            return;
        }
        if character.is_whitespace() {
            self.pending_space = !self.text.is_empty() || !self.runs.is_empty();
            return;
        }
        if self.pending_space {
            self.append_char(' ');
            self.pending_space = false;
        }
        self.append_char(character);
    }

    fn push_break(&mut self) {
        if self.verbatim {
            self.append_char('\n');
            return;
        }
        self.pending_space = false;
        if !self.text.is_empty()
            && !self.text.ends_with('\n')
            && self.text.len() < DOCUMENT_TEXT_LIMIT
        {
            self.append_char('\n');
        }
    }

    fn is_empty(&self) -> bool {
        self.runs.is_empty() && self.text.is_empty()
    }

    fn into_runs(mut self) -> Vec<DocumentRun> {
        self.flush();
        self.runs
    }
}

fn feed_html_text(runs: &mut PendingRuns, value: &str) -> bool {
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
                match decode_entity(entity) {
                    Some(decoded) => {
                        character = decoded;
                        consumed = end + 2;
                    }
                    None => unsupported = true,
                }
            }
        }
        cursor += consumed;
        runs.push_char(character);
        if runs.overflow || runs.run_overflow {
            break;
        }
    }
    unsupported
}

fn parse_color(value: &str) -> Option<DocumentColor> {
    let value = value.trim();
    if value.len() > 256 {
        return None;
    }
    let normalized = value.to_ascii_lowercase();
    let named = match normalized.as_str() {
        "black" => Some([0, 0, 0]),
        "white" => Some([255, 255, 255]),
        "red" => Some([255, 0, 0]),
        "green" => Some([0, 128, 0]),
        "blue" => Some([0, 0, 255]),
        "yellow" => Some([255, 255, 0]),
        "gray" | "grey" => Some([128, 128, 128]),
        "orange" => Some([255, 165, 0]),
        "purple" => Some([128, 0, 128]),
        _ => None,
    };
    if let Some([red, green, blue]) = named {
        return Some(DocumentColor { red, green, blue });
    }
    if let Some(channels) = normalized
        .strip_prefix("rgb(")
        .and_then(|raw| raw.strip_suffix(')'))
    {
        let channels: Vec<_> = channels.split(',').map(str::trim).collect();
        if channels.len() != 3 {
            return None;
        }
        return Some(DocumentColor {
            red: channels[0].parse().ok()?,
            green: channels[1].parse().ok()?,
            blue: channels[2].parse().ok()?,
        });
    }
    let digits = value.strip_prefix('#')?;
    if !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let expand = |character: char| -> Option<u8> {
        let digit = character.to_digit(16)?;
        Some((digit * 17) as u8)
    };
    match digits.len() {
        3 => {
            let mut characters = digits.chars();
            Some(DocumentColor {
                red: expand(characters.next()?)?,
                green: expand(characters.next()?)?,
                blue: expand(characters.next()?)?,
            })
        }
        6 => Some(DocumentColor {
            red: u8::from_str_radix(&digits[0..2], 16).ok()?,
            green: u8::from_str_radix(&digits[2..4], 16).ok()?,
            blue: u8::from_str_radix(&digits[4..6], 16).ok()?,
        }),
        _ => None,
    }
}

fn apply_inline_style(value: &str, style: &mut DocumentRunStyle) -> bool {
    let mut ignored = false;
    for declaration in value.split(';') {
        let declaration = declaration.trim();
        if declaration.is_empty() {
            continue;
        }
        let Some((property, raw)) = declaration.split_once(':') else {
            ignored = true;
            continue;
        };
        let property = property.trim().to_ascii_lowercase();
        let raw = raw.trim().to_ascii_lowercase();
        let raw = raw.strip_suffix("!important").unwrap_or(&raw).trim();
        match property.as_str() {
            "color" => match parse_color(raw) {
                Some(color) => style.color = Some(color),
                None => ignored = true,
            },
            "background-color" => match parse_color(raw) {
                Some(color) => style.highlight = Some(color),
                None => ignored = true,
            },
            "font-family" => {
                if is_monospace_family(raw) {
                    style.monospace = true;
                } else {
                    ignored = true;
                }
            }
            "font-weight" => match raw {
                "bold" | "bolder" => style.bold = true,
                "normal" | "lighter" => style.bold = false,
                _ => match raw.parse::<u16>() {
                    Ok(weight) if (1..=1000).contains(&weight) => style.bold = weight >= 600,
                    _ => ignored = true,
                },
            },
            "font-style" => match raw {
                "italic" | "oblique" => style.italic = true,
                "normal" => style.italic = false,
                _ => ignored = true,
            },
            "text-decoration" | "text-decoration-line" => {
                let tokens: Vec<_> = raw.split_whitespace().collect();
                if tokens
                    .iter()
                    .any(|token| matches!(*token, "none" | "underline" | "line-through"))
                {
                    style.underline = tokens.contains(&"underline");
                    style.strikethrough = tokens.contains(&"line-through");
                } else {
                    ignored = true;
                }
            }
            "vertical-align" => match raw {
                "sub" => {
                    style.subscript = true;
                    style.superscript = false;
                }
                "super" => {
                    style.superscript = true;
                    style.subscript = false;
                }
                "baseline" => {
                    style.subscript = false;
                    style.superscript = false;
                }
                _ => ignored = true,
            },
            _ => ignored = true,
        }
    }
    ignored
}

fn is_monospace_family(value: &str) -> bool {
    value
        .split(',')
        .map(|family| {
            family
                .trim()
                .trim_matches(|character| character == '\'' || character == '"')
                .trim()
                .to_ascii_lowercase()
        })
        .any(|family| {
            matches!(
                family.as_str(),
                "monospace"
                    | "ui-monospace"
                    | "sfmono-regular"
                    | "sf mono"
                    | "menlo"
                    | "monaco"
                    | "consolas"
                    | "courier"
                    | "courier new"
                    | "dejavu sans mono"
                    | "liberation mono"
                    | "andale mono"
                    | "lucida console"
                    | "source code pro"
                    | "fira code"
                    | "cascadia code"
                    | "cascadia mono"
                    | "jetbrains mono"
            )
        })
}

fn attribute_spans(
    bytes: &[u8],
    mut cursor: usize,
    end: usize,
    name: &[u8],
) -> Vec<Option<(usize, usize)>> {
    let mut spans = Vec::new();
    while cursor < end {
        while cursor < end && (bytes[cursor].is_ascii_whitespace() || bytes[cursor] == b'/') {
            cursor += 1;
        }
        let start = cursor;
        while cursor < end
            && !bytes[cursor].is_ascii_whitespace()
            && !matches!(bytes[cursor], b'=' | b'/' | b'>')
        {
            cursor += 1;
        }
        let matched = bytes[start..cursor].eq_ignore_ascii_case(name);
        while cursor < end && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        let mut value = None;
        if bytes.get(cursor) == Some(&b'=') {
            cursor += 1;
            while cursor < end && bytes[cursor].is_ascii_whitespace() {
                cursor += 1;
            }
            let quote = bytes
                .get(cursor)
                .copied()
                .filter(|byte| matches!(byte, b'\'' | b'"'));
            if quote.is_some() {
                cursor += 1;
            }
            let value_start = cursor;
            while cursor < end
                && match quote {
                    Some(quote) => bytes[cursor] != quote,
                    None => {
                        !bytes[cursor].is_ascii_whitespace()
                            && !matches!(bytes[cursor], b'/' | b'>')
                    }
                }
            {
                cursor += 1;
            }
            value = Some((value_start, cursor));
            if quote.is_some() && cursor < end {
                cursor += 1;
            }
        } else if cursor == start {
            cursor += 1;
        }
        if matched {
            spans.push(value);
        }
    }
    spans
}

fn decode_attribute_value(raw: &str) -> Option<String> {
    let mut decoded = String::with_capacity(raw.len());
    let mut cursor = 0usize;
    while cursor < raw.len() {
        let character = raw[cursor..].chars().next()?;
        if character == '&' {
            let tail = &raw[cursor + 1..];
            if let Some(end) = tail
                .as_bytes()
                .iter()
                .take(33)
                .position(|byte| *byte == b';')
            {
                match decode_entity(&tail[..end]) {
                    Some(value) => {
                        decoded.push(value);
                        cursor += end + 2;
                        continue;
                    }
                    None => return None,
                }
            }
        }
        decoded.push(character);
        cursor += character.len_utf8();
    }
    if decoded
        .chars()
        .any(|character| character.is_whitespace() || character.is_control() || character == '\\')
    {
        return None;
    }
    Some(decoded)
}

enum LinkResolution {
    Absent,
    Rejected,
    Attached(String),
}

fn absolute_link(value: &str, base: Option<&url::Url>) -> Option<String> {
    if value.is_empty()
        || value.len() > DOCUMENT_LINK_LIMIT
        || super::display_document::link_has_userinfo(value)
    {
        return None;
    }
    let absolute = match url::Url::parse(value) {
        Ok(url) if super::display_document::valid_document_link(value) => url,
        Ok(_) => return None,
        Err(url::ParseError::RelativeUrlWithoutBase) => base?.join(value).ok()?,
        Err(_) => return None,
    };
    let normalized = absolute.to_string();
    super::display_document::valid_document_link(&normalized).then_some(normalized)
}

fn resolve_link(
    bytes: &[u8],
    cursor: usize,
    end: usize,
    html: &str,
    base: Option<&url::Url>,
) -> LinkResolution {
    let spans = attribute_spans(bytes, cursor, end, b"href");
    if spans.len() != 1 {
        return if spans.is_empty() {
            LinkResolution::Absent
        } else {
            LinkResolution::Rejected
        };
    }
    let Some((start, finish)) = spans[0] else {
        return LinkResolution::Absent;
    };
    let Some(value) = decode_attribute_value(&html[start..finish]) else {
        return LinkResolution::Rejected;
    };
    match absolute_link(&value, base) {
        Some(url) => LinkResolution::Attached(url),
        None => LinkResolution::Rejected,
    }
}

struct PendingParagraph {
    list: Option<DocumentListKind>,
    list_depth: u8,
    heading: Option<u8>,
    quote_depth: u8,
    code_block: bool,
    runs: PendingRuns,
}

#[derive(Clone, Default)]
struct ElementState {
    style: DocumentRunStyle,
    link: Option<String>,
    heading: Option<u8>,
    quote_depth: u8,
    code_block: bool,
    list: Option<DocumentListKind>,
    list_depth: u8,
}

fn paragraph_from_state(state: &ElementState) -> PendingParagraph {
    let mut runs = PendingRuns::new();
    runs.apply(state.style, state.link.clone(), state.code_block);
    PendingParagraph {
        list: state.list,
        list_depth: state.list_depth,
        heading: state.heading,
        quote_depth: state.quote_depth,
        code_block: state.code_block,
        runs,
    }
}

struct PendingTable {
    rows: Vec<DocumentTableRow>,
    row: Vec<DocumentTableCell>,
    cell: Option<(bool, PendingRuns)>,
    truncated: bool,
}

struct HtmlDocBuilder<'a> {
    images: &'a [ImagePlacement],
    base: Option<url::Url>,
    images_seen: usize,
    blocks: Vec<DocumentBlock>,
    partial: Vec<DocumentPartial>,
    styles_partial: bool,
    runs_total: usize,
    cells_total: usize,
    text_bytes: usize,
    tag_stack: Vec<String>,
    element_stack: Vec<ElementState>,
    paragraph: Option<PendingParagraph>,
    table: Option<PendingTable>,
    raw_tag: Option<String>,
    stopped: bool,
}

impl<'a> HtmlDocBuilder<'a> {
    fn new(images: &'a [ImagePlacement], base: Option<url::Url>) -> Self {
        Self {
            images,
            base,
            images_seen: 0,
            blocks: Vec::new(),
            partial: Vec::new(),
            styles_partial: false,
            runs_total: 0,
            cells_total: 0,
            text_bytes: 0,
            tag_stack: Vec::new(),
            element_stack: vec![ElementState::default()],
            paragraph: None,
            table: None,
            raw_tag: None,
            stopped: false,
        }
    }

    fn current_state(&self) -> ElementState {
        self.element_stack.last().cloned().unwrap_or_default()
    }

    fn sync_active_runs(&mut self) {
        let state = self.current_state();
        self.active_runs()
            .apply(state.style, state.link, state.code_block);
    }

    fn add_partial(&mut self, code: DocumentPartialCode, detail: &str) {
        push_partial(&mut self.partial, code, detail);
    }

    fn count_run(&mut self, run: &DocumentRun) {
        self.runs_total = self.runs_total.saturating_add(1);
        self.text_bytes = self
            .text_bytes
            .saturating_add(run.text.len() + run.link.as_ref().map_or(0, |link| link.len()));
        if self.runs_total > DOCUMENT_RUN_LIMIT {
            self.add_partial(DocumentPartialCode::RunLimit, "run budget reached");
            self.stopped = true;
        } else if self.text_bytes > DOCUMENT_TEXT_LIMIT {
            self.add_partial(DocumentPartialCode::TextLimit, "text byte budget reached");
            self.stopped = true;
        }
    }

    fn collect_runs(&mut self, runs: PendingRuns) -> Vec<DocumentRun> {
        if runs.overflow {
            self.add_partial(DocumentPartialCode::TextLimit, "text byte budget reached");
        }
        if runs.run_overflow {
            self.add_partial(DocumentPartialCode::RunLimit, "run budget reached");
        }
        let mut collected = Vec::new();
        for mut run in runs.into_runs() {
            if self.runs_total >= DOCUMENT_RUN_LIMIT {
                self.add_partial(DocumentPartialCode::RunLimit, "run budget reached");
                self.stopped = true;
                break;
            }
            let available = DOCUMENT_TEXT_LIMIT.saturating_sub(self.text_bytes);
            if run.link.as_ref().map_or(0, |link| link.len()) > available {
                run.link = None;
                self.add_partial(DocumentPartialCode::TextLimit, "text byte budget reached");
            }
            let available = available - run.link.as_ref().map_or(0, |link| link.len());
            if run.text.len() > available {
                let mut end = available;
                while !run.text.is_char_boundary(end) {
                    end -= 1;
                }
                run.text.truncate(end);
                self.add_partial(DocumentPartialCode::TextLimit, "text byte budget reached");
                self.stopped = true;
            }
            self.count_run(&run);
            collected.push(run);
            if self.stopped {
                break;
            }
        }
        collected
    }

    fn push_block(&mut self, block: DocumentBlock) {
        if self.blocks.len() >= DOCUMENT_BLOCK_LIMIT {
            self.add_partial(DocumentPartialCode::BlockLimit, "block budget reached");
            self.stopped = true;
            return;
        }
        self.blocks.push(block);
    }

    fn flush_paragraph(&mut self) {
        let Some(paragraph) = self.paragraph.take() else {
            return;
        };
        if paragraph.runs.is_empty() {
            return;
        }
        let runs = self.collect_runs(paragraph.runs);
        if runs.is_empty() {
            return;
        }
        self.push_block(DocumentBlock {
            kind: DocumentBlockKind::Paragraph,
            paragraph: Some(DocumentParagraph {
                list: paragraph.list,
                heading: paragraph.heading,
                quote_depth: paragraph.quote_depth,
                code_block: paragraph.code_block,
                list_depth: paragraph.list_depth,
                runs,
            }),
            table: None,
            image: None,
        });
    }

    fn start_paragraph(&mut self) {
        self.flush_paragraph();
        let state = self.current_state();
        self.paragraph = Some(paragraph_from_state(&state));
    }

    fn active_runs(&mut self) -> &mut PendingRuns {
        if self
            .table
            .as_ref()
            .is_some_and(|table| table.cell.is_some())
        {
            return &mut self.table.as_mut().unwrap().cell.as_mut().unwrap().1;
        }
        if self.paragraph.is_none() {
            let state = self.current_state();
            self.paragraph = Some(paragraph_from_state(&state));
        }
        &mut self.paragraph.as_mut().unwrap().runs
    }

    fn push_text(&mut self, value: &str) {
        if self.stopped || self.raw_tag.is_some() {
            return;
        }
        let unsupported = feed_html_text(self.active_runs(), value);
        if unsupported {
            self.styles_partial = true;
            self.add_partial(
                DocumentPartialCode::UnsupportedMarkup,
                "an entity could not be represented",
            );
        }
    }

    fn push_break(&mut self) {
        if self.stopped {
            return;
        }
        self.active_runs().push_break();
    }

    fn push_image(&mut self) {
        if self.images_seen >= super::display_document::DOCUMENT_ASSET_LIMIT {
            self.add_partial(DocumentPartialCode::ImageLimit, "image budget reached");
            self.stopped = true;
            return;
        }
        if self.table.is_some() {
            self.flush_table();
            self.styles_partial = true;
            self.add_partial(
                DocumentPartialCode::UnsupportedMarkup,
                "table attachment layout is flattened",
            );
        }
        self.flush_paragraph();
        let ordinal = self.images_seen as u16;
        self.images_seen = self.images_seen.saturating_add(1);
        let availability = match self.images.get(ordinal as usize) {
            Some(placement) => availability(placement.availability),
            None => DocumentImageAvailability::MissingSource,
        };
        self.push_block(DocumentBlock {
            kind: DocumentBlockKind::Image,
            paragraph: None,
            table: None,
            image: Some(DocumentImage {
                asset: None,
                availability,
                ordinal,
            }),
        });
    }

    fn flush_cell(&mut self) {
        let Some(cell) = self.table.as_mut().and_then(|table| table.cell.take()) else {
            return;
        };
        let runs = self.collect_runs(cell.1);
        self.cells_total = self.cells_total.saturating_add(1);
        if self.cells_total > DOCUMENT_CELL_LIMIT {
            if let Some(table) = self.table.as_mut() {
                table.truncated = true;
            }
            self.add_partial(DocumentPartialCode::CellLimit, "table cell budget reached");
            self.stopped = true;
            return;
        }
        if let Some(table) = self.table.as_mut() {
            table.row.push(DocumentTableCell {
                header: cell.0,
                runs,
            });
        }
    }

    fn flush_row(&mut self) {
        self.flush_cell();
        let Some(table) = self.table.as_mut() else {
            return;
        };
        if table.row.is_empty() {
            return;
        }
        let row = std::mem::take(&mut table.row);
        table.rows.push(DocumentTableRow { cells: row });
    }

    fn flush_table(&mut self) {
        self.flush_row();
        let Some(table) = self.table.take() else {
            return;
        };
        if table.rows.is_empty() {
            return;
        }
        self.push_block(DocumentBlock {
            kind: DocumentBlockKind::Table,
            paragraph: None,
            table: Some(DocumentTable {
                rows: table.rows,
                truncated: table.truncated,
            }),
            image: None,
        });
    }

    fn open_element(&mut self, tag: &str, cursor: usize, end: usize, bytes: &[u8], html: &str) {
        let mut state = self.current_state();
        match tag {
            "b" | "strong" => state.style.bold = true,
            "i" | "em" => state.style.italic = true,
            "u" => state.style.underline = true,
            "s" | "strike" | "del" => state.style.strikethrough = true,
            "sub" => state.style.subscript = true,
            "sup" => state.style.superscript = true,
            "code" | "kbd" | "samp" => state.style.monospace = true,
            "mark" => {
                state.style.highlight = Some(DocumentColor {
                    red: 255,
                    green: 255,
                    blue: 0,
                });
            }
            "pre" => {
                state.code_block = true;
                state.style.monospace = true;
            }
            "h1" => state.heading = Some(1),
            "h2" => state.heading = Some(2),
            "h3" | "h4" | "h5" | "h6" => state.heading = Some(3),
            "blockquote" => {
                let depth = state.quote_depth.saturating_add(1);
                if depth > 8 {
                    self.add_partial(
                        DocumentPartialCode::DepthLimit,
                        "quote depth budget reached",
                    );
                }
                state.quote_depth = depth.min(8);
            }
            "ul" | "ol" => {
                let depth = if state.list.is_some() {
                    state.list_depth.saturating_add(1)
                } else {
                    0
                };
                if depth > 8 {
                    self.add_partial(DocumentPartialCode::DepthLimit, "list depth budget reached");
                }
                state.list_depth = depth.min(8);
                state.list = Some(if tag == "ol" {
                    DocumentListKind::Ordered
                } else {
                    DocumentListKind::Bullet
                });
            }
            "font" => {
                if let Some((start, finish)) = attribute_span(bytes, cursor, end, b"color") {
                    match parse_color(&html[start..finish]) {
                        Some(color) => state.style.color = Some(color),
                        None => {
                            self.styles_partial = true;
                        }
                    }
                }
                if let Some((start, finish)) = attribute_span(bytes, cursor, end, b"face") {
                    state.style.monospace |= is_monospace_family(&html[start..finish]);
                }
            }
            _ => {}
        }
        if let Some((start, finish)) = attribute_span(bytes, cursor, end, b"style") {
            if apply_inline_style(&html[start..finish], &mut state.style) {
                self.styles_partial = true;
            }
        }
        if tag == "a" {
            state.link = None;
            let resolution = resolve_link(bytes, cursor, end, html, self.base.as_ref());
            match resolution {
                LinkResolution::Attached(url) => state.link = Some(url),
                LinkResolution::Rejected => self.styles_partial = true,
                LinkResolution::Absent => {}
            }
        }
        self.element_stack.push(state);
        self.sync_active_runs();
    }

    fn close_tag(&mut self, tag: &str) {
        if let Some(position) = self.tag_stack.iter().rposition(|open| open == tag) {
            self.tag_stack.truncate(position);
            self.element_stack.truncate(position + 1);
        }
        match tag {
            "p" | "div" | "li" | "blockquote" | "pre" => self.flush_paragraph(),
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => self.flush_paragraph(),
            "td" | "th" => self.flush_cell(),
            "tr" => self.flush_row(),
            "table" => self.flush_table(),
            _ => {}
        }
        if self.stopped {
            return;
        }
        self.sync_active_runs();
    }

    fn open_tag(&mut self, tag: &str, cursor: usize, end: usize, bytes: &[u8], html: &str) {
        let void = matches!(
            tag,
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
        let self_closing = bytes.get(end.saturating_sub(1)) == Some(&b'/');
        if !void && !self_closing {
            if self.tag_stack.len() >= TAG_DEPTH_LIMIT {
                self.add_partial(
                    DocumentPartialCode::DepthLimit,
                    "element nesting budget reached",
                );
                self.stopped = true;
                return;
            }
            self.tag_stack.push(tag.to_string());
            self.open_element(tag, cursor, end, bytes, html);
        }
        match tag {
            "br" => self.push_break(),
            "img" => self.push_image(),
            "script" | "style" => {
                self.styles_partial = true;
                self.raw_tag = Some(tag.to_string());
                self.add_partial(
                    DocumentPartialCode::UnsupportedMarkup,
                    "active markup is not represented",
                );
            }
            "object" | "iframe" | "link" | "embed" => {
                self.styles_partial = true;
                self.add_partial(
                    DocumentPartialCode::UnsupportedDependency,
                    "an embedded dependency is not represented",
                );
            }
            "li" => self.start_paragraph(),
            "p" | "div" | "blockquote" | "pre" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.flush_paragraph()
            }
            "table" => {
                self.flush_paragraph();
                if self.table.is_some() {
                    self.add_partial(
                        DocumentPartialCode::UnsupportedMarkup,
                        "a nested table is flattened",
                    );
                } else {
                    self.table = Some(PendingTable {
                        rows: Vec::new(),
                        row: Vec::new(),
                        cell: None,
                        truncated: false,
                    });
                }
            }
            "tr" => self.flush_row(),
            "td" | "th" => {
                self.flush_cell();
                let state = self.current_state();
                if let Some(table) = self.table.as_mut() {
                    let mut runs = PendingRuns::new();
                    runs.apply(state.style, state.link, state.code_block);
                    table.cell = Some((tag == "th", runs));
                }
            }
            _ => {}
        }
    }

    fn walk(
        &mut self,
        html: &str,
        bytes: &[u8],
        cancellation: &Cancellation,
        deadline: Instant,
    ) -> Result<(), AnalysisError> {
        let mut index = 0usize;
        let mut nodes = 0usize;
        while index < bytes.len() && !self.stopped {
            cancellation.check().map_err(|_| AnalysisError::Cancelled)?;
            if Instant::now() >= deadline {
                return Err(AnalysisError::Deadline);
            }
            if bytes[index] != b'<' {
                let end = html[index..]
                    .find('<')
                    .map(|offset| index + offset)
                    .unwrap_or(bytes.len());
                self.push_text(&html[index..end]);
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
                self.add_partial(DocumentPartialCode::MalformedHtml, "unterminated element");
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
            if let Some(raw) = self.raw_tag.clone() {
                if closing && tag == raw {
                    self.raw_tag = None;
                    self.close_tag(&tag);
                }
                index = end + 1;
                continue;
            }
            nodes += 1;
            if nodes > DOCUMENT_RUN_LIMIT {
                self.add_partial(DocumentPartialCode::RunLimit, "element budget reached");
                self.stopped = true;
                break;
            }
            if closing {
                self.close_tag(&tag);
            } else {
                self.open_tag(&tag, cursor, end, bytes, html);
            }
            index = end + 1;
        }
        Ok(())
    }

    fn finish(mut self, reasons: &[PartialReason]) -> DisplayDocument {
        self.flush_paragraph();
        self.flush_table();
        map_reasons(&mut self.partial, reasons);
        let (assets, first_image) = assign_assets(&mut self.blocks, self.images);
        DisplayDocument {
            version: DOCUMENT_VERSION,
            blocks: self.blocks,
            assets,
            partial: self.partial,
            styles_partial: self.styles_partial,
            first_image,
        }
    }
}

pub fn document_from_html(
    bytes: &[u8],
    images: &[ImagePlacement],
    reasons: &[PartialReason],
) -> Result<DisplayDocument, AnalysisError> {
    document_from_html_checked(
        bytes,
        images,
        reasons,
        &Cancellation::default(),
        Instant::now() + Duration::from_secs(4),
    )
}

pub fn document_from_html_checked(
    bytes: &[u8],
    images: &[ImagePlacement],
    reasons: &[PartialReason],
    cancellation: &Cancellation,
    deadline: Instant,
) -> Result<DisplayDocument, AnalysisError> {
    if bytes.len() > crate::protocol::native_clipboard::NATIVE_LIMIT {
        return Err(AnalysisError::Invalid);
    }
    let html = std::str::from_utf8(bytes).map_err(|_| AnalysisError::Invalid)?;
    let base = super::html::document_link_base(bytes).ok().flatten();
    let mut builder = HtmlDocBuilder::new(images, base);
    builder.walk(html, bytes, cancellation, deadline)?;
    Ok(builder.finish(reasons))
}

fn text_paragraph(text: &str) -> DocumentParagraph {
    DocumentParagraph {
        list: None,
        heading: None,
        quote_depth: 0,
        code_block: false,
        list_depth: 0,
        runs: vec![DocumentRun {
            text: text.to_string(),
            style: DocumentRunStyle::default(),
            link: None,
            unsupported: false,
        }],
    }
}

pub fn document_from_text(text: &TextFacts) -> DisplayDocument {
    document_from_text_with_reasons(text, &[])
}

pub fn document_from_text_with_reasons(
    text: &TextFacts,
    reasons: &[PartialReason],
) -> DisplayDocument {
    let mut blocks = Vec::new();
    if !text.text.is_empty() {
        blocks.push(DocumentBlock {
            kind: DocumentBlockKind::Paragraph,
            paragraph: Some(text_paragraph(&text.text)),
            table: None,
            image: None,
        });
    }
    let mut partial = Vec::new();
    if text.truncated {
        push_partial(
            &mut partial,
            DocumentPartialCode::TextLimit,
            "text was truncated",
        );
    }
    map_reasons(&mut partial, reasons);
    DisplayDocument {
        version: DOCUMENT_VERSION,
        blocks,
        assets: Vec::new(),
        partial,
        styles_partial: false,
        first_image: None,
    }
}

pub fn document_from_native(
    facts: &NativeAnalysisFacts,
    images: &[ImagePlacement],
    reasons: &[PartialReason],
) -> DisplayDocument {
    let units: Vec<u16> = facts.body.encode_utf16().collect();
    let mut attachments: Vec<_> = facts.attachments.iter().collect();
    attachments.sort_by_key(|attachment| attachment.utf16_position);
    let mut blocks = Vec::new();
    let mut partial = Vec::new();
    let mut cursor = 0usize;
    let mut image_ordinal = 0u16;
    for attachment in &attachments {
        let position = attachment.utf16_position as usize;
        push_native_segment(&units, cursor, position, &mut blocks);
        let availability = match attachment.state {
            AttachmentState::Available => DocumentImageAvailability::Available,
            AttachmentState::Missing => DocumentImageAvailability::MissingSource,
            AttachmentState::Unsupported => DocumentImageAvailability::Unsupported,
            AttachmentState::PixelBudget => DocumentImageAvailability::PixelBudget,
        };
        blocks.push(DocumentBlock {
            kind: DocumentBlockKind::Image,
            paragraph: None,
            table: None,
            image: Some(DocumentImage {
                asset: None,
                availability,
                ordinal: image_ordinal,
            }),
        });
        image_ordinal = image_ordinal.saturating_add(1);
        cursor = position.saturating_add(1);
    }
    push_native_segment(&units, cursor, units.len(), &mut blocks);
    if blocks.len() > DOCUMENT_BLOCK_LIMIT {
        blocks.truncate(DOCUMENT_BLOCK_LIMIT);
        push_partial(
            &mut partial,
            DocumentPartialCode::BlockLimit,
            "block budget reached",
        );
    }
    let styles_partial = true;
    if styles_partial {
        push_partial(
            &mut partial,
            DocumentPartialCode::NativeStylesUnavailable,
            "native styles are not positional and were not applied to the whole body",
        );
    }
    if facts.table_rows > 0 || facts.table_cells > 0 {
        push_partial(
            &mut partial,
            DocumentPartialCode::NativeTableStructureUnavailable,
            "native tables are reported as counts, not cells",
        );
    }
    if facts.partial {
        push_partial(
            &mut partial,
            DocumentPartialCode::NativePartial,
            "some native attributes were not representable",
        );
    }
    map_reasons(&mut partial, reasons);
    let (assets, first_image) = assign_assets(&mut blocks, images);
    DisplayDocument {
        version: DOCUMENT_VERSION,
        blocks,
        assets,
        partial,
        styles_partial,
        first_image,
    }
}

fn push_native_segment(units: &[u16], start: usize, end: usize, blocks: &mut Vec<DocumentBlock>) {
    if end <= start {
        return;
    }
    let segment = String::from_utf16_lossy(&units[start.min(units.len())..end.min(units.len())])
        .replace('\u{fffc}', "");
    if segment.is_empty() {
        return;
    }
    blocks.push(DocumentBlock {
        kind: DocumentBlockKind::Paragraph,
        paragraph: Some(text_paragraph(&segment)),
        table: None,
        image: None,
    });
}

pub fn validate_document(
    document: &DisplayDocument,
) -> Result<(), super::display_document::DocumentError> {
    document.validate()
}

pub fn document_asset_bytes(
    document: &DisplayDocument,
    images: &[ImagePlacement],
) -> Result<Vec<u8>, super::display_document::DocumentError> {
    use super::display_document::DocumentError;
    document.validate()?;
    let total = document
        .assets
        .iter()
        .try_fold(0usize, |total, asset| {
            total.checked_add(usize::try_from(asset.length).ok()?)
        })
        .ok_or(DocumentError::Invalid)?;
    let mut payload = Vec::with_capacity(total);
    for asset in &document.assets {
        let png = images
            .iter()
            .map(|placement| placement.png.as_slice())
            .find(|png| !png.is_empty() && asset_digest(png) == asset.sha256)
            .ok_or(DocumentError::Invalid)?;
        if png.len() as u64 != asset.length || payload.len() as u64 != asset.offset {
            return Err(DocumentError::Invalid);
        }
        payload.extend_from_slice(png);
    }
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_links_do_not_erase_empty_credentials_before_validation() {
        for href in [
            "https://@example.com/",
            "https://:@example.com/",
            "//:@example.com/",
        ] {
            let html = format!(
                "<base href=\"https://example.com/\"><p><a href=\"{href}\">readable</a></p>"
            );
            let document = document_from_html(html.as_bytes(), &[], &[]).unwrap();
            let runs = &document.blocks[0].paragraph.as_ref().unwrap().runs;
            assert_eq!(runs[0].text, "readable");
            assert!(runs[0].link.is_none(), "{href}");
        }
    }
    use crate::composite::content_analysis::ImageCoordinate;
    use crate::protocol::content_analysis::AttachmentState;

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

    fn placement(ordinal: u16, availability: ImageAvailability, png: Vec<u8>) -> ImagePlacement {
        ImagePlacement {
            ordinal,
            representation: 0,
            item: 0,
            coordinate: ImageCoordinate::HtmlByteRange {
                offset: 0,
                length: 1,
            },
            resource_id: Some(u16::from(availability == ImageAvailability::Available)),
            availability,
            png,
        }
    }

    fn runs(block: &DocumentBlock) -> &[DocumentRun] {
        block.paragraph.as_ref().unwrap().runs.as_slice()
    }

    fn paragraph_at(document: &DisplayDocument, index: usize) -> &DocumentParagraph {
        document.blocks[index].paragraph.as_ref().unwrap()
    }

    #[test]
    fn html_headings_map_to_three_levels_without_leaking() {
        let html = b"<h1>one</h1><h2>two</h2><h3>three</h3><h4>four</h4><h5>five</h5><h6>six</h6><p>plain</p>";
        let document = document_from_html(html, &[], &[]).unwrap();
        document.validate().unwrap();
        let levels: Vec<Option<u8>> = document
            .blocks
            .iter()
            .map(|block| block.paragraph.as_ref().unwrap().heading)
            .collect();
        assert_eq!(
            levels,
            vec![Some(1), Some(2), Some(3), Some(3), Some(3), Some(3), None]
        );
        assert_eq!(runs(&document.blocks[0])[0].text, "one");
        assert_eq!(runs(&document.blocks[6])[0].text, "plain");
    }

    #[test]
    fn html_blockquote_depth_is_scoped_to_its_subtree() {
        let html = b"<blockquote><p>outer</p><blockquote><p>inner</p></blockquote></blockquote><p>after</p>";
        let document = document_from_html(html, &[], &[]).unwrap();
        document.validate().unwrap();
        assert_eq!(document.blocks.len(), 3);
        assert_eq!(paragraph_at(&document, 0).quote_depth, 1);
        assert_eq!(runs(&document.blocks[0])[0].text, "outer");
        assert_eq!(paragraph_at(&document, 1).quote_depth, 2);
        assert_eq!(runs(&document.blocks[1])[0].text, "inner");
        assert_eq!(paragraph_at(&document, 2).quote_depth, 0);
        assert_eq!(runs(&document.blocks[2])[0].text, "after");
    }

    #[test]
    fn html_pre_preserves_whitespace_and_reports_a_code_block() {
        let html = b"<pre>  a\n\tb   c\n</pre><p>after</p>";
        let document = document_from_html(html, &[], &[]).unwrap();
        document.validate().unwrap();
        assert!(paragraph_at(&document, 0).code_block);
        assert_eq!(runs(&document.blocks[0])[0].text, "  a\n\tb   c\n");
        assert!(runs(&document.blocks[0])[0].style.monospace);
        assert!(!paragraph_at(&document, 1).code_block);
        assert_eq!(runs(&document.blocks[1])[0].text, "after");
    }

    #[test]
    fn html_pre_keeps_breaks_and_entities_verbatim() {
        let html = b"<pre>a<br>&#9;b &amp; c</pre>";
        let document = document_from_html(html, &[], &[]).unwrap();
        document.validate().unwrap();
        assert_eq!(runs(&document.blocks[0])[0].text, "a\n\tb & c");
    }

    #[test]
    fn html_inline_code_marks_monospace_and_font_family_normalizes() {
        let html = b"<p>a <code>c</code> <kbd>k</kbd> <samp>s</samp> <span style='font-family:Menlo, monospace'>m</span> <span style='font-family:Arial'>n</span></p>";
        let document = document_from_html(html, &[], &[]).unwrap();
        document.validate().unwrap();
        let runs = runs(&document.blocks[0]);
        let find = |text: &str| runs.iter().find(|run| run.text.trim() == text).unwrap();
        assert!(find("c").style.monospace);
        assert!(find("k").style.monospace);
        assert!(find("s").style.monospace);
        assert!(find("m").style.monospace);
        assert!(!find("n").style.monospace);
        assert!(document.styles_partial);
    }

    #[test]
    fn pending_runs_bound_repeated_link_bytes_before_collection() {
        let link = format!(
            "https://example.com/{}",
            "a".repeat(DOCUMENT_LINK_LIMIT - "https://example.com/".len())
        );
        let mut pending = PendingRuns::new();
        for _ in 0..600 {
            pending.apply(DocumentRunStyle::default(), Some(link.clone()), false);
            pending.push_char('x');
            pending.apply(DocumentRunStyle::default(), None, false);
        }
        let runs = pending.into_runs();
        let bytes: usize = runs
            .iter()
            .map(|run| run.text.len() + run.link.as_ref().map_or(0, String::len))
            .sum();
        assert!(bytes <= DOCUMENT_TEXT_LIMIT, "retained {bytes} bytes");
    }

    #[test]
    fn pending_runs_bound_style_changes_before_collection() {
        let mut pending = PendingRuns::new();
        for index in 0..DOCUMENT_RUN_LIMIT + 2 {
            pending.apply(
                DocumentRunStyle {
                    bold: index % 2 == 0,
                    ..DocumentRunStyle::default()
                },
                None,
                false,
            );
            pending.push_char('x');
        }
        assert!(pending.into_runs().len() <= DOCUMENT_RUN_LIMIT);
    }

    #[test]
    fn html_mark_and_background_color_set_highlight() {
        let html =
            b"<p><mark>yellow</mark> <span style='background-color:#102030'>solid</span></p>";
        let document = document_from_html(html, &[], &[]).unwrap();
        document.validate().unwrap();
        let runs = runs(&document.blocks[0]);
        let find = |text: &str| runs.iter().find(|run| run.text.trim() == text).unwrap();
        assert_eq!(
            find("yellow").style.highlight,
            Some(DocumentColor {
                red: 255,
                green: 255,
                blue: 0,
            })
        );
        assert_eq!(
            find("solid").style.highlight,
            Some(DocumentColor {
                red: 0x10,
                green: 0x20,
                blue: 0x30,
            })
        );
    }

    #[test]
    fn html_nested_lists_restore_parent_kind_and_depth() {
        let html =
            b"<ul><li>top<ul><li>inner<ol><li>deep</li></ol></li></ul></li><li>second</li></ul>";
        let document = document_from_html(html, &[], &[]).unwrap();
        document.validate().unwrap();
        let items: Vec<(Option<DocumentListKind>, u8, &str)> = document
            .blocks
            .iter()
            .filter_map(|block| block.paragraph.as_ref())
            .map(|paragraph| {
                (
                    paragraph.list,
                    paragraph.list_depth,
                    paragraph.runs[0].text.as_str(),
                )
            })
            .collect();
        assert_eq!(
            items,
            vec![
                (Some(DocumentListKind::Bullet), 0, "top"),
                (Some(DocumentListKind::Bullet), 1, "inner"),
                (Some(DocumentListKind::Ordered), 2, "deep"),
                (Some(DocumentListKind::Bullet), 0, "second"),
            ]
        );
    }

    #[test]
    fn common_inline_css_retains_supported_styles_next_to_ignored_declarations() {
        let html = b"<p style='font-size:18px;color:rgb(255, 0, 0);font-weight:700'>outer <span style='font-style:italic;text-decoration:underline line-through'>inner</span> tail</p>";
        let document = document_from_html(html, &[], &[]).unwrap();
        document.validate().unwrap();
        let paragraph = runs(&document.blocks[0]);
        assert!(paragraph[0].style.bold);
        assert_eq!(
            paragraph[0].style.color,
            Some(DocumentColor {
                red: 255,
                green: 0,
                blue: 0
            })
        );
        let inner = paragraph.iter().find(|run| run.text == "inner").unwrap();
        assert!(
            inner.style.bold
                && inner.style.italic
                && inner.style.underline
                && inner.style.strikethrough
        );
        assert!(!paragraph.last().unwrap().style.italic);
    }

    #[test]
    fn html_document_keeps_positional_styles_and_table_cells() {
        let html = b"<p>plain <b>bold</b> and <i>italic</i></p><table><tr><th>head</th><td>cell</td></tr></table>";
        let document = document_from_html(html, &[], &[]).unwrap();
        document.validate().unwrap();
        assert_eq!(document.blocks.len(), 2);
        let paragraph = runs(&document.blocks[0]);
        assert_eq!(paragraph.len(), 4);
        assert_eq!(paragraph[0].text.trim(), "plain");
        assert!(!paragraph[0].style.bold);
        assert_eq!(paragraph[1].text, "bold");
        assert!(paragraph[1].style.bold);
        assert!(paragraph[2].text.contains("and"));
        assert!(!paragraph[2].style.italic);
        assert_eq!(paragraph[3].text, "italic");
        assert!(paragraph[3].style.italic);
        let table = document.blocks[1].table.as_ref().unwrap();
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.rows[0].cells.len(), 2);
        assert!(table.rows[0].cells[0].header);
        assert_eq!(table.rows[0].cells[0].runs[0].text, "head");
        assert_eq!(table.rows[0].cells[1].runs[0].text, "cell");
    }

    #[test]
    fn html_document_never_applies_a_global_style_to_the_whole_body() {
        let html = b"<p>plain <b>bold</b> tail</p>";
        let document = document_from_html(html, &[], &[]).unwrap();
        let paragraph = runs(&document.blocks[0]);
        assert!(paragraph.len() >= 3);
        assert!(!paragraph[0].style.bold);
        assert!(paragraph[1].style.bold);
        assert!(!paragraph[2].style.bold);
    }

    #[test]
    fn html_document_keeps_a_missing_first_image_as_a_placeholder() {
        let images = vec![
            placement(0, ImageAvailability::MissingResource, vec![]),
            placement(1, ImageAvailability::Available, png_1x1()),
        ];
        let html = b"<p>a</p><img src=\"tailsync-resource:9\"><img src=\"tailsync-resource:1\">";
        let document = document_from_html(html, &images, &[]).unwrap();
        document.validate().unwrap();
        assert_eq!(document.blocks.len(), 3);
        let first = document.blocks[1].image.as_ref().unwrap();
        assert_eq!(
            first.availability,
            DocumentImageAvailability::MissingResource
        );
        assert_eq!(first.asset, None);
        assert_eq!(document.first_image, None);
        let second = document.blocks[2].image.as_ref().unwrap();
        assert_eq!(second.asset, Some(0));
    }

    #[test]
    fn html_document_drops_active_markup_and_reports_it() {
        let html = b"<script>alert(1)</script><p>safe</p>";
        let document = document_from_html(html, &[], &[]).unwrap();
        assert_eq!(runs(&document.blocks[0])[0].text, "safe");
        assert!(document
            .partial
            .iter()
            .any(|reason| reason.code == DocumentPartialCode::UnsupportedMarkup));
        assert!(document.styles_partial);
    }

    #[test]
    fn decorative_styles_do_not_claim_body_or_structure_loss() {
        let html = b"<p style='font-size:18px;line-height:1.5;color:#123456;font-weight:bold'>saved</p><font color='gradient(red,blue)'>body</font>";
        let document = document_from_html(html, &[], &[]).unwrap();
        assert!(document.styles_partial);
        assert!(document.partial.is_empty());
        assert_eq!(runs(&document.blocks[0])[0].text, "saved");
        assert!(runs(&document.blocks[0])[0].style.bold);
        assert_eq!(
            runs(&document.blocks[0])[0].style.color,
            Some(DocumentColor {
                red: 0x12,
                green: 0x34,
                blue: 0x56,
            })
        );
        assert_eq!(runs(&document.blocks[1])[0].text, "body");
    }

    #[test]
    fn document_reasons_preserve_distinct_details_for_the_same_code() {
        let reasons =
            ["first failure", "second failure", "first failure"].map(|detail| PartialReason {
                code: "malformed_html",
                detail: detail.to_string(),
            });
        let document = document_from_text_with_reasons(&TextFacts::default(), &reasons);
        assert_eq!(document.partial.len(), 2);
        assert_eq!(document.partial[0].detail, "first failure");
        assert_eq!(document.partial[1].detail, "second failure");
    }

    #[test]
    fn html_document_deduplicates_identical_assets() {
        let png = png_1x1();
        let images = vec![
            placement(0, ImageAvailability::Available, png.clone()),
            placement(1, ImageAvailability::Available, png),
        ];
        let html = b"<img src=\"a\"><img src=\"b\">";
        let document = document_from_html(html, &images, &[]).unwrap();
        document.validate().unwrap();
        assert_eq!(document.assets.len(), 1);
        assert_eq!(document.blocks[0].image.as_ref().unwrap().asset, Some(0));
        assert_eq!(document.blocks[1].image.as_ref().unwrap().asset, Some(0));
    }

    #[test]
    fn text_document_is_a_single_plain_paragraph() {
        let document = document_from_text(&TextFacts {
            text: "hello world".into(),
            truncated: false,
        });
        document.validate().unwrap();
        assert_eq!(document.blocks.len(), 1);
        assert_eq!(runs(&document.blocks[0])[0].text, "hello world");
        assert!(!document.styles_partial);
    }

    #[test]
    fn native_document_marks_styles_partial_and_keeps_attachment_position() {
        let facts = NativeAnalysisFacts {
            kind: crate::protocol::content_analysis::SavedAnalysisKind::Rtfd,
            item: 0,
            source_format: "com.apple.flat-rtfd".into(),
            converter: "tailsync.rtfd-to-facts.v1".into(),
            body: "AB\u{fffc}C".into(),
            truncated: false,
            partial: false,
            style_bits: crate::protocol::content_analysis::STYLE_BOLD,
            table_rows: 1,
            table_cells: 2,
            attachments: vec![crate::composite::content_analysis::NativeAttachment {
                utf16_position: 2,
                state: AttachmentState::Available,
                png: png_1x1(),
            }],
        };
        let images = vec![placement(0, ImageAvailability::Available, png_1x1())];
        let document = document_from_native(&facts, &images, &[]);
        document.validate().unwrap();
        assert_eq!(document.blocks.len(), 3);
        assert_eq!(runs(&document.blocks[0])[0].text, "AB");
        assert_eq!(document.blocks[1].image.as_ref().unwrap().asset, Some(0));
        assert_eq!(runs(&document.blocks[2])[0].text, "C");
        assert!(document.styles_partial);
        assert!(document
            .partial
            .iter()
            .any(|reason| reason.code == DocumentPartialCode::NativeStylesUnavailable));
        assert!(document
            .partial
            .iter()
            .any(|reason| reason.code == DocumentPartialCode::NativeTableStructureUnavailable));
        assert!(!runs(&document.blocks[0])[0].style.bold);
    }

    #[test]
    fn document_asset_bytes_rebuilds_the_declared_payload() {
        let png = png_1x1();
        let images = vec![placement(0, ImageAvailability::Available, png.clone())];
        let html = b"<p>x</p><img src=\"a\">";
        let document = document_from_html(html, &images, &[]).unwrap();
        let payload = document_asset_bytes(&document, &images).unwrap();
        assert_eq!(payload, png);
        assert_eq!(document.assets[0].length as usize, png.len());
        assert_eq!(document.assets[0].offset, 0);
    }

    #[test]
    fn asset_digest_is_sha256_hex() {
        assert_eq!(
            asset_digest(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn html_anchors_attach_only_safe_absolute_links() {
        let html = b"<p><a href='https://example.com/a?x=1&amp;y=2'>safe</a> <a href='javascript:alert(1)'>bad</a> <a href='https://user:pass@example.com/'>cred</a> <a href='relative'>rel</a> <a href='https://example.com/a\\b'>slash</a></p>";
        let document = document_from_html(html, &[], &[]).unwrap();
        document.validate().unwrap();
        let runs = runs(&document.blocks[0]);
        assert_eq!(
            runs.iter()
                .map(|run| (run.text.as_str(), run.link.as_deref()))
                .collect::<Vec<_>>(),
            vec![
                ("safe", Some("https://example.com/a?x=1&y=2")),
                (" bad cred rel slash", None),
            ]
        );
        assert!(document.styles_partial);
    }

    #[test]
    fn html_anchor_duplicate_href_is_rejected() {
        let html = b"<p><a href='https://example.com/a' href='https://example.com/b'>dup</a></p>";
        let document = document_from_html(html, &[], &[]).unwrap();
        document.validate().unwrap();
        assert_eq!(runs(&document.blocks[0])[0].link, None);
        assert!(document.styles_partial);
    }

    #[test]
    fn html_rejected_nested_anchor_does_not_borrow_its_parents_target() {
        let html = b"<p><a href='https://example.com/'>outer<a href='javascript:alert(1)'>inner</a>outer again</a></p>";
        let document = document_from_html(html, &[], &[]).unwrap();
        document.validate().unwrap();
        assert_eq!(
            runs(&document.blocks[0])
                .iter()
                .map(|run| (run.text.as_str(), run.link.as_deref()))
                .collect::<Vec<_>>(),
            vec![
                ("outer", Some("https://example.com/")),
                ("inner", None),
                ("outer again", Some("https://example.com/")),
            ]
        );
    }

    #[test]
    fn html_relative_links_require_a_safe_base_before_normalization() {
        for base in [
            "https://@example.com/",
            "https:example.com",
            "https://example.com/a\\b",
        ] {
            let html = format!("<base href='{base}'><p><a href='relative'>text</a></p>");
            let document = document_from_html(html.as_bytes(), &[], &[]).unwrap();
            document.validate().unwrap();
            assert_eq!(runs(&document.blocks[0])[0].link, None, "{base}");
        }
    }

    #[test]
    fn html_table_cell_style_is_applied_to_its_first_text() {
        let html = b"<table><tr><td style='font-weight:bold;color:#ff0000;background-color:#ffffff'>first</td><td><code>second</code></td></tr></table>";
        let document = document_from_html(html, &[], &[]).unwrap();
        document.validate().unwrap();
        let cells = &document.blocks[0].table.as_ref().unwrap().rows[0].cells;
        let first = &cells[0].runs[0];
        assert_eq!(first.text, "first");
        assert!(first.style.bold);
        assert_eq!(
            first.style.color,
            Some(DocumentColor {
                red: 255,
                green: 0,
                blue: 0
            })
        );
        assert_eq!(
            first.style.highlight,
            Some(DocumentColor {
                red: 255,
                green: 255,
                blue: 255
            })
        );
        assert!(cells[1].runs[0].style.monospace);
        assert!(!cells[1].runs[0].style.bold);
    }

    #[test]
    fn html_relative_anchor_uses_only_a_unique_verified_base() {
        let with_base = b"<base href='https://example.com/posts/'><p><a href='../other?x=1&amp;y=2'>saved</a></p>";
        let document = document_from_html(with_base, &[], &[]).unwrap();
        document.validate().unwrap();
        assert_eq!(
            runs(&document.blocks[0])[0].link.as_deref(),
            Some("https://example.com/other?x=1&y=2")
        );

        for ambiguous in [
            &b"<base href='https://a.example/'><base href='https://b.example/'><p><a href='x'>saved</a></p>"[..],
            &b"<base href='javascript:active()'><p><a href='x'>saved</a></p>"[..],
            &b"<p><a href='x'>saved</a></p>"[..],
        ] {
            let document = document_from_html(ambiguous, &[], &[]).unwrap();
            document.validate().unwrap();
            assert_eq!(runs(&document.blocks[0])[0].link, None);
        }
    }

    #[test]
    fn html_long_and_repeated_links_stay_within_the_budget() {
        let too_long = format!("https://example.com/{}", "a".repeat(DOCUMENT_LINK_LIMIT));
        let html = format!("<p><a href='{too_long}'>x</a></p>");
        let document = document_from_html(html.as_bytes(), &[], &[]).unwrap();
        document.validate().unwrap();
        assert_eq!(runs(&document.blocks[0])[0].link, None);
        assert!(document.styles_partial);

        let link = format!(
            "https://example.com/{}",
            "a".repeat(DOCUMENT_LINK_LIMIT - "https://example.com/".len())
        );
        assert_eq!(link.len(), DOCUMENT_LINK_LIMIT);
        let mut html = String::from("<p>");
        for _ in 0..600 {
            html.push_str(&format!("<a href='{link}'>x</a>"));
        }
        html.push_str("</p>");
        let document = document_from_html(html.as_bytes(), &[], &[]).unwrap();
        document.validate().unwrap();
        let total: usize = document
            .blocks
            .iter()
            .filter_map(|block| block.paragraph.as_ref())
            .flat_map(|paragraph| paragraph.runs.iter())
            .filter_map(|run| run.link.as_deref())
            .map(str::len)
            .sum();
        assert!(total <= DOCUMENT_TEXT_LIMIT);
        assert!(total > 0);
    }

    #[test]
    fn html_mismatched_closing_tags_keep_every_text_run() {
        let html = b"<blockquote><p>quoted</p></div><p>plain</p>";
        let document = document_from_html(html, &[], &[]).unwrap();
        document.validate().unwrap();
        let texts: Vec<&str> = document
            .blocks
            .iter()
            .filter_map(|block| block.paragraph.as_ref())
            .map(|paragraph| paragraph.runs[0].text.as_str())
            .collect();
        assert_eq!(texts, vec!["quoted", "plain"]);
        assert_eq!(paragraph_at(&document, 0).quote_depth, 1);
    }

    #[test]
    fn documents_built_without_html_semantics_keep_defaults() {
        let document = document_from_text(&TextFacts {
            text: "hello".into(),
            truncated: false,
        });
        let paragraph = paragraph_at(&document, 0);
        assert_eq!(paragraph.heading, None);
        assert_eq!(paragraph.quote_depth, 0);
        assert!(!paragraph.code_block);
        assert_eq!(paragraph.list_depth, 0);
        assert_eq!(paragraph.runs[0].link, None);
        assert!(!paragraph.runs[0].style.monospace);
        assert_eq!(paragraph.runs[0].style.highlight, None);
    }
}
