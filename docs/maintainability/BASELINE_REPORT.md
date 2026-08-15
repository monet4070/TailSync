# TailSync Maintainability Refactor — BASELINE_REPORT (T000)

> 生成时间：2026-08-15（本机时区）
> 依据：真实仓库读取 + 5 个并行只读 subagent 的全量文件阅读 + 本机环境探测。
> 所有条目标记 FACT / INFERENCE / UNKNOWN。本报告仅记录事实，未修改任何代码。

---

## 0. 仓库概况

- FACT: 仓库位于 `/Users/monet/TailSync/TailSync`（父目录 `/Users/monet/TailSync` 非 git 仓库）。
- FACT: 当前分支 `refactor/peer-core`，与 `origin/refactor/peer-core` 同步；HEAD = `1849419 fix(peer): preserve connection session contracts`。
- FACT: 工作树干净，唯一未跟踪内容为 `windows/ci-artifacts/`（先前 CI 下载残留：`TailSync-2.1.0-Windows-x64-{setup,portable}.exe`、`.sha256`、`-build.json`；未被 .gitignore 覆盖）。
- FACT: remote = `git@github.com:monet4070/TailSync.git`；tags：v2.0.0 / v2.0.1 / v2.0.2 / v2.1.0。
- FACT: 相关分支存在：`main`、`codex/fix-ci`、`codex/sync-controls-and-shortcuts`、`feat/iroh-transport-and-review-hardening`、`backup/pre-maintainability-refactor-20260814`（重构前备份）。
- FACT: 仓库已有 `CONTEXT.md`（领域上下文，2026-08-14 更新）与 `docs/adr/ADR-001-peer-directory-delivery.md`（已接受）。

## 1. 目录结构（FACT）

```text
TailSync/
├── .github/workflows/ci.yml        # 151 行
├── .github/workflows/release.yml   # 276 行
├── CONTEXT.md / README.md / LICENSE / rust-toolchain.toml
├── docs/RELEASE.md, docs/adr/ADR-001-*.md
├── scripts/                        # validate-release-version.mjs, generate-update-manifest.mjs (+tests), check-windows-host.sh
├── shared/
│   ├── rust-core/                  # tailsync-core 2.1.0（11 个模块，src 约 14k 行，204 个测试）
│   ├── schema/                     # settings.schema.json + generate-settings.mjs
│   └── scripts/                    # migrate_v1.py + test
├── macos/
│   ├── swift-ui/                   # SwiftUI 外壳（6 源文件 4835 行 + 3 测试文件）
│   ├── src-tauri/                  # Rust 守护进程（无头 Tauri）与平台适配
│   ├── scripts/                    # check_cross_platform_sync.mjs|ps1, check_macos_sources.sh, migrate.py, verify_*.sh
│   ├── build-mac.sh / build-dmg.sh / dev.sh / TailSync.app / release/*.dmg
├── windows/
│   ├── src/                        # React 前端（pages: Settings 1774 行, History 1374 行；hooks/utils；8 个测试文件）
│   ├── src-tauri/                  # Rust 应用（commands 1045 行, api 830 行, tray 693 行, clipboard 1173 行…）
│   ├── scripts/                    # check_cross_platform_sync.mjs|ps1, test_cross_project_interop.ps1, package-windows.ps1, migrate.py
│   └── package.json
├── site/                           # 独立营销站点（React/Vite，独立构建，不进应用包）
└── assets/
```

## 2. 产品事实

- FACT: 产品版本 2.1.0；线协议 v3（`shared/rust-core/src/protocol.rs` 的 `VERSION`）；数据库 schema v9（`db.rs:42 SCHEMA_VERSION: i64 = 9`）。三者互相独立（README 明确说明）。
- FACT: macOS 形态 = SwiftUI 菜单栏外壳 + Rust 守护进程；Windows 形态 = React/Tauri 系统托盘应用（README）。
- FACT: Android 不在 v3 兼容范围（README、CONTEXT.md）。

## 3. VERSION_MATRIX（T003 输入）

- FACT: 版本声明位置共 7 处，当前全部为 2.1.0：
  1. `windows/src-tauri/tauri.conf.json` → `"version": "2.1.0"`
  2. `macos/src-tauri/tauri.conf.json` → `"version": "2.1.0"`
  3. `windows/src-tauri/Cargo.toml` → `version = "2.1.0"`
  4. `macos/src-tauri/Cargo.toml` → `version = "2.1.0"`
  5. `shared/rust-core/Cargo.toml` → `version = "2.1.0"`
  6. `windows/package.json` → `"version": "2.1.0"`
  7. `site/package.json` → `"version": "2.1.0"`（营销站点，独立）
