// Shared portable-publish stage for one stored composite capture (F3-C).
//
// Both platform capture transactions (`macos/src-tauri/src/clipboard.rs` and
// `windows/src-tauri/src/clipboard.rs`) end a successful portable save with the
// same publication tail: record the stored outcome, refuse a copy that a newer
// copy or a user restore superseded while storage was writing, surface an
// incomplete capture, prepare the broadcast payload, hand it to the real
// broadcast gate and enqueue the automatic archive. The platform still owns the
// native provider, the conversion, the origin classifier, the save and the
// acknowledge/version steps; only this post-save stage is single-sourced here.
//
// This file is a canonical include pasted inside each wrapper's private
// `composite_capture` module, so [`CaptureEnvironment`], [`CaptureBroadcastGate`],
// [`enqueue_automatic_archive`] and the surrounding helpers resolve to the
// platform definitions.

/// The already-stored portable body a platform save handed to the shared stage.
///
/// Every field is an existing platform value: the observed snapshot, its
/// portable projection, the admission lease the broadcast still holds, the
/// classified origin, the generation supersede the save returned, and the
/// archive ticket/binding captured around the save. No new protocol or port.
pub(super) struct SavedPortableCapture {
    pub(super) snapshot: NativeSnapshot,
    pub(super) bundle: PortableBundle,
    pub(super) memory: Arc<tailsync_core::composite::budgets::PipelineLease>,
    pub(super) origin: Origin,
    pub(super) capture_generation: u64,
    pub(super) archive_ticket: Option<tailsync_runtime::automatic_image_archive::CaptureTicket>,
    pub(super) archive_binding: Option<tailsync_runtime::history::ArchiveCommit>,
}

/// The borrowed platform services the shared stage publishes through.
///
/// These are the same real stores, engine, pool, settings, archive queue and
/// capture environment the transaction already uses; nothing here is a new
/// seam, only a named borrow of the existing ones.
pub(super) struct PortablePublishContext<'a> {
    pub(super) database: &'a Arc<Mutex<db::HistoryDB>>,
    pub(super) sync_engine: &'a Arc<Mutex<sync::SyncEngine>>,
    pub(super) pool: &'a Arc<Mutex<network::ConnectionPool>>,
    pub(super) settings: &'a Arc<Mutex<crypto::Settings>>,
    pub(super) image_archive: &'a Arc<crate::automatic_images::AutomaticImageArchive>,
    pub(super) recovery_generation: u64,
    pub(super) env: &'a CaptureEnvironment,
}

/// Records the stored outcome and runs the shared publication tail for one
/// portable save that already acknowledged and versioned its own revision.
///
/// The order is the production order. The saved outcome is recorded first. A
/// copy the platform no longer considers current (unknown origin, superseded
/// revision, superseded generation) enqueues its archive binding and returns
/// without broadcasting. A `MissingFormats` capture raises the incomplete fault
/// before preparation. Preparation consumes the snapshot/projection and hands
/// the payload plus the retained lease to the real broadcast gate, whose
/// lock-held check is still the only authorization for the wire. A bound commit
/// replaces the binding before the automatic archive is enqueued.
pub(super) async fn publish_saved_portable_capture(
    saved: SavedPortableCapture,
    context: PortablePublishContext<'_>,
) {
    let SavedPortableCapture {
        snapshot,
        bundle,
        memory,
        origin,
        capture_generation,
        archive_ticket,
        mut archive_binding,
    } = saved;
    let PortablePublishContext {
        database,
        sync_engine,
        pool,
        settings,
        image_archive,
        recovery_generation,
        env,
    } = context;

    tailsync_core::sync_warning::record_saved_capture(
        &snapshot,
        Some(&bundle),
        origin,
        crate::clipboard_composite::supports_native(&snapshot),
    );
    // Saving this event does not authorize sending it after a newer copy or
    // user restore superseded it during storage.
    if matches!(origin, Origin::UnknownMismatch | Origin::Unknown)
        || env.revision() != Some(snapshot.after)
        || sync_engine.lock().await.clipboard_generation() != capture_generation
    {
        if let (Some(ticket), Some(commit)) = (archive_ticket, archive_binding) {
            enqueue_automatic_archive(image_archive, ticket, origin, commit);
        }
        return;
    }
    if bundle.completeness == tailsync_core::composite::Completeness::MissingFormats {
        env.capture_fault(snapshot.after, "capture_incomplete")
            .await;
    }
    tracing::debug!(
        provenance = ?tailsync_core::composite::CaptureProvenance::from_bundle(&bundle),
        "composite capture provenance"
    );
    match tailsync_core::composite::capture::prepare_broadcast(snapshot, bundle, memory) {
        Ok(captured) => {
            let bound = broadcast_composite_to_peers(
                CaptureBroadcastGate {
                    engine: sync_engine.clone(),
                    generation: capture_generation,
                    revision: captured.revision(),
                    recovery_generation,
                },
                pool,
                settings,
                database.clone(),
                archive_binding.clone(),
                image_archive,
                captured.payload().clone(),
            )
            .await;
            if bound.is_some() {
                archive_binding = bound;
            }
        }
        Err(error) => {
            warn!("Composite broadcast preparation failed: {error}");
            tailsync_core::sync_warning::record_capture_not_shared();
        }
    }
    if let (Some(ticket), Some(commit)) = (archive_ticket, archive_binding) {
        enqueue_automatic_archive(image_archive, ticket, origin, commit);
    }
}
