# 修复与可维护性执行报告（阶段 0–3 已完成）

> 基线：`origin/main` = `b61a0a04b88f5828707f7ef64781389944471be8`（2026-09-29 核对）。
> 执行分支：`codex/remediation-2026-09-29`（worktree `TailSync-remediation`）。
> 本报告只记录**已完成并经验证**的部分，以及**明确未做**的部分；未取得设备证据的条目一律保持“待验”。

## 1. 环境与范围约束

- 执行环境仅 macOS。自动化检查由 GitHub 托管运行器承担：`rust-windows` → `windows-latest`、`rust-macos` → `macos-latest`、`frontend`/`scripts` → `ubuntu-latest`。因此 Windows 的构建、Clippy、单测、NSIS 打包与成品 smoke 均可在无本地 Windows 的情况下运行。
- **真实双机验收按指示暂缓**：LAN／Tailscale 配对、双方重启、断线续传、网络切换、跨版本互通，以及 Windows 原生断网/虚拟网卡/VPN 场景、macOS VoiceOver 实操，均未执行，条目状态保持“待验”。
- 旧而脏的主检出（`TailSync`，`5b0cdd6`）全程未被改动；其未提交内容已备份到 `/tmp/tailsync-preserve-<timestamp>/`（含双语 README、两份审计文档、`Package.swift` 与 `SettingsGeneralSection.swift` 及其 diff、`windows/redesign*`、`TailSyncPreview/`）。

## 2. 交付内容（按阶段）

### 阶段 0：PR #68 更新与验收清单 —— 裁决 PASS

| 项目 | 结果 |
|---|---|
| PR #68 更新 | 从 `b8910b3` rebase 到 `b61a0a0`，新 head `7822e01`，0 behind / 6 ahead；`gh pr view 68` = OPEN / MERGEABLE / CLEAN |
| 内容无损 | rebase 前后 PR 自身 diff 逐字节一致，38 个文件 blob 哈希全等，main 的并发改动全部保留 |
| CI | 该 PR 的 11 个 job 全绿（Windows 打包 16m1s、macOS 打包 18m14s、Required verification 通过） |
| 成品端口的事实验证 | `README.md` 现为“Windows 不启动 legacy JSON TCP API”，与 `windows/src-tauri/src/lib.rs` 的 cfg 门禁一致 |
| VoiceOver 清单 | `docs/acceptance/voiceover-connections-checklist.md`：9 处 `accessibilityLabel` + 1 处 `accessibilityValue`，中英对照的朗读名、值/状态与焦点顺序；**执行为待验** |

### 阶段 1：可机器校验的审计台账 —— 裁决 PASS

- `docs/remediation-ledger/`：`manifest.json`（固定初始基线 `b61a0a0`）、`schema.json`、`entries/<ID>.json` ×31（按 ID 分文件，便于并行 PR）、脚本生成的 `INDEX.md`。
- `scripts/check-remediation-ledger.mjs`：读 `schema.json` 做结构校验（必填/未知字段/枚举/长度）；校验 ID 唯一性与集合一致；**校验门禁测试符号真实存在于所引文件**；**校验 `gate.command` 的 `--manifest-path`/`--package-path` 确实编译/构建该文件**（会跟随 `include!`）；**校验每个 `ci_job` 真的运行覆盖该门禁的命令**；校验 `status_counts`/`level_counts` 与实际条目一致。
- 门禁支持两类：`kind: unit`（具名测试）与 `kind: integration`（构建/冒烟脚本中的断言标记）。
- 接入 CI：`.github/workflows/ci.yml` 的 `scripts` job 新增 `Validate remediation ledger` 步骤；`docs/**` 的改动由 scope planner 路由到该 job。
- 测试可观测性：`scripts/record-ci-timings.mjs` + `.github/workflows/ci-timings.yml`（每日/手动，只读 Actions API，产出 job/step 耗时与结论，上传 30 天产物）；`docs/critical-path-tests.md` 列出关键路径的具名测试与当前空白。
- `docs/performance-budgets.md`：第 7 阶段所需负载与阈值的**提案**，明确标注“待维护者签署，签署前不得据此关闭性能项”。
- 来源归档：`docs/audit/CODE-REVIEW-2026-09-27.md` 与 `docs/audit/CODE-REVIEW-REMEDIATION-PLAN-2026-09-29.md`，带历史横幅（注明正文 28 项 vs 矩阵 31 项、第 5 节失效断言）。

