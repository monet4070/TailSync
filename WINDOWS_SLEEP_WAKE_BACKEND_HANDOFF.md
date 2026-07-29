# Windows 端休眠/唤醒卡死修复移植说明

日期：2026-07-27

## 目标

将 macOS 项目中已经验证的共享 Rust 后端修复同步到 Windows 项目，解决设备长时间休眠或网络中断后：

- 休眠前未确认事件超过 5 分钟时间窗后被永久放在队首；
- 后续文字、图片事件无法继续发送；
- `disconnect_all()` 只删除连接池映射，但旧 worker 仍继续连接或重放；
- 远端剪贴板写入失败被静默忽略；
- macOS 文件剪贴板辅助进程可能无限等待；
- API/TCP 端口仍存活时，应用会把已停止工作的剪贴板监听误判为健康。

线协议版本、帧格式、端口和配对数据均未改变，不需要重新配对。

## 推荐移植方式

GitHub monorepo 当前的 Windows 后端在路由健康、发现刷新和 API 快照等位置已经拥有独立实现，尤其是 `api.rs`、`clipboard.rs` 和 `network/mod.rs`。**不要使用 macOS 文件完整覆盖 Windows 同名文件**，否则会回退 Windows 已有功能。

应按本文“后端修改清单”逐项手工合并。下面的 SHA-256 仅用于标识已经验证的 macOS 参考实现，不是 Windows 合并后的目标 hash：

| 文件 | 修复后 SHA-256 |
|---|---|
| `src-tauri/src/api.rs` | `3ffbd515f9eda71f4500389d1c4673fd6cc9c398e6024e39661caec20e331b57` |
| `src-tauri/src/clipboard.rs` | `f8f4800ee17b0aee348a2d9a9d022d63ba40e87480ff8c38ceede70f640df449` |
| `src-tauri/src/clipboard_file.rs` | `7df9d915f48692be66631d582b6e195c8e66021eef2edf203044330903918a92` |
| `src-tauri/src/network/mod.rs` | `f4fac1c299aa2deff2357a4af58b7d5ee845597e0fcc714024a034abe9af0e53` |
| `src-tauri/src/sync.rs` | `84a9f0fd4988ca660cfc00861ad50ba0f2dfea19667e13e752d133167e7ffb23` |

如需核对下载到 Windows 机器上的 macOS 参考文件，可使用：

```powershell
Get-FileHash src-tauri\src\api.rs -Algorithm SHA256
Get-FileHash src-tauri\src\clipboard.rs -Algorithm SHA256
Get-FileHash src-tauri\src\clipboard_file.rs -Algorithm SHA256
Get-FileHash src-tauri\src\network\mod.rs -Algorithm SHA256
Get-FileHash src-tauri\src\sync.rs -Algorithm SHA256
```

Windows 合并优先级：

1. 必须：`network/mod.rs` 的事件时间戳刷新、永久拒绝丢弃和 worker shutdown。
2. 必须：`sync.rs` 的剪贴板写入错误传播。
3. 建议：`clipboard.rs` 的 recovery generation 与 monitor health；合并时保留 Windows 已有的 native change detector 行为。
4. 建议：`api.rs` 暴露 monitor health，并在 `reconnect_peers` 中请求 recovery；保留 Windows 已有 peer snapshot/refresh 实现。
5. 无需移植运行逻辑：`clipboard_file.rs` 新增内容全部是 macOS helper 超时保护，Windows 的 CF_HDROP 路径不受该问题影响。

## 后端修改清单

### 1. `src-tauri/src/network/mod.rs`

#### 1.1 为每个连接 worker 增加关闭信号

`PoolSender` 新增 `watch::Sender<bool>`：

```rust
#[derive(Clone)]
struct PoolSender {
    priority: mpsc::Sender<QueuedFrame>,
    bulk: mpsc::Sender<QueuedFrame>,
    shutdown: watch::Sender<bool>,
}

impl PoolSender {
    fn request_shutdown(&self) {
        let _ = self.shutdown.send(true);
    }
}
```

创建 worker 时同时创建 watch channel，并把 receiver 传给 `connection_task`：

