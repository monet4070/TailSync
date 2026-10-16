use super::*;

/// Start the local API. The production macOS build uses a private Unix-domain
/// socket so the capability token is never exposed to other loopback clients
/// and the endpoint is not reachable over TCP. Non-macOS builds retain the
/// TCP listener for the shared test harness and development binaries.
pub async fn start(
    state: Arc<ApiState>,
    shutdown: watch::Receiver<bool>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    #[cfg(target_os = "macos")]
    {
        start_unix(state, shutdown).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        start_tcp(state, shutdown).await
    }
}

#[cfg(not(target_os = "macos"))]
async fn start_tcp(
    state: Arc<ApiState>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let addr = format!("127.0.0.1:{}", API_PORT);
    let Some(listener) =
        bind_api_listener(addr.parse()?, &mut shutdown, API_BIND_RETRY_DELAY).await
    else {
        return Ok(());
    };
    let connections = Arc::new(Semaphore::new(API_MAX_CONNECTIONS));
    let mut handlers = tokio::task::JoinSet::new();
    let mut reap_tick = tokio::time::interval(Duration::from_secs(1));
    info!("API server listening on {}", addr);

    loop {
        let accepted = tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
                continue;
            }
            accepted = listener.accept() => accepted,
            _ = reap_tick.tick() => {
                reap_finished_handlers(&mut handlers);
                continue;
            }
        };
        let (stream, _) = match accepted {
            Ok(accepted) => accepted,
            Err(error) => {
                warn!("API accept failed; retrying: {error}");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        let permit = match connections.clone().try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => {
                warn!("API connection limit reached; rejecting connection");
                drop(stream);
                continue;
            }
        };
        let st = state.clone();
        handlers.spawn(serve_connection(stream, st, permit));
    }
    drain_handlers(&mut handlers).await;
    Ok(())
}

#[cfg(target_os = "macos")]
async fn start_unix(
    state: Arc<ApiState>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let socket_path = api_socket_path();
    let Some(listener) =
        bind_unix_listener(&socket_path, &mut shutdown, API_BIND_RETRY_DELAY).await
    else {
        return Ok(());
    };
    let expected_parent_pid = std::env::var("TAILSYNC_PARENT_PID")
        .ok()
        .and_then(|value| value.parse::<libc::pid_t>().ok());
    if expected_parent_pid.is_none() {
        warn!("TAILSYNC_PARENT_PID is missing; API socket peer-PID verification is unavailable");
    }
    let connections = Arc::new(Semaphore::new(API_MAX_CONNECTIONS));
    let mut handlers = tokio::task::JoinSet::new();
    let mut reap_tick = tokio::time::interval(Duration::from_secs(1));
    info!("API server listening on unix://{}", socket_path.display());

    loop {
        let accepted = tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
                continue;
            }
            accepted = listener.accept() => accepted,
            _ = reap_tick.tick() => {
                reap_finished_handlers(&mut handlers);
                continue;
            }
        };
        let (stream, _) = match accepted {
            Ok(accepted) => accepted,
            Err(error) => {
                warn!("API accept failed; retrying: {error}");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        if let Some(expected_pid) = expected_parent_pid {
            if let Err(error) = verify_unix_peer_pid(&stream, expected_pid) {
                warn!("Rejecting API connection with unexpected peer process: {error}");
                drop(stream);
                continue;
            }
        }
        let permit = match connections.clone().try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => {
                warn!("API connection limit reached; rejecting connection");
                drop(stream);
                continue;
            }
        };
        handlers.spawn(serve_connection(stream, state.clone(), permit));
    }
    info!("API server stopped accepting connections");
    drain_handlers(&mut handlers).await;
    // Remove only the socket path selected by this process. The parent
    // directory is retained so its 0700 boundary remains stable across runs.
    let _ = std::fs::remove_file(&socket_path);
    Ok(())
}

#[cfg(target_os = "macos")]
fn verify_unix_peer_pid(
    stream: &tokio::net::UnixStream,
    expected_pid: libc::pid_t,
) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;

    let mut peer_pid: libc::pid_t = 0;
    let mut length = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_LOCAL,
            libc::LOCAL_PEERPID,
            (&mut peer_pid as *mut libc::pid_t).cast(),
            &mut length,
        )
    };
    if result != 0 {
        return Err(std::io::Error::last_os_error());
    }
    if peer_pid != expected_pid {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("peer pid {peer_pid} does not match parent pid {expected_pid}"),
        ));
    }
    Ok(())
}

