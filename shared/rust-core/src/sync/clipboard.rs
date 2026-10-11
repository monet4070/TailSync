use super::*;

/// Why a composite publish may or may not proceed, decided under the engine lock.
///
/// `Superseded` and `Unauthorized` are deliberately distinct: the first is a
/// definite no-write that becomes a durable `Superseded` result, the second is a
/// retryable authorization failure. Collapsing them into one `None` is how the
/// previous shape reported a revoked pairing as a lost race.
#[derive(Debug)]
pub enum CompositePublishGate {
    /// A newer local copy won the race; nothing was written.
    Superseded,
    /// Authorization was revoked between validation and publish.
    Unauthorized,
    /// Proceed with the port call.
    Ready(PreparedComposite),
}

/// A composite publish that has passed the generation check but has not been
/// written yet. Carries the port payload so the caller can release the engine lock
/// before the native call (R3-01).
#[derive(Debug)]
pub struct PreparedComposite {
    bundle: std::sync::Arc<crate::composite::PortableBundle>,
    revision: Option<i64>,
}

impl PreparedComposite {
    pub fn bundle(&self) -> &crate::composite::PortableBundle {
        &self.bundle
    }
    pub fn revision(&self) -> Option<i64> {
        self.revision
    }
    pub fn into_parts(
        self,
    ) -> (
        std::sync::Arc<crate::composite::PortableBundle>,
        Option<i64>,
    ) {
        (self.bundle, self.revision)
    }
}

/// Handle for an inbound image activation that has been prepared but not yet
/// committed. Owns the RGBA payload so it can be moved to the port.
pub struct PreparedImage {
    hash: String,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Handle for an inbound text activation that has been prepared but not yet
/// committed. Carries only the echo-guard key, so it is cheap to move across an
/// await boundary.
pub struct PreparedText {
    hash: String,
}

impl SyncEngine {
    pub(crate) fn legacy_arrival_generation(
        &mut self,
        source: &str,
        id: MessageId,
    ) -> Result<u64, String> {
        let key = (source.to_string(), id);
        if let Some(generation) = self.legacy_arrivals.get(&key) {
            return Ok(*generation);
        }
        if self.legacy_arrivals.len() >= 1024 {
            return Err(format!(
                "{}legacy receive capacity reached",
                crate::protocol::TEMPORARY_PRESSURE_MARKER
            ));
        }
        self.legacy_arrivals.insert(key, self.clipboard_generation);
        Ok(self.clipboard_generation)
    }
    pub(crate) fn settle_legacy_arrival(&mut self, source: &str, id: MessageId) {
        self.legacy_arrivals.remove(&(source.to_string(), id));
    }
    pub async fn restore_text_shared(
        engine: std::sync::Arc<tokio::sync::Mutex<Self>>,
        text: String,
    ) -> Result<(), String> {
        engine.lock().await.supersede_file_clipboard();
        let publication: super::AdmissionHold =
            std::sync::Arc::new(super::publication_permit().await);
        let (prepared, port) = {
            let mut state = engine.lock().await;
            (state.prepare_incoming_text(&text)?, state.clipboard_port())
        };
        let result = port
            .ok_or("Clipboard unavailable")?
            .write_text_admitted(text, vec![publication])
            .await;
        engine.lock().await.finish_incoming_text(prepared, result)
    }
    pub async fn restore_image_shared(
        engine: std::sync::Arc<tokio::sync::Mutex<Self>>,
        bytes: Vec<u8>,
    ) -> Result<(), String> {
        engine.lock().await.supersede_file_clipboard();
        let publication: super::AdmissionHold =
            std::sync::Arc::new(super::publication_permit().await);
        let (prepared, port) = tokio::task::spawn_blocking({
            let engine = engine.clone();
            move || {
                let mut state = engine.blocking_lock();
                Ok::<_, String>((
                    state.prepare_incoming_image(&bytes)?,
                    state.clipboard_port(),
                ))
            }
        })
        .await
        .map_err(|_| "Restore preparation failed")??;
        let result = port
            .ok_or("Clipboard unavailable")?
            .write_image_admitted(
                prepared.width,
                prepared.height,
                prepared.rgba.clone(),
                vec![publication],
            )
            .await;
        engine
            .lock()
            .await
            .finish_incoming_image(prepared, result)
            .map(|_| ())
    }