```rust
let (priority, priority_rx) = mpsc::channel::<QueuedFrame>(POOL_CHANNEL_SIZE);
let (bulk, bulk_rx) = mpsc::channel::<QueuedFrame>(POOL_CHANNEL_SIZE);
let (shutdown, shutdown_rx) = watch::channel(false);
let tx = PoolSender {
    priority,
    bulk,
    shutdown,
};
self.senders.insert(key, tx.clone());
tokio::spawn(connection_task(
    candidates,
    hostname,
    priority_rx,
    bulk_rx,
    self.identity.clone(),
    self.settings.clone(),
    shutdown_rx,
));
```

替换同 hostname 的 sender、断开单个设备、断开全部设备时，必须先调用 `request_shutdown()`：

```rust
pub fn disconnect_hostname(&mut self, hostname: &str) {
    self.senders.retain(|(_, peer_hostname), sender| {
        let keep = peer_hostname != hostname;
        if !keep {
            sender.request_shutdown();
        }
        keep
    });
}

pub fn disconnect_all(&mut self) {
    for sender in self.senders.values() {
        sender.request_shutdown();
    }
    self.senders.clear();
}
```

`connection_task` 新增参数：

```rust
mut shutdown: watch::Receiver<bool>,
```

连接、退避等待、pending 交付、心跳、队列等待和普通帧交付都使用 `tokio::select!` 同时监听关闭信号。辅助函数：

```rust
async fn wait_for_shutdown(shutdown: &mut watch::Receiver<bool>) {
    if *shutdown.borrow() {
        return;
    }
    while shutdown.changed().await.is_ok() {
        if *shutdown.borrow() {
            return;
        }
    }
}
```

典型调用方式：

```rust
let connection_result = tokio::select! {
    biased;
    _ = wait_for_shutdown(&mut shutdown) => return,
    result = &mut connection => result,
};
```

#### 1.2 重连时刷新可靠事件时间戳

事件的 `message_id` 必须保持不变，用于接收端重放抑制；只刷新 `timestamp_ms`：

```rust
AckExpectation::Event(message_id) => {
    let mut envelope = EventEnvelope::decode(&pending.queued.payload)
        .map_err(|error| error.to_string())?;
    if envelope.message_id != message_id {
        return Err("queued event ID does not match its acknowledgement".to_string());
    }
    envelope.timestamp_ms = unix_timestamp_ms();
    let frame = Frame::new(
        pending.queued.command,
        0,
        pending.sequence,
        envelope.encode(),
    );
    deliver_event_frame(stream, pending, &frame, message_id).await?;
    Ok(DeliveryReceipt::default())
}
```

不要生成新的 `message_id`，否则 ACK 丢失后的重发可能被重复应用。

#### 1.3 对端明确拒绝的事件不得永久占据队首

`deliver_event_frame` 单独识别 `PeerError`：

```rust
Ok(Ok(frame)) if frame.command == Command::PeerError => {
    return Err(format!(
        "peer rejected event: {}",
        String::from_utf8_lossy(&frame.payload)
    ));
}
```

识别永久拒绝：

```rust
fn is_permanent_delivery_error(error: &str) -> bool {
    error.starts_with("peer rejected event:")
}
```

`connection_task` 的两处 `deliver_pending_frame` 错误处理都增加：

```rust
Err(error) if is_permanent_delivery_error(&error) => {
    warn!("Dropping event rejected by {}: {error}", addr);
    frame.complete(Err(error));
}
```

只有网络断开、超时等可恢复错误才重新放回 `pending`。

#### 1.4 传播剪贴板写入错误

`process_event_content` 中两处调用改为 `await?`：

```rust
sync_engine
    .lock()
    .await
    .handle_incoming_text(&text, source.to_string())
    .await?;

sync_engine
    .lock()
    .await
    .handle_incoming_image(content, source.to_string())
    .await?;
```

#### 1.5 新增回归测试

必须保留以下测试：

- `connection_worker_stops_when_the_pool_disconnects_it`
- `expired_pending_event_does_not_block_a_new_event_after_reconnect`

第二个测试构造一个比 `EVENT_TIMESTAMP_WINDOW_MS` 更旧的事件，随后排入 `after-wake` 事件，并断言接收端最终得到 `after-wake`，而不是重复得到 `before-sleep`。

### 2. `src-tauri/src/sync.rs`

