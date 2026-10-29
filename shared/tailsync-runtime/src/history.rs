use std::path::PathBuf;
use std::sync::Arc;
use tailsync_core::db::{
    FavoriteMutation, HistoryCollection, HistoryDB, HistoryEntry, HistoryQuery, HistoryQueryPage,
    HistoryReadError, MigrationDiagnostics, PreviewError, PreviewMetadata, PreviewPayload,
};
use tokio::sync::Mutex;

use crate::execution::{run_db_named, run_read, DbExecutionError};

/// A transport-neutral paged history request.
#[derive(Debug, Clone)]
pub struct HistoryPageRequest<'a> {
    pub collection: HistoryCollection,
    pub keyword: Option<&'a str>,
    pub category: Option<&'a str>,
    pub start_time: Option<&'a str>,
    pub end_time: Option<&'a str>,
    pub limit: usize,
    pub offset: usize,
}

impl<'a> HistoryPageRequest<'a> {
    // Keep the transport-facing constructor explicit: these names mirror the
    // stable JSON/Tauri request fields and avoid a second conversion struct.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        collection: HistoryCollection,
        keyword: Option<&'a str>,
        category: Option<&'a str>,
        start_time: Option<&'a str>,
        end_time: Option<&'a str>,
        limit: Option<usize>,
        offset: Option<usize>,
        default_limit: usize,
    ) -> Self {
        Self {
            collection,
            keyword,
            category,
            start_time,
            end_time,
            limit: limit.unwrap_or(default_limit),
            offset: offset.unwrap_or(0),
        }
    }

    fn into_core(self) -> HistoryQuery<'a> {
        HistoryQuery {
            collection: self.collection,
            keyword: self.keyword,
            category: self.category,
            start_time: self.start_time,
            end_time: self.end_time,
            limit: self.limit,
            offset: self.offset,
        }
    }
}

/// Owned form used when an async transport hands work to a blocking worker.
/// Keeping request data owned prevents borrowed IPC strings from crossing the
/// worker boundary and makes cancellation safe before the task starts.
#[derive(Debug, Clone)]
pub struct HistoryPageRequestOwned {
    pub collection: HistoryCollection,
    pub keyword: Option<String>,
    pub category: Option<String>,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub limit: usize,
    pub offset: usize,
}

impl HistoryPageRequestOwned {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        collection: HistoryCollection,
        keyword: Option<String>,
        category: Option<String>,
        start_time: Option<String>,
        end_time: Option<String>,
        limit: Option<usize>,
        offset: Option<usize>,
        default_limit: usize,
    ) -> Self {
        Self {
            collection,
            keyword,
            category,
            start_time,
            end_time,
            limit: limit.unwrap_or(default_limit),
            offset: offset.unwrap_or(0),
        }
    }

    fn as_request(&self) -> HistoryPageRequest<'_> {
        HistoryPageRequest {
            collection: self.collection,
            keyword: self.keyword.as_deref(),
            category: self.category.as_deref(),
            start_time: self.start_time.as_deref(),
            end_time: self.end_time.as_deref(),
            limit: self.limit,
            offset: self.offset,
        }
    }
}

#[derive(Debug)]
pub enum PreparedHistoryRestore {
    Ready(Box<tailsync_core::composite::restore::RestorePlan>),
    Unsupported { code: &'static str },
}

enum PreparedRestore {
    Portable(tailsync_core::composite::PortableBundle),
    Native(tailsync_core::protocol::native_clipboard::NativeSnapshot),
    Unsupported { code: &'static str },
}

impl HistoryRestorePayload {
    /// Restore this entry, returning a **typed** outcome the UI can present honestly.
    ///
    /// R5-03: the guards are evaluated server-side, before any format branch, so no
    /// entry point (HTTP route, Tauri command or shortcut) can bypass them by asking
    /// for a format the menu happens to hide. `Unsupported` carries a stable code, not
    /// Rust prose, so both UIs localise the same failure.
    pub fn restore_composite(
        &self,
        engine: &mut tailsync_core::sync::SyncEngine,
        format: Option<&str>,
    ) -> Result<RestoreOutcome, String> {
        let supported = self
            .native
            .as_ref()
            .is_some_and(|native| engine.supports_native_snapshot(native));
        match self.prepare_restore_plan(supported, engine.restore_target(), format)? {
            PreparedHistoryRestore::Unsupported { code } => {
                Ok(RestoreOutcome::Unsupported { code })
            }
            PreparedHistoryRestore::Ready(plan) => {
                let missing_images = plan.missing_images();
                engine
                    .restore_plan(&plan)
                    .map(|outcome| RestoreOutcome::from_prepared(outcome, missing_images))
            }
        }
    }

    /// Production restore: conversion runs off the async worker and native I/O
    /// never holds the engine guard. Every transport shares this preparation.
    pub async fn restore_composite_async(
        self,
        engine: std::sync::Arc<tokio::sync::Mutex<tailsync_core::sync::SyncEngine>>,
        format: Option<String>,
    ) -> Result<RestoreOutcome, String> {
        let (supported, port, target, generation, revision) = {
            let mut engine = engine.lock().await;
            // User restore supersedes assemblies immediately, before preparation.
            let generation = engine.supersede_file_clipboard();
            (
                self.native
                    .as_ref()
                    .is_some_and(|native| engine.supports_native_snapshot(native)),
                engine.clipboard_port(),
                engine.restore_target(),
                generation,
                engine.clipboard_revision(),
            )
        };
        let wire = self.data.as_ref().map_or(0, |data| data.len() as u64);
        let native_bytes = self.native.as_ref().map_or(0, |native| {
            native
                .formats
                .iter()
                .map(|f| f.bytes.len() as u64)
                .sum::<u64>()
        });
        let memory: tailsync_core::sync::AdmissionHold = std::sync::Arc::new(
            tailsync_core::composite::budgets::global()
                .reserve_pipeline(
                    wire.saturating_mul(4)
                        .saturating_add(native_bytes.saturating_mul(2))
                        .saturating_add(4 * 1024 * 1024),
                )
                .map_err(|e| e.to_string())?,
        );
        let held = memory.clone();
        let (prepared, workspace) = tokio::task::spawn_blocking(move || {
            let _held = held;
            let decoded = if self.native_only {
                None
            } else {
                self.data
                    .as_deref()
                    .and_then(|data| tailsync_core::composite::PortableBundle::decode(data).ok())
            };
            let mut rgba_bytes = decoded
                .as_ref()
                .and_then(|b| b.validate_with_metrics().ok())
                .map_or(0, |m| m.rgba_bytes as u64);
            let inline_png = |data: &[u8]| {
                data.windows(22)
                    .any(|s| s.eq_ignore_ascii_case(b"data:image/png;base64,"))
            };
            if self.data.as_deref().is_some_and(inline_png)
                || self.native.as_ref().is_some_and(|s| {
                    s.formats.iter().any(|f| {
                        matches!(f.name.as_str(), "public.html" | "HTML Format")
                            && inline_png(&f.bytes)
                    })
                })
            {
                rgba_bytes = rgba_bytes.max(tailsync_core::composite::RGBA_LIMIT as u64);
            }
            let workspace: tailsync_core::sync::AdmissionHold = std::sync::Arc::new(
                tailsync_core::composite::budgets::global()
                    .reserve_pipeline(
                        rgba_bytes
                            .saturating_mul(3)
                            .saturating_add(if rgba_bytes > 0 { 32 * 1024 * 1024 } else { 0 }),
                    )
                    .map_err(|e| e.to_string())?,
            );
            Ok::<_, String>((
                self.prepare_restore_plan_decoded(
                    supported,
                    target,
                    format.as_deref(),
                    decoded.as_ref(),
                )?,
                workspace,
            ))
        })
        .await
        .map_err(|_| "Restore preparation worker failed")??;
        let plan = match prepared {
            PreparedHistoryRestore::Unsupported { code } => {
                return Ok(RestoreOutcome::Unsupported { code })
            }
            PreparedHistoryRestore::Ready(plan) => *plan,
        };
        let port = port.ok_or("Clipboard port unavailable")?;
        let preparation_cancelled = std::sync::Arc::new(false.into());
        let plan = port
            .prepare_restore_plan_admitted(
                plan,
                preparation_cancelled,
                vec![memory.clone(), workspace.clone()],
            )
            .await?;
        let missing_images = plan.missing_images();
        let publication: tailsync_core::sync::AdmissionHold =
            std::sync::Arc::new(tailsync_core::sync::publication_permit().await);
        // A later local copy or explicit restore supersedes a plan prepared
        // earlier. The helper also checks the captured OS revision before clear.
        if engine.lock().await.clipboard_generation() != generation {
            return Ok(RestoreOutcome::Superseded);
        }
        let outcome = port
            .publish_restore_plan_admitted(
                plan,
                revision,
                std::sync::Arc::new(false.into()),
                vec![publication, memory, workspace],
            )
            .await?;
        Ok(RestoreOutcome::from_prepared(
            engine.lock().await.finish_composite_publish(outcome),
            missing_images,
        ))
    }

