# TailSync 审计整改最终报告

> 基线：`origin/main` = `b61a0a04b88f5828707f7ef64781389944471be8`（PR #72 合并点）。
> 工作分支：`codex/remediation-2026-09-29`（52 个提交；本报告随 `b632b7c` 之后的文档提交一起入库）。
> 日期：2026-09-30。环境：**单机 macOS（Apple Silicon）**；Windows 与 Linux 仅通过 GitHub 托管 runner 验证。
> 审计输入：`docs/audit/CODE-REVIEW-2026-09-27.md`；执行方案：`docs/audit/CODE-REVIEW-REMEDIATION-PLAN-2026-09-29.md`。

---

## 1. 结论摘要

31 项审计发现的当前状态（由 `docs/remediation-ledger/` 的 31 个条目汇总，`scripts/check-remediation-ledger.mjs` 校验）：

| 状态 | 数量 | 含义 |
|---|---|---|
| `fixed_gated` | **28** | 已修复，且有具名门禁测试由门禁执行核对脚本（`run-remediation-tests.mjs`）在**实际测试执行结果**中确认以通过状态出现；CI 侧的核对结果见第 5 节 |
| `accepted` | **1** | 经决策接受、不加改动并记录（`S3-P1-1` 匿名可见字段） |
| `partial` | **1** | 部分处理（`S1-P1-5`，Windows 侧已处理，macOS 侧无对应门禁） |
| `unfixed` | **1** | 未修（`S4-P1-4`，需真实传输的设备级测量后才裁定） |

**可以在单机上完成的部分已全部完成**：28 项修复各自有具名门禁，且每一项都做过变异验证（把修复改回坏的行为，确认门禁变红，再恢复）。**不能单机完成的都已明确记录为待验**，没有把"未验"写成"已通过"：真实双设备配对与通知走查、`S4-P1-4` 的设备级锁等待测量、Windows 侧 UI 走查。

本报告的第 6 节列出全部未验项；第 7 节列出在执行过程中被对抗性复核推翻并纠正的结论——这部分是本轮工作最重要的产出之一。

---

## 2. 执行方式（用户设定的流程）

每个阶段按同一循环推进：

1. 把阶段拆成小任务，逐个实现并本地验证（fmt / clippy / 完整测试 / 台账校验）。
2. 开一个**反驳子代理**，任务是**证伪**本阶段的完成声明，而不是确认它。
3. 开一个**裁决子代理**，独立复核双方材料，逐项判定 PASS / FAIL。
4. 只有 PASS 才进入下一阶段；FAIL 的部分先修，再重跑该循环。

这套流程在本次执行中**多次推翻了我自己的结论**（见第 7 节）。如果没有它，至少 5 处错误会以"已完成"的形式留在仓库里。

---

## 3. 各阶段成果

### 阶段 0–3（台账、文档、P0 与低成本定向门禁）

- 建立 `docs/remediation-ledger/`：`manifest.json`（固定初始审计 SHA `b61a0a0`）、`schema.json`、31 个按 ID 分文件的条目、由脚本生成的 `INDEX.md`。
- 建立门禁基础设施：`scripts/check-remediation-ledger.mjs`（Schema、ID 唯一性、状态与 `ci_job` 一致性、具名门禁确实存在且非 `#[ignore]`、`ci_job` 的命令确实覆盖门禁文件——**包含解析 Rust `include!`**）、`scripts/run-remediation-tests.mjs`（运行真实测试命令并核对具名门禁以**通过状态出现在实际执行结果**中，并绑定源码指纹与提交）、`scripts/remediation-test-results.mjs`。
- 4 个 P0 全部 `fixed_gated`，其中 `S6-P0-1`（Windows 成品不得启动旧本地 API 端口）用**真实 CI 变异运行**证明：临时破坏保护后完整 CI 变红，报出 `Packaged TailSync is listening on the legacy local API port 127.0.0.1:19889`。
- 一次性修正了 3 个把推送后 CI 变红的缺陷：源码指纹包含未跟踪文件、不稳定墙钟断言、CRLF workflow 解析。

### 阶段 4（配对单边信任 `S3-P1-2`，本轮最大的实现项）

审计问题：配对在**双方确认后立即**写入持久信任，然后才等待对端确认；链路断开或超时会在**单边**留下持久信任记录，且没有任何记录表明它是半成品，因此无法对账。

实现（`shared/rust-core/src/pairing/pending.rs` 等）：

- 新增**非权威**的 pending 注记，存放于 sidecar 文件 `pairing-pending.json`（与 `config-v2.json` 同目录）；
- 双方确认后只写注记并发 `PairingPersisted`；**只有**在同一已认证会话上收到对端的 `PairingPersisted` 之后才 `promote_pairing` 写入 `trusted_peer_keys` 并清除注记；
- 超时**不删除**注记（对端可能已落盘而确认丢失）；重启后注记仍在，设备仍显示未配对。