`handle_incoming_text` 和 `handle_incoming_image` 的返回类型由 `()` 改为 `Result<(), String>`。

文字写入：

```rust
pub async fn handle_incoming_text(&mut self, text: &str, source: String) -> Result<(), String> {
    let hash = blake3::hash(text.as_bytes()).to_hex().to_string();
    self.shadow_filter.push(hash.clone());

    let result = self.with_clipboard(|cb| {
        cb.write_text(text.to_string())
            .map_err(|error| format!("write_text failed: {error}"))
    });
    if let Err(error) = result {
        self.shadow_filter.retain(|entry| entry != &hash);
        return Err(error);
    }
    info!(
        "Clipboard ← text from peer {} ({} chars)",
        source,
        text.len()
    );
    Ok(())
}
```

图片写入使用相同模式：写入失败时移除 `image_shadow_filter` 中刚加入的 hash，并返回错误。

`with_clipboard` 改成可返回错误的泛型函数：

```rust
fn with_clipboard<T>(
    &self,
    f: impl FnOnce(&tauri_plugin_clipboard_manager::Clipboard<tauri::Wry>) -> Result<T, String>,
) -> Result<T, String> {
    let handle = self
        .app_handle
        .as_ref()
        .ok_or_else(|| "Clipboard app handle is unavailable".to_string())?;
    let state = handle
        .try_state::<tauri_plugin_clipboard_manager::Clipboard<tauri::Wry>>()
        .ok_or_else(|| "Clipboard plugin state is unavailable".to_string())?;
    f(&state)
}
```

### 3. `src-tauri/src/clipboard.rs`

#### 3.1 增加监听恢复 generation 和健康心跳

```rust
static CLIPBOARD_RECOVERY_GENERATION: AtomicU64 = AtomicU64::new(0);
static CLIPBOARD_MONITOR_LAST_TICK_MS: AtomicU64 = AtomicU64::new(0);
const CLIPBOARD_MONITOR_STALE_AFTER_MS: u64 = 10_000;

pub fn request_wake_recovery() {
    CLIPBOARD_RECOVERY_GENERATION.fetch_add(1, Ordering::AcqRel);
}

pub fn monitor_is_healthy() -> bool {
    let last_tick = CLIPBOARD_MONITOR_LAST_TICK_MS.load(Ordering::Acquire);
    if last_tick == 0 {
        return false;
    }
    let now = crate::protocol::unix_timestamp_ms().max(0) as u64;
    now.saturating_sub(last_tick) <= CLIPBOARD_MONITOR_STALE_AFTER_MS
}
```

`clipboard_loop` 每轮更新 `CLIPBOARD_MONITOR_LAST_TICK_MS`。generation 变化时重新创建 `ClipboardChangeDetector`，清空最后一次 text/image/file 缓存，然后进入下一轮：

```rust
let requested_generation = CLIPBOARD_RECOVERY_GENERATION.load(Ordering::Acquire);
if requested_generation != recovery_generation {
    recovery_generation = requested_generation;
    change_detector = ClipboardChangeDetector::new();
    last_text_hash.clear();
    last_image_hash.clear();
    last_file_list.clear();
    info!("Clipboard monitor reset after system wake");
    continue;
}
```

#### 3.2 macOS 文件辅助程序放入 blocking pool

该部分由 `#[cfg(target_os = "macos")]` 保护，复制到 Windows 项目不会影响 Windows 执行路径：

```rust
#[cfg(target_os = "macos")]
let file_paths =
    match tokio::task::spawn_blocking(clipboard_file::read_clipboard_files).await {
        Ok(paths) => paths,
        Err(error) => {
            error!("Clipboard file helper task failed: {error}");
            None
        }
    };
#[cfg(not(target_os = "macos"))]
let file_paths = clipboard_file::read_clipboard_files();
```

### 4. `src-tauri/src/clipboard_file.rs`

这是 macOS 专用的防卡死修改。Windows 项目不需要复制该运行逻辑，也不要为此覆盖现有 CF_HDROP 实现。

- `Command::output()`/`Command::status()` 改成 `spawn()`；
- 最长等待 2 秒；
- 超时后 `kill()` + `wait()`；
- 新增 `stuck_clipboard_helper_is_killed_after_timeout` 测试；
- 所有新增实现均由 `#[cfg(target_os = "macos")]` 保护。

