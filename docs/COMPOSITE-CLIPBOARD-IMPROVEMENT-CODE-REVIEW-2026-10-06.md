# 复合剪贴板改进：已完成阶段的实现复核与验收

日期：2026-10-06，Asia/Shanghai。
性质：**只读复核**。本轮不修改产品代码、不新增/删除测试、不重构；只新增本复核文档。
复核对象：已完成的 S0、O11-D1、O11-D2、O01、O02、O06。

## 1. 结论摘要

| 维度 | 结论 | 说明 |
|---|---|---|
| 实现是否优雅 | **总体良好，局部偏重** | 分层、不变量、错误语义清晰；存在可合并的抽象层与重复判定 |
| 测试是否冗余 | **有冗余，但无虚假覆盖** | 2 处可直接参数化；3 处属分层互补、可接受 |
| 无用代码 | **存在确证死代码** | 4 项完全无调用；1 条 ClipboardPort 方法梯已失效；1 行空绑定 |
| 修改效果 | **达成阶段目标** | 最终矩阵 26 命令 25 PASS/1 既有 FAIL；5 类复现缺陷中 4 类已实现修复 |
| 本轮改动 | **无代码改动** | 源码指纹与封存完全一致（见第 2 节） |
| 发布资格 | **仍 NO-GO** | 真实应用、Windows 原生、双设备等仍 UNKNOWN |

一句话：**这是一套证据纪律良好、边界划分克制的实现；主要问题是 O06 之后留下的失效抽象层与“写入但从不读取”的计划元数据，其次是几处可合并的测试和一次可避免的重复解析。**

## 2. 复核基线与方法

- 工作树：`/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35`
- HEAD：`f719d581a1aa5198a1b3f2483222df5c69e3dfc0`（未变）
- 源码指纹：`303fc44c9bb0fd6dc18865a59a778055b81e609124798ae14587c922667e4338`，**770 文件**
- 复核前后该指纹逐字节一致；O01（1083 文件）、O02（1397 文件）、O06（2874 文件）三份 `evidence-sha256.json` 全部重算通过。
- 方法：静态阅读生产接缝 + 调用点穷举（`rg` 全仓引用计数）+ 封存证据复算。所有“无调用”结论均以全仓（排除 `target/`、`docs/`、`artifacts/`）引用搜索为准，并区分“完全无调用”与“仅测试调用”。

累计改动量（HEAD → 工作树，排除 `artifacts/` 与 Markdown）：

| 范围 | 规模 |
|---|---|
| 已跟踪文件修改 | 109 文件，+10965 / −786 |
| 新增源码/脚本文件 | 79 文件，约 19466 行 |
| 其中 O06 增量 | 26 改 + 3 新，+2056 / −98 |

## 3. 分阶段实现复核

### 3.1 O11-D1：内容身份与版本兼容设计

**做得好。** `shared/rust-core/src/composite/identity.rs` 把“事件身份、原始内容摘要、归档版本摘要”拆成独立域：`legacy_native_id`、`archive_content_id`、`archive_dedupe_key`、`portable_content_id`、`portable_dedupe_key` 各自使用独立的 `Hasher::new_derive_key` 域前缀（`history.original.v1`、`history.portable.v1`、`history.portable-dedupe.v1`），并有 `same_capture` / `may_rebind` 明确表达“同一采集”与“允许重绑”的差别。这直接对应 S0 复现出的“原件不同却被去重”缺陷。

**观察。** 域分隔符以字符串常量散落在各函数内，新增域时没有集中登记点；风险不高（每处都是 `new_derive_key`，改动会改变全部摘要），但值得在后续把域名集中为一个常量表，便于审计“哪些域已冻结”。

### 3.2 O11-D2：存储、去重与回退接线

