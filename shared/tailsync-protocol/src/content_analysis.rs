use crate::ProtocolError;

pub const SAVED_ANALYSIS_VERSION: u8 = 1;
pub const SAVED_BODY_LIMIT: usize = 1024 * 1024;
pub const SAVED_ATTACHMENT_LIMIT: usize = 64;
pub const SAVED_ATTACHMENT_BYTES_LIMIT: usize = 4 * 1024 * 1024;
pub const SAVED_FRAME_LIMIT: usize = 16 * 1024 * 1024;
pub const RTF_GROUP_DEPTH_LIMIT: usize = 64;
pub const RTF_RUN_LIMIT: usize = 8192;
pub const SAVED_TABLE_CELL_LIMIT: usize = 2048;

const HEADER: usize = 22;

pub const STYLE_BOLD: u8 = 1 << 0;
pub const STYLE_ITALIC: u8 = 1 << 1;
pub const STYLE_UNDERLINE: u8 = 1 << 2;
pub const STYLE_STRIKETHROUGH: u8 = 1 << 3;
pub const STYLE_SUBSCRIPT: u8 = 1 << 4;
pub const STYLE_SUPERSCRIPT: u8 = 1 << 5;
pub const STYLE_UNSUPPORTED: u8 = 1 << 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SavedAnalysisKind {
    Rtf = 1,
    Rtfd = 2,
}

