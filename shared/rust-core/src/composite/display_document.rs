use serde::{Deserialize, Serialize};

fn is_zero(value: &u8) -> bool {
    *value == 0
}

fn is_false(value: &bool) -> bool {
    !*value
}

pub const DOCUMENT_VERSION: u16 = 1;
pub const DOCUMENT_MAX_BYTES: usize = 16 * 1024 * 1024;
pub const DOCUMENT_MAX_METADATA_BYTES: usize = 1024 * 1024;
pub const DOCUMENT_MAX_MODEL_BYTES: usize = 4 * 1024 * 1024;
pub const DOCUMENT_TEXT_LIMIT: usize = 1024 * 1024;
pub const DOCUMENT_BLOCK_LIMIT: usize = 8192;
pub const DOCUMENT_RUN_LIMIT: usize = 8192;
pub const DOCUMENT_CELL_LIMIT: usize = 2048;
pub const DOCUMENT_ASSET_LIMIT: usize = 64;
pub const DOCUMENT_PIXEL_LIMIT: u64 = 16_000_000;
pub const DOCUMENT_DETAIL_LIMIT: usize = 512;
pub const DOCUMENT_LINK_LIMIT: usize = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DocumentBlockKind {
    Paragraph,
    Table,
    Image,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DocumentListKind {
    Bullet,
    Ordered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DocumentImageAvailability {
    Available,
    MissingResource,
    MissingSource,
    ExternalReference,
    Unsupported,
    PixelBudget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DocumentPartialCode {
    TextLimit,
    BlockLimit,
    RunLimit,
    CellLimit,
    ImageLimit,
    PixelBudget,
    DepthLimit,
    MalformedHtml,
    UnsupportedMarkup,
    UnsupportedDependency,
    MissingImageSource,
    ExternalReference,
    NativeAnalysisUnavailable,
    NativeStylesUnavailable,
    NativeTableStructureUnavailable,
    NativePartial,
    UnverifiedProvenance,
    OtherNativeItems,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DocumentColor {
    #[schemars(range(max = 255))]
    pub red: u8,
    #[schemars(range(max = 255))]
    pub green: u8,
    #[schemars(range(max = 255))]
    pub blue: u8,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, schemars::JsonSchema,
)]
pub struct DocumentRunStyle {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub subscript: bool,
    pub superscript: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub monospace: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<DocumentColor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub highlight: Option<DocumentColor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DocumentRun {
    pub text: String,
    pub style: DocumentRunStyle,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    pub unsupported: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DocumentParagraph {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list: Option<DocumentListKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 3))]
    pub heading: Option<u8>,
    #[serde(default, skip_serializing_if = "is_zero")]
    #[schemars(range(max = 8))]
    pub quote_depth: u8,
    #[serde(default, skip_serializing_if = "is_false")]
    pub code_block: bool,
    #[serde(default, skip_serializing_if = "is_zero")]
    #[schemars(range(max = 8))]
    pub list_depth: u8,
    pub runs: Vec<DocumentRun>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DocumentTableCell {
    pub header: bool,
    pub runs: Vec<DocumentRun>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DocumentTableRow {
    pub cells: Vec<DocumentTableCell>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DocumentTable {
    pub rows: Vec<DocumentTableRow>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DocumentImage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset: Option<u16>,
    pub availability: DocumentImageAvailability,
    #[schemars(range(max = 65535))]
    pub ordinal: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DocumentBlock {
    pub kind: DocumentBlockKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paragraph: Option<DocumentParagraph>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<DocumentTable>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<DocumentImage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DocumentAsset {
    #[schemars(range(max = 65535))]
    pub index: u16,
    pub offset: u64,
    pub length: u64,
    pub width: u32,
    pub height: u32,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DocumentPartial {
    pub code: DocumentPartialCode,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DisplayDocument {
    #[schemars(range(min = DOCUMENT_VERSION, max = DOCUMENT_VERSION))]
    pub version: u16,
    pub blocks: Vec<DocumentBlock>,
    pub assets: Vec<DocumentAsset>,
    pub partial: Vec<DocumentPartial>,
    pub styles_partial: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_image: Option<u16>,
}

impl Default for DisplayDocument {
    fn default() -> Self {
        Self {
            version: DOCUMENT_VERSION,
            blocks: Vec::new(),
            assets: Vec::new(),
            partial: Vec::new(),
            styles_partial: false,
            first_image: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentError {
    UnsupportedVersion,
    Invalid,
}

impl DisplayDocument {
    pub fn retained_bytes(&self) -> usize {
        fn runs_bytes(runs: &[DocumentRun], capacity: usize) -> usize {
            capacity * std::mem::size_of::<DocumentRun>()
                + runs
                    .iter()
                    .map(|run| {
                        run.text.capacity() + run.link.as_ref().map_or(0, |link| link.capacity())
                    })
                    .sum::<usize>()
        }
        let mut bytes = self.blocks.capacity() * std::mem::size_of::<DocumentBlock>()
            + self.assets.capacity() * std::mem::size_of::<DocumentAsset>()
            + self.partial.capacity() * std::mem::size_of::<DocumentPartial>();
        for block in &self.blocks {
            if let Some(paragraph) = &block.paragraph {
                bytes += runs_bytes(&paragraph.runs, paragraph.runs.capacity());
            }
            if let Some(table) = &block.table {
                bytes += table.rows.capacity() * std::mem::size_of::<DocumentTableRow>();
                for row in &table.rows {
                    bytes += row.cells.capacity() * std::mem::size_of::<DocumentTableCell>();
                    for cell in &row.cells {
                        bytes += runs_bytes(&cell.runs, cell.runs.capacity());
                    }
                }
            }
        }
        bytes
            + self
                .assets
                .iter()
                .map(|asset| asset.sha256.capacity())
                .sum::<usize>()
            + self
                .partial
                .iter()
                .map(|reason| reason.detail.capacity())
                .sum::<usize>()
    }
    pub fn validate(&self) -> Result<(), DocumentError> {
        if self.version != DOCUMENT_VERSION {
            return Err(DocumentError::UnsupportedVersion);
        }
        if self.blocks.len() > DOCUMENT_BLOCK_LIMIT
            || self.assets.len() > DOCUMENT_ASSET_LIMIT
            || self.partial.len() > DOCUMENT_BLOCK_LIMIT
            || self
                .first_image
                .is_some_and(|value| value as usize >= self.assets.len())
        {
            return Err(DocumentError::Invalid);
        }
        let mut text_bytes = 0usize;
        let mut runs = 0usize;
        let mut cells = 0usize;
        let mut referenced = vec![false; self.assets.len()];
        let mut first_image = None;
        let mut image_count = 0usize;
        for block in &self.blocks {
            match block.kind {
                DocumentBlockKind::Paragraph => {
                    let Some(paragraph) = block.paragraph.as_ref() else {
                        return Err(DocumentError::Invalid);
                    };
                    if block.table.is_some() || block.image.is_some() {
                        return Err(DocumentError::Invalid);
                    }
                    if paragraph
                        .heading
                        .is_some_and(|level| !matches!(level, 1..=3))
                        || paragraph.quote_depth > 8
                        || paragraph.list_depth > 8
                        || (paragraph.list.is_none() && paragraph.list_depth != 0)
                    {
                        return Err(DocumentError::Invalid);
                    }
                    runs = runs.saturating_add(paragraph.runs.len());
                    text_bytes = text_bytes.saturating_add(run_text(&paragraph.runs)?);
                }
                DocumentBlockKind::Table => {
                    let Some(table) = block.table.as_ref() else {
                        return Err(DocumentError::Invalid);
                    };
                    if block.paragraph.is_some() || block.image.is_some() {
                        return Err(DocumentError::Invalid);
                    }
                    if table.rows.len() > DOCUMENT_CELL_LIMIT {
                        return Err(DocumentError::Invalid);
                    }
                    for row in &table.rows {
                        if row.cells.is_empty() || row.cells.len() > DOCUMENT_CELL_LIMIT {
                            return Err(DocumentError::Invalid);
                        }
                        for cell in &row.cells {
                            cells = cells.saturating_add(1);
                            runs = runs.saturating_add(cell.runs.len());
                            text_bytes = text_bytes.saturating_add(run_text(&cell.runs)?);
                        }
                    }
                }
                DocumentBlockKind::Image => {
                    let Some(image) = block.image.as_ref() else {
                        return Err(DocumentError::Invalid);
                    };
                    if block.paragraph.is_some() || block.table.is_some() {
                        return Err(DocumentError::Invalid);
                    }
                    if image
                        .asset
                        .is_some_and(|index| index as usize >= self.assets.len())
                    {
                        return Err(DocumentError::Invalid);
                    }
                    if image.ordinal as usize != image_count
                        || (image.availability == DocumentImageAvailability::Available)
                            != image.asset.is_some()
                    {
                        return Err(DocumentError::Invalid);
                    }
                    if image_count == 0 {
                        first_image = image.asset;
                    }
                    image_count += 1;
                    if let Some(index) = image.asset {
                        referenced[index as usize] = true;
                    }
                }
            }
        }
        if referenced.contains(&false)
            || self.first_image != first_image
            || image_count > DOCUMENT_ASSET_LIMIT
        {
            return Err(DocumentError::Invalid);
        }
        if runs > DOCUMENT_RUN_LIMIT
            || cells > DOCUMENT_CELL_LIMIT
            || text_bytes > DOCUMENT_TEXT_LIMIT
        {
            return Err(DocumentError::Invalid);
        }
        let mut offset = 0u64;
        let mut pixels = 0u64;
        for (position, asset) in self.assets.iter().enumerate() {
            if asset.index as usize != position
                || asset.offset != offset
                || asset.length == 0
                || asset.width == 0
                || asset.height == 0
                || asset.sha256.len() != 64
                || !asset.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(DocumentError::Invalid);
            }
            offset = offset
                .checked_add(asset.length)
                .ok_or(DocumentError::Invalid)?;
            pixels = pixels
                .checked_add(u64::from(asset.width) * u64::from(asset.height))
                .ok_or(DocumentError::Invalid)?;
        }
        if offset > DOCUMENT_MAX_BYTES as u64 || pixels > DOCUMENT_PIXEL_LIMIT {
            return Err(DocumentError::Invalid);
        }
        for reason in &self.partial {
            if reason.detail.len() > DOCUMENT_DETAIL_LIMIT || reason.detail.contains('\0') {
                return Err(DocumentError::Invalid);
            }
        }
        Ok(())
    }

    pub fn validate_assets(&self, payload: &[u8]) -> Result<(), DocumentError> {
        self.validate()?;
        let mut end = 0usize;
        for asset in &self.assets {
            end = end
                .checked_add(usize::try_from(asset.length).map_err(|_| DocumentError::Invalid)?)
                .ok_or(DocumentError::Invalid)?;
            let start = usize::try_from(asset.offset).map_err(|_| DocumentError::Invalid)?;
            let bytes = payload.get(start..end).ok_or(DocumentError::Invalid)?;
            if bytes.len() < 24
                || &bytes[..16] != b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"
                || u32::from_be_bytes(
                    bytes[16..20]
                        .try_into()
                        .map_err(|_| DocumentError::Invalid)?,
                ) != asset.width
                || u32::from_be_bytes(
                    bytes[20..24]
                        .try_into()
                        .map_err(|_| DocumentError::Invalid)?,
                ) != asset.height
                || super::document_build::asset_digest(bytes) != asset.sha256
            {
                return Err(DocumentError::Invalid);
            }
        }
        if end != payload.len() {
            return Err(DocumentError::Invalid);
        }
        Ok(())
    }
}

fn run_text(runs: &[DocumentRun]) -> Result<usize, DocumentError> {
    let mut total = 0usize;
    for run in runs {
        if run.text.contains('\0') || run.text.len() > DOCUMENT_TEXT_LIMIT {
            return Err(DocumentError::Invalid);
        }
        total = total
            .saturating_add(run.text.len())
            .saturating_add(run_link_bytes(run)?);
    }
    Ok(total)
}

fn run_link_bytes(run: &DocumentRun) -> Result<usize, DocumentError> {
    let Some(link) = run.link.as_deref() else {
        return Ok(0);
    };
    if !valid_document_link(link) {
        return Err(DocumentError::Invalid);
    }
    Ok(link.len())
}

pub(super) fn link_has_userinfo(link: &str) -> bool {
    let Some(authority) = link
        .strip_prefix("//")
        .or_else(|| link.split_once("://").map(|(_, authority)| authority))
    else {
        return false;
    };
    let end = authority.find(['/', '?', '#']).unwrap_or(authority.len());
    authority[..end].contains('@')
}

pub fn valid_document_link(link: &str) -> bool {
    if link.is_empty() || link.len() > DOCUMENT_LINK_LIMIT {
        return false;
    }
    if link
        .chars()
        .any(|character| character.is_whitespace() || character.is_control() || character == '\\')
    {
        return false;
    }
    if link_has_userinfo(link) {
        return false;
    }
    let Some((scheme, authority)) = link.split_once("://") else {
        return false;
    };
    if !(scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
        || authority.is_empty()
        || authority.starts_with(['/', '?', '#'])
    {
        return false;
    }
    match url::Url::parse(link) {
        Ok(url) => {
            matches!(url.scheme(), "http" | "https")
                && url.host().is_some()
                && url.username().is_empty()
                && url.password().is_none()
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paragraph(text: &str) -> DocumentBlock {
        DocumentBlock {
            kind: DocumentBlockKind::Paragraph,
            paragraph: Some(DocumentParagraph {
                list: None,
                heading: None,
                quote_depth: 0,
                code_block: false,
                list_depth: 0,
                runs: vec![DocumentRun {
                    text: text.into(),
                    style: DocumentRunStyle::default(),
                    link: None,
                    unsupported: false,
                }],
            }),
            table: None,
            image: None,
        }
    }

    fn image(asset: Option<u16>, ordinal: u16) -> DocumentBlock {
        DocumentBlock {
            kind: DocumentBlockKind::Image,
            paragraph: None,
            table: None,
            image: Some(DocumentImage {
                asset,
                availability: if asset.is_some() {
                    DocumentImageAvailability::Available
                } else {
                    DocumentImageAvailability::MissingResource
                },
                ordinal,
            }),
        }
    }

    fn asset(index: u16, offset: u64, length: u64) -> DocumentAsset {
        DocumentAsset {
            index,
            offset,
            length,
            width: 1,
            height: 1,
            sha256: "0a".repeat(32),
        }
    }

    fn document(blocks: Vec<DocumentBlock>, assets: Vec<DocumentAsset>) -> DisplayDocument {
        let first_image = blocks
            .iter()
            .find_map(|block| block.image.as_ref())
            .and_then(|image| image.asset);
        DisplayDocument {
            version: DOCUMENT_VERSION,
            blocks,
            assets,
            partial: vec![],
            styles_partial: false,
            first_image,
        }
    }

    #[test]
    fn an_unknown_version_is_rejected() {
        let mut value = document(vec![paragraph("hello")], vec![]);
        value.version = DOCUMENT_VERSION + 1;
        assert_eq!(value.validate(), Err(DocumentError::UnsupportedVersion));
    }

    #[test]
    fn a_block_whose_kind_disagrees_with_its_payload_is_rejected() {
        let mut value = document(vec![paragraph("hello")], vec![]);
        value.blocks[0].table = Some(DocumentTable {
            rows: vec![],
            truncated: false,
        });
        assert_eq!(value.validate(), Err(DocumentError::Invalid));
    }

    #[test]
    fn asset_indices_and_offsets_must_be_contiguous() {
        let mut value = document(
            vec![image(Some(0), 0), image(Some(1), 1)],
            vec![asset(0, 0, 4), asset(1, 4, 4)],
        );
        assert_eq!(value.validate(), Ok(()));
        value.assets[1].offset = 5;
        assert_eq!(value.validate(), Err(DocumentError::Invalid));
        value.assets[1].offset = 4;
        value.assets[1].index = 0;
        assert_eq!(value.validate(), Err(DocumentError::Invalid));
    }

    #[test]
    fn an_image_that_references_a_missing_asset_is_rejected() {
        let value = document(vec![image(Some(3), 0)], vec![asset(0, 0, 4)]);
        assert_eq!(value.validate(), Err(DocumentError::Invalid));
    }

    #[test]
    fn a_run_budget_overflow_is_rejected() {
        let runs = (0..=DOCUMENT_RUN_LIMIT)
            .map(|_| DocumentRun {
                text: "x".into(),
                style: DocumentRunStyle::default(),
                link: None,
                unsupported: false,
            })
            .collect();
        let value = document(
            vec![DocumentBlock {
                kind: DocumentBlockKind::Paragraph,
                paragraph: Some(DocumentParagraph {
                    list: None,
                    heading: None,
                    quote_depth: 0,
                    code_block: false,
                    list_depth: 0,
                    runs,
                }),
                table: None,
                image: None,
            }],
            vec![],
        );
        assert_eq!(value.validate(), Err(DocumentError::Invalid));
    }

    #[test]
    fn a_non_hex_asset_digest_is_rejected() {
        let mut value = document(vec![image(Some(0), 0)], vec![asset(0, 0, 4)]);
        value.assets[0].sha256 = "zz".repeat(32);
        assert_eq!(value.validate(), Err(DocumentError::Invalid));
    }

    #[test]
    fn reading_semantics_fields_are_validated() {
        let mut value = document(vec![paragraph("hi")], vec![]);
        let attributes = value.blocks[0].paragraph.as_mut().unwrap();
        attributes.list = Some(DocumentListKind::Bullet);
        attributes.heading = Some(3);
        attributes.quote_depth = 8;
        attributes.code_block = true;
        attributes.list_depth = 2;
        assert_eq!(value.validate(), Ok(()));

        for heading in [Some(0u8), Some(4), Some(u8::MAX)] {
            let mut value = document(vec![paragraph("hi")], vec![]);
            value.blocks[0].paragraph.as_mut().unwrap().heading = heading;
            assert_eq!(value.validate(), Err(DocumentError::Invalid));
        }
        for depth in [9u8, u8::MAX] {
            let mut value = document(vec![paragraph("hi")], vec![]);
            value.blocks[0].paragraph.as_mut().unwrap().quote_depth = depth;
            assert_eq!(value.validate(), Err(DocumentError::Invalid));
            let mut value = document(vec![paragraph("hi")], vec![]);
            let paragraph = value.blocks[0].paragraph.as_mut().unwrap();
            paragraph.list = Some(DocumentListKind::Ordered);
            paragraph.list_depth = depth;
            assert_eq!(value.validate(), Err(DocumentError::Invalid));
        }
        let mut value = document(vec![paragraph("hi")], vec![]);
        value.blocks[0].paragraph.as_mut().unwrap().list_depth = 1;
        assert_eq!(value.validate(), Err(DocumentError::Invalid));
    }

    #[test]
    fn document_link_validation_rejects_unsafe_forms() {
        let prefix = "https://example.com/";
        let longest = format!("{prefix}{}", "a".repeat(DOCUMENT_LINK_LIMIT - prefix.len()));
        assert_eq!(longest.len(), DOCUMENT_LINK_LIMIT);
        assert!(valid_document_link(&longest));
        assert!(valid_document_link("http://example.com"));
        assert!(valid_document_link("https://example.com/path?q=1&r=2#frag"));
        for unsafe_link in [
            "",
            "/relative/path",
            "example.com",
            "https://",
            "javascript:alert(1)",
            "data:text/html,x",
            "file:///etc/passwd",
            "https://user:pass@example.com/",
            "https://@example.com/",
            "https://user@example.com/",
            "https://example.com/a\\b",
            "https://example.com/a b",
            "https://example.com/a\tb",
            "https://example.com/a\nb",
            "https://example.com/a\u{7}b",
            "https://example.com/a\u{a0}b",
        ] {
            assert!(!valid_document_link(unsafe_link), "{unsafe_link}");
        }
        let too_long = format!("{prefix}{}", "a".repeat(DOCUMENT_LINK_LIMIT));
        assert!(!valid_document_link(&too_long));
    }

    #[test]
    fn a_link_counts_toward_the_text_budget_and_retention() {
        let mut value = document(vec![paragraph("hi")], vec![]);
        let link = "https://example.com/a".to_string();
        value.blocks[0].paragraph.as_mut().unwrap().runs[0].link = Some(link.clone());
        assert_eq!(value.validate(), Ok(()));
        let plain = document(vec![paragraph("hi")], vec![]);
        assert_eq!(
            value.retained_bytes(),
            plain.retained_bytes() + link.capacity()
        );

        let text = "x".repeat(DOCUMENT_TEXT_LIMIT);
        let mut value = document(vec![paragraph(&text)], vec![]);
        value.blocks[0].paragraph.as_mut().unwrap().runs[0].link = Some(link);
        assert_eq!(value.validate(), Err(DocumentError::Invalid));
    }

    #[test]
    fn documents_without_the_new_fields_decode_with_defaults() {
        let json = r#"{"version":1,"blocks":[{"kind":"paragraph","paragraph":{"list":null,"runs":[{"text":"hi","style":{"bold":false,"italic":false,"underline":false,"strikethrough":false,"subscript":false,"superscript":false},"unsupported":false}]}}],"assets":[],"partial":[],"styles_partial":false}"#;
        let value: DisplayDocument = serde_json::from_str(json).unwrap();
        assert_eq!(value.validate(), Ok(()));
        let paragraph = value.blocks[0].paragraph.as_ref().unwrap();
        assert_eq!(paragraph.heading, None);
        assert_eq!(paragraph.quote_depth, 0);
        assert!(!paragraph.code_block);
        assert_eq!(paragraph.list_depth, 0);
        assert_eq!(paragraph.runs[0].link, None);
        assert!(!paragraph.runs[0].style.monospace);
        assert_eq!(paragraph.runs[0].style.highlight, None);
    }

    #[test]
    fn default_reading_semantics_are_omitted_when_serialized() {
        let json = serde_json::to_string(&document(vec![paragraph("hi")], vec![])).unwrap();
        for field in [
            "heading",
            "quote_depth",
            "code_block",
            "list_depth",
            "monospace",
            "highlight",
            "link",
        ] {
            assert!(!json.contains(field), "{field} in {json}");
        }
    }
}