    /// Prepare an immutable versioned output set, without any system effect.
    pub fn prepare_restore_plan(
        &self,
        native_supported: bool,
        target: tailsync_core::composite::restore::RestoreTarget,
        format: Option<&str>,
    ) -> Result<PreparedHistoryRestore, String> {
        self.prepare_restore_plan_decoded(native_supported, target, format, None)
    }
    fn prepare_restore_plan_decoded(
        &self,
        native_supported: bool,
        target: tailsync_core::composite::restore::RestoreTarget,
        format: Option<&str>,
        decoded: Option<&tailsync_core::composite::PortableBundle>,
    ) -> Result<PreparedHistoryRestore, String> {
        use tailsync_core::composite::restore::{RestoreInput, RestorePlan};
        let selected =
            self.prepare_composite_for_target(native_supported, target, format, decoded)?;
        let input = RestoreInput {
            source_platform: self.native_archive_platform,
            requested_format: format.map(str::to_owned),
        };
        if format.is_none()
            && !self.native_only
            && matches!(
                selected,
                PreparedRestore::Native(_) | PreparedRestore::Portable(_)
            )
        {
            let bundle = match decoded {
                Some(bundle) => std::borrow::Cow::Borrowed(bundle),
                None => std::borrow::Cow::Owned(
                    tailsync_core::composite::PortableBundle::decode(
                        self.data
                            .as_deref()
                            .ok_or("Composite data is unavailable")?,
                    )
                    .map_err(|error| error.to_string())?,
                ),
            };
            if tailsync_core::composite::restore::prefers_document_compatibility(&bundle)
                .map_err(|error| error.to_string())?
            {
                return RestorePlan::portable_default(&bundle, input, target)
                    .map(|plan| PreparedHistoryRestore::Ready(Box::new(plan)))
                    .map_err(|error| error.to_string());
            }
        }
        if format.is_none()
            && !self.native_only
            && matches!(
                selected,
                PreparedRestore::Unsupported {
                    code: "resources_incomplete"
                }
            )
        {
            let bundle = match decoded {
                Some(bundle) => std::borrow::Cow::Borrowed(bundle),
                None => std::borrow::Cow::Owned(
                    tailsync_core::composite::PortableBundle::decode(
                        self.data
                            .as_deref()
                            .ok_or("Composite data is unavailable")?,
                    )
                    .map_err(|error| error.to_string())?,
                ),
            };
            if let Ok(plan) = RestorePlan::portable_default(&bundle, input.clone(), target) {
                return Ok(PreparedHistoryRestore::Ready(Box::new(plan)));
            }
        }
        Ok(match selected {
            PreparedRestore::Unsupported { code } => PreparedHistoryRestore::Unsupported { code },
            PreparedRestore::Native(s) => PreparedHistoryRestore::Ready(Box::new(
                RestorePlan::originals(s, input, target).map_err(|e| e.to_string())?,
            )),
            PreparedRestore::Portable(b) => PreparedHistoryRestore::Ready(Box::new(
                RestorePlan::compatibility(b, input, target).map_err(|e| e.to_string())?,
            )),
        })
    }
    #[cfg(test)]
    fn prepare_composite(
        &self,
        native_supported: bool,
        format: Option<&str>,
    ) -> Result<PreparedRestore, String> {
        self.prepare_composite_for_target(
            native_supported,
            tailsync_core::composite::restore::RestoreTarget::Adapter,
            format,
            None,
        )
    }
    fn prepare_composite_for_target(
        &self,
        native_supported: bool,
        target: tailsync_core::composite::restore::RestoreTarget,
        format: Option<&str>,
        decoded: Option<&tailsync_core::composite::PortableBundle>,
    ) -> Result<PreparedRestore, String> {
        let data = self
            .data
            .as_deref()
            .ok_or("Composite data is unavailable")?;

        let foreign = self
            .native_archive_platform
            .zip(target.archive_platform())
            .is_some_and(|(a, b)| a != b);
        let native_supported = native_supported && !foreign;
        if format.is_none() {
            match self.native_archive_complete {
                Some(false) => {
                    return Ok(PreparedRestore::Unsupported {
                        code: "originals_incomplete",
                    })
                }
                Some(true)
                    if !native_supported
                        && self
                            .native_archive_platform
                            .zip(target.archive_platform())
                            .is_none_or(|(a, b)| a == b) =>
                {
                    return Ok(PreparedRestore::Unsupported {
                        code: "native_only",
                    })
                }
                _ => {}
            }
        }

        let needs_materialization = format.is_none()
            && self
                .native
                .as_ref()
                .map(tailsync_core::composite::restore::native_needs_materialization)
                .transpose()
                .map_err(|e| e.to_string())?
                .unwrap_or(false);
        if self.native_only && needs_materialization {
            return Ok(PreparedRestore::Unsupported {
                code: "resources_incomplete",
            });
        }

        // A native-only shape is a property of the *entry*, not of the requested
        // format, so it is validated first: `format=text` must not become a way to
        // export a truncated summary and call it a restore.
        if self.native_only {
            if format.is_some() {
                return Ok(PreparedRestore::Unsupported {
                    code: "native_only",
                });
            }
            if let Some(native) = &self.native {
                if native_supported {
                    native.validate().map_err(|e| e.to_string())?;
                    return Ok(PreparedRestore::Native(native.clone()));
                }
            }
            return Ok(PreparedRestore::Unsupported {
                code: "native_only",
            });
        }

        if format.is_none() {
            if let Some(native) = &self.native {
                if native_supported && !needs_materialization {
                    native.validate().map_err(|e| e.to_string())?;
                    return Ok(PreparedRestore::Native(native.clone()));
                }
            }
        }
        use tailsync_core::composite::{Completeness, Mime, PortableBundle, Representation};
        let bundle = match decoded {
            Some(bundle) => std::borrow::Cow::Borrowed(bundle),
            None => {
                std::borrow::Cow::Owned(PortableBundle::decode(data).map_err(|e| e.to_string())?)
            }
        };
        if let Some(format) = format {
            let mime = match format {
                "text" => Mime::Text,
                "html" => Mime::Html,
                "rtf" => Mime::Rtf,
                "png" => Mime::Png,
                _ => {
                    return Ok(PreparedRestore::Unsupported {
                        code: "unsupported_format",
                    })
                }
            };
            let inventory = tailsync_core::composite::restore::RestoreFormatInventory {
                native_only: self.native_only,
                completeness: bundle.completeness,
                standalone_png: bundle.representation(Mime::Png).is_some(),
                resource_count: bundle.resources.len(),
            };
            // Availability precedes completeness so a missing requested format
            // retains its existing format_unavailable reason.
            if bundle.representation(mime).is_some() || mime == Mime::Png {
                if let Some(code) = inventory.withheld_reason(mime) {
                    return Ok(PreparedRestore::Unsupported { code });
                }
            }
            let bytes = bundle.representation(mime).or_else(|| {
                (mime == Mime::Png && bundle.resources.len() == 1)
                    .then(|| bundle.resources[0].png.as_slice())
            });
            let Some(bytes) = bytes else {
                return Ok(PreparedRestore::Unsupported {
                    code: "format_unavailable",
                });
            };
            let chosen = if mime == Mime::Html {
                let index = bundle
                    .representations
                    .iter()
                    .position(|r| r.mime == Mime::Html)
                    .ok_or("HTML is unavailable")?;
                let mut bindings = bundle
                    .bindings
                    .iter()
                    .filter(|b| b.representation as usize == index)
                    .cloned()
                    .collect::<Vec<_>>();
                for b in &mut bindings {
                    b.representation = 0;
                }
                PortableBundle {
                    representations: vec![Representation {
                        mime,
                        bytes: bytes.to_vec(),
                    }],
                    resources: bundle.resources.clone(),
                    bindings,
                    completeness: bundle.completeness,
                }
            } else {
                PortableBundle {
                    representations: vec![Representation {
                        mime,
                        bytes: bytes.to_vec(),
                    }],
                    resources: vec![],
                    bindings: vec![],
                    completeness: Completeness::Complete,
                }
            };
            if !tailsync_core::composite::restore::dependencies_complete(&chosen)
                .map_err(|e| e.to_string())?
            {
                return Ok(PreparedRestore::Unsupported {
                    code: "resources_incomplete",
                });
            }
            tailsync_core::composite::validate_for_publish(&chosen).map_err(|e| e.to_string())?;
            return Ok(PreparedRestore::Portable(chosen));
        }

        if !tailsync_core::composite::restore::dependencies_complete(&bundle)
            .map_err(|e| e.to_string())?
        {
            return Ok(PreparedRestore::Unsupported {
                code: "resources_incomplete",
            });
        }
        tailsync_core::composite::validate_for_publish(&bundle).map_err(|e| e.to_string())?;
        Ok(PreparedRestore::Portable(bundle.into_owned()))
    }