async fn serve_connection<S>(
    stream: S,
    state: Arc<ApiState>,
    _permit: tokio::sync::OwnedSemaphorePermit,
) where
    S: AsyncRead + AsyncWriteExt + Unpin + Send + 'static,
{
    let (mut reader, mut writer) = tokio::io::split(stream);
    let req = match read_request(&mut reader).await {
        Ok(req) => req,
        Err(error) => {
            let _ = write_response(
                &mut writer,
                false,
                None,
                error,
                "request",
                false,
                API_WRITE_TIMEOUT,
            )
            .await;
            return;
        }
    };
    let command = req.cmd.clone();
    let stable_errors =
        req.error_schema_version == Some(tailsync_runtime::contracts::STABLE_ERROR_SCHEMA_VERSION);
    if !state.token.matches(req.token.as_deref()) {
        let _ = write_response(
            &mut writer,
            false,
            None,
            "unauthorized",
            &command,
            stable_errors,
            API_WRITE_TIMEOUT,
        )
        .await;
        return;
    }

    let should_shutdown = req.cmd == "quit";
    if req
        .request_id
        .as_ref()
        .is_some_and(|id| id.is_empty() || id.len() > 128)
    {
        let _ = write_response(
            &mut writer,
            false,
            None,
            "invalid request_id",
            &command,
            stable_errors,
            API_WRITE_TIMEOUT,
        )
        .await;
        return;
    }
    if req.cmd == "get_preview_binary" {
        let Some(response) = until_disconnect(
            &mut reader,
            crate::api::preview_binary_response(&state, req.id, req.batch_id, req.request_id),
        )
        .await
        else {
            return;
        };
        match response {
            Ok(bytes) => {
                let _ = write_binary_response(
                    &mut writer,
                    &bytes,
                    response_timeout_for_command(&req.cmd),
                )
                .await;
            }
            Err(error) => {
                let _ = write_response(
                    &mut writer,
                    false,
                    None,
                    error,
                    &command,
                    stable_errors,
                    response_timeout_for_command(&req.cmd),
                )
                .await;
            }
        }
        return;
    }
    if req.cmd == "get_document_preview" {
        let Some(response) = until_disconnect(
            &mut reader,
            crate::api::document_preview_response(&state, req.document_request, req.request_id),
        )
        .await
        else {
            return;
        };
        match response {
            Ok(preview) => {
                let _ = write_binary_response(
                    &mut writer,
                    preview.bytes(),
                    response_timeout_for_command(&req.cmd),
                )
                .await;
                drop(preview);
            }
            Err(error) => {
                let _ = write_response(
                    &mut writer,
                    false,
                    None,
                    error,
                    &command,
                    stable_errors,
                    response_timeout_for_command(&req.cmd),
                )
                .await;
            }
        }
        return;
    }
    // A history preview can contain up to 64 MiB of decrypted bytes,
    // expanding to roughly 90 MiB when wrapped in Base64/JSON. Keep the
    // normal five-second API timeout for every other command but allow this
    // bounded response enough time to drain locally.
    let response_timeout = response_timeout_for_command(&req.cmd);
    // Opt-in read cancellation preserves legacy clients that half-close their
    // write side after a request. Mutations always finish their commit/hooks.
    // `wait_runtime_snapshot` holds a long poll, so cancelling it on disconnect
    // returns the connection permit promptly instead of waiting out the poll.
    let cancel_on_disconnect = req.request_id.is_some()
        && matches!(
            req.cmd.as_str(),
            "get_history"
                | "get_history_page"
                | "get_preview_data"
                | "wait_runtime_snapshot"
                | "archive_entry_images"
                | "source_context_import"
        );
    let response = if cancel_on_disconnect {
        let Some(response) = until_disconnect(&mut reader, handle_cmd(req, &state)).await else {
            return;
        };
        response
    } else {
        handle_cmd(req, &state).await
    };
    let sent = write_response(
        &mut writer,
        response.ok,
        response.data,
        response.error.unwrap_or_default(),
        &command,
        stable_errors,
        response_timeout,
    )
    .await;
    if should_shutdown && sent.is_ok() {
        graceful_shutdown(&state).await;
    }
}

/// One request per connection: EOF, read failure, or further input ends an
/// opted-in read. Dropping its future propagates to the runtime read worker.
async fn until_disconnect<R: AsyncRead + Unpin, F: std::future::Future>(
    reader: &mut R,
    response: F,
) -> Option<F::Output> {
    use tokio::io::AsyncReadExt;
    let mut unexpected = [0_u8; 1];
    tokio::select! {
        biased;
        _ = reader.read(&mut unexpected) => None,
        response = response => Some(response),
    }
}

#[cfg(test)]
mod cancellation_tests {
    use super::*;

    pub(super) fn test_state() -> Arc<ApiState> {
        let settings = Arc::new(Mutex::new(crypto::Settings::default()));
        let identity = Arc::new(DeviceIdentity::generate_for_test());
        let database = Arc::new(Mutex::new(db::HistoryDB::new_unavailable().unwrap()));
        Arc::new(ApiState {
            documents: Default::default(),
            db: database.clone(),
            image_archive: Arc::new(crate::automatic_images::AutomaticImageArchive::new(
                database,
                false,
                Arc::new(tailsync_core::composite::resource_archive::ReqwestTransport::new()),
                Arc::new(crate::api::bump_clipboard_version),
            )),
            sync_engine: Arc::new(Mutex::new(sync::SyncEngine::new())),
            pool: Arc::new(Mutex::new(network::ConnectionPool::new(
                identity.clone(),
                settings.clone(),
            ))),
            pairing: crate::pairing::PairingManager::new(settings.clone(), identity.clone()),
            remote_invites: Arc::new(crate::pairing::RemotePairingInviteManager::default()),
            identity,
            settings,
            token: ApiToken::parse(&"a".repeat(64)).unwrap(),
            shutdown: watch::channel(false).0,
            pending_storage_cleanup: Arc::new(Mutex::new(None)),
            imports: Mutex::new(ImportRegistry::default()),
        })
    }

