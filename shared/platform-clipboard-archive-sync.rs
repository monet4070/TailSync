fn enqueue_automatic_archive(
    queue: &crate::automatic_images::AutomaticImageArchive,
    ticket: tailsync_runtime::automatic_image_archive::CaptureTicket,
    origin: tailsync_core::composite::origin::Origin,
    commit: tailsync_runtime::history::ArchiveCommit,
) {
    if queue.enqueue_captured(ticket, origin, commit.id, commit.guard)
        == tailsync_runtime::automatic_image_archive::EnqueueResult::Full
    {
        warn!("Automatic image archive queue full; images remain available for manual saving");
    }
}

async fn broadcast_composite_to_peers(
    capture: CaptureBroadcastGate,
    pool: &Arc<Mutex<network::ConnectionPool>>,
    settings: &Arc<Mutex<crypto::Settings>>,
    database: Arc<Mutex<db::HistoryDB>>,
    binding: Option<tailsync_runtime::history::ArchiveCommit>,
    queue: &crate::automatic_images::AutomaticImageArchive,
    payload: Arc<[u8]>,
) -> Option<tailsync_runtime::history::ArchiveCommit> {
    if !settings.lock().await.sync_enabled {
        return None;
    }
    let event =
        match network::SharedEvent::encode_shared(Command::CompositePayload, payload.clone()) {
            Ok(event) => event,
            Err(error) => {
                warn!("Skipping composite broadcast: {error}");
                tailsync_core::sync_warning::record_capture_not_shared();
                return None;
            }
        };
    let bound = match binding {
        Some(binding) => match tailsync_runtime::archive_sync::bind_local_event(
            database, binding, &event, payload,
        )
        .await
        {
            Ok(bound) => bound,
            Err(error) => {
                warn!("Archive event binding skipped: {error}");
                None
            }
        },
        None => None,
    };
    broadcast_shared_to_peers(Some(capture), pool, settings, event, true).await;
    if let Some(commit) = &bound {
        if commit.has_saved_archive() {
            queue.committed(commit.clone());
        }
    }
    bound
}
