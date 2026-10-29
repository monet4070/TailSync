use super::html::ImageSourceSpan;
use super::network_image::{self, NetworkImageFormat, NetworkImageLimits};
use super::network_originals::NetworkImageOriginal;
use super::{Completeness, Mime, PortableBundle, Representation, Resource, ResourceBinding};
use crate::cancellation::Cancellation;
use crate::protocol::ProtocolError;
use std::fmt;
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Semaphore, SemaphorePermit};
use url::Url;
pub use url::Url as ArchiveUrl;

static NETWORK_ADMISSION: Semaphore = Semaphore::const_new(2);

pub const ARCHIVE_RESOURCE_LIMIT: usize = 8;
pub const ARCHIVE_BYTES_PER_RESOURCE: usize = 8 * 1024 * 1024;
pub const ARCHIVE_TOTAL_BYTES: usize = 16 * 1024 * 1024;
pub const ARCHIVE_REDIRECT_LIMIT: u8 = 3;
pub const ARCHIVE_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
pub const ARCHIVE_TOTAL_DEADLINE: Duration = Duration::from_secs(30);

pub const ARCHIVE_DERIVED_BYTES_PER_RESOURCE: usize = 16 * 1024 * 1024;
pub const ARCHIVE_TOTAL_DERIVED_BYTES: usize = 24 * 1024 * 1024;
pub const ARCHIVE_PIXELS_PER_RESOURCE: u64 = 4 * 1024 * 1024;
pub const ARCHIVE_TOTAL_PIXELS: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchiveError {
    NotHttp,
    UserInfo,
    NotPublic,
    Resolve,
    Transport,
    Status(u16),
    RedirectLimit,
    Deadline,
    TooManyResources,
    ResourceTooLarge,
    TotalBudget,
    Decode,
    Mime,
    Animated,
    DerivedBudget,
    Cancelled,
}

impl ArchiveError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotHttp => "archive_not_http",
            Self::UserInfo => "archive_user_info",
            Self::NotPublic => "archive_not_public",
            Self::Resolve => "archive_resolve",
            Self::Transport => "archive_transport",
            Self::Status(_) => "archive_status",
            Self::RedirectLimit => "archive_redirect_limit",
            Self::Deadline => "archive_deadline",
            Self::TooManyResources => "archive_too_many",
            Self::ResourceTooLarge => "archive_too_large",
            Self::TotalBudget => "archive_total_budget",
            Self::Decode => "archive_decode",
            Self::Mime => "archive_mime",
            Self::Animated => "archive_animated",
            Self::DerivedBudget => "archive_derived_budget",
            Self::Cancelled => "archive_cancelled",
        }
    }
}

impl fmt::Display for ArchiveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Status(status) => write!(formatter, "archive status {status}"),
            other => formatter.write_str(other.code()),
        }
    }
}

impl std::error::Error for ArchiveError {}

#[derive(Debug, Clone, Copy)]
pub struct ArchiveLimits {
    pub max_resources: usize,
    pub max_bytes_per_resource: usize,
    pub max_total_bytes: usize,
    pub max_redirects: u8,
    pub request_timeout: Duration,
    pub total_deadline: Duration,
    pub max_derived_bytes_per_resource: usize,
    pub max_total_derived_bytes: usize,
    pub max_pixels_per_resource: u64,
    pub max_total_pixels: u64,
}

impl Default for ArchiveLimits {
    fn default() -> Self {
        Self {
            max_resources: ARCHIVE_RESOURCE_LIMIT,
            max_bytes_per_resource: ARCHIVE_BYTES_PER_RESOURCE,
            max_total_bytes: ARCHIVE_TOTAL_BYTES,
            max_redirects: ARCHIVE_REDIRECT_LIMIT,
            request_timeout: ARCHIVE_REQUEST_TIMEOUT,
            total_deadline: ARCHIVE_TOTAL_DEADLINE,
            max_derived_bytes_per_resource: ARCHIVE_DERIVED_BYTES_PER_RESOURCE,
            max_total_derived_bytes: ARCHIVE_TOTAL_DERIVED_BYTES,
            max_pixels_per_resource: ARCHIVE_PIXELS_PER_RESOURCE,
            max_total_pixels: ARCHIVE_TOTAL_PIXELS,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FetchLimits {
    pub max_bytes: usize,
    pub request_timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct FetchResponse {
    pub status: u16,
    pub location: Option<String>,
    pub content_type: Option<String>,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
struct FetchedBody {
    bytes: Vec<u8>,
    declared: NetworkImageFormat,
}

pub type FetchFuture<'a> =
    Pin<Box<dyn Future<Output = Result<FetchResponse, ArchiveError>> + Send + 'a>>;

pub trait ResourceTransport: Send + Sync {
    fn fetch_once<'a>(
        &'a self,
        url: &'a Url,
        limits: &'a FetchLimits,
        cancellation: &'a Cancellation,
    ) -> FetchFuture<'a>;
}

pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_ipv4(v4),
        IpAddr::V6(v6) => is_public_ipv6(v6),
    }
}

fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_multicast()
        || a == 0
        || a >= 240
        || (a == 100 && (64..=127).contains(&b))
        || (a == 192 && b == 0 && c == 0)
        || (a == 192 && b == 0 && c == 2)
        || (a == 192 && b == 88 && c == 99)
        || (a == 198 && (b == 18 || b == 19))
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113))
}

fn is_public_ipv6(ip: Ipv6Addr) -> bool {
    let [s0, s1, ..] = ip.segments();
    (s0 & 0xe000) == 0x2000
        && !(ip.to_ipv4_mapped().is_some()
            || (s0 == 0x2001 && s1 == 0x0db8)
            || (s0 == 0x2001 && s1 < 0x0200)
            || s0 == 0x2002
            || (s0 & 0xfff0) == 0x3ff0)
}

pub fn validate_public_url(url: &Url) -> Result<(), ArchiveError> {
    match url.scheme() {
        "http" | "https" => {}
        _ => return Err(ArchiveError::NotHttp),
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(ArchiveError::UserInfo);
    }
    if url.port_or_known_default().is_none() {
        return Err(ArchiveError::NotPublic);
    }
    match url.host() {
        Some(url::Host::Ipv4(address)) => {
            if !is_public_ipv4(address) {
                return Err(ArchiveError::NotPublic);
            }
        }
        Some(url::Host::Ipv6(address)) => {
            if !is_public_ipv6(address) {
                return Err(ArchiveError::NotPublic);
            }
        }
        Some(url::Host::Domain(_)) => {}
        None => return Err(ArchiveError::NotPublic),
    }
    Ok(())
}

pub async fn resolve_public(
    url: &Url,
    cancellation: &Cancellation,
) -> Result<Vec<SocketAddr>, ArchiveError> {
    validate_public_url(url)?;
    if let Some(host) = url.host() {
        match host {
            url::Host::Ipv4(address) => {
                return Ok(vec![SocketAddr::new(
                    address.into(),
                    url.port_or_known_default().ok_or(ArchiveError::NotPublic)?,
                )])
            }
            url::Host::Ipv6(address) => {
                return Ok(vec![SocketAddr::new(
                    address.into(),
                    url.port_or_known_default().ok_or(ArchiveError::NotPublic)?,
                )])
            }
            url::Host::Domain(_) => {}
        }
    }
    let host = url.host_str().ok_or(ArchiveError::NotPublic)?;
    let port = url.port_or_known_default().ok_or(ArchiveError::NotPublic)?;
    let lookup = tokio::net::lookup_host((host, port));
    let addrs = tokio::select! {
        biased;
        _ = cancellation.cancelled() => return Err(ArchiveError::Cancelled),
        result = lookup => result.map_err(|_| ArchiveError::Resolve)?,
    }
    .collect::<Vec<_>>();
    validate_addresses(&addrs)?;
    Ok(addrs)
}