    #[cfg(target_os = "macos")]
    #[tokio::test(flavor = "current_thread")]
    async fn authenticated_runtime_route_keeps_notification_window_consistent_during_overflow() {
        {
            let mut buffer = RUNTIME_NOTIFICATIONS.lock().unwrap();
            *buffer = RuntimeNotificationBuffer::default();
            buffer.push("info", "first".into());
        }
        NOTIFICATION_SNAPSHOT_TEST_HOOK.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(|| {
                let mut buffer = RUNTIME_NOTIFICATIONS.lock().unwrap();
                for _ in 0..MAX_RUNTIME_NOTIFICATIONS {
                    buffer.push("info", "overflow".into());
                }
            }));
        });
        let permits = Arc::new(Semaphore::new(1));
        let permit = permits.clone().acquire_owned().await.unwrap();
        let (server, mut client) = tokio::net::UnixStream::pair().unwrap();
        let task = tokio::spawn(serve_connection(server, test_state(), permit));
        let request = serde_json::json!({"cmd":"wait_runtime_snapshot", "token":"a".repeat(64),
            "request_id":"overflow-route", "since_revision":0, "since_notification_id":0, "wait_ms":0});
        let mut encoded = serde_json::to_vec(&request).unwrap();
        encoded.push(b'\n');
        client.write_all(&encoded).await.unwrap();
        let mut line = String::new();
        timeout(
            Duration::from_secs(2),
            BufReader::new(client).read_line(&mut line),
        )
        .await
        .unwrap()
        .unwrap();
        task.await.unwrap();
        let response: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        let data = &response["data"];
        assert_eq!(
            data["notifications"][0]["id"],
            data["notification_earliest_available_id"]
        );
        assert_eq!(
            data["notification_dropped_total"],
            data["notification_earliest_available_id"].as_u64().unwrap() - 1
        );
        assert_eq!(
            RUNTIME_NOTIFICATIONS
                .lock()
                .unwrap()
                .earliest_available_id(),
            2,
            "the production route must have crossed the injected overflow"
        );
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn authenticated_long_poll_disconnect_returns_connection_permit() {
        let state = test_state();
        let permits = Arc::new(Semaphore::new(1));
        let permit = permits.clone().acquire_owned().await.unwrap();
        let (server, mut client) = tokio::net::UnixStream::pair().unwrap();
        let baseline = RUNTIME_REVISION.receiver_count();
        let task = tokio::spawn(serve_connection(server, state, permit));
        let request = serde_json::json!({"cmd": "wait_runtime_snapshot", "token": "a".repeat(64),
            "request_id": "real-long-poll", "since_revision": get_runtime_revision(), "wait_ms": 15000});
        let mut encoded = serde_json::to_vec(&request).unwrap();
        encoded.push(b'\n');
        client.write_all(&encoded).await.unwrap();
        // Prove the real authenticated route entered its long poll before closing.
        timeout(Duration::from_secs(1), async {
            while RUNTIME_REVISION.receiver_count() <= baseline {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("route did not enter long poll");
        assert_eq!(permits.available_permits(), 0);
        drop(client);
        timeout(Duration::from_secs(1), task)
            .await
            .expect("disconnect left route waiting for its poll timeout")
            .unwrap();
        assert!(
            permits.try_acquire().is_ok(),
            "API connection permit was leaked"
        );
    }

    #[tokio::test]
    async fn disconnected_client_drops_pending_response() {
        let (mut server, client) = tokio::io::duplex(64);
        let cancelled = tailsync_core::cancellation::Cancellation::default();
        let observed = cancelled.clone();
        struct OnDrop(tailsync_core::cancellation::Cancellation);
        impl Drop for OnDrop {
            fn drop(&mut self) {
                self.0.cancel();
            }
        }
        let guard = OnDrop(cancelled);
        let task = tokio::spawn(async move {
            until_disconnect(&mut server, async move {
                let _guard = guard;
                std::future::pending::<()>().await;
            })
            .await
        });
        drop(client);
        assert!(timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap()
            .is_none());
        assert!(observed.is_cancelled());
    }

    /// The paged history read is a long database read. A client that
    /// disconnects while it is parked on the database lock must end the route
    /// and return the connection permit instead of waiting for the read.
    #[cfg(target_os = "macos")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn paged_history_disconnect_ends_the_route_and_returns_the_permit() {
        use tokio::io::AsyncReadExt;
        let state = test_state();
        let permits = Arc::new(Semaphore::new(1));
        let permit = permits.clone().acquire_owned().await.unwrap();
        let (server, mut client) = tokio::net::UnixStream::pair().unwrap();
        // Holding the database keeps the real paged route inside its read.
        let held = state.db.lock().await;
        let task = tokio::spawn(serve_connection(server, state.clone(), permit));
        let request = serde_json::json!({"cmd": "get_history_page", "token": "a".repeat(64),
            "request_id": "paged-read", "limit": 30, "offset": 0});
        let mut encoded = serde_json::to_vec(&request).unwrap();
        encoded.push(b'\n');
        client.write_all(&encoded).await.unwrap();
        // A rejected or finished request would already have answered, so
        // silence here proves the route consumed the request and is parked on
        // the database instead of failing before it.
        let mut probe = [0_u8; 1];
        assert!(
            timeout(Duration::from_millis(250), client.read(&mut probe))
                .await
                .is_err(),
            "the paged route answered while the database lock was held"
        );
        assert_eq!(permits.available_permits(), 0);
        drop(client);
        timeout(Duration::from_secs(1), task)
            .await
            .expect("disconnect left the paged route waiting on its database read")
            .unwrap();
        assert!(
            permits.try_acquire().is_ok(),
            "API connection permit was leaked"
        );
        drop(held);
    }

    /// Legacy clients half-close the write side after sending a request and
    /// never send `request_id`. Their read must still finish and answer.
    #[cfg(target_os = "macos")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn legacy_paged_history_half_close_still_answers() {
        use tokio::io::AsyncReadExt;
        let state = test_state();
        let permits = Arc::new(Semaphore::new(1));
        let permit = permits.clone().acquire_owned().await.unwrap();
        let (server, mut client) = tokio::net::UnixStream::pair().unwrap();
        let held = state.db.lock().await;
        let task = tokio::spawn(serve_connection(server, state.clone(), permit));
        let request = serde_json::json!({"cmd": "get_history_page", "token": "a".repeat(64),
            "limit": 30, "offset": 0});
        let mut encoded = serde_json::to_vec(&request).unwrap();
        encoded.push(b'\n');
        client.write_all(&encoded).await.unwrap();
        client.shutdown().await.unwrap();
        let mut probe = [0_u8; 1];
        assert!(
            timeout(Duration::from_millis(250), client.read(&mut probe))
                .await
                .is_err(),
            "the half-close cancelled a read that had not finished yet"
        );
        drop(held);
        let mut line = String::new();
        timeout(
            Duration::from_secs(2),
            BufReader::new(client).read_line(&mut line),
        )
        .await
        .expect("a legacy half-close never received the paged response")
        .unwrap();
        let response: Value = serde_json::from_str(&line).unwrap();
        assert!(
            response.get("ok").is_some(),
            "unexpected paged response: {response}"
        );
        task.await.unwrap();
        assert!(permits.try_acquire().is_ok());
    }

    /// Mutations are never opted into disconnect cancellation: a favorite
    /// commit with `request_id` must still reach the database after the client
    /// is gone.
    #[cfg(target_os = "macos")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn mutation_with_request_id_commits_after_the_client_disconnects() {
        use tokio::io::AsyncReadExt;
        let state = test_state();
        let id = {
            let mut database = state.db.lock().await;
            database
                .add_text("mutation survives disconnect", "local")
                .unwrap();
            database.get_all(None, None, 1, 0).unwrap()[0].id
        };
        let permits = Arc::new(Semaphore::new(1));
        let permit = permits.clone().acquire_owned().await.unwrap();
        let (server, mut client) = tokio::net::UnixStream::pair().unwrap();
        let held = state.db.lock().await;
        let task = tokio::spawn(serve_connection(server, state.clone(), permit));
        let request = serde_json::json!({"cmd": "set_history_favorite", "token": "a".repeat(64),
            "request_id": "favorite-mutation", "id": id, "favorite": true});
        let mut encoded = serde_json::to_vec(&request).unwrap();
        encoded.push(b'\n');
        client.write_all(&encoded).await.unwrap();
        let mut probe = [0_u8; 1];
        assert!(
            timeout(Duration::from_millis(250), client.read(&mut probe))
                .await
                .is_err(),
            "the mutation answered while the database lock was held"
        );
        drop(client);
        drop(held);
        let mut pinned = false;
        timeout(Duration::from_secs(2), async {
            while !pinned {
                pinned = state.db.lock().await.get_all(None, None, 1, 0).unwrap()[0].pinned;
                if !pinned {
                    tokio::task::yield_now().await;
                }
            }
        })
        .await
        .expect("disconnect cancelled a mutation that must still commit");
        assert!(pinned, "the favorite commit did not reach the database");
        timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap();
    }
}