    /// The restore actions this entry actually supports.
    ///
    /// Full-payload eligibility oracle used by regression tests. Production menus
    /// use CompositeInfo from the bounded DB inventory; shared format rules and
    /// cross-layer regressions keep those menus consistent with preparation.
    pub fn restore_capabilities(&self) -> Result<RestoreCapabilities, String> {
        use tailsync_core::composite::PortableBundle;
        let data = self
            .data
            .as_deref()
            .ok_or("Composite data is unavailable")?;

        let bundle = PortableBundle::decode(data).map_err(|e| e.to_string())?;
        // A native-only entry can only be replayed on its original platform, so it
        // offers no explicit format at all rather than a truncated summary.
        if self.native_only {
            let selected = self.prepare_composite_for_target(
                self.native.is_some(),
                tailsync_core::composite::restore::RestoreTarget::Adapter,
                None,
                Some(&bundle),
            )?;
            let code = match selected {
                PreparedRestore::Unsupported { code } => Some(code),
                _ => None,
            };
            return Ok(RestoreCapabilities {
                whole_group: code.is_none(),
                whole_group_code: code,
                formats: Vec::new(),
                withheld: present_formats(&bundle)
                    .into_iter()
                    .map(|format| (format, "native_only"))
                    .collect(),
            });
        }

        let mut formats = Vec::new();
        let mut withheld = Vec::new();
        for format in present_formats(&bundle) {
            match self.prepare_composite_for_target(
                false,
                tailsync_core::composite::restore::RestoreTarget::Adapter,
                Some(format),
                Some(&bundle),
            )? {
                PreparedRestore::Unsupported { code } => withheld.push((format, code)),
                _ => formats.push(format),
            }
        }
        let whole = self.prepare_composite_for_target(
            self.native.is_some(),
            tailsync_core::composite::restore::RestoreTarget::Adapter,
            None,
            Some(&bundle),
        )?;
        let code = match whole {
            PreparedRestore::Unsupported { code } => Some(code),
            _ => None,
        };
        Ok(RestoreCapabilities {
            whole_group: code.is_none(),
            whole_group_code: code,
            formats,
            withheld,
        })
    }
}

/// The single implementation of history operations used by every transport.
///
/// The synchronous methods are the small Core-facing implementation seam used
/// by tests. The async methods below own queueing, database locking, error
/// mapping, and post-commit notification so platform adapters only map DTOs.
pub struct HistoryOperations<'a> {
    db: &'a mut HistoryDB,
}

/// The honest result of a user-initiated restore (R5-03).
///
/// `Unsupported` carries a stable code so both UIs localise the same failure instead
/// of surfacing Rust prose; the other variants mirror the native outcomes so a
/// partial or unknown publish can never be presented as a clean success.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreOutcome {
    /// The clipboard now holds the requested content.
    Published,
    PublishedAvailable {
        missing_images: u16,
    },
    /// Part of the content landed.
    Partial {
        detail: String,
    },
    /// A side effect may have happened and the receipt was lost.
    Unknown {
        detail: String,
    },
    /// A newer local copy won the race; nothing was written.
    Superseded,
    /// The request cannot be served for this entry on this platform. `code` is a
    /// stable identifier (`native_only`, `no_single_image`, `format_unavailable`,
    /// `unsupported_format`), never prose.
    Unsupported {
        code: &'static str,
    },
}

impl RestoreOutcome {
    pub fn error_envelope(&self) -> crate::contracts::StableErrorEnvelope {
        use crate::contracts::{StableErrorCode, StableErrorEnvelope};
        let code = match self.code() {
            "publish_partial" => StableErrorCode::PublishPartial,
            "publish_unknown" => StableErrorCode::PublishUnknown,
            "publish_superseded" => StableErrorCode::PublishSuperseded,
            "native_only" => StableErrorCode::NativeOnly,
            "originals_incomplete" => StableErrorCode::OriginalsIncomplete,
            "resources_incomplete" => StableErrorCode::ResourcesIncomplete,
            "no_single_image" => StableErrorCode::NoSingleImage,
            "format_unavailable" => StableErrorCode::FormatUnavailable,
            "unsupported_format" => StableErrorCode::UnsupportedFormat,
            _ => StableErrorCode::InternalError,
        };
        StableErrorEnvelope::new(code)
    }

    pub fn is_published(&self) -> bool {
        matches!(self, Self::Published | Self::PublishedAvailable { .. })
    }

    pub fn report(&self) -> Option<HistoryRestoreReport> {
        match self {
            Self::Published => Some(HistoryRestoreReport::default()),
            Self::PublishedAvailable { missing_images } => Some(HistoryRestoreReport {
                complete: false,
                missing_images: *missing_images,
            }),
            _ => None,
        }
    }

    fn from_prepared(
        outcome: tailsync_core::composite::NativePublish,
        missing_images: u16,
    ) -> Self {
        let outcome = Self::from(outcome);
        if outcome == Self::Published && missing_images > 0 {
            Self::PublishedAvailable { missing_images }
        } else {
            outcome
        }
    }

    pub fn unsupported_code(&self) -> Option<&'static str> {
        match self {
            Self::Unsupported { code } => Some(code),
            _ => None,
        }
    }

    /// Stable code for the UI to localise. Never Rust prose, so both platforms show
    /// the same failure with their own wording (R5-04).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Published => "published",
            Self::PublishedAvailable { .. } => "published_available",
            Self::Partial { .. } => "publish_partial",
            Self::Unknown { .. } => "publish_unknown",
            Self::Superseded => "publish_superseded",
            Self::Unsupported { code } => code,
        }
    }
}

#[derive(
    Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
pub struct HistoryRestoreReport {
    pub complete: bool,
    #[schemars(range(max = 4096))]
    pub missing_images: u16,
}

impl Default for HistoryRestoreReport {
    fn default() -> Self {
        Self {
            complete: true,
            missing_images: 0,
        }
    }
}

impl From<tailsync_core::composite::NativePublish> for RestoreOutcome {
    fn from(outcome: tailsync_core::composite::NativePublish) -> Self {
        use tailsync_core::composite::NativePublish;
        match outcome {
            NativePublish::Published(_) => Self::Published,
            NativePublish::Partial(_) => Self::Partial {
                detail: "the platform published only part of the requested set".into(),
            },
            NativePublish::Unknown { detail } => Self::Unknown { detail },
            NativePublish::Superseded(_) => Self::Superseded,
            NativePublish::NoEffect { detail } => Self::Unknown { detail },
        }
    }
}

/// The formats a bundle actually carries, in a stable order.
fn present_formats(bundle: &tailsync_core::composite::PortableBundle) -> Vec<&'static str> {
    use tailsync_core::composite::Mime;
    let mut formats = Vec::new();
    for mime in [Mime::Text, Mime::Html, Mime::Rtf, Mime::Png] {
        if bundle.representation(mime).is_some() {
            formats.push(match mime {
                Mime::Text => "text",
                Mime::Html => "html",
                Mime::Rtf => "rtf",
                Mime::Png => "png",
            });
        }
    }
    if !bundle.resources.is_empty() && !formats.contains(&"png") {
        formats.push("png");
    }
    formats
}

/// Which restore actions the server will actually accept for one entry (R5-04).
///
/// This full-payload oracle complements the production CompositeInfo inventory.
/// Presence alone does not establish restorability; preparation still performs
/// resource closure, pixel decoding, target admission and revision checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreCapabilities {
    /// Restore the whole group as captured.
    pub whole_group: bool,
    /// Why the whole group is unavailable, when it is. A stable code.
    pub whole_group_code: Option<&'static str>,
    /// Explicit single-format actions the server will accept, in a stable order.
    pub formats: Vec<&'static str>,
    /// Formats that are present but deliberately not offered, with the reason code the
    /// UI localises instead of inventing its own explanation.
    pub withheld: Vec<(&'static str, &'static str)>,
}

impl RestoreCapabilities {
    pub fn offers(&self, format: &str) -> bool {
        self.formats.contains(&format)
    }
}

/// Decrypted/materialized data needed by a transport to restore one history
/// entry.  The runtime decides whether a file is represented by a path or by
/// legacy inline bytes; clipboard adapters decide how to publish it.
#[derive(Debug)]
pub struct HistoryRestorePayload {
    pub entry_type: String,
    pub file_path: Option<PathBuf>,
    pub file_name: Option<String>,
    pub data: Option<Vec<u8>>,
    pub identity: Option<tailsync_core::composite::identity::HistoryIdentity>,
    pub native_only: bool,
    /// None denotes legacy storage; CNA1 records require a complete original set
    /// and native support for default restoration, even when a projection exists.
    pub native_archive_complete: Option<bool>,
    pub native_archive_platform: Option<u8>,
    pub native: Option<tailsync_core::protocol::native_clipboard::NativeSnapshot>,
}

impl<'a> HistoryOperations<'a> {
    pub fn new(db: &'a mut HistoryDB) -> Self {
        Self { db }
    }

    pub fn restore_payload(&mut self, id: i64) -> Result<HistoryRestorePayload, String> {
        let entry_type = self.db.get_type(id).map_err(|error| error.to_string())?;
        let file_path = if entry_type == "file" {
            self.db
                .get_file_path(id)
                .map_err(|error| error.to_string())?
        } else {
            None
        };
        let file_name = if entry_type == "file" {
            Some(
                self.db
                    .get_description(id)
                    .map_err(|error| error.to_string())?,
            )
        } else {
            None
        };
        let data = if file_path.is_none() {
            Some(self.db.get_data(id).map_err(|error| error.to_string())?)
        } else {
            None
        };

        let archive = if entry_type == "composite" {
            self.db
                .get_composite_archive(id)
                .map_err(|e| e.to_string())?
        } else {
            None
        };
        let native_archive_complete = archive.as_ref().map(|archive| archive.originals_complete());
        let native_archive_platform = archive.as_ref().map(|archive| archive.platform);
        let native = if let Some(archive) = archive {
            Some(archive.publication().map_err(|e| e.to_string())?)
        } else if entry_type == "composite" {
            self.db
                .get_composite_native(id)
                .map_err(|e| e.to_string())?
        } else {
            None
        };
        let identity = if entry_type == "composite" {
            self.db.composite_identity(id).map_err(|e| e.to_string())?
        } else {
            None
        };
        Ok(HistoryRestorePayload {
            native_archive_complete,
            native_archive_platform,
            identity,
            native_only: entry_type == "composite"
                && self
                    .db
                    .composite_native_only(id)
                    .map_err(|e| e.to_string())?,
            native,
            entry_type,
            file_path,
            file_name,
            data,
        })
    }

    pub fn page(&mut self, request: HistoryPageRequest<'_>) -> Result<HistoryQueryPage, String> {
        self.db
            .get_page_in_collection(request.into_core())
            .map_err(|error| error.to_string())
    }

