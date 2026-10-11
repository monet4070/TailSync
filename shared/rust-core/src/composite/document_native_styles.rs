use super::display_document::{
    DisplayDocument, DocumentBlockKind, DocumentRun, DocumentRunStyle, DOCUMENT_RUN_LIMIT,
};
use crate::protocol::content_analysis::{
    RtfStyleSpan, STYLE_BOLD, STYLE_ITALIC, STYLE_STRIKETHROUGH, STYLE_SUBSCRIPT,
    STYLE_SUPERSCRIPT, STYLE_UNDERLINE,
};

pub(super) fn apply_rtf_styles(
    document: &mut DisplayDocument,
    body: &str,
    spans: &[RtfStyleSpan],
) -> bool {
    let mut reconstructed = String::new();
    for block in &document.blocks {
        match block.kind {
            DocumentBlockKind::Paragraph => {
                let Some(paragraph) = &block.paragraph else {
                    return false;
                };
                for run in &paragraph.runs {
                    reconstructed.push_str(&run.text);
                }
            }
            DocumentBlockKind::Image => reconstructed.push('\u{fffc}'),
            DocumentBlockKind::Table => return false,
        }
    }
    if reconstructed != body {
        return false;
    }
    let mut cursor = 0usize;
    for span in spans {
        if span.start != cursor
            || span.end <= span.start
            || span.end > body.len()
            || !body.is_char_boundary(span.start)
            || !body.is_char_boundary(span.end)
        {
            return false;
        }
        cursor = span.end;
    }
    if cursor != body.len() {
        return false;
    }
    let mut replacements = Vec::new();
    let mut span_index = 0usize;
    let mut offset = 0usize;
    let mut run_count = 0usize;
    for (block_index, block) in document.blocks.iter().enumerate() {
        if block.kind == DocumentBlockKind::Image {
            offset += '\u{fffc}'.len_utf8();
            continue;
        }
        let paragraph = block.paragraph.as_ref().unwrap();
        let end = offset
            + paragraph
                .runs
                .iter()
                .map(|run| run.text.len())
                .sum::<usize>();
        let mut runs = Vec::new();
        while offset < end {
            while spans.get(span_index).is_some_and(|span| span.end <= offset) {
                span_index += 1;
            }
            let Some(span) = spans.get(span_index) else {
                return false;
            };
            let finish = span.end.min(end);
            runs.push(DocumentRun {
                text: body[offset..finish].to_owned(),
                style: DocumentRunStyle {
                    bold: span.style_bits & STYLE_BOLD != 0,
                    italic: span.style_bits & STYLE_ITALIC != 0,
                    underline: span.style_bits & STYLE_UNDERLINE != 0,
                    strikethrough: span.style_bits & STYLE_STRIKETHROUGH != 0,
                    subscript: span.style_bits & STYLE_SUBSCRIPT != 0,
                    superscript: span.style_bits & STYLE_SUPERSCRIPT != 0,
                    ..Default::default()
                },
                unsupported: false,
                link: None,
            });
            run_count += 1;
            if run_count > DOCUMENT_RUN_LIMIT {
                return false;
            }
            offset = finish;
        }
        replacements.push((block_index, runs));
    }
    for (block_index, runs) in replacements {
        document.blocks[block_index]
            .paragraph
            .as_mut()
            .unwrap()
            .runs = runs;
    }
    document.partial.retain(|reason| {
        reason.code != super::display_document::DocumentPartialCode::NativeStylesUnavailable
    });
    true
}
