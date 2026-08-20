# TailSync Maintainability Refactor — REQUIREMENTS_TRACEABILITY (T001)

> 生成时间：2026-08-15
> 依据：`BASELINE_REPORT.md`（T000，全 FACT 标注）。本文件将 R001-R022 映射到：真实文件 / 相关 Task / 验证方式。
> 不修改任何代码。行号与文件均为 T000 实测事实。

---

## R001 — 保持现有行为

- 覆盖文件：全部（纯重构任务的硬约束）。
- 相关 Task：所有 implementation Task（T101+/T201+/T221+/T241+/T301+/T321+/T351+/T402+/T421+）。
- 验证方式：T002 regression baseline + 每 Task 的 targeted tests + 漂移检查 + 行为对比（BEHAVIOR_CHANGE_ALLOWED=FALSE 协议）。
- 关键证据锚点：core 204 测试、macos 63、windows 64、前端 8 文件、漂移检查器全量。

## R002 — 保持跨平台兼容

- 覆盖文件：`shared/rust-core/src/protocol.rs`（v3）、`shared/rust-core/src/crypto.rs`（Settings 契约）、`shared/rust-core/src/peer/types.rs`（JSON 契约）、`shared/rust-core/src/db/`（storage format v9）、两端 `api/routes.rs` + `api.rs`（API contract）。
- 相关 Task：T001（本表）、T100/T101+、T300/T301+、T320/T321+、T350/T351+、Phase 4。
- 验证方式：`windows/scripts/check_cross_platform_sync.mjs`（四端 Settings 契约、peer 契约、端口、shim 钉扎）+ `interop_probe` 双向测试 + core 契约测试。

## R003 — Shared Core 保持平台无关

- 覆盖文件：`shared/rust-core/Cargo.toml`（L29-36 已存在 cfg 平台依赖——边界是"不新增"）、`shared/rust-core/src/`（生产代码零 cfg，sync.rs L1-1518 已验证；唯一例外：db/paths.rs 与 db/file_encryption.rs 的平台 cfg，属合理平台差异）。
- 相关 Task：所有涉及 core 的 Task；新增 abstraction 时必须经 §20 Maintainability Decision Test。
- 验证方式：code review + grep 检查新增 cfg；同步检查 `RUSTFLAGS`/host 编译（check-windows-host.sh）。

## R004 — 消除 byte-identical runtime source duplication

- 覆盖文件（当前 byte-identical 清单，BASELINE §5/§6.1）：`sync_adapter.rs`（230 行，md5 df6700…）、`updates.rs`、`api/imports.rs`、`api/transport.rs`、`network/iroh.rs`、`network/pool.rs`、`network/rate_limit.rs`、`network/server.rs`、`network/types.rs`、`main.rs`、`build.rs`、`examples/interop_probe.rs`。
- 相关 Task：T100（分类）→ T101+（逐模块迁移，候选优先级：sync_adapter 高重复+平台无关+测试充分）。
- 验证方式：T101+ 每模块迁移后核心测试 + 漂移检查（迁移后从 byte-identical 清单移除并同步缩小 allowed-drift）；`cmp` 复核。

## R005 — 保留 drift checker

- 覆盖文件：`windows/scripts/check_cross_platform_sync.mjs`（469 行，多层契约检查器）、`macos/scripts/check_cross_platform_sync.ps1`、`windows/scripts/check_cross_platform_sync.ps1`。
- 相关 Task：T100/T101+（迁移后逐步缩小 byte 比对职责，只收窄 allowed-drift 清单，**不得删除检查器**）。
- 验证方式：每次漂移相关修改后运行 mjs 检查（CI rust-windows job 同款命令）。

## R006 — Settings UI 降低复杂度

- 覆盖文件：`windows/src/pages/Settings.tsx`（1774 行，26 命令/31 invoke/29 useState/10 effect/0 memo）。
- 相关 Task：T200（责任图，已完成数据收集于 BASELINE §7.1）→ T201+（逐 feature 提取，顺序由依赖图定：update 隔离度最高可先行）。
- 验证方式：前端 vitest（现有 Settings.test.tsx 4 场景）+ tsc build + oxlint；每 feature 提取后跑全套前端测试。