fn validate_addresses(addrs: &[SocketAddr]) -> Result<(), ArchiveError> {
    if addrs.is_empty() || addrs.len() > 16 {
        return Err(ArchiveError::Resolve);
    }
    if addrs.iter().any(|addr| !is_public_ip(addr.ip())) {
        return Err(ArchiveError::NotPublic);
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalImage {
    pub image_index: usize,
    pub span: ImageSourceSpan,
    pub url: String,
}

pub fn external_images(bundle: &PortableBundle) -> Result<Vec<ExternalImage>, ProtocolError> {
    let Some((index, html)) = bundle
        .representations
        .iter()
        .enumerate()
        .find(|(_, representation)| representation.mime == Mime::Html)
    else {
        return Ok(Vec::new());
    };
    let inventory = super::html::reference_inventory(&html.bytes)?;
    let mut images = Vec::new();
    for (image_index, span) in inventory.images.into_iter().enumerate() {
        if bundle.bindings.iter().any(|binding| {
            binding.representation as usize == index && binding.offset as usize == span.start
        }) {
            continue;
        }
        let value = std::str::from_utf8(&html.bytes[span.start..span.end])
            .map_err(|_| ProtocolError::InvalidComposite)?;
        if value.to_ascii_lowercase().starts_with("data:") {
            continue;
        }
        images.push(ExternalImage {
            image_index,
            span,
            url: super::html::resolve_resource_reference(value, inventory.base.as_ref())?,
        });
    }
    Ok(images)
}

#[derive(Debug)]
pub struct ArchiveOutcome {
    pub bundle: PortableBundle,
    pub archived: usize,
    pub deduplicated: usize,
    pub failures: Vec<(usize, ArchiveError)>,
    pub originals: Vec<NetworkImageOriginal>,
}

pub fn validate_archive_derivation(
    base: &PortableBundle,
    derived: &PortableBundle,
) -> Result<(), ProtocolError> {
    let invalid = || ProtocolError::InvalidComposite;
    base.validate()?;
    derived.validate()?;
    if base.representations.len() != derived.representations.len()
        || base
            .resources
            .iter()
            .any(|resource| !derived.resources.contains(resource))
    {
        return Err(invalid());
    }
    let index = base
        .representations
        .iter()
        .position(|representation| representation.mime == Mime::Html)
        .ok_or_else(invalid)?;
    for (position, (original, archived)) in base
        .representations
        .iter()
        .zip(&derived.representations)
        .enumerate()
    {
        if original.mime != archived.mime || (position != index && original != archived) {
            return Err(invalid());
        }
    }
    let original = &base.representations[index].bytes;
    let archived = &derived.representations[index].bytes;
    let original_inventory = super::html::reference_inventory(original)?;
    let original_spans = original_inventory.images;
    let archived_spans = super::html::reference_inventory(archived)?.images;
    if original_spans.len() != archived_spans.len() {
        return Err(invalid());
    }
    let mut replacements = Vec::new();
    let mut expected_bindings = Vec::new();
    for (source, target) in original_spans.iter().zip(&archived_spans) {
        let old = &original[source.start..source.end];
        let new = &archived[target.start..target.end];
        if old == new {
            continue;
        }
        if base.bindings.iter().any(|binding| {
            binding.representation as usize == index && binding.offset as usize == source.start
        }) {
            return Err(invalid());
        }
        let value = std::str::from_utf8(old).map_err(|_| invalid())?;
        let resolved =
            super::html::resolve_resource_reference(value, original_inventory.base.as_ref())?;
        validate_public_url(&Url::parse(&resolved).map_err(|_| invalid())?)
            .map_err(|_| invalid())?;
        let binding = derived
            .bindings
            .iter()
            .find(|binding| {
                binding.representation as usize == index
                    && binding.offset as usize == target.start
                    && binding.length as usize == target.end - target.start
            })
            .ok_or_else(invalid)?;
        if new != format!("tailsync-resource:{}", binding.resource).as_bytes() {
            return Err(invalid());
        }
        expected_bindings.push(binding.clone());
        replacements.push((*source, new));
    }
    let mut output = Vec::new();
    let mut cursor = 0;
    for (span, replacement) in &replacements {
        output.extend_from_slice(&original[cursor..span.start]);
        output.extend_from_slice(replacement);
        cursor = span.end;
    }
    output.extend_from_slice(&original[cursor..]);
    if output != *archived {
        return Err(invalid());
    }
    for binding in &base.bindings {
        let mut adjusted = binding.clone();
        if binding.representation as usize == index {
            let shift: i64 = replacements
                .iter()
                .filter(|(span, _)| span.end <= binding.offset as usize)
                .map(|(span, value)| value.len() as i64 - (span.end - span.start) as i64)
                .sum();
            adjusted.offset =
                u32::try_from(binding.offset as i64 + shift).map_err(|_| invalid())?;
        }
        expected_bindings.push(adjusted);
    }
    let key = |binding: &ResourceBinding| {
        (
            binding.representation,
            binding.offset,
            binding.length,
            binding.resource,
        )
    };
    expected_bindings.sort_by_key(key);
    let mut actual_bindings = derived.bindings.clone();
    actual_bindings.sort_by_key(key);
    if expected_bindings != actual_bindings
        || derived.resources.iter().any(|resource| {
            !base.resources.contains(resource)
                && !derived
                    .bindings
                    .iter()
                    .any(|binding| binding.resource == resource.id)
        })
    {
        return Err(invalid());
    }
    let html_bindings = derived
        .bindings
        .iter()
        .filter(|binding| binding.representation as usize == index)
        .cloned()
        .collect::<Vec<_>>();
    let closed = super::html::restore_reference_inventory_complete(archived, &html_bindings)?;
    let completeness = if base.completeness == Completeness::MissingFormats {
        Completeness::MissingFormats
    } else if closed {
        Completeness::Complete
    } else {
        Completeness::ExternalReferences
    };
    if derived.completeness != completeness {
        return Err(invalid());
    }
    Ok(())
}

impl ArchiveOutcome {
    /// Completion of the image-download operation, distinct from preserving
    /// every external dependency in the original HTML document.
    pub fn images_complete(&self) -> bool {
        self.failures.is_empty()
            && self.bundle.completeness != Completeness::MissingFormats
            && self
                .bundle
                .representations
                .iter()
                .enumerate()
                .filter(|(_, representation)| representation.mime == Mime::Html)
                .all(|(index, representation)| {
                    let bindings = self
                        .bundle
                        .bindings
                        .iter()
                        .filter(|binding| binding.representation as usize == index)
                        .cloned()
                        .collect::<Vec<_>>();
                    super::html::image_reference_inventory_complete(
                        &representation.bytes,
                        &bindings,
                    )
                    .unwrap_or(false)
                })
    }

    pub fn complete(&self) -> bool {
        self.failures.is_empty()
            && self.bundle.completeness == Completeness::Complete
            && self
                .bundle
                .representations
                .iter()
                .enumerate()
                .filter(|(_, representation)| representation.mime == Mime::Html)
                .all(|(index, representation)| {
                    let bindings = self
                        .bundle
                        .bindings
                        .iter()
                        .filter(|binding| binding.representation as usize == index)
                        .cloned()
                        .collect::<Vec<_>>();
                    super::html::restore_reference_inventory_complete(
                        &representation.bytes,
                        &bindings,
                    )
                    .unwrap_or(false)
                })
    }
}

async fn read_bounded(
    mut response: reqwest::Response,
    max_bytes: usize,
) -> Result<Vec<u8>, ArchiveError> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| ArchiveError::Transport)?
    {
        if bytes.len().saturating_add(chunk.len()) > max_bytes {
            return Err(ArchiveError::ResourceTooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

async fn fetch_with_redirects(
    transport: &dyn ResourceTransport,
    url: &Url,
    limits: &ArchiveLimits,
    deadline: Instant,
    cancellation: &Cancellation,
    remaining: &mut usize,
) -> Result<FetchedBody, ArchiveError> {
    let mut current = url.clone();
    let mut hops = 0u8;
    loop {
        if cancellation.is_cancelled() {
            return Err(ArchiveError::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(ArchiveError::Deadline);
        }
        validate_public_url(&current)?;
        if *remaining == 0 {
            return Err(ArchiveError::TotalBudget);
        }
        let fetch_limits = FetchLimits {
            max_bytes: limits.max_bytes_per_resource.min(*remaining),
            request_timeout: limits
                .request_timeout
                .min(deadline.saturating_duration_since(Instant::now())),
        };
        let result = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(ArchiveError::Cancelled),
            result = tokio::time::timeout(fetch_limits.request_timeout, transport.fetch_once(&current, &fetch_limits, cancellation)) => result.map_err(|_| ArchiveError::Deadline)?,
        };
        let response = match result {
            Ok(response) => response,
            Err(error) => {
                *remaining = remaining.saturating_sub(fetch_limits.max_bytes);
                return Err(error);
            }
        };
        if response.bytes.len() > fetch_limits.max_bytes {
            *remaining = 0;
            return Err(ArchiveError::ResourceTooLarge);
        }
        *remaining -= response.bytes.len();
        if response.status == 200 {
            let declared = response
                .content_type
                .as_deref()
                .and_then(network_image::declared_format)
                .ok_or(ArchiveError::Mime)?;
            return Ok(FetchedBody {
                bytes: response.bytes,
                declared,
            });
        }
        if matches!(response.status, 301 | 302 | 303 | 307 | 308) {
            if hops >= limits.max_redirects {
                return Err(ArchiveError::RedirectLimit);
            }
            hops += 1;
            let location = response.location.ok_or(ArchiveError::Transport)?;
            current = current
                .join(&location)
                .map_err(|_| ArchiveError::Transport)?;
            continue;
        }
        return Err(ArchiveError::Status(response.status));
    }
}

fn bundle_rgba_bytes(bundle: &PortableBundle) -> Result<usize, ArchiveError> {
    let mut total = 0usize;
    for representation in &bundle.representations {
        if representation.mime == Mime::Png {
            total = total
                .checked_add(
                    crate::protocol::composite::png_rgba_size(&representation.bytes)
                        .map_err(|_| ArchiveError::Decode)?,
                )
                .ok_or(ArchiveError::Decode)?;
        }
    }
    for resource in &bundle.resources {
        total = total
            .checked_add(
                crate::protocol::composite::png_rgba_size(&resource.png)
                    .map_err(|_| ArchiveError::Decode)?,
            )
            .ok_or(ArchiveError::Decode)?;
    }
    Ok(total)
}

pub async fn archive_external_images(
    bundle: &PortableBundle,
    transport: &dyn ResourceTransport,
    limits: &ArchiveLimits,
    cancellation: &Cancellation,
) -> Result<ArchiveOutcome, ArchiveError> {
    archive_images_admitted(bundle, transport, limits, cancellation, None).await
}

/// Retain capture admission in decode workers even when the caller times out or
/// is dropped. A cancelled worker must not outlive its memory accounting.
pub async fn archive_capture_images(
    bundle: &PortableBundle,
    transport: &dyn ResourceTransport,
    limits: &ArchiveLimits,
    cancellation: &Cancellation,
    memory: Arc<super::budgets::PipelineLease>,
) -> Result<ArchiveOutcome, ArchiveError> {
    archive_images_admitted(bundle, transport, limits, cancellation, Some(memory)).await
}

async fn archive_images_admitted(
    bundle: &PortableBundle,
    transport: &dyn ResourceTransport,
    limits: &ArchiveLimits,
    cancellation: &Cancellation,
    memory: Option<Arc<super::budgets::PipelineLease>>,
) -> Result<ArchiveOutcome, ArchiveError> {
    let limits = ArchiveLimits {
        max_resources: limits.max_resources.min(ARCHIVE_RESOURCE_LIMIT),
        max_bytes_per_resource: limits
            .max_bytes_per_resource
            .min(ARCHIVE_BYTES_PER_RESOURCE),
        max_total_bytes: limits.max_total_bytes.min(ARCHIVE_TOTAL_BYTES),
        max_redirects: limits.max_redirects.min(ARCHIVE_REDIRECT_LIMIT),
        request_timeout: limits.request_timeout.min(ARCHIVE_REQUEST_TIMEOUT),
        total_deadline: limits.total_deadline.min(ARCHIVE_TOTAL_DEADLINE),
        max_derived_bytes_per_resource: limits
            .max_derived_bytes_per_resource
            .min(ARCHIVE_DERIVED_BYTES_PER_RESOURCE),
        max_total_derived_bytes: limits
            .max_total_derived_bytes
            .min(ARCHIVE_TOTAL_DERIVED_BYTES),
        max_pixels_per_resource: limits
            .max_pixels_per_resource
            .min(ARCHIVE_PIXELS_PER_RESOURCE),
        max_total_pixels: limits.max_total_pixels.min(ARCHIVE_TOTAL_PIXELS),
    };
    let deadline = Instant::now() + limits.total_deadline;
    // First publication reserves a small finalization window so a slow later
    // image need not discard earlier completed images. Background saves keep
    // their existing deadline semantics.
    let resource_deadline = if memory.is_some() {
        deadline
            .checked_sub(Duration::from_millis(100))
            .filter(|at| *at > Instant::now())
            .unwrap_or(deadline)
    } else {
        deadline
    };
    tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err(ArchiveError::Cancelled),
        result = tokio::time::timeout(limits.total_deadline, archive_images_inner(bundle, transport, &limits, cancellation, deadline, resource_deadline, memory)) => result.map_err(|_| ArchiveError::Deadline)?,
    }
}

async fn run_archive_worker<Output, Operation>(
    admission: Arc<ArchiveAdmission>,
    cancellation: &Cancellation,
    deadline: Instant,
    operation: Operation,
) -> Result<Output, ArchiveError>
where
    Output: Send + 'static,
    Operation: FnOnce() -> Result<Output, ArchiveError> + Send + 'static,
{
    if cancellation.is_cancelled() {
        return Err(ArchiveError::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(ArchiveError::Deadline);
    }
    let worker_cancellation = cancellation.clone();
    let worker = tokio::task::spawn_blocking(move || {
        let _admission = admission;
        if worker_cancellation.is_cancelled() {
            return Err(ArchiveError::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(ArchiveError::Deadline);
        }
        operation()
    });
    tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err(ArchiveError::Cancelled),
        result = tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), worker) => {
            result.map_err(|_| ArchiveError::Deadline)?.map_err(|_| ArchiveError::Decode)?
        },
    }
}

#[derive(Debug)]
struct ArchivedOriginal {
    image_index: u16,
    format: NetworkImageFormat,
    png_hash: [u8; 32],
    body: Vec<u8>,
}

struct ArchiveAdmission {
    _memory: Option<Arc<super::budgets::PipelineLease>>,
    _network: SemaphorePermit<'static>,
}

fn resolve_originals(
    derived: &PortableBundle,
    originals: Vec<ArchivedOriginal>,
) -> Result<Vec<NetworkImageOriginal>, ArchiveError> {
    let by_hash: std::collections::BTreeMap<[u8; 32], u16> = derived
        .resources
        .iter()
        .map(|resource| (*blake3::hash(&resource.png).as_bytes(), resource.id))
        .collect();
    let mut resolved = Vec::with_capacity(originals.len());
    for original in originals {
        let resource_id = *by_hash
            .get(&original.png_hash)
            .ok_or(ArchiveError::Decode)?;
        resolved.push(NetworkImageOriginal {
            image_index: original.image_index,
            resource_id,
            format: original.format,
            bytes: original.body,
        });
    }
    Ok(resolved)
}

async fn archive_images_inner(
    bundle: &PortableBundle,
    transport: &dyn ResourceTransport,
    limits: &ArchiveLimits,
    cancellation: &Cancellation,
    deadline: Instant,
    resource_deadline: Instant,
    memory: Option<Arc<super::budgets::PipelineLease>>,
) -> Result<ArchiveOutcome, ArchiveError> {
    bundle.validate().map_err(|_| ArchiveError::Decode)?;
    let images = external_images(bundle).map_err(|_| ArchiveError::Decode)?;
    if images.is_empty() {
        return Ok(ArchiveOutcome {
            bundle: bundle.clone(),
            archived: 0,
            deduplicated: 0,
            failures: Vec::new(),
            originals: Vec::new(),
        });
    }
    if images.len() > limits.max_resources {
        return Err(ArchiveError::TooManyResources);
    }
    let network = tokio::select! {
        biased;
        _ = cancellation.cancelled() => return Err(ArchiveError::Cancelled),
        permit = NETWORK_ADMISSION.acquire() => permit.map_err(|_| ArchiveError::Transport)?,
    };
    let admission = Arc::new(ArchiveAdmission {
        _network: network,
        _memory: memory,
    });
    let image_limits = NetworkImageLimits {
        max_pixels: limits.max_pixels_per_resource,
        max_rgba_bytes: usize::try_from(limits.max_pixels_per_resource.saturating_mul(4))
            .unwrap_or(usize::MAX)
            .min(super::RGBA_LIMIT),
        max_derived_bytes: limits.max_derived_bytes_per_resource,
    };
    let mut replacements: Vec<ResourceReplacement> = Vec::new();
    let mut originals: Vec<ArchivedOriginal> = Vec::new();
    let mut failures = Vec::new();
    let mut remaining_wire = limits.max_total_bytes;
    let mut remaining_derived = limits.max_total_derived_bytes;
    let mut remaining_pixels = limits
        .max_total_pixels
        .saturating_sub((bundle_rgba_bytes(bundle)? / 4) as u64);

    for image in &images {
        let position = image.image_index;
        if remaining_derived == 0 || remaining_pixels == 0 {
            failures.push((position, ArchiveError::DerivedBudget));
            continue;
        }
        let parsed = match Url::parse(&image.url) {
            Ok(url) => url,
            Err(_) => {
                failures.push((position, ArchiveError::NotHttp));
                continue;
            }
        };
        let fetched = match fetch_with_redirects(
            transport,
            &parsed,
            limits,
            resource_deadline,
            cancellation,
            &mut remaining_wire,
        )
        .await
        {
            Ok(fetched) => fetched,
            Err(ArchiveError::Cancelled) => return Err(ArchiveError::Cancelled),
            Err(error @ (ArchiveError::TotalBudget | ArchiveError::Deadline))
                if resource_deadline < deadline =>
            {
                failures.push((position, error.clone()));
                if error == ArchiveError::TotalBudget || Instant::now() >= resource_deadline {
                    break;
                }
                continue;
            }
            Err(ArchiveError::TotalBudget) => return Err(ArchiveError::TotalBudget),
            Err(ArchiveError::Deadline) => return Err(ArchiveError::Deadline),
            Err(error) => {
                failures.push((position, error));
                continue;
            }
        };
        let total_limited = remaining_pixels < image_limits.max_pixels
            || remaining_derived < image_limits.max_derived_bytes;
        let decode_limits = NetworkImageLimits {
            max_pixels: image_limits.max_pixels.min(remaining_pixels),
            max_derived_bytes: image_limits.max_derived_bytes.min(remaining_derived),
            ..image_limits
        };
        let (derived, body, image_format) = match run_archive_worker(
            admission.clone(),
            cancellation,
            resource_deadline,
            move || {
                let declared = fetched.declared;
                let decoded =
                    network_image::decode_static_png(&fetched.bytes, declared, &decode_limits);
                let body = fetched.bytes;
                decoded
                    .map(|derived| (derived, body, declared))
                    .map_err(|error| match error {
                        network_image::NetworkImageError::Mime => ArchiveError::Mime,
                        network_image::NetworkImageError::Animated => ArchiveError::Animated,
                        network_image::NetworkImageError::TooLarge if total_limited => {
                            ArchiveError::DerivedBudget
                        }
                        network_image::NetworkImageError::TooLarge => {
                            ArchiveError::ResourceTooLarge
                        }
                        _ => ArchiveError::Decode,
                    })
            },
        )
        .await
        {
            Ok(decoded) => decoded,
            Err(ArchiveError::Deadline) if resource_deadline < deadline => {
                failures.push((position, ArchiveError::Deadline));
                break;
            }
            Err(error @ (ArchiveError::Cancelled | ArchiveError::Deadline)) => return Err(error),
            Err(error) => {
                failures.push((position, error));
                continue;
            }
        };
        if derived.png.len() > remaining_derived || derived.pixels > remaining_pixels {
            failures.push((position, ArchiveError::DerivedBudget));
            continue;
        }
        remaining_derived -= derived.png.len();
        remaining_pixels -= derived.pixels;
        let image_index = u16::try_from(position).map_err(|_| ArchiveError::Decode)?;
        let png_hash = *blake3::hash(&derived.png).as_bytes();
        replacements.push(ResourceReplacement {
            span: image.span,
            png: derived.png,
        });
        originals.push(ArchivedOriginal {
            image_index,
            format: image_format,
            png_hash,
            body,
        });
    }

    if replacements.is_empty() {
        return Ok(ArchiveOutcome {
            bundle: bundle.clone(),
            archived: 0,
            deduplicated: 0,
            failures,
            originals: Vec::new(),
        });
    }

    let archived = replacements.len();
    let source = bundle.clone();
    let (derived, deduplicated, originals) =
        run_archive_worker(admission, cancellation, deadline, move || {
            let (derived, deduplicated) = apply_resource_bindings(&source, &replacements)?;
            let originals = resolve_originals(&derived, originals)?;
            Ok((derived, deduplicated, originals))
        })
        .await?;
    Ok(ArchiveOutcome {
        archived,
        bundle: derived,
        deduplicated,
        failures,
        originals,
    })
}

#[derive(Debug, Clone)]
pub struct ResourceReplacement {
    pub span: ImageSourceSpan,
    pub png: Vec<u8>,
}

pub fn apply_resource_bindings(
    bundle: &PortableBundle,
    replacements: &[ResourceReplacement],
) -> Result<(PortableBundle, usize), ArchiveError> {
    bundle.validate().map_err(|_| ArchiveError::Decode)?;
    let index = bundle
        .representations
        .iter()
        .position(|representation| representation.mime == Mime::Html)
        .ok_or(ArchiveError::Decode)?;
    let mut resources: Vec<Resource> = bundle.resources.clone();
    let mut by_hash: std::collections::BTreeMap<[u8; 32], u16> = resources
        .iter()
        .map(|resource| (*blake3::hash(&resource.png).as_bytes(), resource.id))
        .collect();
    let mut next_id = resources
        .iter()
        .map(|resource| resource.id)
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or(ArchiveError::TooManyResources)?;
    let mut deduplicated = 0usize;
    let mut mapped: Vec<(ImageSourceSpan, u16)> = Vec::with_capacity(replacements.len());
    for replacement in replacements {
        if replacement.span.start >= replacement.span.end {
            return Err(ArchiveError::Decode);
        }
        let hash = *blake3::hash(&replacement.png).as_bytes();
        let id = if let Some(existing) = by_hash.get(&hash) {
            deduplicated += 1;
            *existing
        } else {
            if resources.len() >= super::RESOURCE_LIMIT {
                return Err(ArchiveError::TooManyResources);
            }
            let id = next_id;
            next_id = next_id
                .checked_add(1)
                .ok_or(ArchiveError::TooManyResources)?;
            resources.push(Resource {
                id,
                png: replacement.png.clone(),
            });
            by_hash.insert(hash, id);
            id
        };
        mapped.push((replacement.span, id));
    }
    mapped.sort_by_key(|(span, _)| span.start);
    for pair in mapped.windows(2) {
        if pair[0].0.end > pair[1].0.start {
            return Err(ArchiveError::Decode);
        }
    }

    let mut base_bindings: Vec<&ResourceBinding> = bundle
        .bindings
        .iter()
        .filter(|binding| binding.representation as usize == index)
        .collect();
    base_bindings.sort_by_key(|binding| binding.offset);
    let html = &bundle.representations[index].bytes;
    let mut output: Vec<u8> = Vec::with_capacity(html.len());
    let mut bindings: Vec<ResourceBinding> = bundle
        .bindings
        .iter()
        .filter(|binding| binding.representation as usize != index)
        .cloned()
        .collect();
    let mut cursor = 0usize;
    let mut binding_cursor = 0usize;
    let mut replacement_cursor = 0usize;
    while cursor < html.len() {
        let binding = base_bindings.get(binding_cursor);
        let replacement = mapped.get(replacement_cursor);
        let next_binding = binding.map(|binding| binding.offset as usize);
        let next_replacement = replacement.map(|(span, _)| span.start);
        if let Some(start) =
            next_binding.filter(|start| next_replacement.is_none_or(|next| *start <= next))
        {
            let binding = binding.expect("binding present");
            let end = start + binding.length as usize;
            if start < cursor || end > html.len() {
                return Err(ArchiveError::Decode);
            }
            output.extend_from_slice(&html[cursor..end]);
            bindings.push(ResourceBinding {
                representation: index as u16,
                resource: binding.resource,
                offset: (output.len() - binding.length as usize) as u32,
                length: binding.length,
            });
            cursor = end;
            binding_cursor += 1;
            continue;
        }
        let Some((span, id)) = replacement else {
            break;
        };
        if span.start < cursor || span.end > html.len() {
            return Err(ArchiveError::Decode);
        }
        let reference = format!("tailsync-resource:{id}");
        output.extend_from_slice(&html[cursor..span.start]);
        let offset = output.len();
        output.extend_from_slice(reference.as_bytes());
        bindings.push(ResourceBinding {
            representation: index as u16,
            resource: *id,
            offset: offset as u32,
            length: reference.len() as u32,
        });
        cursor = span.end;
        replacement_cursor += 1;
    }
    output.extend_from_slice(&html[cursor..]);

    let mut derived = bundle.clone();
    derived.representations[index] = Representation {
        mime: Mime::Html,
        bytes: output,
    };
    derived.resources = resources;
    derived.bindings = bindings;
    let html_bindings = derived
        .bindings
        .iter()
        .filter(|binding| binding.representation as usize == index)
        .cloned()
        .collect::<Vec<_>>();
    let closed = super::html::restore_reference_inventory_complete(
        &derived.representations[index].bytes,
        &html_bindings,
    )
    .unwrap_or(false);
    derived.completeness = if bundle.completeness == Completeness::MissingFormats {
        Completeness::MissingFormats
    } else if closed {
        Completeness::Complete
    } else if bundle.completeness == Completeness::Complete {
        Completeness::ExternalReferences
    } else {
        bundle.completeness
    };
    derived.validate().map_err(|_| ArchiveError::Decode)?;
    Ok((derived, deduplicated))
}

pub struct ReqwestTransport;

impl ReqwestTransport {
    pub fn new() -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        Self
    }
}

impl Default for ReqwestTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl ResourceTransport for ReqwestTransport {
    fn fetch_once<'a>(
        &'a self,
        url: &'a Url,
        limits: &'a FetchLimits,
        cancellation: &'a Cancellation,
    ) -> FetchFuture<'a> {
        Box::pin(async move {
            validate_public_url(url)?;
            let fetch = async {
                let addresses = resolve_public(url, cancellation).await?;
                let address = *addresses.first().ok_or(ArchiveError::Resolve)?;
                fetch_pinned(url, address, limits, cancellation).await
            };
            tokio::select! {
                biased;
                _ = cancellation.cancelled() => Err(ArchiveError::Cancelled),
                result = tokio::time::timeout(limits.request_timeout.min(ARCHIVE_REQUEST_TIMEOUT), fetch) => result.map_err(|_| ArchiveError::Deadline)?,
            }
        })
    }
}

