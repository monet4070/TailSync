# TailSync 领域上下文

## 项目

TailSync 是端到端加密的跨平台剪贴板同步工具：macOS（SwiftUI 菜单栏外壳 + Rust 守护进程）与
Windows（React + Tauri）之间同步文本、图片和文件，优先局域网，必要时经 Tailscale 或 Iroh。

线协议 v4；产品版本 2.2.2；数据库 schema v10。详见 `README.md`。

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
    pairing.rs + pairing/    配对状态机与测试
    peer/                    types/directory/health/delivery 等设备与可靠投递规则
    secure.rs + secure/      握手、认证与安全会话
    sync.rs + sync/          同步编排、批次、接收、恢复与状态
    themes_v2.rs             对 tailsync-themes 的数据目录 Adapter
  tailsync-protocol/         线协议类型、编解码与契约测试
  tailsync-history-classifier/
    model.rs                 分类模型、类别常量与公开结果
    website.rs/paths.rs      纯规则检测器
    command.rs/code.rs       命令与代码检测器
  tailsync-themes/           主题包格式、验证、解析、存储与读取策略
    package_io.rs             用户选包的扩展名/普通文件/符号链接/64 MiB 门禁

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
```

- shared Core 的纯规则和状态机是 Leverage 最高的 Module；平台 Adapter 不得复制规则。
  新逻辑先考虑 Core，再决定在哪个 Adapter 绑定系统能力。
- 文件拆分以 Seam 和 Locality 为准：页面 façade 只编排状态/副作用；子 Module 通过显式、
  可测试的 Interface 接收数据和回调。不要为了缩短行数引入没有独立职责的薄包装。
- `themes_v2::read_theme_package_file` 是两端共用的主题文件读取策略；响应编码仍属于
  transport Adapter。macOS 有 Unix socket JSON routes，Windows 有 Tauri commands，
  不为表面结构对称而复制路由。
- Windows 的 `commands/preview.rs` 保留独立文件是有意设计，原始 ArrayBuffer 预览协议
  见 `docs/adr/ADR-002-independent-history-preview-window.md`。
- 收藏是历史的一个受保护集合：`shared/rust-core/src/db/favorites.rs` 负责逻辑条目/文件批次的
  原子收藏、取消收藏与收藏窗口删除；数据库仍使用 `pinned` 字段保持 v8/v9 wire 兼容，v10
  启动迁移会把旧的部分收藏批次规范化。平台只负责窗口、手势和命令/Unix route Adapter。
- 历史窗口的右键删除必须经过 Core 的 `delete` 保护；收藏条目只能经
  `delete_favorite_entry` 从收藏窗口删除。清空历史只删除未收藏条目，不能绕过收藏保护。
- 长按手势的宽限期为 220 ms、可见充能为 420 ms；Swift 的 AppKit responder 与 Windows 的
  pointer hook 共享同一时序语义，进度由声明式动画绘制，不使用逐帧计时器；完成后必须抑制
  同一手势的选择、点击与双击。历史和收藏窗口分别拥有可见性、轮询、关闭与资源释放生命周期。
- 平台 `network/*` 中**被漂移检查强制逐字节一致**的文件：`build.rs`、
  `examples/interop_probe.rs`、`network/types.rs`、`network/server.rs`、
  `scripts/check_cross_platform_sync.mjs|ps1`、`scripts/test_cross_project_interop.ps1`。
  修改这些文件必须两端同步。
- `network/mod.rs`、`network/health.rs`、`network/pool.rs`、`network/tailscale.rs`、
  `network/lan.rs`、`network/mdns.rs`、`network/peer_cache.rs` 等仍允许平台差异
  （编排与 Adapter），但共享逻辑一旦迁入 Core，平台文件应只剩接线/适配。
- macOS API route 的 `handles()` 与 `handle()` 必须同增同删；主题 route 有专门的
  dispatch 回归测试。跨平台契约检查只校验共享协议字段，不替代各平台的 route/command
  dispatch 测试。
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
cargo clippy --locked --manifest-path shared/rust-core/Cargo.toml --all-targets -- -D warnings
cargo test --locked --manifest-path shared/rust-core/Cargo.toml
node windows/scripts/check_cross_platform_sync.mjs --win-root windows --mac-root macos --core-root shared/rust-core
```

共享逻辑的测试在 core 内（单点）；平台测试覆盖真实 I/O 回归。Windows Rust crate 在 macOS
上通过 host 编译验证（见 `scripts/check-windows-host.sh`：fmt + check --all-targets +
test --no-run），Windows 原生编译/打包/运行由 CI 负责。注意 host 编译会因
`#[cfg(target_os = windows)]` 块被裁掉而产生 dead-code 伪警告，属正常现象。

## 已知缺口

- `iroh_transport` 的 `repeated_rtt_probes` 测试已于 2026-08-27 解除 `#[ignore]`；两端 connect
  各带 10 秒外层超时，服务端在连续 probe 之间等待前一条 QUIC 连接关闭，避免把关闭握手竞态误报为
  endpoint 隔离回归。
- Android 客户端不在 v4 协议兼容范围。
