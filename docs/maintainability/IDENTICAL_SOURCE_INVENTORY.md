# TailSync Maintainability Refactor — IDENTICAL SOURCE INVENTORY (T100)

> 生成时间：2026-08-15
> 依据：T000 实测 byte-identical 清单（`cmp` 验证）+ 本机依赖事实核查（tauri / cfg(target_os) / std::os 引用计数）。
> 分类：A = platform-specific（保留差异）；B = shared-domain / 契约面（保持 byte-identical）；C = shared-runtime candidate（候选迁移 core）；D = uncertain（依赖未来 shared app-runtime 决策）。
> 本任务只分类，不迁移。

---

## 1. 检查器强制 byte-identical 的完整清单（FACT，T000 `cmp` 实测）

### src-tauri/src 树（assertTreeMatch，不在 allowed-drift）

| 文件 | 行数 | tauri 引用 | cfg/os API | 分类 | 理由 |
|---|---|---|---|---|---|
| `network/server.rs` | 958 | 0 | 无 | **C** | TCP 服务编排（配对+数据）；纯 tokio/std；ADR-001 已把它移出豁免清单强制一致，是 core 化的自然延续 |
| `network/pool.rs` | 580 | 0 | 无 | **C** | 连接池/竞速编排；提交 6b025be 刚达成一致；纯逻辑 |
| `network/iroh.rs` | 268 | 0 | 无 | **C** | Iroh 连接适配；iroh 依赖已在 core（Cargo.toml `iroh = "=1.0.3"`）；纯网络逻辑 |
| `network/rate_limit.rs` | 123 | 0 | 无 | **C** | 限速纯逻辑；依赖最少，最高优先级候选 |
| `api/imports.rs` | 122 | 0 | 无 | **C** | 导入注册/分块导入逻辑；纯业务 |
| `api/transport.rs` | ~110 | 0 | 无 | **C** | JSON-lines 帧 + token 校验；纯 std/tokio（注意：属 I/O 适配面，迁移时需 §22 决策测试） |
| `network/types.rs` | ~40（shim） | 0 | 无 | **B** | 已是 re-export shim（钉扎 PeerStatus）——共享已完成，shim 保留 |
| `sync_adapter.rs` | 230 | 7 | 无 | **D** | SyncPlatform 实现；含 `tauri::AppHandle`/Wry 剪贴板/`tauri::image::Image`——**Tauri 绑定**，不能进 rust-core（R003）；仅当未来建 shared app-runtime crate 才可迁移 |
| `updates.rs` | 281 | 5 | 无 | **D** | Tauri updater 插件接线；同上 |
| `main.rs` | ~10 | 0 | 无 | **A** | 平台入口，保持 |

### assertFileMatch（单文件强制一致）

| 文件 | 分类 | 理由 |
|---|---|---|
| `src-tauri/build.rs` | **A** | Tauri 构建胶水（per-crate 必须存在），一致是约定非业务 |
| `src-tauri/examples/interop_probe.rs`（307 行） | **B** | 契约/互操作验证面（R005/R019），非 runtime；保持 |
| `scripts/check_cross_platform_sync.mjs`（469 行） | **B** | drift checker 本体（R005），保持 |
| `scripts/check_cross_platform_sync.ps1` | **B** | 同上 |
| `scripts/test_cross_project_interop.ps1`（124 行） | **B** | 互操作测试（R019），保持 |

### allowed-drift（平台差异合法，A 类）

`api.rs`、`api/routes.rs`、`clipboard.rs`、`clipboard_change.rs`、`clipboard_file.rs`、`commands.rs`、`lib.rs`、`network/lan.rs`、`network/mdns.rs`、`network/mod.rs`、`network/health.rs`、`network/peer_cache.rs`、`network/tailscale.rs`、`tray.rs`（仅 Windows）—— 平台编排/适配/接线，**不在迁移范围**（R004 目标不含它们；K006 双喂入模式即在此）。

## 2. 分类汇总

```text
A platform-specific（6 + 13 allowed-drift）: main.rs, build.rs, + api.rs, api/routes.rs, clipboard*.rs,
  commands.rs, lib.rs, network/{lan,mdns,mod,health,peer_cache,tailscale}.rs, tray.rs
B shared-domain/契约面（5）: network/types.rs (shim), interop_probe.rs, check_cross_platform_sync.{mjs,ps1},
  test_cross_project_interop.ps1
C shared-runtime candidate（6 → 4，T101/T102 已迁移 2）: network/rate_limit.rs ✅T101, api/imports.rs ✅T102,
  network/iroh.rs, api/transport.rs, network/pool.rs, network/server.rs
D uncertain（2）: sync_adapter.rs, updates.rs（Tauri 绑定 → 需 shared app-runtime crate 决策）
```

