# TailSync 领域上下文

## 项目

TailSync 是端到端加密的跨平台剪贴板同步工具：macOS（SwiftUI 菜单栏外壳 + Rust 守护进程）与
Windows（React + Tauri）之间同步文本、图片和文件，优先局域网，必要时经 Tailscale 或 Iroh。

产品版本 2.3.2。复合剪贴板的版本分别管理：identity/base **4**；data frame **5**（认证连接可协商回退到 4）；main history **条件 v11/v12/v13**；effect journal **1**；local contract **2**；helper protocol **macOS 4 / Windows 2**；origin fingerprint **便携 2 / macOS 原件独立域 1**。v11→v12/v13 受构建开关、运行时开关和 helper 健康控制；已有 v12/v13 在关闭态可读取，迁移目的库保持至少来源版本。v13 增加历史身份及 Manifest 2；旧 CNS1/helper protocol 2 只提供 LegacyProjection；macOS CNA1/protocol 4 分开原件与派生表示，完整枚举且依赖已知闭合的原件才提供安全去重键。pre-v12 与 pre-v13 回退包分别保留。版本变更须有独立决策，不能把这些编号统称为“协议版本”。 详见 `README.md`。

## 分层与契约面

TailSync 的代码按 Module、Interface、Implementation 和 Adapter 分层。跨平台
业务规则优先落在 shared Core；平台层只负责把系统 I/O、全局单例、窗口和传输协议接到
Core 的 Interface。每个新 Module 必须有明确的 Seam（输入/输出边界），避免通过全局状态
或隐式副作用耦合。

```
shared/
  rust-core/                 共享深模块（跨平台单一事实来源）
    crypto.rs + crypto/      密钥存储与加密边界
    db.rs + db/              数据库生命周期、查询、迁移、文件存储、收藏、预览
    pairing.rs + pairing/    配对状态机、一次性 Iroh 邀请与测试
    peer/                    types/directory/health/delivery/pool 等设备与可靠投递规则
    secure.rs + secure/      握手、认证与安全会话
    iroh_transport.rs        Iroh 端点、RTT 探测与共享生命周期注册表
    updates.rs               签名更新元数据校验规则
    sync.rs + sync/          同步编排、批次、接收、恢复与状态
  tailsync-protocol/         线协议类型、编解码与契约测试
  tailsync-history-classifier/
    model.rs                 分类模型、类别常量与公开结果
    website.rs/paths.rs      纯规则检测器
    command.rs/code.rs       命令与代码检测器
  tailsync-themes/           主题包格式、验证、解析、存储与读取策略
    package_io.rs             用户选包的扩展名/普通文件/符号链接/64 MiB 门禁
  tailsync-runtime/          transport-neutral application use cases
    history.rs                history paging, restore payloads, and mutations

macos|windows/src-tauri/
  api/                        Tauri/Unix socket API 接线与 transport Adapter
    routes/{history,peers,settings,theme}.rs
  commands/                   Tauri 命令接线；平台 I/O 只留在 Adapter
  clipboard/                  跨平台剪贴板传输实现与测试
  network/                    LAN/mDNS/Tailscale/Iroh 的平台 Adapter

macos/swift-ui/
  Services/ApiClient.swift    Swift API façade；Transport/History/Peers/Runtime/
                              StorageSettings/Themes 承载具体实现
  Views/SettingsView.swift    设置页面 façade；各功能 section 承载视图实现
  Views/HistoryView.swift     历史页面 façade；HistoryRow、收藏交互与预览模块独立

windows/
  src/pages/Settings.tsx      设置页面 façade；settings/ 按连接/常规/历史/存储/
                              外观/更新/对话框拆分
  src/pages/History.tsx       历史页面 façade；history/ 按 header/list/item/main/footer 拆分
  src/hooks/                  可复用的设备、配对、快捷键、更新、缩略图、长按与运行时状态

shared/platform-network-{server,pool,iroh}.rs、shared/platform-updates.rs、
shared/platform-clipboard-{transfer,tests}.rs 是平台 Rust 模块的 canonical include 源：归档
解析/通知、TCP/Iroh 入口等 Adapter 差异留在接线层，网络/更新编排与纯测试规则只维护一份。
```

- shared Core 的纯规则和状态机是 Leverage 最高的 Module；平台 Adapter 不得复制规则。
  新逻辑先考虑 Core，再决定在哪个 Adapter 绑定系统能力。
- 文件拆分以 Seam 和 Locality 为准：页面 façade 只编排状态/副作用；子 Module 通过显式、
  可测试的 Interface 接收数据和回调。不要为了缩短行数引入没有独立职责的薄包装。
- `tailsync_themes::read_theme_package_file` 是两端共用的主题文件读取策略；响应编码仍属于
  transport Adapter。macOS 有 Unix socket JSON routes，Windows 有 Tauri commands，
  不为表面结构对称而复制路由。