**做得好。** `shared/rust-core/src/db/rollback.rs` 把 v13 快照、清单校验、`verify` / `verify_v13`、`retire`、`restore_into` / `restore_v13_into` 做成显式流程，并有独立的 `scripts/run-composite-v13-rollback-proof.py` 用真实旧 reader 反向验证。O06 又用最终源码重跑了一次（`rollback-frozen/proof.json`），并逐字节核对了全部 shared Rust/TOML 输入。这是本仓库最扎实的一处“回退不是口头承诺”。

**观察。** `rollback.rs` 中 `MANIFEST_NAME`、`MANIFEST_VERSION`、`ROLLBACK_DIR_NAME`、`SNAPSHOT_DB_NAME` 等常量只在文件内使用，属正常封装，非死代码。

### 3.3 O01：原件与派生表示隔离

**做得好。** `NativeArchive` 把 `original` 与 `derived` 分开，`enumeration_complete` 显式记录枚举完整性；`originals_complete()` 作为默认恢复的准入条件。生产接缝上，默认恢复只回放原集合，显式格式另走独立投影（`history.rs:307` 起的 `prepare_composite_for_target`）。S0 中“RTFD 保留但 HTML/文字被转换结果替换”的缺陷被正确修在“存什么”而不是“显示什么”。

**观察。** `CaptureProvenance::from_snapshot`（`shared/rust-core/src/composite/provenance.rs:51`）与 `standalone_image_is_unbound`（同文件 :127）**只在自身模块的测试里被调用**。二者描述的是“首图/绑定关系”语义（F06 的前置数据），目前没有生产消费者；属“为后续任务预留”的测试专用 API，而非死代码，但应在 F06 落地前保持标注，避免被误认为已生效能力。

### 3.4 O02：原生字节准入与敏感采集阻止

**做得好，这是本批改动里最优雅的一处。**

- 策略单一来源：`shared/schema/native-capture-policy.json` → `scripts/generate-native-capture-policy.mjs` 同时生成 `shared/tailsync-protocol/src/native_policy.generated.rs` 与 `macos/src-tauri/clipboard-helper.swift` 内的 `BEGIN/END GENERATED NATIVE POLICY` 区块，并有 `--check` 门禁进入 CI。跨语言常量不再靠人工同步。
- 隐私边界前置：四种精确标记在读取任何正文之前用元数据判定，命中即返回 `Suppressed`（`state=6`、空格式、revision 稳定），既不建历史也不做 legacy 回退读取。这是“先判后读”，不是“读完再丢”。
- 不透明字节有明确上限与失败事实：未知合法 UTF-8 名称按 Data 捕获，超限/未读取/枚举不完整都以显式状态保留，而不是伪装成功。
- 版本克制：CNA1 仍为 1，仅新增 metadata-only 状态；Mac helper 3→4，由启动前 resolver 拒绝旧 helper。

**观察 1（确证死代码）。** `shared/rust-core/src/composite/supervisor.rs:102` 的 `pub enum HelperPath { Found(PathBuf), Missing }` 在全仓**没有任何引用**。当前 helper 解析实际由 `shared/platform-clipboard-composite.rs` 的 `resolve_helper() -> Result<PathBuf, HelperHealth>` 承担。这是 O02 期间被替换后遗留的类型。

**观察 2（确证死代码）。** `shared/rust-core/src/composite/outcome.rs:30` 的 `NativePublish::is_proven_no_effect` 无调用点。注意不要与**同名近似、确实在用**的 `tailsync_protocol::failure_is_proven_no_effect`（`shared/tailsync-protocol/src/lib.rs:426`，被 `sync/clipboard.rs:376`、`:438`、`peer/event_receiver.rs:326` 使用）混淆——后者才是生产路径。

**观察 3。** `origin.rs:205` 的 `OriginTracker::unresolved_intent_age` 无调用点（文档说明它“供 UI/诊断解释降级状态”，但当前没有任何 UI/诊断调用）。属“已设计未接线”。

### 3.5 O06：准备恢复计划，再集中发布

**做得好。** 这是本轮最核心的新增，优点集中在四点：

