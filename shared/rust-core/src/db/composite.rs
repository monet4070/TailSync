//! One encrypted manifest, related encrypted parts, and transactional network
//! identity tombstones. Parts share the existing private payload directory.
use super::entries::ExternalHistoryPayload;
use super::types::WithheldRestoreFormat;
use super::*;
use crate::composite::archive_sync::{ArchiveResourceUpdate, ArchiveUpdate};
use crate::composite::identity::{self, HistoryIdentity};
use crate::composite::{
    self, Completeness, Mime, PortableBundle, Representation, Resource, ResourceBinding,
};
use crate::protocol::native_archive::NativeArchive;
use crate::protocol::native_clipboard::{FormatState, NativeFormat, NativeSnapshot, SnapshotState};
pub(super) const ARCHIVE_LAYER_ROLE: &str = "archive_layer";
const NETWORK_ORIGINALS_ROLE: &str = "network_originals";
#[cfg(test)]
#[path = "archive_sync_tests.rs"]
mod archive_sync_tests;
#[cfg(test)]
#[path = "network_originals_tests.rs"]
mod network_originals_tests;
pub(super) const LOCAL_ARCHIVE_EVENT_SOURCE: &str = "tailsync-local-archive-v1";
const REMOTE_ARCHIVE_EVENT_PREFIX: &str = "archive:";
const ARCHIVE_SOURCE_IDENTITY_LIMIT: usize = 256;
const COMPOSITE_EVENT_METADATA_LIMIT: i64 = 4096;
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteArchiveOutcome {
    Applied(i64),
    Duplicate(i64),
    Superseded,
    BaseMissing,
}
struct ArchiveSupplement<'a> {
    source_info: Option<crate::composite::source_context::SourceProof>,
    originals: &'a [composite::network_originals::NetworkImageOriginal],
    receipt: Option<ArchiveReceipt<'a>>,
}
struct ArchiveReceipt<'a> {
    source_identity: &'a str,
    base_event: String,
    base_digest: String,
    event_id: String,
    previous_digest: Option<&'a str>,
    digest: String,
    history_id: i64,
    timestamp_ms: i64,
}
#[derive(Clone, Copy)]
enum NativeInput<'a> {
    Legacy(&'a NativeSnapshot),
    Archive(&'a NativeArchive),
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Part {
    id: u16,
    role: String,
    mime: u8,
    item: u16,
    name: String,
    hash: String,
    size: usize,
    width: Option<u32>,
    height: Option<u32>,
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Binding {
    representation: u16,
    resource: u16,
    offset: u32,
    length: u32,
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct ArchivedInfo {
    formats: Vec<String>,
    standalone_png: bool,
    resource_count: u16,
    representation_count: u16,
    image_count: u16,
    completeness: u8,
    preview_width: Option<u32>,
    preview_height: Option<u32>,
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Manifest {
    version: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    identity: Option<HistoryIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    integrity_method: Option<String>,
    #[serde(default)]
    native_only: bool,
    #[serde(default)]
    native_items: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    archive_version: Option<u8>,
    summary: String,
    completeness: u8,
    parts: Vec<Part>,
    bindings: Vec<Binding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    archived_info: Option<ArchivedInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_info: Option<crate::composite::source_context::SourceProof>,
}
fn archived_info(archived: &PortableBundle) -> ArchivedInfo {
    ArchivedInfo {
        formats: archived
            .representations
            .iter()
            .map(|representation| match representation.mime {
                Mime::Text => "text",
                Mime::Html => "html",
                Mime::Rtf => "rtf",
                Mime::Png => "png",
            })
            .chain(archived.resources.iter().map(|_| "png"))
            .map(str::to_string)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect(),
        standalone_png: archived.representation(Mime::Png).is_some(),
        resource_count: archived.resources.len() as u16,
        representation_count: archived.representations.len() as u16,
        image_count: (archived.resources.len()
            + usize::from(archived.representation(Mime::Png).is_some()))
            as u16,
        completeness: archived.completeness as u8,
        preview_width: archived
            .representation(Mime::Png)
            .or_else(|| {
                archived
                    .resources
                    .first()
                    .map(|resource| resource.png.as_slice())
            })
            .and_then(|bytes| bytes.get(16..20))
            .and_then(|bytes| bytes.try_into().ok())
            .map(u32::from_be_bytes),
        preview_height: archived
            .representation(Mime::Png)
            .or_else(|| {
                archived
                    .resources
                    .first()
                    .map(|resource| resource.png.as_slice())
            })
            .and_then(|bytes| bytes.get(20..24))
            .and_then(|bytes| bytes.try_into().ok())
            .map(u32::from_be_bytes),
    }
}
// Listing eligibility uses bounded, hash-checked HTML bytes on the read
// worker, without decoding images or changing a saved manifest/identity.
pub(super) struct PreparedCompositeMenu {
    html: reads::PayloadSource,
    bindings: Vec<ResourceBinding>,
    size: usize,
    archived: bool,
}
impl PreparedCompositeMenu {
    pub(super) fn declared_bytes(&self) -> usize {
        self.size
    }
    pub(super) fn apply(
        self,
        info: &mut CompositeInfo,
        cancellation: &crate::cancellation::Cancellation,
    ) -> Option<String> {
        let html = self
            .html
            .read(
                cancellation,
                if self.archived {
                    crate::protocol::native_clipboard::NATIVE_LIMIT as u64
                } else {
                    Mime::Html.limit() as u64
                },
            )
            .ok()
            .and_then(|b| {
                if self.archived {
                    let bundle = PortableBundle::decode(&b).ok()?;
                    let index = bundle
                        .representations
                        .iter()
                        .position(|representation| representation.mime == Mime::Html)?;
                    let bindings = bundle
                        .bindings
                        .iter()
                        .filter(|binding| binding.representation as usize == index)
                        .map(|binding| ResourceBinding {
                            representation: 0,
                            ..binding.clone()
                        })
                        .collect::<Vec<_>>();
                    return Some((bundle.representations[index].bytes.clone(), bindings));
                }
                Some((b, self.bindings))
            });
        let complete = html.as_ref().and_then(|(bytes, bindings)| {
            composite::html::restore_reference_inventory_complete(bytes, bindings).ok()
        }) == Some(true);
        if !complete {
            tracing::debug!(target: "tailsync.runtime.execution", operation = "history.restore_menu", outcome = "withheld", "HTML restore inventory is incomplete or unreadable");
            Self::withhold_html(info);
        }
        html.map(|(bytes, _)| composite::html::history_text_summary(&bytes))
            .filter(|text| !text.is_empty())
    }
    pub(super) fn withhold_html(info: &mut CompositeInfo) {
        if !info.native_only && info.formats.iter().any(|f| f == "html") {
            info.restorable_formats.retain(|f| f != "html");
            if !info.withheld_formats.iter().any(|f| f.format == "html") {
                info.withheld_formats.push(WithheldRestoreFormat {
                    format: "html".into(),
                    code: "resources_incomplete".into(),
                });
            }
        }
    }
    pub(super) fn apply_prepared(
        prepared: Result<Option<Self>, String>,
        info: &mut CompositeInfo,
        cancellation: &crate::cancellation::Cancellation,
    ) -> Option<String> {
        match prepared {
            Ok(Some(menu)) => menu.apply(info, cancellation),
            Err(_) => {
                tracing::debug!(target: "tailsync.runtime.execution", operation = "history.restore_menu", outcome = "unavailable", "HTML restore inventory unavailable");
                Self::withhold_html(info);
                None
            }
            Ok(None) => None,
        }
    }
}
#[derive(Clone)]
pub struct AnalysisHandle {
    pub entry_id: i64,
    pub identity: Option<HistoryIdentity>,
    pub authority: String,
    pub generation: u64,
    pub owner: String,
    pub revision: crate::db::HistoryReadRevision,
}

pub struct ContentAnalysisInput {
    pub bundle: PortableBundle,
    pub archive: Option<NativeArchive>,
    pub native_snapshot: Option<NativeSnapshot>,
}

struct PreparedAnalysisPart {
    role: String,
    mime: u8,
    item: u16,
    name: String,
    hash: String,
    size: usize,
    source: reads::PayloadSource,
}

pub struct PreparedContentAnalysis {
    handle: AnalysisHandle,
    native_only: bool,
    data_hash: String,
    completeness: u8,
    bindings: Vec<Binding>,
    version: u8,
    source_info: Option<crate::composite::source_context::SourceProof>,
    native_items: u16,
    parts: Vec<PreparedAnalysisPart>,
}
fn manifest(stored: &[u8]) -> Result<Manifest, Box<dyn std::error::Error>> {
    let decoded = crypto::decrypt(stored)?;
    decoded_manifest(&decoded)
}
fn decoded_manifest(decoded: &[u8]) -> Result<Manifest, Box<dyn std::error::Error>> {
    if decoded.len() > 8 * 1024 * 1024 {
        return Err("Composite manifest exceeds limit".into());
    }
    let m: Manifest = serde_json::from_slice(decoded)?;
    if !matches!(m.version, 1..=5) || m.parts.len() > 100 {
        return Err("Invalid composite manifest".into());
    }
    validate_manifest(&m)?;
    Ok(m)
}
fn validate_manifest(m: &Manifest) -> Result<(), Box<dyn std::error::Error>> {
    let layers = m
        .parts
        .iter()
        .filter(|part| part.role == ARCHIVE_LAYER_ROLE)
        .count();
    let originals: Vec<_> = m
        .parts
        .iter()
        .filter(|part| part.role == NETWORK_ORIGINALS_ROLE)
        .collect();
    if (matches!(m.version, 3..=5) && (layers != 1 || m.archived_info.is_none() || m.native_only))
        || (!matches!(m.version, 3..=5) && (layers != 0 || m.archived_info.is_some()))
        || (m.version == 4 && m.source_info.is_none())
        || (m.version < 4 && m.source_info.is_some())
        || (m.version == 5 && originals.len() != 1)
        || (m.version != 5 && !originals.is_empty())
        || originals.iter().any(|part| {
            part.name != "NIO1"
                || part.mime != 0
                || part.item != 0
                || part.size <= 8
                || part.size
                    > composite::resource_archive::ARCHIVE_TOTAL_BYTES
                        + 8
                        + composite::resource_archive::ARCHIVE_RESOURCE_LIMIT * 12
        })
    {
        return Err("Invalid resource archive layer version".into());
    }
    if let Some(source) = &m.source_info {
        if crate::composite::source_context::canonical_origin(&source.origin)
            .ok()
            .as_deref()
            != Some(source.origin.as_str())
            || source.nodes.is_empty()
            || source.nodes.len() > crate::composite::source_context::SOURCE_NODE_LIMIT
        {
            return Err("Invalid source context proof".into());
        }
    }
    let archives = m.parts.iter().filter(|p| p.role == "archive").count();
    if (m.archive_version.is_some()
        && (!matches!(m.version, 2..=5)
            || m.archive_version != Some(1)
            || archives != 1
            || m.parts.iter().any(|p| p.role == "native")))
        || (m.archive_version.is_none() && archives != 0)
    {
        return Err("Invalid archive role/version".into());
    }
    if matches!(m.version, 2..=5) {
        let identity = m.identity.as_ref().ok_or("Missing composite identity")?;
        if identity.version != 1
            || identity.capture_id.len() != 32
            || identity.archive_revision < 1
            || m.integrity_method.as_deref()
                != Some(if m.version == 5 && m.source_info.is_some() {
                    "portable-cbp1+resource-archive+source-context+network-originals"
                } else if m.version == 5 {
                    "portable-cbp1+resource-archive+network-originals"
                } else if m.version == 4 {
                    "portable-cbp1+resource-archive+source-context"
                } else if m.version == 3 {
                    "portable-cbp1+resource-archive"
                } else if m.native_only {
                    "native-cns1"
                } else {
                    "portable-cbp1"
                })
            || archive_digest(m)? != identity.archive_digest
        {
            return Err("Invalid composite archive identity".into());
        }
    } else if m.identity.is_some() || m.integrity_method.is_some() {
        return Err("Unexpected identity in legacy manifest".into());
    }
    Ok(())
}
fn archive_digest(m: &Manifest) -> Result<String, Box<dyn std::error::Error>> {
    let mut h = blake3::Hasher::new_derive_key("TailSync.history.archive.v1");
    // Struct field order is stable across serde_json's optional preserve_order
    // feature. Hashing a generic Value map would differ between Core and Tauri
    // builds that unify different JSON features.
    let mut view = m.clone();
    if let Some(identity) = &mut view.identity {
        identity.archive_digest.clear();
    }
    h.update(&serde_json::to_vec(&view)?);
    Ok(h.finalize().to_hex().to_string())
}
pub(super) fn summary_bytes(decoded: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let m: Manifest = serde_json::from_slice(decoded)?;
    if !matches!(m.version, 1..=5) {
        return Err("Invalid composite manifest".into());
    }
    validate_manifest(&m)?;
    // Older portable/Windows and Qt image captures stored the generic label.
    // A complete, single-item archive may also have a standalone PNG projection
    // alongside app metadata. Show its saved dimensions without rewriting the
    // archive identity or decoding full images on every history listing.
    let summary = if m.summary == "Rich clipboard content"
        && !m.native_only
        && (m.archive_version.is_none()
            || (m.native_items == 1
                && m.completeness == Completeness::Complete as u8
                && m.archived_info.is_none()))
    {
        let representations: Vec<_> = m
            .parts
            .iter()
            .filter(|p| p.role == "representation")
            .collect();
        if representations.len() == 1
            && representations[0].mime == Mime::Png as u8
            && !m.parts.iter().any(|p| p.role == "resource")
            && m.bindings.is_empty()
        {
            match (representations[0].width, representations[0].height) {
                (Some(width), Some(height)) if width > 0 && height > 0 => {
                    format!("image/png · {width}×{height} · 1 frame")
                }
                _ => m.summary,
            }
        } else {
            m.summary
        }
    } else {
        m.summary
    };
    Ok(summary.into_bytes())
}
pub(super) fn info_bytes(decoded: &[u8]) -> Result<CompositeInfo, Box<dyn std::error::Error>> {
    let m: Manifest = serde_json::from_slice(decoded)?;
    if !matches!(m.version, 1..=5) || m.parts.len() > 100 {
        return Err("Invalid composite manifest".into());
    }
    validate_manifest(&m)?;
    let base_formats: Vec<String> = m
        .parts
        .iter()
        .filter(|p| p.role == "representation" || p.role == "resource")
        .filter_map(|p| Mime::from_byte(p.mime).ok())
        .map(|mime| {
            match mime {
                Mime::Text => "text",
                Mime::Html => "html",
                Mime::Rtf => "rtf",
                Mime::Png => "png",
            }
            .to_string()
        })
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let formats = m
        .archived_info
        .as_ref()
        .map(|info| info.formats.clone())
        .unwrap_or(base_formats);
    let completeness = m
        .archived_info
        .as_ref()
        .map(|info| info.completeness)
        .unwrap_or(m.completeness);
    // Shared cheap eligibility facts. HTML closure is refined by the owned read
    // worker; actual publication adds image decoding and target-specific admission.
    let base_standalone_png = m
        .parts
        .iter()
        .any(|p| p.role == "representation" && p.mime == Mime::Png as u8);
    let standalone_png = m
        .archived_info
        .as_ref()
        .map(|info| info.standalone_png)
        .unwrap_or(base_standalone_png);
    let resource_count = m
        .archived_info
        .as_ref()
        .map(|info| info.resource_count as usize)
        .unwrap_or_else(|| m.parts.iter().filter(|p| p.role == "resource").count());
    let inventory = composite::restore::RestoreFormatInventory {
        native_only: m.native_only,
        completeness: match completeness {
            0 => Completeness::Complete,
            1 => Completeness::ExternalReferences,
            _ => Completeness::MissingFormats,
        },
        standalone_png,
        resource_count,
    };
    let mut restorable_formats = Vec::new();
    let mut withheld_formats = Vec::new();
    for format in &formats {
        let mime = match format.as_str() {
            "text" => Mime::Text,
            "html" => Mime::Html,
            "rtf" => Mime::Rtf,
            _ => Mime::Png,
        };
        if let Some(code) = inventory.withheld_reason(mime) {
            withheld_formats.push(WithheldRestoreFormat {
                format: format.clone(),
                code: code.to_string(),
            });
        } else {
            restorable_formats.push(format.clone());
        }
    }
    Ok(CompositeInfo {
        identity: m.identity,
        native_only: m.native_only,
        native_item_count: m.native_items,
        formats,
        restorable_formats,
        withheld_formats,
        representation_count: m
            .archived_info
            .as_ref()
            .map(|info| info.representation_count)
            .unwrap_or_else(|| {
                m.parts
                    .iter()
                    .filter(|p| p.role == "representation")
                    .count() as u16
            }),
        image_count: m
            .archived_info
            .as_ref()
            .map(|info| info.image_count)
            .unwrap_or_else(|| {
                m.parts
                    .iter()
                    .filter(|p| {
                        p.role == "resource"
                            || p.role == "representation" && p.mime == Mime::Png as u8
                    })
                    .count() as u16
            }),
        completeness: match completeness {
            0 => "complete",
            1 => "external_references",
            2 => "missing_formats",
            _ => return Err("Invalid composite state".into()),
        }
        .into(),
    })
}
fn identity_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<HistoryIdentity> {
    Ok(HistoryIdentity {
        version: row.get(0)?,
        capture_id: row.get(1)?,
        content_id: row.get(2)?,
        dedupe_key: row.get(3)?,
        archive_revision: row.get(4)?,
        archive_digest: row.get(5)?,
    })
}
pub(super) fn insert_identity(
    conn: &Connection,
    id: i64,
    identity: &HistoryIdentity,
) -> Result<(), rusqlite::Error> {
    conn.execute("INSERT INTO history_composite_identity(history_id,version,capture_id,content_id,dedupe_key,archive_revision,archive_digest) VALUES (?1,?2,?3,?4,?5,?6,?7)", params![id,identity.version,identity.capture_id,identity.content_id,identity.dedupe_key,identity.archive_revision,identity.archive_digest])?;
    Ok(())
}
fn ensure_native_capture_allowed(
    snapshot: &NativeSnapshot,
) -> Result<(), Box<dyn std::error::Error>> {
    if snapshot.state == crate::protocol::native_clipboard::SnapshotState::Suppressed
        || snapshot
            .formats
            .iter()
            .any(|f| crate::protocol::native_policy::suppressed(&f.name))
    {
        return Err("Clipboard capture suppressed by metadata".into());
    }
    Ok(())
}
fn image_summary(archive: &NativeArchive) -> Option<String> {
    use crate::protocol::image;
    let formats: Vec<_> = archive
        .original
        .formats
        .iter()
        .filter(|f| !crate::protocol::native_policy::control(&f.name))
        .collect();
    if archive.original.items != 1
        || formats
            .iter()
            .any(|f| !image::is_image_format_name(&f.name))
    {
        return None;
    }
    let m = formats
        .iter()
        .filter(|f| f.state == FormatState::Captured)
        .filter_map(|f| image::metadata(&f.name, &f.bytes))
        .max_by_key(|m| (m.animation, m.frames))?;
    let cover = archive.derived.iter().any(|d| {
        d.name == "public.png"
            && matches!(
                d.converter.as_str(),
                crate::protocol::native_archive::CONVERTER_STATIC_COVER
                    | crate::protocol::native_archive::CONVERTER_TIFF_TO_PNG
            )
    });
    Some(format!(
        "{} · {}×{} · {} frame{}{}",
        m.mime,
        m.width,
        m.height,
        m.frames,
        if m.frames == 1 { "" } else { "s" },
        if cover {
            " · static PNG cover"
        } else if !m.within_budget() {
            " · cover budget exceeded"
        } else {
            ""
        }
    ))
}
fn portable_image_summary(bundle: &PortableBundle) -> Option<String> {
    if bundle.representations.len() != 1
        || !bundle.resources.is_empty()
        || !bundle.bindings.is_empty()
    {
        return None;
    }
    let png = bundle.representation(Mime::Png)?;
    let metadata = crate::protocol::image::metadata("public.png", png)?;
    Some(format!(
        "{} · {}×{} · {} frame{}",
        metadata.mime,
        metadata.width,
        metadata.height,
        metadata.frames,
        if metadata.frames == 1 { "" } else { "s" }
    ))
}
fn native_summary(snapshot: &NativeSnapshot) -> PortableBundle {
    let text = snapshot
        .formats
        .iter()
        .filter(|f| f.state == FormatState::Captured && f.name == "public.utf8-plain-text")
        .filter_map(|f| std::str::from_utf8(&f.bytes).ok())
        .collect::<Vec<_>>()
        .join("\n");
    let text = if text.is_empty() {
        format!("Native clipboard ({} items)", snapshot.items)
    } else {
        text
    };
    let mut end = text.len().min(Mime::Text.limit());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    PortableBundle {
        representations: vec![Representation {
            mime: Mime::Text,
            bytes: text.as_bytes()[..end].to_vec(),
        }],
        resources: vec![],
        bindings: vec![],
        completeness: Completeness::MissingFormats,
    }
}
impl HistoryDB {
    pub fn composite_identity(
        &self,
        id: i64,
    ) -> Result<Option<HistoryIdentity>, Box<dyn std::error::Error>> {
        let stored: Vec<u8> = self.conn.query_row(
            "SELECT data FROM history WHERE id=?1 AND type='composite'",
            [id],
            |row| row.get(0),
        )?;
        let m = manifest(&stored)?;
        self.verify_composite_identity(id, &m)?;
        Ok(m.identity)
    }
    fn verify_composite_identity(
        &self,
        id: i64,
        m: &Manifest,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let persisted = if schema::identity_schema_present(&self.conn)? {
            self.conn.query_row("SELECT version,capture_id,content_id,dedupe_key,archive_revision,archive_digest FROM history_composite_identity WHERE history_id=?1", [id], identity_row).optional()?
        } else {
            None
        };
        if persisted != m.identity {
            return Err("Composite identity relation mismatch".into());
        }
        Ok(())
    }
    /// Bounded metadata commits for future preview/archival workers. The held
    /// token is checked transactionally, so late jobs cannot revive deleted rows.
    /// Payload/resource replacement is a separate operation (O04).
    pub fn update_composite_archive_summary(
        &mut self,
        id: i64,
        expected: &HistoryIdentity,
        summary: &str,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        if summary.len() > Mime::Text.limit() {
            return Err("Composite summary exceeds limit".into());
        }
        let stored: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT data FROM history WHERE id=?1 AND type='composite'",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        let Some(stored) = stored else {
            return Ok(false);
        };
        let mut m = manifest(&stored)?;
        self.verify_composite_identity(id, &m)?;
        if m.identity.as_ref() != Some(expected) {
            return Ok(false);
        }
        let Some(revision) = expected
            .archive_revision
            .checked_add(1)
            .filter(|v| *v <= 9007199254740991)
        else {
            return Ok(false);
        };
        m.summary = summary.into();
        m.identity
            .as_mut()
            .ok_or("Composite manifest has no identity")?
            .archive_revision = revision;
        let digest = archive_digest(&m)?;
        m.identity
            .as_mut()
            .ok_or("Composite manifest has no identity")?
            .archive_digest = digest.clone();
        let encrypted = crypto::encrypt(&serde_json::to_vec(&m)?)?;
        let tx = self.conn.transaction()?;
        let changed = tx.execute("UPDATE history_composite_identity SET archive_revision=?1,archive_digest=?2 WHERE history_id=?3 AND capture_id=?4 AND content_id=?5 AND archive_revision=?6 AND archive_digest=?7", params![revision,digest,id,expected.capture_id,expected.content_id,expected.archive_revision,expected.archive_digest])?;
        if changed != 1 {
            return Ok(false);
        }
        tx.execute(
            "UPDATE history SET data=?1,classifier_version=0 WHERE id=?2 AND type='composite'",
            params![encrypted, id],
        )?;
        tx.commit()?;
        Ok(true)
    }
    pub fn commit_composite_archive(
        &mut self,
        id: i64,
        expected: &HistoryIdentity,
        archived: &PortableBundle,
    ) -> Result<Option<HistoryIdentity>, Box<dyn std::error::Error>> {
        self.commit_composite_archive_with_cancellation(
            id,
            expected,
            archived,
            &crate::cancellation::Cancellation::default(),
        )
    }

    pub fn commit_composite_archive_with_cancellation(
        &mut self,
        id: i64,
        expected: &HistoryIdentity,
        archived: &PortableBundle,
        cancellation: &crate::cancellation::Cancellation,
    ) -> Result<Option<HistoryIdentity>, Box<dyn std::error::Error>> {
        self.commit_composite_layer_with_cancellation(
            id,
            expected,
            archived,
            ArchiveSupplement {
                source_info: None,
                originals: &[],
                receipt: None,
            },
            cancellation,
            None,
        )
    }

    pub fn commit_composite_archive_with_originals(
        &mut self,
        id: i64,
        expected: &HistoryIdentity,
        archived: &PortableBundle,
        originals: &[composite::network_originals::NetworkImageOriginal],
        cancellation: &crate::cancellation::Cancellation,
    ) -> Result<Option<HistoryIdentity>, Box<dyn std::error::Error>> {
        self.commit_composite_layer_with_cancellation(
            id,
            expected,
            archived,
            ArchiveSupplement {
                source_info: None,
                originals,
                receipt: None,
            },
            cancellation,
            None,
        )
    }

    pub fn commit_composite_source_archive_with_cancellation(
        &mut self,
        id: i64,
        expected: &HistoryIdentity,
        archived: &PortableBundle,
        source: &crate::composite::source_context::SourceProof,
        cancellation: &crate::cancellation::Cancellation,
    ) -> Result<Option<HistoryIdentity>, Box<dyn std::error::Error>> {
        self.commit_composite_source_archive_until(
            id,
            expected,
            archived,
            source,
            cancellation,
            None,
        )
    }

    pub fn commit_composite_source_archive_until(
        &mut self,
        id: i64,
        expected: &HistoryIdentity,
        archived: &PortableBundle,
        source: &crate::composite::source_context::SourceProof,
        cancellation: &crate::cancellation::Cancellation,
        expires_at: Option<std::time::Instant>,
    ) -> Result<Option<HistoryIdentity>, Box<dyn std::error::Error>> {
        self.commit_composite_layer_with_cancellation(
            id,
            expected,
            archived,
            ArchiveSupplement {
                source_info: Some(source.clone()),
                originals: &[],
                receipt: None,
            },
            cancellation,
            expires_at,
        )
    }

    fn commit_composite_layer_with_cancellation(
        &mut self,
        id: i64,
        expected: &HistoryIdentity,
        archived: &PortableBundle,
        supplement: ArchiveSupplement<'_>,
        cancellation: &crate::cancellation::Cancellation,
        expires_at: Option<std::time::Instant>,
    ) -> Result<Option<HistoryIdentity>, Box<dyn std::error::Error>> {
        let ArchiveSupplement {
            source_info,
            originals,
            receipt,
        } = supplement;
        cancellation.check()?;
        composite::validate_storable(archived)?;
        let stored: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT data FROM history WHERE id=?1 AND type='composite'",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        let Some(stored) = stored else {
            return Ok(None);
        };
        let mut m = manifest(&stored)?;
        self.verify_composite_identity(id, &m)?;
        if m.identity.as_ref() != Some(expected) {
            return Ok(None);
        }
        if m.native_only {
            return Err("Native-only archive cannot derive a portable document".into());
        }
        let base = self.get_composite(id)?;
        let previous = self.get_composite_archived(id)?;
        let current = previous.as_ref().unwrap_or(&base);
        let mut retained_originals = self.read_network_image_originals(id, &m)?;
        composite::network_originals::validate_originals(archived, originals)?;
        for original in originals {
            if let Some(retained) = retained_originals
                .iter()
                .find(|retained| retained.image_index == original.image_index)
            {
                if retained != original {
                    return Err("Network original cannot be replaced".into());
                }
            } else {
                retained_originals.push(original.clone());
            }
        }
        retained_originals.sort_by_key(|original| original.image_index);
        composite::network_originals::validate_originals(archived, &retained_originals)?;
        let originals_encoded = if retained_originals.is_empty() {
            None
        } else {
            Some(composite::network_originals::encode_originals(
                &retained_originals,
            )?)
        };
        match &source_info {
            Some(proof) => crate::composite::source_context::validate_source_derivation(
                current, archived, proof,
            )?,
            None => composite::resource_archive::validate_archive_derivation(current, archived)?,
        }
        let source_info = match (m.source_info.take(), source_info) {
            (Some(mut previous), Some(next)) => {
                if previous.origin != next.origin
                    || next
                        .nodes
                        .iter()
                        .any(|node| previous.nodes.iter().any(|old| old.index == node.index))
                {
                    return Err(crate::protocol::ProtocolError::InvalidComposite.into());
                }
                previous.nodes.extend(next.nodes);
                previous.nodes.sort_by_key(|node| node.index);
                Some(previous)
            }
            (previous, next) => next.or(previous),
        };
        if let Some(proof) = &source_info {
            crate::composite::source_context::validate_source_derivation(&base, archived, proof)?;
        } else {
            composite::resource_archive::validate_archive_derivation(&base, archived)?;
        }
        let is_source = source_info.is_some();
        if m.parts
            .iter()
            .filter(|part| part.role != ARCHIVE_LAYER_ROLE && part.role != NETWORK_ORIGINALS_ROLE)
            .count()
            + 1
            + usize::from(originals_encoded.is_some())
            > 100
        {
            return Err("Composite manifest part limit reached".into());
        }
        let Some(revision) = expected
            .archive_revision
            .checked_add(1)
            .filter(|v| *v <= 9007199254740991)
        else {
            return Ok(None);
        };
        let encoded = archived.encode()?;
        if encoded.len() > crate::protocol::native_clipboard::NATIVE_LIMIT {
            return Err("Composite archive layer exceeds limit".into());
        }
        let hash = blake3::hash(&encoded).to_hex().to_string();
        let mut replaced_ids = Vec::new();
        let mut replaced_payloads = Vec::new();
        for part in m
            .parts
            .iter()
            .filter(|part| part.role == ARCHIVE_LAYER_ROLE || part.role == NETWORK_ORIGINALS_ROLE)
        {
            replaced_ids.push(part.id);
        }
        for part_id in &replaced_ids {
            let stored: Vec<u8> = self.conn.query_row(
                "SELECT stored FROM history_parts WHERE history_id=?1 AND part_id=?2",
                params![id, part_id],
                |r| r.get(0),
            )?;
            if let Some(reference) = decode_image_reference(&stored) {
                replaced_payloads.push(ExternalHistoryPayload {
                    path: resolve_file_reference_at(&self.image_history_dir, &reference)?,
                });
            }
        }
        let external = encoded.len() > 64 * 1024 && self.storage_available;
        if self.storage_available {
            let original_reservation = originals_encoded
                .as_ref()
                .map_or(0, |bytes| bytes.len() as u64 + 28);
            self.check_archive_storage_reservation(
                encoded.len() as u64
                    + 28
                    + original_reservation
                    + if originals_encoded.is_some() { 4096 } else { 0 },
            )?;
        }
        let mut written = Vec::new();
        let stored_bytes = if external {
            let reference = persist_image_at(&self.image_history_dir, &hash, &encoded)?;
            let path = resolve_file_reference_at(
                &self.image_history_dir,
                &decode_image_reference(&reference).ok_or("Invalid composite reference")?,
            )?;
            written.push(ExternalHistoryPayload { path });
            reference
        } else {
            crypto::encrypt(&encoded)?
        };
        let mut pending = vec![self.arm_pending_payloads(&written)];
        let part_id = m
            .parts
            .iter()
            .map(|p| p.id)
            .max()
            .map(|id| id.checked_add(1).ok_or("Composite part ID exhausted"))
            .transpose()?
            .unwrap_or(0);
        m.parts
            .retain(|part| part.role != ARCHIVE_LAYER_ROLE && part.role != NETWORK_ORIGINALS_ROLE);
        m.parts.push(Part {
            id: part_id,
            role: ARCHIVE_LAYER_ROLE.into(),
            mime: 0,
            item: 0,
            name: if is_source { "O05" } else { "O04" }.into(),
            hash: hash.clone(),
            size: encoded.len(),
            width: None,
            height: None,
        });
        let original_part = if let Some(bytes) = originals_encoded {
            let original_part_id = part_id
                .checked_add(1)
                .ok_or("Composite part ID exhausted")?;
            let original_hash = blake3::hash(&bytes).to_hex().to_string();
            let original_stored = if bytes.len() > 64 * 1024 && self.storage_available {
                let reference = persist_image_at(&self.image_history_dir, &original_hash, &bytes)?;
                let path = resolve_file_reference_at(
                    &self.image_history_dir,
                    &decode_image_reference(&reference).ok_or("Invalid composite reference")?,
                )?;
                pending.push(self.arm_pending_payloads(&[ExternalHistoryPayload { path }]));
                let verified = self
                    .prepare_text(reference.clone())?
                    .read(cancellation, bytes.len() as u64)?;
                if verified.len() != bytes.len()
                    || blake3::hash(&verified).to_hex().as_str() != original_hash
                {
                    return Err("Network original payload digest mismatch".into());
                }
                reference
            } else {
                crypto::encrypt(&bytes)?
            };
            let original_part = Part {
                id: original_part_id,
                role: NETWORK_ORIGINALS_ROLE.into(),
                mime: 0,
                item: 0,
                name: "NIO1".into(),
                hash: original_hash,
                size: bytes.len(),
                width: None,
                height: None,
            };
            m.parts.push(original_part.clone());
            Some((original_part, original_stored))
        } else {
            None
        };
        m.version = if original_part.is_some() {
            5
        } else if is_source {
            4
        } else {
            3
        };
        m.integrity_method = Some(if original_part.is_some() && is_source {
            "portable-cbp1+resource-archive+source-context+network-originals".into()
        } else if original_part.is_some() {
            "portable-cbp1+resource-archive+network-originals".into()
        } else if is_source {
            "portable-cbp1+resource-archive+source-context".into()
        } else {
            "portable-cbp1+resource-archive".into()
        });
        m.archived_info = Some(archived_info(archived));
        m.source_info = source_info;
        {
            let identity = m
                .identity
                .as_mut()
                .ok_or("Composite manifest has no identity")?;
            identity.archive_revision = revision;
            identity.archive_digest.clear();
        }
        let digest = archive_digest(&m)?;
        m.identity
            .as_mut()
            .ok_or("Composite manifest has no identity")?
            .archive_digest = digest.clone();
        let encrypted = crypto::encrypt(&serde_json::to_vec(&m)?)?;
        let write = (|| -> Result<(), Box<dyn std::error::Error>> {
            cancellation.check()?;
            if expires_at.is_some_and(|deadline| std::time::Instant::now() >= deadline) {
                return Err("source_grant_expired".into());
            }
            let tx = self.conn.transaction()?;
            for old in &replaced_ids {
                tx.execute(
                    "DELETE FROM history_parts WHERE history_id=?1 AND part_id=?2",
                    params![id, old],
                )?;
            }
            tx.execute("INSERT OR REPLACE INTO history_parts(history_id,part_id,kind,mime,data_hash,size_bytes,stored) VALUES (?1,?2,?3,?4,?5,?6,?7)", params![id, part_id, ARCHIVE_LAYER_ROLE, "0", hash, encoded.len() as i64, stored_bytes])?;
            if let Some((part, bytes)) = &original_part {
                tx.execute("INSERT INTO history_parts(history_id,part_id,kind,mime,data_hash,size_bytes,stored) VALUES (?1,?2,?3,?4,?5,?6,?7)", params![id, part.id, NETWORK_ORIGINALS_ROLE, "0", part.hash, part.size as i64, bytes])?;
            }
            let changed = tx.execute("UPDATE history_composite_identity SET archive_revision=?1,archive_digest=?2 WHERE history_id=?3 AND capture_id=?4 AND content_id=?5 AND archive_revision=?6 AND archive_digest=?7 AND version=?8 AND dedupe_key IS ?9", params![revision, digest, id, expected.capture_id, expected.content_id, expected.archive_revision, expected.archive_digest, expected.version, expected.dedupe_key])?;
            if changed != 1 {
                return Err("Composite archive identity mismatch".into());
            }
            let changed = tx.execute(
                "UPDATE history SET data=?1, size_bytes=?2,classifier_version=0 WHERE id=?3 AND type='composite' AND data=?4",
                params![
                    encrypted,
                    m.parts.iter().map(|part| part.size as u64).sum::<u64>() as i64,
                    id,
                    stored
                ],
            )?;
            if changed != 1 {
                return Err("Composite archive manifest mismatch".into());
            }
            if let Some(receipt) = &receipt {
                verify_remote_archive_base(
                    &tx,
                    receipt.source_identity,
                    &receipt.base_event,
                    &receipt.base_digest,
                    id,
                )?;
                write_archive_tracker(
                    &tx,
                    receipt.source_identity,
                    &receipt.event_id,
                    receipt.previous_digest,
                    &receipt.digest,
                    receipt.history_id,
                    receipt.timestamp_ms,
                )?;
                verify_remote_archive_base(
                    &tx,
                    receipt.source_identity,
                    &receipt.base_event,
                    &receipt.base_digest,
                    id,
                )?;
            }
            cancellation.check()?;
            tx.commit()?;
            Ok(())
        })();
        write?;
        for guard in pending {
            guard.commit();
        }
        self.cleanup_external_payloads(&replaced_payloads, None);
        Ok(m.identity)
    }

    pub fn get_composite_archived(
        &self,
        id: i64,
    ) -> Result<Option<PortableBundle>, Box<dyn std::error::Error>> {
        let stored: Vec<u8> = self.conn.query_row(
            "SELECT data FROM history WHERE id=?1 AND type='composite'",
            [id],
            |r| r.get(0),
        )?;
        let m = manifest(&stored)?;
        self.verify_composite_identity(id, &m)?;
        let Some(part) = m.parts.iter().find(|p| p.role == ARCHIVE_LAYER_ROLE) else {
            return Ok(None);
        };
        let bundle = PortableBundle::decode(&self.read_composite_part(id, part)?)?;
        let base = self.get_composite(id)?;
        match (m.version, m.source_info.as_ref()) {
            (4 | 5, Some(proof)) => {
                crate::composite::source_context::validate_source_derivation(&base, &bundle, proof)?
            }
            _ => composite::resource_archive::validate_archive_derivation(&base, &bundle)?,
        }
        Ok(Some(bundle))
    }

    fn read_network_image_originals(
        &self,
        id: i64,
        manifest: &Manifest,
    ) -> Result<Vec<composite::network_originals::NetworkImageOriginal>, Box<dyn std::error::Error>>
    {
        let Some(part) = manifest
            .parts
            .iter()
            .find(|part| part.role == NETWORK_ORIGINALS_ROLE)
        else {
            return Ok(Vec::new());
        };
        Ok(composite::network_originals::decode_originals(
            &self.read_composite_part(id, part)?,
        )?)
    }

    pub fn get_composite_network_originals(
        &self,
        id: i64,
    ) -> Result<Vec<composite::network_originals::NetworkImageOriginal>, Box<dyn std::error::Error>>
    {
        let stored: Vec<u8> = self.conn.query_row(
            "SELECT data FROM history WHERE id=?1 AND type='composite'",
            [id],
            |row| row.get(0),
        )?;
        let manifest = manifest(&stored)?;
        self.verify_composite_identity(id, &manifest)?;
        let originals = self.read_network_image_originals(id, &manifest)?;
        if !originals.is_empty() {
            let archived = self
                .get_composite_archived(id)?
                .ok_or("Network originals have no derived archive")?;
            composite::network_originals::validate_originals(&archived, &originals)?;
        }
        Ok(originals)
    }

    pub fn bind_composite_outgoing_event(
        &mut self,
        id: i64,
        expected: &HistoryIdentity,
        event_id: crate::protocol::MessageId,
        base_digest: [u8; 32],
        timestamp_ms: i64,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        if event_id.0 == [0u8; 16] {
            return Err("Composite outgoing event id must not be zero".into());
        }
        let stored: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT data FROM history WHERE id=?1 AND type='composite'",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        let Some(stored) = stored else {
            return Ok(false);
        };
        let m = manifest(&stored)?;
        self.verify_composite_identity(id, &m)?;
        if m.identity.as_ref() != Some(expected) {
            return Ok(false);
        }
        let received: bool = self.conn.query_row("SELECT EXISTS(SELECT 1 FROM history_composite_events WHERE history_id=?1 AND source_device_id<>?2)", params![id, LOCAL_ARCHIVE_EVENT_SOURCE], |row| row.get(0))?;
        if received {
            return Ok(false);
        }
        let base = self.get_composite(id)?;
        let stored_digest = base.digest()?;
        let digest = hex::encode(stored_digest);
        if stored_digest != base_digest {
            return Err("Composite outgoing base digest mismatch".into());
        }
        let event = event_id.as_hex();
        if let Some((bound, history)) = self
            .conn
            .query_row(
                "SELECT digest, history_id FROM history_composite_events WHERE source_device_id=?1 AND event_id=?2",
                params![LOCAL_ARCHIVE_EVENT_SOURCE, event],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<i64>>(1)?)),
            )
            .optional()?
        {
            if bound == digest && history == Some(id) {
                return Ok(true);
            }
            return Err("Composite outgoing event is already bound".into());
        }
        self.prune_composite_tombstones()?;
        self.conn.execute(
            "INSERT INTO history_composite_events(source_device_id,event_id,digest,history_id,timestamp_ms) VALUES (?1,?2,?3,?4,?5)",
            params![LOCAL_ARCHIVE_EVENT_SOURCE, event, digest, id, timestamp_ms],
        )?;
        Ok(true)
    }

    pub fn export_composite_archive_update(
        &self,
        id: i64,
        expected: &HistoryIdentity,
    ) -> Result<Option<ArchiveUpdate>, Box<dyn std::error::Error>> {
        let stored: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT data FROM history WHERE id=?1 AND type='composite'",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        let Some(stored) = stored else {
            return Ok(None);
        };
        let m = manifest(&stored)?;
        self.verify_composite_identity(id, &m)?;
        if m.identity.as_ref() != Some(expected) {
            return Ok(None);
        }
        let binding: Option<(String, String)> = self
            .conn
            .query_row(
                "SELECT event_id, digest FROM history_composite_events WHERE source_device_id=?1 AND history_id=?2 ORDER BY timestamp_ms DESC, event_id DESC LIMIT 1",
                params![LOCAL_ARCHIVE_EVENT_SOURCE, id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((event_hex, digest_hex)) = binding else {
            return Ok(None);
        };
        let base = self.get_composite(id)?;
        let base_digest = base.digest()?;
        if hex::encode(base_digest) != digest_hex {
            return Err("Composite archive binding digest mismatch".into());
        }
        // Images already present in the frozen first event are not updates.
        // Later archive commits only supplement nodes that were unbound there.
        let external = composite::resource_archive::external_images(&base)?;
        let originals = self
            .read_network_image_originals(id, &m)?
            .into_iter()
            .filter(|original| {
                external
                    .iter()
                    .any(|image| image.image_index == original.image_index as usize)
            })
            .collect::<Vec<_>>();
        if originals.is_empty() {
            return Ok(None);
        }
        let event_bytes = hex::decode(&event_hex)?;
        let base_event = crate::protocol::MessageId(
            <[u8; 16]>::try_from(event_bytes.as_slice())
                .map_err(|_| "Invalid composite archive event id")?,
        );
        let identity = m
            .identity
            .as_ref()
            .ok_or("Composite manifest has no identity")?;
        let revision = u64::try_from(identity.archive_revision)?;
        if !(2..=9007199254740991).contains(&revision) {
            return Err("Composite archive revision is not exportable".into());
        }
        let update = ArchiveUpdate {
            base_event,
            base_digest,
            revision,
            images: originals
                .iter()
                .map(|original| ArchiveResourceUpdate {
                    image_index: original.image_index,
                    format: original.format,
                    bytes: original.bytes.clone(),
                })
                .collect(),
        };
        update.derive(&base)?;
        Ok(Some(update))
    }

    pub fn commit_remote_composite_archive_update(
        &mut self,
        source_identity: &str,
        update: &ArchiveUpdate,
        cancellation: &crate::cancellation::Cancellation,
    ) -> Result<RemoteArchiveOutcome, Box<dyn std::error::Error>> {
        cancellation.check()?;
        if source_identity.is_empty()
            || source_identity.len() > ARCHIVE_SOURCE_IDENTITY_LIMIT
            || source_identity == LOCAL_ARCHIVE_EVENT_SOURCE
        {
            return Err("Invalid composite archive source identity".into());
        }
        if update.base_event.0 == [0u8; 16] {
            return Err("Invalid composite archive base event".into());
        }
        if !(2..=9007199254740991).contains(&update.revision) {
            return Err("Invalid composite archive revision".into());
        }
        let base_event_hex = update.base_event.as_hex();
        let base_row: Option<(Option<i64>, String)> = self
            .conn
            .query_row(
                "SELECT history_id, digest FROM history_composite_events WHERE source_device_id=?1 AND event_id=?2",
                params![source_identity, base_event_hex],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((history_id, stored_digest)) = base_row else {
            return Ok(RemoteArchiveOutcome::BaseMissing);
        };
        let Some(history_id) = history_id else {
            return Ok(RemoteArchiveOutcome::Superseded);
        };
        if stored_digest != hex::encode(update.base_digest) {
            return Err("Composite archive base digest mismatch".into());
        }
        let stored: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT data FROM history WHERE id=?1 AND type='composite'",
                [history_id],
                |r| r.get(0),
            )
            .optional()?;
        let Some(stored) = stored else {
            return Ok(RemoteArchiveOutcome::Superseded);
        };
        let m = manifest(&stored)?;
        self.verify_composite_identity(history_id, &m)?;
        let identity = m
            .identity
            .clone()
            .ok_or("Composite manifest has no identity")?;
        let base = self.get_composite(history_id)?;
        if base.digest()? != update.base_digest {
            return Err("Composite archive base content mismatch".into());
        }
        let encoded = update.encode()?;
        let payload_digest = format!("{}:{}", update.revision, blake3::hash(&encoded).to_hex());
        let tracker_event = format!("{REMOTE_ARCHIVE_EVENT_PREFIX}{base_event_hex}");
        let tracker: Option<String> = self
            .conn
            .query_row(
                "SELECT digest FROM history_composite_events WHERE source_device_id=?1 AND event_id=?2",
                params![source_identity, tracker_event],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(previous) = &tracker {
            let (revision_text, _) = previous
                .split_once(':')
                .ok_or("Composite archive tracker digest is invalid")?;
            let previous_revision: u64 = revision_text
                .parse()
                .map_err(|_| "Composite archive tracker digest is invalid")?;
            if previous_revision > update.revision {
                return Ok(RemoteArchiveOutcome::Superseded);
            }
            if previous_revision == update.revision {
                if previous == &payload_digest {
                    let originals = self.get_composite_network_originals(history_id)?;
                    for image in &update.images {
                        if !originals.iter().any(|original| {
                            original.image_index == image.image_index
                                && original.format == image.format
                                && original.bytes == image.bytes
                        }) {
                            return Err("Composite archive saved original is unavailable".into());
                        }
                    }
                    cancellation.check()?;
                    return Ok(RemoteArchiveOutcome::Duplicate(history_id));
                }
                return Err("Composite archive revision collision".into());
            }
        } else {
            self.prune_composite_tombstones()?;
        }
        let (derived, incoming) = update.derive(&base)?;
        let previous = self.get_composite_archived(history_id)?;
        let current = previous.as_ref().unwrap_or(&base);
        let retained = self.read_network_image_originals(history_id, &m)?;
        let mut replacements = Vec::new();
        let mut pending: Vec<(u16, composite::network_image::NetworkImageFormat, Vec<u8>)> =
            Vec::new();
        for original in &incoming {
            let index = original.image_index;
            let derived_png = image_resource_png(&derived, index)?
                .ok_or("Composite archive position is not bound")?;
            match image_resource_png(current, index)? {
                Some(bound) => {
                    if bound != derived_png {
                        return Err("Composite archive position conflict".into());
                    }
                    if let Some(existing) = retained
                        .iter()
                        .find(|existing| existing.image_index == index)
                    {
                        if existing.bytes != original.bytes || existing.format != original.format {
                            return Err("Composite archive original conflict".into());
                        }
                    } else {
                        pending.push((index, original.format, original.bytes.clone()));
                    }
                }
                None => {
                    let span = image_source_span(current, index)?;
                    replacements.push(composite::resource_archive::ResourceReplacement {
                        span,
                        png: derived_png.to_vec(),
                    });
                    pending.push((index, original.format, original.bytes.clone()));
                }
            }
        }
        let timestamp_ms = crate::protocol::unix_timestamp_ms();
        if pending.is_empty() {
            let write = (|| -> Result<(), Box<dyn std::error::Error>> {
                cancellation.check()?;
                let tx = self.conn.transaction()?;
                verify_archive_history(&tx, history_id, &identity, &stored)?;
                verify_remote_archive_base(
                    &tx,
                    source_identity,
                    &base_event_hex,
                    &stored_digest,
                    history_id,
                )?;
                write_archive_tracker(
                    &tx,
                    source_identity,
                    &tracker_event,
                    tracker.as_deref(),
                    &payload_digest,
                    history_id,
                    timestamp_ms,
                )?;
                verify_archive_history(&tx, history_id, &identity, &stored)?;
                verify_remote_archive_base(
                    &tx,
                    source_identity,
                    &base_event_hex,
                    &stored_digest,
                    history_id,
                )?;
                cancellation.check()?;
                tx.commit()?;
                Ok(())
            })();
            write?;
            return Ok(RemoteArchiveOutcome::Duplicate(history_id));
        }
        let merged = if replacements.is_empty() {
            current.clone()
        } else {
            composite::resource_archive::apply_resource_bindings(current, &replacements)
                .map_err(|_| "Composite archive merge failed")?
                .0
        };
        let mut originals = Vec::with_capacity(pending.len());
        for (index, format, bytes) in pending {
            let resource_id = image_resource_id(&merged, index)?
                .ok_or("Composite archive position is not bound")?;
            originals.push(composite::network_originals::NetworkImageOriginal {
                image_index: index,
                resource_id,
                format,
                bytes,
            });
        }
        let receipt = ArchiveReceipt {
            source_identity,
            base_event: base_event_hex,
            base_digest: stored_digest,
            event_id: tracker_event,
            previous_digest: tracker.as_deref(),
            digest: payload_digest,
            history_id,
            timestamp_ms,
        };
        let committed = self.commit_composite_layer_with_cancellation(
            history_id,
            &identity,
            &merged,
            ArchiveSupplement {
                source_info: None,
                originals: &originals,
                receipt: Some(receipt),
            },
            cancellation,
            None,
        )?;
        Ok(match committed {
            Some(_) => RemoteArchiveOutcome::Applied(history_id),
            None => RemoteArchiveOutcome::Superseded,
        })
    }

    pub fn add_composite(
        &mut self,
        bundle: &PortableBundle,
        native: Option<&NativeSnapshot>,
        source: &str,
    ) -> Result<i64, Box<dyn std::error::Error>> {
        self.add_composite_event(bundle, native, source, None)
    }
    /// A retry returns the original row or a deletion tombstone. A changed
    /// digest under an authenticated event identity is rejected.
    pub fn add_composite_event(
        &mut self,
        bundle: &PortableBundle,
        native: Option<&NativeSnapshot>,
        source: &str,
        event: Option<(&str, crate::protocol::MessageId, i64)>,
    ) -> Result<i64, Box<dyn std::error::Error>> {
        self.add_composite_inner(
            bundle,
            native.map(NativeInput::Legacy),
            source,
            event,
            false,
            false,
            &[],
        )
    }
    /// Store and protect a remote event until its effect reaches a durable result.
    pub fn prepare_composite_event(
        &mut self,
        bundle: &PortableBundle,
        source: &str,
        event: (&str, crate::protocol::MessageId, i64),
    ) -> Result<i64, Box<dyn std::error::Error>> {
        self.add_composite_inner(bundle, None, source, Some(event), false, true, &[])
    }
    pub fn add_native_snapshot(
        &mut self,
        snapshot: &NativeSnapshot,
        source: &str,
    ) -> Result<i64, Box<dyn std::error::Error>> {
        snapshot.validate()?;
        ensure_native_capture_allowed(snapshot)?;
        let bundle = native_summary(snapshot);
        self.add_composite_inner(
            &bundle,
            Some(NativeInput::Legacy(snapshot)),
            source,
            None,
            true,
            false,
            &[],
        )
    }
    pub fn add_native_archive(
        &mut self,
        archive: &NativeArchive,
        source: &str,
    ) -> Result<i64, Box<dyn std::error::Error>> {
        archive.validate()?;
        ensure_native_capture_allowed(&archive.original)?;
        if archive.original.state != crate::protocol::native_clipboard::SnapshotState::Stable {
            return Err("Archive is not a stable capture".into());
        }
        if !schema::identity_schema_present(&self.conn)? {
            return Err("Exact archive requires history schema 13".into());
        }
        let (bundle, native_only) = match composite::from_archive(archive) {
            Ok(bundle) => (bundle, false),
            Err(
                composite::CaptureError::NativeOnly | composite::CaptureError::UnsupportedItems,
            ) => (native_summary(&archive.original), true),
            Err(error) => return Err(format!("Archive capture refused: {error:?}").into()),
        };
        self.add_composite_inner(
            &bundle,
            Some(NativeInput::Archive(archive)),
            source,
            None,
            native_only,
            false,
            &[],
        )
    }
    /// Freeze the network-enriched portable event while retaining exact CNA1
    /// source bytes and downloaded originals in the same history transaction.
    pub fn add_native_archive_with_images(
        &mut self,
        archive: &NativeArchive,
        images: &composite::resource_archive::ArchiveOutcome,
        source: &str,
    ) -> Result<i64, Box<dyn std::error::Error>> {
        let base = composite::from_archive(archive).map_err(|_| "Invalid source archive")?;
        self.add_enriched_capture(&base, NativeInput::Archive(archive), images, source)
    }

    pub fn add_native_capture_with_images(
        &mut self,
        snapshot: &NativeSnapshot,
        images: &composite::resource_archive::ArchiveOutcome,
        source: &str,
    ) -> Result<i64, Box<dyn std::error::Error>> {
        let base =
            composite::from_native_capture(snapshot).map_err(|_| "Invalid source capture")?;
        self.add_enriched_capture(&base, NativeInput::Legacy(snapshot), images, source)
    }

    fn add_enriched_capture(
        &mut self,
        base: &PortableBundle,
        native: NativeInput<'_>,
        images: &composite::resource_archive::ArchiveOutcome,
        source: &str,
    ) -> Result<i64, Box<dyn std::error::Error>> {
        if !schema::identity_schema_present(&self.conn)?
            || base.completeness == Completeness::MissingFormats
            || images.originals.is_empty()
        {
            return Err("Network capture requires a complete source and identity schema".into());
        }
        composite::resource_archive::validate_archive_derivation(base, &images.bundle)?;
        composite::network_originals::validate_originals(&images.bundle, &images.originals)?;
        self.add_composite_inner(
            &images.bundle,
            Some(native),
            source,
            None,
            false,
            false,
            &images.originals,
        )
    }
    pub fn get_composite_archive(
        &self,
        id: i64,
    ) -> Result<Option<NativeArchive>, Box<dyn std::error::Error>> {
        let stored: Vec<u8> = self.conn.query_row(
            "SELECT data FROM history WHERE id=?1 AND type='composite'",
            [id],
            |r| r.get(0),
        )?;
        let m = manifest(&stored)?;
        self.verify_composite_identity(id, &m)?;
        let parts: Vec<_> = m.parts.iter().filter(|p| p.role == "archive").collect();
        if m.archive_version.is_none() && parts.is_empty() {
            return Ok(None);
        }
        if m.archive_version != Some(1) || parts.len() != 1 || !matches!(m.version, 2..=5) {
            return Err("Invalid archive role/version".into());
        }
        let archive = NativeArchive::decode(&self.read_composite_part(id, parts[0])?)?;
        let identity = m.identity.as_ref().ok_or("Archive identity missing")?;
        if identity::archive_content_id(&archive)? != identity.content_id
            || identity::archive_dedupe_key(&archive)? != identity.dedupe_key
        {
            return Err("Original archive identity mismatch".into());
        }
        Ok(Some(archive))
    }
    // These values are one atomic capture, including the optional network parts.
    #[allow(clippy::too_many_arguments)]
    fn add_composite_inner(
        &mut self,
        bundle: &PortableBundle,
        input: Option<NativeInput<'_>>,
        source: &str,
        event: Option<(&str, crate::protocol::MessageId, i64)>,
        native_only: bool,
        protected: bool,
        network_originals: &[composite::network_originals::NetworkImageOriginal],
    ) -> Result<i64, Box<dyn std::error::Error>> {
        let archive = match input {
            Some(NativeInput::Archive(a)) => Some(a),
            _ => None,
        };
        let native = match input {
            Some(NativeInput::Legacy(s)) => Some(s),
            Some(NativeInput::Archive(a)) => Some(&a.original),
            None => None,
        };
        if let Some(snapshot) = native {
            ensure_native_capture_allowed(snapshot)?;
        }
        // Storage uses the structural gate only: native preparation is a
        // publish-time concern (R2-02).
        composite::validate_storable(bundle)?;
        let digest = hex::encode(if native_only {
            composite::native_digest(native.ok_or("Native snapshot missing")?)?
        } else {
            bundle.digest()?
        });
        if let Some((identity, id, _)) = event {
            if let Some((old,history))=self.conn.query_row("SELECT digest, history_id FROM history_composite_events WHERE source_device_id=?1 AND event_id=?2",params![identity,id.as_hex()],|r|Ok((r.get::<_,String>(0)?,r.get::<_,Option<i64>>(1)?))).optional()?{if old!=digest{return Err("Composite event identity collision".into());}let id = history.unwrap_or(0); if protected { self.protect_history(id); } return Ok(id);}
        }
        if event.is_some() {
            self.prune_composite_tombstones()?;
        }
        let has_identity = schema::identity_schema_present(&self.conn)?;
        // CNS1 is a legacy projection. It cannot prove exhaustive originals or
        // dependency closure, so native captures never delete an older row.
        let dedupe_key = if let Some(archive) = archive {
            identity::archive_dedupe_key(archive)?
        } else if native.is_none() {
            identity::portable_dedupe_key(bundle)?
        } else {
            None
        };
        let candidates = if has_identity {
            let mut query = self.conn.prepare("SELECT h.id FROM history h JOIN history_composite_identity i ON i.history_id=h.id WHERE h.type='composite' AND i.version=1 AND i.dedupe_key=?1")?;
            let ids = query
                .query_map([dedupe_key.as_deref()], |row| row.get(0))?
                .collect::<Result<Vec<i64>, _>>()?;
            ids
        } else {
            vec![]
        };
        let duplicate_ids = self.unfavorited_duplicate_ids(&candidates)?;
        let old = self.external_payloads_for_ids(&duplicate_ids)?;
        let mut m = Manifest {
            version: if has_identity { 2 } else { 1 },
            archived_info: None,
            source_info: None,
            identity: None,
            integrity_method: has_identity.then(|| {
                if native_only {
                    "native-cns1"
                } else {
                    "portable-cbp1"
                }
                .into()
            }),
            native_only,
            native_items: native.map_or(0, |s| s.items),
            archive_version: archive.map(|_| 1),
            summary: archive
                .and_then(image_summary)
                .or_else(|| {
                    (archive.is_none()
                        || (!native_only
                            && native.is_some_and(|s| s.items == 1)
                            && bundle.completeness == Completeness::Complete))
                        .then(|| portable_image_summary(bundle))
                        .flatten()
                })
                .or_else(|| {
                    bundle
                        .representation(Mime::Html)
                        .map(composite::html::history_text_summary)
                        .filter(|s| !s.is_empty())
                        .or_else(|| {
                            bundle
                                .text()
                                .map(str::trim)
                                .filter(|s| !s.is_empty())
                                .map(str::to_string)
                        })
                })
                .unwrap_or_else(|| "Rich clipboard content".into()),
            completeness: bundle.completeness as u8,
            parts: vec![],
            bindings: bundle
                .bindings
                .iter()
                .map(|b| Binding {
                    representation: b.representation,
                    resource: b.resource,
                    offset: b.offset,
                    length: b.length,
                })
                .collect(),
        };
        let mut inputs: Vec<(String, u8, u16, String, Vec<u8>)> = bundle
            .representations
            .iter()
            .map(|r| {
                (
                    "representation".into(),
                    r.mime as u8,
                    0,
                    String::new(),
                    r.bytes.clone(),
                )
            })
            .collect();
        if !network_originals.is_empty() {
            m.version = 5;
            m.integrity_method = Some("portable-cbp1+resource-archive+network-originals".into());
            m.archived_info = Some(archived_info(bundle));
            inputs.push((
                ARCHIVE_LAYER_ROLE.into(),
                0,
                0,
                "O04".into(),
                bundle.encode()?,
            ));
            inputs.push((
                NETWORK_ORIGINALS_ROLE.into(),
                0,
                0,
                "NIO1".into(),
                composite::network_originals::encode_originals(network_originals)?,
            ));
        }
        inputs.extend(bundle.resources.iter().map(|r| {
            (
                "resource".into(),
                Mime::Png as u8,
                r.id,
                String::new(),
                r.png.clone(),
            )
        }));
        if let Some(archive) = archive {
            inputs.push(("archive".into(), 0, 0, "CNA1".into(), archive.encode()?));
        }
        if let Some(native) = native.filter(|_| archive.is_none()) {
            native.validate()?;
            inputs.extend(
                native
                    .formats
                    .iter()
                    .filter(|f| {
                        f.state == FormatState::Captured
                            && crate::protocol::native_policy::legacy_storable(&f.name)
                    })
                    .map(|f| ("native".into(), 0, f.item, f.name.clone(), f.bytes.clone())),
            );
        }
        if native_only && !inputs.iter().any(|i| i.0 == "native" || i.0 == "archive") {
            return Err("Native capture has no storable source bytes".into());
        }
        if native_only
            && archive.is_none()
            && native.is_some_and(|snapshot| {
                snapshot.formats.iter().any(|format| {
                    format.state == FormatState::Captured
                        && !crate::protocol::native_policy::control(&format.name)
                        && !inputs.iter().any(|part| {
                            part.0 == "native" && part.2 == format.item && part.3 == format.name
                        })
                })
            })
        {
            return Err("Native capture contains source bytes outside the storage policy".into());
        }
        if inputs.len() > 100 {
            return Err("Too many composite history parts".into());
        }
        let logical: usize = inputs
            .iter()
            .try_fold(0usize, |n, i| n.checked_add(i.4.len()))
            .ok_or("Composite size overflow")?;
        if logical > 72 * 1024 * 1024 {
            return Err("Composite history exceeds limit".into());
        }
        // Reserve only new physical files; identical PNG/native parts share a
        // hash path. Inline manifests are included in conservative overhead.
        if self.storage_available {
            let inline_reserve = (serde_json::to_vec(&m.summary)?.len() + 4096 + inputs.len() * 512)
                as u64
                + inputs
                    .iter()
                    .filter(|i| i.4.len() <= 64 * 1024)
                    .map(|i| i.4.len() as u64 + 28)
                    .sum::<u64>();
            let external = inputs
                .iter()
                .filter(|i| i.4.len() > 64 * 1024)
                .map(|i| {
                    (
                        blake3::hash(&i.4).to_hex().to_string(),
                        i.4.len() as u64 + 28,
                    )
                })
                .collect::<std::collections::BTreeMap<_, _>>();
            loop {
                let missing = || {
                    external
                        .iter()
                        .filter(|(hash, _)| {
                            !self.image_history_dir.join(format!("{hash}.bin")).exists()
                        })
                        .map(|(_, size)| *size)
                        .sum::<u64>()
                };
                let before = missing();
                self.reserve_for_file_batch(inline_reserve.saturating_add(before), 0)?;
                // Quota cleanup may have removed the last reference to a
                // formerly shared part. Reserve its new physical copy too.
                let after = external
                    .iter()
                    .filter(|(hash, _)| {
                        !self.image_history_dir.join(format!("{hash}.bin")).exists()
                    })
                    .map(|(_, size)| *size)
                    .sum::<u64>();
                if after <= before {
                    break;
                }
            }
        }
        let mut stored_parts = Vec::new();
        let mut written = Vec::new();
        let prepare = (|| -> Result<(), Box<dyn std::error::Error>> {
            for (index, (role, mime, item, name, bytes)) in inputs.into_iter().enumerate() {
                let hash = blake3::hash(&bytes).to_hex().to_string();
                let stored = if bytes.len() > 64 * 1024 && self.storage_available {
                    let reference = persist_image_at(&self.image_history_dir, &hash, &bytes)?;
                    let path = resolve_file_reference_at(
                        &self.image_history_dir,
                        &decode_image_reference(&reference).ok_or("Invalid composite reference")?,
                    )?;
                    written.push(ExternalHistoryPayload { path });
                    reference
                } else {
                    crypto::encrypt(&bytes)?
                };
                m.parts.push(Part {
                    id: index as u16,
                    role,
                    mime,
                    item,
                    name,
                    hash,
                    size: bytes.len(),
                    width: (mime == Mime::Png as u8)
                        .then(|| u32::from_be_bytes(bytes[16..20].try_into().unwrap_or([0; 4]))),
                    height: (mime == Mime::Png as u8)
                        .then(|| u32::from_be_bytes(bytes[20..24].try_into().unwrap_or([0; 4]))),
                });
                stored_parts.push(stored);
            }
            Ok(())
        })();
        if let Err(e) = prepare {
            self.cleanup_external_payloads(&written, None);
            return Err(e);
        }
        // R5-02: from here the freshly written payloads are owned by a guard, so any
        // early return cleans them up. The manifest encryption below used to return
        // through `?` and leak every payload file it had just written.
        let pending = self.arm_pending_payloads(&written);
        if has_identity {
            let content_id = if let Some(archive) = archive {
                identity::archive_content_id(archive)?
            } else if let Some(native) = native {
                let mut retained = native.clone();
                retained.formats.retain(|format| {
                    m.parts.iter().any(|part| {
                        part.role == "native"
                            && part.item == format.item
                            && part.name == format.name
                    })
                });
                let platform = if cfg!(target_os = "macos") {
                    1
                } else if cfg!(target_os = "windows") {
                    2
                } else {
                    0
                };
                identity::legacy_native_id(&retained, platform)?
            } else {
                identity::portable_content_id(bundle)?
            };
            m.identity = Some(HistoryIdentity {
                version: 1,
                capture_id: hex::encode(rand::random::<[u8; 16]>()),
                content_id,
                dedupe_key,
                archive_revision: 1,
                archive_digest: String::new(),
            });
            let digest = archive_digest(&m)?;
            m.identity
                .as_mut()
                .ok_or("Composite manifest has no identity")?
                .archive_digest = digest;
        }
        let encrypted = crypto::encrypt(&serde_json::to_vec(&m)?)?;
        let native_item_facts: Vec<composite::content_analysis::NativeItemFact> = native
            .map(|snapshot| {
                let mut items: Vec<(u16, Vec<String>)> = Vec::new();
                for format in &snapshot.formats {
                    match items.iter_mut().find(|(item, _)| *item == format.item) {
                        Some((_, formats)) => formats.push(format.name.clone()),
                        None => items.push((format.item, vec![format.name.clone()])),
                    }
                }
                items
                    .into_iter()
                    .map(
                        |(item, formats)| composite::content_analysis::NativeItemFact {
                            item,
                            formats,
                        },
                    )
                    .collect()
            })
            .unwrap_or_default();
        let has_image = bundle.representation(Mime::Png).is_some() || !bundle.resources.is_empty();
        let mut classification = if native_only
            || bundle
                .representations
                .iter()
                .any(|representation| matches!(representation.mime, Mime::Html | Mime::Rtf))
        {
            composite::display::unknown_classification(has_image)
        } else {
            composite::display::classify_bundle(bundle, native_only, native_item_facts)
        };
        if classification.kind == composite::display::DisplayKind::Unknown {
            classification.version = 0;
        }
        let category = if has_image { "image" } else { "text" };
        let encoded_categories = classification.categories_json();
        let write = (|| -> Result<i64, Box<dyn std::error::Error>> {
            // Read the capability before the transaction borrows `self.conn`.
            let has_composite_schema = self.has_composite_schema();
            let tx = self.conn.transaction()?;
            Self::delete_rows_in_transaction(&tx, &duplicate_ids, has_composite_schema)?;
            tx.execute("INSERT INTO history (timestamp,type,description,data,size_bytes,source_peer,data_hash,category,categories,category_confidence,classifier_version) VALUES (?1,'composite','Encrypted composite',?2,?3,?4,?5,?6,?7,?8,?9)",params![chrono::Utc::now().to_rfc3339(),encrypted,logical as i64,source,digest,category,encoded_categories,classification.confidence,classification.version])?;
            let id = tx.last_insert_rowid();
            for (part, stored) in m.parts.iter().zip(stored_parts.iter()) {
                tx.execute("INSERT INTO history_parts(history_id,part_id,kind,mime,data_hash,size_bytes,stored) VALUES (?1,?2,?3,?4,?5,?6,?7)",params![id,part.id,part.role,part.mime.to_string(),part.hash,part.size as i64,stored])?;
            }
            if let Some(identity) = &m.identity {
                insert_identity(&tx, id, identity)?;
            }
            if let Some((identity, event_id, timestamp)) = event {
                tx.execute("INSERT INTO history_composite_events(source_device_id,event_id,digest,history_id,timestamp_ms) VALUES (?1,?2,?3,?4,?5)",params![identity,event_id.as_hex(),digest,id,timestamp])?;
            }
            tx.commit()?;
            Ok(id)
        })();
        match write {
            Ok(id) => {
                // Disarm before touching anything else: the rows are committed, so
                // these payloads are live and must survive.
                pending.commit();
                if protected {
                    self.protect_history(id);
                }
                self.cleanup_external_payloads(&old, None);
                self.trim(category)?;
                Ok(id)
            }
            // A failed or rolled-back transaction leaves the live set unchanged, so
            // the guard removes exactly the payloads nothing references.
            Err(e) => Err(e),
        }
    }
    pub fn get_composite(&self, id: i64) -> Result<PortableBundle, Box<dyn std::error::Error>> {
        let stored: Vec<u8> = self.conn.query_row(
            "SELECT data FROM history WHERE id=?1 AND type='composite'",
            [id],
            |r| r.get(0),
        )?;
        let m = manifest(&stored)?;
        self.verify_composite_identity(id, &m)?;
        let mut bundle = PortableBundle {
            representations: vec![],
            resources: vec![],
            bindings: m
                .bindings
                .iter()
                .map(|b| ResourceBinding {
                    representation: b.representation,
                    resource: b.resource,
                    offset: b.offset,
                    length: b.length,
                })
                .collect(),
            completeness: match m.completeness {
                0 => Completeness::Complete,
                1 => Completeness::ExternalReferences,
                2 => Completeness::MissingFormats,
                _ => return Err("Invalid completeness".into()),
            },
        };
        for p in m.parts.iter().filter(|part| {
            part.role != "native"
                && part.role != "archive"
                && part.role != ARCHIVE_LAYER_ROLE
                && part.role != NETWORK_ORIGINALS_ROLE
        }) {
            let bytes = self.read_composite_part(id, p)?;
            match p.role.as_str() {
                "representation" => bundle.representations.push(Representation {
                    mime: Mime::from_byte(p.mime)?,
                    bytes,
                }),
                "resource" => bundle.resources.push(Resource {
                    id: p.item,
                    png: bytes,
                }),
                _ => return Err("Invalid part role".into()),
            }
        }
        // Read path: structural validity is what makes the content recoverable.
        composite::validate_storable(&bundle)?;
        let hash: String =
            self.conn
                .query_row("SELECT data_hash FROM history WHERE id=?1", [id], |r| {
                    r.get(0)
                })?;
        if !m.native_only && hex::encode(bundle.digest()?) != hash {
            return Err("Composite history digest mismatch".into());
        }
        if let Some(identity) = &m.identity {
            if identity.content_id.starts_with("hcp1:")
                && identity::portable_content_id(&bundle)? != identity.content_id
            {
                return Err("Portable content identity mismatch".into());
            }
        }
        Ok(bundle)
    }
    pub(super) fn prepare_composite_menu(
        &self,
        id: i64,
        decoded: &[u8],
    ) -> Result<Option<PreparedCompositeMenu>, Box<dyn std::error::Error>> {
        let m = decoded_manifest(decoded)?;
        if m.native_only {
            return Ok(None);
        }
        if let Some(part) = m.parts.iter().find(|p| p.role == ARCHIVE_LAYER_ROLE) {
            self.verify_composite_identity(id, &m)?;
            let stored: Vec<u8> = self.conn.query_row(
                "SELECT stored FROM history_parts WHERE history_id=?1 AND part_id=?2",
                params![id, part.id],
                |row| row.get(0),
            )?;
            let source = self.prepare_text(stored)?;
            return Ok(Some(PreparedCompositeMenu {
                size: part.size,
                html: reads::PayloadSource::Checked(Box::new(source), part.hash.clone(), part.size),
                bindings: Vec::new(),
                archived: true,
            }));
        }
        let reps = m
            .parts
            .iter()
            .filter(|p| p.role == "representation")
            .collect::<Vec<_>>();
        let Some((index, part)) = reps
            .iter()
            .enumerate()
            .find(|(_, p)| p.mime == Mime::Html as u8)
        else {
            return Ok(None);
        };
        if part.size > Mime::Html.limit() {
            return Err("HTML listing payload exceeds limit".into());
        }
        let stored: Vec<u8> = self.conn.query_row(
            "SELECT stored FROM history_parts WHERE history_id=?1 AND part_id=?2",
            params![id, part.id],
            |r| r.get(0),
        )?;
        let source = self.prepare_text(stored)?;
        let bindings = m
            .bindings
            .iter()
            .filter(|b| {
                b.representation as usize == index
                    && m.parts
                        .iter()
                        .any(|p| p.role == "resource" && p.item == b.resource)
            })
            .map(|b| ResourceBinding {
                representation: 0,
                resource: b.resource,
                offset: b.offset,
                length: b.length,
            })
            .collect();
        Ok(Some(PreparedCompositeMenu {
            archived: false,
            html: reads::PayloadSource::Checked(Box::new(source), part.hash.clone(), part.size),
            bindings,
            size: part.size,
        }))
    }
    fn read_composite_part(
        &self,
        id: i64,
        p: &Part,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        if p.size > crate::protocol::native_clipboard::NATIVE_LIMIT {
            return Err("Composite part exceeds limit".into());
        }
        let stored: Vec<u8> = self.conn.query_row(
            "SELECT stored FROM history_parts WHERE history_id=?1 AND part_id=?2",
            params![id, p.id],
            |r| r.get(0),
        )?;
        let bytes = self
            .prepare_text(stored)?
            .read(&crate::cancellation::Cancellation::default(), p.size as u64)?;
        if bytes.len() != p.size || blake3::hash(&bytes).to_hex().as_str() != p.hash {
            return Err("Composite part digest mismatch".into());
        }
        Ok(bytes)
    }
    pub fn get_composite_image(&self, id: i64) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let _memory = composite::budgets::global().reserve_pipeline(192 * 1024 * 1024)?;
        let prepared = self
            .prepare_content_analysis(id)?
            .ok_or("Composite has no image")?;
        prepared.read_first_image(&crate::cancellation::Cancellation::default())
    }
    pub fn prepare_composite_image(&self, id: i64) -> Result<PreparedPreview, PreviewError> {
        let prepared = self
            .prepare_content_analysis(id)
            .map_err(|error| PreviewError::PayloadUnavailable {
                entry_id: id,
                reason: error.to_string(),
            })?
            .ok_or_else(|| PreviewError::PayloadUnavailable {
                entry_id: id,
                reason: "Composite has no image".into(),
            })?;
        Ok(PreparedPreview {
            metadata: PreviewMetadata {
                entry_id: id,
                kind: PreviewKind::Image,
                name: "composite image".into(),
                size_bytes: 0,
                batch: None,
            },
            source: reads::PayloadSource::CompositeFirstImage(Box::new(prepared)),
            data_hash: String::new(),
        })
    }
    pub fn composite_native_only(&self, id: i64) -> Result<bool, Box<dyn std::error::Error>> {
        let stored: Vec<u8> = self.conn.query_row(
            "SELECT data FROM history WHERE id=?1 AND type='composite'",
            [id],
            |r| r.get(0),
        )?;
        Ok(manifest(&stored)?.native_only)
    }
    pub fn get_composite_native(
        &self,
        id: i64,
    ) -> Result<Option<NativeSnapshot>, Box<dyn std::error::Error>> {
        if let Some(archive) = self.get_composite_archive(id)? {
            return Ok(Some(archive.publication()?));
        }
        use crate::protocol::native_clipboard::NativeFormat;
        let stored: Vec<u8> = self.conn.query_row(
            "SELECT data FROM history WHERE id=?1 AND type='composite'",
            [id],
            |r| r.get(0),
        )?;
        let m = manifest(&stored)?;
        self.verify_composite_identity(id, &m)?;
        let mut formats = vec![];
        for p in m.parts.iter().filter(|p| p.role == "native") {
            formats.push(NativeFormat {
                item: p.item,
                name: p.name.clone(),
                state: FormatState::Captured,
                bytes: self.read_composite_part(id, p)?,
            });
        }
        if formats.is_empty() {
            return Ok(None);
        }
        let items = m
            .native_items
            .max(formats.iter().map(|f| f.item).max().unwrap_or(0) + 1);
        let snapshot = NativeSnapshot {
            before: 0,
            after: 0,
            items,
            state: crate::protocol::native_clipboard::SnapshotState::Stable,
            formats,
        };
        if let Some(identity) = &m.identity {
            let platform = identity
                .content_id
                .strip_prefix("hcl1:")
                .and_then(|s| s.split(':').next())
                .and_then(|s| s.parse::<u8>().ok())
                .ok_or("Invalid native identity domain")?;
            if identity::legacy_native_id(&snapshot, platform)? != identity.content_id {
                return Err("Native content identity mismatch".into());
            }
        }
        Ok(Some(snapshot))
    }
    pub fn prepare_content_analysis(
        &self,
        id: i64,
    ) -> Result<Option<PreparedContentAnalysis>, Box<dyn std::error::Error>> {
        if !self.has_composite_schema() {
            return Ok(None);
        }
        let stored_size: Option<i64> = self
            .conn
            .query_row(
                "SELECT length(data) FROM history WHERE id=?1 AND type='composite'",
                [id],
                |row| row.get(0),
            )
            .optional()?;
        if stored_size.is_some_and(|size| !(0..=8 * 1024 * 1024 + 128).contains(&size)) {
            return Err("Analysis manifest exceeds limit".into());
        }
        let stored: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT data FROM history WHERE id=?1 AND type='composite'",
                [id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(stored) = stored else {
            return Ok(None);
        };
        let analysis_manifest = manifest(&stored)?;
        self.verify_composite_identity(id, &analysis_manifest)?;
        let revision = self.read_revision()?;
        let owner = if self.is_logical_item_favorite(&[id])? {
            "favorites"
        } else {
            "history"
        }
        .to_string();
        let data_hash: String =
            self.conn
                .query_row("SELECT data_hash FROM history WHERE id=?1", [id], |row| {
                    row.get(0)
                })?;
        let authority = format!("{:016x}", revision.authority());
        let generation = revision.generation();
        let mut parts = Vec::with_capacity(analysis_manifest.parts.len());
        let mut total = 0usize;
        for part in &analysis_manifest.parts {
            if part.role == NETWORK_ORIGINALS_ROLE {
                continue;
            }
            total = total
                .checked_add(part.size)
                .ok_or("Analysis input size overflow")?;
            if total > 48 * 1024 * 1024 {
                return Err("Analysis input exceeds display workspace limit".into());
            }
            if part.size > crate::protocol::native_clipboard::NATIVE_LIMIT {
                return Err("Composite part exceeds limit".into());
            }
            let stored_size: i64 = self.conn.query_row(
                "SELECT length(stored) FROM history_parts WHERE history_id=?1 AND part_id=?2",
                params![id, part.id],
                |row| row.get(0),
            )?;
            if !(0..=part.size as i64 + 65536).contains(&stored_size) {
                return Err("Analysis stored part exceeds declared workspace".into());
            }
            let stored: Vec<u8> = self.conn.query_row(
                "SELECT stored FROM history_parts WHERE history_id=?1 AND part_id=?2",
                params![id, part.id],
                |row| row.get(0),
            )?;
            let source = self.prepare_text(stored)?;
            parts.push(PreparedAnalysisPart {
                role: part.role.clone(),
                mime: part.mime,
                item: part.item,
                name: part.name.clone(),
                hash: part.hash.clone(),
                size: part.size,
                source,
            });
        }
        Ok(Some(PreparedContentAnalysis {
            handle: AnalysisHandle {
                entry_id: id,
                identity: analysis_manifest.identity.clone(),
                authority,
                generation,
                owner,
                revision,
            },
            native_only: analysis_manifest.native_only,
            data_hash,
            completeness: analysis_manifest.completeness,
            bindings: analysis_manifest.bindings.clone(),
            version: analysis_manifest.version,
            source_info: analysis_manifest.source_info.clone(),
            native_items: analysis_manifest.native_items,
            parts,
        }))
    }

    pub fn verify_content_analysis(
        &self,
        handle: &AnalysisHandle,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        if self.validate_read_revision(&handle.revision).is_err() {
            return Ok(false);
        }
        let current = self.read_revision()?;
        if format!("{:016x}", current.authority()) != handle.authority
            || current.generation() != handle.generation
        {
            return Ok(false);
        }
        let stored: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT data FROM history WHERE id=?1 AND type='composite'",
                [handle.entry_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(stored) = stored else {
            return Ok(false);
        };
        let analysis_manifest = manifest(&stored)?;
        self.verify_composite_identity(handle.entry_id, &analysis_manifest)?;
        if analysis_manifest.identity != handle.identity {
            return Ok(false);
        }
        let owner = if self.is_logical_item_favorite(&[handle.entry_id])? {
            "favorites"
        } else {
            "history"
        };
        Ok(owner == handle.owner)
    }
}

impl PreparedContentAnalysis {
    pub fn handle(&self) -> AnalysisHandle {
        self.handle.clone()
    }

    pub fn native_only(&self) -> bool {
        self.native_only
    }

    pub fn read_input(
        self,
        cancellation: &crate::cancellation::Cancellation,
    ) -> Result<ContentAnalysisInput, Box<dyn std::error::Error>> {
        let mut representations = vec![];
        let mut resources = vec![];
        let mut archive = None;
        let mut archive_layer = None;
        let mut native_formats: Vec<NativeFormat> = vec![];
        for part in self.parts {
            let PreparedAnalysisPart {
                role,
                mime,
                item,
                name,
                hash,
                size,
                source,
            } = part;
            match role.as_str() {
                "representation" => {
                    let bytes = read_analysis_part(source, size, &hash, cancellation)?;
                    representations.push(Representation {
                        mime: Mime::from_byte(mime)?,
                        bytes,
                    });
                }
                "resource" => {
                    let bytes = read_analysis_part(source, size, &hash, cancellation)?;
                    resources.push(Resource {
                        id: item,
                        png: bytes,
                    });
                }
                "archive" => {
                    let bytes = read_analysis_part(source, size, &hash, cancellation)?;
                    let decoded = NativeArchive::decode(&bytes)?;
                    let identity = self
                        .handle
                        .identity
                        .as_ref()
                        .ok_or("Archive identity missing")?;
                    if identity::archive_content_id(&decoded)? != identity.content_id
                        || identity::archive_dedupe_key(&decoded)? != identity.dedupe_key
                    {
                        return Err("Original archive identity mismatch".into());
                    }
                    archive = Some(decoded);
                }
                "native" => {
                    let bytes = read_analysis_part(source, size, &hash, cancellation)?;
                    native_formats.push(NativeFormat {
                        item,
                        name,
                        state: FormatState::Captured,
                        bytes,
                    });
                }
                ARCHIVE_LAYER_ROLE => {
                    let bytes = read_analysis_part(source, size, &hash, cancellation)?;
                    archive_layer = Some(PortableBundle::decode(&bytes)?);
                }
                _ => return Err("Invalid part role".into()),
            }
        }
        let base = PortableBundle {
            representations,
            resources,
            bindings: self
                .bindings
                .iter()
                .map(|binding| ResourceBinding {
                    representation: binding.representation,
                    resource: binding.resource,
                    offset: binding.offset,
                    length: binding.length,
                })
                .collect(),
            completeness: match self.completeness {
                0 => Completeness::Complete,
                1 => Completeness::ExternalReferences,
                2 => Completeness::MissingFormats,
                _ => return Err("Invalid completeness".into()),
            },
        };
        composite::validate_storable(&base)?;
        if !self.native_only && hex::encode(base.digest()?) != self.data_hash {
            return Err("Composite history digest mismatch".into());
        }
        if let Some(identity) = &self.handle.identity {
            if identity.content_id.starts_with("hcp1:")
                && identity::portable_content_id(&base)? != identity.content_id
            {
                return Err("Portable content identity mismatch".into());
            }
        }
        let bundle = match archive_layer {
            Some(layer) => {
                match (self.version, self.source_info.as_ref()) {
                    (4 | 5, Some(proof)) => {
                        crate::composite::source_context::validate_source_derivation(
                            &base, &layer, proof,
                        )?
                    }
                    _ => composite::resource_archive::validate_archive_derivation(&base, &layer)?,
                }
                layer
            }
            None => base,
        };
        let native_snapshot = if native_formats.is_empty() {
            None
        } else {
            let items = self.native_items.max(
                native_formats
                    .iter()
                    .map(|format| format.item)
                    .max()
                    .unwrap_or(0)
                    + 1,
            );
            Some(NativeSnapshot {
                before: 0,
                after: 0,
                items,
                state: SnapshotState::Stable,
                formats: native_formats,
            })
        };
        if self.native_only {
            let snapshot = archive
                .as_ref()
                .map(|archive| &archive.original)
                .or(native_snapshot.as_ref())
                .ok_or("Native analysis source missing")?;
            if hex::encode(composite::native_digest(snapshot)?) != self.data_hash {
                return Err("Native analysis digest mismatch".into());
            }
        }
        if self.native_only && archive.is_none() {
            let snapshot = native_snapshot
                .as_ref()
                .ok_or("Native analysis source missing")?;
            if let Some(identity) = &self.handle.identity {
                let platform = identity
                    .content_id
                    .strip_prefix("hcl1:")
                    .and_then(|value| value.split(':').next())
                    .and_then(|value| value.parse::<u8>().ok())
                    .ok_or("Invalid native identity domain")?;
                if identity::legacy_native_id(snapshot, platform)? != identity.content_id {
                    return Err("Native analysis identity mismatch".into());
                }
            }
        }
        Ok(ContentAnalysisInput {
            bundle,
            archive,
            native_snapshot,
        })
    }

    pub fn read_first_image(
        self,
        cancellation: &crate::cancellation::Cancellation,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        use composite::content_analysis::{
            analyze, AnalysisIdentity, AnalysisRequest, NativeItemFact, RepresentationSource,
        };
        let native_only = self.native_only;
        let handle = self.handle.clone();
        let mut native_items: Vec<NativeItemFact> = Vec::new();
        for part in &self.parts {
            if part.role != "native" {
                continue;
            }
            match native_items.iter_mut().find(|item| item.item == part.item) {
                Some(item) => item.formats.push(part.name.clone()),
                None => native_items.push(NativeItemFact {
                    item: part.item,
                    formats: vec![part.name.clone()],
                }),
            }
        }
        let input = self.read_input(cancellation)?;
        let bundle = input.bundle;
        let provenance = vec![RepresentationSource::Original; bundle.representations.len()];
        let request = AnalysisRequest {
            bundle: bundle.clone(),
            provenance,
            native_only,
            native: vec![],
            native_items,
            decode_images: false,
            document: false,
        };
        let identity = AnalysisIdentity {
            entry_id: handle.entry_id,
            authority: handle.authority,
            generation: handle.generation,
            owner: handle.owner,
            capture_id: handle
                .identity
                .as_ref()
                .map(|value| value.capture_id.clone())
                .unwrap_or_default(),
            content_id: handle
                .identity
                .as_ref()
                .map(|value| value.content_id.clone())
                .unwrap_or_default(),
            archive_revision: handle
                .identity
                .as_ref()
                .map(|value| value.archive_revision)
                .unwrap_or(0),
            archive_digest: handle
                .identity
                .as_ref()
                .map(|value| value.archive_digest.clone())
                .unwrap_or_default(),
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
        let analysis = analyze(identity, &request, cancellation, deadline)
            .map_err(|error| format!("Composite first image analysis failed: {error:?}"))?;
        if let Some(first) = analysis.first_image_if_available() {
            let bytes = if let Some(resource) = first
                .resource_id
                .and_then(|id| bundle.resources.iter().find(|resource| resource.id == id))
            {
                resource.png.clone()
            } else if matches!(
                first.coordinate,
                composite::content_analysis::ImageCoordinate::Representation
            ) {
                bundle
                    .representations
                    .get(first.representation as usize)
                    .filter(|representation| representation.mime == Mime::Png)
                    .ok_or("Composite first image representation is unavailable")?
                    .bytes
                    .clone()
            } else if let composite::content_analysis::ImageCoordinate::HtmlByteRange {
                offset,
                length,
            } = first.coordinate
            {
                use base64::Engine as _;
                let representation = bundle
                    .representations
                    .get(first.representation as usize)
                    .ok_or("Composite first image representation is unavailable")?;
                let source = representation
                    .bytes
                    .get(offset as usize..offset as usize + length as usize)
                    .ok_or("Composite first image coordinates are invalid")?;
                if source
                    .get(..22)
                    .is_none_or(|prefix| !prefix.eq_ignore_ascii_case(b"data:image/png;base64,"))
                {
                    return Err("Composite first image bytes are unavailable".into());
                }
                base64::engine::general_purpose::STANDARD.decode(&source[22..])?
            } else {
                return Err("Composite first image bytes are unavailable".into());
            };
            return Ok(composite::png_to_packed(&bytes)?);
        }
        let has_positioned = analysis.images.iter().any(|image| {
            !matches!(
                image.coordinate,
                composite::content_analysis::ImageCoordinate::Representation
            )
        });
        if !has_positioned && !analysis.unknown {
            if let Some(representation) = bundle.representation(Mime::Png) {
                return Ok(composite::png_to_packed(representation)?);
            }
        }
        Err("Composite has no displayable first image".into())
    }
}

fn read_analysis_part(
    source: reads::PayloadSource,
    size: usize,
    hash: &str,
    cancellation: &crate::cancellation::Cancellation,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let bytes = source.read(cancellation, size as u64)?;
    if bytes.len() != size || blake3::hash(&bytes).to_hex().as_str() != hash {
        return Err("Composite part digest mismatch".into());
    }
    Ok(bytes)
}

impl HistoryDB {
    /// External payloads referenced by composite parts of the given entries.
    ///
    /// Returns nothing on a database without the composite tables: a legacy store
    /// has no composite parts by construction, and querying `history_parts` there
    /// would fail the caller's whole delete/GC transaction. Guarding here covers
    /// every caller (`external_payloads_for_ids`, `delete_entries_with_batch_policy`
    /// and the quota eviction scan) in one place.
    pub(super) fn composite_part_payloads(
        &self,
        ids: &[i64],
    ) -> Result<Vec<ExternalHistoryPayload>, Box<dyn std::error::Error>> {
        let mut result = vec![];
        if !self.has_composite_schema() {
            return Ok(result);
        }
        for id in ids {
            let stored = self
                .conn
                .prepare("SELECT stored FROM history_parts WHERE history_id=?1")?
                .query_map([id], |r| r.get::<_, Vec<u8>>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            for data in stored {
                if let Some(reference) = decode_image_reference(&data) {
                    result.push(ExternalHistoryPayload {
                        path: resolve_file_reference_at(&self.image_history_dir, &reference)?,
                    });
                }
            }
        }
        Ok(result)
    }
    fn prune_composite_tombstones(&self) -> Result<(), Box<dyn std::error::Error>> {
        let cutoff = crate::protocol::unix_timestamp_ms()
            .saturating_sub(crate::protocol::EVENT_TIMESTAMP_WINDOW_MS * 2);
        self.conn.execute(
            "DELETE FROM history_composite_events WHERE history_id IS NULL AND timestamp_ms<?1",
            [cutoff],
        )?;
        let count: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM history_composite_events", [], |r| {
                    r.get(0)
                })?;
        if count >= COMPOSITE_EVENT_METADATA_LIMIT {
            return Err("Composite history event capacity exhausted".into());
        }
        Ok(())
    }
}

fn verify_archive_history(
    conn: &Connection,
    id: i64,
    expected: &HistoryIdentity,
    stored: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let valid: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM history h JOIN history_composite_identity i ON i.history_id=h.id WHERE h.id=?1 AND h.type='composite' AND h.data=?2 AND i.capture_id=?3 AND i.content_id=?4 AND i.archive_revision=?5 AND i.archive_digest=?6 AND i.version=?7 AND i.dedupe_key IS ?8)", params![id, stored, expected.capture_id, expected.content_id, expected.archive_revision, expected.archive_digest, expected.version, expected.dedupe_key], |row| row.get(0))?;
    if !valid {
        return Err("Composite archive history changed".into());
    }
    Ok(())
}

fn verify_remote_archive_base(
    conn: &Connection,
    source: &str,
    event: &str,
    digest: &str,
    id: i64,
) -> Result<(), Box<dyn std::error::Error>> {
    let valid: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM history_composite_events WHERE source_device_id=?1 AND event_id=?2 AND digest=?3 AND history_id=?4)", params![source, event, digest, id], |row| row.get(0))?;
    if !valid {
        return Err("Composite archive base mapping changed".into());
    }
    Ok(())
}

fn write_archive_tracker(
    conn: &rusqlite::Connection,
    source_identity: &str,
    event_id: &str,
    expected: Option<&str>,
    digest: &str,
    history_id: i64,
    timestamp_ms: i64,
) -> Result<(), Box<dyn std::error::Error>> {
    let existing: Option<String> = conn
        .query_row(
            "SELECT digest FROM history_composite_events WHERE source_device_id=?1 AND event_id=?2",
            params![source_identity, event_id],
            |r| r.get(0),
        )
        .optional()?;
    if existing.as_deref() != expected {
        return Err("Composite archive tracker changed".into());
    }
    match existing {
        Some(_) => {
            conn.execute(
                "UPDATE history_composite_events SET digest=?1, history_id=?2, timestamp_ms=?3 WHERE source_device_id=?4 AND event_id=?5",
                params![digest, history_id, timestamp_ms, source_identity, event_id],
            )?;
        }
        None => {
            let count: i64 =
                conn.query_row("SELECT COUNT(*) FROM history_composite_events", [], |r| {
                    r.get(0)
                })?;
            if count >= COMPOSITE_EVENT_METADATA_LIMIT {
                return Err("Composite history event capacity exhausted".into());
            }
            conn.execute(
                "INSERT INTO history_composite_events(source_device_id,event_id,digest,history_id,timestamp_ms) VALUES (?1,?2,?3,?4,?5)",
                params![source_identity, event_id, digest, history_id, timestamp_ms],
            )?;
        }
    }
    Ok(())
}

fn html_image_spans(
    bundle: &PortableBundle,
) -> Result<Vec<crate::composite::html::ImageSourceSpan>, crate::protocol::ProtocolError> {
    let Some(representation) = bundle
        .representations
        .iter()
        .find(|representation| representation.mime == Mime::Html)
    else {
        return Ok(Vec::new());
    };
    Ok(crate::composite::html::reference_inventory(&representation.bytes)?.images)
}

fn bound_image_resource(
    bundle: &PortableBundle,
    span: &crate::composite::html::ImageSourceSpan,
) -> Option<u16> {
    let index = bundle
        .representations
        .iter()
        .position(|representation| representation.mime == Mime::Html)?;
    let length = span.end.checked_sub(span.start)?;
    bundle
        .bindings
        .iter()
        .find(|binding| {
            binding.representation as usize == index
                && binding.offset as usize == span.start
                && binding.length as usize == length
        })
        .map(|binding| binding.resource)
}

fn image_resource_id(
    bundle: &PortableBundle,
    image_index: u16,
) -> Result<Option<u16>, crate::protocol::ProtocolError> {
    let spans = html_image_spans(bundle)?;
    let span = spans
        .get(image_index as usize)
        .ok_or(crate::protocol::ProtocolError::InvalidComposite)?;
    Ok(bound_image_resource(bundle, span))
}

fn image_resource_png(
    bundle: &PortableBundle,
    image_index: u16,
) -> Result<Option<&[u8]>, crate::protocol::ProtocolError> {
    let Some(resource_id) = image_resource_id(bundle, image_index)? else {
        return Ok(None);
    };
    let resource = bundle
        .resources
        .iter()
        .find(|resource| resource.id == resource_id)
        .ok_or(crate::protocol::ProtocolError::InvalidComposite)?;
    Ok(Some(&resource.png))
}

fn image_source_span(
    bundle: &PortableBundle,
    image_index: u16,
) -> Result<crate::composite::html::ImageSourceSpan, crate::protocol::ProtocolError> {
    html_image_spans(bundle)?
        .get(image_index as usize)
        .copied()
        .ok_or(crate::protocol::ProtocolError::InvalidComposite)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn root() -> PathBuf {
        std::env::temp_dir().join(format!(
            "tailsync-composite-db-{:016x}",
            rand::random::<u64>()
        ))
    }
    fn original_archive() -> NativeArchive {
        use crate::protocol::{native_archive::DerivedFormat, native_clipboard::*};
        NativeArchive {
            platform: 1,
            enumeration_complete: true,
            original: NativeSnapshot {
                before: 4,
                after: 4,
                items: 1,
                state: SnapshotState::Stable,
                formats: vec![
                    NativeFormat {
                        item: 0,
                        name: "com.apple.flat-rtfd".into(),
                        state: FormatState::Captured,
                        bytes: b"original RTFD".to_vec(),
                    },
                    NativeFormat {
                        item: 0,
                        name: "public.html".into(),
                        state: FormatState::Captured,
                        bytes: b"<p>original HTML</p>".to_vec(),
                    },
                    NativeFormat {
                        item: 0,
                        name: "public.utf8-plain-text".into(),
                        state: FormatState::Captured,
                        bytes: b"original text".to_vec(),
                    },
                ],
            },
            derived: vec![
                DerivedFormat {
                    item: 0,
                    name: "public.html".into(),
                    source_name: "com.apple.flat-rtfd".into(),
                    converter: "tailsync.rtfd-to-portable.v1".into(),
                    bytes: b"<p>derived HTML</p>".to_vec(),
                },
                DerivedFormat {
                    item: 0,
                    name: "public.utf8-plain-text".into(),
                    source_name: "com.apple.flat-rtfd".into(),
                    converter: "tailsync.rtfd-to-portable.v1".into(),
                    bytes: b"derived text".to_vec(),
                },
            ],
        }
    }
    #[test]
    fn o10_saved_native_only_and_failed_storage_have_distinct_notices() {
        use crate::composite::{origin::Origin, CaptureError};
        use crate::sync_warning;
        let _warnings = sync_warning::test_lock();
        while sync_warning::take().is_some() {}
        let root = root();
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let mut archive = original_archive();
        archive.derived.clear();
        archive.original.formats.truncate(1);
        archive.original.formats[0].name = "com.tailsync.fixture.opaque".into();
        assert_eq!(
            composite::from_archive(&archive),
            Err(CaptureError::NativeOnly)
        );
        let id = db.add_native_archive(&archive, "").unwrap();
        assert_eq!(db.get_composite_archive(id).unwrap(), Some(archive.clone()));
        sync_warning::record_saved_capture(&archive.original, None, Origin::External, true);
        assert!(
            sync_warning::peek().is_none(),
            "saved native bytes are not a failed read"
        );

        let rows = db.get_all(None, None, 20, 0).unwrap().len();
        assert!(db.add_native_snapshot(&archive.original, "").is_err());
        assert_eq!(db.get_all(None, None, 20, 0).unwrap().len(), rows);
        let mut legacy = archive.original.clone();
        legacy.formats[0].name = "CF_UNICODETEXT".into();
        legacy.formats[0].bytes = vec![1]; // Unconvertible, but retained raw bytes.
        let legacy_id = db.add_native_snapshot(&legacy, "").unwrap();
        // CNS1 reconstruction deliberately has no original OS revision.
        let restored = db.get_composite_native(legacy_id).unwrap().unwrap();
        assert_eq!(restored.formats, legacy.formats);
        legacy.formats.push(archive.original.formats[0].clone());
        assert!(db.add_native_snapshot(&legacy, "").is_err());
        assert_eq!(
            db.get_composite_native(legacy_id).unwrap().unwrap().formats,
            restored.formats
        );
        for (origin, failed_read, kind) in [
            (Origin::UnknownMismatch, false, "capture_origin_unknown"),
            (Origin::External, true, "capture_failed"),
        ] {
            let mut saved = archive.clone();
            if failed_read {
                let mut failed = saved.original.formats[0].clone();
                failed.name = "public.png".into();
                failed.state = FormatState::Failed;
                failed.bytes.clear();
                saved.original.formats.push(failed);
            }
            db.add_native_archive(&saved, "").unwrap();
            sync_warning::record_saved_capture(&saved.original, None, origin, !failed_read);
            let warning = sync_warning::peek().unwrap();
            assert_eq!(warning.kind, kind);
            assert!(sync_warning::ack(warning.id));
            assert!(sync_warning::peek().is_none());
        }
        sync_warning::record_capture_not_shared();
        let warning = sync_warning::peek().unwrap();
        assert_eq!(warning.kind, "capture_not_shared");
        assert!(sync_warning::ack(warning.id));

        // A real SQLite write rejection exercises the production persistence seam
        // without filling the user's disk or changing a global storage flag.
        db.conn.execute_batch("PRAGMA query_only=ON").unwrap();
        assert!(db.add_native_archive(&archive, "").is_err());
        sync_warning::record_capture_not_saved();
        let warning = sync_warning::peek().unwrap();
        assert_eq!(warning.kind, "capture_not_saved");
        assert_eq!(sync_warning::peek().unwrap().id, warning.id);
        assert!(!sync_warning::ack(warning.id.wrapping_add(1)));
        assert!(sync_warning::ack(warning.id));
        assert_eq!(db.get_composite_archive(id).unwrap(), Some(archive));
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn o01_original_and_derived_survive_restart_as_separate_roles() {
        let root = root();
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let a = original_archive();
        let id = db.add_native_archive(&a, "self").unwrap();
        let original_id = db.composite_identity(id).unwrap().unwrap();
        assert!(original_id.content_id.starts_with("hcn1:1:"));
        assert!(original_id.dedupe_key.is_none());
        drop(db);
        let db = HistoryDB::open_at_with_policy(&root, MigrationPolicy::CompositeAllowed).unwrap();
        assert_eq!(db.get_composite_archive(id).unwrap().unwrap(), a);
        assert_eq!(
            db.get_composite_native(id).unwrap().unwrap(),
            a.publication().unwrap()
        );
        assert_eq!(db.get_composite(id).unwrap().text(), Some("derived text"));
        assert_eq!(db.composite_identity(id).unwrap().unwrap(), original_id);
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }

    // ---- O03: original image encodings and animation -----------------------

    fn animated_gif() -> Vec<u8> {
        include_bytes!("../../../tailsync-protocol/tests/fixtures/images/animated.gif").to_vec()
    }
    fn animated_png() -> Vec<u8> {
        include_bytes!("../../../tailsync-protocol/tests/fixtures/images/animated.png").to_vec()
    }
    fn transparent_png() -> Vec<u8> {
        composite::packed_to_png(
            2,
            2,
            &[
                255, 0, 0, 0, 0, 255, 0, 255, 0, 0, 255, 128, 255, 255, 255, 255,
            ],
        )
        .unwrap()
    }
    fn jpeg_4x3() -> Vec<u8> {
        include_bytes!("../../../tailsync-protocol/tests/fixtures/images/color.jpg").to_vec()
    }
    fn tiff_4x3() -> Vec<u8> {
        include_bytes!("../../../tailsync-protocol/tests/fixtures/images/color.tiff").to_vec()
    }
    fn image_archive(name: &str, bytes: Vec<u8>, cover: Option<Vec<u8>>) -> NativeArchive {
        use crate::protocol::native_archive::{DerivedFormat, CONVERTER_STATIC_COVER};
        use crate::protocol::native_clipboard::*;
        let mut archive = NativeArchive {
            platform: 1,
            enumeration_complete: true,
            original: NativeSnapshot {
                before: 7,
                after: 7,
                items: 1,
                state: SnapshotState::Stable,
                formats: vec![NativeFormat {
                    item: 0,
                    name: name.into(),
                    state: FormatState::Captured,
                    bytes,
                }],
            },
            derived: vec![],
        };
        if let Some(cover) = cover {
            archive.derived.push(DerivedFormat {
                item: 0,
                name: "public.png".into(),
                source_name: name.into(),
                converter: CONVERTER_STATIC_COVER.into(),
                bytes: cover,
            });
        }
        archive
    }

    #[test]
    fn o03_animated_gif_original_survives_reopen_with_cover_and_stable_frames() {
        use crate::protocol::image;
        let gif = animated_gif();
        let cover = composite::packed_to_png(
            2,
            2,
            &[
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255,
            ],
        )
        .unwrap();
        let archive = image_archive("com.compuserve.gif", gif.clone(), Some(cover.clone()));
        assert_eq!(
            image::metadata("com.compuserve.gif", &gif).unwrap().frames,
            2
        );
        // The portable projection is the explicit cover, never the animation.
        let projected = composite::from_archive(&archive).unwrap();
        assert_eq!(projected.representation(Mime::Png), Some(cover.as_slice()));
        assert_ne!(projected.representation(Mime::Png).unwrap(), gif.as_slice());

        let root = root();
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let id = db.add_native_archive(&archive, "self").unwrap();
        let identity = db.composite_identity(id).unwrap().unwrap();
        assert!(identity.content_id.starts_with("hcn1:1:"));
        drop(db);

        let db = HistoryDB::open_at_with_policy(&root, MigrationPolicy::CompositeAllowed).unwrap();
        let page = db
            .prepare_read_chunk(
                HistoryQuery {
                    collection: HistoryCollection::All,
                    keyword: None,
                    category: None,
                    start_time: None,
                    end_time: None,
                    limit: 10,
                    offset: 0,
                },
                None,
                &db.read_revision().unwrap(),
                10,
            )
            .unwrap();
        let rows = page
            .matching_entries(None, &crate::cancellation::Cancellation::default())
            .unwrap();
        assert_eq!(
            rows[0].description,
            "image/gif · 4×3 · 2 frames · static PNG cover"
        );
        let stored = db.get_composite_archive(id).unwrap().unwrap();
        assert_eq!(stored, archive);
        assert_eq!(stored.publication().unwrap().formats[0].bytes, gif);
        assert_eq!(
            image::metadata("com.compuserve.gif", &stored.original.formats[0].bytes)
                .unwrap()
                .frames,
            2,
            "frame count is recomputed from retained bytes and cannot drift on reopen"
        );
        assert_eq!(db.composite_identity(id).unwrap().unwrap(), identity);
        // The reopened row still projects the cover, not the original animation.
        assert_eq!(
            db.get_composite(id).unwrap().representation(Mime::Png),
            Some(cover.as_slice())
        );
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn o03_apng_original_is_retained_and_not_refused_by_the_static_png_gate() {
        use crate::protocol::image;
        let apng = animated_png();
        assert_eq!(
            image::metadata("public.png", &apng).unwrap().kind,
            image::ImageKind::Apng
        );
        let archive = image_archive("public.png", apng.clone(), None);
        // Without a cover the animation cannot be projected, but it is retained:
        // the static PNG gate must not turn the capture into InvalidFormat.
        assert_eq!(
            composite::from_archive(&archive),
            Err(composite::CaptureError::NativeOnly)
        );

        let root = root();
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let id = db.add_native_archive(&archive, "self").unwrap();
        let stored = db.get_composite_archive(id).unwrap().unwrap();
        assert_eq!(stored, archive);
        assert_eq!(stored.publication().unwrap().formats[0].bytes, apng);
        assert!(db
            .composite_identity(id)
            .unwrap()
            .unwrap()
            .content_id
            .starts_with("hcn1:1:"));
        drop(db);
        std::fs::remove_dir_all(root).unwrap();

        // With a bounded cover the same original projects statically and stays
        // animated in the archive.
        let cover = composite::packed_to_png(
            2,
            2,
            &[
                255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255,
            ],
        )
        .unwrap();
        let with_cover = image_archive("public.png", apng.clone(), Some(cover.clone()));
        let projected = composite::from_archive(&with_cover).unwrap();
        assert_eq!(projected.representation(Mime::Png), Some(cover.as_slice()));
        assert_eq!(with_cover.original.formats[0].bytes, apng);
    }

    #[test]
    fn o03_large_static_png_is_retained_and_prepared_from_originals() {
        use crate::protocol::native_clipboard::*;
        let large =
            include_bytes!("../../../tailsync-protocol/tests/fixtures/images/over-budget.png")
                .to_vec();
        assert!(
            !crate::protocol::image::probe(&large)
                .unwrap()
                .within_budget(),
            "the fixture must exceed the portable cover/decoder budget"
        );
        for with_text in [false, true] {
            let mut archive = image_archive("public.png", large.clone(), None);
            if with_text {
                archive.original.formats.push(NativeFormat {
                    item: 0,
                    name: "public.utf8-plain-text".into(),
                    state: FormatState::Captured,
                    bytes: b"large image caption".to_vec(),
                });
            }
            assert_eq!(
                composite::from_archive(&archive),
                Err(composite::CaptureError::NativeOnly),
                "with_text={with_text}"
            );
            assert!(crate::composite::fingerprint::requires_original_domain(
                &archive.original
            ));

            let root = root();
            let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
            let id = db.add_native_archive(&archive, "self").unwrap();
            drop(db);
            let db =
                HistoryDB::open_at_with_policy(&root, MigrationPolicy::CompositeAllowed).unwrap();
            let stored = db.get_composite_archive(id).unwrap().unwrap();
            assert_eq!(stored, archive, "with_text={with_text}");
            let publication = stored.publication().unwrap();
            assert_eq!(publication, archive.publication().unwrap());
            assert!(publication
                .formats
                .iter()
                .any(|f| f.name == "public.png" && f.bytes == large));
            let plan = crate::composite::restore::RestorePlan::originals(
                publication,
                crate::composite::restore::RestoreInput {
                    source_platform: Some(1),
                    requested_format: None,
                },
                crate::composite::restore::RestoreTarget::Macos,
            )
            .unwrap();
            assert!(plan
                .outputs()
                .iter()
                .any(|o| o.name == "public.png" && o.bytes == large.len()));
            drop(db);
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    /// O03 — the v2 manifest serialization is frozen. A newly serialized field
    /// would change `archive_digest` for every existing row and make it invalid,
    /// so future metadata must be defaulted and skipped while absent.
    #[test]
    fn o03_frozen_v2_manifest_digest_still_verifies() {
        const FROZEN_MANIFEST: &str = r#"{"version":2,"identity":{"version":1,"capture_id":"ec9f347224a1658c1e3b586f32bb1e41","content_id":"hcn1:1:99c4e4f77da4816bb4a1e39e4ae0806e76ebe38d227427bbf2757745e362c4c6","dedupe_key":null,"archive_revision":1,"archive_digest":"4aa7777e80e9edb1cc461a17fd899d858f6655d0d83cc932bb0e089eaad968c6"},"integrity_method":"native-cns1","native_only":true,"native_items":1,"archive_version":1,"summary":"Native clipboard (1 items)","completeness":2,"parts":[{"id":0,"role":"representation","mime":1,"item":0,"name":"","hash":"1f670b43fc50314166f4da92964ede0f35cbf0ce5d5db3c8e2f0be0bdba4ce0a","size":26,"width":null,"height":null},{"id":1,"role":"archive","mime":0,"item":0,"name":"CNA1","hash":"271d9184009af30c51ab61a2e6c684f75e478df99e72b6e9cd0c2a99b2e2062b","size":150,"width":null,"height":null}],"bindings":[]}"#;
        const FROZEN_DIGEST: &str =
            "4aa7777e80e9edb1cc461a17fd899d858f6655d0d83cc932bb0e089eaad968c6";
        const FROZEN_CONTENT_ID: &str =
            "hcn1:1:99c4e4f77da4816bb4a1e39e4ae0806e76ebe38d227427bbf2757745e362c4c6";
        let m: Manifest = serde_json::from_str(FROZEN_MANIFEST).unwrap();
        assert_eq!(
            archive_digest(&m).unwrap(),
            FROZEN_DIGEST,
            "adding a serialized manifest field changes existing archive identities"
        );
        assert_eq!(serde_json::to_string(&m).unwrap(), FROZEN_MANIFEST);

        let root = root();
        let db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let stored = crypto::encrypt(FROZEN_MANIFEST.as_bytes()).unwrap();
        db.conn
            .execute(
                "INSERT INTO history (timestamp,type,description,data,size_bytes,source_peer,data_hash,category,categories,category_confidence,classifier_version) VALUES (?1,'composite','Encrypted composite',?2,0,?3,?4,'image','[\"text\",\"image\"]',100,?5)",
                rusqlite::params!["2026-10-06T00:00:00Z", stored, "self", FROZEN_DIGEST, crate::history_classifier::CLASSIFIER_VERSION],
            )
            .unwrap();
        let id = db.conn.last_insert_rowid();
        db.conn
            .execute(
                "INSERT INTO history_composite_identity(history_id,version,capture_id,content_id,dedupe_key,archive_revision,archive_digest) VALUES (?1,1,?2,?3,NULL,1,?4)",
                rusqlite::params![id, "ec9f347224a1658c1e3b586f32bb1e41", FROZEN_CONTENT_ID, FROZEN_DIGEST],
            )
            .unwrap();
        let identity = db.composite_identity(id).unwrap().unwrap();
        assert_eq!(identity.archive_digest, FROZEN_DIGEST);
        // Identity verification recomputes `archive_digest` from the parsed
        // manifest, so a changed serialization fails right here.
        assert_eq!(
            summary_bytes(FROZEN_MANIFEST.as_bytes()).unwrap(),
            b"Native clipboard (1 items)"
        );
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn o03_transparent_png_static_passthrough_and_foreign_image_covers() {
        use crate::protocol::image;
        let png = transparent_png();
        let meta = image::metadata("public.png", &png).unwrap();
        assert_eq!(
            (meta.kind, meta.width, meta.height, meta.frames),
            (image::ImageKind::Png, 2, 2, 1)
        );
        assert!(meta.is_static_png());
        // A static transparent PNG is already the portable encoding: unchanged.
        let passthrough = image_archive("public.png", png.clone(), None);
        assert_eq!(
            composite::from_archive(&passthrough)
                .unwrap()
                .representation(Mime::Png),
            Some(png.as_slice())
        );

        for (name, bytes, width, height) in [
            ("public.jpeg", jpeg_4x3(), 4, 3),
            ("public.tiff", tiff_4x3(), 4, 3),
        ] {
            let meta = image::metadata(name, &bytes).unwrap();
            assert_eq!(
                (meta.width, meta.height),
                (width, height),
                "{name} dimensions are grounded in the bytes"
            );
            let cover = composite::packed_to_png(
                2,
                2,
                &[9, 9, 9, 255, 9, 9, 9, 255, 9, 9, 9, 255, 9, 9, 9, 255],
            )
            .unwrap();
            let archive = image_archive(name, bytes.clone(), Some(cover.clone()));
            let projected = composite::from_archive(&archive).unwrap();
            // The compatibility projection is static and single-frame; the
            // original encoding is what the archive replays.
            assert_eq!(projected.representation(Mime::Png), Some(cover.as_slice()));
            assert!(
                !image::metadata(name, &archive.original.formats[0].bytes)
                    .unwrap()
                    .animation
            );
            assert_eq!(archive.publication().unwrap().formats[0].bytes, bytes);
        }
    }
    #[test]
    fn o03_cover_cannot_replace_a_static_original_or_claim_a_text_source() {
        use crate::protocol::native_clipboard::*;
        let png = transparent_png();
        let mut a = image_archive(
            "com.compuserve.gif",
            animated_gif(),
            Some(composite::packed_to_png(1, 1, &[0, 0, 0, 255]).unwrap()),
        );
        a.original.formats.push(NativeFormat {
            item: 0,
            name: "public.png".into(),
            state: FormatState::Captured,
            bytes: png.clone(),
        });
        assert_eq!(
            composite::from_archive(&a)
                .unwrap()
                .representation(Mime::Png),
            Some(png.as_slice())
        );
        let mut a = image_archive("public.utf8-plain-text", b"text".to_vec(), Some(png));
        let b = composite::from_archive(&a).unwrap();
        assert_eq!(b.text(), Some("text"));
        assert!(b.representation(Mime::Png).is_none());
        a.original.formats[0].name = "com.compuserve.gif".into();
        a.original.formats[0].bytes = animated_gif();
        a.derived[0].source_name = "com.compuserve.gif".into();
        a.derived[0].bytes = animated_png();
        assert_eq!(
            composite::from_archive(&a),
            Err(composite::CaptureError::NativeOnly)
        );
    }
    #[test]
    fn o01_source_control_marker_is_not_conversion_evidence() {
        use crate::protocol::native_clipboard::*;
        let mut a = original_archive();
        a.original.formats.push(NativeFormat {
            item: 0,
            name: "org.tailsync.rtfd-converted".into(),
            state: FormatState::Captured,
            bytes: vec![1],
        });
        assert_eq!(
            composite::from_archive(&a).unwrap().text(),
            Some("derived text")
        );
        a.derived.clear();
        assert!(matches!(
            composite::from_archive(&a),
            Err(composite::CaptureError::NativeOnly)
        ));
    }
    #[test]
    fn o02_sensitive_metadata_refuses_whole_archive_and_legacy_snapshot() {
        use crate::protocol::native_clipboard::*;
        let root = root();
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        for marker in [
            "org.nspasteboard.ConcealedType",
            "org.nspasteboard.TransientType",
            "org.nspasteboard.AutoGeneratedType",
            "com.agilebits.onepassword",
        ] {
            let mut a = original_archive();
            a.original.formats.push(NativeFormat {
                item: 0,
                name: marker.into(),
                state: FormatState::Absent,
                bytes: vec![],
            });
            assert!(db.add_native_archive(&a, "self").is_err(), "{marker}");
            assert!(
                db.add_native_snapshot(&a.original, "self").is_err(),
                "{marker}"
            );
        }
        assert!(db.get_all(None, None, 50, 0).unwrap().is_empty());
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn o01_derived_outputs_cannot_change_original_identity_or_default_restore() {
        let root = root();
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let mut a = original_archive();
        let first = db.add_native_archive(&a, "self").unwrap();
        let held = db.composite_identity(first).unwrap().unwrap();
        a.derived[1].bytes = b"new conversion".to_vec();
        let second = db.add_native_archive(&a, "self").unwrap();
        let updated = db.composite_identity(second).unwrap().unwrap();
        assert_eq!(held.content_id, updated.content_id);
        assert_ne!(held.archive_digest, updated.archive_digest);
        assert_eq!(
            db.get_composite_native(first).unwrap(),
            db.get_composite_native(second).unwrap()
        );
        a.original.formats[0].bytes.push(0);
        let different = db.add_native_archive(&a, "self").unwrap();
        assert_ne!(
            updated.content_id,
            db.composite_identity(different)
                .unwrap()
                .unwrap()
                .content_id
        );
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn o01_exact_closed_text_dedupes_but_unread_or_incomplete_enumeration_does_not() {
        use crate::protocol::native_clipboard::*;
        let root = root();
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let mut a = original_archive();
        a.derived.clear();
        a.original
            .formats
            .retain(|f| f.name == "public.utf8-plain-text");
        let first = db.add_native_archive(&a, "self").unwrap();
        let second = db.add_native_archive(&a, "self").unwrap();
        assert!(db.get_composite_archive(first).is_err());
        assert!(db
            .composite_identity(second)
            .unwrap()
            .unwrap()
            .dedupe_key
            .is_some());
        a.enumeration_complete = false;
        let third = db.add_native_archive(&a, "self").unwrap();
        assert!(db.get_composite_archive(second).unwrap().is_some());
        assert!(db
            .composite_identity(third)
            .unwrap()
            .unwrap()
            .dedupe_key
            .is_none());
        a.enumeration_complete = true;
        a.original.formats.push(NativeFormat {
            item: 0,
            name: "unattempted.provider".into(),
            state: FormatState::Absent,
            bytes: vec![],
        });
        let fourth = db.add_native_archive(&a, "self").unwrap();
        assert_eq!(
            db.get_composite_archive(fourth)
                .unwrap()
                .unwrap()
                .original
                .formats
                .len(),
            2
        );
        assert!(db
            .composite_identity(fourth)
            .unwrap()
            .unwrap()
            .dedupe_key
            .is_none());
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn o01_native_only_multi_item_keeps_boundaries_and_failed_read_facts() {
        use crate::protocol::native_clipboard::*;
        let root = root();
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let mut a = original_archive();
        a.derived.clear();
        a.original.items = 2;
        a.original.formats.push(NativeFormat {
            item: 1,
            name: "public.html".into(),
            state: FormatState::Failed,
            bytes: vec![],
        });
        let id = db.add_native_archive(&a, "self").unwrap();
        assert!(db.composite_native_only(id).unwrap());
        assert_eq!(db.get_composite_archive(id).unwrap().unwrap(), a);
        assert_eq!(db.get_composite_native(id).unwrap().unwrap().items, 2);
        assert_eq!(
            db.get_composite_native(id).unwrap().unwrap().formats,
            a.original.formats
        );
        assert!(db
            .composite_identity(id)
            .unwrap()
            .unwrap()
            .dedupe_key
            .is_none());
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn o01_external_archive_is_validated_and_deleted_with_its_history() {
        let root = root();
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let mut a = original_archive();
        a.original.formats[0].bytes = vec![9; 100_000];
        let id = db.add_native_archive(&a, "self").unwrap();
        let parts = db.composite_part_payloads(&[id]).unwrap();
        assert!(!parts.is_empty());
        assert_eq!(db.get_composite_archive(id).unwrap().unwrap(), a);
        db.delete(id).unwrap();
        assert!(parts.iter().all(|p| !p.path.exists()));
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
    fn bundle(text: &str) -> PortableBundle {
        PortableBundle {
            representations: vec![
                Representation {
                    mime: Mime::Text,
                    bytes: text.as_bytes().to_vec(),
                },
                Representation {
                    mime: Mime::Html,
                    bytes: format!("<b>{text}</b>").into_bytes(),
                },
            ],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::Complete,
        }
    }

    #[test]
    fn rich_history_body_projects_new_legacy_and_archived_rows_without_mutation() {
        for mode in ["mixed", "html_only", "archived"] {
            let root = root();
            let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
            let body = "正文识别 & unique-body";
            let html = "<div class='lightbox-wrapper'><a class='lightbox'><img src='tailsync-resource:1' alt='image'><div class='meta'><span class='filename'>image</span><span class='informations'>2557×1320 154 KB</span></div></a></div><p>正文识别 &amp; unique-body</p>";
            let marker = "tailsync-resource:1";
            let mut b = PortableBundle {
                representations: vec![Representation {
                    mime: Mime::Html,
                    bytes: html.as_bytes().to_vec(),
                }],
                resources: vec![Resource {
                    id: 1,
                    png: composite::packed_to_png(1, 1, &[1, 2, 3, 255]).unwrap(),
                }],
                bindings: vec![ResourceBinding {
                    representation: 0,
                    resource: 1,
                    offset: html.find(marker).unwrap() as u32,
                    length: marker.len() as u32,
                }],
                completeness: Completeness::Complete,
            };
            if mode != "html_only" {
                b.representations.push(Representation {
                    mime: Mime::Text,
                    bytes: "image\n2557×1320 154 KB\n正文识别 & unique-body"
                        .as_bytes()
                        .to_vec(),
                });
            }
            let id = db.add_composite(&b, None, "fixture").unwrap();
            let stored: Vec<u8> = db
                .conn
                .query_row("SELECT data FROM history WHERE id=?1", [id], |r| r.get(0))
                .unwrap();
            assert_eq!(manifest(&stored).unwrap().summary, body, "{mode}");
            if mode == "archived" {
                let expected = db.composite_identity(id).unwrap().unwrap();
                assert!(db
                    .commit_composite_archive(id, &expected, &b)
                    .unwrap()
                    .is_some());
            }
            // An old row can have no body in its saved summary. Listing and
            // searching must still use the validated HTML, without migration.
            let stored: Vec<u8> = db
                .conn
                .query_row("SELECT data FROM history WHERE id=?1", [id], |r| r.get(0))
                .unwrap();
            let mut m = manifest(&stored).unwrap();
            m.summary = "image\n2557×1320 154 KB".into();
            let digest = archive_digest(&m).unwrap();
            m.identity.as_mut().unwrap().archive_digest = digest.clone();
            let legacy = crypto::encrypt(&serde_json::to_vec(&m).unwrap()).unwrap();
            db.conn
                .execute(
                    "UPDATE history SET data=?1 WHERE id=?2",
                    params![legacy, id],
                )
                .unwrap();
            db.conn
                .execute(
                    "UPDATE history_composite_identity SET archive_digest=?1 WHERE history_id=?2",
                    params![digest, id],
                )
                .unwrap();
            let identity = db.composite_identity(id).unwrap();
            let parts: Vec<(i64, Vec<u8>)> = db
                .conn
                .prepare(
                    "SELECT part_id,stored FROM history_parts WHERE history_id=?1 ORDER BY part_id",
                )
                .unwrap()
                .query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            for keyword in [None, Some("unique-body"), Some("正文识别")] {
                let rows = db.get_all(keyword, None, 10, 0).unwrap();
                assert_eq!(rows.len(), 1, "{mode}: {keyword:?}");
                assert_eq!(rows[0].description, body, "{mode}");
                let chunk = db
                    .prepare_read_chunk(
                        HistoryQuery {
                            collection: HistoryCollection::All,
                            keyword,
                            category: None,
                            start_time: None,
                            end_time: None,
                            limit: 10,
                            offset: 0,
                        },
                        None,
                        &db.read_revision().unwrap(),
                        10,
                    )
                    .unwrap();
                let rows = chunk
                    .matching_entries(keyword, &crate::cancellation::Cancellation::default())
                    .unwrap();
                assert_eq!(rows.len(), 1, "{mode}: {keyword:?}");
                assert_eq!(rows[0].description, body, "{mode}");
            }
            assert!(db.get_all(Some("154 KB"), None, 10, 0).unwrap().is_empty());
            assert_eq!(db.composite_identity(id).unwrap(), identity);
            assert_eq!(db.get_composite(id).unwrap(), b);
            let after: Vec<u8> = db
                .conn
                .query_row("SELECT data FROM history WHERE id=?1", [id], |r| r.get(0))
                .unwrap();
            assert_eq!(after, legacy);
            let after_parts: Vec<(i64, Vec<u8>)> = db
                .conn
                .prepare(
                    "SELECT part_id,stored FROM history_parts WHERE history_id=?1 ORDER BY part_id",
                )
                .unwrap()
                .query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            assert_eq!(after_parts, parts);
            drop(db);
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn rich_history_missing_or_corrupt_html_retains_saved_summary() {
        for corrupt in [false, true] {
            let root = root();
            let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
            let b = bundle("正文 safe fallback");
            let id = db.add_composite(&b, None, "fixture").unwrap();
            if corrupt {
                db.conn
                    .execute(
                        "UPDATE history_parts SET stored=?1 WHERE history_id=?2 AND mime=?3",
                        params![
                            crypto::encrypt(b"corrupt HTML").unwrap(),
                            id,
                            Mime::Html as u8
                        ],
                    )
                    .unwrap();
            } else {
                db.conn
                    .execute(
                        "DELETE FROM history_parts WHERE history_id=?1 AND mime=?2",
                        params![id, Mime::Html as u8],
                    )
                    .unwrap();
            }
            for keyword in [None, Some("safe fallback")] {
                let rows = db.get_all(keyword, None, 10, 0).unwrap();
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0].description, "正文 safe fallback");
                let chunk = db
                    .prepare_read_chunk(
                        HistoryQuery {
                            collection: HistoryCollection::All,
                            keyword,
                            category: None,
                            start_time: None,
                            end_time: None,
                            limit: 10,
                            offset: 0,
                        },
                        None,
                        &db.read_revision().unwrap(),
                        10,
                    )
                    .unwrap();
                let rows = chunk
                    .matching_entries(keyword, &crate::cancellation::Cancellation::default())
                    .unwrap();
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0].description, "正文 safe fallback");
            }
            drop(db);
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn o06_sync_and_owned_listing_withhold_unbound_missing_or_corrupt_html() {
        for mode in ["unbound", "missing", "corrupt", "missing_src", "comment"] {
            let root = root();
            let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
            let mut b = bundle("retained search summary");
            if mode == "unbound" {
                b.representations[1].bytes = b"<img src='https://fixture.invalid/a.png'>".to_vec();
            } else if matches!(mode, "missing_src" | "comment") {
                use base64::Engine as _;
                let png = base64::engine::general_purpose::STANDARD
                    .encode(composite::packed_to_png(1, 1, &[255, 0, 0, 128]).unwrap());
                let extra = if mode == "missing_src" {
                    "<img>"
                } else {
                    "<!-- <script><img> -->"
                };
                b.representations[1].bytes =
                    format!("<img src='data:image/png;base64,{png}'>{extra}").into_bytes();
            }
            let id = db.add_composite(&b, None, "self").unwrap();
            let stored: Vec<u8> = db
                .conn
                .query_row("SELECT data FROM history WHERE id=?1", [id], |r| r.get(0))
                .unwrap();
            let m = manifest(&stored).unwrap();
            let part = m
                .parts
                .iter()
                .find(|p| p.role == "representation" && p.mime == Mime::Html as u8)
                .unwrap();
            match mode {
                "missing" => {
                    db.conn
                        .execute(
                            "DELETE FROM history_parts WHERE history_id=?1 AND part_id=?2",
                            params![id, part.id],
                        )
                        .unwrap();
                }
                "corrupt" => {
                    db.conn
                        .execute(
                            "UPDATE history_parts SET stored=?3 WHERE history_id=?1 AND part_id=?2",
                            params![id, part.id, crypto::encrypt(b"changed bytes").unwrap()],
                        )
                        .unwrap();
                }
                _ => {}
            }
            let verify = |rows: Vec<HistoryEntry>| {
                assert_eq!(rows.len(), 1);
                let info = rows[0].composite.as_ref().unwrap();
                assert!(info.formats.iter().any(|f| f == "html"));
                let allowed = mode == "comment";
                assert_eq!(
                    info.restorable_formats.iter().any(|f| f == "html"),
                    allowed,
                    "{mode}"
                );
                assert!(info.restorable_formats.iter().any(|f| f == "text"));
                assert_eq!(
                    info.withheld_formats
                        .iter()
                        .any(|f| f.format == "html" && f.code == "resources_incomplete"),
                    !allowed
                );
            };
            verify(db.get_all(None, None, 10, 0).unwrap());
            verify(db.get_all(Some("retained"), None, 10, 0).unwrap());
            let revision = db.read_revision().unwrap();
            let chunk = db
                .prepare_read_chunk(
                    HistoryQuery {
                        collection: HistoryCollection::All,
                        keyword: None,
                        category: None,
                        start_time: None,
                        end_time: None,
                        limit: 10,
                        offset: 0,
                    },
                    None,
                    &revision,
                    10,
                )
                .unwrap();
            // The file handles and bytes are now owned; no DB borrow is needed.
            drop(db);
            verify(
                chunk
                    .matching_entries(None, &crate::cancellation::Cancellation::default())
                    .unwrap(),
            );
            std::fs::remove_dir_all(root).unwrap();
        }
    }
    #[test]
    fn o11_different_originals_same_portable_are_preserved_after_restart() {
        use crate::protocol::native_clipboard::{NativeFormat, SnapshotState};
        let root = root();
        let b = bundle("same portable projection");
        let native = |name: &str| NativeSnapshot {
            before: 7,
            after: 7,
            items: 1,
            state: SnapshotState::Stable,
            formats: vec![NativeFormat {
                item: 0,
                name: "com.apple.flat-rtfd".into(),
                state: FormatState::Captured,
                bytes: name.as_bytes().to_vec(),
            }],
        };
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let first = db
            .add_composite(&b, Some(&native("attachment-a.png")), "self")
            .unwrap();
        let second = db
            .add_composite(&b, Some(&native("attachment-b.png")), "self")
            .unwrap();
        drop(db);
        let db = HistoryDB::open_at(&root).unwrap();
        assert_eq!(db.get_all(None, None, 20, 0).unwrap().len(), 2);
        assert_eq!(
            db.get_composite_native(first).unwrap().unwrap(),
            NativeSnapshot {
                before: 0,
                after: 0,
                ..native("attachment-a.png")
            }
        );
        assert_eq!(
            db.get_composite_native(second).unwrap().unwrap(),
            NativeSnapshot {
                before: 0,
                after: 0,
                ..native("attachment-b.png")
            }
        );
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn o11_safe_portable_dedupe_preserves_favorites_and_protected_rows() {
        let root = root();
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let b = bundle("safe content");
        let favorite = db.add_composite(&b, None, "self").unwrap();
        db.set_favorite(favorite, true).unwrap();
        let held = db.add_composite(&b, None, "self").unwrap();
        db.protect_history(held);
        let replaced = db.add_composite(&b, None, "self").unwrap();
        let old_identity = db.composite_identity(replaced).unwrap().unwrap();
        let current = db.add_composite(&b, None, "self").unwrap();
        assert!(db.get_composite(replaced).is_err());
        assert_eq!(db.get_all(None, None, 20, 0).unwrap().len(), 3);
        assert!(old_identity.may_rebind(&db.composite_identity(current).unwrap().unwrap()));
        assert!(db.get_composite(favorite).is_ok());
        assert!(db.get_composite(held).is_ok());
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn o11_unclosed_and_legacy_captures_never_delete_a_row_by_portable_hash() {
        let root = root();
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        for completeness in [
            Completeness::ExternalReferences,
            Completeness::MissingFormats,
        ] {
            let mut b = bundle("unclosed");
            b.completeness = completeness;
            let first = db.add_composite(&b, None, "self").unwrap();
            let second = db.add_composite(&b, None, "self").unwrap();
            assert!(db.get_composite(first).is_ok());
            assert!(db.get_composite(second).is_ok());
            assert!(db
                .composite_identity(second)
                .unwrap()
                .unwrap()
                .dedupe_key
                .is_none());
        }
        // A dishonest completeness label cannot close a live external reference.
        let mut b = bundle("url");
        b.representations[1].bytes = b"<img src='https://fixture.invalid/same.png'>".to_vec();
        assert!(identity::portable_dedupe_key(&b).unwrap().is_none());
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn o11_archive_cas_preserves_capture_hash_and_rejects_late_deleted_wrong_tasks() {
        let root = root();
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let b = bundle("archive");
        let id = db.add_composite(&b, None, "self").unwrap();
        let original = db.composite_identity(id).unwrap().unwrap();
        let hash = db.get_all(None, None, 20, 0).unwrap()[0].data_hash.clone();
        let mut wrong = original.clone();
        wrong.capture_id = hex::encode([8u8; 16]);
        assert!(!db
            .update_composite_archive_summary(id, &wrong, "wrong")
            .unwrap());
        wrong = original.clone();
        wrong.content_id = "different".into();
        assert!(!db
            .update_composite_archive_summary(id, &wrong, "wrong")
            .unwrap());
        assert!(db
            .update_composite_archive_summary(id, &original, "new preview")
            .unwrap());
        assert!(!db
            .update_composite_archive_summary(id, &original, "late preview")
            .unwrap());
        let current = db.composite_identity(id).unwrap().unwrap();
        assert!(original.same_capture(&current));
        assert_eq!(current.archive_revision, 2);
        assert_ne!(original.archive_digest, current.archive_digest);
        assert_eq!(original.dedupe_key, current.dedupe_key);
        assert_eq!(db.get_composite(id).unwrap(), b);
        assert_eq!(db.get_all(None, None, 20, 0).unwrap()[0].data_hash, hash);
        db.delete(id).unwrap();
        assert!(!db
            .update_composite_archive_summary(id, &current, "deleted")
            .unwrap());
        assert!(db.composite_identity(id).is_err());
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
    fn derived_bundle(padding: usize) -> PortableBundle {
        let png = crate::composite::packed_to_png(1, 1, &[10, 20, 30, 255]).unwrap();
        let mut html = String::from("<p>");
        html.push_str(&"x".repeat(padding));
        html.push_str("</p><img src=\"tailsync-resource:1\">");
        let offset = html.find("tailsync-resource:1").unwrap();
        PortableBundle {
            representations: vec![Representation {
                mime: Mime::Html,
                bytes: html.into_bytes(),
            }],
            resources: vec![Resource { id: 1, png }],
            bindings: vec![ResourceBinding {
                representation: 0,
                resource: 1,
                offset: offset as u32,
                length: 19,
            }],
            completeness: Completeness::Complete,
        }
    }
    fn archive_base(padding: usize) -> PortableBundle {
        let mut base = derived_bundle(padding);
        base.representations[0].bytes = std::str::from_utf8(&base.representations[0].bytes)
            .unwrap()
            .replace("tailsync-resource:1", "https://fixture.example/a.png")
            .into_bytes();
        base.resources.clear();
        base.bindings.clear();
        base.completeness = Completeness::ExternalReferences;
        base
    }
    #[test]
    fn o04_archive_commit_preserves_base_and_exposes_derived_layer() {
        let _pipeline = crate::composite::budgets::pipeline_budget_test_lock();
        let root = root();
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let base = archive_base(0);
        let id = db.add_composite(&base, None, "self").unwrap();
        db.set_favorite(id, true).unwrap();
        let original = db.composite_identity(id).unwrap().unwrap();
        let hash = db.get_all(None, None, 20, 0).unwrap()[0].data_hash.clone();
        let derived = derived_bundle(0);
        let committed = db
            .commit_composite_archive(id, &original, &derived)
            .unwrap()
            .unwrap();
        assert_eq!(committed.capture_id, original.capture_id);
        assert_eq!(committed.content_id, original.content_id);
        assert_eq!(committed.dedupe_key, original.dedupe_key);
        assert_eq!(committed.archive_revision, original.archive_revision + 1);
        assert_ne!(committed.archive_digest, original.archive_digest);
        assert_eq!(db.get_composite(id).unwrap(), base);
        assert_eq!(db.get_composite_archived(id).unwrap().unwrap(), derived);
        let entry = &db.get_all(None, None, 20, 0).unwrap()[0];
        assert_eq!(entry.data_hash, hash);
        assert!(entry.pinned);
        assert!(db.get_composite_image(id).is_ok());
        drop(db);
        let db = HistoryDB::open_at(&root).unwrap();
        assert_eq!(db.get_composite_archived(id).unwrap().unwrap(), derived);
        assert_eq!(db.get_composite(id).unwrap(), base);
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn o05_source_archive_composes_on_top_of_a_public_archive_layer() {
        use crate::composite::resource_archive::{apply_resource_bindings, ResourceReplacement};
        use crate::composite::source_context as sc;
        use base64::Engine as _;
        let root = root();
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let html = r#"<img src="https://example.com/a.png"><img src="blob:https://example.com/b">"#;
        let base = PortableBundle {
            representations: vec![
                Representation {
                    mime: Mime::Text,
                    bytes: b"t".to_vec(),
                },
                Representation {
                    mime: Mime::Html,
                    bytes: html.as_bytes().to_vec(),
                },
            ],
            resources: Vec::new(),
            bindings: Vec::new(),
            completeness: Completeness::ExternalReferences,
        };
        let id = db.add_composite(&base, None, "self").unwrap();
        let identity = db.composite_identity(id).unwrap().unwrap();
        let spans = crate::composite::html::reference_inventory(&base.representations[1].bytes)
            .unwrap()
            .images;
        let public_png = crate::composite::packed_to_png(1, 1, &[1, 2, 3, 4]).unwrap();
        let (v3, _dedup) = apply_resource_bindings(
            &base,
            &[ResourceReplacement {
                span: spans[0],
                png: public_png,
            }],
        )
        .unwrap();
        let committed = db
            .commit_composite_archive(id, &identity, &v3)
            .unwrap()
            .unwrap();

        let layer = db.get_composite_archived(id).unwrap().unwrap();
        let missing = sc::missing_source_candidates(&base, Some(&layer)).unwrap();
        assert_eq!(missing.len(), 1);
        let (origin, nodes) = sc::select_grant(&missing).unwrap();
        assert_eq!(origin, "https://example.com");
        let blob_png = crate::composite::packed_to_png(2, 2, &[9u8; 16]).unwrap();
        let target = &missing[0];
        let packet = sc::SourcePacket {
            kind: sc::SOURCE_PACKET_KIND.to_string(),
            version: 1,
            grant_id: "ab".repeat(32),
            origin: origin.clone(),
            entries: vec![sc::SourcePacketEntry {
                index: target.index as u16,
                src_sha256: target.src_sha256.clone(),
                png_b64: base64::engine::general_purpose::STANDARD.encode(&blob_png),
            }],
        };
        let resolved = sc::resolve_nodes(&layer, &packet, &origin, &nodes).unwrap();
        let (v4, proof, _dedup) = sc::derive_source_archive(&layer, &resolved, &origin).unwrap();
        let committed = db
            .commit_composite_source_archive_with_cancellation(
                id,
                &committed,
                &v4,
                &proof,
                &crate::cancellation::Cancellation::default(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(committed.archive_revision, 3);
        assert_eq!(db.get_composite(id).unwrap(), base);
        let archived = db.get_composite_archived(id).unwrap().unwrap();
        assert_eq!(archived.resources.len(), 2);
        drop(db);
        let db = HistoryDB::open_at(&root).unwrap();
        assert_eq!(db.get_composite_archived(id).unwrap().unwrap(), archived);
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn o04_archive_commit_rejects_stale_wrong_or_deleted() {
        let root = root();
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let id = db.add_composite(&archive_base(0), None, "self").unwrap();
        let original = db.composite_identity(id).unwrap().unwrap();
        let derived = derived_bundle(0);
        let mut wrong = original.clone();
        wrong.capture_id = hex::encode([3u8; 16]);
        assert!(db
            .commit_composite_archive(id, &wrong, &derived)
            .unwrap()
            .is_none());
        let mut stale = original.clone();
        stale.archive_revision = 5;
        assert!(db
            .commit_composite_archive(id, &stale, &derived)
            .unwrap()
            .is_none());
        let mut other = original.clone();
        other.content_id = "hcp1:other".into();
        assert!(db
            .commit_composite_archive(id, &other, &derived)
            .unwrap()
            .is_none());
        assert!(db
            .commit_composite_archive(id, &original, &derived)
            .unwrap()
            .is_some());
        let held = db.composite_identity(id).unwrap().unwrap();
        assert!(db
            .commit_composite_archive(id, &original, &derived)
            .unwrap()
            .is_none());
        let mut replaced = derived_bundle(0);
        replaced.resources[0].png =
            crate::composite::packed_to_png(1, 1, &[30, 20, 10, 255]).unwrap();
        assert!(db.commit_composite_archive(id, &held, &replaced).is_err());
        assert_eq!(db.get_composite_archived(id).unwrap().unwrap(), derived);
        db.delete(id).unwrap();
        assert!(db
            .commit_composite_archive(id, &held, &derived)
            .unwrap()
            .is_none());
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }

    fn source_base() -> PortableBundle {
        PortableBundle {
            representations: vec![
                Representation {
                    mime: Mime::Text,
                    bytes: b"caption".to_vec(),
                },
                Representation {
                    mime: Mime::Html,
                    bytes: br#"<p>hi</p><img src="blob:https://example.com/one">"#.to_vec(),
                },
            ],
            resources: Vec::new(),
            bindings: Vec::new(),
            completeness: Completeness::ExternalReferences,
        }
    }
    fn source_derived(
        base: &PortableBundle,
        png: &[u8],
    ) -> (
        PortableBundle,
        crate::composite::source_context::SourceProof,
    ) {
        use crate::composite::source_context as sc;
        use base64::Engine as _;
        let candidates = sc::source_candidates(base).unwrap();
        let (origin, nodes) = sc::select_grant(&candidates).unwrap();
        let target = candidates
            .iter()
            .find(|candidate| candidate.origin.is_some())
            .unwrap();
        let packet = sc::SourcePacket {
            kind: sc::SOURCE_PACKET_KIND.to_string(),
            version: 1,
            grant_id: "ab".repeat(32),
            origin: origin.clone(),
            entries: vec![sc::SourcePacketEntry {
                index: target.index as u16,
                src_sha256: target.src_sha256.clone(),
                png_b64: base64::engine::general_purpose::STANDARD.encode(png),
            }],
        };
        let resolved = sc::resolve_nodes(base, &packet, &origin, &nodes).unwrap();
        let (derived, proof, _deduplicated) =
            sc::derive_source_archive(base, &resolved, &origin).unwrap();
        (derived, proof)
    }
    #[test]
    fn o05_source_archive_commits_v4_and_survives_reopen() {
        let root = root();
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let base = source_base();
        let id = db.add_composite(&base, None, "self").unwrap();
        let original = db.composite_identity(id).unwrap().unwrap();
        let png = crate::composite::packed_to_png(2, 2, &[9u8; 16]).unwrap();
        let (derived, proof) = source_derived(&base, &png);
        let committed = db
            .commit_composite_source_archive_with_cancellation(
                id,
                &original,
                &derived,
                &proof,
                &crate::cancellation::Cancellation::default(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(committed.archive_revision, original.archive_revision + 1);
        assert_eq!(committed.content_id, original.content_id);
        assert_eq!(db.get_composite(id).unwrap(), base);
        assert_eq!(db.get_composite_archived(id).unwrap().unwrap(), derived);
        let stored: Vec<u8> = db
            .conn
            .query_row("SELECT data FROM history WHERE id=?1", [id], |row| {
                row.get(0)
            })
            .unwrap();
        let manifest: serde_json::Value =
            serde_json::from_slice(&crypto::decrypt(&stored).unwrap()).unwrap();
        assert_eq!(manifest["version"], 4);
        assert_eq!(
            manifest["integrity_method"],
            "portable-cbp1+resource-archive+source-context"
        );
        assert_eq!(manifest["source_info"]["origin"], proof.origin);
        drop(db);
        let db = HistoryDB::open_at(&root).unwrap();
        assert_eq!(db.get_composite_archived(id).unwrap().unwrap(), derived);
        assert_eq!(db.get_composite(id).unwrap(), base);
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn o05_source_archive_rejects_public_overwrite_and_wrong_proof() {
        let root = root();
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let base = source_base();
        let id = db.add_composite(&base, None, "self").unwrap();
        let original = db.composite_identity(id).unwrap().unwrap();
        let png = crate::composite::packed_to_png(2, 2, &[9u8; 16]).unwrap();
        let (derived, proof) = source_derived(&base, &png);
        let mut wrong = proof.clone();
        wrong.nodes[0].src_sha256 = "00".repeat(32);
        assert!(db
            .commit_composite_source_archive_with_cancellation(
                id,
                &original,
                &derived,
                &wrong,
                &crate::cancellation::Cancellation::default(),
            )
            .is_err());
        let committed = db
            .commit_composite_source_archive_with_cancellation(
                id,
                &original,
                &derived,
                &proof,
                &crate::cancellation::Cancellation::default(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(committed.archive_revision, original.archive_revision + 1);
        let mut public = derived.clone();
        public.resources.clear();
        public.bindings.clear();
        public.representations[1].bytes =
            br#"<p>hi</p><img src="blob:https://example.com/one">"#.to_vec();
        assert!(db
            .commit_composite_archive(id, &committed, &public)
            .is_err());
        assert_eq!(db.get_composite_archived(id).unwrap().unwrap(), derived);
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn o04_review_archive_cas_covers_identity_and_encrypted_manifest() {
        let root = root();
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let base = archive_base(0);
        let id = db.add_composite(&base, None, "self").unwrap();
        let identity = db.composite_identity(id).unwrap().unwrap();
        let before: Vec<u8> = db
            .conn
            .query_row("SELECT data FROM history WHERE id=?1", [id], |row| {
                row.get(0)
            })
            .unwrap();
        for mutation in [
            "UPDATE history_composite_identity SET version=version+1 WHERE history_id=NEW.history_id;",
            "UPDATE history_composite_identity SET dedupe_key='changed' WHERE history_id=NEW.history_id;",
            "UPDATE history SET data=X'00' WHERE id=NEW.history_id;",
        ] {
            db.conn.execute_batch(&format!("CREATE TRIGGER o04_cas_race AFTER INSERT ON history_parts WHEN NEW.kind='archive_layer' BEGIN {mutation} END;")).unwrap();
            assert!(db.commit_composite_archive(id, &identity, &derived_bundle(80 * 1024)).is_err());
            db.conn.execute_batch("DROP TRIGGER o04_cas_race").unwrap();
            assert_eq!(db.composite_identity(id).unwrap().unwrap(), identity);
            assert_eq!(db.get_composite(id).unwrap(), base);
            assert!(db.get_composite_archived(id).unwrap().is_none());
            let after: Vec<u8> = db.conn.query_row("SELECT data FROM history WHERE id=?1", [id], |row| row.get(0)).unwrap();
            assert_eq!(after, before);
            assert!(std::fs::read_dir(root.join("image-history")).unwrap().next().is_none());
        }
        let cancellation = crate::cancellation::Cancellation::default();
        cancellation.cancel();
        assert!(db
            .commit_composite_archive_with_cancellation(
                id,
                &identity,
                &derived_bundle(80 * 1024),
                &cancellation
            )
            .is_err());
        assert_eq!(db.composite_identity(id).unwrap().unwrap(), identity);
        assert!(db.get_composite_archived(id).unwrap().is_none());
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn o04_archive_commit_quota_failure_leaves_manifest_and_payloads_untouched() {
        let root = root();
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let id = db
            .add_composite(&archive_base(80 * 1024), None, "self")
            .unwrap();
        let original = db.composite_identity(id).unwrap().unwrap();
        let before: Vec<u8> = db
            .conn
            .query_row("SELECT data FROM history WHERE id=?1", [id], |r| r.get(0))
            .unwrap();
        let directory = db.image_history_dir.clone();
        let payloads = || {
            std::fs::read_dir(&directory)
                .map(|dir| dir.count())
                .unwrap_or(0)
        };
        let before_payloads = payloads();
        db.set_storage_quota(1);
        let large = derived_bundle(80 * 1024);
        assert!(db
            .commit_composite_archive(id, &original, &large)
            .unwrap_err()
            .to_string()
            .contains("quota"));
        let after: Vec<u8> = db
            .conn
            .query_row("SELECT data FROM history WHERE id=?1", [id], |r| r.get(0))
            .unwrap();
        assert_eq!(before, after);
        assert_eq!(db.composite_identity(id).unwrap().unwrap(), original);
        assert!(db.get_composite_archived(id).unwrap().is_none());
        assert_eq!(payloads(), before_payloads);
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn o04_review_unrelated_derived_content_and_old_layer_version_are_rejected() {
        let root = root();
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let id = db.add_composite(&archive_base(0), None, "self").unwrap();
        let original = db.composite_identity(id).unwrap().unwrap();
        let mut unrelated = derived_bundle(8);
        assert!(db
            .commit_composite_archive(id, &original, &unrelated)
            .is_err());
        assert_eq!(db.composite_identity(id).unwrap(), Some(original.clone()));
        unrelated = derived_bundle(0);
        db.commit_composite_archive(id, &original, &unrelated)
            .unwrap()
            .unwrap();
        let stored: Vec<u8> = db
            .conn
            .query_row("SELECT data FROM history WHERE id=?1", [id], |row| {
                row.get(0)
            })
            .unwrap();
        let mut current = manifest(&stored).unwrap();
        assert_eq!(current.version, 3);
        current.version = 2;
        assert!(validate_manifest(&current).is_err());
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn o04_review_resource_layer_preserves_the_cna1_original_and_its_identity_domain() {
        let root = root();
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let base = archive_base(0);
        let archive = NativeArchive {
            platform: 1,
            enumeration_complete: true,
            derived: Vec::new(),
            original: NativeSnapshot {
                before: 17,
                after: 17,
                items: 1,
                state: crate::protocol::native_clipboard::SnapshotState::Stable,
                formats: vec![crate::protocol::native_clipboard::NativeFormat {
                    item: 0,
                    name: "public.html".into(),
                    state: FormatState::Captured,
                    bytes: base.representations[0].bytes.clone(),
                }],
            },
        };
        let id = db.add_native_archive(&archive, "self").unwrap();
        let original = db.composite_identity(id).unwrap().unwrap();
        db.commit_composite_archive(id, &original, &derived_bundle(0))
            .unwrap()
            .unwrap();
        assert_eq!(db.get_composite_archive(id).unwrap().unwrap(), archive);
        assert_eq!(
            db.composite_identity(id).unwrap().unwrap().content_id,
            original.content_id
        );
        drop(db);
        let db = HistoryDB::open_at(&root).unwrap();
        assert_eq!(db.get_composite_archive(id).unwrap().unwrap(), archive);
        assert_eq!(
            db.get_composite_archived(id).unwrap().unwrap(),
            derived_bundle(0)
        );
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn o04_review_sql_failure_cleans_new_payload_but_keeps_original_and_favorite() {
        let root = root();
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let base = archive_base(80 * 1024);
        let id = db.add_composite(&base, None, "self").unwrap();
        db.set_favorite(id, true).unwrap();
        let original = db.composite_identity(id).unwrap().unwrap();
        let before: Vec<u8> = db
            .conn
            .query_row("SELECT data FROM history WHERE id=?1", [id], |row| {
                row.get(0)
            })
            .unwrap();
        let paths = || {
            std::fs::read_dir(root.join("image-history"))
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect::<std::collections::BTreeSet<_>>()
        };
        let initial_paths = paths();
        db.conn.execute_batch("CREATE TRIGGER o04_archive_failure BEFORE INSERT ON history_parts WHEN NEW.kind='archive_layer' BEGIN SELECT RAISE(ABORT, 'archive write refused'); END;").unwrap();
        assert!(db
            .commit_composite_archive(id, &original, &derived_bundle(80 * 1024))
            .is_err());
        assert_eq!(paths(), initial_paths);
        assert_eq!(db.composite_identity(id).unwrap(), Some(original.clone()));
        assert_eq!(db.get_composite(id).unwrap(), base);
        assert!(db.get_all(None, None, 10, 0).unwrap()[0].pinned);
        let after: Vec<u8> = db
            .conn
            .query_row("SELECT data FROM history WHERE id=?1", [id], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(before, after);
        db.conn
            .execute_batch("DROP TRIGGER o04_archive_failure")
            .unwrap();
        db.commit_composite_archive(id, &original, &derived_bundle(80 * 1024))
            .unwrap()
            .unwrap();
        db.set_favorite(id, false).unwrap();
        db.delete(id).unwrap();
        assert!(paths().is_empty());
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn o04_review_reopen_invalidates_authority_but_a_new_copy_does_not() {
        let root = root();
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let id = db.add_composite(&archive_base(0), None, "self").unwrap();
        let identity = db.composite_identity(id).unwrap();
        let authority = db.read_revision().unwrap();
        db.add_composite(&bundle("later user copy"), None, "self")
            .unwrap();
        assert!(db.validate_storage_authority(&authority).is_ok());
        let previous = get_storage_dir();
        db.reopen_storage_at(&root).unwrap();
        assert_eq!(db.composite_identity(id).unwrap(), identity);
        assert!(db.validate_storage_authority(&authority).is_err());
        configure_storage_dir(Some(&previous)).unwrap();
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn o11_relation_tampering_is_rejected_before_restore() {
        let root = root();
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let id = db.add_composite(&bundle("checked"), None, "self").unwrap();
        db.conn
            .execute(
                "UPDATE history_composite_identity SET archive_revision=2 WHERE history_id=?1",
                [id],
            )
            .unwrap();
        assert!(db.get_composite(id).is_err());
        assert!(db.composite_identity(id).is_err());
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn o11_degraded_recovery_preserves_distinct_captures_with_equal_data_hash() {
        use crate::protocol::native_clipboard::{NativeFormat, SnapshotState};
        let mut source =
            HistoryDB::new_unavailable_with_policy(MigrationPolicy::CompositeAllowed).unwrap();
        let b = bundle("same projection");
        for name in ["attachment-a.png", "attachment-b.png"] {
            let native = NativeSnapshot {
                before: 0,
                after: 0,
                items: 1,
                state: SnapshotState::Stable,
                formats: vec![NativeFormat {
                    item: 0,
                    name: "com.apple.flat-rtfd".into(),
                    state: FormatState::Captured,
                    bytes: name.as_bytes().to_vec(),
                }],
            };
            let id = source.add_composite(&b, Some(&native), "self").unwrap();
            source.set_favorite(id, true).unwrap();
        }
        let root = root();
        let mut target = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        assert_eq!(
            super::super::open::migrate_degraded_history_into(
                &source.conn,
                &target.conn,
                true,
                true
            )
            .unwrap(),
            2
        );
        assert_eq!(
            super::super::open::migrate_degraded_history_into(
                &source.conn,
                &target.conn,
                true,
                true
            )
            .unwrap(),
            0
        );
        assert_eq!(target.get_all(None, None, 20, 0).unwrap().len(), 2);
        for row in target.get_all(None, None, 20, 0).unwrap() {
            assert!(row.pinned);
            assert!(target.get_composite_native(row.id).unwrap().is_some());
            assert!(target.composite_identity(row.id).unwrap().is_some());
        }
        let id = target.get_all(None, None, 20, 0).unwrap()[0].id;
        let held = target.composite_identity(id).unwrap().unwrap();
        assert!(target
            .update_composite_archive_summary(id, &held, "newer destination archive")
            .unwrap());
        assert!(super::super::open::migrate_degraded_history_into(
            &source.conn,
            &target.conn,
            true,
            true
        )
        .is_err());
        assert_eq!(
            target
                .composite_identity(id)
                .unwrap()
                .unwrap()
                .archive_revision,
            2
        );
        assert_eq!(source.get_all(None, None, 20, 0).unwrap().len(), 2);
        drop(source);
        drop(target);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn o11_v12_upgrade_preserves_legacy_bytes_and_copies_composite_payloads() {
        use crate::protocol::native_clipboard::{NativeFormat, SnapshotState};
        let root = root();
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        db.conn.execute_batch("DROP TABLE history_composite_identity; DELETE FROM schema_version WHERE version>=13;").unwrap();
        let b = bundle(&"legacy".repeat(20_000));
        let native = NativeSnapshot {
            before: 0,
            after: 0,
            items: 1,
            state: SnapshotState::Stable,
            formats: vec![NativeFormat {
                item: 0,
                name: "com.apple.flat-rtfd".into(),
                state: FormatState::Captured,
                bytes: vec![7; 128 * 1024],
            }],
        };
        let id = db.add_composite(&b, Some(&native), "self").unwrap();
        db.set_favorite(id, true).unwrap();
        let encrypted: Vec<u8> = db
            .conn
            .query_row("SELECT data FROM history WHERE id=?1", [id], |r| r.get(0))
            .unwrap();
        let hash: String = db
            .conn
            .query_row("SELECT data_hash FROM history WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .unwrap();
        drop(db);
        let db = HistoryDB::open_at_with_policy(&root, MigrationPolicy::CompositeAllowed).unwrap();
        assert_eq!(schema::schema_version(&db.conn).unwrap(), 13);
        assert_eq!(
            db.conn
                .query_row::<Vec<u8>, _, _>("SELECT data FROM history WHERE id=?1", [id], |r| r
                    .get(0))
                .unwrap(),
            encrypted
        );
        assert_eq!(db.get_composite(id).unwrap(), b);
        assert_eq!(db.get_composite_native(id).unwrap().unwrap(), native);
        assert!(db.composite_identity(id).unwrap().is_none());
        assert_eq!(db.get_all(None, None, 20, 0).unwrap()[0].data_hash, hash);
        let package = rollback::verify_v13(&root).unwrap();
        assert_eq!(package.source_schema_version, 12);
        assert!(package.payloads.len() >= 2);
        let restored = root.with_extension("restored");
        rollback::restore_v13_into(&root, &restored).unwrap();
        let old = HistoryDB::open_at_with_policy(&restored, MigrationPolicy::LegacyOnly).unwrap();
        assert_eq!(schema::schema_version(&old.conn).unwrap(), 12);
        assert_eq!(old.get_composite(id).unwrap(), b);
        assert!(old.get_all(None, None, 20, 0).unwrap()[0].pinned);
        assert!(rollback::restore_v13_into(&root, &restored).is_err());
        drop(old);
        drop(db);
        std::fs::remove_dir_all(restored).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn o11_failed_v13_commit_leaves_v12_readable_and_retryable() {
        let root = root();
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        db.conn.execute_batch("DROP TABLE history_composite_identity; DELETE FROM schema_version WHERE version>=13;").unwrap();
        let b = bundle("unchanged on failure");
        let id = db.add_composite(&b, None, "self").unwrap();
        db.conn.execute_batch("CREATE TRIGGER reject_v13 BEFORE INSERT ON schema_version WHEN NEW.version=13 BEGIN SELECT RAISE(ABORT,'injected migration failure'); END;").unwrap();
        drop(db);
        assert!(HistoryDB::open_at_with_policy(&root, MigrationPolicy::CompositeAllowed).is_err());
        let db = HistoryDB::open_at_with_policy(&root, MigrationPolicy::LegacyOnly).unwrap();
        assert_eq!(schema::schema_version(&db.conn).unwrap(), 12);
        assert!(!schema::identity_schema_present(&db.conn).unwrap());
        assert_eq!(db.get_composite(id).unwrap(), b);
        assert!(rollback::verify_v13(&root).is_ok());
        db.conn.execute_batch("DROP TRIGGER reject_v13").unwrap();
        drop(db);
        let db = HistoryDB::open_at_with_policy(&root, MigrationPolicy::CompositeAllowed).unwrap();
        assert_eq!(db.get_composite(id).unwrap(), b);
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn o11_sqlite_full_during_v13_ddl_rolls_back_table_and_version() {
        let conn = Connection::open_in_memory().unwrap();
        schema::initialize(&conn).unwrap();
        schema::initialize_composite(&conn).unwrap();
        conn.execute("INSERT INTO schema_version(version) VALUES(12)", [])
            .unwrap();
        HistoryDB::migrate_to(&conn, Path::new("unused"), Path::new("unused"), 12).unwrap();
        conn.execute_batch("CREATE TABLE fill_page(bytes BLOB);")
            .unwrap();
        let pages: i64 = conn
            .query_row("PRAGMA page_count", [], |r| r.get(0))
            .unwrap();
        conn.execute_batch(&format!("PRAGMA max_page_count={pages};"))
            .unwrap();
        // Consume any free slots until SQLite itself returns SQLITE_FULL.
        loop {
            match conn.execute("INSERT INTO fill_page VALUES(randomblob(3000))", []) {
                Ok(_) => continue,
                Err(rusqlite::Error::SqliteFailure(error, _))
                    if error.code == rusqlite::ErrorCode::DiskFull =>
                {
                    break
                }
                Err(error) => panic!("unexpected filler failure: {error}"),
            }
        }
        let error =
            HistoryDB::migrate_to(&conn, Path::new("unused"), Path::new("unused"), 13).unwrap_err();
        assert!(error.to_string().contains("full"));
        assert_eq!(schema::schema_version(&conn).unwrap(), 12);
        assert!(!schema::identity_schema_present(&conn).unwrap());
    }
    #[test]
    fn o11_v13_rollback_tampering_prevents_upgrade_without_touching_rows() {
        let root = root();
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        db.conn.execute_batch("DROP TABLE history_composite_identity; DELETE FROM schema_version WHERE version>=13;").unwrap();
        let b = bundle("keep me");
        let id = db.add_composite(&b, None, "self").unwrap();
        rollback::prepare_for(
            &db.conn,
            &root.join("history-v2.db"),
            &root,
            &db.file_history_dir,
            &db.image_history_dir,
            12,
            13,
        )
        .unwrap();
        drop(db);
        std::fs::write(
            root.join("pre-v13-rollback/history-before-v13.db"),
            b"truncated",
        )
        .unwrap();
        assert!(HistoryDB::open_at_with_policy(&root, MigrationPolicy::CompositeAllowed).is_err());
        let db = HistoryDB::open_at_with_policy(&root, MigrationPolicy::LegacyOnly).unwrap();
        assert_eq!(schema::schema_version(&db.conn).unwrap(), 12);
        assert_eq!(db.get_composite(id).unwrap(), b);
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }

    /// R5-02 / H-2 — the pending-payload guard removes what a failed write orphaned,
    /// and never removes a payload a committed entry still references.
    ///
    /// Before the guard, cleanup depended on an explicit call at each exit, so the
    /// manifest encryption's `?` in `add_composite_event` returned early and leaked
    /// every payload file it had just written.
    #[test]
    fn rr23_pending_payloads_guard_removes_orphans_and_keeps_live_payloads() {
        let root = root();
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();

        // A committed entry whose part is large enough to be stored externally
        // (> 64 KiB), so it owns a real payload file.
        let big = vec![b'x'; 128 * 1024];
        let bundle = PortableBundle {
            representations: vec![Representation {
                mime: Mime::Text,
                bytes: big,
            }],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::Complete,
        };
        let id = db.add_composite(&bundle, None, "fixture").unwrap();
        let stored: Vec<u8> = db
            .conn
            .query_row(
                "SELECT stored FROM history_parts WHERE history_id=?1",
                [id],
                |row| row.get(0),
            )
            .unwrap();
        let reference = decode_image_reference(&stored).expect("the part must be external");
        let live_path = resolve_file_reference_at(&db.image_history_dir, &reference).unwrap();
        assert!(
            live_path.exists(),
            "fixture must produce an external payload for this regression to mean anything"
        );

        // A payload nothing references, standing in for one a failed write orphaned.
        let orphan = db.image_history_dir.join("orphan-payload.bin");
        std::fs::write(&orphan, b"orphan").unwrap();

        // Dropping the guard without committing is exactly the failure path.
        let guard = db.arm_pending_payloads(&[
            super::super::entries::ExternalHistoryPayload {
                path: orphan.clone(),
            },
            super::super::entries::ExternalHistoryPayload {
                path: live_path.clone(),
            },
        ]);
        drop(guard);

        assert!(
            !orphan.exists(),
            "the guard must remove what the failed write orphaned"
        );
        assert!(
            live_path.exists(),
            "a payload a committed entry still references must never be removed"
        );
        // And the committed entry is still readable.
        assert_eq!(db.get_composite(id).unwrap(), bundle);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn composite_history_roundtrip_search_restart_and_encryption() {
        let root = root();
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let b = bundle("secret composite 中文");
        let id = db.add_composite(&b, None, "fixture").unwrap();
        assert_eq!(db.get_composite(id).unwrap(), b);
        assert_eq!(db.get_all(Some("中文"), None, 20, 0).unwrap().len(), 1);
        let query = HistoryQuery {
            collection: HistoryCollection::All,
            keyword: Some("中文"),
            category: None,
            start_time: None,
            end_time: None,
            limit: 20,
            offset: 0,
        };
        let c = db
            .prepare_read_chunk(query, None, &db.read_revision().unwrap(), 20)
            .unwrap();
        assert_eq!(
            c.matching_entries(Some("中文"), &crate::cancellation::Cancellation::default())
                .unwrap()
                .len(),
            1
        );
        let stored: Vec<u8> = db
            .conn
            .query_row("SELECT data FROM history WHERE id=?1", [id], |r| r.get(0))
            .unwrap();
        assert!(!stored.windows(6).any(|w| w == b"secret"));
        drop(db);
        let db = HistoryDB::open_at(&root).unwrap();
        assert_eq!(db.get_composite(id).unwrap(), b);
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn event_retry_delete_tombstone_and_collision() {
        let root = root();
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let b = bundle("event");
        let event = crate::protocol::MessageId([3; 16]);
        let timestamp = crate::protocol::unix_timestamp_ms();
        let id = db
            .add_composite_event(&b, None, "fixture", Some(("identity", event, timestamp)))
            .unwrap();
        assert_eq!(
            db.add_composite_event(&b, None, "fixture", Some(("identity", event, timestamp)))
                .unwrap(),
            id
        );
        assert!(db
            .add_composite_event(
                &bundle("collision"),
                None,
                "fixture",
                Some(("identity", event, timestamp))
            )
            .is_err());
        db.delete(id).unwrap();
        assert_eq!(
            db.add_composite_event(&b, None, "fixture", Some(("identity", event, timestamp)))
                .unwrap(),
            0
        );
        assert_eq!(
            db.conn
                .query_row("SELECT COUNT(*) FROM history_parts", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn composite_favorite_is_one_protected_item() {
        let root = root();
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let id = db
            .add_composite(&bundle("favorite"), None, "fixture")
            .unwrap();
        db.set_favorite(id, true).unwrap();
        assert!(db.delete(id).is_err());
        db.clear_all().unwrap();
        assert!(db.get_composite(id).is_ok());
        db.delete_favorite(id).unwrap();
        assert!(db.get_composite(id).is_err());
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use rand::{RngCore, SeedableRng};
    fn root() -> PathBuf {
        std::env::temp_dir().join(format!(
            "tailsync-composite-life-{:016x}",
            rand::random::<u64>()
        ))
    }
    fn png() -> Vec<u8> {
        let mut pixels = vec![0; 200 * 200 * 4];
        rand::rngs::StdRng::seed_from_u64(19).fill_bytes(&mut pixels);
        composite::packed_to_png(200, 200, &pixels).unwrap()
    }
    fn bundle(text: &str, png: &[u8]) -> PortableBundle {
        PortableBundle {
            representations: vec![
                Representation {
                    mime: Mime::Text,
                    bytes: text.as_bytes().to_vec(),
                },
                Representation {
                    mime: Mime::Png,
                    bytes: png.to_vec(),
                },
            ],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::Complete,
        }
    }
    #[test]
    fn external_png_shared_across_items_is_removed_only_after_last_reference() {
        let root = root();
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let png = png();
        let a = db
            .add_composite(&bundle("first", &png), None, "fixture")
            .unwrap();
        let b = db
            .add_composite(&bundle("second", &png), None, "fixture")
            .unwrap();
        let payload = root
            .join("image-history")
            .join(format!("{}.bin", blake3::hash(&png).to_hex()));
        assert!(payload.exists());
        assert_eq!(
            std::fs::read_dir(root.join("image-history"))
                .unwrap()
                .count(),
            1
        );
        db.delete(a).unwrap();
        assert!(payload.exists());
        assert_eq!(
            db.get_composite(b)
                .unwrap()
                .representation(Mime::Png)
                .unwrap(),
            png
        );
        db.delete(b).unwrap();
        assert!(!payload.exists());
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn failed_transaction_cleans_new_external_parts_and_quota_refuses_before_write() {
        let root = root();
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let png = png();
        db.reject_composite_inserts_for_test(true).unwrap();
        assert!(db
            .add_composite(&bundle("failure", &png), None, "fixture")
            .is_err());
        assert_eq!(
            std::fs::read_dir(root.join("image-history"))
                .unwrap()
                .count(),
            0
        );
        db.reject_composite_inserts_for_test(false).unwrap();
        db.set_storage_quota(1);
        assert!(db
            .add_composite(&bundle("quota", &png), None, "fixture")
            .is_err());
        assert_eq!(
            std::fs::read_dir(root.join("image-history"))
                .unwrap()
                .count(),
            0
        );
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn multi_item_native_history_survives_degraded_recovery_and_keeps_favorite() {
        use crate::protocol::native_clipboard::*;
        // This degraded store is going to hold composite history, so it must be
        // created with the policy that carries the composite schema. A store
        // opened under LegacyOnly cannot represent composite parts at all.
        let mut db =
            HistoryDB::new_unavailable_with_policy(MigrationPolicy::CompositeAllowed).unwrap();
        let snapshot = NativeSnapshot {
            before: 4,
            after: 4,
            items: 2,
            state: SnapshotState::Stable,
            formats: vec![
                NativeFormat {
                    item: 0,
                    name: "public.utf8-plain-text".into(),
                    state: FormatState::Captured,
                    bytes: b"first native".to_vec(),
                },
                NativeFormat {
                    item: 1,
                    name: "public.utf8-plain-text".into(),
                    state: FormatState::Captured,
                    bytes: b"second native".to_vec(),
                },
            ],
        };
        let id = db.add_native_snapshot(&snapshot, "fixture").unwrap();
        db.set_favorite(id, true).unwrap();
        let root = root();
        db.adopt_replacement(HistoryDB::open_isolated_for_test_composite(&root).unwrap())
            .unwrap();
        let entries = db.get_all(Some("second native"), None, 10, 0).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].pinned);
        assert!(entries[0].composite.as_ref().unwrap().native_only);
        let restored = db.get_composite_native(entries[0].id).unwrap().unwrap();
        assert_eq!(restored.items, 2);
        assert_eq!(restored.formats, snapshot.formats);
        assert!(db.delete(entries[0].id).is_err());
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn preview_uses_safe_summary_and_decodes_only_the_selected_image() {
        let _pipeline = crate::composite::budgets::pipeline_budget_test_lock();
        let root = root();
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let png = png();
        let b = bundle("safe text", &png);
        let id = db.add_composite(&b, None, "fixture").unwrap();
        let p = db.get_preview_payload(id).unwrap();
        assert_eq!(p.kind, "text");
        assert_eq!(p.data, b"safe text");
        let (_, p) = db
            .prepare_composite_image(id)
            .unwrap()
            .read(&crate::cancellation::Cancellation::default())
            .unwrap();
        assert_eq!(p.data, composite::png_to_packed(&png).unwrap());
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn browser_image_with_unavailable_html_reference_previews_saved_png() {
        let _pipeline = crate::composite::budgets::pipeline_budget_test_lock();
        let root = root();
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let png = png();
        for reference in [
            "blob:https://example.com/image",
            "file:///C:/browser/image.png",
        ] {
            let bundle = PortableBundle {
                representations: vec![
                    Representation {
                        mime: Mime::Html,
                        bytes: format!("<html><body><img src='{reference}'></body></html>")
                            .into_bytes(),
                    },
                    Representation {
                        mime: Mime::Png,
                        bytes: png.clone(),
                    },
                ],
                resources: vec![],
                bindings: vec![],
                completeness: Completeness::ExternalReferences,
            };
            let id = db.add_composite(&bundle, None, "browser fixture").unwrap();
            let handle = db
                .prepare_content_analysis(id)
                .unwrap()
                .unwrap()
                .handle
                .clone();
            let classification = composite::display::classify_bundle(&bundle, false, vec![]);
            db.commit_display_classification(
                &handle,
                &classification,
                &crate::cancellation::Cancellation::default(),
            )
            .unwrap();
            let preview = db.get_preview_payload(id).unwrap();
            assert_eq!(preview.kind, "image");
            assert_eq!(preview.data, composite::png_to_packed(&png).unwrap());
            assert_eq!(db.get_composite(id).unwrap(), bundle);
        }
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }

    fn solid_png(red: u8, green: u8, blue: u8) -> Vec<u8> {
        composite::packed_to_png(1, 1, &[red, green, blue, 255]).unwrap()
    }

    #[test]
    fn standalone_portable_windows_and_qt_images_have_image_summaries() {
        let root = root();
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let png = solid_png(255, 0, 0);
        let bundle = PortableBundle {
            representations: vec![Representation {
                mime: Mime::Png,
                bytes: png.clone(),
            }],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::Complete,
        };
        let native = NativeSnapshot {
            before: 1,
            after: 1,
            items: 1,
            state: SnapshotState::Stable,
            formats: vec![NativeFormat {
                item: 0,
                name: "PNG".into(),
                state: FormatState::Captured,
                bytes: png,
            }],
        };
        let mut qt_archive = NativeArchive {
            platform: 1,
            enumeration_complete: true,
            original: native.clone(),
            derived: vec![],
        };
        qt_archive.original.formats[0].name = "public.png".into();
        qt_archive.original.formats.extend([
            NativeFormat {
                item: 0,
                name: "public.file-url".into(),
                state: FormatState::Absent,
                bytes: vec![],
            },
            NativeFormat {
                item: 0,
                name: "com.trolltech.anymime.application--x-qt-image".into(),
                state: FormatState::Captured,
                bytes: vec![],
            },
        ]);
        for input in 0..3 {
            let id = match input {
                0 => db.add_composite(&bundle, None, "fixture"),
                1 => db.add_composite(&bundle, Some(&native), "fixture"),
                _ => db.add_native_archive(&qt_archive, "fixture"),
            }
            .unwrap();
            assert_eq!(db.display_kind(id).unwrap(), "image");
            assert_eq!(
                db.get_all(None, None, 10, 0)
                    .unwrap()
                    .iter()
                    .find(|entry| entry.id == id)
                    .unwrap()
                    .description,
                "image/png · 1×1 · 1 frame"
            );

            // Recreate an older encrypted row with the generic label. Reading
            // corrects its presentation only; identity and stored bytes survive.
            let stored: Vec<u8> = db
                .conn
                .query_row("SELECT data FROM history WHERE id=?1", [id], |r| r.get(0))
                .unwrap();
            let mut m = manifest(&stored).unwrap();
            m.summary = "Rich clipboard content".into();
            let digest = archive_digest(&m).unwrap();
            m.identity.as_mut().unwrap().archive_digest = digest.clone();
            let legacy = crypto::encrypt(&serde_json::to_vec(&m).unwrap()).unwrap();
            db.conn
                .execute(
                    "UPDATE history SET data=?1 WHERE id=?2",
                    params![legacy, id],
                )
                .unwrap();
            db.conn
                .execute(
                    "UPDATE history_composite_identity SET archive_digest=?1 WHERE history_id=?2",
                    params![digest, id],
                )
                .unwrap();
            let identity = db.composite_identity(id).unwrap();
            let page = db
                .prepare_read_chunk(
                    HistoryQuery {
                        collection: HistoryCollection::All,
                        keyword: None,
                        category: None,
                        start_time: None,
                        end_time: None,
                        limit: 10,
                        offset: 0,
                    },
                    None,
                    &db.read_revision().unwrap(),
                    10,
                )
                .unwrap();
            let entries = page
                .matching_entries(None, &crate::cancellation::Cancellation::default())
                .unwrap();
            assert_eq!(
                entries
                    .iter()
                    .find(|entry| entry.id == id)
                    .unwrap()
                    .description,
                "image/png · 1×1 · 1 frame"
            );
            assert_eq!(
                db.get_all(None, None, 10, 0)
                    .unwrap()
                    .iter()
                    .find(|entry| entry.id == id)
                    .unwrap()
                    .description,
                "image/png · 1×1 · 1 frame"
            );
            assert_eq!(db.composite_identity(id).unwrap(), identity);
            let after: Vec<u8> = db
                .conn
                .query_row("SELECT data FROM history WHERE id=?1", [id], |r| r.get(0))
                .unwrap();
            assert_eq!(after, legacy);
            if input == 2 {
                assert_eq!(
                    db.get_composite_archive(id).unwrap(),
                    Some(qt_archive.clone())
                );
                // Do not project a single image title onto multi-item or
                // incomplete archives, even when they contain a PNG cover.
                for (items, completeness) in [
                    (2, Completeness::Complete),
                    (1, Completeness::MissingFormats),
                ] {
                    let mut guarded = m.clone();
                    guarded.native_items = items;
                    guarded.completeness = completeness as u8;
                    guarded.identity.as_mut().unwrap().archive_digest =
                        archive_digest(&guarded).unwrap();
                    assert_eq!(
                        summary_bytes(&serde_json::to_vec(&guarded).unwrap()).unwrap(),
                        b"Rich clipboard content"
                    );
                }
            }
        }
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mixed_rich_content_is_not_relabelled_as_a_standalone_image() {
        let root = root();
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let bundle = PortableBundle {
            representations: vec![
                Representation {
                    mime: Mime::Png,
                    bytes: solid_png(0, 0, 255),
                },
                Representation {
                    mime: Mime::Html,
                    bytes: b"<p>caption</p>".to_vec(),
                },
            ],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::Complete,
        };
        assert_eq!(portable_image_summary(&bundle), None);
        let id = db.add_composite(&bundle, None, "fixture").unwrap();
        let stored: Vec<u8> = db
            .conn
            .query_row("SELECT data FROM history WHERE id=?1", [id], |r| r.get(0))
            .unwrap();
        let mut m = manifest(&stored).unwrap();
        m.summary = "Rich clipboard content".into();
        m.identity.as_mut().unwrap().archive_digest = archive_digest(&m).unwrap();
        assert_eq!(
            summary_bytes(&serde_json::to_vec(&m).unwrap()).unwrap(),
            b"Rich clipboard content"
        );
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn image_classified_composite_preview_reads_pixels_instead_of_summary() {
        let _pipeline = crate::composite::budgets::pipeline_budget_test_lock();
        let root = root();
        let mut database = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let image = solid_png(255, 0, 0);
        let inputs = [
            PortableBundle {
                representations: vec![Representation {
                    mime: Mime::Png,
                    bytes: image.clone(),
                }],
                resources: vec![],
                bindings: vec![],
                completeness: Completeness::Complete,
            },
            document_bundle(
                "<div><img src=\"tailsync-resource:7\"></div>",
                vec![(7, image.clone())],
            ),
        ];
        for input in inputs {
            let id = database.add_composite(&input, None, "fixture").unwrap();
            let prepared = database.prepare_content_analysis(id).unwrap().unwrap();
            let classification = composite::display::classify_bundle(&input, false, vec![]);
            assert!(database
                .commit_display_classification(
                    &prepared.handle(),
                    &classification,
                    &crate::cancellation::Cancellation::default(),
                )
                .unwrap());
            assert_eq!(database.display_kind(id).unwrap(), "image");
            let (metadata, payload) = database
                .prepare_preview(id, None)
                .unwrap()
                .read(&crate::cancellation::Cancellation::default())
                .unwrap();
            assert_eq!(metadata.kind, PreviewKind::Image);
            assert_eq!(payload.kind, "image");
            assert_eq!(payload.data, composite::png_to_packed(&image).unwrap());
            assert_eq!(metadata.size_bytes, payload.data.len() as u64);
            assert_eq!(database.get_preview_payload(id).unwrap(), payload);
        }
        drop(database);
        std::fs::remove_dir_all(root).unwrap();
    }

    fn document_bundle(html: &str, resources: Vec<(u16, Vec<u8>)>) -> PortableBundle {
        let resources: Vec<Resource> = resources
            .into_iter()
            .map(|(id, png)| Resource { id, png })
            .collect();
        let mut bindings = Vec::new();
        for resource in &resources {
            let marker = format!("tailsync-resource:{}", resource.id);
            for (offset, _) in html.match_indices(&marker) {
                bindings.push(ResourceBinding {
                    representation: 0,
                    resource: resource.id,
                    offset: offset as u32,
                    length: marker.len() as u32,
                });
            }
        }
        PortableBundle {
            representations: vec![Representation {
                mime: Mime::Html,
                bytes: html.as_bytes().to_vec(),
            }],
            resources,
            bindings,
            completeness: Completeness::Complete,
        }
    }

    #[test]
    fn composite_image_uses_body_order_not_storage_or_standalone_png() {
        let _pipeline = crate::composite::budgets::pipeline_budget_test_lock();
        let root = root();
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let red = solid_png(255, 0, 0);
        let blue = solid_png(0, 0, 255);
        let green = solid_png(0, 255, 0);
        let html = "<p><img src=\"tailsync-resource:2\"><img src=\"tailsync-resource:1\"></p>";
        let mut bundle = document_bundle(html, vec![(1, blue.clone()), (2, red.clone())]);
        bundle.representations.push(Representation {
            mime: Mime::Png,
            bytes: green.clone(),
        });
        let id = db.add_composite(&bundle, None, "fixture").unwrap();
        let (_, payload) = db
            .prepare_composite_image(id)
            .unwrap()
            .read(&crate::cancellation::Cancellation::default())
            .unwrap();
        assert_eq!(payload.data, composite::png_to_packed(&red).unwrap());
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn composite_image_missing_first_does_not_substitute_the_second() {
        let _pipeline = crate::composite::budgets::pipeline_budget_test_lock();
        let root = root();
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let blue = solid_png(0, 0, 255);
        let html = "<p><img src=\"tailsync-resource:9\"><img src=\"tailsync-resource:1\"></p>";
        let mut bundle = document_bundle(html, vec![(1, blue)]);
        bundle.representations.push(Representation {
            mime: Mime::Png,
            bytes: solid_png(0, 255, 0),
        });
        let id = db.add_composite(&bundle, None, "fixture").unwrap();
        let outcome = db
            .prepare_composite_image(id)
            .unwrap()
            .read(&crate::cancellation::Cancellation::default());
        assert!(outcome.is_err());
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn committed_classification_separates_single_image_from_document() {
        let root = root();
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let png = solid_png(10, 20, 30);
        let single = PortableBundle {
            representations: vec![Representation {
                mime: Mime::Png,
                bytes: png.clone(),
            }],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::Complete,
        };
        let single_id = db.add_composite(&single, None, "fixture").unwrap();
        let document = document_bundle(
            "<p>hello world</p><img src=\"tailsync-resource:1\">",
            vec![(1, png.clone())],
        );
        let document_id = db.add_composite(&document, None, "fixture").unwrap();
        let text = PortableBundle {
            representations: vec![Representation {
                mime: Mime::Text,
                bytes: b"just a plain note".to_vec(),
            }],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::Complete,
        };
        let text_id = db.add_composite(&text, None, "fixture").unwrap();

        let prepared = db.prepare_content_analysis(document_id).unwrap().unwrap();
        let handle = prepared.handle();
        let input = prepared
            .read_input(&crate::cancellation::Cancellation::default())
            .unwrap();
        let classification = composite::display::classify_bundle(&input.bundle, false, vec![]);
        assert!(db
            .commit_display_classification(
                &handle,
                &classification,
                &crate::cancellation::Cancellation::default()
            )
            .unwrap());

        let entries = db.get_all(None, None, 10, 0).unwrap();
        let find = |id: i64| entries.iter().find(|entry| entry.id == id).unwrap();
        assert_eq!(find(single_id).category, "image");
        assert_eq!(find(single_id).categories, vec!["image"]);
        assert_eq!(find(document_id).category, "image");
        assert_eq!(find(document_id).categories, vec!["text", "image"]);
        assert_eq!(find(text_id).categories, vec!["text"]);
        assert_eq!(
            find(document_id).classifier_version,
            composite::display::DISPLAY_CLASSIFIER_VERSION
        );

        let image_filter = db.get_all(None, Some("image"), 10, 0).unwrap();
        let ids: Vec<i64> = image_filter.iter().map(|entry| entry.id).collect();
        assert!(ids.contains(&single_id));
        assert!(ids.contains(&document_id));
        assert!(!ids.contains(&text_id));
        let text_filter = db.get_all(None, Some("text"), 10, 0).unwrap();
        let ids: Vec<i64> = text_filter.iter().map(|entry| entry.id).collect();
        assert!(ids.contains(&document_id));
        assert!(ids.contains(&text_id));
        assert!(!ids.contains(&single_id));
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
}