- FACT: `scripts/validate-release-version.mjs` 校验其中 5 处（tauri.conf ×2 + Cargo ×3，经 `cargo metadata --locked`），**不**校验 package.json ×2；tag 必须为 `vX.Y.Z` 且与 5 处完全一致；同时判定 stable/prerelease 通道。
- FACT: CI 的 `scripts` job 与 release.yml 的 `validate` job 均调用该校验器；ci.yml 中校验命令：`RELEASE_VERSION="$(node -p "require('./windows/src-tauri/tauri.conf.json').version")"; node scripts/validate-release-version.mjs --root . --tag "v$RELEASE_VERSION"`。
- FACT: `scripts/generate-update-manifest.mjs` 从 release 目录读取 `release-*.json` 片段生成 `latest.json`（仅 stable tag），强制 windows-x86_64 与至少一个 darwin 平台，校验签名文件与版本一致。
- FACT: Cargo.lock 中 tailsync-core = 2.1.0。
- FACT: README 徽章亦含版本（文档性，非校验对象）。
- INFERENCE: 统一 version 工具（R017）的核心价值 = 一次同步 5 个被校验位置 + 2 个 package.json；现有校验器已能机械发现不一致，缺口在"更新入口分散"而非"缺少检测"。

## 4. CI / 门禁（FACT，均为 ci.yml / release.yml 原文）

- ci.yml 四个 job：
  - `frontend`（ubuntu，matrix: windows, site）：`npm ci` → `npm run lint`（oxlint）→ windows 仅 `npm test`（vitest run）→ `npm run build`（tsc -b && vite build）。
  - `rust-windows`（windows-latest，90min）：生成 settings 契约 `--check` → 漂移检查 ps1 → 前端构建 → fmt（core+windows）→ clippy（core+windows, `-D warnings`）→ core 全部测试 → windows lib 测试 → 双向互操作探针 → NSIS 打包 + 烟测。
  - `rust-macos`（macos-latest）：`check_macos_sources.sh`（fmt、clippy、core 测试、macos all-targets 测试、swift test、swift release build、漂移检查）→ `build-mac.sh --skip-swift-build` → `verify_macos_bundle.sh`。
  - `scripts`（ubuntu）：python py_compile 三个 migrate 脚本 → `test_migrate_v1.py` → node --test 两个 mjs 测试 → 版本一致性校验。
- FACT: 工具链固定 1.91.0（`rust-toolchain.toml`：minimal + clippy, rustfmt；CI dtolnay 同版本）。
- FACT: release.yml：tag `v*` 触发；`validate`（版本校验+层级校验）→ `windows`/`macos` 打包（community/trusted 两档，updater 私钥缺失即失败）→ `publish`（stable 才写 latest.json）。
- FACT: R015 列出的门禁（lint/format/clippy/unit/interop/contract/smoke）在当前 CI 中全部真实存在。

## 5. 跨平台漂移检查器（T100 输入核心）

- FACT: 检查器为 `windows/scripts/check_cross_platform_sync.mjs`（469 行，与 macos 侧 byte-identical），CI 经 ps1 包装调用；ps1 与 mjs 均强制 byte-identical。
- FACT: 当前强制 byte-identical 的 `src-tauri/src` 文件（不在 allowed-drift 清单，已用 `cmp` 实机验证全部 IDENTICAL）：
  `api/imports.rs`、`api/transport.rs`、`main.rs`、`network/iroh.rs`、`network/pool.rs`、`network/rate_limit.rs`、`network/server.rs`、`network/types.rs`、`sync_adapter.rs`、`updates.rs`。
- FACT: 允许平台差异（allowed-drift 清单，实机验证确实 DIFFERS 或单侧存在）：`api.rs`、`api/routes.rs`、`clipboard.rs`、`clipboard_change.rs`、`clipboard_file.rs`、`commands.rs`、`lib.rs`、`network/lan.rs`、`network/mdns.rs`、`network/mod.rs`、`network/health.rs`、`network/peer_cache.rs`、`network/tailscale.rs`、`tray.rs`（tray.rs 仅 Windows 存在）。
- FACT: 额外 byte-identical 文件（assertFileMatch）：`src-tauri/build.rs`、`src-tauri/examples/interop_probe.rs`、`scripts/check_cross_platform_sync.mjs`、`scripts/check_cross_platform_sync.ps1`、`scripts/test_cross_project_interop.ps1`。
- FACT: 检查器职责已远超"逐字节比对"，包括：re-export shim 钉扎（`network/types.rs` 必须 re-export `PeerStatus`、`network/tailscale.rs` 必须 re-export `PeerInfo`，注释掉不算）、Settings 四端契约（schema ↔ Rust `crypto.rs::Settings` ↔ Swift `AppSettings` ↔ TS `SettingsData`，字段名+类型）、PairingStatus/PairingPeerStatus、HistoryEntry、FileProgress、PeerSnapshot/PeerInfo+routes、PeerRouteSnapshot 固定字段集、端口 19889/19890 三端一致、macOS stdin token 管道、Info.plist Bonjour、build-mac.sh PlistBuddy、clipboard-helper `--write-files`、sync_adapter 必须 `add_file_batch_with_status(..., true, ...)` 等。
- FACT: 检查器自带负向自检（注释掉的 re-export 不得通过）与 stale allowed-drift 条目检查。
- INFERENCE: 该检查器即 R005 要求的 drift checker，且已实现"逐渐缩小职责（byte 比对 → 契约比对）"的形态；byte-identical 清单目前集中于真正需要一致的平台实现。

## 6. shared/rust-core（R002/R003/R004/R010-R014 输入）