1. **不可变、owned、版本化的计划。** `shared/rust-core/src/composite/restore.rs` 的 `RestorePlan` 字段与字节在准备成功后不可变，同一个值跨 FIFO/worker 边界，`RESTORE_PLAN_VERSION = 1` 显式声明。这正面回答了“准备与发布之间不能被重选格式”的要求。
2. **准备与发布彻底分离。** 结构门（`validate`）、可发布门（`validate_for_publish`）、语义依赖门（`dependencies_complete`）三层分开，缺依赖时返回稳定码 `resources_incomplete`，且**发布前**就拒绝，不会先清空剪贴板。
3. **取消与 FIFO 语义严谨。** `sync/port.rs` 的 `CancelOnDrop` + 阻塞池 + 内存/FIFO 许可一直持有到 worker 静止；`o06_cancelled_plan_keeps_owned_output_fifo_and_memory_until_worker_stops` 与既有的 composite 版测试共同证明“取消 async waiter 不会提前释放许可”。
4. **不新增持久版本。** RestorePlan 是纯内存概念，history 13 / Manifest 1-2 / CNA1 1 / local contract 2 / error schema 1 / helper 4|2 全部保持不变，新增的只是生成契约里的一个稳定原因码。这种“能不升版本就不升”的克制值得保留。

**观察 4（本项最值得处理的问题：计划元数据写入后从不读取）。** `RestoreInput`（`restore.rs:33-40`）有五个字段，实际语义如下：

| 字段 | 写入点 | 读取点 | 结论 |
|---|---|---|---|
| `identity` | `history.rs:267`、receiver `:212` | 仅 `restore_plan_probe.rs:182` 断言 | **生产从不读取** |
| `source_archive_version` | `history.rs:268` | 仅测试断言 | **生产从不读取** |
| `source_platform` | `history.rs:269` | `restore.rs:120` 的跨平台拒绝 | 有效 |
| `payload_digest` | `history.rs:270`、receiver `:215` | **无** | **写入后从不比较** |
| `requested_format` | `history.rs:271` | `restore.rs:138` 仅用 `.is_some()` 判定选择类型 | 值本身未使用 |

也就是说，ADR 第 11 节把它描述为“输入绑定/完整性绑定”，但**没有任何代码在发布时校验这个摘要**；`payload_digest` 只在准备阶段被计算（并且是整段载荷 + `native.encode()` 的完整 BLAKE3，见观察 9）。这是“文档承诺强于实现”的典型例子，应视为设计缺口而不是实现错误：要么在发布前真正比对，要么把字段降级为诊断用途并在 ADR 中改写措辞。

**观察 5（O06 后失效的抽象层：确证无用代码）。** 第 6 节详列。要点是：`ClipboardPort` 上原有的 composite 发布方法梯（`publish_composite` → `publish_composite_cancellable` → `publish_composite_admitted` → `publish_composite_shared_admitted`）以及 `publish_native_admitted`，在 O06 把接收端与用户恢复端都改为 `publish_restore_plan_admitted` 之后，**在生产路径上已无调用者**。

**观察 6（重复判定，漂移风险）。** `shared/rust-core/src/composite/html.rs` 中“哪些属性构成外部依赖”的判定被写了两遍，内容一致：

- 采集侧 `extract_resources`（约 :288-294）：命中则标记 `incomplete`；
- 恢复侧 `restore_reference_inventory_complete`（约 :488-493）：命中则直接拒绝。

外加 `<img>` 无 `src` 的兜底判定也重复了一次（:296-299 与 :494-496）。两侧语义不同（标记 vs 拒绝）是正确的，但判定条件必须永远一致；目前靠人工同步。另有一处**不对称**值得记录：`restore_reference_inventory_complete` 对 `<script/<object/<iframe/<embed/<base` 与 `<style ... url(` 使用**原始子串**扫描，而 `extract_resources` 使用有界词法分析器（会跳过注释与 raw text）。结果是“注释里出现 `<script>`”这类样本会被采集判为可存、却被恢复判为不可恢复。方向是安全的（宁可多拒），但两套判定不一致，建议后续统一到词法分析器。

