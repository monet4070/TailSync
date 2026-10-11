//! Checked DIB conversion used by the isolated Windows helper. Only proven
//! uncompressed 24/32-bit layouts are admitted; handles never cross IPC.
use crate::{pack_rgba_image, PackedImage, ProtocolError};
fn invalid() -> ProtocolError {
    ProtocolError::InvalidImage
}
fn u32_at(b: &[u8], n: usize) -> Result<u32, ProtocolError> {
    Ok(u32::from_le_bytes(
        b.get(n..n + 4)
            .ok_or_else(invalid)?
            .try_into()
            .map_err(|_| invalid())?,
    ))
}
pub fn decode_dib(b: &[u8]) -> Result<Vec<u8>, ProtocolError> {
    let header = u32_at(b, 0)? as usize;
    if !matches!(header, 40 | 108 | 124) || header > b.len() {
        return Err(invalid());
    }
    let width = u32_at(b, 4)? as i32;
    let signed_height = u32_at(b, 8)? as i32;
    if width <= 0 || signed_height == 0 || signed_height == i32::MIN || b[12..14] != [1, 0] {
        return Err(invalid());
    }
    let height = signed_height.unsigned_abs();
    let bits = u16::from_le_bytes([b[14], b[15]]);
    let compression = u32_at(b, 16)?;
    if !matches!(bits, 24 | 32) || !matches!(compression, 0 | 3) || (bits == 24 && compression != 0)
    {
        return Err(invalid());
    }
    let pixels = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .filter(|n| *n <= 32 * 1024 * 1024 - 8)
        .ok_or_else(invalid)?;
    let mut offset = header;
    let mut alpha = false;
    if compression == 3 {
        let masks = if header == 40 {
            offset = offset.checked_add(12).ok_or_else(invalid)?;
            40
        } else {
            40
        };
        if u32_at(b, masks)? != 0x00ff0000
            || u32_at(b, masks + 4)? != 0x0000ff00
            || u32_at(b, masks + 8)? != 0x000000ff
        {
            return Err(invalid());
        }
        if header >= 108 {
            let a = u32_at(b, 52)?;
            if !matches!(a, 0 | 0xff000000) {
                return Err(invalid());
            }
            alpha = a != 0;
        }
    }
    let colors = u32_at(b, 32)? as usize;
    if colors != 0 {
        return Err(invalid());
    }
    let stride = (width as usize)
        .checked_mul(bits as usize)
        .and_then(|n| n.checked_add(31))
        .map(|n| n / 32 * 4)
        .ok_or_else(invalid)?;
    let end = offset
        .checked_add(stride.checked_mul(height as usize).ok_or_else(invalid)?)
        .filter(|n| *n <= b.len())
        .ok_or_else(invalid)?;
    let declared = u32_at(b, 20)? as usize;
    if declared != 0 && declared != end - offset {
        return Err(invalid());
    }
    let mut rgba = vec![0; pixels];
    for y in 0..height as usize {
        let source_y = if signed_height > 0 {
            height as usize - 1 - y
        } else {
            y
        };
        let row = &b[offset + source_y * stride..offset + (source_y + 1) * stride];
        for x in 0..width as usize {
            let s = x * (bits as usize / 8);
            let d = (y * width as usize + x) * 4;
            rgba[d..d + 4].copy_from_slice(&[
                row[s + 2],
                row[s + 1],
                row[s],
                if alpha { row[s + 3] } else { 255 },
            ]);
        }
    }
    pack_rgba_image(width as u32, height, &rgba)
}
pub fn encode_dib_v5(image: PackedImage<'_>) -> Result<Vec<u8>, ProtocolError> {
    if image.width > i32::MAX as u32 || image.height > i32::MAX as u32 {
        return Err(invalid());
    }
    let packed = pack_rgba_image(image.width, image.height, image.rgba)?;
    let mut b = vec![0; 124];
    b[0..4].copy_from_slice(&124u32.to_le_bytes());
    b[4..8].copy_from_slice(&image.width.to_le_bytes());
    b[8..12].copy_from_slice(&(-(image.height as i32)).to_le_bytes());
    b[12..14].copy_from_slice(&1u16.to_le_bytes());
    b[14..16].copy_from_slice(&32u16.to_le_bytes());
    b[16..20].copy_from_slice(&3u32.to_le_bytes());
    b[20..24].copy_from_slice(&(image.rgba.len() as u32).to_le_bytes());
    for (offset, mask) in [
        (40, 0x00ff0000u32),
        (44, 0x0000ff00),
        (48, 0x000000ff),
        (52, 0xff000000),
        (56, 0x73524742),
    ] {
        b[offset..offset + 4].copy_from_slice(&mask.to_le_bytes());
    }
    for p in packed[8..].chunks_exact(4) {
        b.extend_from_slice(&[p[2], p[1], p[0], p[3]]);
    }
    Ok(b)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn alpha_top_down_round_trip() {
        let bytes = pack_rgba_image(2, 1, &[255, 0, 0, 128, 0, 255, 0, 0]).unwrap();
        let dib = encode_dib_v5(PackedImage::try_from(bytes.as_slice()).unwrap()).unwrap();
        assert_eq!(decode_dib(&dib).unwrap(), bytes);
    }
    #[test]
    fn malformed_dimensions_stride_and_compression_fail() {
        let bytes = pack_rgba_image(1, 1, &[1, 2, 3, 4]).unwrap();
        let mut dib = encode_dib_v5(PackedImage::try_from(bytes.as_slice()).unwrap()).unwrap();
        for n in 0..dib.len() {
            assert!(decode_dib(&dib[..n]).is_err());
        }
        dib[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode_dib(&dib).is_err());
    }
}