核心等待函数：

```rust
fn wait_for_child(mut child: Child, timeout: Duration) -> Result<(ExitStatus, Vec<u8>), String> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut output = Vec::new();
                if let Some(mut stdout) = child.stdout.take() {
                    stdout.read_to_end(&mut output).map_err(|error| {
                        format!("Could not read clipboard helper output: {error}")
                    })?;
                }
                return Ok((status, output));
            }
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "Clipboard helper timed out after {} ms",
                    timeout.as_millis()
                ));
            }
            Err(error) => return Err(format!("Could not wait for clipboard helper: {error}")),
        }
    }
}
```

### 5. `src-tauri/src/api.rs`

`get_status` 新增剪贴板监听健康字段：

```rust
"clipboard_monitor_healthy": crate::clipboard::monitor_is_healthy(),
```

`reconnect_peers` 在断开连接池后请求重置剪贴板监听：

```rust
"reconnect_peers" => {
    state.pool.lock().await.disconnect_all();
    crate::clipboard::request_wake_recovery();
    network::clear_peer_cache().await;
    Response {
        ok: true,
        data: None,
        error: None,
    }
}
```

Windows UI 当前不读取 `clipboard_monitor_healthy` 也不影响兼容性；字段只是向 JSON 对象增加一项。

## 不需要移植到 Windows 的代码

以下是 macOS SwiftUI 外壳的唤醒/watchdog 改动，Windows Tauri UI 不应复制：

- `swift-ui/Sources/TailSync/TailSyncApp.swift`
- `swift-ui/Sources/TailSync/Services/ApiClient.swift`

Windows 端不要复制 Swift 文件，也不要整文件覆盖现有 Rust 平台实现；应按本文清单合并对应行为。

## 验证命令

在 Windows 项目 PowerShell 中执行：

```powershell
cargo fmt --manifest-path src-tauri\Cargo.toml --all -- --check
cargo test --manifest-path src-tauri\Cargo.toml --lib
npm run lint
npm run build
```

重点确认测试输出包含：

```text
network::tests::connection_worker_stops_when_the_pool_disconnects_it ... ok
network::tests::expired_pending_event_does_not_block_a_new_event_after_reconnect ... ok
clipboard_file::tests::stuck_clipboard_helper_is_killed_after_timeout ...
```

最后从任一项目运行跨平台一致性检查：

```powershell
node scripts\check_cross_platform_sync.mjs `
  --win-root C:\path\to\tailsync-v2-win `
  --mac-root C:\path\to\tailsync-v2-mac-1
```

`clipboard_file` 的 helper 超时测试带有 `#[cfg(target_os = "macos")]`，因此在 Windows 上不会执行，这是正常结果。

## 双设备验收

1. 安装同步后的 Windows 构建和本次修复后的 macOS 构建。
2. 确认原配对信息仍然有效，不重新配对。
3. 双向复制文字和图片，确认正常。
4. 保持两端运行，Mac 盒盖至少 10 分钟。
5. 唤醒后等待约 3 秒。
6. 依次验证 Windows→Mac 文字、Windows→Mac 图片、Mac→Windows 文字、Mac→Windows 图片。
7. 连续复制多条不同文字，确认旧事件不会阻塞后续事件。
8. 打开历史记录，确认收到内容的来源设备正确。

## 已完成验证

macOS 修复分支已完成：

- Rust：73 通过，0 失败，1 个真实 Tailscale 环境测试忽略；
- Swift debug/release 构建通过；
- 前端 lint/build 通过；
- 打包应用启动后 `tcp_server_healthy=true`；
- 打包应用启动后 `clipboard_monitor_healthy=true`；
- `19889` 和 `19890` 监听正常。

## 建议提交信息

```text
fix(sync): recover clipboard delivery after sleep and reconnect

Refresh reliable event timestamps while preserving message IDs, cancel stale
connection workers, drop permanently rejected head-of-line events, expose
clipboard write failures, and add clipboard monitor recovery/health checks.
```

建议 Windows 端使用 `diagnose` 工作流运行回归测试；若 Windows 分支的共享 Rust 文件存在独立改动，先做逐文件三方合并，再运行跨平台漂移检查。