- FACT: crate `tailsync-core` 2.1.0，edition 2021，无 workspace（三个独立 crate：core、macos app、windows app）。
- FACT: 模块（lib.rs）：`crypto`、`db`、`history_classifier`、`identity`、`iroh_transport`、`pairing`、`peer`（delivery/directory/health/types）、`protocol`、`secure`、`sync`、`sync_warning`；无统一 facade re-export。
- FACT: 平台相关依赖已存在（R003 现状而非目标）：`cfg(windows)` → windows-sys；`cfg(macos)` → core-foundation/libc/security-framework；用途为密钥存储/路径判定（INFERENCE：具体位置在 identity/secure/paths）。
- FACT: 错误类型现状：类型化 error 已有 3 处 —— `ProtocolError`（protocol.rs:338，thiserror）、`IdentityError`（identity.rs:13，thiserror）、crypto.rs:396 的 derive(Error) 枚举。其余大量 `Result<_, String>`：core 共 66 处（sync.rs 22、peer/delivery.rs 17、iroh_transport.rs 9、peer/directory.rs 4、pairing.rs 4、crypto.rs 2、protocol.rs 1、identity.rs 1）；`map_err(|e| e.to_string())` 类约 142 处；平台 crate 另有 189 处 `Result<_, String>`。
- FACT: 日志现状：core 全库仅 15 处 log 调用（debug 8 / warn 6 / error 1；sync.rs 仅 1 处，delivery.rs 13 处中的大部分）；平台 crate 约 84 处（lib.rs 22/21、tray 7、sync_adapter 6/6、updates 4/4、lan 4/4…）。无 tracing，无结构化字段，无 diagnostics 概念。
- FACT: 测试分布（core 共 204）：db.rs 37、sync.rs 24、crypto.rs 20、peer/delivery.rs 26、peer/types.rs 14、peer/directory.rs 13、peer/health.rs 12、secure.rs 10、protocol.rs 10、iroh_transport.rs 7、identity.rs 7、pairing.rs 6、history_classifier.rs 5、sync_warning.rs 1。
- FACT: `iroh_transport.rs` 的 `repeated_rtt_probes` 测试已 `#[ignore]`（CONTEXT.md：本机 QUIC 环境回归，2026-08-14 起记录）。
- FACT: core 的 `test-support` feature 存在（测试辅助门控）。
- INFERENCE: R012/R013 的自然起点 = protocol/identity/crypto 已有类型化错误，sync/db/pairing/delivery 是主要 String-error 区域；测试断言依赖错误子串（sync.rs 测试多处 `.contains(...)`），迁移需同步改测试契约。

### 6.1 sync.rs（R010 输入）

- FACT: 2344 行；生产代码 L1–1518 **零 cfg 块**（仅测试内 L1782/1786 有 cfg(windows)/cfg(unix) 符号链接分支）；24 个测试全部在单一 `mod tests`（L1519–2344，占文件 35%）。
- FACT: 公共面：`SyncEngine`（struct，字段为 6 个 keyed HashMap + clipboard_generation，L515-531）、`pub trait SyncPlatform`（L409-425，7 个方法，全部 `Result<(), String>`：write_text/write_image/set_file_progress/clear_file_progress/set_file_batch_progress/files_received/file_batch_failed）、常量（INCOMPLETE_TRANSFER_RETENTION_SECONDS=24h、MAX_FILE_BATCH_COUNT=20、MAX_FILE_BATCH_BYTES=1GiB 等）、自由函数（prepare_file_batch、revalidate_prepared_file、normalize_transferred_file_name、cleanup_expired_transfers、file_batch_admission_lock）。
- FACT: sync.rs 是**接收侧 facade**：依赖 `protocol`（FileChunkPayload/MessageId/TransferId/FILE_CHUNK_SIZE）与 `db::get_incoming_dir()`；**不引用 peer/ 或 delivery**；DB 写入（add_file_batch_with_status 等）发生在平台 sync_adapter，不在 sync.rs。
- FACT: 状态模型 = 隐式 map 增删（incoming_batches/completed_batches/cancelled_batches/active_receives/completed_transfers/seen_messages + 剪影过滤器 ShadowFilter L35-91），**无显式状态机枚举**；重试/超时语义在 peer/delivery.rs。
- FACT: 续传高风险区：`begin_file_receive`（L860-1030，`{id}.part` + `{id}.resume.json`，恢复时 deferred 全文件校验）、`handle_resumable_file_chunk`（L1032-1099，严格 `chunk.offset == state.received` 单等式门）、`persist_transfer_state`/`persist_incoming_batch`/`restore_persisted_received_file`（L1398-1455）、`cleanup_expired_transfers`（L1457-1517）。5 个专项测试覆盖续传（L1919/2072/2142/2193/2253 附近）。
- FACT: 建议拆分方向（R010 的 sync/{mod,engine,shadow,text,image,file/...}）与现状不符：shadow/text/image/file 职责全部内联在单文件，且无 engine 概念（SyncEngine 即 facade）。拆分需先做职责图（T320）。
- FACT: 两端 `sync_adapter.rs` **byte-identical**（230 行，md5 `df670040f711f54501672a333a5e0072`）：实现 SyncPlatform，含 Wry 剪贴板写入、`FileProgressCleanup`/`HistoryVersionBump` RAII、files_received 内 spawn_blocking DB 写入、通知；平台差异仅为类型层（tauri::image::Image）。这是 R004 最清晰的候选之一。

