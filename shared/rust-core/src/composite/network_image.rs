use super::png_to_packed;
use crate::protocol::PackedImage;
use image::metadata::Orientation;
use image::ImageDecoder as _;
use std::io::{Cursor, Write};

pub const NETWORK_IMAGE_PIXEL_LIMIT: u64 = 8 * 1024 * 1024;
pub const NETWORK_IMAGE_RGBA_LIMIT: usize = 32 * 1024 * 1024;
pub const NETWORK_IMAGE_DERIVED_LIMIT: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkImageFormat {
    Png,
    Jpeg,
    WebP,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkImageError {
    Unsupported,
    Mime,
    Animated,
    Decode,
    TooLarge,
}

#[derive(Debug, Clone, Copy)]
pub struct NetworkImageLimits {
    pub max_pixels: u64,
    pub max_rgba_bytes: usize,
    pub max_derived_bytes: usize,
}

impl Default for NetworkImageLimits {
    fn default() -> Self {
        Self {
            max_pixels: NETWORK_IMAGE_PIXEL_LIMIT,
            max_rgba_bytes: NETWORK_IMAGE_RGBA_LIMIT,
            max_derived_bytes: NETWORK_IMAGE_DERIVED_LIMIT,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivedImage {
    pub png: Vec<u8>,
    pub pixels: u64,
}

pub fn declared_format(content_type: &str) -> Option<NetworkImageFormat> {
    let essence = content_type.split(';').next().unwrap_or("").trim();
    if essence.eq_ignore_ascii_case("image/png") {
        Some(NetworkImageFormat::Png)
    } else if essence.eq_ignore_ascii_case("image/jpeg") {
        Some(NetworkImageFormat::Jpeg)
    } else if essence.eq_ignore_ascii_case("image/webp") {
        Some(NetworkImageFormat::WebP)
    } else {
        None
    }
}

pub fn detect_format(bytes: &[u8]) -> Option<NetworkImageFormat> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some(NetworkImageFormat::Png);
    }
    if bytes.starts_with(b"\xff\xd8\xff") {
        return Some(NetworkImageFormat::Jpeg);
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some(NetworkImageFormat::WebP);
    }
    None
}

pub fn decode_static_png(
    bytes: &[u8],
    declared: NetworkImageFormat,
    limits: &NetworkImageLimits,
) -> Result<DerivedImage, NetworkImageError> {
    let actual = detect_format(bytes).ok_or(NetworkImageError::Unsupported)?;
    if actual != declared {
        return Err(NetworkImageError::Mime);
    }
    let (width, height, rgba) = match actual {
        NetworkImageFormat::Png => decode_png(bytes, limits)?,
        NetworkImageFormat::Jpeg => decode_jpeg(bytes, limits)?,
        NetworkImageFormat::WebP => decode_webp(bytes, limits)?,
    };
    let pixels = u64::from(width) * u64::from(height);
    let png = encode_bounded_png(width, height, &rgba, limits.max_derived_bytes)?;
    Ok(DerivedImage { png, pixels })
}

fn decode_png(
    bytes: &[u8],
    limits: &NetworkImageLimits,
) -> Result<(u32, u32, Vec<u8>), NetworkImageError> {
    let (width, height) = png_header_size(bytes)?;
    check_pixels(width, height, limits)?;
    if png_declares_animation(bytes)? {
        return Err(NetworkImageError::Animated);
    }
    let packed = png_to_packed(bytes).map_err(|_| NetworkImageError::Decode)?;
    let image = PackedImage::try_from(packed.as_slice()).map_err(|_| NetworkImageError::Decode)?;
    if image.width != width || image.height != height {
        return Err(NetworkImageError::Decode);
    }
    Ok((image.width, image.height, image.rgba.to_vec()))
}

fn decode_jpeg(
    bytes: &[u8],
    limits: &NetworkImageLimits,
) -> Result<(u32, u32, Vec<u8>), NetworkImageError> {
    let mut decoder = image::codecs::jpeg::JpegDecoder::new(Cursor::new(bytes))
        .map_err(|_| NetworkImageError::Decode)?;
    let orientation = decoder
        .orientation()
        .map_err(|_| NetworkImageError::Decode)?;
    decode_with_image(decoder, orientation, limits)
}

fn decode_webp(
    bytes: &[u8],
    limits: &NetworkImageLimits,
) -> Result<(u32, u32, Vec<u8>), NetworkImageError> {
    if webp_declares_animation(bytes)? {
        return Err(NetworkImageError::Animated);
    }
    let mut decoder = image::codecs::webp::WebPDecoder::new(Cursor::new(bytes))
        .map_err(|_| NetworkImageError::Decode)?;
    if decoder.has_animation() {
        return Err(NetworkImageError::Animated);
    }
    let orientation = decoder
        .orientation()
        .map_err(|_| NetworkImageError::Decode)?;
    decode_with_image(decoder, orientation, limits)
}

fn decode_with_image<D: image::ImageDecoder>(
    decoder: D,
    orientation: Orientation,
    limits: &NetworkImageLimits,
) -> Result<(u32, u32, Vec<u8>), NetworkImageError> {
    let (width, height) = decoder.dimensions();
    check_pixels(width, height, limits)?;
    let mut dynamic =
        image::DynamicImage::from_decoder(decoder).map_err(|_| NetworkImageError::Decode)?;
    if dynamic.width() != width || dynamic.height() != height {
        return Err(NetworkImageError::Decode);
    }
    dynamic.apply_orientation(orientation);
    let rgba = dynamic.into_rgba8();
    let (width, height) = (rgba.width(), rgba.height());
    check_pixels(width, height, limits)?;
    Ok((width, height, rgba.into_raw()))
}

fn check_pixels(
    width: u32,
    height: u32,
    limits: &NetworkImageLimits,
) -> Result<u64, NetworkImageError> {
    if width == 0 || height == 0 {
        return Err(NetworkImageError::Decode);
    }
    let pixels = u64::from(width) * u64::from(height);
    if pixels > limits.max_pixels {
        return Err(NetworkImageError::TooLarge);
    }
    let rgba = pixels.checked_mul(4).ok_or(NetworkImageError::TooLarge)?;
    if rgba > limits.max_rgba_bytes as u64 {
        return Err(NetworkImageError::TooLarge);
    }
    Ok(pixels)
}

fn png_declares_animation(bytes: &[u8]) -> Result<bool, NetworkImageError> {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(NetworkImageError::Decode);
    }
    let mut offset = 8usize;
    while offset < bytes.len() {
        let header = bytes
            .get(offset..offset + 8)
            .ok_or(NetworkImageError::Decode)?;
        let length = u32::from_be_bytes(
            header[..4]
                .try_into()
                .map_err(|_| NetworkImageError::Decode)?,
        ) as usize;
        let kind = &header[4..8];
        if kind == b"acTL" || kind == b"fcTL" || kind == b"fdAT" {
            return Ok(true);
        }
        let next = offset
            .checked_add(8)
            .and_then(|value| value.checked_add(length))
            .and_then(|value| value.checked_add(4))
            .ok_or(NetworkImageError::Decode)?;
        if next > bytes.len() {
            return Err(NetworkImageError::Decode);
        }
        if kind == b"IEND" {
            return if length == 0 && next == bytes.len() {
                Ok(false)
            } else {
                Err(NetworkImageError::Decode)
            };
        }
        offset = next;
    }
    Err(NetworkImageError::Decode)
}

fn webp_declares_animation(bytes: &[u8]) -> Result<bool, NetworkImageError> {
    if bytes.len() < 12 || !bytes.starts_with(b"RIFF") || &bytes[8..12] != b"WEBP" {
        return Err(NetworkImageError::Decode);
    }
    let declared = u32::from_le_bytes(
        bytes[4..8]
            .try_into()
            .map_err(|_| NetworkImageError::Decode)?,
    ) as usize;
    if declared.checked_add(8) != Some(bytes.len()) {
        return Err(NetworkImageError::Decode);
    }
    let mut offset = 12usize;
    while offset < bytes.len() {
        let header = bytes
            .get(offset..offset + 8)
            .ok_or(NetworkImageError::Decode)?;
        let fourcc = &header[..4];
        let length = u32::from_le_bytes(
            header[4..8]
                .try_into()
                .map_err(|_| NetworkImageError::Decode)?,
        ) as usize;
        let payload = offset + 8;
        let end = payload
            .checked_add(length)
            .ok_or(NetworkImageError::Decode)?;
        let padded = end
            .checked_add(length & 1)
            .ok_or(NetworkImageError::Decode)?;
        if padded > bytes.len() {
            return Err(NetworkImageError::Decode);
        }
        if fourcc == b"VP8X" {
            if length < 10 {
                return Err(NetworkImageError::Decode);
            }
            let flags = *bytes.get(payload).ok_or(NetworkImageError::Decode)?;
            if flags & 0x02 != 0 {
                return Ok(true);
            }
        }
        if fourcc == b"ANIM" {
            return Ok(true);
        }
        if fourcc == b"ANMF" {
            return Ok(true);
        }
        offset = padded;
    }
    Ok(false)
}

fn png_header_size(bytes: &[u8]) -> Result<(u32, u32), NetworkImageError> {
    if bytes.len() < 33
        || !bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || &bytes[8..16] != b"\0\0\0\rIHDR"
    {
        return Err(NetworkImageError::Decode);
    }
    let width = u32::from_be_bytes(
        bytes[16..20]
            .try_into()
            .map_err(|_| NetworkImageError::Decode)?,
    );
    let height = u32::from_be_bytes(
        bytes[20..24]
            .try_into()
            .map_err(|_| NetworkImageError::Decode)?,
    );
    Ok((width, height))
}

struct BoundedWriter {
    bytes: Vec<u8>,
    limit: usize,
    exceeded: bool,
}

impl Write for BoundedWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let Some(required) = self
            .bytes
            .len()
            .checked_add(bytes.len())
            .filter(|size| *size <= self.limit)
        else {
            self.exceeded = true;
            return Err(std::io::Error::other("derived png budget exceeded"));
        };
        if required > self.bytes.capacity() {
            let capacity = self
                .bytes
                .capacity()
                .max(8)
                .saturating_mul(2)
                .max(required)
                .min(self.limit);
            self.bytes
                .try_reserve_exact(capacity - self.bytes.len())
                .map_err(std::io::Error::other)?;
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn encode_bounded_png(
    width: u32,
    height: u32,
    rgba: &[u8],
    max_bytes: usize,
) -> Result<Vec<u8>, NetworkImageError> {
    let mut sink = BoundedWriter {
        bytes: Vec::new(),
        limit: max_bytes,
        exceeded: false,
    };
    let encoded = (|| -> Result<(), png::EncodingError> {
        let mut encoder = png::Encoder::new(&mut sink, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        {
            let mut stream = writer.stream_writer_with_size(4096)?;
            stream.write_all(rgba)?;
            stream.finish()?;
        }
        writer.finish()?;
        Ok(())
    })();
    match encoded {
        Ok(()) => Ok(sink.bytes),
        Err(_) if sink.exceeded => Err(NetworkImageError::TooLarge),
        Err(_) => Err(NetworkImageError::Decode),
    }
}

#[cfg(test)]
pub(crate) mod fixtures {
    use base64::{engine::general_purpose::STANDARD, Engine as _};

    pub(crate) const PNG_1X1: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==";
    pub(crate) const JPEG_1X1: &str = "/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAAgGBgcGBQgHBwcJCQgKDBQNDAsLDBkSEw8UHRofHh0aHBwgJC4nICIsIxwcKDcpLDAxNDQ0Hyc5PTgyPC4zNDL/2wBDAQkJCQwLDBgNDRgyIRwhMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjL/wAARCAABAAEDASIAAhEBAxEB/8QAHwAAAQUBAQEBAQEAAAAAAAAAAAECAwQFBgcICQoL/8QAtRAAAgEDAwIEAwUFBAQAAAF9AQIDAAQRBRIhMUEGE1FhByJxFDKBkaEII0KxwRVS0fAkM2JyggkKFhcYGRolJicoKSo0NTY3ODk6Q0RFRkdISUpTVFVWV1hZWmNkZWZnaGlqc3R1dnd4eXqDhIWGh4iJipKTlJWWl5iZmqKjpKWmp6ipqrKztLW2t7i5usLDxMXGx8jJytLT1NXW19jZ2uHi4+Tl5ufo6erx8vP09fb3+Pn6/8QAHwEAAwEBAQEBAQEBAQAAAAAAAAECAwQFBgcICQoL/8QAtREAAgECBAQDBAcFBAQAAQJ3AAECAxEEBSExBhJBUQdhcRMiMoEIFEKRobHBCSMzUvAVYnLRChYkNOEl8RcYGRomJygpKjU2Nzg5OkNERUZHSElKU1RVVldYWVpjZGVmZ2hpanN0dXZ3eHl6goOEhYaHiImKkpOUlZaXmJmaoqOkpaanqKmqsrO0tba3uLm6wsPExcbHyMnK0tPU1dbX2Nna4uPk5ebn6Onq8vP09fb3+Pn6/9oADAMBAAIRAxEAPwDWooor80PyA//Z";
    pub(crate) const WEBP_LOSSLESS_1X1: &str = "UklGRhwAAABXRUJQVlA4TA8AAAAvAAAAAAcQ0f/+ByKi/wEA";
    pub(crate) const WEBP_LOSSY_1X1: &str = "UklGRjwAAABXRUJQVlA4IDAAAADQAQCdASoBAAEAAgA0JaACdLoB+AADsAD+8EXf/xu3GG/Bd/8jj+Rx/I4/+QyAAAA=";
    pub(crate) const WEBP_ANIMATED: &str = "UklGRoQAAABXRUJQVlA4WAoAAAACAAAAAQAAAQAAQU5JTQYAAAAAAAAAAABBTk1GKAAAAAAAAAAAAAEAAAEAAGQAAAJWUDhMDwAAAC8BQAAABxD9j/4HIqL/AQBBTk1GKAAAAAAAAAAAAAEAAAEAAGQAAABWUDhMDwAAAC8BQAAABxDR//4HIqL/AQA=";
    pub(crate) const WEBP_SINGLE_FRAME_ANIMATION_FLAG: &str =
        "UklGRi4AAABXRUJQVlA4WAoAAAACAAAAAQAAAQAAVlA4TBAAAAAvAUAAAAcQ/Y/+ByKi/wEA";
    pub(crate) const APNG_TWO_FRAME: &str = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAYAAABytg0kAAAACGFjVEwAAAACAAAAAPONk3AAAAAaZmNUTAAAAAAAAAACAAAAAgAAAAAAAAAAAAEACgAA6FTcAAAAABVJREFUeJxj/M/A8J+BgYGBCUSAMAAfFwICAkezFAAAABpmY1RMAAAAAQAAAAIAAAACAAAAAAAAAAAAAQAKAABzJzbUAAAAGWZkQVQAAAACeJxjZGD4/5+BgYGBCUSAMAAdGQICRW6eVwAAAABJRU5ErkJggg==";
    pub(crate) const PNG_SINGLE_FRAME_ANIMATION_FLAG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAACGFjVEwAAAABAAAAALQt6aAAAAANSURBVHicY/jPwPAfAAUAAf+JmT0dAAAAAElFTkSuQmCC";
    pub(crate) const PNG_2X2: &str = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAAEklEQVR4nGNkaPjPwMDAxAAGABEfAYP1piHtAAAAAElFTkSuQmCC";
    pub(crate) const PNG_NOISE_16X16: &str = "iVBORw0KGgoAAAANSUhEUgAAABAAAAAQCAIAAACQkWg2AAADG0lEQVR4nAEQA+/8AKVNyhglMLsdbRMs3tYjey7ZHj9yH8sZcRdElNZJPJ1cNGC+MSAeaf7aoO7ouZl/XAB8KZn9r+WTJTzWVK9N+tcUJ6Cus/7pIy+K8iEfnuSRxbEL7LVWO/web5NCfsvI/ikAVeXNjkbcjtS3wnZNKlpNdncG+F2GkAJK1r2jQBvpyMvMyTX2zR9hImrhUziuGjQAAE0zug0kasBMgbG68j47+e71958rSTSvh/VSC2m5Sw2YLoW7VbZyqHJjes10Zvy2DgAOj/GEY7DksropcDR08GSsaPcA9bArPcZm9FveqizK7c0rUVdBDk3uSvKzT0MKBzQBR95jJTAdAIf7Ou9bnZvfp7iNYzKfEPlr+0am5de0uhd3iCtwltFa+e77mEarQY7xAIRn5UbVPsjioSV72yVsmz5Pu0mBRu9wMMv5U3JS3M6t12S2oy+7Ca3q4QnEqZcgOQF1NSsSVunV/y58+o10edjhkXYwSwgsLwj8IkSdXc3Gfm/thJVB8fY8DOEK4YZAAvUA5FuKsQmAEgcJYfN95Dbd/cmdbnWvZUfPsRtCBySC3FMcK8OQfJYX615QieQBhrqoAKV9EZ5vtl0Aq8Mq845mfwIuhy1JzBXJC5mbdytPx6b9TJFKFttHCHUrDxVEuDXA5wAZCX36hwHpIy8h8oEmh3hpduv8wyf1kxdlJ0upgptEBvYf+Ikyb/qUku3u7jxmnysC2f8X8KDloKM8SjStIv9A2hlOvrPXAzb17NqwPU3/BM+6oam0d5GsGTlQdhhYGx/2AcnHJ+8UZWC0qAIDGGX2kyLZ4DqtDY0bTWq9LcAS069f2JNYn8TT9Tf4FShPo16J6QBq/nDnqubaR2J8LlmvLqN6vIRnCtPE02vAiq0f/464QG4vin/EzOTdnwtBENny+gABJcjvwLdIjdAWxZve3RYVYxNbym1E+LJFYDCi7BA6gCjriQeRZdcCG1ofDjnG48zGAl6s6tj1xZ9eioLgOu+V4vLqy17jl7U/Y/7VN9oA+NCCXzt68RnQVgxMwa3bPTpRKwXbhTUxk+NAAAAAAElFTkSuQmCC";

    pub(crate) fn decode(encoded: &str) -> Vec<u8> {
        STANDARD.decode(encoded).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::super::packed_to_png;
    use super::fixtures::*;
    use super::*;

    fn png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
        packed_to_png(width, height, rgba).unwrap()
    }

    #[test]
    fn declared_content_type_maps_only_the_admitted_formats() {
        assert_eq!(declared_format("image/png"), Some(NetworkImageFormat::Png));
        assert_eq!(
            declared_format("IMAGE/PNG; charset=binary"),
            Some(NetworkImageFormat::Png)
        );
        assert_eq!(
            declared_format(" image/jpeg "),
            Some(NetworkImageFormat::Jpeg)
        );
        assert_eq!(
            declared_format("image/webp"),
            Some(NetworkImageFormat::WebP)
        );
        for refused in ["image/gif", "image/svg+xml", "text/html", "", "image/pngx"] {
            assert_eq!(declared_format(refused), None, "{refused}");
        }
    }

    #[test]
    fn magic_bytes_identify_the_real_container() {
        assert_eq!(
            detect_format(&decode(PNG_1X1)),
            Some(NetworkImageFormat::Png)
        );
        assert_eq!(
            detect_format(&decode(JPEG_1X1)),
            Some(NetworkImageFormat::Jpeg)
        );
        assert_eq!(
            detect_format(&decode(WEBP_LOSSLESS_1X1)),
            Some(NetworkImageFormat::WebP)
        );
        assert_eq!(detect_format(b"not an image"), None);
    }

    #[test]
    fn every_admitted_format_derives_a_static_png() {
        for (declared, encoded) in [
            (NetworkImageFormat::Png, PNG_1X1),
            (NetworkImageFormat::Jpeg, JPEG_1X1),
            (NetworkImageFormat::WebP, WEBP_LOSSLESS_1X1),
            (NetworkImageFormat::WebP, WEBP_LOSSY_1X1),
        ] {
            let derived =
                decode_static_png(&decode(encoded), declared, &NetworkImageLimits::default())
                    .unwrap();
            assert!(
                derived.png.starts_with(b"\x89PNG\r\n\x1a\n"),
                "{declared:?}"
            );
            assert_eq!(derived.pixels, 1);
            assert_eq!(png_to_packed(&derived.png).unwrap().len(), 8 + 4);
        }
    }

    #[test]
    fn a_mismatched_or_unknown_type_is_refused() {
        assert_eq!(
            decode_static_png(
                &decode(PNG_1X1),
                NetworkImageFormat::Jpeg,
                &NetworkImageLimits::default()
            ),
            Err(NetworkImageError::Mime)
        );
        assert_eq!(
            decode_static_png(
                b"not an image",
                NetworkImageFormat::Png,
                &NetworkImageLimits::default()
            ),
            Err(NetworkImageError::Unsupported)
        );
    }

    #[test]
    fn animation_is_refused_even_with_a_single_frame() {
        for (declared, encoded) in [
            (NetworkImageFormat::Png, APNG_TWO_FRAME),
            (NetworkImageFormat::Png, PNG_SINGLE_FRAME_ANIMATION_FLAG),
            (NetworkImageFormat::WebP, WEBP_ANIMATED),
            (NetworkImageFormat::WebP, WEBP_SINGLE_FRAME_ANIMATION_FLAG),
        ] {
            assert_eq!(
                decode_static_png(&decode(encoded), declared, &NetworkImageLimits::default()),
                Err(NetworkImageError::Animated),
                "{declared:?}"
            );
        }
    }

    #[test]
    fn truncated_and_corrupt_bodies_are_refused() {
        let mut truncated_png = decode(PNG_1X1);
        truncated_png.truncate(40);
        let mut truncated_webp = decode(WEBP_LOSSLESS_1X1);
        truncated_webp.truncate(20);
        let mut corrupt_jpeg = decode(JPEG_1X1);
        for byte in &mut corrupt_jpeg[3..] {
            *byte = 0;
        }
        for (declared, body) in [
            (NetworkImageFormat::Png, truncated_png),
            (NetworkImageFormat::WebP, truncated_webp),
            (NetworkImageFormat::Jpeg, corrupt_jpeg),
        ] {
            assert_eq!(
                decode_static_png(&body, declared, &NetworkImageLimits::default()),
                Err(NetworkImageError::Decode),
                "{declared:?}"
            );
        }
    }

    #[test]
    fn pixel_and_derived_byte_budgets_bound_the_allocation() {
        let four_pixels = png(
            2,
            2,
            &[0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255],
        );
        let pixel_limits = NetworkImageLimits {
            max_pixels: 3,
            ..NetworkImageLimits::default()
        };
        assert_eq!(
            decode_static_png(&four_pixels, NetworkImageFormat::Png, &pixel_limits),
            Err(NetworkImageError::TooLarge)
        );
        let rgba_limits = NetworkImageLimits {
            max_rgba_bytes: 8,
            ..NetworkImageLimits::default()
        };
        assert_eq!(
            decode_static_png(&four_pixels, NetworkImageFormat::Png, &rgba_limits),
            Err(NetworkImageError::TooLarge)
        );
        let derived_limits = NetworkImageLimits {
            max_derived_bytes: 64,
            ..NetworkImageLimits::default()
        };
        assert_eq!(
            decode_static_png(
                &decode(PNG_NOISE_16X16),
                NetworkImageFormat::Png,
                &derived_limits
            ),
            Err(NetworkImageError::TooLarge)
        );
    }

    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = 0xffff_ffffu32;
        for &byte in bytes {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                let mask = 0u32.wrapping_sub(crc & 1);
                crc = (crc >> 1) ^ (0xedb8_8320 & mask);
            }
        }
        !crc
    }

    fn png_chunk(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut chunk = Vec::new();
        chunk.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        chunk.extend_from_slice(kind);
        chunk.extend_from_slice(payload);
        let crc = crc32(&chunk[4..]);
        chunk.extend_from_slice(&crc.to_be_bytes());
        chunk
    }

    fn png_chunk_offset(png_bytes: &[u8], kind: &[u8; 4]) -> Option<usize> {
        let mut offset = 8;
        while offset + 12 <= png_bytes.len() {
            let length =
                u32::from_be_bytes(png_bytes[offset..offset + 4].try_into().ok()?) as usize;
            if &png_bytes[offset + 4..offset + 8] == kind {
                return Some(offset);
            }
            offset += 12 + length;
        }
        None
    }

    fn insert_before_chunk(png_bytes: &[u8], kind: &[u8; 4], extra: &[u8]) -> Vec<u8> {
        let at = png_chunk_offset(png_bytes, kind).expect("anchor chunk");
        let mut out = png_bytes[..at].to_vec();
        out.extend_from_slice(extra);
        out.extend_from_slice(&png_bytes[at..]);
        out
    }

    fn webp_chunk(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut chunk = Vec::new();
        chunk.extend_from_slice(kind);
        chunk.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        chunk.extend_from_slice(payload);
        if payload.len() % 2 == 1 {
            chunk.push(0);
        }
        chunk
    }

    fn riff_from_chunks(chunks: &[Vec<u8>]) -> Vec<u8> {
        let mut body = b"WEBP".to_vec();
        for chunk in chunks {
            body.extend_from_slice(chunk);
        }
        let mut out = b"RIFF".to_vec();
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&body);
        out
    }

    fn vp8x_chunk(flags: u8, width: u32, height: u32) -> Vec<u8> {
        let mut payload = vec![flags, 0, 0, 0];
        payload.extend_from_slice(&(width - 1).to_le_bytes()[..3]);
        payload.extend_from_slice(&(height - 1).to_le_bytes()[..3]);
        webp_chunk(b"VP8X", &payload)
    }

    fn tiff_orientation(orientation: u16) -> Vec<u8> {
        let mut tiff = b"II*\0".to_vec();
        tiff.extend_from_slice(&8u32.to_le_bytes());
        tiff.extend_from_slice(&1u16.to_le_bytes());
        tiff.extend_from_slice(&0x0112u16.to_le_bytes());
        tiff.extend_from_slice(&3u16.to_le_bytes());
        tiff.extend_from_slice(&1u32.to_le_bytes());
        tiff.extend_from_slice(&orientation.to_le_bytes());
        tiff.extend_from_slice(&0u16.to_le_bytes());
        tiff.extend_from_slice(&0u32.to_le_bytes());
        tiff
    }

    fn jpeg_with_orientation(width: u32, height: u32, rgb: &[u8], orientation: u16) -> Vec<u8> {
        let mut encoded = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, 100)
            .encode(rgb, width, height, image::ExtendedColorType::Rgb8)
            .unwrap();
        let tiff = tiff_orientation(orientation);
        let mut app1 = vec![0xff, 0xe1];
        app1.extend_from_slice(&((2 + 6 + tiff.len()) as u16).to_be_bytes());
        app1.extend_from_slice(b"Exif\0\0");
        app1.extend_from_slice(&tiff);
        let mut out = encoded[..2].to_vec();
        out.extend_from_slice(&app1);
        out.extend_from_slice(&encoded[2..]);
        out
    }

    fn webp_with_orientation(width: u32, height: u32, rgba: &[u8], orientation: u16) -> Vec<u8> {
        let mut encoded = Vec::new();
        image::codecs::webp::WebPEncoder::new_lossless(&mut encoded)
            .encode(rgba, width, height, image::ExtendedColorType::Rgba8)
            .unwrap();
        let has_alpha = rgba.chunks_exact(4).any(|pixel| pixel[3] != 255);
        let flags = 0x08 | if has_alpha { 0x10 } else { 0x00 };
        riff_from_chunks(&[
            vp8x_chunk(flags, width, height),
            encoded[12..].to_vec(),
            webp_chunk(b"EXIF", &tiff_orientation(orientation)),
        ])
    }

    fn read_png_rgba(png_bytes: &[u8]) -> (u32, u32, Vec<u8>) {
        let packed = png_to_packed(png_bytes).unwrap();
        let image = PackedImage::try_from(packed.as_slice()).unwrap();
        (image.width, image.height, image.rgba.to_vec())
    }

    #[test]
    fn png_budgets_are_checked_before_the_image_is_decoded() {
        let mut corrupt = decode(PNG_NOISE_16X16);
        for byte in &mut corrupt[41..41 + 795] {
            *byte = 0;
        }
        let pixel_limits = NetworkImageLimits {
            max_pixels: 255,
            ..NetworkImageLimits::default()
        };
        assert_eq!(
            decode_static_png(&corrupt, NetworkImageFormat::Png, &pixel_limits),
            Err(NetworkImageError::TooLarge)
        );
        let rgba_limits = NetworkImageLimits {
            max_rgba_bytes: 1023,
            ..NetworkImageLimits::default()
        };
        assert_eq!(
            decode_static_png(&corrupt, NetworkImageFormat::Png, &rgba_limits),
            Err(NetworkImageError::TooLarge)
        );
        let mut oversized_header = decode(PNG_1X1);
        oversized_header[16..20].copy_from_slice(&4096u32.to_be_bytes());
        oversized_header[20..24].copy_from_slice(&4096u32.to_be_bytes());
        let header_limits = NetworkImageLimits {
            max_pixels: 4096,
            ..NetworkImageLimits::default()
        };
        assert_eq!(
            decode_static_png(&oversized_header, NetworkImageFormat::Png, &header_limits),
            Err(NetworkImageError::TooLarge)
        );
    }

    #[test]
    fn late_actl_after_idat_is_refused_as_animation() {
        let with_late_actl = insert_before_chunk(
            &decode(PNG_1X1),
            b"IEND",
            &png_chunk(b"acTL", &[0, 0, 0, 2, 0, 0, 0, 0]),
        );
        assert_eq!(
            decode_static_png(
                &with_late_actl,
                NetworkImageFormat::Png,
                &NetworkImageLimits::default()
            ),
            Err(NetworkImageError::Animated)
        );
    }

    #[test]
    fn png_end_chunk_is_complete_and_terminal() {
        let base = decode(PNG_1X1);
        let mut trailing = base.clone();
        trailing.extend_from_slice(&png_chunk(b"acTL", &[0; 8]));
        let truncated = base[..base.len() - 1].to_vec();
        for body in [trailing, truncated] {
            assert_eq!(
                decode_static_png(
                    &body,
                    NetworkImageFormat::Png,
                    &NetworkImageLimits::default()
                ),
                Err(NetworkImageError::Decode)
            );
        }
    }

    #[test]
    fn bounded_writer_retains_no_more_than_its_output_budget() {
        let mut sink = BoundedWriter {
            bytes: Vec::new(),
            limit: 200,
            exceeded: false,
        };
        for _ in 0..200 {
            sink.write_all(&[42]).unwrap();
            assert!(sink.bytes.capacity() <= sink.limit);
        }
        assert!(sink.write_all(&[42]).is_err());
        assert!(sink.exceeded);
        assert_eq!(sink.bytes.len(), 200);
    }

    #[test]
    fn late_webp_animation_chunks_after_image_data_are_refused() {
        let base = decode(WEBP_LOSSLESS_1X1);
        let image_data = base[12..].to_vec();
        let anim = webp_chunk(b"ANIM", &[0; 6]);
        let anmf = webp_chunk(b"ANMF", &[0; 24]);
        for extra in [
            vec![anim.clone()],
            vec![anmf.clone()],
            vec![anim.clone(), anmf.clone()],
            vec![vp8x_chunk(0x02, 1, 1)],
        ] {
            let mut chunks = vec![image_data.clone()];
            chunks.extend(extra);
            let body = riff_from_chunks(&chunks);
            assert_eq!(
                decode_static_png(
                    &body,
                    NetworkImageFormat::WebP,
                    &NetworkImageLimits::default()
                ),
                Err(NetworkImageError::Animated)
            );
        }
    }

    #[test]
    fn webp_riff_length_is_validated() {
        let base = decode(WEBP_LOSSLESS_1X1);
        let mut truncated = base.clone();
        truncated[4..8].copy_from_slice(&((base.len() + 8) as u32).to_le_bytes());
        let mut surplus = base.clone();
        surplus[4..8].copy_from_slice(&((base.len() - 12) as u32).to_le_bytes());
        for body in [truncated, surplus] {
            assert_eq!(
                decode_static_png(
                    &body,
                    NetworkImageFormat::WebP,
                    &NetworkImageLimits::default()
                ),
                Err(NetworkImageError::Decode)
            );
        }
    }

    #[test]
    fn webp_chunk_padding_and_truncation_are_validated() {
        let base = decode(WEBP_LOSSLESS_1X1);
        let mut missing_pad = base[..base.len() - 1].to_vec();
        let declared = (missing_pad.len() - 8) as u32;
        missing_pad[4..8].copy_from_slice(&declared.to_le_bytes());
        let mut truncated_chunk = base.clone();
        truncated_chunk[16..20].copy_from_slice(&23u32.to_le_bytes());
        for body in [missing_pad, truncated_chunk] {
            assert_eq!(
                decode_static_png(
                    &body,
                    NetworkImageFormat::WebP,
                    &NetworkImageLimits::default()
                ),
                Err(NetworkImageError::Decode)
            );
        }
        let odd_payload = webp_chunk(b"XMP ", &[1, 2, 3]);
        assert_eq!(odd_payload.len() % 2, 0);
        let padded = riff_from_chunks(&[
            vp8x_chunk(0x00, 1, 1),
            odd_payload,
            webp_chunk(b"ANIM", &[0; 6]),
            webp_chunk(b"ANMF", &[0; 24]),
        ]);
        assert_eq!(
            decode_static_png(
                &padded,
                NetworkImageFormat::WebP,
                &NetworkImageLimits::default()
            ),
            Err(NetworkImageError::Animated)
        );
    }

    #[test]
    fn webp_declared_dimensions_are_checked_before_pixel_allocation() {
        let header = [0x2f, 0xfe, 0xbf, 0xff, 0x0f];
        let body = riff_from_chunks(&[webp_chunk(b"VP8L", &header)]);
        assert_eq!(
            decode_static_png(
                &body,
                NetworkImageFormat::WebP,
                &NetworkImageLimits::default()
            ),
            Err(NetworkImageError::TooLarge)
        );
    }

    #[test]
    fn jpeg_exif_orientation_is_applied() {
        let rgb = [0u8, 0, 0, 255, 255, 255];
        let upright = jpeg_with_orientation(2, 1, &rgb, 1);
        let derived = decode_static_png(
            &upright,
            NetworkImageFormat::Jpeg,
            &NetworkImageLimits::default(),
        )
        .unwrap();
        assert_eq!(derived.pixels, 2);
        let (width, height, rgba) = read_png_rgba(&derived.png);
        assert_eq!((width, height), (2, 1));
        assert_eq!(rgba[3], 255);
        assert_eq!(rgba[7], 255);
        let left = u32::from(rgba[0]) + u32::from(rgba[1]) + u32::from(rgba[2]);
        let right = u32::from(rgba[4]) + u32::from(rgba[5]) + u32::from(rgba[6]);
        assert!(left + 64 < right, "left {left} right {right}");

        let rotated = jpeg_with_orientation(2, 1, &rgb, 6);
        let derived = decode_static_png(
            &rotated,
            NetworkImageFormat::Jpeg,
            &NetworkImageLimits::default(),
        )
        .unwrap();
        assert_eq!(derived.pixels, 2);
        let (width, height, rgba) = read_png_rgba(&derived.png);
        assert_eq!((width, height), (1, 2));
        let top = u32::from(rgba[0]) + u32::from(rgba[1]) + u32::from(rgba[2]);
        let bottom = u32::from(rgba[4]) + u32::from(rgba[5]) + u32::from(rgba[6]);
        assert!(top + 64 < bottom, "top {top} bottom {bottom}");
        assert_eq!(rgba[3], 255);
        assert_eq!(rgba[7], 255);
    }

    #[test]
    fn webp_exif_orientation_is_applied() {
        let rgba = [255u8, 0, 0, 128, 0, 0, 255, 255];
        let upright = webp_with_orientation(2, 1, &rgba, 1);
        let derived = decode_static_png(
            &upright,
            NetworkImageFormat::WebP,
            &NetworkImageLimits::default(),
        )
        .unwrap();
        assert_eq!(read_png_rgba(&derived.png), (2, 1, rgba.to_vec()));

        let rotated = webp_with_orientation(2, 1, &rgba, 6);
        let derived = decode_static_png(
            &rotated,
            NetworkImageFormat::WebP,
            &NetworkImageLimits::default(),
        )
        .unwrap();
        assert_eq!(derived.pixels, 2);
        assert_eq!(read_png_rgba(&derived.png), (1, 2, rgba.to_vec()));
    }

    #[test]
    fn orientation_budgets_are_enforced_on_final_dimensions() {
        let rgba = [255u8, 0, 0, 128, 0, 0, 255, 255];
        let rotated = webp_with_orientation(2, 1, &rgba, 6);
        let pixel_limits = NetworkImageLimits {
            max_pixels: 2,
            ..NetworkImageLimits::default()
        };
        let derived = decode_static_png(&rotated, NetworkImageFormat::WebP, &pixel_limits).unwrap();
        assert_eq!(derived.pixels, 2);
        assert_eq!(read_png_rgba(&derived.png).0, 1);
        let tight_pixels = NetworkImageLimits {
            max_pixels: 1,
            ..NetworkImageLimits::default()
        };
        assert_eq!(
            decode_static_png(&rotated, NetworkImageFormat::WebP, &tight_pixels),
            Err(NetworkImageError::TooLarge)
        );
        let exact_bytes = NetworkImageLimits {
            max_derived_bytes: derived.png.len(),
            ..NetworkImageLimits::default()
        };
        let exact = decode_static_png(&rotated, NetworkImageFormat::WebP, &exact_bytes).unwrap();
        assert_eq!(exact.png, derived.png);
        assert!(exact.png.ends_with(b"\0\0\0\0IEND\xae\x42\x60\x82"));
        let tight_bytes = NetworkImageLimits {
            max_derived_bytes: derived.png.len() - 1,
            ..NetworkImageLimits::default()
        };
        assert_eq!(
            decode_static_png(&rotated, NetworkImageFormat::WebP, &tight_bytes),
            Err(NetworkImageError::TooLarge)
        );
    }
}
