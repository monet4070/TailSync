//! History content identity is separate from wire integrity and origin matching.
use super::{Completeness, Mime, PortableBundle};
use crate::protocol::native_clipboard::{NativeSnapshot, SnapshotState};
use crate::protocol::ProtocolError;

#[derive(
    Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
pub struct HistoryIdentity {
    #[schemars(range(min = 1, max = 1))]
    pub version: u8,
    pub capture_id: String,
    pub content_id: String,
    pub dedupe_key: Option<String>,
    #[schemars(range(min = 1, max = 9007199254740991i64))]
    pub archive_revision: i64,
    pub archive_digest: String,
}

fn field(h: &mut blake3::Hasher, bytes: &[u8]) {
    h.update(&(bytes.len() as u64).to_be_bytes());
    h.update(bytes);
}

pub fn legacy_native_id(snapshot: &NativeSnapshot, platform: u8) -> Result<String, ProtocolError> {
    native_content_id(snapshot, platform, false)
}
fn native_content_id(
    snapshot: &NativeSnapshot,
    platform: u8,
    exact: bool,
) -> Result<String, ProtocolError> {
    snapshot.validate()?;
    if snapshot.state != SnapshotState::Stable || platform > 2 {
        return Err(ProtocolError::InvalidComposite);
    }
    let mut h = blake3::Hasher::new_derive_key("TailSync.history.original.v1");
    h.update(&[platform, if exact { 1 } else { 2 }]);
    h.update(&snapshot.items.to_be_bytes());
    for item in 0..snapshot.items {
        let formats: Vec<_> = snapshot
            .formats
            .iter()
            .filter(|f| f.item == item && !crate::protocol::native_policy::control(&f.name))
            .collect();
        h.update(&item.to_be_bytes());
        h.update(&(formats.len() as u16).to_be_bytes());
        for f in formats {
            field(&mut h, f.name.as_bytes());
            h.update(&[f.state as u8]);
            field(&mut h, &f.bytes);
        }
    }
    let prefix = if exact { "hcn1" } else { "hcl1" };
    Ok(format!("{prefix}:{platform}:{}", h.finalize().to_hex()))
}
pub fn archive_content_id(
    archive: &crate::protocol::native_archive::NativeArchive,
) -> Result<String, ProtocolError> {
    archive.validate()?;
    native_content_id(&archive.original, archive.platform, true)
}
pub fn archive_dedupe_key(
    archive: &crate::protocol::native_archive::NativeArchive,
) -> Result<Option<String>, ProtocolError> {
    let content = archive_content_id(archive)?;
    if !archive.enumeration_complete || archive.platform == 0 {
        return Ok(None);
    }
    for item in 0..archive.original.items {
        let formats: Vec<_> = archive
            .original
            .formats
            .iter()
            .filter(|f| f.item == item && !crate::protocol::native_policy::control(&f.name))
            .collect();
        if formats.is_empty() {
            return Ok(None);
        }
        for f in formats {
            if f.state != crate::protocol::native_clipboard::FormatState::Captured {
                return Ok(None);
            }
            let mime = match f.name.as_str() {
                "public.utf8-plain-text" => Mime::Text,
                "public.png" => Mime::Png,
                "public.html" => Mime::Html,
                _ => return Ok(None),
            };
            let b = PortableBundle {
                representations: vec![super::Representation {
                    mime,
                    bytes: f.bytes.clone(),
                }],
                resources: vec![],
                bindings: vec![],
                completeness: Completeness::Complete,
            };
            if portable_dedupe_key(&b).ok().flatten().is_none() {
                return Ok(None);
            }
        }
    }
    let mut h = blake3::Hasher::new_derive_key("TailSync.history.dedupe.v1");
    field(&mut h, content.as_bytes());
    h.update(&0u16.to_be_bytes());
    Ok(Some(format!(
        "hnd1:{}:{}",
        archive.platform,
        h.finalize().to_hex()
    )))
}

/// Complete is necessary, but a caller's completeness label does not prove that
/// RTF/provider paths or external HTML dependencies have been persisted.
pub fn portable_content_id(bundle: &PortableBundle) -> Result<String, ProtocolError> {
    let mut h = blake3::Hasher::new_derive_key("TailSync.history.portable.v1");
    field(&mut h, &bundle.encode()?);
    Ok(format!("hcp1:{}", h.finalize().to_hex()))
}

pub fn portable_dedupe_key(bundle: &PortableBundle) -> Result<Option<String>, ProtocolError> {
    let content = portable_content_id(bundle)?;
    if bundle.completeness != Completeness::Complete || bundle.representation(Mime::Rtf).is_some() {
        return Ok(None);
    }
    // Recheck the actual markup: a received completeness flag does not prove
    // that all provider and resource dependencies are closed.
    if let Some(html) = bundle.representation(Mime::Html) {
        let text = std::str::from_utf8(html).map_err(|_| ProtocolError::InvalidComposite)?;
        // A deliberately narrow positive proof: only attribute-free formatting
        // tags are intrinsically closed. String searches for URLs cannot prove
        // closure through entities, CSS, objects or unfamiliar providers.
        let mut remaining = text;
        while let Some(start) = remaining.find('<') {
            let Some(end) = remaining[start..].find('>') else {
                return Ok(None);
            };
            let tag = remaining[start + 1..start + end]
                .trim()
                .trim_matches('/')
                .trim()
                .to_ascii_lowercase();
            if !matches!(
                tag.as_str(),
                "p" | "br"
                    | "div"
                    | "span"
                    | "b"
                    | "strong"
                    | "i"
                    | "em"
                    | "u"
                    | "s"
                    | "strike"
                    | "sub"
                    | "sup"
                    | "pre"
                    | "code"
                    | "blockquote"
                    | "ul"
                    | "ol"
                    | "li"
                    | "table"
                    | "thead"
                    | "tbody"
                    | "tr"
                    | "td"
                    | "th"
            ) {
                return Ok(None);
            }
            remaining = &remaining[start + end + 1..];
        }
    }
    let mut h = blake3::Hasher::new_derive_key("TailSync.history.portable-dedupe.v1");
    field(&mut h, content.as_bytes());
    Ok(Some(format!("hpd1:{}", h.finalize().to_hex())))
}

impl HistoryIdentity {
    pub fn same_capture(&self, other: &Self) -> bool {
        self.version == 1
            && other.version == 1
            && self.capture_id == other.capture_id
            && self.content_id == other.content_id
    }
    pub fn may_rebind(&self, other: &Self) -> bool {
        self.version == 1
            && other.version == 1
            && self.content_id == other.content_id
            && matches!((&self.dedupe_key, &other.dedupe_key), (Some(a), Some(b)) if !a.is_empty() && a == b)
    }
}