### 6.2 db.rs（R011 输入）

- FACT: 2473 行；`HistoryDB` 定义于 db.rs:70-77（包装私有 rusqlite::Connection）；**64.6% 为测试**（test module L877-2473，1597 行）。
- FACT: db.rs = 单体内核 + re-export 枢纽：10 个私有 `mod`（file_encryption/file_storage/legacy_v1/lifecycle/migrations/paths/queries/schema/storage/types，**无 mod.rs**）；`HistoryDB` 的 impl 分散在 6 个文件（db.rs/lifecycle/queries/migrations/storage/legacy_v1），通过 `use super::*` 共享私有 conn —— 按关注点拆分但未解耦。
- FACT: schema.rs 持有 DDL（initialize L3-52：schema_version/history/4 索引/migration_issues）；`SCHEMA_VERSION = 9` 在 db.rs:42（版本常量与 DDL 分居两文件）；migrations.rs 为命令式 `if version < N` 阶梯（`HistoryDB::migrate` L40-270，open_at db.rs:115 调用）：v1 基础（L54-58）、v2 占位（L60-64）、v3 文件外置+解密持久化（L66-99，记录 migration_issues）、v4 图片外置+VACUUM+WAL checkpoint（L101-137）、v5 分类列+索引（L139-166，add_column_if_missing L636-650）、v6 多标签 json_array 回填（L168-182）、v7 加密文件历史（L200-206；批式行加密 `migrate_file_history_encryption_batch` L525-538 用**独立 SQLite 连接**避免阻塞启动）、v8 批次+pinned 列（L208-245）、v9 明文预览擦除（secure_delete+占位符+索引重建+WAL checkpoint+VACUUM，L247-267）；`retry_unresolved_migration_issues`（L475-520）仅重试 v3/v4。
- FACT: 布局常量：`history-v2.db`、`file-history/`、`image-history/`、`incoming/`、`clipboard-files/`（storage.rs:291-301 bulk_storage_names）。
- FACT: 错误处理：db 层**无 thiserror**，无 error 枚举；主导签名是 `Result<_, Box<dyn std::error::Error>>`，面向用户的校验错误用 `"…".into()` 临时转字符串；仅 paths.rs 4 个函数返回 `Result<_, String>`。字符串错误现场分布：db.rs ~21、storage.rs ~13、file_encryption.rs 17+、file_storage.rs 4、legacy_v1.rs 4、migrations.rs 2、queries.rs 2（含 "Unsupported history category"、"start_time must be earlier than end_time"）。
- FACT: 文件/容器格式：file_encryption.rs 容器 MAGIC `"TSFENC1\0"`（L11）、CHUNK_SIZE=1MiB（L12）、TAG_SIZE=16（L13）、HEADER_NONCE_INDEX=u32::MAX（L17）、KEY_INFO `"tailsync-file-history-v1"`（L18），ring AES_256_GCM + HKDF-SHA256（DEK 来自 crypto::get_dek），header/chunk AAD 分离；file_storage.rs 引用 MAGIC `"TSFILE1\0"`/`"TSIMAGE1"`（L8-9）、MAX_STORED_ORIGINAL_NAME_BYTES=120（L10）、5GiB FILE_HISTORY_BYTE_LIMIT（L11）；文件命名 `{data_hash}-{sanitized_name}`、图片 `{hash}.bin`（file_storage.rs:139）。
- FACT: legacy_v1.rs：Fernet 导入旧 history.db（`migrate_legacy_v1_at` L47-122，只读打开、解密、分发 add_*_migrated）；幂等性靠 `v1-migration-report.json`（键 = 源大小+mtime，L11/L161-166）；数据目录 `TAILSYNC_V1_DATA_DIR` 覆盖，默认 `$HOME/TailSync_History`（L151-159）；非阻塞（open_at db.rs:134-136 失败仅 warn）。文件 50% 为测试（2 个：全类型导入+幂等 L283-337、坏行记录+变更后重试 L340-380）。
- FACT: 测试基础设施：统一 temp-dir（`tailsync-…-{rand}`）+ `open_in_memory` 模式，无持久 fixture；多个测试模块手工声明 `history` 表结构（与 schema.rs 存在漂移，如 db.rs:892-910 缺 `created_at`）；storage.rs 自带 4 测试（含 corrupt_sqlite_is_rejected_during_migration_verification L423）、paths.rs 2 测试（含 unc_storage_is_rejected L181）。db.rs 内测试数：本人 grep 计数 37（subagent 名列表 38，差异源于多行属性写法）。
- FACT: 平台 cfg 集中在 paths.rs（Windows GetDriveTypeW / macOS libc::statfs+MNT_LOCAL / fallback）与 file_encryption.rs 原子替换（Windows MoveFileExW / 其他 rename）。
- FACT: 加密容器：file_encryption.rs 用 ring AES-256-GCM，CHUNK_SIZE=1MiB，TAG_SIZE=16，HKDF-SHA256（DEK 来自 crypto::get_dek）；file_storage.rs 有 5GiB FILE_HISTORY_BYTE_LIMIT、文件名清理 ≤120 字节。
- FACT: legacy_v1.rs：Fernet 导入旧 history.db；幂等为双层机制——整库迁移由 `v1-migration-report.json` 按**源大小+mtime** 门控（legacy_v1.rs:11/L161-166），行级插入由内容哈希去重（`exists_by_hash`，db.rs:238-245）；损坏条目记录到报告（LegacyMigrationFailure）但不阻塞；原库保留不删除。db 树共 56 个唯一测试，本人 grep 计数 db.rs 内 37 个；测试存在 schema 重复定义（如 db.rs:892-910 与 schema.rs:28 的 created_at 差异）。
- FACT: 与 R011 判断一致：db.rs 行数主要由测试构成，拆分重点是"测试移出 + impl 块解耦"，而非重设计 DB。