async fn drain_handlers(handlers: &mut tokio::task::JoinSet<()>) {
    if timeout(Duration::from_secs(2), async {
        while handlers.join_next().await.is_some() {}
    })
    .await
    .is_err()
    {
        warn!("Timed out while draining local API requests");
        handlers.abort_all();
        while handlers.join_next().await.is_some() {}
    }
}

fn reap_finished_handlers(handlers: &mut tokio::task::JoinSet<()>) {
    while let Some(result) = handlers.try_join_next() {
        if let Err(error) = result {
            warn!("Local API request handler ended unexpectedly: {error}");
        }
    }
}

#[cfg(target_os = "macos")]
fn api_socket_path() -> std::path::PathBuf {
    if let Ok(path) = std::env::var("TAILSYNC_API_SOCKET") {
        return std::path::PathBuf::from(path);
    }
    directories::BaseDirs::new()
        .map(|dirs| dirs.data_dir().join("TailSync"))
        .unwrap_or_else(|| std::env::temp_dir().join("TailSync"))
        .join("tailsyncd.sock")
}

#[cfg(target_os = "macos")]
async fn bind_unix_listener(
    path: &std::path::Path,
    shutdown: &mut watch::Receiver<bool>,
    retry_delay: Duration,
) -> Option<tokio::net::UnixListener> {
    loop {
        match bind_private_unix_listener(path) {
            Ok(listener) => return Some(listener),
            Err(error) => warn!("API Unix socket bind failed; retrying: {error}"),
        }

        tokio::select! {
            _ = tokio::time::sleep(retry_delay) => {}
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    return None;
                }
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn bind_private_unix_listener(path: &std::path::Path) -> std::io::Result<tokio::net::UnixListener> {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};

    let bytes = path.as_os_str().as_bytes();
    if !path.is_absolute() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "API socket path must be absolute",
        ));
    }
    // sockaddr_un.sun_path is 104 bytes on macOS, including the trailing NUL.
    if bytes.is_empty() || bytes.len() >= 104 || bytes.contains(&0) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "API socket path is too long or contains NUL",
        ));
    }
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "API socket has no parent")
    })?;
    std::fs::create_dir_all(parent)?;
    let metadata = std::fs::metadata(parent)?;
    if metadata.uid() != unsafe { libc::geteuid() } {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "API socket directory is not owned by the current user",
        ));
    }
    std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;

    if let Ok(existing) = std::fs::symlink_metadata(path) {
        if !existing.file_type().is_socket() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "API socket path is not a socket",
            ));
        }
        if existing.uid() != unsafe { libc::geteuid() } {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "API socket is not owned by the current user",
            ));
        }
        std::fs::remove_file(path)?;
    }

    let listener = tokio::net::UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