impl SavedAnalysisKind {
    pub fn from_byte(value: u8) -> Result<Self, ProtocolError> {
        match value {
            1 => Ok(Self::Rtf),
            2 => Ok(Self::Rtfd),
            _ => Err(ProtocolError::InvalidNativeSnapshot),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AttachmentState {
    Available = 0,
    Missing = 1,
    Unsupported = 2,
    PixelBudget = 3,
}

impl AttachmentState {
    pub fn from_byte(value: u8) -> Result<Self, ProtocolError> {
        match value {
            0 => Ok(Self::Available),
            1 => Ok(Self::Missing),
            2 => Ok(Self::Unsupported),
            3 => Ok(Self::PixelBudget),
            _ => Err(ProtocolError::InvalidNativeSnapshot),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedAttachment {
    pub utf16_position: u32,
    pub state: AttachmentState,
    pub png: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedTextAnalysis {
    pub kind: SavedAnalysisKind,
    pub source_format: String,
    pub converter: String,
    pub body: String,
    pub truncated: bool,
    pub partial: bool,
    pub style_bits: u8,
    pub table_rows: u16,
    pub table_cells: u16,
    pub attachments: Vec<SavedAttachment>,
}

fn invalid() -> ProtocolError {
    ProtocolError::InvalidNativeSnapshot
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<usize, ProtocolError> {
    Ok(u16::from_be_bytes(
        bytes
            .get(offset..offset + 2)
            .ok_or_else(invalid)?
            .try_into()
            .map_err(|_| invalid())?,
    ) as usize)
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<usize, ProtocolError> {
    Ok(u32::from_be_bytes(
        bytes
            .get(offset..offset + 4)
            .ok_or_else(invalid)?
            .try_into()
            .map_err(|_| invalid())?,
    ) as usize)
}

impl SavedTextAnalysis {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.body.len() > SAVED_BODY_LIMIT
            || self.attachments.len() > SAVED_ATTACHMENT_LIMIT
            || self.source_format.len() > 256
            || self.converter.len() > 128
            || self.source_format.contains('\0')
            || self.converter.contains('\0')
            || self.style_bits & !0b0111_1111 != 0
            || self.table_cells as usize > SAVED_TABLE_CELL_LIMIT
            || self.table_rows as usize > SAVED_TABLE_CELL_LIMIT
        {
            return Err(invalid());
        }
        let units: Vec<u16> = self.body.encode_utf16().collect();
        let mut size = HEADER + self.source_format.len() + self.converter.len() + self.body.len();
        let mut previous = None;
        for attachment in &self.attachments {
            if units.get(attachment.utf16_position as usize) != Some(&0xfffc)
                || previous.is_some_and(|position| position >= attachment.utf16_position)
                || attachment.png.len() > SAVED_ATTACHMENT_BYTES_LIMIT
                || (attachment.state == AttachmentState::Available) == attachment.png.is_empty()
            {
                return Err(invalid());
            }
            previous = Some(attachment.utf16_position);
            size = size
                .checked_add(10 + attachment.png.len())
                .ok_or_else(invalid)?;
            if size > SAVED_FRAME_LIMIT {
                return Err(invalid());
            }
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>, ProtocolError> {
        self.validate()?;
        let mut output = Vec::with_capacity(self.body.len() + 64);
        output.extend_from_slice(b"RTA1");
        output.push(SAVED_ANALYSIS_VERSION);
        output.push(self.kind as u8);
        output.push((u8::from(self.partial) << 1) | u8::from(self.truncated));
        output.push(self.style_bits);
        output.extend_from_slice(&(self.source_format.len() as u16).to_be_bytes());
        output.extend_from_slice(&(self.converter.len() as u16).to_be_bytes());
        output.extend_from_slice(&(self.body.len() as u32).to_be_bytes());
        output.extend_from_slice(&(self.attachments.len() as u16).to_be_bytes());
        output.extend_from_slice(&self.table_rows.to_be_bytes());
        output.extend_from_slice(&self.table_cells.to_be_bytes());
        output.extend_from_slice(self.source_format.as_bytes());
        output.extend_from_slice(self.converter.as_bytes());
        output.extend_from_slice(self.body.as_bytes());
        for attachment in &self.attachments {
            if attachment.png.len() > SAVED_ATTACHMENT_BYTES_LIMIT
                || (attachment.state == AttachmentState::Available) == attachment.png.is_empty()
            {
                return Err(invalid());
            }
            output.extend_from_slice(&attachment.utf16_position.to_be_bytes());
            output.push(attachment.state as u8);
            output.push(0);
            output.extend_from_slice(&(attachment.png.len() as u32).to_be_bytes());
            output.extend_from_slice(&attachment.png);
        }
        if output.len() > SAVED_FRAME_LIMIT {
            return Err(ProtocolError::AdmissionRejected(
                "saved analysis frame exceeds limit".into(),
            ));
        }
        Ok(output)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        if bytes.len() < HEADER || bytes.len() > SAVED_FRAME_LIMIT || &bytes[..5] != b"RTA1\x01" {
            return Err(invalid());
        }
        let kind = SavedAnalysisKind::from_byte(bytes[5])?;
        let flags = bytes[6];
        if flags & !0b11 != 0 || bytes[7] & !0b0111_1111 != 0 {
            return Err(invalid());
        }
        let source_len = read_u16(bytes, 8)?;
        let converter_len = read_u16(bytes, 10)?;
        let body_len = read_u32(bytes, 12)?;
        let attachment_count = read_u16(bytes, 16)?;
        let table_rows = read_u16(bytes, 18)?;
        let table_cells = read_u16(bytes, 20)?;
        if source_len > 256
            || converter_len > 128
            || body_len > SAVED_BODY_LIMIT
            || attachment_count > SAVED_ATTACHMENT_LIMIT
            || table_cells > SAVED_TABLE_CELL_LIMIT
        {
            return Err(invalid());
        }
        let mut offset = HEADER;
        let source_format =
            std::str::from_utf8(bytes.get(offset..offset + source_len).ok_or_else(invalid)?)
                .map_err(|_| invalid())?
                .to_string();
        offset += source_len;
        let converter = std::str::from_utf8(
            bytes
                .get(offset..offset + converter_len)
                .ok_or_else(invalid)?,
        )
        .map_err(|_| invalid())?
        .to_string();
        offset += converter_len;
        let body = std::str::from_utf8(bytes.get(offset..offset + body_len).ok_or_else(invalid)?)
            .map_err(|_| invalid())?
            .to_string();
        offset += body_len;
        let mut attachments = Vec::with_capacity(attachment_count);
        for _ in 0..attachment_count {
            let position = read_u32(bytes, offset)?;
            let state = AttachmentState::from_byte(*bytes.get(offset + 4).ok_or_else(invalid)?)?;
            if bytes.get(offset + 5) != Some(&0) {
                return Err(invalid());
            }
            let png_len = read_u32(bytes, offset + 6)?;
            if png_len > SAVED_ATTACHMENT_BYTES_LIMIT {
                return Err(invalid());
            }
            let start = offset + 10;
            let png = bytes.get(start..start + png_len).ok_or_else(invalid)?;
            if (state == AttachmentState::Available) == png.is_empty() {
                return Err(invalid());
            }
            attachments.push(SavedAttachment {
                utf16_position: position as u32,
                state,
                png: png.to_vec(),
            });
            offset = start + png_len;
        }
        if offset != bytes.len() {
            return Err(invalid());
        }
        let analysis = Self {
            kind,
            source_format,
            converter,
            body,
            truncated: flags & 0b01 != 0,
            partial: flags & 0b10 != 0,
            style_bits: bytes[7],
            table_rows: table_rows as u16,
            table_cells: table_cells as u16,
            attachments,
        };
        analysis.validate()?;
        Ok(analysis)
    }
}

pub fn scan_rtf(bytes: &[u8]) -> SavedTextAnalysis {
    scan_rtf_internal(bytes, false).0
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RtfStyleSpan {
    pub start: usize,
    pub end: usize,
    pub style_bits: u8,
}

pub fn scan_rtf_with_styles(bytes: &[u8]) -> (SavedTextAnalysis, Option<Vec<RtfStyleSpan>>) {
    scan_rtf_internal(bytes, true)
}

fn scan_rtf_internal(
    bytes: &[u8],
    track_styles: bool,
) -> (SavedTextAnalysis, Option<Vec<RtfStyleSpan>>) {
    let mut scanner = RtfScanner {
        unicode_fallback_count: 1,
        track_styles,
        ..RtfScanner::default()
    };
    if bytes.len() > crate::composite::Mime::Rtf.limit()
        || !bytes.starts_with(b"{\\rtf1")
        || !bytes
            .get(6)
            .is_some_and(|byte| byte.is_ascii_whitespace() || matches!(byte, b'\\' | b'}' | b'{'))
    {
        scanner.partial = true;
        return (scanner.finish(), None);
    }
    scanner.run(bytes);
    let spans = std::mem::take(&mut scanner.style_spans);
    let styles_overflow = scanner.styles_overflow || scanner.style_scope_mismatch;
    let analysis = scanner.finish();
    let proven = track_styles && !styles_overflow && !analysis.partial && !analysis.truncated;
    (analysis, proven.then_some(spans))
}

#[derive(Default)]
struct RtfScanner {
    track_styles: bool,
    current_style: u8,
    style_stack: Vec<u8>,
    style_spans: Vec<RtfStyleSpan>,
    styles_overflow: bool,
    style_scope_mismatch: bool,
    pending_surrogate_style: u8,
    body: String,
    utf16: u32,
    attachments: Vec<SavedAttachment>,
    depth: usize,
    runs: usize,
    partial: bool,
    truncated: bool,
    skip: Vec<bool>,
    pending_unicode_skip: u8,
    unicode_fallback_count: u8,
    unicode_fallback_stack: Vec<u8>,
    pending_high_surrogate: Option<u16>,
    picture: Option<PictureGroup>,
    picture_depth: usize,
    attachment_bytes: usize,
    malformed: bool,
    root_seen: bool,
    unsupported_codepage: bool,
    style_bits: u8,
    table_rows: u16,
    table_cells: u16,
    table_cell_limit_hit: bool,
}

#[derive(Default)]
struct PictureGroup {
    hex: String,
    blip: Blip,
}

#[derive(Default, PartialEq, Eq)]
enum Blip {
    #[default]
    Unknown,
    Png,
    Other,
}

impl RtfScanner {
    fn skipping(&self) -> bool {
        self.skip.iter().any(|flag| *flag)
    }

    fn push(&mut self, ch: char) {
        if self.pending_high_surrogate.take().is_some() {
            self.partial = true;
        }
        if self.body.len().saturating_add(ch.len_utf8()) > SAVED_BODY_LIMIT {
            self.truncated = true;
            return;
        }
        let start = self.body.len();
        self.body.push(ch);
        if self.track_styles && !self.styles_overflow {
            let end = self.body.len();
            if let Some(previous) = self
                .style_spans
                .last_mut()
                .filter(|span| span.end == start && span.style_bits == self.current_style)
            {
                previous.end = end;
            } else if self.style_spans.len() < RTF_RUN_LIMIT {
                self.style_spans.push(RtfStyleSpan {
                    start,
                    end,
                    style_bits: self.current_style,
                });
            } else {
                self.styles_overflow = true;
                self.style_spans.clear();
            }
        }
        self.utf16 = self.utf16.saturating_add(ch.len_utf16() as u32);
    }

    fn run(&mut self, bytes: &[u8]) {
        let mut index = 0usize;
        while index < bytes.len() {
            let byte = bytes[index];
            if self.depth == 0 && byte != b'{' && !byte.is_ascii_whitespace() {
                self.malformed = true;
                return;
            }
            if self.pending_unicode_skip > 0 && !matches!(byte, b'\\' | b'{' | b'}') {
                self.pending_unicode_skip -= 1;
                index += 1;
                continue;
            }
            match byte {
                b'{' => {
                    if self.depth == 0 && self.root_seen {
                        self.malformed = true;
                        return;
                    }
                    self.root_seen = true;
                    if self.track_styles {
                        self.style_stack.push(self.current_style);
                    }
                    self.unicode_fallback_stack
                        .push(self.unicode_fallback_count);
                    self.depth += 1;
                    if self.depth > RTF_GROUP_DEPTH_LIMIT {
                        self.partial = true;
                        return;
                    }
                    let destination =
                        bytes.get(index + 1) == Some(&b'\\') && bytes.get(index + 2) == Some(&b'*');
                    self.skip.push(self.skipping() || destination);
                    index += 1;
                }
                b'}' => {
                    if self.depth == 0 {
                        self.malformed = true;
                        return;
                    }
                    self.unicode_fallback_count = self.unicode_fallback_stack.pop().unwrap_or(1);
                    if self.track_styles {
                        self.current_style = self.style_stack.pop().unwrap_or_default();
                    }
                    self.depth = self.depth.saturating_sub(1);
                    let active = self.skip.pop().unwrap_or(false);
                    if !active {
                        if self.depth < self.picture_depth {
                            if let Some(picture) = self.picture.take() {
                                self.record_picture(picture);
                            }
                        }
                    } else if self.picture.is_some() {
                        self.picture = None;
                    }
                    index += 1;
                }
                b'\\' => index = self.control(bytes, index),
                _ => {
                    if !self.skipping() {
                        if let Some(picture) = self.picture.as_mut() {
                            if byte.is_ascii_hexdigit()
                                && picture.hex.len() < SAVED_ATTACHMENT_BYTES_LIMIT * 2
                            {
                                picture.hex.push(byte as char);
                            } else if byte.is_ascii_hexdigit() {
                                self.partial = true;
                            }
                        } else if byte != b'\r' && byte != b'\n' {
                            self.push_ansi(byte);
                        }
                    }
                    index += 1;
                }
            }
        }
    }

    fn control(&mut self, bytes: &[u8], start: usize) -> usize {
        self.runs += 1;
        if self.runs > RTF_RUN_LIMIT {
            self.partial = true;
            return bytes.len();
        }
        let mut index = start + 1;
        let symbol = bytes.get(index).copied();
        if let Some(symbol) = symbol {
            if !symbol.is_ascii_alphabetic() {
                index += 1;
                if self.pending_unicode_skip > 0 && matches!(symbol, b'\\' | b'{' | b'}' | b'\'') {
                    self.pending_unicode_skip -= 1;
                    return if symbol == b'\'' {
                        index.saturating_add(2).min(bytes.len())
                    } else {
                        index
                    };
                }
                if self.skipping() {
                    return index;
                }
                match symbol {
                    b'\\' | b'{' | b'}' => self.push(symbol as char),
                    b'\'' => {
                        let hex = bytes.get(index..index + 2).and_then(|pair| {
                            std::str::from_utf8(pair)
                                .ok()
                                .and_then(|value| u8::from_str_radix(value, 16).ok())
                        });
                        if let Some(value) = hex {
                            self.push_ansi(value);
                            index += 2;
                        } else {
                            self.partial = true;
                        }
                    }
                    b'~' => self.push('\u{00a0}'),
                    b'-' | b'_' | b'*' => {}
                    _ => self.partial = true,
                }
                return index;
            }
        }
        let word_start = index;
        while index < bytes.len() && bytes[index].is_ascii_alphabetic() {
            index += 1;
        }
        let word = std::str::from_utf8(&bytes[word_start..index])
            .unwrap_or("")
            .to_ascii_lowercase();
        let mut parameter: i64 = 0;
        let mut has_parameter = false;
        let sign = if bytes.get(index) == Some(&b'-') {
            index += 1;
            -1
        } else {
            1
        };
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            has_parameter = true;
            parameter = parameter
                .saturating_mul(10)
                .saturating_add((bytes[index] - b'0') as i64);
            index += 1;
        }
        parameter *= sign;
        if bytes.get(index) == Some(&b' ') {
            index += 1;
        }
        if self.skipping() {
            return index;
        }
        if self.picture.is_some() {
            match word.as_str() {
                "pngblip" => {
                    if let Some(picture) = self.picture.as_mut() {
                        picture.blip = Blip::Png;
                    }
                }
                "jpegblip" | "emfblip" | "wmetafile" | "macpict" | "pict" | "pmmetafile" => {
                    if let Some(picture) = self.picture.as_mut() {
                        picture.blip = Blip::Other;
                    }
                }
                _ => {}
            }
            return index;
        }
        let enabled = !has_parameter || parameter != 0;
        match word.as_str() {
            "par" | "line" | "sect" | "page" => self.push('\n'),
            "tab" => self.push('\t'),
            "b" => self.set_style(STYLE_BOLD, enabled),
            "i" => self.set_style(STYLE_ITALIC, enabled),
            "ul" | "ulnone" => {
                self.set_style(STYLE_UNDERLINE, word == "ul");
                if !enabled {
                    self.current_style &= !STYLE_UNDERLINE;
                }
            }
            "strike" => self.set_style(STYLE_STRIKETHROUGH, enabled),
            "sub" => self.set_style(STYLE_SUBSCRIPT, enabled),
            "super" => self.set_style(STYLE_SUPERSCRIPT, enabled),
            "trowd" => self.table_rows = self.table_rows.saturating_add(1),
            "cell" | "nestcell" => self.count_cell(),
            "u" if has_parameter && (-32768..=65535).contains(&parameter) => {
                let unit = parameter as u16;
                if (0xd800..=0xdbff).contains(&unit) {
                    self.pending_surrogate_style = self.current_style;
                    if self.pending_high_surrogate.replace(unit).is_some() {
                        self.partial = true;
                    }
                } else if (0xdc00..=0xdfff).contains(&unit) {
                    if let Some(high) = self.pending_high_surrogate.take() {
                        if self.track_styles && self.pending_surrogate_style != self.current_style {
                            self.style_scope_mismatch = true;
                        }
                        let scalar =
                            0x10000 + (((high as u32) - 0xd800) << 10) + (unit as u32 - 0xdc00);
                        if let Some(character) = char::from_u32(scalar) {
                            self.push(character);
                        }
                    } else {
                        self.partial = true;
                    }
                } else {
                    if self.pending_high_surrogate.take().is_some() {
                        self.partial = true;
                    }
                    if let Some(character) = char::from_u32(unit as u32) {
                        self.push(character);
                    }
                }
                self.pending_unicode_skip = self.unicode_fallback_count;
            }
            "uc" if has_parameter && (0..=10).contains(&parameter) => {
                self.unicode_fallback_count = parameter as u8
            }
            "ansicpg" if has_parameter && parameter != 1252 => {
                self.unsupported_codepage = true;
                self.partial = true;
            }
            "pict" => {
                self.picture = Some(PictureGroup::default());
                self.picture_depth = self.depth;
            }
            "fonttbl" | "colortbl" | "stylesheet" | "info" | "themedata" | "datastore"
            | "latentstyles" | "generator" | "listtable" | "listoverridetable" | "rsidtbl"
            | "filetbl" | "header" | "headerl" | "headerr" | "headerf" | "footer" | "footerl"
            | "footerr" | "footerf" | "footnote" | "object" | "objdata" | "fldinst"
            | "bkmkstart" | "bkmkend" | "nonshppict" | "shppict" => {
                if let Some(flag) = self.skip.last_mut() {
                    *flag = true;
                }
            }
            "bin" if has_parameter => {
                let count = parameter.max(0) as usize;
                self.partial = true;
                index = index.saturating_add(count).min(bytes.len());
            }
            "plain" => self.current_style = 0,
            "nosupersub" => self.current_style &= !(STYLE_SUBSCRIPT | STYLE_SUPERSCRIPT),
            "rtf" | "ansi" | "ansicpg" | "deff" | "f" | "fs" | "cf" | "highlight" | "pard"
            | "ql" | "qr" | "qc" | "qj" | "sa" | "sb" | "li" | "ri" | "fi" | "sl" | "slmult"
            | "intbl" | "cellx" | "row" | "nestrow" | "fldrslt" => {}
            _ => self.partial = true,
        }
        index
    }

    fn set_style(&mut self, bit: u8, enabled: bool) {
        if enabled {
            self.style_bits |= bit;
            if matches!(bit, STYLE_SUBSCRIPT | STYLE_SUPERSCRIPT) {
                self.current_style &= !(STYLE_SUBSCRIPT | STYLE_SUPERSCRIPT);
            }
            self.current_style |= bit;
        } else {
            self.current_style &= !bit;
        }
    }

    fn push_ansi(&mut self, byte: u8) {
        let character = if !(128..160).contains(&byte) {
            if byte >= 128 && self.unsupported_codepage {
                '\u{fffd}'
            } else {
                byte as char
            }
        } else if self.unsupported_codepage {
            '\u{fffd}'
        } else {
            let characters = [
                '€', '\u{fffd}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{fffd}',
                'Ž', '\u{fffd}', '\u{fffd}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›',
                'œ', '\u{fffd}', 'ž', 'Ÿ',
            ];
            characters[(byte - 128) as usize]
        };
        if character == '\u{fffd}' {
            self.partial = true;
        }
        self.push(character);
    }

    fn count_cell(&mut self) {
        if self.table_cells as usize >= SAVED_TABLE_CELL_LIMIT {
            self.table_cell_limit_hit = true;
            self.partial = true;
            return;
        }
        self.table_cells = self.table_cells.saturating_add(1);
    }

    fn record_picture(&mut self, picture: PictureGroup) {
        let position = self.utf16;
        if self.attachments.len() >= SAVED_ATTACHMENT_LIMIT {
            self.partial = true;
            return;
        }
        let (state, png) = if picture.hex.len() / 2
            > SAVED_FRAME_LIMIT - SAVED_BODY_LIMIT - 65536 - self.attachment_bytes
        {
            self.partial = true;
            (AttachmentState::Unsupported, Vec::new())
        } else {
            match picture.blip {
                Blip::Png => match decode_hex(&picture.hex) {
                    Some(png) if !png.is_empty() => (AttachmentState::Available, png),
                    _ => (AttachmentState::Unsupported, Vec::new()),
                },
                Blip::Other => (AttachmentState::Unsupported, Vec::new()),
                Blip::Unknown => (AttachmentState::Missing, Vec::new()),
            }
        };
        self.attachment_bytes += png.len();
        self.attachments.push(SavedAttachment {
            utf16_position: position,
            state,
            png,
        });
        self.push('\u{fffc}');
    }

    fn finish(mut self) -> SavedTextAnalysis {
        if self.depth != 0 || self.pending_high_surrogate.is_some() || self.malformed {
            self.partial = true;
            self.body.clear();
            self.attachments.clear();
        }
        let _ = self.table_cell_limit_hit;
        SavedTextAnalysis {
            kind: SavedAnalysisKind::Rtf,
            source_format: "public.rtf".into(),
            converter: "tailsync.rtf-scan.v1".into(),
            body: self.body,
            truncated: self.truncated,
            partial: self.partial,
            style_bits: self.style_bits,
            table_rows: self.table_rows,
            table_cells: self.table_cells,
            attachments: self.attachments,
        }
    }
}

fn decode_hex(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    let mut index = 0;
    while index < hex.len() {
        bytes.push(u8::from_str_radix(&hex[index..index + 2], 16).ok()?);
        index += 2;
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positional_styles_preserve_group_scope_and_saved_analysis_encoding() {
        let bytes = br"{\rtf1 A{\b B{\i C}D}E\ul F\ul0 G\sub H\super I\nosupersub J\plain K}";
        let (analysis, spans) = scan_rtf_with_styles(bytes);
        assert_eq!(
            analysis.encode().unwrap(),
            scan_rtf(bytes).encode().unwrap()
        );
        let spans = spans.unwrap();
        let parts: Vec<_> = spans
            .iter()
            .map(|span| (&analysis.body[span.start..span.end], span.style_bits))
            .collect();
        assert_eq!(
            parts,
            vec![
                ("A", 0),
                ("B", STYLE_BOLD),
                ("C", STYLE_BOLD | STYLE_ITALIC),
                ("D", STYLE_BOLD),
                ("E", 0),
                ("F", STYLE_UNDERLINE),
                ("G", 0),
                ("H", STYLE_SUBSCRIPT),
                ("I", STYLE_SUPERSCRIPT),
                ("JK", 0),
            ]
        );
    }

    #[test]
    fn positional_styles_cover_unicode_and_refuse_unproven_body() {
        let bytes = br"{\rtf1\uc1 \u20013?{\b \u-10179?\u-8638?}\u25991?}";
        let (analysis, spans) = scan_rtf_with_styles(bytes);
        let spans = spans.unwrap();
        assert_eq!(analysis.body, "中🙂文");
        assert_eq!(&analysis.body[spans[1].start..spans[1].end], "🙂");
        assert_eq!(spans[1].style_bits, STYLE_BOLD);
        for bytes in [
            br"{\rtf1 missing".as_slice(),
            br"{\rtf1\unknown x}",
            br"{\rtf1\ansicpg936 \'d6\'d0}",
        ] {
            assert!(scan_rtf_with_styles(bytes).1.is_none());
        }
    }

    #[test]
    fn positional_styles_do_not_guess_a_style_split_across_surrogate_units() {
        let bytes = br"{\rtf1\uc1 \u-10179?{\b \u-8638?}}";
        let (analysis, spans) = scan_rtf_with_styles(bytes);
        assert_eq!(analysis.body, "🙂");
        assert!(spans.is_none());
        assert_eq!(
            analysis.encode().unwrap(),
            scan_rtf(bytes).encode().unwrap()
        );
    }

    #[test]
    fn malformed_rtf_and_unbalanced_groups_never_claim_a_complete_body() {
        for bytes in [
            b"not RTF".as_slice(),
            b"{\\rtf1 x}}",
            b"{\\rtf1 x} injected",
            b"{\\rtf1 x",
        ] {
            let facts = scan_rtf(bytes);
            assert!(facts.partial);
            assert!(facts.body.is_empty());
        }
    }

    #[test]
    fn utf16_surrogates_and_group_scoped_fallback_preserve_text() {
        assert_eq!(
            scan_rtf(b"{\\rtf1\\ansi\\uc1 \\u-10179?\\u-8638?}").body,
            "🙂"
        );
        assert_eq!(
            scan_rtf(b"{\\rtf1\\uc1 {\\uc0\\u20013}\\u25991?}").body,
            "中文"
        );
        assert_eq!(scan_rtf(b"{\\rtf1\\ansi\\ansicpg1252 \\'80}").body, "€");
        let unsupported = scan_rtf(b"{\\rtf1\\ansi\\ansicpg936 \\'d6\\'d0}");
        assert!(unsupported.partial);
        assert_eq!(unsupported.body, "\u{fffd}\u{fffd}");
    }

    #[test]
    fn attachment_coordinates_must_point_to_ordered_placeholders() {
        let mut facts = scan_rtf(b"{\\rtf1 x}");
        facts.attachments.push(SavedAttachment {
            utf16_position: 999,
            state: AttachmentState::Unsupported,
            png: vec![],
        });
        assert!(facts.validate().is_err());
        let mut facts = sample();
        facts.attachments.push(facts.attachments[0].clone());
        assert!(facts.validate().is_err());
    }

    #[test]
    fn unknown_controls_and_out_of_range_unicode_are_partial() {
        let facts = scan_rtf(b"{\\rtf1\\foobar123 readable}");
        assert!(facts.partial);
        assert_eq!(facts.body, "readable");
        let facts = scan_rtf(b"{\\rtf1\\uc0\\u999999}");
        assert!(facts.partial);
        assert!(facts.body.is_empty());
    }

    fn sample() -> SavedTextAnalysis {
        SavedTextAnalysis {
            kind: SavedAnalysisKind::Rtfd,
            source_format: "com.apple.flat-rtfd".into(),
            converter: "tailsync.rtfd-to-facts.v1".into(),
            body: "RTFD 中文\u{fffc} 🙂".into(),
            truncated: false,
            partial: true,
            style_bits: STYLE_BOLD | STYLE_UNDERLINE,
            table_rows: 2,
            table_cells: 3,
            attachments: vec![SavedAttachment {
                utf16_position: 7,
                state: AttachmentState::Available,
                png: vec![1, 2, 3],
            }],
        }
    }

    #[test]
    fn frame_round_trips_and_rejects_trailing_bytes() {
        let analysis = sample();
        let encoded = analysis.encode().unwrap();
        assert_eq!(SavedTextAnalysis::decode(&encoded).unwrap(), analysis);
        let mut trailing = encoded.clone();
        trailing.push(0);
        assert!(SavedTextAnalysis::decode(&trailing).is_err());
    }

    #[test]
    fn available_attachment_requires_bytes_and_missing_must_not_carry_any() {
        let mut analysis = sample();
        analysis.attachments[0].png = vec![];
        assert!(analysis.encode().is_err());
        analysis.attachments[0].state = AttachmentState::Missing;
        analysis.attachments[0].png = vec![9];
        assert!(analysis.encode().is_err());
    }

    #[test]
    fn unknown_style_bits_and_oversized_tables_are_rejected() {
        let mut analysis = sample();
        analysis.style_bits |= 0b1000_0000;
        assert!(analysis.encode().is_err());
        analysis.style_bits = 0;
        analysis.table_cells = SAVED_TABLE_CELL_LIMIT as u16 + 1;
        assert!(analysis.encode().is_err());
    }

    #[test]
    fn pure_scan_keeps_unicode_and_locates_png_attachment_by_position() {
        let png = [0x89u8, b'P', b'N', b'G', 1, 2, 3, 4];
        let hex: String = png.iter().map(|byte| format!("{byte:02x}")).collect();
        let rtf = format!(
            "{{\\rtf1\\ansi \\u20013?\\u25991? {{\\pict\\pngblip {hex}}} \\u23614?\\u37096?}}"
        );
        let analysis = scan_rtf(rtf.as_bytes());
        assert!(analysis.body.starts_with("中文"), "{}", analysis.body);
        assert!(analysis.body.ends_with("尾部"), "{}", analysis.body);
        assert_eq!(analysis.attachments.len(), 1);
        assert_eq!(analysis.attachments[0].state, AttachmentState::Available);
        assert_eq!(analysis.attachments[0].png, png);
        assert_eq!(analysis.attachments[0].utf16_position, 3);
    }

    #[test]
    fn destinations_and_field_instructions_never_become_body_or_attachments() {
        let rtf = br#"{\rtf1{\*\generator tailsync}{\fonttbl{\f0 Arial}}\field{\*\fldinst HYPERLINK "http://example.com/secret"}{\fldrslt shown}}"#;
        let analysis = scan_rtf(rtf);
        assert!(analysis.attachments.is_empty());
        assert!(!analysis.body.contains("example.com"), "{}", analysis.body);
        assert!(analysis.body.contains("shown"), "{}", analysis.body);
    }

    #[test]
    fn basic_styles_and_tables_are_reported_from_control_words() {
        let rtf =
            br"{\rtf1\b bold\b0 \i it\i0 \ul u\ulnone \trowd\cell a\cell b\row \trowd\cell c\row}";
        let analysis = scan_rtf(rtf);
        assert_ne!(analysis.style_bits & STYLE_BOLD, 0);
        assert_ne!(analysis.style_bits & STYLE_ITALIC, 0);
        assert_ne!(analysis.style_bits & STYLE_UNDERLINE, 0);
        assert_eq!(analysis.table_rows, 2);
        assert_eq!(analysis.table_cells, 3);
    }

    #[test]
    fn non_png_picture_is_unsupported_not_a_fabricated_image() {
        let rtf = br"{\rtf1 A{\pict\wmetafile8 0102030405}B}";
        let analysis = scan_rtf(rtf);
        assert_eq!(analysis.attachments.len(), 1);
        assert_eq!(analysis.attachments[0].state, AttachmentState::Unsupported);
        assert!(analysis.attachments[0].png.is_empty());
    }

    #[test]
    fn deep_or_overrun_input_stops_at_the_admission_bound() {
        let mut rtf = String::from("{\\rtf1 ");
        for _ in 0..(RTF_GROUP_DEPTH_LIMIT + 10) {
            rtf.push('{');
        }
        let analysis = scan_rtf(rtf.as_bytes());
        assert!(analysis.partial);
    }
}