### 6.3 peer/（ADR-001 已迁移，R004 现状）

- FACT: `peer/directory.rs` 887 行（13 测试）— 发现合并、候选补全/排序、模式与地址判定、配对目标解析。
- FACT: `peer/health.rs` 798 行（12 测试）— HealthTracker 状态机（discovered→online→confirming→offline、12s TTL、两轮 miss）、SessionRegistry/SessionGuard RAII 租约。
- FACT: `peer/delivery.rs` 1919 行（26 测试）— 帧记账、ACK 期望、DeliveryError（类型化，thiserror 风格 Display）、连接竞速、ConnectionWorker、ConnectionAdapter trait（register_session 返回租约）。
- FACT: `peer/types.rs` 500 行（14 测试）— PeerInfo/PeerCandidate/PeerRouteSnapshot/PeerStatus 等契约类型（serde 字段名即 JSON 契约）。
- FACT: 平台 network/types.rs 与 network/tailscale.rs 已降级为纯 re-export shim（漂移检查钉扎）。
- FACT: 重构净效果（backup 分支对比）：+4974/−3481 行；平台 mod.rs（mac −689/win −844）、pool.rs（mac −694/win −685）大幅瘦身；pool.rs/server.rs 变为 byte-identical。
- FACT: CONTEXT.md 记录两个未决差异：peer_cache.rs 探活循环（macOS 轮式）vs health.rs 的 update_peer_health/record_probe_*（Windows 逐路由式）双喂入方式暂保留。

## 7. Windows UI（R006/R007/R008 输入）

### 7.1 Settings.tsx（1774 行）

- FACT: UI 分区（行号）：标题栏 L970-985（loading 态 L947-962）；连接与设备 L990-1262；通用 L1264-1393；历史 L1395-1427；存储 L1429-1483（注释误标 "Appearance"）；外观 L1485-1580；更新 L1582-1614；快捷键录制对话框 L1617-1693；配对对话框 L1695-1766；toast L1768-1771。
- FACT: 状态：29 useState（L197-224）+ 16 useRef（L233-247）；**0 useMemo / 0 useCallback**；useEffect 10 个（L249-876 区域）。轮询内联实现（设备 5s setInterval L324、配对 1s L551），**未使用已存在的 useVisiblePolling hook**。
- FACT: invoke 面：**26 个不同 command，31 个调用点**（L250-916），事件订阅 2 个（`listen("sync-state-changed")` L281、`listen("peer-health-changed")` L327）+ `getCurrentWindow().hide()`。仅约 9 个调用带 TS 泛型注解；`update_settings`/`toggle_peer`/`forget_peer`/`set_sync_*`/`suspend|resume_sync_shortcut`/`enable|start|cancel|confirm_pairing`/`refresh_peers` 等均无类型契约（参数形状只能从调用点推断）。
- FACT: 跨 feature 耦合中心：单一 `settings` state + 单一 `update()` 写回通道（L338-374，LatestRequest+SerialTaskQueue 乐观写 + 双重回滚），update/连接模式/同步开关/历史/存储配额/主题/语言/快捷键全部读写它；`devices` state 被设备/配对/连接测试三个 feature 共享。更新 feature（get_update_status/check_for_update/install_update）是唯一干净隔离的。
- FACT: 已抽取模块：useTheme、useI18n、asyncControl（LatestRequest/SerialTaskQueue）、utils/shortcut、ThemeLogo、settings.generated.ts。内联未抽：10 个 useEffect、10 个页面级 interface（L41-186）、~900 行 JSX 单组件、两个近似相同的焦点陷阱实现（L443-450 vs L818-823）、重复 toast 计时（L352-354 vs L697-699）、changeStorage 重跑 init 水合（L398-424 重复 L250-272）。
- FACT: 复杂度热点：update() 双回滚编排；设备行 JSX 5-6 层嵌套三元（L1104-1244）；shortcut 录制状态机横跨 6 个函数 + 2 组键盘 effect（L680-876）；配对流程轮询自开对话框（L530-624）；i18n 手动拼接（.replace("{version}") 等 5 处）。
- FACT: Settings.test.tsx（296 行）：mock invoke/listen/window/dialog/useTheme/useI18n；覆盖 = 整页渲染下的 4 个场景（版本检查/更新安装、sync-state-changed 事件、快捷键录制确认、快捷键冲突）+ pairingAddressForPeer 3 例 + routeSupportsLatencyTest 2 例。未覆盖：存储迁移/配额、历史滑块、主题/语言切换、连接模式、连接测试、peer toggle/forget、配对全流程、焦点陷阱、轮询计时器。