## 3. T101+ 建议顺序（按 §8 优先级：高重复 × 平台无关 × 测试充分 × 依赖少）

1. **network/rate_limit.rs** ✅ **T101 已完成**（2026-08-15）：逻辑迁入 `shared/rust-core/src/peer/rate_limit.rs`（含原测试 + 2 个新增确定性测试）；平台文件降为逐字节一致 re-export shim；漂移检查器新增 shim 钉扎（isReExportShim，与 types.rs/tailscale.rs 同模式）。验证：core 205+、macos 61+、windows host check PASS、漂移检查 PASS。
2. **api/imports.rs** ✅ **T102 已完成**（2026-08-15）：导入会话逻辑（260 行/份）迁入 `shared/rust-core/src/import.rs`（同步化 + 显式 incoming_dir 参数 + 6 个确定性测试）；平台文件降为薄适配器（~95 行，保留原函数签名 → routes.rs 零改动）；api.rs 的 ImportRegistry 改为从 core re-export，4 个导入常量移入 core。JSON 契约（cmd 面 + 全部错误字符串 + response 字段）逐字保留。验证：core 211 passed、macos 61、windows host check、漂移检查、adapter cmp 一致。
3. **network/iroh.rs**（需先评估 server.rs 依赖：它引用 server::ConnectionLimiter 与 handle_iroh_connection —— 建议与 server.rs 同批或后置）。
4. **api/transport.rs**（需 §22 决策测试：I/O 适配面是否进 core；依赖 handle_cmd 分发 → 实为 app-runtime 候选）。
5. **network/pool.rs / network/server.rs**（大块编排，高风险；已开始渐进提取：见下）。

## 3.1 server.rs 渐进提取进度（T103 起）