**观察 7（性能：列表路径新增逐行解密）。** `db/queries.rs:214`、`:324` 与 `db/reads.rs:415` 在**每次历史列表读取**时，对每条 composite 记录调用 `prepare_composite_menu`，后者会从 `history_parts` 取出并解密该行的 HTML 部件（上限 2 MiB）。也就是说，即使 UI 从不打开恢复菜单，翻一页历史也会解密这一页所有 composite 行的 HTML。对“历史列表”这个高频路径，这是 O06 引入的实质成本；`PreparedCompositeMenu` 的设计（owned、hash 校验、可取消）是好的，问题只在触发时机。

**观察 8（性能：能力查询最多 7 次解析）。** `history.rs:492 restore_capabilities` 依次：`present_formats(data)`（1 次 `PortableBundle::decode`）→ `let _bundle = PortableBundle::decode(data)`（第 2 次，且**绑定未使用**）→ 对每个存在格式各调用一次 `prepare_composite_for_target`（≤4 次，每次内部再 decode）→ 最后整组再来 1 次。合计对同一段最大 32 MiB 的载荷最多 7 次完整解析。建议改为“解析一次、把 bundle 传给各分支”。

**观察 9（性能：摘要计算无消费者）。** `history.rs:283 payload_digest()` 对整段便携载荷做 BLAKE3，并在存在原生集合时对 `native.encode()`（完整重新编码原生快照）再做一次。结合观察 4，这份成本目前没有任何校验用途。

**观察 10（文档与实现不一致）。** `history.rs:490` 的注释称 `restore_capabilities` 是“两个 UI 菜单的单一事实源”。但全仓搜索显示该方法**只有测试调用**；两个 UI 实际使用的是 DB 列表里的 `CompositeInfo.restorable_formats`（`macos/swift-ui/Sources/TailSync/Views/HistoryRow.swift:263`、`windows/src/pages/history/HistoryItem.tsx:177`），而它由 `db/composite.rs:168 info_bytes` + `PreparedCompositeMenu::apply` 另一套实现产生。于是同一语义存在**三个并行实现**：DB 清单投影、DB 内容修正、runtime 能力查询，另有真正生效的 `prepare_composite_for_target`。`rr30`/`rr24` 正是为守住这三者一致而存在——测试有价值，但根源是事实源不唯一。建议：要么把 `restore_capabilities` 真正接到生产菜单，要么修正注释并明确它是“可执行规格/测试预言机”。

**观察 11（轻微：伪造回执）。** `shared/rust-core/src/sync.rs:205-210` 的默认 `write_restore_plan` 在预期 revision 不符时返回 `Superseded`，但回执里填的是 `revision: 0, token: [0; 16]`——即合成一个不存在的 revision。生产 Mac/Windows adapter 都覆写了该方法、不会走到这里，所以影响有限；但“回执必须来自真实观测”是本项目自己定的原则（ADR-004 K2），建议这里改为携带 `clipboard_revision()` 的实测值，或在类型上区分“未观测”的 Superseded。

**观察 12（轻微：类型形状）。** `db/reads.rs:144` 的 `Candidate.menu` 是 `Result<Option<PreparedCompositeMenu>, String>`，消费处需要 `std::mem::replace(&mut candidate.menu, Ok(None))`（:193）才能移动出来。`Option<Result<…>>` 或“失败即已 fail-closed 的 `Option`”会更贴合语义。另外 `apply_prepared` 的 `Err(_)` 分支静默转成“隐藏 HTML”，行为正确（fail-closed），但没有留下任何诊断日志，后续排查“为什么菜单里没有 HTML”会比较困难。

## 4. 优雅性评估

**值得保留的设计**