    pub fn supports_native_snapshot(
        &self,
        snapshot: &crate::protocol::native_clipboard::NativeSnapshot,
    ) -> bool {
        self.platform
            .as_ref()
            .is_some_and(|platform| platform.supports_native_snapshot(snapshot))
    }
    pub fn restore_target(&self) -> crate::composite::restore::RestoreTarget {
        self.platform
            .as_ref()
            .map_or(crate::composite::restore::RestoreTarget::Adapter, |p| {
                p.restore_target()
            })
    }
    pub fn restore_plan(
        &mut self,
        plan: &crate::composite::restore::RestorePlan,
    ) -> Result<crate::composite::NativePublish, String> {
        self.supersede_file_clipboard();
        let outcome =
            self.platform()?
                .write_restore_plan(plan, None, std::sync::Arc::new(false.into()))?;
        Ok(self.finish_composite_publish(outcome))
    }
    pub fn clipboard_generation(&self) -> u64 {
        self.clipboard_generation
    }
    pub fn clipboard_revision(&self) -> Option<i64> {
        self.platform.as_ref().and_then(|p| p.clipboard_revision())
    }
    /// Phase 1 of a remote composite publish: validate, check the generation and
    /// capture the port — all under the engine lock, with **no system I/O**.
    ///
    /// `Ok(None)` means the race was lost before anything was written, which is a
    /// definite no-write outcome rather than a failure.
    pub fn prepare_composite_publish(
        &mut self,
        bundle: &crate::composite::PortableBundle,
        generation: u64,
        revision: Option<i64>,
        authorized: bool,
    ) -> Result<CompositePublishGate, String> {
        crate::composite::validate_for_publish(bundle).map_err(|e| e.to_string())?;
        self.prepare_composite_context(
            std::sync::Arc::new(bundle.clone()),
            generation,
            revision,
            authorized,
        )
    }

    // Receiver has already completed native preparation on the blocking pool.
    pub(crate) fn prepare_composite_context(
        &mut self,
        bundle: std::sync::Arc<crate::composite::PortableBundle>,
        generation: u64,
        revision: Option<i64>,
        authorized: bool,
    ) -> Result<CompositePublishGate, String> {
        // Authorization is re-checked here, under the same lock as the generation
        // check, so nothing can be revoked between the two (R4-05).
        if !authorized {
            return Ok(CompositePublishGate::Unauthorized);
        }
        if self.clipboard_generation != generation || revision.is_none() {
            return Ok(CompositePublishGate::Superseded);
        }
        Ok(CompositePublishGate::Ready(PreparedComposite {
            bundle,
            revision,
        }))
    }

    /// Phase 3: apply the outcome of the port call, with a short lock.
    ///
    /// Only a write that actually landed supersedes the file clipboard. An unknown
    /// outcome is neither a landed write nor a no-op.
    pub fn finish_composite_publish(
        &mut self,
        outcome: crate::composite::NativePublish,
    ) -> crate::composite::NativePublish {
        if outcome.changed_the_clipboard() {
            self.supersede_file_clipboard();
        }
        outcome
    }

    /// Restore a native shape, returning the typed outcome (K2).
    ///
    /// A platform that cannot represent the shape reports `NoEffect`, which the
    /// caller reads as "not applicable here" rather than as a failure — that is what
    /// keeps `Partial` and `Unknown` from being flattened into a generic error the UI
    /// then presents as success.
    pub fn restore_native(
        &mut self,
        snapshot: &crate::protocol::native_clipboard::NativeSnapshot,
    ) -> Result<crate::composite::NativePublish, String> {
        if !self.platform()?.supports_native_snapshot(snapshot) {
            return Ok(crate::composite::NativePublish::NoEffect {
                detail: "this platform cannot represent the native shape".into(),
            });
        }
        snapshot.validate().map_err(|e| e.to_string())?;
        self.supersede_file_clipboard();
        self.platform()?.write_native_snapshot(snapshot)
    }
    /// Restore a portable bundle, returning the typed outcome (K2).
    pub fn restore_composite(
        &mut self,
        bytes: &[u8],
    ) -> Result<crate::composite::NativePublish, String> {
        let bundle = crate::composite::PortableBundle::decode(bytes).map_err(|e| e.to_string())?;
        self.supersede_file_clipboard();
        crate::composite::validate_for_publish(&bundle).map_err(|e| e.to_string())?;
        self.platform()?.write_composite(&bundle, None)
    }
    pub fn new() -> Self {
        SyncEngine {
            seen_messages: HashMap::new(),
            seen_message_order: VecDeque::new(),
            applied_events: HashMap::new(),
            applied_event_order: VecDeque::new(),
            active_receives: HashMap::new(),
            receive_operation_locks: HashMap::new(),
            batch_operation_locks: HashMap::new(),
            pending_receives: HashMap::new(),
            inflight_receives: HashSet::new(),
            completed_transfers: HashMap::new(),
            incoming_batches: HashMap::new(),
            cancelled_batches: HashMap::new(),
            completed_batches: HashMap::new(),
            completed_batch_manifests: HashMap::new(),
            peer_device_ids: HashMap::new(),
            receive_epochs: HashMap::new(),
            clipboard_generation: 0,
            legacy_arrivals: HashMap::new(),
            shadow_filter: ShadowFilter::new(),
            image_shadow_filter: ShadowFilter::new(),
            platform: None,
        }
    }

    pub fn set_platform(&mut self, platform: Arc<dyn SyncPlatform>) {
        self.platform = Some(platform);
    }

    /// Bind the authenticated public-key fingerprint to the display hostname
    /// used by the in-memory receive maps. Hostnames can change; the
    /// fingerprint is the durable identity used by receive receipts.
    pub fn set_peer_device_identity(&mut self, source: &str, device_id: String) {
        self.peer_device_ids.insert(source.to_string(), device_id);
    }

