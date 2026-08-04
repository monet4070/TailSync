# TailSync v2.0.0 代码审查整改方案

> 状态：可执行缺陷整改已完成，产品决策与架构重构除外
> 日期：2026-07-31
> 适用范围：`macos/`、`windows/`、`shared/`
> 输入依据：`CODE_REVIEW_REPORT.md`、代码逐项核验、两端 TypeScript 类型检查、Windows Rust 测试

## 1. 文档目标

本文档不是对原审查报告的机械执行清单，而是经过代码核验后的完整整改方案。目标是：

1. 区分真实缺陷、严重度夸大、设计取舍和误报。
2. 优先消除可能导致历史数据永久无法解密的问题。
3. 给出可以直接进入开发的接口、代码结构、测试和迁移方案。
4. 避免将安全修复、产品决策和大规模架构重构混在同一个版本中。

本文中的代码分为两类：

- **参考实现**：可以按现有技术栈落地，但仍需在实际分支中编译和测试。
- **接口草案**：用于固定模块边界和数据格式，不应未经验证直接复制到生产环境。

## 2. 总体结论

原报告发现了一批真实问题，但严重度整体偏高，并包含若干明确误报。原报告的 5 个 Critical 均不应直接按其原始描述定为当前 Critical。

真正的最高优先级问题是原报告遗漏的密钥错误处理：

- `get_dek()` 将 `read_keychain()` 的所有错误都视为“密钥不存在”。
- Keychain/DPAPI 权限拒绝、瞬态失败或损坏可能触发生成并覆盖新 DEK。
- 一旦覆盖，旧文本和图片历史将永久无法解密。
- Windows `.dek` 当前直接写入，不具备原子替换和跨进程初始化保护。

另一个遗漏问题是 `Settings::load().unwrap_or_default()`：配置损坏时会静默丢失配对信任和地址信息，后续保存可能覆盖原文件。

## 3. 优先级定义

| 优先级 | 含义 | 发布要求 |
|---|---|---|
| P0 | 可能永久破坏密钥或用户数据 | 独立安全修复，优先发布 |
| P1 | 明确的安全边界、核心可靠性或资源耗尽问题 | 下一个修复版本完成 |
| P2 | 功能正确性、性能、无障碍和错误恢复问题 | 分模块迭代完成 |
| P3 | 架构、重复代码和长期维护问题 | 行为稳定后实施 |
| ACCEPT | 已披露且符合当前产品范围的设计取舍 | 记录风险，不立即改代码 |
| REJECT | 误报、没有证据或收益不足 | 不按报告建议修改 |

## 4. 全量问题核验矩阵

### 4.1 Critical

| 编号 | 核验结论 | 修正优先级 | 处理决定 |
|---|---|---:|---|
| C1 DEK 初始化竞态 | 代码竞态真实存在，但当前在启动主线程预初始化，因此当前路径难触发 | P0/P1 | 与密钥状态机一起修复；同时处理跨进程首次启动竞争 |
| C2 `Frame::new()` panic | API 设计不理想；当前动态 payload 基本先经过校验，没有已知生产触发链 | P2 | 改为 `try_new()`，统一命令级 payload 校验 |
| C3 `process::exit(0)` | 跳过协调关闭属实；“WAL 损坏”不成立 | P2 | 回复 API 后发出统一 shutdown 信号，再由 Tauri 退出 |
| C4 DPAPI 失败路径泄漏 | 报告缺少 Windows API 契约依据；不能定为 Critical | REJECT/P3 | 不作为漏洞修复；可用 RAII 缩小 unsafe 审计面 |
| C5 文件历史明文 | 事实成立，但 README 已明确披露并建议 FileVault/BitLocker | ACCEPT 或产品 P2 | 先做安全 ADR；需要应用级静态加密时再实施分块 AEAD |

### 4.2 High

| 编号 | 核验结论 | 修正优先级 | 处理决定 |
|---|---|---:|---|
| H1 生产路径 `unwrap/expect` | 部分属实，但报告按语法计数推导崩溃风险过度 | P2 | 只处理输入边界和可恢复启动错误，不清除测试中的正常 `unwrap` |
| H2 迁移 hash 不匹配后继续 | 现有行为存在；AEAD 解密成功已验证密文完整性，stored hash 不是可信安全证据 | P2 | 增加迁移问题表、用户可见诊断和可重试状态，不宣称“篡改” |
| H3 缺少速率限制 | 属实；历史有条数裁剪，但缺少字节、吞吐和并发传输配额 | P1 | 每认证 peer 令牌桶、并发传输上限、全局磁盘字节配额 |
| H4 shadow filter 无界 `Vec` | 属实 | P1 | 改为有界、带计数和 TTL 的哈希表 |
| H5 SQL LIKE 未转义 | 字面搜索语义错误属实；报告的 SQL DoS 描述夸大 | P2 | 转义 `\`、`%`、`_`，显式 `ESCAPE '\'` |
| H6 图片迁移绕过尺寸校验 | 属实；可写入畸形图片并在缩略图/恢复阶段产生错误状态，但当前代码不足以证明稳定的 release 越界 panic | P1 | 建立唯一 `PackedImage` 解析器，所有写入/恢复/缩略图路径复用 |
| H7 Keychain hex `unwrap_or_default` | 属实，且属于 P0 密钥错误处理的一部分 | P0 | 无效 hex 返回 `Corrupt`，禁止空 key 进入缓存 |
| H8 panic guard 是空操作 | 属实；不是可利用漏洞，但任务 panic 后核心功能静默停止 | P1 | 删除空 guard，用 supervisor 监控并重启 clipboard task |
| H9 API 无认证、无长度上限 | 属实且项目文档已披露；报告中的 `curl` 示例不适用于 JSON-lines 原始 TCP | P1 | 先加能力令牌、长度/超时/并发限制；长期改 ACL socket/pipe |
| H10 临时文件路径拼接 | macOS `temp_dir().join(fname)` 和 Windows `temp_dir()/tailsync` 后再 `join(fname)` 都不安全；报告给出的新 `migrate_entry` 利用链走不通 | P1/P2 | 两个平台的 legacy BLOB 都必须通过受控目录和统一文件名清理 |
| H11 启动清空 incoming | 当前明确只承诺运行期续传，不是功能 bug | ACCEPT | 保持现状并删除矛盾注释/冗余 TTL；如扩展跨重启续传另立项目 |

### 4.3 Medium

