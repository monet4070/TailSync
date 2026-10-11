//! CNA1 is a local capture archive, never a peer frame or publication request.
//! Original advertised formats and conversion outputs occupy separate roles.
use super::native_clipboard::*;
use super::ProtocolError;
use std::collections::BTreeSet;

/// Bounded static PNG cover derived from an original image encoding the
/// portable model cannot carry (GIF/JPEG/WebP/APNG, multi-page TIFF). The
/// converter name is the explicit derived role: the archive keeps the original
/// bytes and this is only a compatibility representation.
pub const CONVERTER_STATIC_COVER: &str = "tailsync.image-static-cover.v1";
/// Existing single-frame TIFF projection.
pub const CONVERTER_TIFF_TO_PNG: &str = "tailsync.tiff-to-png.v1";
/// Existing RTFD projection.
pub const CONVERTER_RTFD_TO_PORTABLE: &str = "tailsync.rtfd-to-portable.v1";

/// QQ's ordered message elements are one semantic document even when its
/// images advertise separate pasteboard items. Conversion preserves originals.
pub const QQ_MESSAGE_FORMAT: &str = "dyn.ah62d4rv4gu8zk3psqrw067cbsv4he4pcsz4gny5ysmy0634ytf2gn";
pub const CONVERTER_QQ_TO_PORTABLE: &str = "tailsync.qq-to-portable.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivedFormat {
    pub item: u16,
    pub name: String,
    pub source_name: String,
    pub converter: String,
    pub bytes: Vec<u8>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeArchive {
    pub platform: u8,
    pub enumeration_complete: bool,
    pub original: NativeSnapshot,
    pub derived: Vec<DerivedFormat>,
}
fn invalid() -> ProtocolError {
    ProtocolError::InvalidNativeSnapshot
}
impl NativeArchive {
    /// Whether every advertised original item can be restored without omission.
    /// This says nothing about an application's external resource dependencies.
    pub fn originals_complete(&self) -> bool {
        self.platform != 0
            && self.enumeration_complete
            && self.original.state == SnapshotState::Stable
            && (0..self.original.items).all(|item| {
                self.original.formats.iter().any(|f| {
                    f.item == item && !is_control(&f.name) && f.state == FormatState::Captured
                })
            })
            && self
                .original
                .formats
                .iter()
                .all(|f| is_control(&f.name) || f.state == FormatState::Captured)
    }
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.original.validate()?;
        if self.platform > 2 || self.original.formats.len() + self.derived.len() > FORMAT_LIMIT {
            return Err(invalid());
        }
        let mut size = self.original.encode()?.len();
        let mut seen = BTreeSet::new();
        for d in &self.derived {
            if d.item >= self.original.items
                || d.name.is_empty()
                || d.name.len() > 256
                || d.source_name.is_empty()
                || d.source_name.len() > 256
                || d.converter.is_empty()
                || d.converter.len() > 128
                || [
                    d.name.as_str(),
                    d.source_name.as_str(),
                    d.converter.as_str(),
                ]
                .iter()
                .any(|s| s.contains('\0'))
                || !seen.insert((d.item, &d.name))
                || !self.original.formats.iter().any(|f| {
                    f.item == d.item && f.name == d.source_name && f.state == FormatState::Captured
                })
            {
                return Err(invalid());
            }
            size = size
                .checked_add(12 + d.name.len() + d.source_name.len() + d.converter.len())
                .and_then(|n| n.checked_add(d.bytes.len()))
                .ok_or_else(invalid)?;
        }
        if size > NATIVE_LIMIT
            || (self.original.state != SnapshotState::Stable && !self.derived.is_empty())
        {
            return Err(invalid());
        }
        Ok(())
    }
    pub fn encode(&self) -> Result<Vec<u8>, ProtocolError> {
        self.validate()?;
        let original = self.original.encode()?;
        let mut b = Vec::with_capacity(original.len());
        b.extend_from_slice(b"CNA1");
        b.push(1);
        b.push(self.original.state as u8);
        b.push(self.platform);
        b.push(u8::from(self.enumeration_complete));
        b.extend_from_slice(&self.original.before.to_be_bytes());
        b.extend_from_slice(&self.original.after.to_be_bytes());
        b.extend_from_slice(&self.original.items.to_be_bytes());
        b.extend_from_slice(&(self.original.formats.len() as u16).to_be_bytes());
        b.extend_from_slice(&(self.derived.len() as u16).to_be_bytes());
        b.extend_from_slice(&[0; 2]);
        b.extend_from_slice(&original[HEADER..]);
        for d in &self.derived {
            b.extend_from_slice(&d.item.to_be_bytes());
            for s in [&d.name, &d.source_name, &d.converter] {
                b.extend_from_slice(&(s.len() as u16).to_be_bytes());
            }
            b.extend_from_slice(&(d.bytes.len() as u32).to_be_bytes());
            for s in [&d.name, &d.source_name, &d.converter] {
                b.extend_from_slice(s.as_bytes());
            }
            b.extend_from_slice(&d.bytes);
        }
        Ok(b)
    }
    pub fn decode(b: &[u8]) -> Result<Self, ProtocolError> {
        if b.len() < HEADER
            || b.len() > NATIVE_LIMIT
            || &b[..5] != b"CNA1\x01"
            || b[7] > 1
            || b[30..32] != [0; 2]
        {
            return Err(invalid());
        }
        let u16_at = |i| u16::from_be_bytes([b[i], b[i + 1]]);
        let count = u16_at(26) as usize;
        let derived = u16_at(28) as usize;
        if count + derived > FORMAT_LIMIT {
            return Err(invalid());
        }
        let mut offset = HEADER;
        for _ in 0..count {
            let h = b.get(offset..offset + 12).ok_or_else(invalid)?;
            let length = u16::from_be_bytes([h[4], h[5]]) as usize
                + u32::from_be_bytes(h[8..12].try_into().map_err(|_| invalid())?) as usize;
            offset = offset
                .checked_add(12)
                .and_then(|n| n.checked_add(length))
                .filter(|n| *n <= b.len())
                .ok_or_else(invalid)?;
        }
        let mut cns = Vec::with_capacity(offset);
        cns.extend_from_slice(b"CNS1\x01");
        cns.push(b[5]);
        cns.extend_from_slice(&b[24..26]);
        cns.extend_from_slice(&b[8..24]);
        cns.extend_from_slice(&b[26..28]);
        cns.extend_from_slice(&[0; 6]);
        cns.extend_from_slice(&b[HEADER..offset]);
        let original = NativeSnapshot::decode(&cns)?;
        let mut output = Self {
            platform: b[6],
            enumeration_complete: b[7] == 1,
            original,
            derived: vec![],
        };
        for _ in 0..derived {
            let h = b.get(offset..offset + 12).ok_or_else(invalid)?;
            let item = u16::from_be_bytes([h[0], h[1]]);
            let lengths = [
                u16::from_be_bytes([h[2], h[3]]) as usize,
                u16::from_be_bytes([h[4], h[5]]) as usize,
                u16::from_be_bytes([h[6], h[7]]) as usize,
            ];
            let length = u32::from_be_bytes(h[8..12].try_into().map_err(|_| invalid())?) as usize;
            offset += 12;
            let mut strings = Vec::new();
            for n in lengths {
                let end = offset.checked_add(n).ok_or_else(invalid)?;
                strings.push(
                    std::str::from_utf8(b.get(offset..end).ok_or_else(invalid)?)
                        .map_err(|_| invalid())?
                        .to_string(),
                );
                offset = end;
            }
            let end = offset.checked_add(length).ok_or_else(invalid)?;
            let bytes = b.get(offset..end).ok_or_else(invalid)?.to_vec();
            offset = end;
            output.derived.push(DerivedFormat {
                item,
                name: strings[0].clone(),
                source_name: strings[1].clone(),
                converter: strings[2].clone(),
                bytes,
            });
        }
        if offset != b.len() {
            return Err(invalid());
        }
        output.validate()?;
        Ok(output)
    }
    /// Original restore request, retaining unread descriptors so preparation
    /// cannot mistake a captured subset for the complete set. Callers must also
    /// check originals_complete(); CNS1 cannot carry enumeration completeness.
    pub fn publication(&self) -> Result<NativeSnapshot, ProtocolError> {
        self.validate()?;
        let mut s = self.original.clone();
        s.before = 0;
        s.after = 0;
        s.formats.retain(|f| !is_control(&f.name));
        s.validate()?;
        Ok(s)
    }
}

