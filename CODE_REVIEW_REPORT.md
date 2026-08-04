# TailSync v2.0.0 完整代码审查报告

**审查日期**: 2026-07-31
**项目**: TailSync — 跨平台剪贴板同步工具
**技术栈**: Rust (Tauri) + TypeScript/React + Noise Protocol (XX_25519_ChaChaPoly_BLAKE2s)
**审查范围**: macOS + Windows 后端（Rust）、前端（TypeScript/React）、构建脚本、配置文件
**审查方法**: 逐文件完整阅读 + 跨文件交叉验证 + 四个并行审查代理

---

## 目录

1. [严重问题 (Critical) — 5 个](#1-严重问题-critical)
2. [高危问题 (High) — 11 个](#2-高危问题-high)
3. [中危问题 (Medium) — 17 个](#3-中危问题-medium)
4. [低危问题 (Low) — 10 个](#4-低危问题-low)
5. [代码优雅性与可维护性](#5-代码优雅性与可维护性)
6. [macOS vs Windows 平台差异分析](#6-macos-vs-windows-平台差异分析)
7. [正面发现](#7-正面发现)
8. [总结与优先级](#8-总结与优先级)

---

## 1. 严重问题 (Critical)

### C1. 加密密钥竞态条件 — DEK 初始化非原子操作

- **文件**: [crypto.rs:241-273](macos/src-tauri/src/crypto.rs#L241-L273)
- **分类**: Bug / 并发安全

#### 问题代码

```rust
#[cfg(not(test))]
static DEK: OnceLock<Vec<u8>> = OnceLock::new();

pub(crate) fn get_dek() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    #[cfg(not(test))]
    {
        if let Some(k) = DEK.get() {       // ← 线程 A 和 B 同时到达这里
            return Ok(k.clone());           // ← 都得到 None（尚未初始化）
        }

        let key = match read_keychain() {   // ← 都去读 keychain
            Ok(k) => k,
            Err(_) => {                     // ← 全新安装，都进入这里
                warn!("No existing encryption key found, generating new one");
                let rng = SystemRandom::new();
                let mut key = vec![0u8; 32]; // ← AES-256
                rng.fill(&mut key)
                    .map_err(|_| "Failed to generate encryption key")?;
                write_keychain(&key)?;       // ← 都写入 keychain（后写覆盖先写）
                info!("New encryption key stored in OS keychain");
                key                           // ← 返回不同的 key
            }
        };

        // 注释已经承认了竞态：
        // Safety: DEK is initialized exactly once. If another thread
        // initialized it between our get() check and set(), we lose the race
        // but the stored value is used consistently.
        let _ = DEK.set(key.clone());        // ← 一个 set 成功，另一个失败
        Ok(key)                              // ← 各自返回不同的 key
    }
}
```

#### 并发时序图

```
时间轴:    T1              T2              T3              T4              T5
线程 A:    DEK.get()→None → 读keychain→None → 生成key_A → write_keychain=A → DEK.set(key_A)→用key_A
线程 B:           DEK.get()→None → 读keychain→None → 生成key_B → write_keychain=B → DEK.set失败→用key_B
```

#### 后果

- 线程 A 用 `key_A` 加密了身份密钥和部分剪贴板历史
- 线程 B 用 `key_B` 加密了另一部分数据
- keychain 中存储的是 `key_B`（后写覆盖）
- 下一次启动时，只有 `key_B` 能从 keychain 读取
- 用 `key_A` 加密的数据**永久且静默地无法解密**（AES-256-GCM 认证失败）
- 目前 `get_dek()` 的第一个调用发生在 `lib.rs:80` 的主线程上，在任务 spawn 之前，所以竞态尚未触发。但这是一个脆弱的假设——任何未来的代码改动都可能引入并发调用。

#### 正确做法

`get_dek()` 的读-keychain/生成/写-keychain 整个流程应该被包裹在 `OnceLock::get_or_init()` 的闭包中，确保无论有多少个线程同时调用，初始化的三个步骤只执行一次且只产生一个密钥。

---

### C2. 可恢复 panic — `Frame::new()` 在过大 payload 时 panic

- **文件**: [protocol.rs:357-361](macos/src-tauri/src/protocol.rs#L357-L361)
- **分类**: Bug / 可靠性

#### 问题代码

```rust
impl Frame {
    /// Create a new frame
    pub fn new(command: Command, flags: u8, sequence: u32, payload: Vec<u8>) -> Self {
        if payload.len() > MAX_PAYLOAD_SIZE {
            panic!("Payload too large");   // ← 直接崩溃整个进程
        }
        Frame {
            command,
            flags: Flags(flags),
            sequence,
            payload,
        }
    }
}
```

`MAX_PAYLOAD_SIZE = MAX_IMAGE_PAYLOAD_SIZE = 32 * 1024 * 1024`（32 MB）。

#### 调用链中的风险

这个函数被以下生产代码调用，如果 payload 意外超过限制，进程就会崩溃：

```rust
// secure.rs:344 — 握手确认（payload 为空，安全）
secure.write_frame(&Frame::new(Command::HandshakeReady, 0, 0, Vec::new())).await

// secure.rs:353-356 — 错误响应（已用 min() 截断，相对安全）
let length = payload.len().min(protocol::MAX_CONTROL_PAYLOAD_SIZE);
secure.write_frame(&Frame::new(Command::PeerError, 0, 0, payload[..length].to_vec())).await

// network/mod.rs:1431 — 心跳（payload 为空，安全）
let hb = Frame::new(Command::Heartbeat, 0, next_sequence, vec![]);

// network/mod.rs:1526-1531 — 事件投递（payload 来自网络，已通过 Frame::decode 验证）
let frame = Frame::new(pending.queued.command, 0, pending.sequence,
    pending.queued.payload.clone());
```

虽然当前所有调用者在创建 `Frame` 前都验证了 payload 大小，但：
- `Frame::new` 是公开 API，未来任何调用者忘记验证就会导致崩溃
- 合理的做法是返回 `Result<Self, ProtocolError>`，将 panic 风险降为零

---

### C3. `std::process::exit(0)` 在 API 请求处理中跳过所有析构

- **文件**: [api.rs:983-986](macos/src-tauri/src/api.rs#L983-L986)
- **分类**: Bug / 可靠性

#### 问题代码

```rust
"quit" => {
    info!("Quit via API");
    std::process::exit(0);
    //     ^^^^^^^^^^^^^^^^^
    //     不运行 Drop，不检查 WAL，不清理任何东西
}
```

#### 被跳过的清理

`std::process::exit()` 不会运行任何析构函数。以下清理逻辑全部被跳过：

| 组件 | 清理逻辑 | 跳过后果 |
|------|---------|---------|
| `HistoryDB` | SQLite WAL checkpoint + 关闭连接 | WAL 文件可能损坏，下次启动需要恢复 |
| `ConnectionPool` | 断开所有 TCP 连接 | 对端收到 RST 而非 FIN |
| `FileReceiveState` | 删除 `.part` 临时文件 | 磁盘残留文件 |
| `AuthenticatedSessionGuard` | 从注册表移除活跃会话 | 内存泄漏（进程退出后自动回收，但非优雅） |
| `PairingManager` | 关闭配对窗口 | 对端看到断连而非取消 |

---

### C4. Windows DPAPI unsafe 代码中的潜在内存泄漏

- **文件**: [crypto.rs:418-446](macos/src-tauri/src/crypto.rs#L418-L446)
- **分类**: Bug / 内存安全 (仅 Windows 平台)

#### 问题代码

```rust
#[cfg(target_os = "windows")]
fn write_keychain(key: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    use windows_sys::Win32::Security::Cryptography::{CryptProtectData, CRYPT_INTEGER_BLOB};
    let blob_in = CRYPT_INTEGER_BLOB {
        cbData: key.len() as u32,
        pbData: key.as_ptr() as *mut u8,
    };
    unsafe {
        let mut blob_out = std::mem::zeroed::<CRYPT_INTEGER_BLOB>();
        if CryptProtectData(
            &blob_in,
            windows_sys::core::w!("TailSync Encryption Key"),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
            &mut blob_out,
        ) == 0
        {
            return Err("DPAPI encrypt failed".into());
            // ⚠️ CryptProtectData 返回 0 时，blob_out.pbData 可能非空（部分成功）。
            //    此处 return 前没有调用 LocalFree — 内存泄漏
        }
        let protected =
            std::slice::from_raw_parts(blob_out.pbData, blob_out.cbData as usize).to_vec();
        LocalFree(blob_out.pbData as *mut core::ffi::c_void);  // 仅在成功路径释放
        // ...
    }
}
```

#### 风险

`CryptProtectData` 返回 0 时，`pbData` 的状态是未定义的——可能已分配也可能未分配。虽然 MSDN 说失败时 `pbData` 通常为 NULL，但在进度回调取消等边缘情况中可能非空。RAII 封装可以完全消除此风险。

同理，`read_keychain()` 中的 `CryptUnprotectData` 也有相同问题（`crypto.rs:367-384`）。

---

### C5. 文件 payload 以明文存储，而文本/图片已加密

- **文件**: [db.rs:132-157](macos/src-tauri/src/db.rs#L132-L157)、[db.rs:179-218](macos/src-tauri/src/db.rs#L179-L218)
- **分类**: Security / 数据泄露

#### 问题代码 — 三种数据类型的存储对比

```rust
// ✅ 文本 — 加密后存入 SQLite BLOB
pub fn add_text(&mut self, text: &str, source_peer: &str) -> ... {
    let encrypted = crypto::encrypt(text.as_bytes())?;  // ← AES-256-GCM 加密
    self.conn.execute(
        "INSERT INTO history (...) VALUES (?1, ...)",
        params![encrypted, ...],                         // ← 密文存入 DB
    )?;
}

// ✅ 图片 — 加密后写入磁盘
fn persist_image_at(directory: &Path, data_hash: &str, data: &[u8]) -> ... {
    std::fs::create_dir_all(directory)?;
    let file_name = format!("{data_hash}.bin");
    let encrypted = crypto::encrypt(data)?;              // ← AES-256-GCM 加密
    let temporary = directory.join(format!("{file_name}.{}.tmp", std::process::id()));
    std::fs::write(&temporary, encrypted)?;              // ← 密文写入 .bin 文件
    // ...
}

// ❌ 文件 — 明文写入磁盘（与其他类型不一致！）
fn persist_history_file_at(directory: &Path, data_hash: &str,
                           original_name: &str, data: &[u8]) -> ... {
    std::fs::create_dir_all(directory)?;
    let file_name = format!("{data_hash}-{safe_name}");
    let temporary = directory.join(format!("{file_name}.{}.tmp", std::process::id()));
    std::fs::write(&temporary, data)?;                   // ← 明文！无加密！
    // 文件名形如: file-history/abc123def-report.pdf  ← 打开即可读取
}
```

#### 后果

任何能访问 `~/Library/Application Support/com.tailsync.TailSync/file-history/`（macOS）或 `%APPDATA%/com.tailsync.TailSync/file-history/`（Windows）的人都可以直接读取通过剪贴板同步的所有文档内容。

迁移 v3（`db.rs:397-408`）显式解密旧 BLOB 并调用 `persist_history_file_at` 写入明文——这使得从 v2 迁移的数据也暴露了。

**这与应用的加密声明矛盾**。应用将密钥存储在 OS keychain 中，给人一种"所有数据都受保护"的印象，但文件历史的保护缺失使其形同虚设。

---

## 2. 高危问题 (High)

### H1. 生产代码路径中多处 `unwrap()` / `expect()` 调用

#### H1.1 — 图片数据解包

**文件**: [clipboard.rs:157-158](macos/src-tauri/src/clipboard.rs#L157-L158)、[api.rs:576-577](macos/src-tauri/src/api.rs#L576-L577)、[api.rs:908-909](macos/src-tauri/src/api.rs#L908-L909)

```rust
let w = u32::from_le_bytes(image_data[0..4].try_into().unwrap());
let h = u32::from_le_bytes(image_data[4..8].try_into().unwrap());
```

这里 `[u8]` 到 `[u8; 4]` 的 `try_into()` 在切片长度正好为 4 时不会失败。但如果有人重构代码改变了切片方式，编译器不会发出警告——运行时直接 panic。

#### H1.2 — 数据库和身份初始化

**文件**: [lib.rs:90](macos/src-tauri/src/lib.rs#L90)、[lib.rs:137-138](macos/src-tauri/src/lib.rs#L137-L138)

```rust
let db = Arc::new(Mutex::new(
    db::HistoryDB::new().expect("Failed to initialize database"),
    //                    ^^^^^^ 磁盘满或权限错误 → 进程崩溃
));

let identity = Arc::new(
    identity::DeviceIdentity::load_or_create()
        .expect("Failed to initialize device identity"),
    //  ^^^^^^ keychain 访问被拒绝 → 进程崩溃
);
```

#### H1.3 — 托盘图标 fallback 尺寸错误

**文件**: [tray.rs:148](macos/src-tauri/src/tray.rs#L148)、[tray.rs:206](macos/src-tauri/src/tray.rs#L206)

```rust
let icon = Image::from_bytes(include_bytes!("../icons/icon.png"))
    .unwrap_or_else(|_| Image::new(&[0u8; 1024], 32, 32));
    //                              ^^^^^^^^       ^^  ^^
    //                            只分配了 1024 字节   32×32
```

`Image::new(rgba: &[u8], width: u32, height: u32)` 期望 RGBA 数据：
`32 × 32 × 4 = 4096` 字节。

但 fallback 只提供了 1024 字节。`Image::new` 内部会尝试读取 `rgba[0..4096]`，这会导致**越界读取**（panic 或潜在的 UB，取决于内部实现）。

---

### H2. DB 迁移中 hash 校验失败时静默继续

- **文件**: [db.rs:393-416](macos/src-tauri/src/db.rs#L393-L416)

```rust
for (id, stored, stored_hash, description) in legacy_files {
    // ...
    match crypto::decrypt(&stored) {
        Ok(data) => {
            let actual_hash = blake3::hash(&data).to_hex().to_string();
            if !stored_hash.is_empty() && stored_hash != actual_hash {
                warn!("File history hash corrected during migration for entry {id}");
                // ⚠️ 仅 warn — 数据可能已被篡改，但仍然使用并迁移
            }
            let (reference, _) = persist_history_file_at(
                file_history_dir, &actual_hash, &description, &data,
            )?;
            // ⚠️ 用计算出的 hash 替代存储的 hash，原文/原hash 的证据被丢弃
        }
        Err(error) => warn!("Legacy file entry {id} could not be migrated: {error}"),
        // ⚠️ 解密失败 → 数据永久丢失，仅一行 warn，用户不知情
    }
}
```

当 `stored_hash` 与 `actual_hash` 不匹配时，可能的原因包括 bit rot、恶意篡改、或之前版本的 bug。无论哪种情况，仅记录 warning 并继续是不够的——应该保留原始数据和原始 hash，或至少将此条目标记为"可疑"而非静默修复。

---

### H3. 缺少速率限制

- **文件**: [network/mod.rs:1822](macos/src-tauri/src/network/mod.rs#L1822)、[api.rs:326](macos/src-tauri/src/api.rs#L326)

```rust
// 仅有的限制是连接级别的
let limiter = ConnectionLimiter::new(64, 8);
// 总连接 64，每 IP 8

// 但没有任何内容级别的速率限制：
// - 单连接可以无限发送 TextPayload（每个最大 1MB）
// - 单连接可以无限发送 ImagePayload（每个最大 32MB）
// - API 服务器无任何速率限制
// - 文件传输无带宽控制
```

**攻击场景**: 一个合法配对的对端（或被攻破的对端）发送无限循环的 1MB 文本，填满 SQLite 数据库和磁盘。

---

### H4. `shadow_filter` 使用 O(n) 线性搜索且从不自动过期

- **文件**: [clipboard.rs:498-518](macos/src-tauri/src/clipboard.rs#L498-L518)

```rust
pub(crate) shadow_filter: Vec<String>,       // ← 无上限的 Vec
pub(crate) image_shadow_filter: Vec<String>, // ← 同上

async fn shadow_check(sync_engine: &Arc<Mutex<sync::SyncEngine>>, hash: &str) -> bool {
    let mut sync = sync_engine.lock().await; // ← 持有锁
    let key = hash.to_string();
    if sync.shadow_filter.contains(&key) {   // ← O(n) 线性扫描
        debug!("Text shadow-filter hit: {}", &hash[..8]);
        sync.shadow_filter.retain(|h| h != &key); // ← O(n) 删除
        true
    } else {
        false
    }
}
```

#### 问题链条

1. 每次从远程接收剪贴板内容时，hash 被 push 进 `shadow_filter`
2. 每次本地剪贴板检测到变化时，遍历 `shadow_filter` 看是否是回声
3. 如果一个 hash **从未被匹配**（接收成功但本地剪贴板恰好发生了其他变化），它**永远留在列表中**
4. 唯一的清理路径是 `CLIPBOARD_RECOVERY_GENERATION` 重置时的 `.clear()`——极为罕见
5. 长期运行：100天 × 100次/天复制 = 10,000 条目，每次本地复制操作执行 O(n) 扫描

**应该使用**: 带时间戳的 `HashMap<String, Instant>`，定期清理过期条目。

---

### H5. SQL LIKE 模式中通配符未转义

- **文件**: [db.rs:725](macos/src-tauri/src/db.rs#L725)

```rust
let pattern = format!("%{keyword}%");
//                     ↑  ^^^^^^^^  ↑
// 用户输入的 % 和 _ 不会被转义
```

**示例**:
- 用户搜索 `100%` → SQL 变成 `LIKE '%100%%'` → 第二个 `%` 是通配符，匹配任意后缀
- 用户搜索 `a_b` → SQL 变成 `LIKE '%a_b%'` → `_` 匹配任意单个字符，所以 "aXb"、"a1b" 都会被匹配
- 攻击者输入 `%%%%%%%%%%%%%%` → 极度缓慢的 LIKE 匹配，可能造成 SQLite 层面的 DoS

虽然使用了参数化查询（SQL 注入不可能），但 LIKE 模式中的特殊字符仍需要转义。

---

### H6. 图片尺寸验证存在间隙

- **文件**: [network/mod.rs:2301-2315](macos/src-tauri/src/network/mod.rs#L2301-L2315)

```rust
fn validate_packed_image(content: &[u8]) -> Result<(), String> {
    if content.len() < 8 {
        return Err("image event header is incomplete".to_string());
    }
    let width = u32::from_le_bytes(content[0..4].try_into().expect("..."));
    let height = u32::from_le_bytes(content[4..8].try_into().expect("..."));
    let rgba_len = (width as usize)
        .checked_mul(height as usize)         // ← 检查溢出
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "image dimensions overflow".to_string())?;
    if content.len() != 8 + rgba_len {         // ← 检查 RGBA 一致性
        return Err("image event dimensions do not match...".to_string());
    }
    Ok(())
}
```

这个函数在从网络接收图片时被调用（`process_event_content` → `validate_packed_image`）。但图片数据**也可以通过 `migrate_entry` API 直接写入数据库**：

```rust
// api.rs:938-981 — 绕过 validate_packed_image！
"image" => db.add_image_migrated(time, desc, &data),
// no validation!
```

之后 `restore_entry` 或 `get_image_data` 读取时，用 `from_le_bytes` 解包——从无效数据中读取 width/height。

---

### H7. macOS keychain 密钥解码使用 `unwrap_or_default()`

- **文件**: [crypto.rs:345-348](macos/src-tauri/src/crypto.rs#L345-L348)

```rust
Ok(o) if o.status.success() => {
    let pass = String::from_utf8_lossy(&o.stdout).trim().to_string();
    if !pass.is_empty() {
        return Ok(hex::decode(&pass).unwrap_or_default());
        //           ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        // keychain 返回了非 hex 数据 → 返回空 Vec<u8> → 后续失败
    }
}
```

如果 keychain 条目被损坏（例如手动编辑、备份还原错误），返回的不是有效的十六进制字符串，`hex::decode` 失败后 `unwrap_or_default()` 返回空字节。后续 `UnboundKey::new(&AES_256_GCM, &[])` 会失败并显示误导性的错误信息 "Invalid key"，而非 "Keychain entry is corrupted — data may need recovery"。

---

### H8. `clipboard_loop` 恐慌保护是空操作

- **文件**: [clipboard.rs:124-131](macos/src-tauri/src/clipboard.rs#L124-L131)

#### 问题代码（带行号上下文）

```rust
// 第 124-131 行：空的 panic guard
let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    // Just return the clipboard state; we handle it outside
    //                                    ↑ 注释说 "we handle it" 但实际没有
}));
// ← result 永远是 Ok(())，因为闭包是空的！
if result.is_err() {
    error!("Clipboard monitor panic caught, continuing");
    continue;
}

// 第 133-143 行：真正的危险代码 — 完全不受 guard 保护！
let clipboard = match handle.try_state::<Clipboard<Wry>>() {
    Some(c) => c,
    None => { ... continue; }
};

// 第 198 行：
match clipboard.read_text() { ... }    // ← 如果这里 panic，整个 loop 崩溃

// 第 234 行：
match clipboard.read_image() { ... }   // ← 同上
```

`clipboard_loop` 在 `tauri::async_runtime::spawn` 中运行。如果 panic 发生：
- 任务静默终止
- 剪贴板监视永久停止
- 不再向其他设备广播
- 不再保存到历史
- 用户完全不知情（应用不会崩溃，但核心功能停止）

---

### H9. API 服务器无认证、无行长度限制

- **文件**: [api.rs:326-357](macos/src-tauri/src/api.rs#L326-L357)

```rust
pub async fn start(state: Arc<ApiState>) -> ... {
    let addr = format!("127.0.0.1:{}", API_PORT);  // 19889
    let listener = network::bind_tcp_listener(addr.parse()?)?;
    info!("API server listening on {}", addr);

    loop {
        let (stream, _) = listener.accept().await?;
        //                ↑ 任何本地进程都能连接
        tokio::spawn(async move {
            let mut reader = BufReader::new(reader);
            let mut line = String::new();
            if reader.read_line(&mut line).await.is_ok() {
                // ↑ 无行长度上限 — 发送无限长的行 → OOM
                let req: Request = match serde_json::from_str(line.trim()) {
                    Ok(r) => r,
                    // ↑ 无认证 — 任何本地进程都能调用 clear_history, quit
                };
                let resp = handle_cmd(req, &st).await;
                // ...
            }
        });
    }
}
```

#### 攻击面

虽绑定在 loopback 上限制外部访问，但在多用户系统上或浏览器/JS 被攻破时：

```bash
# 其他用户（或恶意脚本）可以：
curl http://127.0.0.1:19889 -d '{"cmd":"clear_history"}'    # 删除所有历史
curl http://127.0.0.1:19889 -d '{"cmd":"quit"}'             # 停止守护进程
curl http://127.0.0.1:19889 -d '{"cmd":"migrate_entry","type":"file","desc":"../../evil.sh","data_b64":"..."}'
```

此外 `listener.accept().await?` 在 transient 错误时直接返回 `Err`，整个 API 服务器停止运行且不会重启。

---

### H10. 通过文件描述字段的路径遍历

- **文件**: [api.rs:58-65](macos/src-tauri/src/api.rs#L58-L65)

```rust
#[cfg(target_os = "macos")]
pub fn restore_file_to_clipboard(data: &[u8], fname: &str) {
    let tmp = std::env::temp_dir().join(fname);  // ← fname 来自 DB，未经清理！
    if std::fs::write(&tmp, data).is_err() {
        // ↑ 如果 fname = "../../../../tmp/evil.sh"
        //   实际写入路径可能是 /tmp/evil.sh（取决于 temp_dir 位置）
        return;
    }
    restore_file_path_to_clipboard(&tmp, fname);
}
```

`fname` 的来源链：
```
api.rs:restore_entry → db.get_description(id) → SQLite description 列
→ 可通过 migrate_entry API 写入任意值:
  {"cmd":"migrate_entry","type":"file","desc":"../../.ssh/authorized_keys","data_b64":"..."}
```

虽然 `sanitize_history_file_name`（`db.rs:101-130`）对历史目录中的文件名做了清理，但 `restore_file_to_clipboard` 中的临时文件路径完全绕过了此清理。攻击者可以通过 `migrate_entry` API 的 `desc` 字段注入 `../` 路径，实现以守护进程权限写入任意文件。

---

### H11. `HistoryDB::new()` 清空 incoming 目录使断点续传失效

- **文件**: [db.rs:319-332](macos/src-tauri/src/db.rs#L319-L332)

#### 两个互相矛盾的代码路径

**路径 1 — 启动时销毁所有状态**:

```rust
// db.rs:319-332
impl HistoryDB {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        // ...
        let incoming = get_incoming_dir();
        if let Err(error) = std::fs::remove_dir_all(&incoming) {
            // ↑ 每次启动清空 incoming/，包括所有 .part 和 .resume.json
        }
        std::fs::create_dir_all(&incoming)?;
        // ...
    }
}
```

**路径 2 — 断点续传依赖于这些文件存在**:

```rust
// sync.rs:210-211 — 恢复文件名基于 incoming 目录
let tmp_path = parent.join(format!("{id}.part"));
let state_path = resumable.then(|| parent.join(format!("{id}.resume.json")));

// sync.rs:219-230 — 尝试从 .resume.json 恢复状态
if let Some(ref path) = state_path {
    if let Ok(data) = fs::read(path) {
        if let Ok(saved) = serde_json::from_slice::<PersistedTransfer>(&data) {
            // ...恢复断点续传
        }
    }
}

// sync.rs:500-524 — 24 小时 TTL 清理（永远不会被调用到）
const INCOMPLETE_TRANSFER_RETENTION_SECONDS: u64 = 24 * 60 * 60;
pub fn cleanup_expired_transfers() {
    // 检查 .part 和 .resume.json 的修改时间...
    // 但 HistoryDB::new() 早已把它们删除了！
}
```

**矛盾**: `cleanup_expired_transfers` 被设计为在 24 小时内保留未完成的传输。但 `HistoryDB::new()` 在启动时立即删除所有内容。跨进程重启的断点续传实际上永远不会工作——它只在单次进程运行中的断连重连时有用。

---

## 3. 中危问题 (Medium)

### M0. macOS History 全量加载 10000 行 + 竞态条件

- **文件**: [History.tsx:247-301](macos/src/pages/History.tsx#L247-L301)

```typescript
const loadHistory = useCallback(async () => {
    setLoading(true);
    try {
        const result = await invoke<HistoryEntry[]>("get_history", {
            keyword: keyword || null,
            limit: 10000,   // ← 每次加载 10000 行
            offset: 0,      // ← 无服务端分页
        });
        setAllEntries(result);

        // 为所有图片加载缩略图，不仅仅是当前页！
        const imageIds = result
            .filter((e) => e.type === "image")
            .map((e) => e.id);
        if (imageIds.length > 0) {
            loadThumbnails(imageIds);  // ← 可能有数百个 IPC 调用
        }
    } catch (e) {
        console.error("Failed to load history:", e);
    }
}, [keyword, loadThumbnails]);
```

#### 问题分析

1. **IPC 开销**: 10000 行 × (~200 bytes/行) ≈ 2MB JSON 每 800ms 传输一次
2. **缩略图洪水**: 如果有 200 张图片，就会产生 200 个 `get_image_data` IPC 调用（即使只显示前 30 条）
3. **竞态条件**: 快速输入 "a" → "ab" → "abc"：
   ```
   T1: 请求 get_history(keyword="a")     → 响应在 T4 到达
   T2: 请求 get_history(keyword="ab")    → 响应在 T5 到达
   T3: 请求 get_history(keyword="abc")   → 响应在 T6 到达
   T4-T6: setAllEntries 被调用三次，最后一次覆盖前两次 — 但如果 T5 > T6 到达顺序？
   ```
4. **卸载后更新**: `running = false` 只在 `while (running)` 的顶部检查，当前迭代可能仍在进行并在卸载后调用 `setAllEntries`

Windows 版本正确地使用了 `get_history_page`（服务端分页）+ `historyRequestGeneration` 计数器来拒绝过期响应。

---

### M0b. History 列表项无法通过键盘访问

- **文件**: [History.tsx:550-608](macos/src/pages/History.tsx#L550-L608)

```tsx
<div
    className={`history-item${isNew ? " is-new" : ""}${selectedId === entry.id ? " restored" : ""}`}
    style={{ animationDelay: `${delay}ms` }}
    data-id={entry.id}
    onDoubleClick={() => handleRestore(entry.id)}   // ← 仅双击
    onContextMenu={(e) => {
        e.preventDefault();
        handleDelete(entry.id);                     // ← 仅右键菜单
    }}
>
```

`<div>` 不可聚焦，没有 `role`，没有 `tabIndex`，没有键盘事件处理器。仅支持键盘的用户完全无法操作历史记录。右键删除路径不可被发现。

---

### M10. macOS History DST 日期分组 Bug

- **文件**: [History.tsx:62-78](macos/src/pages/History.tsx#L62-L78)

```typescript
function getDateGroup(dateStr: string): DateGroup {
    const d = new Date(dateStr);
    const now = new Date();
    const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
    const yesterday = new Date(today);
    yesterday.setDate(yesterday.getDate() - 1);
    const itemDate = new Date(d.getFullYear(), d.getMonth(), d.getDate());
    const diffDays = Math.floor(
        (today.getTime() - itemDate.getTime()) / (1000 * 60 * 60 * 24),
        //                                           ^^^^^^^^^^^^^^^^^^
        //                                           固定 86400000ms
    );
```

夏令时：
- 春季前拨（"spring forward"）：这一天只有 23 小时（82,800,000ms）
- 秋季后拨（"fall back"）：这一天有 25 小时（90,000,000ms）

`diffDays` 用 86,400,000ms 除以不准确的时间差，可能差一天，导致 "today"/"yesterday"/"thisWeek" 分组显示错误。

Windows 版本已通过使用 UTC ordinals 修复此问题（见 Windows `History.tsx:337-344`）。

---

### M11. macOS History 混用两种 i18n 策略

- **文件**: [History.tsx:80-89](macos/src/pages/History.tsx#L80-L89)

```typescript
function getGroupLabel(group: DateGroup, locale: string): string {
    const labels: Record<DateGroup, Record<string, string>> = {
        today: { en: "Today", "zh-CN": "今天" },
        yesterday: { en: "Yesterday", "zh-CN": "昨天" },
        // ... 硬编码的翻译
    };
    return labels[group][locale] || labels[group]["en"];
}
```

同时页面上又混用了 `t()` 函数和大量 `locale === "zh-CN" ? "中文" : "English"` 三元表达式。类型标签 "Text"/"Image"/"File" 根本没有本地化。Windows 版本统一使用 `t()` 键查找。

---

### M12. 前端代码大量重复

以下文件在 macOS 和 Windows 之间**完全字节相同**（通过 `cmp` 验证）：

```
src/hooks/useI18n.ts       ← 完全相同
src/hooks/useTheme.ts       ← 完全相同
src/i18n/en.json            ← 完全相同
src/i18n/zh-CN.json         ← 完全相同
src/history-main.tsx        ← 完全相同
src/settings-main.tsx       ← 完全相同
```

`History.tsx` 和 `Settings.tsx` 约 70% 重复但有实质差异。任何修改需要在两个位置分别进行，极易出现差异漂移。

---

### M13. 滑块每次拖动都触发 IPC

- **文件**: [Settings.tsx:666-675](macos/src/pages/Settings.tsx#L666-L675)

```tsx
<input
    type="range"
    min={10}
    max={500}
    value={settings.history_limit}
    onChange={(e) =>
        update({ history_limit: parseInt(e.target.value) })
        // ↑ 每次 onChange（每个像素的拖动）触发一次 update_settings IPC
    }
/>
```

拖动滑块从 10 到 500 可以产生 50-100 次 `update_settings` IPC 调用，每次包括 JSON 序列化 → 进程间通信 → 文件写入 → toast 显示。

---

### M14. `App.tsx` 类型错误

- **文件**: [App.tsx:19](macos/src/App.tsx#L19)

```typescript
const paths: Record<IconName, React.ReactNode> = {
    //                           ^^^^^^^^^^^^^^
    // React 未导入！如果 tsconfig 中 allowUmdGlobalAccess 为 false，tsc 编译失败
```

---

### M15. macOS/Windows `refresh_peers` 协议不一致

**macOS** [Settings.tsx:185-195](macos/src/pages/Settings.tsx#L185-L195):
```typescript
await invoke("refresh_peers");                          // 丢弃返回值
const result = await invoke<PeersResponse>("get_peers");  // 单独获取
```

**Windows** [Settings.tsx:300-310](windows/src/pages/Settings.tsx#L300-L310):
```typescript
const result = await invoke<PeersResponse>("refresh_peers");  // 直接用返回值
```

`refresh_peers` 只能返回一种格式。其中一个前端对后端 API 的假设是错误的。

---

### M16. 配对对话框缺少无障碍语义

- **文件**: [Settings.tsx:776-830](macos/src/pages/Settings.tsx#L776-L830)

```tsx
{pairingOpen && (
    <div className="dialog-backdrop" onMouseDown={() => void closePairing()}>
        <div className="pair-dialog" onMouseDown={(event) => event.stopPropagation()}>
            {/* 普通 div — 无 role="dialog"，无 aria-modal，无焦点管理 */}
            <h2>Device pairing · {pairingStatus?.peer?.hostname}</h2>
            <div className="pairing-code" aria-label="配对验证码">
                {pairingStatus.peer.verification_code}
                {/* 六位数验证码 — 不在 aria-live 区域，屏幕阅读器不主动播报 */}
            </div>
            {/* ... */}
        </div>
    </div>
)}
```

缺失：
- `role="dialog"` 和 `aria-modal="true"` — 屏幕阅读器不知道这是模态对话框
- 焦点陷阱 — Tab 键可以移出对话框到背景元素
- Escape 关闭 — 只能用鼠标点击背景或按钮
- `aria-live="polite"` — 验证码变化时屏幕阅读器不播报

---

### M17. `read_text_data()` 函数命名和实现不匹配

- **文件**: [db.rs:789-798](macos/src-tauri/src/db.rs#L789-L798)

```rust
fn read_text_data(&self, stored: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    if let Some(reference) = decode_image_reference(stored) {
    //                       ^^^^^^^^^^^^^^^^^^^^^
    // 名为 read_text_data，却在检查 IMAGE 引用 magic bytes
        let encrypted = std::fs::read(resolve_file_reference_at(
            &self.image_history_dir, &reference,
        )?)?;
        return crypto::decrypt(&encrypted);
    }
    crypto::decrypt(stored)
}
```

这个函数被 `get_data()` 在 `entry_type == "text"` 时调用。它检查 `IMAGE_REFERENCE_MAGIC` 的原因是为了向后兼容旧数据库中错误地以图片引用格式存储的文本条目。但命名导致了维护困惑。

---

### M18. 其他中危问题汇总

**M1. 死代码** — 多处 `#[allow(dead_code)]` 标记的函数实际未被使用，包括 `spawn_helper`（引用不存在的 Swift 二进制文件）。

**M2. SyncEngine 使用非线程安全的 HashMap** — `SyncEngine` 的 `seen_messages`、`active_receives` 等字段是 `HashMap`，通过 `Arc<Mutex<>>` 包装才安全。如果有人直接使用裸 `SyncEngine`，编译器不会阻止。

**M4. ThumbnailCanvas base64 解码低效** — 每个 64×64 缩略图的 16KB base64 解码使用逐字节循环 ~16,000 次。

**M6. Settings 5 秒/1 秒轮询** — 设备列表每 5 秒轮询，配对状态每 1 秒轮询。设置窗口长时间打开时持续消耗资源。

**M7. ClipboardChangeDetector 平台差异** — macOS 使用 `NSPasteboard.changeCount`，Windows 使用 `GetClipboardSequenceNumber`。需要验证 Windows 实现。

**M8. `shared/art-direction.css` 用 `!important` 覆盖全局样式** — `letter-spacing: 0 !important` 在 `.app *` 上，任何未来需要调整字间距的元素都会被静默覆盖。

---

## 4. 低危问题 (Low)

### L1. Option 包裹必定初始化的字段

**文件**: [sync.rs:88](macos/src-tauri/src/sync.rs#L88)

```rust
pub struct SyncEngine {
    pub app_handle: Option<AppHandle>,  // 在 lib.rs:183 必定初始化
    pub db: Option<Arc<Mutex<HistoryDB>>>,  // 在 lib.rs:184 必定初始化
    // ...
}
```

导致各处需要不必要的 `None` 检查和 `unwrap()`。

### L2. `tray.rs` 图标 fallback 尺寸错误

已作为 H1.3 详细描述——1024 字节 vs 需要的 4096 字节。

### L3. 日志中记录 IP 和主机名

```rust
info!("Authenticated peer {} ({}) connected as {}", peer_addr, peer_info.tailscale_ip, peer_info.hostname);
```

某些部署场景中可能不希望记录这些地址信息。

### L4. 测试中 `unwrap()` 过多

测试 setup 使用 `unwrap()` 失败时错误信息不明确——不容易知道是哪个测试的哪一步失败了。

### L5. `clipboard_file.rs` macOS 特有代码

大部分文件剪贴板逻辑针对 macOS（使用 `NSFilenamesPboardType`），Windows 使用单独的 `CF_HDROP` 路径。代码共享度低。

### L6. `vite.config.ts` 完全重复

两个平台有几乎相同的 Vite 配置，可以提取共享。

### L7. `history_classifier.rs` UTF-8 边界处理

```rust
fn sample_prefix(text: &str) -> (&str, bool) {
    let mut end = MAX_SAMPLE_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;  // ← 如果文本在 16KB 内全为多字节字符，可能退到开头
    }
    (&text[..end], true)
}
```

对 ASCII 文本不会发生，但纯中文文本在极端情况下可能产生比预期更短的截断。正确处理了边界，只是极端长度下退得比较多。

### L8. 心跳间隔 30 秒

`HEARTBEAT_INTERVAL = 30s`，可能被 NAT 网关丢弃（常见超时 30-60 秒）。如果恰好碰上，连接会被中间设备静默丢弃而应用要等到 90 秒 idle timeout。

### L9. HTML 文件硬编码 `lang="en"`

```html
<html lang="en">
```

即使用户切换到中文，屏幕阅读器仍使用英文发音，`:lang()` CSS 选择器失效。

### L10. 未跟踪的 setTimeout

多处 `setTimeout` 没有在组件卸载时清理（`runDemo`、`newIds` glow、toast timers），在卸载后可能调用 `setState`。

---

## 5. 代码优雅性与可维护性

### 5.1 后端代码重复 — macOS vs Windows

macOS 和 Windows 的 Rust 源代码几乎完全相同：

| 文件 | macOS 行数 | Windows 行数 | 重复度 |
|------|-----------|-------------|--------|
| `network/mod.rs` | 3,237 | 3,237 | ~95% |
| `db.rs` | 2,091 | 2,091 | ~98% |
| `api.rs` | 1,152 | 1,152 | ~95% |
| `pairing.rs` | 748 | 748 | ~98% |
| `clipboard.rs` | 661 | 661 | ~90% |
| `sync.rs` | 629 | 629 | ~98% |
| `crypto.rs` | 574 | 574 | ~98% |
| `protocol.rs` | 567 | 567 | ~100% |
| **合计** | **~9,659** | **~9,659** | **~97%** |

**实质差异**仅存在于：
- `network/mod.rs` — 对等节点健康模型完全不同（Windows 有 `route_health()` 和 `request_peer_refresh(&mode)`）
- `api.rs` — Windows 多了 `test_connection` 命令
- `clipboard.rs` — Windows 有 `SYSTEM_RESUME_GAP_MS` 检测逻辑
- `lib.rs` — Windows 有背景通知轮询

**建议**: 提取共享 crate（`tailsync-core`），平台特定代码留在 `macos/` 和 `windows/` 中作为薄适配层。

### 5.2 前端代码重复

以下文件在 macOS 和 Windows 之间完全字节相同：
- `useI18n.ts`、`useTheme.ts`、`i18n/en.json`、`i18n/zh-CN.json`
- `history-main.tsx`、`settings-main.tsx`

`History.tsx` 和 `Settings.tsx` 有约 70% 重复但存在有意义的差异。

### 5.3 超大型文件

- **`network/mod.rs`** (3,237 行): 应拆分为 `connection_pool.rs`、`peer_discovery.rs`、`tcp_server.rs`、`peer_health.rs`
- **`db.rs`** (2,091 行): 应拆分为 `schema.rs`、`migrations.rs`、`queries.rs`、`file_storage.rs`
- **`api.rs`** (1,152 行): 每个 API 端点处理逻辑都在一个大 match 中，应使用路由分发模式

### 5.4 错误处理不一致

代码库混用了五种不同的错误类型：

| 类型 | 使用位置 | 问题 |
|------|---------|------|
| `Box<dyn std::error::Error>` | `db.rs`、`crypto.rs` | 丢失具体错误类型信息 |
| `Box<dyn std::error::Error + Send + Sync>` | `network/` 异步路径 | 与前一种不兼容 |
| `Result<_, String>` | `api.rs`、`sync.rs` | 丢失 backtrace |
| `ProtocolError` | `protocol.rs`、`secure.rs` | 设计最好的错误类型 |
| `rusqlite::Error` | `db.rs` 内部 | 调用者需要手动转换 |

转换模式不统一：
```rust
db.delete(id).map_err(|e| e.to_string())   // 丢失类型信息
db.get_data(id).map_err(|e| e.to_string())  // 同上
// vs
Err(format!("DB save file failed: {error}")) // 另一种风格
```

### 5.5 Settings 结构体职责过重

[`crypto::Settings`](macos/src-tauri/src/crypto.rs#L13-L38) 混合了：
- 用户偏好（主题、语言、通知开关）
- 安全关键数据（`trusted_peer_keys` — 公钥固定映射）
- 路由信息（`trusted_peer_addresses`、`paired_peer_endpoints`）

安全数据和用户偏好应该分离。更新语言设置不应该有接触密钥材料的风险。

### 5.6 硬编码常量

以下常量不能通过配置文件修改：
- `TCP_PORT = 19890`、`API_PORT = 19889` — 端口冲突时无法使用
- `FILE_CHUNK_SIZE = 1 * 1024 * 1024` — 高延迟链路可能需要更小的块
- `MAX_FILE_SIZE = 1 * 1024 * 1024 * 1024` — 1GB 上限可能偏大或偏小
- `HEARTBEAT_INTERVAL = 30s` — 某些网络环境需要更短

---

## 6. macOS vs Windows 平台差异分析

两个平台存在的实质性差异（非重复代码中的细微差异）：

| 差异领域 | macOS | Windows |
|---------|-------|---------|
| 对等节点健康 | `PeerHealthTracker` + `apply_peer_health` | `route_health()` + per-round status |
| 对等节点刷新 | `request_peer_refresh_and_wait()` | `request_peer_refresh(&mode)` |
| Sleep/wake | `CLIPBOARD_RECOVERY_GENERATION.reset()` | `SYSTEM_RESUME_GAP_MS` + 断开所有连接 |
| 剪贴板检测 | `NSPasteboard.changeCount` | `GetClipboardSequenceNumber` |
| 文件剪贴板 | 外部 Swift `clipboard-helper` 二进制 | 内联 `CF_HDROP` Win32 API |
| 托盘 | macOS: SwiftUI 菜单栏（主路径）/ Tauri 托盘（fallback） | Windows: Tauri 内置托盘 |
| Tailscale CLI | 仅 `/usr/local/bin`、`/opt/homebrew/bin` | 搜索 `Program Files` + `PATH` |
| 背景通知 | 依赖 SwiftUI shell | 内建轮询 `clipboard_version` |
| History 分页 | 客户端（`get_history` + `filtered.slice`） | 服务端（`get_history_page`） |
| History 筛选 | 仅关键词搜索 | 分类下拉 + 日期范围筛选 |
| Landing 页 | 静态营销页 | Canvas 动画页 |
| `refresh_peers` 返回 | 丢弃，再调 `get_peers` | 直接使用返回值 |

---

## 7. 正面发现

### 7.1 安全协议设计 ✅

- **Noise XX 握手**: 双向身份验证 + 前向安全性
- **密钥固定**: `trusted_peer_keys` HashMap 在首次配对后固定对端公钥，有效防御 MITM
- **配对验证码**: 从 Noise 握手 hash 派生 6 位数字码（HKDF + X25519 公钥排序），双方必须确认一致
- **传输加密**: ChaCha20-Poly1305 AEAD（snow/ring），每帧 blake3 校验和
- **静止数据加密**: AES-256-GCM，256 位随机密钥存储在 OS keychain（文本/图片）

### 7.2 可靠性机制 ✅

- **可靠事件投递**: 序列号 + 最多 4 次指数退避重试 + `MessageId` 去重（每对端独立作用域）
- **断点续传**: 文件传输使用 `FileChunkPayload` + `FileResume`/`FileAck` — 可在断连重连后恢复
- **连接池**: 持久 TCP 连接避免每次复制的握手开销
- **竞速连接**: 同时尝试 LAN 和 Tailscale，LAN 优先 250ms 后 fallback
- **心跳检测**: 30 秒心跳 + 90 秒空闲超时
- **Sleep/Wake 恢复**: CLIPBOARD_RECOVERY_GENERATION 触发剪贴板监视器重置

### 7.3 代码质量 ✅

- 全面的单元测试覆盖（`db.rs` 27 个测试、`protocol.rs` 7 个测试、`network/mod.rs` 16 个测试、`history_classifier.rs` 5 个测试、`pairing.rs` 4 个测试、`secure.rs` 4 个测试）
- 所有 SQL 查询使用参数化语句
- 配置保存使用原子写入（`tmp file → rename`）
- 大文件存储在磁盘（文件引用），而非 SQLite BLOB — 防止数据库膨胀
- `resolve_file_reference_at()` 拒绝包含 `../` 的路径 — 路径遍历防护
- `sanitize_history_file_name()` 移除控制字符、Windows 禁用字符、trim 空格/句点
- `sample_prefix()` 正确处理 UTF-8 字符边界
- `ConnectionLimiter` 每 IP 8 连接 / 总共 64 连接限制
- `history_classifier` 自动分类：website/code/command/path/structured_data/image/file/text

### 7.4 架构设计 ✅

- 二进制帧协议（magic bytes → version → flags → command → sequence → payload length → payload → blake3 checksum）
- 文件块专用 `bulk` 通道 — 不阻塞优先级消息（文本/图片/心跳）
- 完整的中英文国际化
- 多主题支持（system/light/dark × 5 个配色方案 = tailsync/ocean/forest/rose/high-contrast）

---

## 8. 总结与优先级

### 统计

| 严重性 | 数量 | 关键主题 |
|--------|------|----------|
| Critical | 5 | DEK 竞态、Frame::new panic、process::exit、DPAPI 泄漏、文件明文存储 |
| High | 11 | unwrap 调用、空 panic guard、shadow filter 泄漏、API 无认证、路径遍历、断点续传矛盾、SQL LIKE、图片验证 bypass、macOS History 全量加载+竞态、键盘无障碍、前端重复 |
| Medium | 17 | 死代码、DST Bug、i18n 不一致、滑块 IPC 泛滥、配对对话框无障碍、refresh_peers 不一致、React 类型错误、DIB 在 macOS 上构建、per-chunk flush、read_text_data 命名错误 |
| Low | 10 | 硬编码常量、fallback 图标尺寸、未清理计时器、lang 属性固定、日志隐私 |

### 优先修复建议（按顺序）

1. **C5** — 加密文件历史 payload（目前明文存储，与其他数据模型矛盾）
2. **C1** — 修复 DEK 初始化竞态（用 `get_or_init` 包裹读/生成/写 keychain）
3. **C2** — `Frame::new()` 返回 `Result` 而非 panic
4. **C3** — `std::process::exit(0)` → 优雅关闭信号
5. **H8** — 修复空 panic guard — 将实际的剪贴板操作移入 guard
6. **H11** — 统一 incoming 目录清理和断点续传设计
7. **H9** — API 服务器添加行长度限制和基本认证
8. **H10** — 修复 `restore_file_to_clipboard` 路径遍历风险
9. **H4** — shadow filter 改用 `HashMap<Hash, Instant>` 并添加基于时间的过期
10. **M0** — macOS History 改用服务端分页 + 请求 generation 守卫
11. **M0b** — History 列表项增加键盘可访问性
12. **M15** — 统一 `refresh_peers` 的前端/后端协议
13. **整体** — 提取共享 Rust crate 和共享前端包，减少约 15,000 行重复代码
14. **整体** — 拆分 `network/mod.rs` (3,237 行) 为多个模块

### 整体评价

TailSync v2 是一个设计良好、实现扎实的跨平台剪贴板同步工具。安全协议设计是专业级的：Noise XX 握手 + 公钥固定 + 验证码配对提供了强大的对等端认证。可靠事件投递系统和断点续传是成熟的分布式系统设计。

最主要的风险集中在**并发安全性**（DEK 竞态条件）、**panic 恢复能力**（多个 `unwrap()` 和 `expect()` 在生产路径中）和**平台代码重复**（约 15,000 行后端代码 ~97% 重复，多个前端文件完全字节相同）。

作为一个 v2.0.0 版本，代码库在功能和设计上是成熟的，但工程化方面（代码组织、错误处理一致性、平台代码共享）还有很大的改进空间。
