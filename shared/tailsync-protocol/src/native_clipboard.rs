//! Local helper protocol. Source item boundaries and read failures are retained.
//! This codec is never sent to a peer.
use crate::ProtocolError;
use std::collections::BTreeSet;
pub const NATIVE_LIMIT: usize = 40 * 1024 * 1024;
pub const ITEM_LIMIT: usize = 16;
pub const FORMAT_LIMIT: usize = 64;
pub const HEADER: usize = 32;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SnapshotState {
    Stable = 0,
    Empty = 1,
    Unstable = 2,
    Busy = 3,
    Failed = 4,
    /// The pasteboard holds a genuine file selection: every item is a file URL.
    ///
    /// Reported **before** the composite item limit is applied, so a large file
    /// selection is classified rather than failing the read and vanishing from both
    /// the sync path and the history (R5-01). It carries no payload — the file
    /// pipeline reads the paths itself — which is why it is allowed to exceed
    /// [`ITEM_LIMIT`].
    FileSelection = 5,
    /// Metadata blocked this entire copy before any payload read. No body or
    /// format descriptors may cross the process boundary in this state.
    Suppressed = 6,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FormatState {
    Captured = 0,
    Absent = 1,
    Failed = 2,
    TooLarge = 3,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeFormat {
    pub item: u16,
    pub name: String,
    pub state: FormatState,
    pub bytes: Vec<u8>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeSnapshot {
    pub before: i64,
    pub after: i64,
    pub items: u16,
    pub state: SnapshotState,
    pub formats: Vec<NativeFormat>,
}
fn invalid() -> ProtocolError {
    ProtocolError::InvalidNativeSnapshot
}
impl NativeSnapshot {
    /// The exact byte length [`Self::encode`] produces, after the same checks.
    ///
    /// The size budget needs the encoded length, and measuring it by encoding the
    /// snapshot would copy the whole payload only to drop it again. Every caller
    /// that needs the length asks here; `encode` stays the only writer.
    pub(crate) fn validated_encoded_len(&self) -> Result<usize, ProtocolError> {
        // A file selection carries no payload, so it is not subject to the composite
        // item limit; the file pipeline handles however many paths were copied.
        let over_item_limit =
            self.items as usize > ITEM_LIMIT && self.state != SnapshotState::FileSelection;
        if over_item_limit
            || self.formats.len() > FORMAT_LIMIT
            || (self.state == SnapshotState::Stable
                && (self.before != self.after || self.items == 0))
            || (self.state == SnapshotState::Empty
                && (self.items != 0 || !self.formats.is_empty() || self.before != self.after))
            || (self.state == SnapshotState::Unstable && self.before == self.after)
            || (self.state == SnapshotState::FileSelection
                && (!self.formats.is_empty() || self.before != self.after || self.items == 0))
            || (self.state == SnapshotState::Suppressed
                && (self.items != 0 || !self.formats.is_empty() || self.before != self.after))
        {
            return Err(invalid());
        }
        let mut names = BTreeSet::new();
        let mut size = HEADER;
        for f in &self.formats {
            if f.item >= self.items
                || f.name.is_empty()
                || f.name.len() > 256
                || f.name.contains('\0')
                || !names.insert((f.item, &f.name))
                || (f.state != FormatState::Captured && !f.bytes.is_empty())
            {
                return Err(invalid());
            }
            size = size
                .checked_add(12 + f.name.len())
                .and_then(|n| n.checked_add(f.bytes.len()))
                .ok_or_else(invalid)?;
        }
        if size > NATIVE_LIMIT {
            return Err(invalid());
        }
        Ok(size)
    }

    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.validated_encoded_len().map(|_| ())
    }

    pub fn encode(&self) -> Result<Vec<u8>, ProtocolError> {
        self.validate()?;
        let mut b = Vec::new();
        b.extend_from_slice(b"CNS1");
        b.push(1);
        b.push(self.state as u8);
        b.extend_from_slice(&self.items.to_be_bytes());
        b.extend_from_slice(&self.before.to_be_bytes());
        b.extend_from_slice(&self.after.to_be_bytes());
        b.extend_from_slice(&(self.formats.len() as u16).to_be_bytes());
        b.extend_from_slice(&[0; 6]);
        for f in &self.formats {
            b.extend_from_slice(&f.item.to_be_bytes());
            b.push(f.state as u8);
            b.push(0);
            b.extend_from_slice(&(f.name.len() as u16).to_be_bytes());
            b.extend_from_slice(&[0; 2]);
            b.extend_from_slice(&(f.bytes.len() as u32).to_be_bytes());
            b.extend_from_slice(f.name.as_bytes());
            b.extend_from_slice(&f.bytes);
        }
        Ok(b)
    }
    pub fn decode(b: &[u8]) -> Result<Self, ProtocolError> {
        if b.len() < HEADER
            || b.len() > NATIVE_LIMIT
            || &b[..5] != b"CNS1\x01"
            || b[26..32] != [0; 6]
        {
            return Err(invalid());
        }
        let state = match b[5] {
            0 => SnapshotState::Stable,
            1 => SnapshotState::Empty,
            2 => SnapshotState::Unstable,
            3 => SnapshotState::Busy,
            4 => SnapshotState::Failed,
            5 => SnapshotState::FileSelection,
            6 => SnapshotState::Suppressed,
            _ => return Err(invalid()),
        };
        let read16 = |n| -> Result<u16, ProtocolError> {
            Ok(u16::from_be_bytes(
                b.get(n..n + 2)
                    .ok_or_else(invalid)?
                    .try_into()
                    .map_err(|_| invalid())?,
            ))
        };
        let count = read16(24)? as usize;
        if count > FORMAT_LIMIT {
            return Err(invalid());
        }
        let mut s = Self {
            state,
            items: read16(6)?,
            before: i64::from_be_bytes(b[8..16].try_into().map_err(|_| invalid())?),
            after: i64::from_be_bytes(b[16..24].try_into().map_err(|_| invalid())?),
            formats: Vec::with_capacity(count),
        };
        let mut offset = HEADER;
        for _ in 0..count {
            let d = b.get(offset..offset + 12).ok_or_else(invalid)?;
            if d[3] != 0 || d[6..8] != [0; 2] {
                return Err(invalid());
            }
            let state = match d[2] {
                0 => FormatState::Captured,
                1 => FormatState::Absent,
                2 => FormatState::Failed,
                3 => FormatState::TooLarge,
                _ => return Err(invalid()),
            };
            let name_len = u16::from_be_bytes([d[4], d[5]]) as usize;
            let len = u32::from_be_bytes(d[8..12].try_into().map_err(|_| invalid())?) as usize;
            let start = offset + 12;
            let end = start
                .checked_add(name_len)
                .and_then(|n| n.checked_add(len))
                .filter(|n| *n <= b.len())
                .ok_or_else(invalid)?;
            let name = std::str::from_utf8(&b[start..start + name_len])
                .map_err(|_| invalid())?
                .to_string();
            s.formats.push(NativeFormat {
                item: u16::from_be_bytes([d[0], d[1]]),
                name,
                state,
                bytes: b[start + name_len..end].to_vec(),
            });
            offset = end;
        }
        if offset != b.len() {
            return Err(invalid());
        }
        s.validate()?;
        Ok(s)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PublishState {
    Published = 0,
    Failed = 1,
    Partial = 2,
    Superseded = 3,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublishReceipt {
    pub state: PublishState,
    pub revision: i64,
    pub token: [u8; 16],
}
impl PublishReceipt {
    pub fn encode(self) -> Vec<u8> {
        let mut b = Vec::with_capacity(32);
        b.extend_from_slice(b"CWR1");
        b.push(self.state as u8);
        b.extend_from_slice(&[0; 3]);
        b.extend_from_slice(&self.revision.to_be_bytes());
        b.extend_from_slice(&self.token);
        b
    }
    pub fn decode(b: &[u8]) -> Result<Self, ProtocolError> {
        if b.len() != 32 || &b[..4] != b"CWR1" || b[5..8] != [0; 3] {
            return Err(invalid());
        }
        let state = match b[4] {
            0 => PublishState::Published,
            1 => PublishState::Failed,
            2 => PublishState::Partial,
            3 => PublishState::Superseded,
            _ => return Err(invalid()),
        };
        Ok(Self {
            state,
            revision: i64::from_be_bytes(b[8..16].try_into().map_err(|_| invalid())?),
            token: b[16..32].try_into().map_err(|_| invalid())?,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failures_and_item_boundaries_survive_round_trip() {
        let s = NativeSnapshot {
            before: 9,
            after: 9,
            items: 2,
            state: SnapshotState::Stable,
            formats: vec![
                NativeFormat {
                    item: 0,
                    name: "public.utf8-plain-text".into(),
                    state: FormatState::Captured,
                    bytes: b"a".to_vec(),
                },
                NativeFormat {
                    item: 1,
                    name: "public.png".into(),
                    state: FormatState::Failed,
                    bytes: vec![],
                },
            ],
        };
        let bytes = s.encode().unwrap();
        assert_eq!(NativeSnapshot::decode(&bytes).unwrap(), s);
        for i in 0..bytes.len() {
            assert!(NativeSnapshot::decode(&bytes[..i]).is_err());
        }
    }
    #[test]
    fn unstable_cannot_be_labelled_stable() {
        assert!(NativeSnapshot {
            before: 1,
            after: 2,
            items: 0,
            state: SnapshotState::Stable,
            formats: vec![]
        }
        .encode()
        .is_err());
    }
    #[test]
    fn receipt_has_actual_revision_and_token() {
        let r = PublishReceipt {
            state: PublishState::Partial,
            revision: 10,
            token: [4; 16],
        };
        assert_eq!(PublishReceipt::decode(&r.encode()).unwrap(), r);
    }
}