    pub fn entries(
        &mut self,
        request: HistoryPageRequest<'_>,
    ) -> Result<Vec<HistoryEntry>, String> {
        Ok(self.page(request)?.entries)
    }

    pub fn delete(&mut self, id: i64) -> Result<(), String> {
        self.db.delete(id).map_err(|error| error.to_string())
    }

    pub fn set_favorite(&mut self, id: i64, favorite: bool) -> Result<FavoriteMutation, String> {
        self.db
            .set_favorite(id, favorite)
            .map_err(|error| error.to_string())
    }

    pub fn delete_favorite(&mut self, id: i64) -> Result<FavoriteMutation, String> {
        self.db
            .delete_favorite(id)
            .map_err(|error| error.to_string())
    }

    pub fn clear(&mut self) -> Result<(), String> {
        self.db.clear_all().map_err(|error| error.to_string())
    }

    pub async fn page_async(
        database: Arc<Mutex<HistoryDB>>,
        request: HistoryPageRequestOwned,
    ) -> Result<HistoryQueryPage, String> {
        Self::page_read(database, request)
            .await
            .map_err(|error| error.to_string())
    }

    pub async fn page_read(
        database: Arc<Mutex<HistoryDB>>,
        request: HistoryPageRequestOwned,
    ) -> Result<HistoryQueryPage, HistoryReadError> {
        run_read("history.page", database, move |context| {
            // A changed revision discards the entire attempt. Bound retries
            // so a busy receiver cannot keep a search alive indefinitely.
            for attempt in 0..3 {
                let result = (|| {
                    let has_keyword = request.keyword.as_ref().is_some_and(|s| !s.trim().is_empty());
                    let (revision, total) = context.with_database(|database| {
                        let revision = database.read_revision()?;
                        let total = if has_keyword { None } else {
                            Some(database.count_in_collection(request.collection, None, request.category.as_deref(), request.start_time.as_deref(), request.end_time.as_deref())
                                .map_err(|error| HistoryReadError::Operation(error.to_string()))?)
                        };
                        database.validate_read_revision(&revision)?;
                        Ok((revision, total))
                    })?;
                    let mut entries = Vec::new();
                    let mut cursor = None;
                    let mut matched = 0usize;
                    let mut has_more = false;
                    loop {
                        context.cancellation.check()?;
                        let chunk = context.with_database(|database| database.prepare_read_chunk(request.as_request().into_core(), cursor.as_ref(), &revision, 128))?;
                        let exhausted = chunk.exhausted;
                        cursor = chunk.cursor.clone();
                        let started = std::time::Instant::now();
                        for entry in chunk.matching_entries(request.keyword.as_deref(), &context.cancellation)? {
                            if has_keyword && matched < request.offset {
                                matched += 1;
                                continue;
                            }
                            if entries.len() == request.limit { has_more = true; break; }
                            entries.push(entry);
                        }
                        tracing::debug!(target: "tailsync.runtime.execution", operation = "history.page", decrypt_match_ms = started.elapsed().as_secs_f64() * 1000.0, "history matching timing");
                        if has_more || exhausted || cursor.is_none() { break; }
                    }
                    context.with_database(|database| {
                        database.validate_read_revision(&revision)?;
                        for entry in &mut entries {
                            database.attach_display_info(entry).map_err(|error| HistoryReadError::Operation(error.to_string()))?;
                        }
                        Ok(())
                    })?;
                    Ok(HistoryQueryPage { entries, total, has_more, display_index_pending: None })
                })();
                match result {
                    Err(HistoryReadError::Changed) if attempt < 2 => continue,
                    result => return result,
                }
            }
            unreachable!("bounded query retries always return")
        })
        .await
    }

    pub async fn entries_async(
        database: Arc<Mutex<HistoryDB>>,
        request: HistoryPageRequestOwned,
    ) -> Result<Vec<HistoryEntry>, String> {
        Ok(Self::page_async(database, request).await?.entries)
    }

    pub async fn delete_async(database: Arc<Mutex<HistoryDB>>, id: i64) -> Result<(), String> {
        Self::delete_async_with_hook(database, id, || {}).await
    }