fn is_control(name: &str) -> bool {
    super::native_policy::control(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> NativeArchive {
        NativeArchive {
            platform: 1,
            enumeration_complete: true,
            original: NativeSnapshot {
                before: 1,
                after: 1,
                items: 1,
                state: SnapshotState::Stable,
                formats: vec![NativeFormat {
                    item: 0,
                    name: "com.apple.flat-rtfd".into(),
                    state: FormatState::Captured,
                    bytes: b"original".to_vec(),
                }],
            },
            derived: vec![DerivedFormat {
                item: 0,
                name: "public.html".into(),
                source_name: "com.apple.flat-rtfd".into(),
                converter: "tailsync.rtfd-to-portable.v1".into(),
                bytes: b"<p>derived</p>".to_vec(),
            }],
        }
    }
    #[test]
    fn roles_roundtrip_and_cns1_rejects_archive() {
        let s = sample();
        let b = s.encode().unwrap();
        assert_eq!(NativeArchive::decode(&b).unwrap(), s);
        assert!(NativeSnapshot::decode(&b).is_err());
        assert_eq!(s.publication().unwrap().formats.len(), 1);
    }
    #[test]
    fn unread_originals_remain_visible_to_restore_preparation() {
        let mut a = sample();
        a.original.formats.push(NativeFormat {
            item: 0,
            name: "public.html".into(),
            state: FormatState::Failed,
            bytes: vec![],
        });
        assert!(!a.originals_complete());
        assert_eq!(a.publication().unwrap().formats, a.original.formats);
        a.original.formats.pop();
        a.enumeration_complete = false;
        assert!(!a.originals_complete());
        a.enumeration_complete = true;
        assert!(a.originals_complete());
    }
    #[test]
    fn suppressed_archive_is_metadata_only_and_cannot_carry_any_body() {
        let mut a = sample();
        a.original.state = SnapshotState::Suppressed;
        a.original.items = 0;
        a.original.formats.clear();
        a.derived.clear();
        let bytes = a.encode().unwrap();
        assert_eq!(bytes.len(), 32);
        assert_eq!(NativeArchive::decode(&bytes).unwrap(), a);
        assert!(!a.originals_complete());
        a.original.items = 1;
        assert!(a.encode().is_err());
        a.original.items = 0;
        a.original.formats = sample().original.formats;
        assert!(a.encode().is_err());
    }
    #[test]
    fn every_truncation_and_trailing_data_is_rejected() {
        let b = sample().encode().unwrap();
        for i in 0..b.len() {
            assert!(NativeArchive::decode(&b[..i]).is_err());
        }
        let mut trailing = b;
        trailing.push(0);
        assert!(NativeArchive::decode(&trailing).is_err());
    }
    #[test]
    fn bad_links_versions_and_budget_are_rejected() {
        let mut s = sample();
        s.derived[0].source_name = "missing".into();
        assert!(s.encode().is_err());
        let mut b = sample().encode().unwrap();
        b[4] = 2;
        assert!(NativeArchive::decode(&b).is_err());
        let mut s = sample();
        s.derived[0].bytes = vec![0; NATIVE_LIMIT];
        assert!(s.encode().is_err());
    }
}