### 7.2 History.tsx（1374 行）

- FACT: UI 分区：标题栏/搜索 L920-960、分类/日期 FilterDropdown L962-1015、结果计数/迁移警告 L1017-1031、骨架/空态/列表（含批次头）L1034-1263、分页 L1265-1298、文件进度条 L1300-1330、toast/清空确认 L1332-1371；子组件 FilterDropdown（177-276）、ThumbnailCanvas（414-450）、LazyThumbnail（452-490）。
- FACT: 状态：24 useState + 12 useRef + 3 useMemo；useEffect 10 个；两个 useVisiblePolling 轮询（5000ms 设置轮询 L767；800ms 版本/同步/进度轮询 L768-798，版本变化触发 loadHistory）。
- FACT: invoke 面：**14 个调用点**（get_settings L621、get_image_data L634、get_migration_diagnostics L662、get_history_capabilities L672、get_history_page L706、get_version L770、get_sync_warning L779、get_file_progress L793、restore_entry L831、delete_entry L846、clear_history L858、restore_file_batch L881、set_history_pinned L891、cancel_file_batch L903）；无 listen（已 grep 复核，仅 Settings 有 listen）。
- FACT: `get_history_page` 参数契约：`{keyword: string|null, category: HistoryCategory|null, startTime: ISO|null, endTime: ISO|null, limit: 30, offset: page*30}` → `HistoryPageResult {entries, total, has_more}`（L697-710）；`get_image_data` 返回 `ImageThumbnail` b64+尺寸（L634）。这些形状仅由调用点与测试 fixtures 隐式定义（R008 输入）。
- FACT: History.test.tsx 的 5 个用例均在同一 describe（L32-279）：手势-only 行操作（restore_entry/delete_entry L59-80）、紧凑工具栏+分类筛选（L82-103）、copy-all 与 pin 持久化（L105-176）、批次折叠/展开+动画（L178-233）、不完整批次计数（L235-279）；**invoke mock 的 mockImplementation switch 块在测试内逐字重复 4 次**（L34-56/136-158/193-215/250-272）。
- FACT: 职责归属：已抽离 = pagination（historyPagination.ts）、异步过期（asyncControl.LatestRequest）、轮询（useVisiblePolling）；页内内联 = query、filter、preview、render、thumbnail cache（Map + refs，有 `MAX_CACHED_THUMBNAILS` 上限 L115/644）。
- FACT: 复杂度热点：loadHistory 80 行 7 依赖内联回调（L679-758）；分组 IIFE（L1080-1261）含批次折叠算术；13 字段 fileProgress state（L528-541）；4 处重复 toast 超时模式；document.querySelector(".history-list").scrollTo ×2。
- FACT: History.test.tsx（280 行）：5 个用例（手势操作、单工具栏、copy-all+pin、批次折叠/展开+动画、不完整批次计数）；未覆盖缩略图、clear_history、cancel_file_batch、自定义日期、进度条、迁移横幅。

### 7.3 前端总体（R008 输入）

- FACT: 全部前端 invoke 集中在 2 个页面文件（Settings 31 + History 14 = 45 调用点）；hooks/utils 层无任何 invoke —— typed client boundary 的引入面非常集中。
- FACT: 栈：React 19 / Vite 8 / Vitest 4 / oxlint / TS 6.0 / @tauri-apps/api 2.11；8 个测试文件（hooks×3、pages×2、utils×3）。vitest 配置：jsdom + setup 文件。
- FACT: 无 lint/test 之外的类型门禁问题：`tsc -b` 在 build 中执行（noUnusedLocals/noUnusedParameters 开启）。
- FACT: 前端测试 mock 模式：vi.hoisted + vi.mock 整模块替换 invoke/listen/window/dialog，测试契约靠 fixtures 隐式定义（即"mock 即契约"，与 Rust 命令层无自动一致性检查）。

## 8. 命令层与本地 API（R009 输入）

