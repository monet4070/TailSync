use crate::execution::{run_db_named, run_db_named_cancellable, DbExecutionError};
use crate::history::ArchiveResourceFailure;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tailsync_core::cancellation::Cancellation;
use tailsync_core::composite::identity::HistoryIdentity;
use tailsync_core::composite::source_context::{self as sc, SourceNode};
use tailsync_core::db::{HistoryDB, HistoryReadRevision};
use tokio::sync::{Mutex as AsyncMutex, Semaphore};

pub const SOURCE_GRANT_LIMIT: usize = 8;
pub const SOURCE_IMPORT_ADMISSION: usize = 2;
pub const SOURCE_IMPORT_DEADLINE: Duration = Duration::from_secs(45);

static IMPORT_ADMISSION: Semaphore = Semaphore::const_new(SOURCE_IMPORT_ADMISSION);

#[derive(Clone)]
struct GrantRecord {
    history_id: i64,
    identity: HistoryIdentity,
    authority: HistoryReadRevision,
    origin: String,
    nodes: Vec<SourceNode>,
    budget_bytes: u64,
    expires_at: Instant,
}

struct GrantEntry {
    record: GrantRecord,
    consumed: bool,
    revoked: bool,
    cancellation: Cancellation,
    inflight: Option<Cancellation>,
    finished: bool,
}

#[derive(Default)]
struct RegistryInner {
    grants: HashMap<String, GrantEntry>,
}

pub struct SourceConsentRegistry {
    inner: Mutex<RegistryInner>,
}

impl Default for SourceConsentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub struct GrantHandle {
    pub grant_id: String,
    pub history_id: i64,
    pub identity: HistoryIdentity,
    pub authority: HistoryReadRevision,
    pub origin: String,
    pub nodes: Vec<SourceNode>,
    pub budget_bytes: u64,
    pub expires_at: Instant,
}

impl SourceConsentRegistry {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(RegistryInner::default()),
        }
    }

    fn prune(inner: &mut RegistryInner, now: Instant) {
        inner
            .grants
            .retain(|_, entry| !entry.revoked && !entry.finished && now < entry.record.expires_at);
    }

    fn create(&self, grant_id: String, record: GrantRecord) -> Result<(), String> {
        let now = Instant::now();
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "source_registry".to_string())?;
        Self::prune(&mut inner, now);
        if inner.grants.len() >= SOURCE_GRANT_LIMIT {
            return Err("source_grant_limit".to_string());
        }
        inner.grants.insert(
            grant_id,
            GrantEntry {
                record,
                consumed: false,
                revoked: false,
                cancellation: Cancellation::default(),
                inflight: None,
                finished: false,
            },
        );
        Ok(())
    }

    pub fn revoke(&self, grant_id: &str) -> bool {
        let Ok(mut inner) = self.inner.lock() else {
            return false;
        };
        let Some(entry) = inner.grants.get_mut(grant_id) else {
            return false;
        };
        entry.revoked = true;
        entry.cancellation.cancel();
        if let Some(inflight) = &entry.inflight {
            inflight.cancel();
        }
        true
    }

    pub fn consume(
        &self,
        grant_id: &str,
        origin: &str,
        inflight: Cancellation,
    ) -> Result<GrantHandle, String> {
        let now = Instant::now();
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "source_registry".to_string())?;
        let entry = inner
            .grants
            .get_mut(grant_id)
            .ok_or_else(|| "source_grant_unknown".to_string())?;
        if entry.revoked {
            return Err("source_grant_revoked".to_string());
        }
        if now >= entry.record.expires_at {
            return Err("source_grant_expired".to_string());
        }
        if entry.consumed {
            return Err("source_grant_consumed".to_string());
        }
        if entry.record.origin != origin {
            return Err("source_origin_mismatch".to_string());
        }
        entry.consumed = true;
        entry.inflight = Some(inflight);
        let record = entry.record.clone();
        Ok(GrantHandle {
            grant_id: grant_id.to_string(),
            history_id: record.history_id,
            identity: record.identity,
            authority: record.authority,
            origin: record.origin,
            nodes: record.nodes,
            budget_bytes: record.budget_bytes,
            expires_at: record.expires_at,
        })
    }

    pub fn recheck(&self, grant_id: &str, now: Instant) -> Result<(), String> {
        let inner = self
            .inner
            .lock()
            .map_err(|_| "source_registry".to_string())?;
        let entry = inner
            .grants
            .get(grant_id)
            .ok_or_else(|| "source_grant_revoked".to_string())?;
        if entry.revoked {
            return Err("source_grant_revoked".to_string());
        }
        if now >= entry.record.expires_at {
            return Err("source_grant_expired".to_string());
        }
        Ok(())
    }

    fn finish(&self, grant_id: &str) {
        if let Ok(mut inner) = self.inner.lock() {
            if let Some(entry) = inner.grants.get_mut(grant_id) {
                entry.finished = true;
                entry.inflight = None;
            }
        }
    }
}