| 编号 | 核验结论 | 修正优先级 | 处理决定 |
|---|---|---:|---|
| M0 macOS History 全量加载与竞态 | 属实，极端规模被报告放大 | P2 | 复用 Windows 服务端分页、generation guard、可见缩略图加载 |
| M0b History 无键盘操作 | 属实 | P2 | 使用真实按钮/操作组，恢复和删除都可聚焦、可发现 |
| M10 DST 日期分组 | today/yesterday 已直接比较；7/30 天边界仍可能受 DST 影响 | P2 | 使用本地日期对应的 UTC ordinal |
| M11 i18n 混用 | 属实 | P2 | 所有显示文本进入字典，禁止 locale 三元表达式 |
| M12 前端重复 | 属实 | P3 | 先统一行为，再抽取共享 hooks、i18n、页面组件和平台 adapter |
| M13 滑块持续 IPC | 属实，两平台都有 | P2 | 本地 draft 即时显示，`pointerup/blur/键盘提交` 时保存 |
| M14 `React.ReactNode` 类型错误 | 明确误报，两端类型检查通过 | REJECT | 可改为 `import type { ReactNode }` 统一风格，但不是修复 |
| M15 `refresh_peers` 协议不一致 | 明确误报；两个后端均返回 peer snapshot | P3 | 删除 macOS 冗余的第二次 `get_peers` 调用 |
| M16 配对模态框无障碍 | 属实，两端都存在 | P2 | 原生 `<dialog>` 或共享 Modal，焦点管理、Escape、aria-live |
| M17 `read_text_data` 命名 | 函数名本身并未错误，内部兼容旧 image reference 需要解释 | P3 | 改名为 `read_text_payload_compat` 并补迁移注释/测试 |
| M1 dead code | 部分存在；`spawn_helper` 相关 Swift 源码并非不存在 | P3 | 按实际构建入口删除或用 feature/cfg 明确保留原因 |
| M2 HashMap 非线程安全 | 明确误报，Rust 所有权和 `Arc<Mutex<_>>` 会阻止数据竞争 | REJECT | 不改为并发容器 |
| M4 base64 缩略图解码 | 实现存在但不是主要瓶颈 | P3 | 优先批量缩略图 IPC；必要时改二进制资源协议 |
| M6 Settings 轮询 | 属实但负载较小 | P3 | Tauri event 为主，窗口可见时低频兜底轮询 |
| M7 ClipboardChangeDetector 差异 | 只是“需要验证”，不是已发现 bug；现有 Windows 序列号测试通过 | REJECT | 保留真实设备 sleep/wake 验收 |
| M8 全局 `!important` | 维护性风险属实，且当前文件存在用户改动 | P3 | 后续限定选择器范围，不在安全整改中触碰 |

### 4.4 Low

| 编号 | 核验结论 | 修正优先级 | 处理决定 |
|---|---|---:|---|
| L1 必定初始化字段使用 Option | 当前构造时序需要 setup 后注入，不是 bug | P3 | 后续通过 `SyncEngine::attach_runtime` 显式状态化 |
| L2 tray fallback 长度 | 1024 字节不符合 32x32 RGBA；不会在 `Image::new` 当场产生报告声称的 UB | P3 | 改为 4096 字节静态透明图标 |
| L3 日志记录 IP/hostname | 属于隐私策略 | P2/P3 | 默认 info 记录 hostname，IP 降为 debug 或哈希化 |
| L4 测试 `unwrap` | 测试中属于常规写法 | REJECT | 只改善难定位的 fixture 错误上下文 |
| L5 平台剪贴板代码不同 | OS API 天然不同，不是低共享度缺陷 | REJECT | 共享接口，不强行共享实现 |
| L6 Vite 配置重复 | 属实但规模很小 | P3 | 抽取根级 `vite.shared.ts` |
| L7 UTF-8 边界回退 | 明确误报，UTF-8 最多回退 3 字节 | REJECT | 保留现有实现和边界测试 |
| L8 30 秒心跳 | 没有故障证据；30 秒是合理默认值 | ACCEPT | 增加可观测性后再决定是否可配 |
| L9 HTML `lang="en"` | 属实 | P2 | locale 变化时同步 `document.documentElement.lang` |
| L10 未清理 timeout | 部分属实 | P2 | 统一 tracked timeout hook 或 effect cleanup |

### 4.5 架构与维护性

| 编号 | 核验结论 | 优先级 | 处理决定 |
|---|---|---:|---|
| 5.1 后端重复 | 属实，多个核心 Rust 文件完全相同 | P3 | 建立 Cargo workspace 和 `tailsync-core`，平台能力通过 trait 注入 |
| 5.2 前端重复 | 属实 | P3 | 建立共享前端包，页面通过 platform adapter 获取差异能力 |
| 5.3 超大文件 | 属实 | P3 | 按职责拆分，不以行数为唯一标准 |
| 5.4 错误处理不一致 | 属实 | P2/P3 | 模块内使用 typed error，IPC/API 边界统一序列化 |
| 5.5 Settings 职责过重 | 属实，但 trusted peer key 是公钥而非秘密 | P3 | 拆为 Preferences、TrustStore、RouteStore |
| 5.6 硬编码常量 | 部分属实 | P3 | 协议安全上限保持常量；端口/策略参数仅在发现协议支持后开放配置 |

## 5. 新增漏报问题

### N1. KeyStore 错误被误判为 NotFound

**优先级：P0**

当前逻辑：

```rust
let key = match read_keychain() {
    Ok(k) => k,
    Err(_) => {
        let key = generate_key()?;
        write_keychain(&key)?;
        key
    }
};
```

这里将不存在、权限拒绝、数据损坏、DPAPI 失败和 I/O 错误全部折叠。正确不变量是：**只有确定为 NotFound 才允许生成**。

### N2. Settings 损坏后静默回退

**优先级：P1**

`Settings::load().unwrap_or_default()` 会将解析失败视为首次启动。默认配置的信任表为空，因此它不会直接信任攻击者，但会造成配对信息丢失，并可能在下一次保存时覆盖原配置。

### N3. DeviceIdentity 私钥文件缺少显式权限和耐久写入策略

**优先级：P1/P2（防御纵深，不等同于明文私钥泄漏）**

`identity-v1.bin` 已使用 DEK 进行 AEAD 加密，读取后也会校验公钥和私钥均为 32 字节，因此“身份私钥完全无保护”并不准确。实际缺口是：

- 文件和数据目录没有由 TailSync 显式设置最小权限，只依赖 umask 或 AppData 继承 ACL。
- 临时文件写入后直接 rename，没有 `flush/sync_all` 耐久保证。
- 身份读取、解密、格式损坏和权限拒绝仍通过模糊的 `Box<dyn Error>` 传播。
- 没有独立测试证明密钥访问失败或身份文件损坏时不会生成并覆盖设备身份。

该项应与 P0 KeyStore 一起设计，但不应把已经加密的 identity 文件描述成明文私钥。

## 6. P0：密钥安全专项

### 6.1 错误模型

以下为参考实现：

```rust
use thiserror::Error;

pub type DataKey = [u8; 32];

#[derive(Debug, Error)]
pub enum KeyStoreError {
    #[error("encryption key does not exist")]
    NotFound,
    #[error("encryption key access was denied: {0}")]
    AccessDenied(String),
    #[error("encryption key is corrupt: {0}")]
    Corrupt(String),
    #[error("encryption key store I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("encryption key platform operation failed ({code}): {message}")]
    Platform { code: i64, message: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateOutcome {
    Created,
    AlreadyExists,
}

pub trait KeyStore: Send + Sync {
    fn read(&self) -> Result<DataKey, KeyStoreError>;
    fn create(&self, key: &DataKey) -> Result<CreateOutcome, KeyStoreError>;
}
```