- Windows 的 `commands/preview.rs` 保留独立文件是有意设计，原始 ArrayBuffer 预览协议
  见 `docs/adr/ADR-002-independent-history-preview-window.md`。
- 收藏是历史的一个受保护集合：`shared/rust-core/src/db/favorites.rs` 负责逻辑条目/文件批次的
  原子收藏、取消收藏与收藏窗口删除；数据库仍使用 `pinned` 字段保持 v8/v9 wire 兼容。
  v10 迁移负责规范化旧的部分收藏批次；当前 v11 另加接收回执和文件批次唯一约束，不改变
  收藏语义。平台只负责窗口、手势和命令/Unix route Adapter。
- 历史窗口的右键删除必须经过 Core 的 `delete` 保护；收藏条目只能经
  `delete_favorite_entry` 从收藏窗口删除。清空历史只删除未收藏条目，不能绕过收藏保护。
- 长按手势的宽限期为 220 ms、可见充能为 420 ms；Swift 的 AppKit responder 与 Windows 的
  pointer hook 共享同一时序语义，进度由声明式动画绘制，不使用逐帧计时器；完成后必须抑制
  同一手势的选择、点击与双击。历史和收藏窗口分别拥有可见性、轮询、关闭与资源释放生命周期。
- 平台 `network/*` 中**被漂移检查强制逐字节一致**的文件：`build.rs`、
  `examples/interop_probe.rs`、`network/types.rs`、`network/server.rs`、
  `network/pool.rs`、`network/iroh.rs`。修改这些文件必须两端同步。
- 跨平台检查与互操作脚本以 `windows/scripts/` 为唯一实现；`macos/scripts/`
  保留同名兼容转发入口，并由漂移检查验证其指向，避免两份脚本再次分叉。
- `network/mod.rs`、`network/health.rs`、`network/tailscale.rs`、`network/lan.rs`、
  `network/mdns.rs`、`network/peer_cache.rs` 等仍允许平台差异
  （编排与 Adapter），但共享逻辑一旦迁入 Core，平台文件应只剩接线/适配。
- macOS API route 的 `handles()` 与 `handle()` 必须同增同删；主题 route 有专门的
  dispatch 回归测试。跨平台契约检查只校验共享协议字段，不替代各平台的 route/command
  dispatch 测试。
- 文件批次恢复由 Core 的持久态定义：发送端 `sync/outgoing.rs` 写私有 journal，接收端
  `sync/resume.rs`/`receive_engine.rs` 从 `.part` 实际长度恢复；只有历史文件事务和 v11
  `received_file_batches` 回执提交成功后才能 ACK 完成。平台传输层只负责调用该状态机，详见
  `docs/features/resumable-file-transfer.md`。
- Iroh 远程配对由 `pairing/invite.rs` 定义版本化、一次性、内存态邀请；平台 Adapter 只负责
  Endpoint 连接、`tailsync://` scheme 事件和 API/Unix route 接线。邀请不依赖 TailSync 自建
  服务器，仍必须经过 Noise XX、六位验证码和双端确认，详见 `docs/features/remote-iroh-pairing.md`。
- Windows 历史运行时提示必须使用 `useHistoryNotice` 的页面内联槽位；相同 key 的重复事件只能
  增加计数，不能刷新 TTL 或创建固定定位浮层；不同 key 的连续错误共享 8 秒可见预算和 1.5 秒
  冷却间隔，保证错误风暴也不能让提示常驻或遮挡分页和操作控件。
## 领域词汇（以 README 与代码为准）

| 术语 | 含义 |
|---|---|
| Peer | 一台已发现或已配对的设备；`PeerInfo` 是其统一快照 |
| Candidate / Route | 到 Peer 的一条可达路径（LAN / Iroh / Tailscale）；`PeerCandidate` 携带优先级与健康字段 |
| Directory | 把发现快照 + 设置记忆合并为最终 peer 视图的规则层（`peer/directory.rs`） |
| Health | 路由的在线状态机：`discovered → online → confirming → offline`（探测成功进 online，失联 1 轮 confirming、2 轮 offline；认证会话强制 `connected`）；12 秒 TTL、两轮 miss 判定 |
| Session | 一条已认证连接；引用计数，强制路由 `connected`，注册同时视为一次探测成功 |
| Delivery | 可靠投递：帧入队、ACK 期望、投递执行（类型化重试/超时/永久失败）、连接竞速与生命周期（重连、心跳、队列选择、会话租约） |
| Adapter | 平台侧的 I/O 实现（TCP/iroh 连接、UDP/mDNS/Tailscale 发现、剪贴板监听），实现 core 定义的规则输入输出 |
| 接线层 | 平台侧把全局单例与 tokio 原语绑定到 core 纯函数/状态机的薄包装 |

## 健康状态语义（统一后）

- 从未被探测的路由：`discovered`。
- 探测成功：`online`（0 miss，12 秒 TTL 内）。
- 成功后再失联 1 轮：`confirming`（仍在 TTL 内视为在线）；连续 2 轮 miss：`offline`。
- 存在认证会话：强制 `connected`，会话 latency 优先于探测 latency。
- 设置派生的 remembered 候选从不直接标记在线（`PeerCandidate::remembered`）。