不变量（可证明的部分）与明确残留见 `docs/remediation/PHASE-4-REPORT.md` §2。14 个本地测试覆盖每个边界——对端确认前断开、注记落盘后断开、超时、取消先于 ack、取消插入"检查与写信任之间"的窗口、重复确认、完成后清除、重启、两向新旧兼容、冻结的旧版配置、以及旧端只在本端注记落盘后才收到持久化帧。变异验证：改回修复前行为 14 中 6 个失败；去掉注记写入 14 中 7 个失败；把锁序改回"检查后释放" 1 个失败。

### 阶段 5（存储与 SQLite 生命周期）

- `S4-P1-3`：配额淘汰原先每删一组就递归扫描一次物理存储、且只抵扣 payload 字节（导致过度驱逐）；改为**一次**物理基线测量 + 按 `freed + db/wal/shm 净变化` 抵扣，并保留 `file_batch_admission_lock()` 的既有不变量（并发双批次不得重复认领空间）。
- `S5-P2-1`：外部孤儿文件清理（确认无数据库引用、非进行中传输、过宽限期才删），并接入常驻维护任务。
- `S5-P2-2`：`PRAGMA wal_checkpoint(TRUNCATE)` 的结果行不再被丢弃——忙时留下可验证的警告且删除仍然成功（`prepare` + `query_row` 读取 `busy` 行）。
- `S4-P1-1`、`S4-P1-2`、`S5-P1-1`、`S5-P1-2`、`S4-P2-1`、`S4-P2-2` 各自有具名门禁。

### 阶段 6（通知交付语义）

- `S6-P2-1`（macOS）：运行时通知缓冲区暴露服务实例标识、最早可用游标与丢弃总数，慢客户端/重连客户端可判断何时需要完整快照。
- `S6-P2-4`（Windows）：`sync_warning` 改为带 ID 的可确认读取（`peek` + `ack(id)`），且**只有可见窗口才确认**——单独把 `take()` 改成 `peek()` 会造成重复提示，这一点由门禁覆盖。
- `S6-P2-3`、`S6-P2-6` 各有具名门禁。

### 阶段 7（性能裁定）

- `docs/performance-budgets.md` 由维护者签署：文本 p99 ≤ 100 ms、队列水位 ≤ 48、永久丢 0。
- 单机基线与复现：判定依据是**签署阈值**（所有观测会话均满足，p99 距上限 1–2 个数量级），观测值本身离散且随机器负载移动，文档明确禁止把观测值当界限。
- `S2-F5` 因此**关闭并留下回归门禁**（不实现独立认证文件通道，符合方案"达标即如此处置"）：CI 门禁覆盖"满队不静默丢弃"与"生产通道几何被静默收缩"（实测生产构造出的 priority 容量，收缩常量或调用点都会变红）；`#[ignore]` 的基线在打印后自行断言签署阈值。
- `S4-P1-4`（引擎锁内的平台进度回调）**仍未修**：它需要在真实传输中测量平台回调耗时才能裁定"是否超预算"，属设备级测量，本环境无法完成。

### 阶段 8（UI / runtime-IPC 深化）

- 已删除孤儿组件 `macos/src-tauri/tray-helper/` 及其 `.gitignore` 条目。
- 已废弃手工 v1 恢复入口 `migrate_v1.py`（自动导入已存在）。
- 其余 UI/IPC 深化（含 pending 配对的界面出口）未做，见第 6 节。

---

## 4. 验证证据（分层）

| 层次 | 内容 | 本轮结果 |
|---|---|---|
| 具名门禁 | 28 项 `fixed_gated` 各自的门禁在**真实测试执行结果**中以通过状态出现 | 本地 `run-remediation-tests.mjs` 逐 job 核对通过（rust-macos 全量、rust-windows 由 CI 承担） |
| 变异验证 | 每项修复都做过"改回坏行为 → 门禁变红 → 恢复" | 全部通过；关键结果已写入各条目 `gate.asserts` |
| 本地全量 | `cargo fmt` / `cargo clippy -D warnings` / 完整测试（rust-core、tailsync-runtime、tailsync-protocol、macOS crate）/ Windows crate `cargo check --all-targets` | 全绿 |
| 台账 | `check-remediation-ledger.mjs`（Schema、ID、状态、门禁、`ci_job` 覆盖） | 31 条通过 |
| CI（托管 runner） | `rust-macos` / `rust-windows` / `frontend` / `scripts` 的测试与打包 | 见第 5 节 |
| 真实设备 | 双设备配对、通知、UI 走查 | **未做**（环境限制，见第 6 节） |