### 阶段 2：P0 防回归与两项定向门禁 —— 裁决 PASS

| 条目 | 交付 | 故障注入验证 |
|---|---|---|
| `S6-P0-1` | `windows/scripts/package-windows.ps1` 在启动成品前确认 19889 未被占用，启动后以进程所有权断言 TailSync 未监听 `127.0.0.1:19889`；`Get-NetTCPConnection` 缺失**或报错**时回退 `netstat`（失败关闭） | 临时分支移除三处 cfg → Windows 打包 job 在 `Build NSIS and run packaged executable/deep-link smoke tests` 报红，消息为 “Packaged TailSync is listening on the legacy local API port 127.0.0.1:19889…”，run `36603236594`。证据记录于 `docs/acceptance/s6-p0-1-mutation-proof.md` |
| `S6-P0-1`（脚本） | `windows/scripts/check_cross_platform_sync.mjs` 不再要求 `API_PORT == 19889`，改为断言两处 cfg 门禁存在；通过消息同步更新 | 移除 cfg → 退出 1 并给出对应消息 |
| `S2-F2` | `peer/delivery/tests.rs` 新增虚拟时钟测试 `delivery_failure_backoff_delays_the_next_reconnect` | 令退避立即返回 → 测试失败 |
| `S6-P2-3` | `ApiClientCancellationTests.swift` 新增 `testLocalCapabilitiesAcceptsMacOSPlatform` / `testLocalCapabilitiesRejectsNonMacOSPlatform` | 放行 `platform == "windows"` → 拒绝用测试失败 |

### 阶段 3：网络边界 —— 裁决 PASS

- `S1-P1-4`：`resolve_candidates` 改为**逐候选跳过**无作用域的链路本地 IPv6（不再因单个坏候选丢弃整个对端的可路由路由），并在仅剩不可路由候选时报错；`peer_socket_addr` 与两端 `network::test_connection` 也加了同守卫，全仓不再构造 scope-0 的链路本地 socket。门禁 `resolve_candidates_skips_link_local_ipv6_without_a_scope`（已做移除守卫的变异验证）。
- `S1-P1-5`：契约决策落定——**零设备是成功而非失败**（`merge_lan_discovery_results` 仅在全部传输失败时返回 Err），`lan_only` 下“无合格 LAN 接口”由空 `local_ip` 表达，`auto` 下该字段可能被 tailnet 地址填充（已用测试钉住该行为），UI 以带标签的接口判断；因此**不新增结果字段**。门禁为共享契约测试；Windows 侧结构性测试 `broadcast_targets_always_include_the_global_broadcast` 在 `rust-windows` 编译运行（本地以 CI 同款方式补 `windows/dist` 桩后已验证通过）；接口过滤逻辑本身无单元门禁，由 Windows 原生验收覆盖（待验）。

### 台账现状

`fixed_gated 21 / fixed_ungated 0 / partial 3 / unfixed 6 / needs_adjudication 1`（合计 31）。
自初始基线起，已把 `S2-F2`、`S6-P0-1`、`S6-P2-3`、`S1-P1-4`、`S1-P1-5` 五项从“缺门禁/部分处理”推进为“已修且受保护”。

## 3. 本轮采用的验证协议及其结果

每个阶段都在完成后经过**独立反驳**再**独立裁决**（均为独立子代理，从 `origin/main` 重新取证、可自行复跑命令）：

| 阶段 | 反驳发现的真实缺陷 | 处理 | 裁决 |
|---|---|---|---|
| 0 | 1（无障碍行数写成 11，实为 9 标签 + 1 值；另有未备份的脏文件） | 已修 + 补齐备份 | PASS |
| 1 | 6（`S4-P1-5` 门禁命令指向从不编译该文件的 crate；校验器经符号链接静默退出 0；校验器不读 schema、无法校验命令覆盖；台账来源文档未入库；预算文档读起来像已冻结；关键路径文档命令写成 `--lib`） | 全部修复；校验器新增 schema 校验、命令/作业覆盖校验、符号链接安全的入口；新增 10 个测试（共 20 个） | PASS |
| 2 | 裁决者另提 2 项 | 已修（P0 变异证据入库；`Get-NetTCPConnection` 报错时的空过路径改为失败关闭） | PASS |
| 3 | 6（含一个会阻塞 CI 的 rustfmt 违规；`test_connection`/`peer_socket_addr` 仍构造 scope-0；整对端 Err 会丢弃可路由候选；门禁测试同义反复；`auto` 模式表述不准；Windows 测试未本地验证） | 全部修复；裁决者另用 CI 同款桩在本地实际编译并运行了 Windows 测试（3 通过） | PASS |