struct FinishGrant<'registry> {
    registry: &'registry SourceConsentRegistry,
    grant_id: String,
}

impl Drop for FinishGrant<'_> {
    fn drop(&mut self) {
        self.registry.finish(&self.grant_id);
    }
}

fn execution_error(error: DbExecutionError<String>) -> String {
    match error {
        DbExecutionError::Cancelled => "source_cancelled".into(),
        DbExecutionError::Operation(code) => code,
        DbExecutionError::QueueClosed | DbExecutionError::Worker(_) => "source_worker".into(),
    }
}

pub fn default_registry() -> &'static SourceConsentRegistry {
    static REGISTRY: OnceLock<SourceConsentRegistry> = OnceLock::new();
    REGISTRY.get_or_init(SourceConsentRegistry::new)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct SourceContextGrantReport {
    pub grant_id: String,
    pub origin: String,
    pub expires_at: u64,
    pub node_count: u16,
    pub budget_bytes: u64,
    pub ticket: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct SourceContextRevokeReport {
    pub revoked: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct SourceContextImportReport {
    pub imported: u16,
    pub deduplicated: u16,
    pub complete: bool,
    pub failures: Vec<ArchiveResourceFailure>,
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

struct CancelOnDrop(Cancellation);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

pub async fn create_grant(
    database: std::sync::Arc<AsyncMutex<HistoryDB>>,
    registry: &SourceConsentRegistry,
    id: i64,
) -> Result<SourceContextGrantReport, String> {
    let (identity, authority, origin, nodes) = run_db_named(
        "history.source_context_grant.read",
        database,
        move |database| -> Result<_, String> {
            let identity = database
                .composite_identity(id)
                .map_err(|_| "storage_unavailable".to_string())?
                .ok_or_else(|| "not_found".to_string())?;
            let authority = database
                .read_revision()
                .map_err(|_| "storage_unavailable".to_string())?;
            let original = database
                .get_composite(id)
                .map_err(|_| "storage_unavailable".to_string())?;
            let layer = database
                .get_composite_archived(id)
                .map_err(|_| "storage_unavailable".to_string())?;
            let candidates = sc::missing_source_candidates(&original, layer.as_ref())
                .map_err(|_| "source_decode".to_string())?;
            let (origin, nodes) =
                sc::select_grant(&candidates).map_err(|error| error.code().to_string())?;
            Ok((identity, authority, origin, nodes))
        },
    )
    .await
    .map_err(execution_error)?;
    let grant_id = sc::new_grant_id();
    let now = Instant::now();
    let expires_at = now + Duration::from_secs(sc::SOURCE_GRANT_TTL_SECS);
    let expires_at_unix = unix_now().saturating_add(sc::SOURCE_GRANT_TTL_SECS);
    let budget_bytes = sc::SOURCE_TOTAL_BYTES as u64;
    registry.create(
        grant_id.clone(),
        GrantRecord {
            history_id: id,
            identity,
            authority,
            origin: origin.clone(),
            nodes: nodes.clone(),
            budget_bytes,
            expires_at,
        },
    )?;
    let ticket = sc::ticket_json(&grant_id, &origin, expires_at_unix, budget_bytes, &nodes)
        .map_err(|error| error.code().to_string())?;
    Ok(SourceContextGrantReport {
        grant_id,
        origin,
        expires_at: expires_at_unix,
        node_count: nodes.len() as u16,
        budget_bytes,
        ticket,
    })
}

pub fn revoke_grant(registry: &SourceConsentRegistry, grant_id: &str) -> SourceContextRevokeReport {
    SourceContextRevokeReport {
        revoked: registry.revoke(grant_id),
    }
}

fn read_packet_file(path: &str) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let metadata =
        std::fs::symlink_metadata(path).map_err(|_| "source_packet_unreadable".to_string())?;
    if !metadata.file_type().is_file() {
        return Err("source_packet_type".to_string());
    }
    if metadata.len() > sc::SOURCE_PACKET_BYTES as u64 {
        return Err("source_packet_too_large".to_string());
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000).share_mode(1);
    }
    let mut file = options
        .open(path)
        .map_err(|_| "source_packet_unreadable".to_string())?;
    let opened = file
        .metadata()
        .map_err(|_| "source_packet_unreadable".to_string())?;
    if !stable_metadata(&metadata, &opened) {
        return Err("source_packet_unstable".to_string());
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take(sc::SOURCE_PACKET_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "source_packet_unreadable".to_string())?;
    if bytes.len() > sc::SOURCE_PACKET_BYTES {
        return Err("source_packet_too_large".to_string());
    }
    let after =
        std::fs::symlink_metadata(path).map_err(|_| "source_packet_unreadable".to_string())?;
    let descriptor_after = file
        .metadata()
        .map_err(|_| "source_packet_unreadable".to_string())?;
    if !stable_metadata(&opened, &after)
        || !stable_metadata(&opened, &descriptor_after)
        || after.len() != bytes.len() as u64
    {
        return Err("source_packet_unstable".to_string());
    }
    Ok(bytes)
}

fn stable_metadata(before: &std::fs::Metadata, after: &std::fs::Metadata) -> bool {
    if !before.is_file()
        || !after.is_file()
        || before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
    {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if before.dev() != after.dev()
            || before.ino() != after.ino()
            || before.ctime() != after.ctime()
            || before.ctime_nsec() != after.ctime_nsec()
        {
            return false;
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if before.file_attributes() & 0x400 != 0
            || after.file_attributes() & 0x400 != 0
            || before.creation_time() != after.creation_time()
        {
            return false;
        }
    }
    true
}

pub async fn export_ticket(path: String, ticket: String) -> Result<(), String> {
    sc::parse_ticket(ticket.as_bytes(), unix_now())
        .map_err(|_| "source_ticket_invalid".to_string())?;
    tokio::task::spawn_blocking(move || {
        use std::io::Write;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let mut file = options
            .open(path)
            .map_err(|_| "source_ticket_write_failed".to_string())?;
        file.write_all(ticket.as_bytes())
            .and_then(|_| file.sync_all())
            .map_err(|_| "source_ticket_write_failed".to_string())
    })
    .await
    .map_err(|_| "source_worker".to_string())?
}

pub async fn import_packet(
    database: std::sync::Arc<AsyncMutex<HistoryDB>>,
    registry: &SourceConsentRegistry,
    path: String,
    cancellation: Cancellation,
) -> Result<SourceContextImportReport, String> {
    import_packet_checked(database, registry, path, None, cancellation).await
}

pub async fn import_packet_checked(
    database: Arc<AsyncMutex<HistoryDB>>,
    registry: &SourceConsentRegistry,
    path: String,
    expected_id: Option<i64>,
    cancellation: Cancellation,
) -> Result<SourceContextImportReport, String> {
    let _cancel_on_drop = CancelOnDrop(cancellation.clone());
    tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err("source_cancelled".to_string()),
        result = tokio::time::timeout(
            SOURCE_IMPORT_DEADLINE,
            import_packet_inner(database, registry, path, expected_id, cancellation.clone()),
        ) => result.map_err(|_| "source_deadline".to_string())?,
    }
}

async fn import_packet_inner(
    database: std::sync::Arc<AsyncMutex<HistoryDB>>,
    registry: &SourceConsentRegistry,
    path: String,
    expected_id: Option<i64>,
    cancellation: Cancellation,
) -> Result<SourceContextImportReport, String> {
    let admission = tokio::select! {
        biased;
        _ = cancellation.cancelled() => return Err("source_cancelled".to_string()),
        permit = IMPORT_ADMISSION.acquire() => permit.map_err(|_| "source_busy".to_string())?,
    };
    let pipeline = tailsync_core::composite::budgets::global()
        .reserve_pipeline(64 * 1024 * 1024)
        .map_err(|_| "source_busy".to_string())?;
    let lease = Arc::new((admission, pipeline));
    let read_lease = lease.clone();
    let read_cancellation = cancellation.clone();
    let (packet, decoded) = tokio::task::spawn_blocking(move || {
        let _lease = read_lease;
        read_cancellation
            .check()
            .map_err(|_| "source_cancelled".to_string())?;
        let bytes = read_packet_file(&path)?;
        read_cancellation
            .check()
            .map_err(|_| "source_cancelled".to_string())?;
        sc::parse_packet_decoded(&bytes, sc::SOURCE_PACKET_BYTES)
            .map_err(|error| error.code().to_string())
    })
    .await
    .map_err(|_| "source_packet_unreadable".to_string())??;
    cancellation
        .check()
        .map_err(|_| "source_cancelled".to_string())?;
    let handle = registry.consume(&packet.grant_id, &packet.origin, cancellation.clone())?;
    let _finish = FinishGrant {
        registry,
        grant_id: handle.grant_id.clone(),
    };
    if expected_id.is_some_and(|id| id != handle.history_id) {
        return Err("source_target_mismatch".into());
    }

    let origin = handle.origin.clone();
    let granted = handle.nodes.clone();
    let expected_identity = handle.identity.clone();
    let authority = handle.authority.clone();
    let history_id = handle.history_id;
    let grant_id = handle.grant_id.clone();
    let cancellation_for_read = cancellation.clone();
    let authority_for_read = authority.clone();
    let identity_for_read = expected_identity.clone();
    let read_lease = lease.clone();
    let snapshot_lease = Arc::new(
        tailsync_core::composite::budgets::global()
            .reserve_pipeline(96 * 1024 * 1024)
            .map_err(|_| "source_busy".to_string())?,
    );
    let read_snapshot_lease = snapshot_lease.clone();
    let base = run_db_named_cancellable(
        "history.source_context_import.read",
        database.clone(),
        cancellation.clone(),
        move |database| -> Result<tailsync_core::composite::PortableBundle, String> {
            let _lease = read_lease;
            let _snapshot_lease = read_snapshot_lease;
            if cancellation_for_read.is_cancelled() {
                return Err("source_cancelled".to_string());
            }
            let current = database
                .composite_identity(history_id)
                .map_err(|_| "storage_unavailable".to_string())?
                .ok_or_else(|| "not_found".to_string())?;
            if current != identity_for_read {
                return Err("source_superseded".to_string());
            }
            database
                .validate_storage_authority(&authority_for_read)
                .map_err(|_| "source_superseded".to_string())?;
            let original = database
                .get_composite(history_id)
                .map_err(|_| "storage_unavailable".to_string())?;
            let layer = database
                .get_composite_archived(history_id)
                .map_err(|_| "storage_unavailable".to_string())?;
            Ok(layer.unwrap_or(original))
        },
    )
    .await
    .map_err(execution_error)?;
    drop(snapshot_lease);
    let base_bytes = base
        .representations
        .iter()
        .map(|representation| representation.bytes.len())
        .sum::<usize>()
        + base
            .resources
            .iter()
            .map(|resource| resource.png.len())
            .sum::<usize>();
    let decoded_bytes = decoded.iter().map(Vec::len).sum::<usize>();
    let work_lease = Arc::new(
        tailsync_core::composite::budgets::global()
            .reserve_pipeline(
                (base_bytes * 3
                    + decoded_bytes * 3
                    + tailsync_core::protocol::composite::RGBA_LIMIT * 3) as u64,
            )
            .map_err(|_| "source_busy".to_string())?,
    );
    let derive_lease = lease.clone();
    let derive_work_lease = work_lease.clone();
    let derive_cancellation = cancellation.clone();
    let derive_granted = granted.clone();
    let (derived, proof, deduplicated) = tokio::task::spawn_blocking(move || {
        let _lease = derive_lease;
        let _work_lease = derive_work_lease;
        derive_cancellation
            .check()
            .map_err(|_| "source_cancelled".to_string())?;
        let resolved = sc::resolve_decoded_nodes(&base, &packet, decoded, &origin, &derive_granted)
            .map_err(|error| error.code().to_string())?;
        derive_cancellation
            .check()
            .map_err(|_| "source_cancelled".to_string())?;
        sc::derive_source_archive(&base, &resolved, &origin)
            .map_err(|error| error.code().to_string())
    })
    .await
    .map_err(|_| "source_worker".to_string())??;

    let mut failures = granted
        .iter()
        .filter(|node| !proof.nodes.iter().any(|bound| bound.index == node.index))
        .map(|node| ArchiveResourceFailure {
            image_index: node.index,
            code: "source_node_missing".to_string(),
        })
        .collect::<Vec<_>>();
    failures.sort_by_key(|failure| failure.image_index);
    if proof.nodes.is_empty() {
        return Err("source_no_nodes".to_string());
    }
    let complete = failures.is_empty()
        && derived.completeness == tailsync_core::composite::Completeness::Complete;
    let imported = proof.nodes.len() as u16;
    let deduplicated = deduplicated as u16;

    cancellation
        .check()
        .map_err(|_| "source_cancelled".to_string())?;
    registry.recheck(&grant_id, Instant::now())?;
    let cancellation_for_commit = cancellation.clone();
    let proof_for_commit = proof.clone();
    let identity_for_commit = expected_identity.clone();
    let authority_for_commit = authority.clone();
    let expires_at = handle.expires_at;
    let commit_lease = lease.clone();
    let commit_work_lease = work_lease.clone();
    run_db_named_cancellable(
        "history.source_context_import.commit",
        database,
        cancellation.clone(),
        move |database| -> Result<(), String> {
            let _lease = commit_lease;
            let _work_lease = commit_work_lease;
            if cancellation_for_commit.is_cancelled() {
                return Err("source_cancelled".to_string());
            }
            if Instant::now() >= expires_at {
                return Err("source_grant_expired".to_string());
            }
            database
                .validate_storage_authority(&authority_for_commit)
                .map_err(|_| "source_superseded".to_string())?;
            match database
                .commit_composite_source_archive_until(
                    history_id,
                    &identity_for_commit,
                    &derived,
                    &proof_for_commit,
                    &cancellation_for_commit,
                    Some(expires_at),
                )
                .map_err(|_| "source_commit_failed".to_string())?
            {
                Some(_) => Ok(()),
                None => Err("source_superseded".to_string()),
            }
        },
    )
    .await
    .map_err(execution_error)?;

    Ok(SourceContextImportReport {
        imported,
        deduplicated,
        complete,
        failures,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(index: u16) -> SourceNode {
        SourceNode {
            index,
            src_sha256: "ab".repeat(32),
        }
    }

    fn record() -> GrantRecord {
        GrantRecord {
            history_id: 1,
            identity: HistoryIdentity {
                version: 1,
                capture_id: "01".repeat(16),
                content_id: "hcp1:1:test".into(),
                dedupe_key: None,
                archive_revision: 1,
                archive_digest: "02".repeat(32),
            },
            authority: test_authority(),
            origin: "https://example.com".into(),
            nodes: vec![node(0)],
            budget_bytes: sc::SOURCE_TOTAL_BYTES as u64,
            expires_at: Instant::now() + Duration::from_secs(60),
        }
    }

    fn test_authority() -> HistoryReadRevision {
        let root =
            std::env::temp_dir().join(format!("tailsync-source-registry-{:016x}", rand_u64()));
        let db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let revision = db.read_revision().unwrap();
        std::fs::remove_dir_all(root).unwrap();
        revision
    }

    fn rand_u64() -> u64 {
        use std::hash::{BuildHasher, Hasher};
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u64(unix_now());
        hasher.finish()
    }

    #[test]
    fn grant_is_one_use_revocable_and_expiring() {
        let registry = SourceConsentRegistry::new();
        registry.create("g1".into(), record()).unwrap();
        assert_eq!(
            registry
                .consume("missing", "https://example.com", Cancellation::default())
                .unwrap_err(),
            "source_grant_unknown"
        );
        let handle = registry
            .consume("g1", "https://example.com", Cancellation::default())
            .unwrap();
        assert_eq!(handle.origin, "https://example.com");
        assert_eq!(
            registry
                .consume("g1", "https://example.com", Cancellation::default())
                .unwrap_err(),
            "source_grant_consumed"
        );
        assert!(registry.revoke("g1"));
        assert_eq!(
            registry.recheck("g1", Instant::now()).unwrap_err(),
            "source_grant_revoked"
        );
    }

    #[test]
    fn grant_expiry_and_origin_binding_are_enforced() {
        let registry = SourceConsentRegistry::new();
        let mut expiring = record();
        expiring.expires_at = Instant::now() - Duration::from_secs(1);
        registry.create("expired".into(), expiring).unwrap();
        assert_eq!(
            registry
                .consume("expired", "https://example.com", Cancellation::default())
                .unwrap_err(),
            "source_grant_expired"
        );
        let mut other = record();
        other.origin = "https://other.example".into();
        registry.create("other".into(), other).unwrap();
        assert_eq!(
            registry
                .consume("other", "https://example.com", Cancellation::default())
                .unwrap_err(),
            "source_origin_mismatch"
        );
    }

    #[test]
    fn revoke_cancels_a_consumed_inflight_import() {
        let registry = SourceConsentRegistry::new();
        registry.create("g1".into(), record()).unwrap();
        let inflight = Cancellation::default();
        let handle = registry
            .consume("g1", "https://example.com", inflight.clone())
            .unwrap();
        assert!(!inflight.is_cancelled());
        assert!(registry.revoke(&handle.grant_id));
        assert!(inflight.is_cancelled());
    }
    use tailsync_core::composite::{Completeness, Mime, PortableBundle, Representation};

    fn blob_bundle(images: &[&str]) -> PortableBundle {
        let mut html = String::from("<p>caption</p>");
        for image in images {
            html.push_str(&format!("<img src=\"{image}\">"));
        }
        PortableBundle {
            representations: vec![
                Representation {
                    mime: Mime::Text,
                    bytes: b"caption".to_vec(),
                },
                Representation {
                    mime: Mime::Html,
                    bytes: html.into_bytes(),
                },
            ],
            resources: Vec::new(),
            bindings: Vec::new(),
            completeness: Completeness::ExternalReferences,
        }
    }

    fn temp_path(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("tailsync-o05-{tag}-{:016x}", rand_u64()))
    }

    #[test]
    fn completed_grants_release_capacity_without_authorizing_replay() {
        let registry = SourceConsentRegistry::new();
        for index in 0..SOURCE_GRANT_LIMIT {
            let id = index.to_string();
            registry.create(id.clone(), record()).unwrap();
            registry
                .consume(&id, "https://example.com", Cancellation::default())
                .unwrap();
        }
        assert_eq!(
            registry.create("blocked".into(), record()).unwrap_err(),
            "source_grant_limit"
        );
        registry.finish("0");
        registry.create("new".into(), record()).unwrap();
        assert!(registry
            .consume("0", "https://example.com", Cancellation::default())
            .is_err());
        assert_eq!(
            registry.inner.lock().unwrap().grants.len(),
            SOURCE_GRANT_LIMIT
        );
    }

    #[test]
    fn packet_file_refuses_symlinks_special_files_and_changed_identity() {
        let root = temp_path("regular-file");
        std::fs::create_dir(&root).unwrap();
        let path = root.join("packet");
        std::fs::write(&path, b"abcd").unwrap();
        assert_eq!(read_packet_file(path.to_str().unwrap()).unwrap(), b"abcd");
        assert_eq!(
            read_packet_file(root.to_str().unwrap()).unwrap_err(),
            "source_packet_type"
        );
        let other = root.join("other");
        std::fs::write(&other, b"abcd").unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&path, root.join("link")).unwrap();
            assert_eq!(
                read_packet_file(root.join("link").to_str().unwrap()).unwrap_err(),
                "source_packet_type"
            );
            assert!(!stable_metadata(
                &std::fs::metadata(&path).unwrap(),
                &std::fs::metadata(&other).unwrap()
            ));
            use std::os::unix::ffi::OsStrExt;
            let fifo = root.join("fifo");
            let name = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
            assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
            assert_eq!(
                read_packet_file(fifo.to_str().unwrap()).unwrap_err(),
                "source_packet_type"
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn export_ticket_validates_and_never_overwrites_existing_files() {
        let path = temp_path("export");
        let ticket = sc::ticket_json(
            &"ab".repeat(32),
            "https://example.com",
            unix_now() + 200,
            1024,
            &[node(0)],
        )
        .unwrap();
        export_ticket(path.to_string_lossy().into(), ticket.clone())
            .await
            .unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), ticket);
        assert_eq!(
            export_ticket(path.to_string_lossy().into(), ticket.clone())
                .await
                .unwrap_err(),
            "source_ticket_write_failed"
        );
        assert_eq!(
            export_ticket(path.to_string_lossy().into(), "{}".into())
                .await
                .unwrap_err(),
            "source_ticket_invalid"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), ticket);
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn waiting_import_refuses_revoke_expiry_cancellation_and_reopened_authority() {
        let _fixture = crate::test_fixtures::PIPELINE.lock().await;
        for mode in ["revoke", "expire", "cancel", "reopen"] {
            let root = temp_path(mode);
            let mut raw = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
            let base = blob_bundle(&["blob:https://example.com/one"]);
            let id = raw.add_composite(&base, None, "self").unwrap();
            let db = Arc::new(AsyncMutex::new(raw));
            let registry = Arc::new(SourceConsentRegistry::new());
            let report = create_grant(db.clone(), &registry, id).await.unwrap();
            let ticket: sc::SourceTicket = serde_json::from_str(&report.ticket).unwrap();
            let png = tailsync_core::composite::packed_to_png(1, 1, &[1, 2, 3, 255]).unwrap();
            let path = root.join("packet.json");
            std::fs::write(
                &path,
                packet_json(
                    &report,
                    ticket.nodes[0].index,
                    &ticket.nodes[0].src_sha256,
                    &png,
                ),
            )
            .unwrap();
            let mut guard = db.lock().await;
            let identity = guard.composite_identity(id).unwrap().unwrap();
            let cancellation = Cancellation::default();
            let worker_db = db.clone();
            let worker_registry = registry.clone();
            let worker_cancellation = cancellation.clone();
            let worker = tokio::spawn(async move {
                import_packet_checked(
                    worker_db,
                    &worker_registry,
                    path.to_string_lossy().into(),
                    Some(id),
                    worker_cancellation,
                )
                .await
            });
            tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    if registry
                        .inner
                        .lock()
                        .unwrap()
                        .grants
                        .get(&report.grant_id)
                        .unwrap()
                        .consumed
                    {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            match mode {
                "revoke" => {
                    assert!(registry.revoke(&report.grant_id));
                }
                "expire" => {
                    registry
                        .inner
                        .lock()
                        .unwrap()
                        .grants
                        .get_mut(&report.grant_id)
                        .unwrap()
                        .record
                        .expires_at = Instant::now() - Duration::from_secs(1);
                }
                "cancel" => {
                    cancellation.cancel();
                }
                "reopen" => {
                    *guard = HistoryDB::open_with_policy_for_probe(
                        &root,
                        tailsync_core::db::MigrationPolicy::CompositeAllowed,
                    )
                    .unwrap();
                }
                _ => unreachable!(),
            }
            drop(guard);
            let error = worker.await.unwrap().unwrap_err();
            let expected = match mode {
                "revoke" | "cancel" => "source_cancelled",
                "expire" => "source_grant_expired",
                "reopen" => "source_superseded",
                _ => unreachable!(),
            };
            assert_eq!(error, expected);
            let guard = db.lock().await;
            assert_eq!(guard.composite_identity(id).unwrap().unwrap(), identity);
            assert_eq!(guard.get_composite(id).unwrap(), base);
            assert!(guard.get_composite_archived(id).unwrap().is_none());
            drop(guard);
            drop(db);
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[tokio::test]
    async fn import_refuses_a_different_selected_history_row() {
        let _fixture = crate::test_fixtures::PIPELINE.lock().await;
        let root = temp_path("wrong-row");
        let mut raw = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let id = raw
            .add_composite(
                &blob_bundle(&["blob:https://example.com/one"]),
                None,
                "self",
            )
            .unwrap();
        let db = Arc::new(AsyncMutex::new(raw));
        let registry = SourceConsentRegistry::new();
        let report = create_grant(db.clone(), &registry, id).await.unwrap();
        let ticket: sc::SourceTicket = serde_json::from_str(&report.ticket).unwrap();
        let path = root.join("packet.json");
        let png = tailsync_core::composite::packed_to_png(1, 1, &[1, 2, 3, 255]).unwrap();
        std::fs::write(
            &path,
            packet_json(
                &report,
                ticket.nodes[0].index,
                &ticket.nodes[0].src_sha256,
                &png,
            ),
        )
        .unwrap();
        assert_eq!(
            import_packet_checked(
                db.clone(),
                &registry,
                path.to_string_lossy().into(),
                Some(id + 1),
                Cancellation::default()
            )
            .await
            .unwrap_err(),
            "source_target_mismatch"
        );
        assert!(db
            .lock()
            .await
            .get_composite_archived(id)
            .unwrap()
            .is_none());
        assert!(import_packet_checked(
            db.clone(),
            &registry,
            path.to_string_lossy().into(),
            Some(id),
            Cancellation::default()
        )
        .await
        .is_err());
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }

    fn packet_json(
        report: &SourceContextGrantReport,
        index: u16,
        src_sha256: &str,
        png: &[u8],
    ) -> String {
        use base64::Engine as _;
        serde_json::json!({
            "kind": sc::SOURCE_PACKET_KIND,
            "version": 1,
            "grant_id": report.grant_id,
            "origin": report.origin,
            "entries": [{
                "index": index,
                "src_sha256": src_sha256,
                "png_b64": base64::engine::general_purpose::STANDARD.encode(png),
            }],
        })
        .to_string()
    }

    #[tokio::test]
    async fn source_context_import_binds_packet_once_and_reports_complete() {
        let _fixture = crate::test_fixtures::PIPELINE.lock().await;
        let root = temp_path("import");
        let mut raw = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let base = blob_bundle(&["blob:https://example.com/one"]);
        let id = raw.add_composite(&base, None, "self").unwrap();
        let db = std::sync::Arc::new(AsyncMutex::new(raw));
        let registry = SourceConsentRegistry::new();
        let report = create_grant(db.clone(), &registry, id).await.unwrap();
        let ticket: sc::SourceTicket = serde_json::from_str(&report.ticket).unwrap();
        assert_eq!(ticket.origin, "https://example.com");
        assert_eq!(ticket.nodes.len(), 1);
        let png = tailsync_core::composite::packed_to_png(2, 2, &[1u8; 16]).unwrap();
        let packet = packet_json(
            &report,
            ticket.nodes[0].index,
            &ticket.nodes[0].src_sha256,
            &png,
        );
        let path = temp_path("packet");
        std::fs::write(&path, packet.as_bytes()).unwrap();
        let imported = import_packet(
            db.clone(),
            &registry,
            path.to_string_lossy().to_string(),
            Cancellation::default(),
        )
        .await
        .unwrap();
        assert_eq!(imported.imported, 1);
        assert_eq!(imported.deduplicated, 0);
        assert!(imported.complete);
        assert!(imported.failures.is_empty());
        {
            let guard = db.lock().await;
            assert_eq!(guard.get_composite(id).unwrap(), base);
            let archived = guard.get_composite_archived(id).unwrap().unwrap();
            assert_eq!(archived.resources.len(), 1);
        }
        let replay = import_packet(
            db.clone(),
            &registry,
            path.to_string_lossy().to_string(),
            Cancellation::default(),
        )
        .await;
        assert_eq!(replay.unwrap_err(), "source_grant_consumed");
        std::fs::remove_file(&path).ok();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn source_context_import_is_partial_and_rejects_tampering() {
        let _fixture = crate::test_fixtures::PIPELINE.lock().await;
        let root = temp_path("partial");
        let mut raw = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let base = blob_bundle(&[
            "blob:https://example.com/one",
            "blob:https://example.com/two",
        ]);
        let id = raw.add_composite(&base, None, "self").unwrap();
        let db = std::sync::Arc::new(AsyncMutex::new(raw));
        let registry = SourceConsentRegistry::new();
        let report = create_grant(db.clone(), &registry, id).await.unwrap();
        let ticket: sc::SourceTicket = serde_json::from_str(&report.ticket).unwrap();
        assert_eq!(ticket.nodes.len(), 2);
        let png = tailsync_core::composite::packed_to_png(1, 1, &[2u8; 4]).unwrap();
        let packet = packet_json(
            &report,
            ticket.nodes[0].index,
            &ticket.nodes[0].src_sha256,
            &png,
        );
        let path = temp_path("partial-packet");
        std::fs::write(&path, packet.as_bytes()).unwrap();
        let imported = import_packet(
            db.clone(),
            &registry,
            path.to_string_lossy().to_string(),
            Cancellation::default(),
        )
        .await
        .unwrap();
        assert_eq!(imported.imported, 1);
        assert!(!imported.complete);
        assert_eq!(imported.failures.len(), 1);
        assert_eq!(imported.failures[0].code, "source_node_missing");
        assert_eq!(imported.failures[0].image_index, ticket.nodes[1].index);

        let report = create_grant(db.clone(), &registry, id).await.unwrap();
        let ticket: sc::SourceTicket = serde_json::from_str(&report.ticket).unwrap();
        let bad_hash = packet_json(&report, ticket.nodes[0].index, &"00".repeat(32), &png);
        std::fs::write(&path, bad_hash.as_bytes()).unwrap();
        assert_eq!(
            import_packet(
                db.clone(),
                &registry,
                path.to_string_lossy().to_string(),
                Cancellation::default()
            )
            .await
            .unwrap_err(),
            "source_hash_mismatch"
        );
        let report = create_grant(db.clone(), &registry, id).await.unwrap();
        let ticket: sc::SourceTicket = serde_json::from_str(&report.ticket).unwrap();
        let unknown = packet_json(&report, 99, &ticket.nodes[0].src_sha256, &png);
        std::fs::write(&path, unknown.as_bytes()).unwrap();
        assert_eq!(
            import_packet(
                db.clone(),
                &registry,
                path.to_string_lossy().to_string(),
                Cancellation::default()
            )
            .await
            .unwrap_err(),
            "source_unknown_node"
        );
        std::fs::remove_file(&path).ok();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn source_context_import_is_blocked_by_revoke_supersede_and_unknown_fields() {
        let _fixture = crate::test_fixtures::PIPELINE.lock().await;
        let root = temp_path("blocked");
        let mut raw = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let base = blob_bundle(&["blob:https://example.com/one"]);
        let id = raw.add_composite(&base, None, "self").unwrap();
        let db = std::sync::Arc::new(AsyncMutex::new(raw));
        let registry = SourceConsentRegistry::new();
        let png = tailsync_core::composite::packed_to_png(1, 1, &[3u8; 4]).unwrap();
        let path = temp_path("blocked-packet");

        let report = create_grant(db.clone(), &registry, id).await.unwrap();
        let ticket: sc::SourceTicket = serde_json::from_str(&report.ticket).unwrap();
        let mut value: serde_json::Value = serde_json::from_str(&packet_json(
            &report,
            ticket.nodes[0].index,
            &ticket.nodes[0].src_sha256,
            &png,
        ))
        .unwrap();
        value["consent"] = serde_json::json!(true);
        std::fs::write(&path, value.to_string().as_bytes()).unwrap();
        assert_eq!(
            import_packet(
                db.clone(),
                &registry,
                path.to_string_lossy().to_string(),
                Cancellation::default()
            )
            .await
            .unwrap_err(),
            "source_packet_malformed"
        );

        let report = create_grant(db.clone(), &registry, id).await.unwrap();
        let ticket: sc::SourceTicket = serde_json::from_str(&report.ticket).unwrap();
        let packet = packet_json(
            &report,
            ticket.nodes[0].index,
            &ticket.nodes[0].src_sha256,
            &png,
        );
        std::fs::write(&path, packet.as_bytes()).unwrap();
        assert!(registry.revoke(&report.grant_id));
        assert_eq!(
            import_packet(
                db.clone(),
                &registry,
                path.to_string_lossy().to_string(),
                Cancellation::default()
            )
            .await
            .unwrap_err(),
            "source_grant_revoked"
        );

        let report = create_grant(db.clone(), &registry, id).await.unwrap();
        let ticket: sc::SourceTicket = serde_json::from_str(&report.ticket).unwrap();
        let packet = packet_json(
            &report,
            ticket.nodes[0].index,
            &ticket.nodes[0].src_sha256,
            &png,
        );
        std::fs::write(&path, packet.as_bytes()).unwrap();
        {
            let mut guard = db.lock().await;
            let identity = guard.composite_identity(id).unwrap().unwrap();
            assert!(guard
                .update_composite_archive_summary(id, &identity, "changed")
                .unwrap());
        }
        assert_eq!(
            import_packet(
                db.clone(),
                &registry,
                path.to_string_lossy().to_string(),
                Cancellation::default()
            )
            .await
            .unwrap_err(),
            "source_superseded"
        );
        std::fs::remove_file(&path).ok();
        std::fs::remove_dir_all(root).unwrap();
    }
}