### 6.2 进程内原子初始化

不建议缓存 `Result`，因为瞬态 AccessDenied 在用户授权后应该可以重试。使用 `OnceLock<DataKey>` 加初始化锁：

```rust
use std::sync::{Mutex, OnceLock};

static DEK: OnceLock<DataKey> = OnceLock::new();
static DEK_INIT: Mutex<()> = Mutex::new(());

pub fn get_dek() -> Result<&'static DataKey, KeyStoreError> {
    if let Some(key) = DEK.get() {
        return Ok(key);
    }

    let _guard = DEK_INIT
        .lock()
        .map_err(|_| KeyStoreError::Corrupt("DEK initialization lock was poisoned".into()))?;

    if let Some(key) = DEK.get() {
        return Ok(key);
    }

    let store = PlatformKeyStore::new();
    let key = load_or_create_key(&store)?;
    DEK.set(key)
        .map_err(|_| KeyStoreError::Corrupt("DEK was initialized twice".into()))?;
    DEK.get()
        .ok_or_else(|| KeyStoreError::Corrupt("DEK initialization did not persist".into()))
}

fn load_or_create_key(store: &dyn KeyStore) -> Result<DataKey, KeyStoreError> {
    match store.read() {
        Ok(key) => Ok(key),
        Err(KeyStoreError::NotFound) => {
            let generated = generate_data_key()?;
            match store.create(&generated)? {
                CreateOutcome::Created => Ok(generated),
                CreateOutcome::AlreadyExists => store.read(),
            }
        }
        Err(error) => Err(error),
    }
}
```

`generate_data_key()` 必须直接返回 `[u8; 32]`，不得经过可变长度 `Vec<u8>`。

### 6.3 macOS Keychain

正常启动禁止使用覆盖语义：

```text
read:
  SecItemCopyMatching 成功 -> 解码 -> 长度校验
  errSecItemNotFound       -> NotFound
  errSecAuthFailed         -> AccessDenied
  其他 OSStatus            -> Platform

create:
  SecItemAdd 成功          -> Created
  errSecDuplicateItem      -> AlreadyExists
  errSecAuthFailed         -> AccessDenied
  其他 OSStatus            -> Platform
```

首选直接使用 Security.framework，而不是解析 `security` CLI 的 stderr。如果暂时保留 CLI，也必须严格识别 `errSecItemNotFound` 和 duplicate，且创建时去掉 `-U`。

Hex 解析不得回退为空：

```rust
fn decode_hex_key(value: &str) -> Result<DataKey, KeyStoreError> {
    let bytes = hex::decode(value.trim())
        .map_err(|error| KeyStoreError::Corrupt(format!("invalid hex: {error}")))?;
    bytes
        .try_into()
        .map_err(|bytes: Vec<u8>| KeyStoreError::Corrupt(
            format!("expected 32 bytes, got {}", bytes.len())
        ))
}
```

### 6.4 Windows DPAPI

Windows 需要同时解决跨进程竞争和写入中断：

1. 获取当前用户范围的命名 Mutex 或 `.dek.lock` 独占文件锁。
2. 获得锁后重新读取 `.dek`。
3. 只有确认为不存在时才生成。
4. DPAPI 加密结果写入同目录随机临时文件。
5. `write_all`、`flush`、`sync_all`。
6. 原子 rename 到不存在的 `.dek`。
7. 如果目标已存在，删除临时文件并重新读取胜出者的 key。
8. DPAPI 解密失败或文件损坏时保留原文件，禁止覆盖。

接口草案：

```rust
fn create_dpapi_file(path: &Path, protected: &[u8]) -> Result<CreateOutcome, KeyStoreError> {
    let _lock = acquire_key_file_lock(path)?;
    if path.exists() {
        return Ok(CreateOutcome::AlreadyExists);
    }

    let temp = path.with_extension(format!("dek.{}.tmp", std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    use std::io::Write;
    file.write_all(protected)?;
    file.flush()?;
    file.sync_all()?;
    std::fs::rename(&temp, path)?;
    Ok(CreateOutcome::Created)
}
```

实际实现还要保证失败时清理本进程临时文件，但不得删除既有 `.dek`。

### 6.5 DeviceIdentity 存储闭环

身份文件继续使用现有 DEK 加密格式，避免无必要迁移；补充权限、原子写和失败关闭：

```rust
fn save_identity_atomic(path: &Path, encrypted: &[u8]) -> Result<(), IdentityError> {
    use std::io::Write;

    let temporary = path.with_extension(format!("bin.{}.tmp", std::process::id()));
    let mut file = open_private_temp_file(&temporary)?;
    file.write_all(encrypted)?;
    file.flush()?;
    file.sync_all()?;
    std::fs::rename(&temporary, path)?;
    restrict_private_file(path)?;
    Ok(())
}

#[cfg(unix)]
fn restrict_private_file(path: &Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}
```

Windows 不应照搬 Unix mode；`restrict_private_file()` 的 Windows adapter 应创建或验证仅当前用户 SID、SYSTEM 和必要管理员可访问的 DACL。默认 AppData ACL 可以作为兼容兜底，但应在真实安装包上验收继承权限。

身份加载状态机：

```text
文件不存在                 -> 生成新身份并 create-only 保存
文件存在且解密/格式校验成功 -> 使用既有身份
DEK AccessDenied           -> 失败关闭，禁止生成身份
AEAD 解密失败              -> IdentityError::Corrupt，保留原文件
Base64/版本/长度错误        -> IdentityError::Corrupt，保留原文件
文件读取失败               -> IdentityError::Io，禁止覆盖
```

还应验证 public/private 均为规范 32 字节，并在可用的 X25519 API 下验证 public key 与 private key 对应。重置设备身份必须是单独的显式破坏性操作，因为它会使所有已配对设备上的公钥固定失效。

### 6.6 Settings 失败关闭

```rust
#[derive(Debug, Error)]
pub enum SettingsLoadError {
    #[error("settings file could not be read: {0}")]
    Io(#[from] std::io::Error),
    #[error("settings file is corrupt: {0}")]
    Corrupt(#[from] serde_json::Error),
}

pub fn load() -> Result<Self, SettingsLoadError> {
    let path = settings_path();
    if !path.exists() {
        return Ok(Self::default());
    }
    let data = std::fs::read_to_string(path)?;
    let mut settings: Self = serde_json::from_str(&data)?;
    settings.connection_mode = normalize_connection_mode(settings.connection_mode);
    Ok(settings)
}
```

启动处不得再使用 `unwrap_or_default()`。首个版本可选择显式启动失败并显示文件路径；更完整的版本可进入只读恢复模式，但不得用默认信任库继续保存。

### 6.7 P0 测试