---

## 5. CI 状态

本轮推送后由维护者手动触发（工作流为 `workflow_dispatch`，不在 push 时自动运行）。

- 上一个完整矩阵通过的是 `ec3e8b4`（run `36694626150`）——**它早于 `S3-P1-2` 的实现**，因此不能用来证明配对改动。
- `74ee37a`（`S3-P1-2` 第一版）在 `Run application runtime Clippy` 步骤于两个平台 job 上**失败**（`tailsync-runtime` 视角的 dead code），导致其后所有门禁步骤被跳过。该缺陷已修复。
- 本分支历史上共 3 类导致 CI 变红的自身缺陷，均已定位并修复：源码指纹包含未跟踪文件、CRLF workflow 解析、上述 dead code。
- `b632b7c` 的矩阵（run `36707387508`）**全绿**，全部 11 个 job 成功：`Plan CI scope`、`Frontend (windows)`、`Frontend (site)`、`Release and recovery scripts`、`Shared production dependency resolution`、`Packaged application (Windows)`、`Packaged application (macOS)`、三个 `RustSec advisories`、`Required verification`。其中两个打包 job 各含 `Verify remediation gate execution`，即 `S3-P1-2` 及其余具名门禁在 **macOS 与 Windows 两个平台**上都以通过状态出现在实际执行结果中。此前失败的那一步（`Run application runtime Clippy`）在两个 job 上均已通过。
- 该矩阵覆盖包含全部代码改动的提交；其后只有文档改动（台账与报告），并由 `scripts` 侧的门禁校验脚本在本地复核（`check-remediation-ledger.mjs` 通过）。

**必须区分**：CI 通过不等于本报告第 6 节的"未验"项已完成；反之，Windows job 尚在运行时，也不能用 macOS job 的成功代替它。

---

## 6. 未完成与未验证（必须与上面所有结论一起读）

1. **真实双设备验收未做**。本轮只有一台 macOS 可用。以下只能由两台真机确认：
   - 配对在真实网络中的中断/重启/重复确认（本地已用确定性测试覆盖同等状态机路径，但不是真实链路）；
   - `S6-P2-1` 的慢客户端/重连/daemon 重启行为；
   - 两端 UI 在真实状态下的一致性（尤其 pending 配对不显示为已配对）。
2. **`S4-P1-4` 未修**：需要在真实传输中测量平台进度回调的锁等待，数值目前是暂定值（p99 ≤ 20 ms 待确认）。
3. **`S1-P1-5` 仍是 `partial`**：Windows 侧已处理，macOS 侧无对应门禁；这是台账如实记录的状态，不是遗漏的"已修"。
4. **Windows 侧的实机行为未验**：Windows crate 在本机只能 `cargo check`，实际执行由 CI 的 `rust-windows` job 承担；Windows UI 走查未做。
5. **阶段 8 的 UI/IPC 深化未做**：pending 配对目前只有 Rust API 出口，没有界面提示；新增界面字段要走 `contracts.rs` + 生成器流程（先例见 `S6-P2-1`：新增字段要求 app 与 daemon 同时升级）。
6. **`S3-P1-2` 的残留**：一端 active、另一端 pending 的**瞬时不一致**无法用有限次消息消除（"两个将军"问题）。这不是被审计的危害（其中不含未经双方确认的信任），且可恢复（pending 侧保留注记、active 侧接受重新配对），但它**不是**"已解决"，台账与阶段报告都按此表述。

---

## 7. 被对抗性复核推翻并纠正的结论（重要）

这套流程的价值集中体现在这里。以下每一条都是我先写成"已完成/已证明"，随后被独立复核证伪并纠正的：

