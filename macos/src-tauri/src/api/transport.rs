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
                &error,
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
                    &error,
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
    let cancel_on_disconnect =
        req.request_id.is_some() && matches!(req.cmd.as_str(), "get_history" | "get_preview_data");
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
        &response.error.unwrap_or_default(),
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
    error: &str,
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
    if command == "get_preview_data" || command == "get_preview_binary" {
        Duration::from_secs(30)
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
    error: &str,
    command: &str,
    stable_errors: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let resp = if ok {
        serde_json::json!({ "ok": true, "data": data })
    } else if stable_errors {
        serde_json::json!({
            "ok": false,
            "error": tailsync_runtime::contracts::StableErrorEnvelope::from_legacy_message(
                command, error,
            )
        })
    } else {
        serde_json::json!({ "ok": false, "error": error })
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
}