- FACT: Windows `commands.rs` 1045 行：35 个 `#[tauri::command]`，**全部 `Result<…, String>`**，无 thiserror/anyhow；注册于 lib.rs:579-622 单点 invoke_handler。领域分布与行号（W = windows commands.rs）：history 10（get_history W126-147、get_history_page W149-175、get_history_capabilities W177-180、get_migration_diagnostics W182-192、search_history W195-203、delete_entry W206-212、clear_history W215-221、restore_entry W225-350、set_history_pinned W802-815、restore_file_batch W827-835）；devices/peers 4（get_peers W353-363、refresh_peers W366-371、test_connection W374-390、toggle_peer W491-506）；pairing 7（enable/get/start/confirm/cancel_pairing W447-488、trust_peer W393-430、forget_peer W433-445）；settings 4（get_settings W564-568、update_settings W571-609、set_sync_shortcut W545-561、get_sync_state W509-516）；sync 4（set_sync_enabled W519-522、toggle_sync W524-527、suspend/resume_sync_shortcut W529-543）；storage 4（get_storage_status W729-732、change_storage_location W734-799、delete_old_storage W817-824、cancel_file_batch W699-704 + 实现 W706-727）；updater 3（get_update_status W873-879、check_for_update W881-886、install_update W888-891）；clipboard/misc 7（get_file_progress W690-697、get_image_data W669-687、get_version W852-857、get_sync_warning W859-863、open_history_window W612-638、open_settings_window W641-666、cancel_file_batch 已计）。
- FACT: macOS `commands.rs` 28 个命令（M 行号：history 10、peers 6、pairing 5、settings 2、misc 5），**缺 11 个 Windows-only 命令**：get_sync_state、set_sync_enabled、toggle_sync、suspend/resume/set_sync_shortcut、test_connection、open_history_window、open_settings_window、get_update_status、check_for_update、install_update；注册于 macos lib.rs:503-534。
- FACT: 厚命令（直接 DB + 业务规则 + 网络编排 + 回滚状态机）：`restore_entry`、`update_settings`、`change_storage_location`、`trust_peer`、`cancel_file_batch_impl`；其余为薄转发。
- FACT: 两端 `api.rs` = 127.0.0.1:19889 JSON-lines TCP 服务（routes.rs 单大 match 分发，32 字节 hex token，常量时间比较），**与 commands.rs 大量重复业务逻辑**；两端 lib.rs 均启动它（windows lib.rs:419、macos lib.rs:374）。
- FACT: api.rs 的 cmd 面（routes.rs match + imports.rs）：`ping`、`check_for_update`、`install_update`、`get_file_progress`、`cancel_file_batch`、`restore_file_batch`、`get_storage_status`、`get_version`、`get_sync_warning`、`get_history_capabilities`、`get_migration_diagnostics`、`get_status`、`enable_pairing`、`get_pairing_status`、`start_pairing`、`confirm_pairing`、`cancel_pairing`、`get_history`、`delete_entry`、`clear_history`|`clear_all`、`restore_entry`、`get_settings`、`get_sync_state`、`set_sync_enabled`、`toggle_sync`、`set_sync_shortcut`、`update_settings`、`change_storage_location`、`delete_old_storage`、`set_history_pinned`、`get_peers`、`refresh_peers`、`toggle_peer`、`trust_peer`、`forget_peer`、`test_connection`、`reconnect_peers`、`get_image_data`、`begin_import`、`import_chunk`、`finish_import`、`migrate_entry`、`quit`（import 命令在 imports.rs AI61-259）。api.rs 与 commands.rs 的重复实现已定位：restore_entry（AR403-499，且实现路径不同：走 sync_engine.restore_* 而非命令层直接剪贴板写）、update_settings（AR569-634）、get_peers/refresh_peers（AR754-784）、change_storage_location（AR636-716）、trust_peer（AR819-898）；共享 helper 仅 peer_snapshot_data、history_capabilities_data、cancel_file_batch_impl、materialize_file_batch_paths。
- FACT: macOS 接线特殊：守护进程是**无头 Tauri**（tauri.conf windows:[]）；SwiftUI **不调用 invoke**，经 ApiClient.swift 直连 TCP 19889 JSON-lines（同 token）；macOS commands.rs 28 个命令对真实 UI 是死表面（唯一 webview 面是极简 frontend/index.html）。
- FACT: 错误到用户面：SwiftUI 用 `message.contains(...)` 子串匹配原始 Rust 错误字符串做本地化（ApiClient.swift L730-744，`ApiError::pairingErrorDescription`，匹配 "Pairing window is closed" / "Pairing handshake timed out" / "Connection reset by peer" / "early eof"）—— 字符串错误已是跨层契约。
- FACT: 全局状态：两端 `AppState`（Arc<Mutex> db/sync_engine/settings/identity/pool/pairing + shutdown watch）；api.rs 侧另有进程级 `CLIPBOARD_VERSION` AtomicU64（A41，被 W210/219/348/813 等 bump）、`FILE_PROGRESS` 与 `CANCELLED_FILE_BATCHES` LazyLock 单例（A52-55）、独立 `ApiState`（A381-391，含 token 与 imports）；macOS 无 global_shortcut 插件/剪贴板状态（ML388-392 vs WL436），且 macOS 不启动 Tauri tray（WL478-479 有 `not(target_os="macos")` 门控）并隐藏 daemon dock 图标（ML153-201）。

## 9. 测试布局（T002 输入）

- FACT: Rust 测试：core 204 / macos app 63 / windows app 64（`#[test]`+`#[tokio::test]` 计数）；macos/windows app 的 CI 只跑 `--lib`。
- FACT: Swift 测试：3 个文件（AppBehaviorTests/GlobalShortcutTests/ThemeTests）。
- FACT: 前端测试：8 个 vitest 文件（上述）。
- FACT: Python：`shared/scripts/test_migrate_v1.py`（CI 运行）、migrate.py 仅 py_compile。
- FACT: Node：`scripts/generate-update-manifest.test.mjs`、`validate-release-version.test.mjs`（CI `node --test`）。
- FACT: 互操作：`test_cross_project_interop.ps1`（双向起两个 interop_probe：一侧 server 一侧 client，含 Noise 握手与文件/文本往返断言；CI Windows job 运行）+ `interop_probe.rs`（byte-identical 示例二进制）。
- FACT: 无 property/fuzz 测试目标（R020 现状为 0）。

