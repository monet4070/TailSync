//! Low-level HTML lexical helpers shared by the composite consumers.
//!
//! These routines only locate a tag end, locate an attribute value span, and
//! decode a single character entity. Every caller keeps its own whitespace,
//! budget, unsupported, and rejection policy on top of them; moving the shared
//! logic here must not change any of those policies.

/// Decode one HTML character entity body, that is the text between `&` and `;`.
///
/// Named entities and `#x`/`#X` hex or decimal numeric references are accepted;
/// unknown names, invalid scalars, and out-of-range code points return `None`.
pub(crate) fn decode_entity(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" | "#39" => Some('\''),
        "nbsp" => Some('\u{a0}'),
        "copy" => Some('©'),
        "reg" => Some('®'),
        "trade" => Some('™'),
        "hellip" => Some('…'),
        "mdash" => Some('—'),
        "ndash" => Some('–'),
        "lsquo" => Some('‘'),
        "rsquo" => Some('’'),
        "ldquo" => Some('“'),
        "rdquo" => Some('”'),
        _ => entity
            .strip_prefix("#x")
            .or_else(|| entity.strip_prefix("#X"))
            .and_then(|digits| u32::from_str_radix(digits, 16).ok())
            .or_else(|| {
                entity
                    .strip_prefix('#')
                    .and_then(|digits| digits.parse().ok())
            })
            .and_then(char::from_u32),
    }
}

/// Return the index of the `>` that ends a tag starting at `cursor`, skipping
/// any `>` that sits inside a single or double quoted attribute value.
pub(crate) fn find_tag_end(bytes: &[u8], mut cursor: usize) -> Option<usize> {
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'>' => return Some(cursor),
            b'\'' | b'"' => {
                let quote = bytes[cursor];
                cursor += 1;
                while cursor < bytes.len() && bytes[cursor] != quote {
                    cursor += 1;
                }
                if cursor >= bytes.len() {
                    return None;
                }
            }
            _ => {}
        }
        cursor += 1;
    }
    None
}

/// Return the byte span of the first `name` attribute value inside
/// `bytes[cursor..end]`. The value is `None` for a bare attribute and the
/// unquoted value ends at whitespace only, so a trailing `/` or `>` is part of
/// the span; callers that need a narrower scan keep their own routine.
pub(crate) fn attribute_span(
    bytes: &[u8],
    mut cursor: usize,
    end: usize,
    name: &[u8],
) -> Option<(usize, usize)> {
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
        let source = bytes[start..cursor].eq_ignore_ascii_case(name);
        while cursor < end && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if bytes.get(cursor) != Some(&b'=') {
            if cursor == start {
                cursor += 1;
            }
            continue;
        }
        cursor += 1;
        while cursor < end && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        let quote = bytes
            .get(cursor)
            .copied()
            .filter(|value| matches!(value, b'\'' | b'"'));
        if quote.is_some() {
            cursor += 1;
        }
        let value_start = cursor;
        while cursor < end
            && match quote {
                Some(quote) => bytes[cursor] != quote,
                None => !bytes[cursor].is_ascii_whitespace(),
            }
        {
            cursor += 1;
        }
        if source {
            return Some((value_start, cursor));
        }
        if quote.is_some() {
            cursor += 1;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{attribute_span, decode_entity, find_tag_end};

    #[test]
    fn named_and_numeric_entities_decode() {
        assert_eq!(decode_entity("amp"), Some('&'));
        assert_eq!(decode_entity("lt"), Some('<'));
        assert_eq!(decode_entity("gt"), Some('>'));
        assert_eq!(decode_entity("quot"), Some('"'));
        assert_eq!(decode_entity("apos"), Some('\''));
        assert_eq!(decode_entity("#39"), Some('\''));
        assert_eq!(decode_entity("nbsp"), Some('\u{a0}'));
        assert_eq!(decode_entity("copy"), Some('©'));
        assert_eq!(decode_entity("hellip"), Some('…'));
        assert_eq!(decode_entity("#x1F642"), Some('\u{1F642}'));
        assert_eq!(decode_entity("#X1F642"), Some('\u{1F642}'));
        assert_eq!(decode_entity("#x1f642"), Some('\u{1F642}'));
        assert_eq!(decode_entity("#20013"), Some('中'));
    }

    #[test]
    fn invalid_entities_are_rejected() {
        assert_eq!(decode_entity("#0"), Some('\0'));
        assert_eq!(decode_entity("#xD800"), None);
        assert_eq!(decode_entity("#x110000"), None);
        assert_eq!(decode_entity("#-1"), None);
        assert_eq!(decode_entity("#xnothex"), None);
        assert_eq!(decode_entity("unknown"), None);
        assert_eq!(decode_entity("AMP"), None);
        assert_eq!(decode_entity(""), None);
        assert_eq!(
            decode_entity("9999999999999999999999999999999999999999"),
            None
        );
    }

    #[test]
    fn tag_end_skips_quoted_delimiters() {
        assert_eq!(find_tag_end(b"a href=\"x>y\">", 0), Some(12));
        assert_eq!(find_tag_end(b"a href='x>y'>", 0), Some(12));
        assert_eq!(find_tag_end(b"a href=\"x>y\"", 0), None);
        assert_eq!(find_tag_end(b">", 0), Some(0));
        assert_eq!(find_tag_end(b"abc", 0), None);
    }

    #[test]
    fn attribute_span_stops_at_whitespace_only() {
        assert_eq!(attribute_span(b"class=x/", 0, 8, b"class"), Some((6, 8)));
        assert_eq!(
            attribute_span(b"class=\"x/y\"", 0, 11, b"class"),
            Some((7, 10))
        );
        assert_eq!(attribute_span(b"href", 0, 4, b"href"), None);
        assert_eq!(
            attribute_span(b"href=\"a b\"", 0, 10, b"href"),
            Some((6, 9))
        );
    }
}