- ✅ **T103（2026-08-15）：ConnectionLimiter/ConnectionPermit 提取**。依赖面评估结论：iroh.rs 引用 server.rs 的 `ConnectionLimiter`（L143）与 `handle_iroh_connection`；server.rs 958 行整体迁移过大，采用渐进提取。本次将纯逻辑限流结构（Semaphore + per-source HashMap，60 行）迁入 `shared/rust-core/src/peer/connection_limiter.rs`（3 个确定性测试）；server.rs 两端降为 re-export（`pub(super) use tailsync_core::peer::connection_limiter::ConnectionLimiter;`）；network/mod.rs 移除因此未用的 Semaphore 导入。验证：core 214 passed、macos 61、windows host check、漂移检查、server.rs cmp 一致。
- ✅ **T104（2026-08-15）：InboundSource 提取**。纯枚举/映射逻辑（34 行）迁入 `shared/rust-core/src/peer/inbound_source.rs`（3 个确定性测试：interface/address+description/is_allowed）；server.rs 以 `use tailsync_core::peer::inbound_source::InboundSource;` 引用，两端 byte-identical；server.rs 899→865 行。注意 `source_matches_mode` re-export 被 mdns.rs/mod.rs 消费，保留。验证：core 217 passed、macos 61、windows host check、漂移检查、cmp 一致。
- ✅ **T105（2026-08-15）：准入判定提取（安全路径，R014 高价值）**。`handle_accepted_connection` 的 trusted/enabled/source_allowed 三条件判定迁入 `shared/rust-core/src/peer/admission.rs`（`peer_is_allowed(settings, hostname, public_key, source) -> bool`，7 个安全相关测试：已配对/未配对/错钥/坏钥/禁用/缺省启用/模式不符）；server.rs 865→852 行，错误串 "Peer is not paired or is disabled" 留在平台。验证：core 224 passed、macos 61、windows host check、漂移检查、cmp 一致。
- ✅ **T106（2026-08-15）：可靠事件接收管线提取**。`receive_reliable_event` + `process_event_content` + `validate_packed_image`（~135 行）迁入 `shared/rust-core/src/peer/event_receiver.rs`（`process_reliable_event(stream, frame, source, engine, db, last_sequence, on_applied)`，5 个 in-memory Noise 连接对测试：应用+ACK、去重不重放、重放拒绝、过期时间戳、非可靠命令）；`on_applied` 回调保持"bump 在 ACK 之前"的原始时序；事件循环中的 still_authorized 复查块同时改用 T105 的 `peer_is_allowed`；server.rs 852→715 行。验证：core 229 passed、macos 61、windows host check、漂移检查、cmp 一致。
- ✅ **T107（2026-08-15）：FileMeta 校验提取（不可信输入，R014/R020 高价值）**。`Command::FileMeta` 分支的 4 步校验（batch 协议要求、1 GiB 上限、文件名 basename+规范化、resumable chunk_size）迁入 `sync::validate_incoming_file_meta(&mut FileMeta) -> Result<(), String>`（5 个确定性测试：正常+规范化、缺 batch、超限、非法名、非法 chunk_size）；`MAX_FILE_SIZE` 常量迁入 core（平台 mod.rs 改为 re-export 保持 clipboard.rs 引用不变）；server.rs 715→~693 行。验证：core 234 passed、macos 61、windows host check、漂移检查、cmp 一致。
- ✅ **T108（2026-08-15）：verify_and_commit_received_file 提取**。延迟校验+提交编排（30 行）迁入 `sync::verify_and_commit_received_file(&Arc<tokio::Mutex<SyncEngine>>, source, pending)`（2 个确定性测试：成功提交→platform.files_received 1 次；校验失败→删临时文件+discard+不提交）；server.rs 两处调用点改 `sync::` 前缀，~693→658 行。验证：core 236 passed、macos 61、windows host check、漂移检查、cmp 一致。
- ✅ **T109（2026-08-15）：ReceiveSuspendGuard 提取**。配对期 RAII 挂起守卫（14 行）迁入 `sync::ReceiveSuspendGuard::new(Arc<tokio::Mutex<SyncEngine>>, source)`（1 个确定性测试：drop 后活动接收被挂起——后续 chunk 报 "file transfer metadata is not available"）；server.rs 调用点改 `sync::` 前缀，658→641 行。验证：core 237 passed、macos 61、windows host check、漂移检查、cmp 一致。
- ✅ **T110（2026-08-15）：配对安装段提取（R014 高价值）**。`handle_accepted_connection` 的配对分支（~20 行，含 stream 所有权转移）迁入 `pairing::install_pairing_session(Option<&Arc<PairingManager>>, stream, ...)`（4 个确定性测试：无 manager→写 PeerError 帧、窗口关闭→"Pairing window is closed"、自配对→"Cannot pair this device with itself"、非法 interface→"Invalid pairing interface"）；server.rs 641→636 行。验证：core 241 passed、macos 61、windows host check、漂移检查、cmp 一致。
- ✅ **T111（2026-08-15）：pool.rs resolve_candidates 提取**。候选→连接目标解析（33 行纯规则）迁入 `peer::directory::resolve_candidates(&PeerInfo, tcp_port: u16) -> Result<Vec<ResolvedCandidate>, String>`（6 个确定性测试：排序+映射、无候选合成、tailscale_ip 回退、无地址报错、非法 TCP 地址、非法 Iroh 端点）；`TCP_PORT` 以参数注入保持平台常量单一来源；pool.rs 580→548 行（4 处调用点 + 参数）。验证：core 247 passed、macos 61、windows host check、漂移检查、cmp 一致。
- 剩余（I/O 接线/编排，按 ADR-001 平台保留原则收益递减）：server.rs（636 行）start_server/接收循环/文件响应帧、pool.rs（548 行）连接任务/竞速执行器、iroh.rs、transport.rs —— 后续按需分批，或转入 Phase 2（Windows UI）。

## 4. 已知约束（FACT）

- R003：C 类迁移进 core 时不得引入 tauri/Swift/React/平台 API —— 本清单 6 个 C 类文件当前零 tauri、零 cfg、零 std::os（实测），满足前置条件。
- R005：每次迁移后，将对应文件从 byte-identical 要求中移除（同步收窄检查器职责），并确认 allowed-drift 无 stale 条目（检查器自带检查）。
- 迁移后平台文件保留为薄包装/调用点（仿照 network/types.rs shim 模式，ADR-001 先例）。
- D 类（sync_adapter/updates）不在本 phase 迁移；如未来引入 shared app-runtime crate，重新评估（§22 决策测试）。