- **分层清晰且命名诚实**：结构门 / 可发布门 / 语义依赖门 / 目标适配门四层，各自有独立错误与测试；`StoredIncomplete` 把“合法但暂不可发布”与“损坏包”区分开。
- **版本治理克制**：能不加版本就不加；每类版本（identity、data frame、history、effect journal、local contract、helper protocol、origin fingerprint、RestorePlan）在 ADR-005 中分别登记，不混称“协议版本”。
- **策略单一来源 + 生成门禁**：native policy 的 JSON→Rust/Swift 生成与 `--check` 是跨语言常量管理的正例。
- **失败语义不撒谎**：`NoEffect/Published/Partial/Unknown/Superseded` 五态贯穿到 UI 文案，Partial/Unknown 不自动重试、不显示成功。
- **证据纪律**：真实 RED 日志（exit 101/1）原样保留，first/second/final 三轮指纹与全量源码快照并存，未提高超时、未新增 skip/ignore（既有 ignore 13 → 13）、未削弱断言。

**偏重或可简化之处**

- `ClipboardPort` 的方法梯（观察 5）是典型的“兼容期叠加后未回收”。
- `RestorePlan` 暴露的 `version()/input()/selection()/outputs()` 与 `RestoreOutput` 结构在生产中无消费者，却要为每次准备分配 `String` 名并计算全量 BLAKE3 摘要（观察 4/9）。
- `restore_capabilities` 与 DB 菜单投影双实现（观察 10）。
- HTML 依赖判定双实现（观察 6）。

## 5. 测试冗余评估

**可以合并（建议后续处理，本轮不改）**

1. `shared/rust-core/src/sync/port.rs:455 cancelled_publisher_keeps_fifo_and_memory_until_worker_quiescence` 与 `:504 o06_cancelled_plan_keeps_owned_output_fifo_and_memory_until_worker_stops`：结构约 90% 相同（同一 `Controlled` 平台、同一 ledger/许可准备、同一 abort→断言序列），差别只在调用 `publish_composite_admitted` 还是 `publish_restore_plan_admitted`，外加一行 `flag` 断言。二者可合并为一个对“发布闭包”参数化的测试，或让 plan 版只保留其真正新增的断言（cancellation flag 与 owned plan 存活），避免同一不变量被两条长测试各写一遍。
2. 三份几乎相同的“双图”夹具：`shared/rust-core/src/composite/restore.rs:345 rich()`、`shared/tailsync-runtime/examples/restore_plan_probe.rs:79 rich()`、`shared/tailsync-runtime/tests/repair_regressions.rs:135 two_image_html_bundle()`。跨 crate 无法直接共享，但在 `tailsync-runtime` 内部（测试与 example 同 crate）可抽成一处 fixture，减少“改一处忘一处”的风险。

**分层互补、判定为可接受**

3. `rr30_the_listing_capability_matches_the_restore_capability`、`rr24_offered_restore_actions_are_exactly_the_accepted_ones`、`rr24b_portable_entry_offers_every_format_it_carries` 三者构成“DB 列表 ↔ runtime 能力 ↔ 实际接受”的传递链，夹具重复但覆盖不同环节，属必要的三方一致性守卫。
4. `o06_native_only_menu_does_not_offer_a_default_restore_with_known_missing_resources`（`tests/o06_restore_plan.rs:136`）与 `o06_missing_or_external_resources_never_report_whole_group_success`（同文件 :101）重叠度较高（后者已断言 `!whole_group` 且不提供 html），新增价值主要是 `native_only = true` 分支与 `whole_group_code`。属“薄但确实覆盖独立分支”。
5. `o06_original_constructor_refuses_known_external_dependencies`（core）与运行时的缺依赖测试属不同层的同一性质；`o06_invalid_image_or_unbound_resource_is_a_preparation_failure` 的“损坏 PNG”一项与既有 `validate_for_publish` 覆盖部分重叠。均可接受。