## 10. 本机环境（T002 可运行性）

- FACT: cargo/rustc 1.91.0（rust-toolchain 匹配，active）；node v26.0.0；python 3.14.3；swift/xcodebuild 可用。
- FACT: **pwsh 不可用** → `test_cross_project_interop.ps1` 本机无法运行（CI-only）。
- FACT: windows/node_modules 与 site/node_modules 不存在 → 前端 lint/test/build 需先 `npm ci`；swift .build 不存在。
- FACT: macos app crate 在 macOS 本机可完整编译/测试；windows app crate 可用 `scripts/check-windows-host.sh`（fmt + check --all-targets + test --no-run）做 host 侧验证（CONTEXT.md 明确：host 编译会产生 dead-code 伪警告，故不用 clippy -D warnings）。
- INFERENCE: 本机可验证 = core fmt/clippy/test、macos crate fmt/clippy/test、windows host check、swift test、前端（npm ci 后）、漂移检查 mjs、版本校验、mjs/python 测试；不可验证 = Windows 原生 clippy/打包/烟测、双向互操作 PS1、macOS 打包验证脚本。

## 11. 日志与错误处理汇总（T350/T400 输入）

- FACT: 级别分布（全仓 rust）：error 38 / warn 30 / info 14 / debug 17；core 仅 15 处（8 debug / 6 warn / 1 error）。平台 lib.rs 承担绝大多数日志（22+21）。
- FACT: 无结构化日志字段、无 transfer/peer 标识符约定（sync.rs 仅 1 处 log；错误上下文靠 String 拼装）。
- FACT: 字符串错误面：core 66 处 `Result<_,String>` + ~142 处 map_err/to_string；平台 189 处；Swift 端依赖子串匹配。windows commands.rs 内 map_err(to_string) 约 40 处；api.rs 用专用 `Response {ok, data, error}` 结构（A453-460）序列化为 `{"ok":…,"error":…}`（AT160-164），传输层另有 "request read timed out"/"unauthorized" 等字符串错误（AT122/55）。
- FACT: 类型化错误仅 protocol/identity/crypto 三处（thiserror）。
- INFERENCE: R018 diagnostics 与 R012 typed error 有共同前置：先为 core 的 sync/db/pairing/delivery 建立结构化错误与标识符（TransferId 已存在），再谈导出。

## 12. 假设核对（Prompt 假设 vs 真实代码）

- FACT: Prompt 假设 "Settings.tsx 1684 行" → 实际 **1774** 行。
- FACT: Prompt 假设 "shared/rust-core 平台无关" → 基本成立（生产代码几乎无 cfg），但**已存在 cfg 平台依赖**（windows-sys / security-framework 等，用于密钥与路径），R003 应解释为"不再新增"，而非"现状为零"。
- FACT: Prompt 假设 "commands.rs 是唯一命令层" → 实际存在**双命令面**（Tauri commands + JSON-lines api.rs），且 api.rs 与 commands.rs 大量重复 —— R009 拆分必须同时考虑两端。
- FACT: Prompt 假设 "sync.rs 可拆 sync/{engine,shadow,text,image,file}" → 现状为单文件 facade + 隐式 map 状态机；peer/ 已按 ADR-001 迁移完成；sync_adapter.rs 已 byte-identical（R004 就绪候选）。
- FACT: Prompt 假设 "drift checker 只做 byte 比对" → 实际已演进为多层契约检查器（见 §5）。
- FACT: Prompt 假设 "History.tsx 职责需拆" → 已有 historyPagination/asyncControl/useVisiblePolling 抽取，剩余 query/filter/preview/render/cache 内联（R007 输入确认）。
- UNKNOWN: 本仓库是否还有 Prompt 中提及的其他历史分析文档（如 windows/src/pages/Settings.tsx 1684 行数字来源）；T001 会以本报告为准重建追溯矩阵。

## 13. 已知缺口 / 风险（登记，不处理）

- FACT: `iroh_transport::repeated_rtt_probes` 被 ignore（本机 QUIC 回归）。
- FACT: peer_cache/health 双喂入模式未统一（有意的平台差异，ADR-001 未决项）。
- FACT: windows/ci-artifacts/ 未跟踪残留未 gitignore。
- FACT: 前端测试契约与 Rust 命令签名无自动一致性检查（mock 即契约）。
- FACT: macOS commands.rs 死表面（28 命令）——维护成本项，但**不在任何当前任务范围内**，仅登记。
- INFERENCE: 所有上述问题均不影响 T001/T002 启动。

## 14. 环境能力总结

- FACT: 本机可运行：core fmt/clippy/test、macos crate 全套、windows host 检查、swift test、漂移检查、版本/清单校验、mjs/python 测试、前端（npm ci 后）。
- FACT: 本机不可运行：Windows 原生 clippy/打包/烟测、双向互操作 PS1、macOS 打包验证、真实网络 QUIC 测试（部分）。
