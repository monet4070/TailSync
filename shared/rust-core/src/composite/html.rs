//! CF_HTML offsets are UTF-8 byte offsets, never character offsets.
use crate::protocol::ProtocolError;
fn invalid() -> ProtocolError {
    ProtocolError::InvalidComposite
}
pub fn encode_cf_html(html: &str) -> Result<Vec<u8>, ProtocolError> {
    if html.len() > crate::protocol::composite::Mime::Html.limit() {
        return Err(invalid());
    }
    let doc = if html.contains("<!--StartFragment-->") && html.contains("<!--EndFragment-->") {
        html.to_string()
    } else {
        format!("<html><body><!--StartFragment-->{html}<!--EndFragment--></body></html>")
    };
    let template="Version:1.0\r\nStartHTML:0000000000\r\nEndHTML:0000000000\r\nStartFragment:0000000000\r\nEndFragment:0000000000\r\n";
    let start = template.len();
    let fragment_start = start + doc.find("<!--StartFragment-->").ok_or_else(invalid)? + 20;
    let fragment_end = start + doc.find("<!--EndFragment-->").ok_or_else(invalid)?;
    if fragment_start > fragment_end {
        return Err(invalid());
    }
    let header=format!("Version:1.0\r\nStartHTML:{start:010}\r\nEndHTML:{:010}\r\nStartFragment:{fragment_start:010}\r\nEndFragment:{fragment_end:010}\r\n",start+doc.len());
    let mut result = header.into_bytes();
    result.extend_from_slice(doc.as_bytes());
    result.push(0);
    Ok(result)
}
pub fn decode_cf_html(bytes: &[u8]) -> Result<String, ProtocolError> {
    if bytes.len() > crate::protocol::composite::Mime::Html.limit() + 4096 {
        return Err(invalid());
    }
    // Headers must fit the control budget; do not scan body text as metadata.
    let header_end = bytes
        .iter()
        .position(|b| *b == b'<')
        .unwrap_or(bytes.len())
        .min(4096);
    let header = std::str::from_utf8(&bytes[..header_end]).map_err(|_| invalid())?;
    let number = |name: &str| -> Result<usize, ProtocolError> {
        let line = header
            .lines()
            .find_map(|l| l.strip_prefix(name))
            .ok_or_else(invalid)?;
        line.trim().parse().map_err(|_| invalid())
    };
    let start = number("StartHTML:")?;
    let end = number("EndHTML:")?;
    let fs = number("StartFragment:")?;
    let fe = number("EndFragment:")?;
    if start < header_end
        || start > fs
        || fs > fe
        || fe > end
        || end > bytes.len()
        || bytes[end..].iter().any(|b| *b != 0)
    {
        return Err(invalid());
    }
    let html = std::str::from_utf8(&bytes[start..end]).map_err(|_| invalid())?;
    if !html.is_char_boundary(fs - start) || !html.is_char_boundary(fe - start) {
        return Err(invalid());
    }
    // Preserve full context; SourceURL becomes base context in the portable
    // copy, while the native snapshot retains the untouched original bytes.
    let source = header
        .lines()
        .find_map(|l| l.strip_prefix("SourceURL:"))
        .map(str::trim);
    if let Some(url) = source.filter(|u| u.starts_with("https://") || u.starts_with("http://")) {
        if inventory(html)?.bases.is_empty() {
            let escaped = url
                .replace('&', "&amp;")
                .replace('"', "&quot;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            return Ok(format!("<base href=\"{escaped}\">{html}"));
        }
    }
    Ok(html.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chinese_and_emoji_offsets_are_bytes() {
        let html = "<p>中文 🙂</p>";
        let b = encode_cf_html(html).unwrap();
        let restored = decode_cf_html(&b).unwrap();
        assert!(restored.contains(html));
        let h = String::from_utf8(b.clone()).unwrap();
        let fs: usize = h
            .lines()
            .find_map(|l| l.strip_prefix("StartFragment:"))
            .unwrap()
            .parse()
            .unwrap();
        assert!(b[fs..].starts_with(html.as_bytes()));
    }
    #[test]
    fn rejects_out_of_bounds_and_character_split_offsets() {
        let mut b = encode_cf_html("中文").unwrap();
        let h = std::str::from_utf8(&b).unwrap();
        let start = h.find("StartFragment:").unwrap() + 14;
        b[start..start + 10].copy_from_slice(b"9999999999");
        assert!(decode_cf_html(&b).is_err());
    }

    #[test]
    fn validated_source_context_alone_is_not_an_unloaded_resource() {
        assert!(restore_reference_inventory_complete(
            b"<base href='https://example.com/posts/'><p><a href='../other'>saved</a></p>",
            &[],
        )
        .unwrap());
        for html in [
            "<base href='javascript:active()'><p>saved</p>",
            "<base href='https://example.com/'><base href='https://other.example/'><p>saved</p>",
            "<base href='https://example.com/'><p>saved</p><img src='missing'>",
            "<base href='https://example.com/'><p style='background:url(relative)'>saved</p>",
            "<base href href='https://example.com/'><p>saved</p>",
            "<base href='https://example.com/'><p>saved</p><img src src='data:image/png;base64,AAAA'>",
        ] {
            assert!(
                !matches!(restore_reference_inventory_complete(html.as_bytes(), &[]), Ok(true)),
                "unexpected closure: {html}"
            );
        }
    }
}