**结论**：测试**没有**出现“同一断言复制多份以凑数”的情况，也没有为通过而放宽的断言；冗余集中在“同一不变量被长测试重复表达”，属可维护性问题，不影响证据可信度。

## 6. 无用与死代码清单

以下均已用全仓引用搜索确认（排除 `target/`、`docs/`、`artifacts/`）。

**A. 完全无调用（确证死代码）**

| 位置 | 符号 | 说明 |
|---|---|---|
| `shared/rust-core/src/composite/supervisor.rs:102` | `enum HelperPath` | 已被 `platform-clipboard-composite.rs::resolve_helper()` 取代 |
| `shared/rust-core/src/composite/outcome.rs:30` | `NativePublish::is_proven_no_effect` | 无调用；注意与在用的 `protocol::failure_is_proven_no_effect` 同名近似 |
| `shared/rust-core/src/composite/origin.rs:205` | `OriginTracker::unresolved_intent_age` | 设计用于诊断，未接线 |
| `shared/rust-core/src/peer/effect_journal.rs:367` | `EffectJournalRecord::set_history_ref` | 无调用；`history_ref` 列在 claim 插入时已写，此“事后回填”路径未使用 |
| `shared/tailsync-runtime/src/history.rs:522` | `let _bundle = PortableBundle::decode(data)…` | 空绑定，仅重复了 `present_formats` 已做的校验 |

**B. O06 后失效的 ClipboardPort 方法梯（生产无调用，仅测试使用）**

| 位置 | 符号 |
|---|---|
| `shared/rust-core/src/sync/port.rs:98` / `:220` | `publish_composite`（必需方法，仍须实现） |
| `shared/rust-core/src/sync/port.rs:103` / `:234` | `publish_composite_cancellable` |
| `shared/rust-core/src/sync/port.rs:111` / `:242` | `publish_composite_admitted` |
| `shared/rust-core/src/sync/port.rs:124` / `:259` | `publish_composite_shared_admitted` |
| `shared/rust-core/src/sync/port.rs:88` / `:322` | `publish_native_admitted` |

接收端（`peer/composite_receiver.rs:348`）与用户恢复端（`tailsync-runtime/src/history.rs:245`）现在都走 `publish_restore_plan_admitted`。上表五项合计约 100 行 trait 默认实现与 impl；其中 `publish_composite_cancellable` / `publish_composite_admitted` 仅被 `port.rs:467` 一处测试间接触及，`publish_composite`、`publish_composite_shared_admitted`、`publish_native_admitted` 在全仓完全无调用。建议在后续清理时保留 `publish_composite`（必需）与 plan 方法，删除其余默认层。

**C. 写入但从不读取（惰性元数据，非语法死代码）**

- `shared/rust-core/src/composite/restore.rs:34/35/38`：`RestoreInput` 的 `identity`、`source_archive_version`、`payload_digest`。
- 连带成本：`shared/tailsync-runtime/src/history.rs:283` 的 `payload_digest()`（整段载荷 + `native.encode()` 的 BLAKE3）。

**D. 仅测试/示例使用的生产 API（不算死代码，但生产无消费者）**

- `shared/rust-core/src/composite/restore.rs:66/69/75/78`：`version()`、`input()`、`selection()`、`outputs()`；以及 `RestoreOutput`（:42）。
- `shared/tailsync-runtime/src/history.rs:492`：`restore_capabilities`（含 `RestoreCapabilities`）。
- `shared/tailsync-runtime/src/history.rs:130` `restore_composite`（同步版）+ `shared/rust-core/src/sync/clipboard.rs:150` `SyncEngine::restore_plan`：同步测试接缝；生产四个入口（macOS/Windows 的 command 与 api route）都走 `restore_composite_async`。

**E. 重复逻辑（非死代码，漂移风险）**

