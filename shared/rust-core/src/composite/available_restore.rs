use super::{Completeness, Mime, PortableBundle, Representation};
use crate::protocol::ProtocolError;
use std::collections::BTreeSet;

pub struct AvailableContent {
    pub bundle: PortableBundle,
    pub missing_images: u16,
}

/// Count the picture references in an HTML flavor that renders no visible text.
/// An app's HTML copy of a picture is such a wrapper: its markup carries the
/// picture's placement and metadata, never a document body. `None` means the
/// HTML is not a wrapper at all.
pub(crate) fn textless_wrapper_images(
    bundle: &PortableBundle,
) -> Result<Option<usize>, ProtocolError> {
    let Some(html_index) = bundle
        .representations
        .iter()
        .position(|r| r.mime == Mime::Html)
    else {
        return Ok(None);
    };
    let html = &bundle.representations[html_index].bytes;
    let inventory = super::html::reference_inventory(html)?;
    if inventory.unsupported_dependencies || !super::html::text_summary(html).trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(inventory.images.len()))
}

/// The standalone pixels of a picture an HTML flavor merely wraps. A browser
/// inlines its own rendition of that picture at whatever size the page used, so
/// the standalone flavor is the same picture at full resolution.
pub(crate) fn wrapped_picture_png(bundle: &PortableBundle) -> Result<Option<usize>, ProtocolError> {
    if bundle.completeness == Completeness::MissingFormats {
        return Ok(None);
    }
    let Some(png) = bundle
        .representations
        .iter()
        .position(|r| r.mime == Mime::Png)
    else {
        return Ok(None);
    };
    if bundle.representation(Mime::Rtf).is_some()
        || bundle
            .representation(Mime::Text)
            .is_some_and(|text| !text.iter().all(u8::is_ascii_whitespace))
        || textless_wrapper_images(bundle)? != Some(1)
    {
        return Ok(None);
    }
    let html_index = bundle
        .representations
        .iter()
        .position(|r| r.mime == Mime::Html)
        .ok_or(ProtocolError::InvalidComposite)?;
    let html = &bundle.representations[html_index].bytes;
    let image = &super::html::reference_inventory(html)?.images[0];
    let standalone = bundle.representations[png].bytes.as_slice();
    let Some((width, height)) = dimensions(standalone) else {
        return Ok(None);
    };
    // A saved wrapper image is the same picture only when it keeps that shape.
    let bound = bundle.bindings.iter().find(|binding| {
        binding.representation as usize == html_index && binding.offset as usize == image.start
    });
    if let Some(resource) = bound.and_then(|binding| {
        bundle
            .resources
            .iter()
            .find(|resource| resource.id == binding.resource)
    }) {
        let matches =
            dimensions(&resource.png).is_some_and(|(w, h)| same_shape(width, height, w, h));
        return Ok(matches.then_some(png));
    }
    Ok(Some(png))
}

/// A source page inlines its own rendition of a picture, so a copy inside a
/// wrapper keeps that picture's shape and is never bigger than the standalone
/// flavor. That keeps a downscaled copy a flavor and a different picture content.
pub(crate) fn same_shape(width: u32, height: u32, other_width: u32, other_height: u32) -> bool {
    if width == 0 || height == 0 || other_width == 0 || other_height == 0 {
        return false;
    }
    if other_width > width || other_height > height {
        return false;
    }
    let left = u64::from(width) * u64::from(other_height);
    let right = u64::from(other_width) * u64::from(height);
    left.abs_diff(right) <= u64::from(width.max(height))
}

fn dimensions(png: &[u8]) -> Option<(u32, u32)> {
    let metadata = crate::protocol::image::metadata("public.png", png)?;
    (metadata.width > 0 && metadata.height > 0).then_some((metadata.width, metadata.height))
}

/// Select saved image bytes as an alternative; never bind them to an unrelated HTML node.
/// The caller has already decoded/validated the portable bundle at its input boundary.
pub(crate) fn image_only_png_alternative(
    bundle: &PortableBundle,
) -> Result<Option<usize>, ProtocolError> {
    let Some(png) = wrapped_picture_png(bundle)? else {
        return Ok(None);
    };
    let html_index = bundle
        .representations
        .iter()
        .position(|r| r.mime == Mime::Html)
        .ok_or(ProtocolError::InvalidComposite)?;
    let html = &bundle.representations[html_index].bytes;
    let inventory = super::html::reference_inventory(html)?;
    let image = &inventory.images[0];
    let already_saved = bundle.bindings.iter().any(|binding| {
        binding.representation as usize == html_index && binding.offset as usize == image.start
    }) || html[image.start..image.end]
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"data:"));
    Ok((!already_saved).then_some(png))
}

