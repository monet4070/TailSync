//! Bounded container metadata for retained originals; never a decoding guarantee.
//! Metadata is recomputed from encrypted archive bytes. Retention, enumeration
//! and static cover decoding have independent limits; unknown metadata does not
//! prevent byte retention or native replay.

use super::native_policy;

/// Maximum number of container frames enumerated without decoding.
const FRAME_LIMIT: u32 = 200;
/// Static cover decoding keeps the helper's generated 20-frame/8-Mpixel budget.
pub const COVER_FRAME_LIMIT: u32 = native_policy::COVER_FRAME_LIMIT;
pub const PIXEL_LIMIT: u64 = native_policy::COVER_PIXEL_LIMIT;
const SCAN_LIMIT: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageKind {
    Png,
    Apng,
    Gif,
    Jpeg,
    Tiff,
    WebP,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageMetadata {
    pub kind: ImageKind,
    pub mime: &'static str,
    /// First page dimensions for TIFF; canvas dimensions for animations.
    pub width: u32,
    pub height: u32,
    pub frames: u32,
    pub animation: bool,
    pub declared_mime: Option<&'static str>,
    /// TIFF sums actual page dimensions; animations conservatively use canvas.
    pub cumulative_pixels: u64,
}
impl ImageMetadata {
    pub fn within_budget(&self) -> bool {
        self.frames <= COVER_FRAME_LIMIT && self.cumulative_pixels <= PIXEL_LIMIT
    }
    pub fn is_static_png(&self) -> bool {
        self.kind == ImageKind::Png
    }
}
fn mime_for_format_name(name: &str) -> Option<&'static str> {
    if let Some((_, mime)) = native_policy::IMAGE_FORMATS
        .iter()
        .find(|(candidate, _)| *candidate == name)
    {
        return Some(*mime);
    }
    Some(match name {
        "public.jpg" | "JPEG" => "image/jpeg",
        "PNG" => "image/png",
        "TIFF" => "image/tiff",
        "GIF" => "image/gif",
        "WEBP" => "image/webp",
        _ => return None,
    })
}
pub fn is_image_format_name(name: &str) -> bool {
    mime_for_format_name(name).is_some()
}
pub fn metadata(name: &str, bytes: &[u8]) -> Option<ImageMetadata> {
    let mut m = probe(bytes)?;
    m.declared_mime = mime_for_format_name(name);
    Some(m)
}
pub fn probe(bytes: &[u8]) -> Option<ImageMetadata> {
    png(bytes)
        .or_else(|| gif(bytes))
        .or_else(|| jpeg(bytes))
        .or_else(|| tiff(bytes))
        .or_else(|| webp(bytes))
}
pub fn is_static_png(bytes: &[u8]) -> bool {
    probe(bytes).is_some_and(|m| m.is_static_png())
}
fn meta(kind: ImageKind, width: u32, height: u32, frames: u32) -> Option<ImageMetadata> {
    if width == 0 || height == 0 || frames == 0 || frames > FRAME_LIMIT {
        return None;
    }
    let mime = match kind {
        ImageKind::Png => "image/png",
        ImageKind::Apng => "image/apng",
        ImageKind::Gif => "image/gif",
        ImageKind::Jpeg => "image/jpeg",
        ImageKind::Tiff => "image/tiff",
        ImageKind::WebP => "image/webp",
    };
    Some(ImageMetadata {
        kind,
        mime,
        width,
        height,
        frames,
        animation: frames > 1 && matches!(kind, ImageKind::Apng | ImageKind::Gif | ImageKind::WebP),
        declared_mime: None,
        cumulative_pixels: u64::from(width)
            .checked_mul(u64::from(height))?
            .checked_mul(u64::from(frames))?,
    })
}
fn be32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        bytes.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}
fn le32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}
fn le24(bytes: &[u8]) -> Option<u32> {
    let b = bytes.get(..3)?;
    Some(u32::from(b[0]) | u32::from(b[1]) << 8 | u32::from(b[2]) << 16)
}
fn png(bytes: &[u8]) -> Option<ImageMetadata> {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || be32(bytes, 8)? != 13
        || bytes.get(12..16)? != b"IHDR"
    {
        return None;
    }
    let (w, h) = (be32(bytes, 16)?, be32(bytes, 20)?);
    let mut at = 8usize;
    let mut frames = None;
    while at < SCAN_LIMIT {
        let size = be32(bytes, at)? as usize;
        let kind = bytes.get(at + 4..at + 8)?;
        let end = at.checked_add(12)?.checked_add(size)?;
        if end > bytes.len() {
            return None;
        }
        if kind == b"acTL" {
            if size != 8 || frames.is_some() {
                return None;
            }
            frames = Some(be32(bytes, at + 8)?);
        }
        // acTL must precede IDAT. Do not decode or scan a large pixel payload.
        if kind == b"IDAT" {
            return meta(
                if frames.is_some() {
                    ImageKind::Apng
                } else {
                    ImageKind::Png
                },
                w,
                h,
                frames.unwrap_or(1),
            );
        }
        if kind == b"IEND" {
            return None;
        }
        at = end;
    }
    None
}
fn gif(bytes: &[u8]) -> Option<ImageMetadata> {
    if !(bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")) {
        return None;
    }
    let end = bytes.len().min(SCAN_LIMIT);
    let b = bytes.get(..13)?;
    let w = u32::from(u16::from_le_bytes([b[6], b[7]]));
    let h = u32::from(u16::from_le_bytes([b[8], b[9]]));
    let mut at = 13
        + if b[10] & 0x80 != 0 {
            3usize << ((b[10] & 7) + 1)
        } else {
            0
        };
    let mut frames = 0;
    while at < end {
        match bytes[at] {
            0x2c => {
                let d = bytes.get(at..at.checked_add(10)?)?;
                let left = u32::from(u16::from_le_bytes([d[1], d[2]]));
                let top = u32::from(u16::from_le_bytes([d[3], d[4]]));
                let fw = u32::from(u16::from_le_bytes([d[5], d[6]]));
                let fh = u32::from(u16::from_le_bytes([d[7], d[8]]));
                if fw == 0 || fh == 0 || left + fw > w || top + fh > h {
                    return None;
                }
                at += 10
                    + if d[9] & 0x80 != 0 {
                        3usize << ((d[9] & 7) + 1)
                    } else {
                        0
                    };
                if !(2..=8).contains(bytes.get(at)?) || *bytes.get(at + 1)? == 0 {
                    return None;
                }
                at = skip_blocks(bytes, at + 1, end)?;
                frames += 1;
                if frames > FRAME_LIMIT {
                    return None;
                }
            }
            0x21 => {
                bytes.get(at + 1)?;
                at = skip_blocks(bytes, at + 2, end)?;
            }
            0x3b => return meta(ImageKind::Gif, w, h, frames),
            _ => return None,
        }
    }
    None // No trailer or exhausted scan window: frame count is unknown.
}
fn skip_blocks(bytes: &[u8], mut at: usize, end: usize) -> Option<usize> {
    loop {
        if at >= end {
            return None;
        }
        let n = usize::from(*bytes.get(at)?);
        at = at.checked_add(1 + n)?;
        if at > end {
            return None;
        }
        if n == 0 {
            return Some(at);
        }
    }
}
fn jpeg(bytes: &[u8]) -> Option<ImageMetadata> {
    if !bytes.starts_with(&[0xff, 0xd8]) {
        return None;
    }
    let end = bytes.len().min(SCAN_LIMIT);
    let mut at = 2usize;
    while at + 4 <= end {
        if bytes[at] != 0xff {
            return None;
        }
        while at < end && bytes[at] == 0xff {
            at += 1;
        }
        let marker = *bytes.get(at)?;
        at += 1;
        if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        if matches!(marker, 0x00 | 0xd9 | 0xda) {
            return None;
        }
        let pair = bytes.get(at..at + 2)?;
        let n = usize::from(u16::from_be_bytes([pair[0], pair[1]]));
        if n < 2 || at.checked_add(n)? > end {
            return None;
        }
        if matches!(marker,0xc0..=0xc3|0xc5..=0xc7|0xc9..=0xcb|0xcd..=0xcf) {
            if n < 8 {
                return None;
            }
            let b = bytes.get(at..at + n)?;
            let components = usize::from(b[7]);
            if components == 0 || n != 8 + components * 3 {
                return None;
            }
            return meta(
                ImageKind::Jpeg,
                u32::from(u16::from_be_bytes([b[5], b[6]])),
                u32::from(u16::from_be_bytes([b[3], b[4]])),
                1,
            );
        }
        at += n;
    }
    None
}
fn tiff(bytes: &[u8]) -> Option<ImageMetadata> {
    let little = match bytes.get(..4)? {
        b"II\x2a\0" => true,
        b"MM\0\x2a" => false,
        _ => return None,
    };
    let read16 = |at: usize| -> Option<u16> {
        let p = bytes.get(at..at.checked_add(2)?)?.try_into().ok()?;
        Some(if little {
            u16::from_le_bytes(p)
        } else {
            u16::from_be_bytes(p)
        })
    };
    let read32 = |at: usize| -> Option<u32> {
        let p = bytes.get(at..at.checked_add(4)?)?.try_into().ok()?;
        Some(if little {
            u32::from_le_bytes(p)
        } else {
            u32::from_be_bytes(p)
        })
    };
    let end = bytes.len().min(SCAN_LIMIT);
    let mut at = read32(4)? as usize;
    let mut seen = std::collections::BTreeSet::new();
    let mut first = None;
    let mut pixels = 0u64;
    while at != 0 {
        if seen.len() >= FRAME_LIMIT as usize || !seen.insert(at) {
            return None;
        }
        let count = usize::from(read16(at)?);
        if count > 4096 || at.checked_add(6 + count * 12)? > end {
            return None;
        }
        let (mut w, mut h) = (None, None);
        for i in 0..count {
            let p = at + 2 + i * 12;
            let tag = read16(p)?;
            if tag != 256 && tag != 257 {
                continue;
            }
            if read32(p + 4)? != 1 {
                return None;
            }
            let value = match read16(p + 2)? {
                3 => u32::from(read16(p + 8)?),
                4 => read32(p + 8)?,
                _ => return None,
            };
            if value == 0 {
                return None;
            }
            let slot = if tag == 256 { &mut w } else { &mut h };
            if slot.replace(value).is_some() {
                return None;
            }
        }
        let (w, h) = (w?, h?);
        first.get_or_insert((w, h));
        pixels = pixels.checked_add(u64::from(w) * u64::from(h))?;
        at = read32(at + 2 + count * 12)? as usize;
    }
    let (w, h) = first?;
    let mut m = meta(ImageKind::Tiff, w, h, seen.len() as u32)?;
    m.cumulative_pixels = pixels;
    Some(m)
}
/// Validate only the image payload header, not the compressed pixels.
fn webp_frame(kind: &[u8], b: &[u8]) -> Option<(u32, u32)> {
    match kind {
        b"VP8 " => {
            if b.len() < 10 || b[0] & 1 != 0 || b.get(3..6)? != [0x9d, 0x01, 0x2a] {
                return None;
            }
            Some((
                u32::from(u16::from_le_bytes([b[6], b[7]])) & 0x3fff,
                u32::from(u16::from_le_bytes([b[8], b[9]])) & 0x3fff,
            ))
        }
        b"VP8L" => {
            if b.len() < 5 || b[0] != 0x2f {
                return None;
            }
            let v = le32(b, 1)?;
            if v >> 29 != 0 {
                return None;
            }
            Some(((v & 0x3fff) + 1, ((v >> 14) & 0x3fff) + 1))
        }
        _ => None,
    }
}
fn webp(bytes: &[u8]) -> Option<ImageMetadata> {
    if bytes.get(..4)? != b"RIFF" || bytes.get(8..12)? != b"WEBP" {
        return None;
    }
    let end = 8usize.checked_add(le32(bytes, 4)? as usize)?;
    if end != bytes.len() || end > SCAN_LIMIT {
        return None;
    }
    let mut at = 12;
    let mut canvas = None;
    let mut animated = false;
    let mut control = false;
    let mut frames = 0;
    while at < end {
        let kind = bytes.get(at..at + 4)?;
        let size = le32(bytes, at + 4)? as usize;
        let body = at + 8;
        let next = body.checked_add(size)?.checked_add(size & 1)?;
        if next > end {
            return None;
        }
        let b = bytes.get(body..body + size)?;
        match kind {
            b"VP8X" => {
                if size != 10 || canvas.is_some() {
                    return None;
                }
                animated = b[0] & 2 != 0;
                canvas = Some((1 + le24(&b[4..])?, 1 + le24(&b[7..])?));
            }
            b"ANIM" => {
                if !animated || size != 6 || control {
                    return None;
                }
                control = true;
            }
            b"ANMF" => {
                if !animated || !control || size < 24 {
                    return None;
                }
                let (w, h) = canvas?;
                let (fw, fh) = (1 + le24(&b[6..])?, 1 + le24(&b[9..])?);
                if 2 * le24(b)? + fw > w || 2 * le24(&b[3..])? + fh > h {
                    return None;
                }
                let mut pos = 16;
                let mut image = false;
                while pos < size {
                    let k = b.get(pos..pos + 4)?;
                    let n = le32(b, pos + 4)? as usize;
                    let e = (pos + 8).checked_add(n)?.checked_add(n & 1)?;
                    if e > size {
                        return None;
                    }
                    if k == b"VP8 " || k == b"VP8L" {
                        if image || webp_frame(k, b.get(pos + 8..pos + 8 + n)?)? != (fw, fh) {
                            return None;
                        }
                        image = true;
                    }
                    pos = e;
                }
                if !image {
                    return None;
                }
                frames += 1;
                if frames > FRAME_LIMIT {
                    return None;
                }
            }
            b"VP8 " | b"VP8L" => {
                if animated || frames != 0 {
                    return None;
                }
                let dims = webp_frame(kind, b)?;
                if canvas.is_some_and(|c| c != dims) {
                    return None;
                }
                canvas = Some(dims);
                frames = 1;
            }
            _ => {}
        }
        at = next;
    }
    let (w, h) = canvas?;
    meta(ImageKind::WebP, w, h, frames)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generated_image_formats_and_windows_aliases_keep_their_mime() {
        assert_eq!(
            crate::native_policy::IMAGE_FORMATS,
            &[
                ("public.png", "image/png"),
                ("public.tiff", "image/tiff"),
                ("public.jpeg", "image/jpeg"),
                ("com.compuserve.gif", "image/gif"),
                ("org.webmproject.webp", "image/webp"),
            ]
        );
        for (name, mime) in crate::native_policy::IMAGE_FORMATS {
            assert_eq!(mime_for_format_name(name), Some(*mime), "{name}");
        }
        for (name, mime) in [
            ("public.jpg", "image/jpeg"),
            ("PNG", "image/png"),
            ("TIFF", "image/tiff"),
            ("JPEG", "image/jpeg"),
            ("GIF", "image/gif"),
            ("WEBP", "image/webp"),
        ] {
            assert_eq!(mime_for_format_name(name), Some(mime), "{name}");
        }
        assert_eq!(mime_for_format_name("com.example.vendor-format"), None);
    }
    #[test]
    fn cover_budget_matches_the_helper_contract() {
        assert_eq!(COVER_FRAME_LIMIT, 20);
        assert_eq!(PIXEL_LIMIT, 8 * 1024 * 1024);
    }
    const PNG: &[u8] = include_bytes!("../tests/fixtures/images/transparent.png");
    const APNG: &[u8] = include_bytes!("../tests/fixtures/images/animated.png");
    const SINGLE_APNG: &[u8] = include_bytes!("../tests/fixtures/images/single-frame-apng.png");
    const GIF: &[u8] = include_bytes!("../tests/fixtures/images/animated.gif");
    const JPEG: &[u8] = include_bytes!("../tests/fixtures/images/color.jpg");
    const TIFF: &[u8] = include_bytes!("../tests/fixtures/images/color.tiff");
    const PAGES: &[u8] = include_bytes!("../tests/fixtures/images/pages.tiff");
    const WEBP: &[u8] = include_bytes!("../tests/fixtures/images/animated.webp");
    const STATIC_WEBP: &[u8] = include_bytes!("../tests/fixtures/images/static.webp");
    #[test]
    fn real_containers_report_original_facts_and_cover_admission() {
        for (b, k, f) in [
            (PNG, ImageKind::Png, 1),
            (APNG, ImageKind::Apng, 2),
            (SINGLE_APNG, ImageKind::Apng, 1),
            (GIF, ImageKind::Gif, 2),
            (JPEG, ImageKind::Jpeg, 1),
            (TIFF, ImageKind::Tiff, 1),
            (WEBP, ImageKind::WebP, 2),
            (STATIC_WEBP, ImageKind::WebP, 1),
        ] {
            let m = probe(b).unwrap();
            assert_eq!((m.kind, m.width, m.height, m.frames), (k, 4, 3, f));
            assert_eq!(m.animation, f > 1);
            assert_eq!(m.is_static_png(), k == ImageKind::Png);
            assert!(m.within_budget());
        }
        let m = probe(PAGES).unwrap();
        assert_eq!((m.frames, m.cumulative_pixels), (2, 12 + 40));
        assert!(!m.animation);
        assert_eq!(
            metadata("public.png", GIF).unwrap().declared_mime,
            Some("image/png")
        );
    }
    #[test]
    fn single_frame_apng_and_truncated_actl_never_become_static_png() {
        assert!(!is_static_png(SINGLE_APNG));
        let mut b = PNG[..33].to_vec();
        b.extend(8u32.to_be_bytes());
        b.extend(b"acTL");
        assert!(probe(&b).is_none());
        let mut b = SINGLE_APNG.to_vec();
        b[41..45].copy_from_slice(&0u32.to_be_bytes());
        assert!(probe(&b).is_none());
        let mut b = SINGLE_APNG.to_vec();
        b[41..45].copy_from_slice(&21u32.to_be_bytes());
        let m = probe(&b).unwrap();
        assert_eq!(m.frames, 21);
        assert!(
            !m.within_budget(),
            "enumeration does not loosen the cover decoder"
        );
    }
    fn directory(w: u32, h: u32, next: u32) -> Vec<u8> {
        let mut b = b"II\x2a\0".to_vec();
        b.extend(8u32.to_le_bytes());
        b.extend(2u16.to_le_bytes());
        for (t, v) in [(256u16, w), (257u16, h)] {
            b.extend(t.to_le_bytes());
            b.extend(4u16.to_le_bytes());
            b.extend(1u32.to_le_bytes());
            b.extend(v.to_le_bytes());
        }
        b.extend(next.to_le_bytes());
        b
    }
    #[test]
    fn tiff_cycles_and_larger_later_pages_cannot_evade_budget() {
        assert!(probe(&directory(1, 1, 8)).is_none());
        let mut b = directory(1, 1, 38);
        b.extend_from_slice(&directory(4096, 4096, 0)[8..]);
        let m = probe(&b).unwrap();
        assert_eq!(m.cumulative_pixels, 1 + 4096 * 4096);
        assert!(!m.within_budget());
        b[14..18].copy_from_slice(&2u32.to_le_bytes());
        assert!(
            probe(&b).is_none(),
            "dimension arrays are not inline scalar values"
        );
    }
    #[test]
    fn truncated_frames_and_invalid_riff_extent_have_unknown_metadata() {
        assert!(probe(&GIF[..GIF.len() - 1]).is_none());
        let mut b = WEBP.to_vec();
        b[4..8].copy_from_slice(&4u32.to_le_bytes());
        assert!(probe(&b).is_none());
        let mut b = b"RIFF".to_vec();
        b.extend(30u32.to_le_bytes());
        b.extend(b"WEBPVP8X");
        b.extend(10u32.to_le_bytes());
        b.extend([2, 0, 0, 0, 3, 0, 0, 2, 0, 0]);
        b.extend(b"ANMF");
        b.extend(0u32.to_le_bytes());
        assert!(
            probe(&b).is_none(),
            "a canvas and empty frame are not animation proof"
        );
    }
    #[test]
    fn every_fixture_prefix_is_safe_to_probe() {
        for b in [
            PNG,
            APNG,
            SINGLE_APNG,
            GIF,
            JPEG,
            TIFF,
            PAGES,
            WEBP,
            STATIC_WEBP,
        ] {
            for n in 0..b.len() {
                let _ = probe(&b[..n]);
            }
        }
        let mut b = APNG.to_vec();
        b[16..24].fill(255);
        assert!(probe(&b).is_none(), "cumulative pixels must not overflow");
    }
}
