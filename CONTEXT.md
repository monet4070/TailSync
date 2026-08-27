# TailSync 领域上下文

## 项目

TailSync 是端到端加密的跨平台剪贴板同步工具：macOS（SwiftUI 菜单栏外壳 + Rust 守护进程）与
Windows（React + Tauri）之间同步文本、图片和文件，优先局域网，必要时经 Tailscale 或 Iroh。

线协议 v4；产品版本 2.2.2；数据库 schema v9。详见 `README.md`。

## 分层与契约面

```
shared/rust-core/           共享深模块（单一事实来源，跨平台共用）
  peer/types.rs             设备/路由/投递的类型契约（serde 字段名是 JSON 契约，改动需过漂移检查）
  peer/directory.rs         发现合并、候选补全/排序、模式与地址判定（纯规则）
  peer/health.rs            健康状态机 + 认证会话记账（纯状态机）
  peer/delivery.rs          可靠投递：帧记账、ACK 校验、投递执行、连接竞速与连接生命周期 worker
  private_fs.rs             私有存储权限策略（Unix 0700/0600、Windows 受保护 DACL）：身份/设置/数据库/
                            加密容器/incoming/剪贴板恢复文件统一走这里，勿再各写一套
macos|windows/src-tauri/    平台层：接线（全局单例、薄包装）+ 适配器（socket/mDNS/Tailscale/Iroh/剪贴板）
```

- 平台 `network/*` 中**被漂移检查强制逐字节一致**的文件：`build.rs`、`examples/interop_probe.rs`、
  `network/types.rs`、`network/server.rs`、`scripts/check_cross_platform_sync.mjs|ps1`、
  `scripts/test_cross_project_interop.ps1`。修改这些文件必须两端同步。
- `network/mod.rs`、`network/health.rs`、`network/pool.rs`、`network/tailscale.rs`、
  `network/lan.rs`、`network/mdns.rs`、`network/peer_cache.rs` 等仍允许平台差异（编排与适配器），
  但共享逻辑一旦迁入 core，平台文件应只剩接线/适配，新逻辑一律先考虑 core。

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

- `iroh_transport` 的 `repeated_rtt_probes` 测试因本机 QUIC 环境回归已 `#[ignore]`（2026-08-14 起记录）。
- Android 客户端不在 v4 协议兼容范围。