- 32 个线程并发调用，只生成和写入一次，所有调用得到相同 key。
- 两个 store 实例模拟跨进程竞争，失败方读取胜出 key。
- NotFound 才调用 RNG 和 `create()`。
- AccessDenied、Corrupt、Io、Platform 均不调用 `create()`。
- 0、31、33 字节以及无效 hex 均返回 Corrupt。
- 写入失败时生成的 key 不进入 `OnceLock`。
- 已有 v2 key 能在重启后解密旧历史。
- `.dek` 内容和 mtime 在解密失败后不变化。
- Settings JSON 损坏后不回退、不保存、不修改原文件。
- identity 权限拒绝、AEAD 失败、版本错误和长度错误均不生成新身份。
- identity 临时写入失败不替换旧身份，旧身份仍可加载。
- macOS identity 文件权限为 `0600`；Windows 安装环境 DACL 满足当前用户范围要求。

## 7. P1：本地 API 安全与关闭流程

### 7.1 短期兼容方案

保留 JSON-lines TCP，但增加：

- 每次启动随机 256-bit 能力令牌。
- 请求必须包含 token，并使用常量时间比较。
- 控制请求最大 1 MiB；文件迁移从 JSON base64 拆为流式接口。
- 首字节、整行读取和写响应超时。
- API 连接并发 semaphore。
- 每个连接只处理一个请求并立即关闭。
- `accept` 瞬态错误记录后继续，只有 listener 永久失效才退出并由 supervisor 重启。
- `quit`、`clear_history`、`migrate_entry` 视为高权限命令。

受限读取参考实现：

```rust
const MAX_API_LINE: usize = 1024 * 1024;
const API_READ_TIMEOUT: Duration = Duration::from_secs(5);

async fn read_request(reader: BufReader<OwnedReadHalf>) -> Result<Request, ApiError> {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt};
    let mut limited = reader.take((MAX_API_LINE + 1) as u64);
    let mut bytes = Vec::new();
    tokio::time::timeout(API_READ_TIMEOUT, limited.read_until(b'\n', &mut bytes))
        .await
        .map_err(|_| ApiError::ReadTimeout)??;

    if bytes.len() > MAX_API_LINE {
        return Err(ApiError::RequestTooLarge);
    }
    if !bytes.ends_with(b"\n") {
        return Err(ApiError::IncompleteRequest);
    }
    Ok(serde_json::from_slice(&bytes)?)
}
```

注意：迁移 1 GiB 文件不能继续使用单行 base64。应建立 `begin_import`、`import_chunk`、`finish_import`，或仅允许受控本地文件路径并验证路径所有权。

### 7.2 长期传输方案

- macOS：Unix domain socket，文件权限 `0600`，SwiftUI shell 连接 socket。
- Windows：Named Pipe，ACL 仅允许当前用户 SID。
- Tauri WebView 继续优先使用 invoke，不经过本地 TCP。
- 迁移 CLI 使用同一受保护 IPC，不开放固定 loopback 端口。

### 7.3 优雅关闭

用 `watch` 通道建立现有依赖可实现的 shutdown coordinator：

```rust
#[derive(Clone)]
pub struct Shutdown {
    tx: tokio::sync::watch::Sender<bool>,
}

impl Shutdown {
    pub fn request(&self) {
        let _ = self.tx.send(true);
    }
}

async fn run_server(mut shutdown: watch::Receiver<bool>) -> Result<(), ServerError> {
    loop {
        tokio::select! {
            _ = shutdown.changed() => break,
            accepted = listener.accept() => handle_accept_result(accepted).await,
        }
    }
    Ok(())
}
```

`quit` 的顺序应为：

1. 生成成功响应。
2. 写完并 flush 响应。
3. 请求 shutdown。
4. 停止接收新连接和新文件。
5. 等待有界时间让 writer flush。
6. 关闭连接池。
7. 调用 `AppHandle::exit(0)`。

## 8. P1：统一输入验证

### 8.1 `Frame::try_new`

```rust
impl Frame {
    pub fn try_new(
        command: Command,
        flags: u8,
        sequence: u32,
        payload: Vec<u8>,
    ) -> Result<Self, ProtocolError> {
        let limit = command.payload_limit();
        if payload.len() > limit {
            return Err(ProtocolError::CommandPayloadTooLarge {
                command,
                actual: payload.len(),
                limit,
            });
        }
        Ok(Self {
            command,
            flags: Flags(flags),
            sequence,
            payload,
        })
    }
}
```

替换所有生产调用；测试可通过 `expect("fixture payload is valid")` 明确表达不变量。完成迁移后删除 `Frame::new()`，避免两个构造入口漂移。

### 8.2 `PackedImage`

当前可确认的数据流是：

```text
本地进程连接 127.0.0.1:19889 的原始 JSON-lines TCP
  -> cmd=migrate_entry, type=image
  -> api.rs 调用 add_image_migrated()
  -> 畸形 payload 未经 validate_packed_image 即加密写入 DB
  -> get_image_data / restore_entry 解密并直接读取前 8 字节宽高
  -> thumbnail_rgba 或 Tauri clipboard image 接收到不一致的维度和 RGBA
```

这里不能使用普通 HTTP `curl` 直接触发，因为服务不是 HTTP；但任意能打开本地 TCP socket 的进程都可以发送 JSON-lines。H9 的无认证使该输入边界仍然成立。

风险需要准确描述：当前 `thumbnail_rgba()` 在复制像素前有 `si + 3 < rgba.len()`，所以“尺寸不匹配必然产生 slice 越界 panic”并不成立；`w = h = 0` 当前也只会得到空缩略图。不过畸形数据会被永久写入、可能生成透明/失真缩略图，并把尺寸与 buffer 不一致的 `Image` 交给平台插件；极端尺寸的索引乘法也缺少 checked arithmetic，在 debug 或未来代码变动中可能 panic。因此必须在持久化之前拒绝，而不是依赖所有消费者各自容错。

```rust
const MAX_IMAGE_PIXELS: usize = MAX_IMAGE_PAYLOAD_SIZE / 4;

pub struct PackedImage<'a> {
    pub width: u32,
    pub height: u32,
    pub rgba: &'a [u8],
}

impl<'a> TryFrom<&'a [u8]> for PackedImage<'a> {
    type Error = ProtocolError;

    fn try_from(value: &'a [u8]) -> Result<Self, Self::Error> {
        let width = u32::from_le_bytes(
            value.get(0..4)
                .ok_or(ProtocolError::InvalidImage)?
                .try_into()
                .map_err(|_| ProtocolError::InvalidImage)?,
        );
        let height = u32::from_le_bytes(
            value.get(4..8)
                .ok_or(ProtocolError::InvalidImage)?
                .try_into()
                .map_err(|_| ProtocolError::InvalidImage)?,
        );
        if width == 0 || height == 0 {
            return Err(ProtocolError::InvalidImage);
        }
        let pixels = (width as usize)
            .checked_mul(height as usize)
            .filter(|pixels| *pixels <= MAX_IMAGE_PIXELS)
            .ok_or(ProtocolError::InvalidImage)?;
        let expected = pixels.checked_mul(4).ok_or(ProtocolError::InvalidImage)?;
        let rgba = value.get(8..).ok_or(ProtocolError::InvalidImage)?;
        if rgba.len() != expected {
            return Err(ProtocolError::InvalidImage);
        }
        Ok(Self { width, height, rgba })
    }
}
```

以下入口必须全部调用它：