- `shared/rust-core/src/composite/html.rs`：外部依赖属性判定（约 :288-294 与 :488-493）与 `<img>` 无 src 兜底（:296-299 与 :494-496）各写两遍。
- 可恢复格式判定三处并行：`shared/rust-core/src/db/composite.rs:168 info_bytes`、同文件 `PreparedCompositeMenu::apply`、`history.rs:492 restore_capabilities`（真正生效的是 `prepare_composite_for_target`）。
- 示例探针管道重复：`shared/tailsync-runtime/examples/restore_plan_probe.rs:20-77` 与 `shared/rust-core/examples/opaque_archive_probe.rs:16-34` 的 helper 调用/读回封装同构。

## 7. 修改效果评估

### 7.1 对 S0 复现缺陷的覆盖

| S0 复现缺陷 | 对应实现 | 状态 |
|---|---|---|
| 采集把原 HTML/文字替换为转换结果 | O01 原件/派生分离 | 已实现并有 7 组私有 board 逐字节证明 |
| 原件不同但便携相同被误合并 | O11-D2 身份与去重域 | 已实现，三次重放保留两份 |
| 自定义原生格式入库被过滤 | O02 不透明字节准入 | 已实现，policy 1 + provider 证据 |
| 缺依赖仍报整组成功 / 旧恢复覆盖新恢复 | O06 RestorePlan + generation/revision | 已实现，真实 RED→GREEN |
| 原生附件显示 0 张图、缩略图选错图 | F04/F06（未实施） | **仍未实现** |

即：五类中四类已落地，**F06 首图顺序与 F04 富文本预览仍属后续 S2**，不应被本轮的“恢复计划”成果掩盖。

### 7.2 最终验证矩阵（O06 冻结源码）

26 条命令：**25 PASS / 1 实际 FAIL**。

| 检查 | 结果 |
|---|---|
| Protocol 全套 | 42 PASS |
| Core（composite + file sliding window + image compressed chunks，serial） | 642 PASS，4 既有 ignored |
| DB all-features | 143 PASS，2 既有 ignored |
| Runtime all-features + Core composite | 45 PASS（含 O06 7 项与既有 11 项 integration） |
| macOS Rust enabled | 118 PASS，3 既有 ignored |
| Windows Rust on macOS host | 127 PASS，2 既有 ignored（不认证 Windows 原生） |
| workspace / macOS Clippy `-D warnings` | 均 PASS，0 warning |
| MSVC helper portable-check | PASS（host check） |
| 生成契约 / cross-platform / canonical / native policy | PASS（含 23 + 3 项负向） |
| Swift 全套 | 265 PASS，4 既有 skipped |
| Windows UI / 前端 build | 275 PASS / PASS |
| production-features | **FAIL exit 1**，恰为既有两处 `repair-regressions = ["test-support"]`，未豁免未修复 |

回归保全：新增 17 个 Rust 测试（restore.rs 7、db/composite.rs 1、peer/composite_receiver.rs 1、sync/port.rs 1、runtime o06 7），无删除既有测试、无新增 ignore（13 → 13）。

### 7.3 真实私有 board 证明

使用全新 `TailSync-O06-final-native.app`（arm64，composite enabled，helper 协议 4）在合成私有 board 上完成：

- 双图已绑定资源：DB 保存/重开 → 计划准备 → 正常删除源历史并关闭 DB → production supervisor → helper 发布，一次整组提交，红/蓝图顺序与输出字节正确，回读得到 SelfWrite；
- RTFD / TIFF / 两 item opaque 三组原件：重开后逐字节一致，显式格式请求不改变之后的默认计划；
- 拒绝边界：缺绑定准备失败后私有 board 字节与 revision 不变；旧 expected revision 返回 Superseded 且在 clear 之前拒绝；
- 旧 helper 3 被新 daemon 拒绝（expected 4 / found 3），O02 helper 4 与新 daemon 健康兼容。