/// Byte spans from a bounded HTML lexer. Comments and raw script/style text
/// never contribute image bindings. This preserves the original markup rather
/// than reparsing it through a renderer or executing any content.
#[derive(Debug)]
struct Attribute {
    tag: String,
    name: String,
    element_start: usize,
    value_present: bool,
    start: usize,
    end: usize,
}
struct BaseElementSpan {
    start: usize,
    end: usize,
    attributes: std::ops::Range<usize>,
}
struct HtmlInventory {
    attributes: Vec<Attribute>,
    images: Vec<ImageElementSpan>,
    bases: Vec<BaseElementSpan>,
    missing_image_source: bool,
    unsupported_dependencies: bool,
}
fn css_requires_materialization(bytes: &[u8]) -> bool {
    // CSS imports need no url() token. Escapes and HTML entities can hide both
    // imports and URL functions; without a CSS/entity parser their closure is
    // unproven. Ordinary literal formatting declarations remain eligible.
    bytes.iter().any(|b| matches!(b, b'@' | b'\\' | b'&'))
        || bytes.windows(4).any(|s| s.eq_ignore_ascii_case(b"url("))
}
fn external_attribute(attr: &Attribute, value: &str) -> bool {
    matches!(
        attr.name.as_str(),
        "src" | "srcset" | "background" | "poster"
    ) || (attr.name == "style" && css_requires_materialization(value.as_bytes()))
        || (matches!(attr.name.as_str(), "href" | "xlink:href")
            && !matches!(attr.tag.as_str(), "a" | "base"))
}
fn inventory(html: &str) -> Result<HtmlInventory, ProtocolError> {
    let bytes = html.as_bytes();
    let mut pos = 0;
    let mut result = vec![];
    let mut images = Vec::new();
    let mut bases = Vec::new();
    let mut missing_image_source = false;
    let mut unsupported_dependencies = false;
    while pos < bytes.len() {
        if bytes[pos] != b'<' {
            pos += 1;
            continue;
        }
        if bytes[pos..].starts_with(b"<!--") {
            pos = html[pos + 4..]
                .find("-->")
                .map(|n| pos + 4 + n + 3)
                .unwrap_or(bytes.len());
            continue;
        }
        let tag_start = pos;
        pos += 1;
        let start = pos;
        while pos < bytes.len() && bytes[pos].is_ascii_alphanumeric() {
            pos += 1;
        }
        let tag = html[start..pos].to_ascii_lowercase();
        if tag.is_empty() {
            while pos < bytes.len() && bytes[pos] != b'>' {
                pos += 1;
            }
            pos = pos.saturating_add(1);
            continue;
        }
        unsupported_dependencies |= matches!(
            tag.as_str(),
            "script" | "object" | "iframe" | "embed" | "svg" | "math"
        );
        let attribute_start = result.len();
        while pos < bytes.len() && bytes[pos] != b'>' {
            if bytes[pos].is_ascii_whitespace() || bytes[pos] == b'/' {
                pos += 1;
                continue;
            }
            let name_start = pos;
            while pos < bytes.len()
                && !bytes[pos].is_ascii_whitespace()
                && !matches!(bytes[pos], b'=' | b'>' | b'/')
            {
                pos += 1;
            }
            if pos == name_start {
                pos += 1;
                continue;
            }
            let name = html[name_start..pos].to_ascii_lowercase();
            while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
                pos += 1;
            }
            if bytes.get(pos) != Some(&b'=') {
                if result.len() >= 4096 {
                    return Err(invalid());
                }
                result.push(Attribute {
                    tag: tag.clone(),
                    name,
                    element_start: tag_start,
                    value_present: false,
                    start: pos,
                    end: pos,
                });
                continue;
            }
            pos += 1;
            while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
                pos += 1;
            }
            let quote = bytes
                .get(pos)
                .copied()
                .filter(|b| matches!(b, b'\'' | b'"'));
            if quote.is_some() {
                pos += 1;
            }
            let value_start = pos;
            while pos < bytes.len()
                && if let Some(quote) = quote {
                    bytes[pos] != quote
                } else {
                    !bytes[pos].is_ascii_whitespace() && bytes[pos] != b'>'
                }
            {
                pos += 1;
            }
            let end = pos;
            if quote.is_some() && pos == bytes.len() {
                return Err(invalid());
            }
            if quote.is_some() && pos < bytes.len() {
                pos += 1;
            }
            if result.len() >= 4096 {
                return Err(invalid());
            }
            result.push(Attribute {
                tag: tag.clone(),
                name,
                element_start: tag_start,
                value_present: true,
                start: value_start,
                end,
            });
        }
        if pos >= bytes.len() {
            return Err(invalid());
        }
        if tag == "base" {
            if bases.len() >= 4096 {
                return Err(invalid());
            }
            bases.push(BaseElementSpan {
                start: tag_start,
                end: pos + 1,
                attributes: attribute_start..result.len(),
            });
        }
        if tag == "img" {
            let sources = result[attribute_start..]
                .iter()
                .filter(|attribute| attribute.name == "src")
                .count();
            if sources > 1 {
                return Err(invalid());
            }
            missing_image_source |= sources == 0;
            if images.len() >= 4096 {
                return Err(invalid());
            }
            images.push(ImageElementSpan {
                start: tag_start,
                end: pos + 1,
                source: result[attribute_start..]
                    .iter()
                    .find(|attribute| attribute.name == "src")
                    .map(|attribute| ImageSourceSpan {
                        start: attribute.start,
                        end: attribute.end,
                    }),
            });
        }
        pos = pos.saturating_add(1);
        if matches!(tag.as_str(), "script" | "style") {
            let closing = format!("</{tag}");
            let raw_start = pos.min(bytes.len());
            pos = bytes[raw_start..]
                .windows(closing.len())
                .position(|span| span.eq_ignore_ascii_case(closing.as_bytes()))
                .map(|n| raw_start + n)
                .unwrap_or(bytes.len());
            if tag == "style" && css_requires_materialization(&bytes[raw_start..pos]) {
                unsupported_dependencies = true;
            }
        }
    }
    let mut result = HtmlInventory {
        attributes: result,
        images,
        bases,
        missing_image_source,
        unsupported_dependencies,
    };
    let invalid_context = validated_base_context(html, &result).is_err();
    result.unsupported_dependencies |= invalid_context;
    Ok(result)
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageSourceSpan {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageElementSpan {
    pub start: usize,
    pub end: usize,
    pub source: Option<ImageSourceSpan>,
}

pub fn available_image_inventory(bytes: &[u8]) -> Result<Vec<ImageElementSpan>, ProtocolError> {
    let html = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    let inventory = inventory(html)?;
    if inventory.unsupported_dependencies
        || inventory.attributes.iter().any(|attribute| {
            !(attribute.tag == "img" && attribute.name == "src")
                && external_attribute(attribute, &html[attribute.start..attribute.end])
        })
    {
        return Err(invalid());
    }
    Ok(inventory.images)
}

fn decode_context_attribute(value: &str) -> Result<String, ProtocolError> {
    let mut result = String::with_capacity(value.len());
    let mut remaining = value;
    while let Some(start) = remaining.find('&') {
        result.push_str(&remaining[..start]);
        remaining = &remaining[start + 1..];
        let Some(end) = remaining.find(';').filter(|end| *end <= 32) else {
            result.push('&');
            continue;
        };
        let entity = &remaining[..end];
        let character = match entity {
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "lt" => Some('<'),
            "gt" => Some('>'),
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
        .filter(|character| !character.is_control())
        .ok_or_else(invalid)?;
        result.push(character);
        remaining = &remaining[end + 1..];
    }
    result.push_str(remaining);
    if result.chars().any(char::is_control) {
        return Err(invalid());
    }
    Ok(result)
}

struct ContextEdit {
    start: usize,
    end: usize,
    replacement: Vec<u8>,
}

fn validated_base_context(
    html: &str,
    inventory: &HtmlInventory,
) -> Result<Option<url::Url>, ProtocolError> {
    if inventory.bases.is_empty() {
        return Ok(None);
    }
    if inventory.bases.len() != 1 {
        return Err(invalid());
    }
    let attributes = &inventory.attributes[inventory.bases[0].attributes.clone()];
    if attributes.len() != 1 || attributes[0].name != "href" {
        return Err(invalid());
    }
    let value = &html[attributes[0].start..attributes[0].end];
    let base = url::Url::parse(&decode_context_attribute(value)?).map_err(|_| invalid())?;
    if !matches!(base.scheme(), "http" | "https")
        || base.host().is_none()
        || !base.username().is_empty()
        || base.password().is_some()
    {
        return Err(invalid());
    }
    Ok(Some(base))
}

pub fn prepare_available_html_context(
    bytes: &[u8],
    bindings: &[super::ResourceBinding],
) -> Result<(Vec<u8>, Vec<super::ResourceBinding>), ProtocolError> {
    let html = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    let inventory = inventory(html)?;
    let Some(base) = validated_base_context(html, &inventory)? else {
        return Ok((bytes.to_vec(), bindings.to_vec()));
    };
    let element = &inventory.bases[0];
    let mut edits = vec![ContextEdit {
        start: element.start,
        end: element.end,
        replacement: Vec::new(),
    }];
    let mut href_elements = std::collections::BTreeSet::new();
    for attribute in inventory
        .attributes
        .iter()
        .filter(|attribute| attribute.tag == "a" && attribute.name == "href")
    {
        if !href_elements.insert(attribute.element_start) {
            return Err(invalid());
        }
        if !attribute.value_present {
            continue;
        }
        let value = decode_context_attribute(&html[attribute.start..attribute.end])?;
        match url::Url::parse(&value) {
            Ok(_) => continue,
            Err(url::ParseError::RelativeUrlWithoutBase) => {}
            Err(_) => return Err(invalid()),
        }
        let target = base.join(&value).map_err(|_| invalid())?;
        if !matches!(target.scheme(), "http" | "https")
            || !target.username().is_empty()
            || target.password().is_some()
        {
            return Err(invalid());
        }
        let escaped = target
            .as_str()
            .replace('&', "&amp;")
            .replace('"', "&quot;")
            .replace('\'', "&#39;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        edits.push(ContextEdit {
            start: attribute.start,
            end: attribute.end,
            replacement: escaped.into_bytes(),
        });
    }
    edits.sort_by_key(|edit| edit.start);
    let mut output = Vec::with_capacity(bytes.len());
    let mut offset = 0;
    let mut shifts = vec![0_i64];
    for edit in &edits {
        let added = edit.start.checked_sub(offset).ok_or_else(invalid)?;
        let length = output
            .len()
            .checked_add(added)
            .and_then(|length| length.checked_add(edit.replacement.len()))
            .filter(|length| *length <= crate::protocol::composite::Mime::Html.limit())
            .ok_or_else(invalid)?;
        output.reserve(length - output.len());
        output.extend_from_slice(&bytes[offset..edit.start]);
        output.extend_from_slice(&edit.replacement);
        offset = edit.end;
        let delta = edit.replacement.len() as i64 - (edit.end - edit.start) as i64;
        shifts.push(
            shifts
                .last()
                .unwrap()
                .checked_add(delta)
                .ok_or_else(invalid)?,
        );
    }
    if output.len() + bytes.len() - offset > crate::protocol::composite::Mime::Html.limit() {
        return Err(invalid());
    }
    output.extend_from_slice(&bytes[offset..]);
    let mut adjusted = bindings.to_vec();
    for binding in &mut adjusted {
        let start = binding.offset as usize;
        let end = start
            .checked_add(binding.length as usize)
            .ok_or_else(invalid)?;
        let edit_index = edits.partition_point(|edit| edit.end <= start);
        if edits.get(edit_index).is_some_and(|edit| edit.start < end) {
            return Err(invalid());
        }
        binding.offset = u32::try_from(start as i64 + shifts[edit_index]).map_err(|_| invalid())?;
    }
    Ok((output, adjusted))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HtmlReferenceInventory {
    pub images: Vec<ImageSourceSpan>,
    pub base: Option<url::Url>,
    pub unsupported_dependencies: bool,
    pub missing_image_source: bool,
}

pub fn reference_inventory(bytes: &[u8]) -> Result<HtmlReferenceInventory, ProtocolError> {
    let html = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    let inventory = inventory(html)?;
    Ok(HtmlReferenceInventory {
        base: validated_base_context(html, &inventory)?,
        images: inventory
            .attributes
            .iter()
            .filter(|attribute| attribute.tag == "img" && attribute.name == "src")
            .map(|attribute| ImageSourceSpan {
                start: attribute.start,
                end: attribute.end,
            })
            .collect(),
        unsupported_dependencies: inventory.unsupported_dependencies,
        missing_image_source: inventory.missing_image_source,
    })
}

pub(super) fn document_link_base(bytes: &[u8]) -> Result<Option<url::Url>, ProtocolError> {
    let html = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    let inventory = inventory(html)?;
    let Some(base) = validated_base_context(html, &inventory)? else {
        return Ok(None);
    };
    let attribute = &inventory.attributes[inventory.bases[0].attributes.start];
    let raw = decode_context_attribute(&html[attribute.start..attribute.end])?;
    if !super::display_document::valid_document_link(&raw) {
        return Err(invalid());
    }
    Ok(Some(base))
}

pub(super) fn resolve_resource_reference(
    value: &str,
    base: Option<&url::Url>,
) -> Result<String, ProtocolError> {
    let decoded = decode_context_attribute(value)?;
    if decoded.contains('\\') {
        return Err(invalid());
    }
    if decoded.is_empty() {
        return Ok(decoded);
    }
    match url::Url::parse(&decoded) {
        Ok(_) => Ok(decoded),
        Err(url::ParseError::RelativeUrlWithoutBase) => match base {
            Some(base) => base
                .join(&decoded)
                .map(|url| url.to_string())
                .map_err(|_| invalid()),
            None => Ok(decoded),
        },
        Err(_) => Err(invalid()),
    }
}

/// Extract only bytes that already exist in the source. No network, local file,
/// cookie, or provider dereference is performed.
pub fn extract_resources(bundle: &mut super::PortableBundle) -> Result<(), ProtocolError> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let Some(index) = bundle
        .representations
        .iter()
        .position(|r| r.mime == super::Mime::Html)
    else {
        return Ok(());
    };
    let html = std::str::from_utf8(&bundle.representations[index].bytes).map_err(|_| invalid())?;
    let inventory = inventory(html)?;
    let attrs = &inventory.attributes;
    let mut output = String::with_capacity(html.len());
    let mut offset = 0;
    let mut incomplete = inventory.unsupported_dependencies;
    let mut missing = inventory.missing_image_source;
    let mut hashes = std::collections::BTreeMap::new();
    for attr in attrs {
        let value = &html[attr.start..attr.end];
        if attr.tag == "img" && attr.name == "src" {
            output.push_str(&html[offset..attr.start]);
            let normalized = value.to_ascii_lowercase();
            if normalized.starts_with("data:image/png;base64,") {
                let raw = STANDARD.decode(&value[22..]).ok();
                let png = raw
                    .and_then(|bytes| super::png_to_packed(&bytes).ok())
                    .and_then(|packed| {
                        crate::protocol::PackedImage::try_from(packed.as_slice())
                            .ok()
                            .and_then(|image| {
                                super::packed_to_png(image.width, image.height, image.rgba).ok()
                            })
                    });
                if let Some(png) = png {
                    let hash = *blake3::hash(&png).as_bytes();
                    let id = if let Some(id) = hashes.get(&hash) {
                        *id
                    } else {
                        if bundle.resources.len() >= super::RESOURCE_LIMIT {
                            return Err(invalid());
                        }
                        let id = (bundle.resources.len() + 1) as u16;
                        bundle.resources.push(super::Resource { id, png });
                        hashes.insert(hash, id);
                        id
                    };
                    let uri = format!("tailsync-resource:{id}");
                    bundle.bindings.push(super::ResourceBinding {
                        representation: index as u16,
                        resource: id,
                        offset: output.len() as u32,
                        length: uri.len() as u32,
                    });
                    output.push_str(&uri);
                } else {
                    output.push_str(value);
                    missing = true;
                }
            } else {
                output.push_str(value);
                incomplete = true;
            }
            offset = attr.end;
        } else if external_attribute(attr, value) {
            incomplete = true;
        }
    }
    output.push_str(&html[offset..]);
    bundle.representations[index].bytes = output.into_bytes();
    if missing {
        bundle.completeness = super::Completeness::MissingFormats;
    } else if incomplete && bundle.completeness == super::Completeness::Complete {
        bundle.completeness = super::Completeness::ExternalReferences;
    }
    Ok(())
}
/// Materialize verified resource bindings as self-contained HTML. Nothing is
/// fetched during publication or preview. Acceptance in specific applications
/// is a separate native gate.
/// Exact byte length `materialize` would produce, computed with checked
/// arithmetic and **no allocation**.
///
/// R2-02: a payload can pass the wire gate (2 MiB of HTML, thousands of bindings)
/// and still be impossible to materialise, because every bound resource is inlined
/// as a base64 `data:` URI. Discovering that after `clearContents()` leaves the user
/// with an empty clipboard, so the length is computed up front and the payload is
/// rejected as "not publishable" — while still being safe to store in history.
pub fn materialized_len(
    bundle: &super::PortableBundle,
    index: usize,
) -> Result<usize, ProtocolError> {
    let html = bundle
        .representations
        .get(index)
        .filter(|r| r.mime == super::Mime::Html)
        .ok_or_else(invalid)?;
    let mut bindings = bundle
        .bindings
        .iter()
        .filter(|b| b.representation as usize == index)
        .collect::<Vec<_>>();
    bindings.sort_by_key(|b| b.offset);
    let mut total = 0usize;
    let mut offset = 0usize;
    for binding in bindings {
        let start = binding.offset as usize;
        let end = start
            .checked_add(binding.length as usize)
            .filter(|end| *end <= html.bytes.len())
            .ok_or_else(invalid)?;
        if start < offset {
            return Err(invalid());
        }
        if !reference_matches(&html.bytes[start..end], binding.resource) {
            return Err(invalid());
        }
        let png = &bundle
            .resources
            .iter()
            .find(|r| r.id == binding.resource)
            .ok_or_else(invalid)?
            .png;
        // `segment + "data:image/png;base64," + base64(png)`.
        let encoded = base64_len(png.len())?;
        total = total
            .checked_add(start - offset)
            .and_then(|n| n.checked_add(DATA_URI_PREFIX.len()))
            .and_then(|n| n.checked_add(encoded))
            .ok_or_else(invalid)?;
        if total > super::Mime::Html.limit() {
            return Err(invalid());
        }
        offset = end;
    }
    total
        .checked_add(html.bytes.len() - offset)
        .filter(|total| *total <= super::Mime::Html.limit())
        .ok_or_else(invalid)
}

/// Length of `STANDARD.encode` output for `len` input bytes: `4 * ceil(len / 3)`.
fn base64_len(len: usize) -> Result<usize, ProtocolError> {
    len.checked_add(2)
        .map(|n| n / 3)
        .and_then(|groups| groups.checked_mul(4))
        .ok_or_else(invalid)
}

const DATA_URI_PREFIX: &[u8] = b"data:image/png;base64,";

/// Conservative dependency inventory for restoration, not an HTML sanitizer.
/// Bindings cover only the precise img/src value they replace; neighboring
/// srcset, CSS, objects and provider references cannot borrow that proof.
pub fn restore_dependencies_complete(
    bundle: &super::PortableBundle,
    index: usize,
) -> Result<bool, ProtocolError> {
    let bindings = bundle
        .bindings
        .iter()
        .filter(|b| b.representation as usize == index)
        .cloned()
        .collect::<Vec<_>>();
    let html = std::str::from_utf8(&bundle.representations.get(index).ok_or_else(invalid)?.bytes)
        .map_err(|_| invalid())?;
    let inventory = inventory(html)?;
    if !reference_inventory_complete(html, &bindings, &inventory)? {
        return Ok(false);
    }
    // Decode source-inline images one at a time. Bound resource decoding belongs
    // to validate_for_publish: do not materialize and decode them a second time.
    // Count source-inline pixels together with all declared resource workspaces.
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let mut pixels = bundle.validate_with_metrics()?.rgba_bytes;
    for a in inventory.attributes {
        if a.tag != "img"
            || a.name != "src"
            || bindings
                .iter()
                .any(|b| b.offset as usize == a.start && b.length as usize == a.end - a.start)
        {
            continue;
        }
        let value = &html[a.start..a.end];
        let png = STANDARD.decode(&value[22..]).map_err(|_| invalid())?;
        pixels = pixels
            .checked_add(super::png_rgba_size(&png)?)
            .ok_or_else(invalid)?;
        if pixels > super::RGBA_LIMIT {
            return Ok(false);
        }
        if super::png_to_packed(&png).is_err() {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Read-only eligibility inventory, shared with history menus. It neither
/// decodes PNG pixels nor prepares a native write; actual publication remains
/// subject to the stricter preparation gate.
pub fn restore_reference_inventory_complete(
    bytes: &[u8],
    bindings: &[super::ResourceBinding],
) -> Result<bool, ProtocolError> {
    let html = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    reference_inventory_complete(html, bindings, &inventory(html)?)
}
/// Whether every primary img/src is locally represented. Download feedback
/// concerns these images; srcset, CSS and decorative SVG dependencies still
/// keep the full original document's restore completeness conservative.
pub fn image_reference_inventory_complete(
    bytes: &[u8],
    bindings: &[super::ResourceBinding],
) -> Result<bool, ProtocolError> {
    let html = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    let mut images = inventory(html)?;
    images
        .attributes
        .retain(|attribute| attribute.tag == "img" && attribute.name == "src");
    images.unsupported_dependencies = false;
    reference_inventory_complete(html, bindings, &images)
}

fn reference_inventory_complete(
    html: &str,
    bindings: &[super::ResourceBinding],
    inventory: &HtmlInventory,
) -> Result<bool, ProtocolError> {
    if inventory.unsupported_dependencies || inventory.missing_image_source {
        return Ok(false);
    }
    let mut inline_pixels = 0usize;
    let mut inline_count = 0usize;
    let bound_count = bindings
        .iter()
        .map(|b| b.resource)
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    for a in &inventory.attributes {
        let value = &html[a.start..a.end];
        if a.tag == "img" && a.name == "src" {
            let bound = bindings.iter().any(|b| {
                b.offset as usize == a.start
                    && b.length as usize == a.end - a.start
                    && reference_matches(value.as_bytes(), b.resource)
            });
            if !bound
                && !value
                    .to_ascii_lowercase()
                    .starts_with("data:image/png;base64,")
            {
                return Ok(false);
            }
            if !bound {
                use base64::{engine::general_purpose::STANDARD, Engine as _};
                let Some(png) = STANDARD.decode(&value[22..]).ok() else {
                    return Ok(false);
                };
                let Ok(pixels) = super::png_rgba_size(&png) else {
                    return Ok(false);
                };
                inline_pixels = inline_pixels.checked_add(pixels).ok_or_else(invalid)?;
                inline_count += 1;
                if inline_pixels > super::RGBA_LIMIT
                    || inline_count.saturating_add(bound_count) > super::RESOURCE_LIMIT
                {
                    return Ok(false);
                }
            }
        } else if external_attribute(a, value) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Whether `reference` is exactly the canonical reference to `resource`.
///
/// Allocation-free, matching the check `validate` performs.
fn reference_matches(reference: &[u8], resource: u16) -> bool {
    let prefix = super::RESOURCE_REFERENCE_PREFIX;
    let mut expected = [0_u8; 5];
    let expected_len = super::decimal_digits(resource as u32, &mut expected);
    reference.len() == prefix.len() + expected_len
        && reference.starts_with(prefix)
        && reference[prefix.len()..] == expected[..expected_len]
}

pub fn materialize(bundle: &super::PortableBundle, index: usize) -> Result<Vec<u8>, ProtocolError> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    bundle.validate()?;
    let html = bundle
        .representations
        .get(index)
        .filter(|r| r.mime == super::Mime::Html)
        .ok_or_else(invalid)?;
    // R2-02: size the result before allocating anything.
    let total = materialized_len(bundle, index)?;
    let mut bindings = bundle
        .bindings
        .iter()
        .filter(|b| b.representation as usize == index)
        .collect::<Vec<_>>();
    bindings.sort_by_key(|b| b.offset);
    let mut bytes = Vec::with_capacity(total);
    let mut offset = 0;
    for binding in bindings {
        let start = binding.offset as usize;
        let end = start + binding.length as usize;
        if !reference_matches(&html.bytes[start..end], binding.resource) {
            return Err(invalid());
        }
        let png = &bundle
            .resources
            .iter()
            .find(|r| r.id == binding.resource)
            .ok_or_else(invalid)?
            .png;
        bytes.extend_from_slice(&html.bytes[offset..start]);
        bytes.extend_from_slice(DATA_URI_PREFIX);
        bytes.extend_from_slice(STANDARD.encode(png).as_bytes());
        offset = end;
    }
    bytes.extend_from_slice(&html.bytes[offset..]);
    debug_assert_eq!(bytes.len(), total, "the pre-computed length must be exact");
    Ok(bytes)
}
/// Search/preview projection only; the original HTML and resources stay intact.
pub fn text_summary(bytes: &[u8]) -> String {
    let Ok(html) = std::str::from_utf8(bytes) else {
        return String::new();
    };
    let mut result = String::new();
    let mut in_tag = false;
    let mut raw = false;
    let mut tag = String::new();
    for c in html.chars() {
        if c == '<' {
            in_tag = true;
            tag.clear();
            continue;
        }
        if in_tag {
            if c == '>' {
                in_tag = false;
                let name = tag
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if matches!(name.as_str(), "script" | "style") {
                    raw = true;
                }
                if matches!(name.as_str(), "/script" | "/style") {
                    raw = false;
                }
                if matches!(name.as_str(), "br" | "/p" | "/div" | "/li" | "/tr") {
                    result.push('\n');
                }
            } else if tag.len() < 256 {
                tag.push(c);
            }
        } else if !raw {
            result.push(c);
        }
        if result.len() > super::Mime::Text.limit() {
            break;
        }
    }
    let mut result = result
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    let mut end = result.len().min(super::Mime::Text.limit());
    while !result.is_char_boundary(end) {
        end -= 1;
    }
    result.truncate(end);
    result
}
#[cfg(test)]
mod resource_tests {
    use super::*;
    use crate::composite::*;
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    #[test]
    fn dependency_inventory_ignores_comments_and_checks_each_image() {
        let png = STANDARD.encode(packed_to_png(1, 1, &[255, 0, 0, 128]).unwrap());
        let image = format!("<img src='data:image/png;base64,{png}'>");
        for (html, expected) in [
            (format!("{image}<img>"), Completeness::MissingFormats),
            (
                format!("<img alt='missing'>{image}"),
                Completeness::MissingFormats,
            ),
            (
                "<!-- <img><script src='x'> --><p>text</p>".into(),
                Completeness::Complete,
            ),
            (format!("<!-- <script> -->{image}"), Completeness::Complete),
            (
                "<style>.x{color:red}</style><p>url(plain text)</p>".into(),
                Completeness::Complete,
            ),
            (
                "<style>.x{background:URL(x)}</style>".into(),
                Completeness::ExternalReferences,
            ),
            ("<iframe></iframe>".into(), Completeness::ExternalReferences),
        ] {
            let mut bundle = PortableBundle {
                representations: vec![Representation {
                    mime: Mime::Html,
                    bytes: html.as_bytes().to_vec(),
                }],
                resources: vec![],
                bindings: vec![],
                completeness: Completeness::Complete,
            };
            extract_resources(&mut bundle).unwrap();
            assert_eq!(bundle.completeness, expected, "capture: {html}");
            // Check the actual inventory even if a sender labels it Complete.
            bundle.completeness = Completeness::Complete;
            assert_eq!(
                restore_dependencies_complete(&bundle, 0).unwrap(),
                expected == Completeness::Complete,
                "restore: {html}"
            );
        }
    }
    #[test]
    fn two_images_bound_and_restored_offline_with_alpha() {
        let p1 = packed_to_png(1, 1, &[255, 0, 0, 128]).unwrap();
        let p2 = packed_to_png(1, 1, &[0, 255, 0, 0]).unwrap();
        let html=format!("<p>中文 🙂</p><IMG src='data:image/png;base64,{}'><img src=\"data:image/png;base64,{}\">",STANDARD.encode(&p1),STANDARD.encode(&p2));
        let mut bundle = PortableBundle {
            representations: vec![Representation {
                mime: Mime::Html,
                bytes: html.into_bytes(),
            }],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::Complete,
        };
        extract_resources(&mut bundle).unwrap();
        validate_for_publish(&bundle).unwrap();
        assert_eq!(bundle.resources.len(), 2);
        assert_eq!(bundle.bindings.len(), 2);
        assert_eq!(bundle.completeness, Completeness::Complete);
        let restored = materialize(&bundle, 0).unwrap();
        assert!(String::from_utf8(restored.clone())
            .unwrap()
            .contains("中文 🙂"));
        let mut roundtrip = PortableBundle {
            representations: vec![Representation {
                mime: Mime::Html,
                bytes: restored,
            }],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::Complete,
        };
        extract_resources(&mut roundtrip).unwrap();
        assert_eq!(bundle, roundtrip);
        assert_eq!(
            png_to_packed(&bundle.resources[0].png).unwrap()[8..],
            [255, 0, 0, 128]
        );
    }
    #[test]
    fn references_comments_and_raw_text_do_not_invent_resources() {
        let mut bundle=PortableBundle{representations:vec![Representation{mime:Mime::Html,bytes:b"<!-- <img src='x'> --><script>const s='<img src=y>';</script><img src='https://example.invalid/a.png'><p>visible</p>".to_vec()}],resources:vec![],bindings:vec![],completeness:Completeness::Complete};
        extract_resources(&mut bundle).unwrap();
        assert_eq!(bundle.completeness, Completeness::ExternalReferences);
        assert!(bundle.resources.is_empty());
        assert!(text_summary(&bundle.representations[0].bytes).contains("visible"));
        assert!(!text_summary(&bundle.representations[0].bytes).contains("const"));
    }
}

#[cfg(test)]
mod image_archive_feedback_tests {
    use super::*;
    use crate::composite::ResourceBinding;

    #[test]
    fn saved_primary_image_is_complete_without_certifying_other_web_dependencies() {
        let html = br##"<link rel="stylesheet" href="https://fixture.example/site.css"><img src="tailsync-resource:1" srcset="https://fixture.example/large.png 2x"><svg><use href="#icon"></use></svg>"##;
        let offset = std::str::from_utf8(html)
            .unwrap()
            .find("tailsync-resource:1")
            .unwrap();
        let bindings = [ResourceBinding {
            representation: 0,
            resource: 1,
            offset: offset as u32,
            length: 19,
        }];
        assert!(image_reference_inventory_complete(html, &bindings).unwrap());
        assert!(!restore_reference_inventory_complete(html, &bindings).unwrap());
    }

    #[test]
    fn missing_ambiguous_and_invalid_primary_images_do_not_report_success() {
        for html in [
            "<img>",
            "<img src='https://fixture.example/missing.png' srcset='https://fixture.example/large.png 2x'>",
            "<img src='tailsync-resource:1'>",
            "<img src='data:image/png;base64,AAAA'>",
            "<img src='https://fixture.example/a.png' src='https://fixture.example/b.png'>",
            "<img src='https://fixture.example/a.png'",
        ] {
            assert!(!matches!(image_reference_inventory_complete(html.as_bytes(), &[]), Ok(true)), "{html}");
        }
    }
}