## R007 — History UI 降低复杂度

- 覆盖文件：`windows/src/pages/History.tsx`（1374 行，14 invoke/24 useState/10 effect）。
- 相关 Task：T220（责任图，数据已收集 BASELINE §7.2）→ T221+（query/pagination/filter/preview/render/cache 逐职责提取；pagination/asyncControl/polling 已抽离）。
- 验证方式：History.test.tsx 5 场景 + tsc + oxlint + vitest 全套。

## R008 — 建立 typed frontend/backend client boundary

- 覆盖文件：`windows/src/pages/Settings.tsx`（31 invoke）、`windows/src/pages/History.tsx`（14 invoke）；目标新增 `windows/src/tailsyncClient.ts` 类模块（尚未存在）。
- 相关 Task：T240（TAURI_API_MATRIX：35 命令签名 + 43 api cmd，数据已收集）→ T241+（逐 feature 迁移）。
- 验证方式：迁移后页面不再直接 import `invoke`（grep 验证）；vitest + tsc；mock 契约保持。

## R009 — Tauri commands 变薄

- 覆盖文件：`windows/src-tauri/src/commands.rs`（1045 行，35 命令全 `Result<_,String>`）、`macos/src-tauri/src/commands.rs`（676 行，28 命令，死表面 K003）、`windows/src-tauri/src/api/routes.rs`（1101 行单 match，与 commands 重复 5 处）。
- 相关 Task：T300（责任图，数据已收集 BASELINE §8）→ T301+（按领域拆；厚命令 restore_entry/update_settings/change_storage_location/trust_peer/cancel_file_batch_impl 优先）。
- 验证方式：core + macos/windows crate 测试、漂移检查（commands.rs 在 allowed-drift 清单内，拆分需同步两端并保持行为）。

## R010 — Sync Engine 渐进式拆分

- 覆盖文件：`shared/rust-core/src/sync.rs`（2344 行，24 测试，22 个 `Result<_,String>`，零 cfg 生产代码）。
- 相关 Task：T320（责任图，数据已收集 BASELINE §6.1）→ T321+（优先提取低耦合/边界明确/测试充分的内部逻辑；保持 `SyncEngine`/`SyncPlatform` public facade 稳定）。
- 验证方式：core 全套测试（204）+ 漂移检查；每次提取后 `cargo test --locked`。

## R011 — 不重构已经合理的 DB 架构

- 覆盖文件：`shared/rust-core/src/db.rs`（2473 行，64.6% 测试）+ `db/*.rs`（10 模块，impl 分散 6 文件）。
- 相关 Task：仅允许"测试移出"类整理（如把 db.rs 测试迁往测试目录），**禁止重新设计 DB 层**。
- 验证方式：core 测试不动；移动测试必须保持 56 个测试全部可运行且语义不变。

## R012 — 引入 Typed Error

- 覆盖文件（String-error 面，BASELINE §6/§11）：`sync.rs`（106 to_string/22 签名）、`peer/delivery.rs`（17）、`iroh_transport.rs`（9）、`pairing.rs`（4）、`peer/directory.rs`（4）、db 层（Box<dyn Error> + 字符串 into）、平台 189 处。已有类型化先例：`ProtocolError`（protocol.rs:338）、`IdentityError`（identity.rs:13）、crypto.rs:396。
- 相关 Task：T350（错误清单，数据已收集）→ T351+（一次一个 domain，先 sync 或 pairing）。
- 验证方式：每 domain 迁移后其全部测试通过；测试断言从子串匹配改为错误类型匹配（K004 约束）。

## R013 — Error Boundary 明确

- 覆盖文件：`windows/src-tauri/src/commands.rs`/`api.rs`/`macos ApiClient.swift`（L730-744 子串匹配，K004）。
- 相关 Task：T351+（配套）→ 最终 Swift 端错误映射。
- 验证方式：跨层测试（Swift 测试 + api.rs 测试）；子串匹配移除后漂移检查不受影响。

## R014 — 测试优先保护高风险路径