pub fn prepare(bundle: &PortableBundle) -> Result<AvailableContent, ProtocolError> {
    bundle.validate()?;
    if let Some(index) = image_only_png_alternative(bundle)? {
        return Ok(AvailableContent {
            bundle: PortableBundle {
                representations: vec![bundle.representations[index].clone()],
                resources: vec![],
                bindings: vec![],
                completeness: Completeness::Complete,
            },
            missing_images: 1,
        });
    }
    let invalid = || ProtocolError::InvalidComposite;
    let index = bundle
        .representations
        .iter()
        .position(|representation| representation.mime == Mime::Html)
        .ok_or_else(invalid)?;
    let original_bindings = bundle
        .bindings
        .iter()
        .filter(|binding| binding.representation as usize == index)
        .cloned()
        .collect::<Vec<_>>();
    let (original, contextual_bindings) = super::html::prepare_available_html_context(
        &bundle.representations[index].bytes,
        &original_bindings,
    )?;
    let images = super::html::available_image_inventory(&original)?;
    let mut output = Vec::with_capacity(original.len());
    let mut bindings = Vec::new();
    let mut used_resources = BTreeSet::new();
    let mut missing_images = 0_u16;
    let mut saved_images = 0_usize;
    let mut offset = 0;
    for image in images {
        output.extend_from_slice(&original[offset..image.start]);
        let binding = image.source.and_then(|source| {
            contextual_bindings.iter().find(|binding| {
                binding.representation as usize == index
                    && binding.offset as usize == source.start
                    && binding.length as usize == source.end - source.start
            })
        });
        let inline = image.source.is_some_and(|source| {
            original[source.start..source.end].starts_with(b"data:image/png;base64,")
        });
        if binding.is_some() || inline {
            if let Some(binding) = binding {
                let mut adjusted = binding.clone();
                adjusted.representation = 1;
                adjusted.offset =
                    u32::try_from(output.len() + binding.offset as usize - image.start)
                        .map_err(|_| invalid())?;
                used_resources.insert(binding.resource);
                bindings.push(adjusted);
            }
            output.extend_from_slice(&original[image.start..image.end]);
            saved_images += 1;
        } else {
            output.extend_from_slice(b"<span>[\xf0\x9f\x96\xbc]</span>");
            missing_images = missing_images.checked_add(1).ok_or_else(invalid)?;
        }
        offset = image.end;
    }
    output.extend_from_slice(&original[offset..]);
    if missing_images == 0 || output.len() > Mime::Html.limit() {
        return Err(invalid());
    }
    let text = super::html::text_summary(&original);
    if text.trim().is_empty() && saved_images == 0 {
        return Err(invalid());
    }
    let projected = PortableBundle {
        representations: vec![
            Representation {
                mime: Mime::Text,
                bytes: super::html::text_summary(&output).into_bytes(),
            },
            Representation {
                mime: Mime::Html,
                bytes: output,
            },
        ],
        resources: bundle
            .resources
            .iter()
            .filter(|resource| used_resources.contains(&resource.id))
            .cloned()
            .collect(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composite::{Resource, ResourceBinding};

    fn bundle(html: &str) -> PortableBundle {
        PortableBundle {
            representations: vec![Representation {
                mime: Mime::Html,
                bytes: html.as_bytes().to_vec(),
            }],
            resources: Vec::new(),
            bindings: Vec::new(),
            completeness: Completeness::ExternalReferences,
        }
    }

    #[test]
    fn saved_images_remain_bound_in_order_after_missing_nodes_are_replaced() {
        let html = "<p style='font-family:serif;font-size:18px'><b>saved</b></p><img src='https://example.com/missing'><img src='tailsync-resource:1'><img alt='missing'><img src='tailsync-resource:1'>";
        let mut original = bundle(html);
        original.resources.push(Resource {
            id: 1,
            png: crate::composite::packed_to_png(1, 1, &[255, 0, 0, 255]).unwrap(),
        });
        for (offset, reference) in html.match_indices("tailsync-resource:1") {
            original.bindings.push(ResourceBinding {
                representation: 0,
                resource: 1,
                offset: offset as u32,
                length: reference.len() as u32,
            });
        }
        let original_bytes = original.encode().unwrap();
        let available = prepare(&original).unwrap();
        assert_eq!(available.missing_images, 2);
        assert_eq!(available.bundle.resources.len(), 1);
        assert_eq!(available.bundle.bindings.len(), 2);
        assert_eq!(original.encode().unwrap(), original_bytes);
        let projected = available.bundle.representation(Mime::Html).unwrap();
        let projected = std::str::from_utf8(projected).unwrap();
        assert!(projected.contains("font-family:serif;font-size:18px"));
        let positions: Vec<_> = projected
            .match_indices("[🖼]")
            .map(|(offset, _)| offset)
            .collect();
        let images: Vec<_> = projected
            .match_indices("tailsync-resource:1")
            .map(|(offset, _)| offset)
            .collect();
        assert!(positions[0] < images[0] && images[0] < positions[1] && positions[1] < images[1]);
        assert!(crate::composite::restore::dependencies_complete(&available.bundle).unwrap());
    }

    #[test]
    fn empty_corrupt_or_unclosed_content_is_not_reported_as_available() {
        for html in [
            "<img src='https://example.com/missing'>",
            "<p>saved</p><img src='first' src='second'>",
            "<p>saved</p><img src='unterminated>",
            "<p>saved</p><script>active()</script><img src='missing'>",
            "<p style='background-image:url(https://example.com/bg)'>saved</p><img src='missing'>",
            "<p>saved</p><img src='missing' srcset='other 2x'>",
            "<p>saved</p><img src='data:image/png;base64,corrupt'><img src='missing'>",
        ] {
            assert!(
                prepare(&bundle(html)).is_err(),
                "unexpected projection: {html}"
            );
        }
    }

    #[test]
    fn comments_do_not_create_missing_images_or_change_body_whitespace() {
        let original =
            bundle("<!-- <img src='comment'> --><pre>one\n  two</pre><img src='missing'>");
        let available = prepare(&original).unwrap();
        assert_eq!(available.missing_images, 1);
        let html =
            std::str::from_utf8(available.bundle.representation(Mime::Html).unwrap()).unwrap();
        assert!(html.contains("<pre>one\n  two</pre>"));
    }

    #[test]
    fn browser_source_context_preserves_relative_links_and_saved_image_bindings() {
        let html = "<base href='https://example.com/posts/'><p><a href='../other?x=1&amp;y=2'><b>saved</b></a></p><img src='missing'><img src='tailsync-resource:1'>";
        let mut original = bundle(html);
        original.resources.push(Resource {
            id: 1,
            png: crate::composite::packed_to_png(1, 1, &[255, 0, 0, 255]).unwrap(),
        });
        original.bindings.push(ResourceBinding {
            representation: 0,
            resource: 1,
            offset: html.find("tailsync-resource:1").unwrap() as u32,
            length: "tailsync-resource:1".len() as u32,
        });
        let before = original.encode().unwrap();
        let available = prepare(&original).unwrap();
        assert_eq!(available.missing_images, 1);
        let restored =
            std::str::from_utf8(available.bundle.representation(Mime::Html).unwrap()).unwrap();
        assert!(!restored.contains("<base"));
        assert!(restored.contains("href='https://example.com/other?x=1&amp;y=2'"));
        assert!(restored.contains("<b>saved</b>"));
        let binding = &available.bundle.bindings[0];
        assert_eq!(
            binding.offset as usize,
            restored.find("tailsync-resource:1").unwrap()
        );
        assert_eq!(original.encode().unwrap(), before);
        assert!(crate::composite::restore::dependencies_complete(&available.bundle).unwrap());
    }

    #[test]
    fn cf_html_source_url_does_not_block_available_body_restoration() {
        let document = "<html><body><p>saved body</p><img src='missing'></body></html>";
        let template = "Version:1.0\r\nStartHTML:0000000000\r\nEndHTML:0000000000\r\nStartFragment:0000000000\r\nEndFragment:0000000000\r\nSourceURL:https://example.com/post/1\r\n";
        let start = template.len();
        let end = start + document.len();
        let fragment_start = start + document.find("<p>").unwrap();
        let fragment_end = start + document.find("</body>").unwrap();
        let header = format!("Version:1.0\r\nStartHTML:{start:010}\r\nEndHTML:{end:010}\r\nStartFragment:{fragment_start:010}\r\nEndFragment:{fragment_end:010}\r\nSourceURL:https://example.com/post/1\r\n");
        let encoded = [header.as_bytes(), document.as_bytes()].concat();
        let decoded = crate::composite::html::decode_cf_html(&encoded).unwrap();
        let original = bundle(&decoded);
        let available = prepare(&original).unwrap();
        assert_eq!(available.missing_images, 1);
        assert!(
            std::str::from_utf8(available.bundle.representation(Mime::Html).unwrap())
                .unwrap()
                .contains("<p>saved body</p>")
        );
    }

    #[test]
    fn ambiguous_or_active_source_context_still_fails_closed() {
        for html in [
            "<base href='https://example.com/'><base href='https://other.example/'><p>saved</p><img src='missing'>",
            "<base href='javascript:active()'><p>saved</p><img src='missing'>",
            "<base href='https://user:password@example.com/'><p>saved</p><img src='missing'>",
            "<base href='https://example.com/' href='https://other.example/'><p>saved</p><img src='missing'>",
            "<base href href='https://example.com/'><p>saved</p><img src='missing'>",
            "<base href='https://example.com/'><a href='first' href='second'>saved</a><img src='missing'>",
            "<base href='https://example.com/'><a href href='second'>saved</a><img src='missing'>",
            "<base href='https://example.com/'><script>active()</script><p>saved</p><img src='missing'>",
            "<base href='https://example.com/'><p style='background:url(relative)'>saved</p><img src='missing'>",
        ] {
            assert!(prepare(&bundle(html)).is_err(), "unexpected projection: {html}");
        }
    }

    #[test]
    fn valueless_anchor_does_not_gain_a_fabricated_url() {
        let html =
            "<base href='https://example.com/post/'><p><a href>saved</a></p><img src='missing'>";
        let available = prepare(&bundle(html)).unwrap();
        let restored =
            std::str::from_utf8(available.bundle.representation(Mime::Html).unwrap()).unwrap();
        assert!(restored.contains("<a href>saved</a>"));
        assert!(!restored.contains("example.com"));
    }
}