这一协议本身是本轮最有价值的产出之一：四个阶段共被独立复核出 **15 个真实缺陷**，其中至少 2 个会直接让 CI 变红（rustfmt、门禁命令指向错误 crate），若只做自查都会漏过。

## 4. 明确未完成的部分

以下 9 个 ID 尚未推进（阶段 4–8），台账中已分别标注为 `partial` / `unfixed` / `needs_adjudication` 并带有通过条件与重新打开条件：

| 阶段 | 条目 | 现状 | 备注 |
|---|---|---|---|
| 4 | `S3-P1-2` 配对单边信任 | 未修 | 需可持久化 `pending`/`active` 状态机 + 迁移 + 旧↔新双向互通夹具；本轮未开始 |
| 4 | `S3-P1-1` 匿名可见字段 | 待裁定 | 需先产出匿名可见字段矩阵与最小匿名发起者实验 |
| 4 | `S3-P1-3` 未认证改写持久地址 | 部分 | 审计所述调用链未复现；先给复现条件，不预加状态 |
| 5 | `S4-P1-3` 配额扫描与锁占用 | 未修 | 淘汰循环仍每次递归扫盘并持 DB 锁 |
| 5 | `S5-P2-1` 孤儿文件对账 GC | 部分 | 删除容错已修；缺按 DB 引用对账 + 宽限期的 GC |
| 5 | `S5-P2-2` WAL 忙时降级 | 部分 | `execute_batch` 丢弃 `(busy, log, checkpointed)` 结果行，需改 `prepare` + 取行；**修复方案已确定，未实施** |
| 6 | `S6-P2-1` 通知 gap/实例标识 | 未修 | 需在版本化响应中新增服务实例标识与最早可用游标 |
| 6 | `S6-P2-4` Windows 后台偷吃警告 | 未修 | 需带 ID 的可确认读取 |
| 7 | `S2-F5`、`S4-P1-4` | 未修 | 硬门槛：`docs/performance-budgets.md` 未经维护者签署前不得以“显著/达标”作结论 |
| 8 | UI 与 runtime/IPC 可维护性、`tray-helper` 清理、`migrate_v1.py` 支持政策 | 未开始 | 见台账与计划中的条件 |

## 5. 阻塞项与继续方式

1. **分支无法推送（需你处理）**：当前 OAuth 令牌缺少 `workflow` scope，而本分支修改了 `.github/workflows/ci.yml` 并新增 `ci-timings.yml`，push 被 GitHub 拒绝（`refusing to allow an OAuth App to create or update workflow … without workflow scope`）。解决方式二选一：
   - `gh auth refresh -s workflow` 后由我推送；或
   - 你本地 `git -C /Users/monet/TailSync/TailSync-remediation push -u origin codex/remediation-2026-09-29`。
   工作已全部提交在该 worktree 的分支上，不会丢失。
2. **设备验收（暂缓）**：`docs/performance-budgets.md` 需要你作为维护者兼发布负责人签署；Windows 原生断网/虚拟网卡/VPN 场景、macOS VoiceOver 实操、以及最终双机验收均待环境就绪。
3. **阶段 7 硬门槛**：未签署的性能预算意味着 `S2-F5` 与 `S4-P1-4` 不能关闭。

## 6. 复现与验收命令

```bash
cd /Users/monet/TailSync/TailSync-remediation
node scripts/check-remediation-ledger.mjs --root .          # 台账校验
node --test scripts/check-remediation-ledger.test.mjs        # 校验器测试（20）
cargo test --locked --manifest-path shared/rust-core/Cargo.toml
swift test --package-path macos/swift-ui
node --test scripts/*.test.mjs
cargo fmt --all -- --check
node windows/scripts/check_cross_platform_sync.mjs --win-root ./windows --mac-root ./macos --core-root ./shared/rust-core
```

上一次全量执行结果：Rust 445 通过 / Swift 208 通过（3 跳过）/ 脚本 91 通过 / 台账校验通过 / 格式检查通过。