1. **配置事实来自过期工作树**。我从一个 HEAD 落后于 `origin/main` 的检出得出结论，因此关于 README 与 Windows 启动路径的判断是错的。**教训：一律以 `origin/main` 为测量基准。**
2. **声称"全量本地校验通过"但从未运行 Clippy**。而 `cargo clippy --all-targets -- -D warnings` 是必需的 CI 步骤，实际是红的。已补齐并在后续每轮执行。
3. **`cargo fmt --all` 的假保证**。根 workspace `exclude` 了两个平台 crate，所以"格式化通过"根本没覆盖平台代码，CI 的按 crate 检查失败。现在逐 crate 运行。
4. **变异链式执行造成假结论**。我连续叠加变异后得出"加倍抵扣失败"的结论，实际是叠加运行的假象。现在每次变异单独运行并验证恢复。
5. **`S3-P1-3` 的门禁不覆盖被审计的向量**（第一次复核的 P1）。原门禁只证明"未信任的 hostname 不能写路由"，而审计向量里的 hostname **是**受信任的，pin-key 守卫拦不住它；真正的防线是发现适配器的 `remember` 空操作，**当时完全没有测试**。已补共享门禁（两个平台 crate 用 `include!` 引入同一文件）并变异验证。
6. **`S2-F5` 的门禁是常量同义反复**（同一份复核的 P2）。它比较两个编译期常量，而**生产**通道被改小（常量不变、只改调用点）时门禁仍然全绿——复核者用真实的调用点收缩演示了这一点。已改为用生产构造器 `sender_for_candidates` **实测**容量。
7. **性能文档两次把观测值写成界限**。第一次写 p50 上界 59 µs，下一次运行得到 90 µs；改成 p99/max/深度后，再下一次运行又得到 1.24 ms / 65 ms / 36。现在文档只发布"观测到的会话"，明确说明既不穷尽也不是门槛。
8. **`S3-P1-2` 的核心不变量被证伪**（第二次复核的关键结论）。我最初声称"任何失败路径都不会留下单边 active 信任"。复核者指出：持久化帧是在**非权威注记**之后发出的，因此收到它并 promote 的一方无法证明对方也 active——单边 active 是可达的。这是"两个将军"问题的实例，**该不变量无法成立**。已把主张收窄为可证明的部分，并把残留明确写成已知限制。
9. **`promote_pairing` 的锁释放窗口**（同一次复核）。检查会话有效性与写 trust 之间释放了状态锁，`cancel()`/`expire()` 可在此完成——结果是"界面显示已取消、设备却被持久信任"。已改为全程持锁，并加了能真正抓住它的确定性门禁（此前我写的取消测试即使把锁序改回去也能通过，即它并没有覆盖这个缺陷——这一点也是复核发现的）。
10. **CI 红灯来自本轮改动**（第三次复核）。`with_pending_store_at` 在"依赖启用 `test-support`"的构建视角下是 dead code，导致 `tailsync-runtime` 的 Clippy 失败、其后所有门禁步骤被跳过。**只跑 `rust-core` 自己的 Clippy 发现不了。**
11. **`forget_peer` 只修了一半**（同一次复核）。macOS 的 SwiftUI 走 Unix socket 路由而不是 Tauri 命令，注记在那条路径上仍然残留。
12. **测试与变异计数不实**（同一次复核）。报告里的变异失败数是从上一版测试集照抄的，实际是 6/14 与 7/14 而不是 5/12 与 6/12；台账 `gate.asserts` 还停在 9 个测试的描述上。

---

## 8. 产物清单

**台账与门禁**：`docs/remediation-ledger/{manifest,schema}.json`、`entries/*.json`（31）、`INDEX.md`；`scripts/{check-remediation-ledger,run-remediation-tests,remediation-test-results,record-ci-timings}.mjs`；`.github/workflows/{ci,ci-timings,coverage}.yml`。

**阶段报告**：`docs/remediation/PHASE-0-3-REPORT.md`、`docs/remediation/PHASE-4-REPORT.md`、本文件。

**裁定与证据文档**：`docs/performance-budgets.md`（已签署）、`docs/critical-path-tests.md`、`docs/remediation/wal-checkpoint-semantics.md`、`docs/security/anonymous-visible-fields.md`、`docs/acceptance/{acceptance-environment,s6-p0-1-mutation-proof,voiceover-connections-checklist}.md`、`docs/audit/{CODE-REVIEW-2026-09-27,CODE-REVIEW-REMEDIATION-PLAN-2026-09-29}.md`。

**实现要点**：`shared/rust-core/src/pairing/pending.rs`（新增）、`shared/platform-peer-cache-tests.rs`（新增，两个平台 `include!` 的共享门禁）、`shared/rust-core/tests/fixtures/legacy-config-v2.json`（冻结旧版配置）、`shared/rust-core/src/peer/pool.rs`（发送超时而非静默丢弃）、`shared/rust-core/src/db/storage.rs`（配额记账）、`shared/rust-core/src/db/migrations.rs`（WAL checkpoint 忙语义）、`shared/rust-core/src/sync_warning.rs`（可确认读取）、`macos/src-tauri/src/api.rs`（通知缓冲游标与服务实例）。

---

## 9. 建议的后续顺序

1. 在**两台真机**上跑一遍：配对（含中断/重启/重复确认）、通知慢客户端与 daemon 重启、两端 UI 对 pending 的显示。
2. 做 `S4-P1-4` 的设备级测量，用实测值确认或调整 `docs/performance-budgets.md` 中暂定的 20 ms。
3. 阶段 8：把 `pending_trust()` 接到两端 UI（走 `contracts.rs` + 生成器流程），并继续 UI/runtime-IPC 深化。
4. 处理 `S1-P1-5` 的 macOS 侧（或把该条目明确改为"仅 Windows 适用"）。