- 网络 `ImagePayload`。
- `migrate_entry(type=image)`。
- `add_image_migrated()`。
- `restore_entry`。
- `get_image_data` 和 thumbnail。
- 历史 backfill 对旧图片的检测。

回归测试至少覆盖：

- payload 少于 8 字节。
- width 或 height 为 0。
- `width * height` 和 `* 4` 溢出。
- 声明尺寸与 RGBA 长度不符。
- 超过 `MAX_IMAGE_PIXELS`。
- 畸形 `migrate_entry` 返回稳定错误，DB 不增加 row。
- 旧 DB 中已有畸形图片时，thumbnail/restore 返回错误而不是 panic。

### 8.3 安全临时文件

禁止 `temp_dir().join(untrusted_name)`。新增统一接口：

当前两个危险分支分别是：

```text
macOS:   temp_dir().join(fname)
Windows: temp_dir().join("tailsync").join(fname)
```

绝对路径会替换 base，`..` 也可能逃逸。新 `migrate_entry(type=file)` 会先生成安全的历史文件引用，正常恢复再经过 `materialize_clipboard_file()`，因此报告给出的新迁移利用链不成立；真正需要封堵的是 legacy BLOB、旧数据库和手工损坏数据进入 `restore_file_to_clipboard()` 的兼容分支。两平台都要删除旁路，不能只修 macOS。

```rust
pub fn materialize_clipboard_bytes(
    data: &[u8],
    original_name: &str,
) -> Result<PathBuf, FileStorageError> {
    use std::io::Write;

    let safe_name = sanitize_history_file_name(original_name);
    let directory = get_clipboard_files_dir().join(format!("{:016x}", rand::random::<u64>()));
    std::fs::create_dir_all(&directory)?;
    let target = directory.join(safe_name);
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)?
        .write_all(data)?;
    Ok(target)
}
```

legacy BLOB 恢复也必须调用该接口，不能保留特殊旁路。

## 9. P1：资源配额与回环过滤

### 9.1 配额模型

当前 `HistoryDB::trim()` 主要按条目数裁剪：文件条目上限约为 `(history_limit / 10).max(10)`。在 UI 允许的 `history_limit = 10..500` 范围内，仍可能保留 10 到 50 个文件；结合单文件 1 GiB 上限，仅 `file-history/` 理论上就可能占用约 10 到 50 GiB。总条目 cap 不能约束该磁盘规模，`incoming/` 中的并发接收还会增加瞬时占用。因此必须增加按字节计算的配额，而不是只增加消息频率限制。

建议默认值：

| 资源 | 默认限制 |
|---|---:|
| 每 peer 文本/图片事件 | 120 次/分钟，允许短时 burst 30 |
| 每 peer 事件字节 | 64 MiB/分钟 |
| 每 peer 并发文件接收 | 2 |
| 全局并发文件接收 | 8 |
| 单文件 | 保持 1 GiB |
| 全局历史文件字节 | 默认 5 GiB，可配置 |
| API 并发连接 | 16 |

限流应在身份认证和 frame header 校验后、分配大 payload 和写磁盘前执行。超限返回结构化 `PeerError`，持续违规可暂时断开该 peer。

接口草案：

```rust
pub struct PeerBudget {
    messages: TokenBucket,
    bytes: TokenBucket,
    active_files: usize,
}

pub enum BudgetDecision {
    Allow,
    RetryAfter(Duration),
    Reject(&'static str),
}

impl PeerBudget {
    pub fn check_event(&mut self, bytes: usize, now: Instant) -> BudgetDecision;
    pub fn begin_file(&mut self, declared_size: u64, now: Instant) -> BudgetDecision;
}
```

历史裁剪必须同时支持条数和总字节数，优先删除最旧文件，并在事务提交后清理对应外部文件。

### 9.2 Shadow filter

简单 `HashSet` 不足以处理同一 hash 连续出现多次，应保存剩余消费次数和过期时间：

```rust
struct ShadowEntry {
    remaining: u16,
    expires_at: Instant,
}

struct ShadowFilter {
    entries: HashMap<String, ShadowEntry>,
    ttl: Duration,
    max_entries: usize,
}

impl ShadowFilter {
    fn insert(&mut self, hash: String, now: Instant) {
        self.prune(now);
        if self.entries.len() >= self.max_entries {
            self.evict_oldest();
        }
        self.entries
            .entry(hash)
            .and_modify(|entry| {
                entry.remaining = entry.remaining.saturating_add(1);
                entry.expires_at = now + self.ttl;
            })
            .or_insert(ShadowEntry { remaining: 1, expires_at: now + self.ttl });
    }

    fn consume(&mut self, hash: &str, now: Instant) -> bool {
        self.prune(now);
        let Some(entry) = self.entries.get_mut(hash) else { return false };
        entry.remaining -= 1;
        if entry.remaining == 0 {
            self.entries.remove(hash);
        }
        true
    }
}
```

建议 TTL 30 秒、最大 1024 条，并记录 eviction 指标而不是打印每个 hash。

## 10. P1/P2：剪贴板任务监督

删除当前空 `catch_unwind`。利用 Tokio `JoinHandle` 监控 panic：

```rust
async fn supervise_clipboard_monitor(context: ClipboardContext, mut shutdown: watch::Receiver<bool>) {
    let mut failures = 0_u32;
    loop {
        let worker = tokio::spawn(clipboard_loop(context.clone(), shutdown.clone()));
        tokio::select! {
            _ = shutdown.changed() => {
                worker.abort();
                break;
            }
            result = worker => {
                failures = failures.saturating_add(1);
                match result {
                    Ok(()) => log::warn!("Clipboard monitor stopped unexpectedly"),
                    Err(error) if error.is_panic() => log::error!("Clipboard monitor panicked"),
                    Err(error) => log::error!("Clipboard monitor join failed: {error}"),
                }
                if failures >= 5 {
                    crate::api::set_health_error("clipboard monitor repeatedly failed");
                }
                tokio::time::sleep(backoff(failures)).await;
            }
        }
    }
}
```

同时让 History/Settings 显示核心服务健康状态，避免静默停止。

## 11. P2：数据库与迁移

### 11.1 LIKE 字面搜索

```rust
fn escape_like_literal(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

let pattern = format!("%{}%", escape_like_literal(keyword));
conditions.push("description LIKE ? ESCAPE '\\' OR source_peer LIKE ? ESCAPE '\\'");
```

添加 `100%`、`a_b`、反斜杠和中文搜索测试。

### 11.2 迁移问题记录

不要把 hash 不一致直接定性为攻击，也不要只打印日志。新增：

```sql
CREATE TABLE IF NOT EXISTS migration_issues (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    history_id INTEGER NOT NULL,
    migration_version INTEGER NOT NULL,
    issue_type TEXT NOT NULL,
    details TEXT NOT NULL,
    created_at TEXT NOT NULL,
    resolved_at TEXT
);
```

处理原则：

- AEAD 解密失败：保留原 row，记录 `decrypt_failed`，不删除数据。
- stored hash 与实际 hash 不同：迁移可继续，但记录原 hash 和实际 hash。
- 文件写入成功、DB 更新失败：保留可识别的临时文件，下次启动清理或重试。
- Schema version 可以前进，但必须允许按 `migration_issues` 重试单条数据。
- UI 显示“有 N 条历史未完成迁移”，而不是只写日志。

