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
        self.validated_encoded_len().map(|_| ())
    }

    /// The exact byte length [`Self::encode`] produces, after the same checks.
    ///
    /// The CNA1 header is a byte-for-byte replacement of the CNS1 header, so the
    /// validated original length is already the right base: the header is counted
    /// once. Measuring the original by encoding it would copy the whole original
    /// payload just to learn its length.
    pub(crate) fn validated_encoded_len(&self) -> Result<usize, ProtocolError> {
        let mut size = self.original.validated_encoded_len()?;
        if self.platform > 2 || self.original.formats.len() + self.derived.len() > FORMAT_LIMIT {
            return Err(invalid());
        }
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
        Ok(size)
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

    fn non_trivial_snapshot() -> NativeSnapshot {
        NativeSnapshot {
            before: 7,
            after: 7,
            items: 3,
            state: SnapshotState::Stable,
            formats: vec![
                NativeFormat {
                    item: 0,
                    name: "public.utf8-plain-text".into(),
                    state: FormatState::Captured,
                    bytes: "héllo ✓".as_bytes().to_vec(),
                },
                NativeFormat {
                    item: 1,
                    name: "图片.png".into(),
                    state: FormatState::Captured,
                    bytes: vec![0x89, 0x50, 0x4e, 0x47, 0x00, 0xff],
                },
                NativeFormat {
                    item: 2,
                    name: "public.html".into(),
                    state: FormatState::Absent,
                    bytes: vec![],
                },
                NativeFormat {
                    item: 2,
                    name: "public.rtf".into(),
                    state: FormatState::Failed,
                    bytes: vec![],
                },
                NativeFormat {
                    item: 2,
                    name: "public.tiff".into(),
                    state: FormatState::TooLarge,
                    bytes: vec![],
                },
            ],
        }
    }

    fn non_trivial_archive() -> NativeArchive {
        NativeArchive {
            platform: 2,
            enumeration_complete: true,
            original: NativeSnapshot {
                before: 11,
                after: 11,
                items: 2,
                state: SnapshotState::Stable,
                formats: vec![
                    NativeFormat {
                        item: 0,
                        name: "com.apple.flat-rtfd".into(),
                        state: FormatState::Captured,
                        bytes: b"original-rtfd-bytes".to_vec(),
                    },
                    NativeFormat {
                        item: 1,
                        name: "公共.png".into(),
                        state: FormatState::Failed,
                        bytes: vec![],
                    },
                ],
            },
            derived: vec![
                DerivedFormat {
                    item: 0,
                    name: "public.html".into(),
                    source_name: "com.apple.flat-rtfd".into(),
                    converter: "tailsync.rtfd-to-portable.v1".into(),
                    bytes: b"<p>derived</p>".to_vec(),
                },
                DerivedFormat {
                    item: 0,
                    name: "纯文本.txt".into(),
                    source_name: "com.apple.flat-rtfd".into(),
                    converter: "转换器.v1".into(),
                    bytes: "派生文本".as_bytes().to_vec(),
                },
            ],
        }
    }

    /// The exact bytes the writer produced before F7 replaced the measurement
    /// allocation. Captured from the pre-change build (`CNS1_LEN=172`,
    /// `CNA1_LEN=256`) so a writer change cannot pass unnoticed.
    const CNS1_FROZEN_HEX: &str = "434e53310100000300000000000000070000000000000007000500000000000000000000001600000000000a7075626c69632e757466382d706c61696e2d7465787468c3a96c6c6f20e29c9300010000000a000000000006e59bbee789872e706e6789504e4700ff00020100000b0000000000007075626c69632e68746d6c00020200000a0000000000007075626c69632e72746600020300000b0000000000007075626c69632e74696666";
    const CNA1_FROZEN_HEX: &str = "434e413101000201000000000000000b000000000000000b0002000200020000000000000013000000000013636f6d2e6170706c652e666c61742d727466646f726967696e616c2d727466642d627974657300010200000a000000000000e585ace585b12e706e670000000b0013001c0000000e7075626c69632e68746d6c636f6d2e6170706c652e666c61742d727466647461696c73796e632e727466642d746f2d706f727461626c652e76313c703e646572697665643c2f703e0000000d0013000c0000000ce7baafe69687e69cac2e747874636f6d2e6170706c652e666c61742d72746664e8bdace68da2e599a82e7631e6b4bee7949fe69687e69cac";

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn frozen_codec_bytes_are_unchanged_and_lengths_match_the_writer() {
        let snapshot = non_trivial_snapshot();
        let encoded = snapshot.encode().unwrap();
        assert_eq!(hex(&encoded), CNS1_FROZEN_HEX);
        assert_eq!(encoded.len(), 172);
        assert_eq!(
            snapshot.validated_encoded_len().unwrap(),
            encoded.len(),
            "the validated length must be the length the writer produces"
        );

        let archive = non_trivial_archive();
        let encoded = archive.encode().unwrap();
        assert_eq!(hex(&encoded), CNA1_FROZEN_HEX);
        assert_eq!(encoded.len(), 256);
        assert_eq!(
            archive.validated_encoded_len().unwrap(),
            encoded.len(),
            "the archive header replaces the CNS1 header, so it is counted once"
        );
    }

    /// 32-byte header + 12 descriptor bytes per format + UTF-8 name + payload.
    fn snapshot_encoded_len(snapshot: &NativeSnapshot) -> usize {
        HEADER
            + snapshot
                .formats
                .iter()
                .map(|f| 12 + f.name.len() + f.bytes.len())
                .sum::<usize>()
    }

    fn archive_encoded_len(archive: &NativeArchive) -> usize {
        snapshot_encoded_len(&archive.original)
            + archive
                .derived
                .iter()
                .map(|d| {
                    12 + d.name.len() + d.source_name.len() + d.converter.len() + d.bytes.len()
                })
                .sum::<usize>()
    }

    #[test]
    fn validated_length_matches_the_writer_for_every_shape() {
        let mut suppressed = non_trivial_snapshot();
        suppressed.state = SnapshotState::Suppressed;
        suppressed.items = 0;
        suppressed.formats.clear();
        let mut unstable = non_trivial_snapshot();
        unstable.state = SnapshotState::Unstable;
        unstable.before = 3;
        unstable.after = 4;
        let mut selection = non_trivial_snapshot();
        selection.state = SnapshotState::FileSelection;
        selection.items = 300;
        selection.formats.clear();

        for snapshot in [non_trivial_snapshot(), suppressed, unstable, selection] {
            assert_eq!(
                snapshot.validated_encoded_len().unwrap(),
                snapshot.encode().unwrap().len(),
                "snapshot length accounting drifted"
            );
        }

        let mut original_only = non_trivial_archive();
        original_only.derived.clear();
        let metadata_only = |state, items| NativeArchive {
            platform: 1,
            enumeration_complete: true,
            original: NativeSnapshot {
                before: 1,
                after: 1,
                items,
                state,
                formats: vec![],
            },
            derived: vec![],
        };
        for archive in [
            non_trivial_archive(),
            original_only,
            metadata_only(SnapshotState::Suppressed, 0),
            metadata_only(SnapshotState::FileSelection, 300),
        ] {
            assert_eq!(
                archive.validated_encoded_len().unwrap(),
                archive.encode().unwrap().len(),
                "archive length accounting drifted"
            );
        }
    }

    #[test]
    fn the_exact_native_limit_is_accepted_and_one_byte_more_is_rejected() {
        let payload = NATIVE_LIMIT - HEADER - 12 - 4;
        let at_limit = NativeSnapshot {
            before: 1,
            after: 1,
            items: 1,
            state: SnapshotState::Stable,
            formats: vec![NativeFormat {
                item: 0,
                name: "abcd".into(),
                state: FormatState::Captured,
                bytes: vec![7; payload],
            }],
        };
        assert_eq!(snapshot_encoded_len(&at_limit), NATIVE_LIMIT);
        let encoded = at_limit.encode().unwrap();
        assert_eq!(encoded.len(), NATIVE_LIMIT);
        assert_eq!(at_limit.validated_encoded_len().unwrap(), NATIVE_LIMIT);

        let mut over = at_limit.clone();
        over.formats[0].bytes.push(0);
        assert_eq!(snapshot_encoded_len(&over), NATIVE_LIMIT + 1);
        assert!(over.validated_encoded_len().is_err());
        assert!(over.encode().is_err());
    }

    #[test]
    fn the_archive_limit_counts_the_original_and_every_derived_record_once() {
        let original = NativeSnapshot {
            before: 1,
            after: 1,
            items: 1,
            state: SnapshotState::Stable,
            formats: vec![NativeFormat {
                item: 0,
                name: "src".into(),
                state: FormatState::Captured,
                bytes: vec![1, 2, 3],
            }],
        };
        let original_len = snapshot_encoded_len(&original);
        let derived_overhead = 12 + 1 + 3 + 1;
        let at_limit = NativeArchive {
            platform: 1,
            enumeration_complete: true,
            original: original.clone(),
            derived: vec![DerivedFormat {
                item: 0,
                name: "d".into(),
                source_name: "src".into(),
                converter: "c".into(),
                bytes: vec![0; NATIVE_LIMIT - original_len - derived_overhead],
            }],
        };
        assert_eq!(archive_encoded_len(&at_limit), NATIVE_LIMIT);
        let encoded = at_limit.encode().unwrap();
        assert_eq!(
            encoded.len(),
            NATIVE_LIMIT,
            "the archive header must not be counted twice"
        );
        assert_eq!(at_limit.validated_encoded_len().unwrap(), NATIVE_LIMIT);

        let mut over = at_limit.clone();
        over.derived[0].bytes.push(0);
        assert_eq!(archive_encoded_len(&over), NATIVE_LIMIT + 1);
        assert!(over.validated_encoded_len().is_err());
        assert!(over.encode().is_err());
    }

    #[test]
    fn unicode_names_and_converters_are_bounded_by_utf8_byte_length() {
        let name_256 = "é".repeat(128);
        assert_eq!(name_256.len(), 256);
        let mut snapshot = non_trivial_snapshot();
        snapshot.formats[0].name = name_256.clone();
        assert!(snapshot.validated_encoded_len().is_ok());
        snapshot.formats[0].name = format!("{name_256}a");
        assert!(snapshot.validated_encoded_len().is_err());

        let mut archive = non_trivial_archive();
        archive.derived[0].name = name_256.clone();
        assert!(archive.validated_encoded_len().is_ok());
        archive.derived[0].name = format!("{name_256}a");
        assert!(archive.validated_encoded_len().is_err());

        // Every derived record links to the original format by name, so the whole
        // chain has to move together.
        let mut archive = non_trivial_archive();
        archive.original.formats[0].name = name_256.clone();
        for derived in archive.derived.iter_mut() {
            derived.source_name = name_256.clone();
        }
        assert!(archive.validated_encoded_len().is_ok());
        archive.original.formats[0].name = format!("{name_256}a");
        for derived in archive.derived.iter_mut() {
            derived.source_name = format!("{name_256}a");
        }
        assert!(
            archive.validated_encoded_len().is_err(),
            "a 257-byte source name is rejected by the byte-length bound"
        );

        let converter_128 = "转".repeat(42) + "ab";
        assert_eq!(converter_128.len(), 128);
        let mut archive = non_trivial_archive();
        archive.derived[0].converter = converter_128;
        assert!(archive.validated_encoded_len().is_ok());
        archive.derived[0].converter = format!("{}a", "转".repeat(42) + "ab");
        assert!(archive.validated_encoded_len().is_err());
    }

    #[test]
    fn duplicates_bad_links_counts_states_and_versions_are_rejected() {
        let mut duplicate = non_trivial_snapshot();
        duplicate.formats.push(duplicate.formats[0].clone());
        assert!(duplicate.validated_encoded_len().is_err());

        let mut duplicate_derived = non_trivial_archive();
        duplicate_derived
            .derived
            .push(duplicate_derived.derived[0].clone());
        assert!(duplicate_derived.validated_encoded_len().is_err());

        let mut bad_link = non_trivial_archive();
        bad_link.derived[0].source_name = "not-an-original".into();
        assert!(bad_link.validated_encoded_len().is_err());

        let mut failed_link = non_trivial_archive();
        failed_link.original.formats[0].state = FormatState::Failed;
        failed_link.original.formats[0].bytes.clear();
        assert!(
            failed_link.validated_encoded_len().is_err(),
            "a derived record may only point at a captured original"
        );

        let mut over_items = non_trivial_snapshot();
        over_items.items = ITEM_LIMIT as u16 + 1;
        assert!(over_items.validated_encoded_len().is_err());
        let mut selection = over_items.clone();
        selection.state = SnapshotState::FileSelection;
        selection.formats.clear();
        assert!(
            selection.validated_encoded_len().is_ok(),
            "a file selection is classified before the composite item limit"
        );

        let mut over_formats = non_trivial_snapshot();
        over_formats.formats = (0..=FORMAT_LIMIT)
            .map(|index| NativeFormat {
                item: 0,
                name: format!("format-{index}"),
                state: FormatState::Absent,
                bytes: vec![],
            })
            .collect();
        assert!(over_formats.validated_encoded_len().is_err());

        let mut version = non_trivial_snapshot().encode().unwrap();
        version[4] = 2;
        assert!(NativeSnapshot::decode(&version).is_err());
        let mut state = non_trivial_snapshot().encode().unwrap();
        state[5] = 9;
        assert!(NativeSnapshot::decode(&state).is_err());
        let mut archive_version = non_trivial_archive().encode().unwrap();
        archive_version[4] = 2;
        assert!(NativeArchive::decode(&archive_version).is_err());
    }

    /// An invalid original stays invalid even when the archive around it is also
    /// invalid: the same fixed error category, no drift.
    #[test]
    fn an_invalid_original_keeps_its_error_even_when_the_archive_is_also_invalid() {
        let mut archive = non_trivial_archive();
        archive.original.items = 0;
        archive.original.formats.clear();
        archive.platform = 9;
        archive.derived[0].source_name = "missing".into();
        assert!(matches!(
            archive.validated_encoded_len().unwrap_err(),
            ProtocolError::InvalidNativeSnapshot
        ));
        assert!(matches!(
            archive.original.validated_encoded_len().unwrap_err(),
            ProtocolError::InvalidNativeSnapshot
        ));
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