## 开发门禁

```bash
cargo fmt --manifest-path shared/rust-core/Cargo.toml --all -- --check   # 三端同样
cargo fmt --manifest-path shared/tailsync-runtime/Cargo.toml --all -- --check
cargo clippy --locked --manifest-path shared/rust-core/Cargo.toml --all-targets -- -D warnings
cargo clippy --locked --manifest-path shared/tailsync-runtime/Cargo.toml --all-targets -- -D warnings
cargo test --locked --manifest-path shared/rust-core/Cargo.toml
cargo test --locked --manifest-path shared/tailsync-runtime/Cargo.toml
TAILSYNC_WEB_SVG_RENDER_TESTS=1 swift test --package-path macos/swift-ui
node windows/scripts/check_cross_platform_sync.mjs --win-root windows --mac-root macos --core-root shared/rust-core
```

共享规则的测试在 core 内（单点），跨 transport 的应用用例在 tailsync-runtime 测试；平台测试覆盖真实 I/O 回归。Windows Rust crate 在 macOS
上通过 host 编译验证（见 `scripts/check-windows-host.sh`：fmt + check --all-targets +
test --no-run），Windows 原生编译/打包/运行由 CI 负责。注意 host 编译会因
`#[cfg(target_os = windows)]` 块被裁掉而产生 dead-code 伪警告，属正常现象。

本地验证命令可统一用 `node scripts/run-isolated-tests.mjs -- <命令>` 启动：每次调用都会覆盖
`TAILSYNC_DATA_DIR`、`TAILSYNC_STORAGE_DIR`、`TAILSYNC_V1_DATA_DIR`，使用私有临时目录，结束后清理；
不修改 `HOME` 或用户 Keychain。CI 整改门禁和 `check_macos_sources.sh` 已使用同一隔离器。
直接运行 Core 单元测试或带 dev-only `test-support` 的平台/runtime 测试时，未显式指定数据目录也会
自动选择进程独立的临时目录，并停止从真实 HOME 隐式发现旧版历史。显式目录供受控测试使用；
不要手动将测试环境变量指向日常应用数据。普通应用可执行文件仍使用原生产目录和密钥策略。

## 当前实现说明与已知边界

- `iroh_transport` 的 `repeated_rtt_probes` 测试已于 2026-08-27 解除 `#[ignore]`；测速复用应用的
  常驻 Endpoint，仅为 RTT ALPN 新建并关闭独立 QUIC 连接。测试会校验连续 probe 的来源 Endpoint ID
  不变、业务流不受 probe 关闭影响，并在连续 probe 之间等待前一条连接关闭；Iroh connect 使用 10 秒
  外层超时，避免把冷启动路径发现误报为失败。
- 文件续传状态最多保留 24 小时；源文件在发送期间发生内容、大小或身份变化时，恢复会失败并要求
  用户重新复制，而不是继续发送与原 manifest 不一致的数据。
- Iroh 远程邀请只保存在创建端内存中，默认 120 秒有效；应用退出、取消、过期或成功配对后失效。
- Android 客户端不在 v4 协议兼容范围。

## Composite R6 verification

`python3 scripts/run-composite-r6.py <new-run-id>` 运行本地正式矩阵。关闭态过滤器命中 3 个 `runtime_disabled` 用例；Iroh 回环与使用 N0/本机接口地址的真实网络组均执行。canonical include 正向及删除任一端的负向检查进入 CI。结果从真实日志/JSON diagnostics 解析，缓存 lint 另由独立 scratch 负向 probe 验证。

源码指纹覆盖产品、测试、工具、构建配置和锁文件，文档及包单独封存 hash。历史阶段证据只对应原指纹。R6 当前证据入口为 `artifacts/composite-repair/2026-10-05-single-mac-r6-retry-identity-final/`；R7 本机原生观察另见 `docs/COMPOSITE-CLIPBOARD-SINGLE-MAC-ACCEPTANCE-2026-10-05.md`。Windows 原生、完整目标应用组、双设备仍为 UNKNOWN，发布 NO-GO。完整 R6 结论见 `docs/COMPOSITE-CLIPBOARD-R6-VERIFICATION.md`。


R6 正式矩阵现为 44 项：增加 R7 harness 与 R6 只读证据校验的回归。源码指纹还纳入
`artifacts/composite-repair/2026-10-04-r0/expected-failures.json`，因为它是测试预期配置。
`composite-evidence.py <run-path> --verify` 校验封存、当前源码、当前 runner 的正式矩阵与包文件；
封存 run 不得继续写入或重新封存。R7 PASS 须逐场景观察 JSON，绑定完整封存 R6、候选源码和
测试包/原始证据；本机故障与回退支持项不关闭设备门禁。执行说明见 `docs/COMPOSITE-CLIPBOARD-R7-ACCEPTANCE.md`。