### 11.3 incoming 目录

当前产品只支持运行期续传，建议二选一并写清楚：

**方案 A，保持当前范围：**

- 启动清空 incoming。
- 删除误导性的 24 小时跨启动 TTL 和不必要的持久化描述。
- 文档明确“应用重启后重新发送文件”。

**方案 B，支持跨重启续传：**

- 不再启动清空。
- 校验 resume JSON 中的 source、transfer id、大小和 final path。
- final path 必须重新基于受控 incoming 目录计算，不能直接信任序列化路径。
- 传输双方都持久化 transfer id 和确认 offset。
- 24 小时后清理，并提供磁盘配额。

当前建议选 A，不在本轮扩大产品范围。

## 12. C5：文件历史静态加密决策

### 12.1 先做 ADR

需要明确威胁模型：

- 如果只防止磁盘离线读取，FileVault/BitLocker 已覆盖，保持明文并继续披露是可接受方案。
- 如果要求应用数据目录复制后也不可直接读取，则文件历史必须应用级加密。
- 同一用户上下文中的恶意进程通常仍能调用 Keychain/DPAPI，因此应用级加密不能替代本机账户安全。

### 12.2 如选择应用级加密

不能对 1 GiB 文件直接 `read_to_end + AES-GCM`。采用版本化分块容器：

```rust
#[derive(Serialize, Deserialize)]
struct EncryptedFileHeader {
    magic: [u8; 4],       // TSFE
    version: u8,          // 1
    chunk_size: u32,      // 1 MiB
    plaintext_size: u64,
    file_nonce: [u8; 8],
}

pub fn encrypt_file<R: Read, W: Write>(
    input: &mut R,
    output: &mut W,
    key: &DataKey,
    header: &EncryptedFileHeader,
) -> Result<(), FileCryptoError>;

pub fn decrypt_file<R: Read, W: Write>(
    input: &mut R,
    output: &mut W,
    key: &DataKey,
) -> Result<(), FileCryptoError>;
```

每个 chunk 使用独立 nonce：`file_nonce || chunk_index_be_u32`，AAD 包含 header hash、chunk index 和明文长度。每块有独立 AEAD tag，可流式验证和恢复。

迁移流程：

1. 明文源只读打开。
2. 加密到同目录 `.enc.tmp`。
3. flush、sync、完整解密校验 hash。
4. 原子 rename 为版本化密文文件。
5. 事务更新 DB reference。
6. DB 成功后删除明文。
7. 失败时保留明文并删除本次临时密文。

恢复剪贴板时解密到 `clipboard-files/<random>/<safe_name>`，窗口关闭或 TTL 后清理。该目录仍是短期明文，应在文档中说明。

## 13. P2：History 页面

### 13.1 服务端分页和 generation guard

macOS 应复用 Windows 已有协议：

```tsx
const requestGeneration = useRef(0);

const loadHistory = useCallback(async () => {
  const generation = ++requestGeneration.current;
  setLoading(true);
  try {
    const result = await invoke<HistoryPage>("get_history_page", {
      keyword: keyword || null,
      category: selectedCategory,
      startTime,
      endTime,
      limit: PAGE_SIZE,
      offset: page * PAGE_SIZE,
    });
    if (generation !== requestGeneration.current) return;
    setEntries(result.entries);
    setTotalEntries(result.total);
    void loadThumbnails(result.entries.filter(e => e.type === "image").map(e => e.id));
  } finally {
    if (generation === requestGeneration.current) setLoading(false);
  }
}, [keyword, selectedCategory, startTime, endTime, page]);
```

进一步增加 `get_image_thumbnails(ids)` 批量命令，避免每张图片一次 IPC。

### 13.2 日期算法

```ts
const dayOrdinal = (date: Date) =>
  Date.UTC(date.getFullYear(), date.getMonth(), date.getDate()) / 86_400_000;

const diffDays = dayOrdinal(today) - dayOrdinal(itemDate);
```

增加 DST 前拨、后拨、跨年、无效时间字符串测试。

### 13.3 键盘和可发现操作

不要让整个 `<div>` 模拟按钮。使用操作组：

```tsx
<article className="history-item" aria-label={entry.description}>
  <button
    type="button"
    className="history-item-main"
    onClick={() => void handleRestore(entry.id)}
  >
    <HistoryPreview entry={entry} />
  </button>
  <button
    type="button"
    className="history-item-delete"
    aria-label={t("history.deleteEntry", { name: entry.description })}
    onClick={() => void handleDelete(entry.id)}
  >
    <Trash2 aria-hidden="true" />
  </button>
</article>
```

右键菜单可以保留为快捷方式，但不能是唯一删除入口。

## 14. P2：Settings 和无障碍

### 14.1 滑块提交

```tsx
const [historyLimitDraft, setHistoryLimitDraft] = useState(settings.history_limit);

const commitHistoryLimit = async () => {
  if (historyLimitDraft === settings.history_limit) return;
  await update({ history_limit: historyLimitDraft });
};

<input
  type="range"
  min={10}
  max={500}
  value={historyLimitDraft}
  onChange={(event) => setHistoryLimitDraft(Number(event.target.value))}
  onPointerUp={() => void commitHistoryLimit()}
  onBlur={() => void commitHistoryLimit()}
  onKeyUp={(event) => {
    if (["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) {
      void commitHistoryLimit();
    }
  }}
/>
```

更新请求要有序列号或单飞锁，避免旧请求失败时把较新的 optimistic state 回滚。

### 14.2 配对 dialog

优先使用原生 `<dialog>`：

```tsx
function PairingDialog({ open, onClose, status }: Props) {
  const ref = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    if (open && !dialog.open) dialog.showModal();
    if (!open && dialog.open) dialog.close();
  }, [open]);

  return (
    <dialog ref={ref} aria-labelledby="pairing-title" onCancel={onClose}>
      <h2 id="pairing-title">{t("pairing.title")}</h2>
      <output aria-live="polite" aria-label={t("pairing.verificationCode")}>
        {status?.peer?.verification_code}
      </output>
      <button type="button" onClick={onClose}>{t("common.cancel")}</button>
    </dialog>
  );
}
```

如因 WebView 兼容性保留自定义 modal，必须实现初始焦点、焦点循环、Escape、关闭后恢复焦点、`role="dialog"` 和 `aria-modal="true"`。

### 14.3 i18n 与 HTML lang

```tsx
useEffect(() => {
  document.documentElement.lang = locale;
}, [locale]);
```

日期组、类型标签、配对文本和 aria-label 全部进入 JSON 字典。禁止继续新增 `locale === "zh-CN" ? ... : ...`。

### 14.4 timeout 清理

```tsx
function useTrackedTimeout() {
  const timers = useRef(new Set<number>());
  useEffect(() => () => {
    timers.current.forEach(window.clearTimeout);
    timers.current.clear();
  }, []);
  return (callback: () => void, delay: number) => {
    const id = window.setTimeout(() => {
      timers.current.delete(id);
      callback();
    }, delay);
    timers.current.add(id);
    return id;
  };
}
```