async fn fetch_pinned(
    url: &Url,
    address: SocketAddr,
    limits: &FetchLimits,
    cancellation: &Cancellation,
) -> Result<FetchResponse, ArchiveError> {
    let host = url.host_str().ok_or(ArchiveError::NotPublic)?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .timeout(limits.request_timeout.min(ARCHIVE_REQUEST_TIMEOUT))
        .resolve(host, address)
        .build()
        .map_err(|_| ArchiveError::Transport)?;
    let response = tokio::select! {
        biased;
        _ = cancellation.cancelled() => return Err(ArchiveError::Cancelled),
        response = client.get(url.clone()).header(reqwest::header::ACCEPT, "image/png, image/jpeg, image/webp").header(reqwest::header::ACCEPT_ENCODING, "identity").send() => response.map_err(|_| ArchiveError::Transport)?,
    };
    if response.remote_addr() != Some(address) {
        return Err(ArchiveError::NotPublic);
    }
    if response
        .headers()
        .get_all(reqwest::header::CONTENT_ENCODING)
        .iter()
        .any(|value| value.to_str().ok() != Some("identity"))
    {
        return Err(ArchiveError::Mime);
    }
    let status = response.status().as_u16();
    let location = response
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let content_types = response
        .headers()
        .get_all(reqwest::header::CONTENT_TYPE)
        .iter()
        .collect::<Vec<_>>();
    let content_type = match content_types.as_slice() {
        [value] => value.to_str().ok().map(str::to_string),
        _ => None,
    };
    if status != 200 {
        return Ok(FetchResponse {
            status,
            location,
            content_type,
            bytes: Vec::new(),
        });
    }
    if content_type
        .as_deref()
        .and_then(network_image::declared_format)
        .is_none()
    {
        return Err(ArchiveError::Mime);
    }
    if response
        .content_length()
        .is_some_and(|length| length > limits.max_bytes.min(ARCHIVE_BYTES_PER_RESOURCE) as u64)
    {
        return Err(ArchiveError::ResourceTooLarge);
    }
    let bytes = tokio::select! {
        biased;
        _ = cancellation.cancelled() => return Err(ArchiveError::Cancelled),
        bytes = read_bounded(response, limits.max_bytes.min(ARCHIVE_BYTES_PER_RESOURCE)) => bytes?,
    };
    Ok(FetchResponse {
        status,
        location,
        content_type,
        bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composite::network_image::NetworkImageFormat;
    use crate::composite::network_originals::NetworkImageOriginal;
    use crate::composite::{packed_to_png, png_to_packed, PortableBundle, Representation};
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    struct MockTransport {
        responses: Mutex<BTreeMap<String, Vec<FetchResponse>>>,
        calls: Mutex<Vec<String>>,
    }

    impl MockTransport {
        fn new() -> Self {
            Self {
                responses: Mutex::new(BTreeMap::new()),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn push(&self, url: &str, response: FetchResponse) {
            self.responses
                .lock()
                .unwrap()
                .entry(url.to_string())
                .or_default()
                .push(response);
        }
    }

    impl ResourceTransport for MockTransport {
        fn fetch_once<'a>(
            &'a self,
            url: &'a Url,
            _limits: &'a FetchLimits,
            cancellation: &'a Cancellation,
        ) -> FetchFuture<'a> {
            Box::pin(async move {
                if cancellation.is_cancelled() {
                    return Err(ArchiveError::Cancelled);
                }
                self.calls.lock().unwrap().push(url.to_string());
                let mut responses = self.responses.lock().unwrap();
                let queue = responses
                    .get_mut(url.as_str())
                    .ok_or(ArchiveError::Resolve)?;
                if queue.is_empty() {
                    return Err(ArchiveError::Transport);
                }
                Ok(queue.remove(0))
            })
        }
    }

    fn png(color: [u8; 4]) -> Vec<u8> {
        packed_to_png(1, 1, &color).unwrap()
    }

    fn html_bundle(html: &str) -> PortableBundle {
        PortableBundle {
            representations: vec![Representation {
                mime: Mime::Html,
                bytes: html.as_bytes().to_vec(),
            }],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::ExternalReferences,
        }
    }

    fn fetch(status: u16, bytes: Vec<u8>) -> FetchResponse {
        FetchResponse {
            status,
            location: None,
            content_type: Some("image/png".into()),
            bytes,
        }
    }

    #[test]
    fn private_and_mapped_addresses_are_refused() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.5.4",
            "192.168.1.1",
            "169.254.10.1",
            "100.64.0.1",
            "0.0.0.0",
            "224.0.0.1",
            "198.51.100.7",
            "203.0.113.9",
            "::1",
            "fc00::1",
            "fe80::1",
            "ff02::1",
            "::ffff:10.0.0.1",
            "::ffff:127.0.0.1",
            "64:ff9b::1.2.3.4",
        ] {
            let addr: IpAddr = ip.parse().unwrap();
            assert!(!is_public_ip(addr), "{ip} must not be public");
        }
        for ip in ["1.1.1.1", "8.8.8.8", "2606:4700::1111"] {
            let addr: IpAddr = ip.parse().unwrap();
            assert!(is_public_ip(addr), "{ip} must be public");
        }
    }

    #[test]
    fn url_policy_refuses_user_info_and_non_http() {
        assert_eq!(
            validate_public_url(&Url::parse("ftp://example.com/a.png").unwrap()),
            Err(ArchiveError::NotHttp)
        );
        assert_eq!(
            validate_public_url(&Url::parse("https://user:pw@example.com/a.png").unwrap()),
            Err(ArchiveError::UserInfo)
        );
        assert!(validate_public_url(&Url::parse("https://example.com/a.png").unwrap()).is_ok());
    }

    #[test]
    fn two_images_bind_to_their_exact_spans_and_materialize_offline() {
        let red = png([255, 0, 0, 255]);
        let green = png([0, 255, 0, 255]);
        let html =
            "<p>x</p><img src=\"https://a.example/1.png\"><img src='https://b.example/2.png'>";
        let bundle = html_bundle(html);
        let transport = MockTransport::new();
        transport.push("https://a.example/1.png", fetch(200, red.clone()));
        transport.push("https://b.example/2.png", fetch(200, green.clone()));
        let outcome = futures_lite_block_on(archive_external_images(
            &bundle,
            &transport,
            &ArchiveLimits::default(),
            &Cancellation::default(),
        ))
        .unwrap();
        assert!(outcome.complete(), "both images archive");
        assert_eq!(outcome.archived, 2);
        assert_eq!(outcome.bundle.resources.len(), 2);
        let first = outcome.bundle.representations[0]
            .bytes
            .windows(18)
            .any(|window| window == b"tailsync-resource:");
        assert!(first);
        let materialized = crate::composite::html::materialize(&outcome.bundle, 0).unwrap();
        let materialized = String::from_utf8(materialized).unwrap();
        assert!(materialized.contains("data:image/png;base64,"));
        assert!(materialized.contains("<p>x</p>"));
        assert!(!materialized.contains("a.example"));
    }

    #[test]
    fn same_bytes_from_different_urls_deduplicate() {
        let red = png([255, 0, 0, 255]);
        let html = "<img src='https://a.example/1.png'><img src='https://b.example/2.png'>";
        let bundle = html_bundle(html);
        let transport = MockTransport::new();
        transport.push("https://a.example/1.png", fetch(200, red.clone()));
        transport.push("https://b.example/2.png", fetch(200, red.clone()));
        let outcome = futures_lite_block_on(archive_external_images(
            &bundle,
            &transport,
            &ArchiveLimits::default(),
            &Cancellation::default(),
        ))
        .unwrap();
        assert_eq!(outcome.archived, 2);
        assert_eq!(outcome.deduplicated, 1);
        assert_eq!(outcome.bundle.resources.len(), 1);
    }

    #[test]
    fn changed_content_is_a_new_resource() {
        let html = "<img src='https://a.example/1.png'>";
        let transport = MockTransport::new();
        transport.push("https://a.example/1.png", fetch(200, png([1, 2, 3, 255])));
        let first = futures_lite_block_on(archive_external_images(
            &html_bundle(html),
            &transport,
            &ArchiveLimits::default(),
            &Cancellation::default(),
        ))
        .unwrap();
        let transport = MockTransport::new();
        transport.push("https://a.example/1.png", fetch(200, png([9, 9, 9, 255])));
        let second = futures_lite_block_on(archive_external_images(
            &html_bundle(html),
            &transport,
            &ArchiveLimits::default(),
            &Cancellation::default(),
        ))
        .unwrap();
        assert_ne!(
            first.bundle.resources[0].png, second.bundle.resources[0].png,
            "same URL with new bytes is a new capture"
        );
    }

    #[test]
    fn bad_bytes_and_status_keep_the_reference_verbatim() {
        let html = "<img src='https://a.example/bad.png'>";
        let transport = MockTransport::new();
        transport.push(
            "https://a.example/bad.png",
            fetch(200, b"not a png".to_vec()),
        );
        let outcome = futures_lite_block_on(archive_external_images(
            &html_bundle(html),
            &transport,
            &ArchiveLimits::default(),
            &Cancellation::default(),
        ))
        .unwrap();
        assert_eq!(outcome.archived, 0);
        assert_eq!(outcome.failures, vec![(0, ArchiveError::Decode)]);
        assert!(
            String::from_utf8(outcome.bundle.representations[0].bytes.clone())
                .unwrap()
                .contains("https://a.example/bad.png")
        );
        assert_eq!(
            outcome.bundle.completeness,
            Completeness::ExternalReferences
        );
    }

    #[test]
    fn redirects_are_bounded_and_revalidated() {
        let html = "<img src='https://a.example/1.png'>";
        let transport = MockTransport::new();
        let mut redirect = fetch(302, Vec::new());
        redirect.location = Some("http://127.0.0.1/secret.png".into());
        transport.push("https://a.example/1.png", redirect);
        let outcome = futures_lite_block_on(archive_external_images(
            &html_bundle(html),
            &transport,
            &ArchiveLimits::default(),
            &Cancellation::default(),
        ))
        .unwrap();
        assert_eq!(outcome.archived, 0);
        assert_eq!(outcome.failures, vec![(0, ArchiveError::NotPublic)]);
    }

    #[test]
    fn redirect_limit_is_enforced() {
        let html = "<img src='https://a.example/1.png'>";
        let transport = MockTransport::new();
        let limits = ArchiveLimits {
            max_redirects: 1,
            ..ArchiveLimits::default()
        };
        for url in ["https://a.example/1.png", "https://a.example/3.png"] {
            let mut redirect = fetch(302, Vec::new());
            redirect.location = Some("https://a.example/3.png".into());
            transport.push(url, redirect);
        }
        let outcome = futures_lite_block_on(archive_external_images(
            &html_bundle(html),
            &transport,
            &limits,
            &Cancellation::default(),
        ))
        .unwrap();
        assert_eq!(outcome.archived, 0);
        assert_eq!(outcome.failures, vec![(0, ArchiveError::RedirectLimit)]);
    }

    #[test]
    fn oversized_body_is_refused() {
        let html = "<img src='https://a.example/1.png'>";
        let transport = MockTransport::new();
        transport.push("https://a.example/1.png", fetch(200, vec![0; 32]));
        let limits = ArchiveLimits {
            max_bytes_per_resource: 16,
            ..ArchiveLimits::default()
        };
        let outcome = futures_lite_block_on(archive_external_images(
            &html_bundle(html),
            &transport,
            &limits,
            &Cancellation::default(),
        ))
        .unwrap();
        assert_eq!(outcome.archived, 0);
        assert!(outcome
            .failures
            .iter()
            .any(|(_, error)| *error == ArchiveError::ResourceTooLarge));
    }

    #[test]
    fn cancelled_archive_returns_cancelled() {
        let html = "<img src='https://a.example/1.png'>";
        let transport = MockTransport::new();
        transport.push("https://a.example/1.png", fetch(200, png([1, 1, 1, 255])));
        let cancellation = Cancellation::default();
        cancellation.cancel();
        let error = futures_lite_block_on(archive_external_images(
            &html_bundle(html),
            &transport,
            &ArchiveLimits::default(),
            &cancellation,
        ))
        .unwrap_err();
        assert_eq!(error, ArchiveError::Cancelled);
    }

    #[test]
    fn already_bound_and_inline_images_are_not_external() {
        let inline = STANDARD.encode(png([2, 2, 2, 255]));
        let html = format!(
            "<img src=\"data:image/png;base64,{inline}\"><img src='https://a.example/x.png'>"
        );
        let mut bundle = html_bundle(&html);
        crate::composite::html::extract_resources(&mut bundle).unwrap();
        let images = external_images(&bundle).unwrap();
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].url, "https://a.example/x.png");
    }

    #[test]
    fn o04_review_special_addresses_and_mixed_dns_fail_closed() {
        for address in [
            "::2",
            "::ffff:8.8.8.8",
            "2001:2::1",
            "2001:20::1",
            "3fff::1",
            "5f00::1",
        ] {
            assert!(!is_public_ip(address.parse().unwrap()));
        }
        assert!(validate_addresses(&[
            "8.8.8.8:443".parse().unwrap(),
            "127.0.0.1:443".parse().unwrap()
        ])
        .is_err());
        assert!(validate_addresses(&["8.8.8.8:443".parse().unwrap()]).is_ok());
    }

    #[tokio::test]
    async fn o04_review_pending_fetch_is_deadlined_and_cancelled() {
        struct Pending;
        impl ResourceTransport for Pending {
            fn fetch_once<'a>(
                &'a self,
                _: &'a Url,
                _: &'a FetchLimits,
                _: &'a Cancellation,
            ) -> FetchFuture<'a> {
                Box::pin(std::future::pending())
            }
        }
        let limits = ArchiveLimits {
            total_deadline: Duration::from_millis(20),
            ..ArchiveLimits::default()
        };
        let result = tokio::time::timeout(
            Duration::from_millis(200),
            archive_external_images(
                &html_bundle("<img src='https://a.example/a.png'>"),
                &Pending,
                &limits,
                &Cancellation::default(),
            ),
        )
        .await
        .unwrap();
        assert!(matches!(result, Err(ArchiveError::Deadline)));
        let cancellation = Cancellation::default();
        let cancel = cancellation.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            cancel.cancel();
        });
        assert!(matches!(
            archive_external_images(
                &html_bundle("<img src='https://a.example/a.png'>"),
                &Pending,
                &ArchiveLimits::default(),
                &cancellation
            )
            .await,
            Err(ArchiveError::Cancelled)
        ));
    }

    #[test]
    fn o04_review_ambiguous_sources_mime_and_missing_formats_are_not_certified() {
        for html in [
            "<img src='https://a.example/a.png' src='https://a.example/b.png'>",
            "<img src='https://a.example/a.png'",
            "<img src='https://a.example/a.png?x=1&unknown;y=2'>",
        ] {
            assert!(external_images(&html_bundle(html)).is_err());
        }
        let transport = MockTransport::new();
        let mut response = fetch(200, png([1, 2, 3, 255]));
        response.content_type = Some("text/html".into());
        transport.push("https://a.example/a.png", response);
        let outcome = futures_lite_block_on(archive_external_images(
            &html_bundle("<img src='https://a.example/a.png'>"),
            &transport,
            &ArchiveLimits::default(),
            &Cancellation::default(),
        ))
        .unwrap();
        assert_eq!(outcome.failures, vec![(0, ArchiveError::Mime)]);
        let transport = MockTransport::new();
        transport.push("https://a.example/a.png", fetch(200, png([1, 2, 3, 255])));
        let mut base = html_bundle("<img src='https://a.example/a.png'>");
        base.completeness = Completeness::MissingFormats;
        let outcome = futures_lite_block_on(archive_external_images(
            &base,
            &transport,
            &ArchiveLimits::default(),
            &Cancellation::default(),
        ))
        .unwrap();
        assert_eq!(outcome.bundle.completeness, Completeness::MissingFormats);
        assert!(!outcome.complete());
        assert!(!outcome.images_complete());
    }

    #[test]
    fn o04_review_bad_images_spend_the_aggregate_wire_budget() {
        let transport = MockTransport::new();
        for index in 1..=3 {
            transport.push(
                &format!("https://a.example/{index}.png"),
                fetch(200, vec![0; 10]),
            );
        }
        let limits = ArchiveLimits {
            max_total_bytes: 16,
            ..ArchiveLimits::default()
        };
        let result = futures_lite_block_on(archive_external_images(&html_bundle("<img src='https://a.example/1.png'><img src='https://a.example/2.png'><img src='https://a.example/3.png'>"), &transport, &limits, &Cancellation::default()));
        assert!(result.is_err());
        assert_eq!(transport.calls.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn o04_review_real_pinned_transport_checks_encoding_mime_and_body() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let _ = ReqwestTransport::new();
        let image = png([1, 2, 3, 255]);
        let jpeg = fixture(JPEG_1X1);
        let webp = fixture(WEBP_LOSSLESS_1X1);
        for (headers, body, max_bytes, success) in [
            ("Content-Type: image/png\r\n".to_string(), image.clone(), image.len(), true),
            ("Content-Type: image/jpeg\r\n".to_string(), jpeg.clone(), jpeg.len(), true),
            ("Content-Type: image/webp\r\n".to_string(), webp.clone(), webp.len(), true),
            ("Content-Type: image/gif\r\n".to_string(), jpeg.clone(), jpeg.len(), false),
            ("Content-Type: text/html\r\n".to_string(), image.clone(), image.len(), false),
            ("Content-Type: image/png\r\nContent-Encoding: gzip\r\n".to_string(), image.clone(), image.len(), false),
            ("Content-Type: image/png\r\nContent-Encoding: identity\r\nContent-Encoding: gzip\r\n".to_string(), image.clone(), image.len(), false),
            ("Content-Type: image/png\r\n".to_string(), image.clone(), 8, false),
            ("Content-Type: image/png\r\nTransfer-Encoding: chunked\r\n".to_string(), b"20\r\n01234567890123456789012345678901\r\n0\r\n\r\n".to_vec(), 8, false),
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let response = if headers.contains("Transfer-Encoding") { format!("HTTP/1.1 200 OK\r\n{headers}Connection: close\r\n\r\n") } else { format!("HTTP/1.1 200 OK\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n", body.len()) };
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 1024];
                while !request.ends_with(b"\r\n\r\n") && request.len() < 4096 { let read = socket.read(&mut buffer).await.unwrap(); if read == 0 { break; } request.extend_from_slice(&buffer[..read]); }
                let request = String::from_utf8(request).unwrap().to_ascii_lowercase();
                assert!(!request.contains("authorization:") && !request.contains("cookie:"));
                assert!(request.contains("accept: image/png, image/jpeg, image/webp"));
                let _ = socket.write_all(&[response.as_bytes(), &body].concat()).await;
            });
            let url = Url::parse(&format!("http://archive-fixture.example:{}/image.png", address.port())).unwrap();
            let result = fetch_pinned(&url, address, &FetchLimits { max_bytes, request_timeout: Duration::from_secs(1) }, &Cancellation::default()).await;
            assert_eq!(result.is_ok(), success);
            server.await.unwrap();
        }
        let url = Url::parse("http://127.0.0.1/image.png").unwrap();
        assert!(matches!(
            ReqwestTransport::new()
                .fetch_once(
                    &url,
                    &FetchLimits {
                        max_bytes: 1024,
                        request_timeout: Duration::from_secs(1)
                    },
                    &Cancellation::default()
                )
                .await,
            Err(ArchiveError::NotPublic)
        ));
    }

    use crate::composite::network_image::fixtures::{
        APNG_TWO_FRAME, JPEG_1X1, PNG_1X1, PNG_2X2, PNG_NOISE_16X16,
        PNG_SINGLE_FRAME_ANIMATION_FLAG, WEBP_ANIMATED, WEBP_LOSSLESS_1X1, WEBP_LOSSY_1X1,
        WEBP_SINGLE_FRAME_ANIMATION_FLAG,
    };

    fn fixture(encoded: &str) -> Vec<u8> {
        STANDARD.decode(encoded).unwrap()
    }

    fn typed_fetch(status: u16, content_type: &str, bytes: Vec<u8>) -> FetchResponse {
        FetchResponse {
            status,
            location: None,
            content_type: Some(content_type.into()),
            bytes,
        }
    }

    fn archive_one(declared: &str, body: Vec<u8>) -> ArchiveOutcome {
        let transport = MockTransport::new();
        transport.push("https://a.example/a", typed_fetch(200, declared, body));
        futures_lite_block_on(archive_external_images(
            &html_bundle("<img src='https://a.example/a'>"),
            &transport,
            &ArchiveLimits::default(),
            &Cancellation::default(),
        ))
        .unwrap()
    }

    #[test]
    fn public_png_jpeg_and_static_webp_archive_in_order() {
        let html = "<p>x</p><img src='https://a.example/1.png'><img src='https://b.example/2.jpg'><img src='https://c.example/3.webp'>";
        let transport = MockTransport::new();
        transport.push(
            "https://a.example/1.png",
            typed_fetch(200, "image/png", fixture(PNG_1X1)),
        );
        transport.push(
            "https://b.example/2.jpg",
            typed_fetch(200, "image/jpeg", fixture(JPEG_1X1)),
        );
        transport.push(
            "https://c.example/3.webp",
            typed_fetch(200, "image/webp", fixture(WEBP_LOSSLESS_1X1)),
        );
        let outcome = futures_lite_block_on(archive_external_images(
            &html_bundle(html),
            &transport,
            &ArchiveLimits::default(),
            &Cancellation::default(),
        ))
        .unwrap();
        assert!(outcome.failures.is_empty(), "{:?}", outcome.failures);
        assert_eq!(outcome.archived, 3);
        assert_eq!(outcome.bundle.resources.len(), 3);
        assert!(outcome.complete());
        for resource in &outcome.bundle.resources {
            assert!(resource.png.starts_with(b"\x89PNG\r\n\x1a\n"));
            assert_eq!(png_to_packed(&resource.png).unwrap().len(), 8 + 4);
        }
        let bindings = outcome
            .bundle
            .bindings
            .iter()
            .filter(|binding| binding.representation == 0)
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(bindings.len(), 3);
        assert!(bindings
            .windows(2)
            .all(|pair| pair[0].offset < pair[1].offset));
        let html_bytes = &outcome.bundle.representations[0].bytes;
        for (position, binding) in bindings.iter().enumerate() {
            let start = binding.offset as usize;
            let end = start + binding.length as usize;
            let text = std::str::from_utf8(&html_bytes[start..end]).unwrap();
            assert_eq!(
                text,
                format!(
                    "tailsync-resource:{}",
                    outcome.bundle.resources[position].id
                )
            );
        }
        let materialized =
            String::from_utf8(crate::composite::html::materialize(&outcome.bundle, 0).unwrap())
                .unwrap();
        assert_eq!(materialized.matches("data:image/png;base64,").count(), 3);
        assert!(!materialized.contains("a.example"));
        assert!(!materialized.contains("b.example"));
        assert!(!materialized.contains("c.example"));
    }

    #[test]
    fn declared_mime_must_match_the_real_container() {
        for (declared, body) in [
            ("image/jpeg", PNG_1X1),
            ("image/png", JPEG_1X1),
            ("image/webp", JPEG_1X1),
            ("image/png", WEBP_LOSSLESS_1X1),
        ] {
            let outcome = archive_one(declared, fixture(body));
            assert_eq!(outcome.archived, 0, "{declared}");
            assert_eq!(
                outcome.failures,
                vec![(0, ArchiveError::Mime)],
                "{declared}"
            );
        }
        let outcome = archive_one("image/png", b"not an image".to_vec());
        assert_eq!(outcome.failures, vec![(0, ArchiveError::Decode)]);
    }

    #[test]
    fn animated_containers_are_refused_even_with_a_single_frame() {
        for (declared, body) in [
            ("image/png", APNG_TWO_FRAME),
            ("image/png", PNG_SINGLE_FRAME_ANIMATION_FLAG),
            ("image/webp", WEBP_ANIMATED),
            ("image/webp", WEBP_SINGLE_FRAME_ANIMATION_FLAG),
        ] {
            let outcome = archive_one(declared, fixture(body));
            assert_eq!(outcome.archived, 0, "{declared}");
            assert_eq!(
                outcome.failures,
                vec![(0, ArchiveError::Animated)],
                "{declared}"
            );
        }
        assert_eq!(
            archive_one("image/webp", fixture(WEBP_LOSSY_1X1)).archived,
            1
        );
        assert_eq!(
            archive_one("image/webp", fixture(WEBP_LOSSLESS_1X1)).archived,
            1
        );
    }

    #[test]
    fn truncated_or_corrupt_bodies_are_refused() {
        let mut truncated_png = fixture(PNG_1X1);
        truncated_png.truncate(40);
        let mut truncated_webp = fixture(WEBP_LOSSLESS_1X1);
        truncated_webp.truncate(20);
        let mut corrupt_jpeg = fixture(JPEG_1X1);
        for byte in &mut corrupt_jpeg[3..] {
            *byte = 0;
        }
        for (declared, body) in [
            ("image/png", truncated_png),
            ("image/webp", truncated_webp),
            ("image/jpeg", corrupt_jpeg),
        ] {
            let outcome = archive_one(declared, body);
            assert_eq!(outcome.archived, 0, "{declared}");
            assert_eq!(
                outcome.failures,
                vec![(0, ArchiveError::Decode)],
                "{declared}"
            );
        }
    }

    #[test]
    fn oversized_pixels_and_derived_output_are_bounded() {
        let transport = MockTransport::new();
        transport.push(
            "https://a.example/a",
            typed_fetch(200, "image/png", fixture(PNG_2X2)),
        );
        let limits = ArchiveLimits {
            max_pixels_per_resource: 3,
            ..ArchiveLimits::default()
        };
        let outcome = futures_lite_block_on(archive_external_images(
            &html_bundle("<img src='https://a.example/a'>"),
            &transport,
            &limits,
            &Cancellation::default(),
        ))
        .unwrap();
        assert_eq!(outcome.failures, vec![(0, ArchiveError::ResourceTooLarge)]);

        let transport = MockTransport::new();
        transport.push(
            "https://a.example/a",
            typed_fetch(200, "image/png", fixture(PNG_NOISE_16X16)),
        );
        let limits = ArchiveLimits {
            max_derived_bytes_per_resource: 64,
            ..ArchiveLimits::default()
        };
        let outcome = futures_lite_block_on(archive_external_images(
            &html_bundle("<img src='https://a.example/a'>"),
            &transport,
            &limits,
            &Cancellation::default(),
        ))
        .unwrap();
        assert_eq!(outcome.failures, vec![(0, ArchiveError::ResourceTooLarge)]);

        let noise = fixture(PNG_NOISE_16X16);
        let derived_bytes = crate::composite::network_image::decode_static_png(
            &noise,
            crate::composite::network_image::NetworkImageFormat::Png,
            &crate::composite::network_image::NetworkImageLimits::default(),
        )
        .unwrap()
        .png
        .len();
        let transport = MockTransport::new();
        transport.push(
            "https://a.example/1",
            typed_fetch(200, "image/png", noise.clone()),
        );
        transport.push("https://a.example/2", typed_fetch(200, "image/png", noise));
        let limits = ArchiveLimits {
            max_derived_bytes_per_resource: derived_bytes + 16,
            max_total_derived_bytes: derived_bytes + derived_bytes / 2,
            ..ArchiveLimits::default()
        };
        let outcome = futures_lite_block_on(archive_external_images(
            &html_bundle("<img src='https://a.example/1'><img src='https://a.example/2'>"),
            &transport,
            &limits,
            &Cancellation::default(),
        ))
        .unwrap();
        assert_eq!(outcome.archived, 1);
        assert_eq!(outcome.failures, vec![(1, ArchiveError::DerivedBudget)]);
        assert_eq!(outcome.bundle.resources.len(), 1);

        let transport = MockTransport::new();
        transport.push(
            "https://a.example/1",
            typed_fetch(200, "image/png", fixture(PNG_NOISE_16X16)),
        );
        transport.push(
            "https://a.example/2",
            typed_fetch(200, "image/png", fixture(PNG_NOISE_16X16)),
        );
        let limits = ArchiveLimits {
            max_total_pixels: 300,
            ..ArchiveLimits::default()
        };
        let outcome = futures_lite_block_on(archive_external_images(
            &html_bundle("<img src='https://a.example/1'><img src='https://a.example/2'>"),
            &transport,
            &limits,
            &Cancellation::default(),
        ))
        .unwrap();
        assert_eq!(outcome.archived, 1);
        assert_eq!(outcome.failures, vec![(1, ArchiveError::DerivedBudget)]);
    }

    #[test]
    fn cancellation_still_stops_a_jpeg_and_webp_archive() {
        for (declared, body) in [("image/jpeg", JPEG_1X1), ("image/webp", WEBP_LOSSLESS_1X1)] {
            let transport = MockTransport::new();
            transport.push(
                "https://a.example/a",
                typed_fetch(200, declared, fixture(body)),
            );
            let cancellation = Cancellation::default();
            cancellation.cancel();
            let error = futures_lite_block_on(archive_external_images(
                &html_bundle("<img src='https://a.example/a'>"),
                &transport,
                &ArchiveLimits::default(),
                &cancellation,
            ))
            .unwrap_err();
            assert_eq!(error, ArchiveError::Cancelled);
        }
    }

    #[test]
    fn existing_images_spend_pixels_not_rgba_bytes() {
        let mut bundle = html_bundle("<img src='https://a.example/1'>");
        bundle.resources.push(Resource {
            id: 1,
            png: png([1, 2, 3, 255]),
        });
        let transport = MockTransport::new();
        transport.push("https://a.example/1", fetch(200, png([3, 2, 1, 255])));
        let limits = ArchiveLimits {
            max_total_pixels: 2,
            ..ArchiveLimits::default()
        };
        let outcome = futures_lite_block_on(archive_external_images(
            &bundle,
            &transport,
            &limits,
            &Cancellation::default(),
        ))
        .unwrap();
        assert_eq!(outcome.archived, 1);
        assert!(outcome.failures.is_empty());
        assert_eq!(outcome.bundle.resources.len(), 2);
    }

    #[test]
    fn browser_context_resolves_relative_images_and_query_entities() {
        let bundle = html_bundle("<base href='https://a.example/posts/'><p>saved</p><img src='../image.png?x=1&amp;y=2'>");
        let transport = MockTransport::new();
        transport.push(
            "https://a.example/image.png?x=1&y=2",
            fetch(200, png([3, 2, 1, 255])),
        );
        let original = bundle.encode().unwrap();
        let outcome = futures_lite_block_on(archive_external_images(
            &bundle,
            &transport,
            &ArchiveLimits::default(),
            &Cancellation::default(),
        ))
        .unwrap();
        assert_eq!(outcome.archived, 1);
        assert!(outcome.failures.is_empty());
        assert_eq!(
            transport.calls.lock().unwrap().as_slice(),
            ["https://a.example/image.png?x=1&y=2"]
        );
        assert_eq!(bundle.encode().unwrap(), original);
        validate_archive_derivation(&bundle, &outcome.bundle).unwrap();
        let binding = &outcome.bundle.bindings[0];
        let html = &outcome.bundle.representations[0].bytes;
        assert_eq!(
            &html[binding.offset as usize..(binding.offset + binding.length) as usize],
            b"tailsync-resource:1"
        );
    }

    #[test]
    fn resolved_image_urls_still_reject_private_credentials_and_control_bytes() {
        for html in [
            "<base href='https://127.0.0.1/'><img src='image.png'>",
            "<base href='https://a.example/'><base href='https://b.example/'><img src='image.png'>",
            "<img src='https://user:password@a.example/image.png?x=1&amp;y=2'>",
            "<img src='https://a.example/image.png?x=1&#10;y=2'>",
            "<img src='https://a.example&#92;@127.0.0.1/image.png'>",
            "<base href='https://a.example/'><img src>",
        ] {
            let transport = MockTransport::new();
            let result = futures_lite_block_on(archive_external_images(
                &html_bundle(html),
                &transport,
                &ArchiveLimits::default(),
                &Cancellation::default(),
            ));
            assert!(result.is_err() || result.as_ref().unwrap().archived == 0);
            assert!(
                transport.calls.lock().unwrap().is_empty(),
                "unexpected request: {html}"
            );
        }
    }

    #[tokio::test]
    async fn cancelled_waiter_does_not_release_a_running_archive_worker() {
        static WORKER_ADMISSION: Semaphore = Semaphore::const_new(1);
        let ledger = Arc::new(super::super::budgets::BudgetLedger::default());
        let memory = Arc::new(ledger.reserve_pipeline(64 * 1024 * 1024).unwrap());
        let admission = Arc::new(ArchiveAdmission {
            _network: WORKER_ADMISSION.acquire().await.unwrap(),
            _memory: Some(memory),
        });
        let cancellation = Cancellation::default();
        let worker_cancellation = cancellation.clone();
        let (started, observed) = tokio::sync::oneshot::channel();
        let (release, held) = std::sync::mpsc::channel();
        let waiter = tokio::spawn(async move {
            run_archive_worker(
                admission,
                &worker_cancellation,
                Instant::now() + Duration::from_secs(5),
                move || {
                    started.send(()).unwrap();
                    held.recv().unwrap();
                    Ok(())
                },
            )
            .await
        });
        observed.await.unwrap();
        cancellation.cancel();
        let outcome = tokio::time::timeout(Duration::from_secs(1), waiter).await;
        let retained = WORKER_ADMISSION.try_acquire().is_err();
        assert_eq!(ledger.snapshot().pipeline_reserved, 64 * 1024 * 1024);
        release.send(()).unwrap();
        assert_eq!(outcome.unwrap().unwrap(), Err(ArchiveError::Cancelled));
        assert!(retained);
        assert!(
            tokio::time::timeout(Duration::from_secs(1), WORKER_ADMISSION.acquire())
                .await
                .unwrap()
                .is_ok()
        );
    }

    #[tokio::test]
    async fn deadline_returns_without_waiting_for_a_blocked_archive_worker() {
        static WORKER_ADMISSION: Semaphore = Semaphore::const_new(1);
        let ledger = Arc::new(super::super::budgets::BudgetLedger::default());
        let memory = Arc::new(ledger.reserve_pipeline(64 * 1024 * 1024).unwrap());
        let admission = Arc::new(ArchiveAdmission {
            _network: WORKER_ADMISSION.acquire().await.unwrap(),
            _memory: Some(memory),
        });
        let (started, observed) = tokio::sync::oneshot::channel();
        let (release, held) = std::sync::mpsc::channel();
        let waiter = tokio::spawn(async move {
            run_archive_worker(
                admission,
                &Cancellation::default(),
                Instant::now() + Duration::from_millis(100),
                move || {
                    started.send(()).unwrap();
                    held.recv().unwrap();
                    Ok(())
                },
            )
            .await
        });
        observed.await.unwrap();
        let outcome = tokio::time::timeout(Duration::from_secs(1), waiter).await;
        let retained = WORKER_ADMISSION.try_acquire().is_err();
        assert_eq!(ledger.snapshot().pipeline_reserved, 64 * 1024 * 1024);
        release.send(()).unwrap();
        assert_eq!(outcome.unwrap().unwrap(), Err(ArchiveError::Deadline));
        assert!(retained);
        assert!(
            tokio::time::timeout(Duration::from_secs(1), WORKER_ADMISSION.acquire())
                .await
                .unwrap()
                .is_ok()
        );
    }

    fn derived_png(declared: NetworkImageFormat, body: &[u8]) -> Vec<u8> {
        crate::composite::network_image::decode_static_png(
            body,
            declared,
            &crate::composite::network_image::NetworkImageLimits::default(),
        )
        .unwrap()
        .png
    }

    fn resource_id_for(bundle: &PortableBundle, png: &[u8]) -> u16 {
        let hash = *blake3::hash(png).as_bytes();
        bundle
            .resources
            .iter()
            .find(|resource| *blake3::hash(&resource.png).as_bytes() == hash)
            .unwrap()
            .id
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

    fn png_with_ancillary_text(base: &[u8], keyword: &[u8], text: &[u8]) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(keyword);
        payload.push(0);
        payload.extend_from_slice(text);
        let mut crc_input = Vec::new();
        crc_input.extend_from_slice(b"tEXt");
        crc_input.extend_from_slice(&payload);
        let mut chunk = Vec::new();
        chunk.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        chunk.extend_from_slice(&crc_input);
        chunk.extend_from_slice(&crc32(&crc_input).to_be_bytes());
        let insert_at = base.len() - 12;
        let mut tagged = Vec::with_capacity(base.len() + chunk.len());
        tagged.extend_from_slice(&base[..insert_at]);
        tagged.extend_from_slice(&chunk);
        tagged.extend_from_slice(&base[insert_at..]);
        tagged
    }

    #[test]
    fn originals_keep_each_admitted_body_in_source_order() {
        let cases = [
            ("image/png", PNG_1X1, NetworkImageFormat::Png),
            ("image/jpeg", JPEG_1X1, NetworkImageFormat::Jpeg),
            ("image/webp", WEBP_LOSSLESS_1X1, NetworkImageFormat::WebP),
        ];
        let urls = [
            "https://a.example/1",
            "https://b.example/2",
            "https://c.example/3",
        ];
        let transport = MockTransport::new();
        for (url, (declared, encoded, _)) in urls.iter().zip(cases.iter()) {
            transport.push(url, typed_fetch(200, declared, fixture(encoded)));
        }
        let outcome = futures_lite_block_on(archive_external_images(
            &html_bundle(
                "<img src='https://a.example/1'><img src='https://b.example/2'><img src='https://c.example/3'>",
            ),
            &transport,
            &ArchiveLimits::default(),
            &Cancellation::default(),
        ))
        .unwrap();
        assert!(outcome.failures.is_empty(), "{:?}", outcome.failures);
        assert_eq!(outcome.archived, 3);
        assert_eq!(outcome.originals.len(), 3);
        for (position, (_, encoded, declared)) in cases.iter().enumerate() {
            let body = fixture(encoded);
            assert_eq!(
                outcome.originals[position],
                NetworkImageOriginal {
                    image_index: position as u16,
                    resource_id: resource_id_for(&outcome.bundle, &derived_png(*declared, &body)),
                    format: *declared,
                    bytes: body,
                }
            );
        }
    }

    #[test]
    fn duplicate_pngs_keep_one_resource_and_both_positions() {
        let body = fixture(PNG_1X1);
        let transport = MockTransport::new();
        transport.push(
            "https://a.example/1",
            typed_fetch(200, "image/png", body.clone()),
        );
        transport.push(
            "https://b.example/2",
            typed_fetch(200, "image/png", body.clone()),
        );
        let outcome = futures_lite_block_on(archive_external_images(
            &html_bundle("<img src='https://a.example/1'><img src='https://b.example/2'>"),
            &transport,
            &ArchiveLimits::default(),
            &Cancellation::default(),
        ))
        .unwrap();
        assert_eq!(outcome.archived, 2);
        assert_eq!(outcome.deduplicated, 1);
        assert_eq!(outcome.bundle.resources.len(), 1);
        assert_eq!(outcome.originals.len(), 2);
        assert_eq!(outcome.originals[0].image_index, 0);
        assert_eq!(outcome.originals[1].image_index, 1);
        assert_eq!(
            outcome.originals[0].resource_id,
            outcome.originals[1].resource_id
        );
        assert_eq!(outcome.originals[0].bytes, body);
        assert_eq!(outcome.originals[1].bytes, body);
        assert_eq!(outcome.originals[0].format, NetworkImageFormat::Png);
        assert_eq!(outcome.originals[1].format, NetworkImageFormat::Png);
    }

    #[test]
    fn distinct_bodies_with_the_same_derived_png_keep_both_originals() {
        let base = fixture(PNG_1X1);
        let tagged = png_with_ancillary_text(&base, b"note", b"tail");
        assert_ne!(base, tagged);
        assert_eq!(
            derived_png(NetworkImageFormat::Png, &base),
            derived_png(NetworkImageFormat::Png, &tagged)
        );
        let transport = MockTransport::new();
        transport.push(
            "https://a.example/1",
            typed_fetch(200, "image/png", base.clone()),
        );
        transport.push(
            "https://b.example/2",
            typed_fetch(200, "image/png", tagged.clone()),
        );
        let outcome = futures_lite_block_on(archive_external_images(
            &html_bundle("<img src='https://a.example/1'><img src='https://b.example/2'>"),
            &transport,
            &ArchiveLimits::default(),
            &Cancellation::default(),
        ))
        .unwrap();
        assert_eq!(outcome.archived, 2);
        assert_eq!(outcome.deduplicated, 1);
        assert_eq!(outcome.bundle.resources.len(), 1);
        assert_eq!(outcome.originals.len(), 2);
        assert_eq!(outcome.originals[0].bytes, base);
        assert_eq!(outcome.originals[1].bytes, tagged);
        assert_eq!(
            outcome.originals[0].resource_id,
            outcome.originals[1].resource_id
        );
    }

    #[test]
    fn failed_positions_do_not_produce_originals() {
        let good = fixture(PNG_1X1);
        let transport = MockTransport::new();
        transport.push(
            "https://a.example/1",
            typed_fetch(200, "image/png", b"not an image".to_vec()),
        );
        transport.push(
            "https://b.example/2",
            typed_fetch(200, "image/png", good.clone()),
        );
        transport.push(
            "https://c.example/3",
            typed_fetch(200, "image/png", fixture(APNG_TWO_FRAME)),
        );
        let outcome = futures_lite_block_on(archive_external_images(
            &html_bundle(
                "<img src='https://a.example/1'><img src='https://b.example/2'><img src='https://c.example/3'>",
            ),
            &transport,
            &ArchiveLimits::default(),
            &Cancellation::default(),
        ))
        .unwrap();
        assert_eq!(outcome.archived, 1);
        assert_eq!(
            outcome.failures,
            vec![(0, ArchiveError::Decode), (2, ArchiveError::Animated)]
        );
        assert_eq!(outcome.originals.len(), 1);
        assert_eq!(
            outcome.originals[0],
            NetworkImageOriginal {
                image_index: 1,
                resource_id: resource_id_for(
                    &outcome.bundle,
                    &derived_png(NetworkImageFormat::Png, &good)
                ),
                format: NetworkImageFormat::Png,
                bytes: good,
            }
        );
    }

    #[test]
    fn cancelled_archive_discards_successful_originals() {
        struct CancelAfterFirst {
            cancellation: Cancellation,
            calls: Mutex<usize>,
            body: Vec<u8>,
        }
        impl ResourceTransport for CancelAfterFirst {
            fn fetch_once<'a>(
                &'a self,
                _: &'a Url,
                _: &'a FetchLimits,
                _: &'a Cancellation,
            ) -> FetchFuture<'a> {
                Box::pin(async move {
                    let mut calls = self.calls.lock().unwrap();
                    *calls += 1;
                    if *calls > 1 {
                        self.cancellation.cancel();
                        return Err(ArchiveError::Cancelled);
                    }
                    Ok(FetchResponse {
                        status: 200,
                        location: None,
                        content_type: Some("image/png".into()),
                        bytes: self.body.clone(),
                    })
                })
            }
        }
        let cancellation = Cancellation::default();
        let transport = CancelAfterFirst {
            cancellation: cancellation.clone(),
            calls: Mutex::new(0),
            body: fixture(PNG_1X1),
        };
        let error = futures_lite_block_on(archive_external_images(
            &html_bundle("<img src='https://a.example/1'><img src='https://b.example/2'>"),
            &transport,
            &ArchiveLimits::default(),
            &cancellation,
        ))
        .unwrap_err();
        assert_eq!(error, ArchiveError::Cancelled);
    }

    #[test]
    fn derived_budget_failures_keep_only_successful_originals() {
        let noise = fixture(PNG_NOISE_16X16);
        let derived_bytes = derived_png(NetworkImageFormat::Png, &noise).len();
        let transport = MockTransport::new();
        transport.push(
            "https://a.example/1",
            typed_fetch(200, "image/png", noise.clone()),
        );
        transport.push(
            "https://a.example/2",
            typed_fetch(200, "image/png", noise.clone()),
        );
        let limits = ArchiveLimits {
            max_derived_bytes_per_resource: derived_bytes + 16,
            max_total_derived_bytes: derived_bytes + derived_bytes / 2,
            ..ArchiveLimits::default()
        };
        let outcome = futures_lite_block_on(archive_external_images(
            &html_bundle("<img src='https://a.example/1'><img src='https://a.example/2'>"),
            &transport,
            &limits,
            &Cancellation::default(),
        ))
        .unwrap();
        assert_eq!(outcome.archived, 1);
        assert_eq!(outcome.failures, vec![(1, ArchiveError::DerivedBudget)]);
        assert_eq!(outcome.originals.len(), 1);
        assert_eq!(outcome.originals[0].image_index, 0);
        assert_eq!(outcome.originals[0].bytes, noise);
    }

    #[test]
    fn exhausted_wire_budget_returns_no_originals() {
        let transport = MockTransport::new();
        for index in 1..=3 {
            transport.push(
                &format!("https://a.example/{index}.png"),
                fetch(200, vec![0; 10]),
            );
        }
        let limits = ArchiveLimits {
            max_total_bytes: 16,
            ..ArchiveLimits::default()
        };
        let error = futures_lite_block_on(archive_external_images(
            &html_bundle("<img src='https://a.example/1.png'><img src='https://a.example/2.png'><img src='https://a.example/3.png'>"),
            &transport,
            &limits,
            &Cancellation::default(),
        ))
        .unwrap_err();
        assert_eq!(error, ArchiveError::TotalBudget);
    }

    fn futures_lite_block_on<F: Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(future)
    }
}