    // ── Text ─────────────────────────────────────────────────────

    /// Handle incoming text from a remote peer.
    ///
    /// Shadow-filter → write system clipboard.
    pub async fn handle_incoming_text(&mut self, text: &str, source: String) -> Result<(), String> {
        self.restore_text(text)?;
        info!(
            "Clipboard ← text from peer {} ({} chars)",
            source,
            text.len()
        );
        Ok(())
    }

    pub fn restore_text(&mut self, text: &str) -> Result<(), String> {
        let prepared = self.prepare_incoming_text(text)?;
        let outcome = self.platform()?.write_text(text);
        self.finish_incoming_text(prepared, outcome)
    }

    /// Phase 1 of an inbound text activation: everything that must happen under the
    /// engine lock, and **no system I/O**.
    ///
    /// The echo guard is registered *before* the irreversible native write — the
    /// discipline the legacy path already had — so a failed or unknown outcome can
    /// roll it back rather than leaving the content suppressed forever.
    /// An `Arc` handle to the platform, so a caller can perform the native call
    /// **after** releasing the engine lock (R3-01).
    ///
    /// Prefer [`Self::clipboard_port`], which also moves the blocking call off the
    /// async worker.
    pub fn platform_handle(&self) -> Option<std::sync::Arc<dyn crate::sync::SyncPlatform>> {
        self.platform.clone()
    }

    /// The asynchronous port for this engine, if a platform is attached.
    pub fn clipboard_port(&self) -> Option<std::sync::Arc<dyn crate::sync::ClipboardPort>> {
        self.platform.clone().map(|platform| {
            std::sync::Arc::new(crate::sync::BlockingPlatformPort::new(platform))
                as std::sync::Arc<dyn crate::sync::ClipboardPort>
        })
    }

    pub fn prepare_incoming_text(&mut self, text: &str) -> Result<PreparedText, String> {
        let hash = blake3::hash(text.as_bytes()).to_hex().to_string();
        self.shadow_filter.insert(hash.clone());
        Ok(PreparedText { hash })
    }

    /// Phase 2: commit or roll back once the port call has completed.
    ///
    /// Called with the engine lock held again, but only for a short bookkeeping step —
    /// never across the native call (R3-01).
    pub fn finish_incoming_text(
        &mut self,
        prepared: PreparedText,
        outcome: Result<(), String>,
    ) -> Result<(), String> {
        if let Err(error) = outcome {
            self.shadow_filter.remove(&prepared.hash);
            if !crate::protocol::failure_is_proven_no_effect(&error) {
                self.supersede_file_clipboard();
            }
            return Err(error);
        }
        self.supersede_file_clipboard();
        Ok(())
    }

    // ── Images ───────────────────────────────────────────────────

    /// Handle incoming image from a remote peer.
    ///
    /// The packed format from clipboard.rs is [width:4 LE][height:4 LE][rgba].
    /// We reconstruct a `tauri::image::Image` and write it to the clipboard.
    pub async fn handle_incoming_image(
        &mut self,
        image_data: &[u8],
        source: String,
    ) -> Result<(), String> {
        let (width, height) = self.restore_image(image_data)?;
        info!(
            "Clipboard ← image from peer {} ({}×{} {} bytes)",
            source,
            width,
            height,
            image_data.len()
        );
        Ok(())
    }

    pub fn restore_image(&mut self, image_data: &[u8]) -> Result<(u32, u32), String> {
        let prepared = self.prepare_incoming_image(image_data)?;
        let outcome =
            self.platform()?
                .write_image(prepared.width, prepared.height, prepared.rgba.as_slice());
        self.finish_incoming_image(prepared, outcome)
    }

    /// Phase 1 of an inbound image activation: validate, register the echo guard, and
    /// hand back the payload — with **no system I/O** (R3-01).
    pub fn prepare_incoming_image(&mut self, image_data: &[u8]) -> Result<PreparedImage, String> {
        let image = crate::protocol::PackedImage::try_from(image_data)
            .map_err(|error| format!("Invalid packed image: {error}"))?;
        let hash = blake3::hash(image_data).to_hex().to_string();
        self.image_shadow_filter.insert(hash.clone());
        Ok(PreparedImage {
            hash,
            width: image.width,
            height: image.height,
            rgba: image.rgba.to_vec(),
        })
    }

    /// Phase 2: commit or roll back once the port call has completed.
    pub fn finish_incoming_image(
        &mut self,
        prepared: PreparedImage,
        outcome: Result<(), String>,
    ) -> Result<(u32, u32), String> {
        if let Err(error) = outcome {
            self.image_shadow_filter.remove(&prepared.hash);
            if !crate::protocol::failure_is_proven_no_effect(&error) {
                self.supersede_file_clipboard();
            }
            return Err(error);
        }
        self.supersede_file_clipboard();
        Ok((prepared.width, prepared.height))
    }

    // ── Files ────────────────────────────────────────────────────
}