短期也可为每类 timer 使用 `useRef` 并在 cleanup 中逐一清理。

## 15. P2/P3：错误处理

每个模块定义 typed error：

```rust
#[derive(Debug, Error)]
pub enum DbError {
    #[error("database operation failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("history file operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("history encryption failed: {0}")]
    Crypto(#[from] CryptoError),
    #[error("history reference is invalid")]
    InvalidReference,
}
```

边界转换原则：

- 模块内部保留具体 error source。
- Tauri command 返回稳定的 `{ code, message, recoverable }`。
- JSON-lines API 使用同一错误码。
- 日志记录内部 source，但客户端消息不暴露路径、密钥或堆栈。
- 禁止在模块中间层过早 `.map_err(|e| e.to_string())`。

## 16. P3：共享架构

### 16.1 目标目录

```text
Cargo.toml                    # workspace
crates/
  tailsync-core/
    src/
      protocol/
      crypto/
      history/
      sync/
      network/
  tailsync-platform/
    src/
      clipboard.rs
      key_store.rs
      notifications.rs
macos/src-tauri/
windows/src-tauri/
frontend/
  shared/
    hooks/
    i18n/
    pages/
    components/
  macos-adapter/
  windows-adapter/
```

平台接口示例：

```rust
pub trait PlatformServices: Send + Sync {
    fn key_store(&self) -> &dyn KeyStore;
    fn clipboard(&self) -> &dyn ClipboardService;
    fn notify(&self, notification: Notification) -> Result<(), PlatformError>;
}
```

不要试图共享 Win32 与 AppKit 实现，只共享调用契约和业务逻辑。

### 16.2 拆分顺序

1. 先抽取字节完全相同的 Rust 文件，确保现有测试不变。
2. 抽取前端 hooks、i18n 和入口文件。
3. 统一 History 行为后再共享页面。
4. 最后拆分 `network/mod.rs`、`db.rs`、`api.rs`。

推荐模块：

```text
network/
  connection_pool.rs
  delivery.rs
  discovery.rs
  health.rs
  server.rs
  limits.rs

history/
  schema.rs
  migrations.rs
  repository.rs
  file_storage.rs
  classification.rs

api/
  transport.rs
  auth.rs
  commands.rs
  responses.rs
```

### 16.3 Settings 拆分

```rust
pub struct Preferences {
    pub notifications_enabled: bool,
    pub progress_bar_enabled: bool,
    pub history_limit: u32,
    pub theme: Theme,
    pub language: Language,
}

pub struct TrustStore {
    pub trusted_peer_keys: HashMap<String, String>,
}

pub struct RouteStore {
    pub enabled_peers: HashMap<String, bool>,
    pub trusted_peer_addresses: HashMap<String, PeerAddresses>,
    pub paired_peer_endpoints: HashMap<String, Endpoint>,
}
```

更新偏好设置时只反序列化和写 `Preferences`，不让前端提交完整信任表。

## 17. 其他低风险修改

### 17.1 Tray fallback

```rust
static TRANSPARENT_TRAY_RGBA: [u8; 32 * 32 * 4] = [0; 32 * 32 * 4];

let icon = Image::from_bytes(include_bytes!("../icons/icon.png"))
    .unwrap_or_else(|error| {
        log::error!("Bundled tray icon is invalid: {error}");
        Image::new(&TRANSPARENT_TRAY_RGBA, 32, 32)
    });
```

同时在构建测试中直接解码 bundled icon，让错误在 CI 出现，而不是运行时 fallback。

### 17.2 日志隐私

```rust
info!("Authenticated peer {} connected", peer_info.hostname);
debug!("Peer route selected: {}", peer_addr);
```

增加 `diagnostic_logging` 设置时才输出完整地址。verification code、密钥、剪贴板内容和文件内容永不写日志。

### 17.3 Vite 配置

```ts
// vite.shared.ts
export const sharedViteConfig = defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { strictPort: true },
});
```

平台配置只合并各自的 root、端口或资源路径。

### 17.4 常量配置边界

保持硬编码：

- 协议版本、frame header、payload 安全上限。
- 文件 chunk wire format。
- 加密算法和 nonce 格式。

可考虑配置：

- 历史总字节配额。
- 诊断日志等级。
- UI 轮询兜底间隔。

需要先修改发现/互操作协议才能配置：

- TCP peer port。
- API transport endpoint。
- 心跳和 idle timeout。

## 18. 实施路线图

### 阶段 A：P0 密钥修复

- KeyStore typed error。
- 进程内与跨进程原子初始化。
- macOS 禁止 `-U` 覆盖。
- Windows `.dek` 加锁和原子写。
- DeviceIdentity 失败关闭、私有文件权限和耐久原子写。
- Settings 失败关闭。
- 密钥和旧历史回归测试。

交付方式：独立修复版本，不包含 UI 重构。

### 阶段 B：P1 安全与可靠性

- API 令牌、长度、超时、连接上限。
- 图片统一验证。
- legacy 文件路径修复。
- peer 速率和磁盘配额。
- shadow filter TTL。
- clipboard supervisor。
- 优雅 shutdown。

### 阶段 C：P2 产品质量

- macOS History 服务端分页。
- generation guard 和批量缩略图。
- LIKE 字面搜索。
- 迁移问题可见化。
- History 键盘操作。
- 配对 dialog 无障碍。
- slider 提交、i18n、lang、timer cleanup。

### 阶段 D：产品决策

- ADR：文件历史是否应用级静态加密。
- ADR：是否支持应用重启后的文件续传。
- ADR：是否从 TCP API 迁移到 Unix socket/Named Pipe。

### 阶段 E：P3 架构

- Cargo workspace 和共享 core。
- 共享前端包。
- 模块拆分和 typed errors。
- Settings 持久化拆分。
- 删除 dead code 和冗余配置。

## 19. 验收标准

### 19.1 自动化

每个阶段至少运行：

```powershell
npm exec tsc -- -p tsconfig.app.json --noEmit --incremental false --pretty false
npm run lint
cargo fmt --manifest-path src-tauri\Cargo.toml -- --check
cargo clippy --manifest-path src-tauri\Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri\Cargo.toml
```

两端共享协议还必须运行 cross-platform interop probe。

### 19.2 安全回归

- 权限拒绝不会改变 Keychain 条目或 `.dek`。
- key 损坏不会自动生成新 key。
- identity 损坏或权限拒绝不会自动生成并覆盖设备身份。
- API 超长行、慢速发送、无 token、错误 token 均被拒绝且内存有界。
- 无效图片不能进入 DB、clipboard 或 thumbnail。
- `../`、绝对路径、Windows drive/UNC 名不能逃离受控目录。
- 配额超限不会继续分配大 payload 或写入磁盘。

### 19.3 真实设备

- macOS Keychain 首次授权、拒绝、稍后重试。
- Windows DPAPI 首次启动、并发启动、强制终止写入。
- Mac/Windows 双向文本、图片和 1 GiB 边界文件。
- LAN/Tailscale 切换、sleep/wake、clipboard monitor 自动恢复。
- History 键盘全流程和屏幕阅读器 dialog。
- DST 时区中的日期分组。