边界（不可外推）：QQ 与真实来源/目标应用、完整 App GUI 与 live adapter journal、Windows 原生/portable/installer、双设备 Mac↔Windows、hosted CI、物理断电/满盘、以及 O04 的公开引用归档与未来文件 materializer lease，全部仍 UNKNOWN。

## 8. 风险与建议（按优先级；本轮均不改动代码）

**P1（建议在下一项 O10 之前或同期处理）**

1. **澄清或落实 `RestoreInput` 的绑定语义**（观察 4）。要么在发布前真正比对 `payload_digest`（并同时保留 `identity`/`source_archive_version` 的用途），要么在 ADR-005 第 11 节把措辞从“完整性绑定”改为“诊断元数据”，并考虑移除无消费者的字段与摘要计算。当前状态下，文档承诺与实现不一致。
2. **清理 O06 后失效的 ClipboardPort 方法梯**（观察 5 / 6.B）。保留 `publish_composite` 与 plan 方法，删除 `publish_native_admitted`、`publish_composite_cancellable/admitted/shared_admitted` 的默认与实现，或明确标注为兼容保留。
3. **删除确证死代码**（6.A 五项），其中 `set_history_ref` 需确认“事后回填 history_ref”是否本应接线，避免把功能缺口当纯清理。

**P2（性能与一致性）**

4. **统一 HTML 依赖判定**（观察 6）：把外部引用属性谓词与 `<img>` 兜底抽成一处；长期建议恢复侧也使用词法分析器，消除“注释内 `<script>`”这类不对称。
5. **降低列表路径成本**（观察 7）：把 `prepare_composite_menu` 从列表水合路径改为“真正需要菜单/能力时才做”，或复用已读入的部件。
6. **消除重复解析**（观察 8）：`restore_capabilities` 改为解析一次后复用；删除 `let _bundle` 空绑定。异步恢复路径（`history.rs:196` 与 `prepare_composite_for_target`）也存在同一载荷两次解析，可合并。
7. **收敛事实源**（观察 10）：让 `restore_capabilities` 进入生产菜单路径，或修正其注释并把三处实现的关系在 ADR 中写明。

**P3（可读性）**

8. `sync.rs:205-210` 的合成 `Superseded` 回执改为携带实测 revision 或显式“未观测”语义（观察 11）。
9. `reads.rs` 的 `Result<Option<_>, String>` + `mem::replace` 形状与 `apply_prepared` 的静默 fail-closed 补一条诊断（观察 12）。
10. 合并 port.rs 两条取消测试、抽出 runtime 内的双图夹具（第 5 节）。
11. 为 F06/F04 预留但未接线的 API（`CaptureProvenance::from_snapshot`、`standalone_image_is_unbound`、`unresolved_intent_age`）加注释，避免被误读为已生效能力。

## 9. 验收结论

- **本轮无代码改动**：源码指纹 `303fc44c…`（770 文件）与 O06 封存一致，O01/O02/O06 三份证据封存重算全部通过；本文件与 `docs/README.md` 属文档，不参与源码指纹。
- **实现质量**：架构分层、错误语义、版本克制、证据纪律均达到可继续推进的水准；O02 的策略生成与 O06 的不可变计划是本批最值得保留的两个设计。
- **主要问题**：不是正确性缺陷，而是 **O06 之后遗留的失效抽象层**、**写入却从不读取的计划元数据**、**同一语义的多处并行实现**，以及**列表路径新增的逐行解密与能力查询的重复解析**。这些问题不改变现有 PASS 结论，但会持续增加维护成本。
- **效果**：S0 五类复现缺陷已覆盖四类，F04/F06 未实施；最终矩阵 25/26 PASS，唯一 FAIL 是既有的生产 feature 门禁，未修复亦未豁免。
- **发布资格**：仍为 **NO-GO**；真实应用、Windows 原生、双设备等边界保持 UNKNOWN。

本复核仅提出建议，不构成对任何阶段“已完成”结论的推翻；O06 之后的下一项仍为 O10，本轮不启动。
