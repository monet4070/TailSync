use super::*;

pub async fn start(
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
        handlers.spawn(async move {
            let _permit = permit;
            let (reader, mut writer) = stream.into_split();
            let req = match read_request(reader).await {
                Ok(req) => req,
                Err(error) => {
                    let _ =
                        write_response(&mut writer, false, None, &error, API_WRITE_TIMEOUT).await;
                    return;
                }
            };
            if !st.token.matches(req.token.as_deref()) {
                let _ = write_response(&mut writer, false, None, "unauthorized", API_WRITE_TIMEOUT)
                    .await;
                return;
            }

            let should_shutdown = req.cmd == "quit";
            // A history preview can contain up to 64 MiB of decrypted bytes,
            // expanding to roughly 90 MiB when wrapped in Base64/JSON. Keep
            // the normal five-second API timeout for every other command but
            // allow this bounded response enough time to drain locally.
            let response_timeout = response_timeout_for_command(&req.cmd);
            let resp = handle_cmd(req, &st).await;
            let sent = write_response(
                &mut writer,
                resp.ok,
                resp.data,
                &resp.error.unwrap_or_default(),
                response_timeout,
            )
            .await;
            if should_shutdown && sent.is_ok() {
                graceful_shutdown(&st).await;
            }
        });
    }
    info!("API server stopped accepting connections");
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
    Ok(())
}

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
    timeout_duration: Duration,
) -> Result<(), String> {
    timeout(timeout_duration, send_json(writer, ok, data, error))
        .await
        .map_err(|_| "response write timed out".to_string())?
        .map_err(|error| error.to_string())
}

fn response_timeout_for_command(command: &str) -> Duration {
    if command == "get_preview_data" {
        Duration::from_secs(30)
    } else {
        API_WRITE_TIMEOUT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_response_gets_extended_write_timeout_only() {
        assert_eq!(
            response_timeout_for_command("get_preview_data"),
            Duration::from_secs(30)
        );
        assert_eq!(
            response_timeout_for_command("get_history"),
            API_WRITE_TIMEOUT
        );
        assert_eq!(response_timeout_for_command("quit"), API_WRITE_TIMEOUT);
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
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let resp = if ok {
        serde_json::json!({ "ok": true, "data": data })
    } else {
        serde_json::json!({ "ok": false, "error": error })
    };
    let mut bytes = serde_json::to_vec(&resp)?;
    bytes.push(b'\n');
    w.write_all(&bytes).await?;
    w.flush().await?;
    Ok(())
}