    pub async fn delete_async_with_hook<H: FnOnce() + Send + 'static>(
        database: Arc<Mutex<HistoryDB>>,
        id: i64,
        on_commit: H,
    ) -> Result<(), String> {
        run_db_named("history.delete", database, move |database| {
            let result = HistoryOperations { db: database }.delete(id);
            if result.is_ok() {
                on_commit();
            }
            result
        })
        .await
        .map_err(|error: DbExecutionError<String>| error.to_string())
    }

    pub async fn set_favorite_async(
        database: Arc<Mutex<HistoryDB>>,
        id: i64,
        favorite: bool,
    ) -> Result<FavoriteMutation, String> {
        Self::set_favorite_async_with_hook(database, id, favorite, || {}).await
    }

    pub async fn set_favorite_async_with_hook<H: FnOnce() + Send + 'static>(
        database: Arc<Mutex<HistoryDB>>,
        id: i64,
        favorite: bool,
        on_commit: H,
    ) -> Result<FavoriteMutation, String> {
        run_db_named("history.set_favorite", database, move |database| {
            let result = HistoryOperations { db: database }.set_favorite(id, favorite);
            if result.is_ok() {
                on_commit();
            }
            result
        })
        .await
        .map_err(|error: DbExecutionError<String>| error.to_string())
    }

    pub async fn delete_favorite_async(
        database: Arc<Mutex<HistoryDB>>,
        id: i64,
    ) -> Result<FavoriteMutation, String> {
        Self::delete_favorite_async_with_hook(database, id, || {}).await
    }

    pub async fn delete_favorite_async_with_hook<H: FnOnce() + Send + 'static>(
        database: Arc<Mutex<HistoryDB>>,
        id: i64,
        on_commit: H,
    ) -> Result<FavoriteMutation, String> {
        run_db_named("history.delete_favorite", database, move |database| {
            let result = HistoryOperations { db: database }.delete_favorite(id);
            if result.is_ok() {
                on_commit();
            }
            result
        })
        .await
        .map_err(|error: DbExecutionError<String>| error.to_string())
    }

    pub async fn clear_async(database: Arc<Mutex<HistoryDB>>) -> Result<(), String> {
        Self::clear_async_with_hook(database, || {}).await
    }

    pub async fn clear_async_with_hook<H: FnOnce() + Send + 'static>(
        database: Arc<Mutex<HistoryDB>>,
        on_commit: H,
    ) -> Result<(), String> {
        run_db_named("history.clear", database, move |database| {
            let result = HistoryOperations { db: database }.clear();
            if result.is_ok() {
                on_commit();
            }
            result
        })
        .await
        .map_err(|error: DbExecutionError<String>| error.to_string())
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ArchiveResourceFailure {
    pub image_index: u16,
    pub code: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ArchiveEntryImagesReport {
    pub archived: u16,
    pub deduplicated: u16,
    pub complete: bool,
    pub failures: Vec<ArchiveResourceFailure>,
}

struct ArchiveCancelOnDrop(tailsync_core::cancellation::Cancellation);

impl Drop for ArchiveCancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

static ARCHIVE_ADMISSION: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);

#[derive(Clone)]
pub struct ArchiveEntryGuard {
    pub(crate) identity: tailsync_core::composite::identity::HistoryIdentity,
    pub(crate) authority: tailsync_core::db::HistoryReadRevision,
}

#[derive(Clone)]
pub struct ArchiveCommit {
    pub id: i64,
    pub guard: ArchiveEntryGuard,
}

impl ArchiveCommit {
    pub fn has_saved_archive(&self) -> bool {
        self.guard.identity.archive_revision > 1
    }
}

impl ArchiveEntryGuard {
    pub fn capture(database: &HistoryDB, id: i64) -> Result<Self, String> {
        let identity = database
            .composite_identity(id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "not_found".to_string())?;
        let authority = database
            .read_revision()
            .map_err(|_| "storage_unavailable".to_string())?;
        Ok(Self {
            identity,
            authority,
        })
    }
}

impl HistoryOperations<'_> {
    pub async fn archive_entry_images(
        database: Arc<Mutex<HistoryDB>>,
        id: i64,
        transport: Arc<dyn tailsync_core::composite::resource_archive::ResourceTransport>,
        cancellation: tailsync_core::cancellation::Cancellation,
    ) -> Result<ArchiveEntryImagesReport, String> {
        Self::archive_entry_images_with_hook(database, id, transport, cancellation, || {}).await
    }

    pub async fn archive_entry_images_with_hook<H: FnOnce() + Send + 'static>(
        database: Arc<Mutex<HistoryDB>>,
        id: i64,
        transport: Arc<dyn tailsync_core::composite::resource_archive::ResourceTransport>,
        cancellation: tailsync_core::cancellation::Cancellation,
        on_commit: H,
    ) -> Result<ArchiveEntryImagesReport, String> {
        Self::archive_entry_images_with_commit_hook(
            database,
            id,
            transport,
            cancellation,
            move |_| on_commit(),
        )
        .await
    }

    pub async fn archive_entry_images_with_commit_hook<
        H: FnOnce(ArchiveCommit) + Send + 'static,
    >(
        database: Arc<Mutex<HistoryDB>>,
        id: i64,
        transport: Arc<dyn tailsync_core::composite::resource_archive::ResourceTransport>,
        cancellation: tailsync_core::cancellation::Cancellation,
        on_commit: H,
    ) -> Result<ArchiveEntryImagesReport, String> {
        Self::archive_entry_images_guarded(database, id, None, transport, cancellation, on_commit)
            .await
    }

    pub async fn archive_captured_images_with_hook<H: FnOnce() + Send + 'static>(
        database: Arc<Mutex<HistoryDB>>,
        id: i64,
        expected: ArchiveEntryGuard,
        transport: Arc<dyn tailsync_core::composite::resource_archive::ResourceTransport>,
        cancellation: tailsync_core::cancellation::Cancellation,
        on_commit: H,
    ) -> Result<ArchiveEntryImagesReport, String> {
        Self::archive_captured_images_with_commit_hook(
            database,
            id,
            expected,
            transport,
            cancellation,
            move |_| on_commit(),
        )
        .await
    }

    pub async fn archive_captured_images_with_commit_hook<
        H: FnOnce(ArchiveCommit) + Send + 'static,
    >(
        database: Arc<Mutex<HistoryDB>>,
        id: i64,
        expected: ArchiveEntryGuard,
        transport: Arc<dyn tailsync_core::composite::resource_archive::ResourceTransport>,
        cancellation: tailsync_core::cancellation::Cancellation,
        on_commit: H,
    ) -> Result<ArchiveEntryImagesReport, String> {
        Self::archive_entry_images_guarded(
            database,
            id,
            Some(expected),
            transport,
            cancellation,
            on_commit,
        )
        .await
    }

    async fn archive_entry_images_guarded<H: FnOnce(ArchiveCommit) + Send + 'static>(
        database: Arc<Mutex<HistoryDB>>,
        id: i64,
        expected: Option<ArchiveEntryGuard>,
        transport: Arc<dyn tailsync_core::composite::resource_archive::ResourceTransport>,
        cancellation: tailsync_core::cancellation::Cancellation,
        on_commit: H,
    ) -> Result<ArchiveEntryImagesReport, String> {
        let _cancel_on_drop = ArchiveCancelOnDrop(cancellation.clone());
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => Err("archive_cancelled".into()),
            result = tokio::time::timeout(std::time::Duration::from_secs(45), Self::archive_entry_images_inner(database, id, expected, transport, cancellation.clone(), on_commit)) => result.map_err(|_| "archive_deadline".to_string())?,
        }
    }

    async fn archive_entry_images_inner<H: FnOnce(ArchiveCommit) + Send + 'static>(
        database: Arc<Mutex<HistoryDB>>,
        id: i64,
        expected: Option<ArchiveEntryGuard>,
        transport: Arc<dyn tailsync_core::composite::resource_archive::ResourceTransport>,
        cancellation: tailsync_core::cancellation::Cancellation,
        on_commit: H,
    ) -> Result<ArchiveEntryImagesReport, String> {
        use tailsync_core::composite::resource_archive::{
            archive_external_images, ArchiveLimits, ArchiveOutcome,
        };
        let _admission = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err("archive_cancelled".into()),
            permit = ARCHIVE_ADMISSION.acquire() => permit.map_err(|_| "archive_transport".to_string())?,
        };
        let cancellation_for_read = cancellation.clone();
        let (identity, authority, base) = crate::execution::run_db_named_cancellable(
            "history.archive_entry_images.read",
            database.clone(),
            cancellation.clone(),
            move |database| {
                if cancellation_for_read.is_cancelled() {
                    return Err("archive_cancelled".to_string());
                }
                let identity = database
                    .composite_identity(id)
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| "not_found".to_string())?;
                let authority = database
                    .read_revision()
                    .map_err(|_| "storage_unavailable".to_string())?;
                if let Some(expected) = expected {
                    database
                        .validate_storage_authority(&expected.authority)
                        .map_err(|_| "archive_superseded".to_string())?;
                    if identity != expected.identity {
                        return Err("archive_superseded".to_string());
                    }
                }
                let base = database
                    .get_composite_archived(id)
                    .map_err(|_| "storage_unavailable".to_string())?
                    .map(Ok)
                    .unwrap_or_else(|| database.get_composite(id))
                    .map_err(|_| "storage_unavailable".to_string())?;
                Ok((identity, authority, base))
            },
        )
        .await
        .map_err(|error: DbExecutionError<String>| error.to_string())?;
        let outcome: ArchiveOutcome = archive_external_images(
            &base,
            transport.as_ref(),
            &ArchiveLimits::default(),
            &cancellation,
        )
        .await
        .map_err(|error| error.code().to_string())?;
        let report = ArchiveEntryImagesReport {
            archived: outcome.archived.min(u16::MAX as usize) as u16,
            deduplicated: outcome.deduplicated.min(u16::MAX as usize) as u16,
            complete: outcome.images_complete(),
            failures: outcome
                .failures
                .iter()
                .map(|(index, error)| ArchiveResourceFailure {
                    image_index: *index as u16,
                    code: error.code().to_string(),
                })
                .collect(),
        };
        if outcome.archived == 0 {
            return Ok(report);
        }
        if cancellation.is_cancelled() {
            return Err("archive_cancelled".into());
        }
        let cancellation_for_commit = cancellation.clone();
        crate::execution::run_db_named_cancellable(
            "history.archive_entry_images.commit",
            database,
            cancellation.clone(),
            move |database| {
                if cancellation_for_commit.is_cancelled() {
                    return Err("archive_cancelled".to_string());
                }
                database
                    .validate_storage_authority(&authority)
                    .map_err(|_| "archive_superseded".to_string())?;
                match database
                    .commit_composite_archive_with_originals(
                        id,
                        &identity,
                        &outcome.bundle,
                        &outcome.originals,
                        &cancellation_for_commit,
                    )
                    .map_err(|error| error.to_string())?
                {
                    Some(identity) => {
                        on_commit(ArchiveCommit {
                            id,
                            guard: ArchiveEntryGuard {
                                identity,
                                authority,
                            },
                        });
                        Ok(())
                    }
                    None => Err("not_found".to_string()),
                }
            },
        )
        .await
        .map_err(|error: DbExecutionError<String>| error.to_string())?;
        Ok(report)
    }
}

impl<'a> HistoryOperations<'a> {
    /// Validate a UI's held retry identity under the same DB lock used to
    /// prepare the actual payload. Never trust a UI's cross-row hash fallback.
    pub async fn restore_payload_checked_async(
        database: Arc<Mutex<HistoryDB>>,
        id: i64,
        original_id: Option<i64>,
        expected: Option<tailsync_core::composite::identity::HistoryIdentity>,
    ) -> Result<HistoryRestorePayload, String> {
        run_db_named("history.restore_payload", database, move |database| {
            let payload = HistoryOperations { db: database }.restore_payload(id)?;
            if let Some(expected) = expected {
                let current = payload
                    .identity
                    .as_ref()
                    .ok_or_else(|| "not_found".to_string())?;
                let matches = if original_id == Some(id) {
                    expected.same_capture(current)
                } else {
                    original_id.is_some() && expected.may_rebind(current)
                };
                if !matches {
                    return Err("not_found".into());
                }
            } else if original_id.is_some() {
                return Err("invalid_argument".into());
            }
            Ok(payload)
        })
        .await
        .map_err(|error: DbExecutionError<String>| error.to_string())
    }

    pub async fn restore_payload_async(
        database: Arc<Mutex<HistoryDB>>,
        id: i64,
    ) -> Result<HistoryRestorePayload, String> {
        run_db_named("history.restore_payload", database, move |database| {
            HistoryOperations { db: database }.restore_payload(id)
        })
        .await
        .map_err(|error: DbExecutionError<String>| error.to_string())
    }

    /// Decrypt one image entry for in-memory thumbnail rendering.
    ///
    /// The entry type and declared size are validated from database metadata
    /// before the payload is opened or decrypted, so a mis-typed id can never
    /// pull a multi-gigabyte file into memory. The actual read reuses the
    /// shared bounded preview path and runs outside the database lock.
    pub async fn image_payload_async(
        database: Arc<Mutex<HistoryDB>>,
        id: i64,
    ) -> Result<Vec<u8>, String> {
        use tailsync_core::db::{PreviewErrorInfo, PreviewKind};
        let result = run_read("history.image_payload", database, move |context| {
            let prepared = context.with_database(|database| {
                if database
                    .get_type(id)
                    .map_err(|e| tailsync_core::db::HistoryReadError::Operation(e.to_string()))?
                    == "composite"
                {
                    return Ok(database.prepare_composite_image(id));
                }
                Ok(database
                    .get_preview_metadata(id)
                    .and_then(|metadata| match metadata.kind {
                        PreviewKind::Image => database.prepare_preview(id, None),
                        kind => Err(PreviewError::UnsupportedType {
                            kind: kind.as_str().to_string(),
                        }),
                    }))
            })?;
            let read = prepared.and_then(|prepared| prepared.read_admitted(&context.cancellation));
            context.cancellation.check()?;
            Ok(read.map(|(_, payload, _memory)| payload.data))
        })
        .await;
        let encode = |error: PreviewError| {
            let fallback = error.to_string();
            serde_json::to_string(&PreviewErrorInfo::from(error)).unwrap_or(fallback)
        };
        match result {
            Ok(Ok(data)) => Ok(data),
            Ok(Err(error)) => Err(encode(error)),
            Err(error) => Err(error.to_string()),
        }
    }