## 20. 不建议的做法

- 不要为了消除 `unwrap` 数量而机械改写所有测试和已证明不变量。
- 不要在 Keychain 读取失败时自动“修复”或覆盖密钥。
- 不要在同一 PR 中同时进行密钥修复、文件格式迁移和共享 crate 重构。
- 不要将 1 GiB 文件放进单行 JSON/base64。
- 不要把所有 HashMap 改成并发容器；先保持清晰的所有权和锁边界。
- 不要在没有 ADR 和迁移/回滚方案前加密已有文件历史。
- 不要把右键菜单作为唯一操作入口。

## 21. 建议的提交拆分

```text
fix(crypto): distinguish missing, denied and corrupt data keys
fix(crypto): make Windows DPAPI key persistence atomic
fix(identity): preserve and restrict the encrypted device identity
fix(config): fail closed on corrupt trust settings
fix(api): bound and authenticate local JSON-lines requests
fix(protocol): validate frames and packed images at construction
fix(sync): supervise clipboard monitor and bound shadow filters
fix(storage): enforce byte quotas and safe clipboard materialization
fix(history): use server pagination and reject stale responses on macOS
fix(a11y): make history actions and pairing dialog keyboard accessible
refactor(core): extract shared protocol and history modules
```

每个提交都应能独立编译、测试和回滚。

## 22. 实施结果（2026-08-01）

### 22.1 已完成的缺陷修复

| 范围 | 已落地内容 | 状态 |
|---|---|---|
| P0 密钥与配置 | `NotFound`/拒绝/损坏/I/O 分类、32 字节校验、只创建不覆盖、进程内与跨进程首次启动收敛、Windows DPAPI 原子持久化、Settings 损坏时拒绝启动 | 完成 |
| 设备身份 | 身份损坏时拒绝覆盖、原子且耐久的 create-only 写入、Unix 权限、Windows DACL、并发首次启动测试 | 完成 |
| H1 与启动路径 | 生产输入边界移除直接 `unwrap/expect`，DEK、Settings、DB、identity、API token 和 Tauri 初始化错误由 `run_app() -> Result` 返回；所有受跟踪的顶层任务统一使用 Tauri runtime，避免 runtime 建立前 `tokio::spawn` panic | 完成 |
| C2/C3 | 删除 `Frame::new()`，统一 `Frame::try_new()` 与命令级 payload 校验，互操作探针同步升级；统一 shutdown、有界任务排空，macOS SwiftUI 先发送认证 `quit`，仅在超时后使用 SIGTERM/SIGKILL | 完成 |
| H2/H5/H6 | 迁移问题持久化与重试、LIKE 字面量转义、唯一 `PackedImage` 校验、迁移/写入/恢复/缩略图全链路校验 | 完成 |
| H3/H4 | peer 令牌桶、并发与字节配额、5 GiB 历史文件配额和写前大小准入、超过 1 GiB 时禁止 peer 发送、有界 TTL shadow filter | 完成 |
| H8/H9/H10 | clipboard supervisor、本地 API capability token 与长度/超时/并发限制、listener bind/accept 失败重试及 shutdown 中断、分块迁移、安全文件名和受控恢复目录 | 完成 |
| History P2 | 两端服务端分页、generation guard、可视缩略图加载、迁移告警、键盘可聚焦恢复/删除、DST 安全日期边界 | 完成 |
| Settings P2 | 两端滑块提交式保存与设置保存串行化、字典化文案、HTML `lang`、配对弹窗焦点循环/Escape/焦点恢复、前端 timeout 卸载清理 | 完成 |
| L2/L3/M15/M17 | 静态 4096 字节托盘 RGBA fallback 与打包图标解码测试、IP 地址降为 debug、macOS 刷新取消二次查询、兼容读取函数改名并补注释/回归测试 | 完成 |
| C5 文件历史加密 | `TSFENC1` 版本化分块 AEAD、HKDF 文件密钥、原子替换、受控临时明文恢复；v7 旧文件转换使用独立 SQLite 连接按批后台执行，不阻塞启动 | 完成 |
| CI 与测试基础 | 三平台职责化 CI、双向 `interop_probe`、Windows Vitest 运行时测试；删除过时的无 Noise 协议 v1 `test_peer.py` | 完成 |
| Rust/前端架构 | 共享 `tailsync-core` path dependency、macOS 遗留 React 退役、Landing 独立到 `site/`、db/network/api 职责拆分 | 完成 |
| Settings 契约 | Rust `Settings` + schemars 为权威来源，自动生成 JSON Schema 和 TypeScript DTO，CI 检查 Rust/Schema/TS/Swift 漂移 | 完成 |

### 22.2 明确不实施的项目

以下项目不属于“尚未修复的 bug”：

- `REJECT`：C4、M2、M7、M14、L4、L5、L7，按核验结论不修改。
- `ACCEPT`：H11 运行期续传、L8 30 秒心跳，保持已披露的当前产品契约。
- v1 自动迁移增强：按当前唯一用户的实际情况取消投入；保留现有可工作的兼容导入代码，不扩展为后台状态机。
- 应用重启后的文件续传继续作为产品取舍；本轮不改变“仅运行期续传”的契约。
- 根 Cargo workspace 暂不建立：两个平台外壳目前同名，继续使用独立 path dependency；共享前端整页包也不建立，因为 macOS 正式 UI 已统一为 SwiftUI。

### 22.3 验证结果

- shared Rust core：81/81；Windows 外壳：52/52；macOS 外壳：47/47。三套 `cargo clippy --all-targets -- -D warnings`、格式检查和锁定构建均通过。
- Windows 前端：Vitest 4 个文件、17/17 项通过，`npm run lint` 与 `npm run build` 通过；独立 `site/` lint/build 通过。
- Settings Rust→Schema→TypeScript 生成检查和 Rust/Swift/TypeScript 跨平台字段契约检查通过。
- 迁移脚本：Windows/macOS 两份 `migrate.py` 均通过 `py_compile`。
- Windows 调试二进制使用隔离数据目录真实启动 3 秒后仍存活，后台文件加密任务进入 Tauri runtime 后未出现启动 panic。
- 跨工程互操作探针已正反角色各运行一次：两次均输出 `CLIENT_SYNC_OK`、`CLIENT_PAIRING_OK`、`SERVER_SYNC_OK`、`SERVER_PAIRING_OK`，client/server 退出码均为 0；覆盖双向事件/ACK、文件块/offset ACK、Noise 身份固定和配对确认。
- `git diff --check` 通过（仅有 Git 的 LF/CRLF 提示）。
- 当前主机没有 `swift`、`swiftc`、`xcodebuild` 或 Apple SDK/linker；macOS SwiftUI/AppKit 原生编译和真实 Keychain/文件权限验收属于环境验证，必须在 macOS CI 或真机补跑。`aarch64-apple-darwin` Rust target 已安装，但不能在 Windows 单独替代 Apple SDK。