#[cfg(any(test, not(target_os = "macos")))]
pub(super) async fn bind_api_listener(
    address: std::net::SocketAddr,
    shutdown: &mut watch::Receiver<bool>,
    retry_delay: Duration,
) -> Option<tokio::net::TcpListener> {
    loop {
        match network::bind_tcp_listener(address) {
            Ok(listener) => return Some(listener),
            Err(error) => warn!("API listener bind failed; retrying: {error}"),
        }

        tokio::select! {
            _ = tokio::time::sleep(retry_delay) => {}
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    return None;
                }
            }
        }
    }
}

async fn read_request(reader: impl AsyncRead + Unpin) -> Result<Request, String> {
    read_request_with_limits(reader, MAX_API_LINE, API_READ_TIMEOUT).await
}

pub(super) async fn read_request_with_limits(
    reader: impl AsyncRead + Unpin,
    max_line: usize,
    read_timeout: Duration,
) -> Result<Request, String> {
    let mut limited = BufReader::new(reader).take((max_line + 1) as u64);
    let mut bytes = Vec::new();
    let count = timeout(read_timeout, limited.read_until(b'\n', &mut bytes))
        .await
        .map_err(|_| "request read timed out".to_string())?
        .map_err(|error| format!("request read failed: {error}"))?;
    if bytes.len() > max_line {
        return Err(format!("request exceeds {max_line} byte limit"));
    }
    if count == 0 || !bytes.ends_with(b"\n") {
        return Err("incomplete request".to_string());
    }
    bytes.pop();
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    serde_json::from_slice(&bytes).map_err(|error| format!("invalid request JSON: {error}"))
}

async fn write_response(
    writer: &mut (impl AsyncWriteExt + Unpin),
    ok: bool,
    data: Option<Value>,
    error: impl Into<ResponseError>,
    command: &str,
    stable_errors: bool,
    timeout_duration: Duration,
) -> Result<(), String> {
    timeout(
        timeout_duration,
        send_json(writer, ok, data, error, command, stable_errors),
    )
    .await
    .map_err(|_| "response write timed out".to_string())?
    .map_err(|error| error.to_string())
}

