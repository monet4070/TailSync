use super::*;

impl SyncEngine {
    pub fn new() -> Self {
        SyncEngine {
            seen_messages: HashMap::new(),
            seen_message_order: VecDeque::new(),
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
        self.supersede_file_clipboard();
        self.restore_text(text)?;
        info!(
            "Clipboard ← text from peer {} ({} chars)",
            source,
            text.len()
        );
        Ok(())
    }

    pub fn restore_text(&mut self, text: &str) -> Result<(), String> {
        let hash = blake3::hash(text.as_bytes()).to_hex().to_string();
        self.shadow_filter.insert(hash.clone());
        if let Err(error) = self.platform()?.write_text(text) {
            self.shadow_filter.remove(&hash);
            return Err(error);
        }
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
        self.supersede_file_clipboard();
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
        let image = crate::protocol::PackedImage::try_from(image_data)
            .map_err(|error| format!("Invalid packed image: {error}"))?;
        let hash = blake3::hash(image_data).to_hex().to_string();
        self.image_shadow_filter.insert(hash.clone());
        if let Err(error) = self
            .platform()?
            .write_image(image.width, image.height, image.rgba)
        {
            self.image_shadow_filter.remove(&hash);
            return Err(error);
        }
        Ok((image.width, image.height))
    }

    // ── Files ────────────────────────────────────────────────────
}