    pub async fn migration_diagnostics_async(
        database: Arc<Mutex<HistoryDB>>,
        limit: usize,
    ) -> Result<MigrationDiagnostics, String> {
        run_db_named("history.migration_diagnostics", database, move |database| {
            database
                .migration_diagnostics(limit)
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|error: DbExecutionError<String>| error.to_string())
    }

    /// Read, decrypt and encode under the same bounded/cancellable worker
    /// permit. Large frame copies never run on an async executor thread.
    pub async fn preview_binary_async(
        database: Arc<Mutex<HistoryDB>>,
        id: i64,
        batch_id: Option<String>,
        request_id: Option<String>,
    ) -> Result<Vec<u8>, tailsync_core::db::PreviewErrorInfo> {
        use tailsync_core::db::PreviewErrorInfo;
        run_read("history.preview_binary", database, move |context| {
            let prepared = context
                .with_database(|database| Ok(database.prepare_preview(id, batch_id.as_deref())))?;
            let result = prepared
                .and_then(|prepared| prepared.read_admitted(&context.cancellation))
                .map_err(PreviewErrorInfo::from)
                .and_then(|(metadata, payload, _memory)| {
                    crate::preview::encode_preview_response_for_request(
                        metadata, payload, request_id,
                    )
                });
            context.cancellation.check()?;
            Ok(result)
        })
        .await
        .map_err(|error| PreviewErrorInfo::payload_unavailable(id, error.to_string()))?
    }

    pub async fn preview_async(
        database: Arc<Mutex<HistoryDB>>,
        id: i64,
        batch_id: Option<String>,
    ) -> Result<(PreviewMetadata, PreviewPayload), PreviewError> {
        let result = run_read("history.preview", database, move |context| {
            let prepared = context
                .with_database(|database| Ok(database.prepare_preview(id, batch_id.as_deref())))?;
            Ok(prepared.and_then(|prepared| prepared.read(&context.cancellation)))
        })
        .await;
        match result {
            Ok(result) => result,
            Err(error) => Err(PreviewError::PayloadUnavailable {
                entry_id: id,
                reason: error.to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn image_payload_rejects_non_image_entries_before_decrypting() {
        let mut database = HistoryDB::new_unavailable().unwrap();
        database.add_text("not an image", "fixture").unwrap();
        let id = database.get_all(None, None, 1, 0).unwrap()[0].id;
        let database = Arc::new(Mutex::new(database));

        // The kind gate runs on metadata, before the payload is opened or
        // decrypted, so a non-image id never loads clipboard data.
        let error = HistoryOperations::image_payload_async(database, id)
            .await
            .unwrap_err();
        assert!(error.contains("unsupported_type"), "missing code: {error}");
        assert!(
            error.contains("Preview type is unsupported"),
            "unexpected error: {error}"
        );
    }

    #[tokio::test]
    async fn chunked_search_preserves_legacy_offset_filter_and_has_more() {
        let mut database = HistoryDB::new_unavailable().unwrap();
        database.set_max_history(1000);
        for index in 0..300 {
            database
                .add_text(
                    &format!(
                        "fixture {index} {}",
                        if index % 3 == 0 { "needle" } else { "other" }
                    ),
                    "fixture",
                )
                .unwrap();
        }
        let expected = database
            .get_page_filtered(Some("needle"), None, None, None, 17, 80)
            .unwrap();
        let database = Arc::new(Mutex::new(database));
        let request = HistoryPageRequestOwned::new(
            HistoryCollection::All,
            Some("needle".into()),
            None,
            None,
            None,
            Some(17),
            Some(80),
            50,
        );
        let actual = HistoryOperations::page_read(database.clone(), request)
            .await
            .unwrap();
        assert_eq!(
            actual.entries.iter().map(|e| e.id).collect::<Vec<_>>(),
            expected.entries.iter().map(|e| e.id).collect::<Vec<_>>()
        );
        assert_eq!(actual.has_more, expected.has_more);
        assert_eq!(actual.total, expected.total);
        let expected = database
            .lock()
            .await
            .get_page_filtered(None, None, None, None, 17, 290)
            .unwrap();
        let actual = HistoryOperations::page_read(
            database,
            HistoryPageRequestOwned::new(
                HistoryCollection::All,
                None,
                None,
                None,
                None,
                Some(17),
                Some(290),
                50,
            ),
        )
        .await
        .unwrap();
        assert_eq!(actual.entries.len(), expected.entries.len());
        assert_eq!(actual.total, Some(300));
        assert!(!actual.has_more);
    }

    #[tokio::test]
    async fn client_cancellation_after_commit_does_not_drop_notification() {
        let mut database = HistoryDB::new_unavailable().unwrap();
        database.add_text("commit then cancel", "fixture").unwrap();
        let id = database.get_all(None, None, 1, 0).unwrap()[0].id;
        let database = Arc::new(Mutex::new(database));
        let (committed, commit) = tokio::sync::oneshot::channel();
        let (release, released) = std::sync::mpsc::channel();
        let (published, publication) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(HistoryOperations::delete_async_with_hook(
            database.clone(),
            id,
            move || {
                let _ = committed.send(());
                released
                    .recv_timeout(std::time::Duration::from_secs(2))
                    .unwrap();
                let _ = published.send(());
            },
        ));
        commit.await.unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        release.send(()).unwrap();
        publication.await.unwrap();
        assert!(database
            .lock()
            .await
            .get_all(None, None, 10, 0)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn request_defaults_are_applied_at_the_use_case_seam() {
        let request = HistoryPageRequest::new(
            HistoryCollection::All,
            None,
            None,
            None,
            None,
            None,
            None,
            37,
        );
        assert_eq!(request.limit, 37);
        assert_eq!(request.offset, 0);
    }

    #[test]
    fn o01_explicit_conversion_cannot_pollute_default_original_restore() {
        use tailsync_core::protocol::{native_archive::*, native_clipboard::*};
        let root = std::env::temp_dir().join(format!(
            "tailsync-o01-runtime-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let archive = NativeArchive {
            platform: 1,
            enumeration_complete: true,
            original: NativeSnapshot {
                before: 1,
                after: 1,
                items: 1,
                state: SnapshotState::Stable,
                formats: vec![
                    NativeFormat {
                        item: 0,
                        name: "public.utf8-plain-text".into(),
                        state: FormatState::Captured,
                        bytes: b"original".to_vec(),
                    },
                    NativeFormat {
                        item: 0,
                        name: "com.apple.flat-rtfd".into(),
                        state: FormatState::Captured,
                        bytes: b"original RTFD".to_vec(),
                    },
                ],
            },
            derived: vec![
                DerivedFormat {
                    item: 0,
                    name: "public.utf8-plain-text".into(),
                    source_name: "com.apple.flat-rtfd".into(),
                    converter: "tailsync.rtfd-to-portable.v1".into(),
                    bytes: b"derived".to_vec(),
                },
                DerivedFormat {
                    item: 0,
                    name: "public.html".into(),
                    source_name: "com.apple.flat-rtfd".into(),
                    converter: "tailsync.rtfd-to-portable.v1".into(),
                    bytes: b"<p>derived</p>".to_vec(),
                },
            ],
        };
        let id = db.add_native_archive(&archive, "self").unwrap();
        let payload = HistoryOperations::new(&mut db).restore_payload(id).unwrap();
        assert_eq!(payload.native_archive_complete, Some(true));
        assert!(matches!(
            payload.prepare_composite(false, None).unwrap(),
            PreparedRestore::Unsupported {
                code: "native_only"
            }
        ));
        match payload.prepare_composite(true, Some("text")).unwrap() {
            PreparedRestore::Portable(bundle) => assert_eq!(bundle.text(), Some("derived")),
            _ => panic!("unexpected restore selection"),
        }
        match payload.prepare_composite(true, None).unwrap() {
            PreparedRestore::Native(native) => assert_eq!(native, archive.publication().unwrap()),
            _ => panic!("unexpected restore selection"),
        }
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn o01_incomplete_originals_refuse_default_but_keep_explicit_compatibility() {
        use tailsync_core::composite::{Completeness, Mime, PortableBundle, Representation};
        use tailsync_core::protocol::{native_archive::*, native_clipboard::*};
        let root = std::env::temp_dir().join(format!(
            "tailsync-o01-incomplete-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        for failed in [false, true] {
            let mut archive = NativeArchive {
                platform: 1,
                enumeration_complete: failed,
                original: NativeSnapshot {
                    before: 1,
                    after: 1,
                    items: 1,
                    state: SnapshotState::Stable,
                    formats: vec![NativeFormat {
                        item: 0,
                        name: "public.utf8-plain-text".into(),
                        state: FormatState::Captured,
                        bytes: b"original text".to_vec(),
                    }],
                },
                derived: vec![],
            };
            if failed {
                archive.original.formats.push(NativeFormat {
                    item: 0,
                    name: "public.html".into(),
                    state: FormatState::Failed,
                    bytes: vec![],
                });
            }
            let id = db.add_native_archive(&archive, "self").unwrap();
            let payload = HistoryOperations::new(&mut db).restore_payload(id).unwrap();
            assert_eq!(payload.native_archive_complete, Some(false));
            for supported in [false, true] {
                assert!(matches!(
                    payload.prepare_composite(supported, None).unwrap(),
                    PreparedRestore::Unsupported {
                        code: "originals_incomplete"
                    }
                ));
            }
            assert!(!payload.restore_capabilities().unwrap().whole_group);
            assert_eq!(
                payload.restore_capabilities().unwrap().whole_group_code,
                Some("originals_incomplete")
            );
            assert!(
                matches!(payload.prepare_composite(false, Some("text")).unwrap(),
                PreparedRestore::Portable(b) if b.text() == Some("original text"))
            );
        }
        // Old portable records retain their default restore behavior.
        let b = PortableBundle {
            representations: vec![Representation {
                mime: Mime::Text,
                bytes: b"legacy portable".to_vec(),
            }],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::Complete,
        };
        let id = db.add_composite(&b, None, "self").unwrap();
        let payload = HistoryOperations::new(&mut db).restore_payload(id).unwrap();
        assert_eq!(payload.native_archive_complete, None);
        assert!(matches!(
            payload.prepare_composite(false, None).unwrap(),
            PreparedRestore::Portable(_)
        ));
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn o11_daemon_validates_held_identity_under_the_payload_lock() {
        use tailsync_core::composite::{Completeness, Mime, PortableBundle, Representation};
        let root = std::env::temp_dir().join(format!(
            "tailsync-o11-runtime-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut db = HistoryDB::open_isolated_for_test_composite(&root).unwrap();
        let b = PortableBundle {
            representations: vec![Representation {
                mime: Mime::Text,
                bytes: b"same".to_vec(),
            }],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::Complete,
        };
        let first = db.add_composite(&b, None, "self").unwrap();
        let held = db.composite_identity(first).unwrap().unwrap();
        let second = db.add_composite(&b, None, "self").unwrap();
        let db = Arc::new(Mutex::new(db));
        assert!(HistoryOperations::restore_payload_checked_async(
            db.clone(),
            second,
            Some(first),
            Some(held.clone())
        )
        .await
        .is_ok());
        let mut wrong = held.clone();
        wrong.content_id = "different-original".into();
        assert!(HistoryOperations::restore_payload_checked_async(
            db.clone(),
            second,
            Some(first),
            Some(wrong)
        )
        .await
        .is_err());
        assert!(HistoryOperations::restore_payload_checked_async(
            db.clone(),
            second,
            Some(second),
            Some(held)
        )
        .await
        .is_err());
        db.lock().await.delete(second).unwrap();
        assert!(
            HistoryOperations::restore_payload_checked_async(db.clone(), second, None, None)
                .await
                .is_err()
        );
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn restore_payload_reads_the_core_entry_once_without_transport_policy() {
        let mut db = HistoryDB::new_unavailable().expect("in-memory history database");
        db.add_text("runtime seam", "test").unwrap();
        let id = db.get_all(None, None, 1, 0).unwrap()[0].id;

        let payload = HistoryOperations::new(&mut db)
            .restore_payload(id)
            .expect("history payload");

        assert_eq!(payload.entry_type, "text");
        assert!(payload.file_path.is_none());
        assert!(payload.file_name.is_none());
        assert_eq!(
            String::from_utf8(payload.data.expect("text data")).unwrap(),
            "runtime seam"
        );
    }

    #[test]
    fn native_only_restore_payload_keeps_the_recorded_shape() {
        use tailsync_core::protocol::native_clipboard::*;
        // This entry is a composite native shape, so the degraded store must be
        // created with the policy that carries the composite schema (K8).
        let mut db = HistoryDB::new_unavailable_with_policy(
            tailsync_core::db::MigrationPolicy::CompositeAllowed,
        )
        .unwrap();
        let snapshot = NativeSnapshot {
            before: 7,
            after: 7,
            items: 2,
            state: SnapshotState::Stable,
            formats: (0..2)
                .map(|item| NativeFormat {
                    item,
                    name: "public.utf8-plain-text".into(),
                    state: FormatState::Captured,
                    bytes: format!("native item {item}").into_bytes(),
                })
                .collect(),
        };
        let id = db.add_native_snapshot(&snapshot, "fixture").unwrap();
        let payload = HistoryOperations::new(&mut db).restore_payload(id).unwrap();
        assert!(payload.native_only);
        let native = payload.native.unwrap();
        assert_eq!(native.items, 2);
        assert_eq!(native.formats, snapshot.formats);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn successful_mutation_publishes_after_commit_inside_the_worker() {
        let database = Arc::new(Mutex::new(
            HistoryDB::new_unavailable().expect("in-memory history database"),
        ));
        crate::execution::run_db(database.clone(), |database| {
            database
                .add_text("commit notification", "test")
                .map_err(|error| error.to_string())
        })
        .await
        .expect("history entry");
        let id = HistoryOperations::entries_async(
            database.clone(),
            HistoryPageRequestOwned::new(
                HistoryCollection::All,
                None,
                None,
                None,
                None,
                Some(1),
                Some(0),
                1,
            ),
        )
        .await
        .expect("history page")[0]
            .id;
        let notifications = Arc::new(AtomicUsize::new(0));
        let callback_notifications = notifications.clone();

        HistoryOperations::delete_async_with_hook(database.clone(), id, move || {
            callback_notifications.fetch_add(1, Ordering::SeqCst);
        })
        .await
        .expect("delete entry");

        assert_eq!(notifications.load(Ordering::SeqCst), 1);
        assert!(HistoryOperations::entries_async(
            database,
            HistoryPageRequestOwned::new(
                HistoryCollection::All,
                None,
                None,
                None,
                None,
                Some(1),
                Some(0),
                1,
            ),
        )
        .await
        .expect("history page")
        .is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn failed_mutation_does_not_publish_a_revision() {
        let database = Arc::new(Mutex::new(
            HistoryDB::new_unavailable().expect("in-memory history database"),
        ));
        let notifications = Arc::new(AtomicUsize::new(0));
        let callback_notifications = notifications.clone();

        let result = HistoryOperations::delete_async_with_hook(database, 404, move || {
            callback_notifications.fetch_add(1, Ordering::SeqCst);
        })
        .await;

        assert!(result.is_err());
        assert_eq!(notifications.load(Ordering::SeqCst), 0);
    }
}

#[cfg(test)]
mod archive_tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::Mutex as StdMutex;
    use tailsync_core::composite::resource_archive::{
        ArchiveError, ArchiveUrl, FetchFuture, FetchLimits, FetchResponse, ResourceTransport,
    };
    use tailsync_core::composite::{
        packed_to_png, Completeness, Mime, PortableBundle, Representation,
    };
    use tailsync_core::db::MigrationPolicy;

    struct MockTransport(StdMutex<BTreeMap<String, Vec<u8>>>);

    impl ResourceTransport for MockTransport {
        fn fetch_once<'a>(
            &'a self,
            url: &'a ArchiveUrl,
            _limits: &'a FetchLimits,
            cancellation: &'a tailsync_core::cancellation::Cancellation,
        ) -> FetchFuture<'a> {
            Box::pin(async move {
                if cancellation.is_cancelled() {
                    return Err(ArchiveError::Cancelled);
                }
                let bytes = self
                    .0
                    .lock()
                    .unwrap()
                    .get(url.as_str())
                    .cloned()
                    .ok_or(ArchiveError::Resolve)?;
                Ok(FetchResponse {
                    status: 200,
                    location: None,
                    content_type: Some("image/png".into()),
                    bytes,
                })
            })
        }
    }

    fn external_bundle() -> PortableBundle {
        PortableBundle {
            representations: vec![Representation {
                mime: Mime::Html,
                bytes: b"<p>x</p><img src=\"https://fixture.example/a.png\">".to_vec(),
            }],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::ExternalReferences,
        }
    }

    fn transport() -> Arc<MockTransport> {
        Arc::new(MockTransport(StdMutex::new(BTreeMap::from([(
            "https://fixture.example/a.png".to_string(),
            packed_to_png(1, 1, &[1, 2, 3, 255]).unwrap(),
        )]))))
    }

    #[tokio::test(flavor = "current_thread")]
    async fn network_image_download_reports_success_for_saved_forum_image_and_retry() {
        let database = Arc::new(Mutex::new(
            HistoryDB::new_unavailable_with_policy(MigrationPolicy::CompositeAllowed).unwrap(),
        ));
        let mut bundle = external_bundle();
        bundle.representations[0].bytes = br##"<p>forum message</p><a href="https://fixture.example/original.jpg"><img src="https://fixture.example/a.png" srcset="https://fixture.example/a.png, https://fixture.example/large.png 2x"><svg aria-hidden="true"><use href="#far-image"></use></svg></a>"##.to_vec();
        let original = bundle.encode().unwrap();
        let id = database
            .lock()
            .await
            .add_composite(&bundle, None, "self")
            .unwrap();
        let report = HistoryOperations::archive_entry_images(
            database.clone(),
            id,
            transport(),
            tailsync_core::cancellation::Cancellation::default(),
        )
        .await
        .unwrap();
        assert_eq!(report.archived, 1);
        assert!(report.failures.is_empty());
        assert!(
            report.complete,
            "the img/src was saved; srcset and a decorative SVG are not missing downloaded images"
        );
        let guard = database.lock().await;
        let saved = guard.get_composite_archived(id).unwrap().unwrap();
        assert_eq!(saved.completeness, Completeness::ExternalReferences);
        assert_eq!(guard.get_composite(id).unwrap().encode().unwrap(), original);
        let identity = guard.composite_identity(id).unwrap();
        drop(guard);
        let retry = HistoryOperations::archive_entry_images(
            database.clone(),
            id,
            Arc::new(MockTransport(StdMutex::new(BTreeMap::new()))),
            tailsync_core::cancellation::Cancellation::default(),
        )
        .await
        .unwrap();
        assert!(
            retry.complete,
            "retry must recognise the already saved image without network access"
        );
        assert_eq!(retry.archived, 0);
        assert!(retry.failures.is_empty());
        assert_eq!(
            database.lock().await.composite_identity(id).unwrap(),
            identity
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn network_image_download_keeps_real_missing_images_partial() {
        let database = Arc::new(Mutex::new(
            HistoryDB::new_unavailable_with_policy(MigrationPolicy::CompositeAllowed).unwrap(),
        ));
        let mut bundle = external_bundle();
        bundle.representations[0].bytes = br#"<img src="https://fixture.example/a.png" srcset="https://fixture.example/large.png 2x"><img src="https://fixture.example/missing.png">"#.to_vec();
        let id = database
            .lock()
            .await
            .add_composite(&bundle, None, "self")
            .unwrap();
        let report = HistoryOperations::archive_entry_images(
            database.clone(),
            id,
            transport(),
            tailsync_core::cancellation::Cancellation::default(),
        )
        .await
        .unwrap();
        assert_eq!(report.archived, 1);
        assert!(!report.complete);
        assert_eq!(report.failures.len(), 1);
        assert_eq!(report.failures[0].image_index, 1);
        assert_eq!(report.failures[0].code, "archive_resolve");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn o04_review_an_archived_row_reuses_its_resources_without_network_or_revision_change() {
        let database = Arc::new(Mutex::new(
            HistoryDB::new_unavailable_with_policy(MigrationPolicy::CompositeAllowed).unwrap(),
        ));
        let id = database
            .lock()
            .await
            .add_composite(&external_bundle(), None, "self")
            .unwrap();
        HistoryOperations::archive_entry_images(
            database.clone(),
            id,
            transport(),
            tailsync_core::cancellation::Cancellation::default(),
        )
        .await
        .unwrap();
        let original = database.lock().await.composite_identity(id).unwrap();
        let report = HistoryOperations::archive_entry_images(
            database.clone(),
            id,
            Arc::new(MockTransport(StdMutex::new(BTreeMap::new()))),
            tailsync_core::cancellation::Cancellation::default(),
        )
        .await
        .unwrap();
        assert!(report.complete);
        assert_eq!(report.archived, 0);
        assert_eq!(
            database.lock().await.composite_identity(id).unwrap(),
            original
        );
        let entry = database
            .lock()
            .await
            .get_all(None, None, 10, 0)
            .unwrap()
            .remove(0);
        let info = entry.composite.unwrap();
        assert_eq!(info.completeness, "complete");
        assert_eq!(info.image_count, 1);
        assert!(info
            .restorable_formats
            .iter()
            .any(|format| format == "html"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn o04_review_cancellation_and_drop_release_database_waiters() {
        let database = Arc::new(Mutex::new(
            HistoryDB::new_unavailable_with_policy(MigrationPolicy::CompositeAllowed).unwrap(),
        ));
        let id = database
            .lock()
            .await
            .add_composite(&external_bundle(), None, "self")
            .unwrap();
        let guard = database.lock().await;
        let cancellation = tailsync_core::cancellation::Cancellation::default();
        let worker_database = database.clone();
        let worker_cancellation = cancellation.clone();
        let mut task = tokio::spawn(async move {
            HistoryOperations::archive_entry_images(
                worker_database,
                id,
                transport(),
                worker_cancellation,
            )
            .await
        });
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        cancellation.cancel();
        let result = tokio::time::timeout(std::time::Duration::from_millis(200), &mut task).await;
        drop(guard);
        assert_eq!(result.unwrap().unwrap().unwrap_err(), "archive_cancelled");
        let guard = database.lock().await;
        let cancellation = tailsync_core::cancellation::Cancellation::default();
        let worker_cancellation = cancellation.clone();
        let worker_database = database.clone();
        let task = tokio::spawn(async move {
            HistoryOperations::archive_entry_images(
                worker_database,
                id,
                transport(),
                worker_cancellation,
            )
            .await
        });
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(cancellation.is_cancelled());
        drop(guard);
        assert!(database
            .lock()
            .await
            .get_composite_archived(id)
            .unwrap()
            .is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn archive_operation_commits_a_derived_layer_without_touching_the_base() {
        let database = Arc::new(Mutex::new(
            HistoryDB::new_unavailable_with_policy(MigrationPolicy::CompositeAllowed).unwrap(),
        ));
        let id = {
            let mut guard = database.lock().await;
            guard
                .add_composite(&external_bundle(), None, "self")
                .unwrap()
        };
        let report = HistoryOperations::archive_entry_images(
            database.clone(),
            id,
            transport(),
            tailsync_core::cancellation::Cancellation::default(),
        )
        .await
        .unwrap();
        assert_eq!(report.archived, 1);
        assert!(report.complete);
        assert!(report.failures.is_empty());
        let guard = database.lock().await;
        let archived = guard.get_composite_archived(id).unwrap().unwrap();
        assert_eq!(archived.resources.len(), 1);
        assert_eq!(archived.completeness, Completeness::Complete);
        let base = guard.get_composite(id).unwrap();
        assert!(String::from_utf8(base.representations[0].bytes.clone())
            .unwrap()
            .contains("fixture.example"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cancelled_archive_operation_never_commits() {
        let database = Arc::new(Mutex::new(
            HistoryDB::new_unavailable_with_policy(MigrationPolicy::CompositeAllowed).unwrap(),
        ));
        let id = {
            let mut guard = database.lock().await;
            guard
                .add_composite(&external_bundle(), None, "self")
                .unwrap()
        };
        let cancellation = tailsync_core::cancellation::Cancellation::default();
        cancellation.cancel();
        let result = HistoryOperations::archive_entry_images(
            database.clone(),
            id,
            transport(),
            cancellation,
        )
        .await;
        assert_eq!(result.unwrap_err(), "archive_cancelled");
        let guard = database.lock().await;
        assert!(guard.get_composite_archived(id).unwrap().is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn o04_review_reopen_during_fetch_rejects_the_late_commit() {
        struct DelayedTransport {
            started: tokio::sync::Notify,
            resume: tokio::sync::Notify,
        }

        impl ResourceTransport for DelayedTransport {
            fn fetch_once<'a>(
                &'a self,
                _: &'a ArchiveUrl,
                _: &'a FetchLimits,
                _: &'a tailsync_core::cancellation::Cancellation,
            ) -> FetchFuture<'a> {
                Box::pin(async move {
                    self.started.notify_one();
                    self.resume.notified().await;
                    Ok(FetchResponse {
                        status: 200,
                        location: None,
                        content_type: Some("image/png".into()),
                        bytes: packed_to_png(1, 1, &[1, 2, 3, 255]).unwrap(),
                    })
                })
            }
        }

        let root = std::env::temp_dir().join(format!(
            "tailsync-o04-reopen-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let database = Arc::new(Mutex::new(
            HistoryDB::open_isolated_for_test_composite(&root).unwrap(),
        ));
        let id = database
            .lock()
            .await
            .add_composite(&external_bundle(), None, "self")
            .unwrap();
        let identity = database.lock().await.composite_identity(id).unwrap();
        let transport = Arc::new(DelayedTransport {
            started: tokio::sync::Notify::new(),
            resume: tokio::sync::Notify::new(),
        });
        let worker_database = database.clone();
        let worker_transport = transport.clone();
        let worker = tokio::spawn(async move {
            HistoryOperations::archive_entry_images(
                worker_database,
                id,
                worker_transport,
                tailsync_core::cancellation::Cancellation::default(),
            )
            .await
        });
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            transport.started.notified(),
        )
        .await
        .unwrap();
        {
            let mut guard = database.lock().await;
            *guard =
                HistoryDB::open_with_policy_for_probe(&root, MigrationPolicy::CompositeAllowed)
                    .unwrap();
        }
        transport.resume.notify_one();
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(2), worker)
                .await
                .unwrap()
                .unwrap()
                .unwrap_err(),
            "archive_superseded"
        );
        let guard = database.lock().await;
        assert_eq!(guard.composite_identity(id).unwrap(), identity);
        assert!(guard.get_composite_archived(id).unwrap().is_none());
        assert_eq!(guard.get_composite(id).unwrap(), external_bundle());
        drop(guard);
        drop(database);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn o04_review_partial_retry_fetches_only_the_missing_image() {
        let database = Arc::new(Mutex::new(
            HistoryDB::new_unavailable_with_policy(MigrationPolicy::CompositeAllowed).unwrap(),
        ));
        let mut base = external_bundle();
        base.representations[0]
            .bytes
            .extend_from_slice(b"<img src='https://fixture.example/b.png'>");
        let id = database
            .lock()
            .await
            .add_composite(&base, None, "self")
            .unwrap();
        let first = HistoryOperations::archive_entry_images(
            database.clone(),
            id,
            transport(),
            tailsync_core::cancellation::Cancellation::default(),
        )
        .await
        .unwrap();
        assert_eq!(first.archived, 1);
        assert!(!first.complete);
        assert_eq!(first.failures[0].image_index, 1);
        let partial = database
            .lock()
            .await
            .get_composite_archived(id)
            .unwrap()
            .unwrap();
        let identity = database
            .lock()
            .await
            .composite_identity(id)
            .unwrap()
            .unwrap();
        let second = HistoryOperations::archive_entry_images(
            database.clone(),
            id,
            Arc::new(MockTransport(StdMutex::new(BTreeMap::from([(
                "https://fixture.example/b.png".into(),
                packed_to_png(1, 1, &[4, 5, 6, 255]).unwrap(),
            )])))),
            tailsync_core::cancellation::Cancellation::default(),
        )
        .await
        .unwrap();
        assert_eq!(second.archived, 1);
        assert!(second.complete);
        assert!(second.failures.is_empty());
        let guard = database.lock().await;
        let complete = guard.get_composite_archived(id).unwrap().unwrap();
        assert_eq!(complete.resources.len(), 2);
        assert_eq!(complete.resources[0], partial.resources[0]);
        assert_eq!(
            guard
                .composite_identity(id)
                .unwrap()
                .unwrap()
                .archive_revision,
            identity.archive_revision + 1
        );
        assert_eq!(guard.get_composite(id).unwrap(), base);
    }
}