async fn write_binary_response(
    writer: &mut (impl AsyncWriteExt + Unpin),
    bytes: &[u8],
    timeout_duration: Duration,
) -> Result<(), String> {
    timeout(timeout_duration, async {
        writer
            .write_all(bytes)
            .await
            .map_err(|error| error.to_string())?;
        writer.flush().await.map_err(|error| error.to_string())
    })
    .await
    .map_err(|_| "binary preview write timed out".to_string())?
}

fn response_timeout_for_command(command: &str) -> Duration {
    if command == "get_preview_data"
        || command == "get_preview_binary"
        || command == "get_document_preview"
    {
        Duration::from_secs(30)
    } else if command == "archive_entry_images" || command == "source_context_import" {
        Duration::from_secs(60)
    } else {
        API_WRITE_TIMEOUT
    }
}

async fn graceful_shutdown(state: &ApiState) {
    info!("Graceful shutdown requested via API");
    let _ = state.shutdown.send(true);
}

async fn send_json(
    w: &mut (impl AsyncWriteExt + Unpin),
    ok: bool,
    data: Option<Value>,
    error: impl Into<ResponseError>,
    command: &str,
    stable_errors: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let error = error.into();
    let resp = if ok {
        serde_json::json!({ "ok": true, "data": data })
    } else if stable_errors {
        // A typed producer's category is authoritative; the historical text
        // classifier is only the fallback for routes that never had a kind.
        let envelope = match error.code() {
            Some(code) => tailsync_runtime::contracts::StableErrorEnvelope::new(code),
            None => tailsync_runtime::contracts::StableErrorEnvelope::from_legacy_message(
                command,
                error.message(),
            ),
        };
        serde_json::json!({
            "ok": false,
            "error": envelope
        })
    } else {
        serde_json::json!({ "ok": false, "error": error.message() })
    };
    let mut bytes = serde_json::to_vec(&resp)?;
    bytes.push(b'\n');
    w.write_all(&bytes).await?;
    w.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn encoded_error(stable: bool) -> Value {
        let (mut writer, reader) = tokio::io::duplex(4096);
        send_json(
            &mut writer,
            false,
            None,
            "database failure at /private/history.db",
            "change_storage_location",
            stable,
        )
        .await
        .unwrap();
        let mut line = String::new();
        BufReader::new(reader).read_line(&mut line).await.unwrap();
        serde_json::from_str(&line).unwrap()
    }

    #[tokio::test]
    async fn stable_error_opt_in_preserves_legacy_and_redacts_details() {
        let legacy = encoded_error(false).await;
        assert!(legacy["error"].as_str().unwrap().contains("private"));

        let stable = encoded_error(true).await;
        assert_eq!(stable["error"]["schema_version"], 1);
        assert_eq!(stable["error"]["code"], "storage_unavailable");
        assert_eq!(stable["error"]["retryable"], true);
        assert!(!stable.to_string().contains("private"));
    }

    #[tokio::test]
    async fn typed_category_survives_contradictory_text_and_redacts_the_entire_failure_response() {
        use tailsync_core::pairing::{PairingFailure, PairingFailureKind};
        for (kind, code) in [
            (
                PairingFailureKind::IdentityMismatch,
                "pairing_identity_mismatch",
            ),
            (
                PairingFailureKind::ProtocolIncompatible,
                "protocol_incompatible",
            ),
            (PairingFailureKind::WindowClosed, "pairing_window_closed"),
            (PairingFailureKind::TimedOut, "pairing_timed_out"),
            (
                PairingFailureKind::ConnectionClosed,
                "pairing_connection_closed",
            ),
            (PairingFailureKind::Busy, "pairing_busy"),
            (PairingFailureKind::Cancelled, "pairing_cancelled"),
            (PairingFailureKind::InvalidArgument, "invalid_argument"),
            (PairingFailureKind::InvalidAdvertisement, "internal_error"),
            (PairingFailureKind::Transport, "internal_error"),
            (PairingFailureKind::Unknown, "internal_error"),
        ] {
            for message in [
                "database failed at /private/peer; token=secret",
                "not found; Pairing handshake timed out; Peer identity mismatch",
            ] {
                for stable in [true, false] {
                    let error = ResponseError::pairing_failure(&PairingFailure::new(kind, message));
                    assert_eq!(serde_json::to_value(&error).unwrap(), message);
                    let (mut writer, reader) = tokio::io::duplex(4096);
                    send_json(
                        &mut writer,
                        false,
                        Some(serde_json::json!({"error": "/private/data", "token": "secret"})),
                        error,
                        "start_pairing",
                        stable,
                    )
                    .await
                    .unwrap();
                    let mut line = String::new();
                    BufReader::new(reader).read_line(&mut line).await.unwrap();
                    let response: Value = serde_json::from_str(&line).unwrap();
                    assert_eq!(response.as_object().unwrap().len(), 2);
                    assert!(response.get("data").is_none());
                    if stable {
                        assert_eq!(response["error"]["code"], code);
                        assert_eq!(response["error"]["retryable"], false);
                        assert!(!line.contains("private") && !line.contains("secret"));
                    } else {
                        assert_eq!(response["error"], message);
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn incompatible_protocol_socket_error_has_a_fixed_public_envelope() {
        let (mut writer, reader) = tokio::io::duplex(4096);
        send_json(
            &mut writer,
            false,
            None,
            "Pairing handshake failed: Incompatible TailSync protocol: peer at /private/peer uses v2, token=secret",
            "start_pairing",
            true,
        )
        .await
        .unwrap();
        let mut line = String::new();
        BufReader::new(reader).read_line(&mut line).await.unwrap();
        let response: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(response["error"]["schema_version"], 1);
        assert_eq!(response["error"]["code"], "protocol_incompatible");
        assert_eq!(
            response["error"]["message_key"],
            "error.protocol_incompatible"
        );
        assert!(!line.contains("/private/peer"));
        assert!(!line.contains("token=secret"));
    }

    #[tokio::test]
    async fn composite_restore_outcomes_survive_socket_error_encoding() {
        for code in [
            "publish_partial",
            "publish_unknown",
            "publish_superseded",
            "native_only",
            "no_single_image",
            "format_unavailable",
            "unsupported_format",
        ] {
            let (mut writer, reader) = tokio::io::duplex(4096);
            send_json(&mut writer, false, None, code, "restore_entry", true)
                .await
                .unwrap();
            let mut line = String::new();
            BufReader::new(reader).read_line(&mut line).await.unwrap();
            let response: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(response["ok"], false);
            assert_eq!(response["error"]["code"], code);
            assert_eq!(
                response["error"]["message_key"],
                format!("history.restore.code.{code}")
            );
        }
    }

    #[test]
    fn preview_response_gets_extended_write_timeout_only() {
        assert_eq!(
            response_timeout_for_command("get_preview_data"),
            Duration::from_secs(30)
        );
        assert_eq!(
            response_timeout_for_command("get_preview_binary"),
            Duration::from_secs(30)
        );
        assert_eq!(
            response_timeout_for_command("get_history"),
            API_WRITE_TIMEOUT
        );
        assert_eq!(response_timeout_for_command("quit"), API_WRITE_TIMEOUT);
    }

    /// The socket-level answer to one request, through the production
    /// `serve_connection` loop: token check, route dispatch and the real
    /// `write_response`/`send_json` encoding core.
    #[cfg(target_os = "macos")]
    async fn socket_response(state: Arc<ApiState>, request: Value) -> Value {
        let permits = Arc::new(Semaphore::new(1));
        let permit = permits.clone().acquire_owned().await.unwrap();
        let (server, mut client) = tokio::net::UnixStream::pair().unwrap();
        let task = tokio::spawn(serve_connection(server, state, permit));
        let mut encoded = serde_json::to_vec(&request).unwrap();
        encoded.push(b'\n');
        client.write_all(&encoded).await.unwrap();
        let mut line = String::new();
        timeout(
            Duration::from_secs(5),
            BufReader::new(client).read_line(&mut line),
        )
        .await
        .expect("pairing response timed out")
        .expect("pairing response line");
        task.await.unwrap();
        serde_json::from_str(&line).unwrap()
    }

    #[cfg(target_os = "macos")]
    fn authenticated(cmd: &str, stable: bool) -> Value {
        let mut request = serde_json::json!({ "cmd": cmd, "token": "a".repeat(64) });
        if stable {
            request["error_schema_version"] = serde_json::json!(1);
        }
        request
    }

    #[cfg(target_os = "macos")]
    #[tokio::test(flavor = "current_thread")]
    async fn typed_pairing_routes_keep_their_category_through_the_real_connection() {
        let state = super::cancellation_tests::test_state();
        // The pairing window has to be open for a failure to be recorded.
        let enabled = socket_response(state.clone(), authenticated("enable_pairing", false)).await;
        assert_eq!(enabled["ok"], true);

        // `start_pairing` fails structurally before any socket work: the target
        // is a valid IP that the selected mode rejects, so the failure is a
        // typed `InvalidArgument`.
        let mut request = authenticated("start_pairing", true);
        request["address"] = serde_json::json!("203.0.113.5");
        let response = socket_response(state.clone(), request).await;
        assert_eq!(response["ok"], false);
        assert_eq!(response["error"]["schema_version"], 1);
        assert_eq!(response["error"]["code"], "invalid_argument");
        assert_eq!(response["error"]["message_key"], "error.invalid_argument");
        assert_eq!(response["error"]["retryable"], false);
        // The fixed envelope has no field that could carry the source message.
        assert!(response["error"].get("message").is_none());
        assert_eq!(response["error"].as_object().unwrap().len(), 5);

        // An expired invite is a closed pairing window. The historical text
        // classifier has no rule for it, so this code can only come from the
        // typed invite mapping on the production route.
        const ENDPOINT_ID: &str =
            "5866666666666666666666666666666666666666666666666666666666666666";
        let expired = Arc::new(crate::pairing::RemotePairingInviteManager::new())
            .create_from_endpoint_id(ENDPOINT_ID, Duration::ZERO)
            .expect("create an immediately expired invite")
            .as_link();
        let mut request = authenticated("inspect_remote_pairing_link", true);
        request["invite_link"] = serde_json::json!(expired);
        let response = socket_response(state.clone(), request).await;
        assert_eq!(response["error"]["code"], "pairing_window_closed");
        assert_eq!(
            response["error"]["message_key"],
            "error.pairing_window_closed"
        );
        assert_eq!(response["error"]["retryable"], false);

        // A malformed link is the same finite mapping, not a second classifier.
        let mut request = authenticated("inspect_remote_pairing_link", true);
        request["invite_link"] = serde_json::json!("tailsync://settings");
        let response = socket_response(state.clone(), request).await;
        assert_eq!(response["error"]["code"], "invalid_argument");
        assert_eq!(response["error"]["message_key"], "error.invalid_argument");

        // The same failure without the opt-in keeps the historical text verbatim
        // and adds no new wire field.
        let mut request = authenticated("inspect_remote_pairing_link", false);
        request["invite_link"] = serde_json::json!(expired);
        let response = socket_response(state.clone(), request).await;
        assert_eq!(response["error"], "TailSync pairing invite has expired");
        assert_eq!(response.as_object().unwrap().len(), 2);
    }

    #[cfg(target_os = "macos")]
    #[tokio::test(flavor = "current_thread")]
    async fn a_direct_pairing_command_carries_its_category_without_a_status_kind() {
        // A fresh manager has recorded nothing, so the status built alongside
        // the failure carries neither an error text nor a kind. The direct
        // command failure must still reach the fixed category on its own.
        let state = super::cancellation_tests::test_state();
        let request: Request =
            serde_json::from_value(serde_json::json!({ "cmd": "confirm_pairing" })).unwrap();
        let response = crate::api::routes::handle_cmd(request, &state).await;
        assert!(!response.ok);
        let error = response.error.as_ref().expect("classified pairing error");
        assert_eq!(
            error.code(),
            Some(tailsync_runtime::contracts::StableErrorCode::InvalidArgument)
        );
        assert_eq!(
            error.message(),
            "No pairing verification is awaiting confirmation"
        );
        let status = response.data.expect("pairing status alongside the failure");
        assert!(status["error"].is_null());
        assert!(status.get("failure_kind").is_none());
    }

    #[cfg(target_os = "macos")]
    #[tokio::test(flavor = "current_thread")]
    async fn remote_pairing_routes_keep_their_category() {
        use tailsync_runtime::contracts::StableErrorCode;

        let state = super::cancellation_tests::test_state();
        // A mode without Iroh rejects invite creation structurally, before any
        // endpoint or listener work happens.
        state.settings.lock().await.connection_mode = "lan_only".into();
        let request: Request = serde_json::from_value(serde_json::json!({
            "cmd": "create_remote_pairing_invite",
        }))
        .unwrap();
        let response = crate::api::routes::handle_cmd(request, &state).await;
        assert!(!response.ok);
        assert_eq!(
            response.error.as_ref().expect("classified error").code(),
            Some(StableErrorCode::InvalidArgument)
        );

        // An expired invite is a closed window on the start route as well.
        const ENDPOINT_ID: &str =
            "5866666666666666666666666666666666666666666666666666666666666666";
        let expired = Arc::new(crate::pairing::RemotePairingInviteManager::new())
            .create_from_endpoint_id(ENDPOINT_ID, Duration::ZERO)
            .expect("create an immediately expired invite")
            .as_link();
        let request: Request = serde_json::from_value(serde_json::json!({
            "cmd": "start_remote_pairing",
            "invite_link": expired,
        }))
        .unwrap();
        let response = crate::api::routes::handle_cmd(request, &state).await;
        assert!(!response.ok);
        assert_eq!(
            response.error.as_ref().expect("classified error").code(),
            Some(StableErrorCode::PairingWindowClosed)
        );
    }

    #[cfg(target_os = "macos")]
    #[tokio::test(flavor = "current_thread")]
    async fn non_pairing_routes_keep_the_legacy_classifier_and_wire_shape() {
        let state = super::cancellation_tests::test_state();

        // A route with no typed source still classifies from its fixed text.
        let response = socket_response(
            state.clone(),
            authenticated("definitely_not_a_command", true),
        )
        .await;
        assert_eq!(response["error"]["code"], "not_found");
        assert_eq!(response["error"]["message_key"], "error.not_found");

        let legacy = socket_response(
            state.clone(),
            authenticated("definitely_not_a_command", false),
        )
        .await;
        assert_eq!(legacy["error"], "unknown command: definitely_not_a_command");
        assert_eq!(legacy.as_object().unwrap().len(), 2);

        // A successful non-pairing response keeps its exact shape.
        let success = socket_response(state.clone(), authenticated("get_status", true)).await;
        assert_eq!(success["ok"], true);
        assert!(success.get("error").is_none());
    }
}