- 覆盖文件：`protocol.rs`（10 测试）、`crypto.rs`（20）、`pairing.rs`（6）、`db/migrations.rs`（经 db.rs 37 测试）、`sync.rs` 续传 5 专项、`peer/delivery.rs`（26，含 in-memory Noise 投递）。
- 相关 Task：T321+/T351+/T421+ 对触及路径的测试加强（先测试后重构）。
- 验证方式：core 204 测试全绿 + 新增测试逐条列出。

## R015 — CI 不得降低

- 覆盖文件：`.github/workflows/ci.yml`（4 job）、`.github/workflows/release.yml`。
- 相关 Task：所有；重构不得删减/弱化门禁。
- 验证方式：本机按 ci.yml 同命令逐项跑（T002 建立 baseline）；CI 结果记录。

## R016 — Coverage 可观察

- 覆盖文件：无（当前无 coverage 报告）。
- 相关 Task：Phase 4 或适当时机引入 `cargo llvm-cov`/vitest coverage，**REPORT ONLY**，不加 gate。
- 验证方式：报告生成成功且不改变 CI 行为。

## R017 — 单一版本更新入口

- 覆盖文件：7 处版本声明（BASELINE §3 VERSION_MATRIX）。
- 相关 Task：T003（设计）→ 后续 Task（实现 `cargo xtask version X.Y.Z` 或等价脚本）。
- 验证方式：`validate-release-version.mjs` 继续通过（现有 release consistency checks 不得破坏）。

## R018 — 可观察性增强

- 覆盖文件：core 仅 15 处 log（8 debug/6 warn/1 error）、平台约 84 处、无结构化字段。
- 相关 Task：T400（日志清单，数据已收集）→ T401（诊断 schema 设计，先设计后实现）→ T402+（导出实现）。
- 验证方式：schema 审核通过后才实现；诊断内容不包含剪贴板原文/密钥（§18 禁止清单）。

## R019 — 网络测试分层

- 覆盖文件：`iroh_transport.rs`（7 测试，`repeated_rtt_probes` 已 ignore K005）、`interop_probe.rs`（307 行）、`test_cross_project_interop.ps1`（CI-only）。
- 相关 Task：T420（分类，数据已收集：deterministic=core 内 in-memory 测试；real transport=interop probe）→ T421+。
- 验证方式：分类表 + 逐步下沉确定性测试。

## R020 — Parser/Protocol Robustness

- 覆盖文件：`protocol.rs`（Frame/命令解析，10 测试）、`peer/delivery.rs`、`db/migrations.rs`、`sync.rs` 清单校验。
- 相关 Task：T440（候选审计，现状 0 fuzz target）。
- 验证方式：候选表按风险收益排序；不凑数。

## R021 — 不统一平台 UI

- 覆盖文件：`macos/swift-ui/`（SwiftUI 4835 行）与 `windows/src/`（React 1774+1374 行）——保持差异。
- 相关 Task：无（约束性需求，任何 Task 不得尝试统一 UI）。
- 验证方式：code review（无 SwiftUI↔React 合并动作）。

## R022 — 避免过度架构

- 覆盖文件：全部新增 abstraction。
- 相关 Task：所有 implementation Task。
- 验证方式：新增 abstraction 必须通过 §20 Maintainability Decision Test（Q1-Q5 至少一个具体答案）；新增文件必须通过 §21 File Split Decision Test；共享迁移必须通过 §22 Shared Code Decision Test。

---

## 汇总

| 需求 | 状态（T001 时点） | 主要证据文件 |
|---|---|---|
| R001-R022 | 全部 NOT_STARTED / PARTIAL（基线收集完成） | BASELINE_REPORT.md §1-§14 |
| 已完成的基线数据 | T000 | BASELINE_REPORT.md + PROJECT_STATE.md |

## Task 依赖锚点

- T002 依赖 T000（基线事实）+ 本文件（验证范围）。
- T003 依赖 BASELINE §3（VERSION_MATRIX 已收集）。
- T100 依赖 BASELINE §5（byte-identical 清单已实测）。
- T200/T220/T240/T300/T320/T350/T400 的只读数据均已收集于 BASELINE §7/§8/§6/§11 —— 正式执行时只需补细节与格式化为独立报告。
- 所有 implementation Task（T101+ 等）依赖对应只读 Task 的验收。
